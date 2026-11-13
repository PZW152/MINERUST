//! The integrated server: owns the world and advances it at a fixed 20 ticks
//! per second on its own thread. The client only ever sees chunk snapshots and
//! talks to the simulation through commands, so a slow frame can never change
//! the outcome of a tick and a slow tick can never stall a frame.
//!
//! One tick is a pure function of the previous state and the commands applied
//! at its start: chunk loading order, population order and block update order
//! never depend on wall-clock time, hash-map iteration or thread scheduling.

use crate::block::{self, Material};
use crate::entity::survival::GameMode;
use crate::inventory::Inventory;
use crate::java::JavaRandom;
use crate::play::{InvView, ItemView, Open, Play};
use crate::render::mesher::MeshChunk;
use crate::world::save::{self, Level};
use crate::world::{BlockView, ChunkData, ChunkProvider, ServerWorld};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// New chunks brought in per tick (generated in parallel, inserted in order).
const LOADS_PER_TICK: usize = 8;
/// `MinecraftServer` autosave interval.
const AUTOSAVE_TICKS: i64 = 900;

#[path = "commands.rs"]
mod commands;
#[path = "sim_vehicles.rs"]
mod vehicles;

#[derive(Clone, Debug)]
pub enum Command {
    /// The player's chunk; loading is centred on it.
    Center(i32, i32),
    Break([i32; 3]),
    Place {
        pos: [i32; 3],
        id: u8,
        meta: i32,
    },
    /// Position, rotation and flying state (`C03PacketPlayer`).
    Player([f64; 3], [f32; 2], bool),
    /// `onGround`, sneaking and sprinting, sent before `Player`
    /// (`C03PacketPlayer.onGround`, `C0BPacketEntityAction`).
    Motion {
        on_ground: bool,
        sneaking: bool,
        sprinting: bool,
    },
    /// Survival digging finished (or a creative click) at the block.
    Dig([i32; 3]),
    /// Right click on a block face.
    Use {
        target: [i32; 3],
        face: [i32; 3],
        sneaking: bool,
    },
    Select(usize),
    /// `C0EPacketClickWindow`: container slot number (-999 outside),
    /// mouse button or hotbar key, click mode.
    WindowClick {
        slot: i32,
        button: i32,
        mode: i32,
    },
    OpenInventory,
    /// `C11PacketEnchantItem`
    EnchantItem(usize),
    /// `C17PacketCustomPayload` `MC|ItemName` (the anvil's text field)
    RenameItem(String),
    /// `C12PacketUpdateSign`
    UpdateSign([i32; 3], [String; 4]),
    /// `C01PacketChatMessage`: chat text or a command.
    Chat(String),
    /// `C10PacketCreativeInventoryAction`: a `ContainerPlayer` slot set, or
    /// (slot below 0) a stack thrown.
    CreativeSlot {
        slot: i32,
        stack: Option<crate::item::Stack>,
    },
    CloseWindow,
    DropHeld {
        all: bool,
    },
    PickBlock(u8, i32),
    SetMode(GameMode),
    /// Health, food and the like, encoded by the client for the save file.
    Stats(String),
    /// The player died: the inventory spills out.
    Died,
    Respawn,
    /// `/give`: adds the stack to the inventory.
    Give(crate::item::Stack),
    /// Left click on a mob (`attackTargetEntityWithCurrentItem`).
    Attack {
        id: u32,
        damage: f32,
        crit: bool,
        knockback: i32,
    },
    /// Whether mobs may target and hurt the player.
    PlayerState {
        attackable: bool,
    },
    /// Right click on a mob.
    Interact {
        id: u32,
    },
    /// The client has switched to the dimension it was sent.
    DimensionAck,
    /// Right click with nothing targeted (`Item.onItemRightClick`).
    UseAir,
    /// `C0CPacketInput`: `moveStrafing`, `moveForward`, jump and sneak
    /// while riding.
    Input {
        strafe: f32,
        forward: f32,
        jump: bool,
        sneak: bool,
    },
    /// `IntegratedServer`: the view distance follows the client's setting.
    ViewDistance(i32),
    /// `gameSettings.difficulty` (`IntegratedServer.tick`), by id.
    Difficulty(i32),
    /// The use key was let go while using an item
    /// (`C07PacketPlayerDigging` 5, `onPlayerStoppedUsing`).
    StopUsing,
    Shutdown,
}

pub enum Event {
    /// `S36PacketSignEditorOpen`
    OpenSign([i32; 3]),
    /// `S33PacketUpdateSign`
    SignText([i32; 3], [String; 4]),
    /// `S02PacketChat`
    Chat(Vec<crate::chat::ChatMsg>),
    /// `S2BPacketChangeGameState` (reason 3): the game mode changed.
    GameMode(GameMode),
    /// A chunk appeared or changed. `ready` means nothing unpopulated can
    /// still write into it, so it may be meshed.
    Chunk {
        key: (i32, i32),
        chunk: MeshChunk,
        ready: bool,
        /// The 16-high sections that changed (all of them for a new chunk).
        changed: u16,
    },
    /// The end of one server tick's events, with the time it finished: the
    /// client applies the tick as a whole and interpolates what the server
    /// simulates (`prev` to `pos`) from this moment over the next 50 ms.
    /// The number is how many block digs and uses the server has handled
    /// (`ACTIONS_DONE`), which settles the client's predictions of them.
    Tick(Instant, u32),
    /// The chunk left the client's range.
    Unload((i32, i32)),
    Inventory(Box<InvView>),
    /// Every dropped item near the player, sent each tick.
    Items(Vec<ItemView>),
    /// After `Respawn`: where to put the player (feet).
    Respawn([f64; 3]),
    /// Every mob and arrow near the player, sent each tick.
    Mobs(Vec<MobView>, Vec<ArrowView>),
    /// Damage and experience for the player.
    Hits(Vec<crate::entity::mob::Hit>),
    /// Sounds: name, position, volume, pitch.
    Sounds(Vec<(&'static str, [f64; 3], f32, f32)>),
    /// `S0DPacketCollectItem` for an experience orb the player took.
    CollectOrb(u32),
    /// `S27PacketExplosion`s
    Explosions(Vec<crate::world::explosion::View>),
    Weather(Weather),
    /// Primed TNT near the player: position, previous position, fuse.
    Tnt(Vec<([f64; 3], [f64; 3], i32)>),
    /// Note blocks played this tick: position, instrument, pitch 0..24.
    Notes(Vec<([i32; 3], &'static str, i32)>),
    /// Particles from mobs: kind, position, motion.
    Particles(Vec<crate::entity::mob::Puff>),
    /// The player moved to another dimension: drop every chunk and stand here.
    Dimension {
        dim: i32,
        pos: [f64; 3],
        yaw: f32,
    },
    /// Ticks spent in a Nether portal (`Entity.portalCounter`).
    Portal(i32),
    /// Moving piston blocks (`TileEntityPiston`), sent while any exist and
    /// once more when the last one stops.
    Moving(Vec<([i32; 3], crate::world::redstone::MovingView)>),
    /// The player's health, food, air, experience and effects
    /// (`S06PacketUpdateHealth`, `S1F`, `S1D`), when they change.
    PlayerStats(crate::entity::server_player::PlayerStatsView),
    /// `S12PacketEntityVelocity` (`set`) and `S27PacketExplosion`'s push (`add`).
    Velocity {
        set: Option<[f64; 3]>,
        add: [f64; 3],
    },
    /// `S19PacketEntityStatus` 2: the hurt animation and sound.
    Hurt,
    /// `setItemInUse` started (stack, ticks) or `clearItemInUse` (None).
    Using(Option<(crate::item::Stack, i32)>),
    /// `S19PacketEntityStatus` 9: the item use finished.
    UseFinished(crate::item::Stack),
    /// The player died: `CombatTracker.getDeathMessage` (key, arguments).
    Died(crate::entity::server_player::Message),
    /// `S08PacketPlayerPosLook`: the server moved the player (feet).
    Teleport([f64; 3]),
    /// `playAuxSFX(2002)`: splash potions broke (position, damage value).
    Splashes(Vec<([f64; 3], i32)>),
    /// Snowballs, eggs, pearls and potions in flight: position, previous
    /// position, item and damage.
    Throwables(Vec<([f64; 3], [f64; 3], u16, i32)>),
    /// `EntityTrackerEntry` packets about vehicles: id and packet.
    Vehicles(Vec<(u32, crate::entity::tracker::Packet)>),
    /// `S1CPacketEntityMetadata` of vehicles.
    VehicleMeta(Vec<(u32, crate::entity::vehicle::VMeta)>),
    /// `S19PacketEntityStatus` of vehicles (10: a TNT minecart ignites).
    VehicleStatus(Vec<(u32, i32)>),
    /// `S1BPacketEntityAttach` for the player: the vehicle, or none.
    Mount(Option<u32>),
    /// `playAuxSFX(2001)` of blocks the world broke: position, block, meta.
    Destroyed(Vec<([i32; 3], u8, i32)>),
    /// Falling blocks near the player, sent each tick.
    Falling(Vec<crate::entity::falling::FallingView>),
    /// The player picked these items up (`S0DPacketCollectItem`).
    Collect(Vec<crate::play::ItemView>),
    /// `TileEntityPiston.launchWithSlimeBlock` boxes and displacements, for
    /// the player's own `moveEntity`.
    Push(Vec<([f64; 3], [f64; 3], [f64; 3])>),
    /// Fireballs and thrown eyes: position, previous position, kind
    /// (0 large fireball, 1 small fireball, 2 eye of ender).
    Fireballs(Vec<([f64; 3], [f64; 3], u8)>),
}

/// `EntityEnderEye`: floats towards the stronghold for four seconds.
#[derive(Clone, Debug)]
pub struct EnderEye {
    pos: [f64; 3],
    prev: [f64; 3],
    motion: [f64; 3],
    target: [f64; 3],
    timer: i32,
    drop: bool,
}

/// A dimension the player is not in, kept loaded and saved with the rest.
struct Parked {
    dim: i32,
    world: ServerWorld,
    mobs: crate::entity::mob::Mobs,
    unsaved: std::collections::BTreeSet<(i32, i32)>,
    tiles: std::collections::BTreeMap<[i32; 3], crate::play::Tile>,
    items: Vec<crate::entity::item::ItemEntity>,
    falling: Vec<crate::entity::falling::FallingBlock>,
    vehicles: Vec<crate::entity::vehicle::Tracked>,
    teleport_rand: JavaRandom,
}

/// `C0CPacketInput` while riding.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RideInput {
    pub strafe: f32,
    pub forward: f32,
    pub jump: bool,
    pub sneak: bool,
}

/// What the client draws of a mob.
#[derive(Clone, Debug)]
pub struct MobView {
    pub id: u32,
    pub kind: crate::entity::mob::Kind,
    pub pos: [f64; 3],
    pub prev: [f64; 3],
    pub body_yaw: f32,
    pub prev_body_yaw: f32,
    pub head_yaw: f32,
    pub prev_head_yaw: f32,
    pub pitch: f32,
    pub prev_pitch: f32,
    pub limb_swing: f32,
    pub limb_amount: f32,
    pub prev_limb_amount: f32,
    pub swing: f32,
    pub prev_swing: f32,
    pub hurt_time: i32,
    pub death_time: i32,
    pub ticks: i32,
    pub variant: i32,
    pub sheared: bool,
    pub fuse: i32,
    pub prev_fuse: i32,
    pub wing: f32,
    pub prev_wing: f32,
    pub eat_timer: i32,
    pub fire: bool,
    pub bb: [f64; 6],
    /// Slime squish (previous, current).
    pub squish: [f32; 2],
    /// Squid pitch, yaw, rotation, tentacle angle (previous, current).
    pub sq: [[f32; 4]; 2],
    /// 1 tamed, 2 sitting, 4 angry or screaming, 8 hanging bat.
    pub flags: u32,
    /// Enderman carried block and data.
    pub carried: [u8; 2],
    /// Iron golem `attackTimer`.
    pub attack_timer: i32,
    pub health: f32,
    /// Ender dragon movement history (yaw, y), newest at `ring_index`.
    pub ring: Vec<[f64; 2]>,
    pub ring_index: i32,
    /// The tracker's packets as the client got them: target position and
    /// angles of `setPositionAndRotation2`, a counter that moves with every
    /// packet, and the `S19` head yaw.
    pub net_at: [f64; 3],
    pub net_yaw: f32,
    pub net_pitch: f32,
    pub net_seq: u32,
    pub net_head: f32,
    /// Client side: `rotationYaw` and `newPosRotationIncrements`.
    pub yaw: f32,
    pub incs: i32,
}

#[derive(Clone, Debug)]
pub struct ArrowView {
    pub pos: [f64; 3],
    pub prev: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
}

pub struct SimConfig {
    pub seed: i64,
    pub view: i32,
    pub save_dir: Option<PathBuf>,
    pub start_time: i64,
    pub freeze_time: bool,
    pub mode: GameMode,
}

pub struct Start {
    pub pos: [f64; 3],
    pub rot: [f32; 2],
    pub flying: bool,
    pub world_time: i64,
    pub seed: i64,
    pub resumed: bool,
    pub mode: GameMode,
    /// Saved player stats, if any.
    pub stats: Option<String>,
}

pub struct Sim {
    pub world: ServerWorld,
    centre: (i32, i32),
    view: i32,
    offsets: Vec<(i32, i32)>,
    /// The last `Event::Moving` listed some blocks.
    sent_moving: bool,
    /// An ender pearl landed: the player is moved there.
    teleport_to: Option<[f64; 3]>,
    /// Chat lines for the client (`S02PacketChat`).
    chat_out: Vec<crate::chat::ChatMsg>,
    /// `/gamemode` ran: the client is told the new mode.
    mode_changed: bool,
    /// Events from commands, sent with the tick's.
    vehicle_events: Vec<Event>,
    /// Sounds from vehicles outside their tick.
    vehicle_sounds: Vec<(&'static str, [f64; 3], f32, f32)>,
    /// `ridingEntity` of the player: a boat's id.
    riding: Option<u32>,
    /// The last `C0CPacketInput`.
    ride: RideInput,
    /// Where the player's tracker entry last re-checked the watchers.
    track_check: Option<[f64; 3]>,
    /// The item in use started or stopped this tick.
    using_changed: bool,
    /// `S27PacketExplosion` pushes for the client this tick.
    push_add: [f64; 3],
    /// `EntityPlayerMP`: the server side of the player.
    pub splayer: crate::entity::server_player::ServerPlayer,
    /// `World.difficultySetting`
    pub difficulty: crate::entity::living::Difficulty,
    /// The last `Command::Motion`.
    motion_flags: (bool, bool, bool),
    /// Chunks the client currently holds, with the `ready` flag last sent.
    sent: std::collections::BTreeMap<(i32, i32), bool>,
    /// Chunks changed since they were last written to disk.
    unsaved: std::collections::BTreeSet<(i32, i32)>,
    pub world_time: i64,
    freeze_time: bool,
    player: Option<([f64; 3], [f32; 2], bool)>,
    /// The mob the player hit last and the last one that hit the player.
    last_attacked: Option<u32>,
    last_attacker: Option<u32>,
    unknown_level_keys: Vec<(String, String)>,
    pub play: Play,
    spawn: [f64; 3],
    stats: Option<String>,
    respawn_pending: bool,
    pub mobs: crate::entity::mob::Mobs,
    player_attackable: bool,
    pending_out: Vec<crate::entity::mob::Outcome>,
    pub weather: Weather,
    /// `EntityPlayer.dimension`
    pub dim: i32,
    parked: Vec<Parked>,
    /// `Teleporter.random` of the current dimension.
    teleport_rand: JavaRandom,
    /// `Entity.portalCounter` and `timeUntilPortal`.
    portal_ticks: i32,
    portal_cooldown: i32,
    /// Player positions are stale until the client confirms a dimension change.
    await_ack: bool,
    /// The world directory (the overworld's).
    base_dir: Option<PathBuf>,
    /// A dimension change asked for by a command (respawning out of the Nether).
    pending_travel: Option<i32>,
    /// Tell the client on the first tick which dimension the save is in.
    announce_dim: bool,
    eyes: Vec<EnderEye>,
    /// `level.dat` of a world opened from vanilla (Anvil mode).
    anvil_level: Option<crate::world::anvil::Nbt>,
    /// Chunks whose tile entities and entities were taken over already.
    anvil_imported: std::collections::BTreeSet<(i32, i32, i32)>,
}

/// Level keys owned by the gameplay state (kept out of `unknown`).
const PLAY_KEYS: [&str; 7] = [
    "inventory",
    "mode",
    "tile",
    "spawn",
    "stats",
    "weather",
    "dim",
];

/// `WorldInfo` rain and thunder state and `World.updateWeather`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Weather {
    pub raining: bool,
    pub rain_time: i32,
    pub thundering: bool,
    pub thunder_time: i32,
    pub rain: f32,
    pub prev_rain: f32,
    pub thunder: f32,
    pub prev_thunder: f32,
}

impl Weather {
    pub fn update(&mut self, r: &mut JavaRandom) {
        if self.thunder_time <= 0 {
            self.thunder_time = if self.thundering {
                r.next_int_n(12000) + 3600
            } else {
                r.next_int_n(168000) + 12000
            };
        } else {
            self.thunder_time -= 1;
            if self.thunder_time <= 0 {
                self.thundering = !self.thundering;
            }
        }
        self.prev_thunder = self.thunder;
        self.thunder = (self.thunder + if self.thundering { 0.01 } else { -0.01 }).clamp(0.0, 1.0);
        if self.rain_time <= 0 {
            self.rain_time = if self.raining {
                r.next_int_n(12000) + 12000
            } else {
                r.next_int_n(168000) + 12000
            };
        } else {
            self.rain_time -= 1;
            if self.rain_time <= 0 {
                self.raining = !self.raining;
            }
        }
        self.prev_rain = self.rain;
        self.rain = (self.rain + if self.raining { 0.01 } else { -0.01 }).clamp(0.0, 1.0);
    }
    fn encode(&self) -> String {
        format!(
            "{},{},{},{},{},{}",
            self.raining as i32,
            self.rain_time,
            self.thundering as i32,
            self.thunder_time,
            self.rain,
            self.thunder
        )
    }
    fn decode(s: &str) -> Option<Weather> {
        let f: Vec<&str> = s.split(',').collect();
        if f.len() != 6 {
            return None;
        }
        let rain: f32 = f[4].parse().ok()?;
        let thunder: f32 = f[5].parse().ok()?;
        Some(Weather {
            raining: f[0] == "1",
            rain_time: f[1].parse().ok()?,
            thundering: f[2] == "1",
            thunder_time: f[3].parse().ok()?,
            rain,
            prev_rain: rain,
            thunder,
            prev_thunder: thunder,
        })
    }
}

fn spiral(radius: i32) -> Vec<(i32, i32)> {
    let mut v = Vec::new();
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            v.push((dx, dz));
        }
    }
    // Total order: distance first, then coordinates, so ties never depend on
    // the sort implementation.
    v.sort_by_key(|&(x, z)| (x * x + z * z, x, z));
    v
}

impl Sim {
    pub fn new(cfg: &SimConfig) -> (Sim, Start) {
        let level = cfg.save_dir.as_ref().and_then(|d| save::read_level(d));
        // A vanilla world: level.dat and region files, no level.txt.
        let anvil = if level.is_none() {
            cfg.save_dir
                .as_ref()
                .and_then(|d| crate::world::anvil::read_level_dat(d))
        } else {
            None
        };
        let anvil_seed = anvil
            .as_ref()
            .and_then(|r| r.get("Data"))
            .and_then(|d| d.int("RandomSeed"));
        let seed = level
            .as_ref()
            .map_or(anvil_seed.unwrap_or(cfg.seed), |l| l.seed);
        if let Some(l) = &level {
            crate::compat::set_vanilla_parity(l.vanilla_parity);
        }
        let provider = Arc::new(ChunkProvider::new(seed));
        let mut world = ServerWorld::new(provider);
        world.save_dir = cfg.save_dir.clone();
        world.anvil = anvil.is_some();
        if let Some(d) = &cfg.save_dir {
            // data/structures.txt: the MapGenStructureData of vanilla.
            if let Ok(text) = std::fs::read_to_string(d.join("data").join("structures.txt")) {
                if let Some(st) = world.structures.as_mut() {
                    if st.load(text.trim()).is_none() {
                        eprintln!("structure data is damaged; structures will be re-derived");
                    }
                }
            }
        }
        let mut sim = Sim {
            world,
            centre: (0, 0),
            view: cfg.view,
            offsets: spiral(cfg.view + 2),
            sent_moving: false,
            splayer: crate::entity::server_player::ServerPlayer::new(),
            difficulty: crate::entity::living::Difficulty::Normal,
            motion_flags: (false, false, false),
            push_add: [0.0; 3],
            using_changed: false,
            teleport_to: None,
            chat_out: Vec::new(),
            mode_changed: false,
            vehicle_events: Vec::new(),
            vehicle_sounds: Vec::new(),
            riding: None,
            ride: RideInput::default(),
            track_check: None,
            sent: Default::default(),
            unsaved: Default::default(),
            world_time: cfg.start_time,
            freeze_time: cfg.freeze_time,
            player: None,
            last_attacked: None,
            last_attacker: None,
            unknown_level_keys: Vec::new(),
            play: Play::new(cfg.mode),
            spawn: [0.0; 3],
            stats: None,
            respawn_pending: false,
            mobs: crate::entity::mob::Mobs::default(),
            player_attackable: true,
            pending_out: Vec::new(),
            weather: Weather::default(),
            dim: 0,
            parked: Vec::new(),
            teleport_rand: JavaRandom::new(seed),
            portal_ticks: 0,
            portal_cooldown: 0,
            await_ack: false,
            base_dir: cfg.save_dir.clone(),
            pending_travel: None,
            announce_dim: false,
            eyes: Vec::new(),
            anvil_level: None,
            anvil_imported: Default::default(),
        };
        if let Some(d) = &cfg.save_dir {
            if let Ok(text) = std::fs::read_to_string(d.join("data").join("entities.txt")) {
                for line in text.lines() {
                    let _ = sim.mobs.decode(line);
                }
            }
            if let Ok(b) = std::fs::read(d.join("data").join("drops.dat")) {
                let mut r = JavaRandom::new(cfg.seed);
                sim.play.load_drops(&b, &mut r);
            }
        }
        let mut saved_dim = 0;
        sim.anvil_level = anvil;
        let anvil_start = sim
            .anvil_level
            .clone()
            .map(|root| sim.start_from_level_dat(&root, &mut saved_dim));
        let start = match level {
            _ if anvil_start.is_some() => anvil_start.unwrap(),
            Some(l) => {
                sim.world.total_time = l.total_time;
                sim.world_time = l.world_time;
                for t in &l.ticks {
                    sim.world.restore_tick(t.0, t.1, t.2, t.3, t.4);
                }
                sim.unknown_level_keys = l
                    .unknown
                    .iter()
                    .filter(|(k, _)| !PLAY_KEYS.contains(&k.as_str()))
                    .cloned()
                    .collect();
                let mut spawn = None;
                for (k, v) in &l.unknown {
                    match k.as_str() {
                        "dim" => saved_dim = v.parse().unwrap_or(0),
                        "inventory" => {
                            if let Some(inv) = Inventory::decode(v) {
                                sim.play.inv = inv;
                            }
                        }
                        "mode" => {
                            sim.play.mode = if v == "creative" {
                                GameMode::Creative
                            } else {
                                GameMode::Survival
                            }
                        }
                        "tile" => {
                            let _ = sim.play.decode_tile(v);
                        }
                        "spawn" => {
                            let f: Vec<f64> = v.split(',').filter_map(|x| x.parse().ok()).collect();
                            if f.len() == 3 {
                                spawn = Some([f[0], f[1], f[2]]);
                            }
                        }
                        "stats" => {
                            sim.stats = Some(v.clone());
                            sim.splayer.decode(v);
                        }
                        "weather" => {
                            if let Some(w) = Weather::decode(v) {
                                sim.weather = w;
                            }
                        }
                        _ => {}
                    }
                }
                sim.player = l.player;
                sim.spawn = match spawn {
                    Some(s) => s,
                    None => sim.find_spawn(),
                };
                let (pos, rot, flying) = l.player.unwrap_or((sim.spawn, [0.0, 0.0], false));
                Start {
                    pos,
                    rot,
                    flying,
                    world_time: l.world_time,
                    seed,
                    resumed: true,
                    mode: sim.play.mode,
                    stats: sim.stats.clone(),
                }
            }
            None => {
                let pos = sim.find_spawn();
                sim.spawn = pos;
                Start {
                    pos,
                    rot: [0.0, 0.0],
                    flying: false,
                    world_time: cfg.start_time,
                    seed,
                    resumed: false,
                    mode: sim.play.mode,
                    stats: None,
                }
            }
        };
        // `/weather`: forced from the command line.
        if let Ok(w) = std::env::var("MINERUST_WEATHER") {
            let on = w == "rain" || w == "thunder";
            sim.weather.raining = on;
            sim.weather.thundering = w == "thunder";
            sim.weather.rain_time = if on { 24000 } else { 168000 };
            sim.weather.thunder_time = if w == "thunder" { 24000 } else { 168000 };
            sim.weather.rain = if on { 1.0 } else { 0.0 };
            sim.weather.prev_rain = sim.weather.rain;
            sim.weather.thunder = if w == "thunder" { 1.0 } else { 0.0 };
            sim.weather.prev_thunder = sim.weather.thunder;
        }
        sim.play.player = (start.pos, start.rot[0]);
        if let Some(d) = std::env::var("MINERUST_DIM")
            .ok()
            .and_then(|v| v.parse::<i32>().ok())
        {
            if d != saved_dim {
                sim.pending_travel = Some(d);
            }
        }
        if saved_dim != 0 {
            // the player was saved in the Nether or the End
            let next = sim.take_world(saved_dim);
            let old = Parked {
                dim: 0,
                world: std::mem::replace(&mut sim.world, next.world),
                mobs: std::mem::replace(&mut sim.mobs, next.mobs),
                unsaved: std::mem::replace(&mut sim.unsaved, next.unsaved),
                tiles: std::mem::replace(&mut sim.play.tiles, next.tiles),
                items: std::mem::replace(&mut sim.play.items, next.items),
                falling: std::mem::replace(&mut sim.play.falling, next.falling),
                vehicles: std::mem::replace(&mut sim.play.vehicles, next.vehicles),
                teleport_rand: std::mem::replace(&mut sim.teleport_rand, next.teleport_rand),
            };
            sim.world.total_time = old.world.total_time;
            sim.parked.push(old);
            sim.dim = saved_dim;
            sim.announce_dim = true;
        }
        sim.centre = (
            (start.pos[0].floor() as i32) >> 4,
            (start.pos[2].floor() as i32) >> 4,
        );
        (sim, start)
    }

    /// `WorldServer.createSpawnPosition` followed by the player placement of
    /// `EntityPlayerMP` (without its random offset). Returns the feet position.
    fn find_spawn(&mut self) -> [f64; 3] {
        let provider = self.world.provider.clone();
        let world = &mut self.world;
        let (mut sx, mut sz) = {
            let mut top_is_grass = |x: i32, z: i32| {
                let mut k = 63;
                while !world.is_air(x, k + 1, z) {
                    k += 1;
                }
                world.block(x, k, z) == block::GRASS
            };
            provider.find_spawn(&mut top_is_grass)
        };
        let dry = |w: &mut ServerWorld, x: i32, z: i32| {
            let y = w.top_solid_or_liquid(x, z);
            y > 0 && !w.material(x, y, z).is_liquid() && !w.material(x, y - 1, z).is_liquid()
        };
        if crate::compat::fixed() && !dry(world, sx, sz) {
            // MC-7908: when the search above gives up, vanilla leaves the
            // player wherever it stopped, possibly in the middle of an ocean.
            'search: for ring in 1..128i32 {
                for a in -ring..=ring {
                    for (dx, dz) in [(a, -ring), (a, ring), (-ring, a), (ring, a)] {
                        let (x, z) = (sx + dx * 4, sz + dz * 4);
                        if dry(world, x, z) {
                            sx = x;
                            sz = z;
                            break 'search;
                        }
                    }
                }
            }
        }
        let mut y = world.top_solid_or_liquid(sx, sz).max(1);
        let (px, pz) = (sx as f64 + 0.5, sz as f64 + 0.5);
        // Move up while the bounding box is obstructed.
        loop {
            let bb = crate::entity::player::Aabb {
                min: [px - 0.3, y as f64, pz - 0.3],
                max: [px + 0.3, y as f64 + 1.8, pz + 0.3],
            };
            if crate::entity::player::colliding_boxes(&*world as &dyn BlockView, &bb).is_empty()
                || y >= 255
            {
                break;
            }
            y += 1;
        }
        [px, y as f64, pz]
    }

    fn ready(&self, cx: i32, cz: i32) -> bool {
        for dx in -1..=0 {
            for dz in -1..=0 {
                match self.world.chunks.get(&(cx + dx, cz + dz)) {
                    Some(c) if c.terrain_populated => {}
                    _ => return false,
                }
            }
        }
        true
    }

    fn snapshot(&self, key: (i32, i32)) -> Option<MeshChunk> {
        let c = self.world.chunks.get(&key)?;
        Some(MeshChunk {
            data: c.data.clone(),
            sections: c.sections,
            height: Box::new(c.height),
        })
    }

    /// Applies one player command with server-side validation.
    fn apply(&mut self, cmd: &Command) {
        match *cmd {
            Command::Center(x, z) => self.centre = (x, z),
            Command::ViewDistance(v) => {
                self.view = v;
                self.offsets = spiral(v + 2);
            }
            Command::Difficulty(d) => {
                use crate::entity::living::Difficulty as D;
                self.difficulty = match d {
                    0 => D::Peaceful,
                    1 => D::Easy,
                    3 => D::Hard,
                    _ => D::Normal,
                };
            }
            Command::Motion {
                on_ground,
                sneaking,
                sprinting,
            } => self.motion_flags = (on_ground, sneaking, sprinting),
            Command::Player(p, r, f) => {
                if self.await_ack {
                    return;
                }
                if self.riding.is_some() {
                    // NetHandlerPlayServer.processPlayer, riding: only the look
                    // and onGround; the boat places the player.
                    let feet = self.play.player.0;
                    self.player = Some((feet, r, f));
                    self.play.player.1 = r[0];
                    self.play.pitch = r[1];
                    self.splayer.e.on_ground = self.motion_flags.0;
                    let _ = p;
                    return;
                }
                let (g, sn, sp) = self.motion_flags;
                self.splayer.moved(&self.world, p, g, f, sn, sp);
                self.player = Some((p, r, f));
                self.play.player = (p, r[0]);
                self.play.pitch = r[1];
            }
            Command::DimensionAck => self.await_ack = false,
            Command::UseAir => self.use_air(),
            Command::StopUsing => {
                // EntityPlayer.stopUsingItem
                if let Some((_, st, left)) = self.splayer.using.take() {
                    self.stopped_using(st, left);
                }
            }
            Command::Break(p) => {
                let id = self.world.block(p[0], p[1], p[2]);
                if id != block::AIR && id != block::BEDROCK && !block::material(id).is_liquid() {
                    self.world.set_block(p[0], p[1], p[2], block::AIR, 0, 3);
                }
            }
            Command::Place { pos: p, id, meta } => {
                if !(0..256).contains(&p[1]) || !block::info(id).exists || id == block::AIR {
                    return;
                }
                let w = &mut self.world;
                if !w.material(p[0], p[1], p[2]).replaceable() {
                    return;
                }
                // `ItemBlock.onItemUse`: the block decides whether it can
                // exist there (MC-1168: a lily pad must sit on still water and
                // never replaces it).
                if !w.can_place_block_at(id, p[0], p[1], p[2]) {
                    return;
                }
                let needs_support = matches!(
                    block::info(id).material,
                    Material::Plants | Material::Vine | Material::Cactus
                );
                if needs_support
                    && id != block::DOUBLE_PLANT
                    && id != block::VINE
                    && !w.can_block_stay(id, p[0], p[1], p[2])
                {
                    return;
                }
                if id == block::DOUBLE_PLANT {
                    w.set_block(p[0], p[1], p[2], id, meta & 7, 3);
                    w.set_block(p[0], p[1] + 1, p[2], id, 8, 3);
                } else {
                    w.set_block(p[0], p[1], p[2], id, meta, 3);
                }
            }
            Command::Dig(p) => {
                ACTIONS_DONE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let mut play = std::mem::replace(&mut self.play, Play::new(GameMode::Survival));
                let id = self.world.block(p[0], p[1], p[2]);
                play.dig(&mut self.world, p);
                // Block.harvestBlock: player.addExhaustion(0.025)
                if play.mode == GameMode::Survival
                    && id != crate::block::AIR
                    && self.world.block(p[0], p[1], p[2]) != id
                {
                    self.splayer.food.add_exhaustion(0.025);
                }
                self.play = play;
            }
            Command::Use {
                target,
                face,
                sneaking,
            } => {
                ACTIONS_DONE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                // ItemMinecart.onItemUse (rails have no onBlockActivated)
                if self.place_cart(target) {
                    return;
                }
                let mut play = std::mem::replace(&mut self.play, Play::new(GameMode::Survival));
                let used = play.use_block(&mut self.world, target, face, sneaking);
                self.play = play;
                if !used {
                    // Minecraft.func_147121_ag: sendUseItem
                    self.use_air();
                }
            }
            Command::Select(i) => {
                if i < 9 && self.play.inv.selected != i {
                    self.play.inv.selected = i;
                    self.play.inv_dirty = true;
                }
            }
            Command::Chat(ref t) => {
                let t = t.clone();
                self.chat_message(&t);
            }
            Command::EnchantItem(k) => {
                let mut play = std::mem::replace(&mut self.play, Play::new(GameMode::Survival));
                let lvl = self.splayer.xp_level;
                if let Some(cost) = play.enchant_item(&mut self.world, k, lvl) {
                    // EntityPlayer.addExperienceLevel(-cost)
                    let p = &mut self.splayer;
                    p.xp_level -= cost;
                    if p.xp_level < 0 {
                        p.xp_level = 0;
                        p.xp = 0.0;
                        p.xp_total = 0;
                    }
                }
                self.play = play;
            }
            Command::UpdateSign(p, ref lines) => {
                let l = lines.clone();
                self.play.update_sign(p, l);
            }
            Command::CreativeSlot { slot, stack } => {
                let mut play = std::mem::replace(&mut self.play, Play::new(GameMode::Survival));
                play.creative_slot(&mut self.world, slot, stack);
                self.play = play;
            }
            Command::WindowClick { slot, button, mode } => {
                let mut play = std::mem::replace(&mut self.play, Play::new(GameMode::Survival));
                play.xp_level = self.splayer.xp_level;
                let spent = play.window_click(&mut self.world, slot, button, mode);
                self.play = play;
                if spent != 0 {
                    // EntityPlayer.addExperienceLevel(-cost)
                    let p = &mut self.splayer;
                    p.xp_level -= spent;
                    if p.xp_level < 0 {
                        p.xp_level = 0;
                        p.xp = 0.0;
                        p.xp_total = 0;
                    }
                    self.play.xp_level = p.xp_level;
                }
            }
            Command::RenameItem(ref n) => {
                let n = n.clone();
                self.play.xp_level = self.splayer.xp_level;
                self.play.rename(&n);
            }
            Command::OpenInventory => {
                self.play.open = Open::Inventory;
                self.play.inv.grid_size = 2;
                self.play.inv_dirty = true;
            }
            Command::CloseWindow => {
                let mut play = std::mem::replace(&mut self.play, Play::new(GameMode::Survival));
                play.close(&mut self.world);
                self.play = play;
            }
            Command::DropHeld { all } => {
                let mut play = std::mem::replace(&mut self.play, Play::new(GameMode::Survival));
                play.drop_held(&mut self.world, all);
                self.play = play;
            }
            Command::PickBlock(id, meta) => self.play.pick_block(id, meta),
            Command::SetMode(m) => {
                self.play.mode = m;
                self.play.inv_dirty = true;
            }
            Command::Stats(_) => {}
            Command::Died => {}
            Command::Respawn => {
                self.splayer.respawn();
                self.respawn_pending = true;
                if self.dim != 0 {
                    self.pending_travel = Some(0);
                }
            }
            Command::Shutdown => {}
            _ => self.apply_more(cmd),
        }
    }

    /// `EntityPlayer.onDeath`: the inventory spills out.
    fn drop_inventory(&mut self) {
        {
            {
                let mut play = std::mem::replace(&mut self.play, Play::new(GameMode::Survival));
                if play.mode == GameMode::Survival {
                    let mut rnd =
                        std::mem::replace(&mut self.world.rand, crate::java::JavaRandom::new(0));
                    play.close(&mut self.world);
                    for i in 0..crate::inventory::MAIN {
                        if let Some(st) = play.inv.main[i].take() {
                            play.throw_with(st, true, &mut rnd);
                        }
                    }
                    self.world.rand = rnd;
                    play.inv_dirty = true;
                }
                self.play = play;
            }
        }
    }

    fn apply_more(&mut self, cmd: &Command) {
        match *cmd {
            Command::Attack {
                id,
                damage,
                knockback,
                ..
            } if id & crate::entity::vehicle::VEHICLE_ID != 0 => {
                // attackTargetEntityWithCurrentItem: addExhaustion(0.3)
                self.splayer.food.add_exhaustion(0.3);
                self.attack_vehicle(id, damage, knockback);
            }
            Command::Interact { id } if id & crate::entity::vehicle::VEHICLE_ID != 0 => {
                self.interact_vehicle(id);
            }
            Command::Input {
                strafe,
                forward,
                jump,
                sneak,
            } => self.ride_input(strafe, forward, jump, sneak),
            Command::Attack {
                id,
                damage,
                crit,
                knockback,
            } => {
                self.last_attacked = Some(id);
                // attackTargetEntityWithCurrentItem: addExhaustion(0.3)
                self.splayer.food.add_exhaustion(0.3);
                let mut out = crate::entity::mob::Outcome::default();
                let (feet, yaw) = self.play.player;
                self.mobs
                    .attack(id, damage, crit, feet, yaw, knockback, &mut out);
                self.pending_out.push(out);
            }
            Command::PlayerState { attackable } => self.player_attackable = attackable,
            Command::Interact { id } => {
                let mut out = crate::entity::mob::Outcome::default();
                let held = self.play.inv.held();
                let (feet, _) = self.play.player;
                if let Some(next) = self.mobs.interact(id, held, feet, &mut out) {
                    let i = self.play.inv.selected;
                    if self.play.mode == GameMode::Survival || held.map_or(false, |s| s.id == 325) {
                        self.play.inv.main[i] = next;
                    }
                    self.play.inv_dirty = true;
                }
                self.pending_out.push(out);
            }
            Command::Give(s) => {
                if s.count > 0 && s.count <= 64 {
                    let _ = self.play.inv.add(s);
                    self.play.inv_dirty = true;
                }
            }
            Command::Shutdown => {}
            _ => {}
        }
    }

    /// Brings in missing chunks around the centre, nearest first.
    fn load_chunks(&mut self) {
        let mut wanted: Vec<(i32, i32)> = Vec::with_capacity(LOADS_PER_TICK);
        for &(dx, dz) in &self.offsets {
            let key = (self.centre.0 + dx, self.centre.1 + dz);
            if !self.world.chunk_exists(key.0, key.1) {
                wanted.push(key);
                if wanted.len() == LOADS_PER_TICK {
                    break;
                }
            }
        }
        if wanted.is_empty() {
            return;
        }
        if self.world.provider.dim == -1 {
            // ChunkProviderHell shares one generator between provideChunk and
            // populate, so the order of generation matters: one at a time.
            for key in wanted {
                self.world.load_chunk(key.0, key.1);
            }
            return;
        }
        // Generate the new ones in parallel; the terrain of a chunk depends
        // only on the seed and its coordinates.
        let provider = self.world.provider.clone();
        let fresh: Vec<(i32, i32)> = wanted
            .iter()
            .copied()
            .filter(|k| !self.world.is_stored(k.0, k.1))
            .collect();
        let generated: Vec<ChunkData> = std::thread::scope(|s| {
            let handles: Vec<_> = fresh
                .iter()
                .map(|&(cx, cz)| {
                    let p = provider.clone();
                    s.spawn(move || p.provide_chunk(cx, cz))
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().expect("chunk generation panicked"))
                .collect()
        });
        let mut generated = generated.into_iter();
        // Insert strictly in the order they were requested.
        for key in wanted {
            if self.world.chunk_exists(key.0, key.1) {
                // Pulled in by a neighbour's population in the meantime.
                if fresh.contains(&key) {
                    generated.next();
                }
                continue;
            }
            if fresh.contains(&key) {
                let data = generated.next().expect("generated chunk missing");
                self.world.insert_generated(data);
            } else {
                self.world.load_chunk(key.0, key.1);
            }
        }
    }

    fn unload_far(&mut self) {
        let r = self.view + 4;
        let mut far: Vec<(i32, i32)> = self
            .world
            .chunks
            .keys()
            .copied()
            .filter(|&(x, z)| (x - self.centre.0).abs() > r || (z - self.centre.1).abs() > r)
            .collect();
        far.sort();
        for key in far {
            self.world.unload_chunk(key.0, key.1);
            self.unsaved.remove(&key);
        }
    }

    /// Advances the world by one tick and reports what the client must know.
    pub fn step(&mut self, commands: &[Command], events: &mut Vec<Event>) {
        self.splayer.living.armor = self.play.inv.armor_value();
        for c in commands {
            self.apply(c);
        }
        // NetHandlerPlayServer.onNetworkTick
        if self.play.creative_drops > 0 {
            self.play.creative_drops -= 1;
        }
        self.splayer.creative = self.play.mode == GameMode::Creative;
        if !self.respawn_pending {
            let d = self.difficulty;
            self.splayer.tick(&self.world, d, true);
        }
        let mut use_events = Vec::new();
        self.tick_item_use(&mut use_events);
        events.extend(use_events);
        self.load_chunks();
        if self.world.anvil {
            self.import_anvil();
        }
        self.summon_once();
        if !self.freeze_time {
            self.world_time += 1;
        }
        {
            let mut r = std::mem::replace(&mut self.world.rand, JavaRandom::new(0));
            self.weather.update(&mut r);
            self.world.rand = r;
            self.world.raining = self.dim == 0 && self.weather.rain > 0.2;
        }
        self.world.skylight_subtracted = match self.dim {
            // WorldProviderHell.calculateCelestialAngle is always 0.5 (midnight),
            // WorldProviderEnd's always 0.0 (noon).
            -1 => 11,
            1 => 0,
            _ => skylight_subtracted_weather(
                self.world_time,
                self.weather.rain,
                self.weather.thunder,
            ),
        };
        let mut shown = self.weather;
        if self.dim != 0 {
            shown.rain = 0.0;
            shown.prev_rain = 0.0;
            shown.thunder = 0.0;
            shown.prev_thunder = 0.0;
        }
        events.push(Event::Weather(shown));
        self.tick_portal(events);
        self.world.tick();
        let radius = self.view.min(7);
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                self.world
                    .tick_chunk(self.centre.0 + dx, self.centre.1 + dz);
                self.world
                    .update_blocks(self.centre.0 + dx, self.centre.1 + dz);
            }
        }
        // WorldServer.func_147488_Z
        self.world.run_block_events();
        if self.world.total_time % 40 == 0 {
            self.unload_far();
        }
        let mut play = std::mem::replace(&mut self.play, Play::new(GameMode::Survival));
        play.tick(&mut self.world);
        // Entity contact with pressure plates and tripwire.
        {
            use crate::world::redstone::{Aabb6, Occupant};
            let mut occ = Vec::new();
            let (f, _) = play.player;
            if !self.respawn_pending {
                occ.push(Occupant {
                    bb: Aabb6 {
                        min: [f[0] - 0.3, f[1], f[2] - 0.3],
                        max: [f[0] + 0.3, f[1] + 1.8, f[2] + 0.3],
                    },
                    living: true,
                    minecart: false,
                });
            }
            for m in &self.mobs.list {
                if !m.dead {
                    occ.push(Occupant {
                        bb: Aabb6 {
                            min: m.bb.min,
                            max: m.bb.max,
                        },
                        living: true,
                        minecart: false,
                    });
                }
            }
            for it in &play.items {
                let b = it.bb();
                occ.push(Occupant {
                    bb: Aabb6 {
                        min: b.min,
                        max: b.max,
                    },
                    living: false,
                    minecart: false,
                });
            }
            for a in &self.mobs.arrows {
                occ.push(Occupant {
                    bb: Aabb6 {
                        min: a.e.bb.min,
                        max: a.e.bb.max,
                    },
                    living: false,
                    minecart: false,
                });
            }
            for t in &self.mobs.tnt {
                occ.push(Occupant {
                    bb: Aabb6 {
                        min: [t.pos[0] - 0.49, t.pos[1] - 0.49, t.pos[2] - 0.49],
                        max: [t.pos[0] + 0.49, t.pos[1] + 0.49, t.pos[2] + 0.49],
                    },
                    living: false,
                    minecart: false,
                });
            }
            for b in &play.vehicles {
                let e = b.v.e();
                occ.push(Occupant {
                    bb: Aabb6 {
                        min: e.bb.min,
                        max: e.bb.max,
                    },
                    living: false,
                    minecart: matches!(b.v, crate::entity::vehicle::Vehicle::Cart(_)),
                });
            }
            self.world.occupants = occ;
            self.world.redstone_entity_tick();
        }
        for t in std::mem::take(&mut self.world.gen_tiles) {
            play.add_gen_tile(t);
        }
        let mut rnd = std::mem::replace(&mut self.world.rand, JavaRandom::new(0));
        for e in std::mem::take(&mut self.world.gen_entities) {
            use crate::world::server::GenEntity;
            match e {
                GenEntity::Villager { pos, profession } => {
                    let id =
                        self.mobs
                            .spawn(crate::entity::mob::Kind::Villager, pos, 0.0, &mut rnd);
                    if let Some(m) = self.mobs.list.iter_mut().find(|m| m.id == id) {
                        m.variant = profession;
                        m.persistent = true;
                    }
                }
                GenEntity::Witch { pos } => {
                    let id = self
                        .mobs
                        .spawn(crate::entity::mob::Kind::Witch, pos, 0.0, &mut rnd);
                    if let Some(m) = self.mobs.list.iter_mut().find(|m| m.id == id) {
                        m.persistent = true;
                    }
                }
                GenEntity::Dragon { pos, yaw } => {
                    let id =
                        self.mobs
                            .spawn(crate::entity::mob::Kind::EnderDragon, pos, yaw, &mut rnd);
                    if let Some(m) = self.mobs.list.iter_mut().find(|m| m.id == id) {
                        m.persistent = true;
                    }
                }
                GenEntity::Crystal { pos, yaw } => {
                    let id =
                        self.mobs
                            .spawn(crate::entity::mob::Kind::EnderCrystal, pos, yaw, &mut rnd);
                    if let Some(m) = self.mobs.list.iter_mut().find(|m| m.id == id) {
                        m.persistent = true;
                    }
                }
                GenEntity::Mob { kind, pos, variant } => {
                    let id = self.mobs.spawn(kind, pos, 0.0, &mut rnd);
                    if let Some(m) = self.mobs.list.iter_mut().find(|m| m.id == id) {
                        m.variant = variant;
                        m.persistent = true;
                    }
                }
                GenEntity::Animal { kind, pos, yaw } => {
                    let id = self.mobs.spawn(kind, pos, yaw, &mut rnd);
                    if let Some(m) = self.mobs.list.iter_mut().find(|m| m.id == id) {
                        m.persistent = true;
                    }
                }
                other => play.add_gen_entity(other, &mut rnd),
            }
        }
        self.world.rand = rnd;
        self.tick_mobs(&mut play, events);
        self.tick_vehicles(&mut play, events);
        // World.updateEntities: tile entities after the entities.
        self.world.tick_moving();
        self.apply_pushes(&mut play, events);
        {
            use crate::entity::base::Fx;
            let mut sounds = Vec::new();
            let mut parts = Vec::new();
            for f in play.fx.drain(..) {
                match f {
                    Fx::Sound(n, p, v, pi) => sounds.push((n, p, v, pi)),
                    Fx::Particle(n, p, m) => parts.push((n, p, m)),
                    _ => {}
                }
            }
            parts.extend(self.world.particles.drain(..));
            if !sounds.is_empty() {
                events.push(Event::Sounds(sounds));
            }
            if !parts.is_empty() {
                events.push(Event::Particles(parts));
            }
            if !play.collected.is_empty() {
                events.push(Event::Collect(std::mem::take(&mut play.collected)));
            }
        }
        let moving: Vec<([i32; 3], crate::world::redstone::MovingView)> = self
            .world
            .moving
            .iter()
            .map(|m| (m.pos, m.view()))
            .collect();
        if !moving.is_empty() || self.sent_moving {
            self.sent_moving = !moving.is_empty();
            events.push(Event::Moving(moving));
        }
        self.play = play;
        self.pick_up_arrows(events);
        self.player_events(events);
        if self.play.inv_dirty {
            self.play.inv_dirty = false;
            events.push(Event::Inventory(Box::new(self.play.view())));
        }
        events.push(Event::Items(self.play.item_views()));
        events.push(Event::Throwables(
            self.mobs
                .throwables
                .iter()
                .map(|t| {
                    let (id, m) = t.kind.item();
                    (t.e.pos, t.e.prev, id, m)
                })
                .collect(),
        ));
        events.push(Event::Falling(
            self.play.falling.iter().map(|f| f.view()).collect(),
        ));
        if self.respawn_pending {
            self.respawn_pending = false;
            self.centre = (
                (self.spawn[0].floor() as i32) >> 4,
                (self.spawn[2].floor() as i32) >> 4,
            );
            events.push(Event::Respawn(self.spawn));
        }

        // Changes, in a fixed order.
        let mut dirty: Vec<((i32, i32), u16)> = self.world.dirty.drain().collect();
        dirty.sort();
        for (key, _) in &dirty {
            self.unsaved.insert(*key);
        }
        let range = self.view + 2;
        let in_range =
            |c: (i32, i32), k: (i32, i32)| (k.0 - c.0).abs() <= range && (k.1 - c.1).abs() <= range;
        let gone: Vec<(i32, i32)> = self
            .sent
            .keys()
            .copied()
            .filter(|&k| !in_range(self.centre, k) || !self.world.chunk_exists(k.0, k.1))
            .collect();
        for key in gone {
            self.sent.remove(&key);
            events.push(Event::Unload(key));
        }
        for (key, changed) in dirty {
            if in_range(self.centre, key) && self.world.chunk_exists(key.0, key.1) {
                let ready = self.ready(key.0, key.1);
                // a chunk the client has not seen yet is new as a whole
                let changed = if self.sent.insert(key, ready).is_some() {
                    changed
                } else {
                    0xFFFF
                };
                events.push(Event::Chunk {
                    key,
                    chunk: self.snapshot(key).unwrap(),
                    ready,
                    changed,
                });
                for (p, t) in self.play.signs_in(key.0, key.1) {
                    events.push(Event::SignText(p, t));
                }
            }
        }
        // Chunks that entered the range or became ready without changing.
        for &(dx, dz) in &self.offsets {
            let key = (self.centre.0 + dx, self.centre.1 + dz);
            if !self.world.chunk_exists(key.0, key.1) {
                continue;
            }
            let ready = self.ready(key.0, key.1);
            if self.sent.get(&key) != Some(&ready) {
                let known = self.sent.insert(key, ready).is_some();
                events.push(Event::Chunk {
                    key,
                    chunk: self.snapshot(key).unwrap(),
                    ready,
                    changed: if known { 0 } else { 0xFFFF },
                });
                for (p, t) in self.play.signs_in(key.0, key.1) {
                    events.push(Event::SignText(p, t));
                }
            }
        }
        if self.world.save_dir.is_some() && self.world.total_time % AUTOSAVE_TICKS == 0 {
            if let Err(e) = self.save() {
                eprintln!("autosave failed: {e}");
            }
        }
    }

    /// Reads `level.dat` (`WorldInfo`, the player of a single-player world).
    fn start_from_level_dat(&mut self, root: &crate::world::anvil::Nbt, dim: &mut i32) -> Start {
        use crate::world::anvil::Nbt;
        let d = root
            .get("Data")
            .cloned()
            .unwrap_or(Nbt::Compound(Vec::new()));
        self.world.total_time = d.int("Time").unwrap_or(0);
        self.world_time = d.int("DayTime").unwrap_or(self.world.total_time);
        self.weather.raining = d.int("raining").unwrap_or(0) != 0;
        self.weather.rain_time = d.int("rainTime").unwrap_or(0) as i32;
        self.weather.thundering = d.int("thundering").unwrap_or(0) != 0;
        self.weather.thunder_time = d.int("thunderTime").unwrap_or(0) as i32;
        self.weather.rain = if self.weather.raining { 1.0 } else { 0.0 };
        self.weather.prev_rain = self.weather.rain;
        self.play.mode = if d.int("GameType").unwrap_or(0) == 1 {
            GameMode::Creative
        } else {
            GameMode::Survival
        };
        let sx = d.int("SpawnX").unwrap_or(0) as f64 + 0.5;
        let sy = d.int("SpawnY").unwrap_or(64) as f64;
        let sz = d.int("SpawnZ").unwrap_or(0) as f64 + 0.5;
        self.spawn = [sx, sy, sz];
        // WorldServer.initialize: a new world (WorldInfo.isInitialized false)
        // gets createSpawnPosition
        let initialized = match d.get("initialized") {
            Some(_) => d.int("initialized").unwrap_or(1) != 0,
            None => d.get("SpawnX").is_some(),
        };
        if !initialized {
            self.spawn = self.find_spawn();
        }
        let mut pos = self.spawn;
        let mut rot = [0.0f32; 2];
        let mut flying = false;
        if let Some(p) = d.get("Player") {
            let ps = p.list("Pos");
            if let (Some(Nbt::Double(x)), Some(Nbt::Double(y)), Some(Nbt::Double(z))) =
                (ps.first(), ps.get(1), ps.get(2))
            {
                // EntityPlayer.posY sits yOffset (1.62) above the feet
                pos = [*x, *y - 1.62, *z];
            }
            if let (Some(Nbt::Float(a)), Some(Nbt::Float(b))) =
                (p.list("Rotation").first(), p.list("Rotation").get(1))
            {
                rot = [*a, *b];
            }
            *dim = p.int("Dimension").unwrap_or(0) as i32;
            for (k, st) in crate::world::anvil::slots(p.list("Inventory")) {
                if (0..36).contains(&k) {
                    self.play.inv.main[k as usize] = Some(st);
                }
                if (100..104).contains(&k) {
                    self.play.inv.armor[k as usize - 100] = Some(st);
                }
            }
            if let Some(ab) = p.get("abilities") {
                flying = ab.int("flying").unwrap_or(0) != 0;
            }
        }
        self.player = Some((pos, rot, flying));
        Start {
            pos,
            rot,
            flying,
            world_time: self.world_time,
            seed: self.world.seed(),
            resumed: true,
            mode: self.play.mode,
            stats: None,
        }
    }

    /// Tile entities, entities and dropped items of one chunk, in NBT.
    fn chunk_lists(
        play: &Play,
        mobs: &crate::entity::mob::Mobs,
        cx: i32,
        cz: i32,
        dim: i32,
    ) -> (Vec<crate::world::anvil::Nbt>, Vec<crate::world::anvil::Nbt>) {
        let tiles = play.tiles_nbt(cx, cz);
        let mut entities: Vec<crate::world::anvil::Nbt> = mobs
            .list
            .iter()
            .filter(|m| {
                m.health > 0.0
                    && (m.pos[0].floor() as i32) >> 4 == cx
                    && (m.pos[2].floor() as i32) >> 4 == cz
            })
            .map(crate::entity::mob::Mobs::to_nbt)
            .collect();
        entities.extend(play.items_nbt(cx, cz, dim));
        entities.extend(play.falling_nbt(cx, cz, dim));
        entities.extend(play.vehicles_nbt(cx, cz, dim));
        (tiles, entities)
    }

    /// Saves one dimension in Anvil form: changed chunks and every chunk that
    /// holds (or held) tile entities or entities.
    fn save_anvil_world(
        world: &mut ServerWorld,
        play: &Play,
        mobs: &crate::entity::mob::Mobs,
        unsaved: &mut std::collections::BTreeSet<(i32, i32)>,
        touched: &std::collections::BTreeSet<(i32, i32)>,
    ) -> std::io::Result<()> {
        use crate::world::anvil;
        let Some(dir) = world.save_dir.clone() else {
            return Ok(());
        };
        let mut keys: std::collections::BTreeSet<(i32, i32)> = std::mem::take(unsaved);
        keys.extend(touched.iter().copied());
        for p in play.tiles.keys() {
            keys.insert((p[0] >> 4, p[2] >> 4));
        }
        for m in &mobs.list {
            keys.insert((
                (m.pos[0].floor() as i32) >> 4,
                (m.pos[2].floor() as i32) >> 4,
            ));
        }
        for e in &play.items {
            keys.insert((
                (e.e.pos[0].floor() as i32) >> 4,
                (e.e.pos[2].floor() as i32) >> 4,
            ));
        }
        let dim = world.provider.dim;
        for (cx, cz) in keys {
            let lists = Self::chunk_lists(play, mobs, cx, cz, dim);
            if world.chunks.contains_key(&(cx, cz)) {
                world.write_anvil_chunk(&dir, cx, cz, Some(lists))?;
            } else if let Some(mut root) = anvil::read_region_chunk(&dir, cx, cz) {
                if let Some(level) = root.get_mut("Level") {
                    level.set("TileEntities", anvil::Nbt::List(10, lists.0));
                    level.set("Entities", anvil::Nbt::List(10, lists.1));
                }
                anvil::write_region_chunk(&dir, cx, cz, &root, (world.total_time / 20) as u32)?;
            }
        }
        Ok(())
    }

    /// Writes an opened vanilla world back: region files and `level.dat`.
    fn save_anvil(&mut self) -> std::io::Result<()> {
        use crate::world::anvil::{compound, item_nbt, list, Nbt};
        let Some(base) = self.base_dir.clone() else {
            return Ok(());
        };
        let touched: std::collections::BTreeSet<(i32, i32)> = self
            .anvil_imported
            .iter()
            .filter(|k| k.0 == self.dim)
            .map(|k| (k.1, k.2))
            .collect();
        Self::save_anvil_world(
            &mut self.world,
            &self.play,
            &self.mobs,
            &mut self.unsaved,
            &touched,
        )?;
        for p in &mut self.parked {
            let touched: std::collections::BTreeSet<(i32, i32)> = self
                .anvil_imported
                .iter()
                .filter(|k| k.0 == p.dim)
                .map(|k| (k.1, k.2))
                .collect();
            let mut tmp = Play::new(GameMode::Survival);
            tmp.tiles = p.tiles.clone();
            tmp.items = p.items.clone();
            Self::save_anvil_world(&mut p.world, &tmp, &p.mobs, &mut p.unsaved, &touched)?;
        }
        let mut root = self
            .anvil_level
            .clone()
            .unwrap_or_else(|| compound(vec![("Data", compound(vec![]))]));
        if root.get("Data").is_none() {
            root.set("Data", compound(vec![]));
        }
        let (feet, rot, flying) = self.player.unwrap_or((self.spawn, [0.0, 0.0], false));
        let d = root.get_mut("Data").unwrap();
        d.set("RandomSeed", Nbt::Long(self.world.seed()));
        d.set("Time", Nbt::Long(self.world.total_time));
        d.set("DayTime", Nbt::Long(self.world_time));
        d.set("raining", Nbt::Byte(self.weather.raining as i8));
        d.set("rainTime", Nbt::Int(self.weather.rain_time));
        d.set("thundering", Nbt::Byte(self.weather.thundering as i8));
        d.set("thunderTime", Nbt::Int(self.weather.thunder_time));
        d.set(
            "GameType",
            Nbt::Int((self.play.mode == GameMode::Creative) as i32),
        );
        d.set("version", Nbt::Int(19133));
        d.set("SpawnX", Nbt::Int(self.spawn[0].floor() as i32));
        d.set("SpawnY", Nbt::Int(self.spawn[1].floor() as i32));
        d.set("SpawnZ", Nbt::Int(self.spawn[2].floor() as i32));
        if d.get("LevelName").is_none() {
            d.set("LevelName", Nbt::String("MINERUST".into()));
        }
        if d.get("generatorName").is_none() {
            d.set("generatorName", Nbt::String("default".into()));
            d.set("generatorVersion", Nbt::Int(1));
            d.set("MapFeatures", Nbt::Byte(1));
        }
        // InventoryPlayer.writeToNBT: main slots, then armour as 100..104
        let mut inv: Vec<Nbt> = self
            .play
            .inv
            .main
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().map(|s| item_nbt(Some(i as i8), s)))
            .collect();
        for (i, s) in self.play.inv.armor.iter().enumerate() {
            if let Some(s) = s {
                inv.push(item_nbt(Some(100 + i as i8), s));
            }
        }
        let mut player = d.get("Player").cloned().unwrap_or_else(|| compound(vec![]));
        player.set(
            "Pos",
            list(vec![
                Nbt::Double(feet[0]),
                Nbt::Double(feet[1] + 1.62),
                Nbt::Double(feet[2]),
            ]),
        );
        player.set(
            "Rotation",
            list(vec![Nbt::Float(rot[0]), Nbt::Float(rot[1])]),
        );
        player.set("Dimension", Nbt::Int(self.dim));
        player.set("Inventory", Nbt::List(10, inv));
        player.set("SelectedItemSlot", Nbt::Int(self.play.inv.selected as i32));
        let mut ab = player
            .get("abilities")
            .cloned()
            .unwrap_or_else(|| compound(vec![]));
        ab.set("flying", Nbt::Byte(flying as i8));
        ab.set(
            "mayfly",
            Nbt::Byte((self.play.mode == GameMode::Creative) as i8),
        );
        ab.set(
            "instabuild",
            Nbt::Byte((self.play.mode == GameMode::Creative) as i8),
        );
        player.set("abilities", ab);
        d.set("Player", player);
        d.set("initialized", Nbt::Byte(1));
        d.set(
            "LastPlayed",
            Nbt::Long(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |t| t.as_millis() as i64),
            ),
        );
        self.anvil_level = Some(root.clone());
        crate::world::anvil::write_level_dat(&base, &root)
    }

    /// `--export-anvil`: writes this world (in this project's format) as a
    /// vanilla 1.7.10 world in `dst`: every stored chunk, tile entity and
    /// entity, and `level.dat`.
    pub fn export_anvil(&mut self, dst: &std::path::Path) -> std::io::Result<usize> {
        let src = self.world.save_dir.clone();
        let mut keys = Vec::new();
        if let Some(d) = &src {
            let chunks = d.join("chunks");
            if let Ok(regions) = std::fs::read_dir(&chunks) {
                for r in regions.flatten() {
                    if let Ok(files) = std::fs::read_dir(r.path()) {
                        for f in files.flatten() {
                            let name = f.file_name().to_string_lossy().to_string();
                            let parts: Vec<&str> =
                                name.trim_end_matches(".mrc").split('.').collect();
                            if parts.len() == 3 && parts[0] == "c" {
                                if let (Ok(x), Ok(z)) = (parts[1].parse(), parts[2].parse()) {
                                    keys.push((x, z));
                                }
                            }
                        }
                    }
                }
            }
        }
        keys.sort();
        for &(x, z) in &keys {
            self.world.load_chunk(x, z);
        }
        std::fs::create_dir_all(dst)?;
        self.world.save_dir = Some(dst.to_path_buf());
        self.world.anvil = true;
        self.base_dir = Some(dst.to_path_buf());
        self.unsaved.extend(keys.iter().copied());
        self.save_anvil()?;
        Ok(keys.len())
    }

    /// Takes over the tile entities and entities of chunks read from region files.
    fn import_anvil(&mut self) {
        let loaded = std::mem::take(&mut self.world.anvil_loaded);
        let mut rnd = std::mem::replace(&mut self.world.rand, JavaRandom::new(0));
        for (cx, cz, tiles, entities) in loaded {
            if !self.anvil_imported.insert((self.dim, cx, cz)) {
                continue;
            }
            for t in &tiles {
                self.play.tile_from_nbt(t);
            }
            for e in &entities {
                if e.string("id") == Some("Item") {
                    self.play.item_from_nbt(e, &mut rnd);
                } else if e.string("id") == Some("FallingSand") {
                    self.play.falling_from_nbt(e);
                } else if !self.play.vehicle_from_nbt(e) {
                    let _ = self.mobs.from_nbt(e);
                }
            }
        }
        self.world.rand = rnd;
    }

    /// `ItemEnderEye.onItemRightClick`: thrown towards the nearest stronghold.
    fn use_air(&mut self) {
        let Some(held) = self.play.inv.held() else {
            return;
        };
        // ItemBoat
        if held.id == 333 {
            self.place_boat();
            return;
        }
        // ItemArmor.onItemRightClick: into the empty armour slot
        if let Some(t) = crate::container::armor_type(held.id) {
            let k = 3 - t;
            if self.play.inv.armor[k].is_none() {
                self.play.inv.armor[k] = Some(crate::item::Stack { count: 1, ..held });
                // ItemInWorldManager.tryUseItem keeps a creative player's stack
                if self.play.mode != GameMode::Creative {
                    let sel = self.play.inv.selected;
                    self.play.inv.main[sel] = None;
                }
                self.play.inv_dirty = true;
            }
            return;
        }
        // ItemSnowball, ItemEgg, ItemEnderPearl, ItemPotion (splash)
        {
            use crate::entity::throwable::Kind as TK;
            let creative = self.play.mode == GameMode::Creative;
            let kind = match held.id {
                332 => Some(TK::Snowball),
                344 => Some(TK::Egg),
                // ItemEnderPearl: not in creative
                368 if !creative => Some(TK::EnderPearl),
                373 if crate::item_use::is_splash(held.meta) => Some(TK::Potion(held.meta)),
                // ItemExpBottle
                384 => Some(TK::ExpBottle),
                _ => None,
            };
            if let Some(k) = kind {
                if !creative {
                    self.play.inv.consume_held();
                    self.play.inv_dirty = true;
                }
                let (feet, yaw) = self.play.player;
                let p = 0.4 / (self.world.rand.next_float() * 0.4 + 0.8);
                let pitch_snd = if held.id == 373 { 0.5 } else { 0.5 };
                self.pending_out.push(crate::entity::mob::Outcome {
                    sounds: vec![("random.bow", [feet[0], feet[1], feet[2]], pitch_snd, p)],
                    ..Default::default()
                });
                let pitch = self.play.pitch;
                self.mobs.throw_from_player(k, feet, yaw, pitch);
                return;
            }
        }
        // ItemFood, ItemPotion, ItemBucketMilk, ItemBow, ItemSword.onItemRightClick
        if self.splayer.using.is_none() && !self.splayer.dead {
            use crate::item_use::{action, food, is_splash, max_duration, Action};
            let start = match action(held.id, held.meta) {
                Action::Eat => {
                    let f = food(held.id, held.meta).unwrap();
                    self.splayer.can_eat(f.always_edible)
                }
                Action::Drink => !(held.id == 373 && is_splash(held.meta)),
                Action::Bow => {
                    self.play.mode == GameMode::Creative
                        || self.play.inv.main.iter().flatten().any(|s| s.id == 262)
                }
                Action::Block => true,
                Action::None => false,
            };
            if start {
                let n = max_duration(held.id, held.meta);
                self.splayer.using = Some((self.play.inv.selected, held, n));
                self.using_changed = true;
                return;
            }
        }
        if held.id != 381 || self.dim != 0 {
            return;
        }
        let (feet, _) = self.play.player;
        let Some(target) = self
            .world
            .structures
            .as_ref()
            .and_then(|s| s.nearest_stronghold(feet[0] as i32, feet[1] as i32, feet[2] as i32))
        else {
            return;
        };
        let pos = [feet[0], feet[1] + 1.62 - 0.0, feet[2]];
        // EntityEnderEye.moveTowards
        let (dx, dz) = (target[0] as f64 - pos[0], target[2] as f64 - pos[2]);
        let d = (dx * dx + dz * dz).sqrt();
        let t = if d > 12.0 {
            [pos[0] + dx / d * 12.0, pos[1] + 8.0, pos[2] + dz / d * 12.0]
        } else {
            [target[0] as f64, target[1] as f64, target[2] as f64]
        };
        let drop = self.world.rand.next_int_n(5) > 0;
        self.eyes.push(EnderEye {
            pos,
            prev: pos,
            motion: [0.0; 3],
            target: t,
            timer: 0,
            drop,
        });
        let p = 0.4 / (self.world.rand.next_float() * 0.4 + 0.8);
        self.pending_out.push(crate::entity::mob::Outcome {
            sounds: vec![("random.bow", feet, 0.5, p)],
            ..Default::default()
        });
        if self.play.mode == GameMode::Survival {
            self.play.inv.consume_held();
            self.play.inv_dirty = true;
        }
    }

    /// `EntityPlayer.onUpdate`: the item in use counts down; at 0
    /// `onItemUseFinish` eats, drinks or empties it.
    fn tick_item_use(&mut self, events: &mut Vec<Event>) {
        let Some((slot, st, left)) = self.splayer.using else {
            return;
        };
        // getCurrentItem() == itemInUse
        if self.play.inv.selected != slot || self.play.inv.main[slot] != Some(st) {
            self.splayer.using = None;
            self.using_changed = true;
            return;
        }
        let left = left - 1;
        if left == 0 {
            self.splayer.using = None;
            self.using_changed = true;
            events.push(Event::UseFinished(st));
            self.use_finished(slot, st);
        } else {
            self.splayer.using = Some((slot, st, left));
        }
    }

    /// `onItemUseFinish` -> `ItemStack.onFoodEaten`
    fn use_finished(&mut self, slot: usize, st: crate::item::Stack) {
        use crate::item::Stack;
        use crate::potion::{self, Effect};
        let creative = self.play.mode == GameMode::Creative;
        let pos = self.splayer.e.pos;
        let mut result = Some(st);
        let dec = |s: Stack| {
            if s.count > 1 {
                Some(Stack {
                    count: s.count - 1,
                    ..s
                })
            } else {
                None
            }
        };
        if let Some(f) = crate::item_use::food(st.id, st.meta) {
            // ItemFood.onEaten
            result = dec(st);
            self.splayer.food.add_stats(f.heal, f.saturation);
            let p = self.world.rand.next_float() * 0.1 + 0.9;
            self.pending_out.push(crate::entity::mob::Outcome {
                sounds: vec![("random.burp", pos, 0.5, p)],
                ..Default::default()
            });
            let l = &mut self.splayer.living;
            if st.id == 322 {
                // ItemAppleGold.onFoodEaten
                l.add_effect(Effect::new(potion::ABSORPTION, 2400, 0));
                if st.meta > 0 {
                    l.add_effect(Effect::new(potion::REGENERATION, 600, 4));
                    l.add_effect(Effect::new(potion::RESISTANCE, 6000, 0));
                    l.add_effect(Effect::new(potion::FIRE_RESISTANCE, 6000, 0));
                } else if self.world.rand.next_float() < 1.0 {
                    l.add_effect(Effect::new(potion::REGENERATION, 5 * 20, 1));
                }
            } else {
                if st.id == 349 && st.meta == 3 {
                    // ItemFishFood: pufferfish
                    l.add_effect(Effect::new(potion::POISON, 1200, 3));
                    l.add_effect(Effect::new(potion::HUNGER, 300, 2));
                    l.add_effect(Effect::new(potion::CONFUSION, 300, 1));
                }
                if let Some((id, secs, amp, prob)) = f.effect {
                    if self.world.rand.next_float() < prob {
                        l.add_effect(Effect::new(id, secs * 20, amp));
                    }
                }
            }
            if st.id == 282 {
                // ItemSoup: a bowl comes back
                result = Some(Stack::new(281, 0, 1));
            }
        } else if st.id == 373 {
            // ItemPotion.onEaten
            let mut left = st;
            if !creative {
                left.count -= 1;
            }
            for e in potion::effects_of(st.meta, false) {
                if e.potion().instant {
                    let ad = e.amplifier;
                    if e.id == potion::HEAL {
                        self.splayer.living.heal((4 << ad).max(0) as f32);
                    } else if e.id == potion::HARM {
                        let d = self.difficulty;
                        let src = crate::entity::living::Source::named(
                            crate::entity::living::SourceKind::Magic,
                        );
                        self.splayer.attack(&self.world, src, (6 << ad) as f32, d);
                    }
                } else {
                    self.splayer.living.add_effect(e);
                }
            }
            result = if creative {
                Some(left)
            } else if left.count == 0 {
                Some(Stack::new(374, 0, 1))
            } else {
                if let Some(rest) = self.play.inv.add(Stack::new(374, 0, 1)) {
                    let mut r = std::mem::replace(&mut self.world.rand, JavaRandom::new(0));
                    self.play.throw(rest, &mut r);
                    self.world.rand = r;
                }
                Some(left)
            };
        } else if st.id == 335 {
            // ItemBucketMilk.onEaten
            let mut left = st;
            if !creative {
                left.count -= 1;
            }
            self.splayer.living.clear_effects();
            result = if left.count == 0 {
                Some(Stack::new(325, 0, 1))
            } else {
                Some(left)
            };
        }
        self.play.inv.main[slot] = result.filter(|s| s.count > 0);
        self.play.inv_dirty = true;
    }

    /// `ItemStack.onPlayerStoppedUsing` (the bow fires; see `fire_bow`).
    fn stopped_using(&mut self, st: crate::item::Stack, left: i32) {
        self.using_changed = true;
        if st.id == 261 {
            self.fire_bow(st, left);
        }
    }

    /// `ItemBow.onPlayerStoppedUsing`
    fn fire_bow(&mut self, _st: crate::item::Stack, left: i32) {
        let creative = self.play.mode == GameMode::Creative;
        let has_arrow = self.play.inv.main.iter().flatten().any(|s| s.id == 262);
        if !creative && !has_arrow {
            return;
        }
        let j = 72000 - left;
        let mut f = j as f32 / 20.0;
        f = (f * f + f * 2.0) / 3.0;
        if (f as f64) < 0.1 {
            return;
        }
        if f > 1.0 {
            f = 1.0;
        }
        let (feet, yaw) = self.play.player;
        let pitch = self.play.pitch;
        let crit = f == 1.0;
        let p = 1.0 / (self.world.rand.next_float() * 0.4 + 1.2) + f * 0.5;
        self.pending_out.push(crate::entity::mob::Outcome {
            sounds: vec![("random.bow", [feet[0], feet[1] + 1.62, feet[2]], 1.0, p)],
            ..Default::default()
        });
        self.mobs
            .shoot_player_arrow(feet, yaw, pitch, f * 2.0, crit, creative);
        if !creative {
            // InventoryPlayer.consumeInventoryItem(Items.arrow)
            if let Some(i) = self
                .play
                .inv
                .main
                .iter()
                .position(|s| s.map_or(false, |s| s.id == 262))
            {
                let s = self.play.inv.main[i].unwrap();
                self.play.inv.main[i] = if s.count > 1 {
                    Some(crate::item::Stack {
                        count: s.count - 1,
                        ..s
                    })
                } else {
                    None
                };
            }
            // ItemStack.damageItem(1, player)
            let sel = self.play.inv.selected;
            if let Some(mut b) = self.play.inv.main[sel] {
                if b.id == 261 {
                    b.meta += 1;
                    self.play.inv.main[sel] = if b.meta > crate::item::max_damage(261) {
                        None
                    } else {
                        Some(b)
                    };
                }
            }
            self.play.inv_dirty = true;
        }
    }

    /// The directory of a dimension's chunks and data (`DIM-1`, `DIM1`).
    fn dim_dir(&self, dim: i32) -> Option<PathBuf> {
        self.base_dir.as_ref().map(|b| {
            if dim == 0 {
                b.clone()
            } else {
                b.join(format!("DIM{dim}"))
            }
        })
    }

    /// `Entity.onEntityUpdate` portal handling for the player.
    fn tick_portal(&mut self, events: &mut Vec<Event>) {
        if self.announce_dim {
            self.announce_dim = false;
            let (pos, yaw) = self.play.player;
            for key in std::mem::take(&mut self.sent).into_keys() {
                events.push(Event::Unload(key));
            }
            self.await_ack = true;
            events.push(Event::Dimension {
                dim: self.dim,
                pos,
                yaw,
            });
            return;
        }
        if let Some(to) = self.pending_travel.take() {
            self.travel(to, events);
            return;
        }
        if self.await_ack {
            return;
        }
        let (feet, _) = self.play.player;
        let contact = crate::dimension::portal_contact(&mut self.world, feet);
        match contact {
            Some(block::END_PORTAL) => {
                // BlockEndPortal.onEntityCollidedWithBlock
                let to = if self.dim == 1 { 0 } else { 1 };
                self.travel(to, events);
            }
            Some(_) => {
                // setInPortal / the portal counter
                if self.portal_cooldown > 0 {
                    self.portal_cooldown = 10;
                } else {
                    let max = if self.play.mode == GameMode::Creative {
                        0
                    } else {
                        80
                    };
                    let n = self.portal_ticks;
                    self.portal_ticks += 1;
                    if n >= max && self.dim != 1 {
                        self.portal_ticks = max;
                        self.portal_cooldown = 10;
                        let to = if self.dim == -1 { 0 } else { -1 };
                        self.travel(to, events);
                    }
                }
            }
            None => {
                if self.portal_ticks > 0 {
                    self.portal_ticks = (self.portal_ticks - 4).max(0);
                }
            }
        }
        if self.portal_cooldown > 0 && contact.is_none() {
            self.portal_cooldown -= 1;
        }
        events.push(Event::Portal(self.portal_ticks));
    }

    /// Creates the world of a dimension (or fetches it from the parked ones).
    fn take_world(&mut self, dim: i32) -> Parked {
        if let Some(i) = self.parked.iter().position(|p| p.dim == dim) {
            return self.parked.remove(i);
        }
        let seed = self.world.seed();
        let provider = Arc::new(ChunkProvider::new_dim(seed, dim));
        let mut world = ServerWorld::new(provider);
        world.save_dir = self.dim_dir(dim);
        world.total_time = self.world.total_time;
        world.anvil = self.world.anvil;
        let mut mobs = crate::entity::mob::Mobs::default();
        let mut tiles = std::collections::BTreeMap::new();
        let mut drops = Play::new(GameMode::Survival);
        if let Some(d) = &world.save_dir {
            if let Ok(text) = std::fs::read_to_string(d.join("data").join("structures.txt")) {
                if let Some(st) = world.structures.as_mut() {
                    let _ = st.load(text.trim());
                }
            }
            if let Ok(text) = std::fs::read_to_string(d.join("data").join("entities.txt")) {
                for line in text.lines() {
                    let _ = mobs.decode(line);
                }
            }
            if let Ok(text) = std::fs::read_to_string(d.join("data").join("tiles.txt")) {
                let mut tmp = Play::new(GameMode::Survival);
                for line in text.lines() {
                    let _ = tmp.decode_tile(line);
                }
                tiles = tmp.tiles;
            }
            if let Ok(b) = std::fs::read(d.join("data").join("drops.dat")) {
                let mut r = JavaRandom::new(seed);
                drops.load_drops(&b, &mut r);
            }
        }
        Parked {
            dim,
            world,
            mobs,
            unsaved: Default::default(),
            tiles,
            items: drops.items,
            falling: drops.falling,
            vehicles: drops.vehicles,
            teleport_rand: JavaRandom::new(seed),
        }
    }

    /// `ServerConfigurationManager.transferPlayerToDimension`
    fn travel(&mut self, to: i32, events: &mut Vec<Event>) {
        if to == self.dim {
            return;
        }
        let (feet, yaw) = self.play.player;
        let from = self.dim;
        // swap the worlds
        let next = self.take_world(to);
        let mut old = Parked {
            dim: from,
            world: std::mem::replace(&mut self.world, next.world),
            mobs: std::mem::replace(&mut self.mobs, next.mobs),
            unsaved: std::mem::replace(&mut self.unsaved, next.unsaved),
            tiles: std::mem::replace(&mut self.play.tiles, next.tiles),
            items: std::mem::replace(&mut self.play.items, next.items),
            falling: std::mem::replace(&mut self.play.falling, next.falling),
            vehicles: std::mem::replace(&mut self.play.vehicles, next.vehicles),
            teleport_rand: std::mem::replace(&mut self.teleport_rand, next.teleport_rand),
        };
        // Unloaded chunks of the old dimension go to disk now.
        if let Some(d) = old.world.save_dir.clone() {
            for key in std::mem::take(&mut old.unsaved) {
                if let Some(c) = old.world.chunks.get(&key) {
                    let _ = save::write_chunk(&d, key.0, key.1, &c.to_bytes());
                }
            }
        }
        self.parked.push(old);
        self.dim = to;
        self.world.total_time = self.parked.last().unwrap().world.total_time;
        self.last_attacked = None;
        self.last_attacker = None;
        // where the player arrives
        let mut pos = feet;
        let mut new_yaw = yaw;
        if to == 1 {
            // WorldProviderEnd.getEntrancePortalLocation
            pos = [100.0, 50.0, 0.0];
            self.load_around(pos);
            pos = crate::dimension::end_platform(&mut self.world, pos);
        } else if from == 1 {
            // out of the End: back to the spawn point
            pos = self.spawn;
            self.load_around(pos);
        } else {
            let k = if to == -1 { 1.0 / 8.0 } else { 8.0 };
            pos[0] = (pos[0] * k).clamp(-29999872.0, 29999872.0);
            pos[2] = (pos[2] * k).clamp(-29999872.0, 29999872.0);
            self.load_around(pos);
            match crate::dimension::find_portal(&mut self.world, pos, yaw) {
                Some((p, y)) => {
                    pos = p;
                    new_yaw = y;
                }
                None => {
                    let mut r = std::mem::replace(&mut self.teleport_rand, JavaRandom::new(0));
                    crate::dimension::make_portal(&mut self.world, &mut r, pos);
                    self.teleport_rand = r;
                    if let Some((p, y)) = crate::dimension::find_portal(&mut self.world, pos, yaw) {
                        pos = p;
                        new_yaw = y;
                    }
                }
            }
        }
        self.play.player = (pos, new_yaw);
        if let Some(p) = &mut self.player {
            p.0 = pos;
            p.1[0] = new_yaw;
        }
        self.centre = ((pos[0].floor() as i32) >> 4, (pos[2].floor() as i32) >> 4);
        for key in std::mem::take(&mut self.sent).into_keys() {
            events.push(Event::Unload(key));
        }
        self.await_ack = true;
        self.portal_cooldown = 10;
        events.push(Event::Dimension {
            dim: to,
            pos,
            yaw: new_yaw,
        });
        let sound = if to == 1 || from == 1 {
            ""
        } else {
            "portal.travel"
        };
        if !sound.is_empty() {
            events.push(Event::Sounds(vec![(sound, pos, 1.0, 1.0)]));
        }
    }

    /// Loads the 3x3 chunks around a position (portal search and building).
    fn load_around(&mut self, pos: [f64; 3]) {
        let (cx, cz) = ((pos[0].floor() as i32) >> 4, (pos[2].floor() as i32) >> 4);
        for dx in -2..=2 {
            for dz in -2..=2 {
                self.world.load_chunk(cx + dx, cz + dz);
            }
        }
    }

    /// `--summon Kind[:tamed|:sit|:big|:n],...`: a row of mobs four blocks in
    /// front of the player once the world is up (for checking models).
    fn summon_once(&mut self) {
        static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if self.world.total_time < 20 || DONE.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        {
            let (f, _) = self.play.player;
            let (cx, cz) = ((f[0].floor() as i32) >> 4, (f[2].floor() as i32) >> 4);
            for dx in -1..=1 {
                for dz in -1..=1 {
                    if !self.world.chunk_exists(cx + dx, cz + dz) {
                        return;
                    }
                }
            }
        }
        DONE.store(true, std::sync::atomic::Ordering::Relaxed);
        // CommandEffect: id, seconds (30), amplifier (0)
        if let Ok(list) = std::env::var("MINERUST_EFFECT") {
            for e in list.split(',') {
                let v: Vec<i32> = e.split(':').filter_map(|t| t.parse().ok()).collect();
                if let Some(&id) = v.first() {
                    if crate::potion::potion(id).is_some() {
                        let secs = v.get(1).copied().unwrap_or(30);
                        let amp = v.get(2).copied().unwrap_or(0);
                        self.splayer.living.add_effect(crate::potion::Effect::new(
                            id,
                            secs * 20,
                            amp,
                        ));
                    }
                }
            }
        }
        // CommandSetBlock: x y z id [meta]
        if let Ok(list) = std::env::var("MINERUST_SETBLOCK") {
            for cmd in list.split(';') {
                let v: Vec<i32> = cmd
                    .split_whitespace()
                    .filter_map(|t| t.parse().ok())
                    .collect();
                if v.len() >= 4 && (0..256).contains(&v[1]) {
                    let meta = v.get(4).copied().unwrap_or(0);
                    self.world.set_block(v[0], v[1], v[2], v[3] as u8, meta, 3);
                }
            }
        }
        let Ok(list) = std::env::var("MINERUST_SUMMON") else {
            return;
        };
        let (feet, yaw) = self.play.player;
        let (s, c) = (yaw.to_radians().sin() as f64, yaw.to_radians().cos() as f64);
        let fwd = [-s, c];
        let side = [c, s];
        let items: Vec<&str> = list.split(',').filter(|x| !x.is_empty()).collect();
        let n = items.len() as f64;
        let mut rnd = std::mem::replace(&mut self.world.rand, JavaRandom::new(0));
        for (i, item) in items.iter().enumerate() {
            let mut parts = item.split(':');
            let name = parts.next().unwrap_or("");
            let off = (i as f64 - (n - 1.0) / 2.0) * 2.2;
            if let Some(kind) = crate::entity::minecart::CartKind::from_entity_id(name) {
                // on the rail under the summoning spot
                let dist = 3.0;
                let x = feet[0] + fwd[0] * dist + side[0] * off;
                let z = feet[2] + fwd[1] * dist + side[1] * off;
                let (bx, bz) = (x.floor() as i32, z.floor() as i32);
                let mut y = feet[1].floor() as i32 + 3;
                while y > 1
                    && !crate::entity::minecart::is_rail(self.world.block(bx, y, bz))
                    && !self.world.material(bx, y - 1, bz).blocks_movement()
                {
                    y -= 1;
                }
                let id = self.alloc_vehicle_id();
                let seed = self.mobs.rand.next_long();
                let mut c = crate::entity::minecart::Minecart::new(id, kind, seed);
                c.e.set_location_and_angles(
                    bx as f64 + 0.5,
                    y as f64,
                    bz as f64 + 0.5,
                    yaw + 90.0,
                    0.0,
                );
                let parts: Vec<&str> = parts.collect();
                if parts.contains(&"lit") {
                    c.fuel = 3600;
                    c.push = [
                        -(yaw.to_radians().sin() as f64),
                        yaw.to_radians().cos() as f64,
                    ];
                }
                self.play
                    .vehicles
                    .push(crate::entity::vehicle::Tracked::new(
                        crate::entity::vehicle::Vehicle::Cart(c),
                    ));
                if parts.contains(&"ride") {
                    self.interact_vehicle(id);
                }
                continue;
            }
            if name == "XPOrb" {
                let v: i32 = parts.next().and_then(|t| t.parse().ok()).unwrap_or(7);
                let dist: f64 = std::env::var("MINERUST_SUMMON_DIST")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(3.0);
                let x = feet[0] + fwd[0] * dist + side[0] * off;
                let z = feet[2] + fwd[1] * dist + side[1] * off;
                let mut play = std::mem::replace(&mut self.play, Play::new(GameMode::Survival));
                self.spawn_orbs(&mut play, [x, feet[1] + 1.0, z], v);
                self.play = play;
                continue;
            }
            if name == "Boat" {
                // CommandSummon: EntityList.createEntityByName + setLocationAndAngles
                let dist = 4.0;
                let x = feet[0] + fwd[0] * dist + side[0] * off;
                let z = feet[2] + fwd[1] * dist + side[1] * off;
                let (bx, bz) = (x.floor() as i32, z.floor() as i32);
                let mut y = feet[1].floor() as i32 + 3;
                while y > 1 && {
                    let m = self.world.material(bx, y - 1, bz);
                    !m.blocks_movement() && !m.is_liquid()
                } {
                    y -= 1;
                }
                let id = self.alloc_vehicle_id();
                let seed = self.mobs.rand.next_long();
                let mut b = crate::entity::boat::Boat::new(id, seed);
                b.e.set_location_and_angles(x, y as f64, z, yaw + 90.0, 0.0);
                self.world.solid_entities.push((id, b.bounding_box()));
                self.play
                    .vehicles
                    .push(crate::entity::vehicle::Tracked::new(
                        crate::entity::vehicle::Vehicle::Boat(b),
                    ));
                if parts.any(|p| p == "ride") {
                    self.interact_vehicle(id);
                }
                continue;
            }
            let Some(kind) = crate::entity::mob::Kind::from_id(name) else {
                continue;
            };
            let dist: f64 = std::env::var("MINERUST_SUMMON_DIST")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5.0);
            let x = feet[0] + fwd[0] * dist + side[0] * off;
            let z = feet[2] + fwd[1] * dist + side[1] * off;
            let (bx, bz) = (x.floor() as i32, z.floor() as i32);
            let mut y = feet[1].floor() as i32 + 3;
            while y > 1 && !self.world.material(bx, y - 1, bz).blocks_movement() {
                y -= 1;
            }
            let id = self
                .mobs
                .spawn(kind, [x, y as f64, z], yaw + 180.0, &mut rnd);
            if std::env::var("MINERUST_DBG").is_ok() {
                eprintln!("summon {:?} at {:.1} {} {:.1}", kind, x, y, z);
            }
            if let Some(m) = self.mobs.list.iter_mut().find(|m| m.id == id) {
                m.persistent = true;
                for p in parts {
                    match p {
                        "tamed" => m.tamed = true,
                        "sit" => m.sitting = true,
                        "big" => m.set_slime_size(4),
                        "hang" => m.hanging = true,
                        v => {
                            if let Ok(k) = v.parse::<i32>() {
                                m.variant = k;
                            }
                        }
                    }
                }
            }
        }
        self.world.rand = rnd;
    }

    /// Writes the level file and every chunk changed since the last save.
    pub fn save(&mut self) -> std::io::Result<()> {
        let Some(base) = self.base_dir.clone() else {
            return Ok(());
        };
        if self.world.anvil {
            return self.save_anvil();
        }
        // every dimension but the overworld keeps its own data folder
        let wdir = self.world.save_dir.clone().unwrap_or(base.clone());
        for key in std::mem::take(&mut self.unsaved) {
            if let Some(c) = self.world.chunks.get(&key) {
                save::write_chunk(&wdir, key.0, key.1, &c.to_bytes())?;
            }
        }
        for p in &mut self.parked {
            let Some(d) = p.world.save_dir.clone() else {
                continue;
            };
            for key in std::mem::take(&mut p.unsaved) {
                if let Some(c) = p.world.chunks.get(&key) {
                    save::write_chunk(&d, key.0, key.1, &c.to_bytes())?;
                }
            }
            let data = d.join("data");
            std::fs::create_dir_all(&data)?;
            if let Some(st) = &p.world.structures {
                save::write_atomic(&data.join("structures.txt"), st.save().as_bytes())?;
            }
            save::write_atomic(
                &data.join("entities.txt"),
                p.mobs.encode().join("\n").as_bytes(),
            )?;
            save::write_atomic(
                &data.join("drops.dat"),
                &Play::drops_nbt(&p.items, &p.falling, &p.vehicles, p.dim),
            )?;
            if p.dim != 0 {
                let mut tmp = Play::new(GameMode::Survival);
                tmp.tiles = p.tiles.clone();
                save::write_atomic(
                    &data.join("tiles.txt"),
                    tmp.encode_tiles().join("\n").as_bytes(),
                )?;
            }
        }
        if self.dim != 0 {
            let data = wdir.join("data");
            std::fs::create_dir_all(&data)?;
            save::write_atomic(
                &data.join("tiles.txt"),
                self.play.encode_tiles().join("\n").as_bytes(),
            )?;
        }
        let overworld_tiles = if self.dim == 0 {
            self.play.encode_tiles()
        } else {
            let mut tmp = Play::new(GameMode::Survival);
            if let Some(p) = self.parked.iter().find(|p| p.dim == 0) {
                tmp.tiles = p.tiles.clone();
            }
            tmp.encode_tiles()
        };
        let ticks = if self.dim == 0 {
            self.world.pending_tick_list()
        } else {
            self.parked
                .iter()
                .find(|p| p.dim == 0)
                .map(|p| p.world.pending_tick_list())
                .unwrap_or_default()
        };
        let level = Level {
            seed: self.world.seed(),
            total_time: self.world.total_time,
            world_time: self.world_time,
            vanilla_parity: crate::compat::vanilla_parity(),
            player: self.player,
            ticks,
            unknown: {
                let mut u = self.unknown_level_keys.clone();
                u.push(("dim".into(), self.dim.to_string()));
                u.push(("inventory".into(), self.play.inv.encode()));
                u.push(("weather".into(), self.weather.encode()));
                u.push((
                    "mode".into(),
                    if self.play.mode == GameMode::Creative {
                        "creative"
                    } else {
                        "survival"
                    }
                    .into(),
                ));
                u.push((
                    "spawn".into(),
                    format!(
                        "{:?},{:?},{:?}",
                        self.spawn[0], self.spawn[1], self.spawn[2]
                    ),
                ));
                u.push(("stats".into(), self.splayer.encode()));
                for t in overworld_tiles {
                    u.push(("tile".into(), t));
                }
                u
            },
        };
        if let Some(st) = &self.world.structures {
            let data = wdir.join("data");
            std::fs::create_dir_all(&data)?;
            save::write_atomic(&data.join("structures.txt"), st.save().as_bytes())?;
        }
        {
            let data = wdir.join("data");
            std::fs::create_dir_all(&data)?;
            save::write_atomic(
                &data.join("entities.txt"),
                self.mobs.encode().join("\n").as_bytes(),
            )?;
            save::write_atomic(
                &data.join("drops.dat"),
                &Play::drops_nbt(
                    &self.play.items,
                    &self.play.falling,
                    &self.play.vehicles,
                    self.dim,
                ),
            )?;
        }
        save::write_level(&base, &level)
    }

    /// Mob AI, spawning and despawning, and what the client must hear of it.
    /// `attackEntityFrom(explosion)` and the push for the entities outside
    /// `Mobs` an explosion reached.
    fn apply_blasts(&mut self, play: &mut Play, blasts: Vec<crate::world::explosion::Blast>) {
        use crate::entity::vehicle::Vehicle;
        use crate::world::explosion::{TAG_FALLING, TAG_ITEM, TAG_VEHICLE};
        for b in blasts {
            let id = (b.tag & 0xFFFF_FFFF) as u32;
            match b.tag & !0xFFFF_FFFF {
                TAG_ITEM => {
                    if let Some(it) = play.items.iter_mut().find(|i| i.id == id) {
                        it.attack(None, true, b.damage);
                        for a in 0..3 {
                            it.e.motion[a] += b.push[a];
                        }
                    }
                }
                TAG_FALLING => {
                    if let Some(f) = play.falling.iter_mut().find(|f| f.id == id) {
                        for a in 0..3 {
                            f.e.motion[a] += b.push[a];
                        }
                    }
                }
                TAG_VEHICLE => {
                    let Some(i) = play.vehicles.iter().position(|v| v.v.id() == id) else {
                        continue;
                    };
                    let mut outs = Vec::new();
                    let mut broke = None;
                    match &mut play.vehicles[i].v {
                        Vehicle::Boat(boat) => broke = boat.attack(b.damage, false),
                        Vehicle::Cart(c) => c.attack(b.damage, false, true, false, &mut outs),
                        Vehicle::Orb(o) => o.attack(b.damage),
                    }
                    {
                        let e = play.vehicles[i].v.e_mut();
                        for a in 0..3 {
                            e.motion[a] += b.push[a];
                        }
                    }
                    if let Some(drop) = broke {
                        if drop {
                            let p = play.vehicles[i].v.e().pos;
                            let nid = play.next_item;
                            play.next_item += 1;
                            let mut r = std::mem::replace(&mut play.prand, JavaRandom::new(0));
                            let mut it = crate::entity::item::ItemEntity::new(
                                nid,
                                crate::item::Stack::new(333, 0, 1),
                                p,
                            );
                            play.prand = r;
                            it.pickup_delay = 10;
                            play.items.push(it);
                        }
                    }
                    self.vehicle_outs(play, i, outs);
                }
                _ => {}
            }
        }
    }

    fn tick_mobs(&mut self, play: &mut Play, events: &mut Vec<Event>) {
        use crate::entity::mob;
        let (feet, yaw) = play.player;
        let held = play.inv.held().map(|s| s.id);
        let mut effect_bits = 0u32;
        for e in self.splayer.living.effects.iter() {
            effect_bits |= 1 << e.id;
        }
        let info = mob::PlayerInfo {
            motion: self.splayer.e.motion,
            health: self.splayer.living.health,
            effects: effect_bits,
            hurt_resistant_time: self.splayer.living.hurt_resistant_time,
            last_damage: self.splayer.living.last_damage,
            difficulty: self.difficulty as i32,
            feet,
            yaw,
            pitch: self.player.map_or(0.0, |p| p.1[1]),
            attackable: self.player_attackable
                && play.mode == GameMode::Survival
                && !self.splayer.dead,
            held,
            last_attacked: self.last_attacked,
            last_attacker: self.last_attacker,
        };
        // World.getCurrentMoonPhaseFactor
        self.world.moon_factor = [1.0, 0.75, 0.5, 0.25, 0.0, 0.25, 0.5, 0.75]
            [((self.world_time / 24000 % 8 + 8) % 8) as usize];
        let mut out = mob::Outcome::default();
        {
            // entities an explosion in the mobs' tick can reach
            use crate::world::explosion::{Target, TAG_FALLING, TAG_ITEM, TAG_VEHICLE};
            let mut t = Vec::new();
            for it in &play.items {
                t.push(Target {
                    tag: TAG_ITEM | it.id as u64,
                    pos: it.e.pos,
                    eye: 0.0,
                    bb: it.bb(),
                });
            }
            for f in &play.falling {
                t.push(Target {
                    tag: TAG_FALLING | f.id as u64,
                    pos: f.e.pos,
                    eye: 0.0,
                    bb: f.e.bb,
                });
            }
            for v in &play.vehicles {
                t.push(Target {
                    tag: TAG_VEHICLE | v.v.id() as u64,
                    pos: v.v.e().pos,
                    eye: 0.0,
                    bb: v.v.e().bb,
                });
            }
            self.mobs.blast_targets = t;
        }
        self.mobs.tick(&mut self.world, &info, &mut out);
        self.mobs.blast_targets.clear();
        self.apply_blasts(play, std::mem::take(&mut out.blasts));
        // the explosions' drops spawn now, as spawnEntityInWorld does
        {
            use crate::world::server::GenEntity;
            let mut rest = Vec::new();
            let mut rnd = std::mem::replace(&mut self.world.rand, JavaRandom::new(0));
            for e in std::mem::take(&mut self.world.gen_entities) {
                match e {
                    GenEntity::ScatteredItem { .. } => play.add_gen_entity(e, &mut rnd),
                    other => rest.push(other),
                }
            }
            self.world.rand = rnd;
            self.world.gen_entities = rest;
        }
        let animals = self.world.total_time % 400 == 0;
        mob::natural_spawning(&mut self.world, &mut self.mobs, feet, self.spawn, animals);
        mob::village_golems(&mut self.world, &mut self.mobs);
        // forget targets that are gone
        if let Some(id) = self.last_attacked {
            if !self.mobs.list.iter().any(|m| m.id == id && m.health > 0.0) {
                self.last_attacked = None;
            }
        }
        if let Some(id) = self.last_attacker {
            if !self.mobs.list.iter().any(|m| m.id == id && m.health > 0.0) {
                self.last_attacker = None;
            }
        }
        if self.world.total_time % 40 == 0 {
            self.mobs.despawn(feet);
        }
        let mut outs = std::mem::take(&mut self.pending_out);
        outs.push(out);
        let mut hits = Vec::new();
        let mut sounds = Vec::new();
        let mut notes = Vec::new();
        {
            use crate::world::redstone::Effect;
            let mut rnd = std::mem::replace(&mut self.world.rand, JavaRandom::new(0));
            for e in std::mem::take(&mut self.world.redstone_effects) {
                match e {
                    Effect::Sound(n, p, v, pi) => sounds.push((n, p, v, pi)),
                    Effect::Note(p, inst, pitch) => notes.push((p, inst, pitch)),
                    Effect::Tnt(p, fuse) => self.mobs.prime_tnt(p, fuse),
                    Effect::Dispense(p, facing) => {
                        if let Some((s, at, dir)) = play.dispense(p, facing, &mut rnd, &mut sounds)
                        {
                            if s.id == 262 {
                                self.mobs.dispense_arrow(at, dir);
                            } else {
                                // BlockSourceImpl: TNT primes in the block in front.
                                let q = [
                                    p[0] as f64 + 0.5 + dir[0],
                                    p[1] as f64 + 0.5 + dir[1],
                                    p[2] as f64 + 0.5 + dir[2],
                                ];
                                self.mobs.prime_tnt(q, 80);
                            }
                        }
                    }
                }
            }
            self.world.rand = rnd;
        }
        if !notes.is_empty() {
            events.push(Event::Notes(notes));
        }
        let mut explosions = Vec::new();
        let mut particles = Vec::new();
        let mut rnd = std::mem::replace(&mut self.world.rand, JavaRandom::new(0));
        for o in outs {
            if o.attacker.is_some() {
                self.last_attacker = o.attacker;
            }
            particles.extend(o.particles);
            for (p, st) in o.drops {
                play.add_gen_entity(
                    crate::world::server::GenEntity::Item { pos: p, stack: st },
                    &mut rnd,
                );
            }
            hits.extend(o.hits);
            sounds.extend(o.sounds);
            explosions.extend(o.explosions);
            for (p, xp) in o.orbs {
                self.spawn_orbs(play, p, xp);
            }
        }
        for (p, xp) in std::mem::take(&mut play.orbs) {
            self.spawn_orbs(play, p, xp);
        }
        self.world.rand = rnd;
        self.apply_player_hits(hits);
        if !sounds.is_empty() {
            events.push(Event::Sounds(sounds));
        }
        if !explosions.is_empty() {
            events.push(Event::Explosions(explosions));
        }
        if !particles.is_empty() {
            events.push(Event::Particles(particles));
        }
        let r2 = ((self.view + 1) * 16) as f64;
        let views: Vec<MobView> = self
            .mobs
            .list
            .iter()
            .filter(|m| (m.pos[0] - feet[0]).abs() < r2 && (m.pos[2] - feet[2]).abs() < r2)
            .map(|m| MobView {
                id: m.id,
                kind: m.kind,
                pos: m.pos,
                prev: m.prev_pos,
                body_yaw: m.body_yaw,
                prev_body_yaw: m.prev_body_yaw,
                head_yaw: m.head_yaw,
                prev_head_yaw: m.prev_head_yaw,
                pitch: m.pitch,
                prev_pitch: m.prev_pitch,
                limb_swing: m.limb_swing,
                limb_amount: m.limb_amount,
                prev_limb_amount: m.prev_limb_amount,
                swing: m.swing,
                prev_swing: m.prev_swing,
                hurt_time: m.hurt_time,
                death_time: m.death_time,
                ticks: m.ticks,
                variant: m.variant,
                sheared: m.sheared,
                fuse: m.fuse,
                prev_fuse: m.prev_fuse,
                wing: m.wing,
                prev_wing: m.prev_wing,
                eat_timer: m.eat_timer,
                fire: m.fire > 0,
                squish: [m.prev_squish, m.squish],
                sq: [m.prev_sq, m.sq],
                flags: m.tamed as u32
                    | (m.sitting as u32) << 1
                    | (m.angry as u32) << 2
                    | (m.hanging as u32) << 3,
                carried: [m.carried, m.carried_meta],
                attack_timer: m.attack_timer,
                health: m.health,
                ring: if m.kind == crate::entity::mob::Kind::EnderDragon {
                    m.ring.to_vec()
                } else {
                    Vec::new()
                },
                ring_index: m.ring_index,
                net_at: m.net.at,
                net_yaw: m.net.yaw,
                net_pitch: m.net.pitch,
                net_seq: m.net.seq,
                net_head: m.net.head,
                yaw: m.yaw,
                incs: 0,
                bb: [
                    m.bb.min[0],
                    m.bb.min[1],
                    m.bb.min[2],
                    m.bb.max[0],
                    m.bb.max[1],
                    m.bb.max[2],
                ],
            })
            .collect();
        let arrows: Vec<ArrowView> = self
            .mobs
            .arrows
            .iter()
            .map(|a| ArrowView {
                pos: a.e.pos,
                prev: a.e.prev,
                yaw: a.e.yaw,
                pitch: a.e.pitch,
            })
            .collect();
        events.push(Event::Mobs(views, arrows));
        events.push(Event::Tnt(
            self.mobs
                .tnt
                .iter()
                .map(|t| (t.pos, t.prev_pos, t.fuse))
                .collect(),
        ));
        // EntityEnderEye.onUpdate
        let mut drops = Vec::new();
        let mut eye_sounds = Vec::new();
        self.eyes.retain_mut(|e| {
            e.prev = e.pos;
            for k in 0..3 {
                e.pos[k] += e.motion[k];
            }
            let (dx, dz) = (e.target[0] - e.pos[0], e.target[2] - e.pos[2]);
            let dist = (dx * dx + dz * dz).sqrt() as f32;
            let ang = dz.atan2(dx) as f32;
            let speed = (e.motion[0] * e.motion[0] + e.motion[2] * e.motion[2]).sqrt();
            let mut v = speed + (dist as f64 - speed) * 0.0025;
            if dist < 1.0 {
                v *= 0.8;
                e.motion[1] *= 0.8;
            }
            e.motion[0] = (ang.cos() as f64) * v;
            e.motion[2] = (ang.sin() as f64) * v;
            if e.pos[1] < e.target[1] {
                e.motion[1] += (1.0 - e.motion[1]) * 0.015f32 as f64;
            } else {
                e.motion[1] += (-1.0 - e.motion[1]) * 0.015f32 as f64;
            }
            e.timer += 1;
            if e.timer > 80 {
                if e.drop {
                    drops.push((e.pos, crate::item::Stack::new(381, 0, 1)));
                } else {
                    eye_sounds.push(("random.glass", e.pos, 1.0, 1.0));
                }
                return false;
            }
            true
        });
        if !eye_sounds.is_empty() {
            events.push(Event::Sounds(eye_sounds));
        }
        let mut rnd = std::mem::replace(&mut self.world.rand, JavaRandom::new(0));
        for (p, st) in drops {
            play.add_gen_entity(
                crate::world::server::GenEntity::Item { pos: p, stack: st },
                &mut rnd,
            );
        }
        self.world.rand = rnd;
        let mut shots: Vec<([f64; 3], [f64; 3], u8)> = self
            .mobs
            .fireballs
            .iter()
            .map(|f| (f.pos, f.prev_pos, f.small as u8))
            .collect();
        shots.extend(self.eyes.iter().map(|e| (e.pos, e.prev, 2u8)));
        events.push(Event::Fireballs(shots));
    }

    /// Damage from mobs, projectiles, explosions and potions, as their
    /// `attackEntityFrom(source, amount)` calls on the player.
    fn apply_player_hits(&mut self, hits: Vec<crate::entity::mob::Hit>) {
        use crate::entity::living::{Difficulty, Source};
        use crate::entity::mob::Hit;
        use crate::potion;
        let d = self.difficulty;
        let pos_of = |mobs: &crate::entity::mob::Mobs, id: Option<u32>| {
            id.and_then(|i| mobs.list.iter().find(|m| m.id == i).map(|m| (i, m.pos)))
        };
        for h in hits {
            match h {
                Hit::Melee {
                    kind,
                    amount,
                    knock,
                    from,
                    effect,
                } => {
                    if amount <= 0.0 {
                        // Entity.addVelocity: the server-side motion only
                        for a in 0..3 {
                            self.splayer.e.motion[a] += knock[a];
                        }
                        continue;
                    }
                    let src = Source::mob(kind, 0, from);
                    if self.splayer.attack(&self.world, src, amount, d) {
                        if knock != [0.0; 3] {
                            for a in 0..3 {
                                self.splayer.e.motion[a] += knock[a];
                            }
                            self.splayer.velocity = Some(self.splayer.e.motion);
                        }
                        match effect {
                            // EntityCaveSpider.attackEntityAsMob
                            Some((potion::POISON, -1)) => {
                                let s = match d {
                                    Difficulty::Normal => 7,
                                    Difficulty::Hard => 15,
                                    _ => 0,
                                };
                                if s > 0 {
                                    self.splayer.living.add_effect(potion::Effect::new(
                                        potion::POISON,
                                        s * 20,
                                        0,
                                    ));
                                }
                            }
                            Some((id, dur)) => self
                                .splayer
                                .living
                                .add_effect(potion::Effect::new(id, dur, 0)),
                            None => {}
                        }
                    }
                }
                Hit::Arrow {
                    amount, shooter, ..
                } => {
                    let src = Source::arrow(pos_of(&self.mobs, shooter));
                    self.splayer.attack(&self.world, src, amount, d);
                }
                Hit::Fireball {
                    amount,
                    shooter,
                    small,
                } => {
                    let src = Source::fireball(pos_of(&self.mobs, Some(shooter)));
                    // EntitySmallFireball.onImpact: the target is set on fire
                    if self.splayer.attack(&self.world, src, amount, d) && small {
                        self.splayer.e.set_fire(5);
                    }
                }
                Hit::Explosion {
                    amount,
                    knock,
                    from,
                } => {
                    let src = Source::explosion(from.map(|p| (0, p)));
                    self.splayer.attack(&self.world, src, amount, d);
                    for a in 0..3 {
                        self.splayer.e.motion[a] += knock[a];
                    }
                    if self.splayer.velocity.is_some() {
                        self.splayer.velocity = Some(self.splayer.e.motion);
                    }
                    self.push_add = [
                        self.push_add[0] + knock[0],
                        self.push_add[1] + knock[1],
                        self.push_add[2] + knock[2],
                    ];
                }
                Hit::Potion {
                    effects,
                    scale,
                    from,
                } => {
                    // EntityPotion.onImpact
                    for e in effects {
                        let pt = e.potion();
                        if pt.instant {
                            // Potion.affectEntity
                            if e.id == potion::HARM {
                                let n = (scale * (6 << e.amplifier) as f64 + 0.5) as i32;
                                self.splayer.attack(
                                    &self.world,
                                    Source::indirect_magic(Some((0, from))),
                                    n as f32,
                                    d,
                                );
                            } else if e.id == potion::HEAL {
                                let n = (scale * (4 << e.amplifier) as f64 + 0.5) as i32;
                                self.splayer.living.heal(n as f32);
                            }
                        } else {
                            let dur = (scale * e.duration as f64 + 0.5) as i32;
                            if dur > 20 {
                                self.splayer.living.add_effect(potion::Effect::new(
                                    e.id,
                                    dur,
                                    e.amplifier,
                                ));
                            }
                        }
                    }
                }
                Hit::Xp(n) => self.splayer.add_xp(n),
                Hit::Teleport(p) => {
                    // EntityEnderPearl.onImpact: setPositionAndUpdate, then
                    // fallDistance = 0 and 5 fall damage
                    self.teleport_to = Some(p);
                    self.splayer.e.fall_distance = 0.0;
                    let src = crate::entity::living::Source::named(
                        crate::entity::living::SourceKind::Fall,
                    );
                    self.splayer.attack(&self.world, src, 5.0, d);
                }
                Hit::Burn(t) => self.splayer.e.set_fire(t / 20),
            }
        }
    }

    /// `EntityArrow.onCollideWithPlayer` for the arrows in reach.
    fn pick_up_arrows(&mut self, events: &mut Vec<Event>) {
        if self.splayer.dead || self.respawn_pending {
            return;
        }
        let (f, _) = self.play.player;
        let reach = crate::entity::player::Aabb {
            min: [f[0] - 0.3 - 1.0, f[1] - 0.5, f[2] - 0.3 - 1.0],
            max: [f[0] + 0.3 + 1.0, f[1] + 1.8 + 0.5, f[2] + 0.3 + 1.0],
        };
        let creative = self.play.mode == GameMode::Creative;
        let mut sounds = Vec::new();
        for a in self.mobs.arrows.iter_mut() {
            if a.e.dead || !a.in_ground || a.arrow_shake > 0 || !a.e.bb.intersects(&reach) {
                continue;
            }
            let mut ok = a.pickup == 1 || (a.pickup == 2 && creative);
            if a.pickup == 1
                && self
                    .play
                    .inv
                    .add(crate::item::Stack::new(262, 0, 1))
                    .is_some()
            {
                ok = false;
            }
            if ok {
                let r = &mut a.e.rand;
                let p = ((r.next_float() - r.next_float()) * 0.7 + 1.0) * 2.0;
                sounds.push(("random.pop", a.e.pos, 0.2, p));
                a.e.dead = true;
                self.play.inv_dirty = true;
            }
        }
        self.mobs.arrows.retain(|a| !a.e.dead);
        if !sounds.is_empty() {
            events.push(Event::Sounds(sounds));
        }
    }

    /// The player's state for the client, and the consequences of dying.
    fn player_events(&mut self, events: &mut Vec<Event>) {
        // EntityPlayer.damageArmor, and getTotalArmorValue for the next hit
        for a in std::mem::take(&mut self.splayer.armor_wear) {
            if self.play.inv.damage_armor(a) {
                self.play.inv_dirty = true;
            }
        }
        self.splayer.living.armor = self.play.inv.armor_value();
        let fx = std::mem::take(&mut self.splayer.fx);
        let mut sounds = Vec::new();
        let mut parts = Vec::new();
        for f in fx {
            match f {
                crate::entity::base::Fx::Sound(n, p, v, pi) => sounds.push((n, p, v, pi)),
                crate::entity::base::Fx::Particle(n, p, m) => parts.push((n, p, m)),
                // Block.onFallenUpon: farmland is trampled
                crate::entity::base::Fx::Walk(x, y, z, id) => {
                    if id == crate::block::FARMLAND
                        && self.world.rand.next_float() < self.splayer.e.fall_distance - 0.5
                    {
                        self.world.set_block(x, y, z, crate::block::DIRT, 0, 3);
                    }
                }
                _ => {}
            }
        }
        if !sounds.is_empty() {
            events.push(Event::Sounds(sounds));
        }
        if !parts.is_empty() {
            events.push(Event::Particles(parts));
        }
        if std::mem::take(&mut self.splayer.hurt_anim) {
            events.push(Event::Hurt);
        }
        if let Some(p) = self.teleport_to.take() {
            self.play.player.0 = p;
            events.push(Event::Teleport(p));
        }
        if let Some(p) = self.play.sign_open.take() {
            events.push(Event::OpenSign(p));
        }
        for (p, t) in std::mem::take(&mut self.play.sign_updates) {
            events.push(Event::SignText(p, t));
        }
        if std::mem::take(&mut self.mode_changed) {
            events.push(Event::GameMode(self.play.mode));
        }
        if !self.chat_out.is_empty() {
            events.push(Event::Chat(std::mem::take(&mut self.chat_out)));
        }
        let splashes = std::mem::take(&mut self.mobs.splashes);
        if !splashes.is_empty() {
            events.push(Event::Splashes(splashes));
        }
        if std::mem::take(&mut self.using_changed) {
            events.push(Event::Using(self.splayer.using.map(|(_, st, n)| (st, n))));
        }
        let set = self.splayer.velocity.take();
        let add = std::mem::take(&mut self.push_add);
        if set.is_some() || add != [0.0; 3] {
            events.push(Event::Velocity { set, add });
        }
        if self.splayer.dead && !self.splayer.death_reported {
            self.splayer.death_reported = true;
            self.drop_inventory();
            let names = |s: &crate::entity::living::Source| -> Option<String> {
                use crate::entity::living::SourceKind as K;
                match s.kind {
                    K::Mob(k) => Some(format!("entity.{}.name", k.id())),
                    K::Arrow if s.attacker.is_some() => Some("entity.Skeleton.name".into()),
                    K::Fireball if s.attacker.is_some() => Some("entity.Ghast.name".into()),
                    _ => None,
                }
            };
            let msg = self.splayer.death_message(&names);
            events.push(Event::Died(msg));
        }
        let v = self.splayer.view();
        if self.splayer.last_sent.as_ref() != Some(&v) {
            self.splayer.last_sent = Some(v.clone());
            events.push(Event::PlayerStats(v));
        }
    }

    /// `TileEntityPiston.launchWithSlimeBlock`: every entity in a moving
    /// block's box is moved with it (`Entity.moveEntity`).
    fn apply_pushes(&mut self, play: &mut Play, events: &mut Vec<Event>) {
        let pushes = std::mem::take(&mut self.world.pushes);
        if pushes.is_empty() {
            return;
        }
        let mut to_player = Vec::new();
        for (bb, d) in pushes {
            let b = crate::entity::player::Aabb {
                min: bb.min,
                max: bb.max,
            };
            let (f, _) = play.player;
            let pb = crate::entity::player::Aabb {
                min: [f[0] - 0.3, f[1], f[2] - 0.3],
                max: [f[0] + 0.3, f[1] + 1.8, f[2] + 0.3],
            };
            if !self.respawn_pending && pb.intersects(&b) {
                to_player.push((bb.min, bb.max, d));
            }
            for m in self.mobs.list.iter_mut() {
                if !m.dead && m.bb.intersects(&b) {
                    m.push_move(&self.world, d[0], d[1], d[2]);
                }
            }
            let mut fx = Vec::new();
            for it in play.items.iter_mut() {
                if !it.e.dead && it.e.bb.intersects(&b) {
                    it.e.move_entity(
                        &self.world as &dyn crate::world::BlockView,
                        d[0],
                        d[1],
                        d[2],
                        false,
                        &[],
                        &mut fx,
                    );
                }
            }
            for f in fx {
                if let crate::entity::base::Fx::Sound(..) = f {
                    play.fx.push(f);
                }
            }
        }
        if !to_player.is_empty() {
            events.push(Event::Push(to_player));
        }
    }

    /// Order-independent digest of the whole simulation state.
    pub fn state_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |bytes: &[u8]| {
            for &b in bytes {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        let mut keys: Vec<(i32, i32)> = self.world.chunks.keys().copied().collect();
        keys.sort();
        for key in keys {
            let c = &self.world.chunks[&key];
            mix(&key.0.to_le_bytes());
            mix(&key.1.to_le_bytes());
            mix(&c.data.blocks[..]);
            mix(&c.data.meta[..]);
            mix(&c.data.sky[..]);
            mix(&c.data.blk[..]);
            mix(&c.sections.to_le_bytes());
            mix(&[c.terrain_populated as u8, c.light_populated as u8]);
        }
        for t in self.world.pending_tick_list() {
            mix(&t.0.to_le_bytes());
            mix(&t.1.to_le_bytes());
            mix(&t.2.to_le_bytes());
            mix(&[t.3]);
            mix(&t.4.to_le_bytes());
        }
        mix(&self.world.total_time.to_le_bytes());
        mix(&self.world_time.to_le_bytes());
        h
    }
}

/// `World.calculateSkylightSubtracted(1.0)` for a world time.
/// `World.calculateSkylightSubtracted` with rain and thunder.
pub fn skylight_subtracted_weather(world_time: i64, rain: f32, thunder: f32) -> i32 {
    let f = celestial_angle(world_time, 1.0);
    let mut f1 =
        1.0f32 - (crate::util::math::cos(f * crate::util::math::PI * 2.0f32) * 2.0f32 + 0.5f32);
    f1 = f1.clamp(0.0, 1.0);
    f1 = 1.0 - f1;
    f1 = (f1 as f64 * (1.0 - (rain * 5.0f32) as f64 / 16.0)) as f32;
    f1 = (f1 as f64 * (1.0 - (thunder * 5.0f32) as f64 / 16.0)) as f32;
    f1 = 1.0 - f1;
    (f1 * 11.0f32) as i32
}

pub fn skylight_subtracted(world_time: i64) -> i32 {
    let f = celestial_angle(world_time, 1.0);
    let mut f1 =
        1.0f32 - (crate::util::math::cos(f * crate::util::math::PI * 2.0f32) * 2.0f32 + 0.5f32);
    f1 = f1.clamp(0.0, 1.0);
    f1 = 1.0 - f1;
    f1 = 1.0 - f1;
    (f1 * 11.0f32) as i32
}

/// `WorldProvider.calculateCelestialAngle`
pub fn celestial_angle(world_time: i64, partial: f32) -> f32 {
    let j = (world_time % 24000) as i32;
    let mut f1 = (j as f32 + partial) / 24000.0f32 - 0.25f32;
    if f1 < 0.0 {
        f1 += 1.0;
    }
    if f1 > 1.0 {
        f1 -= 1.0;
    }
    let f2 = f1;
    f1 = 1.0f32 - (((f1 as f64 * std::f64::consts::PI).cos() + 1.0) / 2.0) as f32;
    f2 + (f1 - f2) / 3.0f32
}

pub struct SimHandle {
    pub commands: Sender<Command>,
    pub events: Receiver<Event>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl SimHandle {
    /// Stops the server thread; it saves the world before exiting.
    pub fn shutdown(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for SimHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// `Command::Dig` and `Command::Use` handled so far, by any world of this
/// process: the client numbers the ones it sends the same way.
pub static ACTIONS_DONE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Server ticks run and the nanoseconds they took (the F3 graph's
/// `tickTimeArray`; read by the benchmark).
pub static TICK_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static TICK_NANOS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static TICK_WORST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Runs the simulation on its own thread at 20 ticks per second.
pub fn spawn(mut sim: Sim) -> SimHandle {
    let (cmd_tx, cmd_rx) = channel::<Command>();
    let (ev_tx, ev_rx) = channel::<Event>();
    let thread = std::thread::Builder::new()
        .name("server".into())
        .stack_size(16 << 20)
        .spawn(move || {
            let tick = Duration::from_millis(50);
            let mut next = Instant::now();
            let mut events = Vec::new();
            loop {
                let mut commands = Vec::new();
                let mut stop = false;
                loop {
                    match cmd_rx.try_recv() {
                        Ok(Command::Shutdown) => stop = true,
                        Ok(c) => commands.push(c),
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            stop = true;
                            break;
                        }
                    }
                }
                if stop {
                    for c in &commands {
                        sim.apply(c);
                    }
                    if let Err(e) = sim.save() {
                        eprintln!("failed to save the world: {e}");
                    }
                    return;
                }
                let began = Instant::now();
                sim.step(&commands, &mut events);
                {
                    use std::sync::atomic::Ordering::Relaxed;
                    let ns = began.elapsed().as_nanos() as u64;
                    TICK_COUNT.fetch_add(1, Relaxed);
                    TICK_NANOS.fetch_add(ns, Relaxed);
                    TICK_WORST.fetch_max(ns, Relaxed);
                }
                events.push(Event::Tick(
                    Instant::now(),
                    ACTIONS_DONE.load(std::sync::atomic::Ordering::Relaxed),
                ));
                for e in events.drain(..) {
                    if ev_tx.send(e).is_err() {
                        let _ = sim.save();
                        return;
                    }
                }
                next += tick;
                let now = Instant::now();
                if next > now {
                    std::thread::sleep(next - now);
                } else if now - next > Duration::from_secs(1) {
                    // Hopelessly behind (initial load): do not try to catch up.
                    next = now;
                }
            }
        })
        .expect("failed to start the server thread");
    SimHandle {
        commands: cmd_tx,
        events: ev_rx,
        thread: Some(thread),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(seed: i64, ticks: usize) -> (u64, usize) {
        let cfg = SimConfig {
            seed,
            view: 2,
            save_dir: None,
            start_time: 0,
            freeze_time: false,
            mode: GameMode::Creative,
        };
        let (mut sim, start) = Sim::new(&cfg);
        let (cx, cz) = (
            (start.pos[0].floor() as i32) >> 4,
            (start.pos[2].floor() as i32) >> 4,
        );
        let (bx, by, bz) = (
            start.pos[0].floor() as i32,
            start.pos[1] as i32,
            start.pos[2].floor() as i32,
        );
        let mut events = Vec::new();
        let mut count = 0;
        for t in 0..ticks {
            // A scripted session: wander, dig a shaft, pour water, drop sand.
            let mut cmds = vec![Command::Center(cx + (t as i32 / 40), cz - (t as i32 / 60))];
            if t >= 10 && t < 20 {
                cmds.push(Command::Break([bx, by - 1 - (t as i32 - 10), bz]));
            }
            if t == 30 {
                cmds.push(Command::Place {
                    pos: [bx + 1, by + 3, bz],
                    id: block::STONE,
                    meta: 0,
                });
                cmds.push(Command::Place {
                    pos: [bx + 1, by + 4, bz],
                    id: block::FLOWING_WATER,
                    meta: 0,
                });
            }
            if t == 35 {
                cmds.push(Command::Place {
                    pos: [bx - 2, by + 6, bz],
                    id: block::SAND,
                    meta: 0,
                });
                cmds.push(Command::Break([bx + 1, by + 3, bz]));
            }
            sim.step(&cmds, &mut events);
            count += events.len();
            events.clear();
        }
        (sim.state_hash(), count)
    }

    /// Gate G1: the same seed and the same input script give the same world,
    /// run after run, regardless of how the generation threads are scheduled.
    #[test]
    fn replay_is_deterministic() {
        let a = run(2026, 120);
        let b = run(2026, 120);
        let c = run(2026, 120);
        assert_eq!(a, b);
        assert_eq!(a, c);
        assert_ne!(a.0, run(2027, 120).0, "different seeds must differ");
    }

    /// Saving and reloading mid-session must not change the outcome.
    #[test]
    fn save_and_reload_round_trips() {
        let dir = std::env::temp_dir().join(format!("minerust-sim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cfg = SimConfig {
            seed: 77,
            view: 2,
            save_dir: Some(dir.clone()),
            start_time: 0,
            freeze_time: false,
            mode: GameMode::Creative,
        };
        let mut events = Vec::new();
        let (hash_before, pos) = {
            let (mut sim, start) = Sim::new(&cfg);
            let (bx, by, bz) = (
                start.pos[0].floor() as i32,
                start.pos[1] as i32,
                start.pos[2].floor() as i32,
            );
            for t in 0..60 {
                let cmds = if t == 20 {
                    vec![
                        Command::Break([bx, by - 1, bz]),
                        Command::Place {
                            pos: [bx, by + 2, bz],
                            id: block::COBBLESTONE,
                            meta: 0,
                        },
                    ]
                } else {
                    vec![]
                };
                sim.step(&cmds, &mut events);
                events.clear();
            }
            sim.save().unwrap();
            (sim.state_hash(), [bx, by, bz])
        };
        let (mut sim, start) = Sim::new(&cfg);
        assert!(start.resumed);
        assert_eq!(sim.world.total_time, 60);
        // Bring the same chunks back and compare block for block.
        sim.step(&[], &mut events);
        assert_eq!(sim.world.block(pos[0], pos[1] - 1, pos[2]), block::AIR);
        assert_eq!(
            sim.world.block(pos[0], pos[1] + 2, pos[2]),
            block::COBBLESTONE
        );
        let _ = hash_before;
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Survival loop: dig with a pickaxe, the drop is picked up, placing
    /// consumes it, and the crafting grid works through the server.
    #[test]
    fn survival_dig_pickup_place_and_craft() {
        let cfg = SimConfig {
            seed: 2026,
            view: 2,
            save_dir: None,
            start_time: 0,
            freeze_time: true,
            mode: GameMode::Survival,
        };
        let (mut sim, start) = Sim::new(&cfg);
        let mut ev = Vec::new();
        for _ in 0..3 {
            sim.step(&[], &mut ev);
        }
        let (bx, by, bz) = (
            start.pos[0].floor() as i32,
            start.pos[1] as i32,
            start.pos[2].floor() as i32,
        );
        let below = [bx, by - 1, bz];
        // Make sure there is stone to dig right under the player.
        sim.world
            .set_block(below[0], below[1], below[2], block::STONE, 0, 3);
        let feet = [start.pos[0], start.pos[1], start.pos[2]];
        let pick = crate::item::Stack::new(270, 0, 1);
        sim.step(
            &[
                Command::Player(feet, [0.0, 0.0], false),
                Command::Give(pick),
                Command::Select(0),
                Command::Dig(below),
            ],
            &mut ev,
        );
        assert_eq!(sim.world.block(below[0], below[1], below[2]), block::AIR);
        assert_eq!(
            sim.play.inv.main[0].map(|s| s.meta),
            Some(1),
            "pickaxe loses durability"
        );
        let mut picked = false;
        // The player drops into the hole it dug.
        let in_hole = [feet[0], feet[1] - 1.0, feet[2]];
        for _ in 0..60 {
            sim.step(&[Command::Player(in_hole, [0.0, 0.0], false)], &mut ev);
            if sim
                .play
                .inv
                .main
                .iter()
                .flatten()
                .any(|s| s.id == block::COBBLESTONE as u16)
            {
                picked = true;
                break;
            }
        }
        assert!(picked, "cobblestone was not picked up");
        assert!(sim.play.items.is_empty());
        // Digging stone by hand yields nothing.
        let side = [bx + 2, by - 1, bz];
        sim.world
            .set_block(side[0], side[1], side[2], block::STONE, 0, 3);
        sim.step(&[Command::Select(8), Command::Dig(side)], &mut ev);
        assert_eq!(sim.world.block(side[0], side[1], side[2]), block::AIR);
        assert!(sim.play.items.is_empty(), "stone dug by hand must not drop");
        // Place the cobblestone back: the stack shrinks.
        let slot = sim
            .play
            .inv
            .main
            .iter()
            .position(|s| matches!(s, Some(s) if s.id == block::COBBLESTONE as u16))
            .unwrap();
        sim.world.set_block(bx + 3, by - 1, bz, block::STONE, 0, 3);
        sim.world.set_block(bx + 3, by, bz, block::AIR, 0, 3);
        sim.step(
            &[
                Command::Select(slot),
                Command::Use {
                    target: [bx + 3, by - 1, bz],
                    face: [0, 1, 0],
                    sneaking: false,
                },
            ],
            &mut ev,
        );
        assert_eq!(sim.world.block(bx + 3, by, bz), block::COBBLESTONE);
        assert!(sim.play.inv.main[slot].is_none());
        // Craft planks from a log in the 2x2 grid.
        sim.step(
            &[
                Command::Give(crate::item::Stack::new(block::LOG as u16, 0, 1)),
                Command::OpenInventory,
            ],
            &mut ev,
        );
        let log_slot = sim
            .play
            .inv
            .main
            .iter()
            .position(|s| matches!(s, Some(s) if s.id == block::LOG as u16))
            .unwrap();
        sim.step(
            &[
                // ContainerPlayer: hotbar slots are 36..45, the grid 1..5
                Command::WindowClick {
                    slot: if log_slot < 9 {
                        36 + log_slot
                    } else {
                        log_slot
                    } as i32,
                    button: 0,
                    mode: 0,
                },
                Command::WindowClick {
                    slot: 1,
                    button: 0,
                    mode: 0,
                },
                Command::WindowClick {
                    slot: 0,
                    button: 0,
                    mode: 1,
                },
                Command::CloseWindow,
            ],
            &mut ev,
        );
        assert_eq!(
            sim.play
                .inv
                .main
                .iter()
                .flatten()
                .filter(|s| s.id == block::PLANKS as u16)
                .map(|s| s.count)
                .sum::<u32>(),
            4
        );
    }

    #[test]
    #[ignore]
    fn mob_body_follows_walking() {
        let cfg = SimConfig {
            seed: 12345,
            view: 3,
            save_dir: None,
            start_time: 6000,
            freeze_time: true,
            mode: GameMode::Creative,
        };
        let (mut sim, start) = Sim::new(&cfg);
        let mut ev = Vec::new();
        for _ in 0..40 {
            sim.step(&[], &mut ev);
        }
        let p = start.pos;
        let id = 900000;
        sim.mobs.list.push(crate::entity::mob::Mob::new(
            id,
            crate::entity::mob::Kind::Cow,
            [p[0] + 3.0, p[1] + 1.0, p[2] + 3.0],
            0.0,
            7,
        ));
        for t in 0..600 {
            sim.step(&[], &mut ev);
            if let Some(m) = sim.mobs.list.iter().find(|m| m.id == id) {
                let (dx, dz) = (m.pos[0] - m.prev_pos[0], m.pos[2] - m.prev_pos[2]);
                if dx * dx + dz * dz > 1e-6 {
                    let dir = (dz.atan2(dx) * 180.0 / std::f64::consts::PI) as f32 - 90.0;
                    eprintln!(
                        "{t} pos {:.3} {:.3} move {:.1} yaw {:.1} body {:.1} head {:.1}",
                        m.pos[0], m.pos[2], dir, m.yaw, m.body_yaw, m.head_yaw
                    );
                }
            }
        }
    }

    fn redstone_sim() -> (Sim, i32, i32, i32) {
        let cfg = SimConfig {
            seed: 4242,
            view: 2,
            save_dir: None,
            start_time: 6000,
            freeze_time: true,
            mode: GameMode::Creative,
        };
        let (mut sim, start) = Sim::new(&cfg);
        let mut ev = Vec::new();
        // Let the spawn area finish generating and drain the gravel ticks
        // ore population schedules, as the spawn preparation does.
        for _ in 0..400 {
            sim.step(&[], &mut ev);
            if sim.world.pending_ticks() < 200 {
                break;
            }
        }
        let (x, z) = (
            start.pos[0].floor() as i32 + 2,
            start.pos[2].floor() as i32 - 6,
        );
        let y = 200;
        for dx in -2..12 {
            for dz in -2..14 {
                sim.world.set_block(x + dx, y, z + dz, block::STONE, 0, 3);
            }
        }
        (sim, x, y + 1, z)
    }

    fn steps(sim: &mut Sim, n: usize) {
        let mut ev = Vec::new();
        for _ in 0..n {
            sim.step(&[], &mut ev);
        }
    }

    #[test]
    fn lever_wire_lamp_and_torch_inverter() {
        let (mut sim, x, y, z) = redstone_sim();
        let w = &mut sim.world;
        w.set_block(x, y, z, block::LEVER, 5, 3);
        for i in 1..4 {
            w.set_block(x + i, y, z, block::REDSTONE_WIRE, 0, 3);
        }
        w.set_block(x + 4, y, z, block::REDSTONE_LAMP, 0, 3);
        w.toggle_lever(x, y, z);
        assert_eq!(w.meta(x + 1, y, z), 15);
        assert_eq!(w.meta(x + 3, y, z), 13);
        assert_eq!(w.block(x + 4, y, z), block::LIT_REDSTONE_LAMP);
        w.toggle_lever(x, y, z);
        assert_eq!(w.meta(x + 1, y, z), 0);
        assert_eq!(
            w.block(x + 4, y, z),
            block::LIT_REDSTONE_LAMP,
            "lamps go out after 4 ticks"
        );
        steps(&mut sim, 5);
        assert_eq!(sim.world.block(x + 4, y, z), block::REDSTONE_LAMP);

        // A torch on the side of a block, inverted by a lever on top.
        let w = &mut sim.world;
        let (sx, sz) = (x, z + 3);
        w.set_block(sx, y, sz, block::STONE, 0, 3);
        w.set_block(sx + 1, y, sz, block::REDSTONE_TORCH, 1, 3);
        w.set_block(sx, y + 1, sz, block::LEVER, 5, 3);
        assert_eq!(w.block(sx + 1, y, sz), block::REDSTONE_TORCH);
        w.toggle_lever(sx, y + 1, sz);
        steps(&mut sim, 3);
        assert_eq!(sim.world.block(sx + 1, y, sz), block::UNLIT_REDSTONE_TORCH);
        sim.world.toggle_lever(sx, y + 1, sz);
        steps(&mut sim, 3);
        assert_eq!(sim.world.block(sx + 1, y, sz), block::REDSTONE_TORCH);
    }

    #[test]
    fn repeater_delays_and_piston_pushes() {
        let (mut sim, x, y, z) = redstone_sim();
        let w = &mut sim.world;
        // Lever, repeater with its input at -x (direction 1), lamp.
        w.set_block(x, y, z, block::LEVER, 5, 3);
        w.set_block(x + 1, y, z, block::UNPOWERED_REPEATER, 1, 3);
        w.set_block(x + 2, y, z, block::REDSTONE_LAMP, 0, 3);
        w.toggle_lever(x, y, z);
        assert_eq!(w.block(x + 2, y, z), block::REDSTONE_LAMP);
        steps(&mut sim, 1);
        assert_eq!(sim.world.block(x + 2, y, z), block::REDSTONE_LAMP);
        steps(&mut sim, 2);
        assert_eq!(sim.world.block(x + 1, y, z), block::POWERED_REPEATER);
        assert_eq!(sim.world.block(x + 2, y, z), block::LIT_REDSTONE_LAMP);

        // Piston facing east with cobblestone in front; a lever on the
        // stone behind it powers it. The block event runs at the end of the
        // tick, then TileEntityPiston moves the blocks over two ticks and
        // places them on the third.
        let w = &mut sim.world;
        let pz = z + 6;
        w.set_block(x, y, pz, block::PISTON, 5, 3);
        w.set_block(x + 1, y, pz, block::COBBLESTONE, 0, 3);
        w.set_block(x - 1, y, pz, block::STONE, 0, 3);
        w.set_block(x - 1, y + 1, pz, block::LEVER, 5, 3);
        w.toggle_lever(x - 1, y + 1, pz);
        assert_eq!(
            w.meta(x, y, pz),
            5,
            "the event waits for the end of the tick"
        );
        steps(&mut sim, 1);
        let w = &mut sim.world;
        assert_eq!(w.meta(x, y, pz), 13);
        assert_eq!(w.block(x + 1, y, pz), block::PISTON_EXTENSION);
        assert_eq!(w.block(x + 2, y, pz), block::PISTON_EXTENSION);
        assert_eq!(w.moving.len(), 2);
        assert_eq!(
            w.moving_at([x + 2, y, pz]).unwrap().block,
            block::COBBLESTONE
        );
        assert_eq!(w.moving_at([x + 2, y, pz]).unwrap().progress, 0.5);
        steps(&mut sim, 2);
        let w = &mut sim.world;
        assert_eq!(w.block(x + 1, y, pz), block::PISTON_HEAD);
        assert_eq!(w.block(x + 2, y, pz), block::COBBLESTONE);
        assert!(w.moving.is_empty());
        w.toggle_lever(x - 1, y + 1, pz);
        assert_eq!(w.meta(x, y, pz), 5);
        steps(&mut sim, 1);
        let w = &mut sim.world;
        // Retracting: the base becomes a moving block that turns back into
        // the piston; the head is removed at once.
        assert_eq!(w.block(x, y, pz), block::PISTON_EXTENSION);
        assert_eq!(w.block(x + 1, y, pz), block::AIR);
        steps(&mut sim, 2);
        let w = &mut sim.world;
        assert_eq!(w.block(x, y, pz), block::PISTON);
        assert_eq!(w.meta(x, y, pz), 5);
        assert_eq!(w.block(x + 2, y, pz), block::COBBLESTONE);

        // A sticky piston pulls the block back.
        let sz = z + 9;
        w.set_block(x, y, sz, block::STICKY_PISTON, 5, 3);
        w.set_block(x + 1, y, sz, block::COBBLESTONE, 0, 3);
        w.set_block(x - 1, y, sz, block::STONE, 0, 3);
        w.set_block(x - 1, y + 1, sz, block::LEVER, 5, 3);
        w.toggle_lever(x - 1, y + 1, sz);
        steps(&mut sim, 3);
        let w = &mut sim.world;
        assert_eq!(w.block(x + 2, y, sz), block::COBBLESTONE);
        assert_eq!(w.block(x + 1, y, sz), block::PISTON_HEAD);
        w.toggle_lever(x - 1, y + 1, sz);
        steps(&mut sim, 1);
        let w = &mut sim.world;
        assert_eq!(w.block(x + 1, y, sz), block::PISTON_EXTENSION);
        assert_eq!(w.block(x + 2, y, sz), block::AIR);
        steps(&mut sim, 2);
        let w = &mut sim.world;
        assert_eq!(w.block(x + 1, y, sz), block::COBBLESTONE);
        assert_eq!(w.block(x, y, sz), block::STICKY_PISTON);
    }

    /// BlockFalling.tryFall spawns an EntityFallingBlock that lands two
    /// blocks lower; drops.dat keeps items over a save.
    #[test]
    fn sand_falls_as_an_entity() {
        let (mut sim, x, y, z) = redstone_sim();
        sim.world.set_block(x, y + 3, z, block::SAND, 1, 3);
        assert_eq!(sim.world.block(x, y + 3, z), block::SAND);
        assert_eq!(sim.world.meta(x, y + 3, z), 1);
        let mut seen = false;
        for _ in 0..40 {
            steps(&mut sim, 1);
            // Exposed cave gravel falls too (WorldGenMinable scheduled it).
            if let Some(f) = sim.play.falling.iter().find(|f| f.block == block::SAND) {
                seen = true;
                assert_eq!(f.meta, 1);
            }
        }
        assert!(seen, "an EntityFallingBlock appeared");
        assert!(!sim.play.falling.iter().any(|f| f.block == block::SAND));
        assert_eq!(sim.world.block(x, y + 3, z), block::AIR);
        assert_eq!(sim.world.block(x, y, z), block::SAND);
        assert_eq!(sim.world.meta(x, y, z), 1, "red sand stays red");

        let mut r = JavaRandom::new(3);
        let it = crate::entity::item::ItemEntity::new(
            7,
            crate::item::Stack::new(crate::item::STICK, 0, 5),
            [x as f64 + 0.5, y as f64 + 1.0, z as f64 + 0.5],
        );
        let bytes = Play::drops_nbt(&[it], &[], &[], 0);
        let mut back = Play::new(GameMode::Survival);
        back.load_drops(&bytes, &mut r);
        assert_eq!(back.items.len(), 1);
        assert_eq!(back.items[0].stack.count, 5);
    }

    /// ItemFood, ItemPotion and ItemBow through setItemInUse.
    #[test]
    fn eat_drink_and_shoot() {
        let (mut sim, x, y, z) = redstone_sim();
        // EntityPlayer.canEat: not in creative
        sim.play.mode = GameMode::Survival;
        let feet = [x as f64 + 0.5, y as f64, z as f64 + 0.5];
        sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut Vec::new());
        sim.splayer.food.level = 10;
        sim.play.inv.selected = 0;
        sim.play.inv.main[0] = Some(crate::item::Stack::new(364, 0, 2));
        steps(&mut sim, 1);
        sim.step(&[Command::UseAir], &mut Vec::new());
        assert!(sim.splayer.using.is_some(), "steak is being eaten");
        steps(&mut sim, 32);
        assert!(sim.splayer.using.is_none());
        assert_eq!(sim.splayer.food.level, 18);
        assert_eq!(sim.play.inv.main[0].unwrap().count, 1);

        // Potion of Swiftness (8194)
        sim.play.inv.main[0] = Some(crate::item::Stack::new(373, 8194, 1));
        sim.step(&[Command::UseAir], &mut Vec::new());
        steps(&mut sim, 32);
        assert!(sim.splayer.living.active(crate::potion::MOVE_SPEED));
        assert_eq!(
            sim.play.inv.main[0].unwrap().id,
            374,
            "a glass bottle is left"
        );

        // the bow, fully drawn, against the stone floor
        sim.play.inv.main[0] = Some(crate::item::Stack::new(261, 0, 1));
        sim.play.inv.main[1] = Some(crate::item::Stack::new(262, 0, 5));
        sim.play.pitch = 60.0;
        sim.step(&[Command::UseAir], &mut Vec::new());
        steps(&mut sim, 25);
        sim.step(&[Command::StopUsing], &mut Vec::new());
        assert_eq!(sim.mobs.arrows.len(), 1);
        // (crit is cleared once it sticks, on its first tick here)
        assert_eq!(sim.play.inv.main[1].unwrap().count, 4);
        assert_eq!(sim.play.inv.main[0].unwrap().meta, 1, "the bow wears");
        for _ in 0..40 {
            steps(&mut sim, 1);
            if sim.mobs.arrows.first().is_some_and(|a| a.in_ground) {
                break;
            }
        }
        assert!(
            sim.mobs.arrows.first().is_some_and(|a| a.in_ground),
            "the arrow sticks in the ground"
        );
    }

    /// TileEntityBrewingStand: water bottles and nether wart make awkward
    /// potions in 400 ticks; sugar then makes swiftness.
    #[test]
    fn brewing_stand_brews() {
        use crate::item::Stack;
        use crate::play::Tile;
        let (mut sim, x, y, z) = redstone_sim();
        sim.world.set_block(x, y, z, block::BREWING_STAND, 0, 3);
        let p = [x, y, z];
        sim.play.tiles.insert(
            p,
            Tile::Brewing {
                slots: [
                    Some(Stack::new(373, 0, 1)),
                    Some(Stack::new(373, 0, 1)),
                    None,
                    Some(Stack::new(372, 0, 1)),
                ],
                brew_time: 0,
                filled: 0,
                ingredient: 0,
            },
        );
        steps(&mut sim, 1);
        assert_eq!(sim.world.meta(x, y, z), 3, "two bottles show on the block");
        steps(&mut sim, 400);
        let Some(Tile::Brewing { slots, .. }) = sim.play.tiles.get(&p).cloned() else {
            panic!()
        };
        assert_eq!(slots[0].unwrap().meta, 16, "awkward potion");
        assert!(slots[3].is_none(), "the wart is used up");
        if let Some(Tile::Brewing { slots, .. }) = sim.play.tiles.get_mut(&p) {
            slots[3] = Some(Stack::new(353, 0, 1));
        }
        steps(&mut sim, 402);
        let Some(Tile::Brewing { slots, .. }) = sim.play.tiles.get(&p).cloned() else {
            panic!()
        };
        let e = crate::potion::effects_of(slots[0].unwrap().meta, false);
        assert_eq!(e[0].id, crate::potion::MOVE_SPEED);
    }

    /// EntityExpBottle breaks into 3..11 experience; the EntityXPOrbs fly
    /// to the player and onCollideWithPlayer adds them, one per xpCooldown.
    #[test]
    fn experience_orbs_are_collected() {
        use crate::entity::vehicle::Vehicle;
        let (mut sim, x, y, z) = redstone_sim();
        sim.play.mode = GameMode::Survival;
        let feet = [x as f64 + 3.5, y as f64, z as f64 + 3.5];
        for _ in 0..2 {
            sim.step(
                &[Command::Player(feet, [0.0, 90.0], false)],
                &mut Vec::new(),
            );
        }
        let before = sim.splayer.xp_total;
        let sel = sim.play.inv.selected;
        sim.play.inv.main[sel] = Some(crate::item::Stack::new(384, 0, 1));
        let mut ev0 = Vec::new();
        sim.step(&[Command::UseAir], &mut ev0);
        let mut seen = 0;
        let mut collected = ev0
            .iter()
            .filter(|e| matches!(e, Event::CollectOrb(_)))
            .count();
        for _ in 0..100 {
            let mut ev = Vec::new();
            sim.step(&[Command::Player(feet, [0.0, 90.0], false)], &mut ev);
            let n = sim
                .play
                .vehicles
                .iter()
                .filter(|v| matches!(v.v, Vehicle::Orb(_)))
                .count();
            seen = seen.max(n);
            collected += ev
                .iter()
                .filter(|e| matches!(e, Event::CollectOrb(_)))
                .count();
        }
        let _ = seen;
        assert!(collected >= 1, "orbs were picked up");
        let gained = sim.splayer.xp_total - before;
        assert!((3..=11).contains(&gained), "gained {gained}");
        assert!(!sim
            .play
            .vehicles
            .iter()
            .any(|v| matches!(v.v, Vehicle::Orb(_))));
    }

    /// Explosion.doExplosionA: World.getBlockDensity shields a player
    /// behind obsidian; TNT breaks the platform and primes the TNT next to it.
    #[test]
    fn explosions_follow_block_density() {
        let run = |wall: bool| -> (f32, bool, usize) {
            let (mut sim, x, y, z) = redstone_sim();
            sim.play.mode = GameMode::Survival;
            let feet = [x as f64 + 4.5, y as f64, z as f64 + 0.5];
            if wall {
                for dy in 0..4 {
                    for dx in 0..9 {
                        sim.world
                            .set_block(x + dx, y + dy, z + 2, block::OBSIDIAN, 0, 3);
                    }
                }
            }
            sim.world.set_block(x + 4, y, z + 6, block::TNT, 0, 3);
            for _ in 0..2 {
                sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut Vec::new());
            }
            sim.splayer.living.health = 20.0;
            sim.mobs
                .prime_tnt([x as f64 + 4.5, y as f64 + 0.5, z as f64 + 5.5], 0);
            let mut ev = Vec::new();
            for _ in 0..3 {
                sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut ev);
            }
            let broke = sim.world.block(x + 4, y - 1, z + 5) == block::AIR;
            let chained = sim.mobs.tnt.len();
            (20.0 - sim.splayer.living.health, broke, chained)
        };
        let (open, broke, chained) = run(false);
        let (shielded, _, _) = run(true);
        assert!(broke, "the platform under the TNT is gone");
        assert_eq!(chained, 1, "the TNT beside it was primed");
        assert!(open > shielded, "open {open} vs behind obsidian {shielded}");
        assert!(
            shielded <= 1.5,
            "only the +1 of (int)(...+1.0) gets through: {shielded}"
        );
    }

    /// ItemMinecart.onItemUse puts a cart on a rail; moveOnRail keeps it on
    /// the line, a detector rail powers under it, a rider's forward push
    /// starts it from rest, and a broken chest minecart drops the cart, the
    /// chest and its contents.
    #[test]
    fn minecarts_on_rails() {
        use crate::entity::vehicle::Vehicle;
        let (mut sim, x, y, z) = redstone_sim();
        let rx = x + 3;
        for dz in 0..14 {
            let id = if dz == 5 {
                block::DETECTOR_RAIL
            } else {
                block::RAIL
            };
            sim.world.set_block(rx, y, z + dz, id, 0, 3);
        }
        let feet = [rx as f64 + 0.5, y as f64, z as f64 - 1.5];
        for _ in 0..2 {
            sim.step(
                &[Command::Player(feet, [0.0, 30.0], false)],
                &mut Vec::new(),
            );
        }
        let sel = sim.play.inv.selected;
        sim.play.inv.main[sel] = Some(crate::item::Stack::new(328, 0, 1));
        sim.step(
            &[Command::Use {
                target: [rx, y, z + 1],
                face: [0, 1, 0],
                sneaking: false,
            }],
            &mut Vec::new(),
        );
        assert_eq!(sim.play.vehicles.len(), 1, "placed");
        let id = sim.play.vehicles[0].v.id();
        sim.play.vehicles[0].v.e_mut().motion = [0.0, 0.0, 0.3];
        let mut powered = false;
        for _ in 0..40 {
            sim.step(
                &[Command::Player(feet, [0.0, 30.0], false)],
                &mut Vec::new(),
            );
            if sim.world.meta(rx, y, z + 5) & 8 != 0 {
                powered = true;
            }
            let p = sim.play.vehicles[0].v.e().pos;
            assert!(
                (p[0] - (rx as f64 + 0.5)).abs() < 1.0e-9,
                "on the line: {:?}",
                p
            );
        }
        assert!(powered, "the detector rail saw it");
        let p = sim.play.vehicles[0].v.e().pos;
        assert!(p[2] > z as f64 + 5.0, "rolled: {:?}", p);
        // the cart stopped against the end; ride it back with forward
        for _ in 0..80 {
            sim.step(
                &[Command::Player(feet, [0.0, 30.0], false)],
                &mut Vec::new(),
            );
        }
        let feet2 = [p[0], y as f64, p[2] - 1.5];
        sim.step(
            &[Command::Player(feet2, [180.0, 0.0], false)],
            &mut Vec::new(),
        );
        sim.step(&[Command::Interact { id }], &mut Vec::new());
        assert_eq!(sim.riding, Some(id));
        let before = sim.play.vehicles[0].v.e().pos;
        for _ in 0..20 {
            sim.step(
                &[
                    Command::Player(feet2, [180.0, 0.0], false),
                    Command::Input {
                        strafe: 0.0,
                        forward: 1.0,
                        jump: false,
                        sneak: false,
                    },
                ],
                &mut Vec::new(),
            );
        }
        let after = sim.play.vehicles[0].v.e().pos;
        assert!(
            after[2] < before[2] - 0.5,
            "pushed north: {:?} -> {:?}",
            before,
            after
        );
        sim.step(
            &[Command::Input {
                strafe: 0.0,
                forward: 0.0,
                jump: false,
                sneak: true,
            }],
            &mut Vec::new(),
        );
        assert_eq!(sim.riding, None);
        // a chest minecart with a stack in it, broken in survival
        sim.play.mode = GameMode::Survival;
        sim.play.inv.main[sel] = Some(crate::item::Stack::new(342, 0, 1));
        let feet3 = [rx as f64 + 0.5, y as f64, z as f64 - 1.5];
        sim.step(
            &[Command::Player(feet3, [0.0, 30.0], false)],
            &mut Vec::new(),
        );
        sim.step(
            &[Command::Use {
                target: [rx, y, z + 1],
                face: [0, 1, 0],
                sneaking: false,
            }],
            &mut Vec::new(),
        );
        let chest = sim
            .play
            .vehicles
            .iter_mut()
            .find(|b| matches!(&b.v, Vehicle::Cart(c) if c.kind == crate::entity::minecart::CartKind::Chest))
            .expect("chest minecart");
        let cid = chest.v.id();
        if let Vehicle::Cart(c) = &mut chest.v {
            c.slots[0] = Some(crate::item::Stack::new(block::STONE as u16, 0, 40));
        }
        sim.play.items.clear();
        for _ in 0..5 {
            sim.step(
                &[Command::Attack {
                    id: cid,
                    damage: 1.0,
                    crit: false,
                    knockback: 0,
                }],
                &mut Vec::new(),
            );
        }
        assert!(!sim.play.vehicles.iter().any(|b| b.v.id() == cid), "broken");
        let n = |id: u16| -> u32 {
            sim.play
                .items
                .iter()
                .filter(|i| i.stack.id == id)
                .map(|i| i.stack.count as u32)
                .sum()
        };
        assert_eq!(n(328), 1);
        assert_eq!(n(block::CHEST as u16), 1);
        assert_eq!(n(block::STONE as u16), 40);
    }

    /// EntityBoat.onUpdate: running into a wall faster than 0.2 breaks the
    /// boat into three planks and two sticks; the rider is let go.
    #[test]
    fn boats_wreck_on_walls() {
        let (mut sim, x, y, z) = redstone_sim();
        for dx in 0..13 {
            for dz in 0..15 {
                let edge = dx == 0 || dx == 12 || dz == 0 || dz == 14;
                let id = if edge { block::STONE } else { block::WATER };
                sim.world.set_block(x + dx, y, z + dz, id, 0, 3);
            }
        }
        let feet = [x as f64 + 6.5, (y + 1) as f64, z as f64 + 0.5];
        for _ in 0..2 {
            sim.step(
                &[Command::Player(feet, [0.0, 45.0], false)],
                &mut Vec::new(),
            );
        }
        sim.play.inv.main[sim.play.inv.selected] = Some(crate::item::Stack::new(333, 0, 1));
        sim.step(&[Command::UseAir], &mut Vec::new());
        let id = sim.play.vehicles[0].v.id();
        sim.step(&[Command::Interact { id }], &mut Vec::new());
        assert_eq!(sim.riding, Some(id));
        for _ in 0..80 {
            sim.step(
                &[
                    Command::Player(feet, [0.0, 0.0], false),
                    Command::Input {
                        strafe: 0.0,
                        forward: 1.0,
                        jump: false,
                        sneak: false,
                    },
                ],
                &mut Vec::new(),
            );
        }
        assert!(sim.play.vehicles.is_empty(), "wrecked");
        assert_eq!(sim.riding, None);
        let n = |id: u16| -> u32 {
            sim.play
                .items
                .iter()
                .filter(|i| i.stack.id == id)
                .map(|i| i.stack.count as u32)
                .sum()
        };
        assert_eq!(n(block::PLANKS as u16), 3);
        assert_eq!(n(280), 2);
    }

    /// ItemBoat places a boat on water; interactFirst mounts it, the
    /// rider paddles it with C0CPacketInput, sneaking dismounts, and five
    /// fist hits break it into a boat item.
    #[test]
    fn boats_place_ride_and_break() {
        let (mut sim, x, y, z) = redstone_sim();
        // a pool: stone walls on the platform, water inside
        for dx in 0..13 {
            for dz in 0..15 {
                let edge = dx == 0 || dx == 12 || dz == 0 || dz == 14;
                let id = if edge { block::STONE } else { block::WATER };
                sim.world.set_block(x + dx, y, z + dz, id, 0, 3);
            }
        }
        let feet = [x as f64 + 6.5, (y + 1) as f64, z as f64 + 0.5];
        let mut ev = Vec::new();
        for _ in 0..2 {
            sim.step(&[Command::Player(feet, [0.0, 45.0], false)], &mut ev);
        }
        sim.play.inv.selected = 0;
        sim.play.inv.main[0] = Some(crate::item::Stack::new(333, 0, 1));
        sim.step(&[Command::UseAir], &mut ev);
        assert_eq!(sim.play.vehicles.len(), 1, "placed on the water");
        let b0 = sim.play.vehicles[0].v.e().pos;
        assert_eq!(b0[0], x as f64 + 6.5);
        assert!(b0[2] > feet[2] + 1.0);
        let id = sim.play.vehicles[0].v.id();
        // it settles in the water
        for _ in 0..40 {
            sim.step(&[Command::Player(feet, [0.0, 45.0], false)], &mut ev);
        }
        let mut ev = Vec::new();
        sim.step(&[Command::Interact { id }], &mut ev);
        assert_eq!(sim.riding, Some(id));
        assert!(ev
            .iter()
            .any(|e| matches!(e, Event::Mount(Some(i)) if *i == id)));
        let start = sim.play.vehicles[0].v.e().pos;
        // paddle south
        for _ in 0..25 {
            sim.step(
                &[
                    Command::Player(feet, [0.0, 0.0], false),
                    Command::Input {
                        strafe: 0.0,
                        forward: 1.0,
                        jump: false,
                        sneak: false,
                    },
                ],
                &mut Vec::new(),
            );
        }
        let now = sim.play.vehicles[0].v.e().pos;
        assert!(
            now[2] > start[2] + 1.0,
            "moved south: {:?} -> {:?}",
            start,
            now
        );
        // the rider sits on the boat (updateRiderPosition)
        let r = sim.play.vehicles[0].v.rider_position(-0.5f32 as f64);
        assert!((sim.play.player.0[2] - r[2]).abs() < 1.0);
        // sneak: off
        let mut ev = Vec::new();
        sim.step(
            &[Command::Input {
                strafe: 0.0,
                forward: 0.0,
                jump: false,
                sneak: true,
            }],
            &mut ev,
        );
        assert_eq!(sim.riding, None);
        assert!(ev.iter().any(|e| matches!(e, Event::Mount(None))));
        assert!(ev.iter().any(|e| matches!(e, Event::Teleport(_))));
        // five fist hits in survival
        sim.play.mode = GameMode::Survival;
        for _ in 0..5 {
            sim.step(
                &[Command::Attack {
                    id,
                    damage: 1.0,
                    crit: false,
                    knockback: 0,
                }],
                &mut Vec::new(),
            );
        }
        assert!(sim.play.vehicles.is_empty(), "broken");
        assert!(sim.play.items.iter().any(|i| i.stack.id == 333));
    }

    /// EntityEnderPearl moves the thrower and costs 5 health; a splash
    /// potion of harming thrown straight down hurts the thrower.
    #[test]
    fn pearls_and_splash_potions() {
        let (mut sim, x, y, z) = redstone_sim();
        sim.play.mode = GameMode::Survival;
        let feet = [x as f64 + 0.5, y as f64, z as f64 + 0.5];
        for _ in 0..2 {
            sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut Vec::new());
        }
        sim.play.inv.selected = 0;
        sim.play.inv.main[0] = Some(crate::item::Stack::new(368, 0, 1));
        sim.play.pitch = 0.0;
        let mut moved = None;
        let mut ev = Vec::new();
        sim.step(&[Command::UseAir], &mut ev);
        for e in ev {
            if let Event::Teleport(p) = e {
                moved = Some(p);
            }
        }
        for _ in 0..60 {
            if moved.is_some() {
                break;
            }
            let mut ev = Vec::new();
            sim.step(&[], &mut ev);
            for e in ev {
                if let Event::Teleport(p) = e {
                    moved = Some(p);
                }
            }
            if moved.is_some() {
                break;
            }
        }
        let p = moved.expect("the pearl landed");
        assert!((p[2] - feet[2]).abs() > 1.0, "moved along the throw");
        assert_eq!(sim.splayer.living.health, 15.0);

        // Splash Potion of Harming (16396) at the feet
        sim.splayer.living.hurt_resistant_time = 0;
        let before = sim.splayer.living.health;
        sim.play.inv.main[0] = Some(crate::item::Stack::new(373, 16396, 1));
        sim.play.pitch = 90.0;
        sim.step(
            &[Command::Player(p, [0.0, 90.0], false), Command::UseAir],
            &mut Vec::new(),
        );
        for _ in 0..10 {
            sim.step(&[Command::Player(p, [0.0, 90.0], false)], &mut Vec::new());
        }
        assert!(sim.splayer.living.health < before, "harmed by the splash");
    }

    /// TileEntityPiston.launchWithSlimeBlock moves an item lying in front.
    #[test]
    fn piston_pushes_items() {
        let (mut sim, x, y, z) = redstone_sim();
        let w = &mut sim.world;
        w.set_block(x, y, z, block::PISTON, 5, 3);
        w.set_block(x - 1, y, z, block::STONE, 0, 3);
        w.set_block(x - 1, y + 1, z, block::LEVER, 5, 3);
        let mut r = JavaRandom::new(1);
        let mut it = crate::entity::item::ItemEntity::new(
            999,
            crate::item::Stack::new(crate::item::STICK, 0, 1),
            [x as f64 + 1.5, y as f64 + 0.125, z as f64 + 0.5],
        );
        it.e.motion = [0.0; 3];
        it.pickup_delay = 100;
        sim.play.items.push(it);
        let before = sim.play.items[0].e.pos[0];
        sim.world.toggle_lever(x - 1, y + 1, z);
        steps(&mut sim, 3);
        let after = sim.play.items[0].e.pos[0];
        assert!(after > before + 0.4, "pushed from {before} to {after}");
    }

    #[test]
    fn powered_tnt_primes_and_explodes() {
        let (mut sim, x, y, z) = redstone_sim();
        sim.world.set_block(x + 5, y, z + 5, block::TNT, 0, 3);
        sim.world
            .set_block(x + 6, y, z + 5, block::REDSTONE_BLOCK, 0, 3);
        assert_eq!(sim.world.block(x + 5, y, z + 5), block::AIR);
        steps(&mut sim, 1);
        assert_eq!(sim.mobs.tnt.len(), 1);
        steps(&mut sim, 85);
        assert!(sim.mobs.tnt.is_empty());
        assert_eq!(
            sim.world.block(x + 5, y - 1, z + 5),
            block::AIR,
            "the floor is blown away"
        );
    }

    #[test]
    fn pressure_plate_opens_door() {
        let (mut sim, x, y, z) = redstone_sim();
        let w = &mut sim.world;
        w.set_block(x, y, z, block::WOODEN_DOOR, 0, 2);
        w.set_block(x, y + 1, z, block::WOODEN_DOOR, 8, 2);
        w.set_block(x + 1, y, z, block::STONE_PRESSURE_PLATE, 0, 3);
        let feet = [x as f64 + 1.5, y as f64, z as f64 + 0.5];
        let mut ev = Vec::new();
        sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut ev);
        sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut ev);
        assert_eq!(sim.world.meta(x + 1, y, z), 1);
        assert_eq!(sim.world.meta(x, y, z) & 4, 4, "door opened");
    }

    /// EntityCreeper.interact: flint and steel lights the fuse, which then
    /// runs for `fuseTime` (30) ticks whatever the player does.
    #[test]
    fn flint_and_steel_ignites_a_creeper() {
        use crate::entity::mob::Kind;
        let (mut sim, x, y, z) = redstone_sim();
        let mut r = JavaRandom::new(5);
        let id = sim.mobs.spawn(
            Kind::Creeper,
            [x as f64 + 3.5, y as f64, z as f64 + 3.5],
            0.0,
            &mut r,
        );
        sim.play.inv.main[sim.play.inv.selected] = Some(crate::item::Stack::new(259, 0, 1));
        let mut ev = Vec::new();
        sim.step(&[Command::Interact { id }], &mut ev);
        let lit = ev.iter().any(|e| match e {
            Event::Sounds(l) => l.iter().any(|s| s.0 == "fire.ignite"),
            _ => false,
        });
        assert!(lit, "fire.ignite played");
        assert!(sim.mobs.list.iter().any(|m| m.id == id && m.ignited));
        for _ in 0..28 {
            sim.step(&[], &mut ev);
        }
        assert!(
            sim.mobs.list.iter().any(|m| m.id == id && !m.dead),
            "still swelling"
        );
        let mut boom = false;
        for _ in 0..4 {
            ev.clear();
            sim.step(&[], &mut ev);
            boom |= ev.iter().any(|e| matches!(e, Event::Explosions(_)));
        }
        assert!(boom, "exploded after the fuse");
    }

    #[test]
    fn big_slime_splits_and_bone_tames_wolf() {
        use crate::entity::mob::Kind;
        let (mut sim, x, y, z) = redstone_sim();
        let mut r = JavaRandom::new(5);
        let id = sim.mobs.spawn(
            Kind::Slime,
            [x as f64 + 3.5, y as f64, z as f64 + 3.5],
            0.0,
            &mut r,
        );
        let m = sim.mobs.list.iter_mut().find(|m| m.id == id).unwrap();
        m.set_slime_size(4);
        m.health = 0.5;
        let mut out = crate::entity::mob::Outcome::default();
        sim.mobs.attack(
            id,
            5.0,
            false,
            [x as f64, y as f64, z as f64],
            0.0,
            0,
            &mut out,
        );
        steps(&mut sim, 1);
        let small: Vec<i32> = sim
            .mobs
            .list
            .iter()
            .filter(|m| m.kind == Kind::Slime && m.health > 0.0)
            .map(|m| m.variant)
            .collect();
        assert!((2..=4).contains(&small.len()), "{small:?}");
        assert!(small.iter().all(|&v| v == 2));

        let wid = sim.mobs.spawn(
            Kind::Wolf,
            [x as f64 + 6.5, y as f64, z as f64 + 6.5],
            0.0,
            &mut r,
        );
        let mut tamed = false;
        for _ in 0..30 {
            let mut out = crate::entity::mob::Outcome::default();
            let bone = Some(crate::item::Stack::new(352, 0, 64));
            sim.mobs
                .interact(wid, bone, [x as f64, y as f64, z as f64], &mut out);
            let w = sim.mobs.list.iter().find(|m| m.id == wid).unwrap();
            if w.tamed {
                tamed = true;
                assert!(w.sitting);
                assert_eq!(w.health, 20.0);
                break;
            }
        }
        assert!(tamed, "a bone tames one time in three");
    }

    #[test]
    fn nether_portal_round_trip() {
        let (mut sim, x, y, z) = redstone_sim();
        // a 4 x 5 obsidian frame along x
        let w = &mut sim.world;
        for i in 0..4 {
            w.set_block(x + i, y, z, block::OBSIDIAN, 0, 3);
            w.set_block(x + i, y + 4, z, block::OBSIDIAN, 0, 3);
        }
        for j in 1..4 {
            w.set_block(x, y + j, z, block::OBSIDIAN, 0, 3);
            w.set_block(x + 3, y + j, z, block::OBSIDIAN, 0, 3);
        }
        w.set_block(x + 1, y + 1, z, block::FIRE, 0, 3);
        for i in 1..3 {
            for j in 1..4 {
                assert_eq!(w.block(x + i, y + j, z), block::PORTAL, "lit at {i},{j}");
            }
        }
        let inside = [x as f64 + 1.5, (y + 1) as f64, z as f64 + 0.5];
        let mut ev = Vec::new();
        sim.step(&[Command::Player(inside, [0.0, 0.0], false)], &mut ev);
        sim.step(&[Command::Player(inside, [0.0, 0.0], false)], &mut ev);
        assert_eq!(sim.dim, -1, "creative players travel at once");
        let arrived = ev.iter().find_map(|e| match e {
            Event::Dimension { dim: -1, pos, .. } => Some(*pos),
            _ => None,
        });
        let pos = arrived.expect("dimension event");
        assert!((pos[0] - inside[0] / 8.0).abs() < 20.0 && (pos[2] - inside[2] / 8.0).abs() < 20.0);
        assert_eq!(
            crate::dimension::portal_contact(&mut sim.world, pos),
            Some(block::PORTAL),
            "standing in the new portal"
        );
        // stay put: the cooldown keeps us here
        ev.clear();
        sim.step(&[Command::DimensionAck], &mut ev);
        for _ in 0..5 {
            sim.step(&[Command::Player(pos, [0.0, 0.0], false)], &mut ev);
        }
        assert_eq!(sim.dim, -1);
        // step out, wait, step back in
        let out = [pos[0] + 3.0, pos[1], pos[2] + 3.0];
        for _ in 0..15 {
            sim.step(&[Command::Player(out, [0.0, 0.0], false)], &mut ev);
        }
        sim.step(&[Command::Player(pos, [0.0, 0.0], false)], &mut ev);
        sim.step(&[Command::Player(pos, [0.0, 0.0], false)], &mut ev);
        assert_eq!(sim.dim, 0, "back to the overworld");
        let (ppos, _) = sim.play.player;
        assert!(
            (ppos[0] - inside[0]).abs() < 3.0 && (ppos[2] - inside[2]).abs() < 3.0,
            "{ppos:?}"
        );
    }

    #[test]
    fn end_portal_and_dragon_exit() {
        let (mut sim, x, y, z) = redstone_sim();
        // a ring of twelve frames around a 3x3 hole (frames face inwards)
        let (cx, cz) = (x + 3, z + 3);
        let w = &mut sim.world;
        for i in -1..=1 {
            w.set_block(cx + i, y, cz - 2, block::END_PORTAL_FRAME, 0 | 4, 3); // facing +z
            w.set_block(cx + i, y, cz + 2, block::END_PORTAL_FRAME, 2 | 4, 3); // facing -z
            w.set_block(cx - 2, y, cz + i, block::END_PORTAL_FRAME, 3 | 4, 3); // facing +x
            w.set_block(cx + 2, y, cz + i, block::END_PORTAL_FRAME, 1, 3); // facing -x
        }
        w.set_meta(cx + 2, y, cz - 1, 1 | 4, 3);
        w.set_meta(cx + 2, y, cz + 1, 1 | 4, 3);
        let mut ev = Vec::new();
        let eye = crate::item::Stack::new(381, 0, 1);
        sim.step(
            &[
                Command::Give(eye),
                Command::Select(0),
                Command::Use {
                    target: [cx + 2, y, cz],
                    face: [0, 1, 0],
                    sneaking: false,
                },
            ],
            &mut ev,
        );
        for i in -1..=1 {
            for j in -1..=1 {
                assert_eq!(
                    sim.world.block(cx + i, y, cz + j),
                    block::END_PORTAL,
                    "{i},{j}"
                );
            }
        }
        // jump in
        let feet = [cx as f64 + 0.5, y as f64, cz as f64 + 0.5];
        sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut ev);
        sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut ev);
        assert_eq!(sim.dim, 1);
        assert_eq!(
            sim.world.block(100, 48, 0),
            block::OBSIDIAN,
            "the landing platform"
        );
        // a dead dragon leaves the way home
        let mut r = JavaRandom::new(1);
        let id = sim.mobs.spawn(
            crate::entity::mob::Kind::EnderDragon,
            [0.5, 80.0, 0.5],
            0.0,
            &mut r,
        );
        sim.mobs
            .list
            .iter_mut()
            .find(|m| m.id == id)
            .unwrap()
            .health = 0.0;
        ev.clear();
        sim.step(&[Command::DimensionAck], &mut ev);
        for _ in 0..210 {
            sim.step(&[], &mut ev);
        }
        assert!(!sim.mobs.list.iter().any(|m| m.id == id));
        let mut found = false;
        for px in -6..=6 {
            for pz in -6..=6 {
                if sim.world.block(px, 64, pz) == block::END_PORTAL {
                    found = true;
                }
            }
        }
        assert!(found, "exit portal at y 64");
        assert!((0..256).any(|py| sim.world.block(0, py, 0) == block::DRAGON_EGG));
    }

    #[test]
    fn anvil_world_round_trip() {
        use crate::world::anvil::{compound, Nbt};
        let dir = std::env::temp_dir().join(format!("minerust-anvilsim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // a fresh vanilla level.dat
        let root = compound(vec![(
            "Data",
            compound(vec![
                ("RandomSeed", Nbt::Long(77)),
                ("Time", Nbt::Long(1000)),
                ("DayTime", Nbt::Long(1000)),
                ("SpawnX", Nbt::Int(0)),
                ("SpawnY", Nbt::Int(80)),
                ("SpawnZ", Nbt::Int(0)),
                ("GameType", Nbt::Int(1)),
            ]),
        )]);
        crate::world::anvil::write_level_dat(&dir, &root).unwrap();
        let cfg = SimConfig {
            seed: 0,
            view: 2,
            save_dir: Some(dir.clone()),
            start_time: 0,
            freeze_time: true,
            mode: GameMode::Creative,
        };
        let (chest, mob_pos);
        {
            let (mut sim, start) = Sim::new(&cfg);
            assert_eq!(sim.world.seed(), 77);
            assert!(sim.world.anvil);
            let mut ev = Vec::new();
            for _ in 0..30 {
                sim.step(&[], &mut ev);
            }
            let (x, z) = (start.pos[0].floor() as i32, start.pos[2].floor() as i32);
            chest = [x + 1, 100, z + 1];
            sim.world
                .set_block(chest[0], chest[1], chest[2], block::CHEST, 2, 3);
            let mut slots = [None; 27];
            slots[4] = Some(crate::item::Stack::new(264, 0, 3));
            sim.play
                .tiles
                .insert(chest, crate::play::Tile::Chest { slots });
            let mut r = JavaRandom::new(3);
            mob_pos = [x as f64 + 0.5, 101.0, z as f64 + 3.5];
            let id = sim
                .mobs
                .spawn(crate::entity::mob::Kind::Cow, mob_pos, 0.0, &mut r);
            sim.mobs
                .list
                .iter_mut()
                .find(|m| m.id == id)
                .unwrap()
                .persistent = true;
            sim.save().unwrap();
        }
        assert!(dir.join("region").read_dir().unwrap().count() > 0);
        assert!(
            !dir.join("level.txt").exists(),
            "an Anvil world stays Anvil"
        );
        let (mut sim, _) = Sim::new(&cfg);
        let mut ev = Vec::new();
        for _ in 0..30 {
            sim.step(&[], &mut ev);
        }
        assert_eq!(sim.world.block(chest[0], chest[1], chest[2]), block::CHEST);
        match sim.play.tiles.get(&chest) {
            Some(crate::play::Tile::Chest { slots }) => {
                assert_eq!(slots[4].map(|s| (s.id, s.count)), Some((264, 3)))
            }
            other => panic!("chest lost: {other:?}"),
        }
        assert!(sim.mobs.list.iter().any(
            |m| m.kind == crate::entity::mob::Kind::Cow && (m.pos[0] - mob_pos[0]).abs() < 2.0
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_to_anvil() {
        let base = std::env::temp_dir().join(format!("minerust-export-{}", std::process::id()));
        let (src, dst) = (base.join("own"), base.join("vanilla"));
        let _ = std::fs::remove_dir_all(&base);
        let cfg = SimConfig {
            seed: 99,
            view: 2,
            save_dir: Some(src.clone()),
            start_time: 0,
            freeze_time: true,
            mode: GameMode::Creative,
        };
        let marker;
        {
            let (mut sim, start) = Sim::new(&cfg);
            let mut ev = Vec::new();
            for _ in 0..20 {
                sim.step(&[], &mut ev);
            }
            marker = [
                start.pos[0].floor() as i32,
                120,
                start.pos[2].floor() as i32,
            ];
            sim.world
                .set_block(marker[0], marker[1], marker[2], block::GOLD_BLOCK, 0, 3);
            sim.step(&[], &mut ev);
            sim.save().unwrap();
            // unloading writes the rest to disk
            let keys: Vec<(i32, i32)> = sim.world.chunks.keys().copied().collect();
            for k in keys {
                sim.world.unload_chunk(k.0, k.1);
            }
        }
        let (mut sim, _) = Sim::new(&cfg);
        let n = sim.export_anvil(&dst).unwrap();
        assert!(n > 0);
        assert!(dst.join("level.dat").exists());
        let cfg2 = SimConfig {
            save_dir: Some(dst.clone()),
            ..cfg
        };
        let (mut sim, _) = Sim::new(&cfg2);
        assert!(sim.world.anvil);
        assert_eq!(sim.world.seed(), 99);
        sim.world.load_chunk(marker[0] >> 4, marker[2] >> 4);
        assert_eq!(
            sim.world.block(marker[0], marker[1], marker[2]),
            block::GOLD_BLOCK
        );
        let _ = std::fs::remove_dir_all(&base);
    }
}
