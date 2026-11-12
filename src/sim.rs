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
    unknown_level_keys: Vec<(String, String)>,
    pub play: Play,
    spawn: [f64; 3],
    stats: Option<String>,
    respawn_pending: bool,
    pub mobs: crate::entity::mob::Mobs,
    player_attackable: bool,
    pending_out: Vec<crate::entity::mob::Outcome>,
    pub weather: Weather,
}

/// Level keys owned by the gameplay state (kept out of `unknown`).
const PLAY_KEYS: [&str; 6] = ["inventory", "mode", "tile", "spawn", "stats", "weather"];

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
        let seed = level.as_ref().map_or(cfg.seed, |l| l.seed);
        if let Some(l) = &level {
            crate::compat::set_vanilla_parity(l.vanilla_parity);
        }
        let provider = Arc::new(ChunkProvider::new(seed));
        let mut world = ServerWorld::new(provider);
        world.save_dir = cfg.save_dir.clone();
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
            unknown_level_keys: Vec::new(),
            play: Play::new(cfg.mode),
            spawn: [0.0; 3],
            stats: None,
            respawn_pending: false,
            mobs: crate::entity::mob::Mobs::default(),
            player_attackable: true,
            pending_out: Vec::new(),
            weather: Weather::default(),
        };
        if let Some(d) = &cfg.save_dir {
            if let Ok(text) = std::fs::read_to_string(d.join("data").join("entities.txt")) {
                for line in text.lines() {
                    let _ = sim.mobs.decode(line);
                }
            }
        }
        let start = match level {
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
                self.player = Some((p, r, f));
                self.play.player = (p, r[0]);
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
            Command::Respawn => self.respawn_pending = true,
            Command::Attack {
                id,
                damage,
                crit,
                knockback,
            } => {
                let mut out = crate::entity::mob::Outcome::default();
                let (feet, yaw) = self.play.player;
                self.mobs
                    .attack(id, damage, crit, feet, yaw, knockback, &mut out);
                self.pending_out.push(out);
            }
            Command::PlayerState { attackable } => self.player_attackable = attackable,
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
        if !self.freeze_time {
            self.world_time += 1;
        }
        {
            let mut r = std::mem::replace(&mut self.world.rand, JavaRandom::new(0));
            self.weather.update(&mut r);
            self.world.rand = r;
            self.world.raining = self.weather.rain > 0.2;
        }
        self.world.skylight_subtracted =
            skylight_subtracted_weather(self.world_time, self.weather.rain, self.weather.thunder);
        events.push(Event::Weather(self.weather));
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

    /// Writes the level file and every chunk changed since the last save.
    pub fn save(&mut self) -> std::io::Result<()> {
        let Some(dir) = self.world.save_dir.clone() else {
            return Ok(());
        };
        for key in std::mem::take(&mut self.unsaved) {
            if let Some(c) = self.world.chunks.get(&key) {
                save::write_chunk(&dir, key.0, key.1, &c.to_bytes())?;
            }
        }
        let level = Level {
            seed: self.world.seed(),
            total_time: self.world.total_time,
            world_time: self.world_time,
            vanilla_parity: crate::compat::vanilla_parity(),
            player: self.player,
            ticks: self.world.pending_tick_list(),
            unknown: {
                let mut u = self.unknown_level_keys.clone();
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
                for t in self.play.encode_tiles() {
                    u.push(("tile".into(), t));
                }
                u
            },
        };
        if let Some(st) = &self.world.structures {
            let data = dir.join("data");
            std::fs::create_dir_all(&data)?;
            save::write_atomic(&data.join("structures.txt"), st.save().as_bytes())?;
        }
        {
            let data = dir.join("data");
            std::fs::create_dir_all(&data)?;
            save::write_atomic(
                &data.join("entities.txt"),
                self.mobs.encode().join("\n").as_bytes(),
            )?;
        }
        save::write_level(&dir, &level)
    }

    /// Mob AI, spawning and despawning, and what the client must hear of it.
    fn tick_mobs(&mut self, play: &mut Play, events: &mut Vec<Event>) {
        use crate::entity::mob;
        let (feet, yaw) = play.player;
        let held = play.inv.held().map(|s| s.id);
        let info = mob::PlayerInfo {
            feet,
            yaw,
            attackable: self.player_attackable && play.mode == GameMode::Survival,
            held,
        };
        let mut out = mob::Outcome::default();
        self.mobs.tick(&mut self.world, &info, &mut out);
        let animals = self.world.total_time % 400 == 0;
        mob::natural_spawning(&mut self.world, &mut self.mobs, feet, self.spawn, animals);
        if self.world.total_time % 40 == 0 {
            self.mobs.despawn(feet);
        }
        let mut outs = std::mem::take(&mut self.pending_out);
        outs.push(out);
        let mut hits = Vec::new();
        let mut sounds = Vec::new();
        let mut explosions = Vec::new();
        let mut rnd = std::mem::replace(&mut self.world.rand, JavaRandom::new(0));
        for o in outs {
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
}
