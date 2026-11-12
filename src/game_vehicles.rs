//! The client side of boats and minecarts: `NetHandlerPlayClient` (spawn,
//! move, velocity, metadata, status, attach, destroy), the client's
//! `onUpdate`, the local player's `updateRidden`, `RenderBoat` and
//! `RenderMinecart`.

use super::*;
use crate::entity::base::{Ent, Fx};
use crate::entity::boat::{BoatWorld, Rider};
use crate::entity::player::Aabb;
use crate::entity::tracker::{byte_angle, Packet, ServerPos};
use crate::entity::vehicle::{VMeta, Vehicle};
use crate::render::mesher::Vertex;

/// `EntityPlayer.getYOffset()` on the client: `yOffset - 0.5F`.
const PLAYER_Y_OFFSET: f64 = (1.62f32 - 0.5f32) as f64;

/// A vehicle in the client's world.
pub(super) struct ClientVehicle {
    pub v: Vehicle,
    /// `serverPosX/Y/Z`
    pub sp: ServerPos,
}

impl ClientWorld {
    /// A block change the client makes itself (the server's chunk update
    /// follows).
    pub(super) fn set_local(&mut self, x: i32, y: i32, z: i32, id: u8, meta: u8) {
        if !(0..256).contains(&y) {
            return;
        }
        if let Some(c) = self.chunks.get_mut(&(x >> 4, z >> 4)) {
            let d = std::sync::Arc::make_mut(&mut c.mc.data);
            let i = Self::index(x, y, z);
            d.blocks[i] = id;
            d.meta[i] = meta;
        }
    }
}

impl BoatWorld for ClientWorld {
    fn set_air(&mut self, x: i32, y: i32, z: i32) {
        self.set_local(x, y, z, block::AIR, 0);
    }
    fn break_block(&mut self, x: i32, y: i32, z: i32) {
        // World.func_147480_a on the client: the effect, no drops
        let b = self.peek_block(x, y, z);
        if block::material(b) == Material::Air {
            return;
        }
        let m = self.peek_meta(x, y, z) as i32;
        self.destroyed.push(([x, y, z], b, m));
        self.set_local(x, y, z, block::AIR, 0);
    }
    fn collided(&mut self, e: &mut Ent, _x: i32, _y: i32, _z: i32, id: u8) -> Option<f32> {
        // the client half of Block.onEntityCollidedWithBlock
        match id {
            block::WEB => {
                e.in_web = true;
                e.fall_distance = 0.0;
                None
            }
            block::SOUL_SAND => {
                e.motion[0] *= 0.4;
                e.motion[2] *= 0.4;
                None
            }
            block::CACTUS => Some(1.0),
            block::PORTAL => {
                if e.riding.is_none() && e.ridden_by.is_none() {
                    e.set_in_portal();
                }
                None
            }
            _ => None,
        }
    }
}

impl Game {
    /// `handleSpawnObject` (types 1 and 10), `handleEntityMovement`,
    /// `handleEntityTeleport`, `handleEntityVelocity`, `handleDestroyEntities`.
    pub(super) fn vehicle_packets(&mut self, list: Vec<(u32, Packet)>) {
        for (id, p) in list {
            match p {
                Packet::Spawn {
                    object,
                    data,
                    pos,
                    yaw,
                    pitch,
                    velocity,
                } => {
                    self.boats.retain(|b| b.v.id() != id);
                    let seed = self.rng.next_long();
                    let at = [
                        pos[0] as f64 / 32.0,
                        pos[1] as f64 / 32.0,
                        pos[2] as f64 / 32.0,
                    ];
                    let Some(mut v) = Vehicle::from_spawn(object, data, id, at, seed) else {
                        continue;
                    };
                    {
                        let e = v.e_mut();
                        e.pitch = byte_angle(pitch);
                        e.yaw = byte_angle(yaw);
                        e.last_tick = e.pos;
                        e.prev_yaw = e.yaw;
                    }
                    if let Some(s) = velocity {
                        v.set_velocity([
                            s[0] as f64 / 8000.0,
                            s[1] as f64 / 8000.0,
                            s[2] as f64 / 8000.0,
                        ]);
                    }
                    self.boats.push(ClientVehicle {
                        v,
                        sp: ServerPos { pos },
                    });
                }
                Packet::Velocity(s) => {
                    if let Some(c) = self.boats.iter_mut().find(|c| c.v.id() == id) {
                        c.v.set_velocity([
                            s[0] as f64 / 8000.0,
                            s[1] as f64 / 8000.0,
                            s[2] as f64 / 8000.0,
                        ]);
                    }
                }
                Packet::Destroy => {
                    self.boats.retain(|b| b.v.id() != id);
                    self.world.solid_entities.retain(|e| e.0 != id);
                    if self.riding == Some(id) {
                        // Entity.updateRidden: ridingEntity.isDead
                        self.riding = None;
                        self.player.riding = false;
                    }
                }
                _ => {
                    if let Some(c) = self.boats.iter_mut().find(|c| c.v.id() == id) {
                        let (yaw, pitch) = (c.v.e().yaw, c.v.e().pitch);
                        if let Some((at, y, pi)) = c.sp.apply(&p, yaw, pitch) {
                            c.v.set_position_and_rotation2(at, y, pi, 3);
                        }
                    }
                }
            }
        }
    }

    /// `handleEntityMetadata` for vehicles.
    pub(super) fn vehicle_meta(&mut self, list: Vec<(u32, VMeta)>) {
        for (id, m) in list {
            if let Some(c) = self.boats.iter_mut().find(|c| c.v.id() == id) {
                c.v.set_meta(m);
            }
        }
    }

    /// `handleEntityStatus` for vehicles: 10 ignites a TNT minecart.
    pub(super) fn vehicle_status(&mut self, list: Vec<(u32, i32)>) {
        for (id, s) in list {
            if let Some(ClientVehicle {
                v: Vehicle::Cart(c),
                ..
            }) = self.boats.iter_mut().find(|c| c.v.id() == id)
            {
                if s == 10 {
                    // EntityMinecartTNT.ignite on the client
                    c.fuse = 80;
                }
            }
        }
    }

    /// `handleEntityAttach` for the player.
    pub(super) fn mounted(&mut self, v: Option<u32>) {
        match v {
            Some(id) => {
                let first = self.riding.is_none();
                if let Some(c) = self.boats.iter_mut().find(|c| c.v.id() == id) {
                    if let Vehicle::Boat(b) = &mut c.v {
                        b.empty = false;
                    }
                    self.riding = Some(id);
                    self.player.riding = true;
                    if first {
                        // GuiIngame.setRecordPlaying(I18n.format("mount.onboard", ...), false)
                        let key = self.lang.format("mount.onboard", &["LSHIFT".to_string()]);
                        self.record = Some((key, 60, false));
                    }
                }
            }
            None => {
                // EntityPlayer.mountEntity(null) on the client: a boat stays
                // isBoatEmpty = false
                self.riding = None;
                self.player.riding = false;
            }
        }
    }

    /// The boxes of the client's other entities near (for the boats'
    /// `getCollisionBox`).
    fn client_entity_boxes(&self, except: u32) -> Vec<Aabb> {
        let mut v = vec![self.player.bb];
        for m in &self.mobs {
            v.push(Aabb {
                min: [m.bb[0], m.bb[1], m.bb[2]],
                max: [m.bb[3], m.bb[4], m.bb[5]],
            });
        }
        for it in &self.items {
            let p = it.pos;
            v.push(Aabb {
                min: [p[0] - 0.125, p[1] - 0.125, p[2] - 0.125],
                max: [p[0] + 0.125, p[1] + 0.125, p[2] + 0.125],
            });
        }
        for c in &self.boats {
            if c.v.id() != except {
                v.push(c.v.e().bb);
            }
        }
        v
    }

    /// `World.updateEntities` for the client's vehicles.
    pub(super) fn tick_boats(&mut self) {
        let n = self.boats.len();
        for i in 0..n {
            let id = self.boats[i].v.id();
            let boat = matches!(self.boats[i].v, Vehicle::Boat(_));
            let others = if boat {
                self.client_entity_boxes(id)
            } else {
                Vec::new()
            };
            let rider = (self.riding == Some(id)).then(|| Rider {
                yaw: self.player.yaw,
                strafe: self.player.move_strafe,
                forward: self.player.move_forward,
            });
            let mut fx = Vec::new();
            {
                let mut world = std::mem::take(&mut self.world);
                let v = &mut self.boats[i].v;
                // updateEntityWithOptionalForce
                let e = v.e_mut();
                e.last_tick = e.pos;
                e.prev_yaw = e.yaw;
                e.prev_pitch = e.pitch;
                e.ticks_existed += 1;
                match v {
                    Vehicle::Boat(b) => {
                        let _ = b.tick(&mut world, true, rider, &others, &mut fx);
                    }
                    Vehicle::Cart(c) => {
                        let mut out = Vec::new();
                        c.tick(&mut world, true, rider, &others, &mut fx, &mut out);
                    }
                }
                self.world = world;
            }
            if let Some(bb) = self.boats[i].v.bounding_box() {
                match self.world.solid_entities.iter_mut().find(|e| e.0 == id) {
                    Some(e) => e.1 = bb,
                    None => self.world.solid_entities.push((id, bb)),
                }
            }
            for f in fx {
                match f {
                    Fx::Sound(name, p, v, pi) => self.sound(name, Some(p), v, pi),
                    Fx::Particle(name, p, m) => self.named_particle(name, p, m),
                    _ => {}
                }
            }
        }
        let destroyed = std::mem::take(&mut self.world.destroyed);
        self.destroyed_fx(destroyed);
    }

    /// `updateRiderPosition` of the local player's vehicle, after the
    /// player's own update (`Entity.updateRidden`).
    pub(super) fn update_rider(&mut self) {
        let Some(id) = self.riding else {
            return;
        };
        let Some(c) = self.boats.iter().find(|c| c.v.id() == id) else {
            return;
        };
        let r = c.v.rider_position(PLAYER_Y_OFFSET);
        self.player.set_eye_position(r[0], r[1], r[2]);
        // EntityPlayer.updateRidden
        self.player.prev_camera_yaw = self.player.camera_yaw;
        self.player.camera_yaw = 0.0;
    }

    /// `RenderGlobal.playAuxSFX(2001)`: the block's break sound and particles.
    pub(super) fn destroyed_fx(&mut self, list: Vec<([i32; 3], u8, i32)>) {
        for (p, id, meta) in list {
            self.block_sound(id, p, "dig");
            let (dt, dti) = self.debris_of(id, meta as u8);
            self.particles.destroy(p, dt, dti);
        }
    }

    /// `RenderBoat` and `RenderMinecart` for every vehicle.
    pub(super) fn draw_boats(&self, out: &mut Vec<Vertex>, eye: [f64; 3], partial: f32) {
        let boat_tex = self.atlas.mob_tex("boat");
        let cart_tex = self.atlas.mob_tex("minecart");
        let sub = sim::skylight_subtracted(self.world_time);
        for c in &self.boats {
            let e = c.v.e();
            let tp = partial as f64;
            let p = [
                e.last_tick[0] + (e.pos[0] - e.last_tick[0]) * tp,
                e.last_tick[1] + (e.pos[1] - e.last_tick[1]) * tp,
                e.last_tick[2] + (e.pos[2] - e.last_tick[2]) * tp,
            ];
            let yaw = e.prev_yaw + (e.yaw - e.prev_yaw) * partial;
            let rel = [
                (p[0] - eye[0]) as f32,
                (p[1] - eye[1]) as f32,
                (p[2] - eye[2]) as f32,
            ];
            // Entity.getBrightnessForRender / getBrightness
            let ly =
                math::floor_double(p[1] - e.y_offset as f64 + (e.bb.max[1] - e.bb.min[1]) * 0.66);
            let (lx, lz) = (math::floor_double(p[0]), math::floor_double(p[2]));
            let sky = self.world.light(true, lx, ly, lz);
            let blk = self.world.light(false, lx, ly, lz);
            let light = [(blk << 4) as u8, (sky << 4) as u8, 0, 0];
            match &c.v {
                Vehicle::Boat(b) => {
                    if let Some(t) = boat_tex {
                        crate::render::vehicles::boat(
                            out,
                            t,
                            rel,
                            yaw,
                            b.time_since_hit,
                            b.damage,
                            b.forward,
                            partial,
                            light,
                        );
                    }
                }
                Vehicle::Cart(m) => {
                    let Some(t) = cart_tex else { continue };
                    let brightness = self.light_table[((sky - sub).max(blk)).clamp(0, 15) as usize];
                    let pitch = e.prev_pitch + (e.pitch - e.prev_pitch) * partial;
                    let atlas = &self.atlas;
                    crate::render::vehicles::minecart(
                        out,
                        t,
                        &self.world,
                        m,
                        p,
                        rel,
                        yaw,
                        pitch,
                        partial,
                        light,
                        &|o, id, meta, mat, l| {
                            crate::render::entities::block_model_lit(
                                o, atlas, id, meta, mat, l, brightness,
                            )
                        },
                    );
                }
            }
        }
    }

    /// `S27PacketExplosion`: the client's `Explosion.doExplosionB(true)`.
    pub(super) fn explosion_b(&mut self, e: crate::world::explosion::View) {
        let c = e.pos;
        let r = &mut self.particle_rand;
        let pitch = (1.0 + (r.next_float() - r.next_float()) * 0.2) * 0.7;
        self.sound("random.explode", Some(c), 4.0, pitch);
        if e.size >= 2.0 && e.smoking {
            self.particles.huge_explosion(c);
        } else {
            self.particles.large_explode(c, 1.0);
        }
        if e.smoking {
            for [x, y, z] in e.blocks {
                let r = &mut self.particle_rand;
                let px = x as f64 + r.next_float() as f64;
                let py = y as f64 + r.next_float() as f64;
                let pz = z as f64 + r.next_float() as f64;
                let mut d = [px - c[0], py - c[1], pz - c[2]];
                let l = math::sqrt_double(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]) as f64;
                for v in &mut d {
                    *v /= l;
                }
                let mut k = 0.5 / (l / e.size as f64 + 0.1);
                k *= (r.next_float() * r.next_float() + 0.3) as f64;
                for v in &mut d {
                    *v *= k;
                }
                self.particles
                    .explode([(px + c[0]) / 2.0, (py + c[1]) / 2.0, (pz + c[2]) / 2.0], d);
                self.particles.smoke([px, py, pz], d, false);
                // setBlock(air) on the client
                if self.world.peek_block(x, y, z) != block::AIR {
                    self.world.set_local(x, y, z, block::AIR, 0);
                }
            }
        }
    }
}
