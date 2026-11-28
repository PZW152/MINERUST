//! `EntityHanging`: `EntityPainting` and `EntityItemFrame`.

use super::base::Ent;
use super::player::{colliding_boxes_except, Aabb};
use crate::item::Stack;
use crate::world::BlockView;

/// `EntityPainting$EnumArt`: title, size and offset in
/// `paintings_kristoffer_zetterstrand.png` (pixels).
pub const ARTS: [(&str, i32, i32, i32, i32); 26] = [
    ("Kebab", 16, 16, 0, 0),
    ("Aztec", 16, 16, 16, 0),
    ("Alban", 16, 16, 32, 0),
    ("Aztec2", 16, 16, 48, 0),
    ("Bomb", 16, 16, 64, 0),
    ("Plant", 16, 16, 80, 0),
    ("Wasteland", 16, 16, 96, 0),
    ("Pool", 32, 16, 0, 32),
    ("Courbet", 32, 16, 32, 32),
    ("Sea", 32, 16, 64, 32),
    ("Sunset", 32, 16, 96, 32),
    ("Creebet", 32, 16, 128, 32),
    ("Wanderer", 16, 32, 0, 64),
    ("Graham", 16, 32, 16, 64),
    ("Match", 32, 32, 0, 128),
    ("Bust", 32, 32, 32, 128),
    ("Stage", 32, 32, 64, 128),
    ("Void", 32, 32, 96, 128),
    ("SkullAndRoses", 32, 32, 128, 128),
    ("Wither", 32, 32, 160, 128),
    ("Fighters", 64, 32, 0, 96),
    ("Pointer", 64, 64, 0, 192),
    ("Pigscene", 64, 64, 64, 192),
    ("BurningSkull", 64, 64, 128, 192),
    ("Skeleton", 64, 48, 192, 64),
    ("DonkeyKong", 64, 48, 192, 112),
];
pub const ART_COUNT: usize = ARTS.len();

/// `EntityPainting$EnumArt.values()[i]`
pub fn art(i: usize) -> (&'static str, i32, i32, i32, i32) {
    ARTS[i % ART_COUNT]
}

/// `Items.painting`, `Items.item_frame`
pub const PAINTING: u16 = 321;
pub const ITEM_FRAME: u16 = 389;

/// `Direction.offsetX`, `offsetZ`, `facingToDirection`, `rotateOpposite`
pub const OFFSET_X: [i32; 4] = [0, -1, 0, 1];
pub const OFFSET_Z: [i32; 4] = [1, 0, -1, 0];
pub const FACING_TO_DIRECTION: [i32; 6] = [-1, -1, 2, 0, 1, 3];
const ROTATE_OPPOSITE: [i32; 4] = [2, 3, 0, 1];

#[derive(Clone, Debug)]
pub struct Hanging {
    pub id: u32,
    pub e: Ent,
    /// `x`, `y`, `z`: the block it hangs on
    pub tile: [i32; 3],
    /// `hangingDirection`
    pub dir: i32,
    /// `art` of a painting; `None` for an item frame
    pub art: Option<usize>,
    /// The item frame's displayed item (data watcher 2) and rotation (3).
    pub item: Option<Stack>,
    pub rotation: i32,
    /// `itemDropChance`
    pub drop_chance: f32,
    /// `tickCounter1`
    ticks: i32,
    pub velocity_changed: bool,
    /// `EntityLeashKnot`: a lead tied to a fence.
    pub knot: bool,
}

/// `Items.lead`
pub const LEAD: u16 = 420;

impl Hanging {
    /// `EntityHanging(World)`: 0.5 by 0.5, yOffset 0.
    pub fn new(id: u32, tile: [i32; 3], art: Option<usize>, seed: i64) -> Hanging {
        let mut e = Ent::new(0.5, 0.5, seed);
        e.y_offset = 0.0;
        Hanging {
            id,
            e,
            tile,
            dir: 0,
            art,
            item: None,
            rotation: 0,
            drop_chance: 1.0,
            ticks: 0,
            velocity_changed: false,
            knot: false,
        }
    }

    /// `EntityLeashKnot(world, x, y, z)`: at the middle of the fence, its
    /// box the entity's 0.5 by 0.5.
    pub fn knot(id: u32, tile: [i32; 3], seed: i64) -> Hanging {
        let mut h = Hanging::new(id, tile, None, seed);
        h.knot = true;
        let p = [tile[0] as f64 + 0.5, tile[1] as f64 + 0.5, tile[2] as f64 + 0.5];
        h.e.set_position(p[0], p[1], p[2]);
        h.e.prev = p;
        h
    }

    pub fn width_pixels(&self) -> i32 {
        self.art.map_or(9, |a| art(a).1)
    }
    pub fn height_pixels(&self) -> i32 {
        self.art.map_or(9, |a| art(a).2)
    }

    fn position_offset(p: i32) -> f32 {
        if p == 32 || p == 64 {
            0.5
        } else {
            0.0
        }
    }

    /// `EntityHanging.setDirection`: rotation, position and box.
    pub fn set_direction(&mut self, d: i32) {
        if self.knot {
            // EntityLeashKnot.setDirection does nothing
            return;
        }
        self.dir = d;
        let mut yaw = (d * 90) as f32;
        let (wp, hp) = (self.width_pixels(), self.height_pixels());
        let mut w = wp as f32;
        let h = hp as f32;
        let mut depth = wp as f32;
        if d != 2 && d != 0 {
            w = 0.5;
        } else {
            depth = 0.5;
            yaw = (ROTATE_OPPOSITE[d as usize & 3] * 90) as f32;
        }
        self.e.yaw = yaw;
        self.e.prev_yaw = yaw;
        let (w, h, depth) = (w / 32.0, h / 32.0, depth / 32.0);
        let [tx, ty, tz] = self.tile;
        let mut x = tx as f32 + 0.5;
        let mut y = ty as f32 + 0.5;
        let mut z = tz as f32 + 0.5;
        let k = 0.5625f32;
        match d {
            2 => z -= k,
            1 => x -= k,
            0 => z += k,
            3 => x += k,
            _ => {}
        }
        let ow = Self::position_offset(wp);
        match d {
            2 => x -= ow,
            1 => z += ow,
            0 => x += ow,
            3 => z -= ow,
            _ => {}
        }
        y += Self::position_offset(hp);
        self.e.set_position(x as f64, y as f64, z as f64);
        let m = -0.03125f32;
        self.e.bb = Aabb {
            min: [
                (x - w - m) as f64,
                (y - h - m) as f64,
                (z - depth - m) as f64,
            ],
            max: [
                (x + w + m) as f64,
                (y + h + m) as f64,
                (z + depth + m) as f64,
            ],
        };
    }

    /// The block a hanging entity at `pos` facing `d` hangs on: the inverse
    /// of `set_direction` (the client is sent the position).
    pub fn tile_from_pos(pos: [f64; 3], d: i32, wp: i32, hp: i32) -> [i32; 3] {
        let k = 0.5625;
        let ow = Self::position_offset(wp) as f64;
        let (mut dx, mut dz) = (0.5, 0.5);
        match d {
            2 => {
                dz -= k;
                dx -= ow
            }
            1 => {
                dx -= k;
                dz += ow
            }
            0 => {
                dz += k;
                dx += ow
            }
            _ => {
                dx += k;
                dz -= ow
            }
        }
        let dy = 0.5 + Self::position_offset(hp) as f64;
        [
            (pos[0] - dx).round() as i32,
            (pos[1] - dy).round() as i32,
            (pos[2] - dz).round() as i32,
        ]
    }

    /// `EntityHanging.onValidSurface`: nothing solid in the way, a solid
    /// block behind every 16 pixels, and no other hanging entity overlapping
    /// (`others` are their boxes).
    pub fn on_valid_surface(&self, world: &dyn BlockView, others: &[Aabb]) -> bool {
        if self.knot {
            // EntityLeashKnot: the block still renders as a fence (type 11)
            let [x, y, z] = self.tile;
            return crate::block::info(world.peek_block(x, y, z)).shape == crate::block::Shape::Fence;
        }
        if !colliding_boxes_except(world, &self.e.bb, Some(self.id)).is_empty() {
            return false;
        }
        let (wp, hp) = (self.width_pixels(), self.height_pixels());
        let nw = (wp / 16).max(1);
        let nh = (hp / 16).max(1);
        let [tx, _, tz] = self.tile;
        let (mut x0, mut z0) = (tx, tz);
        let off = |p: f64| crate::util::math::floor_double(p - wp as f64 / 32.0);
        match self.dir {
            2 | 0 => x0 = off(self.e.pos[0]),
            _ => z0 = off(self.e.pos[2]),
        }
        let y0 = crate::util::math::floor_double(self.e.pos[1] - hp as f64 / 32.0);
        for i in 0..nw {
            for j in 0..nh {
                let id = if self.dir != 2 && self.dir != 0 {
                    world.peek_block(tx, y0 + j, z0 + i)
                } else {
                    world.peek_block(x0 + i, y0 + j, tz)
                };
                if !crate::block::material(id).is_solid() {
                    return false;
                }
            }
        }
        !others.iter().any(|b| b.intersects(&self.e.bb))
    }

    /// `EntityPainting(World, x, y, z, direction)`: a random art among the
    /// ones that fit.
    pub fn painting(
        id: u32,
        tile: [i32; 3],
        d: i32,
        world: &dyn BlockView,
        others: &[Aabb],
        seed: i64,
    ) -> Hanging {
        let mut h = Hanging::new(id, tile, Some(0), seed);
        let mut fits = Vec::new();
        for a in 0..ART_COUNT {
            h.art = Some(a);
            h.set_direction(d);
            if h.on_valid_surface(world, others) {
                fits.push(a);
            }
        }
        if !fits.is_empty() {
            let k = h.e.rand.next_int_n(fits.len() as i32) as usize;
            h.art = Some(fits[k]);
        }
        h.set_direction(d);
        h
    }

    /// `EntityItemFrame(World, x, y, z, direction)`
    pub fn frame(id: u32, tile: [i32; 3], d: i32, seed: i64) -> Hanging {
        let mut h = Hanging::new(id, tile, None, seed);
        h.set_direction(d);
        h
    }

    /// `EntityHanging.onUpdate`: every 100 ticks a check of the surface;
    /// true when it falls off.
    pub fn tick(&mut self, world: &dyn BlockView, others: &[Aabb]) -> bool {
        self.e.prev = self.e.pos;
        let t = self.ticks;
        self.ticks += 1;
        if t == 100 {
            self.ticks = 0;
            if !self.e.dead && !self.on_valid_surface(world, others) {
                self.e.dead = true;
                return true;
            }
        }
        false
    }

    /// `attackEntityFrom`: an item frame with an item loses the item
    /// first (`Some(false)`); otherwise it breaks (`Some(true)`).
    pub fn attack(&mut self) -> Option<bool> {
        if self.e.dead {
            return None;
        }
        if self.art.is_none() && self.item.is_some() {
            return Some(false);
        }
        self.e.dead = true;
        self.velocity_changed = true;
        Some(true)
    }

    /// `onBroken` / `dropItemOrSelf(entity, self_too)`: what drops. A
    /// creative player gets nothing; the frame's item drops with
    /// `itemDropChance`.
    pub fn broken_drops(&mut self, creative: bool, self_too: bool) -> Vec<Stack> {
        let mut v = Vec::new();
        // EntityLeashKnot.onBroken drops nothing
        if creative || self.knot {
            return v;
        }
        match self.art {
            Some(_) => v.push(Stack::new(PAINTING, 0, 1)),
            None => {
                if self_too {
                    v.push(Stack::new(ITEM_FRAME, 0, 1));
                }
                if let Some(s) = self.item {
                    if self.e.rand.next_float() < self.drop_chance {
                        v.push(s);
                    }
                }
            }
        }
        v
    }

    /// `EntityItemFrame.interactFirst`: an empty frame takes one of the
    /// held items (true: one is used), a full one turns its item.
    pub fn interact(&mut self, held: Option<Stack>) -> bool {
        if self.art.is_some() {
            return false;
        }
        if self.item.is_none() {
            if let Some(s) = held {
                self.item = Some(Stack { count: 1, ..s });
                return true;
            }
        } else {
            self.rotation = (self.rotation + 1) % 4;
        }
        false
    }

    /// `S0EPacketSpawnObject` 71 for frames (data: direction); paintings
    /// have `S10PacketSpawnPainting` (here type 1000, direction and art).
    pub fn spawn_data(&self) -> (i32, i32) {
        if self.knot {
            return (77, 0);
        }
        match self.art {
            None => (71, self.dir),
            Some(a) => (1000, self.dir | (a as i32) << 2),
        }
    }

    /// The client's entity from the spawn packet.
    pub fn from_spawn(object: i32, data: i32, id: u32, p: [f64; 3], seed: i64) -> Hanging {
        let d = data & 3;
        let art = (object == 1000).then_some((data >> 2) as usize % ART_COUNT);
        let mut h = Hanging::new(id, [0; 3], art, seed);
        h.tile = Self::tile_from_pos(p, d, h.width_pixels(), h.height_pixels());
        h.set_direction(d);
        h
    }

    /// `writeEntityToNBT`
    pub fn nbt(&self, v: &mut Vec<(&'static str, crate::world::anvil::Nbt)>) {
        use crate::world::anvil::{item_nbt, Nbt};
        match self.art {
            Some(a) => v.push(("Motive", Nbt::String(art(a).0.into()))),
            None => {
                if let Some(s) = &self.item {
                    v.push(("Item", item_nbt(None, s)));
                    v.push(("ItemRotation", Nbt::Byte(self.rotation as i8)));
                    v.push(("ItemDropChance", Nbt::Float(self.drop_chance)));
                }
            }
        }
        v.push(("Direction", Nbt::Byte(self.dir as i8)));
        v.push(("TileX", Nbt::Int(self.tile[0])));
        v.push(("TileY", Nbt::Int(self.tile[1])));
        v.push(("TileZ", Nbt::Int(self.tile[2])));
        let old = match self.dir {
            0 => 2,
            1 => 1,
            2 => 0,
            _ => 3,
        };
        v.push(("Dir", Nbt::Byte(old)));
    }

    /// `readEntityFromNBT` (`id` is "Painting" or "ItemFrame").
    pub fn from_nbt(t: &crate::world::anvil::Nbt, painting: bool, id: u32, seed: i64) -> Hanging {
        let art = painting.then(|| {
            let m = t.string("Motive").unwrap_or("");
            (0..ART_COUNT).find(|&a| art(a).0 == m).unwrap_or(0)
        });
        let tile = [
            t.int("TileX").unwrap_or(0) as i32,
            t.int("TileY").unwrap_or(0) as i32,
            t.int("TileZ").unwrap_or(0) as i32,
        ];
        let mut h = Hanging::new(id, tile, art, seed);
        if !painting {
            if let Some(it) = t.get("Item") {
                if let Some(s) = crate::world::anvil::item_from_nbt(it) {
                    h.item = Some(Stack { count: 1, ..s });
                    h.rotation = (t.int("ItemRotation").unwrap_or(0) as i32).rem_euclid(4);
                    if let Some(c) = t.float("ItemDropChance") {
                        h.drop_chance = c as f32;
                    }
                }
            }
        }
        let d = match t.int("Direction") {
            Some(d) => d as i32,
            None => match t.int("Dir").unwrap_or(0) {
                0 => 2,
                1 => 1,
                2 => 0,
                _ => 3,
            },
        };
        h.set_direction(d & 3);
        h
    }
}
