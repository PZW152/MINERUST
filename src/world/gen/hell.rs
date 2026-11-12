//! Port of `ChunkProviderHell` (Nether terrain, surface and population) and
//! `MapGenCavesHell`.

use super::chunk_provider::{boxed, ChunkData};
use super::noise::NoiseGeneratorOctaves;
use crate::block;
use crate::java::JavaRandom;
use crate::util::math::{self, floor_double};
use crate::world::server::ServerWorld;
use std::sync::Mutex;

pub struct HellGen {
    seed: i64,
    /// `hellRNG`: shared by `provideChunk` and `populate`, as in vanilla.
    pub rng: Mutex<JavaRandom>,
    /// Number of `provideChunk` calls, to notice one made during `populate`.
    pub provides: std::sync::atomic::AtomicU64,
    /// `provides` when `populate` last took over the generator state.
    observed: std::sync::atomic::AtomicU64,
    gen1: NoiseGeneratorOctaves,
    gen2: NoiseGeneratorOctaves,
    gen3: NoiseGeneratorOctaves,
    sand_gravel: NoiseGeneratorOctaves,
    exclusivity: NoiseGeneratorOctaves,
    gen6: NoiseGeneratorOctaves,
    gen7: NoiseGeneratorOctaves,
}

/// Index into the 128-high Java block array.
#[inline]
fn hidx(x: usize, z: usize, y: usize) -> usize {
    (x * 16 + z) * 128 + y
}

impl HellGen {
    pub fn new(seed: i64) -> HellGen {
        let mut r = JavaRandom::new(seed);
        let gen1 = NoiseGeneratorOctaves::new(&mut r, 16);
        let gen2 = NoiseGeneratorOctaves::new(&mut r, 16);
        let gen3 = NoiseGeneratorOctaves::new(&mut r, 8);
        let sand_gravel = NoiseGeneratorOctaves::new(&mut r, 4);
        let exclusivity = NoiseGeneratorOctaves::new(&mut r, 4);
        let gen6 = NoiseGeneratorOctaves::new(&mut r, 10);
        let gen7 = NoiseGeneratorOctaves::new(&mut r, 16);
        HellGen {
            seed,
            rng: Mutex::new(r),
            provides: std::sync::atomic::AtomicU64::new(0),
            observed: std::sync::atomic::AtomicU64::new(0),
            gen1,
            gen2,
            gen3,
            sand_gravel,
            exclusivity,
            gen6,
            gen7,
        }
    }

    /// `initializeNoiseField`
    fn noise_field(&self, x: i32, y: i32, z: i32, xs: i32, ys: i32, zs: i32) -> Vec<f64> {
        let (a, b) = (684.412, 2053.236);
        let n2 = (xs * zs) as usize;
        let n3 = (xs * ys * zs) as usize;
        let mut d4 = vec![0.0; n2];
        let mut d5 = vec![0.0; n2];
        let mut d1 = vec![0.0; n3];
        let mut d2 = vec![0.0; n3];
        let mut d3 = vec![0.0; n3];
        self.gen6
            .generate(&mut d4, x, y, z, xs, 1, zs, 1.0, 0.0, 1.0);
        self.gen7
            .generate(&mut d5, x, y, z, xs, 1, zs, 100.0, 0.0, 100.0);
        self.gen3
            .generate(&mut d1, x, y, z, xs, ys, zs, a / 80.0, b / 60.0, a / 80.0);
        self.gen1.generate(&mut d2, x, y, z, xs, ys, zs, a, b, a);
        self.gen2.generate(&mut d3, x, y, z, xs, ys, zs, a, b, a);
        let mut out = vec![0.0; n3];
        let mut col = vec![0.0f64; ys as usize];
        for (i, c) in col.iter_mut().enumerate() {
            *c = (i as f64 * std::f64::consts::PI * 6.0 / ys as f64).cos() * 2.0;
            let mut d = i as f64;
            if i as i32 > ys / 2 {
                d = (ys - 1 - i as i32) as f64;
            }
            if d < 4.0 {
                d = 4.0 - d;
                *c -= d * d * d * 10.0;
            }
        }
        let (mut i12, mut i13) = (0usize, 0usize);
        for _ in 0..xs {
            for _ in 0..zs {
                // The scale and depth terms are computed and unused, as in vanilla.
                let _ = d4[i13];
                let _ = d5[i13];
                i13 += 1;
                for k in 0..ys as usize {
                    let lo = d2[i12] / 512.0;
                    let hi = d3[i12] / 512.0;
                    let t = (d1[i12] / 10.0 + 1.0) / 2.0;
                    let mut v = if t < 0.0 {
                        lo
                    } else if t > 1.0 {
                        hi
                    } else {
                        lo + (hi - lo) * t
                    };
                    v -= col[k];
                    if k as i32 > ys - 4 {
                        let f = ((k as i32 - (ys - 4)) as f32 / 3.0f32) as f64;
                        v = v * (1.0 - f) + -10.0 * f;
                    }
                    out[i12] = v;
                    i12 += 1;
                }
            }
        }
        out
    }

    /// `generate`: netherrack against the lava sea.
    fn shape(&self, cx: i32, cz: i32, b: &mut [u8]) {
        let (xs, ys, zs) = (5usize, 17usize, 5usize);
        let f = self.noise_field(cx * 4, 0, cz * 4, 5, 17, 5);
        for i in 0..4 {
            for j in 0..4 {
                for k in 0..16 {
                    let at = |a: usize, c: usize, y: usize| f[((i + a) * zs + j + c) * ys + k + y];
                    let mut v1 = at(0, 0, 0);
                    let mut v2 = at(0, 1, 0);
                    let mut v3 = at(1, 0, 0);
                    let mut v4 = at(1, 1, 0);
                    let s1 = (at(0, 0, 1) - v1) * 0.125;
                    let s2 = (at(0, 1, 1) - v2) * 0.125;
                    let s3 = (at(1, 0, 1) - v3) * 0.125;
                    let s4 = (at(1, 1, 1) - v4) * 0.125;
                    for l in 0..8 {
                        let mut a = v1;
                        let mut c = v2;
                        let da = (v3 - v1) * 0.25;
                        let dc = (v4 - v2) * 0.25;
                        for m in 0..4 {
                            let mut idx = (m + i * 4) << 11 | (j * 4) << 7 | (k * 8 + l);
                            let mut v = a;
                            let dv = (c - a) * 0.25;
                            for _ in 0..4 {
                                let mut id = block::AIR;
                                if k * 8 + l < 32 {
                                    id = block::LAVA;
                                }
                                if v > 0.0 {
                                    id = block::NETHERRACK;
                                }
                                b[idx] = id;
                                idx += 128;
                                v += dv;
                            }
                            a += da;
                            c += dc;
                        }
                        v1 += s1;
                        v2 += s2;
                        v3 += s3;
                        v4 += s4;
                    }
                }
            }
        }
        let _ = xs;
    }

    /// `generateSurfaces`: soul sand, gravel and the bedrock floor and roof.
    fn surface(&self, cx: i32, cz: i32, b: &mut [u8], r: &mut JavaRandom) {
        let sea = 64;
        let s = 0.03125;
        let mut sand = vec![0.0; 256];
        let mut gravel = vec![0.0; 256];
        let mut excl = vec![0.0; 256];
        self.sand_gravel
            .generate(&mut sand, cx * 16, cz * 16, 0, 16, 16, 1, s, s, 1.0);
        self.sand_gravel
            .generate(&mut gravel, cx * 16, 109, cz * 16, 16, 1, 16, s, 1.0, s);
        self.exclusivity.generate(
            &mut excl,
            cx * 16,
            cz * 16,
            0,
            16,
            16,
            1,
            s * 2.0,
            s * 2.0,
            s * 2.0,
        );
        for x in 0..16usize {
            for z in 0..16usize {
                let soul = sand[x + z * 16] + r.next_double() * 0.2 > 0.0;
                let grav = gravel[x + z * 16] + r.next_double() * 0.2 > 0.0;
                let depth = (excl[x + z * 16] / 3.0 + 3.0 + r.next_double() * 0.25) as i32;
                let mut run = -1;
                let mut top = block::NETHERRACK;
                let mut fill = block::NETHERRACK;
                for y in (0..128i32).rev() {
                    let i = (z * 16 + x) * 128 + y as usize;
                    if y < 127 - r.next_int_n(5) && y > r.next_int_n(5) {
                        let cur = b[i];
                        if cur == block::AIR {
                            run = -1;
                        } else if cur == block::NETHERRACK {
                            if run == -1 {
                                if depth <= 0 {
                                    top = block::AIR;
                                    fill = block::NETHERRACK;
                                } else if y >= sea - 4 && y <= sea + 1 {
                                    top = block::NETHERRACK;
                                    fill = block::NETHERRACK;
                                    if grav {
                                        top = block::GRAVEL;
                                        fill = block::NETHERRACK;
                                    }
                                    if soul {
                                        top = block::SOUL_SAND;
                                        fill = block::SOUL_SAND;
                                    }
                                }
                                if y < sea && top == block::AIR {
                                    top = block::LAVA;
                                }
                                run = depth;
                                b[i] = if y >= sea - 1 { top } else { fill };
                            } else if run > 0 {
                                run -= 1;
                                b[i] = fill;
                            }
                        }
                    } else {
                        b[i] = block::BEDROCK;
                    }
                }
            }
        }
    }

    /// `provideChunk`
    pub fn provide(&self, cx: i32, cz: i32) -> ChunkData {
        let mut b = vec![0u8; 32768];
        self.provides
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        {
            let mut r = self.rng.lock().unwrap();
            r.set_seed(
                (cx as i64)
                    .wrapping_mul(341873128712)
                    .wrapping_add((cz as i64).wrapping_mul(132897987541)),
            );
            self.shape(cx, cz, &mut b);
            self.surface(cx, cz, &mut b, &mut r);
        }
        caves(self.seed, cx, cz, &mut b);
        let mut blocks = boxed();
        for x in 0..16 {
            for z in 0..16 {
                for y in 0..128 {
                    blocks[(x * 16 + z) * 256 + y] = b[hidx(x, z, y)];
                }
            }
        }
        ChunkData {
            cx,
            cz,
            blocks,
            meta: boxed(),
            biomes: [8; 256],
            sky: boxed(),
            blk: boxed(),
        }
    }
}

/// `MapGenBase.func_151539_a` with `MapGenCavesHell`.
fn caves(seed: i64, cx: i32, cz: i32, b: &mut [u8]) {
    let mut rand = JavaRandom::new(seed);
    let r1 = rand.next_long();
    let r2 = rand.next_long();
    for x in (cx - 8)..=(cx + 8) {
        for z in (cz - 8)..=(cz + 8) {
            rand.set_seed((x as i64).wrapping_mul(r1) ^ (z as i64).wrapping_mul(r2) ^ seed);
            let n = {
                let a = rand.next_int_n(10) + 1;
                let a = rand.next_int_n(a) + 1;
                let mut n = rand.next_int_n(a);
                if rand.next_int_n(5) != 0 {
                    n = 0;
                }
                n
            };
            for _ in 0..n {
                let px = (x * 16 + rand.next_int_n(16)) as f64;
                let py = rand.next_int_n(128) as f64;
                let pz = (z * 16 + rand.next_int_n(16)) as f64;
                let mut k = 1;
                if rand.next_int_n(4) == 0 {
                    let s = rand.next_long();
                    let w = 1.0 + rand.next_float() * 6.0;
                    branch(s, cx, cz, b, px, py, pz, w, 0.0, 0.0, -1, -1, 0.5);
                    k += rand.next_int_n(4);
                }
                for _ in 0..k {
                    let yaw = rand.next_float() * math::PI * 2.0;
                    let pitch = (rand.next_float() - 0.5) * 2.0 / 8.0;
                    let w = rand.next_float() * 2.0 + rand.next_float();
                    let s = rand.next_long();
                    branch(s, cx, cz, b, px, py, pz, w * 2.0, yaw, pitch, 0, 0, 0.5);
                }
            }
        }
    }
}

/// `MapGenCavesHell.placeBranch`
#[allow(clippy::too_many_arguments)]
fn branch(
    seed: i64,
    cx: i32,
    cz: i32,
    b: &mut [u8],
    mut x: f64,
    mut y: f64,
    mut z: f64,
    width: f32,
    mut yaw: f32,
    mut pitch: f32,
    mut step: i32,
    mut steps: i32,
    vscale: f64,
) {
    let mx = (cx * 16 + 8) as f64;
    let mz = (cz * 16 + 8) as f64;
    let mut dyaw = 0.0f32;
    let mut dpitch = 0.0f32;
    let mut r = JavaRandom::new(seed);
    if steps <= 0 {
        let k = 8 * 16 - 16;
        steps = k - r.next_int_n(k / 4);
    }
    let mut room = false;
    if step == -1 {
        step = steps / 2;
        room = true;
    }
    let split = r.next_int_n(steps / 2) + steps / 4;
    let steep = r.next_int_n(6) == 0;
    while step < steps {
        let h = 1.5 + (math::sin(step as f32 * math::PI / steps as f32) * width * 1.0) as f64;
        let v = h * vscale;
        let cp = math::cos(pitch);
        let sp = math::sin(pitch);
        x += (math::cos(yaw) * cp) as f64;
        y += sp as f64;
        z += (math::sin(yaw) * cp) as f64;
        pitch *= if steep { 0.92 } else { 0.7 };
        pitch += dpitch * 0.1;
        yaw += dyaw * 0.1;
        dpitch *= 0.9;
        dyaw *= 0.75;
        dpitch += (r.next_float() - r.next_float()) * r.next_float() * 2.0;
        dyaw += (r.next_float() - r.next_float()) * r.next_float() * 4.0;
        if !room && step == split && width > 1.0 {
            let s1 = r.next_long();
            let w1 = r.next_float() * 0.5 + 0.5;
            branch(
                s1,
                cx,
                cz,
                b,
                x,
                y,
                z,
                w1,
                yaw - math::PI / 2.0,
                pitch / 3.0,
                step,
                steps,
                1.0,
            );
            let s2 = r.next_long();
            let w2 = r.next_float() * 0.5 + 0.5;
            branch(
                s2,
                cx,
                cz,
                b,
                x,
                y,
                z,
                w2,
                yaw + math::PI / 2.0,
                pitch / 3.0,
                step,
                steps,
                1.0,
            );
            return;
        }
        if room || r.next_int_n(4) != 0 {
            let (dx, dz) = (x - mx, z - mz);
            let left = (steps - step) as f64;
            let reach = (width + 2.0 + 16.0) as f64;
            if dx * dx + dz * dz - left * left > reach * reach {
                return;
            }
            if !(x < mx - 16.0 - h * 2.0
                || z < mz - 16.0 - h * 2.0
                || x > mx + 16.0 + h * 2.0
                || z > mz + 16.0 + h * 2.0)
            {
                let x0 = (floor_double(x - h) - cx * 16 - 1).max(0);
                let x1 = (floor_double(x + h) - cx * 16 + 1).min(16);
                let y0 = (floor_double(y - v) - 1).max(1);
                let y1 = (floor_double(y + v) + 1).min(120);
                let z0 = (floor_double(z - h) - cz * 16 - 1).max(0);
                let z1 = (floor_double(z + h) - cz * 16 + 1).min(16);
                let mut lava = false;
                let mut xx = x0;
                while !lava && xx < x1 {
                    let mut zz = z0;
                    while !lava && zz < z1 {
                        let mut yy = y1 + 1;
                        while !lava && yy >= y0 - 1 {
                            if (0..128).contains(&yy) {
                                let id = b[hidx(xx as usize, zz as usize, yy as usize)];
                                if id == block::FLOWING_LAVA || id == block::LAVA {
                                    lava = true;
                                }
                                if yy != y0 - 1
                                    && xx != x0
                                    && xx != x1 - 1
                                    && zz != z0
                                    && zz != z1 - 1
                                {
                                    yy = y0;
                                }
                            }
                            yy -= 1;
                        }
                        zz += 1;
                    }
                    xx += 1;
                }
                if !lava {
                    for xx in x0..x1 {
                        let fx = ((xx + cx * 16) as f64 + 0.5 - x) / h;
                        for zz in z0..z1 {
                            let fz = ((zz + cz * 16) as f64 + 0.5 - z) / h;
                            let mut yy = y1 - 1;
                            while yy >= y0 {
                                let fy = (yy as f64 + 0.5 - y) / v;
                                if fy > -0.7 && fx * fx + fy * fy + fz * fz < 1.0 {
                                    // the index starts at y1 and steps down with yy
                                    let i = hidx(xx as usize, zz as usize, (yy + 1) as usize);
                                    let id = b[i];
                                    if id == block::NETHERRACK
                                        || id == block::DIRT
                                        || id == block::GRASS
                                    {
                                        b[i] = block::AIR;
                                    }
                                }
                                yy -= 1;
                            }
                        }
                    }
                    if room {
                        break;
                    }
                }
            }
        }
        step += 1;
    }
}

/// `ChunkProviderHell.populate`
pub fn populate(w: &mut ServerWorld, cx: i32, cz: i32) {
    let mut r = match w.provider.hell.as_ref() {
        Some(g) => g.rng.lock().unwrap().clone(),
        None => return,
    };
    {
        let g = w.provider.hell.as_ref().unwrap();
        let n = g.provides.load(std::sync::atomic::Ordering::SeqCst);
        g.observed.store(n, std::sync::atomic::Ordering::SeqCst);
    }
    let sync = |w: &ServerWorld, r: &mut JavaRandom| sync_rng(w, r);
    w.fall_instantly = true;
    let (bx, bz) = (cx * 16, cz * 16);
    if w.map_features {
        super::super::structure::generate_in_chunk(w, &mut r, cx, cz);
    }
    for _ in 0..8 {
        let x = bx + r.next_int_n(16) + 8;
        let y = r.next_int_n(120) + 4;
        let z = bz + r.next_int_n(16) + 8;
        hell_lava(w, &mut r, x, y, z, false);
        sync(w, &mut r);
    }
    let a = r.next_int_n(10) + 1;
    let n = r.next_int_n(a) + 1;
    for _ in 0..n {
        let x = bx + r.next_int_n(16) + 8;
        let y = r.next_int_n(120) + 4;
        let z = bz + r.next_int_n(16) + 8;
        // WorldGenFire
        for _ in 0..64 {
            let fx = x + r.next_int_n(8) - r.next_int_n(8);
            let fy = y + r.next_int_n(4) - r.next_int_n(4);
            let fz = z + r.next_int_n(8) - r.next_int_n(8);
            if w.is_air(fx, fy, fz) && w.block(fx, fy - 1, fz) == block::NETHERRACK {
                w.set_block(fx, fy, fz, block::FIRE, 0, 2);
            }
            sync(w, &mut r);
        }
    }
    let a = r.next_int_n(10) + 1;
    let n = r.next_int_n(a);
    for _ in 0..n {
        let x = bx + r.next_int_n(16) + 8;
        let y = r.next_int_n(120) + 4;
        let z = bz + r.next_int_n(16) + 8;
        glowstone(w, &mut r, x, y, z);
        sync(w, &mut r);
    }
    for _ in 0..10 {
        let x = bx + r.next_int_n(16) + 8;
        let y = r.next_int_n(128);
        let z = bz + r.next_int_n(16) + 8;
        glowstone(w, &mut r, x, y, z);
        sync(w, &mut r);
    }
    if r.next_int_n(1) == 0 {
        let x = bx + r.next_int_n(16) + 8;
        let y = r.next_int_n(128);
        let z = bz + r.next_int_n(16) + 8;
        crate::world::features::misc::flowers(w, &mut r, x, y, z, block::BROWN_MUSHROOM, 0);
        sync(w, &mut r);
    }
    if r.next_int_n(1) == 0 {
        let x = bx + r.next_int_n(16) + 8;
        let y = r.next_int_n(128);
        let z = bz + r.next_int_n(16) + 8;
        crate::world::features::misc::flowers(w, &mut r, x, y, z, block::RED_MUSHROOM, 0);
        sync(w, &mut r);
    }
    for _ in 0..16 {
        let x = bx + r.next_int_n(16);
        let y = r.next_int_n(108) + 10;
        let z = bz + r.next_int_n(16);
        crate::world::features::misc::minable(
            w,
            &mut r,
            x,
            y,
            z,
            block::QUARTZ_ORE,
            13,
            block::NETHERRACK,
        );
        sync(w, &mut r);
    }
    for _ in 0..16 {
        let x = bx + r.next_int_n(16);
        let y = r.next_int_n(108) + 10;
        let z = bz + r.next_int_n(16);
        hell_lava(w, &mut r, x, y, z, true);
        sync(w, &mut r);
    }
    w.fall_instantly = false;
    *w.provider.hell.as_ref().unwrap().rng.lock().unwrap() = r;
}

/// A chunk loaded while populating (a feature looking one block past the
/// populated area) reseeds vanilla's shared `hellRNG`; continue from there.
pub fn sync_rng(w: &ServerWorld, r: &mut JavaRandom) {
    use std::sync::atomic::Ordering::SeqCst;
    let Some(g) = w.provider.hell.as_ref() else {
        return;
    };
    let now = g.provides.load(SeqCst);
    if g.observed.load(SeqCst) != now {
        g.observed.store(now, SeqCst);
        *r = g.rng.lock().unwrap().clone();
    }
}

/// `WorldGenHellLava`
fn hell_lava(w: &mut ServerWorld, r: &mut JavaRandom, x: i32, y: i32, z: i32, inside: bool) {
    if w.block(x, y + 1, z) != block::NETHERRACK {
        return;
    }
    let here = w.block(x, y, z);
    if block::material(here) != block::Material::Air && here != block::NETHERRACK {
        return;
    }
    let mut rock = 0;
    let mut air = 0;
    for (dx, dy, dz) in [(-1, 0, 0), (1, 0, 0), (0, 0, -1), (0, 0, 1), (0, -1, 0)] {
        if w.block(x + dx, y + dy, z + dz) == block::NETHERRACK {
            rock += 1;
        }
        if w.is_air(x + dx, y + dy, z + dz) {
            air += 1;
        }
    }
    sync_rng(w, r);
    if (!inside && rock == 4 && air == 1) || rock == 5 {
        w.set_block(x, y, z, block::FLOWING_LAVA, 0, 2);
        w.scheduled_immediate = true;
        w.update_tick(block::FLOWING_LAVA, x, y, z, r);
        w.scheduled_immediate = false;
    }
}

/// `WorldGenGlowStone1` / `WorldGenGlowStone2` (identical).
fn glowstone(w: &mut ServerWorld, r: &mut JavaRandom, x: i32, y: i32, z: i32) {
    if !w.is_air(x, y, z) || w.block(x, y + 1, z) != block::NETHERRACK {
        return;
    }
    w.set_block(x, y, z, block::GLOWSTONE, 0, 2);
    for _ in 0..1500 {
        sync_rng(w, r);
        let gx = x + r.next_int_n(8) - r.next_int_n(8);
        let gy = y - r.next_int_n(12);
        let gz = z + r.next_int_n(8) - r.next_int_n(8);
        if w.material(gx, gy, gz) == block::Material::Air {
            let mut n = 0;
            for (dx, dy, dz) in [
                (-1, 0, 0),
                (1, 0, 0),
                (0, -1, 0),
                (0, 1, 0),
                (0, 0, -1),
                (0, 0, 1),
            ] {
                if w.block(gx + dx, gy + dy, gz + dz) == block::GLOWSTONE {
                    n += 1;
                }
            }
            if n == 1 {
                w.set_block(gx, gy, gz, block::GLOWSTONE, 0, 2);
            }
        }
    }
}
