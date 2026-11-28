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
    /// `EntitySnowShovelFX`
    SnowShovel,
    /// `EntityFireworkSparkFX`: trail, twinkle, fade colour
    Spark {
        trail: bool,
        twinkle: bool,
        fade: Option<[f32; 3]>,
    },
    /// `EntityFireworkOverlayFX`: the flash of each explosion
    FireOverlay,
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
    /// `EntityEnchantmentTableParticleFX`
    Enchant,
    Heart,
    Bubble,
    Reddust,
    /// `EntitySpellParticleFX` with its base texture index
    Spell(i32),
    /// `EntityAuraFX`: no gravity, slowed by 0.99
    Aura,
    /// `EntityFishWakeFX`: counts its `particleMaxAge` down
    Wake,
}

impl Particle {
    /// `EntityFX.getFXLayer`: 0 `particles.png`, 1 the block atlas,
    /// 3 the ones that bind their own texture (`EntityLargeExplodeFX`).
    fn layer(&self) -> usize {
        match self.sprite {
            Sprite::Index(_) => 0,
            Sprite::Block { .. } => 1,
            Sprite::Explosion => 2,
        }
    }
}

pub struct Particles {
    /// `EffectRenderer.fxLayers`: one list per layer, oldest first, at
    /// most 4000 each; ticked and drawn layer by layer.
    list: [std::collections::VecDeque<Particle>; 3],
    /// The layer of the particle added last.
    last: usize,
    /// While set, every particle added goes through `gate` first: the
    /// ones the game spawns with `World.spawnParticle`.
    pub gated: bool,
    /// `gameSettings.particleSetting` (0 all, 1 decreased, 2 minimal)
    pub setting: i32,
    /// `renderViewEntity`'s position, for the 16-block limit
    pub camera: [f64; 3],
    r: JavaRandom,
    /// `EntityHugeExplodeFX` emitters: position and remaining ticks.
    huge: Vec<([f64; 3], i32)>,
}

impl Default for Particles {
    fn default() -> Self {
        Particles {
            list: Default::default(),
            last: 0,
            gated: false,
            setting: 0,
            camera: [0.0; 3],
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

    /// `RenderGlobal.doSpawnParticle` (past the explosions it always
    /// makes): a decreased setting counts as minimal one time in three,
    /// minimal makes none, and none farther than 16 blocks from the camera.
    fn gate(&mut self, pos: [f64; 3]) -> bool {
        let mut s = self.setting;
        if s == 1 && self.r.next_int_n(3) == 0 {
            s = 2;
        }
        let d = [
            self.camera[0] - pos[0],
            self.camera[1] - pos[1],
            self.camera[2] - pos[2],
        ];
        if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] > 16.0 * 16.0 {
            return false;
        }
        s <= 1
    }

    /// Runs `f` with every particle it adds going through `gate`.
    pub fn spawned(&mut self, f: impl FnOnce(&mut Particles)) {
        let was = self.gated;
        self.gated = true;
        f(self);
        self.gated = was;
    }

    fn last_mut(&mut self) -> Option<&mut Particle> {
        self.list[self.last].back_mut()
    }

    /// `EffectRenderer.addEffect`: a full layer loses its oldest particle.
    fn add(&mut self, p: Particle) {
        if self.gated && !self.gate(p.pos) {
            return;
        }
        let l = p.layer();
        if self.list[l].len() >= 4000 {
            self.list[l].pop_front();
        }
        self.list[l].push_back(p);
        self.last = l;
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

    /// `EntityFireworkSparkFX(world, x, y, z, mx, my, mz)`
    fn spark(&mut self, pos: [f64; 3], m: [f64; 3], trail: bool, twinkle: bool) -> Particle {
        let mut p = self.base(pos);
        p.motion = m;
        p.scale *= 0.75;
        p.base_scale = p.scale;
        p.max_age = 48 + self.r.next_int_n(12);
        p.kind = Kind::Spark {
            trail,
            twinkle,
            fade: None,
        };
        p.sprite = Sprite::Index(167);
        p
    }

    /// `"fireworksSpark"`: the rocket's trail
    pub fn firework_spark(&mut self, pos: [f64; 3], m: [f64; 3]) {
        let p = self.spark(pos, m, false, false);
        self.add(p);
    }

    /// `"snowshovel"` (`EntitySnowShovelFX`)
    pub fn snow_shovel(&mut self, pos: [f64; 3], m: [f64; 3]) {
        let mut p = self.base(pos);
        p.motion = [
            p.motion[0] * 0.1 + m[0],
            p.motion[1] * 0.1 + m[1],
            p.motion[2] * 0.1 + m[2],
        ];
        let c = 1.0 - (crate::util::math::math_random() * 0.3) as f32;
        p.color = [c, c, c, 1.0];
        p.scale *= 0.75;
        p.base_scale = p.scale;
        p.max_age = (8.0 / (crate::util::math::math_random() * 0.8 + 0.2)) as i32;
        p.kind = Kind::SnowShovel;
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

    /// `"magicCrit"`: an `EntityCritFX` tinted (red × 0.3, green × 0.8) on
    /// the next sprite (`nextTextureIndexX`).
    pub fn magic_crit(&mut self, pos: [f64; 3], m: [f64; 3]) {
        self.crit(pos, m);
        if let Some(p) = self.last_mut() {
            p.color[0] *= 0.3;
            p.color[1] *= 0.8;
            p.sprite = Sprite::Index(66);
        }
    }

    /// `"happyVillager"`: an `EntityAuraFX` on sprite 82, white.
    pub fn happy_villager(&mut self, pos: [f64; 3], m: [f64; 3]) {
        let mut p = self.moving(pos, m);
        let _ = self.r.next_float() * 0.1 + 0.2;
        p.scale *= self.r.next_float() * 0.6 + 0.5;
        for v in &mut p.motion {
            *v *= 0.02f32 as f64;
        }
        p.max_age = (20.0 / (crate::util::math::math_random() * 0.8 + 0.2)) as i32;
        p.no_clip = true;
        p.color = [1.0, 1.0, 1.0, 1.0];
        p.kind = Kind::Aura;
        p.sprite = Sprite::Index(82);
        self.add(p);
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

    /// `EntityEnchantmentTableParticleFX`: a glyph flying from a bookshelf
    /// down into the table.
    pub fn enchant(&mut self, pos: [f64; 3], m: [f64; 3]) {
        let mut p = self.base(pos);
        p.motion = m;
        let f = self.r.next_float() * 0.6 + 0.4;
        p.scale = self.r.next_float() * 0.5 + 0.2;
        p.base_scale = p.scale;
        p.color = [f * 0.9, f * 0.9, f, 1.0];
        p.max_age = (self.rnd() * 10.0) as i32 + 30;
        p.no_clip = true;
        p.sprite = Sprite::Index((self.rnd() * 26.0 + 1.0 + 224.0) as i32);
        p.kind = Kind::Enchant;
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

    /// `EntityFishWakeFX`
    pub fn wake(&mut self, pos: [f64; 3], m: [f64; 3]) {
        let mut p = self.moving(pos, [0.0; 3]);
        p.motion[0] *= 0.3f32 as f64;
        p.motion[1] = (self.rnd() as f32 * 0.2 + 0.1) as f64;
        p.motion[2] *= 0.3f32 as f64;
        p.color = [1.0, 1.0, 1.0, 1.0];
        p.sprite = Sprite::Index(19);
        p.max_age = (8.0 / (self.rnd() * 0.8 + 0.2)) as i32;
        p.gravity = 0.0;
        p.motion = m;
        p.kind = Kind::Wake;
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
        if let Some(p) = self.last_mut() {
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

    /// `RenderGlobal.playAuxSFX(2006)`: `EntityBlockDustFX` bursting from
    /// the top of the block a hard fall landed on, more for a longer fall
    /// (`ceil(fallDistance - 3)` = `n`), with exactly the motion given.
    pub fn fall_dust(&mut self, pos: [i32; 3], n: i32, tex: Option<[f32; 4]>) {
        let Some(tex) = tex else { return };
        let mut d = (0.2f32 + n as f32 / 15.0).min(10.0) as f64;
        if d > 2.5 {
            d = 2.5;
        }
        let count = (150.0 * d) as i32;
        let c = [
            pos[0] as f64 + 0.5,
            pos[1] as f64 + 1.0,
            pos[2] as f64 + 0.5,
        ];
        for _ in 0..count {
            // MathHelper.randomFloatClamp
            let a = self.r.next_float() * (std::f32::consts::PI * 2.0);
            let b = (self.r.next_float() * 0.25 + 0.75) as f64;
            let my = 0.2f32 as f64 + d / 100.0;
            let mx = (crate::util::math::cos(a) * 0.2) as f64 * b * b * (d + 0.2);
            let mz = (crate::util::math::sin(a) * 0.2) as f64 * b * b * (d + 0.2);
            let mut p = self.digging(c, [mx, my, mz], tex, [1.0; 3]);
            p.motion = [mx, my, mz];
            self.add(p);
        }
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
        let mut lists = std::mem::take(&mut self.list);
        let mut trails: Vec<([f64; 3], [f32; 3], Option<[f32; 3]>, bool)> = Vec::new();
        for list in lists.iter_mut() {
            list.retain_mut(|p| {
                p.prev = p.pos;
                if let Kind::Spark {
                    trail,
                    twinkle,
                    fade,
                } = p.kind
                {
                    // EntityFireworkSparkFX.onUpdate
                    let alive = p.age < p.max_age;
                    p.age += 1;
                    if p.age > p.max_age / 2 {
                        p.color[3] = 1.0 - (p.age - p.max_age / 2) as f32 / p.max_age as f32;
                        if let Some(f) = fade {
                            for k in 0..3 {
                                p.color[k] += (f[k] - p.color[k]) * 0.2;
                            }
                        }
                    }
                    p.sprite = Sprite::Index(160 + (7 - p.age * 8 / p.max_age));
                    p.motion[1] -= 0.004;
                    move_particle(p, w);
                    for v in &mut p.motion {
                        *v *= 0.91f32 as f64;
                    }
                    if p.on_ground {
                        p.motion[0] *= 0.7f32 as f64;
                        p.motion[2] *= 0.7f32 as f64;
                    }
                    if trail && p.age < p.max_age / 2 && (p.age + p.max_age) % 2 == 0 {
                        trails.push((p.pos, [p.color[0], p.color[1], p.color[2]], fade, twinkle));
                    }
                    return alive;
                }
                if p.kind == Kind::FireOverlay {
                    let alive = p.age < p.max_age;
                    p.age += 1;
                    return alive;
                }
                if p.kind == Kind::Wake {
                    // EntityFishWakeFX.onUpdate
                    p.motion[1] -= p.gravity as f64;
                    move_particle(p, w);
                    for v in &mut p.motion {
                        *v *= 0.98f32 as f64;
                    }
                    let var1 = 60 - p.max_age;
                    p.sprite = Sprite::Index(19 + var1 % 4);
                    let alive = p.max_age > 0;
                    p.max_age -= 1;
                    return alive;
                }
                p.age += 1;
                if p.age >= p.max_age {
                    return false;
                }
                if p.kind == Kind::Enchant {
                    // EntityEnchantmentTableParticleFX.onUpdate (the age was
                    // counted above, before the move as particleAge++ after)
                    let f = 1.0 - (p.age - 1) as f32 / p.max_age as f32;
                    let mut g = 1.0 - f;
                    g *= g;
                    g *= g;
                    for a in 0..3 {
                        p.pos[a] = p.origin[a] + p.motion[a] * f as f64;
                    }
                    p.pos[1] -= (g * 1.2) as f64;
                    return true;
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
                    Kind::SnowShovel => {
                        p.sprite = Sprite::Index(7 - p.age * 8 / p.max_age);
                        p.motion[1] -= 0.03;
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
                    Kind::Drip | Kind::Note | Kind::Heart | Kind::Aura => {}
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
                    Kind::SnowShovel => 0.99,
                    Kind::Crit => 0.7,
                    Kind::Note => 0.66,
                    Kind::Heart => 0.86,
                    Kind::Aura => 0.99,
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
        }
        self.list = lists;
        // EntityFireworkSparkFX: the trail's new sparks, half way through
        for (pos, col, fade, twinkle) in trails {
            let mut q = self.spark(pos, [0.0; 3], false, twinkle);
            q.color = [col[0], col[1], col[2], 1.0];
            q.age = q.max_age / 2;
            if let Kind::Spark { fade: f, .. } = &mut q.kind {
                *f = fade;
            }
            self.add(q);
        }
    }

    /// Camera-facing quads (`EntityFX.renderParticle`), camera-relative.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &self,
        out: &mut Vec<Vertex>,
        blend: &mut Vec<Vertex>,
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
        for p in self.list.iter().flatten() {
            // EntityFireworkSparkFX.renderParticle: twinkling sparks blink
            if let Kind::Spark { twinkle: true, .. } = p.kind {
                if !(p.age < p.max_age / 3 || (p.age + p.max_age) / 3 % 2 == 0) {
                    continue;
                }
            }
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
                Kind::Smoke | Kind::Explode | Kind::SnowShovel => {
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
            let mut alpha = p.color[3];
            let (mut u0, mut v0, mut u1, mut v1) = (u0, v0, u1, v1);
            if p.kind == Kind::FireOverlay {
                // EntityFireworkOverlayFX.renderParticle: a growing flash
                let Some(s) = sheet else { continue };
                let k = 128.0 / s.size;
                let (du, dv) = ((s.rect[2] - s.rect[0]) * k, (s.rect[3] - s.rect[1]) * k);
                u0 = s.rect[0] + 0.25 * du;
                u1 = s.rect[0] + 0.5 * du;
                v0 = s.rect[1] + 0.125 * dv;
                v1 = s.rect[1] + 0.375 * dv;
                let a = p.age as f32 + partial - 1.0;
                size = 7.1 * (a * 0.25 * std::f32::consts::PI).sin();
                alpha = 0.6 - a * 0.25 * 0.5;
                if alpha <= 0.0 {
                    continue;
                }
            }
            let blended = matches!(p.kind, Kind::Spark { .. } | Kind::FireOverlay);
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
            // EntityFireworkSparkFX.getBrightnessForRender: 15728880
            let (sky, blk) = if matches!(p.kind, Kind::Spark { .. }) {
                (15, 15)
            } else {
                (sky, blk)
            };
            let light = [(blk << 4) as u8, (sky << 4) as u8, 0, 0];
            let col = [
                (p.color[0] * 255.0) as u8,
                (p.color[1] * 255.0) as u8,
                (p.color[2] * 255.0) as u8,
                (alpha.clamp(0.0, 1.0) * 255.0) as u8,
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
            let out = if blended { &mut *blend } else { &mut *out };
            out.extend_from_slice(&q);
            out.extend_from_slice(&[q[3], q[2], q[1], q[0]]);
        }
    }

    /// `Block.randomDisplayTick` for the blocks around the player
    /// (`WorldClient.doVoidFogParticles`: 1000 random positions within 16).
    /// `WorldClient.doVoidFogParticles`: the blocks' `randomDisplayTick`,
    /// whose particles all go through `World.spawnParticle`.
    pub fn display_ticks(&mut self, w: &dyn BlockView, eye: [f64; 3]) {
        let was = self.gated;
        self.gated = true;
        self.display_ticks_inner(w, eye);
        self.gated = was;
    }

    fn display_ticks_inner(&mut self, w: &dyn BlockView, eye: [f64; 3]) {
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
                block::ENCHANTING_TABLE => {
                    // BlockEnchantmentTable.randomDisplayTick: glyphs from the
                    // bookshelves with nothing in between
                    for bx in x - 2..=x + 2 {
                        let mut bz = z - 2;
                        while bz <= z + 2 {
                            if bx > x - 2 && bx < x + 2 && bz == z - 1 {
                                bz = z + 2;
                            }
                            if self.r.next_int_n(16) == 0 {
                                for by in y..=y + 1 {
                                    if w.peek_block(bx, by, bz) == block::BOOKSHELF {
                                        let (mx, mz) = ((bx - x) / 2 + x, (bz - z) / 2 + z);
                                        if w.peek_block(mx, by, mz) != block::AIR {
                                            break;
                                        }
                                        let m = [
                                            (bx - x) as f64 + self.r.next_float() as f64 - 0.5,
                                            (by - y) as f64 - self.r.next_float() as f64 - 1.0,
                                            (bz - z) as f64 + self.r.next_float() as f64 - 0.5,
                                        ];
                                        self.enchant([x as f64 + 0.5, y as f64 + 2.0, z as f64 + 0.5], m);
                                    }
                                }
                            }
                            bz += 1;
                        }
                    }
                }
                block::ENDER_CHEST => {
                    // BlockEnderChest.randomDisplayTick
                    for _ in 0..3 {
                        let _ = self.r.next_float();
                        let py = y as f64 + self.r.next_float() as f64;
                        let _ = self.r.next_float();
                        let sx = (self.r.next_int_n(2) * 2 - 1) as f64;
                        let sz = (self.r.next_int_n(2) * 2 - 1) as f64;
                        let _ = self.r.next_float();
                        let my = (self.r.next_float() as f64 - 0.5) * 0.125;
                        let _ = self.r.next_float();
                        let mz = self.r.next_float() as f64 * sz;
                        let mx = self.r.next_float() as f64 * sx;
                        let pos = [x as f64 + 0.5 + 0.25 * sx, py, z as f64 + 0.5 + 0.25 * sz];
                        self.portal(pos, [mx, my, mz]);
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
        self.list.iter().map(|l| l.len()).sum()
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `RenderGlobal.doSpawnParticle`: all, decreased (about two in three),
    /// minimal (none), and nothing past 16 blocks; particles added
    /// directly (block debris) are never held back.
    #[test]
    fn particle_setting_and_distance() {
        let mut p = Particles::default();
        let count = |p: &mut Particles, setting: i32, at: [f64; 3]| {
            p.list = Default::default();
            p.setting = setting;
            for _ in 0..300 {
                p.spawned(|ps| ps.smoke(at, [0.0; 3], false));
            }
            p.count()
        };
        let near = [3.0, 0.0, 4.0];
        assert_eq!(count(&mut p, 0, near), 300);
        let some = count(&mut p, 1, near);
        assert!((170..240).contains(&some), "decreased: {some}");
        assert_eq!(count(&mut p, 2, near), 0);
        assert_eq!(count(&mut p, 0, [17.0, 0.0, 0.0]), 0);
        p.list = Default::default();
        p.setting = 2;
        p.smoke(near, [0.0; 3], false);
        assert_eq!(p.count(), 1, "not spawned through the world: always made");
    }
}

/// One explosion of a firework (`NBTTagCompound` "Explosion").
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Explosion {
    pub shape: i32,
    pub trail: bool,
    pub flicker: bool,
    pub colors: Vec<i32>,
    pub fades: Vec<i32>,
}

impl Explosion {
    pub fn from_nbt(t: &crate::world::anvil::Nbt) -> Explosion {
        use crate::world::anvil::Nbt;
        let ints = |k: &str| match t.get(k) {
            Some(Nbt::IntArray(v)) => v.clone(),
            _ => Vec::new(),
        };
        Explosion {
            shape: t.int("Type").unwrap_or(0) as i32,
            trail: t.int("Trail").unwrap_or(0) != 0,
            flicker: t.int("Flicker").unwrap_or(0) != 0,
            colors: ints("Colors"),
            fades: ints("FadeColors"),
        }
    }

    /// The "Explosions" of a rocket's "Fireworks" tag.
    pub fn of_rocket(s: Option<crate::item::Stack>) -> Vec<Explosion> {
        let Some(t) = s.and_then(|s| s.tag.get()) else {
            return Vec::new();
        };
        match t.get("Fireworks") {
            Some(f) => f
                .list("Explosions")
                .iter()
                .map(Explosion::from_nbt)
                .collect(),
            None => Vec::new(),
        }
    }
}

/// `EntityFireworkStarterFX`: draws nothing, lets the explosions off two
/// ticks apart and asks for the bangs.
pub struct FireworkStarter {
    pos: [f64; 3],
    motion: [f64; 3],
    explosions: Vec<Explosion>,
    age: i32,
    max_age: i32,
    twinkle: bool,
}

impl FireworkStarter {
    pub fn new(pos: [f64; 3], motion: [f64; 3], explosions: Vec<Explosion>) -> FireworkStarter {
        let mut max_age = 8;
        let mut twinkle = false;
        if !explosions.is_empty() {
            max_age = explosions.len() as i32 * 2 - 1;
            if explosions.iter().any(|e| e.flicker) {
                twinkle = true;
                max_age += 15;
            }
        }
        FireworkStarter {
            pos,
            motion,
            explosions,
            age: 0,
            max_age,
            twinkle,
        }
    }

    /// One tick: false once done. Sounds come back as (name, pos, volume, pitch).
    pub fn tick(
        &mut self,
        ps: &mut Particles,
        camera: [f64; 3],
        sounds: &mut Vec<(String, [f64; 3], f32, f32)>,
    ) -> bool {
        let d2 = (camera[0] - self.pos[0]).powi(2)
            + (camera[1] - self.pos[1]).powi(2)
            + (camera[2] - self.pos[2]).powi(2);
        let far = d2 >= 256.0;
        if self.age == 0 && !self.explosions.is_empty() {
            let large = self.explosions.len() >= 3 || self.explosions.iter().any(|e| e.shape == 1);
            let name = format!(
                "fireworks.{}{}",
                if large { "largeBlast" } else { "blast" },
                if far { "_far" } else { "" }
            );
            let pitch = 0.95 + ps.r.next_float() * 0.1;
            sounds.push((name, self.pos, 20.0, pitch));
        }
        if self.age % 2 == 0 && ((self.age / 2) as usize) < self.explosions.len() {
            let e = self.explosions[(self.age / 2) as usize].clone();
            match e.shape {
                1 => self.ball(ps, 0.5, 4, &e),
                2 => self.shaped(
                    ps,
                    0.5,
                    &[
                        [0.0, 1.0],
                        [0.3455, 0.309],
                        [0.9511, 0.309],
                        [0.3795918367346939, -0.12653061224489795],
                        [0.6122448979591837, -0.8040816326530612],
                        [0.0, -0.35918367346938773],
                    ],
                    &e,
                    false,
                ),
                3 => self.shaped(
                    ps,
                    0.5,
                    &[
                        [0.0, 0.2],
                        [0.2, 0.2],
                        [0.2, 0.6],
                        [0.6, 0.6],
                        [0.6, 0.2],
                        [0.2, 0.2],
                        [0.2, 0.0],
                        [0.4, 0.0],
                        [0.4, -0.6],
                        [0.2, -0.6],
                        [0.2, -0.4],
                        [0.0, -0.4],
                    ],
                    &e,
                    true,
                ),
                4 => self.burst(ps, &e),
                _ => self.ball(ps, 0.25, 2, &e),
            }
            // EntityFireworkOverlayFX in the first colour
            let c = e.colors.first().copied().unwrap_or(0);
            let mut o = ps.base(self.pos);
            o.max_age = 4;
            o.color = [
                ((c & 0xFF0000) >> 16) as f32 / 255.0,
                ((c & 0xFF00) >> 8) as f32 / 255.0,
                (c & 0xFF) as f32 / 255.0,
                1.0,
            ];
            o.kind = Kind::FireOverlay;
            ps.add(o);
        }
        self.age += 1;
        if self.age > self.max_age {
            if self.twinkle {
                let name = if far {
                    "fireworks.twinkle_far"
                } else {
                    "fireworks.twinkle"
                };
                let pitch = 0.9 + ps.r.next_float() * 0.15;
                sounds.push((name.to_string(), self.pos, 20.0, pitch));
            }
            return false;
        }
        true
    }

    fn particle(&self, ps: &mut Particles, m: [f64; 3], e: &Explosion) {
        let mut p = ps.spark(self.pos, m, e.trail, e.flicker);
        if !e.colors.is_empty() {
            let c = e.colors[ps.r.next_int_n(e.colors.len() as i32) as usize];
            p.color = [
                ((c & 0xFF0000) >> 16) as f32 / 255.0,
                ((c & 0xFF00) >> 8) as f32 / 255.0,
                (c & 0xFF) as f32 / 255.0,
                1.0,
            ];
        }
        if !e.fades.is_empty() {
            let c = e.fades[ps.r.next_int_n(e.fades.len() as i32) as usize];
            if let Kind::Spark { fade, .. } = &mut p.kind {
                *fade = Some([
                    ((c & 0xFF0000) >> 16) as f32 / 255.0,
                    ((c & 0xFF00) >> 8) as f32 / 255.0,
                    (c & 0xFF) as f32 / 255.0,
                ]);
            }
        }
        ps.add(p);
    }

    /// `createBall`
    fn ball(&self, ps: &mut Particles, speed: f64, size: i32, e: &Explosion) {
        let mut y = -size;
        while y <= size {
            let mut x = -size;
            while x <= size {
                let mut z = -size;
                while z <= size {
                    let dx = x as f64 + (ps.r.next_double() - ps.r.next_double()) * 0.5;
                    let dy = y as f64 + (ps.r.next_double() - ps.r.next_double()) * 0.5;
                    let dz = z as f64 + (ps.r.next_double() - ps.r.next_double()) * 0.5;
                    let l = math::sqrt_double(dx * dx + dy * dy + dz * dz) as f64 / speed
                        + ps.r.next_gaussian() * 0.05;
                    self.particle(ps, [dx / l, dy / l, dz / l], e);
                    if y != -size && y != size && x != -size && x != size {
                        z += size * 2 - 1;
                    }
                    z += 1;
                }
                x += 1;
            }
            y += 1;
        }
    }

    /// `createShaped`
    fn shaped(
        &self,
        ps: &mut Particles,
        speed: f64,
        pts: &[[f64; 2]],
        e: &Explosion,
        creeper: bool,
    ) {
        let (x0, y0) = (pts[0][0], pts[0][1]);
        self.particle(ps, [x0 * speed, y0 * speed, 0.0], e);
        let a0 = ps.r.next_float() * std::f32::consts::PI;
        let step = if creeper { 0.034 } else { 0.34 };
        for k in 0..3 {
            let a = a0 as f64 + (k as f32 * std::f32::consts::PI) as f64 * step;
            let (mut px, mut py) = (x0, y0);
            for q in &pts[1..] {
                let mut t = 0.25;
                while t <= 1.0 {
                    let mut vx = (px + (q[0] - px) * t) * speed;
                    let vy = (py + (q[1] - py) * t) * speed;
                    let vz = vx * a.sin();
                    vx *= a.cos();
                    let mut s = -1.0;
                    while s <= 1.0 {
                        self.particle(ps, [vx * s, vy, vz * s], e);
                        s += 2.0;
                    }
                    t += 0.25;
                }
                px = q[0];
                py = q[1];
            }
        }
    }

    /// `createBurst`
    fn burst(&self, ps: &mut Particles, e: &Explosion) {
        let gx = ps.r.next_gaussian() * 0.05;
        let gz = ps.r.next_gaussian() * 0.05;
        for _ in 0..70 {
            let mx = self.motion[0] * 0.5 + ps.r.next_gaussian() * 0.15 + gx;
            let mz = self.motion[2] * 0.5 + ps.r.next_gaussian() * 0.15 + gz;
            let my = self.motion[1] * 0.5 + ps.r.next_double() * 0.5;
            self.particle(ps, [mx, my, mz], e);
        }
    }
}
