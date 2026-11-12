//! `MapGenStructure`, `StructureStart`, `StructureComponent` and
//! `StructureBoundingBox`, with the four overworld structure types.

pub mod mineshaft;
pub mod scattered;
pub mod stronghold;
pub mod village;

use crate::block;
use crate::item::Stack;
use crate::java::JavaRandom;
use crate::world::gen::chunk_provider::ChunkProvider;
use crate::world::loot::{self, Loot};
use crate::world::server::{GenEntity, GenTile, ServerWorld};
use std::collections::HashSet;

/// `StructureBoundingBox`
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct BB {
    pub x0: i32,
    pub y0: i32,
    pub z0: i32,
    pub x1: i32,
    pub y1: i32,
    pub z1: i32,
}

impl BB {
    pub const fn new(x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32) -> BB {
        BB {
            x0,
            y0,
            z0,
            x1,
            y1,
            z1,
        }
    }
    /// The 2D constructor: `minY = 1`, `maxY = 512`.
    pub const fn flat(x0: i32, z0: i32, x1: i32, z1: i32) -> BB {
        BB {
            x0,
            y0: 1,
            z0,
            x1,
            y1: 512,
            z1,
        }
    }
    pub const fn empty() -> BB {
        BB {
            x0: i32::MAX,
            y0: i32::MAX,
            z0: i32::MAX,
            x1: i32::MIN,
            y1: i32::MIN,
            z1: i32::MIN,
        }
    }
    /// `getComponentToAddBoundingBox`
    #[allow(clippy::too_many_arguments)]
    pub fn component(
        x: i32,
        y: i32,
        z: i32,
        ox: i32,
        oy: i32,
        oz: i32,
        w: i32,
        h: i32,
        d: i32,
        mode: i32,
    ) -> BB {
        match mode {
            1 => BB::new(
                x - d + 1 + oz,
                y + oy,
                z + ox,
                x + oz,
                y + h - 1 + oy,
                z + w - 1 + ox,
            ),
            2 => BB::new(
                x + ox,
                y + oy,
                z - d + 1 + oz,
                x + w - 1 + ox,
                y + h - 1 + oy,
                z + oz,
            ),
            3 => BB::new(
                x + oz,
                y + oy,
                z + ox,
                x + d - 1 + oz,
                y + h - 1 + oy,
                z + w - 1 + ox,
            ),
            _ => BB::new(
                x + ox,
                y + oy,
                z + oz,
                x + w - 1 + ox,
                y + h - 1 + oy,
                z + d - 1 + oz,
            ),
        }
    }
    pub fn intersects(&self, o: &BB) -> bool {
        self.x1 >= o.x0
            && self.x0 <= o.x1
            && self.z1 >= o.z0
            && self.z0 <= o.z1
            && self.y1 >= o.y0
            && self.y0 <= o.y1
    }
    pub fn intersects_xz(&self, x0: i32, z0: i32, x1: i32, z1: i32) -> bool {
        self.x1 >= x0 && self.x0 <= x1 && self.z1 >= z0 && self.z0 <= z1
    }
    pub fn expand(&mut self, o: &BB) {
        self.x0 = self.x0.min(o.x0);
        self.y0 = self.y0.min(o.y0);
        self.z0 = self.z0.min(o.z0);
        self.x1 = self.x1.max(o.x1);
        self.y1 = self.y1.max(o.y1);
        self.z1 = self.z1.max(o.z1);
    }
    pub fn offset(&mut self, x: i32, y: i32, z: i32) {
        self.x0 += x;
        self.y0 += y;
        self.z0 += z;
        self.x1 += x;
        self.y1 += y;
        self.z1 += z;
    }
    pub fn inside(&self, x: i32, y: i32, z: i32) -> bool {
        x >= self.x0 && x <= self.x1 && z >= self.z0 && z <= self.z1 && y >= self.y0 && y <= self.y1
    }
    pub fn xs(&self) -> i32 {
        self.x1 - self.x0 + 1
    }
    pub fn ys(&self) -> i32 {
        self.y1 - self.y0 + 1
    }
    pub fn zs(&self) -> i32 {
        self.z1 - self.z0 + 1
    }
    pub fn cx(&self) -> i32 {
        self.x0 + (self.x1 - self.x0 + 1) / 2
    }
    pub fn cy(&self) -> i32 {
        self.y0 + (self.y1 - self.y0 + 1) / 2
    }
    pub fn cz(&self) -> i32 {
        self.z0 + (self.z1 - self.z0 + 1) / 2
    }
    fn ints(&self) -> [i32; 6] {
        [self.x0, self.y0, self.z0, self.x1, self.y1, self.z1]
    }
    fn from_ints(v: &[i32]) -> BB {
        BB::new(v[0], v[1], v[2], v[3], v[4], v[5])
    }
}

/// `StructureComponent.findIntersecting`
pub fn find_intersecting<'a>(list: &'a [Piece], bb: &BB) -> Option<&'a Piece> {
    list.iter().find(|p| p.bb.intersects(bb))
}

/// `MathHelper.getRandomIntegerInRange`
pub fn rand_range(r: &mut JavaRandom, a: i32, b: i32) -> i32 {
    if a >= b {
        a
    } else {
        r.next_int_n(b - a + 1) + a
    }
}

/// The type-specific part of a component.
#[derive(Clone, Debug)]
pub enum Kind {
    Mine(mineshaft::Mine),
    Village(village::VPiece),
    Hold(stronghold::Hold),
    Scatter(scattered::Scatter),
}

/// `StructureComponent`
#[derive(Clone, Debug)]
pub struct Piece {
    pub bb: BB,
    /// `coordBaseMode`
    pub mode: i32,
    /// `componentType` (generation depth)
    pub ty: i32,
    pub kind: Kind,
}

/// `Block.isNormalCube` / `isFullBlock` shortcuts.
fn full(id: u8) -> bool {
    block::info(id).full_block
}

impl Piece {
    pub fn new(ty: i32, mode: i32, bb: BB, kind: Kind) -> Piece {
        Piece { bb, mode, ty, kind }
    }

    pub fn x_off(&self, x: i32, z: i32) -> i32 {
        match self.mode {
            0 | 2 => self.bb.x0 + x,
            1 => self.bb.x1 - z,
            3 => self.bb.x0 + z,
            _ => x,
        }
    }
    pub fn y_off(&self, y: i32) -> i32 {
        if self.mode == -1 {
            y
        } else {
            y + self.bb.y0
        }
    }
    pub fn z_off(&self, x: i32, z: i32) -> i32 {
        match self.mode {
            0 => self.bb.z0 + z,
            1 | 3 => self.bb.z0 + x,
            2 => self.bb.z1 - z,
            _ => z,
        }
    }

    /// `getMetadataWithOffset`
    pub fn mo(&self, id: u8, m: i32) -> i32 {
        let mode = self.mode;
        if id == block::RAIL {
            if mode == 1 || mode == 3 {
                return if m == 1 { 0 } else { 1 };
            }
            return m;
        }
        if id == block::WOODEN_DOOR || id == block::IRON_DOOR {
            if mode == 0 {
                if m == 0 {
                    return 2;
                }
                if m == 2 {
                    return 0;
                }
            } else if mode == 1 {
                return (m + 1) & 3;
            } else if mode == 3 {
                return (m + 3) & 3;
            }
            return m;
        }
        if block::is_stairs(id) {
            match mode {
                0 => match m {
                    2 => return 3,
                    3 => return 2,
                    _ => {}
                },
                1 => match m {
                    0 => return 2,
                    1 => return 3,
                    2 => return 0,
                    3 => return 1,
                    _ => {}
                },
                3 => match m {
                    0 => return 2,
                    1 => return 3,
                    2 => return 1,
                    3 => return 0,
                    _ => {}
                },
                _ => {}
            }
            return m;
        }
        if id == block::LADDER {
            match mode {
                0 => match m {
                    2 => return 3,
                    3 => return 2,
                    _ => {}
                },
                1 | 3 => {
                    let t = if mode == 1 {
                        [4, 5, 2, 3]
                    } else {
                        [5, 4, 2, 3]
                    };
                    if (2..=5).contains(&m) {
                        return t[(m - 2) as usize];
                    }
                }
                _ => {}
            }
            return m;
        }
        if id == block::STONE_BUTTON {
            match mode {
                0 => match m {
                    3 => return 4,
                    4 => return 3,
                    _ => {}
                },
                1 => match m {
                    3 => return 1,
                    4 => return 2,
                    2 => return 3,
                    1 => return 4,
                    _ => {}
                },
                3 => match m {
                    3 => return 2,
                    4 => return 1,
                    2 => return 3,
                    1 => return 4,
                    _ => {}
                },
                _ => {}
            }
            return m;
        }
        // BlockDirectional: tripwire hook, repeaters, pumpkins, fence gates, beds...
        if matches!(
            id,
            block::TRIPWIRE_HOOK
                | block::UNPOWERED_REPEATER
                | block::POWERED_REPEATER
                | block::PUMPKIN
                | block::LIT_PUMPKIN
                | block::COCOA
        ) {
            match mode {
                0 => {
                    if m == 0 || m == 2 {
                        return [2, 3, 0, 1][m as usize];
                    }
                }
                1 => match m {
                    2 => return 1,
                    0 => return 3,
                    1 => return 2,
                    3 => return 0,
                    _ => {}
                },
                3 => match m {
                    2 => return 3,
                    0 => return 1,
                    1 => return 2,
                    3 => return 0,
                    _ => {}
                },
                _ => {}
            }
            return m;
        }
        if matches!(
            id,
            block::PISTON | block::STICKY_PISTON | block::LEVER | block::DISPENSER
        ) {
            match mode {
                0 => {
                    if m == 2 || m == 3 {
                        return [1, 0, 3, 2, 5, 4][m as usize];
                    }
                }
                1 => match m {
                    2 => return 4,
                    3 => return 5,
                    4 => return 2,
                    5 => return 3,
                    _ => {}
                },
                3 => match m {
                    2 => return 5,
                    3 => return 4,
                    4 => return 2,
                    5 => return 3,
                    _ => {}
                },
                _ => {}
            }
        }
        m
    }

    /// `placeBlockAtCurrentPosition`
    pub fn pb(&self, w: &mut ServerWorld, id: u8, meta: i32, x: i32, y: i32, z: i32, clip: &BB) {
        let (wx, wy, wz) = (self.x_off(x, z), self.y_off(y), self.z_off(x, z));
        if clip.inside(wx, wy, wz) {
            w.set_block(wx, wy, wz, id, meta, 2);
        }
    }

    /// `getBlockAtCurrentPosition`
    pub fn get(&self, w: &mut ServerWorld, x: i32, y: i32, z: i32, clip: &BB) -> u8 {
        let (wx, wy, wz) = (self.x_off(x, z), self.y_off(y), self.z_off(x, z));
        if !clip.inside(wx, wy, wz) {
            block::AIR
        } else {
            w.block(wx, wy, wz)
        }
    }

    fn air_at(&self, w: &mut ServerWorld, x: i32, y: i32, z: i32, clip: &BB) -> bool {
        block::material(self.get(w, x, y, z, clip)) == block::Material::Air
    }

    pub fn fill_air(
        &self,
        w: &mut ServerWorld,
        clip: &BB,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
    ) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    self.pb(w, block::AIR, 0, x, y, z, clip);
                }
            }
        }
    }

    /// `fillWithMetadataBlocks` (and `fillWithBlocks` with metadata 0).
    #[allow(clippy::too_many_arguments)]
    pub fn fill_meta(
        &self,
        w: &mut ServerWorld,
        clip: &BB,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        edge: u8,
        em: i32,
        inner: u8,
        im: i32,
        keep_air: bool,
    ) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    if !keep_air || !self.air_at(w, x, y, z, clip) {
                        if y != y0 && y != y1 && x != x0 && x != x1 && z != z0 && z != z1 {
                            self.pb(w, inner, im, x, y, z, clip);
                        } else {
                            self.pb(w, edge, em, x, y, z, clip);
                        }
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn fill(
        &self,
        w: &mut ServerWorld,
        clip: &BB,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        edge: u8,
        inner: u8,
        keep_air: bool,
    ) {
        self.fill_meta(w, clip, x0, y0, z0, x1, y1, z1, edge, 0, inner, 0, keep_air);
    }

    /// `fillWithRandomizedBlocks`: `sel(rand, x, y, z, edge) -> (id, meta)`.
    #[allow(clippy::too_many_arguments)]
    pub fn fill_random(
        &self,
        w: &mut ServerWorld,
        clip: &BB,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        keep_air: bool,
        r: &mut JavaRandom,
        sel: &dyn Fn(&mut JavaRandom, bool) -> (u8, i32),
    ) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    if !keep_air || !self.air_at(w, x, y, z, clip) {
                        let edge = y == y0 || y == y1 || x == x0 || x == x1 || z == z0 || z == z1;
                        let (id, m) = sel(r, edge);
                        self.pb(w, id, m, x, y, z, clip);
                    }
                }
            }
        }
    }

    /// `randomlyFillWithBlocks`
    #[allow(clippy::too_many_arguments)]
    pub fn fill_chance(
        &self,
        w: &mut ServerWorld,
        clip: &BB,
        r: &mut JavaRandom,
        chance: f32,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        edge: u8,
        inner: u8,
        keep_air: bool,
    ) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    if !(r.next_float() > chance) && (!keep_air || !self.air_at(w, x, y, z, clip)) {
                        if y != y0 && y != y1 && x != x0 && x != x1 && z != z0 && z != z1 {
                            self.pb(w, inner, 0, x, y, z, clip);
                        } else {
                            self.pb(w, edge, 0, x, y, z, clip);
                        }
                    }
                }
            }
        }
    }

    /// `setBlockWithThreshold`
    #[allow(clippy::too_many_arguments)]
    pub fn maybe(
        &self,
        w: &mut ServerWorld,
        clip: &BB,
        r: &mut JavaRandom,
        chance: f32,
        x: i32,
        y: i32,
        z: i32,
        id: u8,
        meta: i32,
    ) {
        if r.next_float() < chance {
            self.pb(w, id, meta, x, y, z, clip);
        }
    }

    /// `placeUpperHemisphere`
    #[allow(clippy::too_many_arguments)]
    pub fn hemisphere(
        &self,
        w: &mut ServerWorld,
        clip: &BB,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        id: u8,
        keep_air: bool,
    ) {
        let fx = (x1 - x0 + 1) as f32;
        let fy = (y1 - y0 + 1) as f32;
        let fz = (z1 - z0 + 1) as f32;
        let cx = x0 as f32 + fx / 2.0f32;
        let cz = z0 as f32 + fz / 2.0f32;
        for y in y0..=y1 {
            let dy = (y - y0) as f32 / fy;
            for x in x0..=x1 {
                let dx = (x as f32 - cx) / (fx * 0.5f32);
                for z in z0..=z1 {
                    let dz = (z as f32 - cz) / (fz * 0.5f32);
                    if !keep_air || !self.air_at(w, x, y, z, clip) {
                        let d = dx * dx + dy * dy + dz * dz;
                        if d <= 1.05f32 {
                            self.pb(w, id, 0, x, y, z, clip);
                        }
                    }
                }
            }
        }
    }

    /// `clearCurrentPositionBlocksUpwards`
    pub fn clear_up(&self, w: &mut ServerWorld, x: i32, y: i32, z: i32, clip: &BB) {
        let (wx, mut wy, wz) = (self.x_off(x, z), self.y_off(y), self.z_off(x, z));
        if clip.inside(wx, wy, wz) {
            while !w.is_air(wx, wy, wz) && wy < 255 {
                w.set_block(wx, wy, wz, block::AIR, 0, 2);
                wy += 1;
            }
        }
    }

    /// `fillColumnDown`
    #[allow(clippy::too_many_arguments)]
    pub fn fill_down(
        &self,
        w: &mut ServerWorld,
        id: u8,
        meta: i32,
        x: i32,
        y: i32,
        z: i32,
        clip: &BB,
    ) {
        let (wx, mut wy, wz) = (self.x_off(x, z), self.y_off(y), self.z_off(x, z));
        if clip.inside(wx, wy, wz) {
            while (w.is_air(wx, wy, wz) || w.material(wx, wy, wz).is_liquid()) && wy > 1 {
                w.set_block(wx, wy, wz, id, meta, 2);
                wy -= 1;
            }
        }
    }

    /// `isLiquidInStructureBoundingBox`
    pub fn liquid_in(&self, w: &mut ServerWorld, clip: &BB) -> bool {
        let x0 = (self.bb.x0 - 1).max(clip.x0);
        let y0 = (self.bb.y0 - 1).max(clip.y0);
        let z0 = (self.bb.z0 - 1).max(clip.z0);
        let x1 = (self.bb.x1 + 1).min(clip.x1);
        let y1 = (self.bb.y1 + 1).min(clip.y1);
        let z1 = (self.bb.z1 + 1).min(clip.z1);
        for x in x0..=x1 {
            for z in z0..=z1 {
                if w.material(x, y0, z).is_liquid() || w.material(x, y1, z).is_liquid() {
                    return true;
                }
            }
        }
        for x in x0..=x1 {
            for y in y0..=y1 {
                if w.material(x, y, z0).is_liquid() || w.material(x, y, z1).is_liquid() {
                    return true;
                }
            }
        }
        for z in z0..=z1 {
            for y in y0..=y1 {
                if w.material(x0, y, z).is_liquid() || w.material(x1, y, z).is_liquid() {
                    return true;
                }
            }
        }
        false
    }

    /// `generateStructureChestContents`
    #[allow(clippy::too_many_arguments)]
    pub fn chest(
        &self,
        w: &mut ServerWorld,
        clip: &BB,
        r: &mut JavaRandom,
        x: i32,
        y: i32,
        z: i32,
        table: &[Loot],
        count: i32,
    ) -> bool {
        let (wx, wy, wz) = (self.x_off(x, z), self.y_off(y), self.z_off(x, z));
        if clip.inside(wx, wy, wz) && w.block(wx, wy, wz) != block::CHEST {
            w.set_block(wx, wy, wz, block::CHEST, 0, 2);
            let mut slots: Vec<Option<Stack>> = vec![None; 27];
            loot::fill(r, table, &mut slots, count);
            w.gen_tiles.push(GenTile::Chest {
                pos: [wx, wy, wz],
                slots,
            });
            true
        } else {
            false
        }
    }

    /// `generateStructureDispenserContents`
    #[allow(clippy::too_many_arguments)]
    pub fn dispenser(
        &self,
        w: &mut ServerWorld,
        clip: &BB,
        r: &mut JavaRandom,
        x: i32,
        y: i32,
        z: i32,
        facing: i32,
        table: &[Loot],
        count: i32,
    ) -> bool {
        let (wx, wy, wz) = (self.x_off(x, z), self.y_off(y), self.z_off(x, z));
        if clip.inside(wx, wy, wz) && w.block(wx, wy, wz) != block::DISPENSER {
            let m = self.mo(block::DISPENSER, facing);
            w.set_block(wx, wy, wz, block::DISPENSER, m, 2);
            let mut slots: Vec<Option<Stack>> = vec![None; 9];
            loot::fill(r, table, &mut slots, count);
            w.gen_tiles.push(GenTile::Dispenser {
                pos: [wx, wy, wz],
                slots,
            });
            true
        } else {
            false
        }
    }

    /// `placeDoorAtCurrentPosition` (`ItemDoor.placeDoorBlock` with a wooden door).
    pub fn door(&self, w: &mut ServerWorld, clip: &BB, x: i32, y: i32, z: i32, facing: i32) {
        let (wx, wy, wz) = (self.x_off(x, z), self.y_off(y), self.z_off(x, z));
        if clip.inside(wx, wy, wz) {
            place_door(w, wx, wy, wz, facing, block::WOODEN_DOOR);
        }
    }

    pub fn spawner(&self, w: &mut ServerWorld, wx: i32, wy: i32, wz: i32, mob: &'static str) {
        w.set_block(wx, wy, wz, block::MOB_SPAWNER, 0, 2);
        w.gen_tiles.push(GenTile::Spawner {
            pos: [wx, wy, wz],
            mob,
        });
    }

    fn save(&self, out: &mut Vec<i32>) {
        out.extend_from_slice(&self.bb.ints());
        out.push(self.mode);
        out.push(self.ty);
        match &self.kind {
            Kind::Mine(m) => {
                out.push(0);
                m.save(out);
            }
            Kind::Village(v) => {
                out.push(1);
                v.save(out);
            }
            Kind::Hold(h) => {
                out.push(2);
                h.save(out);
            }
            Kind::Scatter(s) => {
                out.push(3);
                s.save(out);
            }
        }
    }

    fn load(v: &mut std::slice::Iter<i32>) -> Option<Piece> {
        let mut take = || v.next().copied();
        let mut b = [0i32; 6];
        for x in b.iter_mut() {
            *x = take()?;
        }
        let mode = take()?;
        let ty = take()?;
        let tag = take()?;
        let kind = match tag {
            0 => Kind::Mine(mineshaft::Mine::load(v)?),
            1 => Kind::Village(village::VPiece::load(v)?),
            2 => Kind::Hold(stronghold::Hold::load(v)?),
            3 => Kind::Scatter(scattered::Scatter::load(v)?),
            _ => return None,
        };
        Some(Piece {
            bb: BB::from_ints(&b),
            mode,
            ty,
            kind,
        })
    }
}

/// `ItemDoor.placeDoorBlock`
pub fn place_door(w: &mut ServerWorld, x: i32, y: i32, z: i32, facing: i32, door: u8) {
    let (mut dx, mut dz) = (0, 0);
    match facing {
        0 => dz = 1,
        1 => dx = -1,
        2 => dz = -1,
        3 => dx = 1,
        _ => {}
    }
    let nc = |w: &mut ServerWorld, x: i32, y: i32, z: i32| {
        block::info(w.block(x, y, z)).normal_cube() as i32
    };
    let left = nc(w, x - dx, y, z - dz) + nc(w, x - dx, y + 1, z - dz);
    let right = nc(w, x + dx, y, z + dz) + nc(w, x + dx, y + 1, z + dz);
    let door_l = w.block(x - dx, y, z - dz) == door || w.block(x - dx, y + 1, z - dz) == door;
    let door_r = w.block(x + dx, y, z + dz) == door || w.block(x + dx, y + 1, z + dz) == door;
    let flip = (door_l && !door_r) || right > left;
    w.set_block(x, y, z, door, facing, 2);
    w.set_block(x, y + 1, z, door, 8 | flip as i32, 2);
    w.notify_neighbours(x, y, z, door);
    w.notify_neighbours(x, y + 1, z, door);
}

/// What kind of structure a start is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Type {
    Mineshaft,
    Village,
    Stronghold,
    Temple,
}

/// `StructureStart`
#[derive(Clone, Debug)]
pub struct Start {
    pub cx: i32,
    pub cz: i32,
    pub bb: BB,
    pub pieces: Vec<Piece>,
    /// `isSizeableStructure`
    pub sizeable: bool,
}

impl Start {
    /// `updateBoundingBox`
    pub fn update_bb(&mut self) {
        let mut b = BB::empty();
        for p in &self.pieces {
            b.expand(&p.bb);
        }
        self.bb = b;
    }

    /// `markAvailableHeight`
    pub fn mark_available_height(&mut self, r: &mut JavaRandom, sea_offset: i32) {
        let limit = 63 - sea_offset;
        let mut h = self.bb.ys() + 1;
        if h < limit {
            h += r.next_int_n(limit - h);
        }
        let dy = h - self.bb.y1;
        self.bb.offset(0, dy, 0);
        for p in &mut self.pieces {
            p.bb.offset(0, dy, 0);
        }
    }

    /// `setRandomHeight`
    pub fn set_random_height(&mut self, r: &mut JavaRandom, lo: i32, hi: i32) {
        let room = hi - lo + 1 - self.bb.ys();
        let y = if room > 1 {
            lo + r.next_int_n(room)
        } else {
            lo
        };
        let dy = y - self.bb.y0;
        self.bb.offset(0, dy, 0);
        for p in &mut self.pieces {
            p.bb.offset(0, dy, 0);
        }
    }

    /// `generateStructure`
    fn generate(&mut self, w: &mut ServerWorld, r: &mut JavaRandom, clip: &BB) {
        let mut i = 0;
        while i < self.pieces.len() {
            if self.pieces[i].bb.intersects(clip) && !add_parts(&mut self.pieces, i, w, r, clip) {
                self.pieces.remove(i);
            } else {
                i += 1;
            }
        }
    }

    fn save(&self, out: &mut Vec<i32>) {
        out.push(self.cx);
        out.push(self.cz);
        out.extend_from_slice(&self.bb.ints());
        out.push(self.sizeable as i32);
        out.push(self.pieces.len() as i32);
        for p in &self.pieces {
            p.save(out);
        }
    }

    fn load(v: &mut std::slice::Iter<i32>) -> Option<Start> {
        let cx = *v.next()?;
        let cz = *v.next()?;
        let mut b = [0i32; 6];
        for x in b.iter_mut() {
            *x = *v.next()?;
        }
        let sizeable = *v.next()? != 0;
        let n = *v.next()? as usize;
        let mut pieces = Vec::with_capacity(n.min(4096));
        for _ in 0..n {
            pieces.push(Piece::load(v)?);
        }
        Some(Start {
            cx,
            cz,
            bb: BB::from_ints(&b),
            pieces,
            sizeable,
        })
    }
}

/// `addComponentParts` dispatch.
fn add_parts(
    pieces: &mut [Piece],
    i: usize,
    w: &mut ServerWorld,
    r: &mut JavaRandom,
    clip: &BB,
) -> bool {
    let mut p = pieces[i].clone();
    let ok = match &p.kind {
        Kind::Mine(_) => mineshaft::add_parts(&mut p, w, r, clip),
        Kind::Village(_) => village::add_parts(&mut p, w, r, clip),
        Kind::Hold(_) => stronghold::add_parts(&mut p, w, r, clip),
        Kind::Scatter(_) => scattered::add_parts(&mut p, w, r, clip),
    };
    pieces[i] = p;
    ok
}

/// `java.util.HashMap<Long, StructureStart>` with its iteration order: entries
/// are visited bucket by bucket (`spread(hash) & (capacity - 1)`), and within a
/// bucket in insertion order (resizes keep the relative order).
#[derive(Default)]
pub struct StartMap {
    entries: Vec<(i64, Start)>,
    keys: HashSet<i64>,
}

fn chunk_key(x: i32, z: i32) -> i64 {
    (x as i64 & 0xFFFF_FFFF) | ((z as i64 & 0xFFFF_FFFF) << 32)
}

impl StartMap {
    pub fn contains(&self, x: i32, z: i32) -> bool {
        self.keys.contains(&chunk_key(x, z))
    }
    pub fn insert(&mut self, s: Start) {
        let k = chunk_key(s.cx, s.cz);
        if self.keys.insert(k) {
            self.entries.push((k, s));
        }
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    /// Entry indices in `HashMap.values()` order.
    pub fn order(&self) -> Vec<usize> {
        let n = self.entries.len();
        let mut cap = 16usize;
        while n > cap * 3 / 4 {
            cap *= 2;
        }
        let mut idx: Vec<(usize, usize)> = self
            .entries
            .iter()
            .enumerate()
            .map(|(i, (k, _))| {
                let h = ((*k ^ (*k as u64 >> 32) as i64) as i32) as u32;
                let spread = h ^ (h >> 16);
                ((spread as usize) & (cap - 1), i)
            })
            .collect();
        idx.sort();
        idx.into_iter().map(|(_, i)| i).collect()
    }
    pub fn starts(&self) -> impl Iterator<Item = &Start> {
        self.entries.iter().map(|e| &e.1)
    }
}

/// One `MapGenStructure`.
pub struct Gen {
    pub ty: Type,
    pub map: StartMap,
    /// Chunks whose `canSpawnStructureAtCoords` was already evaluated.
    checked: HashSet<(i32, i32)>,
}

/// The four overworld `MapGenStructure`s of `ChunkProviderGenerate`.
pub struct Structures {
    pub gens: [Gen; 4],
    seed: i64,
    /// `MapGenStronghold.structureCoords`
    pub stronghold_coords: Vec<(i32, i32)>,
}

impl Structures {
    pub fn new(provider: &ChunkProvider) -> Structures {
        let g = |ty| Gen {
            ty,
            map: StartMap::default(),
            checked: HashSet::new(),
        };
        Structures {
            gens: [
                g(Type::Mineshaft),
                g(Type::Village),
                g(Type::Stronghold),
                g(Type::Temple),
            ],
            seed: provider.seed,
            stronghold_coords: stronghold::find_coords(provider),
        }
    }

    /// `func_151539_a` of each generator for the chunk being provided.
    pub fn register(
        &mut self,
        provider: &ChunkProvider,
        cx: i32,
        cz: i32,
        world_rand: &mut JavaRandom,
    ) {
        let seed = self.seed;
        let mut base = JavaRandom::new(seed);
        let l = base.next_long();
        let i1 = base.next_long();
        let coords = self.stronghold_coords.clone();
        // `World.setRandomSeed` reseeds `world.rand` on every village and
        // temple check; track the last such call of this provide.
        let mut last_reseed: Option<(Type, i32, i32)> = None;
        for g in self.gens.iter_mut() {
            for x in cx - 8..=cx + 8 {
                for z in cz - 8..=cz + 8 {
                    if g.map.contains(x, z) {
                        continue;
                    }
                    if matches!(g.ty, Type::Village | Type::Temple) {
                        last_reseed = Some((g.ty, x, z));
                    }
                    if !g.checked.insert((x, z)) {
                        continue;
                    }
                    let mut r = JavaRandom::new(0);
                    r.set_seed((x as i64).wrapping_mul(l) ^ (z as i64).wrapping_mul(i1) ^ seed);
                    r.next_int();
                    let start = match g.ty {
                        Type::Mineshaft => mineshaft::can_spawn(&mut r, x, z)
                            .then(|| mineshaft::start(&mut r, x, z)),
                        Type::Village => village::can_spawn(provider, seed, x, z)
                            .then(|| village::start(provider, &mut r, x, z)),
                        Type::Stronghold => coords
                            .contains(&(x, z))
                            .then(|| stronghold::start(&mut r, x, z)),
                        Type::Temple => scattered::can_spawn(provider, seed, x, z)
                            .then(|| scattered::start(provider, &mut r, x, z)),
                    };
                    if let Some(s) = start {
                        g.map.insert(s);
                    }
                }
            }
        }
        if let Some((ty, x, z)) = last_reseed {
            let (salt, dist, sep) = match ty {
                Type::Village => (10387312i64, 32, 8),
                _ => (14357617i64, 32, 8),
            };
            let (mut gx, mut gz) = (x, z);
            if gx < 0 {
                gx -= dist - 1;
            }
            if gz < 0 {
                gz -= dist - 1;
            }
            let (gx, gz) = (gx / dist, gz / dist);
            world_rand.set_seed(
                (gx as i64)
                    .wrapping_mul(341873128712)
                    .wrapping_add((gz as i64).wrapping_mul(132897987541))
                    .wrapping_add(seed)
                    .wrapping_add(salt),
            );
            world_rand.next_int_n(dist - sep);
            world_rand.next_int_n(dist - sep);
        }
    }

    /// `generateStructuresInChunk` for each type, in `populate` order.
    /// Returns whether a village was generated here (it suppresses lakes).
    pub fn generate(&mut self, w: &mut ServerWorld, r: &mut JavaRandom, cx: i32, cz: i32) -> bool {
        let (x0, z0) = (cx * 16 + 8, cz * 16 + 8);
        let clip = BB::flat(x0, z0, x0 + 15, z0 + 15);
        let mut village = false;
        for g in self.gens.iter_mut() {
            let mut any = false;
            for i in g.map.order() {
                let s = &mut g.map.entries[i].1;
                if s.sizeable && s.bb.intersects_xz(x0, z0, x0 + 15, z0 + 15) {
                    s.generate(w, r, &clip);
                    any = true;
                }
            }
            if g.ty == Type::Village {
                village = any;
            }
        }
        village
    }

    /// `MapGenStructure.getNearestInstance` for strongholds (`/locate`, eyes).
    pub fn nearest_stronghold(&self, x: i32, y: i32, z: i32) -> Option<[i32; 3]> {
        let mut best: Option<(f64, [i32; 3])> = None;
        for s in self.gens[2].map.starts() {
            if s.sizeable {
                if let Some(p) = s.pieces.first() {
                    let c = [p.bb.cx(), p.bb.cy(), p.bb.cz()];
                    let d = ((c[0] - x) as f64).powi(2)
                        + ((c[1] - y) as f64).powi(2)
                        + ((c[2] - z) as f64).powi(2);
                    if best.map_or(true, |b| d < b.0) {
                        best = Some((d, c));
                    }
                }
            }
        }
        if let Some(b) = best {
            return Some(b.1);
        }
        let mut best: Option<(f64, [i32; 3])> = None;
        for &(cx, cz) in &self.stronghold_coords {
            let c = [(cx << 4) + 8, 64, (cz << 4) + 8];
            let d = ((c[0] - x) as f64).powi(2)
                + ((c[1] - y) as f64).powi(2)
                + ((c[2] - z) as f64).powi(2);
            if best.map_or(true, |b| d < b.0) {
                best = Some((d, c));
            }
        }
        best.map(|b| b.1)
    }

    /// Serialised state (the `data/*.dat` files of vanilla).
    pub fn save(&self) -> String {
        let mut out: Vec<i32> = Vec::new();
        for g in &self.gens {
            out.push(g.map.len() as i32);
            for (_, s) in &g.map.entries {
                s.save(&mut out);
            }
        }
        out.iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(",")
    }

    pub fn load(&mut self, text: &str) -> Option<()> {
        let v: Vec<i32> = text
            .split(',')
            .map(|s| s.parse().ok())
            .collect::<Option<Vec<_>>>()?;
        let mut it = v.iter();
        let mut gens: Vec<Vec<Start>> = Vec::new();
        for _ in 0..4 {
            let n = *it.next()? as usize;
            let mut list = Vec::new();
            for _ in 0..n {
                list.push(Start::load(&mut it)?);
            }
            gens.push(list);
        }
        for (g, list) in self.gens.iter_mut().zip(gens) {
            for s in list {
                g.checked.insert((s.cx, s.cz));
                g.map.insert(s);
            }
        }
        Some(())
    }
}

/// `ServerWorld` entry point used by `populate`.
pub fn generate_in_chunk(w: &mut ServerWorld, r: &mut JavaRandom, cx: i32, cz: i32) -> bool {
    let Some(mut s) = w.structures.take() else {
        return false;
    };
    let before = w.gen_tiles.len();
    let village = s.generate(w, r, cx, cz);
    w.structures = Some(s);
    if crate::compat::fixed() {
        // MC-12918: a structure chest picks its facing from the blocks around
        // it when it is placed, before the rest of the piece is built, so it
        // can end up facing a wall. Re-run the facing once the piece is done.
        let chests: Vec<[i32; 3]> = w.gen_tiles[before..]
            .iter()
            .filter_map(|t| match t {
                GenTile::Chest { pos, .. } => Some(*pos),
                _ => None,
            })
            .collect();
        for p in chests {
            if w.block(p[0], p[1], p[2]) == block::CHEST {
                w.chest_init_meta(p[0], p[1], p[2]);
            }
        }
    }
    // Chunks provided meanwhile (should not happen inside the populate area).
    w.flush_structure_registrations();
    village
}

/// Spawns an entity created by a structure.
pub fn spawn(w: &mut ServerWorld, e: GenEntity) {
    w.gen_entities.push(e);
}

/// `WorldChunkManager.areBiomesViable`
pub fn biomes_viable(
    provider: &ChunkProvider,
    x: i32,
    z: i32,
    radius: i32,
    allowed: &[i32],
) -> bool {
    let x0 = (x - radius) >> 2;
    let z0 = (z - radius) >> 2;
    let x1 = (x + radius) >> 2;
    let z1 = (z + radius) >> 2;
    let w = x1 - x0 + 1;
    let h = z1 - z0 + 1;
    let ints = provider.gen_biomes.get_ints(x0, z0, w, h);
    ints.iter()
        .all(|&id| allowed.contains(&(crate::world::biome::get_or_ocean(id).id as i32)))
}

/// `WorldChunkManager.findBiomePosition`
pub fn find_biome_position(
    provider: &ChunkProvider,
    x: i32,
    z: i32,
    range: i32,
    allowed: &[i32],
    r: &mut JavaRandom,
) -> Option<(i32, i32)> {
    let x0 = (x - range) >> 2;
    let z0 = (z - range) >> 2;
    let x1 = (x + range) >> 2;
    let z1 = (z + range) >> 2;
    let w = x1 - x0 + 1;
    let h = z1 - z0 + 1;
    let ints = provider.gen_biomes.get_ints(x0, z0, w, h);
    let mut pos = None;
    let mut n = 0;
    for k in 0..(w * h) {
        let px = (x0 + k % w) << 2;
        let pz = (z0 + k / w) << 2;
        let id = crate::world::biome::get_or_ocean(ints[k as usize]).id as i32;
        if allowed.contains(&id) && (pos.is_none() || r.next_int_n(n + 1) == 0) {
            pos = Some((px, pz));
            n += 1;
        }
    }
    pos
}

/// `BiomeGenBase.biomeList[id]` lookup used by structure biome checks.
pub fn biome_at(provider: &ChunkProvider, x: i32, z: i32) -> i32 {
    provider.biome_at(x, z)
}

pub(crate) fn _unused(_: &ServerWorld) {
    let _ = full(0);
}
