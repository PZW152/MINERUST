//! Travel between the overworld, the Nether and the End: the portal timers
//! of `Entity.onEntityUpdate`, `ServerConfigurationManager.transferEntityToWorld`
//! and `Teleporter` (finding and building Nether portals, the End platform).

use crate::block;
use crate::java::JavaRandom;
use crate::util::math;
use crate::world::server::ServerWorld;

/// `Teleporter.placeInExistingPortal`: the nearest portal block within 128
/// blocks (scanning only chunks that exist, which is where portals can be).
/// Returns the feet position and the new yaw.
pub fn find_portal(w: &mut ServerWorld, pos: [f64; 3], yaw: f32) -> Option<([f64; 3], f32)> {
    let (px, pz) = (math::floor_double(pos[0]), math::floor_double(pos[2]));
    let r = 128;
    let mut best = -1.0f64;
    let mut at = [0i32; 3];
    // Load the chunks that exist on disk or in memory, in scan order.
    for cx in (px - r) >> 4..=(px + r) >> 4 {
        for cz in (pz - r) >> 4..=(pz + r) >> 4 {
            let stored = w.chunk_exists(cx, cz)
                || w.save_dir.as_ref().map_or(false, |d| {
                    crate::world::save::read_chunk(d, cx, cz).is_some()
                });
            if stored && !w.chunk_exists(cx, cz) {
                w.load_chunk(cx, cz);
            }
        }
    }
    let top = if w.provider.dim == -1 { 127 } else { 255 };
    for x in px - r..=px + r {
        let dx = x as f64 + 0.5 - pos[0];
        for z in pz - r..=pz + r {
            if !w.chunk_exists(x >> 4, z >> 4) {
                continue;
            }
            let dz = z as f64 + 0.5 - pos[2];
            let mut y = top;
            while y >= 0 {
                if w.peek_block(x, y, z) == block::PORTAL {
                    while w.peek_block(x, y - 1, z) == block::PORTAL {
                        y -= 1;
                    }
                    let dy = y as f64 + 0.5 - pos[1];
                    let d = dx * dx + dy * dy + dz * dz;
                    if best < 0.0 || d < best {
                        best = d;
                        at = [x, y, z];
                    }
                }
                y -= 1;
            }
        }
    }
    if best < 0.0 {
        return None;
    }
    Some(place_in(w, at, yaw))
}

/// The second half of `placeInExistingPortal`: where in the portal to stand.
fn place_in(w: &mut ServerWorld, at: [i32; 3], yaw: f32) -> ([f64; 3], f32) {
    const OX: [i32; 4] = [0, -1, 0, 1];
    const OZ: [i32; 4] = [1, 0, -1, 0];
    const ROT_LEFT: [usize; 4] = [3, 0, 1, 2];
    const ROT_OPP: [usize; 4] = [2, 3, 0, 1];
    let [mut x, y, mut z] = at;
    let (mut fx, fy, mut fz) = (x as f64 + 0.5, y as f64 + 0.5, z as f64 + 0.5);
    let mut dir: i32 = -1;
    if w.block(x - 1, y, z) == block::PORTAL {
        dir = 2;
    }
    if w.block(x + 1, y, z) == block::PORTAL {
        dir = 0;
    }
    if w.block(x, y, z - 1) == block::PORTAL {
        dir = 3;
    }
    if w.block(x, y, z + 1) == block::PORTAL {
        dir = 1;
    }
    // Entity.getTeleportDirection: 0 for players walking straight in
    let facing = 0usize;
    let mut new_yaw = yaw;
    if dir > -1 {
        let mut d = dir as usize;
        let mut l = ROT_LEFT[d];
        let (mut ox, mut oz, mut lx, mut lz) = (OX[d], OZ[d], OX[l], OZ[l]);
        let blocked =
            |w: &mut ServerWorld, a: i32, b: i32| !w.is_air(a, y, b) || !w.is_air(a, y + 1, b);
        let mut b36 = blocked(w, x + ox + lx, z + oz + lz);
        let mut b37 = blocked(w, x + ox, z + oz);
        if b36 && b37 {
            d = ROT_OPP[d];
            l = ROT_OPP[l];
            ox = OX[d];
            oz = OZ[d];
            lx = OX[l];
            lz = OZ[l];
            x -= lx;
            fx -= lx as f64;
            z -= lz;
            fz -= lz as f64;
            b36 = blocked(w, x + ox + lx, z + oz + lz);
            b37 = blocked(w, x + ox, z + oz);
        }
        let (mut a, mut b) = (0.5f32, 0.5f32);
        if !b36 && b37 {
            a = 1.0;
        } else if b36 && !b37 {
            a = 0.0;
        } else if b36 && b37 {
            b = 0.0;
        }
        fx += (lx as f32 * a + b * ox as f32) as f64;
        fz += (lz as f32 * a + b * oz as f32) as f64;
        new_yaw = yaw - (facing as f32) * 90.0 + d as f32 * 90.0;
    }
    ([fx, fy, fz], new_yaw)
}

/// `Teleporter.makePortal`
pub fn make_portal(w: &mut ServerWorld, r: &mut JavaRandom, pos: [f64; 3]) {
    let range = 16;
    let (px, py, pz) = (
        math::floor_double(pos[0]),
        math::floor_double(pos[1]),
        math::floor_double(pos[2]),
    );
    let top = if w.provider.dim == -1 { 128 } else { 256 };
    let mut best = -1.0f64;
    let (mut bx, mut by, mut bz, mut orient) = (px, py, pz, 0);
    let start = r.next_int_n(4);
    let solid = |w: &mut ServerWorld, x: i32, y: i32, z: i32| w.material(x, y, z).is_solid();
    for pass in 0..2 {
        if pass == 1 && best >= 0.0 {
            break;
        }
        for x in px - range..=px + range {
            let dx = x as f64 + 0.5 - pos[0];
            for z in pz - range..=pz + range {
                let dz = z as f64 + 0.5 - pos[2];
                let mut y = top - 1;
                'col: while y >= 0 {
                    if w.is_air(x, y, z) {
                        while y > 0 && w.is_air(x, y - 1, z) {
                            y -= 1;
                        }
                        let n = if pass == 0 { 4 } else { 2 };
                        for k in start..start + n {
                            let (mut a, mut b) = (k % 2, 1 - k % 2);
                            if pass == 0 && k % 4 >= 2 {
                                a = -a;
                                b = -b;
                            }
                            let depth = if pass == 0 { 3 } else { 1 };
                            for d in 0..depth {
                                for wdt in 0..4 {
                                    for h in -1..4 {
                                        let (cx, cy, cz) = if pass == 0 {
                                            (
                                                x + (wdt - 1) * a + d * b,
                                                y + h,
                                                z + (wdt - 1) * b - d * a,
                                            )
                                        } else {
                                            (x + (wdt - 1) * a, y + h, z + (wdt - 1) * b)
                                        };
                                        if (h < 0 && !solid(w, cx, cy, cz))
                                            || (h >= 0 && !w.is_air(cx, cy, cz))
                                        {
                                            // `continue label299`: give up this column height
                                            y -= 1;
                                            continue 'col;
                                        }
                                    }
                                }
                            }
                            let dy = y as f64 + 0.5 - pos[1];
                            let dist = dx * dx + dy * dy + dz * dz;
                            if best < 0.0 || dist < best {
                                best = dist;
                                bx = x;
                                by = y;
                                bz = z;
                                orient = if pass == 0 { k % 4 } else { k % 2 };
                            }
                        }
                    }
                    y -= 1;
                }
            }
        }
    }
    let (mut a, mut b) = (orient % 2, 1 - orient % 2);
    if orient % 4 >= 2 {
        a = -a;
        b = -b;
    }
    let mut y = by;
    if best < 0.0 {
        // no room anywhere: a floating platform
        y = by.clamp(70, top - 10);
        for side in -1..=1 {
            for wdt in 1..3 {
                for h in -1..3 {
                    let cx = bx + (wdt - 1) * a + side * b;
                    let cz = bz + (wdt - 1) * b - side * a;
                    let id = if h < 0 { block::OBSIDIAN } else { block::AIR };
                    w.set_block(cx, y + h, cz, id, 0, 3);
                }
            }
        }
    }
    let axis = if a != 0 { 1 } else { 2 };
    for _ in 0..4 {
        for wdt in 0..4 {
            for h in -1..4 {
                let cx = bx + (wdt - 1) * a;
                let cz = bz + (wdt - 1) * b;
                let frame = wdt == 0 || wdt == 3 || h == -1 || h == 3;
                if frame {
                    w.set_block(cx, y + h, cz, block::OBSIDIAN, 0, 2);
                } else {
                    w.set_block(cx, y + h, cz, block::PORTAL, axis, 2);
                }
            }
        }
        for wdt in 0..4 {
            for h in -1..4 {
                let cx = bx + (wdt - 1) * a;
                let cz = bz + (wdt - 1) * b;
                let id = w.block(cx, y + h, cz);
                w.notify_neighbours(cx, y + h, cz, id);
            }
        }
    }
}

/// `Teleporter.placeInPortal` in the End: a 5x5 obsidian platform.
pub fn end_platform(w: &mut ServerWorld, pos: [f64; 3]) -> [f64; 3] {
    let (x, y, z) = (
        math::floor_double(pos[0]),
        math::floor_double(pos[1]) - 1,
        math::floor_double(pos[2]),
    );
    for a in -2..=2 {
        for b in -2..=2 {
            for h in -1..3 {
                let id = if h < 0 { block::OBSIDIAN } else { block::AIR };
                w.set_block(x + b, y + h, z - a, id, 0, 3);
            }
        }
    }
    [x as f64, y as f64, z as f64]
}

/// Which portal block the player's box touches, if any.
pub fn portal_contact(w: &mut ServerWorld, feet: [f64; 3]) -> Option<u8> {
    let lo = [
        feet[0] - 0.3 + 0.001,
        feet[1] + 0.001,
        feet[2] - 0.3 + 0.001,
    ];
    let hi = [
        feet[0] + 0.3 - 0.001,
        feet[1] + 1.8 - 0.001,
        feet[2] + 0.3 - 0.001,
    ];
    let f = |v: f64| v.floor() as i32;
    let mut found = None;
    for x in f(lo[0])..=f(hi[0]) {
        for y in f(lo[1])..=f(hi[1]) {
            for z in f(lo[2])..=f(hi[2]) {
                match w.peek_block(x, y, z) {
                    block::PORTAL => found = found.or(Some(block::PORTAL)),
                    // BlockEndPortal: only below 0.75 of the block
                    block::END_PORTAL if lo[1] < y as f64 + 0.75 => return Some(block::END_PORTAL),
                    _ => {}
                }
            }
        }
    }
    found
}
