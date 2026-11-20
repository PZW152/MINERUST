//! `EntityFishHook`, the server side of fishing: casting, the bobber's
//! flight and float, the fish approaching and biting, hooking entities,
//! and `handleHookRetraction` with the `getFishingResult` loot tables.
//! The caller (`Sim`) owns the angler checks and the caught entity, as
//! the hook's `angler` and `caughtEntity` fields live outside it here.

use super::base::{Ent, Fx};
use super::player::Aabb;
use super::raytrace::{calculate_intercept, ray_trace_blocks};
use crate::block::{self, Material};
use crate::item::Stack;
use crate::java::JavaRandom;
use crate::util::math;
use crate::world::{BlockView, ServerWorld};

/// What the hook can catch (`Entity.canBeCollidedWith`). The angler
/// itself is collidable but `EntityPlayer.attackEntityFrom` refuses the
/// hook's 0 damage, so it is never caught and is left out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Caught {
    Mob(u32),
    Vehicle(u32),
}

pub struct FishHook {
    pub e: Ent,
    tile: [i32; 3],
    /// `inTile`: never set, so null
    in_tile: Option<u8>,
    pub in_ground: bool,
    pub shake: i32,
    ticks_in_ground: i32,
    ticks_in_air: i32,
    ticks_catchable: i32,
    ticks_caught_delay: i32,
    ticks_catchable_delay: i32,
    fish_approach_angle: f32,
    pub caught: Option<Caught>,
}

/// A server particle packet (`WorldServer.func_147487_a`): name, position,
/// count, offsets, speed. `NetHandlerPlayClient.handleParticles` turns it
/// into particles with its own random (see `expand`).
pub struct Burst {
    pub name: &'static str,
    pub pos: [f64; 3],
    pub count: i32,
    pub off: [f64; 3],
    pub speed: f64,
}

impl Burst {
    /// `NetHandlerPlayClient.handleParticles`
    pub fn expand(&self, av: &mut JavaRandom, out: &mut Vec<Fx>) {
        if self.count == 0 {
            out.push(Fx::Particle(
                self.name,
                self.pos,
                [
                    self.speed * self.off[0],
                    self.speed * self.off[1],
                    self.speed * self.off[2],
                ],
            ));
        } else {
            for _ in 0..self.count {
                let dx = av.next_gaussian() * self.off[0];
                let dy = av.next_gaussian() * self.off[1];
                let dz = av.next_gaussian() * self.off[2];
                let mx = av.next_gaussian() * self.speed;
                let my = av.next_gaussian() * self.speed;
                let mz = av.next_gaussian() * self.speed;
                out.push(Fx::Particle(
                    self.name,
                    [self.pos[0] + dx, self.pos[1] + dy, self.pos[2] + dz],
                    [mx, my, mz],
                ));
            }
        }
    }
}

/// `MathHelper.getRandomIntegerInRange`
fn rand_int_range(r: &mut JavaRandom, a: i32, b: i32) -> i32 {
    if a >= b {
        a
    } else {
        r.next_int_n(b - a + 1) + a
    }
}

/// `MathHelper.randomFloatClamp`
fn rand_float_clamp(r: &mut JavaRandom, a: f32, b: f32) -> f32 {
    if a >= b {
        a
    } else {
        r.next_float() * (b - a) + a
    }
}

impl FishHook {
    /// `new EntityFishHook(world, player)`: `feet` is the server player's
    /// `posX/posY/posZ` (its `yOffset` is 0, so the eyes are 1.62 up).
    pub fn cast(seed: i64, feet: [f64; 3], yaw: f32, pitch: f32) -> FishHook {
        let mut e = Ent::new(0.25, 0.25, seed);
        e.can_trigger_walking = false;
        e.set_location_and_angles(feet[0], feet[1] + 1.62, feet[2], yaw, pitch);
        let r = std::f32::consts::PI;
        e.pos[0] -= (math::cos(e.yaw / 180.0 * r) * 0.16) as f64;
        e.pos[1] -= 0.1f32 as f64;
        e.pos[2] -= (math::sin(e.yaw / 180.0 * r) * 0.16) as f64;
        let p = e.pos;
        e.set_position(p[0], p[1], p[2]);
        e.y_offset = 0.0;
        let k = 0.4f32;
        let mx = (-math::sin(e.yaw / 180.0 * r) * math::cos(e.pitch / 180.0 * r) * k) as f64;
        let mz = (math::cos(e.yaw / 180.0 * r) * math::cos(e.pitch / 180.0 * r) * k) as f64;
        let my = (-math::sin(e.pitch / 180.0 * r) * k) as f64;
        let mut h = FishHook {
            e,
            tile: [-1, -1, -1],
            in_tile: None,
            in_ground: false,
            shake: 0,
            ticks_in_ground: 0,
            ticks_in_air: 0,
            ticks_catchable: 0,
            ticks_caught_delay: 0,
            ticks_catchable_delay: 0,
            fish_approach_angle: 0.0,
            caught: None,
        };
        h.handle_hook_casting(mx, my, mz, 1.5, 1.0);
        h
    }

    /// `handleHookCasting`
    fn handle_hook_casting(&mut self, mut x: f64, mut y: f64, mut z: f64, v: f32, inacc: f32) {
        let l = math::sqrt_double(x * x + y * y + z * z) as f64;
        x /= l;
        y /= l;
        z /= l;
        let r = &mut self.e.rand;
        let k = 0.0075f32 as f64 * inacc as f64;
        x += r.next_gaussian() * k;
        y += r.next_gaussian() * k;
        z += r.next_gaussian() * k;
        x *= v as f64;
        y *= v as f64;
        z *= v as f64;
        self.e.motion = [x, y, z];
        let h = math::sqrt_double(x * x + z * z) as f64;
        let d = 180.0 / std::f32::consts::PI as f64;
        self.e.yaw = (x.atan2(z) * d) as f32;
        self.e.prev_yaw = self.e.yaw;
        self.e.pitch = (y.atan2(h) * d) as f32;
        self.e.prev_pitch = self.e.pitch;
        self.ticks_in_ground = 0;
    }

    /// The start of `onUpdate`: `super.onUpdate()` (`Entity.onEntityUpdate`).
    pub fn base_tick(&mut self, w: &dyn BlockView, fx: &mut Vec<Fx>) {
        self.e.ticks_existed += 1;
        self.e.base_tick(w, false, fx);
    }

    /// `onUpdate` while an entity is hooked: the hook sits on it.
    pub fn follow(&mut self, pos: [f64; 3], min_y: f64, height: f32) {
        // posX/posY/posZ only: the bounding box stays where it was
        self.e.pos = [pos[0], min_y + height as f64 * 0.8, pos[2]];
    }

    /// The rest of `onUpdate` on the server, after the angler and caught
    /// entity checks. `targets` are the collidable entities with their
    /// boxes; `lure` is `EnchantmentHelper.getLureModifier(angler)`.
    /// Returns the entity it hit this tick, if any (to be hurt for 0).
    pub fn tick(
        &mut self,
        w: &mut ServerWorld,
        targets: &[(Caught, Aabb)],
        lure: i32,
        fx: &mut Vec<Fx>,
        bursts: &mut Vec<Burst>,
    ) -> Option<Caught> {
        if self.shake > 0 {
            self.shake -= 1;
        }
        if self.in_ground {
            let [x, y, z] = self.tile;
            if Some(w.peek_block(x, y, z)) == self.in_tile {
                self.ticks_in_ground += 1;
                if self.ticks_in_ground == 1200 {
                    self.e.dead = true;
                }
                return None;
            }
            self.in_ground = false;
            let r = &mut self.e.rand;
            self.e.motion[0] *= (r.next_float() * 0.2) as f64;
            self.e.motion[1] *= (r.next_float() * 0.2) as f64;
            self.e.motion[2] *= (r.next_float() * 0.2) as f64;
            self.ticks_in_ground = 0;
            self.ticks_in_air = 0;
        } else {
            self.ticks_in_air += 1;
        }
        let p = self.e.pos;
        let m = self.e.motion;
        let a = p;
        let mut b = [p[0] + m[0], p[1] + m[1], p[2] + m[2]];
        let hit_block = ray_trace_blocks(&*w as &dyn BlockView, a, b, false, false, false);
        if let Some(h) = hit_block {
            b = h.vec;
        }
        let reach = Ent::expand(&self.e.bb.add_coord(m[0], m[1], m[2]), 1.0, 1.0, 1.0);
        let mut best: Option<Caught> = None;
        let mut dist = 0.0;
        for (t, bb) in targets {
            if !bb.intersects(&reach) {
                continue;
            }
            let g = Ent::expand(bb, 0.3f32 as f64, 0.3f32 as f64, 0.3f32 as f64);
            if let Some(v) = calculate_intercept(&g, a, b) {
                let d =
                    ((v[0] - a[0]).powi(2) + (v[1] - a[1]).powi(2) + (v[2] - a[2]).powi(2)).sqrt();
                if d < dist || dist == 0.0 {
                    best = Some(*t);
                    dist = d;
                }
            }
        }
        let mut struck = None;
        if best.is_some() {
            // attackEntityFrom(causeThrownDamage(this, angler), 0): the
            // caller hurts it and sets `caught` when that succeeds
            struck = best;
        } else if hit_block.is_some() {
            // inGround; xTile/yTile/zTile and inTile are never set (-1 and
            // null), so the next tick finds a different block, slows the
            // hook down and lets it move on
            self.in_ground = true;
        }
        if self.in_ground {
            return struck;
        }
        let m = self.e.motion;
        self.e
            .move_entity(&*w as &dyn BlockView, m[0], m[1], m[2], false, &[], fx);
        let e = &mut self.e;
        let d = 180.0 / std::f32::consts::PI as f64;
        let h = math::sqrt_double(e.motion[0] * e.motion[0] + e.motion[2] * e.motion[2]) as f64;
        e.yaw = (e.motion[0].atan2(e.motion[2]) * d) as f32;
        e.pitch = (e.motion[1].atan2(h) * d) as f32;
        while e.pitch - e.prev_pitch < -180.0 {
            e.prev_pitch -= 360.0;
        }
        while e.pitch - e.prev_pitch >= 180.0 {
            e.prev_pitch += 360.0;
        }
        while e.yaw - e.prev_yaw < -180.0 {
            e.prev_yaw -= 360.0;
        }
        while e.yaw - e.prev_yaw >= 180.0 {
            e.prev_yaw += 360.0;
        }
        e.pitch = e.prev_pitch + (e.pitch - e.prev_pitch) * 0.2;
        e.yaw = e.prev_yaw + (e.yaw - e.prev_yaw) * 0.2;
        let mut var33 = 0.92f32;
        if e.on_ground || e.collided_h {
            var33 = 0.5;
        }
        let var34 = 5;
        let mut var35 = 0.0f64;
        for i in 0..var34 {
            let y0 =
                e.bb.min[1] + (e.bb.max[1] - e.bb.min[1]) * i as f64 / var34 as f64 - 0.125 + 0.125;
            let y1 = e.bb.min[1] + (e.bb.max[1] - e.bb.min[1]) * (i + 1) as f64 / var34 as f64
                - 0.125
                + 0.125;
            let slice = Aabb {
                min: [e.bb.min[0], y0, e.bb.min[2]],
                max: [e.bb.max[0], y1, e.bb.max[2]],
            };
            if aabb_in_material(&*w, &slice, Material::Water) {
                var35 += 1.0 / var34 as f64;
            }
        }
        if var35 > 0.0 {
            self.fish(w, lure, fx, bursts);
        }
        let e = &mut self.e;
        let var38 = var35 * 2.0 - 1.0;
        e.motion[1] += 0.04f32 as f64 * var38;
        if var35 > 0.0 {
            var33 = (var33 as f64 * 0.9) as f32;
            e.motion[1] *= 0.8;
        }
        e.motion[0] *= var33 as f64;
        e.motion[1] *= var33 as f64;
        e.motion[2] *= var33 as f64;
        let p = e.pos;
        e.set_position(p[0], p[1], p[2]);
        struck
    }

    /// The `!worldObj.isRemote && var35 > 0` part of `onUpdate`.
    fn fish(&mut self, w: &mut ServerWorld, lure: i32, fx: &mut Vec<Fx>, bursts: &mut Vec<Burst>) {
        let (bx, by, bz) = (
            math::floor_double(self.e.pos[0]),
            math::floor_double(self.e.pos[1]) + 1,
            math::floor_double(self.e.pos[2]),
        );
        let mut var39 = 1;
        if self.e.rand.next_float() < 0.25 && w.can_lightning_strike_at(bx, by, bz) {
            var39 = 2;
        }
        if self.e.rand.next_float() < 0.5 && !w.can_see_sky(bx, by, bz) {
            var39 -= 1;
        }
        let width = self.e.width as f64;
        let floor_y = math::floor_double(self.e.bb.min[1]) as f32;
        if self.ticks_catchable > 0 {
            self.ticks_catchable -= 1;
            if self.ticks_catchable <= 0 {
                self.ticks_caught_delay = 0;
                self.ticks_catchable_delay = 0;
            }
        } else if self.ticks_catchable_delay > 0 {
            self.ticks_catchable_delay -= var39;
            if self.ticks_catchable_delay <= 0 {
                self.e.motion[1] -= 0.2f32 as f64;
                let r = &mut self.e.rand;
                let pitch = 1.0 + (r.next_float() - r.next_float()) * 0.4;
                fx.push(Fx::Sound("random.splash", self.e.pos, 0.25, pitch));
                let y = (floor_y + 1.0) as f64;
                let n = (1.0 + self.e.width * 20.0) as i32;
                for name in ["bubble", "wake"] {
                    bursts.push(Burst {
                        name,
                        pos: [self.e.pos[0], y, self.e.pos[2]],
                        count: n,
                        off: [width, 0.0, width],
                        speed: 0.2f32 as f64,
                    });
                }
                self.ticks_catchable = rand_int_range(&mut self.e.rand, 10, 30);
            } else {
                self.fish_approach_angle =
                    (self.fish_approach_angle as f64 + self.e.rand.next_gaussian() * 4.0) as f32;
                let var40 = self.fish_approach_angle * (std::f32::consts::PI / 180.0);
                let var42 = math::sin(var40);
                let var17 = math::cos(var40);
                let d = self.ticks_catchable_delay as f32 * 0.1;
                let var45 = self.e.pos[0] + (var42 * d) as f64;
                let var20 = (math::floor_double(self.e.bb.min[1]) as f32 + 1.0) as f64;
                let var22 = self.e.pos[2] + (var17 * d) as f64;
                if self.e.rand.next_float() < 0.15 {
                    bursts.push(Burst {
                        name: "bubble",
                        pos: [var45, var20 - 0.1f32 as f64, var22],
                        count: 1,
                        off: [var42 as f64, 0.1, var17 as f64],
                        speed: 0.0,
                    });
                }
                let var24 = var42 * 0.04;
                let var25 = var17 * 0.04;
                bursts.push(Burst {
                    name: "wake",
                    pos: [var45, var20, var22],
                    count: 0,
                    off: [var25 as f64, 0.01, -var24 as f64],
                    speed: 1.0,
                });
                bursts.push(Burst {
                    name: "wake",
                    pos: [var45, var20, var22],
                    count: 0,
                    off: [-var25 as f64, 0.01, var24 as f64],
                    speed: 1.0,
                });
            }
        } else if self.ticks_caught_delay > 0 {
            self.ticks_caught_delay -= var39;
            let mut var41 = 0.15f32;
            if self.ticks_caught_delay < 20 {
                var41 = (var41 as f64 + (20 - self.ticks_caught_delay) as f64 * 0.05) as f32;
            } else if self.ticks_caught_delay < 40 {
                var41 = (var41 as f64 + (40 - self.ticks_caught_delay) as f64 * 0.02) as f32;
            } else if self.ticks_caught_delay < 60 {
                var41 = (var41 as f64 + (60 - self.ticks_caught_delay) as f64 * 0.01) as f32;
            }
            if self.e.rand.next_float() < var41 {
                let r = &mut self.e.rand;
                let var43 = rand_float_clamp(r, 0.0, 360.0) * (std::f32::consts::PI / 180.0);
                let var44 = rand_float_clamp(r, 25.0, 60.0);
                let x = self.e.pos[0] + (math::sin(var43) * var44 * 0.1) as f64;
                let y = (math::floor_double(self.e.bb.min[1]) as f32 + 1.0) as f64;
                let z = self.e.pos[2] + (math::cos(var43) * var44 * 0.1) as f64;
                let n = 2 + r.next_int_n(2);
                bursts.push(Burst {
                    name: "splash",
                    pos: [x, y, z],
                    count: n,
                    off: [0.1f32 as f64, 0.0, 0.1f32 as f64],
                    speed: 0.0,
                });
            }
            if self.ticks_caught_delay <= 0 {
                self.fish_approach_angle = rand_float_clamp(&mut self.e.rand, 0.0, 360.0);
                self.ticks_catchable_delay = rand_int_range(&mut self.e.rand, 20, 80);
            }
        } else {
            self.ticks_caught_delay = rand_int_range(&mut self.e.rand, 100, 900);
            self.ticks_caught_delay -= lure * 20 * 5;
        }
        if self.ticks_catchable > 0 {
            let r = &mut self.e.rand;
            let f = r.next_float() * r.next_float() * r.next_float();
            self.e.motion[1] -= f as f64 * 0.2;
        }
    }

    /// Whether a fish is on the line (`ticksCatchable > 0`).
    pub fn catchable(&self) -> bool {
        self.ticks_catchable > 0
    }

    /// `getFishingResult`: `world_rand` is `worldObj.rand`, the tables
    /// draw from the hook's own random. Returns the stack and which
    /// statistic it counts for (0 junk, 1 treasure, 2 fish).
    pub fn fishing_result(
        &mut self,
        world_rand: &mut JavaRandom,
        luck: i32,
        lure: i32,
    ) -> (Stack, u8) {
        let mut var1 = world_rand.next_float();
        let var4 = math::clamp_float(0.1 - luck as f32 * 0.025 - lure as f32 * 0.01, 0.0, 1.0);
        let var5 = math::clamp_float(0.05 + luck as f32 * 0.01 - lure as f32 * 0.01, 0.0, 1.0);
        let r = &mut self.e.rand;
        if var1 < var4 {
            return (pick(r, JUNK), 0);
        }
        var1 -= var4;
        if var1 < var5 {
            return (pick(r, VALUABLES), 1);
        }
        (pick(r, FISH), 2)
    }
}

/// `WeightedRandomFishable`: item, damage, count, weight, max damage
/// percent, enchantable.
type Fishable = (crate::item::ItemId, i32, u32, i32, f32, bool);

const JUNK: &[Fishable] = &[
    (301, 0, 1, 10, 0.9, false), // leather_boots
    (334, 0, 1, 10, 0.0, false), // leather
    (352, 0, 1, 10, 0.0, false), // bone
    (373, 0, 1, 10, 0.0, false), // potion (water bottle)
    (287, 0, 1, 5, 0.0, false),  // string
    (346, 0, 1, 2, 0.9, false),  // fishing_rod
    (281, 0, 1, 10, 0.0, false), // bowl
    (280, 0, 1, 5, 0.0, false),  // stick
    (351, 0, 10, 1, 0.0, false), // dye: 10 ink sacs
    (131, 0, 1, 10, 0.0, false), // tripwire_hook
    (367, 0, 1, 10, 0.0, false), // rotten_flesh
];

const VALUABLES: &[Fishable] = &[
    (111, 0, 1, 1, 0.0, false), // waterlily
    (421, 0, 1, 1, 0.0, false), // name_tag
    (329, 0, 1, 1, 0.0, false), // saddle
    (261, 0, 1, 1, 0.25, true), // bow
    (346, 0, 1, 1, 0.25, true), // fishing_rod
    (340, 0, 1, 1, 0.0, true),  // book
];

const FISH: &[Fishable] = &[
    (349, 0, 1, 60, 0.0, false), // cod
    (349, 1, 1, 25, 0.0, false), // salmon
    (349, 2, 1, 2, 0.0, false),  // clownfish
    (349, 3, 1, 13, 0.0, false), // pufferfish
];

/// `WeightedRandom.getRandomItem` + `WeightedRandomFishable.getItemStack`
fn pick(r: &mut JavaRandom, table: &[Fishable]) -> Stack {
    let total: i32 = table.iter().map(|f| f.3).sum();
    let mut k = r.next_int_n(total);
    let mut chosen = table[table.len() - 1];
    for f in table {
        k -= f.3;
        if k < 0 {
            chosen = *f;
            break;
        }
    }
    let (id, meta, count, _, pct, ench) = chosen;
    let mut s = Stack::new(id, meta, count);
    if pct > 0.0 {
        let max = crate::item::max_damage(id);
        let var3 = (pct * max as f32) as i32;
        let inner = r.next_int_n(var3) + 1;
        let mut var4 = max - r.next_int_n(inner);
        if var4 > var3 {
            var4 = var3;
        }
        if var4 < 1 {
            var4 = 1;
        }
        s.meta = var4;
    }
    if ench {
        // EnchantmentHelper.addRandomEnchantment(rand, stack, 30)
        let list = crate::enchant::build_list(r, &s, 30);
        if s.id == 340 {
            s = Stack { id: 403, ..s };
        }
        let mut have = s.enchantments();
        have.extend(list);
        s = s.with_enchantments(&have);
    }
    s
}

/// `World.isAABBInMaterial`: the liquid's surface must reach the box.
fn aabb_in_material(w: &dyn BlockView, bb: &Aabb, m: Material) -> bool {
    let x0 = math::floor_double(bb.min[0]);
    let x1 = math::floor_double(bb.max[0] + 1.0);
    let y0 = math::floor_double(bb.min[1]);
    let y1 = math::floor_double(bb.max[1] + 1.0);
    let z0 = math::floor_double(bb.min[2]);
    let z1 = math::floor_double(bb.max[2] + 1.0);
    for x in x0..x1 {
        for y in y0..y1 {
            for z in z0..z1 {
                if block::material(w.peek_block(x, y, z)) == m {
                    let meta = w.peek_meta(x, y, z) as i32;
                    let mut top = (y + 1) as f64;
                    if meta < 8 {
                        top = (y + 1) as f64 - meta as f64 / 8.0;
                    }
                    if top >= bb.min[1] {
                        return true;
                    }
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_follow_weighted_random() {
        // WeightedRandom.getRandomItem: nextInt(total), first whose
        // running weight passes it
        let mut r = JavaRandom::new(1);
        let mut counts = [0; 4];
        for _ in 0..1000 {
            let s = pick(&mut r, FISH);
            counts[s.meta as usize] += 1;
        }
        assert!(counts[0] > counts[1] && counts[1] > counts[3] && counts[3] > counts[2]);
        let mut r = JavaRandom::new(7);
        for _ in 0..200 {
            let s = pick(&mut r, JUNK);
            if s.id == 301 {
                // at most 90% of 65 worn, at least 1
                assert!((1..=58).contains(&s.meta), "{}", s.meta);
            }
            if s.id == 351 {
                assert_eq!(s.count, 10);
            }
        }
    }

    #[test]
    fn books_turn_enchanted() {
        let mut r = JavaRandom::new(3);
        let mut seen = false;
        for _ in 0..300 {
            let s = pick(&mut r, VALUABLES);
            assert_ne!(s.id, 340, "a fished book is always enchanted");
            if s.id == 403 {
                seen = true;
                assert!(!s.enchantments().is_empty());
            }
        }
        assert!(seen);
    }
}
