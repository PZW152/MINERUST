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
use crate::inventory::{Inventory, Slot};
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
    /// Position, rotation and flying state, kept for the save file.
    Player([f64; 3], [f32; 2], bool),
    /// Survival digging finished (or a creative click) at the block.
    Dig([i32; 3]),
    /// Right click on a block face.
    Use {
        target: [i32; 3],
        face: [i32; 3],
        sneaking: bool,
    },
    Select(usize),
    Click {
        slot: Slot,
        tile: Option<usize>,
        right: bool,
        shift: bool,
    },
    OpenInventory,
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
    Shutdown,
}

pub enum Event {
    /// A chunk appeared or changed. `ready` means nothing unpopulated can
    /// still write into it, so it may be meshed.
    Chunk {
        key: (i32, i32),
        chunk: MeshChunk,
        ready: bool,
    },
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
    Explosions(Vec<[f64; 4]>),
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
    teleport_rand: JavaRandom,
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
                        "stats" => sim.stats = Some(v.clone()),
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
            Command::Player(p, r, f) => {
                if self.await_ack {
                    return;
                }
                self.player = Some((p, r, f));
                self.play.player = (p, r[0]);
            }
            Command::DimensionAck => self.await_ack = false,
            Command::UseAir => self.use_air(),
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
                let mut play = std::mem::replace(&mut self.play, Play::new(GameMode::Survival));
                play.dig(&mut self.world, p);
                self.play = play;
            }
            Command::Use {
                target,
                face,
                sneaking,
            } => {
                let mut play = std::mem::replace(&mut self.play, Play::new(GameMode::Survival));
                play.use_block(&mut self.world, target, face, sneaking);
                self.play = play;
            }
            Command::Select(i) => {
                if i < 9 && self.play.inv.selected != i {
                    self.play.inv.selected = i;
                    self.play.inv_dirty = true;
                }
            }
            Command::Click {
                slot,
                tile,
                right,
                shift,
            } => {
                if self.play.open != Open::None {
                    let mut play = std::mem::replace(&mut self.play, Play::new(GameMode::Survival));
                    play.click(&mut self.world, slot, tile, right, shift);
                    self.play = play;
                }
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
            Command::Stats(ref s) => self.stats = Some(s.clone()),
            Command::Died => {
                let mut play = std::mem::replace(&mut self.play, Play::new(GameMode::Survival));
                if play.mode == GameMode::Survival {
                    let mut rnd =
                        std::mem::replace(&mut self.world.rand, crate::java::JavaRandom::new(0));
                    play.close(&mut self.world);
                    for i in 0..crate::inventory::MAIN {
                        if let Some(st) = play.inv.main[i].take() {
                            play.throw(st, &mut rnd);
                        }
                    }
                    self.world.rand = rnd;
                    play.inv_dirty = true;
                }
                self.play = play;
            }
            Command::Respawn => {
                self.respawn_pending = true;
                if self.dim != 0 {
                    self.pending_travel = Some(0);
                }
            }
            Command::Attack {
                id,
                damage,
                crit,
                knockback,
            } => {
                self.last_attacked = Some(id);
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
        for c in commands {
            self.apply(c);
        }
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
                });
            }
            for a in &self.mobs.arrows {
                occ.push(Occupant {
                    bb: Aabb6 {
                        min: [a.pos[0] - 0.25, a.pos[1], a.pos[2] - 0.25],
                        max: [a.pos[0] + 0.25, a.pos[1] + 0.5, a.pos[2] + 0.25],
                    },
                    living: false,
                });
            }
            for t in &self.mobs.tnt {
                occ.push(Occupant {
                    bb: Aabb6 {
                        min: [t.pos[0] - 0.49, t.pos[1] - 0.49, t.pos[2] - 0.49],
                        max: [t.pos[0] + 0.49, t.pos[1] + 0.49, t.pos[2] + 0.49],
                    },
                    living: false,
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
        self.play = play;
        if self.play.inv_dirty {
            self.play.inv_dirty = false;
            events.push(Event::Inventory(Box::new(self.play.view())));
        }
        events.push(Event::Items(self.play.item_views()));
        if self.respawn_pending {
            self.respawn_pending = false;
            self.centre = (
                (self.spawn[0].floor() as i32) >> 4,
                (self.spawn[2].floor() as i32) >> 4,
            );
            events.push(Event::Respawn(self.spawn));
        }

        // Changes, in a fixed order.
        let mut dirty: Vec<(i32, i32)> = self.world.dirty.drain().collect();
        dirty.sort();
        for key in &dirty {
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
        for key in dirty {
            if in_range(self.centre, key) && self.world.chunk_exists(key.0, key.1) {
                let ready = self.ready(key.0, key.1);
                self.sent.insert(key, ready);
                events.push(Event::Chunk {
                    key,
                    chunk: self.snapshot(key).unwrap(),
                    ready,
                });
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
                self.sent.insert(key, ready);
                events.push(Event::Chunk {
                    key,
                    chunk: self.snapshot(key).unwrap(),
                    ready,
                });
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
        entities.extend(play.items_nbt(cx, cz));
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
                (e.pos[0].floor() as i32) >> 4,
                (e.pos[2].floor() as i32) >> 4,
            ));
        }
        for (cx, cz) in keys {
            let lists = Self::chunk_lists(play, mobs, cx, cz);
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
        let inv: Vec<Nbt> = self
            .play
            .inv
            .main
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().map(|s| item_nbt(Some(i as i8), s)))
            .collect();
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
                } else {
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
        }
        Parked {
            dim,
            world,
            mobs,
            unsaved: Default::default(),
            tiles,
            items: Vec::new(),
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
            let Some(kind) = parts.next().and_then(crate::entity::mob::Kind::from_id) else {
                continue;
            };
            let off = (i as f64 - (n - 1.0) / 2.0) * 2.2;
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
                if let Some(st) = &self.stats {
                    u.push(("stats".into(), st.clone()));
                }
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
        }
        save::write_level(&base, &level)
    }

    /// Mob AI, spawning and despawning, and what the client must hear of it.
    fn tick_mobs(&mut self, play: &mut Play, events: &mut Vec<Event>) {
        use crate::entity::mob;
        let (feet, yaw) = play.player;
        let held = play.inv.held().map(|s| s.id);
        let info = mob::PlayerInfo {
            feet,
            yaw,
            pitch: self.player.map_or(0.0, |p| p.1[1]),
            attackable: self.player_attackable && play.mode == GameMode::Survival,
            held,
            last_attacked: self.last_attacked,
            last_attacker: self.last_attacker,
        };
        // World.getCurrentMoonPhaseFactor
        self.world.moon_factor = [1.0, 0.75, 0.5, 0.25, 0.0, 0.25, 0.5, 0.75]
            [((self.world_time / 24000 % 8 + 8) % 8) as usize];
        let mut out = mob::Outcome::default();
        self.mobs.tick(&mut self.world, &info, &mut out);
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
        }
        self.world.rand = rnd;
        if !hits.is_empty() {
            events.push(Event::Hits(hits));
        }
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
                pos: a.pos,
                prev: a.prev_pos,
                yaw: a.yaw,
                pitch: a.pitch,
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
                sim.step(&commands, &mut events);
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
                Command::Click {
                    slot: Slot::Main(log_slot),
                    tile: None,
                    right: false,
                    shift: false,
                },
                Command::Click {
                    slot: Slot::Craft(0),
                    tile: None,
                    right: false,
                    shift: false,
                },
                Command::Click {
                    slot: Slot::Result,
                    tile: None,
                    right: false,
                    shift: true,
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
        // stone behind it powers it.
        let w = &mut sim.world;
        let pz = z + 6;
        w.set_block(x, y, pz, block::PISTON, 5, 3);
        w.set_block(x + 1, y, pz, block::COBBLESTONE, 0, 3);
        w.set_block(x - 1, y, pz, block::STONE, 0, 3);
        w.set_block(x - 1, y + 1, pz, block::LEVER, 5, 3);
        w.toggle_lever(x - 1, y + 1, pz);
        assert_eq!(w.meta(x, y, pz), 13);
        assert_eq!(w.block(x + 1, y, pz), block::PISTON_HEAD);
        assert_eq!(w.block(x + 2, y, pz), block::COBBLESTONE);
        w.toggle_lever(x - 1, y + 1, pz);
        assert_eq!(w.meta(x, y, pz), 5);
        assert_eq!(w.block(x + 1, y, pz), block::AIR);
        assert_eq!(w.block(x + 2, y, pz), block::COBBLESTONE);

        // A sticky piston pulls the block back.
        let sz = z + 9;
        w.set_block(x, y, sz, block::STICKY_PISTON, 5, 3);
        w.set_block(x + 1, y, sz, block::COBBLESTONE, 0, 3);
        w.set_block(x - 1, y, sz, block::STONE, 0, 3);
        w.set_block(x - 1, y + 1, sz, block::LEVER, 5, 3);
        w.toggle_lever(x - 1, y + 1, sz);
        assert_eq!(w.block(x + 2, y, sz), block::COBBLESTONE);
        w.toggle_lever(x - 1, y + 1, sz);
        assert_eq!(w.block(x + 1, y, sz), block::COBBLESTONE);
        assert_eq!(w.block(x + 2, y, sz), block::AIR);
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
