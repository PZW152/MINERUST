//! `ItemBucket` and `ItemMonsterPlacer` on the server.

use super::*;
use crate::entity::raytrace::{ray_trace_blocks, BlockHit};
use crate::item::Stack;
use crate::util::math;

/// `Items.bucket`, `water_bucket`, `lava_bucket`, `spawn_egg`
pub const BUCKET: crate::item::ItemId = 325;
pub const WATER_BUCKET: crate::item::ItemId = 326;
pub const LAVA_BUCKET: crate::item::ItemId = 327;
pub const SPAWN_EGG: crate::item::ItemId = 383;

/// `ItemBucket.tryPlaceContainedLiquid`: `liquid` is the flowing block.
/// The fizz of water in the Nether comes back as a sound and particles.
pub fn try_place_contained_liquid(
    w: &mut ServerWorld,
    r: &mut JavaRandom,
    liquid: u8,
    x: i32,
    y: i32,
    z: i32,
    hell: bool,
    fx: &mut Vec<crate::entity::base::Fx>,
) -> bool {
    use crate::entity::base::Fx;
    let m = block::material(w.block(x, y, z));
    let not_solid = !m.is_solid();
    if m != block::Material::Air && !not_solid {
        return false;
    }
    if hell && liquid == block::FLOWING_WATER {
        let c = [x as f64 + 0.5, y as f64 + 0.5, z as f64 + 0.5];
        let pitch = 2.6 + (r.next_float() - r.next_float()) * 0.8;
        fx.push(Fx::Sound("random.fizz", c, 0.5, pitch));
        for _ in 0..8 {
            let p = [
                x as f64 + math::math_random(),
                y as f64 + math::math_random(),
                z as f64 + math::math_random(),
            ];
            fx.push(Fx::Particle("largesmoke", p, [0.0; 3]));
        }
    } else {
        if not_solid && !m.is_liquid() {
            // World.func_147480_a(x, y, z, true): break with drops
            w.destroy_block_with(r, x, y, z);
        }
        w.set_block(x, y, z, liquid, 0, 3);
    }
    true
}

impl Sim {
    /// `Item.getMovingObjectPositionFromPlayer`: 5 blocks from the eyes.
    fn item_ray(&self, liquids: bool) -> Option<BlockHit> {
        let (feet, yaw) = self.play.player;
        let pitch = self.play.pitch;
        // the server player's yOffset is 0
        let a = [feet[0], feet[1] + 1.62, feet[2]];
        let r = std::f32::consts::PI / 180.0;
        let var14 = math::cos(-yaw * r - std::f32::consts::PI);
        let var15 = math::sin(-yaw * r - std::f32::consts::PI);
        let var16 = -math::cos(-pitch * r);
        let var17 = math::sin(-pitch * r);
        let (dx, dy, dz) = ((var15 * var16) as f64, var17 as f64, (var14 * var16) as f64);
        let b = [a[0] + dx * 5.0, a[1] + dy * 5.0, a[2] + dz * 5.0];
        ray_trace_blocks(&self.world, a, b, liquids, !liquids, false)
    }

    /// `ItemBucket.onItemRightClick`
    pub(super) fn use_bucket(&mut self, held: Stack) {
        let empty = held.id == BUCKET;
        let Some(h) = self.item_ray(empty) else {
            return;
        };
        let creative = self.play.mode == GameMode::Creative;
        let sel = self.play.inv.selected;
        let (mut x, mut y, mut z) = (h.x, h.y, h.z);
        if empty {
            let m = block::material(self.world.block(x, y, z));
            let meta = self.world.meta(x, y, z);
            let full = if m == block::Material::Water && meta == 0 {
                WATER_BUCKET
            } else if m == block::Material::Lava && meta == 0 {
                LAVA_BUCKET
            } else {
                return;
            };
            self.world.set_block(x, y, z, block::AIR, 0, 3);
            // fillBucket
            if creative {
                return;
            }
            let mut s = held;
            s.count -= 1;
            if s.count == 0 {
                self.play.inv.main[sel] = Some(Stack::new(full, 0, 1));
            } else {
                self.play.inv.main[sel] = Some(s);
                let mut f = Stack::new(full, 0, 1);
                if !self.play.inv.add_stack(&mut f, false) || f.count > 0 {
                    let mut r = std::mem::replace(&mut self.play.prand, JavaRandom::new(0));
                    self.play.throw_with(Stack::new(full, 0, 1), false, &mut r);
                    self.play.prand = r;
                }
            }
            self.play.inv_dirty = true;
        } else {
            match h.side {
                0 => y -= 1,
                1 => y += 1,
                2 => z -= 1,
                3 => z += 1,
                4 => x -= 1,
                5 => x += 1,
                _ => {}
            }
            let liquid = if held.id == WATER_BUCKET {
                block::FLOWING_WATER
            } else {
                block::FLOWING_LAVA
            };
            let mut fx = Vec::new();
            let hell = self.dim == -1;
            let mut r = std::mem::replace(&mut self.world.rand, JavaRandom::new(0));
            let placed =
                try_place_contained_liquid(&mut self.world, &mut r, liquid, x, y, z, hell, &mut fx);
            self.world.rand = r;
            if placed && !creative {
                self.play.inv.main[sel] = Some(Stack::new(BUCKET, 0, 1));
                self.play.inv_dirty = true;
            }
            self.play.fx.extend(fx);
        }
    }

    /// `ItemMonsterPlacer.spawnCreature`: the yaw comes from the world's
    /// random, then `onSpawnWithEgg` and `playLivingSound`.
    /// A renamed egg names the mob (`setCustomNameTag`).
    pub(super) fn spawn_creature(&mut self, egg: &Stack, pos: [f64; 3]) -> bool {
        let mut rnd = std::mem::replace(&mut self.world.rand, JavaRandom::new(0));
        let ok = self.spawn_creature_with(egg, pos, &mut rnd);
        self.world.rand = rnd;
        ok
    }

    /// `spawn_creature` with the world's random lent out by the caller.
    pub(super) fn spawn_creature_with(
        &mut self,
        egg: &Stack,
        pos: [f64; 3],
        rnd: &mut JavaRandom,
    ) -> bool {
        let Some(e) = crate::item_data::EGGS.iter().find(|e| e.0 == egg.meta) else {
            return false;
        };
        let Some(kind) = crate::entity::mob::Kind::from_id(e.1) else {
            return false;
        };
        let yaw = math::wrap_angle_180(rnd.next_float() * 360.0);
        let id = self.mobs.spawn_in(&mut self.world, kind, pos, yaw, rnd);
        if let Some(m) = self.mobs.list.iter_mut().find(|m| m.id == id) {
            if let Some(n) = egg.custom_name() {
                m.custom_name = n;
            }
            if let Some(s) = m.living_sound() {
                self.pending_out.push(crate::entity::mob::Outcome {
                    sounds: vec![s],
                    ..Default::default()
                });
            }
        }
        true
    }

    /// `ItemMonsterPlacer.onItemUse` on side `face` of block `target`.
    pub(super) fn use_egg_on(&mut self, held: Stack, target: [i32; 3], face: [i32; 3]) {
        let id = self.world.block(target[0], target[1], target[2]);
        // a fence (render type 11) under the top face lifts the mob by 0.5
        let lift = if face == [0, 1, 0] && block::info(id).shape == block::Shape::Fence {
            0.5
        } else {
            0.0
        };
        let p = [
            (target[0] + face[0]) as f64 + 0.5,
            (target[1] + face[1]) as f64 + lift,
            (target[2] + face[2]) as f64 + 0.5,
        ];
        self.egg_spawned(held, p);
    }

    /// `ItemMonsterPlacer.onItemRightClick`: into a liquid in view.
    pub(super) fn use_egg_air(&mut self, held: Stack) {
        let Some(h) = self.item_ray(true) else {
            return;
        };
        if block::material(self.world.block(h.x, h.y, h.z)).is_liquid() {
            self.egg_spawned(held, [h.x as f64, h.y as f64, h.z as f64]);
        }
    }

    fn egg_spawned(&mut self, held: Stack, p: [f64; 3]) {
        if self.spawn_creature(&held, p) && self.play.mode != GameMode::Creative {
            self.play.inv.consume_held();
            self.play.inv_dirty = true;
        }
    }
}

impl Sim {
    /// `TileEntityDaylightDetector.updateEntity`: every 20 ticks,
    /// `BlockDaylightDetector.updateOutputState` (no sky in the Nether).
    pub(super) fn tick_daylight_detectors(&mut self) {
        if self.world.total_time % 20 != 0 || self.dim == -1 {
            return;
        }
        let ps: Vec<[i32; 3]> = self.world.daylight_detectors.iter().copied().collect();
        for [x, y, z] in ps {
            if !self.world.chunk_exists(x >> 4, z >> 4)
                || self.world.block(x, y, z) != block::DAYLIGHT_DETECTOR
            {
                continue;
            }
            let m = self.world.meta(x, y, z) as i32;
            let sky = self.world.saved_light(true, x, y, z) - self.world.skylight_subtracted;
            // getCelestialAngleRadians(1.0F); the End's provider gives 0
            let mut a = if self.dim == 1 {
                0.0
            } else {
                super::celestial_angle(self.world_time, 1.0) * std::f32::consts::PI * 2.0
            };
            if a < std::f32::consts::PI {
                a += (0.0 - a) * 0.2;
            } else {
                a += (std::f32::consts::PI * 2.0 - a) * 0.2;
            }
            // Math.round(float)
            let v = ((sky as f32 * math::cos(a)) + 0.5).floor() as i32;
            let v = v.clamp(0, 15);
            if m != v {
                self.world.set_meta(x, y, z, v as _, 3);
            }
        }
    }
}
