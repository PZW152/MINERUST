//! Ports of `MapGenBase`, `MapGenCaves` and `MapGenRavine`.

use crate::block;
use crate::java::JavaRandom;
use crate::util::math::{self, floor_double, PI};

const RANGE: i32 = 8;

/// `tops[x * 16 + z]` is the `topBlock` of the biome at that column of the chunk
/// (the value the Java code reads through `getBiomeGenForCoords(x, z).topBlock`).
pub struct Carver<'a> {
    pub blocks: &'a mut [u8],
    pub tops: &'a [u8; 256],
    pub cx: i32,
    pub cz: i32,
}

/// `MapGenBase.func_151539_a` for caves.
pub fn generate_caves(seed: i64, c: &mut Carver) {
    let mut rand = JavaRandom::new(seed);
    let r1 = rand.next_long();
    let r2 = rand.next_long();
    for x in (c.cx - RANGE)..=(c.cx + RANGE) {
        for z in (c.cz - RANGE)..=(c.cz + RANGE) {
            let a = (x as i64).wrapping_mul(r1);
            let b = (z as i64).wrapping_mul(r2);
            rand.set_seed(a ^ b ^ seed);
            caves_at(&mut rand, x, z, c);
        }
    }
}

/// `MapGenBase.func_151539_a` for ravines.
pub fn generate_ravines(seed: i64, c: &mut Carver) {
    let mut rand = JavaRandom::new(seed);
    let r1 = rand.next_long();
    let r2 = rand.next_long();
    for x in (c.cx - RANGE)..=(c.cx + RANGE) {
        for z in (c.cz - RANGE)..=(c.cz + RANGE) {
            let a = (x as i64).wrapping_mul(r1);
            let b = (z as i64).wrapping_mul(r2);
            rand.set_seed(a ^ b ^ seed);
            ravine_at(&mut rand, x, z, c);
        }
    }
}

fn caves_at(rand: &mut JavaRandom, ox: i32, oz: i32, c: &mut Carver) {
    let a = rand.next_int_n(15) + 1;
    let b = rand.next_int_n(a) + 1;
    let mut n = rand.next_int_n(b);
    if rand.next_int_n(7) != 0 {
        n = 0;
    }
    for _ in 0..n {
        let x = (ox * 16 + rand.next_int_n(16)) as f64;
        let yb = rand.next_int_n(120) + 8;
        let y = rand.next_int_n(yb) as f64;
        let z = (oz * 16 + rand.next_int_n(16)) as f64;
        let mut cnt = 1;
        if rand.next_int_n(4) == 0 {
            let s = rand.next_long();
            let w = 1.0f32 + rand.next_float() * 6.0f32;
            cave_branch(s, c, x, y, z, w, 0.0, 0.0, -1, -1, 0.5);
            cnt += rand.next_int_n(4);
        }
        for _ in 0..cnt {
            let yaw = rand.next_float() * PI * 2.0f32;
            let pitch = (rand.next_float() - 0.5f32) * 2.0f32 / 8.0f32;
            let mut w = rand.next_float() * 2.0f32 + rand.next_float();
            if rand.next_int_n(10) == 0 {
                w *= rand.next_float() * rand.next_float() * 3.0f32 + 1.0f32;
            }
            cave_branch(rand.next_long(), c, x, y, z, w, yaw, pitch, 0, 0, 1.0);
        }
    }
}

fn ravine_at(rand: &mut JavaRandom, ox: i32, oz: i32, c: &mut Carver) {
    if rand.next_int_n(50) == 0 {
        let x = (ox * 16 + rand.next_int_n(16)) as f64;
        let yb = rand.next_int_n(40) + 8;
        let y = (rand.next_int_n(yb) + 20) as f64;
        let z = (oz * 16 + rand.next_int_n(16)) as f64;
        let yaw = rand.next_float() * PI * 2.0f32;
        let pitch = (rand.next_float() - 0.5f32) * 2.0f32 / 8.0f32;
        let w = (rand.next_float() * 2.0f32 + rand.next_float()) * 2.0f32;
        ravine_branch(rand.next_long(), c, x, y, z, w, yaw, pitch, 0, 0, 3.0);
    }
}

struct Bounds {
    x0: i32,
    x1: i32,
    y0: i32,
    y1: i32,
    z0: i32,
    z1: i32,
}

/// Computes the clamped carve bounds; returns `None` if water is found in them.
fn bounds(c: &Carver, x: f64, y: f64, z: f64, rad: f64, rad_y: f64) -> Option<Bounds> {
    let mut x0 = floor_double(x - rad) - c.cx * 16 - 1;
    let mut x1 = floor_double(x + rad) - c.cx * 16 + 1;
    let mut y0 = floor_double(y - rad_y) - 1;
    let mut y1 = floor_double(y + rad_y) + 1;
    let mut z0 = floor_double(z - rad) - c.cz * 16 - 1;
    let mut z1 = floor_double(z + rad) - c.cz * 16 + 1;
    if x0 < 0 {
        x0 = 0;
    }
    if x1 > 16 {
        x1 = 16;
    }
    if y0 < 1 {
        y0 = 1;
    }
    if y1 > 248 {
        y1 = 248;
    }
    if z0 < 0 {
        z0 = 0;
    }
    if z1 > 16 {
        z1 = 16;
    }
    let mut water = false;
    let mut xx = x0;
    while !water && xx < x1 {
        let mut zz = z0;
        while !water && zz < z1 {
            let mut yy = y1 + 1;
            while !water && yy >= y0 - 1 {
                let idx = (xx * 16 + zz) * 256 + yy;
                if yy >= 0 && yy < 256 {
                    let b = c.blocks[idx as usize];
                    if b == block::FLOWING_WATER || b == block::WATER {
                        water = true;
                    }
                    if yy != y0 - 1 && xx != x0 && xx != x1 - 1 && zz != z0 && zz != z1 - 1 {
                        yy = y0;
                    }
                }
                yy -= 1;
            }
            zz += 1;
        }
        xx += 1;
    }
    if water {
        None
    } else {
        Some(Bounds {
            x0,
            x1,
            y0,
            y1,
            z0,
            z1,
        })
    }
}

/// Shared carving loop; `inside(nx2+nz2, ny, yy)` decides whether a cell is carved.
fn carve(
    c: &mut Carver,
    b: &Bounds,
    x: f64,
    y: f64,
    z: f64,
    rad: f64,
    rad_y: f64,
    lava: u8,
    inside: &dyn Fn(f64, f64, i32) -> bool,
) {
    for xx in b.x0..b.x1 {
        let nx = ((xx + c.cx * 16) as f64 + 0.5 - x) / rad;
        for zz in b.z0..b.z1 {
            let nz = ((zz + c.cz * 16) as f64 + 0.5 - z) / rad;
            let mut idx = ((xx * 16 + zz) * 256 + b.y1) as usize;
            let mut grass = false;
            if nx * nx + nz * nz < 1.0 {
                let mut yy = b.y1 - 1;
                while yy >= b.y0 {
                    let ny = (yy as f64 + 0.5 - y) / rad_y;
                    if inside(nx * nx + nz * nz, ny, yy) {
                        let blk = c.blocks[idx];
                        if blk == block::GRASS {
                            grass = true;
                        }
                        if blk == block::STONE || blk == block::DIRT || blk == block::GRASS {
                            if yy < 10 {
                                c.blocks[idx] = lava;
                            } else {
                                c.blocks[idx] = block::AIR;
                                if grass && c.blocks[idx - 1] == block::DIRT {
                                    c.blocks[idx - 1] = c.tops[(xx * 16 + zz) as usize];
                                }
                            }
                        }
                    }
                    idx -= 1;
                    yy -= 1;
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn cave_branch(
    seed: i64,
    c: &mut Carver,
    mut x: f64,
    mut y: f64,
    mut z: f64,
    width: f32,
    mut yaw: f32,
    mut pitch: f32,
    mut step: i32,
    mut total: i32,
    yscale: f64,
) {
    let mx = (c.cx * 16 + 8) as f64;
    let mz = (c.cz * 16 + 8) as f64;
    let mut dyaw = 0.0f32;
    let mut dpitch = 0.0f32;
    let mut r = JavaRandom::new(seed);
    if total <= 0 {
        let t = RANGE * 16 - 16;
        total = t - r.next_int_n(t / 4);
    }
    let mut room = false;
    if step == -1 {
        step = total / 2;
        room = true;
    }
    let branch_at = r.next_int_n(total / 2) + total / 4;
    let steep = r.next_int_n(6) == 0;
    while step < total {
        let rad = 1.5 + (math::sin(step as f32 * PI / total as f32) * width * 1.0f32) as f64;
        let rad_y = rad * yscale;
        let cp = math::cos(pitch);
        let sp = math::sin(pitch);
        x += (math::cos(yaw) * cp) as f64;
        y += sp as f64;
        z += (math::sin(yaw) * cp) as f64;
        if steep {
            pitch *= 0.92f32;
        } else {
            pitch *= 0.7f32;
        }
        pitch += dpitch * 0.1f32;
        yaw += dyaw * 0.1f32;
        dpitch *= 0.9f32;
        dyaw *= 0.75f32;
        dpitch += (r.next_float() - r.next_float()) * r.next_float() * 2.0f32;
        dyaw += (r.next_float() - r.next_float()) * r.next_float() * 4.0f32;
        if !room && step == branch_at && width > 1.0f32 && total > 0 {
            let s1 = r.next_long();
            let w1 = r.next_float() * 0.5f32 + 0.5f32;
            cave_branch(
                s1,
                c,
                x,
                y,
                z,
                w1,
                yaw - PI / 2.0f32,
                pitch / 3.0f32,
                step,
                total,
                1.0,
            );
            let s2 = r.next_long();
            let w2 = r.next_float() * 0.5f32 + 0.5f32;
            cave_branch(
                s2,
                c,
                x,
                y,
                z,
                w2,
                yaw + PI / 2.0f32,
                pitch / 3.0f32,
                step,
                total,
                1.0,
            );
            return;
        }
        if room || r.next_int_n(4) != 0 {
            let dx = x - mx;
            let dz = z - mz;
            let rem = (total - step) as f64;
            let lim = (width + 2.0f32 + 16.0f32) as f64;
            if dx * dx + dz * dz - rem * rem > lim * lim {
                return;
            }
            if x >= mx - 16.0 - rad * 2.0
                && z >= mz - 16.0 - rad * 2.0
                && x <= mx + 16.0 + rad * 2.0
                && z <= mz + 16.0 + rad * 2.0
            {
                if let Some(b) = bounds(c, x, y, z, rad, rad_y) {
                    carve(c, &b, x, y, z, rad, rad_y, block::LAVA, &|h2, ny, _| {
                        ny > -0.7 && h2 + ny * ny < 1.0
                    });
                    if room {
                        break;
                    }
                }
            }
        }
        step += 1;
    }
}

#[allow(clippy::too_many_arguments)]
fn ravine_branch(
    seed: i64,
    c: &mut Carver,
    mut x: f64,
    mut y: f64,
    mut z: f64,
    width: f32,
    mut yaw: f32,
    mut pitch: f32,
    mut step: i32,
    mut total: i32,
    yscale: f64,
) {
    let mut r = JavaRandom::new(seed);
    let mx = (c.cx * 16 + 8) as f64;
    let mz = (c.cz * 16 + 8) as f64;
    let mut dyaw = 0.0f32;
    let mut dpitch = 0.0f32;
    if total <= 0 {
        let t = RANGE * 16 - 16;
        total = t - r.next_int_n(t / 4);
    }
    let mut room = false;
    if step == -1 {
        step = total / 2;
        room = true;
    }
    let mut rs = [0.0f32; 256];
    let mut f = 1.0f32;
    for (i, v) in rs.iter_mut().enumerate() {
        if i == 0 || r.next_int_n(3) == 0 {
            f = 1.0f32 + r.next_float() * r.next_float() * 1.0f32;
        }
        *v = f * f;
    }
    while step < total {
        let mut rad = 1.5 + (math::sin(step as f32 * PI / total as f32) * width * 1.0f32) as f64;
        let mut rad_y = rad * yscale;
        rad *= r.next_float() as f64 * 0.25 + 0.75;
        rad_y *= r.next_float() as f64 * 0.25 + 0.75;
        let cp = math::cos(pitch);
        let sp = math::sin(pitch);
        x += (math::cos(yaw) * cp) as f64;
        y += sp as f64;
        z += (math::sin(yaw) * cp) as f64;
        pitch *= 0.7f32;
        pitch += dpitch * 0.05f32;
        yaw += dyaw * 0.05f32;
        dpitch *= 0.8f32;
        dyaw *= 0.5f32;
        dpitch += (r.next_float() - r.next_float()) * r.next_float() * 2.0f32;
        dyaw += (r.next_float() - r.next_float()) * r.next_float() * 4.0f32;
        if room || r.next_int_n(4) != 0 {
            let dx = x - mx;
            let dz = z - mz;
            let rem = (total - step) as f64;
            let lim = (width + 2.0f32 + 16.0f32) as f64;
            if dx * dx + dz * dz - rem * rem > lim * lim {
                return;
            }
            if x >= mx - 16.0 - rad * 2.0
                && z >= mz - 16.0 - rad * 2.0
                && x <= mx + 16.0 + rad * 2.0
                && z <= mz + 16.0 + rad * 2.0
            {
                if let Some(b) = bounds(c, x, y, z, rad, rad_y) {
                    carve(
                        c,
                        &b,
                        x,
                        y,
                        z,
                        rad,
                        rad_y,
                        block::FLOWING_LAVA,
                        &|h2, ny, yy| h2 * rs[yy as usize] as f64 + ny * ny / 6.0 < 1.0,
                    );
                    if room {
                        break;
                    }
                }
            }
        }
        step += 1;
    }
}
