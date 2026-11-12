//! Non-tree world generators (`WorldGenLakes`, `WorldGenMinable`, plants, ...).

use super::trees::{can_grow_into, DIR_X, DIR_Z};
use crate::block::{self, Material};
use crate::java::JavaRandom;
use crate::util::math;
use crate::world::server::ServerWorld;

#[inline]
fn air_or_leaves(id: u8) -> bool {
    let m = block::material(id);
    m == Material::Air || m == Material::Leaves
}

/// `WorldGenLakes(block)`
pub fn lake(
    w: &mut ServerWorld,
    r: &mut JavaRandom,
    mut x: i32,
    mut y: i32,
    mut z: i32,
    liquid: u8,
) -> bool {
    x -= 8;
    z -= 8;
    while y > 5 && w.is_air(x, y, z) {
        y -= 1;
    }
    if y <= 4 {
        return false;
    }
    y -= 4;
    let mut m = [false; 2048];
    let n = r.next_int_n(4) + 4;
    for _ in 0..n {
        let d0 = r.next_double() * 6.0 + 3.0;
        let d1 = r.next_double() * 4.0 + 2.0;
        let d2 = r.next_double() * 6.0 + 3.0;
        let d3 = r.next_double() * (16.0 - d0 - 2.0) + 1.0 + d0 / 2.0;
        let d4 = r.next_double() * (8.0 - d1 - 4.0) + 2.0 + d1 / 2.0;
        let d5 = r.next_double() * (16.0 - d2 - 2.0) + 1.0 + d2 / 2.0;
        for i in 1..15 {
            for j in 1..15 {
                for k in 1..7 {
                    let a = (i as f64 - d3) / (d0 / 2.0);
                    let b = (k as f64 - d4) / (d1 / 2.0);
                    let c = (j as f64 - d5) / (d2 / 2.0);
                    if a * a + b * b + c * c < 1.0 {
                        m[(i * 16 + j) * 8 + k] = true;
                    }
                }
            }
        }
    }
    let edge = |m: &[bool; 2048], i: usize, j: usize, k: usize| {
        !m[(i * 16 + j) * 8 + k]
            && (i < 15 && m[((i + 1) * 16 + j) * 8 + k]
                || i > 0 && m[((i - 1) * 16 + j) * 8 + k]
                || j < 15 && m[(i * 16 + j + 1) * 8 + k]
                || j > 0 && m[(i * 16 + (j - 1)) * 8 + k]
                || k < 7 && m[(i * 16 + j) * 8 + k + 1]
                || k > 0 && m[(i * 16 + j) * 8 + (k - 1)])
    };
    for i in 0..16usize {
        for j in 0..16usize {
            for k in 0..8usize {
                if edge(&m, i, j, k) {
                    let id = w.block(x + i as i32, y + k as i32, z + j as i32);
                    let mat = block::material(id);
                    if k >= 4 && mat.is_liquid() {
                        return false;
                    }
                    if k < 4 && !mat.is_solid() && id != liquid {
                        return false;
                    }
                }
            }
        }
    }
    for i in 0..16usize {
        for j in 0..16usize {
            for k in 0..8usize {
                if m[(i * 16 + j) * 8 + k] {
                    w.set_block(
                        x + i as i32,
                        y + k as i32,
                        z + j as i32,
                        if k >= 4 { block::AIR } else { liquid },
                        0,
                        2,
                    );
                }
            }
        }
    }
    for i in 0..16usize {
        for j in 0..16usize {
            for k in 4..8usize {
                let (bx, by, bz) = (x + i as i32, y + k as i32, z + j as i32);
                if m[(i * 16 + j) * 8 + k]
                    && w.block(bx, by - 1, bz) == block::DIRT
                    && w.saved_light(true, bx, by, bz) > 0
                {
                    if w.biome(bx, bz).top_block == block::MYCELIUM {
                        w.set_block(bx, by - 1, bz, block::MYCELIUM, 0, 2);
                    } else {
                        w.set_block(bx, by - 1, bz, block::GRASS, 0, 2);
                    }
                }
            }
        }
    }
    if block::material(liquid) == Material::Lava {
        for i in 0..16usize {
            for j in 0..16usize {
                for k in 0..8usize {
                    let (bx, by, bz) = (x + i as i32, y + k as i32, z + j as i32);
                    if edge(&m, i, j, k)
                        && (k < 4 || r.next_int_n(2) != 0)
                        && w.material(bx, by, bz).is_solid()
                    {
                        w.set_block(bx, by, bz, block::STONE, 0, 2);
                    }
                }
            }
        }
    }
    if block::material(liquid) == Material::Water {
        for i in 0..16 {
            for j in 0..16 {
                if w.can_block_freeze(x + i, y + 4, z + j, false) {
                    w.set_block(x + i, y + 4, z + j, block::ICE, 0, 2);
                }
            }
        }
    }
    true
}

/// `WorldGenMinable(block, count, replace)`
pub fn minable(
    w: &mut ServerWorld,
    r: &mut JavaRandom,
    x: i32,
    y: i32,
    z: i32,
    id: u8,
    count: i32,
    replace: u8,
) -> bool {
    let n = count as f32;
    let f = r.next_float() * math::PI;
    let x0 = ((x + 8) as f32 + math::sin(f) * n / 8.0f32) as f64;
    let x1 = ((x + 8) as f32 - math::sin(f) * n / 8.0f32) as f64;
    let z0 = ((z + 8) as f32 + math::cos(f) * n / 8.0f32) as f64;
    let z1 = ((z + 8) as f32 - math::cos(f) * n / 8.0f32) as f64;
    let y0 = (y + r.next_int_n(3) - 2) as f64;
    let y1 = (y + r.next_int_n(3) - 2) as f64;
    for i in 0..=count {
        crate::world::gen::hell::sync_rng(w, r);
        let cx = x0 + (x1 - x0) * i as f64 / count as f64;
        let cy = y0 + (y1 - y0) * i as f64 / count as f64;
        let cz = z0 + (z1 - z0) * i as f64 / count as f64;
        let d = r.next_double() * count as f64 / 16.0;
        let s = (math::sin(i as f32 * math::PI / n) + 1.0f32) as f64 * d + 1.0;
        let h = (math::sin(i as f32 * math::PI / n) + 1.0f32) as f64 * d + 1.0;
        let bx0 = math::floor_double(cx - s / 2.0);
        let by0 = math::floor_double(cy - h / 2.0);
        let bz0 = math::floor_double(cz - s / 2.0);
        let bx1 = math::floor_double(cx + s / 2.0);
        let by1 = math::floor_double(cy + h / 2.0);
        let bz1 = math::floor_double(cz + s / 2.0);
        for bx in bx0..=bx1 {
            let dx = (bx as f64 + 0.5 - cx) / (s / 2.0);
            if dx * dx < 1.0 {
                for by in by0..=by1 {
                    let dy = (by as f64 + 0.5 - cy) / (h / 2.0);
                    if dx * dx + dy * dy < 1.0 {
                        for bz in bz0..=bz1 {
                            let dz = (bz as f64 + 0.5 - cz) / (s / 2.0);
                            if dx * dx + dy * dy + dz * dz < 1.0 && w.block(bx, by, bz) == replace {
                                w.set_block(bx, by, bz, id, 0, 2);
                            }
                        }
                    }
                }
            }
        }
    }
    true
}

/// `WorldGenSand(block, radius)` (also gravel) and `WorldGenClay(count)`.
pub fn disk(
    w: &mut ServerWorld,
    r: &mut JavaRandom,
    x: i32,
    y: i32,
    z: i32,
    id: u8,
    radius: i32,
) -> bool {
    if w.material(x, y, z) != Material::Water {
        return false;
    }
    let rad = r.next_int_n(radius - 2) + 2;
    let clay = id == block::CLAY;
    let depth = if clay { 1 } else { 2 };
    for bx in (x - rad)..=(x + rad) {
        for bz in (z - rad)..=(z + rad) {
            let (dx, dz) = (bx - x, bz - z);
            if dx * dx + dz * dz <= rad * rad {
                for by in (y - depth)..=(y + depth) {
                    let b = w.block(bx, by, bz);
                    let ok = if clay {
                        b == block::DIRT || b == block::CLAY
                    } else {
                        b == block::DIRT || b == block::GRASS
                    };
                    if ok {
                        w.set_block(bx, by, bz, id, 0, 2);
                    }
                }
            }
        }
    }
    true
}

/// `WorldGenLiquids(block)`: springs.
pub fn spring(w: &mut ServerWorld, r: &mut JavaRandom, x: i32, y: i32, z: i32, id: u8) -> bool {
    if w.block(x, y + 1, z) != block::STONE {
        return false;
    }
    if w.block(x, y - 1, z) != block::STONE {
        return false;
    }
    if w.material(x, y, z) != Material::Air && w.block(x, y, z) != block::STONE {
        return false;
    }
    let mut stone = 0;
    let mut air = 0;
    for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
        if w.block(x + dx, y, z + dz) == block::STONE {
            stone += 1;
        }
    }
    for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
        if w.is_air(x + dx, y, z + dz) {
            air += 1;
        }
    }
    if stone == 3 && air == 1 {
        w.set_block(x, y, z, id, 0, 2);
        w.scheduled_immediate = true;
        w.update_tick(id, x, y, z, r);
        w.scheduled_immediate = false;
    }
    true
}

/// `WorldGenFlowers`: 64 scattered attempts.
pub fn flowers(
    w: &mut ServerWorld,
    r: &mut JavaRandom,
    x: i32,
    y: i32,
    z: i32,
    id: u8,
    meta: i32,
) -> bool {
    for _ in 0..64 {
        crate::world::gen::hell::sync_rng(w, r);
        let bx = x + r.next_int_n(8) - r.next_int_n(8);
        let by = y + r.next_int_n(4) - r.next_int_n(4);
        let bz = z + r.next_int_n(8) - r.next_int_n(8);
        if w.is_air(bx, by, bz) && w.can_block_stay(id, bx, by, bz) {
            w.set_block(bx, by, bz, id, meta, 2);
        }
    }
    true
}

/// `WorldGenTallGrass(tallgrass, meta)`
pub fn tall_grass(
    w: &mut ServerWorld,
    r: &mut JavaRandom,
    x: i32,
    mut y: i32,
    z: i32,
    meta: i32,
) -> bool {
    while air_or_leaves(w.block(x, y, z)) && y > 0 {
        y -= 1;
    }
    for _ in 0..128 {
        let bx = x + r.next_int_n(8) - r.next_int_n(8);
        let by = y + r.next_int_n(4) - r.next_int_n(4);
        let bz = z + r.next_int_n(8) - r.next_int_n(8);
        if w.is_air(bx, by, bz) && w.can_block_stay(block::TALLGRASS, bx, by, bz) {
            w.set_block(bx, by, bz, block::TALLGRASS, meta, 2);
        }
    }
    true
}

/// `WorldGenDeadBush`
pub fn dead_bush(w: &mut ServerWorld, r: &mut JavaRandom, x: i32, mut y: i32, z: i32) -> bool {
    while air_or_leaves(w.block(x, y, z)) && y > 0 {
        y -= 1;
    }
    for _ in 0..4 {
        let bx = x + r.next_int_n(8) - r.next_int_n(8);
        let by = y + r.next_int_n(4) - r.next_int_n(4);
        let bz = z + r.next_int_n(8) - r.next_int_n(8);
        if w.is_air(bx, by, bz) && w.can_block_stay(block::DEADBUSH, bx, by, bz) {
            w.set_block(bx, by, bz, block::DEADBUSH, 0, 2);
        }
    }
    true
}

/// `WorldGenDoublePlant` with `func_150548_a(variant)`.
pub fn double_plant(
    w: &mut ServerWorld,
    r: &mut JavaRandom,
    x: i32,
    y: i32,
    z: i32,
    variant: i32,
) -> bool {
    let mut any = false;
    for _ in 0..64 {
        let bx = x + r.next_int_n(8) - r.next_int_n(8);
        let by = y + r.next_int_n(4) - r.next_int_n(4);
        let bz = z + r.next_int_n(8) - r.next_int_n(8);
        if w.is_air(bx, by, bz) && w.can_place_block_at(block::DOUBLE_PLANT, bx, by, bz) {
            // BlockDoublePlant.func_149889_c
            w.set_block(bx, by, bz, block::DOUBLE_PLANT, variant, 2);
            w.set_block(bx, by + 1, bz, block::DOUBLE_PLANT, 8, 2);
            any = true;
        }
    }
    any
}

/// `WorldGenWaterlily`
pub fn waterlily(w: &mut ServerWorld, r: &mut JavaRandom, x: i32, y: i32, z: i32) -> bool {
    for _ in 0..10 {
        let bx = x + r.next_int_n(8) - r.next_int_n(8);
        let by = y + r.next_int_n(4) - r.next_int_n(4);
        let bz = z + r.next_int_n(8) - r.next_int_n(8);
        if w.is_air(bx, by, bz) && w.can_place_block_at(block::WATERLILY, bx, by, bz) {
            w.set_block(bx, by, bz, block::WATERLILY, 0, 2);
        }
    }
    true
}

/// `WorldGenReed`
pub fn reed(w: &mut ServerWorld, r: &mut JavaRandom, x: i32, y: i32, z: i32) -> bool {
    for _ in 0..20 {
        let bx = x + r.next_int_n(4) - r.next_int_n(4);
        let bz = z + r.next_int_n(4) - r.next_int_n(4);
        if w.is_air(bx, y, bz)
            && (w.material(bx - 1, y - 1, bz) == Material::Water
                || w.material(bx + 1, y - 1, bz) == Material::Water
                || w.material(bx, y - 1, bz - 1) == Material::Water
                || w.material(bx, y - 1, bz + 1) == Material::Water)
        {
            let inner = r.next_int_n(3) + 1;
            let h = 2 + r.next_int_n(inner);
            for i in 0..h {
                if w.can_block_stay(block::REEDS, bx, y + i, bz) {
                    w.set_block(bx, y + i, bz, block::REEDS, 0, 2);
                }
            }
        }
    }
    true
}

/// `WorldGenCactus`
pub fn cactus(w: &mut ServerWorld, r: &mut JavaRandom, x: i32, y: i32, z: i32) -> bool {
    for _ in 0..10 {
        let bx = x + r.next_int_n(8) - r.next_int_n(8);
        let by = y + r.next_int_n(4) - r.next_int_n(4);
        let bz = z + r.next_int_n(8) - r.next_int_n(8);
        if w.is_air(bx, by, bz) {
            let inner = r.next_int_n(3) + 1;
            let h = 1 + r.next_int_n(inner);
            for i in 0..h {
                if w.can_block_stay(block::CACTUS, bx, by + i, bz) {
                    w.set_block(bx, by + i, bz, block::CACTUS, 0, 2);
                }
            }
        }
    }
    true
}

/// `WorldGenPumpkin`
pub fn pumpkin(w: &mut ServerWorld, r: &mut JavaRandom, x: i32, y: i32, z: i32) -> bool {
    for _ in 0..64 {
        let bx = x + r.next_int_n(8) - r.next_int_n(8);
        let by = y + r.next_int_n(4) - r.next_int_n(4);
        let bz = z + r.next_int_n(8) - r.next_int_n(8);
        if w.is_air(bx, by, bz)
            && w.block(bx, by - 1, bz) == block::GRASS
            && w.can_place_block_at(block::PUMPKIN, bx, by, bz)
        {
            let m = r.next_int_n(4);
            w.set_block(bx, by, bz, block::PUMPKIN, m, 2);
        }
    }
    true
}

/// `WorldGenMelon`
pub fn melon(w: &mut ServerWorld, r: &mut JavaRandom, x: i32, y: i32, z: i32) -> bool {
    for _ in 0..64 {
        let bx = x + r.next_int_n(8) - r.next_int_n(8);
        let by = y + r.next_int_n(4) - r.next_int_n(4);
        let bz = z + r.next_int_n(8) - r.next_int_n(8);
        if w.can_place_block_at(block::MELON_BLOCK, bx, by, bz)
            && w.block(bx, by - 1, bz) == block::GRASS
        {
            w.set_block(bx, by, bz, block::MELON_BLOCK, 0, 2);
        }
    }
    true
}

/// `WorldGenVines`
pub fn vines(w: &mut ServerWorld, r: &mut JavaRandom, mut x: i32, mut y: i32, mut z: i32) -> bool {
    let (ox, oz) = (x, z);
    // Direction.facingToDirection[Facing.oppositeSide[side]] for sides 2..=5
    const DIR_FOR_SIDE: [i32; 6] = [0, 0, 0, 2, 3, 1];
    while y < 128 {
        if w.is_air(x, y, z) {
            for side in 2..=5usize {
                let ok = match side {
                    2 => w.vine_can_attach(x, y, z + 1),
                    3 => w.vine_can_attach(x, y, z - 1),
                    4 => w.vine_can_attach(x + 1, y, z),
                    _ => w.vine_can_attach(x - 1, y, z),
                };
                if ok {
                    w.set_block(x, y, z, block::VINE, 1 << DIR_FOR_SIDE[side], 2);
                    break;
                }
            }
        } else {
            x = ox + r.next_int_n(4) - r.next_int_n(4);
            z = oz + r.next_int_n(4) - r.next_int_n(4);
        }
        y += 1;
    }
    true
}

fn dark_leaf(w: &mut ServerWorld, x: i32, y: i32, z: i32) {
    if w.material(x, y, z) == Material::Air {
        w.set_block(x, y, z, block::LEAVES2, 1, 2);
    }
}

/// `WorldGenCanopyTree`: dark oak.
pub fn canopy(w: &mut ServerWorld, r: &mut JavaRandom, x: i32, y: i32, z: i32) -> bool {
    let h = r.next_int_n(3) + r.next_int_n(2) + 6;
    if !(y >= 1 && y + h + 1 <= 256) {
        return false;
    }
    let mut ok = true;
    for yy in y..=y + 1 + h {
        let mut rad = 1;
        if yy == y {
            rad = 0;
        }
        if yy >= y + 1 + h - 2 {
            rad = 2;
        }
        let mut xx = x - rad;
        while xx <= x + rad && ok {
            let mut zz = z - rad;
            while zz <= z + rad && ok {
                if (0..256).contains(&yy) {
                    if !can_grow_into(w.block(xx, yy, zz)) {
                        ok = false;
                    }
                } else {
                    ok = false;
                }
                zz += 1;
            }
            xx += 1;
        }
    }
    if !ok {
        return false;
    }
    let below = w.block(x, y - 1, z);
    if !((below == block::GRASS || below == block::DIRT) && y < 256 - h - 1) {
        return false;
    }
    for (dx, dz) in [(0, 0), (1, 0), (1, 1), (0, 1)] {
        w.set_block(x + dx, y - 1, z + dz, block::DIRT, 0, 2);
    }
    let dir = r.next_int_n(4) as usize;
    let bend = h - r.next_int_n(4);
    let mut len = 2 - r.next_int_n(3);
    let (mut tx, mut tz) = (x, z);
    let mut top = 0;
    for i in 0..h {
        let yy = y + i;
        if i >= bend && len > 0 {
            tx += DIR_X[dir];
            tz += DIR_Z[dir];
            len -= 1;
        }
        if air_or_leaves(w.block(tx, yy, tz)) {
            for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                // MC-10176: only the first of the four trunk logs is checked.
                if crate::compat::vanilla_parity() || can_grow_into(w.block(tx + dx, yy, tz + dz)) {
                    w.set_block(tx + dx, yy, tz + dz, block::LOG2, 1, 2);
                }
            }
            top = yy;
        }
    }
    for i in -2..=0 {
        for j in -2..=0 {
            let mut k = -1;
            dark_leaf(w, tx + i, top + k, tz + j);
            dark_leaf(w, 1 + tx - i, top + k, tz + j);
            dark_leaf(w, tx + i, top + k, 1 + tz - j);
            dark_leaf(w, 1 + tx - i, top + k, 1 + tz - j);
            if (i > -2 || j > -1) && (i != -1 || j != -2) {
                k = 1;
                dark_leaf(w, tx + i, top + k, tz + j);
                dark_leaf(w, 1 + tx - i, top + k, tz + j);
                dark_leaf(w, tx + i, top + k, 1 + tz - j);
                dark_leaf(w, 1 + tx - i, top + k, 1 + tz - j);
            }
        }
    }
    if r.next_boolean() {
        dark_leaf(w, tx, top + 2, tz);
        dark_leaf(w, tx + 1, top + 2, tz);
        dark_leaf(w, tx + 1, top + 2, tz + 1);
        dark_leaf(w, tx, top + 2, tz + 1);
    }
    for i in -3..=4i32 {
        for j in -3..=4i32 {
            if (i != -3 || j != -3)
                && (i != -3 || j != 4)
                && (i != 4 || j != -3)
                && (i != 4 || j != 4)
                && (i.abs() < 3 || j.abs() < 3)
            {
                dark_leaf(w, tx + i, top, tz + j);
            }
        }
    }
    for i in -1..=2 {
        for j in -1..=2 {
            if (i < 0 || i > 1 || j < 0 || j > 1) && r.next_int_n(3) <= 0 {
                let n = r.next_int_n(3) + 2;
                for k in 0..n {
                    if crate::compat::vanilla_parity()
                        || can_grow_into(w.block(x + i, top - k - 1, z + j))
                    {
                        w.set_block(x + i, top - k - 1, z + j, block::LOG2, 1, 2);
                    }
                }
                for a in -1..=1 {
                    for b in -1..=1 {
                        dark_leaf(w, tx + i + a, top, tz + j + b);
                    }
                }
                for a in -2..=2i32 {
                    for b in -2..=2i32 {
                        if a.abs() != 2 || b.abs() != 2 {
                            dark_leaf(w, tx + i + a, top - 1, tz + j + b);
                        }
                    }
                }
            }
        }
    }
    true
}

/// `WorldGenBigMushroom(type)`; `kind < 0` picks randomly.
pub fn big_mushroom(
    w: &mut ServerWorld,
    r: &mut JavaRandom,
    x: i32,
    y: i32,
    z: i32,
    kind: i32,
) -> bool {
    let mut ty = r.next_int_n(2);
    if kind >= 0 {
        ty = kind;
    }
    let h = r.next_int_n(3) + 4;
    if !(y >= 1 && y + h + 1 < 256) {
        return false;
    }
    let mut ok = true;
    for yy in y..=y + 1 + h {
        let rad = if yy <= y + 3 { 0 } else { 3 };
        let mut xx = x - rad;
        while xx <= x + rad && ok {
            let mut zz = z - rad;
            while zz <= z + rad && ok {
                if (0..256).contains(&yy) {
                    if !air_or_leaves(w.block(xx, yy, zz)) {
                        ok = false;
                    }
                } else {
                    ok = false;
                }
                zz += 1;
            }
            xx += 1;
        }
    }
    if !ok {
        return false;
    }
    let below = w.block(x, y - 1, z);
    if below != block::DIRT && below != block::GRASS && below != block::MYCELIUM {
        return false;
    }
    let id = block::BROWN_MUSHROOM_BLOCK + ty as u8;
    let mut start = y + h;
    if ty == 1 {
        start = y + h - 3;
    }
    for yy in start..=y + h {
        let mut rad = 1;
        if yy < y + h {
            rad += 1;
        }
        if ty == 0 {
            rad = 3;
        }
        for xx in (x - rad)..=(x + rad) {
            for zz in (z - rad)..=(z + rad) {
                let mut m = 5;
                if xx == x - rad {
                    m -= 1;
                }
                if xx == x + rad {
                    m += 1;
                }
                if zz == z - rad {
                    m -= 3;
                }
                if zz == z + rad {
                    m += 3;
                }
                if ty == 0 || yy < y + h {
                    if (xx == x - rad || xx == x + rad) && (zz == z - rad || zz == z + rad) {
                        continue;
                    }
                    if xx == x - (rad - 1) && zz == z - rad {
                        m = 1;
                    }
                    if xx == x - rad && zz == z - (rad - 1) {
                        m = 1;
                    }
                    if xx == x + (rad - 1) && zz == z - rad {
                        m = 3;
                    }
                    if xx == x + rad && zz == z - (rad - 1) {
                        m = 3;
                    }
                    if xx == x - (rad - 1) && zz == z + rad {
                        m = 7;
                    }
                    if xx == x - rad && zz == z + (rad - 1) {
                        m = 7;
                    }
                    if xx == x + (rad - 1) && zz == z + rad {
                        m = 9;
                    }
                    if xx == x + rad && zz == z + (rad - 1) {
                        m = 9;
                    }
                }
                if m == 5 && yy < y + h {
                    m = 0;
                }
                if m != 0 && !block::info(w.block(xx, yy, zz)).full_block {
                    w.set_block(xx, yy, zz, id, m, 2);
                }
            }
        }
    }
    for i in 0..h {
        if !block::info(w.block(x, y + i, z)).full_block {
            w.set_block(x, y + i, z, id, 10, 2);
        }
    }
    true
}

/// `WorldGenBlockBlob(block, radius)`
pub fn block_blob(
    w: &mut ServerWorld,
    r: &mut JavaRandom,
    mut x: i32,
    mut y: i32,
    mut z: i32,
    id: u8,
    radius: i32,
) -> bool {
    while y > 3 {
        if !w.is_air(x, y - 1, z) {
            let b = w.block(x, y - 1, z);
            if b == block::GRASS || b == block::DIRT || b == block::STONE {
                break;
            }
        }
        y -= 1;
    }
    if y <= 3 {
        return false;
    }
    let rad = radius;
    let mut i = 0;
    while rad >= 0 && i < 3 {
        let rx = rad + r.next_int_n(2);
        let ry = rad + r.next_int_n(2);
        let rz = rad + r.next_int_n(2);
        let f = (rx + ry + rz) as f32 * 0.333f32 + 0.5f32;
        for bx in (x - rx)..=(x + rx) {
            for bz in (z - rz)..=(z + rz) {
                for by in (y - ry)..=(y + ry) {
                    let (dx, dz, dy) = ((bx - x) as f32, (bz - z) as f32, (by - y) as f32);
                    if !(dx * dx + dz * dz + dy * dy > f * f) {
                        w.set_block(bx, by, bz, id, 0, 4);
                    }
                }
            }
        }
        x += -(rad + 1) + r.next_int_n(2 + rad * 2);
        z += -(rad + 1) + r.next_int_n(2 + rad * 2);
        y += 0 - r.next_int_n(2);
        i += 1;
    }
    true
}

/// `WorldGenDesertWells`
pub fn desert_well(w: &mut ServerWorld, _r: &mut JavaRandom, x: i32, mut y: i32, z: i32) -> bool {
    while w.is_air(x, y, z) && y > 2 {
        y -= 1;
    }
    if w.block(x, y, z) != block::SAND {
        return false;
    }
    for i in -2..=2 {
        for j in -2..=2 {
            if w.is_air(x + i, y - 1, z + j) && w.is_air(x + i, y - 2, z + j) {
                return false;
            }
        }
    }
    for k in -1..=0 {
        for i in -2..=2 {
            for j in -2..=2 {
                w.set_block(x + i, y + k, z + j, block::SANDSTONE, 0, 2);
            }
        }
    }
    for (dx, dz) in [(0, 0), (-1, 0), (1, 0), (0, -1), (0, 1)] {
        w.set_block(x + dx, y, z + dz, block::FLOWING_WATER, 0, 2);
    }
    for i in -2..=2i32 {
        for j in -2..=2i32 {
            if i == -2 || i == 2 || j == -2 || j == 2 {
                w.set_block(x + i, y + 1, z + j, block::SANDSTONE, 0, 2);
            }
        }
    }
    for (dx, dz) in [(2, 0), (-2, 0), (0, 2), (0, -2)] {
        w.set_block(x + dx, y + 1, z + dz, block::STONE_SLAB, 1, 2);
    }
    for i in -1..=1 {
        for j in -1..=1 {
            if i == 0 && j == 0 {
                w.set_block(x + i, y + 4, z + j, block::SANDSTONE, 0, 2);
            } else {
                w.set_block(x + i, y + 4, z + j, block::STONE_SLAB, 1, 2);
            }
        }
    }
    for k in 1..=3 {
        for (dx, dz) in [(-1, -1), (-1, 1), (1, -1), (1, 1)] {
            w.set_block(x + dx, y + k, z + dz, block::SANDSTONE, 0, 2);
        }
    }
    true
}

/// `WorldGenIcePath(width)`
pub fn ice_path(
    w: &mut ServerWorld,
    r: &mut JavaRandom,
    x: i32,
    mut y: i32,
    z: i32,
    width: i32,
) -> bool {
    while w.is_air(x, y, z) && y > 2 {
        y -= 1;
    }
    if w.block(x, y, z) != block::SNOW {
        return false;
    }
    let rad = r.next_int_n(width - 2) + 2;
    for bx in (x - rad)..=(x + rad) {
        for bz in (z - rad)..=(z + rad) {
            let (dx, dz) = (bx - x, bz - z);
            if dx * dx + dz * dz <= rad * rad {
                for by in (y - 1)..=(y + 1) {
                    let b = w.block(bx, by, bz);
                    if b == block::DIRT || b == block::SNOW || b == block::ICE {
                        w.set_block(bx, by, bz, block::PACKED_ICE, 0, 2);
                    }
                }
            }
        }
    }
    true
}

/// `WorldGenIceSpike`
pub fn ice_spike(w: &mut ServerWorld, r: &mut JavaRandom, x: i32, mut y: i32, z: i32) -> bool {
    while w.is_air(x, y, z) && y > 2 {
        y -= 1;
    }
    if w.block(x, y, z) != block::SNOW {
        return false;
    }
    y += r.next_int_n(4);
    let h = r.next_int_n(4) + 7;
    let rad = h / 4 + r.next_int_n(2);
    if rad > 1 && r.next_int_n(60) == 0 {
        y += 10 + r.next_int_n(30);
    }
    let replace = |b: u8| {
        block::material(b) == Material::Air
            || b == block::DIRT
            || b == block::SNOW
            || b == block::ICE
    };
    for i in 0..h {
        let f = (1.0f32 - i as f32 / h as f32) * rad as f32;
        let c = f.ceil() as i32;
        for dx in -c..=c {
            let fx = dx.abs() as f32 - 0.25f32;
            for dz in -c..=c {
                let fz = dz.abs() as f32 - 0.25f32;
                if (dx == 0 && dz == 0 || !(fx * fx + fz * fz > f * f))
                    && (dx != -c && dx != c && dz != -c && dz != c || !(r.next_float() > 0.75f32))
                {
                    if replace(w.block(x + dx, y + i, z + dz)) {
                        w.set_block(x + dx, y + i, z + dz, block::PACKED_ICE, 0, 2);
                    }
                    if i != 0 && c > 1 && replace(w.block(x + dx, y - i, z + dz)) {
                        w.set_block(x + dx, y - i, z + dz, block::PACKED_ICE, 0, 2);
                    }
                }
            }
        }
    }
    let base = (rad - 1).clamp(0, 1);
    for dx in -base..=base {
        for dz in -base..=base {
            let mut yy = y - 1;
            let mut run = 50;
            if dx.abs() == 1 && dz.abs() == 1 {
                run = r.next_int_n(5);
            }
            while yy > 50 {
                let b = w.block(x + dx, yy, z + dz);
                if block::material(b) != Material::Air
                    && b != block::DIRT
                    && b != block::SNOW
                    && b != block::ICE
                    && b != block::PACKED_ICE
                {
                    break;
                }
                w.set_block(x + dx, yy, z + dz, block::PACKED_ICE, 0, 2);
                yy -= 1;
                run -= 1;
                if run <= 0 {
                    yy -= r.next_int_n(5) + 1;
                    run = r.next_int_n(5);
                }
            }
        }
    }
    true
}
