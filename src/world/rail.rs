//! `BlockRailBase$Rail`: how a placed rail picks its shape from its
//! neighbours, and `BlockRailBase.onNeighborBlockChange`.

use super::server::{GenEntity, ServerWorld};
use crate::block;

struct Rail {
    x: i32,
    y: i32,
    z: i32,
    powered: bool,
    links: Vec<(i32, i32, i32)>,
}

/// `BlockRailBase.isRailBlockAt`
fn rail_at(w: &mut ServerWorld, x: i32, y: i32, z: i32) -> bool {
    block::is_rail(w.block(x, y, z))
}

impl Rail {
    fn new(w: &mut ServerWorld, x: i32, y: i32, z: i32) -> Rail {
        let id = w.block(x, y, z);
        let mut m = w.meta(x, y, z);
        // golden and detector rails are the "powered" BlockRailBase subclass
        let powered = id != block::RAIL;
        if powered {
            m &= !8;
        }
        let mut r = Rail {
            x,
            y,
            z,
            powered,
            links: Vec::new(),
        };
        r.update_links(m);
        r
    }

    fn update_links(&mut self, m: i32) {
        let (x, y, z) = (self.x, self.y, self.z);
        self.links = match m {
            0 => vec![(x, y, z - 1), (x, y, z + 1)],
            1 => vec![(x - 1, y, z), (x + 1, y, z)],
            2 => vec![(x - 1, y, z), (x + 1, y + 1, z)],
            3 => vec![(x - 1, y + 1, z), (x + 1, y, z)],
            4 => vec![(x, y + 1, z - 1), (x, y, z + 1)],
            5 => vec![(x, y, z - 1), (x, y + 1, z + 1)],
            6 => vec![(x + 1, y, z), (x, y, z + 1)],
            7 => vec![(x - 1, y, z), (x, y, z + 1)],
            8 => vec![(x - 1, y, z), (x, y, z - 1)],
            9 => vec![(x + 1, y, z), (x, y, z - 1)],
            _ => Vec::new(),
        };
    }

    fn remove_soft_links(&mut self, w: &mut ServerWorld) {
        let mut i = 0;
        while i < self.links.len() {
            let p = self.links[i];
            match neighbour(w, p) {
                Some(n) if n.linked_to(self) => {
                    self.links[i] = (n.x, n.y, n.z);
                    i += 1;
                }
                _ => {
                    self.links.remove(i);
                }
            }
        }
    }

    fn linked_to(&self, o: &Rail) -> bool {
        self.links.iter().any(|p| p.0 == o.x && p.2 == o.z)
    }

    fn linked(&self, x: i32, z: i32) -> bool {
        self.links.iter().any(|p| p.0 == x && p.2 == z)
    }

    fn rail_near(w: &mut ServerWorld, x: i32, y: i32, z: i32) -> bool {
        rail_at(w, x, y, z) || rail_at(w, x, y + 1, z) || rail_at(w, x, y - 1, z)
    }

    /// `countAdjacentRails`
    fn adjacent(&self, w: &mut ServerWorld) -> i32 {
        let (x, y, z) = (self.x, self.y, self.z);
        [(x, z - 1), (x, z + 1), (x - 1, z), (x + 1, z)]
            .iter()
            .filter(|&&(a, b)| Rail::rail_near(w, a, y, b))
            .count() as i32
    }

    fn can_connect(&self, o: &Rail) -> bool {
        self.linked_to(o) || self.links.len() != 2
    }

    /// `connectTo`
    fn connect(&mut self, w: &mut ServerWorld, o: &Rail) {
        self.links.push((o.x, o.y, o.z));
        let (x, y, z) = (self.x, self.y, self.z);
        let n = self.linked(x, z - 1);
        let s = self.linked(x, z + 1);
        let we = self.linked(x - 1, z);
        let e = self.linked(x + 1, z);
        let mut m = -1;
        if n || s {
            m = 0;
        }
        if we || e {
            m = 1;
        }
        if !self.powered {
            if s && e && !n && !we {
                m = 6;
            }
            if s && we && !n && !e {
                m = 7;
            }
            if n && we && !s && !e {
                m = 8;
            }
            if n && e && !s && !we {
                m = 9;
            }
        }
        if m == 0 {
            if rail_at(w, x, y + 1, z - 1) {
                m = 4;
            }
            if rail_at(w, x, y + 1, z + 1) {
                m = 5;
            }
        }
        if m == 1 {
            if rail_at(w, x + 1, y + 1, z) {
                m = 2;
            }
            if rail_at(w, x - 1, y + 1, z) {
                m = 3;
            }
        }
        if m < 0 {
            m = 0;
        }
        let mut v = m;
        if self.powered {
            v |= w.meta(x, y, z) & 8;
        }
        w.set_meta(x, y, z, v, 3);
    }

    fn has_neighbour(&self, w: &mut ServerWorld, p: (i32, i32, i32)) -> bool {
        match neighbour(w, p) {
            Some(mut n) => {
                n.remove_soft_links(w);
                n.can_connect(self)
            }
            None => false,
        }
    }

    /// `updateState(powered, force)`
    fn update_state(&mut self, w: &mut ServerWorld, power: bool, force: bool) {
        let (x, y, z) = (self.x, self.y, self.z);
        let n = self.has_neighbour(w, (x, y, z - 1));
        let s = self.has_neighbour(w, (x, y, z + 1));
        let we = self.has_neighbour(w, (x - 1, y, z));
        let e = self.has_neighbour(w, (x + 1, y, z));
        let mut m = -1;
        if (n || s) && !we && !e {
            m = 0;
        }
        if (we || e) && !n && !s {
            m = 1;
        }
        if !self.powered {
            if s && e && !n && !we {
                m = 6;
            }
            if s && we && !n && !e {
                m = 7;
            }
            if n && we && !s && !e {
                m = 8;
            }
            if n && e && !s && !we {
                m = 9;
            }
        }
        if m == -1 {
            if n || s {
                m = 0;
            }
            if we || e {
                m = 1;
            }
            if !self.powered {
                if power {
                    if s && e {
                        m = 6;
                    }
                    if we && s {
                        m = 7;
                    }
                    if e && n {
                        m = 9;
                    }
                    if n && we {
                        m = 8;
                    }
                } else {
                    if n && we {
                        m = 8;
                    }
                    if e && n {
                        m = 9;
                    }
                    if we && s {
                        m = 7;
                    }
                    if s && e {
                        m = 6;
                    }
                }
            }
        }
        if m == 0 {
            if rail_at(w, x, y + 1, z - 1) {
                m = 4;
            }
            if rail_at(w, x, y + 1, z + 1) {
                m = 5;
            }
        }
        if m == 1 {
            if rail_at(w, x + 1, y + 1, z) {
                m = 2;
            }
            if rail_at(w, x - 1, y + 1, z) {
                m = 3;
            }
        }
        if m < 0 {
            m = 0;
        }
        self.update_links(m);
        let mut v = m;
        if self.powered {
            v |= w.meta(x, y, z) & 8;
        }
        if force || w.meta(x, y, z) != v {
            w.set_meta(x, y, z, v, 3);
            for i in 0..self.links.len() {
                let p = self.links[i];
                if let Some(mut nb) = neighbour(w, p) {
                    nb.remove_soft_links(w);
                    if nb.can_connect(self) {
                        nb.connect(w, self);
                    }
                }
            }
        }
    }
}

/// `getNeighborRail`
fn neighbour(w: &mut ServerWorld, p: (i32, i32, i32)) -> Option<Rail> {
    let (x, y, z) = p;
    if rail_at(w, x, y, z) {
        Some(Rail::new(w, x, y, z))
    } else if rail_at(w, x, y + 1, z) {
        Some(Rail::new(w, x, y + 1, z))
    } else if rail_at(w, x, y - 1, z) {
        Some(Rail::new(w, x, y - 1, z))
    } else {
        None
    }
}

/// `BlockRailBase.refreshTrackShape` (no redstone power is simulated).
pub fn refresh_shape(w: &mut ServerWorld, x: i32, y: i32, z: i32, force: bool) {
    let mut r = Rail::new(w, x, y, z);
    r.update_state(w, false, force);
}

/// `BlockRailBase.onNeighborBlockChange`
pub fn neighbour_changed(w: &mut ServerWorld, x: i32, y: i32, z: i32, src: u8) {
    let id = w.block(x, y, z);
    let m = w.meta(x, y, z);
    let shape = if id == block::RAIL { m } else { m & 7 };
    let mut drop = !w.solid_top_surface(x, y - 1, z);
    if shape == 2 && !w.solid_top_surface(x + 1, y, z) {
        drop = true;
    }
    if shape == 3 && !w.solid_top_surface(x - 1, y, z) {
        drop = true;
    }
    if shape == 4 && !w.solid_top_surface(x, y, z - 1) {
        drop = true;
    }
    if shape == 5 && !w.solid_top_surface(x, y, z + 1) {
        drop = true;
    }
    if drop {
        w.gen_entities.push(GenEntity::Item {
            pos: [x as f64 + 0.5, y as f64 + 0.5, z as f64 + 0.5],
            stack: crate::item::Stack::new(id as u16, 0, 1),
        });
        w.set_block(x, y, z, block::AIR, 0, 3);
    } else if id == block::RAIL {
        // BlockRail.func_150048_a
        let power_source = matches!(
            src,
            block::REDSTONE_TORCH
                | block::LEVER
                | block::STONE_BUTTON
                | block::WOODEN_BUTTON
                | block::STONE_PRESSURE_PLATE
                | block::WOODEN_PRESSURE_PLATE
                | block::REDSTONE_WIRE
                | block::POWERED_REPEATER
                | block::UNPOWERED_REPEATER
                | block::REDSTONE_BLOCK
                | block::DETECTOR_RAIL
                | block::TRIPWIRE_HOOK
                | block::UNLIT_REDSTONE_TORCH
        );
        if power_source && Rail::new(w, x, y, z).adjacent(w) == 3 {
            refresh_shape(w, x, y, z, false);
        }
    }
}
