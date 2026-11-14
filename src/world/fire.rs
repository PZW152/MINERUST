//! `BlockFire` (spreading, burning out, eternal netherrack fire) and the
//! frame logic of `BlockPortal` (`BlockPortal$Size`).

use super::server::ServerWorld;
use crate::block::{self, *};
use crate::java::JavaRandom;

/// `BlockFire.encouragements` / `flammabilities` (`BlockFire.init`).
fn burn(id: u8) -> (i32, i32) {
    match id {
        PLANKS | DOUBLE_WOODEN_SLAB | WOODEN_SLAB | FENCE | OAK_STAIRS | BIRCH_STAIRS
        | SPRUCE_STAIRS | JUNGLE_STAIRS => (5, 20),
        LOG | LOG2 | COAL_BLOCK => (5, 5),
        LEAVES | LEAVES2 | WOOL => (30, 60),
        BOOKSHELF => (30, 20),
        TNT => (15, 100),
        TALLGRASS | DOUBLE_PLANT | YELLOW_FLOWER | RED_FLOWER => (60, 100),
        VINE => (15, 100),
        HAY_BLOCK | CARPET => (60, 20),
        _ => (0, 0),
    }
}

/// `Direction.offsetX` / `offsetZ`
const DX: [i32; 4] = [0, -1, 0, 1];
const DZ: [i32; 4] = [1, 0, -1, 0];
/// `BlockPortal.CARDINAL_DIRECTIONS[axis]`: left and right directions.
const CARDINAL: [[usize; 2]; 3] = [[0, 0], [3, 1], [2, 0]];

impl ServerWorld {
    fn catches(&mut self, x: i32, y: i32, z: i32) -> bool {
        burn(self.block(x, y, z)).0 > 0
    }

    /// `BlockFire.canNeighborBurn`
    fn neighbour_burns(&mut self, x: i32, y: i32, z: i32) -> bool {
        self.catches(x + 1, y, z)
            || self.catches(x - 1, y, z)
            || self.catches(x, y - 1, z)
            || self.catches(x, y + 1, z)
            || self.catches(x, y, z - 1)
            || self.catches(x, y, z + 1)
    }

    /// `World.canLightningStrikeAt`
    fn rain_at(&mut self, x: i32, y: i32, z: i32) -> bool {
        self.can_lightning_strike_at(x, y, z)
    }

    /// `World.canLightningStrikeAt`: rain falls here (no snow biome, not
    /// cold enough to snow, `canSpawnLightningBolt`).
    pub fn can_lightning_strike_at(&mut self, x: i32, y: i32, z: i32) -> bool {
        if !self.raining || !self.can_see_sky(x, y, z) {
            return false;
        }
        let top = self.precipitation_height(x, z);
        if top > y {
            return false;
        }
        let b = self.biome(x, z);
        if b.enable_snow {
            return false;
        }
        if self.can_snow_at(x, y, z, false) {
            return false;
        }
        b.enable_rain
    }

    fn high_humidity(&mut self, x: i32, z: i32) -> bool {
        matches!(self.biome(x, z).id & 127, 6 | 14 | 15 | 21 | 22 | 23)
    }

    /// `BlockFire.onBlockAdded`
    pub(crate) fn fire_added(&mut self, x: i32, y: i32, z: i32) {
        if self.provider.dim > 0 || !self.try_create_portal(x, y, z) {
            if !self.solid_top_surface(x, y - 1, z) && !self.neighbour_burns(x, y, z) {
                self.set_block(x, y, z, AIR, 0, 3);
            } else {
                let d = 30 + self.rand.next_int_n(10);
                self.schedule_update(x, y, z, FIRE, d);
            }
        }
    }

    /// `BlockFire.onNeighborBlockChange`
    pub(crate) fn fire_neighbour(&mut self, x: i32, y: i32, z: i32) {
        if !self.solid_top_surface(x, y - 1, z) && !self.neighbour_burns(x, y, z) {
            self.set_block(x, y, z, AIR, 0, 3);
        }
    }

    /// `BlockFire.updateTick`
    pub(crate) fn fire_tick(&mut self, x: i32, y: i32, z: i32, r: &mut JavaRandom) {
        let below = self.block(x, y - 1, z);
        let eternal = below == NETHERRACK || (self.provider.dim == 1 && below == BEDROCK);
        if !(self.solid_top_surface(x, y - 1, z) || self.neighbour_burns(x, y, z)) {
            self.set_block(x, y, z, AIR, 0, 3);
        }
        let rained = !eternal
            && self.raining
            && (self.rain_at(x, y, z)
                || self.rain_at(x - 1, y, z)
                || self.rain_at(x + 1, y, z)
                || self.rain_at(x, y, z - 1)
                || self.rain_at(x, y, z + 1));
        if rained {
            self.set_block(x, y, z, AIR, 0, 3);
            return;
        }
        let age = self.meta(x, y, z);
        if age < 15 {
            self.set_meta(x, y, z, age + r.next_int_n(3) / 2, 4);
        }
        let d = 30 + r.next_int_n(10);
        self.schedule_update(x, y, z, FIRE, d);
        if !eternal && !self.neighbour_burns(x, y, z) {
            if !self.solid_top_surface(x, y - 1, z) || age > 3 {
                self.set_block(x, y, z, AIR, 0, 3);
            }
            return;
        }
        if !eternal && !self.catches(x, y - 1, z) && age == 15 && r.next_int_n(4) == 0 {
            self.set_block(x, y, z, AIR, 0, 3);
            return;
        }
        let humid = self.high_humidity(x, z);
        let h = if humid { -50 } else { 0 };
        self.try_catch(x + 1, y, z, 300 + h, r, age);
        self.try_catch(x - 1, y, z, 300 + h, r, age);
        self.try_catch(x, y - 1, z, 250 + h, r, age);
        self.try_catch(x, y + 1, z, 250 + h, r, age);
        self.try_catch(x, y, z - 1, 300 + h, r, age);
        self.try_catch(x, y, z + 1, 300 + h, r, age);
        for fx in x - 1..=x + 1 {
            for fz in z - 1..=z + 1 {
                for fy in y - 1..=y + 4 {
                    if fx == x && fy == y && fz == z {
                        continue;
                    }
                    let mut chance = 100;
                    if fy > y + 1 {
                        chance += (fy - (y + 1)) * 100;
                    }
                    let enc = self.encouragement(fx, fy, fz);
                    if enc > 0 {
                        // normal difficulty (2)
                        let mut n = (enc + 40 + 2 * 7) / (age + 30);
                        if humid {
                            n /= 2;
                        }
                        if n > 0
                            && r.next_int_n(chance) <= n
                            && !(self.raining && self.rain_at(fx, fy, fz))
                            && !self.rain_at(fx - 1, fy, z)
                            && !self.rain_at(fx + 1, fy, fz)
                            && !self.rain_at(fx, fy, fz - 1)
                            && !self.rain_at(fx, fy, fz + 1)
                        {
                            let a = (age + r.next_int_n(5) / 4).min(15);
                            self.set_block(fx, fy, fz, FIRE, a, 3);
                        }
                    }
                }
            }
        }
    }

    /// `BlockFire.tryCatchFire`
    fn try_catch(&mut self, x: i32, y: i32, z: i32, chance: i32, r: &mut JavaRandom, age: i32) {
        let id = self.block(x, y, z);
        let flam = burn(id).1;
        if r.next_int_n(chance) < flam {
            if r.next_int_n(age + 10) < 5 && !self.rain_at(x, y, z) {
                let a = (age + r.next_int_n(5) / 4).min(15);
                self.set_block(x, y, z, FIRE, a, 3);
            } else {
                self.set_block(x, y, z, AIR, 0, 3);
            }
            if id == TNT {
                self.prime_tnt(x, y, z, 80);
                self.set_block(x, y, z, AIR, 0, 3);
            }
        }
    }

    /// `BlockFire.getChanceOfNeighborsEncouragingFire`
    fn encouragement(&mut self, x: i32, y: i32, z: i32) -> i32 {
        if !self.is_air(x, y, z) {
            return 0;
        }
        let mut e = 0;
        for (dx, dy, dz) in [
            (1, 0, 0),
            (-1, 0, 0),
            (0, -1, 0),
            (0, 1, 0),
            (0, 0, -1),
            (0, 0, 1),
        ] {
            e = e.max(burn(self.block(x + dx, y + dy, z + dz)).0);
        }
        e
    }

    // ------------------------------------------------------------ portals

    fn portal_empty(id: u8) -> bool {
        block::material(id) == block::Material::Air || id == FIRE || id == PORTAL
    }

    /// `BlockPortal$Size`: `(bottom left, width, height, portal blocks)`.
    fn portal_size(
        &mut self,
        x: i32,
        mut y: i32,
        z: i32,
        axis: usize,
    ) -> Option<([i32; 3], i32, i32, i32)> {
        let left = CARDINAL[axis][0];
        let right = CARDINAL[axis][1];
        let y0 = y;
        while y > y0 - 21 && y > 0 && Self::portal_empty(self.block(x, y - 1, z)) {
            y -= 1;
        }
        let find_width = |w: &mut ServerWorld, x: i32, y: i32, z: i32, d: usize| {
            let mut n = 0;
            while n < 22 {
                let id = w.block(x + DX[d] * n, y, z + DZ[d] * n);
                if !Self::portal_empty(id) {
                    break;
                }
                if w.block(x + DX[d] * n, y - 1, z + DZ[d] * n) != OBSIDIAN {
                    break;
                }
                n += 1;
            }
            if w.block(x + DX[d] * n, y, z + DZ[d] * n) == OBSIDIAN {
                n
            } else {
                0
            }
        };
        let k = find_width(self, x, y, z, left) - 1;
        if k < 0 {
            return None;
        }
        let bl = [x + k * DX[left], y, z + k * DZ[left]];
        let width = find_width(self, bl[0], bl[1], bl[2], right);
        if !(2..=21).contains(&width) {
            return None;
        }
        // calculatePortalHeight
        let mut count = 0;
        let mut height = 0;
        'outer: while height < 21 {
            let py = bl[1] + height;
            for i in 0..width {
                let px = bl[0] + i * DX[right];
                let pz = bl[2] + i * DZ[right];
                let id = self.block(px, py, pz);
                if !Self::portal_empty(id) {
                    break 'outer;
                }
                if id == PORTAL {
                    count += 1;
                }
                if i == 0 {
                    if self.block(px + DX[left], py, pz + DZ[left]) != OBSIDIAN {
                        break 'outer;
                    }
                } else if i == width - 1
                    && self.block(px + DX[right], py, pz + DZ[right]) != OBSIDIAN
                {
                    break 'outer;
                }
            }
            height += 1;
        }
        for i in 0..width {
            let px = bl[0] + i * DX[right];
            let pz = bl[2] + i * DZ[right];
            if self.block(px, bl[1] + height, pz) != OBSIDIAN {
                height = 0;
                break;
            }
        }
        if (3..=21).contains(&height) {
            Some((bl, width, height, count))
        } else {
            None
        }
    }

    /// `BlockPortal.tryToCreatePortal`
    pub fn try_create_portal(&mut self, x: i32, y: i32, z: i32) -> bool {
        for axis in [1usize, 2] {
            if let Some((bl, w, h, count)) = self.portal_size(x, y, z, axis) {
                if count == 0 {
                    let right = CARDINAL[axis][1];
                    for i in 0..w {
                        let px = bl[0] + DX[right] * i;
                        let pz = bl[2] + DZ[right] * i;
                        for j in 0..h {
                            self.set_block(px, bl[1] + j, pz, PORTAL, axis as i32, 2);
                        }
                    }
                    return true;
                }
            }
        }
        false
    }

    /// `BlockPortal.onNeighborBlockChange`: a broken frame empties the portal.
    pub(crate) fn portal_neighbour(&mut self, x: i32, y: i32, z: i32) {
        let axis = self.meta(x, y, z) & 3;
        let full =
            |s: Option<([i32; 3], i32, i32, i32)>| s.map_or(false, |(_, w, h, c)| c >= w * h);
        let a = self.portal_size(x, y, z, 1);
        let b = self.portal_size(x, y, z, 2);
        let broken = (axis == 1 && !full(a))
            || (axis == 2 && !full(b))
            || (axis == 0 && a.is_none() && b.is_none());
        if broken {
            self.set_block(x, y, z, AIR, 0, 3);
        }
    }
}
