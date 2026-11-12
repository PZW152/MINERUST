//! `net.minecraft.entity.Entity`: the state and movement every entity shares.
//!
//! The virtual methods subclasses override (`attackEntityFrom`, `fall`,
//! `playSound`, `onEntityCollidedWithBlock`, ...) are reported as [`Fx`]
//! entries in call order; the owning entity handles them right after the
//! call that produced them.

use super::player::{colliding_boxes, Aabb, Player};
use crate::block::{self, Material};
use crate::java::JavaRandom;
use crate::util::math;
use crate::world::BlockView;

/// `DamageSource` kinds the base class produces.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    InFire,
    OnFire,
    Lava,
    Cactus,
    Drown,
    InWall,
    OutOfWorld,
}

/// Side effects of the base class, in the order vanilla performs them.
#[derive(Clone, Debug)]
pub enum Fx {
    Sound(&'static str, [f64; 3], f32, f32),
    Particle(&'static str, [f64; 3], [f64; 3]),
    Damage(Source, f32),
    /// `Entity.fall(distance)`
    Fall(f32),
    /// `Block.onEntityWalking`
    Walk(i32, i32, i32, u8),
    /// `Block.onEntityCollidedWithBlock`
    Collide(i32, i32, i32, u8),
    /// `Entity.kill` (below y = -64)
    Kill,
}

#[derive(Clone, Debug)]
pub struct Ent {
    pub pos: [f64; 3],
    pub prev: [f64; 3],
    pub last_tick: [f64; 3],
    pub motion: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub prev_yaw: f32,
    pub prev_pitch: f32,
    pub bb: Aabb,
    pub width: f32,
    pub height: f32,
    pub y_offset: f32,
    pub y_size: f32,
    pub step_height: f32,
    pub on_ground: bool,
    pub collided_h: bool,
    pub collided_v: bool,
    pub collided: bool,
    pub in_web: bool,
    pub fall_distance: f32,
    pub no_clip: bool,
    pub in_water: bool,
    pub first_update: bool,
    pub fire: i32,
    pub fire_resistance: i32,
    pub immune_to_fire: bool,
    pub ticks_existed: i32,
    pub dead: bool,
    pub dist_walked_mod: f32,
    pub prev_dist_walked_mod: f32,
    pub dist_walked_on_step: f32,
    pub next_step_distance: i32,
    pub in_portal: bool,
    /// Touched an End portal this tick (`travelToDimension(1)` pending).
    pub in_end_portal: bool,
    pub portal_counter: i32,
    /// `getPortalCooldown`: 300, 10 for players.
    pub portal_cooldown: i32,
    /// `teleportDirection`
    pub teleport_direction: i32,
    pub time_until_portal: i32,
    pub hurt_resistant_time: i32,
    pub entity_collision_reduction: f32,
    pub can_trigger_walking: bool,
    /// `isPushedByWater`
    pub pushed_by_water: bool,
    pub riding: Option<u32>,
    pub ridden_by: Option<u32>,
    pub rand: JavaRandom,
}

impl Ent {
    /// `Entity(World)` + `setSize`. `seed` stands in for `new Random()`.
    pub fn new(width: f32, height: f32, seed: i64) -> Ent {
        Ent {
            pos: [0.0; 3],
            prev: [0.0; 3],
            last_tick: [0.0; 3],
            motion: [0.0; 3],
            yaw: 0.0,
            pitch: 0.0,
            prev_yaw: 0.0,
            prev_pitch: 0.0,
            bb: Aabb {
                min: [0.0; 3],
                max: [0.0; 3],
            },
            width,
            height,
            y_offset: 0.0,
            y_size: 0.0,
            step_height: 0.0,
            on_ground: false,
            collided_h: false,
            collided_v: false,
            collided: false,
            in_web: false,
            fall_distance: 0.0,
            no_clip: false,
            in_water: false,
            first_update: true,
            fire: 0,
            fire_resistance: 1,
            immune_to_fire: false,
            ticks_existed: 0,
            dead: false,
            dist_walked_mod: 0.0,
            prev_dist_walked_mod: 0.0,
            dist_walked_on_step: 0.0,
            next_step_distance: 1,
            in_portal: false,
            in_end_portal: false,
            portal_counter: 0,
            portal_cooldown: 300,
            teleport_direction: 0,
            time_until_portal: 0,
            hurt_resistant_time: 0,
            entity_collision_reduction: 0.0,
            can_trigger_walking: true,
            pushed_by_water: true,
            riding: None,
            ridden_by: None,
            rand: JavaRandom::new(seed),
        }
    }

    /// `Entity.setPosition`
    pub fn set_position(&mut self, x: f64, y: f64, z: f64) {
        self.pos = [x, y, z];
        let f = (self.width / 2.0) as f64;
        let f1 = self.height as f64;
        let min_y = y - self.y_offset as f64 + self.y_size as f64;
        self.bb = Aabb {
            min: [x - f, min_y, z - f],
            max: [x + f, min_y + f1, z + f],
        };
    }

    /// `Entity.setLocationAndAngles`
    pub fn set_location_and_angles(&mut self, x: f64, y: f64, z: f64, yaw: f32, pitch: f32) {
        self.last_tick = [x, y, z];
        self.prev = [x, y + self.y_offset as f64, z];
        self.pos = self.prev;
        self.yaw = yaw;
        self.prev_yaw = yaw;
        self.pitch = pitch;
        self.prev_pitch = pitch;
        self.y_size = 0.0;
        self.set_position(self.pos[0], self.pos[1], self.pos[2]);
    }

    /// `Entity.writeToNBT` before `writeEntityToNBT`. The UUID is derived
    /// from the entity id.
    pub fn nbt_fields(&self, dim: i32, id: u32) -> Vec<(&'static str, crate::world::anvil::Nbt)> {
        use crate::world::anvil::{list, Nbt};
        let most = super::item::entity_seed(id) ^ 0x4D696E6552757374;
        let least = super::item::entity_seed(id.wrapping_add(0x9E3779B9));
        vec![
            (
                "Pos",
                list(vec![
                    Nbt::Double(self.pos[0]),
                    Nbt::Double(self.pos[1] + self.y_size as f64),
                    Nbt::Double(self.pos[2]),
                ]),
            ),
            (
                "Motion",
                list(vec![
                    Nbt::Double(self.motion[0]),
                    Nbt::Double(self.motion[1]),
                    Nbt::Double(self.motion[2]),
                ]),
            ),
            (
                "Rotation",
                list(vec![Nbt::Float(self.yaw), Nbt::Float(self.pitch)]),
            ),
            ("FallDistance", Nbt::Float(self.fall_distance)),
            ("Fire", Nbt::Short(self.fire as i16)),
            ("Air", Nbt::Short(300)),
            ("OnGround", Nbt::Byte(self.on_ground as i8)),
            ("Dimension", Nbt::Int(dim)),
            ("Invulnerable", Nbt::Byte(0)),
            ("PortalCooldown", Nbt::Int(self.time_until_portal)),
            ("UUIDMost", Nbt::Long(most)),
            ("UUIDLeast", Nbt::Long(least)),
        ]
    }

    pub fn expand(bb: &Aabb, x: f64, y: f64, z: f64) -> Aabb {
        Aabb {
            min: [bb.min[0] - x, bb.min[1] - y, bb.min[2] - z],
            max: [bb.max[0] + x, bb.max[1] + y, bb.max[2] + z],
        }
    }

    /// `Entity.onEntityUpdate` from the sprint particles on (the portal
    /// counter is handled by the dimension code).
    pub fn base_tick(&mut self, w: &dyn BlockView, sprinting: bool, fx: &mut Vec<Fx>) {
        self.prev_dist_walked_mod = self.dist_walked_mod;
        self.prev = self.pos;
        self.prev_pitch = self.pitch;
        self.prev_yaw = self.yaw;
        if sprinting && !self.in_water {
            let x = math::floor_double(self.pos[0]);
            let y = math::floor_double(self.pos[1] - 0.2f32 as f64 - self.y_offset as f64);
            let z = math::floor_double(self.pos[2]);
            if block::material(w.peek_block(x, y, z)) != Material::Air {
                let px = self.pos[0] + (self.rand.next_float() as f64 - 0.5) * self.width as f64;
                let pz = self.pos[2] + (self.rand.next_float() as f64 - 0.5) * self.width as f64;
                fx.push(Fx::Particle(
                    "blockcrack",
                    [px, self.bb.min[1] + 0.1, pz],
                    [-self.motion[0] * 4.0, 1.5, -self.motion[2] * 4.0],
                ));
            }
        }
        self.handle_water_movement(w, None, fx);
        if self.fire > 0 {
            if self.immune_to_fire {
                self.fire -= 4;
                if self.fire < 0 {
                    self.fire = 0;
                }
            } else {
                if self.fire % 20 == 0 {
                    fx.push(Fx::Damage(Source::OnFire, 1.0));
                }
                self.fire -= 1;
            }
        }
        if self.handle_lava_movement(w) {
            // setOnFireFromLava
            if !self.immune_to_fire {
                fx.push(Fx::Damage(Source::Lava, 4.0));
                self.set_fire(15);
            }
            self.fall_distance *= 0.5;
        }
        if self.pos[1] < -64.0 {
            fx.push(Fx::Kill);
        }
        self.first_update = false;
    }

    /// `Entity.setInPortal`
    pub fn set_in_portal(&mut self) {
        if self.time_until_portal > 0 {
            self.time_until_portal = self.portal_cooldown;
        } else {
            let dx = self.prev[0] - self.pos[0];
            let dz = self.prev[2] - self.pos[2];
            if !self.in_portal {
                // Direction.getMovementDirection
                self.teleport_direction = if (dx as f32).abs() > (dz as f32).abs() {
                    if dx > 0.0 {
                        1
                    } else {
                        3
                    }
                } else if dz > 0.0 {
                    2
                } else {
                    0
                };
            }
            self.in_portal = true;
        }
    }

    /// `Entity.setFire` (without the Fire Protection reduction, which
    /// living entities apply themselves).
    pub fn set_fire(&mut self, seconds: i32) {
        let t = seconds * 20;
        if self.fire < t {
            self.fire = t;
        }
    }

    /// `Entity.handleWaterMovement`; `bb` overrides the box (`EntityItem`
    /// passes its own box unchanged).
    pub fn handle_water_movement(
        &mut self,
        w: &dyn BlockView,
        bb: Option<Aabb>,
        fx: &mut Vec<Fx>,
    ) -> bool {
        let bb = bb.unwrap_or_else(|| {
            Self::expand(
                &Self::expand(&self.bb, 0.0, -0.4f32 as f64, 0.0),
                -0.001,
                -0.001,
                -0.001,
            )
        });
        if self.handle_material_acceleration(w, &bb, Material::Water) {
            if !self.in_water && !self.first_update {
                let m = self.motion;
                let mut f = math::sqrt_double(
                    m[0] * m[0] * 0.2f32 as f64 + m[1] * m[1] + m[2] * m[2] * 0.2f32 as f64,
                ) * 0.2f32;
                if f > 1.0 {
                    f = 1.0;
                }
                let p = 1.0 + (self.rand.next_float() - self.rand.next_float()) * 0.4;
                fx.push(Fx::Sound("game.neutral.swim.splash", self.pos, f, p));
                let fy = math::floor_double(self.bb.min[1]) as f32;
                let n = 1.0 + self.width * 20.0;
                let mut i = 0;
                while (i as f32) < n {
                    let a = (self.rand.next_float() * 2.0 - 1.0) * self.width;
                    let b = (self.rand.next_float() * 2.0 - 1.0) * self.width;
                    let my = self.motion[1] - (self.rand.next_float() * 0.2) as f64;
                    fx.push(Fx::Particle(
                        "bubble",
                        [
                            self.pos[0] + a as f64,
                            (fy + 1.0) as f64,
                            self.pos[2] + b as f64,
                        ],
                        [self.motion[0], my, self.motion[2]],
                    ));
                    i += 1;
                }
                let mut i = 0;
                while (i as f32) < n {
                    let a = (self.rand.next_float() * 2.0 - 1.0) * self.width;
                    let b = (self.rand.next_float() * 2.0 - 1.0) * self.width;
                    fx.push(Fx::Particle(
                        "splash",
                        [
                            self.pos[0] + a as f64,
                            (fy + 1.0) as f64,
                            self.pos[2] + b as f64,
                        ],
                        self.motion,
                    ));
                    i += 1;
                }
            }
            self.fall_distance = 0.0;
            self.in_water = true;
            self.fire = 0;
        } else {
            self.in_water = false;
        }
        self.in_water
    }

    /// `World.handleMaterialAcceleration`
    pub fn handle_material_acceleration(
        &mut self,
        w: &dyn BlockView,
        bb: &Aabb,
        m: Material,
    ) -> bool {
        let x0 = math::floor_double(bb.min[0]);
        let x1 = math::floor_double(bb.max[0] + 1.0);
        let y0 = math::floor_double(bb.min[1]);
        let y1 = math::floor_double(bb.max[1] + 1.0);
        let z0 = math::floor_double(bb.min[2]);
        let z1 = math::floor_double(bb.max[2] + 1.0);
        if !chunks_exist(w, x0, y0, z0, x1, y1, z1) {
            return false;
        }
        let mut found = false;
        let mut v = [0.0f64; 3];
        for x in x0..x1 {
            for y in y0..y1 {
                for z in z0..z1 {
                    if block::material(w.peek_block(x, y, z)) == m {
                        let d = (y + 1) as f64
                            - liquid_height_percent(w.peek_meta(x, y, z) as i32) as f64;
                        if y1 as f64 >= d {
                            found = true;
                            let f = Player::flow_vector(w, m, x, y, z);
                            for i in 0..3 {
                                v[i] += f[i];
                            }
                        }
                    }
                }
            }
        }
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if len > 0.0 && self.pushed_by_water {
            // Vec3.normalize
            let l = if len < 1.0e-4 {
                [0.0; 3]
            } else {
                [v[0] / len, v[1] / len, v[2] / len]
            };
            for i in 0..3 {
                self.motion[i] += l[i] * 0.014;
            }
        }
        found
    }

    /// `Entity.handleLavaMovement`
    pub fn handle_lava_movement(&self, w: &dyn BlockView) -> bool {
        material_in_bb(
            w,
            &Self::expand(&self.bb, -0.1f32 as f64, -0.4f32 as f64, -0.1f32 as f64),
            Material::Lava,
        )
    }

    /// `Entity.isWet`
    pub fn is_wet(&self, w: &dyn BlockView) -> bool {
        let x = math::floor_double(self.pos[0]);
        let z = math::floor_double(self.pos[2]);
        self.in_water
            || w.lightning_at(x, math::floor_double(self.pos[1]), z)
            || w.lightning_at(x, math::floor_double(self.pos[1] + self.height as f64), z)
    }

    /// `Entity.isInsideOfMaterial`
    pub fn is_inside_of_material(&self, w: &dyn BlockView, m: Material, eye: f32) -> bool {
        let y = self.pos[1] + eye as f64;
        let bx = math::floor_double(self.pos[0]);
        let by = math::floor_float(math::floor_double(y) as f32);
        let bz = math::floor_double(self.pos[2]);
        if block::material(w.peek_block(bx, by, bz)) == m {
            let f = liquid_height_percent(w.peek_meta(bx, by, bz) as i32) - 0.11111111f32;
            let f1 = (by + 1) as f32 - f;
            y < f1 as f64
        } else {
            false
        }
    }

    /// `Entity.pushOutOfBlocks`
    pub fn push_out_of_blocks(&mut self, w: &dyn BlockView, x: f64, y: f64, z: f64) -> bool {
        let (bx, by, bz) = (
            math::floor_double(x),
            math::floor_double(y),
            math::floor_double(z),
        );
        let (fx, fy, fz) = (x - bx as f64, y - by as f64, z - bz as f64);
        if colliding_boxes(w, &self.bb).is_empty() && !full_cube(w, bx, by, bz) {
            return false;
        }
        let xn = !full_cube(w, bx - 1, by, bz);
        let xp = !full_cube(w, bx + 1, by, bz);
        let _yn = !full_cube(w, bx, by - 1, bz);
        let yp = !full_cube(w, bx, by + 1, bz);
        let zn = !full_cube(w, bx, by, bz - 1);
        let zp = !full_cube(w, bx, by, bz + 1);
        let mut side = 3;
        let mut best = 9999.0f64;
        if xn && fx < best {
            best = fx;
            side = 0;
        }
        if xp && 1.0 - fx < best {
            best = 1.0 - fx;
            side = 1;
        }
        if yp && 1.0 - fy < best {
            best = 1.0 - fy;
            side = 3;
        }
        if zn && fz < best {
            best = fz;
            side = 4;
        }
        if zp && 1.0 - fz < best {
            side = 5;
        }
        let f = (self.rand.next_float() * 0.2 + 0.1) as f64;
        match side {
            0 => self.motion[0] = -f,
            1 => self.motion[0] = f,
            2 => self.motion[1] = -f,
            3 => self.motion[1] = f,
            4 => self.motion[2] = -f,
            _ => self.motion[2] = f,
        }
        true
    }

    /// `Entity.moveEntity`. `sneak_edge` is `onGround && isSneaking() &&
    /// this instanceof EntityPlayer`; `extra` are the boxes of entities
    /// (`getBoundingBox` / `getCollisionBox`) the move may hit.
    pub fn move_entity(
        &mut self,
        w: &dyn BlockView,
        mut dx: f64,
        mut dy: f64,
        mut dz: f64,
        sneak_edge: bool,
        extra: &[Aabb],
        fx: &mut Vec<Fx>,
    ) {
        if self.no_clip {
            self.bb.offset(dx, dy, dz);
            self.pos = [
                (self.bb.min[0] + self.bb.max[0]) / 2.0,
                self.bb.min[1] + self.y_offset as f64 - self.y_size as f64,
                (self.bb.min[2] + self.bb.max[2]) / 2.0,
            ];
            return;
        }
        self.y_size *= 0.4;
        let start = self.pos;
        if self.in_web {
            self.in_web = false;
            dx *= 0.25;
            dy *= 0.05f32 as f64;
            dz *= 0.25;
            self.motion = [0.0; 3];
        }
        let (mut ox, oy, mut oz) = (dx, dy, dz);
        let orig = self.bb;
        let boxes = |bb: &Aabb| -> Vec<Aabb> {
            let mut v = colliding_boxes(w, bb);
            v.extend(extra.iter().filter(|b| b.intersects(bb)).copied());
            v
        };
        let sneak = sneak_edge && self.on_ground;
        if sneak {
            let step = 0.05;
            while dx != 0.0 && boxes(&self.bb.offset_copy(dx, -1.0, 0.0)).is_empty() {
                dx = shrink(dx, step);
                ox = dx;
            }
            while dz != 0.0 && boxes(&self.bb.offset_copy(0.0, -1.0, dz)).is_empty() {
                dz = shrink(dz, step);
                oz = dz;
            }
            while dx != 0.0 && dz != 0.0 && boxes(&self.bb.offset_copy(dx, -1.0, dz)).is_empty() {
                dx = shrink(dx, step);
                dz = shrink(dz, step);
                ox = dx;
                oz = dz;
            }
        }
        let list = boxes(&self.bb.add_coord(dx, dy, dz));
        for b in &list {
            dy = b.calc_offset(&self.bb, 1, dy);
        }
        self.bb.offset(0.0, dy, 0.0);
        let stepping = self.on_ground || (oy != dy && oy < 0.0);
        for b in &list {
            dx = b.calc_offset(&self.bb, 0, dx);
        }
        self.bb.offset(dx, 0.0, 0.0);
        for b in &list {
            dz = b.calc_offset(&self.bb, 2, dz);
        }
        self.bb.offset(0.0, 0.0, dz);
        if self.step_height > 0.0
            && stepping
            && (sneak || self.y_size < 0.05)
            && (ox != dx || oz != dz)
        {
            let (sx, sy, sz) = (dx, dy, dz);
            dx = ox;
            dy = self.step_height as f64;
            dz = oz;
            let flat = self.bb;
            self.bb = orig;
            let list = boxes(&self.bb.add_coord(dx, dy, dz));
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
            if oy != dy {
                dx = 0.0;
                dy = 0.0;
                dz = 0.0;
            } else {
                dy = -self.step_height as f64;
                for b in &list {
                    dy = b.calc_offset(&self.bb, 1, dy);
                }
                self.bb.offset(0.0, dy, 0.0);
            }
            if sx * sx + sz * sz >= dx * dx + dz * dz {
                dx = sx;
                dy = sy;
                dz = sz;
                self.bb = flat;
            }
        }
        self.pos = [
            (self.bb.min[0] + self.bb.max[0]) / 2.0,
            self.bb.min[1] + self.y_offset as f64 - self.y_size as f64,
            (self.bb.min[2] + self.bb.max[2]) / 2.0,
        ];
        self.collided_h = ox != dx || oz != dz;
        self.collided_v = oy != dy;
        self.on_ground = oy != dy && oy < 0.0;
        self.collided = self.collided_h || self.collided_v;
        // updateFallState
        if self.on_ground {
            if self.fall_distance > 0.0 {
                fx.push(Fx::Fall(self.fall_distance));
                self.fall_distance = 0.0;
            }
        } else if dy < 0.0 {
            self.fall_distance = (self.fall_distance as f64 - dy) as f32;
        }
        if ox != dx {
            self.motion[0] = 0.0;
        }
        if oy != dy {
            self.motion[1] = 0.0;
        }
        if oz != dz {
            self.motion[2] = 0.0;
        }
        let mx = self.pos[0] - start[0];
        let mut my = self.pos[1] - start[1];
        let mz = self.pos[2] - start[2];
        if self.can_trigger_walking && !sneak && self.riding.is_none() {
            let bx = math::floor_double(self.pos[0]);
            let by = math::floor_double(self.pos[1] - 0.2f32 as f64 - self.y_offset as f64);
            let bz = math::floor_double(self.pos[2]);
            let mut b = w.peek_block(bx, by, bz);
            let below = w.peek_block(bx, by - 1, bz);
            // render types 11 (fence), 32 (wall), 21 (fence gate)
            if block::render_type(below).is_some_and(|t| t == 11 || t == 32 || t == 21) {
                b = below;
            }
            if b != block::LADDER {
                my = 0.0;
            }
            self.dist_walked_mod = (self.dist_walked_mod as f64
                + math::sqrt_double(mx * mx + mz * mz) as f64 * 0.6)
                as f32;
            self.dist_walked_on_step = (self.dist_walked_on_step as f64
                + math::sqrt_double(mx * mx + my * my + mz * mz) as f64 * 0.6)
                as f32;
            if self.dist_walked_on_step > self.next_step_distance as f32
                && block::material(b) != Material::Air
            {
                self.next_step_distance = self.dist_walked_on_step as i32 + 1;
                if self.in_water {
                    let m = self.motion;
                    let mut f = math::sqrt_double(
                        m[0] * m[0] * 0.2f32 as f64 + m[1] * m[1] + m[2] * m[2] * 0.2f32 as f64,
                    ) * 0.35f32;
                    if f > 1.0 {
                        f = 1.0;
                    }
                    let p = 1.0 + (self.rand.next_float() - self.rand.next_float()) * 0.4;
                    fx.push(Fx::Sound("game.neutral.swim", self.pos, f, p));
                }
                // playStepSound
                if w.peek_block(bx, by + 1, bz) == block::SNOW_LAYER {
                    let s = block::step_sound(block::SNOW_LAYER);
                    fx.push(Fx::Sound(s.step, self.pos, s.volume * 0.15, s.pitch));
                } else if !block::material(b).is_liquid() {
                    let s = block::step_sound(b);
                    fx.push(Fx::Sound(s.step, self.pos, s.volume * 0.15, s.pitch));
                }
                fx.push(Fx::Walk(bx, by, bz, b));
            }
        }
        self.do_block_collisions(w, fx);
        let wet = self.is_wet(w);
        if flammable_within(w, &Self::expand(&self.bb, -0.001, -0.001, -0.001)) {
            // dealFireDamage(1)
            fx.push(Fx::Damage(Source::InFire, 1.0));
            if !wet {
                self.fire += 1;
                if self.fire == 0 {
                    self.set_fire(8);
                }
            }
        } else if self.fire <= 0 {
            self.fire = -self.fire_resistance;
        }
        if wet && self.fire > 0 {
            let p = 1.6 + (self.rand.next_float() - self.rand.next_float()) * 0.4;
            fx.push(Fx::Sound("random.fizz", self.pos, 0.7, p));
            self.fire = -self.fire_resistance;
        }
    }

    /// `Entity.doBlockCollisions`
    pub fn do_block_collisions(&self, w: &dyn BlockView, fx: &mut Vec<Fx>) {
        let x0 = math::floor_double(self.bb.min[0] + 0.001);
        let y0 = math::floor_double(self.bb.min[1] + 0.001);
        let z0 = math::floor_double(self.bb.min[2] + 0.001);
        let x1 = math::floor_double(self.bb.max[0] - 0.001);
        let y1 = math::floor_double(self.bb.max[1] - 0.001);
        let z1 = math::floor_double(self.bb.max[2] - 0.001);
        if chunks_exist(w, x0, y0, z0, x1, y1, z1) {
            for x in x0..=x1 {
                for y in y0..=y1 {
                    for z in z0..=z1 {
                        let b = w.peek_block(x, y, z);
                        if b != block::AIR {
                            fx.push(Fx::Collide(x, y, z, b));
                        }
                    }
                }
            }
        }
    }
}

fn shrink(d: f64, step: f64) -> f64 {
    if d < step && d >= -step {
        0.0
    } else if d > 0.0 {
        d - step
    } else {
        d + step
    }
}

/// `BlockLiquid.getLiquidHeightPercent`
pub fn liquid_height_percent(meta: i32) -> f32 {
    let m = if meta >= 8 { 0 } else { meta };
    (m + 1) as f32 / 9.0
}

/// `World.checkChunksExist`
pub fn chunks_exist(
    w: &dyn BlockView,
    x0: i32,
    y0: i32,
    z0: i32,
    x1: i32,
    y1: i32,
    z1: i32,
) -> bool {
    if y1 < 0 || y0 >= 256 {
        return false;
    }
    for cx in (x0 >> 4)..=(x1 >> 4) {
        for cz in (z0 >> 4)..=(z1 >> 4) {
            if !w.loaded(cx << 4, cz << 4) {
                return false;
            }
        }
    }
    true
}

/// `World.isMaterialInBB`
pub fn material_in_bb(w: &dyn BlockView, bb: &Aabb, m: Material) -> bool {
    let x0 = math::floor_double(bb.min[0]);
    let x1 = math::floor_double(bb.max[0] + 1.0);
    let y0 = math::floor_double(bb.min[1]);
    let y1 = math::floor_double(bb.max[1] + 1.0);
    let z0 = math::floor_double(bb.min[2]);
    let z1 = math::floor_double(bb.max[2] + 1.0);
    for x in x0..x1 {
        for y in y0..y1 {
            for z in z0..z1 {
                if block::material(w.peek_block(x, y, z)) == m {
                    return true;
                }
            }
        }
    }
    false
}

/// `World.isAnyLiquid`
pub fn any_liquid(w: &dyn BlockView, bb: &Aabb) -> bool {
    let mut x0 = math::floor_double(bb.min[0]);
    let x1 = math::floor_double(bb.max[0] + 1.0);
    let mut y0 = math::floor_double(bb.min[1]);
    let y1 = math::floor_double(bb.max[1] + 1.0);
    let mut z0 = math::floor_double(bb.min[2]);
    let z1 = math::floor_double(bb.max[2] + 1.0);
    if bb.min[0] < 0.0 {
        x0 -= 1;
    }
    if bb.min[1] < 0.0 {
        y0 -= 1;
    }
    if bb.min[2] < 0.0 {
        z0 -= 1;
    }
    for x in x0..x1 {
        for y in y0..y1 {
            for z in z0..z1 {
                if block::material(w.peek_block(x, y, z)).is_liquid() {
                    return true;
                }
            }
        }
    }
    false
}

/// `World.isFlammableWithin`
pub fn flammable_within(w: &dyn BlockView, bb: &Aabb) -> bool {
    let x0 = math::floor_double(bb.min[0]);
    let x1 = math::floor_double(bb.max[0] + 1.0);
    let y0 = math::floor_double(bb.min[1]);
    let y1 = math::floor_double(bb.max[1] + 1.0);
    let z0 = math::floor_double(bb.min[2]);
    let z1 = math::floor_double(bb.max[2] + 1.0);
    if chunks_exist(w, x0, y0, z0, x1, y1, z1) {
        for x in x0..x1 {
            for y in y0..y1 {
                for z in z0..z1 {
                    let b = w.peek_block(x, y, z);
                    if b == block::FIRE || b == block::FLOWING_LAVA || b == block::LAVA {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// `World.isBlockFullCube`: the collision box has an average edge of 1.
pub fn full_cube(w: &dyn BlockView, x: i32, y: i32, z: i32) -> bool {
    let id = w.peek_block(x, y, z);
    if id == block::AIR {
        return false;
    }
    let get = |x: i32, y: i32, z: i32| (w.peek_block(x, y, z), w.peek_meta(x, y, z));
    let mut boxes = Vec::new();
    crate::shapes::collision(&get, id, w.peek_meta(x, y, z), x, y, z, &mut boxes);
    // getCollisionBoundingBoxFromPool returns one box: the union for
    // blocks made of several.
    if boxes.is_empty() {
        return false;
    }
    let mut u = boxes[0];
    for b in &boxes[1..] {
        for a in 0..3 {
            u[a] = u[a].min(b[a]);
            u[a + 3] = u[a + 3].max(b[a + 3]);
        }
    }
    let avg = ((u[3] - u[0]) as f64 + (u[4] - u[1]) as f64 + (u[5] - u[2]) as f64) / 3.0;
    avg >= 1.0
}
