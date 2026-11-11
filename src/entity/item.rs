//! `EntityItem`: dropped item stacks.

use super::player::{colliding_boxes, Aabb};
use crate::block;
use crate::item::Stack;
use crate::java::JavaRandom;
use crate::util::math;
use crate::world::BlockView;

const SIZE: f64 = 0.25;

#[derive(Clone, Debug)]
pub struct ItemEntity {
    pub id: u32,
    pub stack: Stack,
    pub pos: [f64; 3],
    pub prev: [f64; 3],
    pub motion: [f64; 3],
    pub on_ground: bool,
    pub age: i32,
    /// `delayBeforeCanPickup`
    pub pickup_delay: i32,
    /// `hoverStart`: random phase of the bobbing animation.
    pub hover: f32,
}

impl ItemEntity {
    /// `Block.dropBlockAsItem_do`: spawned inside the broken block.
    pub fn from_block(id: u32, stack: Stack, x: i32, y: i32, z: i32, r: &mut JavaRandom) -> ItemEntity {
        let f = 0.7f32;
        let dx = (r.next_float() * f) as f64 + (1.0 - f) as f64 * 0.5;
        let dy = (r.next_float() * f) as f64 + (1.0 - f) as f64 * 0.5;
        let dz = (r.next_float() * f) as f64 + (1.0 - f) as f64 * 0.5;
        Self::new(id, stack, [x as f64 + dx, y as f64 + dy, z as f64 + dz], r)
    }

    /// `new EntityItem(world, x, y, z, stack)`
    pub fn new(id: u32, stack: Stack, pos: [f64; 3], r: &mut JavaRandom) -> ItemEntity {
        // The motion uses Math.random(); a world-owned generator keeps it deterministic.
        let _yaw = r.next_double();
        let mx = (r.next_double() * 0.2 - 0.1) as f32 as f64;
        let mz = (r.next_double() * 0.2 - 0.1) as f32 as f64;
        let hover = (r.next_double() * std::f64::consts::PI * 2.0) as f32;
        ItemEntity { id, stack, pos, prev: pos, motion: [mx, 0.2f32 as f64, mz], on_ground: false, age: 0, pickup_delay: 10, hover }
    }

    pub fn bb(&self) -> Aabb {
        let h = SIZE / 2.0;
        // yOffset = height / 2: the box is centred on the position.
        Aabb { min: [self.pos[0] - h, self.pos[1] - h, self.pos[2] - h], max: [self.pos[0] + h, self.pos[1] + h, self.pos[2] + h] }
    }

    /// `Entity.func_145771_j`: pushes an item that is inside a solid block
    /// towards the nearest open side.
    fn push_out(&mut self, world: &dyn BlockView) {
        let (x, y, z) = (math::floor_double(self.pos[0]), math::floor_double(self.pos[1]), math::floor_double(self.pos[2]));
        let solid = |x: i32, y: i32, z: i32| block::info(world.peek_block(x, y, z)).normal_cube();
        if !solid(x, y, z) {
            return;
        }
        let (fx, fy, fz) = (self.pos[0] - x as f64, self.pos[1] - y as f64, self.pos[2] - z as f64);
        let options = [
            (!solid(x - 1, y, z), fx, 0usize, -1.0),
            (!solid(x + 1, y, z), 1.0 - fx, 0, 1.0),
            (!solid(x, y - 1, z), fy, 1, -1.0),
            (!solid(x, y + 1, z), 1.0 - fy, 1, 1.0),
            (!solid(x, y, z - 1), fz, 2, -1.0),
            (!solid(x, y, z + 1), 1.0 - fz, 2, 1.0),
        ];
        let mut best = 9999.0;
        let mut pick = (1usize, 1.0f64);
        for (open, d, axis, dir) in options {
            if open && d < best {
                best = d;
                pick = (axis, dir);
            }
        }
        // Vanilla uses `rand.nextFloat() * 0.2F + 0.1F`; the mean keeps the
        // outcome independent of the per-entity generator.
        self.motion[pick.0] = pick.1 * 0.2;
    }

    fn move_by(&mut self, world: &dyn BlockView) {
        let mut bb = self.bb();
        let (ox, oy, oz) = (self.motion[0], self.motion[1], self.motion[2]);
        let (mut dx, mut dy, mut dz) = (ox, oy, oz);
        let list = colliding_boxes(world, &bb.add_coord(dx, dy, dz));
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
        self.pos = [(bb.min[0] + bb.max[0]) / 2.0, bb.min[1] + SIZE / 2.0, (bb.min[2] + bb.max[2]) / 2.0];
        self.on_ground = oy != dy && oy < 0.0;
        if ox != dx {
            self.motion[0] = 0.0;
        }
        if oy != dy {
            self.motion[1] = 0.0;
        }
        if oz != dz {
            self.motion[2] = 0.0;
        }
    }

    /// `EntityItem.onUpdate`. Returns false once the item should despawn.
    pub fn tick(&mut self, world: &dyn BlockView) -> bool {
        self.prev = self.pos;
        if self.pickup_delay > 0 {
            self.pickup_delay -= 1;
        }
        self.motion[1] -= 0.04f32 as f64;
        self.push_out(world);
        // Lava makes items jump and fizz.
        let (bx, by, bz) = (math::floor_double(self.pos[0]), math::floor_double(self.pos[1]), math::floor_double(self.pos[2]));
        if block::material(world.peek_block(bx, by, bz)) == block::Material::Lava {
            self.motion[1] = 0.2f32 as f64;
            return false;
        }
        self.move_by(world);
        let mut f = 0.98f32;
        if self.on_ground {
            let below = world.peek_block(bx, math::floor_double(self.bb().min[1]) - 1, bz);
            f = block::info(below).slipperiness * 0.98f32;
        }
        self.motion[0] *= f as f64;
        self.motion[1] *= 0.98f32 as f64;
        self.motion[2] *= f as f64;
        if self.on_ground {
            self.motion[1] *= -0.5;
        }
        self.age += 1;
        self.age < 6000 && self.pos[1] > -64.0
    }
}
