//! `EntityThrowable` and its subclasses: `EntitySnowball`, `EntityEgg`,
//! `EntityEnderPearl` and `EntityPotion`.

use super::arrow::{Shooter, Target};
use super::base::{Ent, Fx};
use super::player::Aabb;
use super::raytrace::{calculate_intercept, ray_trace_blocks};
use crate::block;
use crate::util::math;
use crate::world::BlockView;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Snowball,
    Egg,
    EnderPearl,
    /// `EntityPotion` with its potion damage value
    Potion(i32),
}

impl Kind {
    /// `getGravityVelocity`
    fn gravity(self) -> f32 {
        match self {
            Kind::Potion(_) => 0.05,
            _ => 0.03,
        }
    }
    /// `func_70182_d` (velocity) and `func_70183_g` (pitch offset)
    fn velocity(self) -> (f32, f32) {
        match self {
            Kind::Potion(_) => (0.5, -20.0),
            _ => (1.5, 0.0),
        }
    }
    /// The item drawn by `RenderSnowball`.
    pub fn item(self) -> (u16, i32) {
        match self {
            Kind::Snowball => (332, 0),
            Kind::Egg => (344, 0),
            Kind::EnderPearl => (368, 0),
            Kind::Potion(m) => (373, m),
        }
    }
}

/// What the owner must do when a throwable lands.
#[derive(Clone, Debug)]
pub enum Impact {
    /// `attackEntityFrom(causeThrownDamage(this, thrower), amount)`
    Hurt(Target, f32),
    /// `EntityEgg`: chickens (count) hatch here.
    Chickens([f64; 3], f32, i32),
    /// `EntityEnderPearl`: the thrower lands here and takes 5 fall damage.
    Teleport([f64; 3]),
    /// `EntityPotion`: the effects at this position (for `playAuxSFX(2002)`)
    /// and every living entity within 4 blocks with its strength.
    Splash([f64; 3], i32, Vec<(Target, f64)>),
}

#[derive(Clone, Debug)]
pub struct Throwable {
    pub id: u32,
    pub e: Ent,
    pub kind: Kind,
    pub thrower: Shooter,
    ticks_in_air: i32,
    pub shake: i32,
}

impl Throwable {
    /// `new EntityThrowable(world, thrower)` from the thrower's eyes.
    pub fn thrown(
        id: u32,
        kind: Kind,
        thrower: Shooter,
        pos: [f64; 3],
        eye: f64,
        yaw: f32,
        pitch: f32,
    ) -> Throwable {
        let mut e = Ent::new(0.25, 0.25, super::item::entity_seed(id));
        e.can_trigger_walking = false;
        e.set_location_and_angles(pos[0], eye, pos[2], yaw, pitch);
        let r = std::f32::consts::PI;
        e.pos[0] -= (math::cos(e.yaw / 180.0 * r) * 0.16) as f64;
        e.pos[1] -= 0.1f32 as f64;
        e.pos[2] -= (math::sin(e.yaw / 180.0 * r) * 0.16) as f64;
        let p = e.pos;
        e.set_position(p[0], p[1], p[2]);
        e.y_offset = 0.0;
        let k = 0.4f32;
        let (v, off) = kind.velocity();
        let mx = (-math::sin(e.yaw / 180.0 * r) * math::cos(e.pitch / 180.0 * r) * k) as f64;
        let mz = (math::cos(e.yaw / 180.0 * r) * math::cos(e.pitch / 180.0 * r) * k) as f64;
        let my = (-math::sin((e.pitch + off) / 180.0 * r) * k) as f64;
        let mut t = Throwable {
            id,
            e,
            kind,
            thrower,
            ticks_in_air: 0,
            shake: 0,
        };
        t.set_heading(mx, my, mz, v, 1.0);
        t
    }

    /// `EntityThrowable.setThrowableHeading`
    pub fn set_heading(&mut self, mut x: f64, mut y: f64, mut z: f64, v: f32, inacc: f32) {
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
    }

    /// `EntityThrowable.onUpdate`. `living` are the living entities with
    /// their boxes (for the splash); returns the impact, if any.
    pub fn tick(
        &mut self,
        w: &dyn BlockView,
        targets: &[(Target, Aabb)],
        fx: &mut Vec<Fx>,
    ) -> Vec<Impact> {
        self.e.ticks_existed += 1;
        let mut f = Vec::new();
        self.e.base_tick(w, false, &mut f);
        for x in f {
            match x {
                Fx::Kill => self.e.dead = true,
                Fx::Sound(..) | Fx::Particle(..) => fx.push(x),
                _ => {}
            }
        }
        if self.shake > 0 {
            self.shake -= 1;
        }
        // inGround is never set for throwables
        self.ticks_in_air += 1;
        let p = self.e.pos;
        let m = self.e.motion;
        let a = p;
        let mut b = [p[0] + m[0], p[1] + m[1], p[2] + m[2]];
        let mut hit_block = ray_trace_blocks(w, a, b, false, false, false);
        if let Some(h) = hit_block {
            b = h.vec;
        }
        let reach = Ent::expand(&self.e.bb.add_coord(m[0], m[1], m[2]), 1.0, 1.0, 1.0);
        let mut best: Option<Target> = None;
        let mut dist = 0.0;
        for (t, bb) in targets {
            if !bb.intersects(&reach) {
                continue;
            }
            let own = match (t, self.thrower) {
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
        let mut impact = Vec::new();
        if best.is_some() {
            hit_block = None;
        }
        if best.is_some() || hit_block.is_some() {
            let portal = hit_block.is_some_and(|h| w.peek_block(h.x, h.y, h.z) == block::PORTAL);
            if portal {
                self.e.set_in_portal();
            } else {
                impact = self.on_impact(best, targets, fx);
            }
        }
        let e = &mut self.e;
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
        for v in &mut e.motion {
            *v *= drag as f64;
        }
        e.motion[1] -= self.kind.gravity() as f64;
        let p = e.pos;
        e.set_position(p[0], p[1], p[2]);
        impact
    }

    /// `onImpact` of each subclass.
    fn on_impact(
        &mut self,
        hit: Option<Target>,
        targets: &[(Target, Aabb)],
        fx: &mut Vec<Fx>,
    ) -> Vec<Impact> {
        let p = self.e.pos;
        let poof = |fx: &mut Vec<Fx>, name: &'static str, n: usize| {
            for _ in 0..n {
                fx.push(Fx::Particle(name, p, [0.0; 3]));
            }
        };
        self.e.dead = true;
        match self.kind {
            Kind::Snowball => {
                poof(fx, "snowballpoof", 8);
                // blazes take 3 (the owner checks the kind)
                hit.map(|t| Impact::Hurt(t, 0.0)).into_iter().collect()
            }
            Kind::Egg => {
                let mut v: Vec<Impact> = hit.map(|t| Impact::Hurt(t, 0.0)).into_iter().collect();
                if self.e.rand.next_int_n(8) == 0 {
                    let mut n = 1;
                    if self.e.rand.next_int_n(32) == 0 {
                        n = 4;
                    }
                    v.push(Impact::Chickens(p, self.e.yaw, n));
                }
                poof(fx, "snowballpoof", 8);
                v
            }
            Kind::EnderPearl => {
                for _ in 0..32 {
                    let r = &mut self.e.rand;
                    let q = [p[0], p[1] + r.next_double() * 2.0, p[2]];
                    let m = [r.next_gaussian(), 0.0, r.next_gaussian()];
                    fx.push(Fx::Particle("portal", q, m));
                }
                let mut v: Vec<Impact> = hit.map(|t| Impact::Hurt(t, 0.0)).into_iter().collect();
                if self.thrower == Shooter::Player {
                    v.push(Impact::Teleport(p));
                }
                v
            }
            Kind::Potion(meta) => {
                let effects = crate::potion::effects_of(meta, false);
                let mut hits = Vec::new();
                if !effects.is_empty() {
                    let bb = Ent::expand(&self.e.bb, 4.0, 2.0, 4.0);
                    for (t, b) in targets {
                        if !b.intersects(&bb) {
                            continue;
                        }
                        // getDistanceSqToEntity (posY of the target's box)
                        let c = [
                            (b.min[0] + b.max[0]) / 2.0,
                            b.min[1],
                            (b.min[2] + b.max[2]) / 2.0,
                        ];
                        let d =
                            (c[0] - p[0]).powi(2) + (c[1] - p[1]).powi(2) + (c[2] - p[2]).powi(2);
                        if d < 16.0 {
                            let mut k = 1.0 - d.sqrt() / 4.0;
                            if Some(*t) == hit {
                                k = 1.0;
                            }
                            hits.push((*t, k));
                        }
                    }
                }
                vec![Impact::Splash(p, meta, hits)]
            }
        }
    }
}
