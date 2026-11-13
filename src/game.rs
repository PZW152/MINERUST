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
    /// Everything that can write into it has been populated.
    ready: bool,
}

#[path = "game_vehicles.rs"]
mod vehicles;

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

struct Job(Box<[MeshChunk; 9]>);

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
    pending_mesh: HashSet<(i32, i32)>,
    dirty: HashSet<(i32, i32)>,
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
    in_portal: bool,
    ticks: u64,
    noise: BiomeNoise,
    /// `(ticks shown, position in the frame order)` per animated tile.
    anim_state: Vec<(u32, usize)>,
    /// Cloud texel the current cloud mesh was built for.
    cloud_cell: Option<(i32, i32)>,
    /// Server view of the inventory and open window.
    pub view: InvView,
    /// `TileEntitySign.signText` as the client knows it.
    sign_texts: HashMap<[i32; 3], [String; 4]>,
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
    /// `GuiContainer` of the open window.
    gui: crate::gui_container::GuiContainer,
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
    pickups: Vec<(ItemView, i32)>,
    /// `EntityPickupFX` of experience orbs.
    orb_pickups: Vec<(crate::entity::xp_orb::XpOrb, i32)>,
    /// Throwables: position, previous position, item, damage.
    throwables: Vec<([f64; 3], [f64; 3], u16, i32)>,
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
    particles: crate::particles::Particles,
    dying: std::collections::HashSet<u32>,
    next_step: f32,
    mobs: Vec<crate::sim::MobView>,
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
    gui_scale: f32,
    pub left_down: bool,
    pub right_down: bool,
    dig: Option<Digging>,
    hit_delay: i32,
    right_delay: i32,
    /// `swingProgressInt`, -1 when not swinging.
    swing: i32,
    prev_swing_f: f32,
    swing_f: f32,
    equip: f32,
    prev_equip: f32,
    equipped: Option<Stack>,
    death_sent: bool,
    jitter: JavaRandom,
    prev_feet: [f64; 3],
    /// F3 overlay.
    pub show_debug: bool,
    fps: u32,
    fps_frames: u32,
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
                    if tx.send(mesher::build(&job.0, &atlas, &noise)).is_err() {
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
            pending_mesh: HashSet::new(),
            dirty: HashSet::new(),
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
            in_portal: false,
            ticks: 0,
            noise: BiomeNoise::new(),
            anim_state: Vec::new(),
            cloud_cell: None,
            view: InvView {
                inv: Default::default(),
                open: Open::None,
                result: None,
                tile_slots: Vec::new(),
                furnace: None,
                clicks: 0,
            },
            gui: Default::default(),
            creative: None,
            creative_tab: 0,
            chest_lids: HashMap::new(),
            sign_texts: HashMap::new(),
            sign_edit: None,
            third_person: 0,
            loading: 0,
            p_body: [0.0; 2],
            p_yaw: [0.0; 2],
            p_pitch: [0.0; 2],
            p_limb: [0.0; 3],
            chat: Default::default(),
            chat_screen: None,
            hide_gui: false,
            sent_clicks: 0,
            start_use: None,
            client_drag: Default::default(),
            items: Vec::new(),
            pickups: Vec::new(),
            orb_pickups: Vec::new(),
            lang: crate::lang::Lang::default(),
            using: None,
            falling: Vec::new(),
            throwables: Vec::new(),
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
            particles: Default::default(),
            dying: Default::default(),
            next_step: 1.0,
            mobs: Vec::new(),
            arrows: Vec::new(),
            tnt: Vec::new(),
            fireballs: Vec::new(),
            target_mob: None,
            stats,
            mode,
            screen: Screen::Playing,
            mouse_px: (0.0, 0.0),
            gui_scale: 2.0,
            left_down: false,
            right_down: false,
            dig: None,
            hit_delay: 0,
            right_delay: 0,
            swing: -1,
            prev_swing_f: 0.0,
            swing_f: 0.0,
            equip: 0.0,
            prev_equip: 0.0,
            equipped: None,
            death_sent: false,
            jitter: JavaRandom::new(312871),
            prev_feet: [0.0; 3],
            show_debug: std::env::var_os("MINERUST_F3").is_some(),
            fps: 0,
            fps_frames: 0,
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
        self.in_flight == 0 && self.dirty.is_empty() && self.renderer.mesh_count() >= side * side
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

    fn mesh_input(&self, cx: i32, cz: i32) -> Option<Box<[MeshChunk; 9]>> {
        if !self.world.chunks.get(&(cx, cz))?.ready {
            return None;
        }
        let mut v = Vec::with_capacity(9);
        for dz in -1..=1 {
            for dx in -1..=1 {
                v.push(self.world.chunks.get(&(cx + dx, cz + dz))?.mc.clone());
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
                self.renderer.upload_mesh(&m);
            }
        }
        // Snapshots from the server.
        while let Ok(e) = self.sim.events.try_recv() {
            match e {
                Event::Chunk { key, chunk, ready } => {
                    let mut portals = Vec::new();
                    let mut chests = Vec::new();
                    let mut signs = Vec::new();
                    for (i, &b) in chunk.data.blocks.iter().enumerate() {
                        if b == block::STANDING_SIGN || b == block::WALL_SIGN {
                            let (x, z, y) =
                                ((i >> 12) as i32, (i >> 8 & 15) as i32, (i & 255) as i32);
                            signs.push([key.0 * 16 + x, y, key.1 * 16 + z]);
                        }
                        if b == block::END_PORTAL || b == block::CHEST {
                            // index ((x << 4 | z) << 8) | y
                            let (x, z, y) =
                                ((i >> 12) as i32, (i >> 8 & 15) as i32, (i & 255) as i32);
                            let p = [key.0 * 16 + x, y, key.1 * 16 + z];
                            if b == block::CHEST {
                                chests.push(p);
                            } else {
                                portals.push(p);
                            }
                        }
                    }
                    self.world.chunks.insert(
                        key,
                        ClientChunk {
                            mc: chunk,
                            ready,
                            portals,
                            chests,
                            signs,
                        },
                    );
                    // Faces and light on the border depend on the neighbours.
                    for dx in -1..=1 {
                        for dz in -1..=1 {
                            self.dirty.insert((key.0 + dx, key.1 + dz));
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
                    self.view = *v;
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
                Event::Vehicles(v) => self.vehicle_packets(v),
                Event::VehicleMeta(v) => self.vehicle_meta(v),
                Event::VehicleStatus(v) => self.vehicle_status(v),
                Event::Mount(v) => self.mounted(v),
                Event::Destroyed(v) => self.destroyed_fx(v),
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
                                self.particles.item_crack(pos, m, rect);
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
                            self.particles.spell(
                                start,
                                [mx, my, mz],
                                base,
                                [cr * f, cg * f, cb * f],
                                1.0,
                            );
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
                Event::Collect(v) => {
                    // NetHandlerPlayClient.handleCollectItem
                    for it in v {
                        self.pickups.push((it, 0));
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
                                self.particles.explode(p, mv);
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
                    // EntityLivingBase.handleHealthUpdate(2)
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
                        self.particles.note([c[0], p[1] as f64 + 1.2, c[2]], pitch);
                    }
                }
                Event::Explosions(list) => {
                    for e in list {
                        self.explosion_b(e);
                    }
                }
                Event::Weather(w) => self.weather = w,
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
            let has = self.renderer.has_mesh(key);
            if has && !self.dirty.contains(&key) {
                continue;
            }
            if let Some(n) = self.mesh_input(key.0, key.1) {
                self.dirty.remove(&key);
                self.pending_mesh.insert(key);
                self.in_flight += 1;
                let _ = self.jobs.send(Job(n));
            }
        }
        // Chunks outside the view never get meshed; forget their dirty marks.
        self.dirty
            .retain(|k| (k.0 - pcx).abs() <= r && (k.1 - pcz).abs() <= r);
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

    /// `TextureAtlasSprite.updateAnimation` for every animated tile.
    fn tick_animations(&mut self) {
        self.tick_particles();
        if self.anim_state.len() != self.atlas.anims.len() {
            self.anim_state = vec![(0, 0); self.atlas.anims.len()];
        }
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
        let st = self.equipped;
        let action = self
            .using
            .map(|(u, _)| crate::item_use::action(u.id, u.meta));
        let pose = crate::render::model::Pose {
            limb_swing: self.p_limb[0] - self.p_limb[1] * (1.0 - partial),
            limb_amount: (self.p_limb[2] + (self.p_limb[1] - self.p_limb[2]) * partial).min(1.0),
            age: self.ticks as f32 + partial,
            head_yaw: yaw - body,
            head_pitch: pitch,
            swing: self.prev_swing_f + (self.swing_f - self.prev_swing_f) * partial,
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
    }

    fn tick(&mut self) {
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
        if self.ticks >= 60 {
            if let Some(t) = self.start_use.take() {
                let _ = self.sim.commands.send(Command::Use {
                    target: t,
                    face: [0, 1, 0],
                    sneaking: false,
                });
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
        if !self.freeze_time {
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
        let sub = sim::skylight_subtracted(self.world_time);
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
        if self.world.loaded(pcx << 4, pcz << 4) && !self.stats.dead {
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
        self.prev_swing_f = self.swing_f;
        if self.swing >= 0 {
            self.swing += 1;
            if self.swing >= 6 {
                self.swing = -1;
            }
        }
        self.swing_f = if self.swing >= 0 {
            self.swing as f32 / 6.0
        } else {
            0.0
        };
        if self.prev_swing_f > self.swing_f && self.swing < 0 {
            self.prev_swing_f = 0.0;
        }
        self.prev_equip = self.equip;
        let held = self.view.inv.held();
        let same = match (held, self.equipped) {
            (Some(a), Some(b)) => a.id == b.id && a.meta == b.meta,
            (None, None) => true,
            _ => false,
        };
        let target = if same { 1.0 } else { 0.0 };
        self.equip += (target - self.equip).clamp(-0.4, 0.4);
        if self.equip < 0.1 {
            self.equipped = held;
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
        self.particles.tick(&self.world);
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
                    self.particles.portal(p, m);
                }
            }
        }
        // EntityTNTPrimed.onUpdate: a puff of smoke every tick.
        for &(pos, _, _) in &self.tnt {
            self.particles
                .smoke([pos[0], pos[1] + 0.5, pos[2]], [0.0; 3], false);
        }
        let eye = self.player.pos;
        self.particles.display_ticks(&self.world, eye);
        // EntityRenderer.addRainParticles
        let rain = self.weather.rain;
        if rain > 0.0 {
            let n = (100.0 * rain * rain) as i32;
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
                self.particles.dust(p, m, tex, tint);
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
        let r = 10;
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
            "largeexplode" => self.particles.large_explode(p, 1.0),
            "hugeexplosion" => self.particles.huge_explosion(p),
            "explode" => self.particles.explode(p, m),
            "splash" => self.particles.splash(p, m),
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

    /// Damage, knockback and experience from mobs.
    /// `EntityPlayer.attackTargetEntityWithCurrentItem` on the client side:
    /// damage of the held item, critical hits and sprint knockback.
    fn attack_mob(&mut self, id: u32) {
        let held = self.view.inv.held();
        let mut dmg = 1.0f32;
        if let Some(s) = held {
            if let Some(t) = crate::item::tool(s.id) {
                let mat = match t.max_uses {
                    59 => 0.0,
                    131 => 1.0,
                    250 => 2.0,
                    1561 => 3.0,
                    _ => 0.0,
                };
                let base = match t.kind {
                    crate::item::ToolKind::Sword => 4.0,
                    crate::item::ToolKind::Axe => 3.0,
                    crate::item::ToolKind::Pickaxe => 2.0,
                    crate::item::ToolKind::Shovel => 1.0,
                    crate::item::ToolKind::Hoe => -mat,
                };
                dmg += base + mat;
            }
        }
        let vehicle = id & crate::entity::vehicle::VEHICLE_ID != 0;
        let crit = self.stats.fall_distance > 0.0
            && !self.player.on_ground
            && !self.player.in_water
            && self.riding.is_none()
            && !vehicle;
        if vehicle && self.player.sprinting {
            // the client's own attackTargetEntityWithCurrentItem knocks its boat
            let r = std::f32::consts::PI;
            let yaw = self.player.yaw;
            if let Some(c) = self.boats.iter_mut().find(|c| c.v.id() == id) {
                let e = c.v.e_mut();
                e.motion[0] += (-math::sin(yaw * r / 180.0) * 0.5) as f64;
                e.motion[1] += 0.1;
                e.motion[2] += (math::cos(yaw * r / 180.0) * 0.5) as f64;
            }
        }
        let knockback = if self.player.sprinting { 1 } else { 0 };
        if knockback > 0 {
            self.player.motion[0] *= 0.6;
            self.player.motion[2] *= 0.6;
            self.player.sprinting = false;
        }
        if crit {
            if let Some(m) = self.mobs.iter().find(|m| m.id == id) {
                let c = [m.pos[0], (m.bb[1] + m.bb[4]) / 2.0, m.pos[2]];
                for _ in 0..16 {
                    let r = &mut self.particle_rand;
                    let mv = [
                        r.next_float() as f64 * 2.0 - 1.0,
                        r.next_float() as f64 * 2.0 - 1.0 + 0.2,
                        r.next_float() as f64 * 2.0 - 1.0,
                    ];
                    self.particles.crit(c, mv);
                }
            }
        }
        let _ = self.sim.commands.send(Command::Attack {
            id,
            damage: dmg,
            crit,
            knockback,
        });
    }

    /// Block under the crosshair within survival reach (4.5 blocks).
    fn pick(&self, eye: [f64; 3]) -> Option<([i32; 3], [i32; 3])> {
        let d = self.look();
        let reach = 4.5f64;
        let mut cell = [
            math::floor_double(eye[0]),
            math::floor_double(eye[1]),
            math::floor_double(eye[2]),
        ];
        let mut t_max = [0f64; 3];
        let mut t_delta = [0f64; 3];
        let mut step = [0i32; 3];
        for i in 0..3 {
            if d[i] > 0.0 {
                step[i] = 1;
                t_max[i] = (cell[i] as f64 + 1.0 - eye[i]) / d[i];
                t_delta[i] = 1.0 / d[i];
            } else if d[i] < 0.0 {
                step[i] = -1;
                t_max[i] = (cell[i] as f64 - eye[i]) / d[i];
                t_delta[i] = -1.0 / d[i];
            } else {
                t_max[i] = f64::INFINITY;
                t_delta[i] = f64::INFINITY;
            }
        }
        let mut normal = [0i32; 3];
        loop {
            let id = self.world.peek_block(cell[0], cell[1], cell[2]);
            if block::selectable(id) {
                return Some((cell, normal));
            }
            let axis = if t_max[0] < t_max[1] {
                if t_max[0] < t_max[2] {
                    0
                } else {
                    2
                }
            } else if t_max[1] < t_max[2] {
                1
            } else {
                2
            };
            if t_max[axis] > reach {
                return None;
            }
            cell[axis] += step[axis];
            t_max[axis] += t_delta[axis];
            normal = [0; 3];
            normal[axis] = -step[axis];
        }
    }

    fn gui_mouse(&self) -> (f32, f32) {
        (
            self.mouse_px.0 / self.gui_scale,
            self.mouse_px.1 / self.gui_scale,
        )
    }

    fn gui_size(&self) -> (f32, f32) {
        let e = self.renderer.extent;
        (
            e.width as f32 / self.gui_scale,
            e.height as f32 / self.gui_scale,
        )
    }

    /// `EntityLivingBase.swingItem`
    fn swing_item(&mut self) {
        if self.swing < 0 || self.swing >= 3 {
            self.swing = -1;
        }
        if self.swing == -1 {
            self.swing = 0;
        }
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
            let _ = self.sim.commands.send(Command::Dig(pos));
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
        let step =
            crate::item::break_progress(self.view.inv.held(), id, in_water, self.player.on_ground);
        match self.dig {
            Some(d) if d.pos == pos => {
                let damage = d.damage + step;
                if damage >= 1.0 {
                    self.block_sound(id, pos, "dig");
                    let (dt, dti) = self.debris(pos);
                    self.particles.destroy(pos, dt, dti);
                    let _ = self.sim.commands.send(Command::Dig(pos));
                    self.dig = None;
                    self.hit_delay = 5;
                } else {
                    self.dig = Some(Digging { pos, damage });
                }
            }
            _ => {
                if step >= 1.0 {
                    self.block_sound(id, pos, "dig");
                    let (dt, dti) = self.debris(pos);
                    self.particles.destroy(pos, dt, dti);
                    let _ = self.sim.commands.send(Command::Dig(pos));
                    self.dig = None;
                } else {
                    self.dig = Some(Digging { pos, damage: 0.0 });
                }
            }
        }
    }

    fn use_item(&mut self) {
        if let Some((b, n)) = self.selected {
            if n != [0, 0, 0] {
                if let Some(id) = self.view.inv.held().and_then(|s| s.block()) {
                    let p = [b[0] + n[0], b[1] + n[1], b[2] + n[2]];
                    if crate::block::material(self.world.peek_block(p[0], p[1], p[2])).replaceable()
                    {
                        self.block_sound(id, p, "place");
                    }
                }
                let _ = self.sim.commands.send(Command::Use {
                    target: b,
                    face: n,
                    sneaking: self.input.sneak,
                });
                self.swing_item();
            }
        } else {
            let _ = self.sim.commands.send(Command::UseAir);
        }
        self.right_delay = 4;
    }

    /// The mouse in scaled GUI pixels (`Mouse.getX() * width / displayWidth`).
    fn gui_mouse_i(&self) -> (i32, i32) {
        (
            (self.mouse_px.0 / self.gui_scale).floor() as i32,
            (self.mouse_px.1 / self.gui_scale).floor() as i32,
        )
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

    /// `PlayerControllerMP.windowClick`: `slotClick` on the client's copy,
    /// then `C0EPacketClickWindow`.
    fn window_click(&mut self, c: crate::gui_container::Click) -> InvView {
        let creative = self.mode == GameMode::Creative;
        let window = crate::play::window_of(&self.view.open);
        let mut tile = std::mem::take(&mut self.view.tile_slots);
        {
            let mut ctx =
                crate::container::Ctx::new(&mut self.view.inv, &mut tile, window, creative);
            crate::container::slot_click(&mut ctx, &mut self.client_drag, c.slot, c.button, c.mode);
            let r = ctx.result();
            self.view.result = r;
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

    /// The client's `WorldClient` copies of the mobs: a new snapshot keeps
    /// the client-side motion state and starts a three-step interpolation
    /// whenever the tracker sent a move (`handleEntityMovement` ->
    /// `setPositionAndRotation2(.., 3)`).
    fn merge_mobs(&mut self, list: Vec<crate::sim::MobView>) {
        let mut old: std::collections::HashMap<u32, crate::sim::MobView> =
            self.mobs.drain(..).map(|m| (m.id, m)).collect();
        for mut v in list {
            let old_v = old.remove(&v.id);
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
                    v.pitch = o.pitch;
                    v.prev_pitch = o.prev_pitch;
                    v.limb_swing = o.limb_swing;
                    v.limb_amount = o.limb_amount;
                    v.prev_limb_amount = o.prev_limb_amount;
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
            // EntityLivingBase.moveEntityWithHeading: limbSwingAmount
            let (dx, dz) = (m.pos[0] - m.prev[0], m.pos[2] - m.prev[2]);
            let f = ((dx * dx + dz * dz).sqrt() as f32 * 4.0).min(1.0);
            m.limb_amount += (f - m.limb_amount) * 0.4;
            m.limb_swing += m.limb_amount;
            // updateDistance
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
        self.screen == Screen::Window && self.creative.is_some()
            || self.screen == Screen::Chat
            || self.screen == Screen::Sign
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
            _ => Vec::new(),
        };
        for p in &users {
            self.chest_lids.entry(*p).or_insert([0.0; 2]);
        }
        let chest = |w: &ClientWorld, x: i32, y: i32, z: i32| w.peek_block(x, y, z) == block::CHEST;
        let mut sounds = Vec::new();
        let keys: Vec<[i32; 3]> = self.chest_lids.keys().copied().collect();
        for p in keys {
            if !chest(&self.world, p[0], p[1], p[2]) {
                self.chest_lids.remove(&p);
                continue;
            }
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
                if w.peek_block(p[0], p[1], p[2]) != block::CHEST {
                    continue;
                }
                let is = |x: i32, z: i32| w.peek_block(x, p[1], z) == block::CHEST;
                if is(p[0] - 1, p[2]) || is(p[0], p[2] - 1) {
                    continue;
                }
                let (x_pos, z_pos) = (is(p[0] + 1, p[2]), is(p[0], p[2] + 1));
                let large = x_pos || z_pos;
                let Some(t) = self
                    .atlas
                    .mob_tex(crate::render::chest::texture(false, large, xmas))
                else {
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
                        let _ = self.sim.commands.send(Command::Interact { id });
                        self.swing_item();
                        self.right_delay = 4;
                    }
                    1 => self.use_item(),
                    _ => {
                        if let Some((b, _)) = self.selected {
                            let id = self.world.peek_block(b[0], b[1], b[2]);
                            let meta = self.world.peek_meta(b[0], b[1], b[2]) as i32;
                            let _ = self.sim.commands.send(Command::PickBlock(id, meta));
                        }
                    }
                }
                Request::None
            }
            Screen::Window => Request::None,
            Screen::Paused => self.menu_mouse(button, true),
            Screen::Chat => Request::None,
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
            let _ = self.sim.commands.send(Command::DropHeld { all: ctrl });
            self.swing_item();
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
            let f1 = self.light_table[i / 16] * f;
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
        for _ in 0..n {
            self.tick();
        }
        self.partial = self.tick_acc as f32;
        let partial = self.partial;

        let eye = self.eye(partial);
        self.selected = self.pick(eye);
        {
            // EntityRenderer.getMouseOver: a mob closer than the block wins.
            let look = self.look();
            let reach = if self.mode == GameMode::Creative {
                5.0
            } else {
                3.0
            };
            let block_d = self.selected.map(|(b, _)| {
                let c = [b[0] as f64 + 0.5, b[1] as f64 + 0.5, b[2] as f64 + 0.5];
                ((c[0] - eye[0]).powi(2) + (c[1] - eye[1]).powi(2) + (c[2] - eye[2]).powi(2)).sqrt()
                    - 0.5
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
                // canBeCollidedWith: !isDead; getCollisionBorderSize 0.1
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
        let mut bob: Mat4 = render::translate(
            math::sin(b1 * math::PI) * b2 * 0.5f32,
            -(math::cos(b1 * math::PI) * b2).abs(),
            0.0,
        );
        bob = render::mul(&bob, &render::rot_z(math::sin(b1 * math::PI) * b2 * 3.0f32));
        bob = render::mul(
            &bob,
            &render::rot_x((math::cos(b1 * math::PI - 0.2f32) * b2).abs() * 5.0f32),
        );
        let shake = render::mul(&hurt, &bob);
        let mut view: Mat4 = shake;
        view = render::mul(&view, &render::rot_x(b3));
        if self.third_person > 0 {
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
        view = render::mul(&view, &render::rot_x(p.pitch));
        view = render::mul(&view, &render::rot_y(p.yaw + 180.0));
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
            let mut cx = (eye[0] + t * 0.03f32 as f64) / 12.0;
            let mut cz = eye[2] / 12.0 + 0.33f32 as f64;
            cx -= (math::floor_double(cx / 2048.0) * 2048) as f64;
            cz -= (math::floor_double(cz / 2048.0) * 2048) as f64;
            let (bx, bz) = (math::floor_double(cx), math::floor_double(cz));
            if self.cloud_cell != Some((bx, bz)) {
                let mesh = self.build_clouds(bx, bz);
                self.renderer.set_clouds(&mesh);
                self.cloud_cell = Some((bx, bz));
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
                    -((cx - bx as f64) * 12.0) as f32,
                    y,
                    -((cz - bz as f64) * 12.0) as f32,
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
            self.fps_since = Instant::now();
        }
        self.gui_scale = scale as f32;

        // --- Dropped items, crack overlay, hand ---
        let mut dynamic: Vec<crate::render::mesher::Vertex> = Vec::new();
        let mut translucent: Vec<crate::render::mesher::Vertex> = Vec::new();
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
            // Entity.getBrightnessForRender
            let ly = math::floor_double(p[1] + (m.bb[4] - m.bb[1]) * 0.66);
            let (lx, lz) = (math::floor_double(p[0]), math::floor_double(p[2]));
            let (sky, blk) = (
                self.world.light(true, lx, ly, lz),
                self.world.light(false, lx, ly, lz),
            );
            let blk = if m.fire { 15 } else { blk };
            let light = [(blk << 4) as u8, (sky << 4) as u8, 0, 0];
            let atlas = &self.atlas;
            crate::render::model::mob(
                &mut dynamic,
                &mut translucent,
                &|n| atlas.mob_tex(n),
                &|o, id, meta, mat, l| entities::block_model(o, atlas, id, meta, mat, l),
                m,
                rel,
                partial,
                light,
            );
        }
        {
            let w = &self.world;
            let lf = |x: i32, y: i32, z: i32| (w.light(true, x, y, z), w.light(false, x, y, z));
            self.particles.render(
                &mut dynamic,
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
        self.draw_player(&mut dynamic, partial);
        self.draw_orbs(&mut translucent, eye, partial);
        if let Some(at) = self.atlas.mob_tex("arrow") {
            for a in &self.arrows {
                let t = partial as f64;
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
                crate::render::model::arrow(&mut dynamic, at, rel, a.yaw, a.pitch, light);
            }
        }
        let fire_tex = self.atlas.mob_tex("../items/fireball");
        let eye_tex = self.atlas.mob_tex("../items/ender_eye");
        {
            for &(pos, prev, kind) in &self.fireballs {
                let Some(t) = (if kind == 2 { eye_tex } else { fire_tex }) else {
                    continue;
                };
                let tp = partial as f64;
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
            let t = partial as f64;
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
                partial,
                light,
            );
        }
        // RenderSnowball
        for &(pos, prev, id, meta) in &self.throwables {
            let t = partial as f64;
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
        // RenderFallingBlock
        for f in &self.falling {
            let t = partial as f64;
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
        self.render_moving(&mut dynamic, eye, partial);
        // EntityPickupFX.renderParticle
        for (it, age) in &self.pickups {
            let mut f = (*age as f32 + partial) / 3.0;
            f *= f;
            let t = partial as f64;
            let d = it.pos;
            let pl = [
                self.player.prev_pos[0] + (self.player.pos[0] - self.player.prev_pos[0]) * t,
                self.player.prev_pos[1] + (self.player.pos[1] - self.player.prev_pos[1]) * t - 0.5,
                self.player.prev_pos[2] + (self.player.pos[2] - self.player.prev_pos[2]) * t,
            ];
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
                    (p[1] - eye[1]) as f32 - 0.125,
                    (p[2] - eye[2]) as f32,
                ],
                it.age as f32 + partial,
                it.hover,
                self.player.yaw,
                light,
            );
        }
        for it in &self.items {
            let t = partial as f64;
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
                [rel[0], rel[1] - 0.125, rel[2]],
                it.age as f32 + partial,
                it.hover,
                self.player.yaw,
                light,
            );
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
        let mut hand: Vec<crate::render::mesher::Vertex> = Vec::new();
        // EntityRenderer.renderHand: first person, GUI shown
        if !self.stats.dead && self.third_person == 0 && !self.hide_gui && self.loading == 2 {
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
            let swing = self.prev_swing_f + (self.swing_f - self.prev_swing_f) * partial;
            let using = self.using.map(|(st, n)| {
                (
                    crate::item_use::action(st.id, st.meta),
                    n as f32 - partial + 1.0,
                    crate::item_use::max_duration(st.id, st.meta),
                )
            });
            entities::hand(
                &mut hand,
                &self.atlas,
                self.equipped,
                equip,
                swing,
                light,
                using,
            );
        }

        let ui_atlas = self.atlas.clone();
        let mut ui = crate::gui::Ui::new(
            &ui_atlas,
            (window_size.0 as f64 / scale as f64).ceil() as f32,
            (window_size.1 as f64 / scale as f64).ceil() as f32,
        );
        // updateCameraAndRender: no GuiIngame with hideGUI and no screen
        let hud_hidden = self.hide_gui && self.screen == Screen::Playing;
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
        if self.mode == GameMode::Survival {
            let under = eye_mat == Material::Water;
            let mut jitter = std::mem::replace(&mut self.jitter, JavaRandom::new(0));
            jitter.set_seed(self.ticks as i64 * 312871);
            screens::survival_bars(&mut ui, &self.stats, self.ticks as u32, &mut jitter, under);
            self.jitter = jitter;
        }
        // GuiIngame: persistantChatGUI.drawChat(updateCounter)
        {
            let st = self.settings();
            self.chat
                .draw(&mut ui, &st, self.ticks as i32, self.screen == Screen::Chat);
        }
        if hud_hidden {
            ui.v.clear();
        }
        if self.loading < 2 {
            ui.v.clear();
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
        if self.show_debug {
            let p = &self.player;
            let (fx, fy, fz) = (
                math::floor_double(p.pos[0]),
                math::floor_double(p.bb.min[1]),
                math::floor_double(p.pos[2]),
            );
            let facing =
                (math::floor_double((p.yaw * 4.0f32 / 360.0f32) as f64 + 0.5) & 3) as usize;
            let left = vec![
                format!(
                    "Minecraft 1.7.10 ({} fps, {} chunk updates)",
                    self.fps, self.in_flight
                ),
                format!(
                    "C: {}/{}. F: 0, O: 0, E: 0",
                    self.renderer.mesh_count(),
                    self.world.chunks.len()
                ),
                String::new(),
                format!(
                    "x: {:.5} ({}) // c: {} ({})",
                    p.pos[0],
                    fx,
                    fx >> 4,
                    fx & 15
                ),
                format!("y: {:.3} (feet pos, {:.3} eyes pos)", p.bb.min[1], p.pos[1]),
                format!(
                    "z: {:.5} ({}) // c: {} ({})",
                    p.pos[2],
                    fz,
                    fz >> 4,
                    fz & 15
                ),
                format!(
                    "f: {} ({}) / {:.5}",
                    facing,
                    ["SOUTH", "WEST", "NORTH", "EAST"][facing],
                    math::wrap_angle_180(p.yaw)
                ),
                format!(
                    "lc: {} b: {} bl: {} sl: {} rl: {}",
                    0,
                    self.world.biome(fx, fz).name,
                    self.world.light(false, fx, fy, fz),
                    self.world.light(true, fx, fy, fz),
                    self.world
                        .light(true, fx, fy, fz)
                        .max(self.world.light(false, fx, fy, fz))
                ),
                format!(
                    "ws: {:.3}, fs: {:.3}, g: {}, fl: {}",
                    0.1, 0.05, p.on_ground, 0
                ),
            ];
            let right = vec![
                format!("Seed: {}", self.seed),
                format!("Time: {}", self.world_time),
            ];
            crate::gui::debug_overlay(&mut ui, &left, &right);
        }
        if let Some((x, y)) = std::env::var("MINERUST_MOUSE")
            .ok()
            .and_then(|v| {
                v.split_once(',')
                    .map(|(a, b)| (a.parse::<f32>(), b.parse::<f32>()))
            })
            .and_then(|(a, b)| Some((a.ok()?, b.ok()?)))
        {
            self.mouse_px = (x * scale as f32, y * scale as f32);
        }
        let m = (
            self.mouse_px.0 / scale as f32,
            self.mouse_px.1 / scale as f32,
        );
        match self.screen {
            Screen::Window if self.creative.is_some() => {
                let lines = self.effect_lines();
                let dims = (ui.w as i32, ui.h as i32, !lines.is_empty());
                let mouse = (m.0.floor() as i32, m.1.floor() as i32);
                let age = self.ticks as f32 + partial;
                let mut c = self.creative.take().unwrap();
                c.draw(
                    &mut ui,
                    &self.view,
                    dims,
                    mouse,
                    self.left_down,
                    age,
                    &self.lang,
                    &lines,
                );
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
                };
                self.gui.draw(&mut ui, &frame, &self.lang, &lines);
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
            }
            Screen::Sign => self.draw_sign_editor(&mut ui, m),
            Screen::Playing => {}
        }
        let ui_verts = ui.v;
        let outline = if self.screen == Screen::Playing {
            self.selected.map(|s| s.0)
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
        let frame = Frame {
            end_portals: &end_portals,
            crosshair: self.screen == Screen::Playing && !self.hide_gui && self.loading == 2,
            dynamic: &dynamic,
            crack: &crack,
            hand: &hand,
            weather: &weather,
            ui: &ui_verts,
            globals: &g,
            cam: eye,
            outline,
            gui_scale: scale as f32,
            screenshot: self.screenshot.take(),
            star_brightness: star,
            sunrise,
            clouds: if self.dim == 0 { Some(clouds) } else { None },
        };
        self.renderer.sync_glyphs(&self.atlas);
        self.renderer.render(&frame, window_size);
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
        gui_scale,
        screenshot,
        star_brightness: 0.0,
        sunrise: None,
        clouds: None,
        ui,
        dynamic: &[],
        crack: &[],
        hand: &[],
        weather: &[],
        end_portals: &[],
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
        menu.mouse = (
            (self.mouse_px.0 / self.gui_scale).floor(),
            (self.mouse_px.1 / self.gui_scale).floor(),
        );
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
        let a = {
            let (Some(menu), Some(shell)) = (self.menu.as_mut(), self.shell.as_mut()) else {
                return Request::None;
            };
            let mut sounds = Vec::new();
            menu.mouse = (
                (self.mouse_px.0 / self.gui_scale).floor(),
                (self.mouse_px.1 / self.gui_scale).floor(),
            );
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
