//! `WorldGenBigTree`, `WorldGenHugeTrees`, `WorldGenMegaJungle`, `WorldGenMegaPineTree`.

use super::trees::can_grow_into;
use crate::block::{self, Material};
use crate::java::JavaRandom;
use crate::util::math;
use crate::world::server::ServerWorld;

const OTHER: [usize; 6] = [2, 0, 0, 1, 2, 1];

#[inline]
fn air_or_leaves(id: u8) -> bool {
    let m = block::material(id);
    m == Material::Air || m == Material::Leaves
}

#[inline]
fn set(w: &mut ServerWorld, x: i32, y: i32, z: i32, id: u8, meta: i32) {
    w.set_block(x, y, z, id, meta, 2);
}

struct BigTree<'a> {
    w: &'a mut ServerWorld,
    rand: JavaRandom,
    base: [i32; 3],
    height_limit: i32,
    height: i32,
    leaf_distance_limit: i32,
    leaf_nodes: Vec<[i32; 4]>,
}

impl BigTree<'_> {
    fn check_block_line(&mut self, a: [i32; 3], b: [i32; 3]) -> i32 {
        let mut d = [0i32; 3];
        let mut major = 0usize;
        for i in 0..3 {
            d[i] = b[i] - a[i];
            if d[i].abs() > d[major].abs() {
                major = i;
            }
        }
        if d[major] == 0 {
            return -1;
        }
        let (o1, o2) = (OTHER[major], OTHER[major + 3]);
        let step: i32 = if d[major] > 0 { 1 } else { -1 };
        let r1 = d[o1] as f64 / d[major] as f64;
        let r2 = d[o2] as f64 / d[major] as f64;
        let mut p = [0i32; 3];
        let mut i: i32 = 0;
        let end = d[major] + step;
        while i != end {
            p[major] = a[major] + i;
            p[o1] = math::floor_double(a[o1] as f64 + i as f64 * r1);
            p[o2] = math::floor_double(a[o2] as f64 + i as f64 * r2);
            if !can_grow_into(self.w.block(p[0], p[1], p[2])) {
                break;
            }
            i += step;
        }
        if i == end {
            -1
        } else {
            i.abs()
        }
    }

    fn layer_size(&self, y: i32) -> f32 {
        if (y as f64) < self.height_limit as f64 * 0.3 {
            return -1.618f32;
        }
        let half = self.height_limit as f32 / 2.0f32;
        let d = self.height_limit as f32 / 2.0f32 - y as f32;
        let r = if d == 0.0 {
            half
        } else if d.abs() >= half {
            0.0
        } else {
            ((half.abs() as f64).powf(2.0) - (d.abs() as f64).powf(2.0)).sqrt() as f32
        };
        r * 0.5f32
    }

    fn leaf_node_list(&mut self) {
        self.height = (self.height_limit as f64 * 0.618) as i32;
        if self.height >= self.height_limit {
            self.height = self.height_limit - 1;
        }
        let mut per_layer = (1.382 + (1.0 * self.height_limit as f64 / 13.0).powf(2.0)) as i32;
        if per_layer < 1 {
            per_layer = 1;
        }
        let mut y = self.base[1] + self.height_limit - self.leaf_distance_limit;
        let trunk_top = self.base[1] + self.height;
        let mut rel = y - self.base[1];
        let mut nodes = vec![[self.base[0], y, self.base[2], trunk_top]];
        y -= 1;
        while rel >= 0 {
            let size = self.layer_size(rel);
            if size < 0.0 {
                y -= 1;
                rel -= 1;
                continue;
            }
            for _ in 0..per_layer {
                let dist = 1.0 * (size as f64 * (self.rand.next_float() as f64 + 0.328));
                let ang = self.rand.next_float() as f64 * 2.0 * 3.14159;
                let nx = math::floor_double(dist * ang.sin() + self.base[0] as f64 + 0.5);
                let nz = math::floor_double(dist * ang.cos() + self.base[2] as f64 + 0.5);
                let p = [nx, y, nz];
                let top = [nx, y + self.leaf_distance_limit, nz];
                if self.check_block_line(p, top) == -1 {
                    let mut b = self.base;
                    let dx = (self.base[0] - p[0]).abs() as f64;
                    let dz = (self.base[2] - p[2]).abs() as f64;
                    let horiz = (dx.powf(2.0) + dz.powf(2.0)).sqrt();
                    let drop = horiz * 0.381;
                    if p[1] as f64 - drop > trunk_top as f64 {
                        b[1] = trunk_top;
                    } else {
                        b[1] = (p[1] as f64 - drop) as i32;
                    }
                    if self.check_block_line(b, p) == -1 {
                        nodes.push([nx, y, nz, b[1]]);
                    }
                }
            }
            y -= 1;
            rel -= 1;
        }
        self.leaf_nodes = nodes;
    }

    /// `func_150529_a` for axis 1 (leaf discs).
    fn leaf_disc(&mut self, x: i32, y: i32, z: i32, size: f32) {
        let r = (size as f64 + 0.618) as i32;
        for i in -r..=r {
            for j in -r..=r {
                let d = (i.abs() as f64 + 0.5).powf(2.0) + (j.abs() as f64 + 0.5).powf(2.0);
                if d > (size * size) as f64 {
                    continue;
                }
                // otherCoordPairs[1] = 0 (x), otherCoordPairs[4] = 2 (z)
                let (px, pz) = (x + i, z + j);
                if air_or_leaves(self.w.block(px, y, pz)) {
                    set(self.w, px, y, pz, block::LEAVES, 0);
                }
            }
        }
    }

    fn leaves(&mut self) {
        for n in self.leaf_nodes.clone() {
            for yy in n[1]..n[1] + self.leaf_distance_limit {
                let k = yy - n[1];
                let size = if k < 0 || k >= self.leaf_distance_limit {
                    -1.0f32
                } else if k != 0 && k != self.leaf_distance_limit - 1 {
                    3.0f32
                } else {
                    2.0f32
                };
                self.leaf_disc(n[0], yy, n[2], size);
            }
        }
    }

    fn branch(&mut self, a: [i32; 3], b: [i32; 3]) {
        let mut d = [0i32; 3];
        let mut major = 0usize;
        for i in 0..3 {
            d[i] = b[i] - a[i];
            if d[i].abs() > d[major].abs() {
                major = i;
            }
        }
        if d[major] == 0 {
            return;
        }
        let (o1, o2) = (OTHER[major], OTHER[major + 3]);
        let step: i32 = if d[major] > 0 { 1 } else { -1 };
        let r1 = d[o1] as f64 / d[major] as f64;
        let r2 = d[o2] as f64 / d[major] as f64;
        let mut p = [0i32; 3];
        let mut i: i32 = 0;
        let end = d[major] + step;
        while i != end {
            p[major] = math::floor_double(a[major] as f64 + i as f64 + 0.5);
            p[o1] = math::floor_double(a[o1] as f64 + i as f64 * r1 + 0.5);
            p[o2] = math::floor_double(a[o2] as f64 + i as f64 * r2 + 0.5);
            let mut meta = 0;
            let dx = (p[0] - a[0]).abs();
            let dz = (p[2] - a[2]).abs();
            let m = dx.max(dz);
            if m > 0 {
                if dx == m {
                    meta = 4;
                } else if dz == m {
                    meta = 8;
                }
            }
            // MC-10176: vanilla places branch logs unconditionally, so they can
            // replace any block, including bedrock.
            if crate::compat::vanilla_parity() || can_grow_into(self.w.block(p[0], p[1], p[2])) {
                set(self.w, p[0], p[1], p[2], block::LOG, meta);
            }
            i += step;
        }
    }

    fn valid_location(&mut self) -> bool {
        let below = self.w.block(self.base[0], self.base[1] - 1, self.base[2]);
        if below != block::DIRT && below != block::GRASS && below != block::FARMLAND {
            return false;
        }
        let top = [
            self.base[0],
            self.base[1] + self.height_limit - 1,
            self.base[2],
        ];
        let n = self.check_block_line(self.base, top);
        if n == -1 {
            return true;
        }
        if n < 6 {
            return false;
        }
        self.height_limit = n;
        true
    }
}

/// `WorldGenBigTree.generate` after `setScale(1, 1, 1)`. `height_limit` is the
/// generator's persistent field (it is never reset between trees in vanilla).
pub fn big_tree(
    w: &mut ServerWorld,
    r: &mut JavaRandom,
    x: i32,
    y: i32,
    z: i32,
    height_limit: &mut i32,
) -> bool {
    let seed = r.next_long();
    let mut t = BigTree {
        w,
        rand: JavaRandom::new(seed),
        base: [x, y, z],
        height_limit: *height_limit,
        height: 0,
        leaf_distance_limit: 5,
        leaf_nodes: Vec::new(),
    };
    if t.height_limit == 0 {
        t.height_limit = 5 + t.rand.next_int_n(12);
    }
    let ok = t.valid_location();
    if ok {
        t.leaf_node_list();
        t.leaves();
        // generateTrunk (trunkSize 1)
        let top = [x, y + t.height, z];
        t.branch([x, y, z], top);
        // generateLeafNodeBases
        for n in t.leaf_nodes.clone() {
            let from = [x, n[3], z];
            if !(((n[3] - y) as f64) < t.height_limit as f64 * 0.2) {
                t.branch(from, [n[0], n[1], n[2]]);
            }
        }
    }
    *height_limit = t.height_limit;
    ok
}

// ---------------------------------------------------------------- huge trees

fn huge_height(r: &mut JavaRandom, base: i32, extra: i32) -> i32 {
    let mut h = r.next_int_n(3) + base;
    if extra > 1 {
        h += r.next_int_n(extra);
    }
    h
}

/// `WorldGenHugeTrees.func_150537_a`
fn huge_can_grow(w: &mut ServerWorld, x: i32, y: i32, z: i32, h: i32) -> bool {
    if !(y >= 1 && y + h + 1 <= 256) {
        return false;
    }
    let mut ok = true;
    for yy in y..=y + 1 + h {
        let mut rad = 2;
        if yy == y {
            rad = 1;
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
    if (below == block::GRASS || below == block::DIRT) && y >= 2 {
        set(w, x, y - 1, z, block::DIRT, 0);
        set(w, x + 1, y - 1, z, block::DIRT, 0);
        set(w, x, y - 1, z + 1, block::DIRT, 0);
        set(w, x + 1, y - 1, z + 1, block::DIRT, 0);
        true
    } else {
        false
    }
}

/// `func_150535_a`: ring around the 2x2 trunk.
fn ring_2x2(w: &mut ServerWorld, x: i32, y: i32, z: i32, rad: i32, leaf: i32) {
    let r2 = rad * rad;
    for xx in (x - rad)..=(x + rad + 1) {
        let dx = xx - x;
        for zz in (z - rad)..=(z + rad + 1) {
            let dz = zz - z;
            let (ex, ez) = (dx - 1, dz - 1);
            if dx * dx + dz * dz <= r2
                || ex * ex + ez * ez <= r2
                || dx * dx + ez * ez <= r2
                || ex * ex + dz * dz <= r2
            {
                if air_or_leaves(w.block(xx, y, zz)) {
                    set(w, xx, y, zz, block::LEAVES, leaf);
                }
            }
        }
    }
}

/// `func_150534_b`: plain ring.
fn ring(w: &mut ServerWorld, x: i32, y: i32, z: i32, rad: i32, leaf: i32) {
    let r2 = rad * rad;
    for xx in (x - rad)..=(x + rad) {
        let dx = xx - x;
        for zz in (z - rad)..=(z + rad) {
            let dz = zz - z;
            if dx * dx + dz * dz <= r2 && air_or_leaves(w.block(xx, y, zz)) {
                set(w, xx, y, zz, block::LEAVES, leaf);
            }
        }
    }
}

/// `WorldGenMegaJungle(false, base, extra, wood, leaf)`
pub fn mega_jungle(
    w: &mut ServerWorld,
    r: &mut JavaRandom,
    x: i32,
    y: i32,
    z: i32,
    base: i32,
    extra: i32,
    wood: i32,
    leaf: i32,
) -> bool {
    let h = huge_height(r, base, extra);
    if !huge_can_grow(w, x, y, z, h) {
        return false;
    }
    for yy in (y + h - 2)..=(y + h) {
        let k = yy - (y + h);
        ring_2x2(w, x, yy, z, 2 + 1 - k, leaf);
    }
    let mut by = y + h - 2 - r.next_int_n(4);
    while by > y + h / 2 {
        let ang = r.next_float() * math::PI * 2.0f32;
        let mut bx = x + (0.5f32 + math::cos(ang) * 4.0f32) as i32;
        let mut bz = z + (0.5f32 + math::sin(ang) * 4.0f32) as i32;
        for i in 0..5 {
            bx = x + (1.5f32 + math::cos(ang) * i as f32) as i32;
            bz = z + (1.5f32 + math::sin(ang) * i as f32) as i32;
            if crate::compat::vanilla_parity() || can_grow_into(w.block(bx, by - 3 + i / 2, bz)) {
                set(w, bx, by - 3 + i / 2, bz, block::LOG, wood);
            }
        }
        let n = 1 + r.next_int_n(2);
        for yy in (by - n)..=by {
            let k = yy - by;
            ring(w, bx, yy, bz, 1 - k, leaf);
        }
        by -= 2 + r.next_int_n(4);
    }
    for i in 0..h {
        if air_or_leaves(w.block(x, y + i, z)) {
            set(w, x, y + i, z, block::LOG, wood);
            if i > 0 {
                if r.next_int_n(3) > 0 && w.is_air(x - 1, y + i, z) {
                    set(w, x - 1, y + i, z, block::VINE, 8);
                }
                if r.next_int_n(3) > 0 && w.is_air(x, y + i, z - 1) {
                    set(w, x, y + i, z - 1, block::VINE, 1);
                }
            }
        }
        if i < h - 1 {
            if air_or_leaves(w.block(x + 1, y + i, z)) {
                set(w, x + 1, y + i, z, block::LOG, wood);
                if i > 0 {
                    if r.next_int_n(3) > 0 && w.is_air(x + 2, y + i, z) {
                        set(w, x + 2, y + i, z, block::VINE, 2);
                    }
                    if r.next_int_n(3) > 0 && w.is_air(x + 1, y + i, z - 1) {
                        set(w, x + 1, y + i, z - 1, block::VINE, 1);
                    }
                }
            }
            if air_or_leaves(w.block(x + 1, y + i, z + 1)) {
                set(w, x + 1, y + i, z + 1, block::LOG, wood);
                if i > 0 {
                    if r.next_int_n(3) > 0 && w.is_air(x + 2, y + i, z + 1) {
                        set(w, x + 2, y + i, z + 1, block::VINE, 2);
                    }
                    if r.next_int_n(3) > 0 && w.is_air(x + 1, y + i, z + 2) {
                        set(w, x + 1, y + i, z + 2, block::VINE, 4);
                    }
                }
            }
            if air_or_leaves(w.block(x, y + i, z + 1)) {
                set(w, x, y + i, z + 1, block::LOG, wood);
                if i > 0 {
                    if r.next_int_n(3) > 0 && w.is_air(x - 1, y + i, z + 1) {
                        set(w, x - 1, y + i, z + 1, block::VINE, 8);
                    }
                    if r.next_int_n(3) > 0 && w.is_air(x, y + i, z + 2) {
                        set(w, x, y + i, z + 2, block::VINE, 4);
                    }
                }
            }
        }
    }
    true
}

/// `WorldGenMegaPineTree(false, use_base_height)`
pub fn mega_pine(
    w: &mut ServerWorld,
    r: &mut JavaRandom,
    x: i32,
    y: i32,
    z: i32,
    use_base_height: bool,
) -> bool {
    let h = huge_height(r, 13, 15);
    if !huge_can_grow(w, x, y, z, h) {
        return false;
    }
    // func_150541_c
    let top = y + h;
    let mut crown = r.next_int_n(5);
    crown += if use_base_height { 13 } else { 3 };
    let mut prev = 0;
    for yy in (top - crown)..=top {
        let k = top - yy;
        let rad = math::floor_float(k as f32 / crown as f32 * 3.5f32);
        ring_2x2(
            w,
            x,
            yy,
            z,
            rad + if k > 0 && rad == prev && (yy & 1) == 0 {
                1
            } else {
                0
            },
            1,
        );
        prev = rad;
    }
    for i in 0..h {
        if air_or_leaves(w.block(x, y + i, z)) {
            set(w, x, y + i, z, block::LOG, 1);
        }
        if i < h - 1 {
            for (dx, dz) in [(1, 0), (1, 1), (0, 1)] {
                if air_or_leaves(w.block(x + dx, y + i, z + dz)) {
                    set(w, x + dx, y + i, z + dz, block::LOG, 1);
                }
            }
        }
    }
    true
}

fn podzol_patch(w: &mut ServerWorld, x: i32, y: i32, z: i32) {
    for dx in -2..=2i32 {
        for dz in -2..=2i32 {
            if dx.abs() != 2 || dz.abs() != 2 {
                let (px, pz) = (x + dx, z + dz);
                let mut yy = y + 2;
                while yy >= y - 3 {
                    let b = w.block(px, yy, pz);
                    if b == block::GRASS || b == block::DIRT {
                        set(w, px, yy, pz, block::DIRT, 2);
                        break;
                    }
                    if block::material(b) != Material::Air && yy < y {
                        break;
                    }
                    yy -= 1;
                }
            }
        }
    }
}

/// `WorldGenMegaPineTree.func_150524_b` (podzol around a grown tree).
pub fn mega_pine_soil(w: &mut ServerWorld, r: &mut JavaRandom, x: i32, y: i32, z: i32) {
    podzol_patch(w, x - 1, y, z - 1);
    podzol_patch(w, x + 2, y, z - 1);
    podzol_patch(w, x - 1, y, z + 2);
    podzol_patch(w, x + 2, y, z + 2);
    for _ in 0..5 {
        let k = r.next_int_n(64);
        let (a, b) = (k % 8, k / 8);
        if a == 0 || a == 7 || b == 0 || b == 7 {
            podzol_patch(w, x - 3 + a, y, z - 3 + b);
        }
    }
}
