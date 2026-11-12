//! Game state, tick loop, chunk scheduling and per-frame environment
//! (`Minecraft.runTick`, `EntityRenderer`).

use crate::block::{self, Material};
use crate::entity::player::{Input, Player};
use crate::entity::survival::{GameMode, Stats};
use crate::inventory::Slot;
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

struct ClientChunk {
    mc: MeshChunk,
    /// Everything that can write into it has been populated.
    ready: bool,
}

/// The client's copy of the world: the chunk snapshots the server last sent.
#[derive(Default)]
pub struct ClientWorld {
    chunks: HashMap<(i32, i32), ClientChunk>,
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
}

struct Job(Box<[MeshChunk; 9]>);

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
    ticks: u64,
    noise: BiomeNoise,
    /// `(ticks shown, position in the frame order)` per animated tile.
    anim_state: Vec<(u32, usize)>,
    /// Cloud texel the current cloud mesh was built for.
    cloud_cell: Option<(i32, i32)>,
    /// Server view of the inventory and open window.
    pub view: InvView,
    items: Vec<ItemView>,
    /// Sound output (absent when no asset directory or device was found).
    pub audio: Option<crate::audio::Audio>,
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

        let mut light_table = [0f32; 16];
        for (i, v) in light_table.iter_mut().enumerate() {
            let f1 = 1.0f32 - i as f32 / 15.0f32;
            *v = (1.0f32 - f1) / (f1 * 3.0f32 + 1.0f32);
        }

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
            },
            items: Vec::new(),
            audio: None,
            weather: Default::default(),
            rain_sound: 0,
            particle_rand: crate::java::JavaRandom::new(77),
            particles: Default::default(),
            dying: Default::default(),
            next_step: 1.0,
            mobs: Vec::new(),
            arrows: Vec::new(),
            tnt: Vec::new(),
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

    /// True once every chunk in view has an up-to-date mesh.
    /// `--open`: start with a screen open (automated captures).
    pub fn apply_start_screen(&mut self) {
        if let Ok(s) = std::env::var("MINERUST_OPEN") {
            match s.as_str() {
                "inventory" => {
                    let _ = self.sim.commands.send(Command::OpenInventory);
                    self.screen = Screen::Window;
                }
                "pause" => self.screen = Screen::Paused,
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
    }

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
                    self.world
                        .chunks
                        .insert(key, ClientChunk { mc: chunk, ready });
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
                Event::Inventory(v) => {
                    let was_open = self.view.open != Open::None;
                    self.view = *v;
                    if self.view.open != Open::None && !was_open && self.screen == Screen::Playing {
                        self.screen = Screen::Window;
                    }
                    if self.view.open == Open::None && self.screen == Screen::Window {
                        self.screen = Screen::Playing;
                    }
                }
                Event::Items(v) => self.items = v,
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
                    self.mobs = m;
                    self.arrows = a;
                }
                Event::Hits(hits) => self.apply_hits(hits),
                Event::Sounds(list) => {
                    for (name, pos, vol, pitch) in list {
                        self.sound(name, Some(pos), vol, pitch);
                    }
                }
                Event::Tnt(list) => self.tnt = list,
                Event::Particles(list) => {
                    let slime = self
                        .atlas
                        .items
                        .get(&(341, 0))
                        .map(|&t| self.atlas.rects[t as usize]);
                    for (kind, p, m) in list {
                        match kind {
                            "portal" => self.particles.portal(p, m),
                            "heart" => self.particles.heart(p, m),
                            "smoke" => self.particles.smoke(p, m, false),
                            "slime" => {
                                if let Some(r) = slime {
                                    self.particles.item_break(p, r);
                                }
                            }
                            _ => {}
                        }
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
                        let c = [e[0], e[1], e[2]];
                        if e[3] >= 2.0 {
                            self.particles.huge_explosion(c);
                        } else {
                            self.particles.large_explode(c, 1.0);
                        }
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
        sim::celestial_angle(self.world_time, partial)
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

    fn tick(&mut self) {
        self.ticks += 1;
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
        if self.world.loaded(pcx << 4, pcz << 4) && !self.stats.dead {
            let input = if self.screen == Screen::Playing {
                self.input
            } else {
                Input::default()
            };
            self.player.tick(&self.world, &input);
            // Entity.moveEntity: a step sound per block walked.
            if self.player.on_ground
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
                    self.block_sound(id, [x, y, z], "step", 0.15, 1.0);
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
        let eye_mat = self.player.eye_material(&self.world, self.player.pos);
        let lava = self.player.in_lava_now;
        let jumped = self.player.jumped;
        self.stats
            .tick(&self.player, moved, jumped, eye_mat, lava, self.mode);
        if self.stats.dead && !self.death_sent {
            self.death_sent = true;
            let _ = self.sim.commands.send(Command::Died);
            self.screen = Screen::Dead;
            self.input = Input::default();
            self.dig = None;
        }
        {
            let p = &self.player;
            let _ = self
                .sim
                .commands
                .send(Command::Player(after, [p.yaw, p.pitch], p.flying));
        }
        if self.ticks % 100 == 0 {
            let _ = self
                .sim
                .commands
                .send(Command::Stats(encode_stats(&self.stats)));
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
        let face = if id == block::GRASS {
            0
        } else if id == block::LOG || id == block::LOG2 {
            2
        } else {
            0
        };
        let tex = self
            .atlas
            .tile(id, meta, face)
            .or_else(|| self.atlas.tile(id, meta, 2));
        let tint = match id {
            block::LEAVES | block::LEAVES2 | block::VINE | block::TALLGRASS => [0.28, 0.71, 0.09],
            _ => [1.0; 3],
        };
        (tex, tint)
    }

    /// Per-tick particle spawning around the player.
    fn tick_particles(&mut self) {
        self.particles.tick(&self.world);
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
                let (tex, tint) = self.debris([x, y, z]);
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

    fn sound(&mut self, name: &str, pos: Option<[f64; 3]>, vol: f32, pitch: f32) {
        if let Some(a) = self.audio.as_mut() {
            a.play(name, pos, vol, pitch);
        }
    }

    /// `Block.stepSound` break sound at a block.
    fn block_sound(&mut self, id: u8, pos: [i32; 3], kind: &str, vol: f32, pitch: f32) {
        let (name, p) = crate::audio::step_sound(id);
        let ev = if kind == "dig" && id == crate::block::GLASS
            || id == crate::block::GLASS_PANE
            || id == crate::block::ICE
        {
            if kind == "dig" {
                "random.glass".to_string()
            } else {
                format!("{kind}.stone")
            }
        } else {
            format!("{kind}.{name}")
        };
        let c = [
            pos[0] as f64 + 0.5,
            pos[1] as f64 + 0.5,
            pos[2] as f64 + 0.5,
        ];
        self.sound(&ev, Some(c), vol, pitch * p);
    }

    /// Damage, knockback and experience from mobs.
    fn apply_hits(&mut self, hits: Vec<crate::entity::mob::Hit>) {
        use crate::entity::mob::Hit;
        use crate::entity::survival::Damage;
        for h in hits {
            let (cause, amount, knock) = match h {
                Hit::Melee {
                    kind,
                    amount,
                    knock,
                    poison,
                } => {
                    if poison > 0 {
                        self.stats.poison = self.stats.poison.max(poison);
                    }
                    (Damage::Mob(kind), amount, knock)
                }
                Hit::Arrow { amount, knock } => (Damage::Arrow, amount, knock),
                Hit::Explosion { amount, knock } => (Damage::Explosion, amount, knock),
                Hit::Xp(n) => {
                    self.stats.add_xp(n);
                    continue;
                }
            };
            if self.stats.damage(cause, amount, self.mode) {
                self.sound("game.player.hurt", None, 1.0, 1.0);
                self.player.motion[0] = self.player.motion[0] / 2.0 + knock[0];
                self.player.motion[1] = (self.player.motion[1] / 2.0 + knock[1]).min(0.4);
                self.player.motion[2] = self.player.motion[2] / 2.0 + knock[2];
            }
        }
    }

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
        let crit =
            self.stats.fall_distance > 0.0 && !self.player.on_ground && !self.player.in_water;
        let knockback = if self.player.sprinting { 1 } else { 0 };
        if knockback > 0 {
            self.player.motion[0] *= 0.6;
            self.player.motion[2] *= 0.6;
            self.player.sprinting = false;
        }
        self.stats.add_exhaustion(0.3);
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
            self.block_sound(id, pos, "dig", 1.0, 0.8);
            let (dt, dti) = self.debris(pos);
            self.particles.destroy(pos, dt, dti);
            let _ = self.sim.commands.send(Command::Dig(pos));
            return;
        }
        if self.ticks % 4 == 0 {
            self.block_sound(id, pos, "step", 0.25, 0.5);
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
                    self.block_sound(id, pos, "dig", 1.0, 0.8);
                    let (dt, dti) = self.debris(pos);
                    self.particles.destroy(pos, dt, dti);
                    let _ = self.sim.commands.send(Command::Dig(pos));
                    self.stats.add_exhaustion(0.025);
                    self.dig = None;
                    self.hit_delay = 5;
                } else {
                    self.dig = Some(Digging { pos, damage });
                }
            }
            _ => {
                if step >= 1.0 {
                    self.block_sound(id, pos, "dig", 1.0, 0.8);
                    let (dt, dti) = self.debris(pos);
                    self.particles.destroy(pos, dt, dti);
                    let _ = self.sim.commands.send(Command::Dig(pos));
                    self.stats.add_exhaustion(0.025);
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
                        self.block_sound(id, p, "dig", 1.0, 0.8);
                    }
                }
                let _ = self.sim.commands.send(Command::Use {
                    target: b,
                    face: n,
                    sneaking: self.input.sneak,
                });
                self.swing_item();
            }
        }
        self.right_delay = 4;
    }

    fn open_window_hit(&self) -> (bool, Option<screens::SlotHit>) {
        let (w, h) = self.gui_size();
        let (left, top, xs, ys, slots) = screens::layout(&self.view, w, h);
        let (mx, my) = self.gui_mouse();
        let inside = mx >= left && my >= top && mx < left + xs && my < top + ys;
        let hit = slots.into_iter().find(|s| {
            mx >= left + s.x - 1.0
                && mx < left + s.x + 17.0
                && my >= top + s.y - 1.0
                && my < top + s.y + 17.0
        });
        (inside, hit)
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
            Screen::Window => {
                let (inside, hit) = self.open_window_hit();
                if button <= 1 {
                    match hit {
                        Some(s) => {
                            let _ = self.sim.commands.send(Command::Click {
                                slot: s.slot,
                                tile: s.tile,
                                right: button == 1,
                                shift,
                            });
                        }
                        None if !inside => {
                            let _ = self.sim.commands.send(Command::Click {
                                slot: Slot::Outside,
                                tile: None,
                                right: button == 1,
                                shift: false,
                            });
                        }
                        None => {}
                    }
                }
                Request::None
            }
            Screen::Paused | Screen::Dead => match self.menu_hit() {
                Some(MenuAction::BackToGame) => {
                    self.screen = Screen::Playing;
                    Request::Grab
                }
                Some(MenuAction::SaveAndQuit) => Request::Quit,
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
                self.screen = Screen::Paused;
                self.input = Input::default();
                Request::Release
            }
            (Screen::Paused, K::Escape) => {
                self.screen = Screen::Playing;
                Request::Grab
            }
            (Screen::Playing, K::KeyE) => {
                let _ = self.sim.commands.send(Command::OpenInventory);
                self.screen = Screen::Window;
                self.input = Input::default();
                Request::Release
            }
            (Screen::Window, K::KeyE | K::Escape) => {
                let _ = self.sim.commands.send(Command::CloseWindow);
                self.screen = Screen::Playing;
                Request::Grab
            }
            (Screen::Playing, K::KeyQ) => {
                let _ = self.sim.commands.send(Command::DropHeld { all: ctrl });
                self.swing_item();
                Request::None
            }
            (Screen::Playing, _) => {
                let digits = [
                    K::Digit1,
                    K::Digit2,
                    K::Digit3,
                    K::Digit4,
                    K::Digit5,
                    K::Digit6,
                    K::Digit7,
                    K::Digit8,
                    K::Digit9,
                ];
                if let Some(i) = digits.iter().position(|d| *d == code) {
                    let _ = self.sim.commands.send(Command::Select(i));
                    self.view.inv.selected = i;
                }
                Request::None
            }
            _ => Request::None,
        }
    }

    pub fn scroll(&mut self, up: bool) {
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
            r = r.min(1.0);
            g = g.min(1.0);
            b = b.min(1.0);
            // Gamma setting 0.0 ("Moody", the 1.7.10 default).
            let gamma = 0.0f32;
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
        let mut fog = [
            0.7529412f32 * (day * 0.94f32 + 0.06f32),
            0.84705883f32 * (day * 0.94f32 + 0.06f32),
            1.0f32 * (day * 0.91f32 + 0.09f32),
        ];
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
            _ => [far * 0.75f32, far, 0.0, 0.0],
        };

        // --- Camera (EntityRenderer.setupCameraTransform) ---
        let mut fov =
            70.0f32 * (self.fov_hand_prev + (self.fov_hand - self.fov_hand_prev) * partial);
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
        view = render::mul(&view, &render::translate(0.0, 0.0, -0.1));
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
            f * f * 0.5f32 * (1.0 - rain)
        };
        let sunrise = {
            let f3 = math::cos(angle * math::PI * 2.0f32);
            if (-0.4f32..=0.4f32).contains(&f3) {
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
        let mut g = Globals {
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
            sky_color: [sky[0], sky[1], sky[2], 1.0],
            fog_params,
            sun_dir: [-theta.sin(), theta.cos(), 0.0, (eye[1] - 63.0) as f32],
            sky_params: [far * 0.8f32, 1.0 - rain, 0.0, 0.0],
            sun_rect: self.atlas.rects[self.atlas.sun as usize],
            moon_rect: self.atlas.rects[self.atlas.moon[moon_phase] as usize],
            void_color: [
                sky[0] * 0.2f32 + 0.04f32,
                sky[1] * 0.2f32 + 0.04f32,
                sky[2] * 0.6f32 + 0.1f32,
                1.0,
            ],
            lightmap: [[0.0; 4]; 256],
        };
        self.lightmap(&mut g.lightmap);

        // GUI scale (ScaledResolution, "Auto").
        let mut scale = 1u32;
        while window_size.0 / (scale + 1) >= 320 && window_size.1 / (scale + 1) >= 240 {
            scale += 1;
        }
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
        if !self.stats.dead {
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
            entities::hand(&mut hand, &self.atlas, self.equipped, equip, swing, light);
        }

        let mut ui = crate::gui::Ui::new(
            &self.atlas,
            window_size.0 as f32 / scale as f32,
            window_size.1 as f32 / scale as f32,
        );
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
        screens::hotbar(&mut ui, &self.view.inv);
        if self.mode == GameMode::Survival {
            let under = eye_mat == Material::Water;
            let mut jitter = std::mem::replace(&mut self.jitter, JavaRandom::new(0));
            jitter.set_seed(self.ticks as i64 * 312871);
            screens::survival_bars(&mut ui, &self.stats, self.ticks as u32, &mut jitter, under);
            self.jitter = jitter;
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
        let m = (
            self.mouse_px.0 / scale as f32,
            self.mouse_px.1 / scale as f32,
        );
        match self.screen {
            Screen::Window => {
                screens::container(&mut ui, &self.view, m);
            }
            Screen::Paused => {
                screens::pause_menu(&mut ui, m);
            }
            Screen::Dead => {
                screens::death_screen(&mut ui, &self.stats, m);
            }
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
        let frame = Frame {
            crosshair: self.screen == Screen::Playing,
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
            clouds: Some(clouds),
        };
        self.renderer.render(&frame, window_size);
    }
}
