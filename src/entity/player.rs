//! Player movement: `Entity.moveEntity`, `EntityLivingBase.moveEntityWithHeading`,
//! `EntityPlayer` / `EntityPlayerSP.onLivingUpdate`.

use crate::block::{self, Material};
use crate::util::math;
use crate::world::BlockView;

#[derive(Clone, Copy, Default)]
pub struct Input {
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
    pub jump: bool,
    pub sneak: bool,
    pub sprint: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Aabb {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

impl Aabb {
    pub fn offset(&mut self, x: f64, y: f64, z: f64) {
        self.min[0] += x;
        self.max[0] += x;
        self.min[1] += y;
        self.max[1] += y;
        self.min[2] += z;
        self.max[2] += z;
    }
    pub fn offset_copy(&self, x: f64, y: f64, z: f64) -> Aabb {
        let mut b = *self;
        b.offset(x, y, z);
        b
    }
    /// `AxisAlignedBB.addCoord`
    pub fn add_coord(&self, x: f64, y: f64, z: f64) -> Aabb {
        let mut b = *self;
        for (i, d) in [x, y, z].into_iter().enumerate() {
            if d < 0.0 {
                b.min[i] += d;
            }
            if d > 0.0 {
                b.max[i] += d;
            }
        }
        b
    }
    pub fn intersects(&self, o: &Aabb) -> bool {
        (0..3).all(|i| o.max[i] > self.min[i] && o.min[i] < self.max[i])
    }
    /// `calculateXOffset` / `calculateYOffset` / `calculateZOffset`: `self` is the
    /// obstacle, `o` the moving box.
    pub fn calc_offset(&self, o: &Aabb, axis: usize, mut d: f64) -> f64 {
        for i in 0..3 {
            if i != axis && !(o.max[i] > self.min[i] && o.min[i] < self.max[i]) {
                return d;
            }
        }
        if d > 0.0 && o.max[axis] <= self.min[axis] {
            let d1 = self.min[axis] - o.max[axis];
            if d1 < d {
                d = d1;
            }
        }
        if d < 0.0 && o.min[axis] >= self.max[axis] {
            let d1 = self.max[axis] - o.min[axis];
            if d1 > d {
                d = d1;
            }
        }
        d
    }
}

/// `World.getCollidingBoundingBoxes` (blocks only).
pub fn colliding_boxes(world: &dyn BlockView, bb: &Aabb) -> Vec<Aabb> {
    let mut out = Vec::new();
    let get = |x: i32, y: i32, z: i32| (world.peek_block(x, y, z), world.peek_meta(x, y, z));
    let mut boxes = Vec::new();
    let x0 = math::floor_double(bb.min[0]);
    let x1 = math::floor_double(bb.max[0] + 1.0);
    let y0 = math::floor_double(bb.min[1]);
    let y1 = math::floor_double(bb.max[1] + 1.0);
    let z0 = math::floor_double(bb.min[2]);
    let z1 = math::floor_double(bb.max[2] + 1.0);
    for x in x0..x1 {
        for z in z0..z1 {
            for y in (y0 - 1)..y1 {
                let id = world.peek_block(x, y, z);
                if id == 0 {
                    continue;
                }
                if id == crate::block::PISTON_EXTENSION {
                    // BlockPistonMoving.getCollisionBoundingBoxFromPool
                    if let Some(c) = world
                        .moving(x, y, z)
                        .and_then(|m| m.collision(world, [x, y, z]))
                    {
                        let b = Aabb {
                            min: c.min,
                            max: c.max,
                        };
                        if b.intersects(bb) {
                            out.push(b);
                        }
                    }
                    continue;
                }
                boxes.clear();
                crate::shapes::collision(&get, id, world.peek_meta(x, y, z), x, y, z, &mut boxes);
                for c in &boxes {
                    let b = Aabb {
                        min: [
                            x as f64 + c[0] as f64,
                            y as f64 + c[1] as f64,
                            z as f64 + c[2] as f64,
                        ],
                        max: [
                            x as f64 + c[3] as f64,
                            y as f64 + c[4] as f64,
                            z as f64 + c[5] as f64,
                        ],
                    };
                    if b.intersects(bb) {
                        out.push(b);
                    }
                }
            }
        }
    }
    out
}

pub struct Player {
    pub bb: Aabb,
    pub prev_pos: [f64; 3],
    /// `posX`, eye `posY`, `posZ`
    pub pos: [f64; 3],
    pub motion: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    pub collided_horizontally: bool,
    pub sneaking: bool,
    pub sprinting: bool,
    pub flying: bool,
    pub allow_flying: bool,
    pub in_water: bool,
    /// Set by `tick` when the player jumped this tick.
    pub jumped: bool,
    pub in_lava_now: bool,
    y_size: f32,
    jump_ticks: i32,
    sprint_toggle_timer: i32,
    fly_toggle_timer: i32,
    prev_jump: bool,
    move_strafe: f32,
    move_forward: f32,
    ai_move_speed: f32,
    jump_movement_factor: f32,
    pub distance_walked: f32,
    pub prev_distance_walked: f32,
    pub camera_yaw: f32,
    pub prev_camera_yaw: f32,
    pub camera_pitch: f32,
    pub prev_camera_pitch: f32,
}

const WIDTH: f32 = 0.6;
const HEIGHT: f32 = 1.8;
const Y_OFFSET: f32 = 1.62;
const STEP_HEIGHT: f32 = 0.5;

impl Player {
    pub fn new(x: f64, feet_y: f64, z: f64) -> Player {
        let mut p = Player {
            bb: Aabb {
                min: [0.0; 3],
                max: [0.0; 3],
            },
            prev_pos: [0.0; 3],
            pos: [0.0; 3],
            motion: [0.0; 3],
            yaw: 0.0,
            pitch: 0.0,
            on_ground: false,
            collided_horizontally: false,
            sneaking: false,
            sprinting: false,
            flying: false,
            allow_flying: true,
            in_water: false,
            jumped: false,
            in_lava_now: false,
            y_size: 0.0,
            jump_ticks: 0,
            sprint_toggle_timer: 0,
            fly_toggle_timer: 0,
            prev_jump: false,
            move_strafe: 0.0,
            move_forward: 0.0,
            ai_move_speed: 0.1,
            jump_movement_factor: 0.02,
            distance_walked: 0.0,
            prev_distance_walked: 0.0,
            camera_yaw: 0.0,
            prev_camera_yaw: 0.0,
            camera_pitch: 0.0,
            prev_camera_pitch: 0.0,
        };
        p.set_position(x, feet_y, z);
        p.prev_pos = p.pos;
        p
    }

    pub fn set_position(&mut self, x: f64, feet_y: f64, z: f64) {
        let f = (WIDTH / 2.0f32) as f64;
        self.bb = Aabb {
            min: [x - f, feet_y, z - f],
            max: [x + f, feet_y + HEIGHT as f64, z + f],
        };
        self.pos = [x, feet_y + Y_OFFSET as f64 - self.y_size as f64, z];
    }

    /// `Entity.setAngles` with the default mouse sensitivity already applied.
    pub fn turn(&mut self, dx: f32, dy: f32) {
        self.yaw = (self.yaw as f64 + dx as f64 * 0.15) as f32;
        self.pitch = (self.pitch as f64 - dy as f64 * 0.15) as f32;
        self.pitch = self.pitch.clamp(-90.0, 90.0);
    }

    /// `EntityPlayerSP.getFOVMultiplier`
    pub fn fov_multiplier(&self) -> f32 {
        let mut f = 1.0f32;
        if self.flying {
            f *= 1.1f32;
        }
        let speed = if self.sprinting {
            0.1f64 * (1.0 + 0.30000001192092896)
        } else {
            0.1f32 as f64
        };
        (f as f64 * ((speed / 0.1f32 as f64 + 1.0) / 2.0)) as f32
    }

    fn material_in_bb(world: &dyn BlockView, bb: &Aabb, m: Material) -> bool {
        let x0 = math::floor_double(bb.min[0]);
        let x1 = math::floor_double(bb.max[0] + 1.0);
        let y0 = math::floor_double(bb.min[1]);
        let y1 = math::floor_double(bb.max[1] + 1.0);
        let z0 = math::floor_double(bb.min[2]);
        let z1 = math::floor_double(bb.max[2] + 1.0);
        for x in x0..x1 {
            for y in y0..y1 {
                for z in z0..z1 {
                    if block::info(world.peek_block(x, y, z)).material == m {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn expand(bb: &Aabb, x: f64, y: f64, z: f64) -> Aabb {
        Aabb {
            min: [bb.min[0] - x, bb.min[1] - y, bb.min[2] - z],
            max: [bb.max[0] + x, bb.max[1] + y, bb.max[2] + z],
        }
    }

    /// `BlockLiquid.getFlowVector`
    pub(crate) fn flow_vector(
        world: &dyn BlockView,
        m: Material,
        x: i32,
        y: i32,
        z: i32,
    ) -> [f64; 3] {
        let decay = |x: i32, y: i32, z: i32| -> i32 {
            if block::material(world.peek_block(x, y, z)) != m {
                return -1;
            }
            let d = world.peek_meta(x, y, z) as i32;
            if d >= 8 {
                0
            } else {
                d
            }
        };
        let solid = |x: i32, y: i32, z: i32, side: usize| -> bool {
            let mat = block::material(world.peek_block(x, y, z));
            if mat == m {
                false
            } else if side == 1 {
                true
            } else if mat == Material::Ice {
                false
            } else {
                mat.is_solid()
            }
        };
        let mut v = [0.0f64; 3];
        let own = decay(x, y, z);
        for (dx, dz) in [(-1, 0), (0, -1), (1, 0), (0, 1)] {
            let (nx, nz) = (x + dx, z + dz);
            let mut d = decay(nx, y, nz);
            if d < 0 {
                if !block::material(world.peek_block(nx, y, nz)).blocks_movement() {
                    d = decay(nx, y - 1, nz);
                    if d >= 0 {
                        let k = (d - (own - 8)) as f64;
                        v[0] += dx as f64 * k;
                        v[2] += dz as f64 * k;
                    }
                }
            } else {
                let k = (d - own) as f64;
                v[0] += dx as f64 * k;
                v[2] += dz as f64 * k;
            }
        }
        let norm = |v: [f64; 3]| {
            let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            if l < 1.0e-4 {
                [0.0; 3]
            } else {
                [v[0] / l, v[1] / l, v[2] / l]
            }
        };
        if world.peek_meta(x, y, z) >= 8 {
            let s = solid(x, y, z - 1, 2)
                || solid(x, y, z + 1, 3)
                || solid(x - 1, y, z, 4)
                || solid(x + 1, y, z, 5)
                || solid(x, y + 1, z - 1, 2)
                || solid(x, y + 1, z + 1, 3)
                || solid(x - 1, y + 1, z, 4)
                || solid(x + 1, y + 1, z, 5);
            if s {
                v = norm(v);
                v[1] -= 6.0;
            }
        }
        norm(v)
    }

    /// `World.handleMaterialAcceleration`: detects water and applies its current.
    fn handle_water(&mut self, world: &dyn BlockView) {
        let mut push = [0.0f64; 3];
        let bb = Self::expand(
            &Self::expand(&self.bb, 0.0, -0.4f32 as f64, 0.0),
            -0.001,
            -0.001,
            -0.001,
        );
        let x0 = math::floor_double(bb.min[0]);
        let x1 = math::floor_double(bb.max[0] + 1.0);
        let y0 = math::floor_double(bb.min[1]);
        let y1 = math::floor_double(bb.max[1] + 1.0);
        let z0 = math::floor_double(bb.min[2]);
        let z1 = math::floor_double(bb.max[2] + 1.0);
        let mut found = false;
        for x in x0..x1 {
            for y in y0..y1 {
                for z in z0..z1 {
                    if block::info(world.peek_block(x, y, z)).material == Material::Water {
                        let meta = world.peek_meta(x, y, z) as i32;
                        let pct = (if meta >= 8 { 0 } else { meta } + 1) as f32 / 9.0f32;
                        let d0 = ((y + 1) as f32 - pct) as f64;
                        if y1 as f64 >= d0 {
                            found = true;
                            let f = Self::flow_vector(world, Material::Water, x, y, z);
                            for i in 0..3 {
                                push[i] += f[i];
                            }
                        }
                    }
                }
            }
        }
        let len = (push[0] * push[0] + push[1] * push[1] + push[2] * push[2]).sqrt();
        // Entity.isPushedByWater: not while flying.
        if len > 0.0 && !self.flying {
            let l = if len < 1.0e-4 { 0.0 } else { 1.0 / len };
            for i in 0..3 {
                self.motion[i] += push[i] * l * 0.014;
            }
        }
        self.in_water = found;
    }

    fn in_lava(&self, world: &dyn BlockView) -> bool {
        Self::material_in_bb(
            world,
            &Self::expand(&self.bb, -0.1f32 as f64, -0.4f32 as f64, -0.1f32 as f64),
            Material::Lava,
        )
    }

    /// `Entity.isInsideOfMaterial` for the eye position.
    pub fn eye_material(&self, world: &dyn BlockView, eye: [f64; 3]) -> Material {
        let (x, y, z) = (
            math::floor_double(eye[0]),
            math::floor_float(math::floor_double(eye[1]) as f32),
            math::floor_double(eye[2]),
        );
        let id = world.peek_block(x, y, z);
        let m = block::info(id).material;
        if m == Material::Water || m == Material::Lava {
            let meta = world.peek_meta(x, y, z) as i32;
            let pct = (if meta >= 8 { 0 } else { meta } + 1) as f32 / 9.0f32;
            let f = pct - 0.11111111f32;
            let f1 = (y + 1) as f32 - f;
            if eye[1] < f1 as f64 {
                return m;
            }
        }
        Material::Air
    }

    fn is_offset_position_in_liquid(&self, world: &dyn BlockView, x: f64, y: f64, z: f64) -> bool {
        let bb = self.bb.offset_copy(x, y, z);
        colliding_boxes(world, &bb).is_empty()
            && !Self::material_in_bb(world, &bb, Material::Water)
            && !Self::material_in_bb(world, &bb, Material::Lava)
    }

    fn move_flying(&mut self, mut strafe: f32, mut forward: f32, friction: f32) {
        let mut f3 = strafe * strafe + forward * forward;
        if f3 >= 1.0e-4f32 {
            f3 = math::sqrt_float(f3);
            if f3 < 1.0 {
                f3 = 1.0;
            }
            f3 = friction / f3;
            strafe *= f3;
            forward *= f3;
            let f4 = math::sin(self.yaw * math::PI / 180.0f32);
            let f5 = math::cos(self.yaw * math::PI / 180.0f32);
            self.motion[0] += (strafe * f5 - forward * f4) as f64;
            self.motion[2] += (forward * f5 + strafe * f4) as f64;
        }
    }

    /// `Entity.moveEntity` from outside (a piston pushing the player).
    pub fn push_move(&mut self, world: &dyn BlockView, dx: f64, dy: f64, dz: f64) {
        self.move_entity(world, dx, dy, dz);
    }

    fn move_entity(&mut self, world: &dyn BlockView, mut dx: f64, mut dy: f64, mut dz: f64) {
        self.y_size *= 0.4f32;
        let start = self.pos;
        let (mut ox, oy, mut oz) = (dx, dy, dz);
        let orig_bb = self.bb;
        let sneak = self.on_ground && self.sneaking;
        if sneak {
            let step = 0.05;
            while dx != 0.0
                && colliding_boxes(world, &self.bb.offset_copy(dx, -1.0, 0.0)).is_empty()
            {
                if dx < step && dx >= -step {
                    dx = 0.0;
                } else if dx > 0.0 {
                    dx -= step;
                } else {
                    dx += step;
                }
                ox = dx;
            }
            while dz != 0.0
                && colliding_boxes(world, &self.bb.offset_copy(0.0, -1.0, dz)).is_empty()
            {
                if dz < step && dz >= -step {
                    dz = 0.0;
                } else if dz > 0.0 {
                    dz -= step;
                } else {
                    dz += step;
                }
                oz = dz;
            }
            while dx != 0.0
                && dz != 0.0
                && colliding_boxes(world, &self.bb.offset_copy(dx, -1.0, dz)).is_empty()
            {
                if dx < step && dx >= -step {
                    dx = 0.0;
                } else if dx > 0.0 {
                    dx -= step;
                } else {
                    dx += step;
                }
                if dz < step && dz >= -step {
                    dz = 0.0;
                } else if dz > 0.0 {
                    dz -= step;
                } else {
                    dz += step;
                }
                ox = dx;
                oz = dz;
            }
        }

        let list = colliding_boxes(world, &self.bb.add_coord(dx, dy, dz));
        for b in &list {
            dy = b.calc_offset(&self.bb, 1, dy);
        }
        self.bb.offset(0.0, dy, 0.0);
        let flag1 = self.on_ground || oy != dy && oy < 0.0;
        for b in &list {
            dx = b.calc_offset(&self.bb, 0, dx);
        }
        self.bb.offset(dx, 0.0, 0.0);
        for b in &list {
            dz = b.calc_offset(&self.bb, 2, dz);
        }
        self.bb.offset(0.0, 0.0, dz);

        if STEP_HEIGHT > 0.0 && flag1 && (sneak || self.y_size < 0.05f32) && (ox != dx || oz != dz)
        {
            let (sx, sy, sz) = (dx, dy, dz);
            dx = ox;
            dy = STEP_HEIGHT as f64;
            dz = oz;
            let saved = self.bb;
            self.bb = orig_bb;
            let list = colliding_boxes(world, &self.bb.add_coord(dx, dy, dz));
            for b in &list {
                dy = b.calc_offset(&self.bb, 1, dy);
            }
            self.bb.offset(0.0, dy, 0.0);
            for b in &list {
                dx = b.calc_offset(&self.bb, 0, dx);
            }
            self.bb.offset(dx, 0.0, 0.0);
            for b in &list {
                dz = b.calc_offset(&self.bb, 2, dz);
            }
            self.bb.offset(0.0, 0.0, dz);
            dy = -(STEP_HEIGHT as f64);
            for b in &list {
                dy = b.calc_offset(&self.bb, 1, dy);
            }
            self.bb.offset(0.0, dy, 0.0);
            if sx * sx + sz * sz >= dx * dx + dz * dz {
                dx = sx;
                dy = sy;
                dz = sz;
                self.bb = saved;
            }
        }

        self.pos[0] = (self.bb.min[0] + self.bb.max[0]) / 2.0;
        self.pos[1] = self.bb.min[1] + Y_OFFSET as f64 - self.y_size as f64;
        self.pos[2] = (self.bb.min[2] + self.bb.max[2]) / 2.0;
        self.collided_horizontally = ox != dx || oz != dz;
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
        if !sneak {
            let ddx = self.pos[0] - start[0];
            let ddz = self.pos[2] - start[2];
            self.distance_walked = (self.distance_walked as f64
                + math::sqrt_double(ddx * ddx + ddz * ddz) as f64 * 0.6)
                as f32;
        }
    }

    fn move_with_heading(&mut self, world: &dyn BlockView, strafe: f32, forward: f32) {
        let in_lava = self.in_lava(world);
        if self.in_water && !self.flying {
            let d0 = self.pos[1];
            self.move_flying(strafe, forward, 0.02);
            let m = self.motion;
            self.move_entity(world, m[0], m[1], m[2]);
            for v in &mut self.motion {
                *v *= 0.8f32 as f64;
            }
            self.motion[1] -= 0.02;
            let m = self.motion;
            if self.collided_horizontally
                && self.is_offset_position_in_liquid(
                    world,
                    m[0],
                    m[1] + 0.6f32 as f64 - self.pos[1] + d0,
                    m[2],
                )
            {
                self.motion[1] = 0.3f32 as f64;
            }
        } else if in_lava && !self.flying {
            let d0 = self.pos[1];
            self.move_flying(strafe, forward, 0.02);
            let m = self.motion;
            self.move_entity(world, m[0], m[1], m[2]);
            for v in &mut self.motion {
                *v *= 0.5;
            }
            self.motion[1] -= 0.02;
            let m = self.motion;
            if self.collided_horizontally
                && self.is_offset_position_in_liquid(
                    world,
                    m[0],
                    m[1] + 0.6f32 as f64 - self.pos[1] + d0,
                    m[2],
                )
            {
                self.motion[1] = 0.3f32 as f64;
            }
        } else {
            let ground_slip = |p: &Player| {
                let id = world.peek_block(
                    math::floor_double(p.pos[0]),
                    math::floor_double(p.bb.min[1]) - 1,
                    math::floor_double(p.pos[2]),
                );
                block::info(id).slipperiness * 0.91f32
            };
            let mut f2 = 0.91f32;
            if self.on_ground {
                f2 = ground_slip(self);
            }
            let f3 = 0.16277136f32 / (f2 * f2 * f2);
            let f4 = if self.on_ground {
                self.ai_move_speed * f3
            } else {
                self.jump_movement_factor
            };
            self.move_flying(strafe, forward, f4);
            f2 = 0.91f32;
            if self.on_ground {
                f2 = ground_slip(self);
            }
            let m = self.motion;
            self.move_entity(world, m[0], m[1], m[2]);
            if !world.loaded(self.pos[0] as i32, self.pos[2] as i32) {
                self.motion[1] = if self.pos[1] > 0.0 { -0.1 } else { 0.0 };
            } else {
                self.motion[1] -= 0.08;
            }
            self.motion[1] *= 0.98f32 as f64;
            self.motion[0] *= f2 as f64;
            self.motion[2] *= f2 as f64;
        }
    }

    /// One game tick (`onUpdate`).
    pub fn tick(&mut self, world: &dyn BlockView, input: &Input) {
        self.jumped = false;
        self.in_lava_now = self.in_lava(world);
        self.prev_pos = self.pos;
        self.prev_distance_walked = self.distance_walked;
        self.prev_camera_pitch = self.camera_pitch;
        self.handle_water(world);
        self.sneaking = input.sneak;

        // EntityPlayerSP.onLivingUpdate
        if self.sprint_toggle_timer > 0 {
            self.sprint_toggle_timer -= 1;
        }
        let was_jump = self.prev_jump;
        let was_forward = self.move_forward >= 0.8f32;
        let mut strafe = 0.0f32;
        let mut forward = 0.0f32;
        if input.forward {
            forward += 1.0;
        }
        if input.back {
            forward -= 1.0;
        }
        if input.left {
            strafe += 1.0;
        }
        if input.right {
            strafe -= 1.0;
        }
        if input.sneak {
            strafe = (strafe as f64 * 0.3) as f32;
            forward = (forward as f64 * 0.3) as f32;
        }
        self.prev_jump = input.jump;
        if input.sneak && self.y_size < 0.2f32 {
            self.y_size = 0.2f32;
        }
        if self.on_ground && !was_forward && forward >= 0.8f32 && !self.sprinting {
            if self.sprint_toggle_timer <= 0 && !input.sprint {
                self.sprint_toggle_timer = 7;
            } else {
                self.sprinting = true;
            }
        }
        if !self.sprinting && forward >= 0.8f32 && input.sprint {
            self.sprinting = true;
        }
        if self.sprinting && (forward < 0.8f32 || self.collided_horizontally) {
            self.sprinting = false;
        }
        if self.allow_flying && !was_jump && input.jump {
            if self.fly_toggle_timer == 0 {
                self.fly_toggle_timer = 7;
            } else {
                self.flying = !self.flying;
                self.fly_toggle_timer = 0;
            }
        }
        if self.flying {
            if input.sneak {
                self.motion[1] -= 0.15;
            }
            if input.jump {
                self.motion[1] += 0.15;
            }
        }

        // EntityPlayer.onLivingUpdate
        if self.fly_toggle_timer > 0 {
            self.fly_toggle_timer -= 1;
        }
        self.prev_camera_yaw = self.camera_yaw;

        // EntityLivingBase.onLivingUpdate
        if self.jump_ticks > 0 {
            self.jump_ticks -= 1;
        }
        for v in &mut self.motion {
            if v.abs() < 0.005 {
                *v = 0.0;
            }
        }
        if input.jump {
            if self.in_water || self.in_lava(world) {
                self.motion[1] += 0.04f32 as f64;
            } else if self.on_ground && self.jump_ticks == 0 {
                self.motion[1] = 0.42f32 as f64;
                self.jumped = true;
                if self.sprinting {
                    let f = self.yaw * 0.017453292f32;
                    self.motion[0] -= (math::sin(f) * 0.2f32) as f64;
                    self.motion[2] += (math::cos(f) * 0.2f32) as f64;
                }
                self.jump_ticks = 10;
            }
        } else {
            self.jump_ticks = 0;
        }
        self.move_strafe = strafe;
        self.move_forward = forward;
        let (ms, mf) = (strafe * 0.98f32, forward * 0.98f32);

        // EntityPlayer.moveEntityWithHeading
        if self.flying {
            let d3 = self.motion[1];
            let f2 = self.jump_movement_factor;
            self.jump_movement_factor = 0.05;
            self.move_with_heading(world, ms, mf);
            self.motion[1] = d3 * 0.6;
            self.jump_movement_factor = f2;
        } else {
            self.move_with_heading(world, ms, mf);
        }

        // Rest of EntityPlayer.onLivingUpdate
        self.jump_movement_factor = 0.02;
        if self.sprinting {
            self.jump_movement_factor =
                (self.jump_movement_factor as f64 + 0.02f32 as f64 * 0.3) as f32;
        }
        self.ai_move_speed = if self.sprinting {
            (0.1f32 as f64 * (1.0 + 0.30000001192092896)) as f32
        } else {
            0.1
        };
        let mut f =
            math::sqrt_double(self.motion[0] * self.motion[0] + self.motion[2] * self.motion[2]);
        let mut f1 = ((-self.motion[1] * 0.2f32 as f64).atan() as f32) * 15.0f32;
        if f > 0.1f32 {
            f = 0.1f32;
        }
        if !self.on_ground {
            f = 0.0;
        }
        if self.on_ground {
            f1 = 0.0;
        }
        self.camera_yaw += (f - self.camera_yaw) * 0.4f32;
        self.camera_pitch += (f1 - self.camera_pitch) * 0.8f32;

        if self.on_ground && self.flying {
            self.flying = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{ChunkData, ChunkProvider, ServerWorld as World};
    use std::sync::Arc;

    fn flat_world() -> World {
        let mut w = World::new(Arc::new(ChunkProvider::new(0)));
        for cx in -2..=2 {
            for cz in -2..=20 {
                let mut blocks: Box<[u8; 65536]> =
                    vec![0u8; 65536].into_boxed_slice().try_into().unwrap();
                for c in 0..256 {
                    for y in 0..64 {
                        blocks[(c << 8) | y] = block::STONE;
                    }
                }
                let meta: Box<[u8; 65536]> =
                    vec![0u8; 65536].into_boxed_slice().try_into().unwrap();
                w.insert_raw(ChunkData {
                    cx,
                    cz,
                    blocks,
                    meta,
                    biomes: [1; 256],
                    sky: crate::world::gen::chunk_provider::boxed(),
                    blk: crate::world::gen::chunk_provider::boxed(),
                });
            }
        }
        w
    }

    fn settle(w: &World) -> Player {
        let mut p = Player::new(0.5, 64.0, 0.5);
        p.allow_flying = false;
        for _ in 0..5 {
            p.tick(w, &Input::default());
        }
        assert!(p.on_ground);
        p
    }

    #[test]
    fn walking_speed_matches_vanilla() {
        let w = flat_world();
        let mut p = settle(&w);
        let input = Input {
            forward: true,
            ..Default::default()
        };
        for _ in 0..100 {
            p.tick(&w, &input);
        }
        let v = p.pos[2] - p.prev_pos[2];
        // 4.317 m/s
        assert!((v * 20.0 - 4.317).abs() < 0.005, "walk speed {}", v * 20.0);
    }

    #[test]
    fn sprinting_speed_matches_vanilla() {
        let w = flat_world();
        let mut p = settle(&w);
        let input = Input {
            forward: true,
            sprint: true,
            ..Default::default()
        };
        for _ in 0..100 {
            p.tick(&w, &input);
        }
        let v = p.pos[2] - p.prev_pos[2];
        // 5.612 m/s
        assert!(
            (v * 20.0 - 5.612).abs() < 0.005,
            "sprint speed {}",
            v * 20.0
        );
    }

    #[test]
    fn jump_height_matches_vanilla() {
        let w = flat_world();
        let mut p = settle(&w);
        let mut peak = 0.0f64;
        let input = Input {
            jump: true,
            ..Default::default()
        };
        p.tick(&w, &input);
        for _ in 0..12 {
            peak = peak.max(p.bb.min[1] - 64.0);
            p.tick(&w, &Input::default());
        }
        // Known apex of a 0.42 jump: 1.2492 blocks.
        assert!((peak - 1.2492).abs() < 0.001, "jump apex {peak}");
    }

    #[test]
    fn flowing_water_pushes_the_player_downstream() {
        let mut w = flat_world();
        // A channel along +Z: source at z=2, levels rising towards z=8.
        for z in 2..9 {
            for x in -1..2 {
                w.set_block(x, 64, z, block::STONE, 0, 2);
            }
            w.set_block(0, 64, z, block::FLOWING_WATER, z - 2, 2);
        }
        let mut p = Player::new(0.5, 64.0, 4.5);
        p.allow_flying = false;
        let z0 = p.pos[2];
        for _ in 0..40 {
            p.tick(&w, &Input::default());
        }
        assert!(p.in_water);
        assert!(
            p.pos[2] > z0 + 0.5,
            "not carried by the current: z={}",
            p.pos[2]
        );
    }

    /// MC-455: standing exactly on a block corner must not confuse collision.
    #[test]
    fn exact_integer_coordinates_collide_normally() {
        let w = flat_world();
        let mut p = Player::new(0.0, 64.0, 0.0);
        p.allow_flying = false;
        for _ in 0..10 {
            p.tick(&w, &Input::default());
        }
        assert!(p.on_ground);
        assert_eq!(p.bb.min[1], 64.0);
    }

    #[test]
    fn sneaking_does_not_walk_off_edges() {
        let mut w = flat_world();
        for z in 3..40 {
            for x in -2..3 {
                for y in 60..64 {
                    w.set_block(x, y, z, block::AIR, 0, 2);
                }
            }
        }
        let mut p = settle(&w);
        let input = Input {
            forward: true,
            sneak: true,
            ..Default::default()
        };
        for _ in 0..200 {
            p.tick(&w, &input);
        }
        assert!(
            p.on_ground && p.bb.min[1] == 64.0,
            "fell off: y={}",
            p.bb.min[1]
        );
        assert!(p.pos[2] < 3.31 && p.pos[2] > 2.9, "z={}", p.pos[2]);
    }
}
