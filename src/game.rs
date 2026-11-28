//! Game state, tick loop, chunk scheduling and per-frame environment
//! (`Minecraft.runTick`, `EntityRenderer`).

use crate::block::{self, Material};
use crate::entity::player::{Input, Player};
use crate::entity::survival::{GameMode, Stats};
use crate::item::Stack;
use crate::java::JavaRandom;
use crate::play::{InvView, ItemView, Open};
use crate::render::atlas::Atlas;
use crate::render::entities;
use crate::render::mesher::{self, ChunkMesh, MeshChunk};
use crate::render::vk::{Frame, Globals, Renderer};
use crate::render::{self, Mat4};
use crate::screens::{self, MenuAction};
use crate::sim::{self, Command, Event, Sim, SimConfig, SimHandle};
use crate::util::math;
use crate::world::biome::{self, Biome, BiomeNoise};
use crate::world::BlockView;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

type MovingKey = ([i32; 3], u8, u8, crate::render::mesher::MovingDraw);

struct ClientChunk {
    mc: MeshChunk,
    /// End portal blocks (their `TileEntityEndPortal`s).
    portals: Vec<[i32; 3]>,
    /// Chest blocks (their `TileEntityChest`s).
    chests: Vec<[i32; 3]>,
    /// Sign blocks (their `TileEntitySign`s).
    signs: Vec<[i32; 3]>,
    /// Enchanting tables (their `TileEntityEnchantmentTable`s).
    tables: Vec<[i32; 3]>,
    /// Everything that can write into it has been populated.
    ready: bool,
}

#[path = "game_vehicles.rs"]
mod vehicles;

impl ClientChunk {
    /// Lists the blocks drawn by tile entity renderers again for the
    /// sections named by `mask`.
    fn rescan(&mut self, key: (i32, i32), mask: u16) {
        if mask == 0 {
            return;
        }
        let keep = |p: &[i32; 3]| mask & (1 << (p[1] >> 4)) == 0;
        self.portals.retain(keep);
        self.chests.retain(keep);
        self.signs.retain(keep);
        self.tables.retain(keep);
        let blocks = &self.mc.data.blocks;
        for sec in 0..16usize {
            if mask & self.mc.sections & (1 << sec) == 0 {
                continue;
            }
            for xz in 0..256usize {
                // index ((x << 4 | z) << 8) | y
                let base = (xz << 8) | (sec << 4);
                for (dy, &b) in blocks[base..base + 16].iter().enumerate() {
                    let list = match b {
                        block::ENCHANTING_TABLE => &mut self.tables,
                        block::STANDING_SIGN | block::WALL_SIGN => &mut self.signs,
                        block::CHEST | block::TRAPPED_CHEST | block::ENDER_CHEST => {
                            &mut self.chests
                        }
                        block::END_PORTAL => &mut self.portals,
                        _ => continue,
                    };
                    let (x, z) = ((xz >> 4) as i32, (xz & 15) as i32);
                    list.push([key.0 * 16 + x, (sec << 4 | dy) as i32, key.1 * 16 + z]);
                }
            }
        }
    }
}

/// The client's copy of the world: the chunk snapshots the server last sent.
#[derive(Default)]
pub struct ClientWorld {
    chunks: HashMap<(i32, i32), ClientChunk>,
    /// `TileEntityPiston`s the server last listed.
    pub moving: Vec<([i32; 3], crate::world::redstone::MovingView)>,
    /// Dimension of the chunks held (no rain outside the overworld).
    pub dim: i32,
    /// The client's boats' boxes, solid to every moving entity.
    pub solid_entities: Vec<(u32, crate::entity::player::Aabb)>,
    /// `playAuxSFX(2001)` from blocks the client broke itself.
    pub destroyed: Vec<([i32; 3], u8, i32)>,
}

impl ClientWorld {
    #[inline]
    fn index(x: i32, y: i32, z: i32) -> usize {
        ((((x & 15) as usize) << 4 | (z & 15) as usize) << 8) | y as usize
    }

    /// `World.getSavedLightValue`
    pub fn light(&self, sky: bool, x: i32, y: i32, z: i32) -> i32 {
        let y = y.clamp(0, 255);
        let Some(c) = self.chunks.get(&(x >> 4, z >> 4)) else {
            return if sky { 15 } else { 0 };
        };
        if c.mc.sections & (1 << (y >> 4)) == 0 {
            let h = c.mc.height[(((z & 15) << 4) | (x & 15)) as usize];
            return if sky && y >= h { 15 } else { 0 };
        }
        let i = Self::index(x, y, z);
        if sky {
            c.mc.data.sky[i] as i32
        } else {
            c.mc.data.blk[i] as i32
        }
    }

    /// `Chunk.getPrecipitationHeight`
    /// `ChunkProviderClient`: chunks held
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    pub fn has_chunk(&self, cx: i32, cz: i32) -> bool {
        self.chunks.contains_key(&(cx, cz))
    }

    /// `Chunk.getTopFilledSegment` of the chunk holding (x, z), if loaded.
    pub fn top_filled_segment(&self, x: i32, z: i32) -> Option<i32> {
        let c = self.chunks.get(&(x >> 4, z >> 4))?;
        Some(
            (0..16)
                .rev()
                .find(|&i| c.mc.sections & (1 << i) != 0)
                .map_or(0, |i| i * 16),
        )
    }

    /// `World.getHeightValue`: the height map, 0 for a chunk not loaded.
    pub fn height_value(&self, x: i32, z: i32) -> i32 {
        self.chunks
            .get(&(x >> 4, z >> 4))
            .map_or(0, |c| c.mc.height[(((z & 15) << 4) | (x & 15)) as usize])
    }

    pub fn precipitation_height(&self, x: i32, z: i32) -> i32 {
        let Some(c) = self.chunks.get(&(x >> 4, z >> 4)) else {
            return 0;
        };
        let (lx, lz) = (x & 15, z & 15);
        let mut y = 255;
        while y > 0 {
            if c.mc.sections & (1 << (y >> 4)) == 0 {
                y = (y & !15) - 1;
                continue;
            }
            let id = c.mc.data.blocks[Self::index(lx, y, lz)];
            let m = block::material(id);
            if m.blocks_movement() || m.is_liquid() {
                return y + 1;
            }
            y -= 1;
        }
        0
    }

    pub fn biome(&self, x: i32, z: i32) -> &'static Biome {
        match self.chunks.get(&(x >> 4, z >> 4)) {
            Some(c) => {
                biome::get_or_ocean(c.mc.data.biomes[(((z & 15) << 4) | (x & 15)) as usize] as i32)
            }
            None => biome::get_or_ocean(1),
        }
    }
}

impl BlockView for ClientWorld {
    fn peek_block(&self, x: i32, y: i32, z: i32) -> u8 {
        if !(0..256).contains(&y) {
            return block::AIR;
        }
        self.chunks
            .get(&(x >> 4, z >> 4))
            .map_or(block::AIR, |c| c.mc.data.blocks[Self::index(x, y, z)])
    }
    fn peek_meta(&self, x: i32, y: i32, z: i32) -> u8 {
        if !(0..256).contains(&y) {
            return 0;
        }
        self.chunks
            .get(&(x >> 4, z >> 4))
            .map_or(0, |c| c.mc.data.meta[Self::index(x, y, z)])
    }
    fn loaded(&self, x: i32, z: i32) -> bool {
        self.chunks.contains_key(&(x >> 4, z >> 4))
    }
    fn moving(&self, x: i32, y: i32, z: i32) -> Option<crate::world::redstone::MovingView> {
        self.moving.iter().find(|m| m.0 == [x, y, z]).map(|m| m.1)
    }
    fn solid_entities(&self) -> &[(u32, crate::entity::player::Aabb)] {
        &self.solid_entities
    }
}

/// A block changed on the client before the server's answer.
struct Prediction {
    /// The dig or use it belongs to (`Game.sent_actions` when sent).
    seq: u32,
    pos: [i32; 3],
    /// What the server last said is there: id, metadata, sky and block light.
    old: [u8; 4],
    /// What the client shows meanwhile.
    new: [u8; 4],
}

/// A 3x3 neighbourhood and the sections of its middle chunk to build.
struct Job(Box<[MeshChunk; 9]>, u16);

#[derive(Clone)]
pub struct Config {
    pub save_dir: Option<std::path::PathBuf>,
    pub seed: i64,
    pub render_distance: i32,
    pub start_time: i64,
    pub pos: Option<[f64; 3]>,
    pub look: Option<[f32; 2]>,
    pub freeze_time: bool,
    pub mode: GameMode,
}

/// What has keyboard and mouse focus.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Playing,
    /// A container window is open (state comes from the server).
    Window,
    Paused,
    Dead,
    /// `GuiChat`
    Chat,
    /// `GuiEditSign`
    Sign,
    /// `GuiScreenBook`
    Book,
}

/// What the window wants after an input event.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Request {
    None,
    /// Release the mouse (a screen opened).
    Release,
    /// Capture the mouse again.
    Grab,
    Quit,
    /// `menu.returnToMenu`: save, leave the world, the title screen.
    QuitToTitle,
}

/// `PlayerControllerMP` digging state.
#[derive(Clone, Copy, Debug)]
struct Digging {
    pos: [i32; 3],
    damage: f32,
}

pub struct Game {
    pub renderer: Renderer,
    pub world: ClientWorld,
    sim: SimHandle,
    last_center: (i32, i32),
    pub seed: i64,
    pub resumed: bool,
    pub player: Player,
    pub input: Input,
    atlas: Arc<Atlas>,
    jobs: Sender<Job>,
    done: Receiver<ChunkMesh>,
    in_flight: usize,
    max_in_flight: usize,
    /// Seconds spent in: chunk pump, ticks, building the frame, the renderer.
    pub prof: [f64; 4],
    /// Chunk snapshots received, meshes uploaded, events received.
    pub counts: [u64; 3],
    /// Events of the server tick still arriving, and of the ticks complete.
    inbox: Vec<Event>,
    arrived: std::collections::VecDeque<Event>,
    /// When the newest complete server tick finished.
    server_tick_at: Instant,
    /// `MINERUST_DEBUG_LATENCY`: when a block was last used, and its chunk.
    latency: Option<(Instant, (i32, i32))>,
    /// Block digs and uses sent to the server so far (see `ACTIONS_DONE`).
    sent_actions: u32,
    /// How many of them the server had handled when the server tick being
    /// applied ended.
    batch_done: u32,
    /// A server tick's events are being applied.
    batch_open: bool,
    /// Blocks the client changed ahead of the server, as the original's
    /// client does in `onPlayerDestroyBlock` and `ItemBlock.onItemUse`.
    predictions: Vec<Prediction>,
    /// `objectMouseOver.hitVec`: where the crosshair's ray met the block.
    hit: [f64; 3],
    pending_mesh: HashSet<(i32, i32)>,
    /// Chunks with render chunks to build again (bit per 16-high section).
    dirty: HashMap<(i32, i32), u16>,
    offsets: Vec<(i32, i32)>,
    render_distance: i32,
    pub world_time: i64,
    freeze_time: bool,
    last: Instant,
    tick_acc: f64,
    partial: f32,
    fog_color1: f32,
    fog_color2: f32,
    fov_hand: f32,
    fov_hand_prev: f32,
    torch_dx: f32,
    torch_x: f32,
    rng: JavaRandom,
    pub selected: Option<([i32; 3], [i32; 3])>,
    pub screenshot: Option<std::path::PathBuf>,
    light_table: [f32; 16],
    /// `WorldProvider.dimensionId` of the world on screen.
    dim: i32,
    /// `EntityPlayerSP.timeInPortal` (and last tick's value).
    portal_time: f32,
    prev_portal_time: f32,
    /// `GuiIngame.prevVignetteBrightness`
    vignette: f32,
    in_portal: bool,
    ticks: u64,
    noise: BiomeNoise,
    /// `(ticks shown, position in the frame order)` per animated tile.
    anim_state: Vec<(u32, usize)>,
    /// `TextureCompass` / `TextureClock`: `currentAngle`, `angleDelta`,
    /// `frameCounter`.
    compass_state: (f64, f64, Option<usize>),
    clock_state: (f64, f64, Option<usize>),
    /// `Math.random` for the dials outside the Overworld.
    dial_rand: JavaRandom,
    /// The world spawn from `S05PacketSpawnPosition`.
    spawn_point: [i32; 3],
    /// `WorldClient`'s `doDaylightCycle`, from `S03PacketTimeUpdate`.
    day_cycle: bool,
    /// `RenderGlobal.damagedBlocks` of other breakers: block, damage and
    /// the `cloudTickCounter` it was last set at.
    damaged_blocks: std::collections::HashMap<u32, ([i32; 3], i32, u64)>,
    /// Cloud texel the current cloud mesh was built for.
    cloud_cell: Option<(i32, i32, bool)>,
    /// Server view of the inventory and open window.
    pub view: InvView,
    /// `TileEntityEnchantmentTable` state of the tables in the world:
    /// tick count, page flip (prev, now), `flipT`, `flipA`, book spread
    /// (prev, now), book rotation (prev, now), `tRot`.
    tables: HashMap<[i32; 3], [f32; 10]>,
    /// `TileEntityEnchantmentTable.field_145923_r` (static `new Random()`).
    table_rand: JavaRandom,
    /// `TileEntitySign.signText` as the client knows it.
    sign_texts: HashMap<[i32; 3], [String; 4]>,
    /// `GuiScreenBook` while it is open.
    book_gui: Option<crate::book::BookScreen>,
    /// The heads' kinds and rotations.
    skulls: HashMap<[i32; 3], (i32, i32)>,
    /// `GuiEditSign`: the sign, its lines, the line edited, `updateCounter`.
    sign_edit: Option<([i32; 3], [String; 4], usize, i32)>,
    /// `TileEntityChest.prevLidAngle` / `lidAngle` of the chests that move.
    chest_lids: HashMap<[i32; 3], [f32; 2]>,
    /// `GuiNewChat` and the open `GuiChat`.
    pub chat: crate::chat::ChatGui,
    chat_screen: Option<crate::chat::ChatScreen>,
    /// The local player as `RenderPlayer` sees it: `renderYawOffset`,
    /// `rotationYaw`, `rotationPitch` (previous, current) and the limb swing
    /// (`limbSwing`, `limbSwingAmount`, `prevLimbSwingAmount`).
    p_body: [f32; 2],
    p_yaw: [f32; 2],
    p_pitch: [f32; 2],
    p_limb: [f32; 3],
    /// 0 while the world is generated (`LoadingScreenRenderer`), 1 while
    /// `GuiDownloadTerrain` waits for the player's chunk, 2 in the game.
    pub loading: u8,
    /// `gameSettings.thirdPersonView` and `hideGUI`.
    pub third_person: i32,
    pub hide_gui: bool,
    /// `Minecraft.guiAchievement`
    gui_achievement: crate::gui_achievement::GuiAchievement,
    /// `EntityClientPlayerMP.getStatFileWriter`: the client's statistics
    client_stats: std::collections::HashMap<String, i32>,
    /// `NetHandlerPlayClient.hasStatistics`
    has_statistics: bool,
    /// `GuiEnchantment`'s book animation.
    book: crate::gui_container::BookAnim,
    /// `GuiContainer` of the open window.
    gui: crate::gui_container::GuiContainer,
    /// `GuiRepair`'s own state while an anvil is open.
    anvil: Option<crate::gui_repair::GuiRepair>,
    /// `GuiContainerCreative` while it is open, and its last tab.
    creative: Option<crate::creative::Creative>,
    creative_tab: usize,
    /// `WindowClick`s sent (and predicted) so far.
    sent_clicks: u32,
    /// `--open use:x,y,z`: the block to right-click once the world is up,
    /// then the `MINERUST_CLICKS` to send.
    start_use: Option<[i32; 3]>,
    /// The client container's drag state (`PlayerControllerMP.windowClick`
    /// runs `slotClick` locally too).
    client_drag: crate::container::Drag,
    items: Vec<ItemView>,
    /// `en_US.lang`
    pub lang: crate::lang::Lang,
    /// `itemInUse` / `itemInUseCount` as the client sees them.
    using: Option<(crate::item::Stack, i32)>,
    /// `EntityPickupFX`: collected items flying to the player, with age.
    /// `EntityPickupFX`: the item, its age, and the mob collecting it
    /// (`None`: the player).
    pickups: Vec<(ItemView, i32, Option<u32>)>,
    /// `EntityPickupFX` of experience orbs.
    orb_pickups: Vec<(crate::entity::xp_orb::XpOrb, i32)>,
    /// `EntityFireworkStarterFX`s going off
    fireworks: Vec<crate::particles::FireworkStarter>,
    /// Throwables: position, previous position, item, damage.
    throwables: Vec<([f64; 3], [f64; 3], u16, i32)>,
    /// The player's `EntityFishHook`: position and previous position.
    fish_hook: Option<([f64; 3], [f64; 3])>,
    /// `EntityBoat`s in the client's world.
    boats: Vec<vehicles::ClientVehicle>,
    /// The player's `ridingEntity`.
    riding: Option<u32>,
    /// `GuiIngame.recordPlaying`, `recordPlayingUpFor`, `recordIsPlaying`.
    record: Option<(String, i32, bool)>,
    /// `EntityFallingBlock`s from the server.
    falling: Vec<crate::entity::falling::FallingView>,
    /// Origin of `Minecraft.getSystemTime`.
    started: Instant,
    /// `TileEntityRendererPiston` geometry by position, block, metadata and draw mode.
    moving_mesh: HashMap<MovingKey, Vec<crate::render::mesher::Vertex>>,
    /// Sound output (absent when no asset directory or device was found).
    pub audio: Option<crate::audio::Audio>,
    /// The settings, languages and saves (`Minecraft` outside the world).
    pub shell: Option<crate::front::Shell>,
    /// `currentScreen` when it is one of the menus (`GuiIngameMenu` ...).
    menu: Option<crate::menu::Menu>,
    menu_size: (f32, f32),
    weather: crate::sim::Weather,
    rain_sound: i32,
    particle_rand: crate::java::JavaRandom,
    /// `EntityCrit2FX` emitters: mob, "magicCrit", ticks lived.
    crit_emitters: Vec<(u32, bool, i32)>,
    /// `EntityPlayer.playerLocation` and the bed's direction while
    /// sleeping, `sleepTimer`, and whether the chat screen is `GuiSleepMP`.
    sleeping: Option<([i32; 3], i32)>,
    sleep_timer: i32,
    sleep_gui: bool,
    particles: crate::particles::Particles,
    dying: std::collections::HashSet<u32>,
    next_step: f32,
    mobs: Vec<crate::sim::MobView>,
    /// The latest server values of the mobs' per-tick animation counters,
    /// taken over one client tick at a time (see `merge_mobs`).
    mob_net: std::collections::HashMap<u32, MobAnim>,
    /// `BossStatus`: name, health scale and `statusBarTime`.
    boss: Option<(String, f32, i32)>,
    /// The client's `EntityLightningBolt`s (position, `lightningState`,
    /// `boltLivingTime`, `boltVertex`, random) and
    /// `WorldClient.lastLightningBolt`.
    bolts: Vec<([f64; 3], i32, i32, i64, JavaRandom)>,
    last_lightning: i32,
    /// Beacons with a beam: place and `beamRenderScale`.
    beacons: Vec<([i32; 3], f32)>,
    /// `S34PacketMaps` as received, and which map each atlas slot shows
    /// (with the tick it was last wanted).
    maps: std::collections::HashMap<i32, crate::map::MapView>,
    map_slots: Vec<Option<(i32, u64)>>,
    /// `EntityPlayerSP.horseJumpPowerCounter`, `horseJumpPower` and the
    /// jump key as it was last tick.
    horse_jump_counter: i32,
    horse_jump_power: f32,
    was_jumping: bool,
    arrows: Vec<crate::sim::ArrowView>,
    /// Primed TNT: position, previous position, fuse.
    tnt: Vec<([f64; 3], [f64; 3], i32)>,
    /// Fireballs and thrown eyes: position, previous position, kind.
    fireballs: Vec<([f64; 3], [f64; 3], u8)>,
    /// Mob under the crosshair and its distance.
    target_mob: Option<(u32, f64)>,
    pub stats: Stats,
    pub mode: GameMode,
    pub screen: Screen,
    /// Mouse position in window pixels.
    pub mouse_px: (f32, f32),
    gui_viewport: crate::gui::Viewport,
    pub left_down: bool,
    pub right_down: bool,
    dig: Option<Digging>,
    hit_delay: i32,
    right_delay: i32,
    /// `swingProgressInt` and `isSwingInProgress`
    swing: i32,
    swinging: bool,
    prev_swing_f: f32,
    swing_f: f32,
    equip: f32,
    prev_equip: f32,
    /// `EntityPlayerSP.renderArmPitch`, `renderArmYaw` and the previous tick's
    render_arm: [f32; 2],
    prev_render_arm: [f32; 2],
    equipped: Option<Stack>,
    /// `ItemRenderer.equippedItemSlot`
    equipped_slot: usize,
    auto_ticks: i32,
    death_sent: bool,
    jitter: JavaRandom,
    prev_feet: [f64; 3],
    /// F3 overlay.
    pub show_debug: bool,
    fps: u32,
    fps_frames: u32,
    /// `WorldRenderer.chunksUpdated` of the last second, and the renderer's
    /// running count when it was taken
    chunk_updates: u64,
    sections_built_mark: u64,
    fps_since: Instant,
}

fn encode_stats(s: &Stats) -> String {
    format!(
        "{},{},{},{},{},{},{},{},{}",
        s.health, s.food, s.saturation, s.exhaustion, s.air, s.fire, s.xp_level, s.xp_total, s.xp
    )
}

fn decode_stats(t: &str) -> Option<Stats> {
    let f: Vec<&str> = t.split(',').collect();
    if f.len() != 6 && f.len() != 9 {
        return None;
    }
    let mut s = Stats::default();
    s.health = f[0].parse::<f32>().ok()?.clamp(0.0, 20.0);
    s.food = f[1].parse::<i32>().ok()?.clamp(0, 20);
    s.saturation = f[2].parse::<f32>().ok()?.clamp(0.0, 20.0);
    s.exhaustion = f[3].parse::<f32>().ok()?.clamp(0.0, 40.0);
    s.air = f[4].parse::<i32>().ok()?.clamp(-20, 300);
    s.fire = f[5].parse::<i32>().ok()?.max(0);
    if f.len() == 9 {
        s.xp_level = f[6].parse::<i32>().ok()?.max(0);
        s.xp_total = f[7].parse::<i32>().ok()?.max(0);
        s.xp = f[8].parse::<f32>().ok()?.clamp(0.0, 1.0);
    }
    if s.health <= 0.0 {
        // Died and quit: come back alive.
        s = Stats::default();
    }
    Some(s)
}

/// `WorldProvider.generateLightBrightnessTable` (the Nether never goes
/// below 0.1).
fn light_table(dim: i32) -> [f32; 16] {
    let f = if dim == -1 { 0.1f32 } else { 0.0 };
    let mut t = [0f32; 16];
    for (i, v) in t.iter_mut().enumerate() {
        let f1 = 1.0f32 - i as f32 / 15.0f32;
        *v = (1.0f32 - f1) / (f1 * 3.0f32 + 1.0f32) * (1.0 - f) + f;
    }
    t
}

fn hsb_to_rgb(hue: f32, s: f32, b: f32) -> [f32; 3] {
    let h = (hue - hue.floor()) * 6.0f32;
    let f = h - h.floor();
    let p = b * (1.0 - s);
    let q = b * (1.0 - s * f);
    let t = b * (1.0 - s * (1.0 - f));
    let (r, g, bl) = match h as i32 {
        0 => (b, t, p),
        1 => (q, b, p),
        2 => (p, b, t),
        3 => (p, q, b),
        4 => (t, p, b),
        _ => (b, p, q),
    };
    let c = |v: f32| ((v * 255.0f32 + 0.5f32) as i32) as f32 / 255.0f32;
    [c(r), c(g), c(bl)]
}

impl Game {
    pub fn new(renderer: Renderer, atlas: Arc<Atlas>, cfg: &Config) -> Game {
        // The integrated server: world generation, population, block updates.
        let (server, start) = Sim::new(&SimConfig {
            seed: cfg.seed,
            view: cfg.render_distance,
            save_dir: cfg.save_dir.clone(),
            start_time: cfg.start_time,
            freeze_time: cfg.freeze_time,
            mode: cfg.mode,
        });
        let mode = if start.resumed { start.mode } else { cfg.mode };
        let stats = start
            .stats
            .as_deref()
            .and_then(decode_stats)
            .unwrap_or_default();
        let sim = sim::spawn(server);

        // Meshing workers.
        let (jobs, job_rx) = channel::<Job>();
        let (done_tx, done) = channel::<ChunkMesh>();
        let job_rx = Arc::new(Mutex::new(job_rx));
        let noise = Arc::new(BiomeNoise::new());
        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .saturating_sub(2)
            .clamp(1, 12);
        for i in 0..threads {
            let rx = job_rx.clone();
            let tx = done_tx.clone();
            let atlas = atlas.clone();
            let noise = noise.clone();
            std::thread::Builder::new()
                .name(format!("mesh-worker-{i}"))
                .spawn(move || loop {
                    let job = match rx.lock().unwrap().recv() {
                        Ok(j) => j,
                        Err(_) => return,
                    };
                    if tx
                        .send(mesher::build(&job.0, &atlas, &noise, job.1))
                        .is_err()
                    {
                        return;
                    }
                })
                .expect("failed to spawn worker");
        }

        let (pos, rot, flying) = match (cfg.pos, start.resumed) {
            (Some(p), _) => (p, cfg.look.unwrap_or([0.0, 0.0]), true),
            (None, _) => (start.pos, cfg.look.unwrap_or(start.rot), start.flying),
        };
        let mut player = Player::new(pos[0], pos[1], pos[2]);
        player.yaw = rot[0];
        player.pitch = rot[1];
        player.flying = flying && mode == GameMode::Creative;
        player.allow_flying = mode == GameMode::Creative;

        let r = cfg.render_distance;
        let mut offsets = Vec::new();
        for dx in -r..=r {
            for dz in -r..=r {
                offsets.push((dx, dz));
            }
        }
        offsets.sort_by_key(|&(x, z)| (x * x + z * z, x, z));

        let light_table = light_table(0);

        let center = (
            math::floor_double(pos[0]) >> 4,
            math::floor_double(pos[2]) >> 4,
        );
        // `--give id:meta:count,...` (the /give command).
        if let Ok(list) = std::env::var("MINERUST_GIVE") {
            for part in list.split(',') {
                let f: Vec<u32> = part.split(':').filter_map(|x| x.parse().ok()).collect();
                if f.len() == 3 {
                    let _ = sim.commands.send(Command::Give(Stack::new(
                        f[0] as u16,
                        f[1] as i32,
                        f[2],
                    )));
                }
            }
        }
        let _ = sim.commands.send(Command::Center(center.0, center.1));
        let gui_viewport =
            crate::gui::Viewport::new((renderer.extent.width, renderer.extent.height), 2);
        Game {
            renderer,
            world: ClientWorld::default(),
            sim,
            last_center: center,
            seed: start.seed,
            resumed: start.resumed,
            player,
            input: Input::default(),
            atlas,
            jobs,
            done,
            in_flight: 0,
            max_in_flight: threads * 3,
            prof: [0.0; 4],
            counts: [0; 3],
            inbox: Vec::new(),
            arrived: std::collections::VecDeque::new(),
            server_tick_at: Instant::now(),
            latency: None,
            sent_actions: crate::sim::ACTIONS_DONE.load(std::sync::atomic::Ordering::Relaxed),
            batch_done: 0,
            batch_open: false,
            predictions: Vec::new(),
            hit: [0.0; 3],
            pending_mesh: HashSet::new(),
            dirty: HashMap::new(),
            offsets,
            render_distance: r,
            world_time: if cfg.start_time != 0 || !start.resumed {
                cfg.start_time.max(start.world_time)
            } else {
                start.world_time
            },
            freeze_time: cfg.freeze_time,
            last: Instant::now(),
            tick_acc: 0.0,
            partial: 0.0,
            fog_color1: 0.0,
            fog_color2: 0.0,
            fov_hand: 1.0,
            fov_hand_prev: 1.0,
            torch_dx: 0.0,
            torch_x: 0.0,
            rng: JavaRandom::new(0x5DEECE66D),
            selected: None,
            screenshot: None,
            light_table,
            dim: 0,
            portal_time: 0.0,
            prev_portal_time: 0.0,
            vignette: 1.0,
            in_portal: false,
            ticks: 0,
            noise: BiomeNoise::new(),
            anim_state: Vec::new(),
            compass_state: (0.0, 0.0, None),
            clock_state: (0.0, 0.0, None),
            dial_rand: JavaRandom::new(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_nanos() as i64),
            ),
            spawn_point: [0; 3],
            day_cycle: true,
            damaged_blocks: std::collections::HashMap::new(),
            cloud_cell: None,
            view: InvView {
                beacon: [0; 3],
                inv: Default::default(),
                open: Open::None,
                result: None,
                tile_slots: Vec::new(),
                furnace: None,
                clicks: 0,
                enchant: [0; 3],
                name_seed: 0,
                repair_cost: 0,
                offers: Vec::new(),
                merchant_name: String::new(),
                merchant_index: 0,
                dropper: false,
            },
            gui: Default::default(),
            anvil: None,
            book: Default::default(),
            creative: None,
            creative_tab: 0,
            chest_lids: HashMap::new(),
            sign_texts: HashMap::new(),
            book_gui: None,
            skulls: HashMap::new(),
            tables: HashMap::new(),
            table_rand: JavaRandom::new(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_nanos() as i64),
            ),
            sign_edit: None,
            third_person: 0,
            loading: 0,
            p_body: [0.0; 2],
            p_yaw: [0.0; 2],
            p_pitch: [0.0; 2],
            p_limb: [0.0; 3],
            chat: Default::default(),
            chat_screen: None,
            // `--hide-gui` (F1 at start) for screenshot comparisons
            hide_gui: std::env::var_os("MINERUST_HIDE_GUI").is_some(),
            gui_achievement: Default::default(),
            client_stats: Default::default(),
            has_statistics: false,
            sent_clicks: 0,
            start_use: None,
            client_drag: Default::default(),
            items: Vec::new(),
            pickups: Vec::new(),
            orb_pickups: Vec::new(),
            fireworks: Vec::new(),
            lang: crate::lang::Lang::default(),
            using: None,
            falling: Vec::new(),
            throwables: Vec::new(),
            fish_hook: None,
            boats: Vec::new(),
            riding: None,
            record: None,
            started: Instant::now(),
            moving_mesh: HashMap::new(),
            audio: None,
            shell: None,
            menu: None,
            menu_size: (0.0, 0.0),
            weather: Default::default(),
            rain_sound: 0,
            particle_rand: crate::java::JavaRandom::new(77),
            crit_emitters: Vec::new(),
            sleeping: None,
            sleep_timer: 0,
            sleep_gui: false,
            particles: Default::default(),
            dying: Default::default(),
            next_step: 1.0,
            mobs: Vec::new(),
            mob_net: Default::default(),
            boss: None,
            bolts: Vec::new(),
            last_lightning: 0,
            beacons: Vec::new(),
            maps: Default::default(),
            map_slots: Vec::new(),
            horse_jump_counter: 0,
            horse_jump_power: 0.0,
            was_jumping: false,
            arrows: Vec::new(),
            tnt: Vec::new(),
            fireballs: Vec::new(),
            target_mob: None,
            stats,
            mode,
            screen: Screen::Playing,
            mouse_px: (0.0, 0.0),
            gui_viewport,
            left_down: false,
            right_down: false,
            dig: None,
            hit_delay: 0,
            right_delay: 0,
            swing: -1,
            prev_swing_f: 0.0,
            swinging: false,
            swing_f: 0.0,
            equip: 0.0,
            prev_equip: 0.0,
            render_arm: [0.0; 2],
            prev_render_arm: [0.0; 2],
            equipped: None,
            equipped_slot: 0,
            auto_ticks: 0,
            death_sent: false,
            jitter: JavaRandom::new(312871),
            prev_feet: [0.0; 3],
            show_debug: std::env::var_os("MINERUST_F3").is_some(),
            fps: 0,
            fps_frames: 0,
            chunk_updates: 0,
            sections_built_mark: 0,
            fps_since: Instant::now(),
        }
    }
    pub fn spawn_info(&self) -> String {
        format!(
            "{:.1} {:.1} {:.1}",
            self.player.pos[0], self.player.bb.min[1], self.player.pos[2]
        )
    }

    pub fn loaded_meshes(&self) -> usize {
        self.renderer.mesh_count()
    }

    /// MINERUST_CLICKS=slot:button:mode,...  window clicks for captures
    fn send_start_clicks(&mut self) {
        if let Ok(list) = std::env::var("MINERUST_CLICKS") {
            for part in list.split(',') {
                let f: Vec<i32> = part.split(':').filter_map(|x| x.parse().ok()).collect();
                if f.len() == 3 {
                    self.sent_clicks = self.sent_clicks.wrapping_add(1);
                    let _ = self.sim.commands.send(Command::WindowClick {
                        slot: f[0],
                        button: f[1],
                        mode: f[2],
                    });
                }
            }
        }
    }

    /// `GuiIngame.renderGameOverlay` with `showDebugInfo` (F3), line by line:
    /// the first five lines with shadow in white at y 2, 12, 22, 32, 42, the
    /// two memory lines on the right and the position lines from y 64 in
    /// 0xE0E0E0.
    fn debug_overlay(
        &self,
        ui: &mut crate::gui::Ui,
        g: &crate::render::vk::Globals,
        eye: [f64; 3],
    ) {
        let p = &self.player;
        let white = 0xFFFFFF;
        let grey = 14737632;
        // Minecraft.debug
        ui.text(
            &format!(
                "Minecraft 1.7.10 ({} fps, {} chunk updates)",
                self.fps, self.chunk_updates
            ),
            2.0,
            2.0,
            white,
            true,
        );
        // RenderGlobal.getDebugInfoRenders: the renderer grid is
        // (2 * renderDistanceChunks + 1)^2 columns of 16; render chunks with
        // nothing for the first pass are skipped (E), occlusion queries are off
        let rd = self.render_distance;
        let loaded = ((2 * rd + 1) * (2 * rd + 1) * 16) as u32;
        let (drawn, clipped) = self.renderer.debug_renders;
        ui.text(
            &format!(
                "C: {}/{}. F: {}, O: 0, E: {}",
                drawn,
                loaded,
                clipped,
                loaded.saturating_sub(drawn + clipped)
            ),
            2.0,
            12.0,
            white,
            true,
        );
        // RenderGlobal.renderEntities: the client's loadedEntityList, and
        // those in their render range (isInRangeToRender3d) and in view
        let (total, rendered) = self.entity_debug(g, eye);
        ui.text(
            &format!("E: {}/{}. B: 0, I: {}", rendered, total, total - rendered),
            2.0,
            22.0,
            white,
            true,
        );
        // EffectRenderer.getStatistics, WorldClient.getDebugLoadedEntities
        ui.text(
            &format!("P: {}. T: All: {}", self.particles.count(), total),
            2.0,
            32.0,
            white,
            true,
        );
        // WorldClient.getProviderName → ChunkProviderClient.makeString
        let n = self.world.chunk_count();
        ui.text(
            &format!("MultiplayerChunkCache: {n}, {n}"),
            2.0,
            42.0,
            white,
            true,
        );
        // Runtime.maxMemory / totalMemory / freeMemory: there is no Java heap,
        // so the process's working set and commit charge out of the
        // computer's physical memory stand in for them
        if let Some((used, allocated, max)) = crate::util::memory_usage() {
            let r = format!(
                "Used memory: {}% ({}MB) of {}MB",
                used * 100 / max.max(1),
                used / 1024 / 1024,
                max / 1024 / 1024
            );
            ui.text(&r, ui.w - ui.text_width(&r) - 2.0, 2.0, grey, true);
            let r = format!(
                "Allocated memory: {}% ({}MB)",
                allocated * 100 / max.max(1),
                allocated / 1024 / 1024
            );
            ui.text(&r, ui.w - ui.text_width(&r) - 2.0, 12.0, grey, true);
        }
        // the client player's posY is the eye height
        let (x, y, z) = (
            math::floor_double(p.pos[0]),
            math::floor_double(p.pos[1]),
            math::floor_double(p.pos[2]),
        );
        let lines = [
            format!("x: {:.5} ({}) // c: {} ({})", p.pos[0], x, x >> 4, x & 15),
            format!("y: {:.3} (feet pos, {:.3} eyes pos)", p.bb.min[1], p.pos[1]),
            format!("z: {:.5} ({}) // c: {} ({})", p.pos[2], z, z >> 4, z & 15),
        ];
        for (i, l) in lines.iter().enumerate() {
            ui.text(l, 2.0, 64.0 + i as f32 * 8.0, grey, true);
        }
        let facing = (math::floor_double((p.yaw * 4.0f32 / 360.0f32) as f64 + 0.5) & 3) as usize;
        ui.text(
            &format!(
                "f: {} ({}) / {}",
                facing,
                ["SOUTH", "WEST", "NORTH", "EAST"][facing],
                crate::util::java_float(math::wrap_angle_180(p.yaw))
            ),
            2.0,
            88.0,
            grey,
            true,
        );
        if (0..256).contains(&y) {
            if let Some(top) = self.world.top_filled_segment(x, z) {
                let (bl, sl) = (
                    self.world.light(false, x, y, z),
                    self.world.light(true, x, y, z),
                );
                ui.text(
                    &format!(
                        "lc: {} b: {} bl: {} sl: {} rl: {}",
                        top + 15,
                        self.world.biome(x, z).name,
                        bl,
                        sl,
                        sl.max(bl)
                    ),
                    2.0,
                    96.0,
                    grey,
                    true,
                );
            }
        }
        // PlayerCapabilities walkSpeed and flySpeed; World.getHeightValue
        ui.text(
            &format!(
                "ws: {:.3}, fs: {:.3}, g: {}, fl: {}",
                0.1f32,
                0.05f32,
                p.on_ground,
                self.world.height_value(x, z)
            ),
            2.0,
            104.0,
            grey,
            true,
        );
    }

    /// `RenderGlobal.renderEntities`: `countEntitiesTotal` (the local player
    /// included) and `countEntitiesRendered`.
    fn entity_debug(&self, g: &crate::render::vk::Globals, eye: [f64; 3]) -> (i32, i32) {
        let planes = crate::render::frustum_planes(&g.view_proj);
        let mut total = 1; // the player itself, not drawn in first person
        let mut rendered = 0;
        // Entity.isInRangeToRender3d with the class's isInRangeToRenderDist,
        // then the frustum (ignoreFrustumCheck for the dragon), and the chunk
        // must exist (blockExists(x, 0, z))
        let mut check = |bb: [f64; 6], weight: f64, ignore_frustum: bool| {
            total += 1;
            let c = [
                (bb[0] + bb[3]) * 0.5 - eye[0],
                (bb[1] + bb[4]) * 0.5 - eye[1],
                (bb[2] + bb[5]) * 0.5 - eye[2],
            ];
            let avg = ((bb[3] - bb[0]) + (bb[4] - bb[1]) + (bb[5] - bb[2])) / 3.0;
            let range = avg * 64.0 * weight;
            if c[0] * c[0] + c[1] * c[1] + c[2] * c[2] >= range * range {
                return;
            }
            let rel = |i: usize| (bb[i] - eye[i % 3]) as f32;
            if !ignore_frustum
                && !crate::render::box_in_frustum(
                    &planes,
                    [rel(0), rel(1), rel(2)],
                    [rel(3), rel(4), rel(5)],
                )
            {
                return;
            }
            let (cx, cz) = (
                math::floor_double((bb[0] + bb[3]) * 0.5) >> 4,
                math::floor_double((bb[2] + bb[5]) * 0.5) >> 4,
            );
            if self.world.has_chunk(cx, cz) {
                rendered += 1;
            }
        };
        // a box of `w` by `h` around `pos` whose bottom is `pos.y - y_offset`
        let sized = |pos: [f64; 3], w: f64, h: f64, y_offset: f64| {
            let y0 = pos[1] - y_offset;
            [
                pos[0] - w / 2.0,
                y0,
                pos[2] - w / 2.0,
                pos[0] + w / 2.0,
                y0 + h,
                pos[2] + w / 2.0,
            ]
        };
        for m in &self.mobs {
            check(m.bb, 1.0, m.kind == crate::entity::mob::Kind::EnderDragon);
        }
        for i in &self.items {
            // EntityItem: setSize(0.25, 0.25), yOffset = height / 2
            check(sized(i.pos, 0.25, 0.25, 0.125), 1.0, false);
        }
        for c in &self.boats {
            let b = &c.v.e().bb;
            check(
                [b.min[0], b.min[1], b.min[2], b.max[0], b.max[1], b.max[2]],
                1.0,
                false,
            );
        }
        for f in &self.falling {
            // EntityFallingBlock: 0.98 x 0.98, yOffset = height / 2
            check(sized(f.pos, 0.98, 0.98, 0.49), 1.0, false);
        }
        for a in &self.arrows {
            // EntityArrow: 0.5 x 0.5, renderDistanceWeight 10
            check(sized(a.pos, 0.5, 0.5, 0.0), 10.0, false);
        }
        for t in &self.tnt {
            // EntityTNTPrimed: 0.98 x 0.98, yOffset = height / 2
            check(sized(t.0, 0.98, 0.98, 0.49), 1.0, false);
        }
        (total, rendered)
    }

    /// `NetHandlerPlayClient.handleStatistics`
    fn handle_statistics(&mut self, list: Vec<(String, i32)>) {
        let mut any_achievement = false;
        for (id, value) in list {
            if crate::stats::is_achievement(&id) && value > 0 {
                let known = self.client_stats.get(&id).copied().unwrap_or(0);
                if self.has_statistics && known == 0 {
                    self.gui_achievement.display_achievement(&self.lang, &id);
                    if id == crate::stats::OPEN_INVENTORY {
                        if let Some(sh) = self.shell.as_mut() {
                            sh.settings.show_inventory_achievement_hint = false;
                            sh.settings.save();
                        }
                    }
                }
                any_achievement = true;
            }
            // StatFileWriter.unlockAchievement
            self.client_stats.insert(id, value);
        }
        let hint = self
            .shell
            .as_ref()
            .map_or(true, |sh| sh.settings.show_inventory_achievement_hint);
        if !self.has_statistics && !any_achievement && hint {
            let key = crate::settings::key_display(self.bind("key.inventory"), &self.lang);
            self.gui_achievement.display_unformatted_achievement(
                &self.lang,
                crate::stats::OPEN_INVENTORY,
                &key,
            );
        }
        self.has_statistics = true;
    }

    /// `--open`: start with a screen open (automated captures).
    pub fn apply_start_screen(&mut self) {
        if let Ok(s) = std::env::var("MINERUST_OPEN") {
            match s.as_str() {
                "inventory" => {
                    let _ = self.sim.commands.send(Command::OpenInventory);
                    self.screen = Screen::Window;
                }
                // chat  GuiChat open
                "chat" => {
                    let _ = self.open_chat("/give ");
                }
                // view:N  thirdPersonView
                o if o.starts_with("view:") => {
                    self.third_person = o[5..].parse().unwrap_or(0);
                }
                // creative:N  GuiContainerCreative on tab N
                o if o.starts_with("creative") => {
                    self.creative_tab = o[8..].trim_start_matches(':').parse().unwrap_or(0);
                    self.screen = Screen::Window;
                    self.open_creative();
                }
                // use:x,y,z  right-click a block (opens its window)
                o if o.starts_with("use:") => {
                    let v: Vec<i32> = o[4..].split(',').filter_map(|t| t.parse().ok()).collect();
                    if v.len() == 3 {
                        self.start_use = Some([v[0], v[1], v[2]]);
                    }
                }
                // stats  GuiStats over the pause menu
                "stats" => {
                    self.screen = Screen::Paused;
                    let mut m = crate::menu::Menu::ingame();
                    m.stack.push(crate::menu::Screen::Stats(
                        crate::menu::Basic { buttons: Vec::new() },
                        Box::new(crate::gui_stats::StatsScreen::new()),
                    ));
                    self.menu = Some(m);
                    self.menu_size = (0.0, 0.0);
                    let _ = self.sim.commands.send(Command::RequestStats);
                }
                // achievements  GuiAchievements over the pause menu
                "achievements" => {
                    self.screen = Screen::Paused;
                    let mut m = crate::menu::Menu::ingame();
                    m.stack.push(crate::menu::Screen::Achievements(
                        crate::menu::Basic { buttons: Vec::new() },
                        Box::new(crate::gui_achievements::State::new()),
                    ));
                    self.menu = Some(m);
                    self.menu_size = (0.0, 0.0);
                }
                "pause" => {
                    self.screen = Screen::Paused;
                    self.menu = Some(crate::menu::Menu::ingame());
                }
                "dead" => {
                    self.stats.health = 0.0;
                    self.stats.dead = true;
                    self.stats.death_time = 40;
                    self.stats.last_cause = Some(crate::entity::survival::Damage::Fall);
                    self.death_sent = true;
                    self.screen = Screen::Dead;
                }
                _ => {}
            }
            self.mouse_px = (
                self.renderer.extent.width as f32 / 2.0 + 40.0,
                self.renderer.extent.height as f32 / 2.0 - 10.0,
            );
        }
        if self.start_use.is_none() {
            self.send_start_clicks();
        }
    }

    /// True once every chunk in view has an up-to-date mesh.
    pub fn idle(&self) -> bool {
        let side = (self.render_distance * 2 + 1) as usize;
        self.in_flight == 0
            && self.dirty.is_empty()
            && self.renderer.pending_uploads() == 0
            && self.renderer.mesh_count() >= side * side
    }

    /// A command from the benchmark harness.
    pub fn bench_command(&mut self, c: Command) {
        let _ = self.sim.commands.send(c);
    }

    /// The partial tick of what the server simulates and the client only
    /// draws (dropped items, arrows, primed TNT, falling blocks, pistons):
    /// how far the newest server tick's `prev` to `pos` step has played.
    /// The server's ticks are not in phase with the client's, so the
    /// client's own partial tick would make these jump back and forth.
    /// `Entity.getBrightness` for the player: `World.getLightBrightness`
    /// two thirds of the way up its box.
    fn player_brightness(&self) -> f32 {
        let p = self.player.pos;
        let x = math::floor_double(p[0]);
        let z = math::floor_double(p[2]);
        let y = math::floor_double(
            self.player.bb.min[1] + (self.player.bb.max[1] - self.player.bb.min[1]) * 0.66,
        );
        if !self.world.chunks.contains_key(&(x >> 4, z >> 4)) {
            return 0.0;
        }
        let sub = client_skylight_subtracted();
        // getBlockLightValue_do(x, y, z, true)
        let at = |x: i32, y: i32, z: i32| -> i32 {
            if y < 0 {
                return 0;
            }
            let y = y.min(255);
            (self.world.light(true, x, y, z) - sub).max(self.world.light(false, x, y, z))
        };
        let id = self.world.peek_block(x, y, z);
        let l = if (0..256).contains(&y) && block::info(id).use_neighbor_brightness {
            at(x, y + 1, z)
                .max(at(x + 1, y, z))
                .max(at(x - 1, y, z))
                .max(at(x, y, z + 1))
                .max(at(x, y, z - 1))
        } else {
            at(x, y, z)
        };
        self.light_table[l.clamp(0, 15) as usize]
    }

    fn server_partial(&self) -> f32 {
        (self.server_tick_at.elapsed().as_secs_f32() / 0.05).clamp(0.0, 1.0)
    }

    fn player_chunk(&self) -> (i32, i32) {
        (
            math::floor_double(self.player.pos[0]) >> 4,
            math::floor_double(self.player.pos[2]) >> 4,
        )
    }

    /// The lines of `InventoryEffectRenderer`: name with its level and
    /// `Potion.getDurationString`.
    fn effect_lines(&self) -> Vec<screens::EffectLine> {
        self.stats
            .effects
            .iter()
            .map(|e| {
                let p = e.potion();
                let mut name = self.lang.get(p.name).to_string();
                match e.amplifier {
                    1 => name = format!("{} {}", name, self.lang.get("enchantment.level.2")),
                    2 => name = format!("{} {}", name, self.lang.get("enchantment.level.3")),
                    3 => name = format!("{} {}", name, self.lang.get("enchantment.level.4")),
                    _ => {}
                }
                let dur = if e.duration_max {
                    "**:**".to_string()
                } else {
                    // StringUtils.ticksToElapsedTime
                    let i = e.duration / 20;
                    let (j, i) = (i / 60, i % 60);
                    if i < 10 {
                        format!("{j}:0{i}")
                    } else {
                        format!("{j}:{i}")
                    }
                };
                (p.icon, name, dur)
            })
            .collect()
    }

    /// `EntityPlayer.updateItemUse`: drinking sounds, eating crumbs.
    fn update_item_use(&mut self, st: crate::item::Stack, n: i32) {
        use crate::item_use::{action, Action};
        let act = action(st.id, st.meta);
        if act == Action::Drink {
            let p = self.particle_rand.next_float() * 0.1 + 0.9;
            self.sound("random.drink", None, 0.5, p);
        }
        if act == Action::Eat {
            let rect = self
                .atlas
                .items
                .get(&(st.id, st.meta))
                .or_else(|| self.atlas.items.get(&(st.id, 0)))
                .map(|&t| self.atlas.rects[t as usize]);
            let (yaw, pitch) = (self.player.yaw, self.player.pitch);
            let rot = |v: [f64; 3]| {
                // Vec3.rotateAroundX(-pitch), rotateAroundY(-yaw)
                let (ax, ay) = ((-pitch).to_radians(), (-yaw).to_radians());
                let (cx, sx) = (ax.cos() as f64, ax.sin() as f64);
                let y = v[1] * cx + v[2] * sx;
                let z = v[2] * cx - v[1] * sx;
                let (cy, sy) = (ay.cos() as f64, ay.sin() as f64);
                let x = v[0] * cy + z * sy;
                let z = z * cy - v[0] * sy;
                [x, y, z]
            };
            for _ in 0..n {
                let r = &mut self.particle_rand;
                let v = rot([
                    (r.next_float() as f64 - 0.5) * 0.1,
                    r.next_double() * 0.1 + 0.1,
                    0.0,
                ]);
                let p = rot([
                    (r.next_float() as f64 - 0.5) * 0.3,
                    -(r.next_float() as f64) * 0.6 - 0.3,
                    0.6,
                ]);
                let e = self.player.pos;
                let at = [p[0] + e[0], p[1] + e[1], p[2] + e[2]];
                if let Some(rect) = rect {
                    self.particles
                        .item_crack(at, [v[0], v[1] + 0.05, v[2]], rect);
                }
            }
            let r = &mut self.particle_rand;
            let vol = 0.5 + 0.5 * r.next_int_n(2) as f32;
            let pi = (r.next_float() - r.next_float()) * 0.2 + 1.0;
            self.sound("random.eat", None, vol, pi);
        }
    }

    /// `TileEntityRendererPiston.renderTileEntityAt` for every moving block.
    fn render_moving(
        &mut self,
        out: &mut Vec<crate::render::mesher::Vertex>,
        eye: [f64; 3],
        partial: f32,
    ) {
        use crate::render::mesher::{build_single, MovingDraw};
        let list = self.world.moving.clone();
        for (pos, m) in list {
            if block::material(m.block) == block::Material::Air || m.progress(partial) >= 1.0 {
                continue;
            }
            let prog = m.progress(partial);
            let off = m.offset(partial);
            // (what to draw, whether the render offset applies)
            let mut parts = Vec::new();
            if m.block == block::PISTON_HEAD && prog < 0.5 {
                parts.push((
                    MovingDraw::Head {
                        long: false,
                        sticky_sprite: None,
                    },
                    true,
                ));
            } else if m.head && !m.extending {
                parts.push((
                    MovingDraw::Head {
                        long: prog < 0.5,
                        sticky_sprite: Some(m.block == block::STICKY_PISTON),
                    },
                    true,
                ));
                parts.push((MovingDraw::Base, false));
            } else {
                parts.push((MovingDraw::Block, true));
            }
            let (cx, cz) = (pos[0] >> 4, pos[2] >> 4);
            for (draw, moves) in parts {
                let key = (pos, m.block, m.meta, draw);
                if !self.moving_mesh.contains_key(&key) {
                    let mut v = Vec::with_capacity(9);
                    for dz in -1..=1 {
                        for dx in -1..=1 {
                            if let Some(c) = self.world.chunks.get(&(cx + dx, cz + dz)) {
                                v.push(c.mc.clone());
                            }
                        }
                    }
                    let Ok(nine) = <[MeshChunk; 9]>::try_from(v) else {
                        continue;
                    };
                    let verts = build_single(
                        &nine,
                        &self.atlas,
                        &self.noise,
                        pos,
                        m.block,
                        m.meta as i32,
                        draw,
                    );
                    self.moving_mesh.insert(key, verts);
                }
                let o = if moves { off } else { [0.0; 3] };
                let base = [
                    (cx * 16) as f64 - eye[0] + o[0] as f64,
                    -eye[1] + o[1] as f64,
                    (cz * 16) as f64 - eye[2] + o[2] as f64,
                ];
                for v in &self.moving_mesh[&key] {
                    let mut w = *v;
                    w.pos = [
                        (v.pos[0] as f64 + base[0]) as f32,
                        (v.pos[1] as f64 + base[1]) as f32,
                        (v.pos[2] as f64 + base[2]) as f32,
                    ];
                    out.push(w);
                }
            }
        }
    }

    /// The chunk and its eight neighbours, once nothing unpopulated can
    /// still write into any of them: a mesh built earlier would only be
    /// built again when the neighbour is decorated.
    fn mesh_input(&self, cx: i32, cz: i32) -> Option<Box<[MeshChunk; 9]>> {
        let mut v = Vec::with_capacity(9);
        for dz in -1..=1 {
            for dx in -1..=1 {
                let c = self.world.chunks.get(&(cx + dx, cz + dz))?;
                if !c.ready {
                    return None;
                }
                v.push(c.mc.clone());
            }
        }
        v.try_into().ok().map(Box::new)
    }

    fn pump_chunks(&mut self) {
        while let Ok(m) = self.done.try_recv() {
            self.in_flight -= 1;
            let key = (m.cx, m.cz);
            self.pending_mesh.remove(&key);
            if self.world.chunks.contains_key(&key) {
                if let Some((t, k)) = self.latency {
                    if k == key {
                        self.latency = None;
                        eprintln!(
                            "latency: block use to rebuilt mesh {:.1} ms",
                            t.elapsed().as_secs_f64() * 1000.0
                        );
                    }
                }
                self.counts[1] += m.sections.len() as u64;
                self.renderer.upload_mesh(m);
            }
        }
        // Snapshots from the server.
        loop {
            // a server tick is applied whole, once its last event is here
            let e = match self.arrived.pop_front() {
                Some(e) => e,
                None if self.batch_open => {
                    // the whole server tick is in: what it left untouched
                    // is as the server has it
                    self.batch_open = false;
                    self.settle_predictions();
                    continue;
                }
                None => match self.sim.events.try_recv() {
                    Ok(Event::Tick(at, done)) => {
                        self.batch_done = done;
                        self.batch_open = true;
                        // Keep the client's ticks half a tick behind the
                        // server's. Both run at 20 Hz off the same clock, so
                        // their phase is arbitrary and constant; when it sits
                        // near zero the two take turns coming first and a
                        // mob's three interpolation steps get two or four
                        // client ticks, which shows as a stutter.
                        let gap = at.saturating_duration_since(self.server_tick_at);
                        if (0.045..=0.055).contains(&gap.as_secs_f64()) {
                            let since = at.elapsed().as_secs_f64() / 0.05;
                            let mut err = (since + 1.0 - self.tick_acc - 0.5).rem_euclid(1.0);
                            if err > 0.5 {
                                err -= 1.0;
                            }
                            self.tick_acc += (err * 0.1).clamp(-0.02, 0.02);
                        }
                        self.server_tick_at = at;
                        self.arrived.extend(self.inbox.drain(..));
                        continue;
                    }
                    Ok(e) => {
                        self.inbox.push(e);
                        continue;
                    }
                    Err(_) => break,
                },
            };
            self.counts[2] += 1;
            match e {
                Event::Lightning(p) => {
                    let mut r = JavaRandom::new(self.particle_rand.next_long());
                    let vertex = r.next_long();
                    let living = r.next_int_n(3) + 1;
                    self.bolts.push((p, 2, living, vertex, r));
                }
                Event::Beacons(list) => {
                    let old = std::mem::take(&mut self.beacons);
                    self.beacons = list
                        .into_iter()
                        .map(|p| (p, old.iter().find(|b| b.0 == p).map_or(0.0, |b| b.1)))
                        .collect();
                }
                Event::Map(v) => {
                    let id = v.id;
                    self.maps.insert(id, v);
                    if let Some(i) = self.map_slots.iter().position(|s| s.is_some_and(|s| s.0 == id)) {
                        self.upload_map(i, id);
                    }
                }
                Event::Tick(..) => {}
                Event::Chunk {
                    key,
                    chunk,
                    ready,
                    changed,
                } => {
                    self.counts[0] += 1;
                    let (was_ready, mut c) = match self.world.chunks.remove(&key) {
                        Some(c) => (c.ready, c),
                        None => (
                            false,
                            ClientChunk {
                                mc: chunk.clone(),
                                ready,
                                portals: Vec::new(),
                                chests: Vec::new(),
                                signs: Vec::new(),
                                tables: Vec::new(),
                            },
                        ),
                    };
                    c.mc = chunk;
                    c.ready = ready;
                    // Predictions here: a snapshot from after the server
                    // handled the action is the answer; an earlier one must
                    // not undo what the player sees.
                    let done = self.batch_done;
                    let mut redo = 0u16;
                    self.predictions.retain_mut(|p| {
                        if (p.pos[0] >> 4, p.pos[2] >> 4) != key {
                            return true;
                        }
                        if p.seq.wrapping_sub(done) as i32 <= 0 {
                            if std::env::var_os("MINERUST_DEBUG_LATENCY").is_some() {
                                let i = ClientWorld::index(p.pos[0], p.pos[1], p.pos[2]);
                                eprintln!(
                                    "predict: {:?} answered, server has {}:{}",
                                    p.pos, c.mc.data.blocks[i], c.mc.data.meta[i]
                                );
                            }
                            return false;
                        }
                        let i = ClientWorld::index(p.pos[0], p.pos[1], p.pos[2]);
                        let d = Arc::make_mut(&mut c.mc.data);
                        p.old = [d.blocks[i], d.meta[i], d.sky[i], d.blk[i]];
                        d.blocks[i] = p.new[0];
                        d.meta[i] = p.new[1];
                        d.sky[i] = p.new[2];
                        d.blk[i] = p.new[3];
                        redo |= 1 << (p.pos[1] >> 4);
                        true
                    });
                    let changed = changed | redo;
                    c.rescan(key, changed);
                    self.world.chunks.insert(key, c);
                    if changed != 0 {
                        *self.dirty.entry(key).or_insert(0) |= changed;
                    }
                    // A chunk that has just become final: the faces and light
                    // of its neighbours along the shared border depend on it.
                    if ready && !was_ready {
                        for dx in -1..=1 {
                            for dz in -1..=1 {
                                *self.dirty.entry((key.0 + dx, key.1 + dz)).or_insert(0) |= 0xFFFF;
                            }
                        }
                    }
                }
                Event::Unload(key) => {
                    self.world.chunks.remove(&key);
                    self.renderer.remove_mesh(key);
                    self.dirty.remove(&key);
                }
                Event::Dimension { dim, pos, yaw } => {
                    // NetHandlerPlayClient.handleRespawn: a new world, and
                    // GuiDownloadTerrain until the player's chunk is there
                    self.dim = dim;
                    self.loading = 1;
                    let keys: Vec<(i32, i32)> = self.world.chunks.keys().copied().collect();
                    for key in keys {
                        self.world.chunks.remove(&key);
                        self.renderer.remove_mesh(key);
                    }
                    self.dirty.clear();
                    self.predictions.clear();
                    self.world.dim = dim;
                    self.mobs.clear();
                    self.items.clear();
                    self.tnt.clear();
                    self.arrows.clear();
                    self.light_table = light_table(dim);
                    self.player.set_position(pos[0], pos[1], pos[2]);
                    self.player.motion = [0.0; 3];
                    self.player.yaw = yaw;
                    self.player.prev_pos = self.player.pos;
                    self.stats.fall_distance = 0.0;
                    self.portal_time = 0.0;
                    self.prev_portal_time = 0.0;
                    let _ = self.sim.commands.send(Command::DimensionAck);
                }
                Event::Portal(t) => {
                    let was = self.in_portal;
                    self.in_portal = t > 0;
                    if self.in_portal && !was && self.portal_time == 0.0 {
                        let r = &mut self.particle_rand;
                        let p = r.next_float() * 0.4 + 0.8;
                        self.sound("portal.trigger", None, 1.0, p);
                    }
                }
                Event::Inventory(v) => {
                    // a view from before the clicks the client predicted
                    let behind = self.sent_clicks.wrapping_sub(v.clicks);
                    if behind > 0 && behind < 1 << 16 {
                        continue;
                    }
                    self.sent_clicks = v.clicks;
                    if self.creative.is_some() {
                        // the creative cursor lives on the client only
                        let cursor = self.view.inv.cursor;
                        self.view = *v;
                        self.view.inv.cursor = cursor;
                        continue;
                    }
                    let was_open = self.view.open != Open::None;
                    if v.open != self.view.open {
                        self.gui = Default::default();
                        self.client_drag = Default::default();
                    }
                    // GuiBeacon keeps the powers picked on the client until
                    // they are confirmed
                    let keep = (v.open == self.view.open
                        && matches!(v.open, Open::Beacon(_))
                        && v.beacon[0] == self.view.beacon[0])
                        .then(|| [self.view.beacon[1], self.view.beacon[2]]);
                    self.view = *v;
                    if let Some(k) = keep {
                        self.view.beacon[1] = k[0];
                        self.view.beacon[2] = k[1];
                    }
                    if self.view.open != Open::None && !was_open && self.screen == Screen::Playing {
                        self.screen = Screen::Window;
                    }
                    if self.view.open == Open::None && self.screen == Screen::Window {
                        self.screen = Screen::Playing;
                    }
                }
                Event::Items(v) => self.items = v,
                Event::Falling(v) => self.falling = v,
                Event::Throwables(v) => self.throwables = v,
                Event::FishHook(v) => self.fish_hook = v,
                // RenderGlobal.playAuxSFX(1005) -> playRecord
                Event::Record(p, id) => {
                    const NAMES: [&str; 12] = [
                        "13", "cat", "blocks", "chirp", "far", "mall", "mellohi", "stal", "strad",
                        "ward", "11", "wait",
                    ];
                    let name = (2256..=2267)
                        .contains(&id)
                        .then(|| NAMES[(id - 2256) as usize]);
                    if let Some(n) = name {
                        // GuiIngame.setRecordPlayingMessage
                        let desc = self.lang.get(&format!("item.record.{n}.desc")).to_string();
                        let text = self.lang.get("record.nowPlaying").replace("%s", &desc);
                        self.record = Some((text, 60, true));
                    }
                    if let Some(a) = self.audio.as_mut() {
                        a.play_record(name.map(|n| format!("records.{n}")).as_deref(), p);
                    }
                }
                Event::Vehicles(v) => self.vehicle_packets(v),
                Event::VehicleMeta(v) => self.vehicle_meta(v),
                Event::VehicleStatus(v) => self.vehicle_status(v),
                // EntityFireworkRocket.handleHealthUpdate(17): makeFireworks
                Event::Firework(p, m, item) => {
                    let e = crate::particles::Explosion::of_rocket(item);
                    self.fireworks
                        .push(crate::particles::FireworkStarter::new(p, m, e));
                }
                Event::Mount(v) => self.mounted(v),
                Event::Destroyed(v) => self.destroyed_fx(v),
                Event::AuxSounds(list) => {
                    // RenderGlobal.playAuxSFX
                    for (kind, p) in list {
                        let name = match kind {
                            1010 => "mob.zombie.wood",
                            1011 => "mob.zombie.metal",
                            1012 => "mob.zombie.woodbreak",
                            _ => continue,
                        };
                        let pitch = (self.rng.next_float() - self.rng.next_float()) * 0.2 + 1.0;
                        let at = [p[0] as f64 + 0.5, p[1] as f64 + 0.5, p[2] as f64 + 0.5];
                        self.sound(name, Some(at), 2.0, pitch);
                    }
                }
                Event::BreakAnims(list) => {
                    // RenderGlobal.destroyBlockPartially
                    for (id, p, k) in list {
                        if (0..10).contains(&k) {
                            let e = self.damaged_blocks.entry(id).or_insert((p, k, self.ticks));
                            if e.0 != p {
                                *e = (p, k, self.ticks);
                            }
                            e.1 = k;
                            e.2 = self.ticks;
                        } else {
                            self.damaged_blocks.remove(&id);
                        }
                    }
                }
                Event::Teleport(p) => {
                    self.player.set_position(p[0], p[1], p[2]);
                    self.player.prev_pos = self.player.pos;
                }
                Event::Splashes(v) => {
                    // RenderGlobal.playAuxSFX(2002)
                    let bottle = self
                        .atlas
                        .items
                        .get(&(373, 0))
                        .map(|&t| self.atlas.rects[t as usize]);
                    for (pos, meta) in v {
                        for _ in 0..8 {
                            let r = &mut self.particle_rand;
                            let m = [
                                r.next_gaussian() * 0.15,
                                r.next_double() * 0.2,
                                r.next_gaussian() * 0.15,
                            ];
                            if let Some(rect) = bottle {
                                self.particles.spawned(|ps| ps.item_crack(pos, m, rect));
                            }
                        }
                        let c = crate::potion::liquid_color(meta);
                        let (cr, cg, cb) = (
                            (c >> 16 & 255) as f32 / 255.0,
                            (c >> 8 & 255) as f32 / 255.0,
                            (c & 255) as f32 / 255.0,
                        );
                        let instant = crate::potion::effects_of(meta, false)
                            .iter()
                            .any(|e| e.potion().instant);
                        let base = if instant { 144 } else { 128 };
                        for _ in 0..100 {
                            let r = &mut self.particle_rand;
                            let d = r.next_double() * 4.0;
                            let a = r.next_double() * std::f64::consts::PI * 2.0;
                            let (mx, mz) = (a.cos() * d, a.sin() * d);
                            let my = 0.01 + r.next_double() * 0.5;
                            let f = 0.75 + r.next_float() * 0.25;
                            let start = [pos[0] + mx * 0.1, pos[1] + 0.3, pos[2] + mz * 0.1];
                            self.particles.spawned(|ps| {
                                ps.spell(start, [mx, my, mz], base, [cr * f, cg * f, cb * f], 1.0);
                            });
                            // multiplyVelocity(d)
                            self.particles.scale_last_motion(d as f32);
                        }
                        let p = self.particle_rand.next_float() * 0.1 + 0.9;
                        self.sound("game.potion.smash", Some(pos), 1.0, p);
                    }
                }
                Event::Moving(v) => {
                    self.world.moving = v;
                    let live: HashSet<[i32; 3]> = self.world.moving.iter().map(|m| m.0).collect();
                    self.moving_mesh.retain(|k, _| live.contains(&k.0));
                }
                Event::CollectOrb(id) => self.collect_orb(id),
                Event::MobCollect(v) => {
                    for (it, mob) in v {
                        self.pickups.push((it, 0, Some(mob)));
                    }
                }
                Event::Collect(v) => {
                    // NetHandlerPlayClient.handleCollectItem
                    for it in v {
                        self.pickups.push((it, 0, None));
                    }
                }
                Event::Push(v) => {
                    for (min, max, d) in v {
                        let b = crate::entity::player::Aabb { min, max };
                        if self.player.bb.intersects(&b) {
                            self.player.push_move(&self.world, d[0], d[1], d[2]);
                        }
                    }
                }
                Event::Mobs(m, a) => {
                    // EntityLivingBase.onDeathUpdate: a puff of smoke when the body vanishes
                    for v in &m {
                        if v.death_time >= 18 && self.dying.insert(v.id) {
                            for _ in 0..20 {
                                let r = &mut self.particle_rand;
                                let p = [
                                    v.pos[0]
                                        + (r.next_float() as f64 * 2.0 - 1.0) * (v.bb[3] - v.bb[0]),
                                    v.pos[1] + r.next_float() as f64 * (v.bb[4] - v.bb[1]),
                                    v.pos[2]
                                        + (r.next_float() as f64 * 2.0 - 1.0) * (v.bb[3] - v.bb[0]),
                                ];
                                let mv = [
                                    r.next_gaussian() * 0.02,
                                    r.next_gaussian() * 0.02,
                                    r.next_gaussian() * 0.02,
                                ];
                                self.particles.spawned(|ps| ps.explode(p, mv));
                            }
                        }
                    }
                    if self.dying.len() > 256 {
                        let alive: std::collections::HashSet<u32> =
                            m.iter().map(|v| v.id).collect();
                        self.dying.retain(|i| alive.contains(i));
                    }
                    self.merge_mobs(m);
                    self.arrows = a;
                }
                Event::Hits(_) => {}
                Event::SignText(p, t) => {
                    self.sign_texts.insert(p, t);
                }
                Event::Skull(p, k, r) => {
                    self.skulls.insert(p, (k, r));
                }
                Event::OpenSign(p) => {
                    // NetHandlerPlayClient.handleSignEditorOpen: GuiEditSign
                    let mut lines: [String; 4] = Default::default();
                    // MINERUST_SIGN=a|b|c|d prefills the editor (captures)
                    if let Ok(t) = std::env::var("MINERUST_SIGN") {
                        for (i, l) in t.split('|').take(4).enumerate() {
                            lines[i] = l.chars().take(15).collect();
                        }
                    }
                    self.sign_edit = Some((p, lines, 0, 0));
                    if std::env::var_os("MINERUST_SIGN_DONE").is_some() {
                        let _ = self.close_sign();
                        continue;
                    }
                    self.screen = Screen::Sign;
                    self.input = Input::default();
                }
                Event::Statistics(list) => self.handle_statistics(list),
                Event::Chat(list) => {
                    for m in list {
                        let t = m.text(&self.lang);
                        self.print_chat(&t);
                    }
                }
                Event::GameMode(m) => {
                    self.mode = m;
                    self.player.allow_flying = m == GameMode::Creative;
                    if m != GameMode::Creative {
                        self.player.flying = false;
                    }
                }
                Event::PlayerStats(v) => {
                    self.stats.apply_view(&v);
                    self.player.speed_attr = v.speed;
                    self.player.food_level = v.food;
                    self.player.blind = v.effects.iter().any(|e| e.id == crate::potion::BLINDNESS);
                    self.player.jump_boost = v
                        .effects
                        .iter()
                        .find(|e| e.id == crate::potion::JUMP)
                        .map(|e| e.amplifier);
                }
                Event::Velocity { set, add } => {
                    // S27PacketExplosion adds, S12PacketEntityVelocity sets
                    for a in 0..3 {
                        self.player.motion[a] += add[a];
                    }
                    if let Some(v) = set {
                        self.player.motion = v;
                    }
                }
                Event::CritFx(id, magic) => {
                    // EntityCrit2FX: its constructor runs onUpdate once
                    self.crit_emitter_update(id, magic);
                    self.crit_emitters.push((id, magic, 1));
                }
                Event::StopSprinting => self.player.sprinting = false,
                // Minecraft.func_147121_ag: interactWithEntitySendPacket was
                // false, so the held item is used (sendUseItem)
                Event::InteractPassed => {
                    if self.view.inv.held().is_some() {
                        self.use_item_air();
                    }
                }
                Event::Sleep(Some((bed, pos))) => {
                    // NetHandlerPlayClient.handleUseBed -> sleepInBedAt on the
                    // client: posY is what the server worked out
                    let d = self.world.peek_meta(bed[0], bed[1], bed[2]) as i32 & 3;
                    self.sleeping = Some((bed, d));
                    self.sleep_timer = 0;
                    self.player.motion = [0.0; 3];
                    self.player.set_eye_position(pos[0], pos[1], pos[2]);
                    self.player.prev_pos = self.player.pos;
                    // Minecraft.runTick: GuiSleepMP while sleeping
                    let _ = self.open_chat("");
                    self.sleep_gui = true;
                }
                Event::Sleep(None) => {
                    // S0BPacketAnimation 2: wakeUpPlayer(false, false, false)
                    self.sleeping = None;
                    self.sleep_timer = 100;
                    if self.sleep_gui {
                        self.sleep_gui = false;
                        if self.chat_screen.take().is_some() {
                            self.chat.reset_scroll();
                            self.screen = Screen::Playing;
                        }
                    }
                }
                Event::BoneMeal([x, y, z]) => {
                    // ItemDye.func_150918_a(world, x, y, z, 0): 15
                    // happyVillager inside the block's bounds
                    let id = self.world.peek_block(x, y, z);
                    if crate::block::material(id) != Material::Air {
                        let w = &self.world;
                        let get =
                            |a: i32, b: i32, c: i32| (w.peek_block(a, b, c), w.peek_meta(a, b, c));
                        let m = w.peek_meta(x, y, z);
                        let top = crate::shapes::selection(&get, id, m, x, y, z)[4] as f64;
                        for _ in 0..15 {
                            let (vx, vy, vz, fx, fy, fz) = crate::util::math::item_rand(|r| {
                                (
                                    r.next_gaussian() * 0.02,
                                    r.next_gaussian() * 0.02,
                                    r.next_gaussian() * 0.02,
                                    r.next_float() as f64,
                                    r.next_float() as f64,
                                    r.next_float() as f64,
                                )
                            });
                            self.particles.spawned(|ps| {
                                ps.happy_villager(
                                    [x as f64 + fx, y as f64 + fy * top, z as f64 + fz],
                                    [vx, vy, vz],
                                );
                            });
                        }
                    }
                }
                Event::Using(u) => {
                    self.using = u;
                    self.player.using_item = u.is_some();
                }
                Event::UseFinished(st) => {
                    // handleHealthUpdate(9) -> onItemUseFinish on the client
                    self.update_item_use(st, 16);
                    self.using = None;
                    self.player.using_item = false;
                    if crate::item_use::food(st.id, st.meta).is_some() {
                        let r = &mut self.particle_rand;
                        let p = r.next_float() * 0.1 + 0.9;
                        self.sound("random.burp", None, 0.5, p);
                    }
                }
                Event::Hurt => {
                    // EntityLivingBase.handleHealthUpdate(2): limbSwingAmount
                    // 1.5, attackedAtYaw 0 (so the camera always tilts the
                    // same way), hurtTime
                    self.p_limb[1] = 1.5;
                    self.stats.hurt_time = crate::entity::survival::MAX_HURT_TIME;
                    self.stats.hurt_resistance = 20;
                    let r = &mut self.particle_rand;
                    let p = (r.next_float() - r.next_float()) * 0.2 + 1.0;
                    self.sound("game.player.hurt", None, 1.0, p);
                }
                Event::Died(msg) => {
                    self.stats.health = 0.0;
                    self.stats.dead = true;
                    self.stats.death_time = 0;
                    self.stats.death_text = self.lang.format(&msg.key, &msg.args);
                    self.death_sent = true;
                    self.screen = Screen::Dead;
                    self.input = Input::default();
                    self.dig = None;
                }
                Event::Sounds(list) => {
                    for (name, pos, vol, pitch) in list {
                        self.sound(name, Some(pos), vol, pitch);
                    }
                }
                Event::Tnt(list) => self.tnt = list,
                Event::Fireballs(list) => self.fireballs = list,
                Event::FallDust(list) => {
                    for (p, n) in list {
                        let id = self.world.peek_block(p[0], p[1], p[2]);
                        if block::material(id) != crate::block::Material::Air {
                            let meta = self.world.peek_meta(p[0], p[1], p[2]);
                            let tex = self
                                .atlas
                                .tile(id, meta, 0)
                                .or_else(|| self.atlas.tile(id, meta, 2));
                            self.particles.spawned(|ps| ps.fall_dust(p, n, tex));
                        }
                    }
                }
                Event::Particles(list) => {
                    for (kind, p, m) in list {
                        self.named_particle(kind, p, m);
                    }
                }
                Event::Notes(list) => {
                    // BlockNote.onBlockEventReceived
                    for (p, inst, pitch) in list {
                        let f = 2f32.powf((pitch - 12) as f32 / 12.0);
                        let c = [p[0] as f64 + 0.5, p[1] as f64 + 0.5, p[2] as f64 + 0.5];
                        self.sound(inst, Some(c), 3.0, f);
                        self.particles
                            .spawned(|ps| ps.note([c[0], p[1] as f64 + 1.2, c[2]], pitch));
                    }
                }
                Event::Explosions(list) => {
                    for e in list {
                        self.explosion_b(e);
                    }
                }
                Event::Weather(w) => self.weather = w,
                Event::SpawnPosition(p) => self.spawn_point = p,
                // NetHandlerPlayClient.handleTimeUpdate, WorldClient.setWorldTime
                Event::TimeUpdate(_, t, cycle) => {
                    self.world_time = t;
                    self.day_cycle = cycle;
                }
                Event::Respawn(p) => {
                    self.player = Player::new(p[0], p[1], p[2]);
                    self.player.allow_flying = self.mode == GameMode::Creative;
                    self.stats = Stats::default();
                    self.death_sent = false;
                    self.screen = Screen::Playing;
                }
            }
        }
        let (pcx, pcz) = self.player_chunk();
        if (pcx, pcz) != self.last_center {
            self.last_center = (pcx, pcz);
            let _ = self.sim.commands.send(Command::Center(pcx, pcz));
        }
        let r = self.render_distance;
        // Nearest first: changed chunks that already have a mesh, then new ones.
        for i in 0..self.offsets.len() {
            if self.in_flight >= self.max_in_flight {
                break;
            }
            let (dx, dz) = self.offsets[i];
            let key = (pcx + dx, pcz + dz);
            if self.pending_mesh.contains(&key) {
                continue;
            }
            // a chunk without render chunks yet is built whole
            let mask = if self.renderer.has_mesh(key) {
                match self.dirty.get(&key) {
                    Some(&m) => m,
                    None => continue,
                }
            } else {
                0xFFFF
            };
            if let Some(n) = self.mesh_input(key.0, key.1) {
                self.dirty.remove(&key);
                self.pending_mesh.insert(key);
                self.in_flight += 1;
                let _ = self.jobs.send(Job(n, mask));
            }
        }
        // Chunks outside the view never get meshed; forget their dirty marks.
        self.dirty
            .retain(|k, _| (k.0 - pcx).abs() <= r && (k.1 - pcz).abs() <= r);
    }

    fn unload_far(&mut self) {
        let (pcx, pcz) = self.player_chunk();
        let r = self.render_distance;
        for dx in -(r + 6)..=(r + 6) {
            for dz in -(r + 6)..=(r + 6) {
                if dx.abs() > r + 1 || dz.abs() > r + 1 {
                    self.renderer.remove_mesh((pcx + dx, pcz + dz));
                }
            }
        }
    }

    /// Saves the world and stops the server thread.
    pub fn shutdown(&mut self) {
        let p = &self.player;
        let _ = self.sim.commands.send(Command::Player(
            [p.pos[0], p.bb.min[1], p.pos[2]],
            [p.yaw, p.pitch],
            p.flying,
        ));
        self.sim.shutdown();
    }
    fn celestial_angle(&self, partial: f32) -> f32 {
        match self.dim {
            -1 => 0.5,
            1 => 0.0,
            _ => sim::celestial_angle(self.world_time, partial),
        }
    }

    /// `RenderGlobal.updateClouds`: damage older than 400 ticks goes.
    fn tick_damaged_blocks(&mut self) {
        if self.ticks % 20 == 0 {
            let now = self.ticks;
            self.damaged_blocks.retain(|_, e| now - e.2 <= 400);
        }
    }

    /// `TextureCompass.updateAnimation` and `TextureClock.updateAnimation`.
    fn tick_dials(&mut self) {
        use std::f64::consts::PI;
        let surface = self.dim == 0;
        // TextureCompass.updateCompass(world, posX, posZ, rotationYaw, false, false)
        let n = self.atlas.compass.frames.len();
        if n > 0 {
            let sp = self.spawn_point;
            let dx = sp[0] as f64 - self.player.pos[0];
            let dz = sp[2] as f64 - self.player.pos[2];
            let yaw = self.player.yaw as f64 % 360.0;
            let mut target = -((yaw - 90.0) * PI / 180.0 - dz.atan2(dx));
            if !surface {
                target = self.dial_rand.next_double() * PI as f32 as f64 * 2.0;
            }
            let (cur, delta, frame) = &mut self.compass_state;
            let mut d = target - *cur;
            while d < -PI {
                d += PI * 2.0;
            }
            while d >= PI {
                d -= PI * 2.0;
            }
            d = d.clamp(-1.0, 1.0);
            *delta += d * 0.1;
            *delta *= 0.8;
            *cur += *delta;
            let mut f = ((*cur / (PI * 2.0) + 1.0) * n as f64) as i32 % n as i32;
            while f < 0 {
                f = (f + n as i32) % n as i32;
            }
            if *frame != Some(f as usize) {
                *frame = Some(f as usize);
                let a = &self.atlas.compass;
                self.renderer.queue_tile_update(crate::render::vk::TileUpdate {
                    x: a.x,
                    y: a.y,
                    size: 16,
                    mips: a.frames[f as usize].clone(),
                });
            }
        }
        // TextureClock.updateAnimation
        let n = self.atlas.clock.frames.len();
        if n > 0 {
            let mut target = self.celestial_angle(1.0) as f64;
            if !surface {
                target = self.dial_rand.next_double();
            }
            let (cur, delta, frame) = &mut self.clock_state;
            let mut d = target - *cur;
            while d < -0.5 {
                d += 1.0;
            }
            while d >= 0.5 {
                d -= 1.0;
            }
            d = d.clamp(-1.0, 1.0);
            *delta += d * 0.1;
            *delta *= 0.8;
            *cur += *delta;
            let mut f = ((*cur + 1.0) * n as f64) as i32 % n as i32;
            while f < 0 {
                f = (f + n as i32) % n as i32;
            }
            if *frame != Some(f as usize) {
                *frame = Some(f as usize);
                let a = &self.atlas.clock;
                self.renderer.queue_tile_update(crate::render::vk::TileUpdate {
                    x: a.x,
                    y: a.y,
                    size: 16,
                    mips: a.frames[f as usize].clone(),
                });
            }
        }
    }

    /// `TextureAtlasSprite.updateAnimation` for every animated tile.
    fn tick_animations(&mut self) {
        self.minecart_sounds();
        // World.updateWeather: the flash fades, then the bolts update
        if self.last_lightning > 0 {
            self.last_lightning -= 1;
        }
        let mut bolts = std::mem::take(&mut self.bolts);
        bolts.retain_mut(|(_, state, living, vertex, r)| {
            *state -= 1;
            if *state < 0 {
                if *living == 0 {
                    return false;
                } else if *state < -r.next_int_n(10) {
                    *living -= 1;
                    *state = 1;
                    *vertex = r.next_long();
                }
            }
            true
        });
        if bolts.iter().any(|b| b.1 >= 0) {
            self.last_lightning = 2;
        }
        self.bolts = bolts;
        if let Some(b) = self.book_gui.as_mut() {
            b.ticks += 1;
        }
        self.refresh_map_slots();
        self.tick_particles();
        if self.anim_state.len() != self.atlas.anims.len() {
            self.anim_state = vec![(0, 0); self.atlas.anims.len()];
        }
        self.tick_dials();
        self.tick_damaged_blocks();
        for (i, a) in self.atlas.anims.iter().enumerate() {
            let st = &mut self.anim_state[i];
            st.0 += 1;
            if st.0 >= a.frametime && !a.order.is_empty() {
                st.0 = 0;
                st.1 = (st.1 + 1) % a.order.len();
                let frame = a.order[st.1];
                self.renderer
                    .queue_tile_update(crate::render::vk::TileUpdate {
                        x: a.x,
                        y: a.y,
                        size: a.size,
                        mips: a.frames[frame].clone(),
                    });
            }
        }
    }

    /// Fancy cloud layer around cloud texel `(bx, bz)`: one 12x4x12 box per
    /// opaque texel of `clouds.png`, only the faces on the outside.
    /// `RenderGlobal.renderClouds` with fast graphics: `clouds.png` laid
    /// flat, 8 blocks a texel, 256 blocks each way; here one quad per
    /// opaque texel around texel `(bx, bz)`.
    fn build_clouds_flat(&self, bx: i32, bz: i32) -> Vec<crate::render::mesher::Vertex> {
        use crate::render::mesher::Vertex;
        let mut v: Vec<Vertex> = Vec::new();
        for i in -33..33i32 {
            for j in -33..33i32 {
                if !self.atlas.clouds
                    [((((bz + j) & 255) as usize) << 8) | ((bx + i) & 255) as usize]
                {
                    continue;
                }
                let (x0, x1) = (i as f32 * 8.0, (i + 1) as f32 * 8.0);
                let (z0, z1) = (j as f32 * 8.0, (j + 1) as f32 * 8.0);
                for pos in [[x0, 0.0, z1], [x1, 0.0, z1], [x1, 0.0, z0], [x0, 0.0, z0]] {
                    v.push(Vertex {
                        pos,
                        uv: [0.0; 2],
                        color: [255; 4],
                        light: [0; 4],
                    });
                }
            }
        }
        v
    }

    fn build_clouds(&self, bx: i32, bz: i32) -> Vec<crate::render::mesher::Vertex> {
        use crate::render::mesher::Vertex;
        let opaque = |i: i32, j: i32| {
            self.atlas.clouds[((((bz + j) & 255) as usize) << 8) | ((bx + i) & 255) as usize]
        };
        let mut v: Vec<Vertex> = Vec::new();
        let mut quad = |p: [[f32; 3]; 4], shade: f32| {
            let c = (shade * 255.0) as u8;
            for pos in p {
                v.push(Vertex {
                    pos,
                    uv: [0.0; 2],
                    color: [c, c, c, 255],
                    light: [0; 4],
                });
            }
        };
        for i in -24..40i32 {
            for j in -24..40i32 {
                if !opaque(i, j) {
                    continue;
                }
                let (x0, x1) = (i as f32 * 12.0, (i + 1) as f32 * 12.0);
                let (z0, z1) = (j as f32 * 12.0, (j + 1) as f32 * 12.0);
                quad(
                    [[x0, 0.0, z1], [x1, 0.0, z1], [x1, 0.0, z0], [x0, 0.0, z0]],
                    0.7,
                );
                quad(
                    [[x0, 4.0, z1], [x1, 4.0, z1], [x1, 4.0, z0], [x0, 4.0, z0]],
                    1.0,
                );
                if !opaque(i - 1, j) {
                    quad(
                        [[x0, 0.0, z1], [x0, 4.0, z1], [x0, 4.0, z0], [x0, 0.0, z0]],
                        0.9,
                    );
                }
                if !opaque(i + 1, j) {
                    quad(
                        [[x1, 0.0, z1], [x1, 4.0, z1], [x1, 4.0, z0], [x1, 0.0, z0]],
                        0.9,
                    );
                }
                if !opaque(i, j - 1) {
                    quad(
                        [[x0, 4.0, z0], [x1, 4.0, z0], [x1, 0.0, z0], [x0, 0.0, z0]],
                        0.8,
                    );
                }
                if !opaque(i, j + 1) {
                    quad(
                        [[x0, 4.0, z1], [x1, 4.0, z1], [x1, 0.0, z1], [x0, 0.0, z1]],
                        0.8,
                    );
                }
            }
        }
        v
    }

    /// `EntityLivingBase.onUpdate` / `moveEntityWithHeading` for the local
    /// player's model: the body turns towards the movement, the limbs swing.
    fn tick_player_model(&mut self) {
        let wrap = |a: f32| {
            let mut a = a % 360.0;
            if a >= 180.0 {
                a -= 360.0;
            }
            if a < -180.0 {
                a += 360.0;
            }
            a
        };
        let p = &self.player;
        self.p_body[0] = self.p_body[1];
        self.p_yaw = [self.p_yaw[1], p.yaw];
        self.p_pitch = [self.p_pitch[1], p.pitch];
        while self.p_yaw[1] - self.p_yaw[0] < -180.0 {
            self.p_yaw[0] -= 360.0;
        }
        while self.p_yaw[1] - self.p_yaw[0] >= 180.0 {
            self.p_yaw[0] += 360.0;
        }
        let (dx, dz) = (p.pos[0] - p.prev_pos[0], p.pos[2] - p.prev_pos[2]);
        let d2 = (dx * dx + dz * dz) as f32;
        let mut target = self.p_body[1];
        if d2 > 0.0025000002 {
            target = (dz.atan2(dx) as f32) * 180.0 / std::f32::consts::PI - 90.0;
        }
        if self.swing_f > 0.0 {
            target = p.yaw;
        }
        let mut b = self.p_body[1];
        b += wrap(target - b) * 0.3;
        let k = wrap(p.yaw - b).clamp(-75.0, 75.0);
        b = p.yaw - k;
        if k * k > 2500.0 {
            b += k * 0.2;
        }
        self.p_body[1] = b;
        while self.p_body[1] - self.p_body[0] < -180.0 {
            self.p_body[0] -= 360.0;
        }
        while self.p_body[1] - self.p_body[0] >= 180.0 {
            self.p_body[0] += 360.0;
        }
        // limbSwingAmount
        self.p_limb[2] = self.p_limb[1];
        let mut f = (d2.sqrt() * 4.0).min(1.0);
        if p.flying {
            f = (((dx * dx + (p.pos[1] - p.prev_pos[1]).powi(2) + dz * dz) as f32).sqrt() * 4.0)
                .min(1.0);
            f = f.min(1.0);
        }
        self.p_limb[1] += (f - self.p_limb[1]) * 0.4;
        self.p_limb[0] += self.p_limb[1];
    }

    /// `RenderPlayer` for the local player in third person.
    fn draw_player(&self, out: &mut Vec<crate::render::mesher::Vertex>, partial: f32) {
        if self.third_person == 0 || self.stats.dead {
            return;
        }
        let lerp = |a: [f32; 2]| a[0] + (a[1] - a[0]) * partial;
        let body = lerp(self.p_body);
        let yaw = lerp(self.p_yaw);
        let pitch = lerp(self.p_pitch);
        let st = self.equipped_icon();
        let action = self
            .using
            .map(|(u, _)| crate::item_use::action(u.id, u.meta));
        let pose = crate::render::model::Pose {
            limb_swing: self.p_limb[0] - self.p_limb[1] * (1.0 - partial),
            limb_amount: (self.p_limb[2] + (self.p_limb[1] - self.p_limb[2]) * partial).min(1.0),
            age: self.ticks as f32 + partial,
            head_yaw: yaw - body,
            head_pitch: pitch,
            swing: self.swing_progress(partial),
            held_right: match (st, action) {
                (Some(_), Some(crate::item_use::Action::Block)) => 3,
                (Some(_), _) => 1,
                _ => 0,
            },
            sneak: self.player.sneaking,
            riding: self.player.riding,
            aimed_bow: matches!(action, Some(crate::item_use::Action::Bow)),
        };
        let p = self.player.pos;
        let (lx, ly, lz) = (
            math::floor_double(p[0]),
            math::floor_double(p[1] - 1.62 + 1.8 * 0.66),
            math::floor_double(p[2]),
        );
        let (sky, blk) = (
            self.world.light(true, lx, ly, lz),
            self.world.light(false, lx, ly, lz),
        );
        let burning = self.stats.fire > 0;
        let (blk, sky) = if burning { (15, 15) } else { (blk, sky) };
        let m = crate::render::model::player_matrix([0.0, -1.62, 0.0], body);
        let tint = if self.stats.hurt_time > 0 {
            [1.0, 0.6, 0.6, 1.0]
        } else {
            [1.0; 4]
        };
        let blocking = matches!(action, Some(crate::item_use::Action::Block));
        for (raw, _) in crate::render::model::player_layers(
            &self.atlas,
            &m,
            &pose,
            &self.view.inv.armor,
            st,
            blocking,
            [(blk << 4) as u8, (sky << 4) as u8, 0, 0],
            tint,
        ) {
            out.extend(raw);
        }
        if burning {
            // the client player's posY is its eye: posY - minY = 1.62
            entities::on_fire(
                out,
                &self.atlas,
                [0.0, 0.0, 0.0],
                0.6,
                1.8,
                1.62,
                self.view_yaw(),
                [240, 240, 0, 0],
            );
        }
    }

    /// `RenderManager.playerViewY`: the camera's yaw, turned round in the
    /// front-facing third-person view.
    fn view_yaw(&self) -> f32 {
        if self.third_person == 2 {
            self.player.yaw + 180.0
        } else {
            self.player.yaw
        }
    }

    fn tick(&mut self) {
        self.tick_tables();
        if let Open::Enchant(_) = self.view.open {
            // GuiEnchantment.updateScreen
            let item = self.view.tile_slots.first().copied().flatten();
            self.book.tick(item, self.view.enchant);
        } else {
            self.book = Default::default();
        }
        if let Some(e) = self.sign_edit.as_mut() {
            e.3 += 1;
        }
        if let Some(c) = self.chat_screen.as_mut() {
            c.field.cursor_counter += 1;
        }
        self.tick_chests();
        if let Some(c) = self.creative.as_mut() {
            c.tick();
            // updateScreen: out of creative mode it becomes GuiInventory
            if self.mode != GameMode::Creative {
                self.creative = None;
                let _ = self.sim.commands.send(Command::OpenInventory);
            }
        }
        self.tick_mobs();
        if self.ticks == 40 {
            // MINERUST_CHAT=line;line  chat lines sent once the world is up
            if let Ok(list) = std::env::var("MINERUST_CHAT") {
                for l in list.split(';') {
                    let _ = self.sim.commands.send(Command::Chat(l.to_string()));
                }
            }
        }
        // MINERUST_USE_AIR: one right click in the air once the world is up
        // (casts a fishing rod, throws a snowball) for captures
        if self.ticks == 60 && std::env::var_os("MINERUST_USE_AIR").is_some() {
            self.use_item_air();
        }
        if self.ticks >= 60 {
            if let Some(t) = self.start_use.take() {
                // the middle of the top face, unless MINERUST_HIT=x,y,z
                // (block space) and MINERUST_FACE=0..5 say otherwise
                let mut h = [0.5f64, 1.0, 0.5];
                if let Ok(v) = std::env::var("MINERUST_HIT") {
                    for (k, part) in v.split(',').take(3).enumerate() {
                        h[k] = part.parse().unwrap_or(h[k]);
                    }
                }
                let n = match std::env::var("MINERUST_FACE").ok().as_deref() {
                    Some("0") => [0, -1, 0],
                    Some("2") => [0, 0, -1],
                    Some("3") => [0, 0, 1],
                    Some("4") => [-1, 0, 0],
                    Some("5") => [1, 0, 0],
                    _ => [0, 1, 0],
                };
                self.hit = [t[0] as f64 + h[0], t[1] as f64 + h[1], t[2] as f64 + h[2]];
                self.use_on(t, n);
                self.send_start_clicks();
            }
        }
        self.ticks += 1;
        // EntityPlayerSP.onLivingUpdate: timeInPortal
        self.prev_portal_time = self.portal_time;
        if self.in_portal {
            self.portal_time = (self.portal_time + 0.0125).min(1.0);
        } else if self.portal_time > 0.0 {
            self.portal_time = (self.portal_time - 0.05).max(0.0);
        }
        self.tick_animations();
        // WorldClient.tick
        if !self.freeze_time && self.day_cycle {
            self.world_time += 1;
        }
        // EntityRenderer.updateRenderer
        self.fov_hand_prev = self.fov_hand;
        self.fov_hand += (self.player.fov_multiplier() - self.fov_hand) * 0.5f32;
        self.fov_hand = self.fov_hand.clamp(0.1, 1.5);
        // updateTorchFlicker
        let mut rnd = || self.rng.next_double();
        self.torch_dx = (self.torch_dx as f64 + (rnd() - rnd()) * rnd() * rnd()) as f32;
        self.torch_dx = (self.torch_dx as f64 * 0.9) as f32;
        self.torch_x += (self.torch_dx - self.torch_x) * 1.0f32;
        self.fog_color2 = self.fog_color1;
        let p = self.player.pos;
        let (bx, by, bz) = (
            math::floor_double(p[0]),
            math::floor_double(p[1]),
            math::floor_double(p[2]),
        );
        let sub = client_skylight_subtracted();
        let level = (self.world.light(true, bx, by, bz) - sub)
            .max(self.world.light(false, bx, by, bz))
            .clamp(0, 15);
        let f1 = self.light_table[level as usize];
        let f2 = self.render_distance as f32 / 16.0f32;
        // MC-31681: vanilla dims the fog (and the sky behind it) with the
        // light level at the player, so it darkens under trees and indoors.
        let f3 = if crate::compat::fixed() {
            1.0
        } else {
            f1 * (1.0f32 - f2) + f2
        };
        self.fog_color1 += (f3 - self.fog_color1) * 0.1f32;

        let (pcx, pcz) = self.player_chunk();
        if self.ticks % 20 == 0 {
            let p = &self.player;
            let _ = self.sim.commands.send(Command::Player(
                [p.pos[0], p.bb.min[1], p.pos[2]],
                [p.yaw, p.pitch],
                p.flying,
            ));
        }
        let before = [
            self.player.pos[0],
            self.player.bb.min[1],
            self.player.pos[2],
        ];
        // GuiIngame.updateTick
        if let Some(r) = &mut self.record {
            r.1 -= 1;
            if r.1 <= 0 {
                self.record = None;
            }
        }
        // World.updateEntities: a rider is updated right after its vehicle.
        if self.riding.is_some() {
            self.tick_boats();
        }
        // EntityPlayer.onUpdate: the sleep timer
        if self.sleeping.is_some() {
            self.sleep_timer = (self.sleep_timer + 1).min(100);
        } else if self.sleep_timer > 0 {
            self.sleep_timer += 1;
            if self.sleep_timer >= 110 {
                self.sleep_timer = 0;
            }
        }
        if self.world.loaded(pcx << 4, pcz << 4) && !self.stats.dead && self.sleeping.is_none() {
            let input = if self.screen == Screen::Playing {
                self.input
            } else {
                Input::default()
            };
            if self.riding.is_some() {
                // Entity.updateRidden
                self.player.motion = [0.0; 3];
            }
            self.player.tick(&self.world, &input);
            self.update_rider();
            self.tick_player_model();
            // Entity.moveEntity: a step sound per block walked.
            if self.riding.is_none()
                && self.player.on_ground
                && !self.player.sneaking
                && self.player.distance_walked > self.next_step
            {
                self.next_step = self.player.distance_walked.floor() + 1.0;
                let (x, y, z) = (
                    math::floor_double(self.player.pos[0]),
                    math::floor_double(self.player.bb.min[1] - 0.2),
                    math::floor_double(self.player.pos[2]),
                );
                let id = self.world.peek_block(x, y, z);
                if id != crate::block::AIR {
                    self.block_sound(id, [x, y, z], "step");
                }
            }
            if let Some(a) = self.audio.as_mut() {
                a.set_listener(self.player.pos, self.player.yaw);
                a.tick_music();
            }
            // EntityRenderer.addRainParticles: the rain sound
            if self.weather.rain > 0.2 {
                let (x, z) = (
                    math::floor_double(self.player.pos[0]),
                    math::floor_double(self.player.pos[2]),
                );
                let top = self.world.precipitation_height(x, z);
                let b = self.world.biome(x, z);
                if b.can_rain_or_snow()
                    && b.float_temperature(&self.noise, x, top, z) >= 0.15
                    && (top as f64) < self.player.pos[1] + 10.0
                {
                    let n = self.ticks as i32;
                    if (n.wrapping_mul(1103515245) >> 8 & 3) < self.rain_sound {
                        self.rain_sound = 0;
                        let above = (top as f64) > self.player.pos[1] + 1.0;
                        let p = [x as f64 + 0.5, top as f64, z as f64 + 0.5];
                        self.sound(
                            "ambient.weather.rain",
                            Some(p),
                            if above { 0.1 } else { 0.2 },
                            if above { 0.5 } else { 1.0 },
                        );
                    } else {
                        self.rain_sound += 1;
                    }
                }
            }
        } else {
            self.player.prev_pos = self.player.pos;
        }
        if self.riding.is_none() {
            self.tick_boats();
        }
        let after = [
            self.player.pos[0],
            self.player.bb.min[1],
            self.player.pos[2],
        ];
        let moved = [
            after[0] - before[0],
            after[1] - before[1],
            after[2] - before[2],
        ];
        self.stats.client_tick(&self.player, moved);
        {
            let p = &self.player;
            let _ = self.sim.commands.send(Command::Motion {
                on_ground: p.on_ground,
                sneaking: p.sneaking,
                sprinting: p.sprinting,
            });
            let _ = self
                .sim
                .commands
                .send(Command::Player(after, [p.yaw, p.pitch], p.flying));
            if self.riding.is_some() {
                // EntityClientPlayerMP.onUpdate: C0CPacketInput
                let i = if self.screen == Screen::Playing {
                    self.input
                } else {
                    Input::default()
                };
                let _ = self.sim.commands.send(Command::Input {
                    strafe: p.move_strafe,
                    forward: p.move_forward,
                    jump: i.jump,
                    sneak: i.sneak,
                });
            }
            // EntityPlayerSP.onLivingUpdate: charging a horse's jump
            let jump_now = self.screen == Screen::Playing && self.input.jump;
            if self.riding_horse() {
                if self.horse_jump_counter < 0 {
                    self.horse_jump_counter += 1;
                    if self.horse_jump_counter == 0 {
                        self.horse_jump_power = 0.0;
                    }
                }
                if self.was_jumping && !jump_now {
                    // C0BPacketEntityAction(6, power * 100)
                    self.horse_jump_counter = -10;
                    let power = (self.horse_jump_power * 100.0) as i32;
                    let _ = self.sim.commands.send(Command::HorseJump(power));
                } else if !self.was_jumping && jump_now {
                    self.horse_jump_counter = 0;
                    self.horse_jump_power = 0.0;
                } else if self.was_jumping {
                    self.horse_jump_counter += 1;
                    let c = self.horse_jump_counter;
                    self.horse_jump_power = if c < 10 {
                        c as f32 * 0.1
                    } else {
                        0.8 + 2.0 / (c - 9) as f32 * 0.1
                    };
                }
            } else {
                self.horse_jump_power = 0.0;
            }
            self.was_jumping = jump_now;
        }
        // EntityPlayer.onUpdate: the item in use (client side)
        if let Some((st, n)) = self.using {
            if self.view.inv.held() == Some(st) {
                if n <= 25 && n % 4 == 0 {
                    self.update_item_use(st, 5);
                }
                self.using = Some((st, (n - 1).max(1)));
            } else {
                self.using = None;
                self.player.using_item = false;
            }
            // Minecraft.runTick: the use key let go
            if !self.right_down && self.using.is_some() {
                let _ = self.sim.commands.send(Command::StopUsing);
                self.using = None;
                self.player.using_item = false;
            }
        }
        // Digging, right-click repeat, arm swing, equip animation.
        self.tick_digging();
        if self.right_delay > 0 {
            self.right_delay -= 1;
        }
        if self.right_down
            && self.right_delay == 0
            && self.screen == Screen::Playing
            && !self.stats.dead
            && self.using.is_none()
        {
            self.use_item();
        }
        // EntityLivingBase.updateArmSwingProgress
        self.prev_swing_f = self.swing_f;
        let end = self.arm_swing_end();
        if self.swinging {
            self.swing += 1;
            if self.swing >= end {
                self.swing = 0;
                self.swinging = false;
            }
        } else {
            self.swing = 0;
        }
        self.swing_f = self.swing as f32 / end as f32;
        self.prev_equip = self.equip;
        // MINERUST_AUTOCLICK=tick:button,...  presses (and releases on the
        // next tick) a mouse button, for checking the hand; with
        // MINERUST_LOG_HAND the hand's state is printed every tick
        if self.screen == Screen::Playing && self.loading == 2 {
            if let Ok(list) = std::env::var("MINERUST_AUTOCLICK") {
                self.auto_ticks += 1;
                for part in list.split(',') {
                    let f: Vec<i32> = part.split(':').filter_map(|x| x.parse().ok()).collect();
                    if f.len() == 2 && f[0] == self.auto_ticks {
                        let _ = self.mouse_button(f[1] as u8, true, false);
                        if f[1] == 0 {
                            self.left_down = true;
                        } else {
                            self.right_down = true;
                        }
                    }
                    if f.len() == 2 && f[0] + 1 == self.auto_ticks {
                        let _ = self.mouse_button(f[1] as u8, false, false);
                        if f[1] == 0 {
                            self.left_down = false;
                        } else {
                            self.right_down = false;
                        }
                    }
                }
            }
            if std::env::var_os("MINERUST_LOG_HAND").is_some() {
                eprintln!(
                    "hand t{} equip {:.2} swing {} {} {:.3} held {:?}",
                    self.auto_ticks,
                    self.equip,
                    self.swing,
                    self.swinging,
                    self.swing_f,
                    self.view.inv.held().map(|s| (s.id, s.meta, s.count))
                );
            }
        }
        // EntityPlayerSP.updateEntityActionState: the arm follows the view
        self.prev_render_arm = self.render_arm;
        self.render_arm[0] += (self.player.pitch - self.render_arm[0]) * 0.5;
        self.render_arm[1] += (self.player.yaw - self.render_arm[1]) * 0.5;
        // ItemRenderer.updateEquippedItem: the same slot and the same item
        let held = self.view.inv.held();
        let slot = self.view.inv.selected;
        let same = slot == self.equipped_slot
            && match (held, self.equipped) {
                (Some(a), Some(b)) => a.id == b.id && a.meta == b.meta,
                (None, None) => true,
                _ => false,
            };
        let target = if same { 1.0 } else { 0.0 };
        self.equip += (target - self.equip).clamp(-0.4, 0.4);
        if self.equip < 0.1 {
            self.equipped = held;
            self.equipped_slot = slot;
        }
        if self.ticks % 40 == 0 {
            self.unload_far();
        }
    }

    fn eye(&self, partial: f32) -> [f64; 3] {
        let (a, b) = (self.player.prev_pos, self.player.pos);
        let t = partial as f64;
        [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
        ]
    }

    /// `Entity.getLook`
    fn look(&self) -> [f64; 3] {
        let (yaw, pitch) = (self.player.yaw, self.player.pitch);
        let f1 = math::cos(-yaw * 0.017453292f32 - math::PI);
        let f2 = math::sin(-yaw * 0.017453292f32 - math::PI);
        let f3 = -math::cos(-pitch * 0.017453292f32);
        let f4 = math::sin(-pitch * 0.017453292f32);
        [(f2 * f3) as f64, f4 as f64, (f1 * f3) as f64]
    }

    /// Texture and tint of the particles a block breaks into (`EntityDiggingFX`).
    fn debris(&self, pos: [i32; 3]) -> (Option<[f32; 4]>, [f32; 3]) {
        let id = self.world.peek_block(pos[0], pos[1], pos[2]);
        let meta = self.world.peek_meta(pos[0], pos[1], pos[2]);
        self.debris_of(id, meta, pos)
    }

    /// `EntityDiggingFX`: `getIcon(0, meta)` and `applyColourMultiplier`
    /// (grass keeps its untinted bottom face, dirt).
    fn debris_of(&self, id: u8, meta: u8, pos: [i32; 3]) -> (Option<[f32; 4]>, [f32; 3]) {
        let tex = self
            .atlas
            .tile(id, meta, 0)
            .or_else(|| self.atlas.tile(id, meta, 2));
        let tint = if id == block::GRASS {
            [1.0; 3]
        } else {
            crate::render::mesher::rgb(self.color_multiplier(id, pos))
        };
        (tex, tint)
    }

    /// `EntityDiggingFX.applyRenderColor` (the sprinting dust).
    fn debris_render_color(&self, id: u8, meta: u8) -> (Option<[f32; 4]>, [f32; 3]) {
        let tex = self
            .atlas
            .tile(id, meta, 0)
            .or_else(|| self.atlas.tile(id, meta, 2));
        let c = match id {
            block::GRASS => 0xFFFFFF,
            // Block.getRenderColor
            block::LEAVES if meta & 3 == 1 => 6396257,
            block::LEAVES if meta & 3 == 2 => 8431445,
            block::LEAVES | block::LEAVES2 | block::VINE => 4764952,
            block::TALLGRASS if meta != 0 => self.grass_basic(),
            block::DOUBLE_PLANT if matches!(meta & 7, 2 | 3) => self.grass_basic(),
            block::WATERLILY => 2129968,
            104 | 105 => stem_color(meta as i32),
            _ => 0xFFFFFF,
        };
        (tex, crate::render::mesher::rgb(c))
    }

    /// `ColorizerGrass.getGrassColor(0.5, 1.0)`
    fn grass_basic(&self) -> u32 {
        let (t, r) = (0.5f64, 1.0f64 * 0.5);
        let i = ((1.0 - t) * 255.0) as usize;
        let j = ((1.0 - r) * 255.0) as usize;
        self.atlas
            .grass_map
            .get(j << 8 | i)
            .copied()
            .unwrap_or(0x7FB238)
    }

    /// `Block.colorMultiplier(world, x, y, z)`
    fn color_multiplier(&self, id: u8, p: [i32; 3]) -> u32 {
        let meta = self.world.peek_meta(p[0], p[1], p[2]) as i32;
        let grass = |x: i32, z: i32| {
            self.world
                .biome(x, z)
                .grass_color(&self.noise, &self.atlas.grass_map, x, p[1], z)
        };
        let foliage = |x: i32, z: i32| {
            self.world
                .biome(x, z)
                .foliage_color(&self.noise, &self.atlas.foliage_map, x, p[1], z)
        };
        match id {
            // BlockOldLeaf: spruce and birch have fixed colours
            block::LEAVES if meta & 3 == 1 => 6396257,
            block::LEAVES if meta & 3 == 2 => 8431445,
            // BlockLeaves: the 3x3 biome average
            block::LEAVES | block::LEAVES2 => {
                let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        let c = foliage(p[0] + dx, p[2] + dz);
                        r += (c & 0xFF0000) >> 16;
                        g += (c & 0xFF00) >> 8;
                        b += c & 0xFF;
                    }
                }
                (r / 9 & 255) << 16 | (g / 9 & 255) << 8 | b / 9 & 255
            }
            block::VINE => foliage(p[0], p[2]),
            block::TALLGRASS => {
                if meta == 0 {
                    0xFFFFFF
                } else {
                    grass(p[0], p[2])
                }
            }
            block::DOUBLE_PLANT => {
                // getVariant: the top half reads the half below
                let v = if meta & 8 != 0 {
                    self.world.peek_meta(p[0], p[1] - 1, p[2]) as i32 & 7
                } else {
                    meta & 7
                };
                if v == 2 || v == 3 {
                    grass(p[0], p[2])
                } else {
                    0xFFFFFF
                }
            }
            block::REEDS => grass(p[0], p[2]),
            block::WATERLILY => 2129968,
            104 | 105 => stem_color(meta),
            block::REDSTONE_WIRE => 8388608,
            _ => 0xFFFFFF,
        }
    }

    /// Per-tick particle spawning around the player.
    fn tick_particles(&mut self) {
        // EntityFireworkStarterFX.onUpdate (an effect like the others)
        {
            let cam = self.player.pos;
            let mut sounds = Vec::new();
            let mut list = std::mem::take(&mut self.fireworks);
            list.retain_mut(|f| f.tick(&mut self.particles, cam, &mut sounds));
            self.fireworks = list;
            for (n, p, v, pi) in sounds {
                self.sound(&n, Some(p), v, pi);
            }
        }
        self.particles.tick(&self.world);
        self.tick_crit_emitters();
        // EntityPickupFX.onUpdate: three ticks
        for p in self.pickups.iter_mut() {
            p.1 += 1;
        }
        self.pickups.retain(|p| p.1 < 3);
        for p in self.orb_pickups.iter_mut() {
            p.1 += 1;
        }
        self.orb_pickups.retain(|p| p.1 < 3);
        // EntityEnderman.onLivingUpdate: two portal particles a tick.
        for v in &self.mobs {
            if v.kind == crate::entity::mob::Kind::Enderman && v.death_time == 0 {
                for _ in 0..2 {
                    let r = &mut self.particle_rand;
                    let p = [
                        v.pos[0] + (r.next_double() - 0.5) * 0.6,
                        v.pos[1] + r.next_double() * 2.9 - 0.25,
                        v.pos[2] + (r.next_double() - 0.5) * 0.6,
                    ];
                    let m = [
                        (r.next_double() - 0.5) * 2.0,
                        -r.next_double(),
                        (r.next_double() - 0.5) * 2.0,
                    ];
                    self.particles.spawned(|ps| ps.portal(p, m));
                }
            }
        }
        // EntityTNTPrimed.onUpdate: a puff of smoke every tick.
        for &(pos, _, _) in &self.tnt {
            self.particles
                .spawned(|ps| ps.smoke([pos[0], pos[1] + 0.5, pos[2]], [0.0; 3], false));
        }
        let eye = self.player.pos;
        self.particles.camera = eye;
        self.particles.setting = self.settings().particle_setting;
        self.particles.display_ticks(&self.world, eye);
        // EntityRenderer.addRainParticles
        let mut rain = self.weather.rain;
        if !crate::render::mesher::rb::fancy() {
            rain /= 2.0;
        }
        if rain > 0.0 {
            let mut n = (100.0 * rain * rain) as i32;
            match self.particles.setting {
                1 => n >>= 1,
                2 => n = 0,
                _ => {}
            }
            let (px, py, pz) = (
                math::floor_double(eye[0]),
                math::floor_double(eye[1]),
                math::floor_double(eye[2]),
            );
            for _ in 0..n {
                let r = &mut self.particle_rand;
                let x = px + r.next_int_n(10) - r.next_int_n(10);
                let z = pz + r.next_int_n(10) - r.next_int_n(10);
                let (fx, fz) = (r.next_float() as f64, r.next_float() as f64);
                let y = self.world.precipitation_height(x, z);
                if y > py + 10 || y < py - 10 {
                    continue;
                }
                let b = self.world.biome(x, z);
                if !b.can_rain_or_snow() || b.float_temperature(&self.noise, x, y, z) < 0.2 {
                    continue;
                }
                let below = self.world.peek_block(x, y - 1, z);
                if block::material(below) == Material::Lava {
                    self.particles.smoke(
                        [x as f64 + fx, y as f64 + 0.1, z as f64 + fz],
                        [0.0; 3],
                        false,
                    );
                } else if below != block::AIR {
                    self.particles
                        .rain([x as f64 + fx, y as f64 + 0.1, z as f64 + fz]);
                }
            }
        }
        // Entity.func_110120_aR: dust behind a sprinting player
        if self.player.sprinting && self.player.on_ground && !self.player.in_water {
            let (x, y, z) = (
                math::floor_double(self.player.pos[0]),
                math::floor_double(self.player.bb.min[1] - 0.2),
                math::floor_double(self.player.pos[2]),
            );
            if self.world.peek_block(x, y, z) != block::AIR {
                let id = self.world.peek_block(x, y, z);
                let meta = self.world.peek_meta(x, y, z);
                let (tex, tint) = self.debris_render_color(id, meta);
                let r = &mut self.particle_rand;
                let p = [
                    self.player.pos[0] + (r.next_float() as f64 - 0.5) * 0.6,
                    self.player.bb.min[1] + 0.1,
                    self.player.pos[2] + (r.next_float() as f64 - 0.5) * 0.6,
                ];
                let m = [
                    -self.player.motion[0] * 4.0,
                    1.5,
                    -self.player.motion[2] * 4.0,
                ];
                self.particles.spawned(|ps| ps.dust(p, m, tex, tint));
            }
        }
    }

    fn rain(&self, partial: f32) -> f32 {
        self.weather.prev_rain + (self.weather.rain - self.weather.prev_rain) * partial
    }

    /// `EntityRenderer.renderRainSnow` geometry around the camera.
    fn weather_quads(&self, eye: [f64; 3], partial: f32) -> Vec<crate::render::mesher::Vertex> {
        let mut out = Vec::new();
        let rain = self.rain(partial);
        if rain <= 0.0 {
            return out;
        }
        let (Some(rt), Some(st)) = (
            self.atlas.mob_tex("../environment/rain"),
            self.atlas.mob_tex("../environment/snow"),
        ) else {
            return out;
        };
        let (px, pz) = (math::floor_double(eye[0]), math::floor_double(eye[2]));
        let py = math::floor_double(eye[1]);
        // EntityRenderer.renderRainSnow: 5 blocks around with fast graphics
        let r = if crate::render::mesher::rb::fancy() {
            10
        } else {
            5
        };
        let t = self.ticks as f32 + partial;
        for z in pz - r..=pz + r {
            for x in px - r..=px + r {
                let (dx, dz) = ((x - px) as f32, (z - pz) as f32);
                let len = (dx * dx + dz * dz).sqrt().max(0.01);
                let (rx, rz) = (-dz / len * 0.5, dx / len * 0.5);
                let b = self.world.biome(x, z);
                if !b.can_rain_or_snow() {
                    continue;
                }
                let top = self.world.precipitation_height(x, z);
                let lo = (py - r).max(top);
                let hi = (py + r).max(top);
                if lo == hi {
                    continue;
                }
                let temp = b.float_temperature(&self.noise, x, top, z);
                let snow = temp < 0.15;
                let tex = if snow { st } else { rt };
                let seed = (x
                    .wrapping_mul(x)
                    .wrapping_mul(3121)
                    .wrapping_add(x.wrapping_mul(45238971))
                    ^ z.wrapping_mul(z)
                        .wrapping_mul(418711)
                        .wrapping_add(z.wrapping_mul(13761))) as i64;
                let mut jr = crate::java::JavaRandom::new(seed);
                let scroll = if snow {
                    (self.ticks as f32 + partial) / 512.0
                } else {
                    (((self.ticks + (seed & 31) as u64) & 31) as f32 + partial) / 32.0
                        * (3.0 + jr.next_float())
                };
                let du = if snow {
                    jr.next_double() as f32 + t * 0.01 * jr.next_gaussian() as f32
                } else {
                    0.0
                };
                let d2 = dx * dx + dz * dz;
                let a = ((1.0 - d2 / (r * r) as f32) * 0.5 + 0.5) * rain;
                if a <= 0.0 {
                    continue;
                }
                let lx = math::floor_double(x as f64);
                let ly = top.max(py);
                let (sky, blk) = (
                    self.world.light(true, lx, ly, z),
                    self.world.light(false, lx, ly, z),
                );
                let light = [(blk << 4) as u8, (sky << 4) as u8, 0, 0];
                let col = [255, 255, 255, (a * 255.0) as u8];
                let (cx, cz) = (
                    (x as f64 - eye[0]) as f32 + 0.5,
                    (z as f64 - eye[2]) as f32 + 0.5,
                );
                let u0 = tex.rect[0] + (tex.rect[2] - tex.rect[0]) * (du.fract().abs() * 0.0);
                let u1 = tex.rect[0] + (tex.rect[2] - tex.rect[0]) * 64.0 / tex.size;
                let vspan = (tex.rect[3] - tex.rect[1]) * 256.0 / tex.size;
                for y in lo..hi {
                    let mut v0 = (y as f32 * 0.25 + scroll).rem_euclid(1.0);
                    let mut yy0 = y as f32;
                    let yy1 = y as f32 + 1.0;
                    while yy0 < yy1 {
                        let room = (1.0 - v0) * 4.0;
                        let seg = room.min(yy1 - yy0);
                        let v1 = v0 + seg * 0.25;
                        let (ya, yb) = (
                            (yy0 as f64 - eye[1]) as f32,
                            ((yy0 + seg) as f64 - eye[1]) as f32,
                        );
                        let (va, vb) = (tex.rect[1] + vspan * v0, tex.rect[1] + vspan * v1);
                        let q = [
                            crate::render::mesher::Vertex {
                                pos: [cx - rx, ya, cz - rz],
                                uv: [u0, vb],
                                color: col,
                                light,
                            },
                            crate::render::mesher::Vertex {
                                pos: [cx + rx, ya, cz + rz],
                                uv: [u1, vb],
                                color: col,
                                light,
                            },
                            crate::render::mesher::Vertex {
                                pos: [cx + rx, yb, cz + rz],
                                uv: [u1, va],
                                color: col,
                                light,
                            },
                            crate::render::mesher::Vertex {
                                pos: [cx - rx, yb, cz - rz],
                                uv: [u0, va],
                                color: col,
                                light,
                            },
                        ];
                        out.extend_from_slice(&q);
                        out.extend_from_slice(&[q[3], q[2], q[1], q[0]]);
                        yy0 += seg;
                        v0 = 0.0;
                    }
                }
            }
        }
        out
    }

    /// `RenderGlobal.doSpawnParticle` for the names the world uses.
    pub(super) fn named_particle(&mut self, kind: &str, p: [f64; 3], m: [f64; 3]) {
        // RenderGlobal.doSpawnParticle: these two are made whatever the
        // setting and distance
        let always = matches!(kind, "hugeexplosion" | "largeexplode");
        let was = self.particles.gated;
        self.particles.gated = !always;
        self.named_particle_inner(kind, p, m);
        self.particles.gated = was;
    }

    fn named_particle_inner(&mut self, kind: &str, p: [f64; 3], m: [f64; 3]) {
        let slime = self
            .atlas
            .items
            .get(&(341, 0))
            .map(|&t| self.atlas.rects[t as usize]);
        match kind {
            "portal" => self.particles.portal(p, m),
            "heart" => self.particles.heart(p, m),
            "bubble" => self.particles.bubble(p, m),
            "mobSpell" => self.particles.spell(
                p,
                [0.0; 3],
                128,
                [m[0] as f32, m[1] as f32, m[2] as f32],
                1.0,
            ),
            "mobSpellAmbient" => self.particles.spell(
                p,
                [0.0; 3],
                128,
                [m[0] as f32, m[1] as f32, m[2] as f32],
                0.15,
            ),
            "snowballpoof" => {
                if let Some(&t) = self.atlas.items.get(&(332, 0)) {
                    let rect = self.atlas.rects[t as usize];
                    self.particles.item_break(p, rect);
                }
            }
            "snowshovel" => self.particles.snow_shovel(p, m),
            "largeexplode" => self.particles.large_explode(p, 1.0),
            "hugeexplosion" => self.particles.huge_explosion(p),
            "explode" => self.particles.explode(p, m),
            "splash" => self.particles.splash(p, m),
            "wake" => self.particles.wake(p, m),
            "reddust" => self
                .particles
                .reddust(p, [m[0] as f32, m[1] as f32, m[2] as f32]),
            "smoke" => self.particles.smoke(p, m, false),
            "slime" => {
                if let Some(r) = slime {
                    self.particles.item_break(p, r);
                }
            }
            _ => {}
        }
    }

    pub(super) fn sound(&mut self, name: &str, pos: Option<[f64; 3]>, vol: f32, pitch: f32) {
        if let Some(a) = self.audio.as_mut() {
            a.play(name, pos, vol, pitch);
        }
    }

    /// `Block.stepSound` at a block: `dig` is the break sound
    /// (`RenderGlobal.playAuxSFX` 2001), `hit` the mining tick
    /// (`PlayerControllerMP.onPlayerDamageBlock`), `place` `ItemBlock`'s,
    /// `step` `Entity.playStepSound`'s.
    fn block_sound(&mut self, id: u8, pos: [i32; 3], kind: &str) {
        let t = crate::block::step_sound(id);
        let (name, vol, pitch) = match kind {
            "dig" => (t.dig, (t.volume + 1.0) / 2.0, t.pitch * 0.8),
            "hit" => (t.step, (t.volume + 1.0) / 8.0, t.pitch * 0.5),
            "place" => (t.place, (t.volume + 1.0) / 2.0, t.pitch * 0.8),
            _ => (t.step, t.volume * 0.15, t.pitch),
        };
        let c = [
            pos[0] as f64 + 0.5,
            pos[1] as f64 + 0.5,
            pos[2] as f64 + 0.5,
        ];
        self.sound(name, Some(c), vol, pitch);
    }

    /// `PlayerControllerMP.attackEntity`: the attack goes to the server
    /// (`C02PacketUseEntity`), and the client runs its own
    /// `attackTargetEntityWithCurrentItem`. Against a mob that does nothing
    /// (`EntityLivingBase.attackEntityFrom` is false on a client world);
    /// against a boat or minecart, whose client-side `attackEntityFrom` is
    /// true, a sprint hit pushes it and slows the player.
    fn attack_mob(&mut self, id: u32) {
        let vehicle = id & crate::entity::vehicle::VEHICLE_ID != 0;
        // EntityLivingBase.isOnLadder at the feet
        let ladder = {
            let b = self.world.peek_block(
                math::floor_double(self.player.pos[0]),
                math::floor_double(self.player.bb.min[1]),
                math::floor_double(self.player.pos[2]),
            );
            b == crate::block::LADDER || b == crate::block::VINE
        };
        let crit = self.stats.fall_distance > 0.0
            && !self.player.on_ground
            && !ladder
            && !self.player.in_water
            && self.riding.is_none();
        let sprint = self.player.sprinting;
        if vehicle && sprint {
            let r = std::f32::consts::PI;
            let yaw = self.player.yaw;
            if let Some(c) = self.boats.iter_mut().find(|c| c.v.id() == id) {
                let e = c.v.e_mut();
                e.motion[0] += (-math::sin(yaw * r / 180.0) * 0.5) as f64;
                e.motion[1] += 0.1;
                e.motion[2] += (math::cos(yaw * r / 180.0) * 0.5) as f64;
            }
            self.player.motion[0] *= 0.6;
            self.player.motion[2] *= 0.6;
            self.player.sprinting = false;
        }
        let _ = self.sim.commands.send(Command::Attack { id, crit, sprint });
    }

    /// `EntityCrit2FX.onUpdate` for each live emitter: up to 16 particles
    /// inside the mob's box, three ticks in a row.
    fn tick_crit_emitters(&mut self) {
        let mut list = std::mem::take(&mut self.crit_emitters);
        for e in list.iter_mut() {
            self.crit_emitter_update(e.0, e.1);
            e.2 += 1;
        }
        list.retain(|e| e.2 < 3);
        self.crit_emitters = list;
    }

    fn crit_emitter_update(&mut self, id: u32, magic: bool) {
        let Some(m) = self.mobs.iter().find(|m| m.id == id) else {
            return;
        };
        let (pos, bb) = (m.pos, m.bb);
        let w = (bb[3] - bb[0]) as f32;
        let h = (bb[4] - bb[1]) as f32;
        for _ in 0..16 {
            let r = &mut self.particle_rand;
            let x = (r.next_float() * 2.0 - 1.0) as f64;
            let y = (r.next_float() * 2.0 - 1.0) as f64;
            let z = (r.next_float() * 2.0 - 1.0) as f64;
            if x * x + y * y + z * z > 1.0 {
                continue;
            }
            let p = [
                pos[0] + x * w as f64 / 4.0,
                bb[1] + (h / 2.0) as f64 + y * h as f64 / 4.0,
                pos[2] + z * w as f64 / 4.0,
            ];
            let mv = [x, y + 0.2, z];
            if magic {
                self.particles.spawned(|ps| ps.magic_crit(p, mv));
            } else {
                self.particles.spawned(|ps| ps.crit(p, mv));
            }
        }
    }

    /// Block under the crosshair within survival reach (4.5 blocks).
    /// `EntityLivingBase.rayTrace(reach, partial)`: the block under the
    /// crosshair by `World.rayTraceBlocks`, which tests each block's own
    /// bounds (`collisionRayTrace`), and where the ray met it.
    fn pick(&self, eye: [f64; 3]) -> Option<([i32; 3], [i32; 3], [f64; 3])> {
        let d = self.look();
        // PlayerControllerMP.getBlockReachDistance
        let reach = if self.mode == GameMode::Creative {
            5.0f32
        } else {
            4.5f32
        } as f64;
        let end = [
            eye[0] + d[0] * reach,
            eye[1] + d[1] * reach,
            eye[2] + d[2] * reach,
        ];
        let h =
            crate::entity::raytrace::ray_trace_blocks(&self.world, eye, end, false, false, false)?;
        let n = match h.side {
            0 => [0, -1, 0],
            1 => [0, 1, 0],
            2 => [0, 0, -1],
            3 => [0, 0, 1],
            4 => [-1, 0, 0],
            _ => [1, 0, 0],
        };
        Some(([h.x, h.y, h.z], n, h.vec))
    }

    fn gui_mouse(&self) -> (f32, f32) {
        self.gui_viewport.pointer(self.mouse_px)
    }

    fn gui_size(&self) -> (f32, f32) {
        self.gui_viewport.size
    }

    /// The held stack as drawn: `EntityPlayer.getItemIcon` shows the cast
    /// rod while the hook is out.
    fn equipped_icon(&self) -> Option<crate::item::Stack> {
        let mut s = self.equipped;
        if let Some(st) = s.as_mut() {
            if st.id == 346 && self.fish_hook.is_some() {
                st.meta = 1000;
            }
        }
        s
    }

    /// `RenderFish`: the bobber, and the line from the rod tip to it.
    fn draw_fish_hook(
        &self,
        out: &mut Vec<crate::render::mesher::Vertex>,
        line: &mut Vec<[f32; 3]>,
        eye: [f64; 3],
        partial: f32,
    ) {
        let Some((pos, prev)) = self.fish_hook else {
            return;
        };
        let t = partial as f64;
        let h = [
            prev[0] + (pos[0] - prev[0]) * t,
            prev[1] + (pos[1] - prev[1]) * t,
            prev[2] + (pos[2] - prev[2]) * t,
        ];
        // Entity.getBrightnessForRender: 0.66 of the 0.25 box up
        let (lx, ly, lz) = (
            math::floor_double(pos[0]),
            math::floor_double(pos[1] + 0.25f32 as f64 * 0.66),
            math::floor_double(pos[2]),
        );
        let light = (
            self.world.light(true, lx, ly, lz),
            self.world.light(false, lx, ly, lz),
        );
        let rel = [
            (h[0] - eye[0]) as f32,
            (h[1] - eye[1]) as f32,
            (h[2] - eye[2]) as f32,
        ];
        if let Some(s) = self.atlas.mob_tex("../particle/particles") {
            let cell = (s.rect[2] - s.rect[0]) * 8.0 / s.size;
            let cellv = (s.rect[3] - s.rect[1]) * 8.0 / s.size;
            let u0 = s.rect[0] + cell;
            let v0 = s.rect[1] + 2.0 * cellv;
            entities::fish_bobber(
                out,
                [u0, v0, u0 + cell, v0 + cellv],
                rel,
                self.player.yaw,
                self.player.pitch,
                light,
            );
        }
        // the angler is the local player: posY is its eye height, and
        // RenderFish adds no getEyeHeight for it
        let swing = self.swing_progress(partial);
        let var21 = math::sin(math::sqrt_float(swing) * math::PI);
        let lerp = |a: [f32; 2]| a[0] + (a[1] - a[0]) * partial;
        let pitch = lerp(self.p_pitch);
        let yaw = lerp(self.p_yaw);
        // Vec3.rotateAroundX / rotateAroundY
        let rot_x = |v: [f64; 3], a: f32| {
            let (c, s) = (math::cos(a) as f64, math::sin(a) as f64);
            [v[0], v[1] * c + v[2] * s, v[2] * c - v[1] * s]
        };
        let rot_y = |v: [f64; 3], a: f32| {
            let (c, s) = (math::cos(a) as f64, math::sin(a) as f64);
            [v[0] * c + v[2] * s, v[1], v[2] * c - v[0] * s]
        };
        let mut v = [-0.5, 0.03, 0.8];
        v = rot_x(v, -pitch * math::PI / 180.0);
        v = rot_y(v, -yaw * math::PI / 180.0);
        v = rot_y(v, var21 * 0.5);
        v = rot_x(v, -var21 * 0.7);
        let e = self.eye(partial);
        let mut start = [e[0] + v[0], e[1] + v[1], e[2] + v[2]];
        if self.third_person > 0 {
            let var31 = lerp(self.p_body) * math::PI / 180.0;
            let (s, c) = (math::sin(var31) as f64, math::cos(var31) as f64);
            start = [
                e[0] - c * 0.35 - s * 0.85,
                e[1] - 0.45,
                e[2] - s * 0.35 + c * 0.85,
            ];
        }
        let d = [
            (start[0] - h[0]) as f32,
            (start[1] - (h[1] + 0.25)) as f32,
            (start[2] - h[2]) as f32,
        ];
        let n = 16;
        let point = |i: i32| {
            let f = i as f32 / n as f32;
            [
                rel[0] + d[0] * f,
                rel[1] + d[1] * (f * f + f) * 0.5 + 0.25,
                rel[2] + d[2] * f,
            ]
        };
        for i in 0..n {
            line.push(point(i));
            line.push(point(i + 1));
        }
    }

    /// `Render.renderShadow` (fancy graphics only): the shadow texture on
    /// the tops of the full blocks under an entity at `p` (its interpolated
    /// position), fading with height and with distance from the camera.
    /// `sh` is `getShadowSize`.
    pub(super) fn draw_shadow(
        &self,
        out: &mut Vec<crate::render::mesher::Vertex>,
        p: [f64; 3],
        sh: f64,
        size: f32,
        opaque: f32,
        eye: [f64; 3],
    ) {
        if size <= 0.0 || !self.settings().fancy_graphics {
            return;
        }
        let Some(tex) = self.atlas.mob_tex("../misc/shadow") else {
            return;
        };
        // doRenderShadowAndFire: getDistanceToCamera
        let d2 = (p[0] - eye[0]).powi(2) + (p[1] - eye[1]).powi(2) + (p[2] - eye[2]).powi(2);
        let opacity = ((1.0 - d2 / 256.0) * opaque as f64) as f32;
        if opacity <= 0.0 {
            return;
        }
        let w = &self.world;
        let sub = client_skylight_subtracted();
        // World.getBlockLightValue (the client's skylightSubtracted)
        let light = |x: i32, y: i32, z: i32| {
            (w.light(true, x, y, z) - sub)
                .max(w.light(false, x, y, z))
                .clamp(0, 15)
        };
        let s = size as f64;
        let (px, py, pz) = (p[0], p[1] + sh, p[2]);
        let (x0, x1) = (math::floor_double(px - s), math::floor_double(px + s));
        let (y0, y1) = (math::floor_double(py - s), math::floor_double(py));
        let (z0, z1) = (math::floor_double(pz - s), math::floor_double(pz + s));
        let r = tex.rect;
        let k = 64.0 / tex.size; // shadow.png is 64 x 64
        let uv = |u: f32, v: f32| [r[0] + (r[2] - r[0]) * u * k, r[1] + (r[3] - r[1]) * v * k];
        for x in x0..=x1 {
            for y in y0..=y1 {
                for z in z0..=z1 {
                    let id = w.peek_block(x, y - 1, z);
                    if block::material(id) == block::Material::Air || light(x, y, z) <= 3 {
                        continue;
                    }
                    // renderShadowOnBlock: renderAsNormalBlock only
                    if !block::info(id).normal {
                        continue;
                    }
                    let a = (opacity as f64 - (py - y as f64) / 2.0)
                        * 0.5
                        * self.light_table[light(x, y, z) as usize] as f64;
                    if a < 0.0 {
                        continue;
                    }
                    let a = a.min(1.0);
                    // the texture is clamped (its edges are clear): keep the
                    // quad inside the shadow's own square so the UVs never
                    // reach a neighbouring atlas tile
                    let (bx0, bx1) = ((x as f64).max(px - s), (x as f64 + 1.0).min(px + s));
                    let (bz0, bz1) = ((z as f64).max(pz - s), (z as f64 + 1.0).min(pz + s));
                    if bx0 >= bx1 || bz0 >= bz1 {
                        continue;
                    }
                    let by = y as f64 + 0.015625;
                    let u0 = ((px - bx0) / 2.0 / s + 0.5) as f32;
                    let u1 = ((px - bx1) / 2.0 / s + 0.5) as f32;
                    let v0 = ((pz - bz0) / 2.0 / s + 0.5) as f32;
                    let v1 = ((pz - bz1) / 2.0 / s + 0.5) as f32;
                    let c = [255, 255, 255, (a * 255.0) as u8];
                    let vtx = |x: f64, z: f64, t: [f32; 2]| crate::render::mesher::Vertex {
                        pos: [
                            (x - eye[0]) as f32,
                            (by - eye[1]) as f32,
                            (z - eye[2]) as f32,
                        ],
                        uv: t,
                        color: c,
                        light: [240, 240, 0, 0],
                    };
                    let q = [
                        vtx(bx0, bz0, uv(u0, v0)),
                        vtx(bx0, bz1, uv(u0, v1)),
                        vtx(bx1, bz1, uv(u1, v1)),
                        vtx(bx1, bz0, uv(u1, v0)),
                    ];
                    out.extend_from_slice(&q);
                    out.extend_from_slice(&[q[3], q[2], q[1], q[0]]);
                }
            }
        }
    }

    /// `EntityLivingBase.swingItem`: a swing starts unless one is in its
    /// first half.
    fn swing_item(&mut self) {
        if !self.swinging || self.swing >= self.arm_swing_end() / 2 || self.swing < 0 {
            self.swing = -1;
            self.swinging = true;
        }
    }

    /// `EntityLivingBase.getArmSwingAnimationEnd`: 6 ticks, shorter with
    /// Haste, longer with Mining Fatigue.
    fn arm_swing_end(&self) -> i32 {
        let amp = |p: i32| {
            self.stats
                .effects
                .iter()
                .find(|e| e.id == p)
                .map(|e| e.amplifier)
        };
        if let Some(a) = amp(crate::potion::DIG_SPEED) {
            6 - (1 + a)
        } else if let Some(a) = amp(crate::potion::DIG_SLOWDOWN) {
            6 + (1 + a) * 2
        } else {
            6
        }
    }

    /// `EntityLivingBase.getSwingProgress`: a restarted swing keeps going
    /// forward instead of jumping back.
    fn swing_progress(&self, partial: f32) -> f32 {
        let mut d = self.swing_f - self.prev_swing_f;
        if d < 0.0 {
            d += 1.0;
        }
        self.prev_swing_f + d * partial
    }

    /// `PlayerControllerMP.clickBlock` / `onPlayerDamageBlock`, once per tick
    /// while the attack button is held.
    fn tick_digging(&mut self) {
        if self.screen != Screen::Playing || !self.left_down || self.stats.dead {
            self.dig = None;
            return;
        }
        let Some((pos, _)) = self.selected else {
            self.dig = None;
            return;
        };
        self.swing_item();
        if self.hit_delay > 0 {
            self.hit_delay -= 1;
            return;
        }
        let id = self.world.peek_block(pos[0], pos[1], pos[2]);
        if self.mode == GameMode::Creative {
            self.hit_delay = 5;
            self.block_sound(id, pos, "dig");
            let (dt, dti) = self.debris(pos);
            self.particles.destroy(pos, dt, dti);
            self.send_dig(pos);
            return;
        }
        if self.ticks % 4 == 0 {
            self.block_sound(id, pos, "hit");
        }
        if let Some((_, side)) = self.selected {
            let (dt, dti) = self.debris(pos);
            self.particles.hit(pos, side, dt, dti);
        }
        let in_water = self.player.eye_material(&self.world, self.eye(0.0)) == Material::Water;
        let amp = |p: i32| {
            self.stats
                .effects
                .iter()
                .find(|e| e.id == p)
                .map(|e| e.amplifier)
        };
        let digger = crate::item::Digger {
            in_water,
            on_ground: self.player.on_ground,
            haste: amp(crate::potion::DIG_SPEED),
            fatigue: amp(crate::potion::DIG_SLOWDOWN),
            aqua_affinity: crate::enchant::max_level_in(
                crate::enchant::AQUA_AFFINITY,
                &self.view.inv.armor,
            ) > 0,
        };
        let step = crate::item::break_progress_by(self.view.inv.held(), id, &digger);
        match self.dig {
            Some(d) if d.pos == pos => {
                let damage = d.damage + step;
                if damage >= 1.0 {
                    self.block_sound(id, pos, "dig");
                    let (dt, dti) = self.debris(pos);
                    self.particles.destroy(pos, dt, dti);
                    self.send_dig(pos);
                    self.dig = None;
                    self.hit_delay = 5;
                } else {
                    self.dig = Some(Digging { pos, damage });
                }
            }
            _ => {
                // PlayerControllerMP.clickBlock: C07 START_DIGGING, which the
                // server answers with onBlockClicked (cake, note block, fire)
                let side = self.selected.map_or(1, |(_, f)| match f {
                    [0, -1, 0] => 0,
                    [0, 1, 0] => 1,
                    [0, 0, -1] => 2,
                    [0, 0, 1] => 3,
                    [-1, 0, 0] => 4,
                    _ => 5,
                });
                let _ = self.sim.commands.send(Command::ClickBlock(pos, side));
                if step >= 1.0 {
                    self.block_sound(id, pos, "dig");
                    let (dt, dti) = self.debris(pos);
                    self.particles.destroy(pos, dt, dti);
                    self.send_dig(pos);
                    self.dig = None;
                } else {
                    self.dig = Some(Digging { pos, damage: 0.0 });
                }
            }
        }
    }

    /// `Minecraft.middleClickMouse` on an entity: what item it is picked as.
    fn pick_entity(&self, id: u32) -> Option<(crate::item::ItemId, i32, bool)> {
        use crate::entity::minecart::CartKind;
        use crate::entity::vehicle::Vehicle;
        if let Some(m) = self.mobs.iter().find(|m| m.id == id) {
            // a spawn egg of its EntityList id, if it has one
            let name = m.kind.id();
            return crate::item_data::EGGS
                .iter()
                .find(|e| e.1 == name)
                .map(|e| (383, e.0, true));
        }
        let c = self.boats.iter().find(|c| c.v.id() == id)?;
        Some(match &c.v {
            Vehicle::Boat(_) => (333, 0, false),
            Vehicle::Cart(m) => (
                match m.kind {
                    CartKind::Furnace => 343,
                    CartKind::Chest => 342,
                    CartKind::Tnt => 407,
                    CartKind::Hopper => 408,
                    CartKind::Command => 422,
                    _ => 328,
                },
                0,
                false,
            ),
            Vehicle::Hang(h) if h.knot => (420, 0, false),
            Vehicle::Hang(h) if h.art.is_some() => (321, 0, false),
            // an item frame: what it shows, else the frame
            Vehicle::Hang(h) => match h.item {
                Some(s) => (s.id, s.meta, true),
                None => (389, 0, false),
            },
            _ => return None,
        })
    }

    /// The render chunks around a block the client changed
    /// (`markBlockForUpdate`).
    fn mark_block_dirty(&mut self, p: [i32; 3]) {
        let (s0, s1) = ((p[1] - 1).clamp(0, 255) >> 4, (p[1] + 1).clamp(0, 255) >> 4);
        let bits = (((1u32 << (s1 - s0 + 1)) - 1) << s0) as u16;
        for cx in ((p[0] - 1) >> 4)..=((p[0] + 1) >> 4) {
            for cz in ((p[2] - 1) >> 4)..=((p[2] + 1) >> 4) {
                *self.dirty.entry((cx, cz)).or_insert(0) |= bits;
            }
        }
    }

    /// Changes a block on the client ahead of the server. Returns false
    /// (and does nothing) where the client has no data to change.
    fn predict(&mut self, pos: [i32; 3], id: u8, meta: u8) -> bool {
        if !(0..256).contains(&pos[1]) {
            return false;
        }
        let i = ClientWorld::index(pos[0], pos[1], pos[2]);
        // light for the faces a removed block uncovers: what would flow in
        // from the six neighbours (the server's light follows in its tick)
        let (mut sky, mut blk) = (0u8, 0u8);
        if id == block::AIR {
            for (k, n) in [
                [0, 1, 0],
                [0, -1, 0],
                [1, 0, 0],
                [-1, 0, 0],
                [0, 0, 1],
                [0, 0, -1],
            ]
            .iter()
            .enumerate()
            {
                let (x, y, z) = (pos[0] + n[0], pos[1] + n[1], pos[2] + n[2]);
                let s = self.world.light(true, x, y, z) as u8;
                let b = self.world.light(false, x, y, z) as u8;
                // full sky light falls straight down undimmed
                sky = sky.max(if k == 0 && s == 15 {
                    15
                } else {
                    s.saturating_sub(1)
                });
                blk = blk.max(b.saturating_sub(1));
            }
        }
        let Some(c) = self.world.chunks.get_mut(&(pos[0] >> 4, pos[2] >> 4)) else {
            return false;
        };
        if c.mc.sections & (1 << (pos[1] >> 4)) == 0 {
            return false;
        }
        let d = Arc::make_mut(&mut c.mc.data);
        let old = [d.blocks[i], d.meta[i], d.sky[i], d.blk[i]];
        let new = [id, meta, sky, blk];
        d.blocks[i] = id;
        d.meta[i] = meta;
        d.sky[i] = sky;
        d.blk[i] = blk;
        // an earlier prediction at the same place keeps the server's value
        let old = match self.predictions.iter().position(|p| p.pos == pos) {
            Some(k) => self.predictions.remove(k).old,
            None => old,
        };
        if std::env::var_os("MINERUST_DEBUG_LATENCY").is_some() {
            eprintln!(
                "predict: {pos:?} = {id}:{meta} (action {})",
                self.sent_actions
            );
        }
        self.predictions.push(Prediction {
            seq: self.sent_actions,
            pos,
            old,
            new,
        });
        self.mark_block_dirty(pos);
        true
    }

    /// After a whole server tick has been applied: a predicted block whose
    /// action the server has handled without sending the chunk again was
    /// refused, so what was there before comes back.
    fn settle_predictions(&mut self) {
        let done = self.batch_done;
        let mut k = 0;
        while k < self.predictions.len() {
            if self.predictions[k].seq.wrapping_sub(done) as i32 > 0 {
                k += 1;
                continue;
            }
            let p = self.predictions.remove(k);
            if std::env::var_os("MINERUST_DEBUG_LATENCY").is_some() {
                eprintln!(
                    "predict: {:?} refused, back to {}:{}",
                    p.pos, p.old[0], p.old[1]
                );
            }
            let i = ClientWorld::index(p.pos[0], p.pos[1], p.pos[2]);
            if let Some(c) = self.world.chunks.get_mut(&(p.pos[0] >> 4, p.pos[2] >> 4)) {
                let d = Arc::make_mut(&mut c.mc.data);
                d.blocks[i] = p.old[0];
                d.meta[i] = p.old[1];
                d.sky[i] = p.old[2];
                d.blk[i] = p.old[3];
                self.mark_block_dirty(p.pos);
            }
        }
    }

    /// `PlayerControllerMP.onPlayerDestroyBlock`: the block goes on the
    /// client at once and the server is told.
    fn send_dig(&mut self, pos: [i32; 3]) {
        self.sent_actions = self.sent_actions.wrapping_add(1);
        let id = self.world.peek_block(pos[0], pos[1], pos[2]);
        // what the server's `dig` refuses is left alone
        let sword = self.mode == GameMode::Creative
            && self.view.inv.held().is_some_and(|s| {
                crate::item::tool(s.id).is_some_and(|t| t.kind == crate::item::ToolKind::Sword)
            });
        if id != block::AIR
            && (self.mode == GameMode::Creative || crate::item::hardness(id) >= 0.0)
            && !block::material(id).is_liquid()
            && !sword
        {
            self.predict(pos, block::AIR, 0);
        }
        let _ = self.sim.commands.send(Command::Dig(pos));
    }

    /// `ItemBlock.onItemUse` as the client runs it, up to the point where
    /// the block would be set: where a held block would go, or `None` when
    /// `World.canPlaceEntityOnSide` refuses (the place is not replaceable,
    /// or the block would stand inside the player or a mob). In that case
    /// the original neither plays the sound nor tells the server.
    fn placement(&self, id: u8, meta: u8, b: [i32; 3], n: [i32; 3]) -> Option<[i32; 3]> {
        let tid = self.world.peek_block(b[0], b[1], b[2]);
        // ItemSlab.func_150936_a: a slab may join the half it is put on,
        // or the half in the block beside it
        if id == block::STONE_SLAB || id == block::WOODEN_SLAB {
            let tm = self.world.peek_meta(b[0], b[1], b[2]);
            let top = tm & 8 != 0;
            if (n == [0, 1, 0] && !top || n == [0, -1, 0] && top) && tid == id && tm & 7 == meta & 7
            {
                return Some(b);
            }
            let q = [b[0] + n[0], b[1] + n[1], b[2] + n[2]];
            if self.world.peek_block(q[0], q[1], q[2]) == id
                && self.world.peek_meta(q[0], q[1], q[2]) & 7 == meta & 7
            {
                return Some(q);
            }
        }
        let in_place = tid == block::SNOW_LAYER && self.world.peek_meta(b[0], b[1], b[2]) & 7 < 1
            || matches!(tid, block::VINE | block::TALLGRASS | block::DEADBUSH);
        let p = if in_place {
            b
        } else {
            [b[0] + n[0], b[1] + n[1], b[2] + n[2]]
        };
        if !(0..256).contains(&p[1]) {
            return None;
        }
        if p[1] == 255 && block::material(id).is_solid() {
            return None;
        }
        if !block::material(self.world.peek_block(p[0], p[1], p[2])).replaceable() {
            return None;
        }
        // World.checkNoEntityCollision: entities that prevent spawning
        if let Some(c) = block::collision_box(id, meta) {
            let bb = crate::entity::player::Aabb {
                min: [
                    p[0] as f64 + c[0] as f64,
                    p[1] as f64 + c[1] as f64,
                    p[2] as f64 + c[2] as f64,
                ],
                max: [
                    p[0] as f64 + c[3] as f64,
                    p[1] as f64 + c[4] as f64,
                    p[2] as f64 + c[5] as f64,
                ],
            };
            if !self.stats.dead && self.player.bb.intersects(&bb) {
                return None;
            }
            for m in &self.mobs {
                if m.death_time > 0 {
                    continue;
                }
                let mb = crate::entity::player::Aabb {
                    min: [m.bb[0], m.bb[1], m.bb[2]],
                    max: [m.bb[3], m.bb[4], m.bb[5]],
                };
                if mb.intersects(&bb) {
                    return None;
                }
            }
        }
        Some(p)
    }

    /// `Minecraft.func_147121_ag` on a block: `onPlayerRightClick`.
    /// `sendUseItem` without a block in view.
    fn use_item_air(&mut self) {
        let _ = self.sim.commands.send(Command::UseAir);
        self.use_item_air_effects();
    }

    /// What `sendUseItem` shows at once on the client.
    fn use_item_air_effects(&mut self) {
        let Some(s) = self.view.inv.held() else {
            return;
        };
        // ItemWritableBook / ItemEditableBook.onItemRightClick:
        // displayGUIBook
        if s.id == 386 || s.id == 387 {
            self.book_gui = Some(crate::book::BookScreen::open(&s, crate::sim::PLAYER_NAME));
            self.screen = Screen::Book;
            self.input = Input::default();
        }
        if s.id == 346 {
            // ItemFishingRod.onItemRightClick: swingItem
            self.swing_item();
        }
        // sendUseItem is true when onItemRightClick changed the stack:
        // resetEquippedProgress2, the hand drops and comes back up
        let survival = self.mode != GameMode::Creative;
        let thrown = match s.id {
            // snowball, egg, ender pearl, bottle o' enchanting, eye of ender
            332 | 344 | 368 | 384 | 381 => survival,
            // a splash potion
            373 => s.meta & 16384 != 0 && survival,
            _ => false,
        };
        if thrown {
            self.equip = 0.0;
        }
    }

    /// `Item.onItemUse` on the client, as far as its return value and the
    /// stack it uses up go: (used, a stack used up).
    fn client_item_use(&self, s: Stack, b: [i32; 3], n: [i32; 3]) -> (bool, bool) {
        let w = &self.world;
        let tid = w.peek_block(b[0], b[1], b[2]);
        let tm = w.peek_meta(b[0], b[1], b[2]);
        let up = n == [0, 1, 0];
        let air_above = w.peek_block(b[0], b[1] + 1, b[2]) == block::AIR;
        let at = [b[0] + n[0], b[1] + n[1], b[2] + n[2]];
        let at_air = w.peek_block(at[0], at[1], at[2]) == block::AIR;
        match s.id {
            // ItemHoe: grass or dirt with air above, not from below
            290..=294 => (
                n != [0, -1, 0] && air_above && matches!(tid, block::GRASS | block::DIRT),
                false,
            ),
            // ItemFlintAndSteel: always
            259 => (true, false),
            // ItemDye: bone meal on what grows, cocoa beans on jungle wood
            351 if s.meta == 15 => {
                let grows = match tid {
                    block::SAPLING | block::GRASS | block::BROWN_MUSHROOM | block::RED_MUSHROOM => {
                        true
                    }
                    block::WHEAT | block::CARROTS | block::POTATOES => tm < 7,
                    block::PUMPKIN_STEM | block::MELON_STEM => tm < 7,
                    block::COCOA => (tm & 12) >> 2 < 2,
                    block::TALLGRASS => tm != 0,
                    block::DOUBLE_PLANT => true,
                    _ => false,
                };
                (grows, grows)
            }
            351 if s.meta == 3 => {
                let ok = tid == block::LOG && tm & 3 == 3 && !up && n != [0, -1, 0] && at_air;
                (ok, ok)
            }
            // ItemSeeds / ItemSeedFood: on the top of farmland (soul sand
            // for nether wart) with air above
            295 | 361 | 362 | 391 | 392 => {
                let ok = up && tid == block::FARMLAND && air_above;
                (ok, ok)
            }
            372 => {
                let ok = up && tid == block::SOUL_SAND && air_above;
                (ok, ok)
            }
            // items that place a block of their own
            323 | 324 | 330 | 355 | 331 | 287 | 354 | 356 | 404 | 379 | 380 | 390 | 397 | 338 => {
                (true, true)
            }
            // ItemHangingEntity: any side but the top and bottom
            321 | 389 => {
                let ok = n[1] == 0;
                (ok, ok)
            }
            // ItemMinecart on a rail
            328 | 342 | 343 | 407 | 408 => {
                let ok = crate::entity::minecart::is_rail(tid);
                (ok, ok)
            }
            // ItemMonsterPlacer, ItemFirework, ItemFireball: always
            383 | 401 | 385 => (true, true),
            // ItemEnderEye on an empty end portal frame
            381 => {
                let ok = tid == block::END_PORTAL_FRAME && tm & 4 == 0;
                (ok, ok)
            }
            // ItemRecord into an empty jukebox
            2256..=2267 => {
                let ok = tid == block::JUKEBOX && tm == 0;
                (ok, ok)
            }
            _ => (false, false),
        }
    }

    fn use_item(&mut self) {
        self.right_delay = 4;
        let Some((b, n)) = self.selected else {
            // ItemFishingRod.onItemRightClick on the client: swingItem
            self.use_item_air();
            return;
        };
        if n == [0, 0, 0] {
            return;
        }
        self.use_on(b, n);
    }

    /// `PlayerControllerMP.onPlayerRightClick` on block `b`, side `n`.
    fn use_on(&mut self, b: [i32; 3], n: [i32; 3]) {
        let held = self.view.inv.held();
        let tid = self.world.peek_block(b[0], b[1], b[2]);
        // Block.onBlockActivated comes first unless sneaking with an item
        let activates = (!self.input.sneak || held.is_none())
            && matches!(
                tid,
                block::CRAFTING_TABLE
                    | block::FURNACE
                    | block::LIT_FURNACE
                    | block::CHEST
                    | block::TRAPPED_CHEST
                    | block::ENDER_CHEST
                    | block::ANVIL
                    | block::ENCHANTING_TABLE
                    | block::DISPENSER
                    | block::DROPPER
                    | block::HOPPER
                    | block::BREWING_STAND
                    | block::LEVER
                    | block::STONE_BUTTON
                    | block::WOODEN_BUTTON
                    | block::UNPOWERED_REPEATER
                    | block::POWERED_REPEATER
                    | block::UNPOWERED_COMPARATOR
                    | block::POWERED_COMPARATOR
                    | block::NOTEBLOCK
                    | block::WOODEN_DOOR
                    | block::TRAPDOOR
            );
        let mut place = None;
        if !activates {
            if let Some(s) = held {
                if let Some(id) = s.block() {
                    // ItemBlock.func_150936_a: refused before any packet
                    let Some(p) = self.placement(id, s.meta as u8, b, n) else {
                        // onPlayerRightClick is false: no swing; creative
                        // still drops the hand, then sendUseItem
                        if self.mode == GameMode::Creative {
                            self.equip = 0.0;
                        }
                        let _ = self.sim.commands.send(Command::UseAir);
                        return;
                    };
                    self.block_sound(id, p, "place");
                    place = Some((p, id, s.meta as u8));
                }
            }
        }
        self.sent_actions = self.sent_actions.wrapping_add(1);
        if let Some((p, id, meta)) = place {
            // ItemBlock.placeBlockAt on the client. Only plain cubes whose
            // metadata is the item's: the ones that turn to face the
            // player or need support wait for the server.
            let plain = block::info(id).shape == block::Shape::Cube
                && !matches!(
                    id,
                    block::LOG
                        | block::LOG2
                        | block::HAY_BLOCK
                        | block::QUARTZ_BLOCK
                        | block::FURNACE
                        | block::LIT_FURNACE
                        | block::DISPENSER
                        | block::DROPPER
                        | block::HOPPER
                        | block::PISTON
                        | block::STICKY_PISTON
                        | block::PUMPKIN
                        | block::LIT_PUMPKIN
                        | block::SAND
                        | block::GRAVEL
                        | block::TNT
                );
            if plain {
                self.predict(p, id, meta & 15);
                // MINERUST_PREDICT_BOGUS: also predict a block the server
                // will not place, to check that it is taken back
                if std::env::var_os("MINERUST_PREDICT_BOGUS").is_some() {
                    self.predict([p[0] + 16, p[1], p[2]], id, meta & 15);
                    self.predict([p[0], p[1] + 2, p[2]], id, meta & 15);
                }
            }
        }
        let _ = self.sim.commands.send(Command::Use {
            target: b,
            face: n,
            sneaking: self.input.sneak,
            hit: [
                (self.hit[0] - b[0] as f64) as f32,
                (self.hit[1] - b[1] as f64) as f32,
                (self.hit[2] - b[2] as f64) as f32,
            ],
        });
        if std::env::var_os("MINERUST_DEBUG_LATENCY").is_some() {
            self.latency = Some((Instant::now(), (b[0] >> 4, b[2] >> 4)));
        }
        // Minecraft.rightClickMouse: swingItem only when onPlayerRightClick
        // did something; the hand drops when the stack changed size, and
        // always in creative
        let (used, consumed) = if activates {
            (true, false)
        } else if place.is_some() {
            (true, true)
        } else if let Some(s) = held {
            self.client_item_use(s, b, n)
        } else {
            (false, false)
        };
        if used {
            self.swing_item();
        }
        let creative = self.mode == GameMode::Creative;
        if held.is_some() && (creative || consumed) {
            self.equip = 0.0;
        }
        if !used {
            // sendUseItem with the held item (the server does it after the
            // Use packet)
            self.use_item_air_effects();
        }
    }

    /// The mouse in scaled GUI pixels (`Mouse.getX() * width / displayWidth`).
    fn gui_mouse_i(&self) -> (i32, i32) {
        let (x, y) = self.gui_mouse();
        (x.floor() as i32, y.floor() as i32)
    }

    fn container_keys(&self) -> crate::gui_container::Keys {
        let b = |d: &str| {
            self.shell.as_ref().map_or_else(
                || crate::settings::GameSettings::default().key(d),
                |s| s.settings.key(d),
            )
        };
        let mut hotbar = [0; 9];
        for (i, h) in hotbar.iter_mut().enumerate() {
            *h = b(&format!("key.hotbar.{}", i + 1));
        }
        crate::gui_container::Keys {
            inventory: b("key.inventory"),
            drop: b("key.drop"),
            pick: b("key.pickItem"),
            hotbar,
        }
    }

    /// `ContainerRepair.updateItemName` on the client's copy.
    fn reprice_anvil(&mut self) {
        let creative = self.mode == GameMode::Creative;
        let name = self
            .anvil
            .as_ref()
            .map_or(String::new(), |a| a.name.clone());
        let mut tile = std::mem::take(&mut self.view.tile_slots);
        {
            let ctx = crate::container::Ctx::new(
                &mut self.view.inv,
                &mut tile,
                crate::container::Window::Repair,
                creative,
            )
            .with_repair(&name, self.stats.xp_level);
            self.view.result = ctx.result();
            self.view.repair_cost = ctx.repair_cost();
        }
        self.view.tile_slots = tile;
    }

    /// `GuiRepair` as a crafter of the anvil's container: made with the
    /// window, told when slot 0 changes (`sendSlotContents`), which can
    /// send `MC|ItemName`.
    fn anvil_sync(&mut self) {
        if !matches!(self.view.open, Open::Anvil(_)) || self.screen != Screen::Window {
            self.anvil = None;
            return;
        }
        let (w, h) = self.gui_size();
        let (left, top) = crate::gui_container::origin(&self.view.open, w as i32, h as i32, false);
        let atlas = self.atlas.clone();
        let ui = crate::gui::Ui::new(&atlas, w, h);
        let slot0 = self.view.tile_slots.first().copied().flatten();
        let a = self
            .anvil
            .get_or_insert_with(|| crate::gui_repair::GuiRepair::new(left, top));
        a.place(left, top);
        if let Some(n) = a.sync(&ui, slot0, &self.lang) {
            let _ = self.sim.commands.send(Command::RenameItem(n));
        }
    }

    /// `PlayerControllerMP.windowClick`: `slotClick` on the client's copy,
    /// then `C0EPacketClickWindow`.
    fn window_click(&mut self, c: crate::gui_container::Click) -> InvView {
        let creative = self.mode == GameMode::Creative;
        let window = crate::play::window_of(&self.view.open);
        let mut tile = std::mem::take(&mut self.view.tile_slots);
        {
            let name = self
                .anvil
                .as_ref()
                .map_or(String::new(), |a| a.name.clone());
            let offers = self.view.offers.clone();
            let mut ctx =
                crate::container::Ctx::new(&mut self.view.inv, &mut tile, window, creative)
                    .with_repair(&name, self.stats.xp_level)
                    .with_merchant(&offers, self.view.merchant_index);
            crate::container::slot_click(&mut ctx, &mut self.client_drag, c.slot, c.button, c.mode);
            let r = ctx.result();
            self.view.result = r;
            self.view.repair_cost = ctx.repair_cost();
        }
        self.view.tile_slots = tile;
        self.sent_clicks = self.sent_clicks.wrapping_add(1);
        let _ = self.sim.commands.send(Command::WindowClick {
            slot: c.slot,
            button: c.button,
            mode: c.mode,
        });
        self.view.clone()
    }

    /// Runs a `GuiContainer` handler with the click sink.
    fn with_container<R>(
        &mut self,
        f: impl FnOnce(
            &mut crate::gui_container::GuiContainer,
            &crate::gui_container::Frame,
            &crate::gui_container::Keys,
            &mut dyn FnMut(crate::gui_container::Click) -> InvView,
        ) -> R,
    ) -> R {
        let (w, h) = self.gui_size();
        let view = self.view.clone();
        let frame = crate::gui_container::Frame {
            layout: None,
            view: &view,
            w: w as i32,
            h: h as i32,
            effects: !self.stats.effects.is_empty(),
            mouse: self.gui_mouse_i(),
            age: self.ticks as f32 + self.partial,
            ench: Some(self.enchant_state(self.partial)),
        };
        let keys = self.container_keys();
        let mut gui = std::mem::take(&mut self.gui);
        let r = f(&mut gui, &frame, &keys, &mut |c| self.window_click(c));
        self.gui = gui;
        r
    }

    /// `GuiScreen.handleMouseInput` move events: `mouseClickMove`.
    pub fn mouse_moved(&mut self) {
        if self.screen == Screen::Window && self.creative.is_some() {
            let (dims, mouse, _) = self.creative_input();
            if let Some(c) = self.creative.as_mut() {
                c.mouse_dragged(&self.view, dims, mouse);
            }
            return;
        }
        if self.screen == Screen::Window {
            self.with_container(|g, f, _, _| g.mouse_dragged(f));
        }
    }

    pub fn focus_lost(&mut self) {
        self.left_down = false;
        self.right_down = false;
        self.gui = Default::default();
        if let Some(creative) = &mut self.creative {
            creative.cancel_pointer();
        }
    }

    /// The client's `WorldClient` copies of the mobs: a new snapshot keeps
    /// the client-side motion state and starts a three-step interpolation
    /// whenever the tracker sent a move (`handleEntityMovement` ->
    /// `setPositionAndRotation2(.., 3)`).
    fn merge_mobs(&mut self, list: Vec<crate::sim::MobView>) {
        let mut old: std::collections::HashMap<u32, crate::sim::MobView> =
            self.mobs.drain(..).map(|m| (m.id, m)).collect();
        self.mob_net.clear();
        for mut v in list {
            let old_v = old.remove(&v.id);
            // The animation counters (swing progress, creeper fuse, chicken
            // wings, slime squish, squid, sheep eating, golem attack, age)
            // are updated on the client's own ticks in the original; here
            // they come from the server, so they are held and advanced one
            // client tick at a time, keeping `prev + (cur - prev) * partial`
            // on the client's clock.
            self.mob_net.insert(v.id, MobAnim::of(&v));
            if let Some(o) = &old_v {
                if v.kind != crate::entity::mob::Kind::EnderDragon {
                    MobAnim::keep(&mut v, o);
                }
            }
            if v.kind == crate::entity::mob::Kind::EnderDragon {
                // EntityDragon moves by its own interpolation: server frames
                self.mobs.push(v);
                continue;
            }
            match old_v {
                Some(o) => {
                    let shift = [
                        o.pos[0] - v.pos[0],
                        o.pos[1] - v.pos[1],
                        o.pos[2] - v.pos[2],
                    ];
                    for i in 0..3 {
                        v.bb[i] += shift[i];
                        v.bb[i + 3] += shift[i];
                    }
                    let seq = v.net_seq;
                    v.incs = if seq != o.net_seq { 3 } else { o.incs };
                    v.pos = o.pos;
                    v.prev = o.prev;
                    v.yaw = o.yaw;
                    v.body_yaw = o.body_yaw;
                    v.prev_body_yaw = o.prev_body_yaw;
                    v.head_yaw = o.head_yaw;
                    v.prev_head_yaw = o.prev_head_yaw;
                    v.body_head = o.body_head;
                    v.body_ticks = o.body_ticks;
                    v.pitch = o.pitch;
                    v.prev_pitch = o.prev_pitch;
                    v.limb_swing = o.limb_swing;
                    v.limb_amount = o.limb_amount;
                    v.prev_limb_amount = o.prev_limb_amount;
                    // EntityLivingBase.onDeathUpdate runs on the client too: once
                    // the mob is dying the client counts deathTime on its own
                    // ticks, so rotateCorpse's deathTime + partial stays on one
                    // clock (the server's count arrives out of phase)
                    if o.death_time > 0 {
                        v.death_time = o.death_time;
                    }
                }
                None => {
                    // handleSpawnMob: setPositionAndRotation at the packet's place
                    v.pos = v.net_at;
                    v.prev = v.net_at;
                    v.yaw = v.net_yaw;
                    v.body_yaw = v.net_yaw;
                    v.prev_body_yaw = v.net_yaw;
                    v.head_yaw = v.net_head;
                    v.prev_head_yaw = v.net_head;
                    v.body_head = v.net_head;
                    v.body_ticks = 0;
                    v.pitch = v.net_pitch;
                    v.prev_pitch = v.net_pitch;
                    v.limb_swing = 0.0;
                    v.limb_amount = 0.0;
                    v.prev_limb_amount = 0.0;
                    v.incs = 0;
                }
            }
            self.mobs.push(v);
        }
    }

    /// `EntityLivingBase.onUpdate` of the client's mobs: interpolation,
    /// `updateDistance` and the limb swing from the client's own movement.
    fn tick_mobs(&mut self) {
        let mut potion_fx: Vec<(&'static str, [f64; 3], [f64; 3])> = Vec::new();
        self.tick_mobs_inner(&mut potion_fx);
        for (n, p, c) in potion_fx {
            self.named_particle(n, p, c);
        }
    }

    fn tick_mobs_inner(&mut self, potion_fx: &mut Vec<(&'static str, [f64; 3], [f64; 3])>) {
        let wrap = |a: f32| {
            let mut a = a % 360.0;
            if a >= 180.0 {
                a -= 360.0;
            }
            if a < -180.0 {
                a += 360.0;
            }
            a
        };
        for m in &mut self.mobs {
            if m.kind == crate::entity::mob::Kind::EnderDragon {
                continue;
            }
            // onDeathUpdate: ++deathTime while dying (the body goes at 20)
            if m.death_time > 0 && m.death_time < 20 {
                m.death_time += 1;
            }
            if let Some(n) = self.mob_net.get(&m.id) {
                n.advance(m);
            }
            m.prev = m.pos;
            m.prev_body_yaw = m.body_yaw;
            m.prev_head_yaw = m.head_yaw;
            m.prev_pitch = m.pitch;
            m.prev_limb_amount = m.limb_amount;
            if m.incs > 0 {
                let n = m.incs as f64;
                let old = m.pos;
                for i in 0..3 {
                    m.pos[i] += (m.net_at[i] - m.pos[i]) / n;
                }
                m.yaw = (m.yaw as f64 + wrap(m.net_yaw - m.yaw) as f64 / n) as f32;
                m.pitch = (m.pitch as f64 + (m.net_pitch - m.pitch) as f64 / n) as f32;
                m.incs -= 1;
                for i in 0..3 {
                    m.bb[i] += m.pos[i] - old[i];
                    m.bb[i + 3] += m.pos[i] - old[i];
                }
            }
            m.head_yaw = m.net_head;
            // EntityLivingBase.updatePotionEffects on the client: the
            // particles from dataWatcher 7 and 8
            let (c, ambient) = m.potion;
            if c > 0 {
                let r = &mut self.particle_rand;
                let mut show = r.next_boolean();
                if ambient {
                    show &= r.next_int_n(5) == 0;
                }
                if show {
                    let (w, h) = ((m.bb[3] - m.bb[0]) as f64, (m.bb[4] - m.bb[1]) as f64);
                    let p = [
                        m.pos[0] + (r.next_double() - 0.5) * w,
                        m.pos[1] + r.next_double() * h,
                        m.pos[2] + (r.next_double() - 0.5) * w,
                    ];
                    let rgb = [
                        (c >> 16 & 255) as f64 / 255.0,
                        (c >> 8 & 255) as f64 / 255.0,
                        (c & 255) as f64 / 255.0,
                    ];
                    potion_fx.push((
                        if ambient {
                            "mobSpellAmbient"
                        } else {
                            "mobSpell"
                        },
                        p,
                        rgb,
                    ));
                }
            }
            // EntityLivingBase.moveEntityWithHeading: limbSwingAmount
            let (dx, dz) = (m.pos[0] - m.prev[0], m.pos[2] - m.prev[2]);
            let f = ((dx * dx + dz * dz).sqrt() as f32 * 4.0).min(1.0);
            m.limb_amount += (f - m.limb_amount) * 0.4;
            m.limb_swing += m.limb_amount;
            update_body(m, dx, dz);
            while m.body_yaw - m.prev_body_yaw < -180.0 {
                m.prev_body_yaw -= 360.0;
            }
            while m.body_yaw - m.prev_body_yaw >= 180.0 {
                m.prev_body_yaw += 360.0;
            }
            while m.head_yaw - m.prev_head_yaw < -180.0 {
                m.prev_head_yaw -= 360.0;
            }
            while m.head_yaw - m.prev_head_yaw >= 180.0 {
                m.prev_head_yaw += 360.0;
            }
        }
    }

    /// Window size, mouse and keys for the creative screen.
    fn creative_input(&self) -> ((i32, i32, bool), (i32, i32), crate::gui_container::Keys) {
        let (w, h) = self.gui_size();
        (
            (w as i32, h as i32, !self.stats.effects.is_empty()),
            self.gui_mouse_i(),
            self.container_keys(),
        )
    }

    fn open_creative(&mut self) {
        let (w, h) = self.gui_size();
        let atlas = self.atlas.clone();
        let ui = crate::gui::Ui::new(&atlas, w, h);
        self.creative = Some(crate::creative::Creative::new(
            self.creative_tab,
            &ui,
            &self.lang,
        ));
        self.gui = Default::default();
    }

    /// Sends the creative screen's `C10PacketCreativeInventoryAction`s.
    fn flush_creative(&mut self) {
        let Some(c) = self.creative.as_mut() else {
            return;
        };
        for o in std::mem::take(&mut c.out) {
            let (slot, stack) = match o {
                crate::creative::Out::Slot(k, s) => (k, s),
                crate::creative::Out::Drop(s) => (-1, Some(s)),
            };
            self.sent_clicks = self.sent_clicks.wrapping_add(1);
            let _ = self
                .sim
                .commands
                .send(Command::CreativeSlot { slot, stack });
        }
        self.creative_tab = c.tab;
    }

    /// Whether typed characters go to a screen (`GuiContainerCreative`'s
    /// search field).
    pub fn text_input(&self) -> bool {
        self.screen == Screen::Window && (self.creative.is_some() || self.anvil.is_some())
            || self.screen == Screen::Chat
            || self.screen == Screen::Sign
            || self.screen == Screen::Book
    }

    /// What `GuiScreenBook` asked for: closing sends the book when it
    /// changed.
    fn book_action(&mut self, a: crate::book::BookAction) -> Request {
        match a {
            crate::book::BookAction::None => Request::None,
            crate::book::BookAction::Close(data) => {
                if let Some((pages, title)) = data {
                    let _ = self.sim.commands.send(Command::BookEdit(pages, title));
                }
                self.book_gui = None;
                self.screen = Screen::Playing;
                Request::Grab
            }
        }
    }

    /// `GuiEditSign.onGuiClosed`: `C12PacketUpdateSign`.
    fn close_sign(&mut self) -> Request {
        if let Some((p, lines, _, _)) = self.sign_edit.take() {
            self.sign_texts.insert(p, lines.clone());
            let _ = self.sim.commands.send(Command::UpdateSign(p, lines));
        }
        self.screen = Screen::Playing;
        Request::Grab
    }

    /// `GuiEditSign.drawScreen`
    fn draw_sign_editor(&mut self, ui: &mut crate::gui::Ui, m: (f32, f32)) {
        let Some((p, lines, line, counter)) = self.sign_edit.clone() else {
            return;
        };
        screens::dim_background(ui);
        let (w, h) = (ui.w, ui.h);
        ui.centered(
            self.lang.get("sign.edit"),
            (w / 2.0).floor(),
            40.0,
            0xFFFFFF,
        );
        let id = self.world.peek_block(p[0], p[1], p[2]);
        let meta = self.world.peek_meta(p[0], p[1], p[2]) as i32;
        let standing = id != block::WALL_SIGN;
        let sc = render::entities::scale;
        let mut mm = render::translate((w / 2.0).floor(), 0.0, 50.0);
        mm = render::mul(&mm, &sc(-93.75, -93.75, -93.75));
        mm = render::mul(&mm, &render::rot_y(180.0));
        if standing {
            mm = render::mul(&mm, &render::rot_y(meta as f32 * 360.0 / 16.0));
        } else {
            let a = match meta {
                2 => 180.0,
                4 => 90.0,
                5 => -90.0,
                _ => 0.0,
            };
            mm = render::mul(&mm, &render::rot_y(a));
        }
        mm = render::mul(&mm, &render::translate(0.0, -1.0625, 0.0));
        mm = render::mul(&mm, &render::translate(-0.5, -0.75, -0.5));
        let edit = (counter / 6 % 2 == 0).then_some(line);
        let (board, words) =
            render::sign::sign(&self.atlas, &mm, standing, meta, &lines, edit, [0; 4]);
        // no lighting in the GUI: full colour, then back to front
        let mut quads = render::model::gui_quads(board, &render::IDENTITY, None);
        for q in quads.iter_mut() {
            for v in q.iter_mut() {
                v.color = [255; 4];
            }
        }
        for q in words.chunks(4) {
            quads.push([q[0], q[1], q[2], q[3]]);
        }
        for q in render::model::sort_quads(quads).chunks(4) {
            let mut c = [q[0], q[1], q[2], q[3]];
            for v in &mut c {
                v.light = [0, 0, 0, 0];
            }
            ui.v.extend_from_slice(&[c[0], c[1], c[2], c[0], c[2], c[3]]);
        }
        // the Done button
        screens::button(
            ui,
            (w / 2.0).floor() - 100.0,
            (h / 4.0).floor() + 120.0,
            200.0,
            self.lang.get("gui.done"),
            true,
            m,
        );
    }

    fn settings(&self) -> crate::settings::GameSettings {
        self.shell
            .as_ref()
            .map_or_else(crate::settings::GameSettings::default, |s| {
                s.settings.clone()
            })
    }

    /// `displayGuiScreen(new GuiChat(text))`
    fn open_chat(&mut self, text: &str) -> Request {
        let (w, h) = self.gui_size();
        let atlas = self.atlas.clone();
        let ui = crate::gui::Ui::new(&atlas, w, h);
        self.chat_screen = Some(crate::chat::ChatScreen::new(&ui, &self.chat, text));
        self.screen = Screen::Chat;
        self.input = Input::default();
        Request::Release
    }

    /// `GuiNewChat.printChatMessage`
    pub fn print_chat(&mut self, text: &str) {
        let (w, h) = self.gui_size();
        let atlas = self.atlas.clone();
        let ui = crate::gui::Ui::new(&atlas, w, h);
        let st = self.settings();
        let open = self.screen == Screen::Chat;
        self.chat.print(&ui, &st, text, self.ticks as i32, open);
    }

    /// `GuiScreen.keyTyped` for the screens that take text.
    pub fn text_key(&mut self, ch: char, code: i32, ctrl: bool, shift: bool) -> Request {
        if self.screen == Screen::Book {
            let (w, h) = self.gui_size();
            let atlas = self.atlas.clone();
            let ui = crate::gui::Ui::new(&atlas, w, h);
            let clip = self
                .shell
                .as_ref()
                .map_or_else(String::new, |s| s.clipboard.clone());
            let a = match self.book_gui.as_mut() {
                Some(b) => b.key(&ui, ch, code, &clip),
                None => crate::book::BookAction::Close(None),
            };
            return self.book_action(a);
        }
        if let Some((_, lines, line, _)) = self.sign_edit.as_mut() {
            // GuiEditSign.keyTyped
            let _ = (ctrl, shift);
            if code == 200 {
                *line = line.wrapping_sub(1) & 3;
            }
            if code == 208 || code == 28 || code == 156 {
                *line = (*line + 1) & 3;
            }
            if code == 14 {
                lines[*line].pop();
            }
            // ChatAllowedCharacters.isAllowedCharacter
            let allowed = ch != '\u{a7}' && ch >= ' ' && ch != '\u{7f}';
            if allowed && lines[*line].chars().count() < 15 {
                lines[*line].push(ch);
            }
            if code == 1 {
                return self.close_sign();
            }
            return Request::None;
        }
        if self.sleep_gui && self.chat_screen.is_some() && (code == 1 || code == 28 || code == 156)
        {
            // GuiSleepMP.keyTyped
            if code == 1 {
                let _ = self.sim.commands.send(Command::LeaveBed);
            } else {
                let (w, h) = self.gui_size();
                let atlas = self.atlas.clone();
                let ui = crate::gui::Ui::new(&atlas, w, h);
                if let Some(c) = self.chat_screen.as_mut() {
                    let t: String = c.field.text.iter().collect::<String>().trim().to_string();
                    if !t.is_empty() {
                        let _ = self.sim.commands.send(Command::Chat(t));
                    }
                    c.field.set_text(&ui, "");
                }
                self.chat.reset_scroll();
            }
            return Request::None;
        }
        if let Some(mut c) = self.chat_screen.take() {
            let (w, h) = self.gui_size();
            let atlas = self.atlas.clone();
            let ui = crate::gui::Ui::new(&atlas, w, h);
            let st = self.settings();
            let mut clip = self
                .shell
                .as_ref()
                .map_or_else(String::new, |s| s.clipboard.clone());
            let r = c.key(&ui, &mut self.chat, &st, ch, code, ctrl, shift, &mut clip);
            if let Some(s) = self.shell.as_mut() {
                s.clipboard = clip;
            }
            match r {
                crate::chat::ChatKey::None => {
                    self.chat_screen = Some(c);
                    return Request::None;
                }
                crate::chat::ChatKey::Send(t) => {
                    let _ = self.sim.commands.send(Command::Chat(t));
                }
                crate::chat::ChatKey::Close => {}
            }
            // onGuiClosed
            self.chat.reset_scroll();
            self.screen = Screen::Playing;
            return Request::Grab;
        }
        if self.screen == Screen::Window && self.anvil.is_some() {
            // GuiRepair.keyTyped: the field first, then GuiContainer
            let (w, h) = self.gui_size();
            let atlas = self.atlas.clone();
            let ui = crate::gui::Ui::new(&atlas, w, h);
            let mut clip = self
                .shell
                .as_ref()
                .map_or_else(String::new, |s| s.clipboard.clone());
            let slot0 = self.view.tile_slots.first().copied().flatten();
            let mut a = self.anvil.take().unwrap();
            let took = a.key(&ui, ch, code, ctrl, shift, &mut clip, slot0, &self.lang);
            self.anvil = Some(a);
            if let Some(s) = self.shell.as_mut() {
                s.clipboard = clip;
            }
            if let Some(n) = took {
                // updateItemName on the client's container, then the packet
                self.reprice_anvil();
                let _ = self.sim.commands.send(Command::RenameItem(n));
                return Request::None;
            }
            let close = self.with_container(|g, f, k, send| g.key_typed(f, code, ctrl, k, send));
            return if close {
                self.close_window()
            } else {
                Request::None
            };
        }
        if let Some(mut c) = self.creative.take() {
            let (dims, mouse, keys) = self.creative_input();
            let atlas = self.atlas.clone();
            let ui = crate::gui::Ui::new(&atlas, dims.0 as f32, dims.1 as f32);
            let chat = self.shell.as_ref().map_or_else(
                || crate::settings::GameSettings::default().key("key.chat"),
                |s| s.settings.key("key.chat"),
            );
            let mut clip = self
                .shell
                .as_ref()
                .map_or_else(String::new, |s| s.clipboard.clone());
            let mut view = std::mem::replace(&mut self.view, InvView::empty());
            let close = c.key_typed(
                &mut view, dims, mouse, ch, code, ctrl, shift, &keys, chat, &ui, &self.lang,
                &mut clip,
            );
            self.view = view;
            if let Some(s) = self.shell.as_mut() {
                s.clipboard = clip;
            }
            self.creative = Some(c);
            self.flush_creative();
            if close {
                return self.close_window();
            }
        }
        Request::None
    }

    /// `TileEntityChest.updateEntity` on the client: the lids of the chests
    /// in use open, the others close, with their sounds.
    fn tick_chests(&mut self) {
        let users: Vec<[i32; 3]> = match &self.view.open {
            Open::Chest(ps) => ps.clone(),
            Open::Ender(p) => vec![*p],
            _ => Vec::new(),
        };
        for p in &users {
            self.chest_lids.entry(*p).or_insert([0.0; 2]);
        }
        let mut sounds = Vec::new();
        let keys: Vec<[i32; 3]> = self.chest_lids.keys().copied().collect();
        for p in keys {
            let own = self.world.peek_block(p[0], p[1], p[2]);
            if !block::is_chest(own) && own != block::ENDER_CHEST {
                self.chest_lids.remove(&p);
                continue;
            }
            // the other half of a double chest: the same kind of chest
            let chest = |w: &ClientWorld, x: i32, y: i32, z: i32| {
                own != block::ENDER_CHEST && w.peek_block(x, y, z) == own
            };
            let using = users.contains(&p);
            let x_neg = chest(&self.world, p[0] - 1, p[1], p[2]);
            let z_neg = chest(&self.world, p[0], p[1], p[2] - 1);
            let x_pos = chest(&self.world, p[0] + 1, p[1], p[2]);
            let z_pos = chest(&self.world, p[0], p[1], p[2] + 1);
            let at = [
                p[0] as f64 + 0.5 + if x_pos { 0.5 } else { 0.0 },
                p[1] as f64 + 0.5,
                p[2] as f64 + 0.5 + if z_pos { 0.5 } else { 0.0 },
            ];
            let l = self.chest_lids.get_mut(&p).unwrap();
            l[0] = l[1];
            if using && l[1] == 0.0 && !z_neg && !x_neg {
                sounds.push(("random.chestopen", at));
            }
            if !using && l[1] > 0.0 || using && l[1] < 1.0 {
                let before = l[1];
                l[1] += if using { 0.1 } else { -0.1 };
                l[1] = l[1].min(1.0);
                if l[1] < 0.5 && before >= 0.5 && !z_neg && !x_neg {
                    sounds.push(("random.chestclosed", at));
                }
                l[1] = l[1].max(0.0);
            }
            if !using && l[0] == 0.0 && l[1] == 0.0 {
                self.chest_lids.remove(&p);
            }
        }
        for (name, at) in sounds {
            let pitch = self.particle_rand.next_float() * 0.1 + 0.9;
            self.sound(name, Some(at), 0.5, pitch);
        }
    }

    /// `TileEntityEnchantmentTable.updateEntity` for every loaded table.
    fn tick_tables(&mut self) {
        use std::f32::consts::PI;
        let list: Vec<[i32; 3]> = self
            .world
            .chunks
            .values()
            .flat_map(|c| c.tables.clone())
            .collect();
        self.tables.retain(|p, _| list.contains(p));
        let eye = self.player.pos;
        for p in list {
            if self.world.peek_block(p[0], p[1], p[2]) != block::ENCHANTING_TABLE {
                continue;
            }
            let t = self.tables.entry(p).or_insert([0.0; 10]);
            // [0] ticks, [1] flip prev, [2] flip, [3] flipT, [4] flipA,
            // [5] spread prev, [6] spread, [7] rot prev, [8] rot, [9] tRot
            t[5] = t[6];
            t[7] = t[8];
            // getClosestPlayer(x + 0.5, y + 0.5, z + 0.5, 3.0) on posY (the eyes)
            let c = [p[0] as f64 + 0.5, p[1] as f64 + 0.5, p[2] as f64 + 0.5];
            let d2 = (eye[0] - c[0]).powi(2) + (eye[1] - c[1]).powi(2) + (eye[2] - c[2]).powi(2);
            if d2 < 9.0 {
                t[9] = ((eye[2] - c[2]) as f32).atan2((eye[0] - c[0]) as f32);
                t[6] += 0.1;
                if t[6] < 0.5 || self.table_rand.next_int_n(40) == 0 {
                    let old = t[3];
                    loop {
                        t[3] +=
                            (self.table_rand.next_int_n(4) - self.table_rand.next_int_n(4)) as f32;
                        if old != t[3] {
                            break;
                        }
                    }
                }
            } else {
                t[9] += 0.02;
                t[6] -= 0.1;
            }
            let wrap = |mut a: f32| {
                while a >= PI {
                    a -= PI * 2.0;
                }
                while a < -PI {
                    a += PI * 2.0;
                }
                a
            };
            t[8] = wrap(t[8]);
            t[9] = wrap(t[9]);
            let d = wrap(t[9] - t[8]);
            t[8] += d * 0.4;
            t[6] = t[6].clamp(0.0, 1.0);
            t[0] += 1.0;
            t[1] = t[2];
            let f = ((t[3] - t[2]) * 0.4).clamp(-0.2, 0.2);
            t[4] += (f - t[4]) * 0.9;
            t[2] += t[4];
        }
    }

    /// `TileEntityEnchantmentTableRenderer`: the floating book.
    fn draw_tables(
        &self,
        out: &mut Vec<crate::render::mesher::Vertex>,
        eye: [f64; 3],
        partial: f32,
    ) {
        use std::f32::consts::PI;
        let Some(tex) = self.atlas.mob_tex(crate::render::model::BOOK_TEXTURE) else {
            return;
        };
        for (p, t) in &self.tables {
            let rel = [
                (p[0] as f64 - eye[0]) as f32,
                (p[1] as f64 - eye[1]) as f32,
                (p[2] as f64 - eye[2]) as f32,
            ];
            if rel[0] * rel[0] + rel[1] * rel[1] + rel[2] * rel[2] > 64.0 * 64.0 {
                continue;
            }
            let time = t[0] + partial;
            let mut m = render::translate(rel[0] + 0.5, rel[1] + 0.75, rel[2] + 0.5);
            m = render::mul(
                &m,
                &render::translate(0.0, 0.1 + (time * 0.1).sin() * 0.01, 0.0),
            );
            let mut d = t[8] - t[7];
            while d >= PI {
                d -= PI * 2.0;
            }
            while d < -PI {
                d += PI * 2.0;
            }
            let rot = t[7] + d * partial;
            m = render::mul(&m, &render::rot_y(-rot * 180.0 / PI));
            m = render::mul(&m, &render::rot_z(80.0));
            let flip = t[1] + (t[2] - t[1]) * partial;
            let (fa, fb) = render::model::book_flips(flip);
            let spread = t[5] + (t[6] - t[5]) * partial;
            let (sky, blk) = (
                self.world.light(true, p[0], p[1], p[2]),
                self.world.light(false, p[0], p[1], p[2]),
            );
            let mut dr = render::model::Draw {
                out: &mut *out,
                tex,
                light: [(blk << 4) as u8, (sky << 4) as u8, 0, 0],
                tint: [1.0; 4],
            };
            render::model::book_world(&mut dr, &m, time, fa, fb, spread);
        }
    }

    /// `TileEntitySignRenderer` for the signs within 64 blocks.
    fn draw_signs(&self, out: &mut Vec<crate::render::mesher::Vertex>, eye: [f64; 3]) {
        let w = &self.world;
        for c in w.chunks.values() {
            for &p in &c.signs {
                let d = [
                    p[0] as f64 + 0.5 - eye[0],
                    p[1] as f64 + 0.5 - eye[1],
                    p[2] as f64 + 0.5 - eye[2],
                ];
                if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] > 64.0 * 64.0 {
                    continue;
                }
                let id = w.peek_block(p[0], p[1], p[2]);
                if id != block::STANDING_SIGN && id != block::WALL_SIGN {
                    continue;
                }
                let editing = self.sign_edit.as_ref().filter(|e| e.0 == p);
                let empty: [String; 4] = Default::default();
                let lines = match editing {
                    Some(e) => &e.1,
                    None => self.sign_texts.get(&p).unwrap_or(&empty),
                };
                let (sky, blk) = (
                    w.light(true, p[0], p[1], p[2]),
                    w.light(false, p[0], p[1], p[2]),
                );
                let base = render::translate(
                    (p[0] as f64 - eye[0]) as f32,
                    (p[1] as f64 - eye[1]) as f32,
                    (p[2] as f64 - eye[2]) as f32,
                );
                let (board, words) = render::sign::sign(
                    &self.atlas,
                    &base,
                    id == block::STANDING_SIGN,
                    w.peek_meta(p[0], p[1], p[2]) as i32,
                    lines,
                    None,
                    [(blk << 4) as u8, (sky << 4) as u8, 0, 0],
                );
                out.extend(board);
                for q in words.chunks(4) {
                    out.extend_from_slice(q);
                    out.extend_from_slice(&[q[3], q[2], q[1], q[0]]);
                }
            }
        }
    }

    /// `TileEntityChestRenderer` for the chests in view.
    fn draw_chests(
        &self,
        out: &mut Vec<crate::render::mesher::Vertex>,
        eye: [f64; 3],
        partial: f32,
    ) {
        let rd = self.render_distance as f64 * 16.0;
        let xmas = crate::gui::christmas();
        let w = &self.world;
        for c in w.chunks.values() {
            for &p in &c.chests {
                let d = [p[0] as f64 + 0.5 - eye[0], p[2] as f64 + 0.5 - eye[2]];
                if d[0] * d[0] + d[1] * d[1] > rd * rd {
                    continue;
                }
                let own = w.peek_block(p[0], p[1], p[2]);
                if !block::is_chest(own) && own != block::ENDER_CHEST {
                    continue;
                }
                let is =
                    |x: i32, z: i32| own != block::ENDER_CHEST && w.peek_block(x, p[1], z) == own;
                if is(p[0] - 1, p[2]) || is(p[0], p[2] - 1) {
                    continue;
                }
                let (x_pos, z_pos) = (is(p[0] + 1, p[2]), is(p[0], p[2] + 1));
                let large = x_pos || z_pos;
                let Some(t) = self.atlas.mob_tex(if own == block::ENDER_CHEST {
                    "chest/ender"
                } else {
                    crate::render::chest::texture(own == block::TRAPPED_CHEST, large, xmas)
                }) else {
                    continue;
                };
                let lid = self
                    .chest_lids
                    .get(&p)
                    .map_or(0.0, |l| l[0] + (l[1] - l[0]) * partial);
                let (sky, blk) = (
                    w.light(true, p[0], p[1], p[2]),
                    w.light(false, p[0], p[1], p[2]),
                );
                let rel = [
                    (p[0] as f64 - eye[0]) as f32,
                    (p[1] as f64 - eye[1]) as f32,
                    (p[2] as f64 - eye[2]) as f32,
                ];
                crate::render::chest::world(
                    out,
                    t,
                    rel,
                    w.peek_meta(p[0], p[1], p[2]) as i32,
                    x_pos,
                    z_pos,
                    lid,
                    [(blk << 4) as u8, (sky << 4) as u8, 0, 0],
                );
            }
        }
    }

    fn enchant_state(&self, partial: f32) -> crate::gui_container::EnchantState {
        let b = &self.book;
        crate::gui_container::EnchantState {
            xp_level: self.stats.xp_level,
            creative: self.mode == GameMode::Creative,
            open: b.o_open + (b.open - b.o_open) * partial,
            flip: b.o_flip + (b.flip - b.o_flip) * partial,
        }
    }

    /// `closeScreen` from a container.
    fn close_window(&mut self) -> Request {
        let _ = self.sim.commands.send(Command::CloseWindow);
        // the client's onContainerClosed: the cursor and grid leave
        self.view.inv.cursor = None;
        self.view.inv.grid = [None; 9];
        self.view.result = None;
        self.view.open = Open::None;
        self.gui = Default::default();
        self.client_drag = Default::default();
        if let Some(c) = self.creative.take() {
            self.creative_tab = c.tab;
        }
        self.screen = Screen::Playing;
        Request::Grab
    }

    fn menu_hit(&mut self) -> Option<MenuAction> {
        let (w, h) = self.gui_size();
        let mut ui = crate::gui::Ui::new(&self.atlas, w, h);
        let m = self.gui_mouse();
        match self.screen {
            Screen::Paused => screens::pause_menu(&mut ui, m),
            Screen::Dead => screens::death_screen(&mut ui, &self.stats, m),
            _ => None,
        }
    }

    /// Mouse buttons: 0 left, 1 right, 2 middle.
    pub fn mouse_button(&mut self, button: u8, pressed: bool, shift: bool) -> Request {
        match button {
            0 => self.left_down = pressed,
            1 => self.right_down = pressed,
            _ => {}
        }
        if self.screen == Screen::Window && self.creative.is_some() {
            let now = self.started.elapsed().as_millis() as u64;
            let (dims, mouse, keys) = self.creative_input();
            let atlas = self.atlas.clone();
            let ui = crate::gui::Ui::new(&atlas, dims.0 as f32, dims.1 as f32);
            let mut c = self.creative.take().unwrap();
            let mut view = std::mem::replace(&mut self.view, InvView::empty());
            if pressed {
                c.mouse_clicked(&mut view, dims, mouse, button as i32, shift, now, &keys);
            } else {
                c.mouse_released(
                    &mut view,
                    dims,
                    mouse,
                    button as i32,
                    shift,
                    &keys,
                    &ui,
                    &self.lang,
                );
            }
            self.view = view;
            self.creative = Some(c);
            self.flush_creative();
            return Request::None;
        }
        if self.screen == Screen::Window {
            let now = self.started.elapsed().as_millis() as u64;
            self.with_container(|g, f, k, send| {
                if pressed {
                    g.mouse_clicked(f, button as i32, shift, now, k, send);
                } else {
                    g.mouse_released(f, button as i32, shift, k, send);
                }
            });
            // GuiRepair.mouseClicked: then the name field
            if pressed {
                let (w, h) = self.gui_size();
                let m = self.gui_mouse();
                let atlas = self.atlas.clone();
                let ui = crate::gui::Ui::new(&atlas, w, h);
                if let Some(a) = self.anvil.as_mut() {
                    a.field.click(&ui, (m.0 as f32, m.1 as f32), button as u8);
                }
            }
            // GuiBeacon.actionPerformed
            if pressed && button == 0 {
                if let Open::Beacon(_) = self.view.open {
                    let (w, h) = self.gui_size();
                    if let Some(b) = crate::gui_container::beacon_button_at(
                        &self.view,
                        w as i32,
                        h as i32,
                        self.gui_mouse_i(),
                    ) {
                        self.sound("random.click", None, 1.0, 1.0);
                        let levels = self.view.beacon[0];
                        match b.id {
                            -2 => {
                                let _ = self.sim.commands.send(Command::CloseWindow);
                                return Request::None;
                            }
                            -1 => {
                                let _ = self
                                    .sim
                                    .commands
                                    .send(Command::BeaconSet(self.view.beacon[1], self.view.beacon[2]));
                                let _ = self.sim.commands.send(Command::CloseWindow);
                                return Request::None;
                            }
                            id if !b.selected => {
                                let e = id & 255;
                                if id >> 8 < 3 {
                                    self.view.beacon[1] = crate::play::beacon_primary(levels, e);
                                } else {
                                    self.view.beacon[2] = crate::play::beacon_secondary(levels, e);
                                }
                                return Request::None;
                            }
                            _ => return Request::None,
                        }
                    }
                }
            }
            // GuiMerchant.actionPerformed: the next and previous buttons
            if pressed && button == 0 {
                if let Open::Merchant(_) = self.view.open {
                    let (w, h) = self.gui_size();
                    if let Some(d) = crate::gui_container::merchant_button_at(
                        &self.view,
                        w as i32,
                        h as i32,
                        self.gui_mouse_i(),
                    ) {
                        let i = (self.view.merchant_index as i32 + d).max(0) as usize;
                        self.view.merchant_index = i;
                        // ContainerMerchant.setCurrentRecipeIndex on the client
                        let creative = self.mode == GameMode::Creative;
                        let offers = self.view.offers.clone();
                        let mut tile = std::mem::take(&mut self.view.tile_slots);
                        {
                            let mut c = crate::container::Ctx::new(
                                &mut self.view.inv,
                                &mut tile,
                                crate::container::Window::Merchant,
                                creative,
                            )
                            .with_merchant(&offers, i);
                            c.set_offer_index(i);
                        }
                        self.view.tile_slots = tile;
                        let _ = self.sim.commands.send(Command::MerchantSelect(i));
                        self.sound("random.click", None, 1.0, 1.0);
                        return Request::None;
                    }
                }
            }
            // GuiEnchantment.mouseClicked: the three buttons
            if pressed {
                if let Open::Enchant(_) = self.view.open {
                    let (w, h) = self.gui_size();
                    let m = self.gui_mouse_i();
                    if let Some(i) = crate::gui_container::enchant_button_at(
                        &self.view.open,
                        w as i32,
                        h as i32,
                        m,
                    ) {
                        let lvl = self.view.enchant[i];
                        // ContainerEnchantment.enchantItem on the client
                        if lvl > 0
                            && (self.stats.xp_level >= lvl || self.mode == GameMode::Creative)
                        {
                            let _ = self.sim.commands.send(Command::EnchantItem(i));
                        }
                    }
                }
            }
            return Request::None;
        }
        if !pressed {
            if button == 0 {
                self.dig = None;
            }
            return Request::None;
        }
        match self.screen {
            Screen::Playing => {
                match button {
                    0 if self.target_mob.is_some() => {
                        self.swing_item();
                        let (id, _) = self.target_mob.unwrap();
                        self.attack_mob(id);
                    }
                    0 => {
                        self.swing_item();
                        // The first hit happens on the press (clickBlock).
                        self.hit_delay = 0;
                        self.tick_digging();
                    }
                    1 if self.target_mob.is_some() => {
                        let (id, _) = self.target_mob.unwrap();
                        // interactWithEntitySendPacket: no swing
                        let _ = self.sim.commands.send(Command::Interact { id });
                        self.right_delay = 4;
                    }
                    1 => self.use_item(),
                    _ => {
                        if let Some((b, _)) = self.selected {
                            let _ = self.sim.commands.send(Command::PickBlock(b));
                        } else if let Some((id, _)) = self.target_mob {
                            if let Some((item, dmg, sub)) = self.pick_entity(id) {
                                let _ = self.sim.commands.send(Command::PickEntity(item, dmg, sub));
                            }
                        }
                    }
                }
                Request::None
            }
            Screen::Window => Request::None,
            Screen::Paused => self.menu_mouse(button, true),
            Screen::Chat => {
                // GuiSleepMP's "Leave Bed"
                if self.sleep_gui && button == 0 {
                    let (w, h) = self.gui_size();
                    let (mx, my) = self.gui_mouse();
                    let (bx, by) = ((w / 2.0).floor() - 100.0, h - 40.0);
                    if mx >= bx && my >= by && mx < bx + 200.0 && my < by + 20.0 {
                        // GuiButton.playPressSound
                        self.sound("gui.button.press", None, 1.0, 1.0);
                        let _ = self.sim.commands.send(Command::LeaveBed);
                    }
                }
                Request::None
            }
            Screen::Book => {
                if button != 0 {
                    return Request::None;
                }
                let (w, _) = self.gui_size();
                let m = self.gui_mouse();
                let r = self.book_gui.as_mut().map(|b| b.click(w, m));
                match r {
                    Some((pressed, a)) => {
                        if pressed {
                            self.sound("gui.button.press", None, 1.0, 1.0);
                        }
                        self.book_action(a)
                    }
                    None => Request::None,
                }
            }
            Screen::Sign => {
                // the Done button
                let (w, h) = self.gui_size();
                let (mx, my) = self.gui_mouse();
                let (bx, by) = ((w / 2.0).floor() - 100.0, (h / 4.0).floor() + 120.0);
                if button == 0 && mx >= bx && my >= by && mx < bx + 200.0 && my < by + 20.0 {
                    return self.close_sign();
                }
                Request::None
            }
            Screen::Dead => match self.menu_hit() {
                Some(MenuAction::BackToGame) => {
                    self.screen = Screen::Playing;
                    Request::Grab
                }
                Some(MenuAction::SaveAndQuit) => Request::QuitToTitle,
                Some(MenuAction::Respawn) => {
                    let _ = self.sim.commands.send(Command::Respawn);
                    Request::Grab
                }
                None => Request::None,
            },
        }
    }

    pub fn key(&mut self, code: winit::keyboard::KeyCode, pressed: bool, ctrl: bool) -> Request {
        use winit::keyboard::KeyCode as K;
        if !pressed {
            return Request::None;
        }
        match (self.screen, code) {
            (Screen::Playing, K::Escape) => {
                // Minecraft.displayInGameMenu
                self.screen = Screen::Paused;
                self.menu = Some(crate::menu::Menu::ingame());
                self.menu_size = (0.0, 0.0);
                self.input = Input::default();
                Request::Release
            }
            (Screen::Paused, _) => {
                let c = if code == K::Escape { '\u{1b}' } else { '\0' };
                let _ = c;
                self.menu_key('\0', crate::settings::lwjgl_code(code))
            }
            (Screen::Window, _) if self.creative.is_some() => Request::None,
            (Screen::Window, _) => {
                let lw = crate::settings::lwjgl_code(code);
                let close = self.with_container(|g, f, k, send| g.key_typed(f, lw, ctrl, k, send));
                if close {
                    self.close_window()
                } else {
                    Request::None
                }
            }
            (Screen::Playing, _) => self.game_key(crate::settings::lwjgl_code(code), ctrl),
            _ => Request::None,
        }
    }

    /// The key binding's code (`GameSettings.keyBind*`).
    fn bind(&self, d: &str) -> i32 {
        self.shell.as_ref().map_or_else(
            || crate::settings::GameSettings::default().key(d),
            |s| s.settings.key(d),
        )
    }

    /// `Minecraft.runTick`'s key handling while no screen is open.
    fn game_key(&mut self, lw: i32, ctrl: bool) -> Request {
        if lw == 59 {
            // F1: hideGUI
            self.hide_gui = !self.hide_gui;
            return Request::None;
        }
        if lw == self.bind("key.togglePerspective") {
            self.third_person = (self.third_person + 1) % 3;
            return Request::None;
        }
        if lw == self.bind("key.inventory") {
            // Minecraft.runTick: C16PacketClientStatus(OPEN_INVENTORY_ACHIEVEMENT)
            // before GuiInventory
            let _ = self.sim.commands.send(Command::OpenInventoryAchievement);
            self.screen = Screen::Window;
            self.input = Input::default();
            if self.mode == GameMode::Creative {
                // GuiInventory.initGui hands over to GuiContainerCreative
                self.open_creative();
            } else {
                let _ = self.sim.commands.send(Command::OpenInventory);
            }
            return Request::Release;
        }
        if lw == self.bind("key.drop") {
            // EntityClientPlayerMP.dropOneItem: only the packet, no swing
            let _ = self.sim.commands.send(Command::DropHeld { all: ctrl });
            return Request::None;
        }
        if lw == self.bind("key.chat") {
            return self.open_chat("");
        }
        if lw == self.bind("key.command") {
            return self.open_chat("/");
        }
        for i in 0..9 {
            if lw == self.bind(&format!("key.hotbar.{}", i + 1)) {
                let _ = self.sim.commands.send(Command::Select(i));
                self.view.inv.selected = i;
            }
        }
        Request::None
    }

    pub fn scroll(&mut self, up: bool) {
        if let Some(mut c) = self.chat_screen.take() {
            let st = self.settings();
            let shift = self.shell.as_ref().is_some_and(|s| s.shift);
            c.wheel(&mut self.chat, &st, up, shift);
            self.chat_screen = Some(c);
            return;
        }
        if let Some(mut c) = self.creative.take() {
            c.wheel(up);
            self.creative = Some(c);
            return;
        }
        if self.screen == Screen::Playing {
            let s = self.view.inv.selected;
            let n = if up { (s + 8) % 9 } else { (s + 1) % 9 };
            self.view.inv.selected = n;
            let _ = self.sim.commands.send(Command::Select(n));
        }
    }

    /// Whether the mouse should be captured for looking around.
    pub fn wants_grab(&self) -> bool {
        self.screen == Screen::Playing
    }
    fn lightmap(&self, out: &mut [[f32; 4]; 256]) {
        // World.getSunBrightness (client)
        let angle = self.celestial_angle(1.0);
        let mut sb = 1.0f32 - (math::cos(angle * math::PI * 2.0f32) * 2.0f32 + 0.2f32);
        sb = sb.clamp(0.0, 1.0);
        sb = 1.0 - sb;
        let rain = self.rain(1.0);
        let thunder = self.weather.thunder;
        sb = (sb as f64 * (1.0 - (rain * 5.0) as f64 / 16.0)) as f32;
        sb = (sb as f64 * (1.0 - (thunder * 5.0) as f64 / 16.0)) as f32;
        let sun = sb * 0.8f32 + 0.2f32;
        for i in 0..256usize {
            let f = sun * 0.95f32 + 0.05f32;
            let mut f1 = self.light_table[i / 16] * f;
            // EntityRenderer.updateLightmap: full sky light during a flash
            if self.last_lightning > 0 {
                f1 = self.light_table[i / 16];
            }
            let f2 = self.light_table[i % 16] * (self.torch_x * 0.1f32 + 1.5f32);
            let f3 = f1 * (sun * 0.65f32 + 0.35f32);
            let f4 = f1 * (sun * 0.65f32 + 0.35f32);
            let f5 = f2 * ((f2 * 0.6f32 + 0.4f32) * 0.6f32 + 0.4f32);
            let f6 = f2 * (f2 * f2 * 0.6f32 + 0.4f32);
            let mut r = f3 + f2;
            let mut g = f4 + f5;
            let mut b = f1 + f6;
            r = r * 0.96f32 + 0.03f32;
            g = g * 0.96f32 + 0.03f32;
            b = b * 0.96f32 + 0.03f32;
            if self.dim == 1 {
                r = 0.22f32 + f2 * 0.75f32;
                g = 0.28f32 + f5 * 0.75f32;
                b = 0.25f32 + f6 * 0.75f32;
            }
            r = r.min(1.0);
            g = g.min(1.0);
            b = b.min(1.0);
            // gameSettings.gammaSetting (0.0 "Moody" by default)
            let gamma = self.shell.as_ref().map_or(0.0, |s| s.settings.gamma);
            let curve = |c: f32| {
                let mut inv = 1.0f32 - c;
                inv = 1.0f32 - inv * inv * inv * inv;
                let c = c * (1.0f32 - gamma) + inv * gamma;
                (c * 0.96f32 + 0.03f32).clamp(0.0, 1.0)
            };
            // Vanilla stores the map as 8-bit texels.
            let q = |c: f32| ((c * 255.0f32) as i32) as f32 / 255.0f32;
            out[i] = [q(curve(r)), q(curve(g)), q(curve(b)), 1.0];
        }
    }

    /// Advances the simulation and renders one frame.
    pub fn frame(&mut self, window_size: (u32, u32)) {
        // Timer.updateTimer: 20 ticks per second, at most 10 per frame.
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f64().clamp(0.0, 1.0);
        self.last = now;
        self.tick_acc += dt * 20.0;
        let mut n = self.tick_acc as i32;
        self.tick_acc -= n as f64;
        if n > 10 {
            n = 10;
        }
        self.pump_chunks();
        let t_pump = Instant::now();
        self.prof[0] += t_pump.duration_since(now).as_secs_f64();
        for _ in 0..n {
            self.tick();
        }
        let t_tick = Instant::now();
        self.prof[1] += t_tick.duration_since(t_pump).as_secs_f64();
        self.partial = (self.tick_acc as f32).clamp(0.0, 1.0);
        let partial = self.partial;

        let eye = self.eye(partial);
        let picked = self.pick(eye);
        self.selected = picked.map(|(b, n, _)| (b, n));
        if let Some((_, _, v)) = picked {
            self.hit = v;
        }
        {
            // EntityRenderer.getMouseOver: a mob closer than the block wins.
            let look = self.look();
            let reach = if self.mode == GameMode::Creative {
                5.0
            } else {
                3.0
            };
            // objectMouseOver.hitVec.distanceTo(eye)
            let block_d = picked.map(|(_, _, v)| {
                ((v[0] - eye[0]).powi(2) + (v[1] - eye[1]).powi(2) + (v[2] - eye[2]).powi(2)).sqrt()
            });
            self.target_mob = None;
            for m in &self.mobs {
                if m.death_time > 0 {
                    continue;
                }
                let bb = crate::entity::player::Aabb {
                    min: [m.bb[0] - 0.1, m.bb[1] - 0.1, m.bb[2] - 0.1],
                    max: [m.bb[3] + 0.1, m.bb[4] + 0.1, m.bb[5] + 0.1],
                };
                if let Some(t) = crate::entity::mob::ray_box(eye, look, &bb) {
                    if t <= reach
                        && block_d.map_or(true, |d| t < d)
                        && self.target_mob.map_or(true, |x| t < x.1)
                    {
                        self.target_mob = Some((m.id, t));
                    }
                }
            }
            for c in &self.boats {
                // canBeCollidedWith: !isDead (not orbs); getCollisionBorderSize 0.1
                if matches!(c.v, crate::entity::vehicle::Vehicle::Orb(_)) {
                    continue;
                }
                let b = &c.v.e().bb;
                let bb = crate::entity::player::Aabb {
                    min: [b.min[0] - 0.1, b.min[1] - 0.1, b.min[2] - 0.1],
                    max: [b.max[0] + 0.1, b.max[1] + 0.1, b.max[2] + 0.1],
                };
                if let Some(t) = crate::entity::mob::ray_box(eye, look, &bb) {
                    if t <= reach
                        && block_d.map_or(true, |d| t < d)
                        && self.target_mob.map_or(true, |x| t < x.1)
                    {
                        self.target_mob = Some((c.v.id(), t));
                    }
                }
            }
            if self.target_mob.is_some() {
                self.selected = None;
            }
        }
        let eye_mat = self.player.eye_material(&self.world, eye);

        // --- Fog and sky colours (EntityRenderer.updateFogColor) ---
        let rd = self.render_distance as f32;
        let angle = self.celestial_angle(partial);
        let day = (math::cos(angle * math::PI * 2.0f32) * 2.0f32 + 0.5f32).clamp(0.0, 1.0);
        let (ex, ey, ez) = (
            math::floor_double(eye[0]),
            math::floor_double(eye[1]),
            math::floor_double(eye[2]),
        );
        let b = self.world.biome(ex, ez);
        let temp = b.float_temperature(&self.noise, ex, ey, ez);
        let t = (temp / 3.0f32).clamp(-1.0, 1.0);
        let sky_rgb = hsb_to_rgb(0.62222224f32 - t * 0.05f32, 0.5f32 + t * 0.1f32, 1.0);
        let mut sky = [sky_rgb[0] * day, sky_rgb[1] * day, sky_rgb[2] * day];
        let rain = self.rain(partial);
        let thunder = self.weather.prev_thunder
            + (self.weather.thunder - self.weather.prev_thunder) * partial;
        // World.getSkyColor: rain and thunder grey the sky
        if rain > 0.0 {
            let l = (sky[0] * 0.3 + sky[1] * 0.59 + sky[2] * 0.11) * 0.6;
            for c in &mut sky {
                *c = *c * (1.0 - rain * 0.75) + l * rain * 0.75;
            }
        }
        if thunder > 0.0 {
            let l = (sky[0] * 0.3 + sky[1] * 0.59 + sky[2] * 0.11) * 0.2;
            for c in &mut sky {
                *c = *c * (1.0 - thunder * 0.75) + l * thunder * 0.75;
            }
        }
        // the lightning's flash
        if self.last_lightning > 0 {
            let f = (self.last_lightning as f32 - partial).min(1.0) * 0.45;
            sky[0] = sky[0] * (1.0 - f) + 0.8 * f;
            sky[1] = sky[1] * (1.0 - f) + 0.8 * f;
            sky[2] = sky[2] * (1.0 - f) + 1.0 * f;
        }
        let mut fog = match self.dim {
            // WorldProviderHell.getFogColor
            -1 => [0.2f32, 0.03, 0.03],
            // WorldProviderEnd.getFogColor
            1 => {
                let c = 0.15f32;
                [0.627451 * c, 0.5019608 * c, 0.627451 * c]
            }
            _ => [
                0.7529412f32 * (day * 0.94f32 + 0.06f32),
                0.84705883f32 * (day * 0.94f32 + 0.06f32),
                1.0f32 * (day * 0.91f32 + 0.09f32),
            ],
        };
        let look = self.look();
        if self.render_distance >= 4 {
            let sun_side = if math::sin(angle * math::PI * 2.0f32) > 0.0 {
                -1.0
            } else {
                1.0
            };
            let mut f5 = (look[0] * sun_side) as f32;
            if f5 < 0.0 {
                f5 = 0.0;
            }
            if f5 > 0.0 {
                // calcSunriseSunsetColors
                let f3 = math::cos(angle * math::PI * 2.0f32);
                if (-0.4f32..=0.4f32).contains(&f3) {
                    let c = f3 / 0.4f32 * 0.5f32 + 0.5f32;
                    let mut a = 1.0f32 - (1.0f32 - math::sin(c * math::PI)) * 0.99f32;
                    a *= a;
                    let col = [c * 0.3f32 + 0.7f32, c * c * 0.7f32 + 0.2f32, 0.2f32];
                    f5 *= a;
                    for i in 0..3 {
                        fog[i] = fog[i] * (1.0f32 - f5) + col[i] * f5;
                    }
                }
            }
        }
        let f1 = 1.0f32 - ((0.25f32 + 0.75f32 * rd / 16.0f32) as f64).powf(0.25) as f32;
        for i in 0..3 {
            fog[i] += (sky[i] - fog[i]) * f1;
        }
        if eye_mat == Material::Water {
            fog = [0.02, 0.02, 0.2];
        } else if eye_mat == Material::Lava {
            fog = [0.6, 0.1, 0.0];
        }
        // EntityRenderer.updateFogColor: rain and thunder
        if rain > 0.0 {
            let (a, b) = (1.0 - rain * 0.5, 1.0 - rain * 0.4);
            fog[0] *= a;
            fog[1] *= a;
            fog[2] *= b;
        }
        if thunder > 0.0 {
            for c in &mut fog {
                *c *= 1.0 - thunder * 0.5;
            }
        }
        let fb = self.fog_color2 + (self.fog_color1 - self.fog_color2) * partial;
        for c in &mut fog {
            *c *= fb;
        }
        let d0 = eye[1] * 0.03125;
        if d0 < 1.0 {
            let d = d0.max(0.0);
            let d = (d * d) as f32;
            for c in &mut fog {
                *c *= d;
            }
        }

        // --- Fog range (EntityRenderer.setupFog) ---
        let mut far = rd * 16.0f32;
        let sky_light = self.world.light(
            true,
            ex,
            math::floor_double(self.player.bb.min[1] + 1.8f32 as f64 * 0.66),
            ez,
        );
        let dv = sky_light as f64 / 16.0 + (eye[1] + 4.0) / 32.0;
        if dv < 1.0 {
            let d = dv.max(0.0);
            let mut f = 100.0f32 * (d * d) as f32;
            if f < 5.0 {
                f = 5.0;
            }
            if far > f {
                far = f;
            }
        }
        let fog_params = match eye_mat {
            Material::Water => [0.0, 0.0, 0.1, 1.0],
            Material::Lava => [0.0, 0.0, 2.0, 1.0],
            // WorldProviderHell.doesXZShowFog: thick fog close by
            _ if self.dim == -1 => [far * 0.05f32, far.min(192.0) * 0.5f32, 0.0, 0.0],
            _ => [far * 0.75f32, far, 0.0, 0.0],
        };

        // --- Camera (EntityRenderer.setupCameraTransform) ---
        // EntityRenderer.getFOVModifier(partial, true): gameSettings.fovSetting
        let fov_setting = self.shell.as_ref().map_or(70.0, |s| s.settings.fov);
        let mut fov =
            fov_setting * (self.fov_hand_prev + (self.fov_hand - self.fov_hand_prev) * partial);
        if eye_mat == Material::Water {
            fov = fov * 60.0f32 / 70.0f32;
        }
        let aspect = window_size.0.max(1) as f32 / window_size.1.max(1) as f32;
        let proj = render::perspective(fov, aspect, 0.05, rd * 16.0 * 2.0);
        let p = &self.player;
        let fw = p.distance_walked - p.prev_distance_walked;
        let b1 = -(p.distance_walked + fw * partial);
        let b2 = p.prev_camera_yaw + (p.camera_yaw - p.prev_camera_yaw) * partial;
        let b3 = p.prev_camera_pitch + (p.camera_pitch - p.prev_camera_pitch) * partial;
        // EntityRenderer.hurtCameraEffect
        let mut hurt: Mat4 = render::IDENTITY;
        if self.stats.dead {
            let d = self.stats.death_time as f32 + partial;
            hurt = render::rot_z(40.0f32 - 8000.0f32 / (d + 200.0f32));
        }
        let hf = self.stats.hurt_time as f32 - partial;
        if hf >= 0.0 && !self.stats.dead {
            let f = hf / crate::entity::survival::MAX_HURT_TIME as f32;
            let f = math::sin(f * f * f * f * math::PI);
            hurt = render::mul(&hurt, &render::rot_z(-f * 14.0f32));
        }
        // EntityRenderer.setupViewBobbing, only with gameSettings.viewBobbing
        let mut bob: Mat4 = render::IDENTITY;
        if self
            .shell
            .as_ref()
            .map_or(true, |s| s.settings.view_bobbing)
        {
            bob = render::translate(
                math::sin(b1 * math::PI) * b2 * 0.5f32,
                -(math::cos(b1 * math::PI) * b2).abs(),
                0.0,
            );
            bob = render::mul(&bob, &render::rot_z(math::sin(b1 * math::PI) * b2 * 3.0f32));
            bob = render::mul(
                &bob,
                &render::rot_x((math::cos(b1 * math::PI - 0.2f32) * b2).abs() * 5.0f32),
            );
        }
        let shake = render::mul(&hurt, &bob);
        let mut view: Mat4 = shake;
        view = render::mul(&view, &render::rot_x(b3));
        if let Some((_, d)) = self.sleeping {
            // EntityRenderer.orientCamera, sleeping: up 0.3, turned to the
            // bed (the yaw and pitch turns cancel out), then glTranslatef(0,
            // yOffset - 1.62 + 1, 0) with the sleeping yOffset of 0.2
            view = render::mul(&view, &render::translate(0.0, 0.3, 0.0));
            view = render::mul(&view, &render::rot_y(d as f32 * 90.0));
            view = render::mul(&view, &render::translate(0.0, 0.2 - 1.62 + 1.0, 0.0));
        } else if self.third_person > 0 {
            // EntityRenderer.orientCamera: back off, short of the blocks
            let mut d = 4.0f64;
            let yaw = p.yaw;
            let mut pitch = p.pitch;
            if self.third_person == 2 {
                pitch += 180.0;
            }
            let r = |a: f32| a / 180.0 * math::PI;
            let dx = (-math::sin(r(yaw)) * math::cos(r(pitch))) as f64 * d;
            let dz = (math::cos(r(yaw)) * math::cos(r(pitch))) as f64 * d;
            let dy = (-math::sin(r(pitch))) as f64 * d;
            let e = self.eye(partial);
            for k in 0..8 {
                let a = ((k & 1) * 2 - 1) as f32 * 0.1;
                let b = ((k >> 1 & 1) * 2 - 1) as f32 * 0.1;
                let c = ((k >> 2 & 1) * 2 - 1) as f32 * 0.1;
                let (a, b, c) = (a as f64, b as f64, c as f64);
                let hit = crate::entity::raytrace::ray_trace_blocks(
                    &self.world,
                    [e[0] + a, e[1] + b, e[2] + c],
                    [e[0] - dx + a + c, e[1] - dy + b, e[2] - dz + c],
                    false,
                    false,
                    false,
                );
                if let Some(h) = hit {
                    let q = h.vec;
                    let l = ((q[0] - e[0]).powi(2) + (q[1] - e[1]).powi(2) + (q[2] - e[2]).powi(2))
                        .sqrt();
                    if l < d {
                        d = l;
                    }
                }
            }
            if self.third_person == 2 {
                view = render::mul(&view, &render::rot_y(180.0));
            }
            view = render::mul(&view, &render::rot_x(p.pitch - pitch));
            view = render::mul(&view, &render::translate(0.0, 0.0, -d as f32));
            view = render::mul(&view, &render::rot_x(pitch - p.pitch));
        } else {
            view = render::mul(&view, &render::translate(0.0, 0.0, -0.1));
        }
        if self.sleeping.is_none() {
            view = render::mul(&view, &render::rot_x(p.pitch));
            view = render::mul(&view, &render::rot_y(p.yaw + 180.0));
        }
        let view_proj = render::mul(&proj, &view);
        let tan = (fov.to_radians() * 0.5).tan();
        let row = |i: usize, s: f32| [view[i] * s, view[4 + i] * s, view[8 + i] * s, 0.0];

        let theta = angle * math::PI * 2.0f32;
        let moon_phase = ((self.world_time / 24000) % 8) as usize;
        // --- Sky extras (RenderGlobal.renderSky / renderCloudsFancy) ---
        let star = {
            let f = (1.0f32 - (math::cos(angle * math::PI * 2.0f32) * 2.0f32 + 0.25f32))
                .clamp(0.0, 1.0);
            if self.dim != 0 {
                0.0
            } else {
                f * f * 0.5f32 * (1.0 - rain)
            }
        };
        let sunrise = {
            let f3 = math::cos(angle * math::PI * 2.0f32);
            if self.dim == 0 && (-0.4f32..=0.4f32).contains(&f3) {
                let c = f3 / 0.4f32 * 0.5f32 + 0.5f32;
                let mut a = 1.0f32 - (1.0f32 - math::sin(c * math::PI)) * 0.99f32;
                a *= a;
                Some((
                    [c * 0.3f32 + 0.7f32, c * c * 0.7f32 + 0.2f32, 0.2f32, a],
                    math::sin(angle * math::PI * 2.0f32) < 0.0,
                ))
            } else {
                None
            }
        };
        let clouds = {
            let t = self.ticks as f64 + partial as f64;
            let fancy = crate::render::mesher::rb::fancy();
            // fancy: 12 blocks a texel; fast (renderClouds): 8, no 0.33
            let (size, zoff) = if fancy {
                (12.0, 0.33f32 as f64)
            } else {
                (8.0, 0.0)
            };
            let mut cx = (eye[0] + t * 0.03f32 as f64) / size;
            let mut cz = eye[2] / size + zoff;
            cx -= (math::floor_double(cx / 2048.0) * 2048) as f64;
            cz -= (math::floor_double(cz / 2048.0) * 2048) as f64;
            let (bx, bz) = (math::floor_double(cx), math::floor_double(cz));
            if self.cloud_cell != Some((bx, bz, fancy)) {
                let mesh = if fancy {
                    self.build_clouds(bx, bz)
                } else {
                    self.build_clouds_flat(bx, bz)
                };
                self.renderer.set_clouds(&mesh);
                self.cloud_cell = Some((bx, bz, fancy));
            }
            let y = 128.0f32 - eye[1] as f32 + 0.33f32;
            let mut color = [
                day * 0.9f32 + 0.1f32,
                day * 0.9f32 + 0.1f32,
                day * 0.85f32 + 0.15f32,
            ];
            // World.getCloudColour
            if rain > 0.0 {
                let l = (color[0] * 0.3 + color[1] * 0.59 + color[2] * 0.11) * 0.6;
                for c in &mut color {
                    *c = *c * (1.0 - rain * 0.95) + l * rain * 0.95;
                }
            }
            crate::render::vk::CloudDraw {
                offset: [
                    -((cx - bx as f64) * size) as f32,
                    y,
                    -((cz - bz as f64) * size) as f32,
                ],
                color,
                before_terrain: eye[1] < 128.0,
            }
        };
        // RenderEndPortal: layer colours from Random(31100), reset every frame.
        let mut portal_layers = [[0.0f32; 4]; 16];
        {
            let mut r = crate::java::JavaRandom::new(31100);
            for (l, c) in portal_layers.iter_mut().enumerate() {
                let depth = 16.0 - l as f32;
                let mut k = 1.0 / (depth + 1.0);
                if l == 0 {
                    k = 0.1;
                }
                let mut a = r.next_float() * 0.5 + 0.1;
                let mut b = r.next_float() * 0.5 + 0.4;
                let mut d = r.next_float() * 0.5 + 0.5;
                if l == 0 {
                    a = 1.0;
                    b = 1.0;
                    d = 1.0;
                }
                *c = [a * k, b * k, d * k, 1.0];
            }
        }
        // ActiveRenderInfo.objectX/Y/Z: the screen centre on the near plane
        // (gluUnProject at depth 0), in camera-relative world space.
        let portal_obj = {
            let p = [0.0f32, 0.0, -0.05];
            let t = [view[12], view[13], view[14]];
            let d = [p[0] - t[0], p[1] - t[1], p[2] - t[2]];
            let mut o = [0.0f32; 4];
            for (c, oc) in o.iter_mut().enumerate().take(3) {
                *oc = view[c * 4] * d[0] + view[c * 4 + 1] * d[1] + view[c * 4 + 2] * d[2];
            }
            o
        };
        let portal_time = (self.started.elapsed().as_millis() % 700000) as f32 / 700000.0;
        let mut g = Globals {
            portal_layers,
            portal_cam: [eye[0] as f32, eye[1] as f32, eye[2] as f32, portal_time],
            portal_obj,
            end_sky_rect: self.atlas.rects[self.atlas.end_sky as usize],
            end_portal_rect: self.atlas.rects[self.atlas.end_portal as usize],
            view_proj,
            sky_rot: render::mul(&render::rot_y(-90.0), &render::rot_x(angle * 360.0)),
            hand_proj: render::mul(
                &render::perspective(70.0, aspect, 0.05, rd * 16.0 * 2.0),
                &shake,
            ),
            cam_right: row(0, tan * aspect),
            cam_up: row(1, tan),
            cam_fwd: row(2, -1.0),
            fog_color: [fog[0], fog[1], fog[2], 1.0],
            // RenderGlobal.renderSky: only the overworld has a sky dome; the
            // End draws its dark box, the Nether nothing but the fog.
            sky_color: match self.dim {
                -1 => [fog[0], fog[1], fog[2], 1.0],
                1 => [0.035, 0.03, 0.045, 1.0],
                _ => [sky[0], sky[1], sky[2], 1.0],
            },
            fog_params,
            sun_dir: [-theta.sin(), theta.cos(), 0.0, (eye[1] - 63.0) as f32],
            sky_params: [
                far * 0.8f32,
                if self.dim == 0 { 1.0 - rain } else { 0.0 },
                0.0,
                0.0,
            ],
            sun_rect: self.atlas.rects[self.atlas.sun as usize],
            moon_rect: self.atlas.rects[self.atlas.moon[moon_phase] as usize],
            void_color: match self.dim {
                -1 => [fog[0], fog[1], fog[2], 1.0],
                1 => [0.035, 0.03, 0.045, 1.0],
                _ => [
                    sky[0] * 0.2f32 + 0.04f32,
                    sky[1] * 0.2f32 + 0.04f32,
                    sky[2] * 0.6f32 + 0.1f32,
                    1.0,
                ],
            },
            lightmap: [[0.0; 4]; 256],
        };
        self.lightmap(&mut g.lightmap);

        // GUI scale (ScaledResolution, "Auto").
        let scale = match &self.shell {
            Some(sh) => sh.scale(&self.atlas, window_size),
            None => {
                let mut scale = 1u32;
                while window_size.0 / (scale + 1) >= 320 && window_size.1 / (scale + 1) >= 240 {
                    scale += 1;
                }
                scale
            }
        };
        // --- GUI (GuiIngame) ---
        self.fps_frames += 1;
        if self.fps_since.elapsed().as_secs_f32() >= 1.0 {
            self.fps = self.fps_frames;
            self.fps_frames = 0;
            // Minecraft.runGameLoop: debug string, then chunksUpdated = 0
            self.chunk_updates = self.renderer.sections_built - self.sections_built_mark;
            self.sections_built_mark = self.renderer.sections_built;
            self.fps_since = Instant::now();
        }
        self.gui_viewport = crate::gui::Viewport::new(window_size, scale);

        // --- Dropped items, crack overlay, hand ---
        entities::set_glint_time(self.started.elapsed().as_millis() as u64);
        let _ = entities::take_glint();
        let _ = entities::take_additive();
        let mut dynamic: Vec<crate::render::mesher::Vertex> = Vec::new();
        let mut fish_line: Vec<[f32; 3]> = Vec::new();
        let mut translucent: Vec<crate::render::mesher::Vertex> = Vec::new();
        let mut labels: Vec<crate::render::mesher::Vertex> = Vec::new();
        if !self.hide_gui {
            use crate::entity::hanging::{OFFSET_X, OFFSET_Z};
            let pointed = self.target_mob.map(|t| t.0);
            for c in &self.boats {
                let crate::entity::vehicle::Vehicle::Hang(h) = &c.v else {
                    continue;
                };
                if Some(h.id) != pointed {
                    continue;
                }
                let Some(name) = h.item.and_then(|s| s.custom_name()) else {
                    continue;
                };
                let p = h.e.pos;
                let f = self.player.pos;
                let d2 = (p[0] - f[0]).powi(2) + (p[1] - f[1]).powi(2) + (p[2] - f[2]).powi(2);
                if d2 >= 64.0 * 64.0 {
                    continue;
                }
                let k = (h.dir & 3) as usize;
                let at = [
                    (p[0] - eye[0]) as f32 + OFFSET_X[k] as f32 * 0.3,
                    (p[1] - eye[1]) as f32 - 0.25 + h.e.height + 0.5,
                    (p[2] - eye[2]) as f32 + OFFSET_Z[k] as f32 * 0.3,
                ];
                entities::living_label(
                    &mut labels,
                    &mut translucent,
                    &self.atlas,
                    &name,
                    at,
                    self.view_yaw(),
                    self.player.pitch,
                );
            }
        }
        // RenderGlobal.renderEntities: isBoundingBoxInFrustum
        let planes = render::frustum_planes(&view_proj);
        let mut boss_seen: Option<(String, f32)> = None;
        for m in &self.mobs {
            let t = partial as f64;
            let p = [
                m.prev[0] + (m.pos[0] - m.prev[0]) * t,
                m.prev[1] + (m.pos[1] - m.prev[1]) * t,
                m.prev[2] + (m.pos[2] - m.prev[2]) * t,
            ];
            let rel = [
                (p[0] - eye[0]) as f32,
                (p[1] - eye[1]) as f32,
                (p[2] - eye[2]) as f32,
            ];
            if rel[0] * rel[0] + rel[2] * rel[2] > (rd * 16.0) * (rd * 16.0) {
                continue;
            }
            // Entity.isInRangeToRenderDist: 64 blocks per block of box size
            let edge =
                ((m.bb[3] - m.bb[0]) + (m.bb[4] - m.bb[1]) + (m.bb[5] - m.bb[2])) / 3.0 * 64.0;
            if (rel[0] * rel[0] + rel[1] * rel[1] + rel[2] * rel[2]) as f64 > edge * edge {
                continue;
            }
            // the dragon has ignoreFrustumCheck
            let b = |i: usize| (m.bb[i] - eye[i % 3]) as f32;
            if m.kind != crate::entity::mob::Kind::EnderDragon
                && !render::box_in_frustum(&planes, [b(0), b(1), b(2)], [b(3), b(4), b(5)])
            {
                continue;
            }
            // Entity.getBrightnessForRender
            let ly = math::floor_double(p[1] + (m.bb[4] - m.bb[1]) * 0.66);
            let (lx, lz) = (math::floor_double(p[0]), math::floor_double(p[2]));
            let (sky, blk) = (
                self.world.light(true, lx, ly, lz),
                self.world.light(false, lx, ly, lz),
            );
            // RenderManager.renderEntityStatic: a burning entity is drawn
            // at full brightness (15728880)
            let light = if m.fire {
                [240, 240, 0, 0]
            } else {
                [(blk << 4) as u8, (sky << 4) as u8, 0, 0]
            };
            let atlas = &self.atlas;
            // RenderDragon / RenderWither: BossStatus.setBossStatus
            if matches!(m.kind, crate::entity::mob::Kind::EnderDragon | crate::entity::mob::Kind::Wither) {
                let key = if m.kind == crate::entity::mob::Kind::Wither {
                    "entity.WitherBoss.name"
                } else {
                    "entity.EnderDragon.name"
                };
                let name = if m.custom_name.is_empty() {
                    self.lang.get(key).to_string()
                } else {
                    m.custom_name.clone()
                };
                boss_seen = Some((name, m.health / m.max_health.max(1.0)));
            }
            crate::render::model::mob(
                &mut dynamic,
                &mut translucent,
                &|n| atlas.mob_tex(n),
                &|o, id, meta, mat, l| entities::block_model(o, atlas, id, meta, mat, l),
                &|o, s, mat, l| entities::render_item(o, atlas, s, mat, l),
                m,
                rel,
                partial,
                light,
            );
            // RenderLiving.renderLeash
            if let Some(l) = m.leash {
                let width = m.bb[3] - m.bb[0];
                let height = m.bb[4] - m.bb[1];
                let holder = match l {
                    crate::sim::LeashView::Player => {
                        // the client player's posY is at the eyes and its
                        // getEyeHeight 0.12
                        let pp = [
                            self.player.prev_pos[0] + (self.player.pos[0] - self.player.prev_pos[0]) * t,
                            self.player.prev_pos[1] + (self.player.pos[1] - self.player.prev_pos[1]) * t,
                            self.player.prev_pos[2] + (self.player.pos[2] - self.player.prev_pos[2]) * t,
                        ];
                        let yaw = (self.player.yaw as f64).to_radians();
                        let pitch = (self.player.pitch as f64).to_radians();
                        let (c, s, sp, cp) = (yaw.cos(), yaw.sin(), pitch.sin(), pitch.cos());
                        [
                            pp[0] - c * 0.7 - s * 0.5 * cp,
                            pp[1] + 1.62 + 0.12 * 0.7 - sp * 0.5 - 0.25,
                            pp[2] - s * 0.7 + c * 0.5 * cp,
                        ]
                    }
                    // an EntityHanging: no turn, eye height 0.425
                    crate::sim::LeashView::Knot(k) => [k[0], k[1] + 0.425 * 0.7 + 0.5 - 0.25, k[2]],
                };
                let ry = ((m.prev_body_yaw + (m.body_yaw - m.prev_body_yaw) * partial) as f64)
                    .to_radians()
                    + std::f64::consts::FRAC_PI_2;
                let (ox, oz) = (ry.cos() * width * 0.4, ry.sin() * width * 0.4);
                let start = [rel[0] + ox as f32, rel[1], rel[2] + oz as f32];
                let base_y = rel[1] - ((1.6 - height) * 0.5) as f32;
                let to = [
                    start[0] + (holder[0] - (p[0] + ox)) as f32,
                    rel[1] + (holder[1] - p[1]) as f32,
                    start[2] + (holder[2] - (p[2] + oz)) as f32,
                ];
                entities::leash_rope(&mut dynamic, atlas.named("white"), start, base_y, to, light);
            }
            // RendererLivingEntity.passSpecialRender: a named mob under the
            // crosshair, not ridden, within 64 blocks
            if !m.custom_name.is_empty()
                && !m.ridden
                && !self.hide_gui
                && self.target_mob.is_some_and(|t| t.0 == m.id)
            {
                let feet = self.player.pos;
                let d = [p[0] - feet[0], p[1] - feet[1], p[2] - feet[2]];
                if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] < 64.0 * 64.0 {
                    entities::living_label(
                        &mut labels,
                        &mut translucent,
                        atlas,
                        &m.custom_name,
                        [rel[0], rel[1] + (m.bb[4] - m.bb[1]) as f32 + 0.5, rel[2]],
                        self.view_yaw(),
                        self.player.pitch,
                    );
                }
            }
            // Render.doRenderShadowAndFire: the shadow first
            {
                use crate::entity::mob::Kind;
                // RendererLivingEntity's shadowSize per renderer
                let mut size = match m.kind {
                    Kind::Pig | Kind::Sheep | Kind::Cow | Kind::Mooshroom | Kind::Squid => 0.7,
                    Kind::Chicken => 0.3,
                    Kind::Ocelot => 0.4,
                    Kind::Horse => 0.75,
                    Kind::Spider => 1.0,
                    Kind::CaveSpider => 0.7,
                    Kind::Silverfish => 0.3,
                    Kind::Bat | Kind::Slime | Kind::MagmaCube => 0.25,
                    // RenderVillager.preRenderCallback
                    Kind::Villager if m.flags & 16 != 0 => 0.25,
                    Kind::EnderCrystal => 0.0,
                    _ => 0.5,
                };
                // EntityLiving.isChild halves it
                if m.flags & 16 != 0 {
                    size *= 0.5;
                }
                let height = m.bb[4] - m.bb[1];
                self.draw_shadow(&mut translucent, p, height / 2.0, size, 1.0, eye);
            }
            if m.fire {
                entities::on_fire(
                    &mut dynamic,
                    atlas,
                    rel,
                    (m.bb[3] - m.bb[0]) as f32,
                    (m.bb[4] - m.bb[1]) as f32,
                    0.0,
                    self.view_yaw(),
                    light,
                );
            }
        }
        if let Some((n, sc)) = boss_seen {
            self.boss = Some((n, sc, 100));
        }
        {
            let w = &self.world;
            let lf = |x: i32, y: i32, z: i32| (w.light(true, x, y, z), w.light(false, x, y, z));
            self.particles.render(
                &mut dynamic,
                &mut translucent,
                &lf,
                eye,
                self.player.yaw,
                self.player.pitch,
                partial,
                self.atlas.mob_tex("../particle/particles"),
                self.atlas.mob_tex("explosion"),
            );
        }
        self.draw_boats(&mut dynamic, eye, partial);
        self.draw_chests(&mut dynamic, eye, partial);
        self.draw_signs(&mut dynamic, eye);
        // TileEntitySkullRenderer
        for (&p, &(kind, rot)) in &self.skulls {
            let w = &self.world;
            if w.peek_block(p[0], p[1], p[2]) != block::SKULL {
                continue;
            }
            let d = [p[0] as f64 + 0.5 - eye[0], p[1] as f64 + 0.5 - eye[1], p[2] as f64 + 0.5 - eye[2]];
            if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] > 64.0 * 64.0 {
                continue;
            }
            let tex = if kind == 3 {
                Some(self.atlas.skin_tex())
            } else {
                self.atlas.mob_tex(render::hanging::skull_texture(kind))
            };
            let Some(tex) = tex else { continue };
            let (sky, blk) = (w.light(true, p[0], p[1], p[2]), w.light(false, p[0], p[1], p[2]));
            let rel = [
                (p[0] as f64 - eye[0]) as f32,
                (p[1] as f64 - eye[1]) as f32,
                (p[2] as f64 - eye[2]) as f32,
            ];
            let side = (w.peek_meta(p[0], p[1], p[2]) & 7) as i32;
            render::hanging::skull(
                &mut dynamic,
                tex,
                rel,
                side,
                rot as f32 * 360.0 / 16.0,
                [(blk << 4) as u8, (sky << 4) as u8, 0, 0],
            );
        }
        self.draw_tables(&mut dynamic, eye, partial);
        self.draw_player(&mut dynamic, partial);
        self.draw_orbs(&mut translucent, eye, partial);
        // TileEntityBeaconRenderer (within 256 blocks)
        if let Some(t) = self.atlas.mob_tex("beacon_beam") {
            let time = self.ticks as f64 + partial as f64;
            for b in self.beacons.iter_mut() {
                let p = b.0;
                let rel = [
                    (p[0] as f64 - eye[0]) as f32,
                    (p[1] as f64 - eye[1]) as f32,
                    (p[2] as f64 - eye[2]) as f32,
                ];
                if rel[0] * rel[0] + rel[1] * rel[1] + rel[2] * rel[2] > 65536.0 {
                    continue;
                }
                // shouldBeamRender: eases in by 0.025 a frame
                b.1 = (b.1 + 0.025).min(1.0);
                crate::render::hanging::beacon_beam(&mut dynamic, &mut translucent, t.rect, rel, time, b.1);
            }
        }
        // Render.doRenderShadowAndFire for the other entities: boats,
        // minecarts, primed TNT and falling blocks (shadowSize 0.5,
        // getShadowSize 0), experience orbs (0.15 at 0.75, 0.5 high)
        {
            use crate::entity::vehicle::Vehicle;
            let mut shadows: Vec<([f64; 3], f64, f32, f32)> = Vec::new();
            let tp = partial as f64;
            for c in &self.boats {
                let e = c.v.e();
                let p = [
                    e.last_tick[0] + (e.pos[0] - e.last_tick[0]) * tp,
                    e.last_tick[1] + (e.pos[1] - e.last_tick[1]) * tp,
                    e.last_tick[2] + (e.pos[2] - e.last_tick[2]) * tp,
                ];
                match &c.v {
                    Vehicle::Boat(_) | Vehicle::Cart(_) => shadows.push((p, 0.0, 0.5, 1.0)),
                    Vehicle::Orb(o) => shadows.push((p, o.e.height as f64 / 2.0, 0.15, 0.75)),
                    _ => {}
                }
            }
            for &(pos, prev, _) in &self.tnt {
                let p = [
                    prev[0] + (pos[0] - prev[0]) * tp,
                    prev[1] + (pos[1] - prev[1]) * tp,
                    prev[2] + (pos[2] - prev[2]) * tp,
                ];
                shadows.push((p, 0.0, 0.5, 1.0));
            }
            for f in &self.falling {
                let p = [
                    f.prev[0] + (f.pos[0] - f.prev[0]) * tp,
                    f.prev[1] + (f.pos[1] - f.prev[1]) * tp,
                    f.prev[2] + (f.pos[2] - f.prev[2]) * tp,
                ];
                shadows.push((p, 0.0, 0.5, 1.0));
            }
            for (p, sh, size, op) in shadows {
                self.draw_shadow(&mut translucent, p, sh, size, op, eye);
            }
        }
        let sp = self.server_partial();
        if let Some(at) = self.atlas.mob_tex("arrow") {
            for a in &self.arrows {
                let t = sp as f64;
                let p = [
                    a.prev[0] + (a.pos[0] - a.prev[0]) * t,
                    a.prev[1] + (a.pos[1] - a.prev[1]) * t,
                    a.prev[2] + (a.pos[2] - a.prev[2]) * t,
                ];
                let rel = [
                    (p[0] - eye[0]) as f32,
                    (p[1] - eye[1]) as f32,
                    (p[2] - eye[2]) as f32,
                ];
                let (lx, ly, lz) = (
                    math::floor_double(p[0]),
                    math::floor_double(p[1]),
                    math::floor_double(p[2]),
                );
                let light = [
                    (self.world.light(false, lx, ly, lz) << 4) as u8,
                    (self.world.light(true, lx, ly, lz) << 4) as u8,
                    0,
                    0,
                ];
                let light = if a.burning { [240, 240, 0, 0] } else { light };
                crate::render::model::arrow(&mut dynamic, at, rel, a.yaw, a.pitch, light);
                if a.burning {
                    entities::on_fire(
                        &mut dynamic,
                        &self.atlas,
                        rel,
                        0.5,
                        0.5,
                        0.0,
                        self.view_yaw(),
                        light,
                    );
                }
            }
        }
        let fire_tex = self.atlas.mob_tex("../items/fireball");
        let eye_tex = self.atlas.mob_tex("../items/ender_eye");
        {
            for &(pos, prev, kind) in &self.fireballs {
                // RenderWitherSkull
                if kind == 3 || kind == 4 {
                    let name = if kind == 4 { "wither/wither_invulnerable" } else { "wither/wither" };
                    if let Some(t) = self.atlas.mob_tex(name) {
                        let tp = sp as f64;
                        let c = [
                            (prev[0] + (pos[0] - prev[0]) * tp - eye[0]) as f32,
                            (prev[1] + (pos[1] - prev[1]) * tp - eye[1]) as f32,
                            (prev[2] + (pos[2] - prev[2]) * tp - eye[2]) as f32,
                        ];
                        let d = [pos[0] - prev[0], pos[1] - prev[1], pos[2] - prev[2]];
                        let yaw = (d[0].atan2(d[2]).to_degrees()) as f32;
                        let pitch = (d[1].atan2((d[0] * d[0] + d[2] * d[2]).sqrt()).to_degrees()) as f32;
                        crate::render::hanging::wither_skull(&mut dynamic, t, c, yaw, pitch);
                    }
                    continue;
                }
                let Some(t) = (if kind == 2 { eye_tex } else { fire_tex }) else {
                    continue;
                };
                let tp = sp as f64;
                let c = [
                    (prev[0] + (pos[0] - prev[0]) * tp - eye[0]) as f32,
                    (prev[1] + (pos[1] - prev[1]) * tp - eye[1]) as f32,
                    (prev[2] + (pos[2] - prev[2]) * tp - eye[2]) as f32,
                ];
                let rect = [
                    t.rect[0],
                    t.rect[1],
                    t.rect[0] + (t.rect[2] - t.rect[0]) * 16.0 / t.size,
                    t.rect[1] + (t.rect[3] - t.rect[1]) * 16.0 / t.size,
                ];
                let size = match kind {
                    0 => 2.0,
                    1 => 0.5,
                    _ => 0.5,
                };
                entities::billboard(
                    &mut dynamic,
                    rect,
                    c,
                    size,
                    self.player.yaw,
                    self.player.pitch,
                );
            }
        }
        for &(pos, prev, fuse) in &self.tnt {
            let t = sp as f64;
            let p = [
                prev[0] + (pos[0] - prev[0]) * t,
                prev[1] + (pos[1] - prev[1]) * t,
                prev[2] + (pos[2] - prev[2]) * t,
            ];
            let (lx, ly, lz) = (
                math::floor_double(p[0]),
                math::floor_double(p[1]),
                math::floor_double(p[2]),
            );
            let light = (
                self.world.light(true, lx, ly, lz),
                self.world.light(false, lx, ly, lz),
            );
            entities::primed_tnt(
                &mut dynamic,
                &self.atlas,
                [
                    (p[0] - eye[0]) as f32,
                    (p[1] - eye[1]) as f32,
                    (p[2] - eye[2]) as f32,
                ],
                fuse,
                sp,
                light,
            );
        }
        // RenderSnowball
        for &(pos, prev, id, meta) in &self.throwables {
            let t = sp as f64;
            let p = [
                prev[0] + (pos[0] - prev[0]) * t,
                prev[1] + (pos[1] - prev[1]) * t,
                prev[2] + (pos[2] - prev[2]) * t,
            ];
            let (lx, ly, lz) = (
                math::floor_double(p[0]),
                math::floor_double(p[1]),
                math::floor_double(p[2]),
            );
            let light = (
                self.world.light(true, lx, ly, lz),
                self.world.light(false, lx, ly, lz),
            );
            let rel = [
                (p[0] - eye[0]) as f32,
                (p[1] - eye[1]) as f32,
                (p[2] - eye[2]) as f32,
            ];
            // potions: getIconFromDamage(16384), the splash bottle
            let m = if id == 373 { meta | 16384 } else { meta };
            for (rect, col) in self.atlas.item_passes(id, m) {
                entities::billboard_lit(
                    &mut dynamic,
                    rect,
                    rel,
                    0.5,
                    self.player.yaw,
                    self.player.pitch,
                    light,
                    col,
                );
            }
        }
        self.draw_fish_hook(&mut dynamic, &mut fish_line, eye, sp);
        // RenderFallingBlock
        for f in &self.falling {
            let t = sp as f64;
            let p = [
                f.prev[0] + (f.pos[0] - f.prev[0]) * t,
                f.prev[1] + (f.pos[1] - f.prev[1]) * t,
                f.prev[2] + (f.pos[2] - f.prev[2]) * t,
            ];
            // posX/Y/Z, not interpolated, choose the light and the skip test
            let (bx, by, bz) = (
                math::floor_double(f.pos[0]),
                math::floor_double(f.pos[1]),
                math::floor_double(f.pos[2]),
            );
            if self.world.peek_block(bx, by, bz) == f.block {
                continue;
            }
            if f.block == block::ANVIL {
                // renderBlockAnvilMetadata with the world as blockAccess,
                // translated by -(block + 0.5)
                use crate::render::mesher::{build_single, MovingDraw};
                let (cx, cz) = (bx >> 4, bz >> 4);
                let mut v = Vec::with_capacity(9);
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        if let Some(c) = self.world.chunks.get(&(cx + dx, cz + dz)) {
                            v.push(c.mc.clone());
                        }
                    }
                }
                let Ok(nine) = <[crate::render::mesher::MeshChunk; 9]>::try_from(v) else {
                    continue;
                };
                let verts = build_single(
                    &nine,
                    &self.atlas,
                    &self.noise,
                    [bx, by, bz],
                    f.block,
                    f.meta as i32,
                    MovingDraw::Anvil,
                );
                let o = [
                    (cx * 16 - bx) as f64 - 0.5 + p[0] - eye[0],
                    -(by as f64) - 0.5 + p[1] - eye[1],
                    (cz * 16 - bz) as f64 - 0.5 + p[2] - eye[2],
                ];
                for mut w in verts {
                    w.pos = [
                        (w.pos[0] as f64 + o[0]) as f32,
                        (w.pos[1] as f64 + o[1]) as f32,
                        (w.pos[2] as f64 + o[2]) as f32,
                    ];
                    dynamic.push(w);
                }
                continue;
            }
            let lv = block::info(f.block).light_value as i32;
            let bright = self.world.light(true, bx, by, bz) << 20
                | self.world.light(false, bx, by, bz).max(lv) << 4;
            let start = dynamic.len();
            crate::render::mesher::rb::Rb::bare(&self.atlas, &mut dynamic, f.block, f.meta as i32)
                .render_sand_falling(bright);
            for v in &mut dynamic[start..] {
                v.pos = [
                    (v.pos[0] as f64 + p[0] - eye[0]) as f32,
                    (v.pos[1] as f64 + p[1] - eye[1]) as f32,
                    (v.pos[2] as f64 + p[2] - eye[2]) as f32,
                ];
            }
        }
        // TileEntityRendererPiston
        self.render_moving(&mut dynamic, eye, sp);
        // EntityPickupFX.renderParticle
        for (it, age, mob) in &self.pickups {
            let mut f = (*age as f32 + partial) / 3.0;
            f *= f;
            let t = partial as f64;
            let d = it.pos;
            let mut pl = [
                self.player.prev_pos[0] + (self.player.pos[0] - self.player.prev_pos[0]) * t,
                self.player.prev_pos[1] + (self.player.pos[1] - self.player.prev_pos[1]) * t - 0.5,
                self.player.prev_pos[2] + (self.player.pos[2] - self.player.prev_pos[2]) * t,
            ];
            if let Some(id) = mob {
                match self.mobs.iter().find(|m| m.id == *id) {
                    Some(m) => {
                        pl = [
                            m.prev[0] + (m.pos[0] - m.prev[0]) * t,
                            m.prev[1] + (m.pos[1] - m.prev[1]) * t - 0.5,
                            m.prev[2] + (m.pos[2] - m.prev[2]) * t,
                        ]
                    }
                    None => pl = d,
                }
            }
            let p = [
                d[0] + (pl[0] - d[0]) * f as f64,
                d[1] + (pl[1] - d[1]) * f as f64,
                d[2] + (pl[2] - d[2]) * f as f64,
            ];
            let (lx, ly, lz) = (
                math::floor_double(p[0]),
                math::floor_double(p[1]),
                math::floor_double(p[2]),
            );
            let light = (
                self.world.light(true, lx, ly, lz),
                self.world.light(false, lx, ly, lz),
            );
            entities::item_entity(
                &mut dynamic,
                &self.atlas,
                &it.stack,
                [
                    (p[0] - eye[0]) as f32,
                    (p[1] - eye[1]) as f32,
                    (p[2] - eye[2]) as f32,
                ],
                it.age as f32 + partial,
                it.hover,
                self.player.yaw,
                light,
            );
        }
        for it in &self.items {
            let t = sp as f64;
            let p = [
                it.prev[0] + (it.pos[0] - it.prev[0]) * t,
                it.prev[1] + (it.pos[1] - it.prev[1]) * t,
                it.prev[2] + (it.pos[2] - it.prev[2]) * t,
            ];
            let rel = [
                (p[0] - eye[0]) as f32,
                (p[1] - eye[1]) as f32,
                (p[2] - eye[2]) as f32,
            ];
            if rel[0] * rel[0] + rel[2] * rel[2] > (rd * 16.0) * (rd * 16.0) {
                continue;
            }
            let (lx, ly, lz) = (
                math::floor_double(p[0]),
                math::floor_double(p[1]),
                math::floor_double(p[2]),
            );
            let light = (
                self.world.light(true, lx, ly, lz),
                self.world.light(false, lx, ly, lz),
            );
            entities::item_entity(
                &mut dynamic,
                &self.atlas,
                &it.stack,
                [rel[0], rel[1], rel[2]],
                it.age as f32 + sp,
                it.hover,
                self.player.yaw,
                light,
            );
            // RenderItem: shadowSize 0.15, shadowOpaque 0.75
            self.draw_shadow(&mut translucent, p, 0.125, 0.15, 0.75, eye);
        }
        let mut crack: Vec<crate::render::mesher::Vertex> = Vec::new();
        if let Some(d) = self.dig {
            let stage = (d.damage * 10.0) as i32 - 1;
            if stage >= 0 {
                let id = self.world.peek_block(d.pos[0], d.pos[1], d.pos[2]);
                let w = &self.world;
                let get = |x: i32, y: i32, z: i32| (w.peek_block(x, y, z), w.peek_meta(x, y, z));
                let b = crate::shapes::selection(
                    &get,
                    id,
                    w.peek_meta(d.pos[0], d.pos[1], d.pos[2]),
                    d.pos[0],
                    d.pos[1],
                    d.pos[2],
                );
                let rel = [
                    (d.pos[0] as f64 - eye[0]) as f32,
                    (d.pos[1] as f64 - eye[1]) as f32,
                    (d.pos[2] as f64 - eye[2]) as f32,
                ];
                entities::crack(&mut crack, &self.atlas, stage as usize, rel, b);
            }
        }
        // RenderGlobal.drawBlockDamageTexture: the other breakers' blocks
        // (a zombie at a door), dropped beyond 32 blocks
        let mut far = Vec::new();
        for (&id, &(p, k, _)) in &self.damaged_blocks {
            let rel = [
                p[0] as f64 - eye[0],
                p[1] as f64 - eye[1],
                p[2] as f64 - eye[2],
            ];
            if rel[0] * rel[0] + rel[1] * rel[1] + rel[2] * rel[2] > 1024.0 {
                far.push(id);
                continue;
            }
            let bid = self.world.peek_block(p[0], p[1], p[2]);
            if block::material(bid) == crate::block::Material::Air {
                continue;
            }
            let w = &self.world;
            let get = |x: i32, y: i32, z: i32| (w.peek_block(x, y, z), w.peek_meta(x, y, z));
            let b = crate::shapes::selection(&get, bid, w.peek_meta(p[0], p[1], p[2]), p[0], p[1], p[2]);
            let rel = [rel[0] as f32, rel[1] as f32, rel[2] as f32];
            entities::crack(&mut crack, &self.atlas, k as usize, rel, b);
        }
        for id in far {
            self.damaged_blocks.remove(&id);
        }
        let world_glint = entities::take_glint();
        let mut hand: Vec<crate::render::mesher::Vertex> = Vec::new();
        // EntityRenderer.renderHand: first person, GUI shown
        // EntityRenderer.renderHand: not while sleeping
        if !self.stats.dead
            && self.third_person == 0
            && self.sleeping.is_none()
            && !self.hide_gui
            && self.loading == 2
        {
            let (lx, ly, lz) = (
                math::floor_double(eye[0]),
                math::floor_double(eye[1]),
                math::floor_double(eye[2]),
            );
            let light = (
                self.world.light(true, lx, ly, lz),
                self.world.light(false, lx, ly, lz),
            );
            let equip = self.prev_equip + (self.equip - self.prev_equip) * partial;
            let swing = self.swing_progress(partial);
            let using = self.using.map(|(st, n)| {
                (
                    crate::item_use::action(st.id, st.meta),
                    n as f32 - partial + 1.0,
                    crate::item_use::max_duration(st.id, st.meta),
                )
            });
            let held_map = self
                .equipped_icon()
                .filter(|s| s.id == 358)
                .and_then(|s| self.map_draw(s.meta));
            entities::hand(
                &mut hand,
                &self.atlas,
                self.equipped_icon(),
                equip,
                swing,
                light,
                using,
                (self.player.pitch, self.player.yaw),
                (
                    self.player.pitch
                        - (self.prev_render_arm[0]
                            + (self.render_arm[0] - self.prev_render_arm[0]) * partial),
                    self.player.yaw
                        - (self.prev_render_arm[1]
                            + (self.render_arm[1] - self.prev_render_arm[1]) * partial),
                ),
                held_map.as_ref(),
            );
        }

        self.anvil_sync();
        let ui_atlas = self.atlas.clone();
        let (w, h) = self.gui_size();
        let mut ui = crate::gui::Ui::new(&ui_atlas, w, h);
        // updateCameraAndRender: no GuiIngame with hideGUI and no screen
        let hud_hidden = self.hide_gui && self.screen == Screen::Playing;
        // GuiIngame.renderGameOverlay begins with renderVignette (fancy
        // graphics only): the screen's edges darken as the player's own
        // spot does, easing in by a hundredth a frame
        if !hud_hidden && self.loading == 2 && self.settings().fancy_graphics {
            let mut b = 1.0 - self.player_brightness();
            b = b.clamp(0.0, 1.0);
            self.vignette = (self.vignette as f64 + (b - self.vignette) as f64 * 0.01) as f32;
            if let Some(t) = self.atlas.mob_tex("../misc/vignette") {
                let c = (self.vignette * 255.0) as u32 & 0xFF;
                ui.atlas_rect(0.0, 0.0, ui.w, ui.h, t.rect, c << 16 | c << 8 | c, 1.0);
                ui.vignette = ui.v.len() as u32;
            }
        }
        if self.loading < 2 {
            // the integrated server's spawn area first, then the chunks
            // around the player drawn
            let (cx, cz) = self.player_chunk();
            let near = || (-2..=2).flat_map(move |dx| (-2..=2).map(move |dz| (cx + dx, cz + dz)));
            if self.loading == 0 && near().all(|k| self.world.chunks.contains_key(&k)) {
                self.loading = 1;
            }
            if self.loading == 1 && near().all(|k| self.renderer.has_mesh(k)) {
                self.loading = 2;
            }
        }
        // ItemRenderer.renderWarpedTextureOverlay: the water texture, repeated four
        // times across the screen and scrolled by the view direction.
        if eye_mat == Material::Water {
            if let Some(t) = self.atlas.mob_tex("../misc/underwater") {
                let (sw, sh) = (ui.w, ui.h);
                let (lx, ly, lz) = (
                    math::floor_double(eye[0]),
                    math::floor_double(eye[1]),
                    math::floor_double(eye[2]),
                );
                let l = self
                    .world
                    .light(true, lx, ly, lz)
                    .max(self.world.light(false, lx, ly, lz));
                let f = 1.0 - l as f32 / 15.0;
                let br = (1.0 - f) / (f * 3.0 + 1.0);
                let g = (br.clamp(0.0, 1.0) * 255.0) as u32;
                let color = g << 16 | g << 8 | g;
                let su = (-self.player.yaw / 64.0).rem_euclid(1.0);
                let sv = (self.player.pitch / 64.0).rem_euclid(1.0);
                let (cw, ch) = (sw / 4.0, sh / 4.0);
                let (tw, th) = (
                    (t.rect[2] - t.rect[0]) * 16.0 / t.size,
                    (t.rect[3] - t.rect[1]) * 16.0 / t.size,
                );
                for i in -1..5 {
                    for j in -1..5 {
                        let mut x0 = (i as f32 - su) * cw;
                        let mut y0 = (j as f32 - sv) * ch;
                        let (mut x1, mut y1) = (x0 + cw, y0 + ch);
                        let (mut u0, mut v0, mut u1, mut v1) = (0.0f32, 0.0f32, 1.0f32, 1.0f32);
                        if x0 < 0.0 {
                            u0 = -x0 / cw;
                            x0 = 0.0;
                        }
                        if y0 < 0.0 {
                            v0 = -y0 / ch;
                            y0 = 0.0;
                        }
                        if x1 > sw {
                            u1 = 1.0 - (x1 - sw) / cw;
                            x1 = sw;
                        }
                        if y1 > sh {
                            v1 = 1.0 - (y1 - sh) / ch;
                            y1 = sh;
                        }
                        if x1 <= x0 || y1 <= y0 {
                            continue;
                        }
                        let uv = [
                            t.rect[0] + u0 * tw,
                            t.rect[1] + v0 * th,
                            t.rect[0] + u1 * tw,
                            t.rect[1] + v1 * th,
                        ];
                        ui.atlas_rect(x0, y0, x1 - x0, y1 - y0, uv, color, 0.5);
                    }
                }
            }
        }
        // ItemRenderer.renderFireInFirstPerson (renderOverlays, after the
        // hand, without view bobbing): two tilted squares of fire_layer_1
        // half a block ahead, at 0.9 alpha, seen through the hand's
        // 70-degree projection
        if self.third_person == 0
            && self.sleeping.is_none()
            && self.stats.fire > 0
            && !self.stats.dead
        {
            let t = self.atlas.named("fire_layer_1");
            let aspect = ui.w / ui.h;
            let tan = (35.0f32).to_radians().tan();
            let (sw, sh) = (ui.w, ui.h);
            let project = |p: [f32; 3]| -> [f32; 2] {
                let d = -p[2];
                [
                    (p[0] / d / (tan * aspect) * 0.5 + 0.5) * sw,
                    (0.5 - p[1] / d / tan * 0.5) * sh,
                ]
            };
            for k in 0..2 {
                let side = (k * 2 - 1) as f32;
                let tx = -side * 0.24;
                let a = (side * 10.0f32).to_radians();
                let (sa, ca) = (a.sin(), a.cos());
                // glTranslatef(tx, -0.3, 0) then glRotatef(a, 0, 1, 0)
                let place = |x: f32, y: f32, z: f32| -> [f32; 2] {
                    project([x * ca + z * sa + tx, y - 0.3, -x * sa + z * ca])
                };
                let (x0, x1, y0, y1, z) = (-0.5f32, 0.5f32, -0.5f32, 0.5f32, -0.5f32);
                ui.atlas_quad(
                    [
                        place(x0, y1, z),
                        place(x0, y0, z),
                        place(x1, y0, z),
                        place(x1, y1, z),
                    ],
                    [[t[2], t[1]], [t[2], t[3]], [t[0], t[3]], [t[0], t[1]]],
                    0xFFFFFF,
                    0.9,
                );
            }
        }
        // GuiIngame.renderPumpkinBlur
        if self.third_person == 0 && self.view.inv.armor[3].is_some_and(|s| s.id == 86) {
            if let Some(t) = self.atlas.mob_tex("../misc/pumpkinblur") {
                ui.atlas_rect(0.0, 0.0, ui.w, ui.h, t.rect, 0xFFFFFF, 1.0);
            }
        }
        // GuiIngame.func_130015_b: the purple portal swirl
        let pt = self.prev_portal_time + (self.portal_time - self.prev_portal_time) * partial;
        if pt > 0.0 {
            if let Some(t) = self.atlas.tile(crate::block::PORTAL, 1, 2) {
                let mut a = pt;
                if a < 1.0 {
                    a *= a;
                    a *= a;
                    a = a * 0.8 + 0.2;
                }
                ui.atlas_rect(0.0, 0.0, ui.w, ui.h, t, 0xFFFFFF, a);
            }
        }
        // GuiIngame: recordPlaying
        if let Some((text, up_for, playing)) = &self.record {
            let f = *up_for as f32 - partial;
            let a = ((f * 255.0 / 20.0) as i32).min(255);
            if a > 8 {
                let color = if *playing {
                    crate::gui::hsb_to_rgb(f / 50.0, 0.7, 0.6) & 0xFFFFFF
                } else {
                    0xFFFFFF
                };
                let w = ui.text_width(text);
                let x = (ui.w / 2.0).floor() - (w as i32 / 2) as f32;
                let y = ui.h - 68.0 - 4.0;
                ui.text_alpha(text, x, y, color, a as f32 / 255.0);
            }
        }
        screens::hotbar(&mut ui, &self.view.inv);
        // GuiIngame.renderBossHealth
        if let Some((name, scale, time)) = self.boss.as_mut() {
            if *time > 0 {
                *time -= 1;
                let x = (ui.w / 2.0).trunc() - 91.0;
                let filled = (*scale * 183.0) as i32;
                ui.icon(x, 12.0, 0.0, 74.0, 182.0, 5.0);
                ui.icon(x, 12.0, 0.0, 74.0, 182.0, 5.0);
                if filled > 0 {
                    ui.icon(x, 12.0, 0.0, 79.0, filled as f32, 5.0);
                }
                let w = ui.text_width(name);
                let tx = (ui.w / 2.0).trunc() - (w / 2.0).trunc();
                let n = name.clone();
                ui.text(&n, tx, 2.0, 0xFFFFFF, true);
            }
        }
        if self.mode == GameMode::Survival {
            let under = eye_mat == Material::Water;
            let mut jitter = std::mem::replace(&mut self.jitter, JavaRandom::new(0));
            jitter.set_seed(self.ticks as i64 * 312871);
            let mount = self.riding.and_then(|id| {
                self.mobs.iter().find(|m| m.id == id).map(|m| (m.health, m.max_health))
            });
            screens::survival_bars(
                &mut ui,
                &self.stats,
                self.ticks as u32,
                &mut jitter,
                under,
                self.riding_horse(),
                mount,
            );
            self.jitter = jitter;
        }
        if self.riding_horse() {
            screens::jump_bar(&mut ui, self.horse_jump_power);
        }
        // GuiIngame: the sleep fade
        if self.sleep_timer > 0 {
            let t = self.sleep_timer;
            let mut f = t as f32 / 100.0;
            if f > 1.0 {
                f = 1.0 - (t - 100) as f32 / 10.0;
            }
            let a = ((220.0 * f) as i32) as f32 / 255.0;
            ui.rect(0.0, 0.0, ui.w, ui.h, 1052704, a);
        }
        // GuiIngame: persistantChatGUI.drawChat(updateCounter)
        {
            let st = self.settings();
            self.chat
                .draw(&mut ui, &st, self.ticks as i32, self.screen == Screen::Chat);
        }
        if hud_hidden {
            ui.v.clear();
            ui.glint.clear();
        }
        if self.loading < 2 {
            ui.v.clear();
            ui.glint.clear();
            screens::dirt_background(&mut ui);
            let (w, h) = (ui.w, ui.h);
            if self.loading == 0 {
                // LoadingScreenRenderer: menu.loadingLevel, menu.generatingTerrain
                let title = self.lang.get("menu.loadingLevel").to_string();
                let msg = self.lang.get("menu.generatingTerrain").to_string();
                let tw = ui.text_width(&title);
                ui.text(
                    &title,
                    ((w - tw) / 2.0).floor(),
                    (h / 2.0).floor() - 4.0 - 16.0,
                    0xFFFFFF,
                    true,
                );
                let mw = ui.text_width(&msg);
                ui.text(
                    &msg,
                    ((w - mw) / 2.0).floor(),
                    (h / 2.0).floor() - 4.0 + 8.0,
                    0xFFFFFF,
                    true,
                );
            } else {
                // GuiDownloadTerrain
                let t = self.lang.get("multiplayer.downloadingTerrain").to_string();
                ui.centered(&t, (w / 2.0).floor(), (h / 2.0).floor() - 50.0, 0xFFFFFF);
            }
        }
        self.renderer.debug_rd = self.render_distance;
        if self.show_debug {
            self.debug_overlay(&mut ui, &g, eye);
        }
        let m = if let Some(mouse) = std::env::var("MINERUST_MOUSE")
            .ok()
            .and_then(|v| {
                v.split_once(',')
                    .map(|(a, b)| (a.parse::<f32>(), b.parse::<f32>()))
            })
            .and_then(|(a, b)| Some((a.ok()?, b.ok()?)))
        {
            self.mouse_px = self.gui_viewport.physical_pointer(mouse);
            mouse
        } else {
            self.gui_mouse()
        };
        match self.screen {
            Screen::Window if self.creative.is_some() => {
                let lines = self.effect_lines();
                let dims = (ui.w as i32, ui.h as i32, !lines.is_empty());
                let mouse = (m.0.floor() as i32, m.1.floor() as i32);
                let age = self.ticks as f32 + partial;
                let mut c = self.creative.take().unwrap();
                c.draw(&mut ui, &self.view, dims, mouse, age, &self.lang, &lines);
                self.creative = Some(c);
            }
            Screen::Window => {
                let lines = self.effect_lines();
                let frame = crate::gui_container::Frame {
                    layout: None,
                    view: &self.view,
                    w: ui.w as i32,
                    h: ui.h as i32,
                    effects: !lines.is_empty(),
                    mouse: (m.0.floor() as i32, m.1.floor() as i32),
                    age: self.ticks as f32 + partial,
                    ench: Some(self.enchant_state(partial)),
                };
                self.gui.draw(&mut ui, &frame, &self.lang, &lines);
                if let Some(a) = &self.anvil {
                    a.draw(&mut ui);
                }
            }
            Screen::Paused => {
                self.draw_menu(&mut ui, partial);
            }
            Screen::Dead => {
                screens::death_screen(&mut ui, &self.stats, m);
            }
            Screen::Chat => {
                if let Some(c) = self.chat_screen.as_mut() {
                    c.draw(&mut ui);
                }
                if self.sleep_gui {
                    let label = self.lang.get("multiplayer.stopSleeping").to_string();
                    let (w, h) = (ui.w, ui.h);
                    screens::button(
                        &mut ui,
                        (w / 2.0).floor() - 100.0,
                        h - 40.0,
                        200.0,
                        &label,
                        true,
                        m,
                    );
                }
            }
            Screen::Sign => self.draw_sign_editor(&mut ui, m),
            Screen::Book => {
                let w = ui.w;
                if let Some(b) = self.book_gui.as_ref() {
                    b.draw(&mut ui, &self.lang, w, m);
                }
            }
            Screen::Playing => {}
        }
        // Minecraft.runGameLoop: guiAchievement.updateAchievementWindow, last
        self.gui_achievement.draw(&mut ui, true);
        let ui_glint = std::mem::take(&mut ui.glint);
        let ui_vignette = ui.vignette;
        let ui_verts = ui.v;
        // RenderGlobal.drawSelectionBox: getSelectedBoundingBoxFromPool
        // (EntityRenderer.renderWorld skips it while F1 hides the GUI)
        let outline = if self.screen == Screen::Playing && !self.hide_gui {
            self.selected.map(|(p, _)| {
                let w = &self.world;
                let get = |x: i32, y: i32, z: i32| (w.peek_block(x, y, z), w.peek_meta(x, y, z));
                let (id, meta) = get(p[0], p[1], p[2]);
                let b = crate::shapes::selection(&get, id, meta, p[0], p[1], p[2]);
                [
                    p[0] as f64 + b[0] as f64,
                    p[1] as f64 + b[1] as f64,
                    p[2] as f64 + b[2] as f64,
                    p[0] as f64 + b[3] as f64,
                    p[1] as f64 + b[4] as f64,
                    p[2] as f64 + b[5] as f64,
                ]
            })
        } else {
            None
        };
        let mut weather = self.weather_quads(eye, partial);
        weather.extend_from_slice(&translucent);
        if std::env::var_os("MINERUST_DEBUG_WEATHER").is_some() && self.ticks % 40 == 0 {
            eprintln!(
                "weather: rain {} quads {} tex {:?}",
                self.rain(partial),
                weather.len() / 8,
                self.atlas.mob_tex("../environment/rain").map(|t| t.rect)
            );
        }
        // TileEntityEndPortal: drawn within 64 blocks.
        let mut end_portals: Vec<crate::render::mesher::Vertex> = Vec::new();
        for c in self.world.chunks.values() {
            for &[x, y, z] in &c.portals {
                let d = [
                    x as f64 + 0.5 - eye[0],
                    y as f64 + 0.5 - eye[1],
                    z as f64 + 0.5 - eye[2],
                ];
                if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] >= 4096.0 {
                    continue;
                }
                let (bx, by, bz) = (
                    (x as f64 - eye[0]) as f32,
                    (y as f64 - eye[1] + 0.75) as f32,
                    (z as f64 - eye[2]) as f32,
                );
                for (dx, dz) in [(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0)] {
                    end_portals.push(crate::render::mesher::Vertex {
                        pos: [bx + dx, by, bz + dz],
                        uv: [0.0; 2],
                        color: [255; 4],
                        light: [0; 4],
                    });
                }
            }
        }
        let hand_glint = entities::take_glint();
        // RenderLightningBolt, after the charged creepers' swirls
        let mut additive = entities::take_additive();
        for b in &self.bolts {
            if b.1 >= 0 {
                let rel = [
                    (b.0[0] - eye[0]) as f32,
                    (b.0[1] - eye[1]) as f32,
                    (b.0[2] - eye[2]) as f32,
                ];
                crate::render::hanging::lightning(&mut additive, self.atlas.named("white"), rel, b.3);
            }
        }
        let frame = Frame {
            additive: &additive,
            glint: &world_glint,
            hand_glint: &hand_glint,
            end_portals: &end_portals,
            labels: &labels,
            crosshair: self.screen == Screen::Playing && !self.hide_gui && self.loading == 2,
            dynamic: &dynamic,
            crack: &crack,
            hand: &hand,
            weather: &weather,
            ui: &ui_verts,
            ui_glint: &ui_glint,
            ui_vignette,
            glint_rect: self.atlas.named("item_glint"),
            globals: &g,
            cam: eye,
            outline,
            fish_line: &fish_line,
            gui_scale: scale as f32,
            screenshot: self.screenshot.take(),
            star_brightness: star,
            sunrise,
            // GameSettings.shouldRenderClouds, WorldProvider.isSurfaceWorld
            clouds: if self.dim == 0
                && self.render_distance >= 4
                && self.shell.as_ref().map_or(true, |s| s.settings.clouds)
            {
                Some(clouds)
            } else {
                None
            },
        };
        self.renderer.sync_glyphs(&self.atlas);
        let t_build = Instant::now();
        self.prof[2] += t_build.duration_since(t_tick).as_secs_f64();
        self.renderer.render(&frame, window_size);
        self.prof[3] += t_build.elapsed().as_secs_f64();
    }
}

/// A frame with no world: only GUI triangles over a black screen (the
/// menus before a world is loaded).
pub fn draw_ui_only(
    renderer: &mut Renderer,
    atlas: &Atlas,
    ui: &[crate::render::mesher::Vertex],
    gui_scale: f32,
    window_size: (u32, u32),
    screenshot: Option<std::path::PathBuf>,
) {
    let black = [0.0, 0.0, 0.0, 1.0];
    let g = Globals {
        view_proj: render::IDENTITY,
        sky_rot: render::IDENTITY,
        hand_proj: render::IDENTITY,
        cam_right: [1.0, 0.0, 0.0, 0.0],
        cam_up: [0.0, 1.0, 0.0, 0.0],
        cam_fwd: [0.0, 0.0, -1.0, 0.0],
        fog_color: black,
        sky_color: black,
        fog_params: [0.0, 1.0, 0.0, 0.0],
        sun_dir: [0.0, -1.0, 0.0, -1000.0],
        sky_params: [1.0, 0.0, 0.0, 0.0],
        sun_rect: atlas.rects[atlas.sun as usize],
        moon_rect: atlas.rects[atlas.moon[0] as usize],
        void_color: black,
        lightmap: [[1.0; 4]; 256],
        portal_layers: [[0.0; 4]; 16],
        portal_cam: [0.0; 4],
        portal_obj: [0.0; 4],
        end_sky_rect: atlas.rects[atlas.end_sky as usize],
        end_portal_rect: atlas.rects[atlas.end_portal as usize],
    };
    let frame = Frame {
        globals: &g,
        cam: [0.0, -1000.0, 0.0],
        outline: None,
        fish_line: &[],
        gui_scale,
        screenshot,
        star_brightness: 0.0,
        sunrise: None,
        clouds: None,
        ui,
        ui_glint: &[],
        ui_vignette: 0,
        glint_rect: [0.0; 4],
        dynamic: &[],
        crack: &[],
        hand: &[],
        glint: &[],
        hand_glint: &[],
        additive: &[],
        weather: &[],
        end_portals: &[],
        labels: &[],
        crosshair: false,
    };
    renderer.render(&frame, window_size);
}

impl Game {
    fn menu_ctx<'a>(
        shell: &'a mut crate::front::Shell,
        lang: &'a crate::lang::Lang,
        sounds: &'a mut Vec<&'static str>,
    ) -> crate::menu::Ctx<'a> {
        crate::menu::Ctx {
            settings: &mut shell.settings,
            lang,
            languages: &shell.languages,
            saves: &shell.saves,
            in_world: true,
            hardcore: false,
            clipboard: &mut shell.clipboard,
            sounds,
            ctrl: shell.ctrl,
            shift: shell.shift,
        }
    }

    /// `GuiScreen.drawScreen` of the open menu.
    fn draw_menu(&mut self, ui: &mut crate::gui::Ui, partial: f32) {
        let (mx, my) = self.gui_mouse();
        let (Some(menu), Some(shell)) = (self.menu.as_mut(), self.shell.as_mut()) else {
            return;
        };
        let mut sounds = Vec::new();
        let size = (ui.w, ui.h);
        if size != self.menu_size {
            self.menu_size = size;
            let mut ctx = Self::menu_ctx(shell, &self.lang, &mut sounds);
            menu.resize(ui, &mut ctx, size.0, size.1);
        }
        menu.mouse = (mx.floor(), my.floor());
        menu.achieved = self
            .client_stats
            .iter()
            .filter(|(k, v)| **v > 0 && crate::stats::is_achievement(k))
            .map(|(k, _)| k.clone())
            .collect();
        menu.stat_values = self.client_stats.iter().map(|(k, v)| (k.clone(), *v)).collect();
        let mut ctx = Self::menu_ctx(shell, &self.lang, &mut sounds);
        menu.draw(ui, &mut ctx, partial);
        let a = menu.take_pending();
        self.play_ui_sounds(sounds);
        let _ = self.menu_action(a);
    }

    fn play_ui_sounds(&mut self, sounds: Vec<&'static str>) {
        if let Some(a) = self.audio.as_mut() {
            for s in sounds {
                a.play(s, None, 1.0, 1.0);
            }
        }
    }

    fn menu_mouse(&mut self, button: u8, pressed: bool) -> Request {
        let (w, h) = self.menu_size;
        let (mx, my) = self.gui_mouse();
        let a = {
            let (Some(menu), Some(shell)) = (self.menu.as_mut(), self.shell.as_mut()) else {
                return Request::None;
            };
            let mut sounds = Vec::new();
            menu.mouse = (mx.floor(), my.floor());
            let ui = crate::gui::Ui::new(&self.atlas, w, h);
            let a = {
                let mut ctx = Self::menu_ctx(shell, &self.lang, &mut sounds);
                if pressed {
                    menu.mouse_down(&ui, &mut ctx, button)
                } else {
                    menu.mouse_up(&mut ctx, button);
                    crate::menu::Action::None
                }
            };
            drop(ui);
            self.play_ui_sounds(sounds);
            a
        };
        self.menu_action(a)
    }

    /// A key typed while a menu is open: character and LWJGL code.
    pub fn menu_key(&mut self, c: char, code: i32) -> Request {
        let (w, h) = self.menu_size;
        let a = {
            let (Some(menu), Some(shell)) = (self.menu.as_mut(), self.shell.as_mut()) else {
                return Request::None;
            };
            let mut sounds = Vec::new();
            let ui = crate::gui::Ui::new(&self.atlas, w, h);
            let a = {
                let mut ctx = Self::menu_ctx(shell, &self.lang, &mut sounds);
                menu.key(&ui, &mut ctx, c, code)
            };
            drop(ui);
            self.play_ui_sounds(sounds);
            a
        };
        self.menu_action(a)
    }

    /// Text typed while a menu is open (`keyTyped` with a character).
    pub fn char_typed(&mut self, c: char) -> Request {
        if self.screen == Screen::Paused {
            return self.menu_key(c, 0);
        }
        Request::None
    }

    pub fn menu_wheel(&mut self, up: bool) {
        if let Some(m) = self.menu.as_mut() {
            m.wheel(up);
        }
    }

    pub fn menu_open(&self) -> bool {
        self.screen == Screen::Paused && self.menu.is_some()
    }

    fn menu_action(&mut self, a: crate::menu::Action) -> Request {
        use crate::menu::Action;
        match a {
            Action::Resume => {
                self.menu = None;
                self.screen = Screen::Playing;
                Request::Grab
            }
            Action::QuitToTitle => Request::QuitToTitle,
            Action::Quit => Request::Quit,
            Action::RequestStats => {
                let _ = self.sim.commands.send(Command::RequestStats);
                Request::None
            }
            Action::Language => {
                if let Some(sh) = &self.shell {
                    self.lang = sh.load_lang(&self.atlas);
                }
                self.menu_size = (0.0, 0.0);
                Request::None
            }
            Action::Settings => {
                self.apply_settings();
                self.menu_size = (0.0, 0.0);
                Request::None
            }
            _ => Request::None,
        }
    }

    /// The game side of a changed `options.txt`.
    pub fn apply_settings(&mut self) {
        // GameSettings.setOptionFloatValue: the chat options refresh the chat
        {
            let (w, h) = self.gui_size();
            let atlas = self.atlas.clone();
            let ui = crate::gui::Ui::new(&atlas, w, h);
            let st = self.settings();
            self.chat.refresh(&ui, &st);
        }
        self.apply_video_settings();
        let Some(sh) = &self.shell else { return };
        let s = &sh.settings;
        // IntegratedServer.tick follows renderDistanceChunks
        let rd = s.render_distance.clamp(2, 16);
        if rd != self.render_distance {
            self.render_distance = rd;
            let mut offsets = Vec::new();
            for dx in -rd..=rd {
                for dz in -rd..=rd {
                    offsets.push((dx, dz));
                }
            }
            offsets.sort_by_key(|&(x, z)| (x * x + z * z, x, z));
            self.offsets = offsets;
            let _ = self.sim.commands.send(Command::ViewDistance(rd));
        }
        let d = s.difficulty;
        let _ = self.sim.commands.send(Command::Difficulty(d));
    }

    /// The video options that do not depend on how the world was started.
    pub fn apply_video_settings(&mut self) {
        let Some(sh) = &self.shell else { return };
        let s = &sh.settings;
        // Display.setVSyncEnabled(gameSettings.enableVsync)
        self.renderer.set_vsync(s.enable_vsync);
        self.renderer.set_mipmap_levels(s.mipmap_levels);
        // GameSettings.setOptionValue(AMBIENT_OCCLUSION): renderGlobal.loadRenderers()
        // GameSettings.setOptionValue(GRAPHICS): renderGlobal.loadRenderers()
        let fancy_changed = crate::render::mesher::rb::set_fancy(s.fancy_graphics);
        if crate::render::mesher::rb::set_ambient_occlusion(s.ambient_occlusion) | fancy_changed {
            for k in self.world.chunks.keys() {
                self.dirty.insert(*k, 0xFFFF);
            }
        }
    }

    /// `Minecraft.getLimitFramerate` with a world loaded.
    pub fn limit_framerate(&self) -> i32 {
        self.shell
            .as_ref()
            .map_or(120, |s| s.settings.limit_framerate)
    }

    /// Takes the renderer back for the title screen: the world's meshes go.
    pub fn into_parts(
        mut self,
    ) -> (
        Renderer,
        Option<crate::front::Shell>,
        crate::lang::Lang,
        Option<crate::audio::Audio>,
    ) {
        let keys: Vec<(i32, i32)> = self.world.chunks.keys().copied().collect();
        let (pcx, pcz) = self.player_chunk();
        let r = self.render_distance + 8;
        for dx in -r..=r {
            for dz in -r..=r {
                self.renderer.remove_mesh((pcx + dx, pcz + dz));
            }
        }
        for k in keys {
            self.renderer.remove_mesh(k);
        }
        (self.renderer, self.shell, self.lang, self.audio)
    }
}

/// `BlockStem.getRenderColor`
fn stem_color(meta: i32) -> u32 {
    ((meta * 32) << 16 | (255 - meta * 8) << 8 | meta * 4) as u32
}

/// `EntityLiving.isAIEnabled`: the classes on the task AI, whose body the
/// client turns with `EntityBodyHelper` (the rest use
/// `EntityLivingBase.updateDistance`).
fn ai_enabled(k: crate::entity::mob::Kind) -> bool {
    use crate::entity::mob::Kind;
    matches!(
        k,
        Kind::Zombie
            | Kind::Skeleton
            | Kind::Creeper
            | Kind::Witch
            | Kind::IronGolem
            | Kind::Pig
            | Kind::Cow
            | Kind::Mooshroom
            | Kind::Sheep
            | Kind::Chicken
            | Kind::Wolf
            | Kind::Ocelot
            | Kind::Villager
            | Kind::Horse
    )
}

/// `updateDistance` on the client for one mob that moved `dx`, `dz` this
/// tick: `EntityBodyHelper` for the task-AI classes, else
/// `EntityLivingBase.updateDistance`.
pub(crate) fn update_body(m: &mut crate::sim::MobView, dx: f64, dz: f64) {
    let wrap = |a: f32| {
        let mut a = a % 360.0;
        if a >= 180.0 {
            a -= 360.0;
        }
        if a < -180.0 {
            a += 360.0;
        }
        a
    };
    if ai_enabled(m.kind) {
        // EntityLiving.updateDistance with isAIEnabled:
        // EntityBodyHelper.updateRenderAngles, so the body turns to
        // follow the head the server sends (S19PacketEntityHeadLook)
        let bound = |a: f32, b: f32, lim: f32| a - wrap(a - b).clamp(-lim, lim);
        if dx * dx + dz * dz > 2.5000003e-7 {
            m.body_yaw = m.yaw;
            m.head_yaw = bound(m.body_yaw, m.head_yaw, 75.0);
            m.body_head = m.head_yaw;
            m.body_ticks = 0;
        } else {
            let mut lim = 75.0f32;
            if (m.head_yaw - m.body_head).abs() > 15.0 {
                m.body_ticks = 0;
                m.body_head = m.head_yaw;
            } else {
                m.body_ticks += 1;
                if m.body_ticks > 10 {
                    lim = (1.0 - (m.body_ticks - 10) as f32 / 10.0).max(0.0) * 75.0;
                }
            }
            m.body_yaw = bound(m.head_yaw, m.body_yaw, lim);
        }
    } else {
        // EntityLivingBase.updateDistance
        let d2 = (dx * dx + dz * dz) as f32;
        let mut target = m.body_yaw;
        if d2 > 0.0025000002 {
            target = (dz.atan2(dx) as f32) * 180.0 / std::f32::consts::PI - 90.0;
        }
        if m.swing > 0.0 {
            target = m.yaw;
        }
        m.body_yaw += wrap(target - m.body_yaw) * 0.3;
        let b = wrap(m.yaw - m.body_yaw).clamp(-75.0, 75.0);
        m.body_yaw = m.yaw - b;
        if b * b > 2500.0 {
            m.body_yaw += b * 0.2;
        }
    }
}

/// A mob's per-tick animation counters as the server last sent them.
#[derive(Clone, Copy)]
struct MobAnim {
    swing: f32,
    fuse: i32,
    wing: f32,
    squish: f32,
    sq: [f32; 4],
    eat_timer: i32,
    attack_timer: i32,
}

impl MobAnim {
    fn of(v: &crate::sim::MobView) -> MobAnim {
        MobAnim {
            swing: v.swing,
            fuse: v.fuse,
            wing: v.wing,
            squish: v.squish[1],
            sq: v.sq[1],
            eat_timer: v.eat_timer,
            attack_timer: v.attack_timer,
        }
    }

    /// A new snapshot keeps the client's own counters.
    fn keep(v: &mut crate::sim::MobView, o: &crate::sim::MobView) {
        v.swing = o.swing;
        v.prev_swing = o.prev_swing;
        v.fuse = o.fuse;
        v.prev_fuse = o.prev_fuse;
        v.wing = o.wing;
        v.prev_wing = o.prev_wing;
        v.squish = o.squish;
        v.sq = o.sq;
        v.eat_timer = o.eat_timer;
        v.attack_timer = o.attack_timer;
        v.ticks = o.ticks;
        v.hanim = o.hanim;
        v.tail_counter = o.tail_counter;
        v.head_course = o.head_course;
    }

    /// One client tick: the previous values become the current ones and
    /// the latest server values become current.
    fn advance(&self, m: &mut crate::sim::MobView) {
        m.prev_swing = m.swing;
        m.swing = self.swing;
        m.prev_fuse = m.fuse;
        m.fuse = self.fuse;
        m.prev_wing = m.wing;
        m.wing = self.wing;
        m.squish = [m.squish[1], self.squish];
        m.sq = [m.sq[1], self.sq];
        m.eat_timer = self.eat_timer;
        m.attack_timer = self.attack_timer;
        // Entity.onUpdate: ++ticksExisted on the client's own tick
        m.ticks += 1;
        if m.kind == crate::entity::mob::Kind::Horse {
            horse_anim(m);
        }
        // EntityWolf.onUpdate on the client: the head tilts while begging
        if m.kind == crate::entity::mob::Kind::Wolf {
            let c = m.head_course[1];
            let to = if m.begging { 1.0 } else { 0.0 };
            m.head_course = [c, c + (to - c) * 0.4];
        }
    }
}

impl Game {
    /// `MovingSoundMinecart` for each minecart and `MovingSoundMinecartRiding`
    /// for the one ridden: minecart.base louder the faster it rolls, and
    /// minecart.inside heard without attenuation.
    fn minecart_sounds(&mut self) {
        let Some(audio) = self.audio.as_mut() else { return };
        let mut keep = Vec::new();
        for b in &self.boats {
            let crate::entity::vehicle::Vehicle::Cart(c) = &b.v else { continue };
            let id = c.id as u64;
            let (dx, dz) = (c.e.pos[0] - c.e.prev[0], c.e.pos[2] - c.e.prev[2]);
            let f = (dx * dx + dz * dz).sqrt() as f32;
            let v = if f >= 0.01 { f.clamp(0.0, 0.5) * 0.7 } else { 0.0 };
            audio.set_loop(1 << 40 | id, "minecart.base", Some(c.e.pos), v);
            keep.push(1 << 40 | id);
            if self.riding == Some(c.id) {
                let v = if f >= 0.01 { f.clamp(0.0, 1.0) * 0.75 } else { 0.0 };
                audio.set_loop(2 << 40 | id, "minecart.inside", None, v);
                keep.push(2 << 40 | id);
            }
        }
        audio.keep_loops(&keep);
    }

    /// Puts map `id`'s texture into slot `i` of the atlas.
    fn upload_map(&mut self, i: usize, id: i32) {
        let Some(v) = self.maps.get(&id) else { return };
        let Some(&(_, (x, y))) = self.atlas.map_slots.get(i) else { return };
        let px = crate::map::texture(&v.colours);
        self.renderer.queue_tile_update(crate::render::vk::TileUpdate {
            x,
            y,
            size: 128,
            mips: crate::render::atlas::mip_chain(px, 128),
        });
    }

    /// `MapItemRenderer.getMapRendererInstance`: the maps in the hand and in
    /// the item frames near by get atlas slots, the longest unused giving
    /// way.
    pub(super) fn refresh_map_slots(&mut self) {
        let n = self.atlas.map_slots.len();
        if self.map_slots.len() != n {
            self.map_slots = vec![None; n];
        }
        let mut wanted: Vec<i32> = Vec::new();
        if let Some(s) = self.equipped_icon().filter(|s| s.id == 358) {
            wanted.push(s.meta);
        }
        for v in &self.boats {
            if let crate::entity::vehicle::Vehicle::Hang(h) = &v.v {
                if let Some(s) = h.item.filter(|s| s.id == 358) {
                    if !wanted.contains(&s.meta) {
                        wanted.push(s.meta);
                    }
                }
            }
        }
        let now = self.ticks as u64;
        for id in wanted {
            if !self.maps.contains_key(&id) {
                continue;
            }
            if let Some(s) = self.map_slots.iter_mut().flatten().find(|s| s.0 == id) {
                s.1 = now;
                continue;
            }
            let i = match self.map_slots.iter().position(|s| s.is_none()) {
                Some(i) => i,
                None => (0..n)
                    .min_by_key(|&i| self.map_slots[i].map_or(0, |s| s.1))
                    .unwrap_or(0),
            };
            if i < n {
                self.map_slots[i] = Some((id, now));
                self.upload_map(i, id);
            }
        }
    }

    /// The textures and markers to draw map `id` with.
    pub(super) fn map_draw(&self, id: i32) -> Option<crate::render::entities::MapDraw<'_>> {
        let v = self.maps.get(&id)?;
        let i = self.map_slots.iter().position(|s| s.is_some_and(|s| s.0 == id))?;
        Some(crate::render::entities::MapDraw {
            tex: self.atlas.map_slots[i].0,
            background: self.atlas.mob_tex("../map/map_background")?.rect,
            icons: self.atlas.mob_tex("../map/map_icons")?.rect,
            marks: &v.marks,
        })
    }

    /// `EntityPlayerSP.isRidingHorse`: on a saddled horse.
    fn riding_horse(&self) -> bool {
        self.riding.is_some_and(|id| {
            self.mobs.iter().any(|m| {
                m.id == id && m.kind == crate::entity::mob::Kind::Horse && m.horse_flags & 4 != 0
            })
        })
    }
}

/// `EntityHorse.onUpdate` on the client: the tail wag, and the head lean
/// (eating), rearing and mouth amounts easing toward the watched flags.
fn horse_anim(m: &mut crate::sim::MobView) {
    // onLivingUpdate: moveTail one time in 200 (the entity's own random)
    let h = (m.id as u64)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(m.ticks as u64)
        .wrapping_mul(0xBF58_476D_1CE4_E5B9);
    if (h >> 33) % 200 == 0 {
        m.tail_counter = 1;
    }
    if m.tail_counter > 0 {
        m.tail_counter += 1;
        if m.tail_counter > 8 {
            m.tail_counter = 0;
        }
    }
    let [_, mut lean, _, mut rear, _, mut mouth] = m.hanim;
    let prev_lean;
    if m.horse_flags & 64 != 0 {
        lean = 0.0;
        prev_lean = 0.0;
        rear = (rear + (1.0 - rear) * 0.4 + 0.05).min(1.0);
    } else {
        prev_lean = lean;
        rear = (rear + (0.8 * rear * rear * rear - rear) * 0.6 - 0.05).max(0.0);
    }
    let prev_rear = m.hanim[3];
    if m.horse_flags & 64 == 0 {
        lean = if m.horse_flags & 32 != 0 {
            (lean + (1.0 - lean) * 0.4 + 0.05).min(1.0)
        } else {
            (lean + (0.0 - lean) * 0.4 - 0.05).max(0.0)
        };
    }
    let prev_mouth = mouth;
    mouth = if m.horse_flags & 128 != 0 {
        (mouth + (1.0 - mouth) * 0.7 + 0.05).min(1.0)
    } else {
        (mouth + (0.0 - mouth) * 0.7 - 0.05).max(0.0)
    };
    m.hanim = [prev_lean, lean, prev_rear, rear, prev_mouth, mouth];
}

/// `WorldClient.skylightSubtracted`: only `calculateInitialSkylight` in the
/// world's constructor sets it, when its time is still 0 and it is not
/// raining, and nothing on the client updates it afterwards (WorldServer
/// does its own). So `getLightBrightness` on the client (the vignette, the
/// fog's dimming, a minecart's brightness) never sees the night.
pub(crate) fn client_skylight_subtracted() -> i32 {
    sim::skylight_subtracted_weather(0, 0.0, 0.0)
}
