//! `TileEntityHopper`: the inventory lookup every hopper, dropper and
//! hopper minecart shares (`getInventoryAtPosition`, `pushItems`,
//! `pullItems`), and the hopper's own tick (`updateHopper`).

use crate::block;
use crate::entity::vehicle::Vehicle;
use crate::item::Stack;
use crate::java::JavaRandom;
use crate::play::{Play, Pos, Tile};
use crate::world::ServerWorld;

/// An `IInventory` found at a position.
#[derive(Clone, Debug, PartialEq)]
pub enum InvRef {
    /// A tile entity, or the two halves of an `InventoryLargeChest`
    /// (first half first).
    Tiles(Vec<Pos>),
    /// An `EntityMinecartChest` / `EntityMinecartHopper`.
    Cart(u32),
}

/// `ISidedInventory`s among the tile entities.
#[derive(Clone, Copy, PartialEq)]
enum Sided {
    No,
    Furnace,
    Brewing,
}

/// `Facing.offsetsXForSide` and friends.
pub const FX: [i32; 6] = [0, 0, 0, 0, -1, 1];
pub const FY: [i32; 6] = [-1, 1, 0, 0, 0, 0];
pub const FZ: [i32; 6] = [0, 0, -1, 1, 0, 0];

/// `ItemStack.getMaxStackSize`, at most `getInventoryStackLimit` (64).
fn max_size(s: &Stack) -> u32 {
    crate::item::max_stack(s.id).min(64)
}

/// `TileEntityHopper.canCombine`
fn can_combine(a: &Stack, b: &Stack) -> bool {
    a.id == b.id && a.meta == b.meta && a.count <= max_size(a) && a.tag == b.tag
}

impl Play {
    /// `TileEntityHopper.getInventoryAtPosition`: a container tile entity
    /// (a chest as `BlockChest.getInventory` gives it), else one of the
    /// container minecarts in the unit box there, picked with the world's
    /// random.
    pub fn inventory_at(
        &self,
        w: &ServerWorld,
        x: f64,
        y: f64,
        z: f64,
        r: &mut JavaRandom,
    ) -> Option<InvRef> {
        let p = [
            crate::util::math::floor_double(x),
            crate::util::math::floor_double(y),
            crate::util::math::floor_double(z),
        ];
        let id = w.peek_block(p[0], p[1], p[2]);
        let tile = match self.tiles.get(&p) {
            Some(Tile::Chest { .. }) if id == block::CHEST => Some(self.chest_inventory(w, p)),
            Some(Tile::Chest { .. }) => Some(Some(InvRef::Tiles(vec![p]))),
            Some(
                Tile::Furnace { .. }
                | Tile::Dispenser { .. }
                | Tile::Hopper { .. }
                | Tile::Brewing { .. },
            ) => Some(Some(InvRef::Tiles(vec![p]))),
            _ => None,
        };
        if let Some(t) = tile {
            // a container tile entity: the chest's own answer stands, even null
            if t.is_some() {
                return t;
            }
        }
        // IEntitySelector.selectInventories in [x, x + 1) etc.
        let area = crate::entity::player::Aabb {
            min: [x, y, z],
            max: [x + 1.0, y + 1.0, z + 1.0],
        };
        let carts: Vec<u32> = self
            .vehicles
            .iter()
            .filter_map(|v| match &v.v {
                Vehicle::Cart(c)
                    if !c.e.dead
                        && matches!(
                            c.kind,
                            crate::entity::minecart::CartKind::Chest
                                | crate::entity::minecart::CartKind::Hopper
                        )
                        && c.e.bb.intersects(&area) =>
                {
                    Some(c.id)
                }
                _ => None,
            })
            .collect();
        if carts.is_empty() {
            return None;
        }
        Some(InvRef::Cart(
            carts[r.next_int_n(carts.len() as i32) as usize],
        ))
    }

    /// `BlockChest.getInventory`: null under a solid block (either half),
    /// else the large chest with the north or west half first.
    fn chest_inventory(&self, w: &ServerWorld, p: Pos) -> Option<InvRef> {
        let normal = |x: i32, y: i32, z: i32| block::info(w.peek_block(x, y, z)).normal_cube();
        let chest = |x: i32, y: i32, z: i32| w.peek_block(x, y, z) == block::CHEST;
        if normal(p[0], p[1] + 1, p[2]) {
            return None;
        }
        for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            if chest(p[0] + dx, p[1], p[2] + dz) && normal(p[0] + dx, p[1] + 1, p[2] + dz) {
                return None;
            }
        }
        let mut halves = vec![p];
        if chest(p[0] - 1, p[1], p[2]) {
            halves.insert(0, [p[0] - 1, p[1], p[2]]);
        }
        if chest(p[0] + 1, p[1], p[2]) {
            halves.push([p[0] + 1, p[1], p[2]]);
        }
        if chest(p[0], p[1], p[2] - 1) {
            halves.insert(0, [p[0], p[1], p[2] - 1]);
        }
        if chest(p[0], p[1], p[2] + 1) {
            halves.push([p[0], p[1], p[2] + 1]);
        }
        // only halves that have a tile entity count
        halves.retain(|q| matches!(self.tiles.get(q), Some(Tile::Chest { .. })));
        Some(InvRef::Tiles(halves))
    }

    fn sided(&self, inv: &InvRef) -> Sided {
        match inv {
            InvRef::Tiles(ps) if ps.len() == 1 => match self.tiles.get(&ps[0]) {
                Some(Tile::Furnace { .. }) => Sided::Furnace,
                Some(Tile::Brewing { .. }) => Sided::Brewing,
                _ => Sided::No,
            },
            _ => Sided::No,
        }
    }

    /// A copy of the inventory's slots, in `getStackInSlot` order.
    pub fn inv_slots(&self, inv: &InvRef) -> Vec<Option<Stack>> {
        match inv {
            InvRef::Tiles(ps) => {
                let mut v = Vec::new();
                for p in ps {
                    match self.tiles.get(p) {
                        Some(Tile::Chest { slots }) => v.extend_from_slice(slots),
                        Some(Tile::Furnace { slots, .. }) => v.extend_from_slice(slots),
                        Some(Tile::Dispenser { slots, .. }) => v.extend_from_slice(slots),
                        Some(Tile::Hopper { slots, .. }) => v.extend_from_slice(slots),
                        Some(Tile::Brewing { slots, .. }) => v.extend_from_slice(slots),
                        _ => {}
                    }
                }
                v
            }
            InvRef::Cart(id) => self
                .vehicles
                .iter()
                .find_map(|v| match &v.v {
                    Vehicle::Cart(c) if c.id == *id => Some(c.slots.clone()),
                    _ => None,
                })
                .unwrap_or_default(),
        }
    }

    /// Writes slots taken with `inv_slots` back.
    pub fn inv_store(&mut self, inv: &InvRef, v: &[Option<Stack>]) {
        match inv {
            InvRef::Tiles(ps) => {
                let mut off = 0;
                for p in ps {
                    let dst: &mut [Option<Stack>] = match self.tiles.get_mut(p) {
                        Some(Tile::Chest { slots }) => slots,
                        Some(Tile::Furnace { slots, .. }) => slots,
                        Some(Tile::Dispenser { slots, .. }) => slots,
                        Some(Tile::Hopper { slots, .. }) => slots,
                        Some(Tile::Brewing { slots, .. }) => slots,
                        _ => continue,
                    };
                    let n = dst.len();
                    dst.copy_from_slice(&v[off..off + n]);
                    off += n;
                }
            }
            InvRef::Cart(id) => {
                for t in self.vehicles.iter_mut() {
                    if let Vehicle::Cart(c) = &mut t.v {
                        if c.id == *id {
                            c.slots.copy_from_slice(v);
                        }
                    }
                }
            }
        }
        self.inv_dirty = true;
    }

    /// `ISidedInventory.getAccessibleSlotsFromSide`, or every slot.
    fn accessible(&self, inv: &InvRef, side: i32, n: usize) -> Vec<usize> {
        match (self.sided(inv), side > -1) {
            // TileEntityFurnace: slotsBottom {2, 1}, slotsTop {0}, slotsSides {1}
            (Sided::Furnace, true) => match side {
                0 => vec![2, 1],
                1 => vec![0],
                _ => vec![1],
            },
            // TileEntityBrewingStand: inputSlots {3}, outputSlots {0, 1, 2}
            (Sided::Brewing, true) => {
                if side == 1 {
                    vec![3]
                } else {
                    vec![0, 1, 2]
                }
            }
            _ => (0..n).collect(),
        }
    }

    /// `isItemValidForSlot` and, for a sided inventory, `canInsertItem`.
    fn can_push(&self, inv: &InvRef, slot: usize, s: &Stack) -> bool {
        match self.sided(inv) {
            Sided::Furnace => match slot {
                2 => false,
                1 => crate::play::burn_time(s) > 0,
                _ => true,
            },
            Sided::Brewing => {
                if slot == 3 {
                    crate::play::is_potion_ingredient(s)
                } else {
                    s.id == 373 || s.id == 374
                }
            }
            Sided::No => true,
        }
    }

    /// `canExtractItem` for a sided inventory.
    fn can_pull(&self, inv: &InvRef, slot: usize, s: &Stack, side: i32) -> bool {
        match self.sided(inv) {
            // TileEntityFurnace: the fuel slot only gives up empty buckets downwards
            Sided::Furnace => side != 0 || slot != 1 || s.id == 325,
            _ => true,
        }
    }

    /// Whether the inventory is a hopper tile (`setTransferCooldown(8)` on insertion).
    fn hopper_of(&self, inv: &InvRef) -> Option<Pos> {
        match inv {
            InvRef::Tiles(ps) if ps.len() == 1 => match self.tiles.get(&ps[0]) {
                Some(Tile::Hopper { .. }) => Some(ps[0]),
                _ => None,
            },
            _ => None,
        }
    }

    /// `TileEntityHopper.pushItems(inventory, stack, side)`: what is left.
    pub fn push_items(&mut self, inv: &InvRef, mut s: Stack, side: i32) -> Option<Stack> {
        let mut slots = self.inv_slots(inv);
        let order = self.accessible(inv, side, slots.len());
        let mut changed = false;
        for k in order {
            if s.count == 0 {
                break;
            }
            if k >= slots.len() || !self.can_push(inv, k, &s) {
                continue;
            }
            let mut moved = false;
            match &mut slots[k] {
                None => {
                    slots[k] = Some(s);
                    s.count = 0;
                    moved = true;
                }
                Some(t) if can_combine(t, &s) => {
                    let room = max_size(&s).saturating_sub(t.count);
                    let n = s.count.min(room);
                    s.count -= n;
                    t.count += n;
                    moved = n > 0;
                }
                _ => {}
            }
            if moved {
                changed = true;
            }
        }
        if changed {
            self.inv_store(inv, &slots);
            if let Some(h) = self.hopper_of(inv) {
                if let Some(Tile::Hopper { cooldown, .. }) = self.tiles.get_mut(&h) {
                    *cooldown = 8;
                }
            }
        }
        if s.count == 0 {
            None
        } else {
            Some(s)
        }
    }

    /// `isFullInventory`
    fn is_full_inventory(&self, inv: &InvRef, side: i32) -> bool {
        let slots = self.inv_slots(inv);
        self.accessible(inv, side, slots.len())
            .into_iter()
            .all(|k| matches!(slots.get(k), Some(Some(s)) if s.count == max_size(s)))
    }

    /// `isEmptyInventory`
    fn is_empty_inventory(&self, inv: &InvRef, side: i32) -> bool {
        let slots = self.inv_slots(inv);
        self.accessible(inv, side, slots.len())
            .into_iter()
            .all(|k| matches!(slots.get(k), Some(None) | None))
    }

    /// `TileEntityHopper.updateEntity` for every hopper, in position order.
    pub fn tick_hoppers(&mut self, w: &mut ServerWorld) {
        let hoppers: Vec<Pos> = self
            .tiles
            .iter()
            .filter(|(_, t)| matches!(t, Tile::Hopper { .. }))
            .map(|(p, _)| *p)
            .collect();
        for p in hoppers {
            if !w.chunk_exists(p[0] >> 4, p[2] >> 4)
                || w.peek_block(p[0], p[1], p[2]) != block::HOPPER
            {
                continue;
            }
            let ready = match self.tiles.get_mut(&p) {
                Some(Tile::Hopper { cooldown, .. }) => {
                    *cooldown -= 1;
                    if *cooldown > 0 {
                        false
                    } else {
                        *cooldown = 0;
                        true
                    }
                }
                _ => false,
            };
            if ready {
                self.update_hopper(w, p);
            }
        }
    }

    /// `TileEntityHopper.updateHopper`
    fn update_hopper(&mut self, w: &mut ServerWorld, p: Pos) {
        let meta = w.peek_meta(p[0], p[1], p[2]) as i32;
        // getActiveStateFromMetadata: bit 8 clear
        if meta & 8 == 8 {
            return;
        }
        let Some(Tile::Hopper { slots, cooldown }) = self.tiles.get(&p).cloned() else {
            return;
        };
        if cooldown > 0 {
            return;
        }
        let empty = slots.iter().all(|s| s.is_none());
        let full = slots
            .iter()
            .all(|s| matches!(s, Some(s) if s.count == max_size(s)));
        let mut moved = false;
        if !empty {
            moved = self.transfer_items_out(w, p, meta & 7);
        }
        if !full {
            moved = self.capture_dropped_items(w, p) || moved;
        }
        if moved {
            if let Some(Tile::Hopper { cooldown, .. }) = self.tiles.get_mut(&p) {
                *cooldown = 8;
            }
            self.inv_dirty = true;
        }
    }

    /// `transferItemsOut`: one item into the inventory it faces.
    fn transfer_items_out(&mut self, w: &mut ServerWorld, p: Pos, dir: i32) -> bool {
        let d = (dir as usize).min(5);
        let mut r = std::mem::replace(&mut w.rand, JavaRandom::new(0));
        let target = self.inventory_at(
            w,
            (p[0] + FX[d]) as f64,
            (p[1] + FY[d]) as f64,
            (p[2] + FZ[d]) as f64,
            &mut r,
        );
        w.rand = r;
        let Some(target) = target else {
            return false;
        };
        let side = [1, 0, 3, 2, 5, 4][d];
        if self.is_full_inventory(&target, side) {
            return false;
        }
        let me = InvRef::Tiles(vec![p]);
        for k in 0..5 {
            let mut own = self.inv_slots(&me);
            let Some(s) = own[k] else { continue };
            let copy = s;
            // decrStackSize(k, 1)
            let one = Stack { count: 1, ..s };
            own[k] = if s.count > 1 {
                Some(Stack {
                    count: s.count - 1,
                    ..s
                })
            } else {
                None
            };
            self.inv_store(&me, &own);
            if self.push_items(&target, one, side).is_none() {
                return true;
            }
            let mut own = self.inv_slots(&me);
            own[k] = Some(copy);
            self.inv_store(&me, &own);
        }
        false
    }

    /// `captureDroppedItems`: from the inventory above, else the first
    /// item entity in the block above.
    fn capture_dropped_items(&mut self, w: &mut ServerWorld, p: Pos) -> bool {
        let mut r = std::mem::replace(&mut w.rand, JavaRandom::new(0));
        let src = self.inventory_at(w, p[0] as f64, (p[1] + 1) as f64, p[2] as f64, &mut r);
        w.rand = r;
        let me = InvRef::Tiles(vec![p]);
        if let Some(src) = src {
            let side = 0;
            if self.is_empty_inventory(&src, side) {
                return false;
            }
            let n = self.inv_slots(&src).len();
            for k in self.accessible(&src, side, n) {
                // pullItems
                let mut from = self.inv_slots(&src);
                let Some(s) = from[k] else { continue };
                if !self.can_pull(&src, k, &s, side) {
                    continue;
                }
                let copy = s;
                let one = Stack { count: 1, ..s };
                from[k] = if s.count > 1 {
                    Some(Stack {
                        count: s.count - 1,
                        ..s
                    })
                } else {
                    None
                };
                self.inv_store(&src, &from);
                if self.push_items(&me, one, -1).is_none() {
                    return true;
                }
                let mut from = self.inv_slots(&src);
                from[k] = Some(copy);
                self.inv_store(&src, &from);
            }
            return false;
        }
        // getItems: EntityItem in [x, x + 1) x [y + 1, y + 2) x [z, z + 1)
        let area = crate::entity::player::Aabb {
            min: [p[0] as f64, (p[1] + 1) as f64, p[2] as f64],
            max: [(p[0] + 1) as f64, (p[1] + 2) as f64, (p[2] + 1) as f64],
        };
        let Some(i) = self
            .items
            .iter()
            .position(|it| !it.e.dead && it.bb().intersects(&area))
        else {
            return false;
        };
        // putDropInInventoryAllSlots
        let s = self.items[i].stack;
        match self.push_items(&me, s, -1) {
            Some(rest) => {
                self.items[i].stack = rest;
                false
            }
            None => {
                self.items[i].e.dead = true;
                true
            }
        }
    }
}
