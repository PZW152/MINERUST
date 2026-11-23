//! `EntityFireworkRocket`.

use super::base::{Ent, Fx};
use crate::item::Stack;
use crate::util::math;
use crate::world::BlockView;

/// `Items.fireworks`, `Items.firework_charge`
pub const FIREWORKS: u16 = 401;
pub const FIREWORK_CHARGE: u16 = 402;

#[derive(Clone, Debug)]
pub struct Rocket {
    pub id: u32,
    pub e: Ent,
    /// Data watcher 8: the firework item it was made from
    pub item: Option<Stack>,
    /// `fireworkAge`, `lifetime`
    pub age: i32,
    pub lifetime: i32,
    pub velocity_changed: bool,
}

impl Rocket {
    /// `EntityFireworkRocket(World)`: 0.25 by 0.25.
    fn bare(id: u32, seed: i64) -> Rocket {
        Rocket {
            id,
            e: Ent::new(0.25, 0.25, seed),
            item: None,
            age: 0,
            lifetime: 0,
            velocity_changed: false,
        }
    }

    /// `EntityFireworkRocket(World, x, y, z, stack)`: Flight adds ten
    /// ticks each to a random lifetime.
    pub fn new(id: u32, pos: [f64; 3], item: Option<Stack>, seed: i64) -> Rocket {
        let mut r = Rocket::bare(id, seed);
        r.e.set_position(pos[0], pos[1], pos[2]);
        r.e.y_offset = 0.0;
        let mut flight = 1;
        if let Some(s) = item.filter(|s| !s.tag.is_none()) {
            r.item = Some(s);
            if let Some(t) = s.tag.get() {
                if let Some(f) = t.get("Fireworks") {
                    flight += f.int("Flight").unwrap_or(0) as i8 as i32;
                }
            }
        }
        r.e.motion = [
            r.e.rand.next_gaussian() * 0.001,
            0.05,
            r.e.rand.next_gaussian() * 0.001,
        ];
        r.lifetime = 10 * flight + r.e.rand.next_int_n(6) + r.e.rand.next_int_n(7);
        r.e.last_tick = r.e.pos;
        r.e.prev = r.e.pos;
        r
    }

    /// The client's rocket from its spawn packet.
    pub fn at(id: u32, pos: [f64; 3], seed: i64) -> Rocket {
        let mut r = Rocket::bare(id, seed);
        r.e.set_position(pos[0], pos[1], pos[2]);
        r.e.last_tick = pos;
        r.e.prev = pos;
        r
    }

    /// `setVelocity`: the first one also sets the rotation.
    pub fn set_velocity(&mut self, v: [f64; 3]) {
        self.e.motion = v;
        if self.e.prev_pitch == 0.0 && self.e.prev_yaw == 0.0 {
            let h = math::sqrt_double(v[0] * v[0] + v[2] * v[2]) as f64;
            self.e.yaw = (v[0].atan2(v[2]) * 180.0 / std::f64::consts::PI) as f32;
            self.e.prev_yaw = self.e.yaw;
            self.e.pitch = (v[1].atan2(h) * 180.0 / std::f64::consts::PI) as f32;
            self.e.prev_pitch = self.e.pitch;
        }
    }

    /// `onUpdate`. On the server, true when it goes off (`setEntityState
    /// 17`, then dead); on the client the trail of sparks comes back as
    /// `(pos, motion)` in `sparks`.
    pub fn tick(
        &mut self,
        w: &dyn BlockView,
        client: bool,
        fx: &mut Vec<Fx>,
        sparks: &mut Vec<([f64; 3], [f64; 3])>,
    ) -> bool {
        let e = &mut self.e;
        e.last_tick = e.pos;
        // Entity.onUpdate -> onEntityUpdate
        e.prev = e.pos;
        e.prev_yaw = e.yaw;
        e.prev_pitch = e.pitch;
        e.ticks_existed += 1;
        e.motion[0] *= 1.15;
        e.motion[2] *= 1.15;
        e.motion[1] += 0.04;
        let m = e.motion;
        e.move_entity(w, m[0], m[1], m[2], false, &[], fx);
        let h = math::sqrt_double(e.motion[0] * e.motion[0] + e.motion[2] * e.motion[2]) as f64;
        e.yaw = (e.motion[0].atan2(e.motion[2]) * 180.0 / std::f64::consts::PI) as f32;
        e.pitch = (e.motion[1].atan2(h) * 180.0 / std::f64::consts::PI) as f32;
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
        if self.age == 0 && !client {
            // playSoundAtEntity reaches the players from the server
            fx.push(Fx::Sound("fireworks.launch", e.pos, 3.0, 1.0));
        }
        self.age += 1;
        if client {
            let mx = e.rand.next_gaussian() * 0.05;
            let mz = e.rand.next_gaussian() * 0.05;
            sparks.push((
                [e.pos[0], e.pos[1] - 0.3, e.pos[2]],
                [mx, -e.motion[1] * 0.5, mz],
            ));
        }
        if !client && self.age > self.lifetime {
            e.dead = true;
            return true;
        }
        false
    }

    /// `writeEntityToNBT`
    pub fn nbt(&self, v: &mut Vec<(&'static str, crate::world::anvil::Nbt)>) {
        use crate::world::anvil::{item_nbt, Nbt};
        v.push(("Life", Nbt::Int(self.age)));
        v.push(("LifeTime", Nbt::Int(self.lifetime)));
        if let Some(s) = &self.item {
            v.push(("FireworksItem", item_nbt(None, s)));
        }
    }

    /// `readEntityFromNBT` (the position comes from Entity's fields)
    pub fn from_nbt(t: &crate::world::anvil::Nbt, id: u32, seed: i64) -> Rocket {
        let mut r = Rocket::bare(id, seed);
        r.e.y_offset = 0.0;
        r.age = t.int("Life").unwrap_or(0) as i32;
        r.lifetime = t.int("LifeTime").unwrap_or(0) as i32;
        if let Some(it) = t.get("FireworksItem") {
            r.item = crate::world::anvil::item_from_nbt(it);
        }
        r
    }
}
