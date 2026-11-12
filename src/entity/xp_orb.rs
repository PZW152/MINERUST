//! `EntityXPOrb`: its `onUpdate` (both sides run it), the pickup
//! (`onCollideWithPlayer`), `getXPSplit` and `getTextureByXP`.

use super::base::{Ent, Fx};
use super::player::Aabb;
use crate::block::{self, Material};
use crate::java::JavaRandom;
use crate::util::math;
use crate::world::BlockView;

#[derive(Clone, Debug)]
pub struct XpOrb {
    pub id: u32,
    pub e: Ent,
    /// `xpValue`
    pub value: i32,
    /// `xpColor`
    pub color: i32,
    /// `xpOrbAge`
    pub age: i32,
    /// `delayBeforeCanPickup`
    pub pickup_delay: i32,
    /// `xpOrbHealth`
    pub health: i32,
    /// `xpTargetColor`
    target_color: i32,
    /// `closestPlayer` is set
    has_player: bool,
    /// `velocityChanged` (`setBeenAttacked`)
    pub velocity_changed: bool,
}

/// `EntityXPOrb.getXPSplit`
pub fn xp_split(v: i32) -> i32 {
    for s in [2477, 1237, 617, 307, 149, 73, 37, 17, 7, 3] {
        if v >= s {
            return s;
        }
    }
    1
}

/// The orbs `while (xp > 0) { split; spawn }` makes of an amount.
pub fn splits(mut xp: i32) -> Vec<i32> {
    let mut v = Vec::new();
    while xp > 0 {
        let s = xp_split(xp);
        xp -= s;
        v.push(s);
    }
    v
}

/// `getTextureByXP`
pub fn texture_index(v: i32) -> i32 {
    let t = [3, 7, 17, 37, 73, 149, 307, 617, 1237, 2477];
    t.iter().take_while(|&&s| v >= s).count() as i32
}

impl XpOrb {
    /// `new EntityXPOrb(world, x, y, z, value)`; `math_random` stands for
    /// `Math.random()`.
    pub fn new(
        id: u32,
        x: f64,
        y: f64,
        z: f64,
        value: i32,
        math_random: &mut JavaRandom,
        seed: i64,
    ) -> XpOrb {
        let mut e = Ent::new(0.5, 0.5, seed);
        e.y_offset = e.height / 2.0;
        e.can_trigger_walking = false;
        e.set_position(x, y, z);
        e.yaw = (math_random.next_double() * 360.0) as f32;
        e.motion = [
            ((math_random.next_double() * 0.2f32 as f64 - 0.1f32 as f64) as f32 * 2.0) as f64,
            ((math_random.next_double() * 0.2) as f32 * 2.0) as f64,
            ((math_random.next_double() * 0.2f32 as f64 - 0.1f32 as f64) as f32 * 2.0) as f64,
        ];
        e.prev = e.pos;
        e.last_tick = e.pos;
        XpOrb {
            id,
            e,
            value,
            color: 0,
            age: 0,
            pickup_delay: 0,
            health: 5,
            target_color: 0,
            has_player: false,
            velocity_changed: false,
        }
    }

    /// `attackEntityFrom`
    pub fn attack(&mut self, amount: f32) {
        self.velocity_changed = true;
        self.health = (self.health as f32 - amount) as i32;
        if self.health <= 0 {
            self.e.dead = true;
        }
    }

    /// `Entity.setPositionAndRotation2` (the client)
    pub fn set_position_and_rotation2(
        &mut self,
        w: &dyn BlockView,
        p: [f64; 3],
        yaw: f32,
        pitch: f32,
    ) {
        self.e.set_position(p[0], p[1], p[2]);
        self.e.yaw = yaw % 360.0;
        self.e.pitch = pitch % 360.0;
        let probe = Ent::expand(&self.e.bb, -0.03125, 0.0, -0.03125);
        let boxes = super::player::colliding_boxes(w, &probe);
        if !boxes.is_empty() {
            let mut top = 0.0f64;
            for b in &boxes {
                if b.max[1] > top {
                    top = b.max[1];
                }
            }
            let y = p[1] + top - self.e.bb.min[1];
            self.e.set_position(p[0], y, p[2]);
        }
    }

    /// `onUpdate`. `player` is the closest player's `posX/Y/Z` plus its
    /// `getEyeHeight` (`getClosestPlayerToEntity` finds it within 8).
    pub fn tick(&mut self, w: &dyn BlockView, player: Option<[f64; 3]>, fx: &mut Vec<Fx>) {
        // Entity.onEntityUpdate, with handleWaterMovement overridden
        {
            let e = &mut self.e;
            e.prev_dist_walked_mod = e.dist_walked_mod;
            e.prev = e.pos;
            e.prev_pitch = e.pitch;
            e.prev_yaw = e.yaw;
            let bb = e.bb;
            let _ = e.handle_material_acceleration(w, &bb, Material::Water);
            if e.fire > 0 {
                if e.fire % 20 == 0 {
                    fx.push(Fx::Damage(super::base::Source::OnFire, 1.0));
                }
                e.fire -= 1;
            }
            if e.handle_lava_movement(w) {
                fx.push(Fx::Damage(super::base::Source::Lava, 4.0));
                e.set_fire(15);
                e.fall_distance *= 0.5;
            }
            if e.pos[1] < -64.0 {
                e.dead = true;
            }
            e.first_update = false;
        }
        self.take_damage(fx);
        if self.pickup_delay > 0 {
            self.pickup_delay -= 1;
        }
        self.e.prev = self.e.pos;
        self.e.motion[1] -= 0.03f32 as f64;
        let (x, y, z) = (
            math::floor_double(self.e.pos[0]),
            math::floor_double(self.e.pos[1]),
            math::floor_double(self.e.pos[2]),
        );
        if block::material(w.peek_block(x, y, z)) == Material::Lava {
            let r = &mut self.e.rand;
            self.e.motion[1] = 0.2f32 as f64;
            self.e.motion[0] = ((r.next_float() - r.next_float()) * 0.2) as f64;
            self.e.motion[2] = ((r.next_float() - r.next_float()) * 0.2) as f64;
            let p = 2.0 + r.next_float() * 0.4;
            fx.push(Fx::Sound("random.fizz", self.e.pos, 0.4, p));
        }
        let cy = (self.e.bb.min[1] + self.e.bb.max[1]) / 2.0;
        let (px, pz) = (self.e.pos[0], self.e.pos[2]);
        self.e.push_out_of_blocks(w, px, cy, pz);
        let reach = 8.0;
        let own = (self.id & 0x3FFF_FFFF) as i32 % 100;
        if self.target_color < self.color - 20 + own {
            // getClosestPlayerToEntity(this, 8.0)
            self.has_player = player.is_some_and(|p| {
                let d = (p[0] - self.e.pos[0]).powi(2)
                    + (p[1] - self.e.pos[1]).powi(2)
                    + (p[2] - self.e.pos[2]).powi(2);
                d < reach * reach
            });
            self.target_color = self.color;
        }
        if let (true, Some(p)) = (self.has_player, player) {
            let dx = (p[0] - self.e.pos[0]) / reach;
            let dy = (p[1] - self.e.pos[1]) / reach;
            let dz = (p[2] - self.e.pos[2]) / reach;
            let d = (dx * dx + dy * dy + dz * dz).sqrt();
            let mut k = 1.0 - d;
            if k > 0.0 {
                k *= k;
                self.e.motion[0] += dx / d * k * 0.1;
                self.e.motion[1] += dy / d * k * 0.1;
                self.e.motion[2] += dz / d * k * 0.1;
            }
        }
        let m = self.e.motion;
        let mut f = Vec::new();
        self.e.move_entity(w, m[0], m[1], m[2], false, &[], &mut f);
        for x in f {
            match x {
                Fx::Sound(..) | Fx::Particle(..) => fx.push(x),
                Fx::Damage(..) => fx.push(x),
                _ => {}
            }
        }
        self.take_damage(fx);
        let mut drag = 0.98f32;
        if self.e.on_ground {
            let below = w.peek_block(
                math::floor_double(self.e.pos[0]),
                math::floor_double(self.e.bb.min[1]) - 1,
                math::floor_double(self.e.pos[2]),
            );
            drag = block::info(below).slipperiness * 0.98;
        }
        self.e.motion[0] *= drag as f64;
        self.e.motion[1] *= 0.98f32 as f64;
        self.e.motion[2] *= drag as f64;
        if self.e.on_ground {
            self.e.motion[1] *= -0.9f32 as f64;
        }
        self.color += 1;
        self.age += 1;
        if self.age >= 6000 {
            self.e.dead = true;
        }
    }

    /// `dealFireDamage` and the fire/lava damage of `onEntityUpdate`.
    fn take_damage(&mut self, fx: &mut Vec<Fx>) {
        let mut keep = Vec::new();
        for x in fx.drain(..) {
            match x {
                Fx::Damage(_, a) => self.attack(a),
                other => keep.push(other),
            }
        }
        *fx = keep;
    }

    /// The player's box `expand(1.0, 0.5, 1.0)` (`EntityPlayer.onLivingUpdate`)
    /// reaches the orb.
    pub fn touches(&self, player: &Aabb) -> bool {
        let r = Ent::expand(player, 1.0, 0.5, 1.0);
        self.e.bb.intersects(&r)
    }

    /// The `random.orb` pitch of `onCollideWithPlayer`.
    pub fn pickup_pitch(&mut self) -> f32 {
        let r = &mut self.e.rand;
        0.5 * ((r.next_float() - r.next_float()) * 0.7 + 1.8)
    }
}
