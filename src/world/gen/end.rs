//! Port of `ChunkProviderEnd` (the floating island) and `BiomeEndDecorator`
//! (obsidian spikes with ender crystals, the dragon).

use super::chunk_provider::{boxed, ChunkData};
use super::noise::NoiseGeneratorOctaves;
use crate::block;
use crate::java::JavaRandom;
use crate::util::math;
use crate::world::server::{GenEntity, ServerWorld};

pub struct EndGen {
    gen1: NoiseGeneratorOctaves,
    gen2: NoiseGeneratorOctaves,
    gen3: NoiseGeneratorOctaves,
    gen4: NoiseGeneratorOctaves,
    gen5: NoiseGeneratorOctaves,
}

impl EndGen {
    pub fn new(seed: i64) -> EndGen {
        let mut r = JavaRandom::new(seed);
        EndGen {
            gen1: NoiseGeneratorOctaves::new(&mut r, 16),
            gen2: NoiseGeneratorOctaves::new(&mut r, 16),
            gen3: NoiseGeneratorOctaves::new(&mut r, 8),
            gen4: NoiseGeneratorOctaves::new(&mut r, 10),
            gen5: NoiseGeneratorOctaves::new(&mut r, 16),
        }
    }

    /// `initializeNoiseField`
    fn noise_field(&self, x: i32, y: i32, z: i32, xs: i32, ys: i32, zs: i32) -> Vec<f64> {
        let a = 684.412 * 2.0;
        let b = 684.412;
        let n2 = (xs * zs) as usize;
        let n3 = (xs * ys * zs) as usize;
        let mut d4 = vec![0.0; n2];
        let mut d5 = vec![0.0; n2];
        let mut d1 = vec![0.0; n3];
        let mut d2 = vec![0.0; n3];
        let mut d3 = vec![0.0; n3];
        self.gen4
            .generate_2d(&mut d4, x, z, xs, zs, 1.121, 1.121, 0.5);
        self.gen5
            .generate_2d(&mut d5, x, z, xs, zs, 200.0, 200.0, 0.5);
        self.gen3
            .generate(&mut d1, x, y, z, xs, ys, zs, a / 80.0, b / 160.0, a / 80.0);
        self.gen2.generate(&mut d2, x, y, z, xs, ys, zs, a, b, a);
        self.gen1.generate(&mut d3, x, y, z, xs, ys, zs, a, b, a);
        // noiseData2 comes from noiseGen1 and noiseData3 from noiseGen2.
        std::mem::swap(&mut d2, &mut d3);
        let mut out = vec![0.0; n3];
        let (mut i12, mut i13) = (0usize, 0usize);
        for i in 0..xs {
            for k in 0..zs {
                let mut scale = (d4[i13] + 256.0) / 512.0;
                if scale > 1.0 {
                    scale = 1.0;
                }
                let fx = (i + x) as f32;
                let fz = (k + z) as f32;
                let mut island = 100.0f32 - math::sqrt_float(fx * fx + fz * fz) * 8.0;
                island = island.clamp(-100.0, 80.0);
                if scale < 0.0 {
                    scale = 0.0;
                }
                scale += 0.5;
                i13 += 1;
                let half = ys as f64 / 2.0;
                for j in 0..ys {
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
                    let _ = (j as f64 - half) * 8.0 / scale;
                    v -= 8.0;
                    v += island as f64;
                    if j > ys / 2 - 2 {
                        let f = (((j - (ys / 2 - 2)) as f32 / 64.0f32) as f64).clamp(0.0, 1.0);
                        v = v * (1.0 - f) + -3000.0 * f;
                    }
                    if j < 8 {
                        let f = ((8 - j) as f32 / (8.0f32 - 1.0)) as f64;
                        v = v * (1.0 - f) + -30.0 * f;
                    }
                    out[i12] = v;
                    i12 += 1;
                }
            }
        }
        out
    }

    /// `provideChunk`: end stone where the density is positive.
    pub fn provide(&self, cx: i32, cz: i32) -> ChunkData {
        let (xs, ys, zs) = (3usize, 33usize, 3usize);
        let f = self.noise_field(cx * 2, 0, cz * 2, 3, 33, 3);
        let mut blocks = boxed();
        for i in 0..2usize {
            for j in 0..2usize {
                for k in 0..32usize {
                    let at = |a: usize, c: usize, y: usize| f[((i + a) * zs + j + c) * ys + k + y];
                    let mut v1 = at(0, 0, 0);
                    let mut v2 = at(0, 1, 0);
                    let mut v3 = at(1, 0, 0);
                    let mut v4 = at(1, 1, 0);
                    let s1 = (at(0, 0, 1) - v1) * 0.25;
                    let s2 = (at(0, 1, 1) - v2) * 0.25;
                    let s3 = (at(1, 0, 1) - v3) * 0.25;
                    let s4 = (at(1, 1, 1) - v4) * 0.25;
                    for l in 0..4 {
                        let mut a = v1;
                        let mut c = v2;
                        let da = (v3 - v1) * 0.125;
                        let dc = (v4 - v2) * 0.125;
                        for m in 0..8 {
                            let x = m + i * 8;
                            let y = k * 4 + l;
                            let mut v = a;
                            let dv = (c - a) * 0.125;
                            for n in 0..8 {
                                let z = j * 8 + n;
                                if v > 0.0 {
                                    blocks[(x * 16 + z) * 256 + y] = block::END_STONE;
                                }
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
        ChunkData {
            cx,
            cz,
            blocks,
            meta: boxed(),
            biomes: [9; 256],
            sky: boxed(),
            blk: boxed(),
        }
    }
}

/// `ChunkProviderEnd.populate` / `BiomeEndDecorator.genDecorations`
/// (with `World.rand`).
pub fn populate(w: &mut ServerWorld, cx: i32, cz: i32) {
    w.fall_instantly = true;
    let (bx, bz) = (cx * 16, cz * 16);
    let mut r = std::mem::replace(&mut w.rand, JavaRandom::new(0));
    crate::world::populate::generate_ores(w, &mut r, bx, bz);
    if r.next_int_n(5) == 0 {
        let x = bx + r.next_int_n(16) + 8;
        let z = bz + r.next_int_n(16) + 8;
        let y = w.top_solid_or_liquid(x, z);
        spike(w, &mut r, x, y, z);
    }
    if cx == 0 && cz == 0 {
        let yaw = r.next_float() * 360.0;
        w.gen_entities.push(GenEntity::Dragon {
            pos: [0.0, 128.0, 0.0],
            yaw,
        });
    }
    w.rand = r;
    w.fall_instantly = false;
}

/// `WorldGenSpikes(end_stone)`: an obsidian pillar with a crystal on top.
fn spike(w: &mut ServerWorld, r: &mut JavaRandom, x: i32, y: i32, z: i32) -> bool {
    if !w.is_air(x, y, z) || w.block(x, y - 1, z) != block::END_STONE {
        return false;
    }
    let h = r.next_int_n(32) + 6;
    let rad = r.next_int_n(4) + 1;
    for px in x - rad..=x + rad {
        for pz in z - rad..=z + rad {
            let (dx, dz) = (px - x, pz - z);
            if dx * dx + dz * dz <= rad * rad + 1 && w.block(px, y - 1, pz) != block::END_STONE {
                return false;
            }
        }
    }
    let mut py = y;
    while py < y + h && py < 256 {
        for px in x - rad..=x + rad {
            for pz in z - rad..=z + rad {
                let (dx, dz) = (px - x, pz - z);
                if dx * dx + dz * dz <= rad * rad + 1 {
                    w.set_block(px, py, pz, block::OBSIDIAN, 0, 2);
                }
            }
        }
        py += 1;
    }
    let yaw = r.next_float() * 360.0;
    w.gen_entities.push(GenEntity::Crystal {
        pos: [x as f64 + 0.5, (y + h) as f64, z as f64 + 0.5],
        yaw,
    });
    w.set_block(x, y + h, z, block::BEDROCK, 0, 2);
    true
}
