//! `EntityItem`: dropped item stacks.

use super::base::{Ent, Fx, Source};
use super::player::Aabb;
use crate::block::{self, Material};
use crate::item::Stack;
use crate::java::JavaRandom;
use crate::util::math;
use crate::world::BlockView;

#[derive(Clone, Debug)]
pub struct ItemEntity {
    pub id: u32,
    pub stack: Stack,
    pub e: Ent,
    pub age: i32,
    /// `delayBeforeCanPickup`
    pub pickup_delay: i32,
    /// `EntityItem.health`
    pub health: i32,
    /// `hoverStart`: random phase of the bobbing animation.
    pub hover: f32,
}

impl ItemEntity {
    /// `Block.dropBlockAsItem_do`: spawned inside the broken block.
    pub fn from_block(
        id: u32,
        stack: Stack,
        x: i32,
        y: i32,
        z: i32,
        r: &mut JavaRandom,
    ) -> ItemEntity {
        let f = 0.7f32;
        let dx = (r.next_float() * f) as f64 + (1.0 - f) as f64 * 0.5;
        let dy = (r.next_float() * f) as f64 + (1.0 - f) as f64 * 0.5;
        let dz = (r.next_float() * f) as f64 + (1.0 - f) as f64 * 0.5;
        let mut e = Self::new(id, stack, [x as f64 + dx, y as f64 + dy, z as f64 + dz]);
        e.pickup_delay = 10;
        e
    }

    /// `new EntityItem(world, x, y, z, stack)`; its `Math.random()` draws
    /// leave every world and entity generator alone.
    pub fn new(id: u32, stack: Stack, pos: [f64; 3]) -> ItemEntity {
        use crate::util::math::math_random;
        let hover = (math_random() * std::f64::consts::PI * 2.0) as f32;
        let mut e = Ent::new(0.25, 0.25, entity_seed(id));
        e.y_offset = e.height / 2.0;
        e.can_trigger_walking = false;
        e.set_position(pos[0], pos[1], pos[2]);
        e.yaw = (math_random() * 360.0) as f32;
        e.motion = [
            (math_random() * 0.2f32 as f64 - 0.1f32 as f64) as f32 as f64,
            0.2f32 as f64,
            (math_random() * 0.2f32 as f64 - 0.1f32 as f64) as f32 as f64,
        ];
        e.prev = e.pos;
        e.last_tick = e.pos;
        ItemEntity {
            id,
            stack,
            e,
            age: 0,
            pickup_delay: 0,
            health: 5,
            hover,
        }
    }

    pub fn pos(&self) -> [f64; 3] {
        self.e.pos
    }

    pub fn bb(&self) -> Aabb {
        self.e.bb
    }

    /// `EntityItem.attackEntityFrom`
    pub fn attack(&mut self, src: Option<Source>, explosion: bool, amount: f32) {
        let _ = src;
        if self.stack.id == crate::item::NETHER_STAR && explosion {
            return;
        }
        self.health = (self.health as f32 - amount) as i32;
        if self.health <= 0 {
            self.e.dead = true;
        }
    }

    fn handle(&mut self, w: &mut crate::world::ServerWorld, fx: Vec<Fx>, out: &mut Vec<Fx>) {
        for f in fx {
            match f {
                Fx::Damage(s, a) => self.attack(Some(s), false, a),
                Fx::Kill => self.e.dead = true,
                Fx::Collide(x, y, z, id) => {
                    if let Some((s, a)) = super::collide::collided(w, &mut self.e, x, y, z, id) {
                        self.attack(Some(s), false, a);
                    }
                }
                Fx::Sound(..) | Fx::Particle(..) => out.push(f),
                Fx::Fall(_) | Fx::Walk(..) | Fx::BrokenItem(_) => {}
            }
        }
    }

    /// `EntityItem.onUpdate` up to `searchForOtherItemsNearby`; returns
    /// whether that search runs this tick.
    fn update(&mut self, w: &mut crate::world::ServerWorld, out: &mut Vec<Fx>) -> bool {
        self.e.ticks_existed += 1;
        let mut fx = Vec::new();
        // Entity.onUpdate -> onEntityUpdate; EntityItem.handleWaterMovement
        // only applies the current (inWater stays false).
        {
            let e = &mut self.e;
            e.prev_dist_walked_mod = e.dist_walked_mod;
            e.prev = e.pos;
            e.prev_pitch = e.pitch;
            e.prev_yaw = e.yaw;
            let bb = e.bb;
            e.handle_material_acceleration(&*w as &dyn BlockView, &bb, Material::Water);
            if e.fire > 0 {
                if e.fire % 20 == 0 {
                    fx.push(Fx::Damage(Source::OnFire, 1.0));
                }
                e.fire -= 1;
            }
            if e.handle_lava_movement(&*w as &dyn BlockView) {
                fx.push(Fx::Damage(Source::Lava, 4.0));
                e.set_fire(15);
                e.fall_distance *= 0.5;
            }
            if e.pos[1] < -64.0 {
                fx.push(Fx::Kill);
            }
            e.first_update = false;
        }
        self.handle(w, std::mem::take(&mut fx), out);
        if self.pickup_delay > 0 {
            self.pickup_delay -= 1;
        }
        let e = &mut self.e;
        e.prev = e.pos;
        e.motion[1] -= 0.04f32 as f64;
        let cy = (e.bb.min[1] + e.bb.max[1]) / 2.0;
        let (px, pz) = (e.pos[0], e.pos[2]);
        e.no_clip = e.push_out_of_blocks(&*w as &dyn BlockView, px, cy, pz);
        let m = e.motion;
        e.move_entity(&*w as &dyn BlockView, m[0], m[1], m[2], false, &[], &mut fx);
        self.handle(w, std::mem::take(&mut fx), out);
        let e = &mut self.e;
        let moved = e.prev[0] as i32 != e.pos[0] as i32
            || e.prev[1] as i32 != e.pos[1] as i32
            || e.prev[2] as i32 != e.pos[2] as i32;
        let search = moved || e.ticks_existed % 25 == 0;
        if search {
            let (bx, by, bz) = (
                math::floor_double(e.pos[0]),
                math::floor_double(e.pos[1]),
                math::floor_double(e.pos[2]),
            );
            if block::material(w.peek_block(bx, by, bz)) == Material::Lava {
                e.motion[1] = 0.2f32 as f64;
                e.motion[0] = ((e.rand.next_float() - e.rand.next_float()) * 0.2) as f64;
                e.motion[2] = ((e.rand.next_float() - e.rand.next_float()) * 0.2) as f64;
                let p = 2.0 + e.rand.next_float() * 0.4;
                out.push(Fx::Sound("random.fizz", e.pos, 0.4, p));
            }
        }
        search
    }

    /// The rest of `EntityItem.onUpdate` after the merge search.
    fn finish(&mut self, w: &dyn BlockView) {
        let e = &mut self.e;
        let mut f = 0.98f32;
        if e.on_ground {
            let below = w.peek_block(
                math::floor_double(e.pos[0]),
                math::floor_double(e.bb.min[1]) - 1,
                math::floor_double(e.pos[2]),
            );
            f = block::info(below).slipperiness * 0.98;
        }
        e.motion[0] *= f as f64;
        e.motion[1] *= 0.98f32 as f64;
        e.motion[2] *= f as f64;
        if e.on_ground {
            e.motion[1] *= -0.5;
        }
        self.age += 1;
        if self.age >= 6000 {
            e.dead = true;
        }
    }
}

/// `new Random()` for an entity, reproducible from its id.
pub fn entity_seed(id: u32) -> i64 {
    (id as i64)
        .wrapping_mul(0x5DEECE66D)
        .wrapping_add(0x2545F491)
}

/// `EntityItem.combineItems`: `i` tries to merge into `j`.
fn combine(items: &mut [ItemEntity], i: usize, j: usize) -> bool {
    if i == j || items[i].e.dead || items[j].e.dead {
        return false;
    }
    let (a, b) = (items[i].stack, items[j].stack);
    if b.id != a.id || b.tag != a.tag {
        return false;
    }
    if crate::item::has_subtypes(b.id) && b.meta != a.meta {
        return false;
    }
    if b.count < a.count {
        return combine(items, j, i);
    }
    if b.count + a.count > crate::item::max_stack(b.id) {
        return false;
    }
    items[j].stack.count = b.count + a.count;
    items[j].pickup_delay = items[j].pickup_delay.max(items[i].pickup_delay);
    items[j].age = items[j].age.min(items[i].age);
    items[i].e.dead = true;
    true
}

/// `World.updateEntities` for the dropped items: each one's `onUpdate`, the
/// merge search included, in list order. Items in unloaded chunks wait.
pub fn tick_all(items: &mut Vec<ItemEntity>, w: &mut crate::world::ServerWorld, out: &mut Vec<Fx>) {
    for i in 0..items.len() {
        if items[i].e.dead {
            continue;
        }
        let p = items[i].e.pos;
        let (bx, bz) = (math::floor_double(p[0]), math::floor_double(p[2]));
        // updateEntityWithOptionalForce: a 32-block margin must be loaded.
        if !super::base::chunks_exist(
            &*w as &dyn BlockView,
            bx - 32,
            0,
            bz - 32,
            bx + 32,
            0,
            bz + 32,
        ) {
            continue;
        }
        if items[i].update(w, out) {
            // searchForOtherItemsNearby: bb.expand(0.5, 0, 0.5)
            let bb = Ent::expand(&items[i].e.bb, 0.5, 0.0, 0.5);
            for j in 0..items.len() {
                if !items[j].e.dead && items[j].e.bb.intersects(&bb) {
                    combine(items, i, j);
                }
            }
        }
        if !items[i].e.dead {
            items[i].finish(&*w as &dyn BlockView);
        }
    }
    items.retain(|e| !e.e.dead);
}
