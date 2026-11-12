//! `EntityArrow`, line by line. The tick is split where vanilla calls
//! `attackEntityFrom` on what the arrow hit: `fly` finds the target, the
//! owner applies the damage, `finish` continues with the result.

use super::base::{Ent, Fx};
use super::player::Aabb;
use super::raytrace::{calculate_intercept, ray_trace_blocks, BlockHit};
use crate::block::{self, Material};
use crate::util::math;
use crate::world::BlockView;

/// `shootingEntity`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shooter {
    None,
    Player,
    Mob(u32),
}

/// What an arrow can hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Player,
    Mob(u32),
}

/// A hit waiting for `attackEntityFrom`.
#[derive(Clone, Copy, Debug)]
pub struct PendingHit {
    pub target: Target,
    pub damage: i32,
    /// `isBurning()`: the target is set on fire for 5 seconds first.
    pub burning: bool,
}

#[derive(Clone, Debug)]
pub struct EntityArrow {
    pub id: u32,
    pub e: Ent,
    pub tile: [i32; 3],
    pub in_tile: u8,
    pub in_data: u8,
    pub in_ground: bool,
    pub ticks_in_ground: i32,
    pub ticks_in_air: i32,
    pub arrow_shake: i32,
    /// `canBePickedUp`: 0 no, 1 yes, 2 creative only
    pub pickup: i32,
    pub damage: f64,
    pub knockback: i32,
    pub crit: bool,
    pub shooter: Shooter,
    /// the block hit this tick, kept between `fly` and `finish`
    pending_block: Option<BlockHit>,
    pending_target: bool,
}

impl EntityArrow {
    fn blank(id: u32) -> EntityArrow {
        let mut e = Ent::new(0.5, 0.5, super::item::entity_seed(id));
        e.can_trigger_walking = false;
        EntityArrow {
            id,
            e,
            tile: [-1, -1, -1],
            in_tile: 0,
            in_data: 0,
            in_ground: false,
            ticks_in_ground: 0,
            ticks_in_air: 0,
            arrow_shake: 0,
            pickup: 0,
            damage: 2.0,
            knockback: 0,
            crit: false,
            shooter: Shooter::None,
            pending_block: None,
            pending_target: false,
        }
    }

    /// `new EntityArrow(world, x, y, z)` (dispensers): pickup allowed.
    pub fn at_pos(id: u32, pos: [f64; 3]) -> EntityArrow {
        let mut a = Self::blank(id);
        a.e.set_position(pos[0], pos[1], pos[2]);
        a.e.y_offset = 0.0;
        a
    }

    /// `new EntityArrow(world, shooter, velocity)`: from the shooter's eyes
    /// along its look (the player's bow). `eye` is posY + getEyeHeight.
    pub fn from_look(
        id: u32,
        shooter: Shooter,
        pos: [f64; 3],
        eye: f64,
        yaw: f32,
        pitch: f32,
        v: f32,
    ) -> EntityArrow {
        let mut a = Self::blank(id);
        a.shooter = shooter;
        if shooter == Shooter::Player {
            a.pickup = 1;
        }
        let e = &mut a.e;
        e.set_location_and_angles(pos[0], eye, pos[2], yaw, pitch);
        let r = std::f32::consts::PI;
        e.pos[0] -= (math::cos(e.yaw / 180.0 * r) * 0.16) as f64;
        e.pos[1] -= 0.1f32 as f64;
        e.pos[2] -= (math::sin(e.yaw / 180.0 * r) * 0.16) as f64;
        let p = e.pos;
        e.set_position(p[0], p[1], p[2]);
        e.y_offset = 0.0;
        let mx = (-math::sin(e.yaw / 180.0 * r) * math::cos(e.pitch / 180.0 * r)) as f64;
        let mz = (math::cos(e.yaw / 180.0 * r) * math::cos(e.pitch / 180.0 * r)) as f64;
        let my = (-math::sin(e.pitch / 180.0 * r)) as f64;
        a.set_heading(mx, my, mz, v * 1.5, 1.0);
        a
    }

    /// `new EntityArrow(world, shooter, target, velocity, inaccuracy)`:
    /// a mob shooting at a target.
    pub fn at_target(
        id: u32,
        shooter: Shooter,
        from: [f64; 3],
        eye_y: f64,
        target: [f64; 3],
        target_min_y: f64,
        target_height: f32,
        v: f32,
        inacc: f32,
    ) -> EntityArrow {
        let mut a = Self::blank(id);
        a.shooter = shooter;
        if shooter == Shooter::Player {
            a.pickup = 1;
        }
        a.e.pos[1] = eye_y - 0.1f32 as f64;
        let dx = target[0] - from[0];
        let dy = target_min_y + (target_height / 3.0) as f64 - a.e.pos[1];
        let dz = target[2] - from[2];
        let h = math::sqrt_double(dx * dx + dz * dz) as f64;
        if h >= 1.0e-7 {
            let yaw = (dz.atan2(dx) * 180.0 / std::f32::consts::PI as f64) as f32 - 90.0;
            let pitch = (-(dy.atan2(h) * 180.0 / std::f32::consts::PI as f64)) as f32;
            let (ox, oz) = (dx / h, dz / h);
            let y = a.e.pos[1];
            a.e.set_location_and_angles(from[0] + ox, y, from[2] + oz, yaw, pitch);
            a.e.y_offset = 0.0;
            let lift = (h as f32 * 0.2) as f64;
            a.set_heading(dx, dy + lift, dz, v, inacc);
        }
        a
    }

    /// `setThrowableHeading`
    pub fn set_heading(&mut self, mut x: f64, mut y: f64, mut z: f64, v: f32, inacc: f32) {
        let l = math::sqrt_double(x * x + y * y + z * z) as f64;
        x /= l;
        y /= l;
        z /= l;
        let r = &mut self.e.rand;
        let k = 0.0075f32 as f64 * inacc as f64;
        x += r.next_gaussian() * (if r.next_boolean() { -1.0 } else { 1.0 }) * k;
        y += r.next_gaussian() * (if r.next_boolean() { -1.0 } else { 1.0 }) * k;
        z += r.next_gaussian() * (if r.next_boolean() { -1.0 } else { 1.0 }) * k;
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

    /// `EntityArrow.onUpdate` up to `attackEntityFrom`. `targets` are the
    /// entities that can be collided with (`canBeCollidedWith`) with their
    /// boxes. Returns the entity hit, if any.
    pub fn fly(
        &mut self,
        w: &dyn BlockView,
        targets: &[(Target, Aabb)],
        fx: &mut Vec<Fx>,
    ) -> Option<PendingHit> {
        self.pending_block = None;
        self.pending_target = false;
        self.e.ticks_existed += 1;
        // Entity.onUpdate -> onEntityUpdate
        let mut f = Vec::new();
        self.e.base_tick(w, false, &mut f);
        for x in f {
            match x {
                Fx::Kill => self.e.dead = true,
                Fx::Sound(..) | Fx::Particle(..) => fx.push(x),
                _ => {}
            }
        }
        let e = &mut self.e;
        let d = 180.0 / std::f32::consts::PI as f64;
        if e.prev_pitch == 0.0 && e.prev_yaw == 0.0 {
            let h = math::sqrt_double(e.motion[0] * e.motion[0] + e.motion[2] * e.motion[2]) as f64;
            e.yaw = (e.motion[0].atan2(e.motion[2]) * d) as f32;
            e.prev_yaw = e.yaw;
            e.pitch = (e.motion[1].atan2(h) * d) as f32;
            e.prev_pitch = e.pitch;
        }
        let [tx, ty, tz] = self.tile;
        let here = w.peek_block(tx, ty, tz);
        if block::material(here) != Material::Air {
            let get = |x: i32, y: i32, z: i32| (w.peek_block(x, y, z), w.peek_meta(x, y, z));
            let mut boxes = Vec::new();
            crate::shapes::collision(&get, here, w.peek_meta(tx, ty, tz), tx, ty, tz, &mut boxes);
            if let Some(b) = union(&boxes) {
                let bb = Aabb {
                    min: [
                        tx as f64 + b[0] as f64,
                        ty as f64 + b[1] as f64,
                        tz as f64 + b[2] as f64,
                    ],
                    max: [
                        tx as f64 + b[3] as f64,
                        ty as f64 + b[4] as f64,
                        tz as f64 + b[5] as f64,
                    ],
                };
                // AxisAlignedBB.isVecInside: strict on every axis
                let p = self.e.pos;
                if (0..3).all(|i| p[i] > bb.min[i] && p[i] < bb.max[i]) {
                    self.in_ground = true;
                }
            }
        }
        if self.arrow_shake > 0 {
            self.arrow_shake -= 1;
        }
        if self.in_ground {
            let m = w.peek_meta(tx, ty, tz);
            if here == self.in_tile && m == self.in_data {
                self.ticks_in_ground += 1;
                if self.ticks_in_ground == 1200 {
                    self.e.dead = true;
                }
            } else {
                self.in_ground = false;
                let r = &mut self.e.rand;
                self.e.motion[0] *= (r.next_float() * 0.2) as f64;
                self.e.motion[1] *= (r.next_float() * 0.2) as f64;
                self.e.motion[2] *= (r.next_float() * 0.2) as f64;
                self.ticks_in_ground = 0;
                self.ticks_in_air = 0;
            }
            return None;
        }
        self.ticks_in_air += 1;
        let p = self.e.pos;
        let m = self.e.motion;
        let a = p;
        let mut b = [p[0] + m[0], p[1] + m[1], p[2] + m[2]];
        let hit_block = ray_trace_blocks(w, a, b, false, true, false);
        if let Some(h) = hit_block {
            b = h.vec;
        }
        // getEntitiesWithinAABBExcludingEntity(this, bb.addCoord(motion).expand(1))
        let reach = Ent::expand(&self.e.bb.add_coord(m[0], m[1], m[2]), 1.0, 1.0, 1.0);
        let mut best: Option<Target> = None;
        let mut dist = 0.0;
        for (t, bb) in targets {
            if !bb.intersects(&reach) {
                continue;
            }
            let own = match (t, self.shooter) {
                (Target::Player, Shooter::Player) => true,
                (Target::Mob(i), Shooter::Mob(j)) => *i == j,
                _ => false,
            };
            if own && self.ticks_in_air < 5 {
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
        if let Some(t) = best {
            let mo = self.e.motion;
            let v = math::sqrt_double(mo[0] * mo[0] + mo[1] * mo[1] + mo[2] * mo[2]);
            let mut dmg = (v as f64 * self.damage).ceil() as i32;
            if self.crit {
                dmg += self.e.rand.next_int_n(dmg / 2 + 2);
            }
            self.pending_target = true;
            return Some(PendingHit {
                target: t,
                damage: dmg,
                burning: self.e.fire > 0,
            });
        }
        self.pending_block = hit_block;
        None
    }

    /// The rest of `onUpdate`. `hit` is `Some(attackEntityFrom(...))` when
    /// `fly` returned an entity; `enderman` that the target was one.
    pub fn finish(
        &mut self,
        w: &dyn BlockView,
        hit: Option<bool>,
        enderman: bool,
        fx: &mut Vec<Fx>,
    ) -> Option<[f64; 3]> {
        if self.in_ground {
            return None;
        }
        let mut push = None;
        match hit {
            Some(true) => {
                if self.knockback > 0 {
                    let m = self.e.motion;
                    let h = math::sqrt_double(m[0] * m[0] + m[2] * m[2]);
                    if h > 0.0 {
                        let k = self.knockback as f32 * 0.6;
                        push = Some([m[0] * k as f64 / h as f64, 0.1, m[2] * k as f64 / h as f64]);
                    }
                }
                let p = 1.2 / (self.e.rand.next_float() * 0.2 + 0.9);
                fx.push(Fx::Sound("random.bowhit", self.e.pos, 1.0, p));
                if !enderman {
                    self.e.dead = true;
                }
            }
            Some(false) => {
                for v in &mut self.e.motion {
                    *v *= -0.1f32 as f64;
                }
                self.e.yaw += 180.0;
                self.e.prev_yaw += 180.0;
                self.ticks_in_air = 0;
            }
            None => {
                if let Some(h) = self.pending_block {
                    self.tile = [h.x, h.y, h.z];
                    self.in_tile = w.peek_block(h.x, h.y, h.z);
                    self.in_data = w.peek_meta(h.x, h.y, h.z);
                    let p = self.e.pos;
                    self.e.motion = [
                        (h.vec[0] - p[0]) as f32 as f64,
                        (h.vec[1] - p[1]) as f32 as f64,
                        (h.vec[2] - p[2]) as f32 as f64,
                    ];
                    let m = self.e.motion;
                    let l = math::sqrt_double(m[0] * m[0] + m[1] * m[1] + m[2] * m[2]) as f64;
                    let k = 0.05f32 as f64;
                    self.e.pos[0] -= m[0] / l * k;
                    self.e.pos[1] -= m[1] / l * k;
                    self.e.pos[2] -= m[2] / l * k;
                    let pi = 1.2 / (self.e.rand.next_float() * 0.2 + 0.9);
                    fx.push(Fx::Sound("random.bowhit", self.e.pos, 1.0, pi));
                    self.in_ground = true;
                    self.arrow_shake = 7;
                    self.crit = false;
                    if self.in_tile != block::AIR {
                        fx.push(Fx::Collide(h.x, h.y, h.z, self.in_tile));
                    }
                }
            }
        }
        let e = &mut self.e;
        if self.crit {
            for i in 0..4 {
                let k = i as f64 / 4.0;
                fx.push(Fx::Particle(
                    "crit",
                    [
                        e.pos[0] + e.motion[0] * k,
                        e.pos[1] + e.motion[1] * k,
                        e.pos[2] + e.motion[2] * k,
                    ],
                    [-e.motion[0], -e.motion[1] + 0.2, -e.motion[2]],
                ));
            }
        }
        for i in 0..3 {
            e.pos[i] += e.motion[i];
        }
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
        let mut drag = 0.99f32;
        if e.in_water {
            for _ in 0..4 {
                let k = 0.25f32 as f64;
                fx.push(Fx::Particle(
                    "bubble",
                    [
                        e.pos[0] - e.motion[0] * k,
                        e.pos[1] - e.motion[1] * k,
                        e.pos[2] - e.motion[2] * k,
                    ],
                    e.motion,
                ));
            }
            drag = 0.8;
        }
        if e.is_wet(w) {
            e.fire = 0;
        }
        for v in &mut e.motion {
            *v *= drag as f64;
        }
        e.motion[1] -= 0.05f32 as f64;
        let p = e.pos;
        e.set_position(p[0], p[1], p[2]);
        e.do_block_collisions(w, fx);
        push
    }
}

fn union(boxes: &[[f32; 6]]) -> Option<[f32; 6]> {
    let mut u = *boxes.first()?;
    for b in &boxes[1..] {
        for a in 0..3 {
            u[a] = u[a].min(b[a]);
            u[a + 3] = u[a + 3].max(b[a + 3]);
        }
    }
    Some(u)
}
