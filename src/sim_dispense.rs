//! `BlockDispenser.dispense` with the behaviours `Bootstrap` registers,
//! and `BlockDropper.dispense`.

use super::*;
use crate::entity::base::Fx;
use crate::hopper::{FX, FY, FZ};
use crate::item::Stack;
use crate::play::{Play, Pos, Tile};

type Sounds = Vec<(&'static str, [f64; 3], f32, f32)>;

impl Sim {
    /// `BlockDispenser.updateTick` → `dispense` (or the dropper's).
    /// `rnd` is the world's random, lent out by the caller.
    pub(super) fn dispense_block(
        &mut self,
        play: &mut Play,
        p: Pos,
        facing: i32,
        rnd: &mut JavaRandom,
        sounds: &mut Sounds,
    ) {
        let Some(Tile::Dispenser { slots, dropper }) = play.tiles.get(&p).cloned() else {
            return;
        };
        // TileEntityDispenser.func_146017_i with the tile's own random
        let mut pick = None;
        let mut j = 1;
        for (k, s) in slots.iter().enumerate() {
            if s.is_some() {
                if self.tile_rand.next_int_n(j) == 0 {
                    pick = Some(k);
                }
                j += 1;
            }
        }
        let Some(k) = pick else {
            self.aux_sound(1001, p, sounds);
            return;
        };
        let s = slots[k].unwrap();
        let f = (facing as usize).min(5);
        let left = if dropper {
            self.dropper_dispense(play, p, f, s, rnd, sounds)
        } else {
            self.dispense_stack(play, p, f, s, rnd, sounds)
        };
        if let Some(Tile::Dispenser { slots, .. }) = play.tiles.get_mut(&p) {
            slots[k] = left.filter(|s| s.count > 0);
        }
        play.inv_dirty = true;
    }

    /// `BlockDropper.dispense`: into the inventory in front, else dropped.
    fn dropper_dispense(
        &mut self,
        play: &mut Play,
        p: Pos,
        f: usize,
        s: Stack,
        rnd: &mut JavaRandom,
        sounds: &mut Sounds,
    ) -> Option<Stack> {
        let front = [p[0] + FX[f], p[1] + FY[f], p[2] + FZ[f]];
        let target = play.inventory_at(
            &self.world,
            front[0] as f64,
            front[1] as f64,
            front[2] as f64,
            rnd,
        );
        if let Some(t) = target {
            let one = Stack { count: 1, ..s };
            let side = [1, 0, 3, 2, 5, 4][f];
            return if play.push_items(&t, one, side).is_none() {
                Some(Stack {
                    count: s.count - 1,
                    ..s
                })
            } else {
                Some(s)
            };
        }
        // dropBehavior: a BehaviorDefaultDispenseItem
        let left = self.default_dispense(play, p, f, s, rnd);
        self.aux_sound(1000, p, sounds);
        self.smoke(play, p, f);
        Some(left)
    }

    /// `IBehaviorDispenseItem.dispense` from the registry: returns the
    /// stack left in the slot.
    fn dispense_stack(
        &mut self,
        play: &mut Play,
        p: Pos,
        f: usize,
        s: Stack,
        rnd: &mut JavaRandom,
        sounds: &mut Sounds,
    ) -> Option<Stack> {
        use crate::entity::throwable::Kind as TK;
        let dir = [FX[f] as f64, FY[f] as f64, FZ[f] as f64];
        let c = [p[0] as f64 + 0.5, p[1] as f64 + 0.5, p[2] as f64 + 0.5];
        // getIPositionFromBlockSource
        let ip = [
            c[0] + 0.7 * dir[0],
            c[1] + 0.7 * dir[1],
            c[2] + 0.7 * dir[2],
        ];
        let front = [p[0] + FX[f], p[1] + FY[f], p[2] + FZ[f]];
        let one_less = |s: Stack| Stack {
            count: s.count - 1,
            ..s
        };
        // BehaviorProjectileDispense: velocity, inaccuracy
        let projectile = match s.id {
            262 => Some((None, 1.1f32, 6.0f32)),
            344 => Some((Some(TK::Egg), 1.1, 6.0)),
            332 => Some((Some(TK::Snowball), 1.1, 6.0)),
            384 => Some((Some(TK::ExpBottle), 1.1 * 1.25, 6.0 * 0.5)),
            373 if crate::item_use::is_splash(s.meta) => {
                Some((Some(TK::Potion(s.meta)), 1.1 * 1.25, 6.0 * 0.5))
            }
            _ => None,
        };
        if let Some((kind, v, inacc)) = projectile {
            match kind {
                // Bootstrap$1: an arrow that can be picked up
                None => self.mobs.dispense_arrow(ip, dir),
                Some(k) => {
                    let id = self.mobs.rand.next_int() as u32;
                    let mut t = crate::entity::throwable::Throwable::at_pos(id, k, ip);
                    t.set_heading(dir[0], dir[1] + 0.1f32 as f64, dir[2], v, inacc);
                    self.mobs.throwables.push(t);
                }
            }
            self.aux_sound(1002, p, sounds);
            self.smoke(play, p, f);
            return Some(one_less(s));
        }
        let left = match s.id {
            // Bootstrap$6: ItemMonsterPlacer.spawnCreature in front
            383 => {
                let at = [c[0] + dir[0], p[1] as f64 + 0.2f32 as f64, c[2] + dir[2]];
                self.spawn_creature_with(s.meta, at, rnd);
                self.aux_sound(1000, p, sounds);
                one_less(s)
            }
            // Bootstrap$7: EntityFireworkRocket (not ported yet: the rocket is
            // used up without flying)
            401 => {
                self.aux_sound(1002, p, sounds);
                one_less(s)
            }
            // Bootstrap$8: EntitySmallFireball (y takes the X offset, as in 1.7.10)
            385 => {
                let at = [
                    ip[0] + (FX[f] as f32 * 0.3) as f64,
                    ip[1] + (FX[f] as f32 * 0.3) as f64,
                    ip[2] + (FZ[f] as f32 * 0.3) as f64,
                ];
                let a = [
                    rnd.next_gaussian() * 0.05 + dir[0],
                    rnd.next_gaussian() * 0.05 + dir[1],
                    rnd.next_gaussian() * 0.05 + dir[2],
                ];
                self.mobs
                    .fireballs
                    .push(crate::entity::mob::Fireball::at(at, a, true));
                self.aux_sound(1009, p, sounds);
                one_less(s)
            }
            // Bootstrap$9: a boat on or above water in front
            333 => {
                let m = block::material(self.world.block(front[0], front[1], front[2]));
                let lift = if m == block::Material::Water {
                    Some(1.0)
                } else if m == block::Material::Air
                    && block::material(self.world.block(front[0], front[1] - 1, front[2]))
                        == block::Material::Water
                {
                    Some(0.0)
                } else {
                    None
                };
                self.aux_sound(1000, p, sounds);
                match lift {
                    Some(l) => {
                        let at = [
                            c[0] + (FX[f] as f32 * 1.125) as f64,
                            c[1] + (FY[f] as f32 * 1.125) as f64 + l,
                            c[2] + (FZ[f] as f32 * 1.125) as f64,
                        ];
                        let id = self.alloc_vehicle_id();
                        let seed = self.mobs.rand.next_long();
                        let boat = crate::entity::boat::Boat::at(id, at[0], at[1], at[2], seed);
                        self.world.solid_entities.push((id, boat.bounding_box()));
                        play.vehicles.push(crate::entity::vehicle::Tracked::new(
                            crate::entity::vehicle::Vehicle::Boat(boat),
                        ));
                        one_less(s)
                    }
                    // fallback.dispense: dropped, with its own sound and smoke
                    None => {
                        let left = self.default_dispense(play, p, f, s, rnd);
                        self.aux_sound(1000, p, sounds);
                        self.smoke(play, p, f);
                        left
                    }
                }
            }
            // Bootstrap$10: pour the bucket in front
            326 | 327 => {
                let liquid = if s.id == 326 {
                    block::FLOWING_WATER
                } else {
                    block::FLOWING_LAVA
                };
                let mut fx = Vec::new();
                let hell = self.dim == -1;
                if items::try_place_contained_liquid(
                    &mut self.world,
                    rnd,
                    liquid,
                    front[0],
                    front[1],
                    front[2],
                    hell,
                    &mut fx,
                ) {
                    play.fx.extend(fx);
                    self.aux_sound(1000, p, sounds);
                    Stack::new(items::BUCKET, 0, 1)
                } else {
                    let left = self.default_dispense(play, p, f, s, rnd);
                    self.aux_sound(1000, p, sounds);
                    self.smoke(play, p, f);
                    self.aux_sound(1000, p, sounds);
                    left
                }
            }
            // Bootstrap$11: fill an empty bucket from a source in front
            325 => {
                let (x, y, z) = (front[0], front[1], front[2]);
                let m = block::material(self.world.block(x, y, z));
                let meta = self.world.meta(x, y, z);
                let full = if m == block::Material::Water && meta == 0 {
                    Some(items::WATER_BUCKET)
                } else if m == block::Material::Lava && meta == 0 {
                    Some(items::LAVA_BUCKET)
                } else {
                    None
                };
                let left = match full {
                    None => self.default_dispense(play, p, f, s, rnd),
                    Some(full) => {
                        self.world.set_block(x, y, z, block::AIR, 0, 3);
                        let rest = one_less(s);
                        if rest.count == 0 {
                            Stack::new(full, 0, 1)
                        } else {
                            // TileEntityDispenser.addItemStack, else dispensed
                            let mut put = false;
                            if let Some(Tile::Dispenser { slots, .. }) = play.tiles.get_mut(&p) {
                                if let Some(e) = slots.iter_mut().find(|s| s.is_none()) {
                                    *e = Some(Stack::new(full, 0, 1));
                                    put = true;
                                }
                            }
                            if !put {
                                self.default_dispense(play, p, f, Stack::new(full, 0, 1), rnd);
                                self.aux_sound(1000, p, sounds);
                                self.smoke(play, p, f);
                            }
                            rest
                        }
                    }
                };
                self.aux_sound(1000, p, sounds);
                left
            }
            // Bootstrap$12: fire, or a primed TNT, in front
            259 => {
                let (x, y, z) = (front[0], front[1], front[2]);
                let mut left = s;
                if block::material(self.world.block(x, y, z)) == block::Material::Air {
                    self.world.set_block(x, y, z, block::FIRE, 0, 3);
                    // attemptDamageItem(1, world.rand)
                    let (rest, broke) = crate::enchant::damage_item(s, 1, false, rnd);
                    left = if broke {
                        Stack { count: 0, ..s }
                    } else {
                        rest.unwrap_or(Stack { count: 0, ..s })
                    };
                } else if self.world.block(x, y, z) == block::TNT {
                    // BlockTNT.onBlockDestroyedByPlayer(meta 1), setBlockToAir
                    self.world.prime_tnt(x, y, z, 80);
                } else {
                    self.flint_succeeded = false;
                }
                let code = if self.flint_succeeded { 1000 } else { 1001 };
                if crate::compat::fixed() {
                    // MC-?: the flag never went back to true
                    self.flint_succeeded = true;
                }
                self.aux_sound(code, p, sounds);
                left
            }
            // Bootstrap$13: bone meal on the block in front
            351 if s.meta == 15 => {
                let (x, y, z) = (front[0], front[1], front[2]);
                let mut left = s;
                if self.world.bone_meal(x, y, z) {
                    self.world
                        .redstone_effects
                        .push(crate::world::redstone::Effect::BoneMeal([x, y, z]));
                    left = one_less(s);
                } else {
                    self.dye_succeeded = false;
                }
                let code = if self.dye_succeeded { 1000 } else { 1001 };
                if crate::compat::fixed() {
                    self.dye_succeeded = true;
                }
                self.aux_sound(code, p, sounds);
                left
            }
            // Bootstrap$14: EntityTNTPrimed in front
            46 => {
                let q = [
                    (front[0] as f32 + 0.5) as f64,
                    (front[1] as f32 + 0.5) as f64,
                    (front[2] as f32 + 0.5) as f64,
                ];
                self.mobs.prime_tnt(q, 80);
                self.aux_sound(1000, p, sounds);
                one_less(s)
            }
            _ => {
                let left = self.default_dispense(play, p, f, s, rnd);
                self.aux_sound(1000, p, sounds);
                left
            }
        };
        self.smoke(play, p, f);
        Some(left)
    }

    /// `BehaviorDefaultDispenseItem.dispenseStack` + `doDispense(.., 6, ..)`:
    /// one item thrown out of the face, 0.3 below the dispense position.
    fn default_dispense(
        &mut self,
        play: &mut Play,
        p: Pos,
        f: usize,
        s: Stack,
        rnd: &mut JavaRandom,
    ) -> Stack {
        let dir = [FX[f] as f64, FY[f] as f64, FZ[f] as f64];
        let ip = [
            p[0] as f64 + 0.5 + 0.7 * dir[0],
            p[1] as f64 + 0.5 + 0.7 * dir[1],
            p[2] as f64 + 0.5 + 0.7 * dir[2],
        ];
        let one = Stack { count: 1, ..s };
        let id = play.next_item;
        play.next_item += 1;
        let mut e = crate::entity::item::ItemEntity::new(id, one, [ip[0], ip[1] - 0.3, ip[2]]);
        let d = rnd.next_double() * 0.1 + 0.2;
        e.e.motion = [dir[0] * d, 0.2f32 as f64, dir[2] * d];
        let k = 0.0075f32 as f64 * 6.0;
        e.e.motion[0] += rnd.next_gaussian() * k;
        e.e.motion[1] += rnd.next_gaussian() * k;
        e.e.motion[2] += rnd.next_gaussian() * k;
        play.spawn(e);
        Stack {
            count: s.count - 1,
            ..s
        }
    }

    /// `World.playAuxSFX` sounds of dispensers (`RenderGlobal.playAuxSFX`).
    fn aux_sound(&mut self, code: i32, p: Pos, sounds: &mut Sounds) {
        let c = [p[0] as f64 + 0.5, p[1] as f64 + 0.5, p[2] as f64 + 0.5];
        match code {
            1000 => sounds.push(("random.click", c, 1.0, 1.0)),
            1001 => sounds.push(("random.click", c, 1.0, 1.2)),
            1002 => sounds.push(("random.bow", c, 1.0, 1.2)),
            1009 => {
                let r = &mut self.av_rand;
                let pitch = (r.next_float() - r.next_float()) * 0.2 + 1.0;
                sounds.push(("mob.ghast.fireball", c, 2.0, pitch));
            }
            _ => {}
        }
    }

    /// `playAuxSFX(2000)`: ten puffs of smoke at the face.
    fn smoke(&mut self, play: &mut Play, p: Pos, f: usize) {
        let data = FX[f] + 1 + (FZ[f] + 1) * 3;
        let var37 = data % 3 - 1;
        let var10 = data / 3 % 3 - 1;
        let x = p[0] as f64 + var37 as f64 * 0.6 + 0.5;
        let y = p[1] as f64 + 0.5;
        let z = p[2] as f64 + var10 as f64 * 0.6 + 0.5;
        let r = &mut self.av_rand;
        for _ in 0..10 {
            let v = r.next_double() * 0.2 + 0.01;
            let px = x + var37 as f64 * 0.01 + (r.next_double() - 0.5) * var10 as f64 * 0.5;
            let py = y + (r.next_double() - 0.5) * 0.5;
            let pz = z + var10 as f64 * 0.01 + (r.next_double() - 0.5) * var37 as f64 * 0.5;
            let mx = var37 as f64 * v + r.next_gaussian() * 0.01;
            let my = -0.03 + r.next_gaussian() * 0.01;
            let mz = var10 as f64 * v + r.next_gaussian() * 0.01;
            play.fx
                .push(Fx::Particle("smoke", [px, py, pz], [mx, my, mz]));
        }
    }
}
