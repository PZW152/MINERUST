//! `EntityFallingBlock`: sand, gravel (and later anvils) falling as entities.

use super::base::{Ent, Fx};
use crate::block::{self, Material};
use crate::item::Stack;
use crate::util::math;
use crate::world::{BlockView, ServerWorld};

#[derive(Clone, Debug)]
pub struct FallingBlock {
    pub id: u32,
    pub e: Ent,
    /// `blockObj` / `metadata`
    pub block: u8,
    pub meta: i32,
    pub fall_time: i32,
    /// `shouldDropItem`
    pub drop_item: bool,
    /// `canSetAsBlock` (an anvil worn out by its fall)
    pub broken: bool,
    pub hurt_entities: bool,
    pub fall_hurt_max: i32,
    pub fall_hurt_amount: f32,
    /// Damage to deal this tick to the entities in a box (`DamageSource.anvil`
    /// or `fallingBlock`).
    pub hurt: Option<(super::player::Aabb, f32)>,
}

/// Snapshot for the client renderer (`RenderFallingBlock`).
#[derive(Clone, Copy, Debug)]
pub struct FallingView {
    pub pos: [f64; 3],
    pub prev: [f64; 3],
    pub block: u8,
    pub meta: u8,
}

impl FallingBlock {
    /// `new EntityFallingBlock(world, x, y, z, block, meta)`
    pub fn new(id: u32, pos: [f64; 3], block: u8, meta: i32) -> FallingBlock {
        let mut e = Ent::new(0.98, 0.98, super::item::entity_seed(id));
        e.y_offset = e.height / 2.0;
        e.can_trigger_walking = false;
        e.set_position(pos[0], pos[1], pos[2]);
        e.motion = [0.0; 3];
        e.prev = pos;
        e.last_tick = pos;
        FallingBlock {
            id,
            e,
            block,
            meta,
            fall_time: 0,
            drop_item: true,
            broken: false,
            // readEntityFromNBT turns this on for anvils; BlockAnvil sets it.
            hurt_entities: block == 145,
            fall_hurt_max: 40,
            fall_hurt_amount: 2.0,
            hurt: None,
        }
    }

    pub fn view(&self) -> FallingView {
        FallingView {
            pos: self.e.pos,
            prev: self.e.prev,
            block: self.block,
            meta: self.meta as u8,
        }
    }

    /// `Block.damageDropped`
    fn damage_dropped(&self) -> i32 {
        match self.block {
            block::SAND => self.meta,
            // BlockAnvil: the damage bits
            145 => self.meta >> 2,
            _ => 0,
        }
    }

    /// `EntityFallingBlock.onUpdate`. Items it drops go to `drops`.
    pub fn tick(&mut self, w: &mut ServerWorld, drops: &mut Vec<([f64; 3], Stack)>, fx: &mut Vec<Fx>) {
        if block::material(self.block) == Material::Air {
            self.e.dead = true;
            return;
        }
        self.e.ticks_existed += 1;
        self.e.prev = self.e.pos;
        self.fall_time += 1;
        self.e.motion[1] -= 0.04f32 as f64;
        let m = self.e.motion;
        let mut f = Vec::new();
        self.e
            .move_entity(&*w as &dyn BlockView, m[0], m[1], m[2], false, &[], &mut f);
        for x in f {
            match x {
                Fx::Fall(d) => self.fall(d),
                Fx::Sound(..) | Fx::Particle(..) => fx.push(x),
                // EntityFallingBlock does not override attackEntityFrom:
                // Entity.attackEntityFrom only marks it as attacked.
                Fx::Collide(x, y, z, id) => {
                    let _ = super::collide::collided(w, &mut self.e, x, y, z, id);
                }
                _ => {}
            }
        }
        self.e.motion[0] *= 0.98f32 as f64;
        self.e.motion[1] *= 0.98f32 as f64;
        self.e.motion[2] *= 0.98f32 as f64;
        let (x, y, z) = (
            math::floor_double(self.e.pos[0]),
            math::floor_double(self.e.pos[1]),
            math::floor_double(self.e.pos[2]),
        );
        if self.fall_time == 1 {
            if w.block(x, y, z) != self.block {
                self.e.dead = true;
                return;
            }
            w.set_air(x, y, z);
        }
        if self.e.on_ground {
            self.e.motion[0] *= 0.7f32 as f64;
            self.e.motion[2] *= 0.7f32 as f64;
            self.e.motion[1] *= -0.5;
            if w.block(x, y, z) != block::PISTON_EXTENSION {
                self.e.dead = true;
                // World.canPlaceEntityOnSide(block, x, y, z, true, 1, null, null)
                let here = w.block(x, y, z);
                let placeable = (block::material(here) == Material::Circuits && self.block == 145)
                    || (block::material(here).replaceable() && w.can_place_block_at(self.block, x, y, z));
                if !self.broken
                    && placeable
                    && !w.can_fall_below(x, y - 1, z)
                    && w.set_block(x, y, z, self.block, self.meta, 3)
                {
                    // BlockAnvil.func_149828_a: the landing sound
                    if self.block == 145 {
                        w.redstone_effects.push(crate::world::redstone::Effect::Sound(
                            "random.anvil_land",
                            [x as f64 + 0.5, y as f64 + 0.5, z as f64 + 0.5],
                            0.3,
                            w.rand.next_float() * 0.1 + 0.9,
                        ));
                    }
                } else if self.drop_item && !self.broken {
                    drops.push((self.e.pos, Stack::new(self.block as u16, self.damage_dropped(), 1)));
                }
            }
        } else if (self.fall_time > 100 && !(1..=256).contains(&y)) || self.fall_time > 600 {
            if self.drop_item {
                drops.push((self.e.pos, Stack::new(self.block as u16, self.damage_dropped(), 1)));
            }
            self.e.dead = true;
        }
    }

    /// `EntityFallingBlock.fall`: anvils hurt what they land on and wear.
    fn fall(&mut self, d: f32) {
        if self.hurt_entities {
            let n = math::ceiling_float_int(d - 1.0);
            if n > 0 {
                // getEntitiesWithinAABBExcludingEntity(this, boundingBox) take
                // min(floor(n * fallHurtAmount), fallHurtMax)
                let amount = (math::floor_float(n as f32 * self.fall_hurt_amount)).min(self.fall_hurt_max);
                self.hurt = Some((self.e.bb, amount as f32));
            }
            if n > 0
                && self.block == 145
                && (self.e.rand.next_float() as f64) < 0.05f32 as f64 + n as f64 * 0.05
            {
                let mut dmg = self.meta >> 2;
                let dir = self.meta & 3;
                dmg += 1;
                if dmg > 2 {
                    self.broken = true;
                } else {
                    self.meta = dir | dmg << 2;
                }
            }
        }
    }
}
