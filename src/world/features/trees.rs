//! Tree generators (`WorldGenTrees`, `WorldGenForest`, `WorldGenTaiga1/2`,
//! `WorldGenShrub`, `WorldGenSwamp`, `WorldGenSavannaTree`).

use crate::block::{self, Material};
use crate::java::JavaRandom;
use crate::world::server::ServerWorld;

/// `Direction.offsetX` / `offsetZ` / `rotateOpposite`
pub const DIR_X: [i32; 4] = [0, -1, 0, 1];
pub const DIR_Z: [i32; 4] = [1, 0, -1, 0];
pub const ROTATE_OPPOSITE: [usize; 4] = [2, 3, 0, 1];

/// `WorldGenAbstractTree.func_150523_a`
pub fn can_grow_into(id: u8) -> bool {
    let m = block::material(id);
    m == Material::Air || m == Material::Leaves || id == block::GRASS || id == block::DIRT || id == block::LOG || id == block::LOG2 || id == 6 || id == block::VINE
}

#[inline]
fn air_or_leaves(id: u8) -> bool {
    let m = block::material(id);
    m == Material::Air || m == Material::Leaves
}

#[inline]
fn set(w: &mut ServerWorld, x: i32, y: i32, z: i32, id: u8, meta: i32) {
    w.set_block(x, y, z, id, meta, 2);
}

/// Shared clearance check: radius 0 at the base, 1 along the trunk, `top` for
/// the upper two layers.
fn clearance(w: &mut ServerWorld, x: i32, y: i32, z: i32, h: i32, top: i32, test: impl Fn(u8) -> bool) -> bool {
    let mut ok = true;
    for yy in y..=y + 1 + h {
        let mut r = 1;
        if yy == y {
            r = 0;
        }
        if yy >= y + 1 + h - 2 {
            r = top;
        }
        let mut xx = x - r;
        while xx <= x + r && ok {
            let mut zz = z - r;
            while zz <= z + r && ok {
                if (0..256).contains(&yy) {
                    if !test(w.block(xx, yy, zz)) {
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
    ok
}

fn grow_vines(w: &mut ServerWorld, x: i32, mut y: i32, z: i32, meta: i32) {
    set(w, x, y, z, block::VINE, meta);
    let mut n = 4;
    loop {
        y -= 1;
        if !(w.material(x, y, z) == Material::Air && n > 0) {
            break;
        }
        set(w, x, y, z, block::VINE, meta);
        n -= 1;
    }
}

/// `WorldGenTrees(false, min_height, wood_meta, leaf_meta, vines)`
pub fn trees(w: &mut ServerWorld, r: &mut JavaRandom, x: i32, y: i32, z: i32, min_height: i32, wood: i32, leaf: i32, vines: bool) -> bool {
    let h = r.next_int_n(3) + min_height;
    if !(y >= 1 && y + h + 1 <= 256) {
        return false;
    }
    if !clearance(w, x, y, z, h, 2, can_grow_into) {
        return false;
    }
    let below = w.block(x, y - 1, z);
    if !((below == block::GRASS || below == block::DIRT || below == block::FARMLAND) && y < 256 - h - 1) {
        return false;
    }
    set(w, x, y - 1, z, block::DIRT, 0);
    for yy in (y - 3 + h)..=(y + h) {
        let k = yy - (y + h);
        let rad = 1 - k / 2;
        for xx in (x - rad)..=(x + rad) {
            let dx = xx - x;
            for zz in (z - rad)..=(z + rad) {
                let dz = zz - z;
                if dx.abs() != rad || dz.abs() != rad || r.next_int_n(2) != 0 && k != 0 {
                    if air_or_leaves(w.block(xx, yy, zz)) {
                        set(w, xx, yy, zz, block::LEAVES, leaf);
                    }
                }
            }
        }
    }
    for i in 0..h {
        if air_or_leaves(w.block(x, y + i, z)) {
            set(w, x, y + i, z, block::LOG, wood);
            if vines && i > 0 {
                if r.next_int_n(3) > 0 && w.is_air(x - 1, y + i, z) {
                    set(w, x - 1, y + i, z, block::VINE, 8);
                }
                if r.next_int_n(3) > 0 && w.is_air(x + 1, y + i, z) {
                    set(w, x + 1, y + i, z, block::VINE, 2);
                }
                if r.next_int_n(3) > 0 && w.is_air(x, y + i, z - 1) {
                    set(w, x, y + i, z - 1, block::VINE, 1);
                }
                if r.next_int_n(3) > 0 && w.is_air(x, y + i, z + 1) {
                    set(w, x, y + i, z + 1, block::VINE, 4);
                }
            }
        }
    }
    if vines {
        for yy in (y - 3 + h)..=(y + h) {
            let k = yy - (y + h);
            let rad = 2 - k / 2;
            for xx in (x - rad)..=(x + rad) {
                for zz in (z - rad)..=(z + rad) {
                    if w.material(xx, yy, zz) == Material::Leaves {
                        if r.next_int_n(4) == 0 && w.material(xx - 1, yy, zz) == Material::Air {
                            grow_vines(w, xx - 1, yy, zz, 8);
                        }
                        if r.next_int_n(4) == 0 && w.material(xx + 1, yy, zz) == Material::Air {
                            grow_vines(w, xx + 1, yy, zz, 2);
                        }
                        if r.next_int_n(4) == 0 && w.material(xx, yy, zz - 1) == Material::Air {
                            grow_vines(w, xx, yy, zz - 1, 1);
                        }
                        if r.next_int_n(4) == 0 && w.material(xx, yy, zz + 1) == Material::Air {
                            grow_vines(w, xx, yy, zz + 1, 4);
                        }
                    }
                }
            }
        }
        if r.next_int_n(5) == 0 && h > 5 {
            for i in 0..2 {
                for d in 0..4usize {
                    if r.next_int_n(4 - i) == 0 {
                        let age = r.next_int_n(3);
                        let o = ROTATE_OPPOSITE[d];
                        set(w, x + DIR_X[o], y + h - 5 + i, z + DIR_Z[o], block::COCOA, age << 2 | d as i32);
                    }
                }
            }
        }
    }
    true
}

/// `WorldGenForest(false, tall)`: birch.
pub fn birch(w: &mut ServerWorld, r: &mut JavaRandom, x: i32, y: i32, z: i32, tall: bool) -> bool {
    let mut h = r.next_int_n(3) + 5;
    if tall {
        h += r.next_int_n(7);
    }
    if !(y >= 1 && y + h + 1 <= 256) {
        return false;
    }
    if !clearance(w, x, y, z, h, 2, can_grow_into) {
        return false;
    }
    let below = w.block(x, y - 1, z);
    if !((below == block::GRASS || below == block::DIRT || below == block::FARMLAND) && y < 256 - h - 1) {
        return false;
    }
    set(w, x, y - 1, z, block::DIRT, 0);
    for yy in (y - 3 + h)..=(y + h) {
        let k = yy - (y + h);
        let rad = 1 - k / 2;
        for xx in (x - rad)..=(x + rad) {
            let dx = xx - x;
            for zz in (z - rad)..=(z + rad) {
                let dz = zz - z;
                if dx.abs() != rad || dz.abs() != rad || r.next_int_n(2) != 0 && k != 0 {
                    if air_or_leaves(w.block(xx, yy, zz)) {
                        set(w, xx, yy, zz, block::LEAVES, 2);
                    }
                }
            }
        }
    }
    for i in 0..h {
        if air_or_leaves(w.block(x, y + i, z)) {
            set(w, x, y + i, z, block::LOG, 2);
        }
    }
    true
}

fn conifer_clearance(w: &mut ServerWorld, x: i32, y: i32, z: i32, h: i32, bare: i32, rad: i32, test: impl Fn(u8) -> bool) -> bool {
    let mut ok = true;
    let mut yy = y;
    while yy <= y + 1 + h && ok {
        let r = if yy - y < bare { 0 } else { rad };
        let mut xx = x - r;
        while xx <= x + r && ok {
            let mut zz = z - r;
            while zz <= z + r && ok {
                if (0..256).contains(&yy) {
                    if !test(w.block(xx, yy, zz)) {
                        ok = false;
                    }
                } else {
                    ok = false;
                }
                zz += 1;
            }
            xx += 1;
        }
        yy += 1;
    }
    ok
}

/// `WorldGenTaiga1`: pine.
pub fn taiga1(w: &mut ServerWorld, r: &mut JavaRandom, x: i32, y: i32, z: i32) -> bool {
    let h = r.next_int_n(5) + 7;
    let bare = h - r.next_int_n(2) - 3;
    let crown = h - bare;
    let rad = 1 + r.next_int_n(crown + 1);
    if !(y >= 1 && y + h + 1 <= 256) {
        return false;
    }
    if !conifer_clearance(w, x, y, z, h, bare, rad, can_grow_into) {
        return false;
    }
    let below = w.block(x, y - 1, z);
    if !((below == block::GRASS || below == block::DIRT) && y < 256 - h - 1) {
        return false;
    }
    set(w, x, y - 1, z, block::DIRT, 0);
    let mut cur = 0;
    let mut yy = y + h;
    while yy >= y + bare {
        for xx in (x - cur)..=(x + cur) {
            let dx = xx - x;
            for zz in (z - cur)..=(z + cur) {
                let dz = zz - z;
                if (dx.abs() != cur || dz.abs() != cur || cur <= 0) && !block::info(w.block(xx, yy, zz)).full_block {
                    set(w, xx, yy, zz, block::LEAVES, 1);
                }
            }
        }
        if cur >= 1 && yy == y + bare + 1 {
            cur -= 1;
        } else if cur < rad {
            cur += 1;
        }
        yy -= 1;
    }
    for i in 0..h - 1 {
        if air_or_leaves(w.block(x, y + i, z)) {
            set(w, x, y + i, z, block::LOG, 1);
        }
    }
    true
}

/// `WorldGenTaiga2`: spruce.
pub fn taiga2(w: &mut ServerWorld, r: &mut JavaRandom, x: i32, y: i32, z: i32) -> bool {
    let h = r.next_int_n(4) + 6;
    let bare = 1 + r.next_int_n(2);
    let crown = h - bare;
    let rad = 2 + r.next_int_n(2);
    if !(y >= 1 && y + h + 1 <= 256) {
        return false;
    }
    if !conifer_clearance(w, x, y, z, h, bare, rad, air_or_leaves) {
        return false;
    }
    let below = w.block(x, y - 1, z);
    if !((below == block::GRASS || below == block::DIRT || below == block::FARMLAND) && y < 256 - h - 1) {
        return false;
    }
    set(w, x, y - 1, z, block::DIRT, 0);
    let mut cur = r.next_int_n(2);
    let mut limit = 1;
    let mut reset = 0;
    for i in 0..=crown {
        let yy = y + h - i;
        for xx in (x - cur)..=(x + cur) {
            let dx = xx - x;
            for zz in (z - cur)..=(z + cur) {
                let dz = zz - z;
                if (dx.abs() != cur || dz.abs() != cur || cur <= 0) && !block::info(w.block(xx, yy, zz)).full_block {
                    set(w, xx, yy, zz, block::LEAVES, 1);
                }
            }
        }
        if cur >= limit {
            cur = reset;
            reset = 1;
            limit += 1;
            if limit > rad {
                limit = rad;
            }
        } else {
            cur += 1;
        }
    }
    let top = r.next_int_n(3);
    for i in 0..h - top {
        if air_or_leaves(w.block(x, y + i, z)) {
            set(w, x, y + i, z, block::LOG, 1);
        }
    }
    true
}

/// `WorldGenShrub(wood_meta, leaf_meta)`
pub fn shrub(w: &mut ServerWorld, r: &mut JavaRandom, x: i32, mut y: i32, z: i32, wood: i32, leaf: i32) -> bool {
    while air_or_leaves(w.block(x, y, z)) && y > 0 {
        y -= 1;
    }
    let b = w.block(x, y, z);
    if b == block::DIRT || b == block::GRASS {
        y += 1;
        set(w, x, y, z, block::LOG, wood);
        for yy in y..=y + 2 {
            let k = yy - y;
            let rad = 2 - k;
            for xx in (x - rad)..=(x + rad) {
                let dx = xx - x;
                for zz in (z - rad)..=(z + rad) {
                    let dz = zz - z;
                    if (dx.abs() != rad || dz.abs() != rad || r.next_int_n(2) != 0) && !block::info(w.block(xx, yy, zz)).full_block {
                        set(w, xx, yy, zz, block::LEAVES, leaf);
                    }
                }
            }
        }
    }
    true
}

/// `WorldGenSwamp`
pub fn swamp(w: &mut ServerWorld, r: &mut JavaRandom, x: i32, mut y: i32, z: i32) -> bool {
    let h = r.next_int_n(4) + 5;
    while w.material(x, y - 1, z) == Material::Water {
        y -= 1;
    }
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
            rad = 3;
        }
        let mut xx = x - rad;
        while xx <= x + rad && ok {
            let mut zz = z - rad;
            while zz <= z + rad && ok {
                if (0..256).contains(&yy) {
                    let b = w.block(xx, yy, zz);
                    if !air_or_leaves(b) {
                        if b != block::WATER && b != block::FLOWING_WATER {
                            ok = false;
                        } else if yy > y {
                            ok = false;
                        }
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
    set(w, x, y - 1, z, block::DIRT, 0);
    for yy in (y - 3 + h)..=(y + h) {
        let k = yy - (y + h);
        let rad = 2 - k / 2;
        for xx in (x - rad)..=(x + rad) {
            let dx = xx - x;
            for zz in (z - rad)..=(z + rad) {
                let dz = zz - z;
                if (dx.abs() != rad || dz.abs() != rad || r.next_int_n(2) != 0 && k != 0) && !block::info(w.block(xx, yy, zz)).full_block {
                    set(w, xx, yy, zz, block::LEAVES, 0);
                }
            }
        }
    }
    for i in 0..h {
        let b = w.block(x, y + i, z);
        if air_or_leaves(b) || b == block::FLOWING_WATER || b == block::WATER {
            set(w, x, y + i, z, block::LOG, 0);
        }
    }
    for yy in (y - 3 + h)..=(y + h) {
        let k = yy - (y + h);
        let rad = 2 - k / 2;
        for xx in (x - rad)..=(x + rad) {
            for zz in (z - rad)..=(z + rad) {
                if w.material(xx, yy, zz) == Material::Leaves {
                    if r.next_int_n(4) == 0 && w.material(xx - 1, yy, zz) == Material::Air {
                        grow_vines(w, xx - 1, yy, zz, 8);
                    }
                    if r.next_int_n(4) == 0 && w.material(xx + 1, yy, zz) == Material::Air {
                        grow_vines(w, xx + 1, yy, zz, 2);
                    }
                    if r.next_int_n(4) == 0 && w.material(xx, yy, zz - 1) == Material::Air {
                        grow_vines(w, xx, yy, zz - 1, 1);
                    }
                    if r.next_int_n(4) == 0 && w.material(xx, yy, zz + 1) == Material::Air {
                        grow_vines(w, xx, yy, zz + 1, 4);
                    }
                }
            }
        }
    }
    true
}

fn acacia_leaf(w: &mut ServerWorld, x: i32, y: i32, z: i32) {
    if air_or_leaves(w.block(x, y, z)) {
        set(w, x, y, z, block::LEAVES2, 0);
    }
}

/// `WorldGenSavannaTree`
pub fn savanna(w: &mut ServerWorld, r: &mut JavaRandom, x: i32, y: i32, z: i32) -> bool {
    let h = r.next_int_n(3) + r.next_int_n(3) + 5;
    if !(y >= 1 && y + h + 1 <= 256) {
        return false;
    }
    if !clearance(w, x, y, z, h, 2, can_grow_into) {
        return false;
    }
    let below = w.block(x, y - 1, z);
    if !((below == block::GRASS || below == block::DIRT) && y < 256 - h - 1) {
        return false;
    }
    set(w, x, y - 1, z, block::DIRT, 0);
    let dir = r.next_int_n(4);
    let bend = h - r.next_int_n(4) - 1;
    let mut len = 3 - r.next_int_n(3);
    let (mut tx, mut tz) = (x, z);
    let mut top = 0;
    for i in 0..h {
        let yy = y + i;
        if i >= bend && len > 0 {
            tx += DIR_X[dir as usize];
            tz += DIR_Z[dir as usize];
            len -= 1;
        }
        if air_or_leaves(w.block(tx, yy, tz)) {
            set(w, tx, yy, tz, block::LOG2, 0);
            top = yy;
        }
    }
    for dx in -1..=1 {
        for dz in -1..=1 {
            acacia_leaf(w, tx + dx, top + 1, tz + dz);
        }
    }
    acacia_leaf(w, tx + 2, top + 1, tz);
    acacia_leaf(w, tx - 2, top + 1, tz);
    acacia_leaf(w, tx, top + 1, tz + 2);
    acacia_leaf(w, tx, top + 1, tz - 2);
    for dx in -3..=3i32 {
        for dz in -3..=3i32 {
            if dx.abs() != 3 || dz.abs() != 3 {
                acacia_leaf(w, tx + dx, top, tz + dz);
            }
        }
    }
    tx = x;
    tz = z;
    let dir2 = r.next_int_n(4);
    if dir2 != dir {
        let start = bend - r.next_int_n(2) - 1;
        let mut n = 1 + r.next_int_n(3);
        top = 0;
        let mut i = start;
        while i < h && n > 0 {
            if i >= 1 {
                let yy = y + i;
                tx += DIR_X[dir2 as usize];
                tz += DIR_Z[dir2 as usize];
                if air_or_leaves(w.block(tx, yy, tz)) {
                    set(w, tx, yy, tz, block::LOG2, 0);
                    top = yy;
                }
            }
            i += 1;
            n -= 1;
        }
        if top > 0 {
            for dx in -1..=1 {
                for dz in -1..=1 {
                    acacia_leaf(w, tx + dx, top + 1, tz + dz);
                }
            }
            for dx in -2..=2i32 {
                for dz in -2..=2i32 {
                    if dx.abs() != 2 || dz.abs() != 2 {
                        acacia_leaf(w, tx + dx, top, tz + dz);
                    }
                }
            }
        }
    }
    true
}
