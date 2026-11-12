//! Redstone: the `World` power queries and the blocks that produce or use
//! power (`BlockRedstoneWire`, `BlockRedstoneTorch`, `BlockRedstoneRepeater`,
//! `BlockLever`, `BlockButton`, `BlockPressurePlate`, `BlockRedstoneLight`,
//! `BlockPistonBase`, `BlockTNT`, `BlockNote`, `BlockDispenser`, `BlockDoor`,
//! `BlockTrapDoor`, `BlockTripWireHook`).

use super::server::{GenEntity, ServerWorld};
use crate::block::{self, *};
use crate::java::JavaRandom;

const FX: [i32; 6] = [0, 0, 0, 0, -1, 1];
const FY: [i32; 6] = [-1, 1, 0, 0, 0, 0];
const FZ: [i32; 6] = [0, 0, -1, 1, 0, 0];
/// `Direction.offsetX/Z`, `directionToFacing`, `rotateOpposite`
const DX: [i32; 4] = [0, -1, 0, 1];
const DZ: [i32; 4] = [1, 0, -1, 0];
const DIR_TO_FACING: [i32; 4] = [3, 4, 2, 5];
const ROT_OPP: [i32; 4] = [2, 3, 0, 1];

/// A side effect the game state outside the world has to carry out.
#[derive(Clone, Debug)]
pub enum Effect {
    Sound(&'static str, [f64; 3], f32, f32),
    /// `BlockNote.triggerNote`: instrument by the material below, and pitch 0..24.
    Note([i32; 3], &'static str, i32),
    /// `BlockDispenser.func_149941_e`
    Dispense([i32; 3], i32),
    /// `EntityTNTPrimed` with its fuse.
    Tnt([f64; 3], i32),
}

/// An axis-aligned box in world space.
#[derive(Clone, Copy, Debug)]
pub struct Aabb6 {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

impl Aabb6 {
    pub fn intersects(&self, o: &Aabb6) -> bool {
        (0..3).all(|a| self.min[a] < o.max[a] && self.max[a] > o.min[a])
    }
}

/// An entity's box for pressure plates and tripwire; `living` entities
/// trigger stone plates (`EntityLivingBase`).
#[derive(Clone, Copy, Debug)]
pub struct Occupant {
    pub bb: Aabb6,
    pub living: bool,
}

pub fn can_provide_power(id: u8) -> bool {
    matches!(
        id,
        REDSTONE_WIRE
            | REDSTONE_TORCH
            | UNLIT_REDSTONE_TORCH
            | LEVER
            | STONE_BUTTON
            | WOODEN_BUTTON
            | STONE_PRESSURE_PLATE
            | WOODEN_PRESSURE_PLATE
            | UNPOWERED_REPEATER
            | POWERED_REPEATER
            | REDSTONE_BLOCK
            | TRIPWIRE_HOOK
            | DETECTOR_RAIL
    )
}

fn is_repeater(id: u8) -> bool {
    id == UNPOWERED_REPEATER || id == POWERED_REPEATER
}

/// Blocks a tick entry treats as the same (`Block.isEqualTo`).
pub fn tick_family(id: u8) -> u8 {
    match id {
        UNLIT_REDSTONE_TORCH => REDSTONE_TORCH,
        POWERED_REPEATER => UNPOWERED_REPEATER,
        LIT_REDSTONE_LAMP => REDSTONE_LAMP,
        _ => id,
    }
}

impl ServerWorld {
    /// `Block.isNormalCube`: opaque, full and not a power source.
    fn nc(&mut self, x: i32, y: i32, z: i32) -> bool {
        let id = self.block(x, y, z);
        block::info(id).normal_cube() && !can_provide_power(id)
    }

    /// `notifyBlocksOfNeighborChange(x, y, z, block, exceptSide)`
    fn notify_except(&mut self, x: i32, y: i32, z: i32, src: u8, side: usize) {
        if side != 4 {
            self.notify_one(x - 1, y, z, src);
        }
        if side != 5 {
            self.notify_one(x + 1, y, z, src);
        }
        if side != 0 {
            self.notify_one(x, y - 1, z, src);
        }
        if side != 1 {
            self.notify_one(x, y + 1, z, src);
        }
        if side != 2 {
            self.notify_one(x, y, z - 1, src);
        }
        if side != 3 {
            self.notify_one(x, y, z + 1, src);
        }
    }

    /// `isProvidingWeakPower`
    pub fn weak_power(&mut self, x: i32, y: i32, z: i32, side: usize) -> i32 {
        let id = self.block(x, y, z);
        let m = self.meta(x, y, z);
        match id {
            REDSTONE_WIRE => self.wire_weak(x, y, z, side),
            REDSTONE_TORCH => {
                if (m == 5 && side == 1)
                    || (m == 3 && side == 3)
                    || (m == 4 && side == 2)
                    || (m == 1 && side == 5)
                    || (m == 2 && side == 4)
                {
                    0
                } else {
                    15
                }
            }
            LEVER | STONE_BUTTON | WOODEN_BUTTON => {
                if m & 8 != 0 {
                    15
                } else {
                    0
                }
            }
            STONE_PRESSURE_PLATE | WOODEN_PRESSURE_PLATE => {
                if m == 1 {
                    15
                } else {
                    0
                }
            }
            POWERED_REPEATER => {
                let d = m & 3;
                if (d == 0 && side == 3)
                    || (d == 1 && side == 4)
                    || (d == 2 && side == 2)
                    || (d == 3 && side == 5)
                {
                    15
                } else {
                    0
                }
            }
            REDSTONE_BLOCK => 15,
            TRIPWIRE_HOOK => {
                if m & 8 != 0 {
                    15
                } else {
                    0
                }
            }
            DETECTOR_RAIL => {
                if m & 8 != 0 {
                    15
                } else {
                    0
                }
            }
            _ => 0,
        }
    }

    /// `isProvidingStrongPower`
    pub fn strong_power(&mut self, x: i32, y: i32, z: i32, side: usize) -> i32 {
        let id = self.block(x, y, z);
        let m = self.meta(x, y, z);
        match id {
            REDSTONE_WIRE => {
                if self.wire_off {
                    0
                } else {
                    self.wire_weak(x, y, z, side)
                }
            }
            REDSTONE_TORCH => {
                if side == 0 {
                    self.weak_power(x, y, z, side)
                } else {
                    0
                }
            }
            LEVER => {
                if m & 8 == 0 {
                    return 0;
                }
                let k = m & 7;
                let hit = ((k == 0 || k == 7) && side == 0)
                    || ((k == 5 || k == 6) && side == 1)
                    || (k == 4 && side == 2)
                    || (k == 3 && side == 3)
                    || (k == 2 && side == 4)
                    || (k == 1 && side == 5);
                if hit {
                    15
                } else {
                    0
                }
            }
            STONE_BUTTON | WOODEN_BUTTON => {
                if m & 8 == 0 {
                    return 0;
                }
                let k = m & 7;
                let hit = (k == 5 && side == 1)
                    || (k == 4 && side == 2)
                    || (k == 3 && side == 3)
                    || (k == 2 && side == 4)
                    || (k == 1 && side == 5);
                if hit {
                    15
                } else {
                    0
                }
            }
            STONE_PRESSURE_PLATE | WOODEN_PRESSURE_PLATE => {
                if side == 1 && m == 1 {
                    15
                } else {
                    0
                }
            }
            POWERED_REPEATER => self.weak_power(x, y, z, side),
            TRIPWIRE_HOOK => {
                let f = m & 3;
                if m & 8 == 0 {
                    0
                } else if (f == 2 && side == 2)
                    || (f == 0 && side == 3)
                    || (f == 1 && side == 4)
                    || (f == 3 && side == 5)
                {
                    15
                } else {
                    0
                }
            }
            DETECTOR_RAIL => {
                if m & 8 != 0 && side == 1 {
                    15
                } else {
                    0
                }
            }
            _ => 0,
        }
    }

    /// `World.getBlockPowerInput`
    pub fn power_input(&mut self, x: i32, y: i32, z: i32) -> i32 {
        let mut p = 0;
        for s in 0..6 {
            p = p.max(self.strong_power(x + FX[s], y + FY[s], z + FZ[s], s));
            if p >= 15 {
                return p;
            }
        }
        p
    }

    /// `World.getIndirectPowerLevelTo`
    pub fn indirect_power_to(&mut self, x: i32, y: i32, z: i32, side: usize) -> i32 {
        if self.nc(x, y, z) {
            self.power_input(x, y, z)
        } else {
            self.weak_power(x, y, z, side)
        }
    }

    /// `World.isBlockIndirectlyGettingPowered`
    pub fn indirectly_powered(&mut self, x: i32, y: i32, z: i32) -> bool {
        (0..6).any(|s| self.indirect_power_to(x + FX[s], y + FY[s], z + FZ[s], s) > 0)
    }

    /// `World.getStrongestIndirectPower`
    pub fn strongest_indirect(&mut self, x: i32, y: i32, z: i32) -> i32 {
        let mut p = 0;
        for s in 0..6 {
            let v = self.indirect_power_to(x + FX[s], y + FY[s], z + FZ[s], s);
            if v >= 15 {
                return 15;
            }
            p = p.max(v);
        }
        p
    }

    // ------------------------------------------------------------ wire

    fn wire_connects(&mut self, x: i32, y: i32, z: i32, dir: i32) -> bool {
        let id = self.block(x, y, z);
        if id == REDSTONE_WIRE {
            return true;
        }
        if is_repeater(id) {
            let m = self.meta(x, y, z);
            if dir == (m & 3) || dir == ROT_OPP[(m & 3) as usize] {
                return true;
            }
            return id == POWERED_REPEATER && dir == (m & 3);
        }
        can_provide_power(id) && dir != -1
    }

    fn wire_weak(&mut self, x: i32, y: i32, z: i32, side: usize) -> i32 {
        if self.wire_off {
            return 0;
        }
        let m = self.meta(x, y, z);
        if m == 0 {
            return 0;
        }
        if side == 1 {
            return m;
        }
        let mut w = self.wire_connects(x - 1, y, z, 1)
            || (!self.nc(x - 1, y, z) && self.wire_connects(x - 1, y - 1, z, -1));
        let mut e = self.wire_connects(x + 1, y, z, 3)
            || (!self.nc(x + 1, y, z) && self.wire_connects(x + 1, y - 1, z, -1));
        let mut n = self.wire_connects(x, y, z - 1, 2)
            || (!self.nc(x, y, z - 1) && self.wire_connects(x, y - 1, z - 1, -1));
        let mut s = self.wire_connects(x, y, z + 1, 0)
            || (!self.nc(x, y, z + 1) && self.wire_connects(x, y - 1, z + 1, -1));
        if !self.nc(x, y + 1, z) {
            if self.nc(x - 1, y, z) && self.wire_connects(x - 1, y + 1, z, -1) {
                w = true;
            }
            if self.nc(x + 1, y, z) && self.wire_connects(x + 1, y + 1, z, -1) {
                e = true;
            }
            if self.nc(x, y, z - 1) && self.wire_connects(x, y + 1, z - 1, -1) {
                n = true;
            }
            if self.nc(x, y, z + 1) && self.wire_connects(x, y + 1, z + 1, -1) {
                s = true;
            }
        }
        if !n && !e && !w && !s && (2..=5).contains(&side) {
            m
        } else if side == 2 && n && !w && !e {
            m
        } else if side == 3 && s && !w && !e {
            m
        } else if side == 4 && w && !n && !s {
            m
        } else if side == 5 && e && !n && !s {
            m
        } else {
            0
        }
    }

    fn wire_highest(&mut self, x: i32, y: i32, z: i32, cur: i32) -> i32 {
        if self.block(x, y, z) != REDSTONE_WIRE {
            return cur;
        }
        cur.max(self.meta(x, y, z))
    }

    /// `BlockRedstoneWire.updatePower`
    fn wire_update(&mut self, x: i32, y: i32, z: i32) {
        let mut pending: Vec<(i32, i32, i32)> = Vec::new();
        self.wire_do_update(x, y, z, x, y, z, &mut pending);
        // HashSet<ChunkPosition> iteration order: bucket of the spread hash.
        let mut keyed: Vec<(u32, usize, (i32, i32, i32))> = pending
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let h =
                    (p.0.wrapping_mul(8976890)
                        .wrapping_add(p.1.wrapping_mul(981131))
                        .wrapping_add(p.2)) as u32;
                (((h ^ (h >> 16)) & 15), i, *p)
            })
            .collect();
        keyed.sort();
        for (_, _, p) in keyed {
            self.notify_neighbours(p.0, p.1, p.2, REDSTONE_WIRE);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn wire_do_update(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        sx: i32,
        sy: i32,
        sz: i32,
        pending: &mut Vec<(i32, i32, i32)>,
    ) {
        let old = self.meta(x, y, z);
        let mut p = self.wire_highest(sx, sy, sz, 0);
        self.wire_off = true;
        let ind = self.strongest_indirect(x, y, z);
        self.wire_off = false;
        if ind > 0 && ind > p - 1 {
            p = ind;
        }
        let mut best = 0;
        for k in 0..4 {
            let (nx, nz) = match k {
                0 => (x - 1, z),
                1 => (x + 1, z),
                2 => (x, z - 1),
                _ => (x, z + 1),
            };
            if nx != sx || nz != sz {
                best = self.wire_highest(nx, y, nz, best);
            }
            if self.nc(nx, y, nz) && !self.nc(x, y + 1, z) {
                if (nx != sx || nz != sz) && y >= sy {
                    best = self.wire_highest(nx, y + 1, nz, best);
                }
            } else if !self.nc(nx, y, nz) && (nx != sx || nz != sz) && y <= sy {
                best = self.wire_highest(nx, y - 1, nz, best);
            }
        }
        if best > p {
            p = best - 1;
        } else if p > 0 {
            p -= 1;
        } else {
            p = 0;
        }
        if ind > p - 1 {
            p = ind;
        }
        if old != p {
            self.set_meta(x, y, z, p, 2);
            for q in [
                (x, y, z),
                (x - 1, y, z),
                (x + 1, y, z),
                (x, y - 1, z),
                (x, y + 1, z),
                (x, y, z - 1),
                (x, y, z + 1),
            ] {
                if !pending.contains(&q) {
                    pending.push(q);
                }
            }
        }
    }

    fn wire_neighbours_of(&mut self, x: i32, y: i32, z: i32) {
        if self.block(x, y, z) == REDSTONE_WIRE {
            self.notify_neighbours(x, y, z, REDSTONE_WIRE);
            for (a, b, c) in [
                (x - 1, y, z),
                (x + 1, y, z),
                (x, y, z - 1),
                (x, y, z + 1),
                (x, y - 1, z),
                (x, y + 1, z),
            ] {
                self.notify_neighbours(a, b, c, REDSTONE_WIRE);
            }
        }
    }

    fn wire_diagonals(&mut self, x: i32, y: i32, z: i32) {
        self.wire_neighbours_of(x - 1, y, z);
        self.wire_neighbours_of(x + 1, y, z);
        self.wire_neighbours_of(x, y, z - 1);
        self.wire_neighbours_of(x, y, z + 1);
        for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            if self.nc(x + dx, y, z + dz) {
                self.wire_neighbours_of(x + dx, y + 1, z + dz);
            } else {
                self.wire_neighbours_of(x + dx, y - 1, z + dz);
            }
        }
    }

    // ------------------------------------------------------------ hooks

    /// `onBlockAdded` of the redstone blocks.
    pub(crate) fn redstone_added(&mut self, id: u8, x: i32, y: i32, z: i32) -> bool {
        match id {
            REDSTONE_WIRE => {
                self.wire_update(x, y, z);
                self.notify_neighbours(x, y + 1, z, id);
                self.notify_neighbours(x, y - 1, z, id);
                self.wire_diagonals(x, y, z);
            }
            REDSTONE_TORCH => {
                for (a, b, c) in [
                    (x, y - 1, z),
                    (x, y + 1, z),
                    (x - 1, y, z),
                    (x + 1, y, z),
                    (x, y, z - 1),
                    (x, y, z + 1),
                ] {
                    self.notify_neighbours(a, b, c, id);
                }
            }
            UNPOWERED_REPEATER | POWERED_REPEATER => self.repeater_neighbours(x, y, z),
            REDSTONE_LAMP | LIT_REDSTONE_LAMP => self.lamp_check(id, x, y, z),
            TNT => {
                if self.indirectly_powered(x, y, z) {
                    self.prime_tnt(x, y, z, 80);
                }
            }
            PISTON | STICKY_PISTON => self.piston_update(id, x, y, z),
            _ => return false,
        }
        true
    }

    /// `breakBlock` of the redstone blocks (`old` was at the position).
    pub(crate) fn redstone_broken(&mut self, old: u8, old_meta: i32, x: i32, y: i32, z: i32) {
        match old {
            REDSTONE_WIRE => {
                for (a, b, c) in [
                    (x, y + 1, z),
                    (x, y - 1, z),
                    (x + 1, y, z),
                    (x - 1, y, z),
                    (x, y, z + 1),
                    (x, y, z - 1),
                ] {
                    self.notify_neighbours(a, b, c, old);
                }
                self.wire_update(x, y, z);
                self.wire_diagonals(x, y, z);
            }
            REDSTONE_TORCH => {
                for (a, b, c) in [
                    (x, y - 1, z),
                    (x, y + 1, z),
                    (x - 1, y, z),
                    (x + 1, y, z),
                    (x, y, z - 1),
                    (x, y, z + 1),
                ] {
                    self.notify_neighbours(a, b, c, old);
                }
            }
            LEVER | STONE_BUTTON | WOODEN_BUTTON => {
                if old_meta & 8 != 0 {
                    self.attached_neighbours(old, old_meta & 7, x, y, z);
                }
            }
            STONE_PRESSURE_PLATE | WOODEN_PRESSURE_PLATE => {
                if old_meta == 1 {
                    self.notify_neighbours(x, y, z, old);
                    self.notify_neighbours(x, y - 1, z, old);
                }
            }
            UNPOWERED_REPEATER | POWERED_REPEATER => {
                self.repeater_neighbours_dir(x, y, z, old_meta & 3);
            }
            _ => {}
        }
    }

    /// `onNeighborBlockChange` of the redstone blocks; false when not handled.
    pub(crate) fn redstone_neighbour(&mut self, id: u8, x: i32, y: i32, z: i32, src: u8) -> bool {
        match id {
            REDSTONE_WIRE => {
                if self.solid_top_surface(x, y - 1, z) || self.block(x, y - 1, z) == GLOWSTONE {
                    self.wire_update(x, y, z);
                } else {
                    self.drop_block(x, y, z, crate::item::REDSTONE, 0);
                }
            }
            REDSTONE_TORCH | UNLIT_REDSTONE_TORCH => {
                if !self.torch_supported(x, y, z) {
                    self.drop_block(x, y, z, REDSTONE_TORCH as u16, 0);
                    return true;
                }
                let on = id == REDSTONE_TORCH;
                let sig = self.torch_signal(x, y, z);
                if on == sig {
                    self.schedule_update(x, y, z, id, 2);
                }
            }
            UNPOWERED_REPEATER | POWERED_REPEATER => {
                if !self.solid_top_surface(x, y - 1, z) {
                    self.drop_block(x, y, z, 356, 0);
                    for (a, b, c) in [
                        (x + 1, y, z),
                        (x - 1, y, z),
                        (x, y, z + 1),
                        (x, y, z - 1),
                        (x, y - 1, z),
                        (x, y + 1, z),
                    ] {
                        self.notify_neighbours(a, b, c, id);
                    }
                } else {
                    self.repeater_check(id, x, y, z);
                }
            }
            LEVER | STONE_BUTTON | WOODEN_BUTTON => {
                let m = self.meta(x, y, z) & 7;
                let off = match m {
                    1 => !self.nc(x - 1, y, z),
                    2 => !self.nc(x + 1, y, z),
                    3 => !self.nc(x, y, z - 1),
                    4 => !self.nc(x, y, z + 1),
                    5 | 6 => id == LEVER && !self.solid_top_surface(x, y - 1, z),
                    _ => id == LEVER && !self.nc(x, y + 1, z),
                };
                if off {
                    self.drop_block(x, y, z, id as u16, 0);
                }
            }
            STONE_PRESSURE_PLATE | WOODEN_PRESSURE_PLATE => {
                if !self.solid_top_surface(x, y - 1, z) && self.block(x, y - 1, z) != FENCE {
                    self.drop_block(x, y, z, id as u16, 0);
                }
            }
            REDSTONE_LAMP | LIT_REDSTONE_LAMP => self.lamp_check(id, x, y, z),
            TNT => {
                if self.indirectly_powered(x, y, z) {
                    self.prime_tnt(x, y, z, 80);
                }
            }
            PISTON | STICKY_PISTON => {
                if src != PISTON_HEAD {
                    self.piston_update(id, x, y, z);
                }
            }
            NOTEBLOCK => {
                let p = self.indirectly_powered(x, y, z);
                let prev = self.note_powered.get(&(x, y, z)).copied().unwrap_or(false);
                if p != prev {
                    if p {
                        self.note_trigger(x, y, z);
                    }
                    self.note_powered.insert((x, y, z), p);
                }
            }
            DISPENSER => {
                let p = self.indirectly_powered(x, y, z) || self.indirectly_powered(x, y + 1, z);
                let m = self.meta(x, y, z);
                let triggered = m & 8 != 0;
                if p && !triggered {
                    self.schedule_update(x, y, z, id, 4);
                    self.set_meta(x, y, z, m | 8, 4);
                } else if !p && triggered {
                    self.set_meta(x, y, z, m & !8, 4);
                }
            }
            TRAPDOOR => {
                let m = self.meta(x, y, z);
                let (ax, az) = match m & 3 {
                    0 => (x, z + 1),
                    1 => (x, z - 1),
                    2 => (x + 1, z),
                    _ => (x - 1, z),
                };
                if !self.nc(ax, y, az) {
                    self.drop_block(x, y, z, TRAPDOOR as u16, 0);
                    return true;
                }
                let p = self.indirectly_powered(x, y, z);
                if p || can_provide_power(src) {
                    let open = m & 4 != 0;
                    if open != p {
                        self.set_meta(x, y, z, m ^ 4, 2);
                        let s = if p {
                            "random.door_open"
                        } else {
                            "random.door_close"
                        };
                        let r = self.rand.next_float() * 0.1 + 0.9;
                        self.redstone_effects.push(Effect::Sound(
                            s,
                            [x as f64 + 0.5, y as f64 + 0.5, z as f64 + 0.5],
                            1.0,
                            r,
                        ));
                    }
                }
            }
            _ => return false,
        }
        true
    }

    /// `updateTick` of the redstone blocks; false when not handled.
    pub(crate) fn redstone_tick(
        &mut self,
        id: u8,
        x: i32,
        y: i32,
        z: i32,
        r: &mut JavaRandom,
    ) -> bool {
        match id {
            REDSTONE_TORCH | UNLIT_REDSTONE_TORCH => {
                let sig = self.torch_signal(x, y, z);
                let now = self.total_time;
                self.torch_toggles.retain(|t| now - t.3 <= 60);
                let m = self.meta(x, y, z);
                if id == REDSTONE_TORCH {
                    if sig {
                        self.set_block(x, y, z, UNLIT_REDSTONE_TORCH, m, 3);
                        if self.torch_burnout(x, y, z, true) {
                            let p = 2.6 + (self.rand.next_float() - self.rand.next_float()) * 0.8;
                            self.redstone_effects.push(Effect::Sound(
                                "random.fizz",
                                [x as f64 + 0.5, y as f64 + 0.5, z as f64 + 0.5],
                                0.5,
                                p,
                            ));
                            for _ in 0..5 {
                                let _ = (r.next_double(), r.next_double(), r.next_double());
                            }
                        }
                    }
                } else if !sig && !self.torch_burnout(x, y, z, false) {
                    self.set_block(x, y, z, REDSTONE_TORCH, m, 3);
                }
            }
            UNPOWERED_REPEATER | POWERED_REPEATER => {
                let m = self.meta(x, y, z);
                if !self.repeater_locked(x, y, z, m) {
                    let input = self.repeater_input(x, y, z, m) > 0;
                    if id == POWERED_REPEATER && !input {
                        self.set_block(x, y, z, UNPOWERED_REPEATER, m, 2);
                    } else if id == UNPOWERED_REPEATER {
                        self.set_block(x, y, z, POWERED_REPEATER, m, 2);
                        if !input {
                            self.schedule_priority(
                                x,
                                y,
                                z,
                                POWERED_REPEATER,
                                repeater_delay(m),
                                -1,
                            );
                        }
                    }
                }
            }
            STONE_BUTTON | WOODEN_BUTTON => {
                let m = self.meta(x, y, z);
                if m & 8 != 0 {
                    // Wooden buttons stay pressed while an arrow touches them; arrows
                    // stuck in buttons are not tracked, so both release.
                    self.set_meta(x, y, z, m & 7, 3);
                    self.attached_neighbours(id, m & 7, x, y, z);
                    self.redstone_effects.push(Effect::Sound(
                        "random.click",
                        [x as f64 + 0.5, y as f64 + 0.5, z as f64 + 0.5],
                        0.3,
                        0.5,
                    ));
                }
            }
            STONE_PRESSURE_PLATE | WOODEN_PRESSURE_PLATE => {
                if self.meta(x, y, z) == 1 {
                    self.plate_update(id, x, y, z, 15);
                }
            }
            LIT_REDSTONE_LAMP => {
                if !self.indirectly_powered(x, y, z) {
                    self.set_block(x, y, z, REDSTONE_LAMP, 0, 2);
                }
            }
            DISPENSER => {
                let m = self.meta(x, y, z);
                self.redstone_effects
                    .push(Effect::Dispense([x, y, z], m & 7));
            }
            _ => return false,
        }
        true
    }

    fn drop_block(&mut self, x: i32, y: i32, z: i32, item: u16, meta: i32) {
        self.gen_entities.push(GenEntity::Item {
            pos: [x as f64 + 0.5, y as f64 + 0.5, z as f64 + 0.5],
            stack: crate::item::Stack::new(item, meta, 1),
        });
        self.set_block(x, y, z, AIR, 0, 3);
    }

    // ------------------------------------------------------------ torches

    fn torch_supported(&mut self, x: i32, y: i32, z: i32) -> bool {
        match self.meta(x, y, z) {
            1 => self.nc(x - 1, y, z),
            2 => self.nc(x + 1, y, z),
            3 => self.nc(x, y, z - 1),
            4 => self.nc(x, y, z + 1),
            _ => {
                self.solid_top_surface(x, y - 1, z)
                    || matches!(self.block(x, y - 1, z), FENCE | GLASS)
            }
        }
    }

    /// `BlockRedstoneTorch.hasNeighborSignal`
    fn torch_signal(&mut self, x: i32, y: i32, z: i32) -> bool {
        match self.meta(x, y, z) {
            5 => self.indirect_power_to(x, y - 1, z, 0) > 0,
            3 => self.indirect_power_to(x, y, z - 1, 2) > 0,
            4 => self.indirect_power_to(x, y, z + 1, 3) > 0,
            1 => self.indirect_power_to(x - 1, y, z, 4) > 0,
            2 => self.indirect_power_to(x + 1, y, z, 5) > 0,
            _ => false,
        }
    }

    /// `shouldBurnOut`: eight toggles within 60 ticks burn the torch out.
    fn torch_burnout(&mut self, x: i32, y: i32, z: i32, add: bool) -> bool {
        if add {
            self.torch_toggles.push((x, y, z, self.total_time));
        }
        self.torch_toggles
            .iter()
            .filter(|t| t.0 == x && t.1 == y && t.2 == z)
            .count()
            >= 8
    }

    // ------------------------------------------------------------ repeaters

    fn repeater_input(&mut self, x: i32, y: i32, z: i32, m: i32) -> i32 {
        let d = (m & 3) as usize;
        let (nx, nz) = (x + DX[d], z + DZ[d]);
        let v = self.indirect_power_to(nx, y, nz, DIR_TO_FACING[d] as usize);
        if v >= 15 {
            v
        } else {
            v.max(if self.block(nx, y, nz) == REDSTONE_WIRE {
                self.meta(nx, y, nz)
            } else {
                0
            })
        }
    }

    fn repeater_side(&mut self, x: i32, y: i32, z: i32, side: usize) -> i32 {
        let id = self.block(x, y, z);
        if is_repeater(id) {
            self.strong_power(x, y, z, side)
        } else {
            0
        }
    }

    fn repeater_locked(&mut self, x: i32, y: i32, z: i32, m: i32) -> bool {
        match m & 3 {
            0 | 2 => {
                self.repeater_side(x - 1, y, z, 4)
                    .max(self.repeater_side(x + 1, y, z, 5))
                    > 0
            }
            _ => {
                self.repeater_side(x, y, z + 1, 3)
                    .max(self.repeater_side(x, y, z - 1, 2))
                    > 0
            }
        }
    }

    fn repeater_check(&mut self, id: u8, x: i32, y: i32, z: i32) {
        let m = self.meta(x, y, z);
        if self.repeater_locked(x, y, z, m) {
            return;
        }
        let input = self.repeater_input(x, y, z, m) > 0;
        let powered = id == POWERED_REPEATER;
        if (powered && !input || !powered && input) && !self.tick_this_tick(x, y, z, id) {
            let d = (m & 3) as usize;
            let behind = self.block(x - DX[d], y, z - DZ[d]);
            let prio = if is_repeater(behind) && (self.meta(x - DX[d], y, z - DZ[d]) & 3) != (m & 3)
            {
                -3
            } else if powered {
                -2
            } else {
                -1
            };
            self.schedule_priority(x, y, z, id, repeater_delay(m), prio);
        }
    }

    fn repeater_neighbours(&mut self, x: i32, y: i32, z: i32) {
        let m = self.meta(x, y, z);
        self.repeater_neighbours_dir(x, y, z, m & 3);
    }

    /// `BlockRedstoneDiode.updateNeighbors` for facing `d`.
    fn repeater_neighbours_dir(&mut self, x: i32, y: i32, z: i32, d: i32) {
        match d {
            1 => {
                self.notify_one(x + 1, y, z, UNPOWERED_REPEATER);
                self.notify_except(x + 1, y, z, UNPOWERED_REPEATER, 4);
            }
            3 => {
                self.notify_one(x - 1, y, z, UNPOWERED_REPEATER);
                self.notify_except(x - 1, y, z, UNPOWERED_REPEATER, 5);
            }
            2 => {
                self.notify_one(x, y, z + 1, UNPOWERED_REPEATER);
                self.notify_except(x, y, z + 1, UNPOWERED_REPEATER, 2);
            }
            _ => {
                self.notify_one(x, y, z - 1, UNPOWERED_REPEATER);
                self.notify_except(x, y, z - 1, UNPOWERED_REPEATER, 3);
            }
        }
    }

    /// `BlockRedstoneDiode.onBlockPlacedBy`
    pub fn repeater_placed(&mut self, x: i32, y: i32, z: i32) {
        let m = self.meta(x, y, z);
        if self.repeater_input(x, y, z, m) > 0 {
            self.schedule_update(x, y, z, UNPOWERED_REPEATER, 1);
        }
    }

    // ------------------------------------------------------------ levers, buttons, plates

    /// The neighbour updates after a lever or button changes state.
    fn attached_neighbours(&mut self, id: u8, k: i32, x: i32, y: i32, z: i32) {
        self.notify_neighbours(x, y, z, id);
        match k {
            1 => self.notify_neighbours(x - 1, y, z, id),
            2 => self.notify_neighbours(x + 1, y, z, id),
            3 => self.notify_neighbours(x, y, z - 1, id),
            4 => self.notify_neighbours(x, y, z + 1, id),
            5 | 6 => self.notify_neighbours(x, y - 1, z, id),
            _ => {
                if id == LEVER {
                    self.notify_neighbours(x, y + 1, z, id)
                } else {
                    self.notify_neighbours(x, y - 1, z, id)
                }
            }
        }
    }

    /// `BlockLever.onBlockActivated`
    pub fn toggle_lever(&mut self, x: i32, y: i32, z: i32) {
        let m = self.meta(x, y, z);
        let k = m & 7;
        let on = 8 - (m & 8);
        self.set_meta(x, y, z, k + on, 3);
        self.redstone_effects.push(Effect::Sound(
            "random.click",
            [x as f64 + 0.5, y as f64 + 0.5, z as f64 + 0.5],
            0.3,
            if on > 0 { 0.6 } else { 0.5 },
        ));
        self.attached_neighbours(LEVER, k, x, y, z);
    }

    /// `BlockButton.onBlockActivated`
    pub fn press_button(&mut self, id: u8, x: i32, y: i32, z: i32) {
        let m = self.meta(x, y, z);
        if m & 8 != 0 {
            return;
        }
        self.set_meta(x, y, z, (m & 7) | 8, 3);
        self.redstone_effects.push(Effect::Sound(
            "random.click",
            [x as f64 + 0.5, y as f64 + 0.5, z as f64 + 0.5],
            0.3,
            0.6,
        ));
        self.attached_neighbours(id, m & 7, x, y, z);
        self.schedule_update(x, y, z, id, if id == WOODEN_BUTTON { 30 } else { 20 });
    }

    /// `BlockBasePressurePlate.onEntityCollidedWithBlock` / `updateTick`.
    pub fn plate_update(&mut self, id: u8, x: i32, y: i32, z: i32, old_power: i32) {
        let sensitive = Aabb6 {
            min: [x as f64 + 0.125, y as f64, z as f64 + 0.125],
            max: [x as f64 + 0.875, y as f64 + 0.25, z as f64 + 0.875],
        };
        let any = self
            .occupants
            .iter()
            .any(|o| (id == WOODEN_PRESSURE_PLATE || o.living) && o.bb.intersects(&sensitive));
        let new_power = if any { 15 } else { 0 };
        if new_power != old_power {
            self.set_meta(x, y, z, if any { 1 } else { 0 }, 2);
            self.notify_neighbours(x, y, z, id);
            self.notify_neighbours(x, y - 1, z, id);
        }
        let c = [x as f64 + 0.5, y as f64 + 0.1, z as f64 + 0.5];
        if !any && old_power > 0 {
            self.redstone_effects
                .push(Effect::Sound("random.click", c, 0.3, 0.5));
        } else if any && old_power == 0 {
            self.redstone_effects
                .push(Effect::Sound("random.click", c, 0.3, 0.6));
        }
        if any {
            self.schedule_update(x, y, z, id, 20);
        }
    }

    // ------------------------------------------------------------ lamps, TNT, notes

    fn lamp_check(&mut self, id: u8, x: i32, y: i32, z: i32) {
        let p = self.indirectly_powered(x, y, z);
        if id == LIT_REDSTONE_LAMP && !p {
            self.schedule_update(x, y, z, id, 4);
        } else if id == REDSTONE_LAMP && p {
            self.set_block(x, y, z, LIT_REDSTONE_LAMP, 0, 2);
        }
    }

    /// `BlockTNT.func_150114_a`
    pub fn prime_tnt(&mut self, x: i32, y: i32, z: i32, fuse: i32) {
        self.set_block(x, y, z, AIR, 0, 3);
        self.redstone_effects.push(Effect::Tnt(
            [x as f64 + 0.5, y as f64 + 0.5, z as f64 + 0.5],
            fuse,
        ));
        self.redstone_effects.push(Effect::Sound(
            "game.tnt.primed",
            [x as f64 + 0.5, y as f64 + 0.5, z as f64 + 0.5],
            1.0,
            1.0,
        ));
    }

    fn note_trigger(&mut self, x: i32, y: i32, z: i32) {
        if block::material(self.block(x, y + 1, z)) != block::Material::Air {
            return;
        }
        let below = block::material(self.block(x, y - 1, z));
        let inst = match below {
            block::Material::Rock => "note.bd",
            block::Material::Sand => "note.snare",
            block::Material::Glass => "note.hat",
            block::Material::Wood => "note.bassattack",
            _ => "note.harp",
        };
        let pitch = self.note_pitch.get(&(x, y, z)).copied().unwrap_or(0);
        self.redstone_effects
            .push(Effect::Note([x, y, z], inst, pitch));
    }

    /// Right click on a note block: next pitch, then play (`TileEntityNote.changePitch`).
    pub fn note_click(&mut self, x: i32, y: i32, z: i32) {
        let p = self.note_pitch.entry((x, y, z)).or_insert(0);
        *p = (*p + 1) % 25;
        self.note_trigger(x, y, z);
    }

    /// `BlockTripWire.func_150140_e`
    pub(crate) fn tripwire_check(&mut self, x: i32, y: i32, z: i32) {
        let mut m = self.meta(x, y, z);
        let was = m & 1 == 1;
        let h = if m & 2 != 2 {
            0.09375
        } else if m & 4 == 0 {
            0.5
        } else {
            0.15625
        };
        let bb = Aabb6 {
            min: [x as f64, y as f64, z as f64],
            max: [x as f64 + 1.0, y as f64 + h, z as f64 + 1.0],
        };
        let now = self.occupants.iter().any(|o| o.bb.intersects(&bb));
        if now && !was {
            m |= 1;
        }
        if !now && was {
            m &= !1;
        }
        if now != was {
            self.set_meta(x, y, z, m, 3);
            super::tripwire::update_hooks(self, x, y, z, m);
        }
        if now {
            self.schedule_update(x, y, z, TRIPWIRE, 10);
        }
    }

    /// `Entity.func_145775_I` for plates and tripwire: each entity touching
    /// one sets it off.
    pub fn redstone_entity_tick(&mut self) {
        let occ = std::mem::take(&mut self.occupants);
        let mut seen: Vec<(i32, i32, i32)> = Vec::new();
        for o in &occ {
            let lo = [
                o.bb.min[0] + 0.001,
                o.bb.min[1] + 0.001,
                o.bb.min[2] + 0.001,
            ];
            let hi = [
                o.bb.max[0] - 0.001,
                o.bb.max[1] - 0.001,
                o.bb.max[2] - 0.001,
            ];
            let f = |v: f64| v.floor() as i32;
            for x in f(lo[0])..=f(hi[0]) {
                for y in f(lo[1])..=f(hi[1]) {
                    for z in f(lo[2])..=f(hi[2]) {
                        if !seen.contains(&(x, y, z)) {
                            seen.push((x, y, z));
                        }
                    }
                }
            }
        }
        self.occupants = occ;
        for (x, y, z) in seen {
            if !self.loaded(x, z) {
                continue;
            }
            let id = self.block(x, y, z);
            match id {
                STONE_PRESSURE_PLATE | WOODEN_PRESSURE_PLATE => {
                    if self.meta(x, y, z) == 0 {
                        self.plate_update(id, x, y, z, 0);
                    }
                }
                TRIPWIRE => {
                    if self.meta(x, y, z) & 1 == 0 {
                        self.tripwire_check(x, y, z);
                    }
                }
                _ => {}
            }
        }
    }

    // ------------------------------------------------------------ pistons

    /// `BlockPistonBase.isIndirectlyPowered`
    fn piston_powered(&mut self, x: i32, y: i32, z: i32, d: usize) -> bool {
        for s in 0..6 {
            let s = [0, 1, 2, 3, 5, 4][s];
            if d != s && self.indirect_power_to(x + FX[s], y + FY[s], z + FZ[s], s) > 0 {
                return true;
            }
        }
        self.indirect_power_to(x, y, z, 0) > 0
            || self.indirect_power_to(x, y + 2, z, 1) > 0
            || self.indirect_power_to(x, y + 1, z - 1, 2) > 0
            || self.indirect_power_to(x, y + 1, z + 1, 3) > 0
            || self.indirect_power_to(x - 1, y + 1, z, 4) > 0
            || self.indirect_power_to(x + 1, y + 1, z, 5) > 0
    }

    /// `BlockPistonBase.updatePistonState`
    fn piston_update(&mut self, id: u8, x: i32, y: i32, z: i32) {
        let m = self.meta(x, y, z);
        let d = (m & 7) as usize;
        if d == 7 {
            return;
        }
        let powered = self.piston_powered(x, y, z, d.min(5));
        if powered && m & 8 == 0 {
            if self.piston_can_extend(x, y, z, d) {
                self.add_block_event(x, y, z, id, 0, d as i32);
            }
        } else if !powered && m & 8 != 0 {
            self.set_meta(x, y, z, d as i32, 2);
            self.add_block_event(x, y, z, id, 1, d as i32);
        }
    }

    /// `World.addBlockEvent` (`WorldServer`: identical events are queued once).
    pub fn add_block_event(&mut self, x: i32, y: i32, z: i32, id: u8, a: i32, b: i32) {
        let e = BlockEvent {
            x,
            y,
            z,
            block: id,
            a,
            b,
        };
        if !self.block_events.contains(&e) {
            self.block_events.push(e);
        }
    }

    /// `WorldServer.func_147488_Z`: runs the queued block events; events added
    /// meanwhile run in the next round of the same call.
    pub fn run_block_events(&mut self) {
        while !self.block_events.is_empty() {
            let list = std::mem::take(&mut self.block_events);
            for e in list {
                if self.block(e.x, e.y, e.z) == e.block {
                    self.block_event_received(e);
                }
            }
        }
    }

    /// `Block.onBlockEventReceived` for the blocks that use events.
    fn block_event_received(&mut self, e: BlockEvent) -> bool {
        match e.block {
            PISTON | STICKY_PISTON => self.piston_event(e.block, e.x, e.y, e.z, e.a, e.b as usize),
            _ => false,
        }
    }

    /// `BlockPistonBase.onBlockEventReceived`
    fn piston_event(&mut self, id: u8, x: i32, y: i32, z: i32, kind: i32, d: usize) -> bool {
        let powered = self.piston_powered(x, y, z, d.min(5));
        if powered && kind == 1 {
            self.set_meta(x, y, z, d as i32 | 8, 2);
            return false;
        }
        if !powered && kind == 0 {
            return false;
        }
        let c = [x as f64 + 0.5, y as f64 + 0.5, z as f64 + 0.5];
        if kind == 0 {
            if !self.piston_extend(id, x, y, z, d) {
                return false;
            }
            self.set_meta(x, y, z, d as i32 | 8, 2);
            let p = self.rand.next_float() * 0.25 + 0.6;
            self.redstone_effects
                .push(Effect::Sound("tile.piston.out", c, 0.5, p));
        } else if kind == 1 {
            let (hx, hy, hz) = (x + FX[d], y + FY[d], z + FZ[d]);
            if let Some(i) = self.moving.iter().position(|m| m.pos == [hx, hy, hz]) {
                self.clear_moving(i);
            }
            self.set_block(x, y, z, PISTON_EXTENSION, d as i32, 3);
            self.set_moving(Moving::new([x, y, z], id, d as i32, d, false, true));
            if id == STICKY_PISTON {
                let (bx, by, bz) = (x + FX[d] * 2, y + FY[d] * 2, z + FZ[d] * 2);
                let mut b = self.block(bx, by, bz);
                let mut bm = self.meta(bx, by, bz);
                let mut pulled_moving = false;
                if b == PISTON_EXTENSION {
                    if let Some(i) = self.moving.iter().position(|m| m.pos == [bx, by, bz]) {
                        let m = self.moving[i];
                        if m.facing == d && m.extending {
                            self.clear_moving(i);
                            b = m.block;
                            bm = m.meta;
                            pulled_moving = true;
                        }
                    }
                }
                if pulled_moving
                    || block::material(b) == block::Material::Air
                    || !pushable(self, b, bx, by, bz, false)
                    || mobility(b) != 0 && b != PISTON && b != STICKY_PISTON
                {
                    if !pulled_moving {
                        self.set_block(hx, hy, hz, AIR, 0, 3);
                    }
                } else {
                    self.set_block(hx, hy, hz, PISTON_EXTENSION, bm, 3);
                    self.set_moving(Moving::new([hx, hy, hz], b, bm, d, false, false));
                    self.set_block(bx, by, bz, AIR, 0, 3);
                }
            } else {
                self.set_block(hx, hy, hz, AIR, 0, 3);
            }
            let p = self.rand.next_float() * 0.15 + 0.6;
            self.redstone_effects
                .push(Effect::Sound("tile.piston.in", c, 0.5, p));
        }
        true
    }

    /// `BlockPistonBase.canExtend`
    fn piston_can_extend(&mut self, x: i32, y: i32, z: i32, d: usize) -> bool {
        if d > 5 {
            return true;
        }
        let (mut px, mut py, mut pz) = (x + FX[d], y + FY[d], z + FZ[d]);
        for n in 0..13 {
            if py <= 0 || py >= 255 {
                return false;
            }
            let b = self.block(px, py, pz);
            if block::material(b) != block::Material::Air {
                if !pushable(self, b, px, py, pz, true) {
                    return false;
                }
                if mobility(b) != 1 {
                    if n == 12 {
                        return false;
                    }
                    px += FX[d];
                    py += FY[d];
                    pz += FZ[d];
                    continue;
                }
            }
            break;
        }
        true
    }

    /// `BlockPistonBase.tryExtend`
    fn piston_extend(&mut self, id: u8, x: i32, y: i32, z: i32, d: usize) -> bool {
        if d > 5 {
            return false;
        }
        let (mut px, mut py, mut pz) = (x + FX[d], y + FY[d], z + FZ[d]);
        let mut n = 0;
        while n < 13 {
            if py <= 0 || py >= 255 {
                return false;
            }
            let b = self.block(px, py, pz);
            if block::material(b) == block::Material::Air {
                break;
            }
            if !pushable(self, b, px, py, pz, true) {
                return false;
            }
            if mobility(b) != 1 {
                if n == 12 {
                    return false;
                }
                px += FX[d];
                py += FY[d];
                pz += FZ[d];
                n += 1;
            } else {
                let bm = self.meta(px, py, pz);
                self.drop_block_as_item(b, bm, px, py, pz);
                self.set_block(px, py, pz, AIR, 0, 3);
                break;
            }
        }
        let end = (px, py, pz);
        let sticky = if id == STICKY_PISTON { 8 } else { 0 };
        let mut moved = Vec::new();
        while (px, py, pz) != (x, y, z) {
            let (sx, sy, sz) = (px - FX[d], py - FY[d], pz - FZ[d]);
            let b = self.block(sx, sy, sz);
            let bm = self.meta(sx, sy, sz);
            if b == id && (sx, sy, sz) == (x, y, z) {
                self.set_block(px, py, pz, PISTON_EXTENSION, d as i32 | sticky, 4);
                self.set_moving(Moving::new(
                    [px, py, pz],
                    PISTON_HEAD,
                    d as i32 | sticky,
                    d,
                    true,
                    false,
                ));
            } else {
                self.set_block(px, py, pz, PISTON_EXTENSION, bm, 4);
                self.set_moving(Moving::new([px, py, pz], b, bm, d, true, false));
            }
            moved.push(b);
            px = sx;
            py = sy;
            pz = sz;
        }
        let (mut px, mut py, mut pz) = end;
        let mut i = 0;
        while (px, py, pz) != (x, y, z) {
            let (sx, sy, sz) = (px - FX[d], py - FY[d], pz - FZ[d]);
            self.notify_neighbours(sx, sy, sz, moved[i]);
            i += 1;
            px = sx;
            py = sy;
            pz = sz;
        }
        true
    }

    /// `Block.dropBlockAsItem` for blocks a piston breaks.
    fn drop_block_as_item(&mut self, b: u8, bm: i32, x: i32, y: i32, z: i32) {
        let mut r = std::mem::replace(&mut self.rand, JavaRandom::new(0));
        let drops = crate::item::drops(b, bm, &mut r);
        self.rand = r;
        for st in drops {
            self.gen_entities.push(GenEntity::Item {
                pos: [x as f64 + 0.5, y as f64 + 0.5, z as f64 + 0.5],
                stack: st,
            });
        }
    }

    /// `World.setTileEntity` for a moving block.
    fn set_moving(&mut self, m: Moving) {
        if let Some(i) = self.moving.iter().position(|o| o.pos == m.pos) {
            self.moving.remove(i);
        }
        self.moving.push(m);
    }

    pub fn moving_at(&self, pos: [i32; 3]) -> Option<&Moving> {
        self.moving.iter().find(|m| m.pos == pos)
    }

    /// `TileEntityPiston.clearPistonTileEntity`
    fn clear_moving(&mut self, i: usize) {
        let m = self.moving[i];
        if m.last < 1.0 {
            self.moving.remove(i);
            let [x, y, z] = m.pos;
            if self.block(x, y, z) == PISTON_EXTENSION {
                self.set_block(x, y, z, m.block, m.meta, 3);
                self.notify_one(x, y, z, m.block);
            }
        }
    }

    /// `TileEntityPiston.updateEntity` for every moving block, in list order.
    /// Blocks that start moving during the loop wait for the next tick.
    pub fn tick_moving(&mut self) {
        let order: Vec<[i32; 3]> = self.moving.iter().map(|m| m.pos).collect();
        for pos in order {
            let Some(i) = self.moving.iter().position(|m| m.pos == pos) else {
                continue;
            };
            let mut m = self.moving[i];
            m.last = m.progress;
            if m.last >= 1.0 {
                self.launch(&m, 1.0, 0.25);
                self.moving.remove(i);
                let [x, y, z] = pos;
                if self.block(x, y, z) == PISTON_EXTENSION {
                    self.set_block(x, y, z, m.block, m.meta, 3);
                    self.notify_one(x, y, z, m.block);
                }
            } else {
                m.progress += 0.5;
                if m.progress >= 1.0 {
                    m.progress = 1.0;
                }
                self.moving[i] = m;
                if m.extending {
                    self.launch(&m, m.progress, m.progress - m.last + 0.0625);
                }
            }
        }
    }

    /// `TileEntityPiston.launchWithSlimeBlock`: the entities in the moving
    /// block's box are moved along with it.
    fn launch(&mut self, m: &Moving, f: f32, dist: f32) {
        let f = if m.extending { 1.0 - f } else { f - 1.0 };
        let Some(bb) = self.moving_collision_shape(m, f) else {
            return;
        };
        let d = m.facing;
        self.pushes.push((
            bb,
            [
                (dist * FX[d] as f32) as f64,
                (dist * FY[d] as f32) as f64,
                (dist * FZ[d] as f32) as f64,
            ],
        ));
    }

    /// `BlockPistonMoving.getCollisionShape`
    fn moving_collision_shape(&mut self, m: &Moving, f: f32) -> Option<Aabb6> {
        moving_shape(self, m.pos, m.block, m.meta, m.facing, f)
    }
}

/// `BlockPistonMoving.getCollisionShape` against any block view.
pub fn moving_shape(
    w: &dyn super::BlockView,
    pos: [i32; 3],
    id: u8,
    meta: i32,
    d: usize,
    f: f32,
) -> Option<Aabb6> {
    if id == PISTON_EXTENSION || block::material(id) == block::Material::Air {
        return None;
    }
    let get = |x: i32, y: i32, z: i32| (w.peek_block(x, y, z), w.peek_meta(x, y, z));
    let mut boxes = Vec::new();
    crate::shapes::collision(&get, id, meta as u8, pos[0], pos[1], pos[2], &mut boxes);
    if boxes.is_empty() {
        return None;
    }
    let mut u = boxes[0];
    for b in &boxes[1..] {
        for a in 0..3 {
            u[a] = u[a].min(b[a]);
            u[a + 3] = u[a + 3].max(b[a + 3]);
        }
    }
    let mut min = [
        pos[0] as f64 + u[0] as f64,
        pos[1] as f64 + u[1] as f64,
        pos[2] as f64 + u[2] as f64,
    ];
    let mut max = [
        pos[0] as f64 + u[3] as f64,
        pos[1] as f64 + u[4] as f64,
        pos[2] as f64 + u[5] as f64,
    ];
    for (a, o) in [FX[d], FY[d], FZ[d]].into_iter().enumerate() {
        if o < 0 {
            min[a] -= (o as f32 * f) as f64;
        } else {
            max[a] -= (o as f32 * f) as f64;
        }
    }
    Some(Aabb6 { min, max })
}

/// A queued `addBlockEvent` (`BlockEventData`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BlockEvent {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub block: u8,
    pub a: i32,
    pub b: i32,
}

/// `TileEntityPiston`
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Moving {
    pub pos: [i32; 3],
    /// `storedBlock` / `storedMetadata`
    pub block: u8,
    pub meta: i32,
    /// `storedOrientation`
    pub facing: usize,
    pub extending: bool,
    /// `shouldHeadBeRendered`
    pub head: bool,
    pub progress: f32,
    /// `lastProgress`
    pub last: f32,
}

/// What collision and rendering read from a moving block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MovingView {
    pub block: u8,
    pub meta: u8,
    pub facing: u8,
    pub extending: bool,
    pub head: bool,
    pub progress: f32,
    pub last: f32,
}

impl Moving {
    pub fn new(
        pos: [i32; 3],
        block: u8,
        meta: i32,
        facing: usize,
        extending: bool,
        head: bool,
    ) -> Moving {
        Moving {
            pos,
            block,
            meta,
            facing,
            extending,
            head,
            progress: 0.0,
            last: 0.0,
        }
    }

    pub fn view(&self) -> MovingView {
        MovingView {
            block: self.block,
            meta: self.meta as u8,
            facing: self.facing as u8,
            extending: self.extending,
            head: self.head,
            progress: self.progress,
            last: self.last,
        }
    }
}

impl MovingView {
    /// `TileEntityPiston.getProgress`
    pub fn progress(&self, partial: f32) -> f32 {
        let p = partial.min(1.0);
        self.last + (self.progress - self.last) * p
    }

    /// `getRenderOffsetX/Y/Z`
    pub fn offset(&self, partial: f32) -> [f32; 3] {
        let d = self.facing as usize;
        let k = if self.extending {
            self.progress(partial) - 1.0
        } else {
            1.0 - self.progress(partial)
        };
        [k * FX[d] as f32, k * FY[d] as f32, k * FZ[d] as f32]
    }

    /// The collision box `BlockPistonMoving.getCollisionBoundingBoxFromPool`
    /// gives at `pos`.
    pub fn collision(&self, w: &dyn super::BlockView, pos: [i32; 3]) -> Option<Aabb6> {
        let mut f = self.progress(0.0);
        if self.extending {
            f = 1.0 - f;
        }
        moving_shape(
            w,
            pos,
            self.block,
            self.meta as i32,
            self.facing as usize,
            f,
        )
    }
}

/// `BlockRedstoneRepeater.getDelay`
fn repeater_delay(m: i32) -> i32 {
    [1, 2, 3, 4][((m & 12) >> 2) as usize] * 2
}

/// `Block.getMobilityFlag`: 0 pushed, 1 destroyed, 2 immovable.
pub fn mobility(id: u8) -> i32 {
    use block::Material as M;
    match id {
        STONE_PRESSURE_PLATE | WOODEN_PRESSURE_PLATE | WOODEN_DOOR | IRON_DOOR => 1,
        RAIL | GOLDEN_RAIL | DETECTOR_RAIL => 0,
        _ => match block::material(id) {
            M::Water
            | M::Lava
            | M::Plants
            | M::Vine
            | M::Web
            | M::Cactus
            | M::Gourd
            | M::Fire
            | M::Snow
            | M::Circuits
            | M::Leaves
            | M::Carpet => 1,
            M::Portal => 2,
            _ => 0,
        },
    }
}

/// `BlockPistonBase.canPushBlock`
fn pushable(w: &ServerWorld, id: u8, x: i32, y: i32, z: i32, destroy_ok: bool) -> bool {
    if id == OBSIDIAN {
        return false;
    }
    if id == PISTON || id == STICKY_PISTON {
        if w.peek_meta(x, y, z) & 8 != 0 {
            return false;
        }
    } else {
        if crate::item::hardness(id) == -1.0 {
            return false;
        }
        match mobility(id) {
            2 => return false,
            1 => return destroy_ok,
            _ => {}
        }
    }
    // ITileEntityProvider blocks
    !matches!(
        id,
        CHEST
            | FURNACE
            | LIT_FURNACE
            | DISPENSER
            | MOB_SPAWNER
            | NOTEBLOCK
            | JUKEBOX
            | FLOWER_POT
            | PISTON_EXTENSION
            | END_PORTAL
    )
}
