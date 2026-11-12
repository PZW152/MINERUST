//! `PathFinder`, `PathEntity` and the parts of `PathNavigate` that follow a
//! path.

use crate::block::{self, Material};
use crate::world::server::ServerWorld;
use std::collections::{BinaryHeap, HashMap};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Pt {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl Pt {
    fn dist2(&self, o: &Pt) -> f32 {
        let (a, b, c) = (
            (o.x - self.x) as f32,
            (o.y - self.y) as f32,
            (o.z - self.z) as f32,
        );
        a * a + b * b + c * c
    }
    fn dist(&self, o: &Pt) -> f32 {
        self.dist2(o).sqrt()
    }
}

/// Size of the entity in blocks, as `PathFinder` rounds it.
#[derive(Clone, Copy)]
pub struct Size {
    pub w: i32,
    pub h: i32,
}

pub struct Opts {
    pub doors: bool,
    pub avoid_water: bool,
    pub can_drown: bool,
}

/// `PathFinder.canEntityStandAt`: 2 = passable with water/trapdoor, 1 = open,
/// 0 = blocked, -1 = water while avoiding it, -2 = lava, -3 = fence or rail,
/// -4 = trapdoor.
pub fn stand_at(
    w: &mut ServerWorld,
    x: i32,
    y: i32,
    z: i32,
    s: Size,
    opts: &Opts,
    feet: (i32, i32, i32),
    in_lava: bool,
) -> i32 {
    let mut wet = false;
    for bx in x..x + s.w {
        for by in y..y + s.h {
            for bz in z..z + s.w {
                let id = w.block(bx, by, bz);
                let m = block::material(id);
                if m == Material::Air {
                    continue;
                }
                if id == block::FLOWING_WATER || id == block::WATER {
                    if opts.avoid_water {
                        return -1;
                    }
                    wet = true;
                    continue;
                }
                if !opts.doors && id == block::WOODEN_DOOR {
                    return 0;
                }
                if block::is_rail(id) {
                    let (fx, fy, fz) = feet;
                    if !block::is_rail(w.block(fx, fy, fz))
                        && !block::is_rail(w.block(fx, fy - 1, fz))
                    {
                        return -3;
                    }
                    continue;
                }
                let passable = match id {
                    block::WOODEN_DOOR | block::IRON_DOOR => {
                        let m = w.meta(bx, by, bz);
                        let full = if m & 8 != 0 {
                            w.meta(bx, by - 1, bz)
                        } else {
                            m
                        };
                        full & 4 != 0
                    }
                    _ => !m.blocks_movement(),
                };
                if !passable {
                    if id == block::FENCE || block::info(id).shape == block::Shape::Fence {
                        return -3;
                    }
                    if m != Material::Lava {
                        return 0;
                    }
                    if !in_lava {
                        return -2;
                    }
                }
            }
        }
    }
    if wet {
        2
    } else {
        1
    }
}

#[derive(PartialEq)]
struct Open {
    f: f32,
    seq: u32,
    p: Pt,
}
impl Eq for Open {}
impl Ord for Open {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        o.f.partial_cmp(&self.f)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(o.seq.cmp(&self.seq))
    }
}
impl PartialOrd for Open {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}

/// `PathFinder.createEntityPathTo`: A* towards `to`, limited to `range`.
#[allow(clippy::too_many_arguments)]
pub fn find(
    w: &mut ServerWorld,
    from: Pt,
    to: Pt,
    s: Size,
    range: f32,
    opts: &Opts,
    max_drop: i32,
    feet: (i32, i32, i32),
    in_lava: bool,
) -> Option<Vec<Pt>> {
    let mut g: HashMap<Pt, f32> = HashMap::new();
    let mut prev: HashMap<Pt, Pt> = HashMap::new();
    let mut closed: std::collections::HashSet<Pt> = std::collections::HashSet::new();
    let mut heap = BinaryHeap::new();
    let mut seq = 0u32;
    g.insert(from, 0.0);
    heap.push(Open {
        f: from.dist2(&to),
        seq,
        p: from,
    });
    let mut best = from;
    let mut expanded = 0;
    while let Some(Open { p, .. }) = heap.pop() {
        if closed.contains(&p) {
            continue;
        }
        if p == to {
            best = p;
            break;
        }
        if p.dist2(&to) < best.dist2(&to) {
            best = p;
        }
        closed.insert(p);
        expanded += 1;
        if expanded > 2000 {
            break;
        }
        // findPathOptions
        let step_up = if stand_at(w, p.x, p.y + 1, p.z, s, opts, feet, in_lava) == 1 {
            1
        } else {
            0
        };
        for (dx, dz) in [(0, 1), (-1, 0), (1, 0), (0, -1)] {
            let Some(n) = safe_point(
                w,
                p.x + dx,
                p.y,
                p.z + dz,
                s,
                opts,
                step_up,
                max_drop,
                feet,
                in_lava,
            ) else {
                continue;
            };
            if closed.contains(&n) || n.dist(&to) >= range {
                continue;
            }
            let cost = g[&p] + p.dist2(&n);
            if g.get(&n).map_or(true, |&c| cost < c) {
                g.insert(n, cost);
                prev.insert(n, p);
                seq += 1;
                heap.push(Open {
                    f: cost + n.dist2(&to),
                    seq,
                    p: n,
                });
            }
        }
    }
    if best == from {
        return None;
    }
    let mut path = vec![best];
    let mut c = best;
    while let Some(&q) = prev.get(&c) {
        path.push(q);
        c = q;
    }
    path.reverse();
    Some(path)
}

/// `PathFinder.getSafePoint`
#[allow(clippy::too_many_arguments)]
fn safe_point(
    w: &mut ServerWorld,
    x: i32,
    mut y: i32,
    z: i32,
    s: Size,
    opts: &Opts,
    step_up: i32,
    max_drop: i32,
    feet: (i32, i32, i32),
    in_lava: bool,
) -> Option<Pt> {
    let v = stand_at(w, x, y, z, s, opts, feet, in_lava);
    if v == 2 {
        return Some(Pt { x, y, z });
    }
    let mut p = None;
    if v == 1 {
        p = Some(Pt { x, y, z });
    }
    if p.is_none()
        && step_up > 0
        && v != -3
        && v != -4
        && stand_at(w, x, y + step_up, z, s, opts, feet, in_lava) == 1
    {
        p = Some(Pt {
            x,
            y: y + step_up,
            z,
        });
        y += step_up;
    }
    if p.is_some() {
        let mut tries = 0;
        let mut below = 0;
        while y > 0 {
            below = stand_at(w, x, y - 1, z, s, opts, feet, in_lava);
            if opts.avoid_water && below == -1 {
                return None;
            }
            if below != 1 {
                break;
            }
            tries += 1;
            if tries >= max_drop {
                return None;
            }
            y -= 1;
            if y > 0 {
                p = Some(Pt { x, y, z });
            }
        }
        if below == -2 {
            return None;
        }
    }
    p
}
