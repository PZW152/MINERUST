//! `World.rayTraceBlocks`, `Block.collisionRayTrace` (with `BlockStairs`'
//! eight-cube trace) and `AxisAlignedBB.calculateIntercept`.

use super::player::Aabb;
use crate::block::{self, Material};
use crate::util::math;
use crate::world::BlockView;

pub type V3 = [f64; 3];

/// `MovingObjectPosition` of a block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockHit {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub side: i32,
    pub vec: V3,
}

fn sq(a: V3, b: V3) -> f64 {
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}

/// `Vec3.getIntermediateWithXValue` (axis 0), `Y` (1), `Z` (2)
fn intermediate(a: V3, b: V3, axis: usize, v: f64) -> Option<V3> {
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    if d[axis] * d[axis] < 1.0e-7 {
        return None;
    }
    let t = (v - a[axis]) / d[axis];
    if !(0.0..=1.0).contains(&t) {
        None
    } else {
        Some([a[0] + d[0] * t, a[1] + d[1] * t, a[2] + d[2] * t])
    }
}

/// The six intercepts of `bb` with `a -> b` and the nearest one that lies
/// on the box (shared by `collisionRayTrace` and `calculateIntercept`).
fn intercept(min: V3, max: V3, a: V3, b: V3) -> Option<(V3, i32)> {
    let inside = |v: &Option<V3>, axis: usize| -> Option<V3> {
        let v = (*v)?;
        let (p, q) = match axis {
            0 => (1, 2),
            1 => (0, 2),
            _ => (0, 1),
        };
        if v[p] >= min[p] && v[p] <= max[p] && v[q] >= min[q] && v[q] <= max[q] {
            Some(v)
        } else {
            None
        }
    };
    let c = [
        inside(&intermediate(a, b, 0, min[0]), 0),
        inside(&intermediate(a, b, 0, max[0]), 0),
        inside(&intermediate(a, b, 1, min[1]), 1),
        inside(&intermediate(a, b, 1, max[1]), 1),
        inside(&intermediate(a, b, 2, min[2]), 2),
        inside(&intermediate(a, b, 2, max[2]), 2),
    ];
    let mut best: Option<(V3, usize)> = None;
    for (i, v) in c.iter().enumerate() {
        if let Some(v) = v {
            if best.map_or(true, |(bv, _)| sq(a, *v) < sq(a, bv)) {
                best = Some((*v, i));
            }
        }
    }
    let (v, i) = best?;
    // side: minX 4, maxX 5, minY 0, maxY 1, minZ 2, maxZ 3
    const SIDE: [i32; 6] = [4, 5, 0, 1, 2, 3];
    Some((v, SIDE[i]))
}

/// `AxisAlignedBB.calculateIntercept`: the hit point, if any.
pub fn calculate_intercept(bb: &Aabb, a: V3, b: V3) -> Option<V3> {
    intercept(bb.min, bb.max, a, b).map(|(v, _)| v)
}

/// `Block.collisionRayTrace` with the block's bounds (block space).
fn block_trace(bounds: [f32; 6], x: i32, y: i32, z: i32, a: V3, b: V3) -> Option<BlockHit> {
    let o = [x as f64, y as f64, z as f64];
    let a = [a[0] - o[0], a[1] - o[1], a[2] - o[2]];
    let b = [b[0] - o[0], b[1] - o[1], b[2] - o[2]];
    let min = [bounds[0] as f64, bounds[1] as f64, bounds[2] as f64];
    let max = [bounds[3] as f64, bounds[4] as f64, bounds[5] as f64];
    let (v, side) = intercept(min, max, a, b)?;
    Some(BlockHit {
        x,
        y,
        z,
        side,
        vec: [v[0] + o[0], v[1] + o[1], v[2] + o[2]],
    })
}

/// `BlockStairs.ROTATIONS`
const STAIR_SKIP: [[usize; 2]; 8] = [
    [2, 6],
    [3, 7],
    [2, 3],
    [6, 7],
    [0, 4],
    [1, 5],
    [0, 1],
    [4, 5],
];

/// `collisionRayTrace` for the block at `(x, y, z)`.
fn collision_ray_trace(
    w: &dyn BlockView,
    x: i32,
    y: i32,
    z: i32,
    a: V3,
    b: V3,
) -> Option<BlockHit> {
    let id = w.peek_block(x, y, z);
    let meta = w.peek_meta(x, y, z);
    if block::is_stairs(id) {
        // eight half-size cubes, minus the two the step leaves out; the hit
        // farthest from the end point (the first along the ray) wins
        let skip = STAIR_SKIP[(meta & 3) as usize + if meta & 4 != 0 { 4 } else { 0 }];
        let mut best: Option<BlockHit> = None;
        let mut far = 0.0;
        for pass in 0..8 {
            if skip.contains(&pass) {
                continue;
            }
            let f = |n: usize| 0.5 * n as f32;
            let bounds = [
                f(pass % 2),
                f(pass / 2 % 2),
                f(pass / 4 % 2),
                0.5 + f(pass % 2),
                0.5 + f(pass / 2 % 2),
                0.5 + f(pass / 4 % 2),
            ];
            if let Some(h) = block_trace(bounds, x, y, z, a, b) {
                let d = sq(h.vec, b);
                if d > far {
                    best = Some(h);
                    far = d;
                }
            }
        }
        return best;
    }
    let get = |x: i32, y: i32, z: i32| (w.peek_block(x, y, z), w.peek_meta(x, y, z));
    let bounds = crate::shapes::selection(&get, id, meta, x, y, z);
    block_trace(bounds, x, y, z, a, b)
}

/// `Block.canCollideCheck(meta, hitIfLiquid)`
fn can_collide_check(id: u8, meta: u8, liquid: bool) -> bool {
    match id {
        block::AIR | block::FIRE => false,
        _ if block::material(id).is_liquid() => liquid && meta == 0,
        _ => true,
    }
}

fn has_collision_box(w: &dyn BlockView, id: u8, x: i32, y: i32, z: i32) -> bool {
    if id == block::AIR {
        return false;
    }
    let get = |x: i32, y: i32, z: i32| (w.peek_block(x, y, z), w.peek_meta(x, y, z));
    let mut v = Vec::new();
    crate::shapes::collision(&get, id, w.peek_meta(x, y, z), x, y, z, &mut v);
    !v.is_empty()
}

/// `World.rayTraceBlocks(start, end, stopOnLiquid, ignoreBlockWithoutBoundingBox,
/// returnLastUncollidableBlock)`
pub fn ray_trace_blocks(
    w: &dyn BlockView,
    mut a: V3,
    b: V3,
    liquid: bool,
    ignore_no_bb: bool,
    return_last: bool,
) -> Option<BlockHit> {
    if a.iter().chain(b.iter()).any(|v| v.is_nan()) {
        return None;
    }
    let (ex, ey, ez) = (
        math::floor_double(b[0]),
        math::floor_double(b[1]),
        math::floor_double(b[2]),
    );
    let (mut x, mut y, mut z) = (
        math::floor_double(a[0]),
        math::floor_double(a[1]),
        math::floor_double(a[2]),
    );
    let id = w.peek_block(x, y, z);
    let meta = w.peek_meta(x, y, z);
    if (!ignore_no_bb || has_collision_box(w, id, x, y, z)) && can_collide_check(id, meta, liquid) {
        if let Some(h) = collision_ray_trace(w, x, y, z, a, b) {
            return Some(h);
        }
    }
    let mut last: Option<BlockHit> = None;
    let mut n = 200;
    loop {
        let go = n >= 0;
        n -= 1;
        if !go {
            break;
        }
        if a.iter().any(|v| v.is_nan()) {
            return None;
        }
        if x == ex && y == ey && z == ez {
            return if return_last { last } else { None };
        }
        let (mut fx, mut fy, mut fz) = (true, true, true);
        let (mut px, mut py, mut pz) = (999.0, 999.0, 999.0);
        if ex > x {
            px = x as f64 + 1.0;
        } else if ex < x {
            px = x as f64 + 0.0;
        } else {
            fx = false;
        }
        if ey > y {
            py = y as f64 + 1.0;
        } else if ey < y {
            py = y as f64 + 0.0;
        } else {
            fy = false;
        }
        if ez > z {
            pz = z as f64 + 1.0;
        } else if ez < z {
            pz = z as f64 + 0.0;
        } else {
            fz = false;
        }
        let (mut tx, mut ty, mut tz) = (999.0, 999.0, 999.0);
        let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        if fx {
            tx = (px - a[0]) / d[0];
        }
        if fy {
            ty = (py - a[1]) / d[1];
        }
        if fz {
            tz = (pz - a[2]) / d[2];
        }
        let side;
        if tx < ty && tx < tz {
            side = if ex > x { 4 } else { 5 };
            a[0] = px;
            a[1] += d[1] * tx;
            a[2] += d[2] * tx;
        } else if ty < tz {
            side = if ey > y { 0 } else { 1 };
            a[0] += d[0] * ty;
            a[1] = py;
            a[2] += d[2] * ty;
        } else {
            side = if ez > z { 2 } else { 3 };
            a[0] += d[0] * tz;
            a[1] += d[1] * tz;
            a[2] = pz;
        }
        x = math::floor_double(a[0]);
        if side == 5 {
            x -= 1;
        }
        y = math::floor_double(a[1]);
        if side == 1 {
            y -= 1;
        }
        z = math::floor_double(a[2]);
        if side == 3 {
            z -= 1;
        }
        let id = w.peek_block(x, y, z);
        let meta = w.peek_meta(x, y, z);
        if !ignore_no_bb || has_collision_box(w, id, x, y, z) {
            if can_collide_check(id, meta, liquid) {
                if let Some(h) = collision_ray_trace(w, x, y, z, a, b) {
                    return Some(h);
                }
            } else {
                last = Some(BlockHit {
                    x,
                    y,
                    z,
                    side,
                    vec: a,
                });
            }
        }
    }
    if return_last {
        last
    } else {
        None
    }
}

/// `Material.air`
pub fn is_air(id: u8) -> bool {
    block::material(id) == Material::Air
}
