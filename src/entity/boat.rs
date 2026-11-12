//! `EntityBoat`: the physics both sides run (`onUpdate`), the rider's seat
//! (`updateRiderPosition`), `attackEntityFrom`, `updateFallState` and the
//! client's `setPositionAndRotation2` / `setVelocity`.

use super::base::{Ent, Fx};
use super::player::Aabb;
use crate::block::{self, Material};
use crate::util::math;
use crate::world::BlockView;

/// The world edits `EntityBoat.onUpdate` makes on the side it runs on.
pub trait BoatWorld: BlockView {
    /// `World.setBlockToAir` (a snow layer the boat runs into)
    fn set_air(&mut self, x: i32, y: i32, z: i32);
    /// `World.breakBlock(x, y, z, true)` (`func_147480_a`, a lily pad)
    fn break_block(&mut self, x: i32, y: i32, z: i32);
    /// `Block.onEntityCollidedWithBlock`; the damage it deals, if any.
    fn collided(&mut self, e: &mut Ent, x: i32, y: i32, z: i32, id: u8) -> Option<f32>;
}

/// What `riddenByEntity` (an `EntityLivingBase`) gives the boat.
#[derive(Clone, Copy, Debug, Default)]
pub struct Rider {
    pub yaw: f32,
    /// `moveStrafing`
    pub strafe: f32,
    /// `moveForward`
    pub forward: f32,
}

/// What the owner must do after `onUpdate`.
#[derive(Clone, Debug, PartialEq)]
pub enum Out {
    /// `setDead` with three planks and two sticks
    /// (`dropItemWithOffset(..., 1, 0.0F)`).
    Wreck,
    /// `Entity.fall`: the rider falls this far.
    Fall(f32),
    /// `attackEntityFrom` by the world (fire, lava, cactus) broke it:
    /// `dropItemWithOffset(Items.boat, 1, 0.0F)`.
    Broken,
}

#[derive(Clone, Debug)]
pub struct Boat {
    pub id: u32,
    pub e: Ent,
    /// `isBoatEmpty`: on the client, true unless the local player rides it.
    pub empty: bool,
    /// `speedMultiplier`
    speed: f64,
    /// `boatPosRotationIncrements`
    inc: i32,
    target: [f64; 3],
    target_yaw: f64,
    target_pitch: f64,
    velocity: [f64; 3],
    /// Data watcher 17 (`getTimeSinceHit`)
    pub time_since_hit: i32,
    /// Data watcher 18 (`getForwardDirection`)
    pub forward: i32,
    /// Data watcher 19 (`getDamageTaken`)
    pub damage: f32,
    /// `riddenByEntity != null`
    pub ridden: bool,
    /// `velocityChanged` (`setBeenAttacked`), for the tracker.
    pub velocity_changed: bool,
}

pub const WIDTH: f32 = 1.5;
pub const HEIGHT: f32 = 0.6;

impl Boat {
    /// `new EntityBoat(world)`
    pub fn new(id: u32, seed: i64) -> Boat {
        let mut e = Ent::new(WIDTH, HEIGHT, seed);
        e.y_offset = e.height / 2.0;
        e.can_trigger_walking = false;
        e.solid_id = Some(id);
        e.set_position(0.0, 0.0, 0.0);
        Boat {
            id,
            e,
            empty: true,
            speed: 0.07,
            inc: 0,
            target: [0.0; 3],
            target_yaw: 0.0,
            target_pitch: 0.0,
            velocity: [0.0; 3],
            time_since_hit: 0,
            forward: 1,
            damage: 0.0,
            ridden: false,
            velocity_changed: false,
        }
    }

    /// `new EntityBoat(world, x, y, z)`
    pub fn at(id: u32, x: f64, y: f64, z: f64, seed: i64) -> Boat {
        let mut b = Boat::new(id, seed);
        b.e.set_position(x, y + b.e.y_offset as f64, z);
        b.e.motion = [0.0; 3];
        b.e.prev = [x, y, z];
        b
    }

    /// `getMountedYOffset`
    pub fn mounted_y_offset(&self) -> f64 {
        self.e.height as f64 * 0.0 - 0.3f32 as f64
    }

    /// `updateRiderPosition`: where the rider is put, given its
    /// `getYOffset()` (`yOffset - 0.5` for players).
    pub fn rider_position(&self, rider_y_offset: f64) -> [f64; 3] {
        let a = self.e.yaw as f64 * std::f64::consts::PI / 180.0;
        let dx = a.cos() * 0.4;
        let dz = a.sin() * 0.4;
        [
            self.e.pos[0] + dx,
            self.e.pos[1] + self.mounted_y_offset() + rider_y_offset,
            self.e.pos[2] + dz,
        ]
    }

    /// `getBoundingBox`: other entities collide with it.
    pub fn bounding_box(&self) -> Aabb {
        self.e.bb
    }

    /// `attackEntityFrom` on the server. `creative` is a creative player's
    /// hit. Returns whether the boat broke and whether it drops itself.
    pub fn attack(&mut self, amount: f32, creative: bool) -> Option<bool> {
        if self.e.dead {
            return None;
        }
        self.forward = -self.forward;
        self.time_since_hit = 10;
        self.damage += amount * 10.0;
        self.velocity_changed = true;
        if creative || self.damage > 40.0 {
            self.e.dead = true;
            return Some(!creative);
        }
        None
    }

    /// `setPositionAndRotation2` (the client, from `S14`/`S18`).
    pub fn set_position_and_rotation2(&mut self, p: [f64; 3], yaw: f32, pitch: f32, inc: i32) {
        if self.empty {
            self.inc = inc + 5;
        } else {
            let d = [
                p[0] - self.e.pos[0],
                p[1] - self.e.pos[1],
                p[2] - self.e.pos[2],
            ];
            let d2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
            if d2 <= 1.0 {
                return;
            }
            self.inc = 3;
        }
        self.target = p;
        self.target_yaw = yaw as f64;
        self.target_pitch = pitch as f64;
        self.e.motion = self.velocity;
    }

    /// `setVelocity` (the client, from `S12`).
    pub fn set_velocity(&mut self, v: [f64; 3]) {
        self.velocity = v;
        self.e.motion = v;
    }

    /// `onUpdate`. `remote` is the client's copy; `rider` the entity in
    /// it; `others` the boxes of every other entity near
    /// (`getCollisionBox` returns the other entity's box).
    pub fn tick<W: BoatWorld>(
        &mut self,
        w: &mut W,
        remote: bool,
        rider: Option<Rider>,
        others: &[Aabb],
        fx: &mut Vec<Fx>,
    ) -> Vec<Out> {
        let mut out = Vec::new();
        self.ridden = rider.is_some();
        // Entity.onUpdate -> onEntityUpdate
        {
            let mut f = Vec::new();
            self.e.base_tick(&*w, false, &mut f);
            for x in f {
                match x {
                    Fx::Kill => self.e.dead = true,
                    Fx::Damage(_, a) => self.hurt(remote, a, &mut out),
                    Fx::Sound(..) | Fx::Particle(..) => fx.push(x),
                    _ => {}
                }
            }
        }
        if self.time_since_hit > 0 {
            self.time_since_hit -= 1;
        }
        if self.damage > 0.0 {
            self.damage -= 1.0;
        }
        self.e.prev = self.e.pos;
        let n = 5;
        let mut water = 0.0f64;
        for i in 0..n {
            let bb = self.e.bb;
            let y0 = bb.min[1] + (bb.max[1] - bb.min[1]) * (i + 0) as f64 / n as f64 - 0.125;
            let y1 = bb.min[1] + (bb.max[1] - bb.min[1]) * (i + 1) as f64 / n as f64 - 0.125;
            let b = Aabb {
                min: [bb.min[0], y0, bb.min[2]],
                max: [bb.max[0], y1, bb.max[2]],
            };
            if super::base::material_in_bb(&*w, &b, Material::Water) {
                water += 1.0 / n as f64;
            }
        }
        let speed0 =
            (self.e.motion[0] * self.e.motion[0] + self.e.motion[2] * self.e.motion[2]).sqrt();
        if speed0 > 0.26249999999999996 {
            let a = self.e.yaw as f64 * std::f64::consts::PI / 180.0;
            let (c, s) = (a.cos(), a.sin());
            let mut k = 0;
            while (k as f64) < 1.0 + speed0 * 60.0 {
                let r = &mut self.e.rand;
                let u = (r.next_float() * 2.0 - 1.0) as f64;
                let v = (r.next_int_n(2) * 2 - 1) as f64 * 0.7;
                let p = if r.next_boolean() {
                    [
                        self.e.pos[0] - c * u * 0.8 + s * v,
                        self.e.pos[1] - 0.125,
                        self.e.pos[2] - s * u * 0.8 - c * v,
                    ]
                } else {
                    [
                        self.e.pos[0] + c + s * u * 0.7,
                        self.e.pos[1] - 0.125,
                        self.e.pos[2] + s - c * u * 0.7,
                    ]
                };
                fx.push(Fx::Particle("splash", p, self.e.motion));
                k += 1;
            }
        }
        if remote && self.empty {
            if self.inc > 0 {
                let i = self.inc as f64;
                let x = self.e.pos[0] + (self.target[0] - self.e.pos[0]) / i;
                let y = self.e.pos[1] + (self.target[1] - self.e.pos[1]) / i;
                let z = self.e.pos[2] + (self.target[2] - self.e.pos[2]) / i;
                let dy = math::wrap_angle_to_180_double(self.target_yaw - self.e.yaw as f64);
                self.e.yaw = (self.e.yaw as f64 + dy / i) as f32;
                self.e.pitch =
                    (self.e.pitch as f64 + (self.target_pitch - self.e.pitch as f64) / i) as f32;
                self.inc -= 1;
                self.e.set_position(x, y, z);
                self.set_rotation(self.e.yaw, self.e.pitch);
            } else {
                let p = [
                    self.e.pos[0] + self.e.motion[0],
                    self.e.pos[1] + self.e.motion[1],
                    self.e.pos[2] + self.e.motion[2],
                ];
                self.e.set_position(p[0], p[1], p[2]);
                if self.e.on_ground {
                    for v in &mut self.e.motion {
                        *v *= 0.5;
                    }
                }
                self.e.motion[0] *= 0.99f32 as f64;
                self.e.motion[1] *= 0.95f32 as f64;
                self.e.motion[2] *= 0.99f32 as f64;
            }
            return out;
        }
        if water < 1.0 {
            let d = water * 2.0 - 1.0;
            self.e.motion[1] += 0.04f32 as f64 * d;
        } else {
            if self.e.motion[1] < 0.0 {
                self.e.motion[1] /= 2.0;
            }
            self.e.motion[1] += 0.007f32 as f64;
        }
        if let Some(r) = rider {
            let f = r.yaw + -r.strafe * 90.0;
            let a = f * std::f32::consts::PI / 180.0;
            self.e.motion[0] += -(a as f64).sin() * self.speed * r.forward as f64 * 0.05f32 as f64;
            self.e.motion[2] += (a as f64).cos() * self.speed * r.forward as f64 * 0.05f32 as f64;
        }
        let mut speed =
            (self.e.motion[0] * self.e.motion[0] + self.e.motion[2] * self.e.motion[2]).sqrt();
        if speed > 0.35 {
            let k = 0.35 / speed;
            self.e.motion[0] *= k;
            self.e.motion[2] *= k;
            speed = 0.35;
        }
        if speed > speed0 && self.speed < 0.35 {
            self.speed += (0.35 - self.speed) / 35.0;
            if self.speed > 0.35 {
                self.speed = 0.35;
            }
        } else {
            self.speed -= (self.speed - 0.07) / 35.0;
            if self.speed < 0.07 {
                self.speed = 0.07;
            }
        }
        for i in 0..4 {
            let x = math::floor_double(self.e.pos[0] + ((i % 2) as f64 - 0.5) * 0.8);
            let z = math::floor_double(self.e.pos[2] + ((i / 2) as f64 - 0.5) * 0.8);
            for j in 0..2 {
                let y = math::floor_double(self.e.pos[1]) + j;
                let b = w.peek_block(x, y, z);
                if b == block::SNOW_LAYER {
                    w.set_air(x, y, z);
                    self.e.collided_h = false;
                } else if b == block::WATERLILY {
                    w.break_block(x, y, z);
                    self.e.collided_h = false;
                }
            }
        }
        if self.e.on_ground {
            for v in &mut self.e.motion {
                *v *= 0.5;
            }
        }
        {
            let m = self.e.motion;
            let mut f = Vec::new();
            let (y0, fall0) = (self.e.pos[1], self.e.fall_distance);
            self.e
                .move_entity(&*w, m[0], m[1], m[2], false, others, &mut f);
            // EntityBoat.updateFallState replaces Entity's: undo the move's
            // own bookkeeping (it ran before the block collisions) and redo.
            let dy = self.e.pos[1] - y0;
            self.e.fall_distance = fall0;
            if self.e.on_ground {
                if self.e.fall_distance > 3.0 {
                    out.push(Out::Fall(self.e.fall_distance));
                    if !remote && !self.e.dead {
                        self.e.dead = true;
                        out.push(Out::Wreck);
                    }
                    self.e.fall_distance = 0.0;
                }
            } else {
                let below = w.peek_block(
                    math::floor_double(self.e.pos[0]),
                    math::floor_double(self.e.pos[1]) - 1,
                    math::floor_double(self.e.pos[2]),
                );
                if block::material(below) != Material::Water && dy < 0.0 {
                    self.e.fall_distance = (self.e.fall_distance as f64 - dy) as f32;
                }
            }
            for x in f {
                match x {
                    Fx::Collide(bx, by, bz, id) => {
                        if let Some(a) = w.collided(&mut self.e, bx, by, bz, id) {
                            self.hurt(remote, a, &mut out);
                        }
                    }
                    Fx::Damage(_, a) => self.hurt(remote, a, &mut out),
                    Fx::Sound(..) | Fx::Particle(..) => fx.push(x),
                    _ => {}
                }
            }
        }
        if !self.e.collided_h || speed0 <= 0.2 {
            self.e.motion[0] *= 0.99f32 as f64;
            self.e.motion[1] *= 0.95f32 as f64;
            self.e.motion[2] *= 0.99f32 as f64;
        } else if !remote && !self.e.dead {
            self.e.dead = true;
            out.push(Out::Wreck);
        }
        self.e.pitch = 0.0;
        let mut yaw = self.e.yaw as f64;
        let dx = self.e.prev[0] - self.e.pos[0];
        let dz = self.e.prev[2] - self.e.pos[2];
        if dx * dx + dz * dz > 0.001 {
            yaw = (dz.atan2(dx) * 180.0 / std::f64::consts::PI) as f32 as f64;
        }
        let mut d = math::wrap_angle_to_180_double(yaw - self.e.yaw as f64);
        d = d.clamp(-20.0, 20.0);
        self.e.yaw = (self.e.yaw as f64 + d) as f32;
        self.set_rotation(self.e.yaw, self.e.pitch);
        out
    }

    /// `attackEntityFrom` with a world damage source.
    fn hurt(&mut self, remote: bool, amount: f32, out: &mut Vec<Out>) {
        if !remote && self.attack(amount, false).is_some() {
            out.push(Out::Broken);
        }
    }

    /// `Entity.setRotation`
    fn set_rotation(&mut self, yaw: f32, pitch: f32) {
        self.e.yaw = yaw % 360.0;
        self.e.pitch = pitch % 360.0;
    }

    /// `Entity.applyEntityCollision` on this boat by `other` (its feet
    /// position): the velocity added to each.
    pub fn collision_push(&self, other: [f64; 3]) -> Option<([f64; 3], [f64; 3])> {
        super::base::collision_push(self.e.pos, self.e.entity_collision_reduction, other)
    }
}

impl BoatWorld for crate::world::ServerWorld {
    fn set_air(&mut self, x: i32, y: i32, z: i32) {
        self.set_block(x, y, z, block::AIR, 0, 3);
    }
    fn break_block(&mut self, x: i32, y: i32, z: i32) {
        // World.func_147480_a(x, y, z, true)
        let b = self.block(x, y, z);
        if block::material(b) == Material::Air {
            return;
        }
        let m = self.meta(x, y, z);
        self.destroyed.push(([x, y, z], b, m));
        self.drop_block_as_item(b, m, x, y, z);
        self.set_block(x, y, z, block::AIR, 0, 3);
    }
    fn collided(&mut self, e: &mut Ent, x: i32, y: i32, z: i32, id: u8) -> Option<f32> {
        super::collide::collided(self, e, x, y, z, id).map(|(_, a)| a)
    }
}
