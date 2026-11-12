//! The client side of boats: `NetHandlerPlayClient` (spawn, move,
//! velocity, metadata, attach, destroy), the client's `EntityBoat.onUpdate`
//! and the local player's `updateRidden`, and `RenderBoat`.

use super::*;
use crate::entity::base::{Ent, Fx};
use crate::entity::boat::{Boat, BoatWorld, Rider};
use crate::entity::player::Aabb;
use crate::entity::tracker::{byte_angle, Packet, ServerPos};
use crate::render::mesher::Vertex;

/// `EntityPlayer.getYOffset()` on the client: `yOffset - 0.5F`.
const PLAYER_Y_OFFSET: f64 = (1.62f32 - 0.5f32) as f64;

/// A boat in the client's world.
pub(super) struct ClientBoat {
    pub b: Boat,
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
    /// `handleSpawnObject` (type 1), `handleEntityMovement`,
    /// `handleEntityTeleport`, `handleEntityVelocity`, `handleDestroyEntities`.
    pub(super) fn vehicle_packets(&mut self, list: Vec<(u32, Packet)>) {
        for (id, p) in list {
            match p {
                Packet::Spawn {
                    pos,
                    yaw,
                    pitch,
                    velocity,
                } => {
                    self.boats.retain(|b| b.b.id != id);
                    let seed = self.rng.next_long();
                    let mut b = Boat::at(
                        id,
                        pos[0] as f64 / 32.0,
                        pos[1] as f64 / 32.0,
                        pos[2] as f64 / 32.0,
                        seed,
                    );
                    b.e.pitch = byte_angle(pitch);
                    b.e.yaw = byte_angle(yaw);
                    b.e.last_tick = b.e.pos;
                    b.e.prev_yaw = b.e.yaw;
                    if let Some(v) = velocity {
                        b.set_velocity([
                            v[0] as f64 / 8000.0,
                            v[1] as f64 / 8000.0,
                            v[2] as f64 / 8000.0,
                        ]);
                    }
                    self.boats.push(ClientBoat {
                        b,
                        sp: ServerPos { pos },
                    });
                }
                Packet::Velocity(v) => {
                    if let Some(c) = self.boats.iter_mut().find(|c| c.b.id == id) {
                        c.b.set_velocity([
                            v[0] as f64 / 8000.0,
                            v[1] as f64 / 8000.0,
                            v[2] as f64 / 8000.0,
                        ]);
                    }
                }
                Packet::Destroy => {
                    self.boats.retain(|b| b.b.id != id);
                    self.world.solid_entities.retain(|e| e.0 != id);
                    if self.riding == Some(id) {
                        // Entity.updateRidden: ridingEntity.isDead
                        self.riding = None;
                        self.player.riding = false;
                    }
                }
                _ => {
                    if let Some(c) = self.boats.iter_mut().find(|c| c.b.id == id) {
                        if let Some((at, y, pi)) = c.sp.apply(&p, c.b.e.yaw, c.b.e.pitch) {
                            c.b.set_position_and_rotation2(at, y, pi, 3);
                        }
                    }
                }
            }
        }
    }

    /// `handleEntityMetadata` for boats.
    pub(super) fn vehicle_meta(&mut self, list: Vec<(u32, i32, i32, f32)>) {
        for (id, hit, fwd, dmg) in list {
            if let Some(c) = self.boats.iter_mut().find(|c| c.b.id == id) {
                c.b.time_since_hit = hit;
                c.b.forward = fwd;
                c.b.damage = dmg;
            }
        }
    }

    /// `handleEntityAttach` for the player.
    pub(super) fn mounted(&mut self, v: Option<u32>) {
        match v {
            Some(id) => {
                let first = self.riding.is_none();
                if let Some(c) = self.boats.iter_mut().find(|c| c.b.id == id) {
                    c.b.empty = false;
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
                // EntityPlayer.mountEntity(null) on the client: the boat
                // stays isBoatEmpty = false
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
            if c.b.id != except {
                v.push(c.b.e.bb);
            }
        }
        v
    }

    /// `World.updateEntities` for the client's boats.
    pub(super) fn tick_boats(&mut self) {
        let n = self.boats.len();
        for i in 0..n {
            let id = self.boats[i].b.id;
            let others = self.client_entity_boxes(id);
            let rider = (self.riding == Some(id)).then(|| Rider {
                yaw: self.player.yaw,
                strafe: self.player.move_strafe,
                forward: self.player.move_forward,
            });
            let mut fx = Vec::new();
            {
                let mut world = std::mem::take(&mut self.world);
                let b = &mut self.boats[i].b;
                // updateEntityWithOptionalForce
                b.e.last_tick = b.e.pos;
                b.e.prev_yaw = b.e.yaw;
                b.e.prev_pitch = b.e.pitch;
                b.e.ticks_existed += 1;
                let _ = b.tick(&mut world, true, rider, &others, &mut fx);
                self.world = world;
            }
            let bb = self.boats[i].b.bounding_box();
            match self.world.solid_entities.iter_mut().find(|e| e.0 == id) {
                Some(e) => e.1 = bb,
                None => self.world.solid_entities.push((id, bb)),
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

    /// `updateRiderPosition` of the local player's boat, after the
    /// player's own update (`Entity.updateRidden`).
    pub(super) fn update_rider(&mut self) {
        let Some(id) = self.riding else {
            return;
        };
        let Some(c) = self.boats.iter().find(|c| c.b.id == id) else {
            return;
        };
        let r = c.b.rider_position(PLAYER_Y_OFFSET);
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

    /// `RenderBoat` for every boat.
    pub(super) fn draw_boats(&self, out: &mut Vec<Vertex>, eye: [f64; 3], partial: f32) {
        let Some(t) = self.atlas.mob_tex("boat") else {
            return;
        };
        for c in &self.boats {
            let e = &c.b.e;
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
            // Entity.getBrightnessForRender
            let ly =
                math::floor_double(p[1] - e.y_offset as f64 + (e.bb.max[1] - e.bb.min[1]) * 0.66);
            let (lx, lz) = (math::floor_double(p[0]), math::floor_double(p[2]));
            let sky = self.world.light(true, lx, ly, lz);
            let blk = self.world.light(false, lx, ly, lz);
            let light = [(blk << 4) as u8, (sky << 4) as u8, 0, 0];
            crate::render::vehicles::boat(
                out,
                t,
                rel,
                yaw,
                c.b.time_since_hit,
                c.b.damage,
                c.b.forward,
                partial,
                light,
            );
        }
    }
}
