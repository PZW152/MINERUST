//! `EffectRenderer` and the `EntityFX` particles of 1.7.10: block break and
//! hit debris, smoke, flame, lava pops, rain splashes, explosions, crits,
//! drips and running dust. Client-side only, as in the original.

use crate::block;
use crate::entity::player::{colliding_boxes, Aabb};
use crate::java::JavaRandom;
use crate::render::mesher::Vertex;
use crate::render::model::Tex;
use crate::util::math;
use crate::world::BlockView;

#[derive(Clone, Copy, PartialEq)]
pub enum Sprite {
    /// Index into `particles.png` (16 x 16 grid of 8 px cells).
    Index(i32),
    /// A quarter of a block face texture (`EntityDiggingFX`).
    Block { rect: [f32; 4], jitter: [f32; 2] },
    /// `EntityLargeExplodeFX`: frame of `explosion.png` (4 x 4).
    Explosion,
}

#[derive(Clone)]
pub struct Particle {
    pos: [f64; 3],
    prev: [f64; 3],
    motion: [f64; 3],
    on_ground: bool,
    age: i32,
    max_age: i32,
    gravity: f32,
    scale: f32,
    color: [f32; 4],
    sprite: Sprite,
    kind: Kind,
    no_clip: bool,
    base_scale: f32,
    flame_base: f32,
    /// `EntityPortalFX.portalPosX/Y/Z`
    origin: [f64; 3],
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Plain,
    Smoke,
    Flame,
    Lava,
    Explode,
    Large,
    Rain,
    Drip,
    Crit,
    Dust,
    Note,
    Portal,
    Heart,
    Bubble,
    Reddust,
    /// `EntitySpellParticleFX` with its base texture index
    Spell(i32),
}

pub struct Particles {
    list: Vec<Particle>,
    r: JavaRandom,
    /// `EntityHugeExplodeFX` emitters: position and remaining ticks.
    huge: Vec<([f64; 3], i32)>,
}

impl Default for Particles {
    fn default() -> Self {
        Particles {
            list: Vec::new(),
            r: JavaRandom::new(0x5041525449),
            huge: Vec::new(),
        }
    }
}

impl Particles {
    fn rnd(&mut self) -> f64 {
        self.r.next_double()
    }

    /// `EntityFX(world, x, y, z)`
    fn base(&mut self, pos: [f64; 3]) -> Particle {
        let jitter = [self.r.next_float() * 3.0, self.r.next_float() * 3.0];
        let _ = jitter;
        let scale = (self.r.next_float() * 0.5 + 0.5) * 2.0;
        let max_age = (4.0 / (self.r.next_float() * 0.9 + 0.1)) as i32;
        Particle {
            pos,
            prev: pos,
            motion: [0.0; 3],
            on_ground: false,
            age: 0,
            max_age,
            gravity: 0.0,
            scale,
            color: [1.0; 4],
            sprite: Sprite::Index(0),
            kind: Kind::Plain,
            no_clip: false,
            base_scale: scale,
            flame_base: 0.0,
            origin: pos,
        }
    }

    /// `EntityFX(world, x, y, z, mx, my, mz)` with its randomised velocity.
    fn moving(&mut self, pos: [f64; 3], m: [f64; 3]) -> Particle {
        let mut p = self.base(pos);
        let mut v = [
            m[0] + (self.rnd() * 2.0 - 1.0) * 0.4,
            m[1] + (self.rnd() * 2.0 - 1.0) * 0.4,
            m[2] + (self.rnd() * 2.0 - 1.0) * 0.4,
        ];
        let f = ((self.rnd() + self.rnd() + 1.0) * 0.15) as f32;
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
        for x in &mut v {
            *x = *x / l * f as f64 * 0.4;
        }
        v[1] += 0.1;
        p.motion = v;
        p
    }

    fn add(&mut self, p: Particle) {
        if self.list.len() >= 4000 {
            self.list.remove(0);
        }
        self.list.push(p);
    }

    fn digging(&mut self, pos: [f64; 3], m: [f64; 3], tex: [f32; 4], tint: [f32; 3]) -> Particle {
        let mut p = self.moving(pos, m);
        let jitter = [self.r.next_float() * 3.0, self.r.next_float() * 3.0];
        p.sprite = Sprite::Block { rect: tex, jitter };
        p.gravity = 1.0;
        p.color = [0.6 * tint[0], 0.6 * tint[1], 0.6 * tint[2], 1.0];
        p.scale /= 2.0;
        p
    }

    /// `EffectRenderer.addBlockDestroyEffects`
    pub fn destroy(&mut self, pos: [i32; 3], tex: Option<[f32; 4]>, tint: [f32; 3]) {
        let Some(tex) = tex else { return };
        for i in 0..4 {
            for j in 0..4 {
                for k in 0..4 {
                    let x = pos[0] as f64 + (i as f64 + 0.5) / 4.0;
                    let y = pos[1] as f64 + (j as f64 + 0.5) / 4.0;
                    let z = pos[2] as f64 + (k as f64 + 0.5) / 4.0;
                    let p = self.digging(
                        [x, y, z],
                        [
                            x - pos[0] as f64 - 0.5,
                            y - pos[1] as f64 - 0.5,
                            z - pos[2] as f64 - 0.5,
                        ],
                        tex,
                        tint,
                    );
                    self.add(p);
                }
            }
        }
    }

    /// `EffectRenderer.addBlockHitEffects` on `side`.
    pub fn hit(&mut self, pos: [i32; 3], side: [i32; 3], tex: Option<[f32; 4]>, tint: [f32; 3]) {
        let Some(tex) = tex else { return };
        let f = 0.1f64;
        let mut p = [
            pos[0] as f64 + self.rnd() * (1.0 - f * 2.0) + f,
            pos[1] as f64 + self.rnd() * (1.0 - f * 2.0) + f,
            pos[2] as f64 + self.rnd() * (1.0 - f * 2.0) + f,
        ];
        for a in 0..3 {
            if side[a] < 0 {
                p[a] = pos[a] as f64 - f;
            } else if side[a] > 0 {
                p[a] = pos[a] as f64 + 1.0 + f;
            }
        }
        let mut q = self.digging(p, [0.0; 3], tex, tint);
        for v in &mut q.motion {
            *v *= 0.2;
        }
        q.motion[1] = (q.motion[1] - 0.1) * 0.2 + 0.1;
        q.scale *= 0.6;
        self.add(q);
    }

    /// `"smoke"` (`EntitySmokeFX`, scale 1) or `"largesmoke"` (scale 2.5).
    pub fn smoke(&mut self, pos: [f64; 3], m: [f64; 3], big: bool) {
        let mut p = self.base(pos);
        p.motion = [
            p.motion[0] * 0.1 + m[0],
            p.motion[1] * 0.1 + m[1],
            p.motion[2] * 0.1 + m[2],
        ];
        let c = self.r.next_float() * 0.3;
        p.color = [c, c, c, 1.0];
        p.scale *= 0.75 * if big { 2.5 } else { 1.0 };
        p.base_scale = p.scale;
        p.max_age =
            ((8.0 / (self.r.next_float() as f64 * 0.8 + 0.2)) * if big { 2.5 } else { 1.0 }) as i32;
        p.kind = Kind::Smoke;
        p.sprite = Sprite::Index(7);
        self.add(p);
    }

    /// `"flame"` (`EntityFlameFX`)
    pub fn flame(&mut self, pos: [f64; 3], m: [f64; 3]) {
        let mut p = self.base(pos);
        p.motion = [
            p.motion[0] * 0.01 + m[0],
            p.motion[1] * 0.01 + m[1],
            p.motion[2] * 0.01 + m[2],
        ];
        p.pos = [
            pos[0] + (self.r.next_float() - self.r.next_float()) as f64 * 0.05,
            pos[1] + (self.r.next_float() - self.r.next_float()) as f64 * 0.05,
            pos[2] + (self.r.next_float() - self.r.next_float()) as f64 * 0.05,
        ];
        p.prev = p.pos;
        p.flame_base = p.scale;
        p.max_age = (8.0 / (self.rnd() * 0.8 + 0.2)) as i32 + 4;
        p.no_clip = true;
        p.kind = Kind::Flame;
        p.sprite = Sprite::Index(48);
        self.add(p);
    }

    /// `"lava"` (`EntityLavaFX`)
    pub fn lava(&mut self, pos: [f64; 3]) {
        let mut p = self.base(pos);
        p.motion[0] *= 0.8;
        p.motion[1] = (self.r.next_float() * 0.4 + 0.05) as f64;
        p.motion[2] *= 0.8;
        p.scale *= self.r.next_float() * 2.0 + 0.2;
        p.base_scale = p.scale;
        p.max_age = (16.0 / (self.rnd() * 0.8 + 0.2)) as i32;
        p.kind = Kind::Lava;
        p.sprite = Sprite::Index(49);
        self.add(p);
    }

    /// `"explode"` (`EntityExplodeFX`)
    pub fn explode(&mut self, pos: [f64; 3], m: [f64; 3]) {
        let mut p = self.base(pos);
        p.motion = [
            m[0] + (self.rnd() * 2.0 - 1.0) * 0.05,
            m[1] + (self.rnd() * 2.0 - 1.0) * 0.05,
            m[2] + (self.rnd() * 2.0 - 1.0) * 0.05,
        ];
        let c = self.r.next_float() * 0.3 + 0.7;
        p.color = [c, c, c, 1.0];
        p.scale = self.r.next_float() * self.r.next_float() * 6.0 + 1.0;
        p.max_age = (16.0 / (self.r.next_float() as f64 * 0.8 + 0.2)) as i32 + 2;
        p.kind = Kind::Explode;
        p.sprite = Sprite::Index(7);
        self.add(p);
    }

    /// `"largeexplode"` (`EntityLargeExplodeFX`)
    pub fn large_explode(&mut self, pos: [f64; 3], size: f32) {
        let mut p = self.base(pos);
        p.max_age = 6 + self.r.next_int_n(4);
        let c = self.r.next_float() * 0.6 + 0.4;
        p.color = [c, c, c, 1.0];
        p.scale = 2.0 * (1.0 - size * 0.5);
        p.kind = Kind::Large;
        p.sprite = Sprite::Explosion;
        p.no_clip = true;
        self.add(p);
    }

    /// `"hugeexplosion"`: an emitter of `largeexplode` for 8 ticks.
    pub fn huge_explosion(&mut self, pos: [f64; 3]) {
        self.huge.push((pos, 8));
    }

    /// `"crit"` (`EntityCritFX`)
    pub fn crit(&mut self, pos: [f64; 3], m: [f64; 3]) {
        let mut p = self.base(pos);
        p.motion = [
            p.motion[0] * 0.1 + m[0] * 0.4,
            p.motion[1] * 0.1 + m[1] * 0.4,
            p.motion[2] * 0.1 + m[2] * 0.4,
        ];
        let c = (self.r.next_float() * 0.3 + 0.6) as f32;
        p.color = [c, c, c, 1.0];
        p.scale *= 0.75;
        p.max_age = ((6.0 / (self.rnd() * 0.8 + 0.6)) as i32).max(1);
        p.kind = Kind::Crit;
        p.sprite = Sprite::Index(65);
        self.add(p);
    }

    /// `EntityNoteFX`: coloured by pitch (0..24).
    pub fn note(&mut self, pos: [f64; 3], pitch: i32) {
        let mut p = self.moving(pos, [0.0; 3]);
        for v in &mut p.motion {
            *v *= 0.01;
        }
        p.motion[1] += 0.2;
        let f = pitch as f32 / 24.0;
        let tau = std::f32::consts::PI * 2.0;
        p.color = [
            (f * tau).sin() * 0.65 + 0.35,
            ((f + 1.0 / 3.0) * tau).sin() * 0.65 + 0.35,
            ((f + 2.0 / 3.0) * tau).sin() * 0.65 + 0.35,
            1.0,
        ];
        p.scale *= 0.75 * 2.0;
        p.max_age = 6;
        p.kind = Kind::Note;
        p.sprite = Sprite::Index(64);
        self.add(p);
    }

    /// `EntityPortalFX`: drifts back towards where it started.
    pub fn portal(&mut self, pos: [f64; 3], m: [f64; 3]) {
        let mut p = self.base(pos);
        p.motion = m;
        p.scale = self.r.next_float() * 0.2 + 0.5;
        p.base_scale = p.scale;
        let f = self.r.next_float() * 0.6 + 0.4;
        p.color = [f * 0.9, f * 0.3, f, 1.0];
        p.max_age = (self.rnd() * 10.0) as i32 + 40;
        p.no_clip = true;
        p.sprite = Sprite::Index((self.rnd() * 8.0) as i32);
        p.kind = Kind::Portal;
        p.origin = pos;
        self.add(p);
    }

    /// `EntityHeartFX`
    pub fn heart(&mut self, pos: [f64; 3], m: [f64; 3]) {
        let mut p = self.moving(pos, [0.0; 3]);
        for (v, a) in p.motion.iter_mut().zip(m) {
            *v = *v * 0.01 + a;
        }
        p.motion[1] += 0.1;
        p.scale *= 0.75 * 2.0;
        p.base_scale = p.scale;
        p.max_age = 16;
        p.kind = Kind::Heart;
        p.sprite = Sprite::Index(80);
        self.add(p);
    }

    /// `EntityBreakingFX` of an item (slime balls when a slime lands).
    pub fn item_break(&mut self, pos: [f64; 3], rect: [f32; 4]) {
        let mut p = self.moving(pos, [0.0; 3]);
        let jitter = [self.r.next_float() * 3.0, self.r.next_float() * 3.0];
        p.sprite = Sprite::Block { rect, jitter };
        p.gravity = 1.0;
        p.scale /= 2.0;
        p.kind = Kind::Dust;
        self.add(p);
    }

    /// `"iconcrack_<id>"`: `EntityBreakingFX(world, x, y, z, mx, my, mz, item)`
    pub fn item_crack(&mut self, pos: [f64; 3], m: [f64; 3], rect: [f32; 4]) {
        let mut p = self.moving(pos, [0.0; 3]);
        let jitter = [self.r.next_float() * 3.0, self.r.next_float() * 3.0];
        p.sprite = Sprite::Block { rect, jitter };
        p.gravity = 1.0;
        p.scale /= 2.0;
        p.kind = Kind::Dust;
        for a in 0..3 {
            p.motion[a] = p.motion[a] * 0.1f32 as f64 + m[a];
        }
        self.add(p);
    }

    /// `EntityRainFX` splash
    pub fn rain(&mut self, pos: [f64; 3]) {
        let mut p = self.base(pos);
        p.motion = [p.motion[0] * 0.3, self.rnd() * 0.2 + 0.1, p.motion[2] * 0.3];
        p.gravity = 0.06;
        p.sprite = Sprite::Index(19 + self.r.next_int_n(4));
        p.max_age = (8.0 / (self.rnd() * 0.8 + 0.2)) as i32;
        p.kind = Kind::Rain;
        self.add(p);
    }

    /// `EntityBubbleFX`
    pub fn bubble(&mut self, pos: [f64; 3], m: [f64; 3]) {
        let mut p = self.moving(pos, m);
        p.color = [1.0, 1.0, 1.0, 1.0];
        p.sprite = Sprite::Index(32);
        p.scale *= self.r.next_float() * 0.6 + 0.2;
        p.base_scale = p.scale;
        p.motion = [
            m[0] * 0.2f32 as f64 + ((self.rnd() * 2.0 - 1.0) as f32 * 0.02) as f64,
            m[1] * 0.2f32 as f64 + ((self.rnd() * 2.0 - 1.0) as f32 * 0.02) as f64,
            m[2] * 0.2f32 as f64 + ((self.rnd() * 2.0 - 1.0) as f32 * 0.02) as f64,
        ];
        p.max_age = (8.0 / (self.rnd() * 0.8 + 0.2)) as i32;
        p.kind = Kind::Bubble;
        self.add(p);
    }

    /// `EntitySplashFX` (an `EntityRainFX` with gravity 0.04)
    pub fn splash(&mut self, pos: [f64; 3], m: [f64; 3]) {
        let mut p = self.base(pos);
        p.motion = [p.motion[0] * 0.3, self.rnd() * 0.2 + 0.1, p.motion[2] * 0.3];
        p.gravity = 0.04;
        p.sprite = Sprite::Index(19 + self.r.next_int_n(4) + 1);
        p.max_age = (8.0 / (self.rnd() * 0.8 + 0.2)) as i32;
        if m[1] == 0.0 && (m[0] != 0.0 || m[2] != 0.0) {
            p.motion = [m[0], m[1] + 0.1, m[2]];
        }
        p.kind = Kind::Rain;
        self.add(p);
    }

    /// `EntitySpellParticleFX`: "spell" (base 128), "instantSpell" and
    /// "witchMagic" (144), "mobSpell" (motion 0, coloured).
    pub fn spell(&mut self, pos: [f64; 3], m: [f64; 3], base: i32, rgb: [f32; 3], alpha: f32) {
        let mut p = self.moving(pos, m);
        p.motion[1] *= 0.2f32 as f64;
        if m[0] == 0.0 && m[2] == 0.0 {
            p.motion[0] *= 0.1f32 as f64;
            p.motion[2] *= 0.1f32 as f64;
        }
        p.scale *= 0.75;
        p.base_scale = p.scale;
        p.max_age = (8.0 / (self.rnd() * 0.8 + 0.2)) as i32;
        p.color = [rgb[0], rgb[1], rgb[2], alpha];
        p.kind = Kind::Spell(base);
        p.sprite = Sprite::Index(base + 7);
        self.add(p);
    }

    /// `EntityFX.multiplyVelocity` on the newest particle.
    pub fn scale_last_motion(&mut self, f: f32) {
        if let Some(p) = self.list.last_mut() {
            p.motion[0] *= f as f64;
            p.motion[1] = (p.motion[1] - 0.1f32 as f64) * f as f64 + 0.1f32 as f64;
            p.motion[2] *= f as f64;
        }
    }

    /// `EntityReddustFX` (red by default)
    pub fn reddust(&mut self, pos: [f64; 3], rgb: [f32; 3]) {
        let mut p = self.moving(pos, [0.0; 3]);
        for v in &mut p.motion {
            *v *= 0.1f32 as f64;
        }
        let r = if rgb[0] == 0.0 { 1.0 } else { rgb[0] };
        let f = self.rnd() as f32 * 0.4 + 0.6;
        p.color = [
            ((self.rnd() * 0.2f32 as f64) as f32 + 0.8) * r * f,
            ((self.rnd() * 0.2f32 as f64) as f32 + 0.8) * rgb[1] * f,
            ((self.rnd() * 0.2f32 as f64) as f32 + 0.8) * rgb[2] * f,
            1.0,
        ];
        p.scale *= 0.75;
        p.base_scale = p.scale;
        p.max_age = (8.0 / (self.rnd() * 0.8 + 0.2)) as i32;
        p.kind = Kind::Reddust;
        p.sprite = Sprite::Index(7);
        self.add(p);
    }

    /// `EntityDropParticleFX` (water or lava drip from a ceiling)
    pub fn drip(&mut self, pos: [f64; 3], lava: bool) {
        let mut p = self.base(pos);
        p.motion = [0.0; 3];
        p.color = if lava {
            [1.0, 0.0, 0.0, 1.0]
        } else {
            [0.0, 0.0, 1.0, 1.0]
        };
        p.sprite = Sprite::Index(113);
        p.gravity = 0.06;
        p.max_age = (64.0 / (self.rnd() * 0.8 + 0.2)) as i32;
        p.kind = Kind::Drip;
        self.add(p);
    }

    /// `"blockcrack"` dust behind a running entity.
    pub fn dust(&mut self, pos: [f64; 3], m: [f64; 3], tex: Option<[f32; 4]>, tint: [f32; 3]) {
        let Some(tex) = tex else { return };
        let mut p = self.digging(pos, m, tex, tint);
        p.kind = Kind::Dust;
        self.add(p);
    }

    pub fn tick(&mut self, w: &dyn BlockView) {
        let mut huge = std::mem::take(&mut self.huge);
        for (pos, t) in huge.iter_mut() {
            for _ in 0..6 {
                let q = [
                    pos[0] + (self.rnd() - self.rnd()) * 4.0,
                    pos[1] + (self.rnd() - self.rnd()) * 4.0,
                    pos[2] + (self.rnd() - self.rnd()) * 4.0,
                ];
                let s = (8 - *t) as f32 / 8.0;
                self.large_explode(q, s);
            }
            *t -= 1;
        }
        huge.retain(|h| h.1 > 0);
        self.huge = huge;
        let mut list = std::mem::take(&mut self.list);
        list.retain_mut(|p| {
            p.prev = p.pos;
            p.age += 1;
            if p.age >= p.max_age {
                return false;
            }
            if p.kind == Kind::Portal {
                // EntityPortalFX.onUpdate
                let f = p.age as f64 / p.max_age as f64;
                let f1 = 1.0 - (-f + f * f * 2.0);
                for a in 0..3 {
                    p.pos[a] = p.origin[a] + p.motion[a] * f1;
                }
                p.pos[1] += 1.0 - f;
                return true;
            }
            if p.kind == Kind::Bubble {
                // EntityBubbleFX.onUpdate (the age was counted above)
                p.motion[1] += 0.002;
                move_particle(p, w);
                for v in &mut p.motion {
                    *v *= 0.85f32 as f64;
                }
                let b = w.peek_block(
                    math::floor_double(p.pos[0]),
                    math::floor_double(p.pos[1]),
                    math::floor_double(p.pos[2]),
                );
                return block::material(b) == block::Material::Water;
            }
            match p.kind {
                Kind::Reddust => {
                    p.sprite = Sprite::Index(7 - p.age * 8 / p.max_age);
                }
                Kind::Spell(base) => {
                    p.sprite = Sprite::Index(base + (7 - p.age * 8 / p.max_age));
                    p.motion[1] += 0.004;
                }
                Kind::Smoke => {
                    p.sprite = Sprite::Index(7 - p.age * 8 / p.max_age);
                    p.motion[1] += 0.004;
                }
                Kind::Explode => {
                    p.sprite = Sprite::Index(7 - p.age * 8 / p.max_age);
                    p.motion[1] += 0.004;
                }
                Kind::Lava => {
                    p.motion[1] -= 0.03;
                }
                Kind::Crit => {
                    p.color[1] *= 0.96;
                    p.color[2] *= 0.9;
                    p.motion[1] -= 0.02;
                }
                Kind::Drip | Kind::Note | Kind::Heart => {}
                _ => {
                    p.motion[1] -= 0.04 * p.gravity as f64;
                }
            }
            if p.kind == Kind::Drip {
                p.motion[1] -= p.gravity as f64;
            }
            if p.kind != Kind::Large {
                move_particle(p, w);
            }
            let damp = match p.kind {
                Kind::Smoke | Kind::Explode | Kind::Reddust | Kind::Spell(_) => 0.96,
                Kind::Flame => 0.96,
                Kind::Lava => 0.999,
                Kind::Crit => 0.7,
                Kind::Note => 0.66,
                Kind::Heart => 0.86,
                Kind::Rain | Kind::Drip => 0.98,
                _ => 0.98,
            };
            for v in &mut p.motion {
                *v *= damp;
            }
            if p.on_ground {
                p.motion[0] *= 0.7;
                p.motion[2] *= 0.7;
                if p.kind == Kind::Rain {
                    return false;
                }
            }
            if matches!(
                p.kind,
                Kind::Smoke
                    | Kind::Explode
                    | Kind::Note
                    | Kind::Heart
                    | Kind::Reddust
                    | Kind::Spell(_)
            ) && p.pos[1] == p.prev[1]
            {
                p.motion[0] *= 1.1;
                p.motion[2] *= 1.1;
            }
            true
        });
        self.list = list;
    }

    /// Camera-facing quads (`EntityFX.renderParticle`), camera-relative.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &self,
        out: &mut Vec<Vertex>,
        w: &dyn Fn(i32, i32, i32) -> (i32, i32),
        eye: [f64; 3],
        yaw: f32,
        pitch: f32,
        partial: f32,
        sheet: Option<Tex>,
        explosion: Option<Tex>,
    ) {
        let (ys, yc) = (yaw.to_radians().sin(), yaw.to_radians().cos());
        let (ps, pc) = (pitch.to_radians().sin(), pitch.to_radians().cos());
        // ActiveRenderInfo rotation values
        let rx = yc;
        let rz = ys;
        let ryz = -rz * ps;
        let rxy = rx * ps;
        let rxz = pc;
        for p in &self.list {
            let t = partial as f64;
            let pos = [
                p.prev[0] + (p.pos[0] - p.prev[0]) * t,
                p.prev[1] + (p.pos[1] - p.prev[1]) * t,
                p.prev[2] + (p.pos[2] - p.prev[2]) * t,
            ];
            let rel = [
                (pos[0] - eye[0]) as f32,
                (pos[1] - eye[1]) as f32,
                (pos[2] - eye[2]) as f32,
            ];
            let mut size = 0.1 * p.scale;
            let (u0, v0, u1, v1);
            match p.sprite {
                Sprite::Index(i) => {
                    let Some(s) = sheet else { continue };
                    let cell = (s.rect[2] - s.rect[0]) * 8.0 / s.size;
                    let cellv = (s.rect[3] - s.rect[1]) * 8.0 / s.size;
                    let i = i.clamp(0, 255);
                    u0 = s.rect[0] + (i % 16) as f32 * cell;
                    v0 = s.rect[1] + (i / 16) as f32 * cellv;
                    u1 = u0 + cell;
                    v1 = v0 + cellv;
                }
                Sprite::Block { rect, jitter } => {
                    let du = (rect[2] - rect[0]) / 16.0;
                    let dv = (rect[3] - rect[1]) / 16.0;
                    u0 = rect[0] + jitter[0] / 4.0 * 16.0 * du / 4.0;
                    v0 = rect[1] + jitter[1] / 4.0 * 16.0 * dv / 4.0;
                    u1 = u0 + du * 4.0;
                    v1 = v0 + dv * 4.0;
                }
                Sprite::Explosion => {
                    let Some(s) = explosion else { continue };
                    let f = ((p.age as f32 + partial) * 15.0 / p.max_age as f32) as i32;
                    if f > 15 {
                        continue;
                    }
                    let cw = (s.rect[2] - s.rect[0]) * 32.0 / s.size;
                    let ch = (s.rect[3] - s.rect[1]) * 32.0 / s.size;
                    u0 = s.rect[0] + (f % 4) as f32 * cw;
                    v0 = s.rect[1] + (f / 4) as f32 * ch;
                    u1 = u0 + cw;
                    v1 = v0 + ch;
                    size = 2.0 * p.scale;
                }
            }
            match p.kind {
                Kind::Smoke | Kind::Explode => {
                    let mut f = (p.age as f32 + partial) / p.max_age as f32 * 32.0;
                    f = f.clamp(0.0, 1.0);
                    size = 0.1 * p.base_scale * f;
                }
                Kind::Flame => {
                    let f = (p.age as f32 + partial) / p.max_age as f32;
                    size = 0.1 * p.flame_base * (1.0 - f * f * 0.5);
                }
                Kind::Lava => {
                    let f = (p.age as f32 + partial) / p.max_age as f32;
                    size = 0.1 * p.base_scale * (1.0 - f * f);
                }
                _ => {}
            }
            let (bx, by, bz) = (
                math::floor_double(pos[0]),
                math::floor_double(pos[1]),
                math::floor_double(pos[2]),
            );
            let (sky, blk) = w(bx, by, bz);
            let blk = if matches!(p.kind, Kind::Flame | Kind::Lava | Kind::Large) {
                15
            } else {
                blk
            };
            let light = [(blk << 4) as u8, (sky << 4) as u8, 0, 0];
            let col = [
                (p.color[0] * 255.0) as u8,
                (p.color[1] * 255.0) as u8,
                (p.color[2] * 255.0) as u8,
                255,
            ];
            let c = |a: f32, b: f32, cc: f32, d: f32, e: f32| {
                [
                    rel[0] + a * size + d * size,
                    rel[1] + b * size,
                    rel[2] + cc * size + e * size,
                ]
            };
            let p0 = c(-rx, -rxz, -rz, -ryz, -rxy);
            let p0 = [
                rel[0] - rx * size - ryz * size,
                rel[1] - rxz * size,
                rel[2] - rz * size - rxy * size,
            ];
            let p1 = [
                rel[0] - rx * size + ryz * size,
                rel[1] + rxz * size,
                rel[2] - rz * size + rxy * size,
            ];
            let p2 = [
                rel[0] + rx * size + ryz * size,
                rel[1] + rxz * size,
                rel[2] + rz * size + rxy * size,
            ];
            let p3 = [
                rel[0] + rx * size - ryz * size,
                rel[1] - rxz * size,
                rel[2] + rz * size - rxy * size,
            ];
            let _ = c;
            let q = [
                Vertex {
                    pos: p0,
                    uv: [u1, v1],
                    color: col,
                    light,
                },
                Vertex {
                    pos: p1,
                    uv: [u1, v0],
                    color: col,
                    light,
                },
                Vertex {
                    pos: p2,
                    uv: [u0, v0],
                    color: col,
                    light,
                },
                Vertex {
                    pos: p3,
                    uv: [u0, v1],
                    color: col,
                    light,
                },
            ];
            out.extend_from_slice(&q);
            out.extend_from_slice(&[q[3], q[2], q[1], q[0]]);
        }
    }

    /// `Block.randomDisplayTick` for the blocks around the player
    /// (`WorldClient.doVoidFogParticles`: 1000 random positions within 16).
    pub fn display_ticks(&mut self, w: &dyn BlockView, eye: [f64; 3]) {
        let (px, py, pz) = (
            math::floor_double(eye[0]),
            math::floor_double(eye[1]),
            math::floor_double(eye[2]),
        );
        for _ in 0..1000 {
            let x = px + self.r.next_int_n(16) - self.r.next_int_n(16);
            let y = py + self.r.next_int_n(16) - self.r.next_int_n(16);
            let z = pz + self.r.next_int_n(16) - self.r.next_int_n(16);
            let id = w.peek_block(x, y, z);
            if id == 0 {
                continue;
            }
            let m = w.peek_meta(x, y, z) as i32;
            match id {
                block::TORCH | block::REDSTONE_TORCH => {
                    // BlockTorch.randomDisplayTick
                    let (cx, cy, cz) = (x as f64 + 0.5, y as f64 + 0.7, z as f64 + 0.5);
                    let (d3, d4) = (0.22, 0.27);
                    let p = match m {
                        1 => [cx - d4, cy + d3, cz],
                        2 => [cx + d4, cy + d3, cz],
                        3 => [cx, cy + d3, cz - d4],
                        4 => [cx, cy + d3, cz + d4],
                        _ => [cx, cy, cz],
                    };
                    if id == block::TORCH {
                        self.smoke(p, [0.0; 3], false);
                        self.flame(p, [0.0; 3]);
                    } else {
                        let mut q = self.base(p);
                        q.color = [1.0, 0.0, 0.0, 1.0];
                        q.sprite = Sprite::Index(7);
                        q.kind = Kind::Smoke;
                        q.max_age = 20;
                        self.add(q);
                    }
                }
                block::LIT_FURNACE => {
                    let f = (
                        x as f64 + 0.5,
                        y as f64 + self.rnd() * 6.0 / 16.0,
                        z as f64 + 0.5,
                    );
                    let d = 0.52;
                    let s = self.rnd() * 0.6 - 0.3;
                    let p = match m {
                        4 => [f.0 - d, f.1, f.2 + s],
                        5 => [f.0 + d, f.1, f.2 + s],
                        2 => [f.0 + s, f.1, f.2 - d],
                        _ => [f.0 + s, f.1, f.2 + d],
                    };
                    self.smoke(p, [0.0; 3], false);
                    self.flame(p, [0.0; 3]);
                }
                block::LAVA | block::FLOWING_LAVA => {
                    // BlockLiquid.randomDisplayTick: lava pops
                    if w.peek_block(x, y + 1, z) == 0
                        && !block::info(w.peek_block(x, y + 1, z)).opaque
                        && self.r.next_int_n(100) == 0
                    {
                        let p = [
                            x as f64 + self.r.next_float() as f64,
                            y as f64 + 1.0,
                            z as f64 + self.r.next_float() as f64,
                        ];
                        self.lava(p);
                    }
                }
                _ => {}
            }
            // drips through the block below a liquid
            if self.r.next_int_n(10) == 0 && block::info(id).normal_cube() {
                let below = w.peek_block(x, y - 1, z);
                let above = w.peek_block(x, y + 1, z);
                let mat = block::material(above);
                if !block::info(below).material.blocks_movement()
                    && !block::material(below).is_liquid()
                    && mat.is_liquid()
                {
                    let p = [
                        x as f64 + self.r.next_float() as f64,
                        y as f64 - 0.05,
                        z as f64 + self.r.next_float() as f64,
                    ];
                    self.drip(p, mat == block::Material::Lava);
                }
            }
        }
    }

    pub fn count(&self) -> usize {
        self.list.len()
    }
}

fn move_particle(p: &mut Particle, w: &dyn BlockView) {
    let h = 0.1f64;
    if p.no_clip {
        for a in 0..3 {
            p.pos[a] += p.motion[a];
        }
        return;
    }
    let mut bb = Aabb {
        min: [p.pos[0] - h, p.pos[1] - h, p.pos[2] - h],
        max: [p.pos[0] + h, p.pos[1] + h, p.pos[2] + h],
    };
    let (ox, oy, oz) = (p.motion[0], p.motion[1], p.motion[2]);
    let (mut dx, mut dy, mut dz) = (ox, oy, oz);
    let list = colliding_boxes(w, &bb.add_coord(dx, dy, dz));
    for b in &list {
        dy = b.calc_offset(&bb, 1, dy);
    }
    bb.offset(0.0, dy, 0.0);
    for b in &list {
        dx = b.calc_offset(&bb, 0, dx);
    }
    bb.offset(dx, 0.0, 0.0);
    for b in &list {
        dz = b.calc_offset(&bb, 2, dz);
    }
    bb.offset(0.0, 0.0, dz);
    p.pos = [
        (bb.min[0] + bb.max[0]) / 2.0,
        (bb.min[1] + bb.max[1]) / 2.0,
        (bb.min[2] + bb.max[2]) / 2.0,
    ];
    p.on_ground = oy != dy && oy < 0.0;
    if ox != dx {
        p.motion[0] = 0.0;
    }
    if oy != dy {
        p.motion[1] = 0.0;
    }
    if oz != dz {
        p.motion[2] = 0.0;
    }
}
