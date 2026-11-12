//! Boats and minecarts as one kind of tracked entity: what the server keeps
//! (`EntityTrackerEntry`, the data watcher last sent) and how they are saved.

use super::base::Ent;
use super::boat::Boat;
use super::minecart::{CartKind, CartMeta, Minecart};
use super::player::Aabb;
use super::tracker::Tracker;

/// Ids of vehicles, apart from mobs' (`Mobs::next_id`).
pub const VEHICLE_ID: u32 = 0x4000_0000;

#[derive(Clone, Debug)]
pub enum Vehicle {
    Boat(Boat),
    Cart(Minecart),
}

/// The data watcher of either.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VMeta {
    Boat { hit: i32, forward: i32, damage: f32 },
    Cart(CartMeta),
}

impl Vehicle {
    pub fn id(&self) -> u32 {
        match self {
            Vehicle::Boat(b) => b.id,
            Vehicle::Cart(c) => c.id,
        }
    }
    pub fn e(&self) -> &Ent {
        match self {
            Vehicle::Boat(b) => &b.e,
            Vehicle::Cart(c) => &c.e,
        }
    }
    pub fn e_mut(&mut self) -> &mut Ent {
        match self {
            Vehicle::Boat(b) => &mut b.e,
            Vehicle::Cart(c) => &mut c.e,
        }
    }
    pub fn dead(&self) -> bool {
        self.e().dead
    }
    pub fn ridden(&self) -> bool {
        match self {
            Vehicle::Boat(b) => b.ridden,
            Vehicle::Cart(c) => c.ridden,
        }
    }
    pub fn set_ridden(&mut self, v: bool) {
        match self {
            Vehicle::Boat(b) => b.ridden = v,
            Vehicle::Cart(c) => c.ridden = v,
        }
    }
    pub fn velocity_changed(&mut self) -> &mut bool {
        match self {
            Vehicle::Boat(b) => &mut b.velocity_changed,
            Vehicle::Cart(c) => &mut c.velocity_changed,
        }
    }
    /// `updateRiderPosition`
    pub fn rider_position(&self, rider_y_offset: f64) -> [f64; 3] {
        match self {
            Vehicle::Boat(b) => b.rider_position(rider_y_offset),
            Vehicle::Cart(c) => c.rider_position(rider_y_offset),
        }
    }
    /// `getBoundingBox`: solid to other entities (boats only).
    pub fn bounding_box(&self) -> Option<Aabb> {
        match self {
            Vehicle::Boat(b) => Some(b.bounding_box()),
            Vehicle::Cart(_) => None,
        }
    }
    pub fn meta(&self) -> VMeta {
        match self {
            Vehicle::Boat(b) => VMeta::Boat {
                hit: b.time_since_hit,
                forward: b.forward,
                damage: b.damage,
            },
            Vehicle::Cart(c) => VMeta::Cart(c.meta()),
        }
    }
    pub fn set_meta(&mut self, m: VMeta) {
        match (self, m) {
            (
                Vehicle::Boat(b),
                VMeta::Boat {
                    hit,
                    forward,
                    damage,
                },
            ) => {
                b.time_since_hit = hit;
                b.forward = forward;
                b.damage = damage;
            }
            (Vehicle::Cart(c), VMeta::Cart(m)) => c.set_meta(m),
            _ => {}
        }
    }
    pub fn set_position_and_rotation2(&mut self, p: [f64; 3], yaw: f32, pitch: f32, inc: i32) {
        match self {
            Vehicle::Boat(b) => b.set_position_and_rotation2(p, yaw, pitch, inc),
            Vehicle::Cart(c) => c.set_position_and_rotation2(p, yaw, pitch, inc),
        }
    }
    pub fn set_velocity(&mut self, v: [f64; 3]) {
        match self {
            Vehicle::Boat(b) => b.set_velocity(v),
            Vehicle::Cart(c) => c.set_velocity(v),
        }
    }
    /// `S0EPacketSpawnObject`'s type and data.
    pub fn spawn_type(&self) -> (i32, i32) {
        match self {
            Vehicle::Boat(_) => (1, 0),
            Vehicle::Cart(c) => (10, c.kind.type_id()),
        }
    }
    /// `NetHandlerPlayClient.handleSpawnObject`: the client's entity.
    pub fn from_spawn(object: i32, data: i32, id: u32, p: [f64; 3], seed: i64) -> Option<Vehicle> {
        match object {
            1 => Some(Vehicle::Boat(Boat::at(id, p[0], p[1], p[2], seed))),
            10 => Some(Vehicle::Cart(Minecart::at(
                id,
                CartKind::from_type(data),
                p[0],
                p[1],
                p[2],
                seed,
            ))),
            _ => None,
        }
    }
}

/// A vehicle with its `EntityTrackerEntry`.
#[derive(Clone, Debug)]
pub struct Tracked {
    pub v: Vehicle,
    pub t: Tracker,
    /// `isAirBorne` (set by `addVelocity`)
    pub airborne: bool,
    /// The data watcher values last sent.
    sent: VMeta,
}

impl Tracked {
    /// `EntityTracker.addEntityToTracker(entity, 80, 3, true)`
    pub fn new(v: Vehicle) -> Tracked {
        let (object, data) = v.spawn_type();
        let t = Tracker::new(v.e(), 80, 3, true, object, data);
        let sent = v.meta();
        Tracked {
            v,
            t,
            airborne: false,
            sent,
        }
    }

    /// `DataWatcher.hasChanges`
    pub fn meta_changed(&mut self) -> bool {
        let m = self.v.meta();
        if m != self.sent {
            self.sent = m;
            true
        } else {
            false
        }
    }

    /// `Entity.writeToNBT`
    pub fn to_nbt(&self, dim: i32) -> crate::world::anvil::Nbt {
        use crate::world::anvil::{compound, item_nbt, Nbt};
        match &self.v {
            Vehicle::Boat(b) => {
                let mut v = vec![("id", Nbt::String("Boat".into()))];
                v.extend(b.e.nbt_fields(dim, b.id));
                compound(v)
            }
            Vehicle::Cart(c) => {
                let mut v = vec![("id", Nbt::String(c.kind.entity_id().into()))];
                v.extend(c.e.nbt_fields(dim, c.id));
                // EntityMinecart.writeEntityToNBT
                if let Some((b, d, o)) = c.display {
                    v.push(("CustomDisplayTile", Nbt::Byte(1)));
                    v.push(("DisplayTile", Nbt::Int(b as i32)));
                    v.push(("DisplayData", Nbt::Int(d)));
                    v.push(("DisplayOffset", Nbt::Int(o)));
                }
                if let Some(n) = &c.name {
                    if !n.is_empty() {
                        v.push(("CustomName", Nbt::String(n.clone())));
                    }
                }
                match c.kind {
                    CartKind::Furnace => {
                        v.push(("PushX", Nbt::Double(c.push[0])));
                        v.push(("PushZ", Nbt::Double(c.push[1])));
                        v.push(("Fuel", Nbt::Short(c.fuel as i16)));
                    }
                    CartKind::Tnt => v.push(("TNTFuse", Nbt::Int(c.fuse))),
                    CartKind::Chest | CartKind::Hopper => {
                        let items = c
                            .slots
                            .iter()
                            .enumerate()
                            .filter_map(|(i, s)| s.map(|s| item_nbt(Some(i as i8), &s)))
                            .collect();
                        v.push(("Items", Nbt::List(10, items)));
                        if c.kind == CartKind::Hopper {
                            v.push(("TransferCooldown", Nbt::Int(c.transfer)));
                        }
                    }
                    _ => {}
                }
                compound(v)
            }
        }
    }

    /// `EntityList.createEntityFromNBT` for the vehicles.
    pub fn from_nbt(t: &crate::world::anvil::Nbt, id: u32, seed: i64) -> Option<Tracked> {
        use crate::world::anvil::Nbt;
        let name = t.string("id")?;
        let mut v = if name == "Boat" {
            Vehicle::Boat(Boat::new(id, seed))
        } else {
            Vehicle::Cart(Minecart::new(id, CartKind::from_entity_id(name)?, seed))
        };
        let d = |l: &[Nbt], i: usize| match l.get(i) {
            Some(Nbt::Double(v)) => *v,
            _ => 0.0,
        };
        let f = |l: &[Nbt], i: usize| match l.get(i) {
            Some(Nbt::Float(v)) => *v,
            _ => 0.0,
        };
        {
            let e = v.e_mut();
            let pos = t.list("Pos");
            let mo = t.list("Motion");
            let rot = t.list("Rotation");
            e.motion = [d(mo, 0), d(mo, 1), d(mo, 2)];
            for m in &mut e.motion {
                if m.abs() > 10.0 {
                    *m = 0.0;
                }
            }
            let p = [d(pos, 0), d(pos, 1), d(pos, 2)];
            e.prev = p;
            e.last_tick = p;
            e.yaw = f(rot, 0);
            e.pitch = f(rot, 1);
            e.prev_yaw = e.yaw;
            e.prev_pitch = e.pitch;
            e.fall_distance = t.float("FallDistance").unwrap_or(0.0) as f32;
            e.fire = t.int("Fire").unwrap_or(0) as i32;
            e.on_ground = t.int("OnGround").unwrap_or(0) != 0;
            e.set_position(p[0], p[1], p[2]);
            e.yaw %= 360.0;
            e.pitch %= 360.0;
        }
        if let Vehicle::Cart(c) = &mut v {
            if t.int("CustomDisplayTile").unwrap_or(0) != 0 {
                c.display = Some((
                    t.int("DisplayTile").unwrap_or(0) as u8,
                    t.int("DisplayData").unwrap_or(0) as i32,
                    t.int("DisplayOffset").unwrap_or(6) as i32,
                ));
            }
            if let Some(n) = t.string("CustomName") {
                if !n.is_empty() {
                    c.name = Some(n.to_string());
                }
            }
            match c.kind {
                CartKind::Furnace => {
                    c.push = [
                        t.float("PushX").unwrap_or(0.0),
                        t.float("PushZ").unwrap_or(0.0),
                    ];
                    c.fuel = t.int("Fuel").unwrap_or(0) as i32;
                }
                CartKind::Tnt => {
                    if let Some(fz) = t.int("TNTFuse") {
                        c.fuse = fz as i32;
                    }
                }
                CartKind::Chest | CartKind::Hopper => {
                    for it in t.list("Items") {
                        let slot = (it.int("Slot").unwrap_or(0) & 255) as usize;
                        if slot < c.slots.len() {
                            c.slots[slot] = crate::world::anvil::item_from_nbt(it);
                        }
                    }
                    if c.kind == CartKind::Hopper {
                        c.transfer = t.int("TransferCooldown").unwrap_or(0) as i32;
                    }
                }
                _ => {}
            }
        }
        Some(Tracked::new(v))
    }
}
