//! Game state, tick loop, chunk scheduling and per-frame environment
//! (`Minecraft.runTick`, `EntityRenderer`).

use crate::block::{self, Material};
use crate::entity::player::{Aabb, Input, Player};
use crate::java::JavaRandom;
use crate::render::atlas::Atlas;
use crate::render::mesher::{self, ChunkMesh, MeshChunk};
use crate::render::vk::{Frame, Globals, Renderer};
use crate::render::{self, Mat4};
use crate::util::math;
use crate::sim::{self, Command, Event, Sim, SimConfig, SimHandle};
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
        let Some(c) = self.chunks.get(&(x >> 4, z >> 4)) else { return if sky { 15 } else { 0 } };
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

    pub fn biome(&self, x: i32, z: i32) -> &'static Biome {
        match self.chunks.get(&(x >> 4, z >> 4)) {
            Some(c) => biome::get_or_ocean(c.mc.data.biomes[(((z & 15) << 4) | (x & 15)) as usize] as i32),
            None => biome::get_or_ocean(1),
        }
    }
}

impl BlockView for ClientWorld {
    fn peek_block(&self, x: i32, y: i32, z: i32) -> u8 {
        if !(0..256).contains(&y) {
            return block::AIR;
        }
        self.chunks.get(&(x >> 4, z >> 4)).map_or(block::AIR, |c| c.mc.data.blocks[Self::index(x, y, z)])
    }
    fn peek_meta(&self, x: i32, y: i32, z: i32) -> u8 {
        if !(0..256).contains(&y) {
            return 0;
        }
        self.chunks.get(&(x >> 4, z >> 4)).map_or(0, |c| c.mc.data.meta[Self::index(x, y, z)])
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
        let (server, start) = Sim::new(&SimConfig { seed: cfg.seed, view: cfg.render_distance, save_dir: cfg.save_dir.clone(), start_time: cfg.start_time, freeze_time: cfg.freeze_time });
        let sim = sim::spawn(server);

        // Meshing workers.
        let (jobs, job_rx) = channel::<Job>();
        let (done_tx, done) = channel::<ChunkMesh>();
        let job_rx = Arc::new(Mutex::new(job_rx));
        let noise = Arc::new(BiomeNoise::new());
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).saturating_sub(2).clamp(1, 12);
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
        player.flying = flying;

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

        let center = (math::floor_double(pos[0]) >> 4, math::floor_double(pos[2]) >> 4);
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
            world_time: if cfg.start_time != 0 || !start.resumed { cfg.start_time.max(start.world_time) } else { start.world_time },
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
        }
    }
    pub fn spawn_info(&self) -> String {
        format!("{:.1} {:.1} {:.1}", self.player.pos[0], self.player.bb.min[1], self.player.pos[2])
    }

    pub fn loaded_meshes(&self) -> usize {
        self.renderer.mesh_count()
    }

    /// True once every chunk in view has an up-to-date mesh.
    pub fn idle(&self) -> bool {
        let side = (self.render_distance * 2 + 1) as usize;
        self.in_flight == 0 && self.dirty.is_empty() && self.renderer.mesh_count() >= side * side
    }

    fn player_chunk(&self) -> (i32, i32) {
        (math::floor_double(self.player.pos[0]) >> 4, math::floor_double(self.player.pos[2]) >> 4)
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
                    self.world.chunks.insert(key, ClientChunk { mc: chunk, ready });
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
        self.dirty.retain(|k| (k.0 - pcx).abs() <= r && (k.1 - pcz).abs() <= r);
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
        let _ = self.sim.commands.send(Command::Player([p.pos[0], p.bb.min[1], p.pos[2]], [p.yaw, p.pitch], p.flying));
        self.sim.shutdown();
    }
    fn celestial_angle(&self, partial: f32) -> f32 {
        sim::celestial_angle(self.world_time, partial)
    }

    fn tick(&mut self) {
        self.ticks += 1;
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
        let (bx, by, bz) = (math::floor_double(p[0]), math::floor_double(p[1]), math::floor_double(p[2]));
        let sub = sim::skylight_subtracted(self.world_time);
        let level = (self.world.light(true, bx, by, bz) - sub).max(self.world.light(false, bx, by, bz)).clamp(0, 15);
        let f1 = self.light_table[level as usize];
        let f2 = self.render_distance as f32 / 16.0f32;
        // MC-31681: vanilla dims the fog (and the sky behind it) with the
        // light level at the player, so it darkens under trees and indoors.
        let f3 = if crate::compat::fixed() { 1.0 } else { f1 * (1.0f32 - f2) + f2 };
        self.fog_color1 += (f3 - self.fog_color1) * 0.1f32;

        let (pcx, pcz) = self.player_chunk();
        if self.ticks % 20 == 0 {
            let p = &self.player;
            let _ = self.sim.commands.send(Command::Player([p.pos[0], p.bb.min[1], p.pos[2]], [p.yaw, p.pitch], p.flying));
        }
        if self.world.loaded(pcx << 4, pcz << 4) {
            let input = self.input;
            self.player.tick(&self.world, &input);
        } else {
            self.player.prev_pos = self.player.pos;
        }
        if self.ticks % 40 == 0 {
            self.unload_far();
        }
    }

    fn eye(&self, partial: f32) -> [f64; 3] {
        let (a, b) = (self.player.prev_pos, self.player.pos);
        let t = partial as f64;
        [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
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

    /// Block under the crosshair within survival reach (4.5 blocks).
    fn pick(&self, eye: [f64; 3]) -> Option<([i32; 3], [i32; 3])> {
        let d = self.look();
        let reach = 4.5f64;
        let mut cell = [math::floor_double(eye[0]), math::floor_double(eye[1]), math::floor_double(eye[2])];
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

    /// Left click: remove the targeted block.
    pub fn break_block(&mut self) {
        if let Some((b, _)) = self.selected {
            let _ = self.sim.commands.send(Command::Break(b));
        }
    }

    /// Right click: place a block against the targeted face.
    pub fn place_block(&mut self, id: u8, meta: i32) {
        if let Some((b, n)) = self.selected {
            let target = self.world.peek_block(b[0], b[1], b[2]);
            // Replaceable targets (tall grass, snow layers, vines) are replaced in place.
            let p = if block::material(target).replaceable() { b } else { [b[0] + n[0], b[1] + n[1], b[2] + n[2]] };
            if n == [0, 0, 0] || !(0..256).contains(&p[1]) {
                return;
            }
            if !block::material(self.world.peek_block(p[0], p[1], p[2])).replaceable() {
                return;
            }
            let bb = Aabb { min: [p[0] as f64, p[1] as f64, p[2] as f64], max: [p[0] as f64 + 1.0, p[1] as f64 + 1.0, p[2] as f64 + 1.0] };
            if block::collision_box(id, meta as u8).is_some() && bb.intersects(&self.player.bb) {
                return;
            }
            let _ = self.sim.commands.send(Command::Place { pos: p, id, meta });
        }
    }

    /// Middle click: id and metadata of the targeted block.
    pub fn pick_block(&self) -> Option<(u8, i32)> {
        self.selected.map(|(b, _)| (self.world.peek_block(b[0], b[1], b[2]), self.world.peek_meta(b[0], b[1], b[2]) as i32))
    }

    fn lightmap(&self, out: &mut [[f32; 4]; 256]) {
        // World.getSunBrightness (client)
        let angle = self.celestial_angle(1.0);
        let mut sb = 1.0f32 - (math::cos(angle * math::PI * 2.0f32) * 2.0f32 + 0.2f32);
        sb = sb.clamp(0.0, 1.0);
        sb = 1.0 - sb;
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
        let eye_mat = self.player.eye_material(&self.world, eye);

        // --- Fog and sky colours (EntityRenderer.updateFogColor) ---
        let rd = self.render_distance as f32;
        let angle = self.celestial_angle(partial);
        let day = (math::cos(angle * math::PI * 2.0f32) * 2.0f32 + 0.5f32).clamp(0.0, 1.0);
        let (ex, ey, ez) = (math::floor_double(eye[0]), math::floor_double(eye[1]), math::floor_double(eye[2]));
        let b = self.world.biome(ex, ez);
        let temp = b.float_temperature(&self.noise, ex, ey, ez);
        let t = (temp / 3.0f32).clamp(-1.0, 1.0);
        let sky_rgb = hsb_to_rgb(0.62222224f32 - t * 0.05f32, 0.5f32 + t * 0.1f32, 1.0);
        let sky = [sky_rgb[0] * day, sky_rgb[1] * day, sky_rgb[2] * day];
        let mut fog = [0.7529412f32 * (day * 0.94f32 + 0.06f32), 0.84705883f32 * (day * 0.94f32 + 0.06f32), 1.0f32 * (day * 0.91f32 + 0.09f32)];
        let look = self.look();
        if self.render_distance >= 4 {
            let sun_side = if math::sin(angle * math::PI * 2.0f32) > 0.0 { -1.0 } else { 1.0 };
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
        let sky_light = self.world.light(true, ex, math::floor_double(self.player.bb.min[1] + 1.8f32 as f64 * 0.66), ez);
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
        let mut fov = 70.0f32 * (self.fov_hand_prev + (self.fov_hand - self.fov_hand_prev) * partial);
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
        let mut view: Mat4 = render::translate(math::sin(b1 * math::PI) * b2 * 0.5f32, -(math::cos(b1 * math::PI) * b2).abs(), 0.0);
        view = render::mul(&view, &render::rot_z(math::sin(b1 * math::PI) * b2 * 3.0f32));
        view = render::mul(&view, &render::rot_x((math::cos(b1 * math::PI - 0.2f32) * b2).abs() * 5.0f32));
        view = render::mul(&view, &render::rot_x(b3));
        view = render::mul(&view, &render::translate(0.0, 0.0, -0.1));
        view = render::mul(&view, &render::rot_x(p.pitch));
        view = render::mul(&view, &render::rot_y(p.yaw + 180.0));
        let view_proj = render::mul(&proj, &view);
        let tan = (fov.to_radians() * 0.5).tan();
        let row = |i: usize, s: f32| [view[i] * s, view[4 + i] * s, view[8 + i] * s, 0.0];

        let theta = angle * math::PI * 2.0f32;
        let moon_phase = ((self.world_time / 24000) % 8) as usize;
        let mut g = Globals {
            view_proj,
            cam_right: row(0, tan * aspect),
            cam_up: row(1, tan),
            cam_fwd: row(2, -1.0),
            fog_color: [fog[0], fog[1], fog[2], 1.0],
            sky_color: [sky[0], sky[1], sky[2], 1.0],
            fog_params,
            sun_dir: [-theta.sin(), theta.cos(), 0.0, (eye[1] - 63.0) as f32],
            sky_params: [far * 0.8f32, 1.0, 0.0, 0.0],
            sun_rect: self.atlas.rects[self.atlas.sun as usize],
            moon_rect: self.atlas.rects[self.atlas.moon[moon_phase] as usize],
            void_color: [sky[0] * 0.2f32 + 0.04f32, sky[1] * 0.2f32 + 0.04f32, sky[2] * 0.6f32 + 0.1f32, 1.0],
            lightmap: [[0.0; 4]; 256],
        };
        self.lightmap(&mut g.lightmap);

        // GUI scale (ScaledResolution, "Auto").
        let mut scale = 1u32;
        while window_size.0 / (scale + 1) >= 320 && window_size.1 / (scale + 1) >= 240 {
            scale += 1;
        }
        let frame = Frame { globals: &g, cam: eye, outline: self.selected.map(|s| s.0), gui_scale: scale as f32, screenshot: self.screenshot.take() };
        self.renderer.render(&frame, window_size);
    }
}
