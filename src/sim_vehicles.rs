//! The server side of boats and minecarts: `ItemBoat.onItemRightClick`,
//! `ItemMinecart.onItemUse`, the entity ticks in `World.updateEntities`
//! order (the vehicle, then its rider's `updateRidden`), `interactFirst`,
//! `mountEntity` / `dismountEntity`, hits, `applyEntityCollision`, and what
//! `EntityTrackerEntry` tells the client.

use super::*;
use crate::entity::boat::{Boat, Out, Rider};
use crate::entity::minecart::{self, CartKind, CartOut, Minecart};
use crate::entity::player::Aabb;
use crate::entity::tracker::Packet;
use crate::entity::vehicle::{Tracked, Vehicle, VEHICLE_ID};
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
            self.entity_boxes_in(&self.play, None, false)
                .into_iter()
                .filter(|b| b.intersects(&probe)),
        );
        if !hits.is_empty() {
            return true;
        }
        self.world.solid_entities.push((id, boat.bounding_box()));
        self.play.vehicles.push(Tracked::new(Vehicle::Boat(boat)));
        if self.play.mode != GameMode::Creative {
            self.play.inv.consume_held();
            self.play.inv_dirty = true;
        }
        true
    }

    /// `ItemMinecart.onItemUse` on a rail. Returns whether it was used.
    pub(super) fn place_cart(&mut self, target: [i32; 3]) -> bool {
        let Some(held) = self.play.inv.held() else {
            return false;
        };
        let Some(kind) = CartKind::from_item(held.id) else {
            return false;
        };
        let [x, y, z] = target;
        if !minecart::is_rail(self.world.block(x, y, z)) {
            return false;
        }
        let id = self.alloc_vehicle_id();
        let seed = self.mobs.rand.next_long();
        let c = Minecart::at(
            id,
            kind,
            (x as f32 + 0.5) as f64,
            (y as f32 + 0.5) as f64,
            (z as f32 + 0.5) as f64,
            seed,
        );
        self.play.vehicles.push(Tracked::new(Vehicle::Cart(c)));
        // ItemInWorldManager: a creative player's stack is put back
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
        for b in &self.play.vehicles {
            if !b.v.dead() {
                v.push((b.v.e().bb, 0.1f32 as f64));
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

    /// The boxes `getCollisionBox` gives a moving vehicle: every other
    /// entity's (boats), or the `canBePushed` ones' (minecarts).
    fn entity_boxes_in(&self, play: &Play, except: Option<u32>, pushable_only: bool) -> Vec<Aabb> {
        let mut v = Vec::new();
        if !self.respawn_pending && !self.splayer.dead {
            v.push(Self::player_box(play.player.0));
        }
        for m in &self.mobs.list {
            if !m.dead {
                v.push(m.bb);
            }
        }
        if !pushable_only {
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
        }
        for b in &play.vehicles {
            if Some(b.v.id()) != except && !b.v.dead() {
                v.push(b.v.e().bb);
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

    /// `EntityPlayerMP.mountEntity(vehicle)`: `S1BPacketEntityAttach` and
    /// `setPlayerLocation`.
    fn mount(&mut self, id: u32) {
        if let Some(old) = self.riding.take() {
            if let Some(b) = self.play.vehicles.iter_mut().find(|b| b.v.id() == old) {
                b.v.set_ridden(false);
            }
        }
        let Some(b) = self.play.vehicles.iter_mut().find(|b| b.v.id() == id) else {
            return;
        };
        b.v.set_ridden(true);
        self.riding = Some(id);
        self.ride = Default::default();
        self.vehicle_events.push(Event::Mount(Some(id)));
        let feet = self.play.player.0;
        self.teleport_to = Some(feet);
    }

    /// `EntityPlayer.mountEntity(null)` on the server:
    /// `EntityLivingBase.dismountEntity`, then the attach packet and
    /// `setPlayerLocation`.
    fn dismount(&mut self) {
        let Some(id) = self.riding.take() else {
            return;
        };
        let Some(i) = self.play.vehicles.iter().position(|b| b.v.id() == id) else {
            return;
        };
        self.play.vehicles[i].v.set_ridden(false);
        let v = self.play.vehicles[i].v.e();
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
        self.vehicle_events.push(Event::Mount(None));
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

    /// `interactFirst`
    pub(super) fn interact_vehicle(&mut self, id: u32) {
        let Some(i) = self
            .play
            .vehicles
            .iter()
            .position(|b| b.v.id() == id && !b.v.dead())
        else {
            return;
        };
        let feet = self.play.player.0;
        let creative = self.play.mode == GameMode::Creative;
        let held = self.play.inv.held();
        match &mut self.play.vehicles[i].v {
            Vehicle::Boat(_) => self.mount(id),
            Vehicle::Cart(c) => match c.kind {
                CartKind::Empty => {
                    // EntityMinecartEmpty.interactFirst: not while a mob rides it
                    if !self.mobs.list.iter().any(|m| m.riding == Some(id)) {
                        self.mount(id);
                    }
                }
                CartKind::Furnace => {
                    let coal = held.map_or(false, |s| s.id == 263);
                    if c.furnace_interact(coal, feet) && !creative {
                        self.play.inv.consume_held();
                        self.play.inv_dirty = true;
                    }
                }
                CartKind::Chest => {
                    // displayGUIChest
                    self.play.open = Open::Cart(id);
                    self.play.inv_dirty = true;
                }
                _ => {}
            },
        }
    }

    /// `EntityPlayer.attackTargetEntityWithCurrentItem` on a vehicle.
    pub(super) fn attack_vehicle(&mut self, id: u32, damage: f32, knockback: i32) {
        let creative = self.play.mode == GameMode::Creative;
        let yaw = self.play.player.1;
        let Some(i) = self.play.vehicles.iter().position(|b| b.v.id() == id) else {
            return;
        };
        if damage <= 0.0 {
            return;
        }
        let mut play = std::mem::replace(&mut self.play, Play::new(GameMode::Survival));
        let mut outs = Vec::new();
        let mut broke = None;
        match &mut play.vehicles[i].v {
            Vehicle::Boat(b) => broke = b.attack(damage, creative),
            Vehicle::Cart(c) => c.attack(damage, creative, false, false, &mut outs),
        }
        if knockback > 0 {
            let r = std::f32::consts::PI;
            let k = knockback as f32 * 0.5;
            let b = &mut play.vehicles[i];
            let e = b.v.e_mut();
            e.motion[0] += (-math::sin(yaw * r / 180.0) * k) as f64;
            e.motion[1] += 0.1;
            e.motion[2] += (math::cos(yaw * r / 180.0) * k) as f64;
            b.airborne = true;
        }
        if let Some(drop) = broke {
            if self.riding == Some(id) {
                // riddenByEntity.mountEntity(this): the attach packet and
                // setPlayerLocation again
                self.vehicle_events.push(Event::Mount(Some(id)));
                self.teleport_to = Some(play.player.0);
            }
            if drop {
                let p = play.vehicles[i].v.e().pos;
                Self::entity_drop(&mut play, p, crate::item::Stack::new(333, 0, 1));
            }
        }
        self.cart_outs(&mut play, i, outs);
        self.play = play;
    }

    /// `cart_outs` for the rest of the sim.
    pub(super) fn vehicle_outs(&mut self, play: &mut Play, i: usize, outs: Vec<CartOut>) {
        self.cart_outs(play, i, outs);
    }

    /// The results of a minecart's tick or hit.
    fn cart_outs(&mut self, play: &mut Play, i: usize, outs: Vec<CartOut>) {
        let id = play.vehicles[i].v.id();
        let pos = play.vehicles[i].v.e().pos;
        for o in outs {
            match o {
                CartOut::Drop(st) => Self::entity_drop(play, pos, st),
                CartOut::Scatter(p, m, st) => {
                    let nid = play.next_item;
                    play.next_item += 1;
                    let mut r = std::mem::replace(&mut play.prand, JavaRandom::new(0));
                    let mut it = crate::entity::item::ItemEntity::new(nid, st, p, &mut r);
                    play.prand = r;
                    it.e.motion = m;
                    play.items.push(it);
                }
                CartOut::Explode(p, size, ignited) => self.mobs.queue_explosion(p, size, ignited),
                CartOut::Sound(n, v, pi) => self.vehicle_sounds.push((n, pos, v, pi)),
                CartOut::Ignited => self
                    .vehicle_events
                    .push(Event::VehicleStatus(vec![(id, 10)])),
                CartOut::Remount => {
                    if self.riding == Some(id) {
                        self.vehicle_events.push(Event::Mount(Some(id)));
                        self.teleport_to = Some(play.player.0);
                    }
                }
            }
        }
    }

    /// `World.updateEntities` for the vehicles, and each rider's
    /// `updateRidden` right after its vehicle (`updateEntityWithOptionalForce`).
    pub(super) fn tick_vehicles(&mut self, play: &mut Play, events: &mut Vec<Event>) {
        use crate::entity::base::Fx;
        let mut sounds = std::mem::take(&mut self.vehicle_sounds);
        // the boats' boxes as the world sees them
        self.world.solid_entities = play
            .vehicles
            .iter()
            .filter_map(|b| b.v.bounding_box().map(|bb| (b.v.id(), bb)))
            .collect();
        // EntityLivingBase.collideWithNearbyEntities of the player and the
        // mobs: vehicle.applyEntityCollision(them).
        {
            let feet = play.player.0;
            let pb = Self::player_box(feet);
            let near = crate::entity::base::Ent::expand(&pb, 0.2f32 as f64, 0.0, 0.2f32 as f64);
            for b in play.vehicles.iter_mut() {
                if b.v.dead() {
                    continue;
                }
                let own = self.riding == Some(b.v.id());
                let id = b.v.id();
                if !own && !self.splayer.dead && b.v.e().bb.intersects(&near) {
                    let push = match &b.v {
                        Vehicle::Boat(boat) => boat.collision_push(feet).map(|p| p.0),
                        Vehicle::Cart(c) => c
                            .push_entity(feet, false, self.riding.is_some())
                            .1
                            .map(|p| p.0),
                    };
                    if let Some(a) = push {
                        for k in 0..3 {
                            b.v.e_mut().motion[k] += a[k];
                        }
                        b.airborne = true;
                    }
                }
                for m in self.mobs.list.iter_mut() {
                    if m.dead || m.riding == Some(id) {
                        continue;
                    }
                    let mn =
                        crate::entity::base::Ent::expand(&m.bb, 0.2f32 as f64, 0.0, 0.2f32 as f64);
                    if !b.v.e().bb.intersects(&mn) {
                        continue;
                    }
                    let (seat, push) = match &b.v {
                        Vehicle::Boat(boat) => (false, boat.collision_push(m.pos)),
                        Vehicle::Cart(c) => {
                            let golem = m.kind == crate::entity::mob::Kind::IronGolem;
                            c.push_entity(m.pos, !golem, m.riding.is_some())
                        }
                    };
                    if seat {
                        // var1.mountEntity(this)
                        m.riding = Some(id);
                        b.v.set_ridden(true);
                    }
                    if let Some((a, o)) = push {
                        for k in 0..3 {
                            b.v.e_mut().motion[k] += a[k];
                            m.motion[k] += o[k];
                        }
                        b.airborne = true;
                    }
                }
            }
        }
        let n = play.vehicles.len();
        for i in 0..n {
            if play.vehicles[i].v.dead() {
                continue;
            }
            let id = play.vehicles[i].v.id();
            let player_rides = self.riding == Some(id) && !self.splayer.dead;
            let mob_rider = self
                .mobs
                .list
                .iter()
                .position(|m| m.riding == Some(id) && !m.dead);
            let rider = if player_rides {
                Some(Rider {
                    yaw: play.player.1,
                    strafe: self.ride.strafe,
                    forward: self.ride.forward,
                })
            } else {
                mob_rider.map(|k| {
                    let m = &self.mobs.list[k];
                    let (strafe, forward) = m.move_input();
                    Rider {
                        yaw: m.yaw,
                        strafe,
                        forward,
                    }
                })
            };
            let boat = matches!(play.vehicles[i].v, Vehicle::Boat(_));
            let others = self.entity_boxes_in(play, Some(id), !boat);
            let mut fx = Vec::new();
            {
                // updateEntityWithOptionalForce
                let e = play.vehicles[i].v.e_mut();
                e.last_tick = e.pos;
                e.prev_yaw = e.yaw;
                e.prev_pitch = e.pitch;
                e.ticks_existed += 1;
            }
            let mut fell = None;
            match &mut play.vehicles[i].v {
                Vehicle::Boat(b) => {
                    let outs = b.tick(&mut self.world, false, rider, &others, &mut fx);
                    let pos = b.e.pos;
                    for o in outs {
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
                                    Self::entity_drop(
                                        play,
                                        pos,
                                        crate::item::Stack::new(280, 0, 1),
                                    );
                                }
                            }
                            Out::Broken => {
                                if player_rides {
                                    self.vehicle_events.push(Event::Mount(Some(id)));
                                    self.teleport_to = Some(play.player.0);
                                }
                                Self::entity_drop(play, pos, crate::item::Stack::new(333, 0, 1));
                            }
                            Out::Fall(d) => fell = Some(d),
                        }
                    }
                }
                Vehicle::Cart(c) => {
                    let mut outs = Vec::new();
                    let fall0 = c.e.fall_distance;
                    c.tick(&mut self.world, false, rider, &others, &mut fx, &mut outs);
                    let _ = fall0;
                    self.cart_outs(play, i, outs);
                }
            }
            if let (Some(d), true) = (fell, rider.is_some()) {
                if player_rides {
                    self.splayer.fall_from(&self.world, d);
                }
            }
            if let Some(bb) = play.vehicles[i].v.bounding_box() {
                if let Some(e) = self.world.solid_entities.iter_mut().find(|e| e.0 == id) {
                    e.1 = bb;
                }
            }
            for f in fx {
                // WorldServer.spawnParticle reaches no client
                if let Fx::Sound(a, p, v, pi) = f {
                    sounds.push((a, p, v, pi));
                }
            }
            // other vehicles near: var.applyEntityCollision(this)
            if !play.vehicles[i].v.dead() {
                let near = crate::entity::base::Ent::expand(
                    &play.vehicles[i].v.e().bb,
                    0.2f32 as f64,
                    0.0,
                    0.2f32 as f64,
                );
                for j in 0..n {
                    if j == i
                        || play.vehicles[j].v.dead()
                        || !play.vehicles[j].v.e().bb.intersects(&near)
                    {
                        continue;
                    }
                    let (a, b) = if i < j {
                        let (l, r) = play.vehicles.split_at_mut(j);
                        (&mut l[i], &mut r[0])
                    } else {
                        let (l, r) = play.vehicles.split_at_mut(i);
                        (&mut r[0], &mut l[j])
                    };
                    match (&mut a.v, &mut b.v) {
                        (Vehicle::Boat(me), Vehicle::Boat(other)) => {
                            if let Some((o, m)) = other.collision_push(me.e.pos) {
                                for k in 0..3 {
                                    other.e.motion[k] += o[k];
                                    me.e.motion[k] += m[k];
                                }
                                a.airborne = true;
                                b.airborne = true;
                            }
                        }
                        (Vehicle::Cart(me), Vehicle::Cart(other)) => {
                            minecart::carts_collide(other, me);
                        }
                        _ => {}
                    }
                }
            }
            if self.splayer.dead && self.riding == Some(id) {
                // riddenByEntity.isDead
                self.riding = None;
                play.vehicles[i].v.set_ridden(false);
            }
            // updateEntity(riddenByEntity)
            if let Some(k) = mob_rider {
                if play.vehicles[i].v.dead() {
                    self.mobs.list[k].riding = None;
                } else {
                    // Entity.updateRidden: motion 0, then the seat
                    let p = play.vehicles[i].v.rider_position(0.0);
                    let m = &mut self.mobs.list[k];
                    m.motion = [0.0; 3];
                    m.set_position(p[0], p[1], p[2]);
                }
            }
            if self.riding == Some(id) {
                if play.vehicles[i].v.dead() {
                    // Entity.updateRidden: ridingEntity.isDead
                    self.riding = None;
                } else if self.ride.sneak {
                    // EntityPlayer.updateRidden
                    std::mem::swap(play, &mut self.play);
                    self.dismount();
                    std::mem::swap(play, &mut self.play);
                    self.ride.sneak = false;
                    self.splayer.sneaking = false;
                } else {
                    let feet = play.vehicles[i].v.rider_position(PLAYER_Y_OFFSET_MP);
                    play.player.0 = feet;
                    self.splayer.place(feet);
                    if let Some(p) = &mut self.player {
                        p.0 = feet;
                    }
                }
            }
        }
        // Container.canInteractWith: a minecart chest more than 8 away
        if let Open::Cart(id) = play.open {
            let feet = play.player.0;
            let ok = play.vehicles.iter().any(|b| {
                let e = b.v.e();
                b.v.id() == id
                    && !e.dead
                    && (e.pos[0] - feet[0]).powi(2)
                        + (e.pos[1] - feet[1]).powi(2)
                        + (e.pos[2] - feet[2]).powi(2)
                        <= 64.0
            });
            if !ok {
                play.close(&mut self.world);
            }
        }
        // removed entities
        let mut packets = Vec::new();
        let mut keep = Vec::new();
        for b in std::mem::take(&mut play.vehicles) {
            if b.v.dead() {
                if b.t.watching {
                    packets.push((b.v.id(), Packet::Destroy));
                }
                self.world.solid_entities.retain(|e| e.0 != b.v.id());
                if self.riding == Some(b.v.id()) {
                    self.riding = None;
                }
                for m in self.mobs.list.iter_mut() {
                    if m.riding == Some(b.v.id()) {
                        m.riding = None;
                    }
                }
                continue;
            }
            keep.push(b);
        }
        play.vehicles = keep;
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
        for b in play.vehicles.iter_mut() {
            let (cx, cz) = (
                math::floor_double(b.v.e().pos[0]) >> 4,
                math::floor_double(b.v.e().pos[2]) >> 4,
            );
            let in_view = (cx - self.centre.0).abs() <= self.view
                && (cz - self.centre.1).abs() <= self.view
                && self.world.chunk_exists(cx, cz);
            let changed = b.meta_changed();
            let mut out = Vec::new();
            let mut air = b.airborne;
            let mut vc = *b.v.velocity_changed();
            let e = b.v.e().clone();
            b.t.update(
                &e,
                &mut air,
                changed,
                &mut vc,
                feet,
                player_moved,
                in_view,
                &mut out,
            );
            b.airborne = air;
            *b.v.velocity_changed() = vc;
            for p in out {
                let spawn = matches!(p, Packet::Spawn { .. });
                packets.push((b.v.id(), p));
                if spawn {
                    metas.push((b.v.id(), b.v.meta()));
                }
            }
            if changed && b.t.watching {
                metas.push((b.v.id(), b.v.meta()));
            }
        }
        if !packets.is_empty() {
            events.push(Event::Vehicles(packets));
        }
        // S1BPacketEntityAttach and S19 from this tick, after the spawns
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
