//! The server side of boats: `ItemBoat.onItemRightClick`, the entity ticks
//! in `World.updateEntities` order (the boat, then its rider's
//! `updateRidden`), `interactFirst` / `mountEntity` / `dismountEntity`,
//! hits, and what `EntityTrackerEntry` tells the client.

use super::*;
use crate::entity::boat::{Boat, Out, Rider, Tracked, VEHICLE_ID};
use crate::entity::player::Aabb;
use crate::entity::tracker::Packet;
use crate::util::math;

/// `EntityPlayer.getYOffset()` on the server, where `yOffset` is 0.
const PLAYER_Y_OFFSET_MP: f64 = -0.5f32 as f64;

impl Sim {
    /// The player's box from the reported feet (`EntityPlayerMP`).
    pub(super) fn player_box(feet: [f64; 3]) -> Aabb {
        Aabb {
            min: [feet[0] - 0.3f32 as f64, feet[1], feet[2] - 0.3f32 as f64],
            max: [
                feet[0] + 0.3f32 as f64,
                feet[1] + 1.8f32 as f64,
                feet[2] + 0.3f32 as f64,
            ],
        }
    }

    /// `ItemBoat.onItemRightClick`. Returns whether the boat item was used.
    pub(super) fn place_boat(&mut self) -> bool {
        let (feet, yaw) = self.play.player;
        let pitch = self.play.pitch;
        // prevPos + (pos - prevPos) * 1.0, posY + 1.62 - yOffset
        let start = [feet[0], feet[1] + 1.62, feet[2]];
        let r = std::f32::consts::PI;
        let f14 = math::cos(-yaw * (r / 180.0) - r);
        let f15 = math::sin(-yaw * (r / 180.0) - r);
        let f16 = -math::cos(-pitch * (r / 180.0));
        let f17 = math::sin(-pitch * (r / 180.0));
        let look = [(f15 * f16) as f64, f17 as f64, (f14 * f16) as f64];
        let reach = 5.0;
        let end = [
            start[0] + look[0] * reach,
            start[1] + look[1] * reach,
            start[2] + look[2] * reach,
        ];
        let Some(hit) =
            crate::entity::raytrace::ray_trace_blocks(&self.world, start, end, true, false, false)
        else {
            return false;
        };
        // EntityLivingBase.getLook(1.0F) is the same vector
        let area = crate::entity::base::Ent::expand(
            &Self::player_box(feet).add_coord(look[0] * reach, look[1] * reach, look[2] * reach),
            1.0,
            1.0,
            1.0,
        );
        let inside = |b: &Aabb, border: f64| {
            let g = crate::entity::base::Ent::expand(b, border, border, border);
            (0..3).all(|i| start[i] > g.min[i] && start[i] < g.max[i])
        };
        let mut blocked = false;
        for (b, border) in self.collidable_boxes() {
            if b.intersects(&area) && inside(&b, border) {
                blocked = true;
            }
        }
        if blocked {
            return false;
        }
        let (bx, mut by, bz) = (hit.x, hit.y, hit.z);
        if self.world.block(bx, by, bz) == block::SNOW_LAYER {
            by -= 1;
        }
        let id = self.alloc_vehicle_id();
        let seed = self.mobs.rand.next_long();
        let mut boat = Boat::at(
            id,
            (bx as f32 + 0.5) as f64,
            (by as f32 + 1.0) as f64,
            (bz as f32 + 0.5) as f64,
            seed,
        );
        boat.e.yaw = ((math::floor_double((yaw * 4.0 / 360.0) as f64 + 0.5) & 3) - 1) as f32 * 90.0;
        let probe = crate::entity::base::Ent::expand(&boat.e.bb, -0.1, -0.1, -0.1);
        let mut hits = crate::entity::player::colliding_boxes(&self.world, &probe);
        // getCollisionBox: every other entity's box
        hits.extend(
            self.entity_boxes(None)
                .into_iter()
                .filter(|b| b.intersects(&probe)),
        );
        if !hits.is_empty() {
            return true;
        }
        self.world.solid_entities.push((id, boat.bounding_box()));
        self.play.boats.push(Tracked::new(boat));
        if self.play.mode != GameMode::Creative {
            self.play.inv.consume_held();
            self.play.inv_dirty = true;
        }
        true
    }

    pub(super) fn alloc_vehicle_id(&mut self) -> u32 {
        let id = VEHICLE_ID | self.play.next_item;
        self.play.next_item += 1;
        id
    }

    /// Entities with `canBeCollidedWith` and their `getCollisionBorderSize`.
    fn collidable_boxes(&self) -> Vec<(Aabb, f64)> {
        let mut v = Vec::new();
        for m in &self.mobs.list {
            if !m.dead {
                v.push((m.bb, 0.1f32 as f64));
            }
        }
        for b in &self.play.boats {
            if !b.b.e.dead {
                v.push((b.b.e.bb, 0.1f32 as f64));
            }
        }
        for f in &self.play.falling {
            v.push((f.e.bb, 0.1f32 as f64));
        }
        for t in &self.mobs.tnt {
            let p = t.pos;
            v.push((
                Aabb {
                    min: [p[0] - 0.49, p[1] - 0.49, p[2] - 0.49],
                    max: [p[0] + 0.49, p[1] + 0.49, p[2] + 0.49],
                },
                0.1f32 as f64,
            ));
        }
        v
    }

    /// The boxes of every entity but `except` (for `getCollisionBox`).
    fn entity_boxes(&self, except: Option<u32>) -> Vec<Aabb> {
        self.entity_boxes_in(&self.play, except)
    }

    fn entity_boxes_in(&self, play: &Play, except: Option<u32>) -> Vec<Aabb> {
        let mut v = Vec::new();
        if !self.respawn_pending && !self.splayer.dead {
            v.push(Self::player_box(play.player.0));
        }
        for m in &self.mobs.list {
            if !m.dead {
                v.push(m.bb);
            }
        }
        for a in &self.mobs.arrows {
            v.push(a.e.bb);
        }
        for t in &self.mobs.throwables {
            v.push(t.e.bb);
        }
        for t in &self.mobs.tnt {
            let p = t.pos;
            v.push(Aabb {
                min: [p[0] - 0.49, p[1] - 0.49, p[2] - 0.49],
                max: [p[0] + 0.49, p[1] + 0.49, p[2] + 0.49],
            });
        }
        for it in &play.items {
            v.push(it.bb());
        }
        for f in &play.falling {
            v.push(f.e.bb);
        }
        for b in &play.boats {
            if Some(b.b.id) != except && !b.b.e.dead {
                v.push(b.b.e.bb);
            }
        }
        v
    }

    /// `Entity.entityDropItem(stack, 0.0F)`: an `EntityItem` at the entity
    /// with `delayBeforeCanPickup` 10.
    fn entity_drop(play: &mut Play, pos: [f64; 3], st: crate::item::Stack) {
        let id = play.next_item;
        play.next_item += 1;
        let mut r = std::mem::replace(&mut play.prand, JavaRandom::new(0));
        let mut it = crate::entity::item::ItemEntity::new(id, st, pos, &mut r);
        play.prand = r;
        it.pickup_delay = 10;
        play.items.push(it);
    }

    /// `EntityPlayerMP.mountEntity(boat)`: `S1BPacketEntityAttach` and
    /// `setPlayerLocation`.
    fn mount(&mut self, id: u32, events: &mut Vec<Event>) {
        if let Some(old) = self.riding.take() {
            if let Some(b) = self.play.boats.iter_mut().find(|b| b.b.id == old) {
                b.b.ridden = false;
            }
        }
        let Some(b) = self.play.boats.iter_mut().find(|b| b.b.id == id) else {
            return;
        };
        b.b.ridden = true;
        self.riding = Some(id);
        self.ride = Default::default();
        events.push(Event::Mount(Some(id)));
        let feet = self.play.player.0;
        self.teleport_to = Some(feet);
    }

    /// `EntityPlayer.mountEntity(null)` on the server:
    /// `EntityLivingBase.dismountEntity`, then the attach packet and
    /// `setPlayerLocation`.
    fn dismount(&mut self, events: &mut Vec<Event>) {
        let Some(id) = self.riding.take() else {
            return;
        };
        let Some(i) = self.play.boats.iter().position(|b| b.b.id == id) else {
            return;
        };
        self.play.boats[i].b.ridden = false;
        let v = &self.play.boats[i].b.e;
        let feet = self.play.player.0;
        let mut to = [v.pos[0], v.bb.min[1] + v.height as f64, v.pos[2]];
        let pb = Self::player_box(feet);
        'search: for dx in -1..=1 {
            for dz in -1..1 {
                if dx == 0 && dz == 0 {
                    continue;
                }
                let x = (feet[0] + dx as f64) as i32;
                let z = (feet[2] + dz as f64) as i32;
                let y = feet[1] as i32;
                let b = pb.offset_copy(dx as f64, 1.0, dz as f64);
                if crate::entity::player::blocks_colliding(&self.world, &b).is_empty() {
                    if self.world.solid_top_surface(x, y, z) {
                        to = [feet[0] + dx as f64, feet[1] + 1.0, feet[2] + dz as f64];
                        break 'search;
                    }
                    if self.world.solid_top_surface(x, y - 1, z)
                        || block::material(self.world.block(x, y - 1, z)) == block::Material::Water
                    {
                        to = [feet[0] + dx as f64, feet[1] + 1.0, feet[2] + dz as f64];
                    }
                }
            }
        }
        self.place_player(to);
        events.push(Event::Mount(None));
        self.teleport_to = Some(to);
    }

    /// The server moves the player (`setPosition`): feet.
    fn place_player(&mut self, feet: [f64; 3]) {
        self.play.player.0 = feet;
        self.splayer.place(feet);
        if let Some(p) = &mut self.player {
            p.0 = feet;
        }
    }

    /// `C0CPacketInput` (`setEntityActionState`).
    pub(super) fn ride_input(&mut self, strafe: f32, forward: f32, jump: bool, sneak: bool) {
        if self.riding.is_some() {
            if (-1.0..=1.0).contains(&strafe) {
                self.ride.strafe = strafe;
            }
            if (-1.0..=1.0).contains(&forward) {
                self.ride.forward = forward;
            }
            self.ride.jump = jump;
            self.ride.sneak = sneak;
            self.splayer.sneaking = sneak;
        }
    }

    /// `EntityBoat.interactFirst`
    pub(super) fn interact_vehicle(&mut self, id: u32, events: &mut Vec<Event>) {
        if self.play.boats.iter().any(|b| b.b.id == id && !b.b.e.dead) {
            self.mount(id, events);
        }
    }

    /// `EntityPlayer.attackTargetEntityWithCurrentItem` on a boat.
    pub(super) fn attack_vehicle(
        &mut self,
        id: u32,
        damage: f32,
        knockback: i32,
        events: &mut Vec<Event>,
    ) {
        let creative = self.play.mode == GameMode::Creative;
        let yaw = self.play.player.1;
        let Some(i) = self.play.boats.iter().position(|b| b.b.id == id) else {
            return;
        };
        if damage <= 0.0 {
            return;
        }
        let broke = self.play.boats[i].b.attack(damage, creative);
        if knockback > 0 {
            let r = std::f32::consts::PI;
            let k = knockback as f32 * 0.5;
            let b = &mut self.play.boats[i];
            b.b.e.motion[0] += (-math::sin(yaw * r / 180.0) * k) as f64;
            b.b.e.motion[1] += 0.1;
            b.b.e.motion[2] += (math::cos(yaw * r / 180.0) * k) as f64;
            b.airborne = true;
        }
        if let Some(drop) = broke {
            self.boat_broken(i, drop, true, events);
        }
    }

    /// The boat at `i` was killed by `attackEntityFrom`: the rider is
    /// mounted again (`riddenByEntity.mountEntity(this)`), the item drops.
    fn boat_broken(&mut self, i: usize, drop: bool, remount: bool, events: &mut Vec<Event>) {
        let id = self.play.boats[i].b.id;
        if remount && self.riding == Some(id) {
            // EntityPlayerMP.mountEntity: the attach packet and setPlayerLocation
            events.push(Event::Mount(Some(id)));
            self.teleport_to = Some(self.play.player.0);
        }
        if drop {
            let p = self.play.boats[i].b.e.pos;
            let mut play = std::mem::replace(&mut self.play, Play::new(GameMode::Survival));
            Self::entity_drop(&mut play, p, crate::item::Stack::new(333, 0, 1));
            self.play = play;
        }
    }

    /// `World.updateEntities` for the boats, and each rider's
    /// `updateRidden` right after its boat (`updateEntityWithOptionalForce`).
    pub(super) fn tick_boats(&mut self, play: &mut Play, events: &mut Vec<Event>) {
        use crate::entity::base::Fx;
        let mut sounds = Vec::new();
        // EntityLivingBase.collideWithNearbyEntities of the player and the
        // mobs: boat.applyEntityCollision(them).
        {
            let feet = play.player.0;
            let pb = Self::player_box(feet);
            let near = crate::entity::base::Ent::expand(&pb, 0.2f32 as f64, 0.0, 0.2f32 as f64);
            for b in play.boats.iter_mut() {
                if b.b.e.dead {
                    continue;
                }
                let own = self.riding == Some(b.b.id);
                if !own && !self.splayer.dead && b.b.e.bb.intersects(&near) {
                    if let Some((a, _)) = b.b.collision_push(feet) {
                        for k in 0..3 {
                            b.b.e.motion[k] += a[k];
                        }
                        b.airborne = true;
                    }
                }
                for m in self.mobs.list.iter_mut() {
                    if m.dead {
                        continue;
                    }
                    let mn =
                        crate::entity::base::Ent::expand(&m.bb, 0.2f32 as f64, 0.0, 0.2f32 as f64);
                    if b.b.e.bb.intersects(&mn) {
                        if let Some((a, o)) = b.b.collision_push(m.pos) {
                            for k in 0..3 {
                                b.b.e.motion[k] += a[k];
                                m.motion[k] += o[k];
                            }
                            b.airborne = true;
                        }
                    }
                }
            }
        }
        let n = play.boats.len();
        for i in 0..n {
            if play.boats[i].b.e.dead {
                continue;
            }
            let id = play.boats[i].b.id;
            let rider = if self.riding == Some(id) && !self.splayer.dead {
                Some(Rider {
                    yaw: play.player.1,
                    strafe: self.ride.strafe,
                    forward: self.ride.forward,
                })
            } else {
                None
            };
            let others = self.entity_boxes_in(play, Some(id));
            let mut fx = Vec::new();
            play.boats[i].b.e.ticks_existed += 1;
            let outs = {
                let b = &mut play.boats[i].b;
                b.tick(&mut self.world, false, rider, &others, &mut fx)
            };
            let bb = play.boats[i].b.bounding_box();
            if let Some(e) = self.world.solid_entities.iter_mut().find(|e| e.0 == id) {
                e.1 = bb;
            }
            for f in fx {
                match f {
                    Fx::Sound(a, p, v, pi) => sounds.push((a, p, v, pi)),
                    // WorldServer.spawnParticle reaches no client
                    _ => {}
                }
            }
            for o in outs {
                let pos = play.boats[i].b.e.pos;
                match o {
                    Out::Wreck => {
                        for _ in 0..3 {
                            Self::entity_drop(
                                play,
                                pos,
                                crate::item::Stack::new(block::PLANKS as u16, 0, 1),
                            );
                        }
                        for _ in 0..2 {
                            Self::entity_drop(play, pos, crate::item::Stack::new(280, 0, 1));
                        }
                    }
                    Out::Broken => {
                        if self.riding == Some(id) {
                            events.push(Event::Mount(Some(id)));
                            self.teleport_to = Some(play.player.0);
                        }
                        Self::entity_drop(play, pos, crate::item::Stack::new(333, 0, 1));
                    }
                    Out::Fall(d) => {
                        if rider.is_some() {
                            self.splayer.fall_from(&self.world, d);
                        }
                    }
                }
            }
            // other boats near: applyEntityCollision
            if !play.boats[i].b.e.dead {
                let me = play.boats[i].b.e.pos;
                let near = crate::entity::base::Ent::expand(
                    &play.boats[i].b.e.bb,
                    0.2f32 as f64,
                    0.0,
                    0.2f32 as f64,
                );
                let mut add_me = [0.0; 3];
                for j in 0..n {
                    if j == i || play.boats[j].b.e.dead || !play.boats[j].b.e.bb.intersects(&near) {
                        continue;
                    }
                    // var18.applyEntityCollision(this): the other boat is `this`
                    if let Some((o, a)) = play.boats[j].b.collision_push(me) {
                        for k in 0..3 {
                            play.boats[j].b.e.motion[k] += o[k];
                            add_me[k] += a[k];
                        }
                        play.boats[j].airborne = true;
                    }
                }
                if add_me != [0.0; 3] {
                    for k in 0..3 {
                        play.boats[i].b.e.motion[k] += add_me[k];
                    }
                    play.boats[i].airborne = true;
                }
            }
            if self.splayer.dead && self.riding == Some(id) {
                // riddenByEntity.isDead
                self.riding = None;
                play.boats[i].b.ridden = false;
            }
            // updateEntity(riddenByEntity): EntityPlayer.updateRidden
            if self.riding == Some(id) {
                if play.boats[i].b.e.dead {
                    // Entity.updateRidden: ridingEntity.isDead
                    self.riding = None;
                } else if self.ride.sneak {
                    std::mem::swap(play, &mut self.play);
                    self.dismount(events);
                    std::mem::swap(play, &mut self.play);
                    self.ride.sneak = false;
                    self.splayer.sneaking = false;
                } else {
                    let feet = play.boats[i].b.rider_position(PLAYER_Y_OFFSET_MP);
                    play.player.0 = feet;
                    self.splayer.place(feet);
                    if let Some(p) = &mut self.player {
                        p.0 = feet;
                    }
                }
            }
        }
        // removed entities
        let mut packets = Vec::new();
        let mut keep = Vec::new();
        for b in std::mem::take(&mut play.boats) {
            if b.b.e.dead {
                if b.t.watching {
                    packets.push((b.b.id, Packet::Destroy));
                }
                self.world.solid_entities.retain(|e| e.0 != b.b.id);
                if self.riding == Some(b.b.id) {
                    self.riding = None;
                }
                continue;
            }
            keep.push(b);
        }
        play.boats = keep;
        // EntityTracker.updateTrackedEntities
        let feet = play.player.0;
        let player_moved = match self.track_check {
            None => true,
            Some(c) => {
                (feet[0] - c[0]).powi(2) + (feet[1] - c[1]).powi(2) + (feet[2] - c[2]).powi(2)
                    > 16.0
            }
        };
        if player_moved {
            self.track_check = Some(feet);
        }
        let mut metas = Vec::new();
        for b in play.boats.iter_mut() {
            let (cx, cz) = (
                math::floor_double(b.b.e.pos[0]) >> 4,
                math::floor_double(b.b.e.pos[2]) >> 4,
            );
            let in_view = (cx - self.centre.0).abs() <= self.view
                && (cz - self.centre.1).abs() <= self.view
                && self.world.chunk_exists(cx, cz);
            let changed = b.meta_changed();
            let mut out = Vec::new();
            let mut air = b.airborne;
            let mut vc = b.b.velocity_changed;
            b.t.update(
                &b.b.e,
                &mut air,
                changed.is_some(),
                &mut vc,
                feet,
                player_moved,
                in_view,
                &mut out,
            );
            b.airborne = air;
            b.b.velocity_changed = vc;
            for p in out {
                if let Packet::Spawn { .. } = p {
                    metas.push((b.b.id, b.b.time_since_hit, b.b.forward, b.b.damage));
                }
                packets.push((b.b.id, p));
            }
            if changed.is_some() && b.t.watching {
                metas.push((b.b.id, b.b.time_since_hit, b.b.forward, b.b.damage));
            }
        }
        if !packets.is_empty() {
            events.push(Event::Vehicles(packets));
        }
        // S1BPacketEntityAttach from this tick's commands, after the spawns
        events.extend(std::mem::take(&mut self.vehicle_events));
        if !metas.is_empty() {
            events.push(Event::VehicleMeta(metas));
        }
        let destroyed = std::mem::take(&mut self.world.destroyed);
        if !destroyed.is_empty() {
            events.push(Event::Destroyed(destroyed));
        }
        if !sounds.is_empty() {
            events.push(Event::Sounds(sounds));
        }
    }
}
