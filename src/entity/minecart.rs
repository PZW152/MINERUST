//! `EntityMinecart` and its subclasses `EntityMinecartEmpty`, `...Chest`,
//! `...Furnace`, `...TNT` and `...Hopper` (`EntityMinecartContainer`):
//! `onUpdate` on both sides, `moveOnRail`, `getPos` / `getPosOffset`,
//! `applyEntityCollision`, `attackEntityFrom` / `killMinecart`.

use super::base::{Ent, Fx, Source};
use super::boat::{BoatWorld, Rider};
use super::player::Aabb;
use crate::block;
use crate::item::Stack;
use crate::util::math;
use crate::world::BlockView;

/// `Blocks.activator_rail`
pub const ACTIVATOR_RAIL: u8 = 157;

/// `BlockRailBase.isRailBlock`
pub fn is_rail(id: u8) -> bool {
    matches!(
        id,
        block::RAIL | block::GOLDEN_RAIL | block::DETECTOR_RAIL | ACTIVATOR_RAIL
    )
}

/// `BlockRailBase.isPowered` (the field: rails whose meta has a power bit)
fn powered_kind(id: u8) -> bool {
    id != block::RAIL
}

/// `EntityMinecart.matrix`
const MATRIX: [[[i32; 3]; 2]; 10] = [
    [[0, 0, -1], [0, 0, 1]],
    [[-1, 0, 0], [1, 0, 0]],
    [[-1, -1, 0], [1, 0, 0]],
    [[-1, 0, 0], [1, -1, 0]],
    [[0, 0, -1], [0, -1, 1]],
    [[0, -1, -1], [0, 0, 1]],
    [[0, 0, 1], [1, 0, 0]],
    [[0, 0, 1], [-1, 0, 0]],
    [[0, 0, -1], [-1, 0, 0]],
    [[0, 0, -1], [1, 0, 0]],
];

/// `getMinecartType`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CartKind {
    Empty,
    Chest,
    Furnace,
    Tnt,
    Spawner,
    Hopper,
    Command,
}

impl CartKind {
    pub fn from_type(t: i32) -> CartKind {
        match t {
            1 => CartKind::Chest,
            2 => CartKind::Furnace,
            3 => CartKind::Tnt,
            4 => CartKind::Spawner,
            5 => CartKind::Hopper,
            6 => CartKind::Command,
            _ => CartKind::Empty,
        }
    }
    pub fn type_id(self) -> i32 {
        self as i32
    }
    /// The item `ItemMinecart` uses for this kind.
    pub fn item(self) -> u16 {
        match self {
            CartKind::Chest => 342,
            CartKind::Furnace => 343,
            CartKind::Tnt => 407,
            CartKind::Hopper => 408,
            CartKind::Command => 422,
            _ => 328,
        }
    }
    pub fn from_item(id: u16) -> Option<CartKind> {
        Some(match id {
            328 => CartKind::Empty,
            342 => CartKind::Chest,
            343 => CartKind::Furnace,
            407 => CartKind::Tnt,
            408 => CartKind::Hopper,
            422 => CartKind::Command,
            _ => return None,
        })
    }
    /// `EntityList` name
    pub fn entity_id(self) -> &'static str {
        match self {
            CartKind::Empty => "MinecartRideable",
            CartKind::Chest => "MinecartChest",
            CartKind::Furnace => "MinecartFurnace",
            CartKind::Tnt => "MinecartTNT",
            CartKind::Spawner => "MinecartSpawner",
            CartKind::Hopper => "MinecartHopper",
            CartKind::Command => "MinecartCommandBlock",
        }
    }
    pub fn from_entity_id(s: &str) -> Option<CartKind> {
        Some(match s {
            "MinecartRideable" => CartKind::Empty,
            "MinecartChest" => CartKind::Chest,
            "MinecartFurnace" => CartKind::Furnace,
            "MinecartTNT" => CartKind::Tnt,
            "MinecartSpawner" => CartKind::Spawner,
            "MinecartHopper" => CartKind::Hopper,
            "MinecartCommandBlock" => CartKind::Command,
            _ => return None,
        })
    }
    /// `getSizeInventory` of the container carts.
    pub fn slots(self) -> usize {
        match self {
            CartKind::Chest => 27,
            CartKind::Hopper => 5,
            _ => 0,
        }
    }
    /// `getDefaultDisplayTile`, `...TileData`, `...TileOffset`
    pub fn default_display(self) -> (u8, i32, i32) {
        match self {
            CartKind::Chest => (block::CHEST, 0, 8),
            CartKind::Furnace => (block::LIT_FURNACE, 2, 6),
            CartKind::Tnt => (block::TNT, 0, 6),
            CartKind::Spawner => (52, 0, 6),
            CartKind::Hopper => (154, 0, 1),
            CartKind::Command => (137, 0, 6),
            CartKind::Empty => (block::AIR, 0, 6),
        }
    }
}

/// What the owner must do after a tick or a hit.
#[derive(Clone, Debug, PartialEq)]
pub enum CartOut {
    /// `entityDropItem(stack, 0.0F)` / `dropItemWithOffset`
    Drop(Stack),
    /// `EntityItem` spawned with the container's scatter: position and
    /// motion.
    Scatter([f64; 3], [f64; 3], Stack),
    /// `World.createExplosion(this, x, y, z, size, true)`, and whether the
    /// cart was ignited (`isIgnited`, which spares rails)
    Explode([f64; 3], f32, bool),
    /// `playSoundAtEntity`
    Sound(&'static str, f32, f32),
    /// `setEntityState(10)`: the client's cart is ignited.
    Ignited,
    /// `riddenByEntity.mountEntity(this)` again (a hit broke it).
    Remount,
}

/// The data watcher values the client is sent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CartMeta {
    pub rolling_amplitude: i32,
    pub rolling_direction: i32,
    pub damage: f32,
    /// Data watcher 20, 21 and 22 (`setDisplayBlock` ...)
    pub display: Option<(u8, i32, i32)>,
    /// Data watcher 16 of the furnace cart
    pub powered: bool,
}

#[derive(Clone, Debug)]
pub struct Minecart {
    pub id: u32,
    pub e: Ent,
    pub kind: CartKind,
    /// `isInReverse`
    in_reverse: bool,
    /// `entityName` (from a named item)
    pub name: Option<String>,
    /// `turnProgress` and the target of `setPositionAndRotation2`
    turn: i32,
    target: [f64; 3],
    target_yaw: f64,
    target_pitch: f64,
    velocity: [f64; 3],
    pub rolling_amplitude: i32,
    pub rolling_direction: i32,
    pub damage: f32,
    /// A custom display tile: block, data, offset.
    pub display: Option<(u8, i32, i32)>,
    /// `riddenByEntity != null`
    pub ridden: bool,
    pub velocity_changed: bool,
    // EntityMinecartFurnace
    pub fuel: i32,
    pub push: [f64; 2],
    pub powered: bool,
    // EntityMinecartTNT
    pub fuse: i32,
    // EntityMinecartContainer
    pub slots: Vec<Option<Stack>>,
    drop_contents_when_dead: bool,
    // EntityMinecartHopper
    pub blocked: bool,
    pub transfer: i32,
}

pub const WIDTH: f32 = 0.98;
pub const HEIGHT: f32 = 0.7;

impl Minecart {
    /// `EntityMinecart.createMinecart(world, ...)` without a position
    pub fn new(id: u32, kind: CartKind, seed: i64) -> Minecart {
        let mut e = Ent::new(WIDTH, HEIGHT, seed);
        e.y_offset = e.height / 2.0;
        e.can_trigger_walking = false;
        e.set_position(0.0, 0.0, 0.0);
        Minecart {
            id,
            e,
            kind,
            in_reverse: false,
            name: None,
            turn: 0,
            target: [0.0; 3],
            target_yaw: 0.0,
            target_pitch: 0.0,
            velocity: [0.0; 3],
            rolling_amplitude: 0,
            rolling_direction: 1,
            damage: 0.0,
            display: None,
            ridden: false,
            velocity_changed: false,
            fuel: 0,
            push: [0.0; 2],
            powered: false,
            fuse: -1,
            slots: vec![None; kind.slots()],
            drop_contents_when_dead: true,
            blocked: true,
            transfer: -1,
        }
    }

    /// `new EntityMinecartXxx(world, x, y, z)`
    pub fn at(id: u32, kind: CartKind, x: f64, y: f64, z: f64, seed: i64) -> Minecart {
        let mut c = Minecart::new(id, kind, seed);
        c.e.set_position(x, y, z);
        c.e.motion = [0.0; 3];
        c.e.prev = [x, y, z];
        c
    }

    pub fn meta(&self) -> CartMeta {
        CartMeta {
            rolling_amplitude: self.rolling_amplitude,
            rolling_direction: self.rolling_direction,
            damage: self.damage,
            display: self.display,
            powered: self.powered,
        }
    }

    pub fn set_meta(&mut self, m: CartMeta) {
        self.rolling_amplitude = m.rolling_amplitude;
        self.rolling_direction = m.rolling_direction;
        self.damage = m.damage;
        self.display = m.display;
        self.powered = m.powered;
    }

    /// `getDisplayTile`, `getDisplayTileData`, `getDisplayTileOffset`
    pub fn display_tile(&self) -> (u8, i32, i32) {
        self.display.unwrap_or(self.kind.default_display())
    }

    /// `getMountedYOffset`
    pub fn mounted_y_offset(&self) -> f64 {
        self.e.height as f64 * 0.0 - 0.3f32 as f64
    }

    /// `Entity.updateRiderPosition`
    pub fn rider_position(&self, rider_y_offset: f64) -> [f64; 3] {
        [
            self.e.pos[0],
            self.e.pos[1] + self.mounted_y_offset() + rider_y_offset,
            self.e.pos[2],
        ]
    }

    /// `setPositionAndRotation2`
    pub fn set_position_and_rotation2(&mut self, p: [f64; 3], yaw: f32, pitch: f32, inc: i32) {
        self.target = p;
        self.target_yaw = yaw as f64;
        self.target_pitch = pitch as f64;
        self.turn = inc + 2;
        self.e.motion = self.velocity;
    }

    /// `setVelocity`
    pub fn set_velocity(&mut self, v: [f64; 3]) {
        self.velocity = v;
        self.e.motion = v;
    }

    /// `attackEntityFrom` on the server: `creative` is a creative player,
    /// `explosion` / `fire` the damage source's kind.
    pub fn attack(
        &mut self,
        amount: f32,
        creative: bool,
        explosion: bool,
        fire: bool,
        out: &mut Vec<CartOut>,
    ) {
        if self.e.dead {
            return;
        }
        self.rolling_direction = -self.rolling_direction;
        self.rolling_amplitude = 10;
        self.velocity_changed = true;
        self.damage += amount * 10.0;
        if creative || self.damage > 40.0 {
            if self.ridden {
                out.push(CartOut::Remount);
            }
            if creative && self.name.is_none() {
                self.set_dead(out);
            } else {
                self.kill(explosion, fire, out);
            }
        }
    }

    /// `EntityMinecartContainer.setDead`: the contents spill.
    fn set_dead(&mut self, out: &mut Vec<CartOut>) {
        if self.drop_contents_when_dead {
            self.scatter_contents(out);
        }
        self.e.dead = true;
    }

    fn scatter_contents(&mut self, out: &mut Vec<CartOut>) {
        for i in 0..self.slots.len() {
            let Some(mut st) = self.slots[i] else {
                continue;
            };
            let r = &mut self.e.rand;
            let fx = r.next_float() * 0.8 + 0.1;
            let fy = r.next_float() * 0.8 + 0.1;
            let fz = r.next_float() * 0.8 + 0.1;
            while st.count > 0 {
                let mut n = (r.next_int_n(21) + 10) as u32;
                if n > st.count {
                    n = st.count;
                }
                st.count -= n;
                let p = [
                    self.e.pos[0] + fx as f64,
                    self.e.pos[1] + fy as f64,
                    self.e.pos[2] + fz as f64,
                ];
                let k = 0.05f32;
                let m = [
                    (r.next_gaussian() as f32 * k) as f64,
                    (r.next_gaussian() as f32 * k + 0.2) as f64,
                    (r.next_gaussian() as f32 * k) as f64,
                ];
                out.push(CartOut::Scatter(p, m, Stack::new(st.id, st.meta, n)));
            }
            // the stack is emptied in place (stackSize -= n)
            self.slots[i] = Some(st);
        }
    }

    /// `killMinecart` with the subclasses' additions.
    pub fn kill(&mut self, explosion: bool, fire: bool, out: &mut Vec<CartOut>) {
        // EntityMinecart.killMinecart
        self.set_dead(out);
        // new ItemStack(Items.minecart, 1), named after the cart
        out.push(CartOut::Drop(Stack::new(328, 0, 1)));
        match self.kind {
            CartKind::Chest | CartKind::Hopper => {
                // EntityMinecartContainer.killMinecart: its own loop runs over
                // the emptied stacks
                for i in 0..self.slots.len() {
                    if self.slots[i].is_some() {
                        let r = &mut self.e.rand;
                        let _ = (r.next_float(), r.next_float(), r.next_float());
                    }
                }
                let b = if self.kind == CartKind::Chest {
                    block::CHEST as u16
                } else {
                    154
                };
                out.push(CartOut::Drop(Stack::new(b, 0, 1)));
            }
            CartKind::Furnace => {
                if !explosion {
                    out.push(CartOut::Drop(Stack::new(block::FURNACE as u16, 0, 1)));
                }
            }
            CartKind::Tnt => {
                let v = self.e.motion[0] * self.e.motion[0] + self.e.motion[2] * self.e.motion[2];
                if !explosion {
                    out.push(CartOut::Drop(Stack::new(block::TNT as u16, 0, 1)));
                }
                if fire || explosion || v >= 0.01f32 as f64 {
                    self.explode_cart(v, out);
                }
            }
            _ => {}
        }
    }

    /// `EntityMinecartTNT.explodeCart`
    fn explode_cart(&mut self, v: f64, out: &mut Vec<CartOut>) {
        let mut d = v.sqrt();
        if d > 5.0 {
            d = 5.0;
        }
        let size = (4.0 + self.e.rand.next_double() * 1.5 * d) as f32;
        out.push(CartOut::Explode(self.e.pos, size, self.fuse > -1));
        self.e.dead = true;
    }

    /// `EntityMinecartTNT.ignite`
    pub fn ignite(&mut self, out: &mut Vec<CartOut>) {
        self.fuse = 80;
        out.push(CartOut::Ignited);
        out.push(CartOut::Sound("game.tnt.primed", 1.0, 1.0));
    }

    /// `EntityMinecartFurnace.interactFirst`: coal adds 3600 ticks of fuel
    /// and the cart is pushed away from the player. Returns whether the
    /// coal was used.
    pub fn furnace_interact(&mut self, coal: bool, player: [f64; 3]) -> bool {
        if coal {
            self.fuel += 3600;
        }
        self.push = [self.e.pos[0] - player[0], self.e.pos[2] - player[2]];
        coal
    }

    /// `Container.calcRedstoneFromInventory`
    pub fn comparator_level(&self) -> i32 {
        if self.slots.is_empty() {
            return 0;
        }
        let mut n = 0;
        let mut f = 0.0f32;
        for s in self.slots.iter().flatten() {
            f += s.count as f32 / 64.min(crate::item::max_stack(s.id)) as f32;
            n += 1;
        }
        f /= self.slots.len() as f32;
        math::floor_float(f * 14.0) + if n > 0 { 1 } else { 0 }
    }

    /// `EntityMinecart.onUpdate` and the subclasses' additions. `rider` is
    /// `riddenByEntity` if it is an `EntityLivingBase`; `others` the boxes
    /// of the entities `getCollisionBox` returns (`canBePushed` ones).
    pub fn tick<W: BoatWorld>(
        &mut self,
        w: &mut W,
        remote: bool,
        rider: Option<Rider>,
        others: &[Aabb],
        fx: &mut Vec<Fx>,
        out: &mut Vec<CartOut>,
    ) {
        if self.rolling_amplitude > 0 {
            self.rolling_amplitude -= 1;
        }
        if self.damage > 0.0 {
            self.damage -= 1.0;
        }
        if self.e.pos[1] < -64.0 {
            // kill() -> setDead
            self.set_dead(out);
        }
        if !remote {
            // the portal part of onEntityUpdate
            if self.e.in_portal {
                self.e.in_portal = false;
            } else {
                if self.e.portal_counter > 0 {
                    self.e.portal_counter -= 4;
                }
                if self.e.portal_counter < 0 {
                    self.e.portal_counter = 0;
                }
            }
            if self.e.time_until_portal > 0 {
                self.e.time_until_portal -= 1;
            }
        }
        if remote {
            if self.turn > 0 {
                let t = self.turn as f64;
                let x = self.e.pos[0] + (self.target[0] - self.e.pos[0]) / t;
                let y = self.e.pos[1] + (self.target[1] - self.e.pos[1]) / t;
                let z = self.e.pos[2] + (self.target[2] - self.e.pos[2]) / t;
                let d = math::wrap_angle_to_180_double(self.target_yaw - self.e.yaw as f64);
                self.e.yaw = (self.e.yaw as f64 + d / t) as f32;
                self.e.pitch =
                    (self.e.pitch as f64 + (self.target_pitch - self.e.pitch as f64) / t) as f32;
                self.turn -= 1;
                self.e.set_position(x, y, z);
                self.set_rotation(self.e.yaw, self.e.pitch);
            } else {
                let p = self.e.pos;
                self.e.set_position(p[0], p[1], p[2]);
                self.set_rotation(self.e.yaw, self.e.pitch);
            }
        } else {
            self.e.prev = self.e.pos;
            self.e.motion[1] -= 0.04f32 as f64;
            let x = math::floor_double(self.e.pos[0]);
            let mut y = math::floor_double(self.e.pos[1]);
            let z = math::floor_double(self.e.pos[2]);
            if is_rail(w.peek_block(x, y - 1, z)) {
                y -= 1;
            }
            let max = 0.4;
            let slope = 0.0078125;
            let b = w.peek_block(x, y, z);
            if is_rail(b) {
                let meta = w.peek_meta(x, y, z) as i32;
                self.move_on_rail(w, x, y, z, max, slope, b, meta, rider, others, fx, out);
                if b == ACTIVATOR_RAIL {
                    self.on_activator_rail_pass(meta & 8 != 0, out);
                }
            } else {
                self.move_off_rail(w, max, others, fx, out);
            }
            // doBlockCollisions
            let mut f = Vec::new();
            self.e.do_block_collisions(&*w, &mut f);
            self.collisions(w, f, out);
            self.e.pitch = 0.0;
            let dx = self.e.prev[0] - self.e.pos[0];
            let dz = self.e.prev[2] - self.e.pos[2];
            if dx * dx + dz * dz > 0.001 {
                self.e.yaw = (dz.atan2(dx) * 180.0 / std::f64::consts::PI) as f32;
                if self.in_reverse {
                    self.e.yaw += 180.0;
                }
            }
            let d = math::wrap_angle_180(self.e.yaw - self.e.prev_yaw) as f64;
            if d < -170.0 || d >= 170.0 {
                self.e.yaw += 180.0;
                self.in_reverse = !self.in_reverse;
            }
            self.set_rotation(self.e.yaw, self.e.pitch);
        }
        self.subclass_tick(w, remote, fx, out);
    }

    /// The `Fx::Collide` and `Fx::Damage` of a move, on this side.
    fn collisions<W: BoatWorld>(&mut self, w: &mut W, f: Vec<Fx>, out: &mut Vec<CartOut>) {
        for x in f {
            match x {
                Fx::Collide(bx, by, bz, id) => {
                    if let Some(a) = w.collided(&mut self.e, bx, by, bz, id) {
                        self.attack(a, false, false, false, out);
                    }
                }
                Fx::Damage(s, a) => {
                    let fire = matches!(s, Source::InFire | Source::OnFire | Source::Lava);
                    self.attack(a, false, false, fire, out);
                }
                _ => {}
            }
        }
    }

    /// The rest of the subclasses' `onUpdate`.
    fn subclass_tick<W: BoatWorld>(
        &mut self,
        _w: &mut W,
        remote: bool,
        fx: &mut Vec<Fx>,
        out: &mut Vec<CartOut>,
    ) {
        match self.kind {
            CartKind::Furnace => {
                if self.fuel > 0 {
                    self.fuel -= 1;
                }
                if self.fuel <= 0 {
                    self.push = [0.0; 2];
                }
                // setMinecartPowered(fuel > 0): the client's own fuel is 0
                self.powered = self.fuel > 0;
                if self.powered && self.e.rand.next_int_n(4) == 0 {
                    fx.push(Fx::Particle(
                        "largesmoke",
                        [self.e.pos[0], self.e.pos[1] + 0.8, self.e.pos[2]],
                        [0.0; 3],
                    ));
                }
            }
            CartKind::Tnt => {
                if self.fuse > 0 {
                    self.fuse -= 1;
                    fx.push(Fx::Particle(
                        "smoke",
                        [self.e.pos[0], self.e.pos[1] + 0.5, self.e.pos[2]],
                        [0.0; 3],
                    ));
                } else if self.fuse == 0 {
                    let v =
                        self.e.motion[0] * self.e.motion[0] + self.e.motion[2] * self.e.motion[2];
                    if !remote {
                        self.explode_cart(v, out);
                    }
                }
                if self.e.collided_h {
                    let v =
                        self.e.motion[0] * self.e.motion[0] + self.e.motion[2] * self.e.motion[2];
                    if v >= 0.01f32 as f64 && !remote {
                        self.explode_cart(v, out);
                    }
                }
            }
            _ => {}
        }
    }

    /// `onActivatorRailPass`
    fn on_activator_rail_pass(&mut self, powered: bool, out: &mut Vec<CartOut>) {
        match self.kind {
            CartKind::Tnt => {
                if powered && self.fuse < 0 {
                    self.ignite(out);
                }
            }
            CartKind::Hopper => {
                let b = !powered;
                if b != self.blocked {
                    self.blocked = b;
                }
            }
            CartKind::Empty => {
                // EntityMinecartEmpty does not override it; the riders are
                // thrown off by EntityMinecart in later versions only.
            }
            _ => {}
        }
    }

    /// `moveOffRail`
    fn move_off_rail<W: BoatWorld>(
        &mut self,
        w: &mut W,
        max: f64,
        others: &[Aabb],
        fx: &mut Vec<Fx>,
        out: &mut Vec<CartOut>,
    ) {
        let m = &mut self.e.motion;
        m[0] = m[0].clamp(-max, max);
        m[2] = m[2].clamp(-max, max);
        if self.e.on_ground {
            for v in &mut self.e.motion {
                *v *= 0.5;
            }
        }
        let m = self.e.motion;
        self.move_entity(w, m[0], m[1], m[2], others, fx, out);
        if !self.e.on_ground {
            self.e.motion[0] *= 0.95f32 as f64;
            self.e.motion[1] *= 0.95f32 as f64;
            self.e.motion[2] *= 0.95f32 as f64;
        }
    }

    /// `Entity.moveEntity`: the minecart has no `updateFallState` of its own
    /// but the TNT cart's `fall`.
    fn move_entity<W: BoatWorld>(
        &mut self,
        w: &mut W,
        dx: f64,
        dy: f64,
        dz: f64,
        others: &[Aabb],
        fx: &mut Vec<Fx>,
        out: &mut Vec<CartOut>,
    ) {
        let mut f = Vec::new();
        self.e.move_entity(&*w, dx, dy, dz, false, others, &mut f);
        let mut rest = Vec::new();
        for x in f {
            match x {
                Fx::Fall(d) => {
                    // EntityMinecartTNT.fall
                    if self.kind == CartKind::Tnt && d >= 3.0 {
                        let k = d / 10.0;
                        self.explode_cart((k * k) as f64, out);
                    }
                    // Entity.fall: the rider falls too (the owner)
                }
                Fx::Sound(..) | Fx::Particle(..) => fx.push(x),
                other => rest.push(other),
            }
        }
        self.collisions(w, rest, out);
    }

    /// `moveOnRail` (and `EntityMinecartFurnace`'s addition)
    #[allow(clippy::too_many_arguments)]
    fn move_on_rail<W: BoatWorld>(
        &mut self,
        w: &mut W,
        x: i32,
        y: i32,
        z: i32,
        max: f64,
        slope: f64,
        b: u8,
        mut meta: i32,
        rider: Option<Rider>,
        others: &[Aabb],
        fx: &mut Vec<Fx>,
        out: &mut Vec<CartOut>,
    ) {
        self.e.fall_distance = 0.0;
        let start = get_pos(&*w, self.e.pos[0], self.e.pos[1], self.e.pos[2]);
        self.e.pos[1] = y as f64;
        let mut boosting = false;
        let mut braking = false;
        if b == block::GOLDEN_RAIL {
            boosting = meta & 8 != 0;
            braking = !boosting;
        }
        if powered_kind(b) {
            meta &= 7;
        }
        if (2..=5).contains(&meta) {
            self.e.pos[1] = (y + 1) as f64;
        }
        match meta {
            2 => self.e.motion[0] -= slope,
            3 => self.e.motion[0] += slope,
            4 => self.e.motion[2] += slope,
            5 => self.e.motion[2] -= slope,
            _ => {}
        }
        let m = MATRIX[meta as usize];
        let mut ax = (m[1][0] - m[0][0]) as f64;
        let mut az = (m[1][2] - m[0][2]) as f64;
        let len = (ax * ax + az * az).sqrt();
        let dot = self.e.motion[0] * ax + self.e.motion[2] * az;
        if dot < 0.0 {
            ax = -ax;
            az = -az;
        }
        let mut speed =
            (self.e.motion[0] * self.e.motion[0] + self.e.motion[2] * self.e.motion[2]).sqrt();
        if speed > 2.0 {
            speed = 2.0;
        }
        self.e.motion[0] = speed * ax / len;
        self.e.motion[2] = speed * az / len;
        if let Some(r) = rider {
            let f = r.forward as f64;
            if f > 0.0 {
                let a = r.yaw * std::f32::consts::PI / 180.0;
                let sx = -(a as f64).sin();
                let cz = (a as f64).cos();
                let v = self.e.motion[0] * self.e.motion[0] + self.e.motion[2] * self.e.motion[2];
                if v < 0.01 {
                    self.e.motion[0] += sx * 0.1;
                    self.e.motion[2] += cz * 0.1;
                    braking = false;
                }
            }
        }
        if braking {
            let s =
                (self.e.motion[0] * self.e.motion[0] + self.e.motion[2] * self.e.motion[2]).sqrt();
            if s < 0.03 {
                self.e.motion = [0.0; 3];
            } else {
                self.e.motion[0] *= 0.5;
                self.e.motion[1] *= 0.0;
                self.e.motion[2] *= 0.5;
            }
        }
        let x0 = x as f64 + 0.5 + m[0][0] as f64 * 0.5;
        let z0 = z as f64 + 0.5 + m[0][2] as f64 * 0.5;
        let x1 = x as f64 + 0.5 + m[1][0] as f64 * 0.5;
        let z1 = z as f64 + 0.5 + m[1][2] as f64 * 0.5;
        let ax = x1 - x0;
        let az = z1 - z0;
        let t;
        if ax == 0.0 {
            self.e.pos[0] = x as f64 + 0.5;
            t = self.e.pos[2] - z as f64;
        } else if az == 0.0 {
            self.e.pos[2] = z as f64 + 0.5;
            t = self.e.pos[0] - x as f64;
        } else {
            let px = self.e.pos[0] - x0;
            let pz = self.e.pos[2] - z0;
            t = (px * ax + pz * az) * 2.0;
        }
        self.e.pos[0] = x0 + ax * t;
        self.e.pos[2] = z0 + az * t;
        let p = self.e.pos;
        self.e
            .set_position(p[0], p[1] + self.e.y_offset as f64, p[2]);
        let mut vx = self.e.motion[0];
        let mut vz = self.e.motion[2];
        if self.ridden {
            vx *= 0.75;
            vz *= 0.75;
        }
        vx = vx.clamp(-max, max);
        vz = vz.clamp(-max, max);
        self.move_entity(w, vx, 0.0, vz, others, fx, out);
        let fx_ = math::floor_double(self.e.pos[0]) - x;
        let fz_ = math::floor_double(self.e.pos[2]) - z;
        if m[0][1] != 0 && fx_ == m[0][0] && fz_ == m[0][2] {
            let p = self.e.pos;
            self.e.set_position(p[0], p[1] + m[0][1] as f64, p[2]);
        } else if m[1][1] != 0 && fx_ == m[1][0] && fz_ == m[1][2] {
            let p = self.e.pos;
            self.e.set_position(p[0], p[1] + m[1][1] as f64, p[2]);
        }
        self.apply_drag();
        let end = get_pos(&*w, self.e.pos[0], self.e.pos[1], self.e.pos[2]);
        if let (Some(e), Some(s)) = (end, start) {
            let d = (s[1] - e[1]) * 0.05;
            let sp =
                (self.e.motion[0] * self.e.motion[0] + self.e.motion[2] * self.e.motion[2]).sqrt();
            if sp > 0.0 {
                self.e.motion[0] = self.e.motion[0] / sp * (sp + d);
                self.e.motion[2] = self.e.motion[2] / sp * (sp + d);
            }
            let p = self.e.pos;
            self.e.set_position(p[0], e[1], p[2]);
        }
        let nx = math::floor_double(self.e.pos[0]);
        let nz = math::floor_double(self.e.pos[2]);
        if nx != x || nz != z {
            let sp =
                (self.e.motion[0] * self.e.motion[0] + self.e.motion[2] * self.e.motion[2]).sqrt();
            self.e.motion[0] = sp * (nx - x) as f64;
            self.e.motion[2] = sp * (nz - z) as f64;
        }
        if boosting {
            let sp =
                (self.e.motion[0] * self.e.motion[0] + self.e.motion[2] * self.e.motion[2]).sqrt();
            if sp > 0.01 {
                let k = 0.06;
                self.e.motion[0] += self.e.motion[0] / sp * k;
                self.e.motion[2] += self.e.motion[2] / sp * k;
            } else if meta == 1 {
                if block::info(w.peek_block(x - 1, y, z)).normal_cube() {
                    self.e.motion[0] = 0.02;
                } else if block::info(w.peek_block(x + 1, y, z)).normal_cube() {
                    self.e.motion[0] = -0.02;
                }
            } else if meta == 0 {
                if block::info(w.peek_block(x, y, z - 1)).normal_cube() {
                    self.e.motion[2] = 0.02;
                } else if block::info(w.peek_block(x, y, z + 1)).normal_cube() {
                    self.e.motion[2] = -0.02;
                }
            }
        }
        if self.kind == CartKind::Furnace {
            // EntityMinecartFurnace.moveOnRail
            let mut d = self.push[0] * self.push[0] + self.push[1] * self.push[1];
            let mv = self.e.motion[0] * self.e.motion[0] + self.e.motion[2] * self.e.motion[2];
            if d > 1.0e-4 && mv > 0.001 {
                d = math::sqrt_double(d) as f64;
                self.push[0] /= d;
                self.push[1] /= d;
                if self.push[0] * self.e.motion[0] + self.push[1] * self.e.motion[2] < 0.0 {
                    self.push = [0.0; 2];
                } else {
                    self.push = [self.e.motion[0], self.e.motion[2]];
                }
            }
        }
    }

    /// `applyDrag` (and the furnace's and the containers' overrides)
    fn apply_drag(&mut self) {
        match self.kind {
            CartKind::Furnace => {
                let mut d = self.push[0] * self.push[0] + self.push[1] * self.push[1];
                if d > 1.0e-4 {
                    d = math::sqrt_double(d) as f64;
                    self.push[0] /= d;
                    self.push[1] /= d;
                    let k = 0.05;
                    self.e.motion[0] *= 0.8f32 as f64;
                    self.e.motion[1] *= 0.0;
                    self.e.motion[2] *= 0.8f32 as f64;
                    self.e.motion[0] += self.push[0] * k;
                    self.e.motion[2] += self.push[1] * k;
                } else {
                    self.e.motion[0] *= 0.98f32 as f64;
                    self.e.motion[1] *= 0.0;
                    self.e.motion[2] *= 0.98f32 as f64;
                }
                self.base_drag();
            }
            CartKind::Chest | CartKind::Hopper => {
                let n = 15 - self.comparator_level();
                let f = 0.98f32 + n as f32 * 0.001f32;
                self.e.motion[0] *= f as f64;
                self.e.motion[1] *= 0.0;
                self.e.motion[2] *= f as f64;
            }
            _ => self.base_drag(),
        }
    }

    /// `EntityMinecart.applyDrag`
    fn base_drag(&mut self) {
        let k = if self.ridden { 0.997f32 } else { 0.96f32 } as f64;
        self.e.motion[0] *= k;
        self.e.motion[1] *= 0.0;
        self.e.motion[2] *= k;
    }

    /// `Entity.setRotation`
    fn set_rotation(&mut self, yaw: f32, pitch: f32) {
        self.e.yaw = yaw % 360.0;
        self.e.pitch = pitch % 360.0;
    }

    /// `applyEntityCollision(other)` for an entity that is not a minecart:
    /// `living` excludes players and iron golems. Returns whether the
    /// other should be seated (`mountEntity`) and the velocities added to
    /// this cart and to the other.
    pub fn push_entity(
        &self,
        other: [f64; 3],
        living_mob: bool,
        other_riding: bool,
    ) -> (bool, Option<([f64; 3], [f64; 3])>) {
        let mount = living_mob
            && self.kind == CartKind::Empty
            && self.e.motion[0] * self.e.motion[0] + self.e.motion[2] * self.e.motion[2] > 0.01
            && !self.ridden
            && !other_riding;
        let Some((dx, dz)) = self.push_dir(other) else {
            return (mount, None);
        };
        (mount, Some(([-dx, 0.0, -dz], [dx / 4.0, 0.0, dz / 4.0])))
    }

    fn push_dir(&self, other: [f64; 3]) -> Option<(f64, f64)> {
        let mut dx = other[0] - self.e.pos[0];
        let mut dz = other[2] - self.e.pos[2];
        let mut d = dx * dx + dz * dz;
        if d < 1.0e-4f32 as f64 {
            return None;
        }
        d = math::sqrt_double(d) as f64;
        dx /= d;
        dz /= d;
        let mut k = 1.0 / d;
        if k > 1.0 {
            k = 1.0;
        }
        dx *= k;
        dz *= k;
        dx *= 0.1f32 as f64;
        dz *= 0.1f32 as f64;
        dx *= (1.0 - self.e.entity_collision_reduction) as f64;
        dz *= (1.0 - self.e.entity_collision_reduction) as f64;
        dx *= 0.5;
        dz *= 0.5;
        Some((dx, dz))
    }
}

/// `EntityMinecart.applyEntityCollision(other)` where both are minecarts
/// (`a` is `this`).
pub fn carts_collide(a: &mut Minecart, b: &mut Minecart) {
    let Some((dx, dz)) = a.push_dir(b.e.pos) else {
        return;
    };
    let rx = b.e.pos[0] - a.e.pos[0];
    let rz = b.e.pos[2] - a.e.pos[2];
    let l = (rx * rx + rz * rz).sqrt();
    let n = if l < 1.0e-4 {
        [0.0, 0.0]
    } else {
        [rx / l, rz / l]
    };
    let r = std::f32::consts::PI;
    let (c, s) = (
        math::cos(a.e.yaw * r / 180.0) as f64,
        math::sin(a.e.yaw * r / 180.0) as f64,
    );
    let lf = (c * c + s * s).sqrt();
    let f = if lf < 1.0e-4 {
        [0.0, 0.0]
    } else {
        [c / lf, s / lf]
    };
    let dot = (n[0] * f[0] + n[1] * f[1]).abs();
    if dot < 0.8f32 as f64 {
        return;
    }
    let mut sx = b.e.motion[0] + a.e.motion[0];
    let mut sz = b.e.motion[2] + a.e.motion[2];
    let bf = b.kind == CartKind::Furnace;
    let af = a.kind == CartKind::Furnace;
    if bf && !af {
        a.e.motion[0] *= 0.2f32 as f64;
        a.e.motion[2] *= 0.2f32 as f64;
        a.e.motion[0] += b.e.motion[0] - dx;
        a.e.motion[2] += b.e.motion[2] - dz;
        b.e.motion[0] *= 0.95f32 as f64;
        b.e.motion[2] *= 0.95f32 as f64;
    } else if !bf && af {
        b.e.motion[0] *= 0.2f32 as f64;
        b.e.motion[2] *= 0.2f32 as f64;
        b.e.motion[0] += a.e.motion[0] + dx;
        b.e.motion[2] += a.e.motion[2] + dz;
        a.e.motion[0] *= 0.95f32 as f64;
        a.e.motion[2] *= 0.95f32 as f64;
    } else {
        sx /= 2.0;
        sz /= 2.0;
        a.e.motion[0] *= 0.2f32 as f64;
        a.e.motion[2] *= 0.2f32 as f64;
        a.e.motion[0] += sx - dx;
        a.e.motion[2] += sz - dz;
        b.e.motion[0] *= 0.2f32 as f64;
        b.e.motion[2] *= 0.2f32 as f64;
        b.e.motion[0] += sx + dx;
        b.e.motion[2] += sz + dz;
    }
}

/// The rail shape at a block, `isPowered` rails masked.
fn rail_meta(w: &dyn BlockView, x: i32, y: i32, z: i32) -> Option<i32> {
    let b = w.peek_block(x, y, z);
    if !is_rail(b) {
        return None;
    }
    let mut m = w.peek_meta(x, y, z) as i32;
    if powered_kind(b) {
        m &= 7;
    }
    Some(m)
}

/// `EntityMinecart.getPos`
pub fn get_pos(w: &dyn BlockView, mut x: f64, mut y: f64, mut z: f64) -> Option<[f64; 3]> {
    let bx = math::floor_double(x);
    let mut by = math::floor_double(y);
    let bz = math::floor_double(z);
    if is_rail(w.peek_block(bx, by - 1, bz)) {
        by -= 1;
    }
    let meta = rail_meta(w, bx, by, bz)?;
    y = by as f64;
    if (2..=5).contains(&meta) {
        y = (by + 1) as f64;
    }
    let _ = y;
    let m = MATRIX[meta as usize];
    let x0 = bx as f64 + 0.5 + m[0][0] as f64 * 0.5;
    let y0 = by as f64 + 0.5 + m[0][1] as f64 * 0.5;
    let z0 = bz as f64 + 0.5 + m[0][2] as f64 * 0.5;
    let x1 = bx as f64 + 0.5 + m[1][0] as f64 * 0.5;
    let y1 = by as f64 + 0.5 + m[1][1] as f64 * 0.5;
    let z1 = bz as f64 + 0.5 + m[1][2] as f64 * 0.5;
    let ax = x1 - x0;
    let ay = (y1 - y0) * 2.0;
    let az = z1 - z0;
    let t;
    if ax == 0.0 {
        x = bx as f64 + 0.5;
        t = z - bz as f64;
    } else if az == 0.0 {
        z = bz as f64 + 0.5;
        t = x - bx as f64;
    } else {
        let px = x - x0;
        let pz = z - z0;
        t = (px * ax + pz * az) * 2.0;
    }
    x = x0 + ax * t;
    let mut yy = y0 + ay * t;
    z = z0 + az * t;
    if ay < 0.0 {
        yy += 1.0;
    }
    if ay > 0.0 {
        yy += 0.5;
    }
    Some([x, yy, z])
}

/// `EntityMinecart.getPosOffset`
pub fn get_pos_offset(
    w: &dyn BlockView,
    mut x: f64,
    mut y: f64,
    mut z: f64,
    d: f64,
) -> Option<[f64; 3]> {
    let bx = math::floor_double(x);
    let mut by = math::floor_double(y);
    let bz = math::floor_double(z);
    if is_rail(w.peek_block(bx, by - 1, bz)) {
        by -= 1;
    }
    let meta = rail_meta(w, bx, by, bz)?;
    y = by as f64;
    if (2..=5).contains(&meta) {
        y = (by + 1) as f64;
    }
    let m = MATRIX[meta as usize];
    let mut ax = (m[1][0] - m[0][0]) as f64;
    let mut az = (m[1][2] - m[0][2]) as f64;
    let l = (ax * ax + az * az).sqrt();
    ax /= l;
    az /= l;
    x += ax * d;
    z += az * d;
    if m[0][1] != 0
        && math::floor_double(x) - bx == m[0][0]
        && math::floor_double(z) - bz == m[0][2]
    {
        y += m[0][1] as f64;
    } else if m[1][1] != 0
        && math::floor_double(x) - bx == m[1][0]
        && math::floor_double(z) - bz == m[1][2]
    {
        y += m[1][1] as f64;
    }
    get_pos(w, x, y, z)
}
