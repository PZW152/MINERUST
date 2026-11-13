//! `Container` and its subclasses: the slot lists of every window,
//! `slotClick` (all seven modes, including the drag-splitting protocol and
//! double-click collection), `mergeItemStack` and each window's
//! `transferStackInSlot`. The same code runs on the server and, as the
//! client's `PlayerControllerMP.windowClick` prediction, on the client.

use crate::crafting::{self, Grid};
use crate::inventory::Inventory;
use crate::item::{self, max_stack, Stack};

/// Which `IInventory` a slot reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Inv {
    /// `InventoryPlayer`: 0..36 main, 36..40 armour (boots first).
    Player,
    /// `InventoryCrafting`
    Craft,
    /// `InventoryCraftResult`
    Result,
    /// The tile entity or minecart behind the window.
    Tile,
}

/// The `Slot` subclass.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotKind {
    Plain,
    /// `SlotCrafting`
    CraftResult,
    /// `ContainerPlayer$1` with its `armorType` (0 is the helmet).
    Armor(usize),
    /// `SlotFurnace`
    FurnaceOut,
    /// `ContainerBrewingStand$Potion`
    Potion,
    /// `ContainerBrewingStand$Ingredient`
    Ingredient,
    /// `ContainerEnchantment$2` in a one-item inventory
    Enchant,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SlotDef {
    pub inv: Inv,
    pub index: usize,
    pub kind: SlotKind,
    pub x: i32,
    pub y: i32,
}

/// The `Container` subclass of the open window.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Window {
    /// `ContainerPlayer`
    Player,
    /// `ContainerWorkbench`
    Workbench,
    /// `ContainerFurnace`
    Furnace,
    /// `ContainerBrewingStand`
    Brewing,
    /// `ContainerChest` with its row count.
    Chest(usize),
    /// `ContainerDispenser`
    Dispenser,
    /// `ContainerEnchantment`
    Enchant,
    /// `GuiContainerCreative$ContainerCreative`: the 45 item-list slots
    /// (the tile slots) and the hotbar.
    Creative,
}

fn slot(inv: Inv, index: usize, kind: SlotKind, x: i32, y: i32) -> SlotDef {
    SlotDef {
        inv,
        index,
        kind,
        x,
        y,
    }
}

/// The 27 main and 9 hotbar slots every window ends with.
fn player_slots(v: &mut Vec<SlotDef>, y0: i32) {
    for r in 0..3 {
        for c in 0..9 {
            v.push(slot(
                Inv::Player,
                c + r * 9 + 9,
                SlotKind::Plain,
                8 + c as i32 * 18,
                y0 + r as i32 * 18,
            ));
        }
    }
    for c in 0..9 {
        v.push(slot(
            Inv::Player,
            c,
            SlotKind::Plain,
            8 + c as i32 * 18,
            y0 + 58,
        ));
    }
}

impl Window {
    /// The `addSlotToContainer` calls of the constructor, in order.
    pub fn slots(self) -> Vec<SlotDef> {
        let mut v = Vec::new();
        match self {
            Window::Player => {
                v.push(slot(Inv::Result, 0, SlotKind::CraftResult, 144, 36));
                for r in 0..2 {
                    for c in 0..2 {
                        v.push(slot(
                            Inv::Craft,
                            c + r * 2,
                            SlotKind::Plain,
                            88 + c as i32 * 18,
                            26 + r as i32 * 18,
                        ));
                    }
                }
                for k in 0..4 {
                    v.push(slot(
                        Inv::Player,
                        39 - k,
                        SlotKind::Armor(k),
                        8,
                        8 + k as i32 * 18,
                    ));
                }
                player_slots(&mut v, 84);
            }
            Window::Workbench => {
                v.push(slot(Inv::Result, 0, SlotKind::CraftResult, 124, 35));
                for r in 0..3 {
                    for c in 0..3 {
                        v.push(slot(
                            Inv::Craft,
                            c + r * 3,
                            SlotKind::Plain,
                            30 + c as i32 * 18,
                            17 + r as i32 * 18,
                        ));
                    }
                }
                player_slots(&mut v, 84);
            }
            Window::Furnace => {
                v.push(slot(Inv::Tile, 0, SlotKind::Plain, 56, 17));
                v.push(slot(Inv::Tile, 1, SlotKind::Plain, 56, 53));
                v.push(slot(Inv::Tile, 2, SlotKind::FurnaceOut, 116, 35));
                player_slots(&mut v, 84);
            }
            Window::Brewing => {
                v.push(slot(Inv::Tile, 0, SlotKind::Potion, 56, 46));
                v.push(slot(Inv::Tile, 1, SlotKind::Potion, 79, 53));
                v.push(slot(Inv::Tile, 2, SlotKind::Potion, 102, 46));
                v.push(slot(Inv::Tile, 3, SlotKind::Ingredient, 79, 17));
                player_slots(&mut v, 84);
            }
            Window::Chest(rows) => {
                for r in 0..rows {
                    for c in 0..9 {
                        v.push(slot(
                            Inv::Tile,
                            c + r * 9,
                            SlotKind::Plain,
                            8 + c as i32 * 18,
                            18 + r as i32 * 18,
                        ));
                    }
                }
                let off = (rows as i32 - 4) * 18;
                player_slots(&mut v, 103 + off);
            }
            Window::Creative => {
                for r in 0..5 {
                    for c in 0..9 {
                        v.push(slot(
                            Inv::Tile,
                            r * 9 + c,
                            SlotKind::Plain,
                            9 + c as i32 * 18,
                            18 + r as i32 * 18,
                        ));
                    }
                }
                for c in 0..9 {
                    v.push(slot(
                        Inv::Player,
                        c,
                        SlotKind::Plain,
                        9 + c as i32 * 18,
                        112,
                    ));
                }
            }
            Window::Dispenser => {
                for r in 0..3 {
                    for c in 0..3 {
                        v.push(slot(
                            Inv::Tile,
                            c + r * 3,
                            SlotKind::Plain,
                            62 + c as i32 * 18,
                            17 + r as i32 * 18,
                        ));
                    }
                }
                player_slots(&mut v, 84);
            }
            Window::Enchant => {
                v.push(slot(Inv::Tile, 0, SlotKind::Enchant, 25, 47));
                player_slots(&mut v, 84);
            }
        }
        v
    }

    /// `InventoryCrafting` width, 0 without a crafting grid.
    pub fn grid_width(self) -> usize {
        match self {
            Window::Player => 2,
            Window::Workbench => 3,
            _ => 0,
        }
    }
}

/// `ItemArmor.armorType` (0 helmet .. 3 boots) of the armour items.
pub fn armor_type(id: item::ItemId) -> Option<usize> {
    if (298..=317).contains(&id) {
        Some(((id - 298) % 4) as usize)
    } else {
        None
    }
}

/// `ItemStack.isItemStackDamageable`
fn damageable(s: &Stack) -> bool {
    item::max_damage(s.id) > 0
}

/// `ItemStack.isStackable`
pub fn stackable(s: &Stack) -> bool {
    max_stack(s.id) > 1 && (!damageable(s) || s.meta <= 0)
}

/// `ItemStack.isItemEqual`: item and damage.
fn item_equal(a: &Stack, b: &Stack) -> bool {
    a.id == b.id && a.meta == b.meta
}

/// `ItemStack.areItemStackTagsEqual` (enchantments are the only tag).
fn tags_equal(a: &Stack, b: &Stack) -> bool {
    a.ench == b.ench
}

/// `ContainerBrewingStand$Potion.canHoldPotion`
pub fn can_hold_potion(s: &Stack) -> bool {
    s.id == 373 || s.id == 374
}

/// What a click did besides moving stacks.
#[derive(Default, Debug)]
pub struct Effects {
    /// `dropPlayerItemWithRandomChoice(stack, flag)`
    pub drops: Vec<(Stack, bool)>,
    /// `SlotFurnace.onCrafting`: the output taken and `removeCount`.
    pub smelted: Vec<(Stack, u32)>,
    /// `SlotCrafting.onCrafting`: the result taken and `amountCrafted`.
    pub crafted: Vec<(Stack, u32)>,
}

/// The drag-splitting state of a `Container`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Drag {
    mode: i32,
    event: i32,
    /// Container slot numbers in the order they were added.
    slots: Vec<usize>,
}

/// One `slotClick` call's view of the inventories behind the window.
pub struct Ctx<'a> {
    pub inv: &'a mut Inventory,
    pub tile: &'a mut [Option<Stack>],
    pub window: Window,
    pub creative: bool,
    pub fx: Effects,
    slots: Vec<SlotDef>,
    result: Option<Stack>,
    amount_crafted: u32,
    remove_count: u32,
}

impl<'a> Ctx<'a> {
    pub fn new(
        inv: &'a mut Inventory,
        tile: &'a mut [Option<Stack>],
        window: Window,
        creative: bool,
    ) -> Ctx<'a> {
        let mut c = Ctx {
            inv,
            tile,
            window,
            creative,
            fx: Effects::default(),
            slots: window.slots(),
            result: None,
            amount_crafted: 0,
            remove_count: 0,
        };
        c.inv.grid_size = window.grid_width().max(2);
        c.craft_changed();
        c
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// `onCraftMatrixChanged`
    fn craft_changed(&mut self) {
        let w = self.window.grid_width();
        if w > 0 {
            self.result = crafting::find_matching(&Grid {
                cells: &self.inv.grid[..w * w],
                width: w,
            });
        }
    }

    pub fn result(&self) -> Option<Stack> {
        self.result
    }

    /// `Slot.getStack`
    pub fn get(&self, i: usize) -> Option<Stack> {
        let d = self.slots[i];
        match d.inv {
            Inv::Player => {
                if d.index < 36 {
                    self.inv.main[d.index]
                } else {
                    self.inv.armor[d.index - 36]
                }
            }
            Inv::Craft => self.inv.grid[d.index],
            Inv::Result => self.result,
            Inv::Tile => self.tile.get(d.index).copied().flatten(),
        }
        .filter(|_| true)
    }

    fn has(&self, i: usize) -> bool {
        self.get(i).is_some()
    }

    /// `IInventory.setInventorySlotContents`
    fn set(&mut self, i: usize, s: Option<Stack>) {
        let d = self.slots[i];
        match d.inv {
            Inv::Player => {
                if d.index < 36 {
                    self.inv.main[d.index] = s;
                } else {
                    self.inv.armor[d.index - 36] = s;
                }
            }
            Inv::Craft => {
                self.inv.grid[d.index] = s;
                self.craft_changed();
            }
            Inv::Result => self.result = s,
            Inv::Tile => {
                if let Some(t) = self.tile.get_mut(d.index) {
                    *t = s;
                }
            }
        }
    }

    /// `IInventory.setInventorySlotContents` from outside the container.
    pub fn set_slot(&mut self, i: usize, s: Option<Stack>) {
        self.set(i, s);
    }

    /// `Slot.putStack`
    fn put(&mut self, i: usize, s: Option<Stack>) {
        self.set(i, s);
    }

    /// `Slot.decrStackSize` (`IInventory.decrStackSize`)
    fn decr(&mut self, i: usize, n: u32) -> Option<Stack> {
        let d = self.slots[i];
        let cur = self.get(i)?;
        match d.kind {
            SlotKind::CraftResult => self.amount_crafted += n.min(cur.count),
            SlotKind::FurnaceOut => self.remove_count += n.min(cur.count),
            _ => {}
        }
        // InventoryCraftResult and TileEntityBrewingStand hand out the
        // whole stack whatever is asked for.
        let whole = d.inv == Inv::Result || self.window == Window::Brewing && d.inv == Inv::Tile;
        if whole || cur.count <= n {
            self.set(i, None);
            return Some(cur);
        }
        let rest = Stack {
            count: cur.count - n,
            ..cur
        };
        self.set(i, if rest.count == 0 { None } else { Some(rest) });
        Some(Stack { count: n, ..cur })
    }

    /// `Slot.isItemValid`
    fn valid(&self, i: usize, s: &Stack) -> bool {
        match self.slots[i].kind {
            SlotKind::Plain | SlotKind::Enchant => true,
            SlotKind::CraftResult | SlotKind::FurnaceOut => false,
            SlotKind::Armor(t) => match armor_type(s.id) {
                Some(a) => a == t,
                // pumpkins and skulls go on the head
                None => (s.id == 86 || s.id == 397) && t == 0,
            },
            SlotKind::Potion => can_hold_potion(s),
            SlotKind::Ingredient => crate::play::is_potion_ingredient(s),
        }
    }

    /// `Slot.getSlotStackLimit`
    fn limit(&self, i: usize) -> u32 {
        match self.slots[i].kind {
            SlotKind::Armor(_) | SlotKind::Potion | SlotKind::Enchant => 1,
            _ => 64,
        }
    }

    /// `Container.canDragIntoSlot` (`ContainerCreative` limits it).
    fn can_drag_into(&self, i: usize) -> bool {
        let d = self.slots[i];
        self.window != Window::Creative || d.inv == Inv::Player || d.y > 90 && d.x <= 162
    }

    fn in_player(&self, i: usize) -> bool {
        self.slots[i].inv == Inv::Player
    }

    /// `Slot.onCrafting(stack)` of the result slots.
    fn on_crafting(&mut self, i: usize, s: &Stack) {
        match self.slots[i].kind {
            SlotKind::CraftResult => {
                self.fx.crafted.push((*s, self.amount_crafted));
                self.amount_crafted = 0;
            }
            SlotKind::FurnaceOut => {
                self.fx.smelted.push((*s, self.remove_count));
                self.remove_count = 0;
            }
            _ => {}
        }
    }

    /// `Slot.onSlotChange(after, before)`
    fn on_slot_change(&mut self, i: usize, after: &Stack, before: &Stack) {
        if after.id == before.id {
            let n = before.count as i64 - after.count as i64;
            if n > 0 {
                match self.slots[i].kind {
                    SlotKind::CraftResult => self.amount_crafted += n as u32,
                    SlotKind::FurnaceOut => self.remove_count += n as u32,
                    _ => {}
                }
                self.on_crafting(i, after);
            }
        }
    }

    /// `Slot.onPickupFromSlot`
    fn on_pickup(&mut self, i: usize, s: &Stack) {
        match self.slots[i].kind {
            SlotKind::CraftResult => {
                self.on_crafting(i, s);
                let w = self.window.grid_width();
                for k in 0..w * w {
                    let Some(g) = self.inv.grid[k] else { continue };
                    // InventoryCrafting.decrStackSize(k, 1)
                    self.inv.grid[k] = if g.count <= 1 {
                        None
                    } else {
                        Some(Stack {
                            count: g.count - 1,
                            ..g
                        })
                    };
                    self.craft_changed();
                    if let Some(c) = crafting::container_item(g.id) {
                        let left = Stack::new(c, 0, 1);
                        // doesContainerItemLeaveCraftingGrid is always true
                        if !self.add_to_inventory(left) {
                            if self.inv.grid[k].is_none() {
                                self.inv.grid[k] = Some(left);
                                self.craft_changed();
                            } else {
                                self.fx.drops.push((left, false));
                            }
                        }
                    }
                }
            }
            SlotKind::FurnaceOut => self.on_crafting(i, s),
            _ => {}
        }
    }

    /// `InventoryPlayer.addItemStackToInventory`; true when anything went in.
    pub fn add_to_inventory(&mut self, s: Stack) -> bool {
        let mut s = s;
        let r = self.inv.add_stack(&mut s, self.creative);
        r
    }

    /// `Container.canAddItemToSlot`
    fn can_add(&self, i: Option<usize>, s: Option<&Stack>, ignore_size: bool) -> bool {
        let cur = i.and_then(|i| self.get(i));
        let mut ok = cur.is_none();
        if let (Some(c), Some(s)) = (cur, s) {
            if item_equal(s, &c) && tags_equal(&c, s) {
                let add = if ignore_size { 0 } else { s.count };
                ok |= c.count + add <= max_stack(s.id);
            }
        }
        ok
    }

    /// `Container.mergeItemStack` over `[a, b)`.
    fn merge(&mut self, s: &mut Stack, a: usize, b: usize, reverse: bool) -> bool {
        let mut moved = false;
        let mut k = if reverse { b as isize - 1 } else { a as isize };
        let in_range = |k: isize| {
            if reverse {
                k >= a as isize
            } else {
                k < b as isize
            }
        };
        if stackable(s) {
            while s.count > 0 && in_range(k) {
                let i = k as usize;
                if let Some(t) = self.get(i) {
                    let max = max_stack(s.id);
                    if t.id == s.id
                        && (!item::has_subtypes(s.id) || s.meta == t.meta)
                        && tags_equal(s, &t)
                    {
                        let sum = t.count + s.count;
                        if sum <= max {
                            s.count = 0;
                            self.set(i, Some(Stack { count: sum, ..t }));
                            moved = true;
                        } else if t.count < max {
                            s.count -= max - t.count;
                            self.set(i, Some(Stack { count: max, ..t }));
                            moved = true;
                        }
                    }
                }
                k += if reverse { -1 } else { 1 };
            }
        }
        if s.count > 0 {
            k = if reverse { b as isize - 1 } else { a as isize };
            while in_range(k) {
                let i = k as usize;
                if self.get(i).is_none() {
                    self.put(i, Some(*s));
                    s.count = 0;
                    moved = true;
                    break;
                }
                k += if reverse { -1 } else { 1 };
            }
        }
        moved
    }

    /// `transferStackInSlot` of the window's container.
    fn transfer(&mut self, i: usize) -> Option<Stack> {
        let mut s = self.get(i)?;
        let before = s;
        let n = self.len();
        let result_slot = matches!(
            self.slots[i].kind,
            SlotKind::CraftResult | SlotKind::FurnaceOut
        );
        let ok = match self.window {
            Window::Player => {
                let arm = armor_type(before.id).map(|t| 5 + t);
                if i == 0 {
                    self.merge(&mut s, 9, 45, true)
                } else if (1..9).contains(&i) {
                    self.merge(&mut s, 9, 45, false)
                } else if let Some(a) = arm.filter(|&a| !self.has(a)) {
                    self.merge(&mut s, a, a + 1, false)
                } else if (9..36).contains(&i) {
                    self.merge(&mut s, 36, 45, false)
                } else if (36..45).contains(&i) {
                    self.merge(&mut s, 9, 36, false)
                } else {
                    self.merge(&mut s, 9, 45, false)
                }
            }
            Window::Workbench => {
                if i == 0 {
                    self.merge(&mut s, 10, 46, true)
                } else if (10..37).contains(&i) {
                    self.merge(&mut s, 37, 46, false)
                } else if (37..46).contains(&i) {
                    self.merge(&mut s, 10, 37, false)
                } else {
                    self.merge(&mut s, 10, 46, false)
                }
            }
            Window::Furnace => {
                if i == 2 {
                    self.merge(&mut s, 3, 39, true)
                } else if i != 1 && i != 0 {
                    if crate::play::smelt(&s).is_some() {
                        self.merge(&mut s, 0, 1, false)
                    } else if crate::play::burn_time(&s) > 0 {
                        self.merge(&mut s, 1, 2, false)
                    } else if (3..30).contains(&i) {
                        self.merge(&mut s, 30, 39, false)
                    } else if (30..39).contains(&i) {
                        self.merge(&mut s, 3, 30, false)
                    } else {
                        true
                    }
                } else {
                    self.merge(&mut s, 3, 39, false)
                }
            }
            Window::Brewing => {
                if i > 3 {
                    if !self.has(3) && self.valid(3, &s) {
                        self.merge(&mut s, 3, 4, false)
                    } else if can_hold_potion(&before) {
                        self.merge(&mut s, 0, 3, false)
                    } else if (4..31).contains(&i) {
                        self.merge(&mut s, 31, 40, false)
                    } else if (31..40).contains(&i) {
                        self.merge(&mut s, 4, 31, false)
                    } else {
                        self.merge(&mut s, 4, 40, false)
                    }
                } else {
                    self.merge(&mut s, 4, 40, true)
                }
            }
            Window::Chest(rows) => {
                let ok = if i < rows * 9 {
                    self.merge(&mut s, rows * 9, n, true)
                } else {
                    self.merge(&mut s, 0, rows * 9, false)
                };
                if !ok {
                    return None;
                }
                // ContainerChest stops here: no size check, no pickup.
                self.slot_after_transfer(i, s);
                return Some(before);
            }
            Window::Creative => {
                // the hotbar slots are emptied, nothing moves
                if i >= n - 9 {
                    self.put(i, None);
                }
                return None;
            }
            Window::Dispenser => {
                if i < 9 {
                    self.merge(&mut s, 9, 45, true)
                } else {
                    self.merge(&mut s, 0, 9, false)
                }
            }
            Window::Enchant => {
                if i == 0 {
                    self.merge(&mut s, 1, 37, true)
                } else {
                    if self.has(0) || !self.valid(0, &s) {
                        return None;
                    }
                    if s.ench != 0 && s.count == 1 {
                        self.put(0, Some(s));
                        s.count = 0;
                    } else if s.count >= 1 {
                        // new ItemStack(item, 1, damage): without the tag
                        self.put(0, Some(Stack::new(s.id, s.meta, 1)));
                        s.count -= 1;
                    }
                    true
                }
            }
        };
        if !ok {
            return None;
        }
        // the remainder is the slot's (mutated) stack
        let first_branch = match self.window {
            Window::Player | Window::Workbench => i == 0,
            Window::Furnace => i == 2,
            Window::Brewing => i <= 3,
            _ => false,
        };
        if first_branch && (result_slot || self.window == Window::Brewing) {
            self.on_slot_change(i, &s, &before);
        }
        self.slot_after_transfer(i, s);
        if s.count == before.count {
            return None;
        }
        self.on_pickup(i, &s);
        Some(before)
    }

    /// `if (stackSize == 0) putStack(null) else onSlotChanged()` with the
    /// slot holding what `mergeItemStack` left.
    fn slot_after_transfer(&mut self, i: usize, s: Stack) {
        if s.count == 0 {
            self.put(i, None);
        } else if self.get(i).map(|c| c.count) != Some(s.count) {
            // the merge mutated the slot's own stack
            if self.slots[i].inv == Inv::Result {
                self.result = Some(s);
            } else {
                self.set(i, Some(s));
            }
        }
    }

    fn drop(&mut self, s: Stack, flag: bool) {
        self.fx.drops.push((s, flag));
    }
}

/// `Container.extractDragMode` and friends.
pub fn quickcraft_mask(event: i32, mode: i32) -> i32 {
    event & 3 | (mode & 3) << 2
}

impl Drag {
    pub fn reset(&mut self) {
        self.event = 0;
        self.slots.clear();
    }
}

/// `Container.computeStackSize`
pub fn compute_stack_size(n_slots: usize, mode: i32, s: &mut Stack, existing: u32) {
    match mode {
        0 => s.count = (s.count as f32 / n_slots as f32).floor() as u32,
        1 => s.count = 1,
        _ => {}
    }
    s.count += existing;
}

/// `Container.slotClick`. `slot` is the container slot number or -999 for
/// outside the window; returns the stack copy vanilla returns (used by the
/// client for `C0EPacketClickWindow`).
pub fn slot_click(
    c: &mut Ctx,
    drag: &mut Drag,
    slot: i32,
    button: i32,
    mode: i32,
) -> Option<Stack> {
    let mut ret = None;
    let n = c.len() as i32;
    let valid_slot = |s: i32| s >= 0 && s < n;
    if mode == 5 {
        let last = drag.event;
        drag.event = button & 3;
        if (last != 1 || drag.event != 2) && last != drag.event {
            drag.reset();
        } else if c.inv.cursor.is_none() {
            drag.reset();
        } else if drag.event == 0 {
            drag.mode = button >> 2 & 3;
            if drag.mode == 0 || drag.mode == 1 {
                drag.event = 1;
                drag.slots.clear();
            } else {
                drag.reset();
            }
        } else if drag.event == 1 {
            if valid_slot(slot) {
                let i = slot as usize;
                let cur = c.inv.cursor.unwrap();
                if c.can_add(Some(i), Some(&cur), true)
                    && c.valid(i, &cur)
                    && c.can_drag_into(i)
                    && cur.count as usize > drag.slots.len()
                    && !drag.slots.contains(&i)
                {
                    drag.slots.push(i);
                }
            }
        } else if drag.event == 2 {
            if !drag.slots.is_empty() {
                let cur = c.inv.cursor.unwrap();
                let mut left = cur.count as i64;
                let k = drag.slots.len();
                for &i in &drag.slots.clone() {
                    let cc = c.inv.cursor.unwrap();
                    if c.can_add(Some(i), Some(&cc), true)
                        && c.valid(i, &cc)
                        && c.can_drag_into(i)
                        && cc.count as usize >= k
                    {
                        let mut put = cur;
                        let have = c.get(i).map_or(0, |s| s.count);
                        compute_stack_size(k, drag.mode, &mut put, have);
                        put.count = put.count.min(max_stack(put.id)).min(c.limit(i));
                        left -= put.count as i64 - have as i64;
                        c.put(i, Some(put));
                    }
                }
                c.inv.cursor = if left <= 0 {
                    None
                } else {
                    Some(Stack {
                        count: left as u32,
                        ..cur
                    })
                };
            }
            drag.reset();
        } else {
            drag.reset();
        }
    } else if drag.event != 0 {
        drag.reset();
    } else if (mode == 0 || mode == 1) && (button == 0 || button == 1) {
        if slot == -999 {
            if let Some(cur) = c.inv.cursor {
                if button == 0 {
                    c.drop(cur, true);
                    c.inv.cursor = None;
                }
                if button == 1 {
                    c.drop(Stack { count: 1, ..cur }, true);
                    c.inv.cursor = if cur.count <= 1 {
                        None
                    } else {
                        Some(Stack {
                            count: cur.count - 1,
                            ..cur
                        })
                    };
                }
            }
        } else if mode == 1 {
            if !valid_slot(slot) {
                return None;
            }
            let i = slot as usize;
            if let Some(t) = c.transfer(i) {
                ret = Some(t);
                if c.get(i).is_some_and(|s| s.id == t.id) {
                    // retrySlotClick
                    slot_click(c, drag, slot, button, 1);
                }
            }
        } else {
            if !valid_slot(slot) {
                return None;
            }
            let i = slot as usize;
            let here = c.get(i);
            let cursor = c.inv.cursor;
            ret = here;
            match (here, cursor) {
                (None, Some(cur)) => {
                    if c.valid(i, &cur) {
                        let k = if button == 0 { cur.count } else { 1 }.min(c.limit(i));
                        if cur.count >= k {
                            c.put(i, Some(Stack { count: k, ..cur }));
                            let rest = cur.count - k;
                            c.inv.cursor = if rest == 0 {
                                None
                            } else {
                                Some(Stack { count: rest, ..cur })
                            };
                        }
                    }
                }
                (None, None) => {}
                (Some(h), None) => {
                    let k = if button == 0 {
                        h.count
                    } else {
                        h.count.div_ceil(2)
                    };
                    let took = c.decr(i, k);
                    c.inv.cursor = took;
                    if let Some(t) = took {
                        c.on_pickup(i, &t);
                    }
                }
                (Some(h), Some(cur)) => {
                    if c.valid(i, &cur) {
                        if h.id == cur.id && h.meta == cur.meta && tags_equal(&h, &cur) {
                            let mut k = if button == 0 { cur.count } else { 1 } as i64;
                            k = k.min(c.limit(i) as i64 - h.count as i64);
                            k = k.min(max_stack(cur.id) as i64 - h.count as i64);
                            // splitStack with a negative size grows the cursor
                            let rest = cur.count as i64 - k;
                            c.inv.cursor = if rest == 0 {
                                None
                            } else {
                                Some(Stack {
                                    count: rest as u32,
                                    ..cur
                                })
                            };
                            c.set(
                                i,
                                Some(Stack {
                                    count: (h.count as i64 + k) as u32,
                                    ..h
                                }),
                            );
                        } else if cur.count <= c.limit(i) {
                            c.put(i, Some(cur));
                            c.inv.cursor = Some(h);
                        }
                    } else if h.id == cur.id
                        && max_stack(cur.id) > 1
                        && (!item::has_subtypes(h.id) || h.meta == cur.meta)
                        && tags_equal(&h, &cur)
                    {
                        let k = h.count;
                        if k > 0 && k + cur.count <= max_stack(cur.id) {
                            let mut nc = cur;
                            nc.count += k;
                            c.inv.cursor = Some(nc);
                            c.decr(i, k);
                            c.on_pickup(i, &nc);
                        }
                    }
                }
            }
        }
    } else if mode == 2 && (0..9).contains(&button) {
        if !valid_slot(slot) {
            return None;
        }
        let i = slot as usize;
        let hb = button as usize;
        let hot = c.inv.main[hb];
        let in_player = c.in_player(i);
        let mut ok = hot.is_none() || in_player && c.valid(i, &hot.unwrap());
        let mut empty = -1;
        if !ok {
            empty = c.inv.first_empty();
            ok |= empty > -1;
        }
        if let (Some(here), true) = (c.get(i), ok) {
            c.inv.main[hb] = Some(here);
            if (!in_player || !hot.is_some_and(|h| c.valid(i, &h))) && hot.is_some() {
                if empty > -1 {
                    c.add_to_inventory(hot.unwrap());
                    c.decr(i, here.count);
                    c.put(i, None);
                    c.on_pickup(i, &here);
                }
            } else {
                c.decr(i, here.count);
                c.put(i, hot);
                c.on_pickup(i, &here);
            }
        } else if let (None, Some(h)) = (c.get(i), hot) {
            if c.valid(i, &h) {
                c.inv.main[hb] = None;
                c.put(i, Some(h));
            }
        }
    } else if mode == 3 && c.creative && c.inv.cursor.is_none() && slot >= 0 {
        if let Some(s) = valid_slot(slot).then(|| c.get(slot as usize)).flatten() {
            c.inv.cursor = Some(Stack {
                count: max_stack(s.id),
                ..s
            });
        }
    } else if mode == 4 && c.inv.cursor.is_none() && slot >= 0 {
        if valid_slot(slot) {
            let i = slot as usize;
            if let Some(s) = c.get(i) {
                let k = if button == 0 { 1 } else { s.count };
                if let Some(t) = c.decr(i, k) {
                    c.on_pickup(i, &t);
                    c.drop(t, true);
                }
            }
        }
    } else if mode == 6 && slot >= 0 {
        let here = valid_slot(slot).then(|| c.get(slot as usize)).flatten();
        if let (Some(mut cur), true) = (c.inv.cursor, here.is_none()) {
            let (start, step) = if button == 0 { (0, 1) } else { (n - 1, -1) };
            let max = max_stack(cur.id);
            for pass in 0..2 {
                let mut k = start;
                while k >= 0 && k < n && cur.count < max {
                    let i = k as usize;
                    if let Some(s) = c.get(i) {
                        // canMergeSlot
                        let mergeable = if c.window == Window::Creative {
                            c.slots[i].y > 90
                        } else {
                            !matches!(c.slots[i].inv, Inv::Result)
                        };
                        if c.can_add(Some(i), Some(&cur), true)
                            && mergeable
                            && (pass != 0 || s.count != max_stack(s.id))
                        {
                            let m = (max - cur.count).min(s.count);
                            let took = c.decr(i, m);
                            cur.count += m;
                            c.inv.cursor = Some(cur);
                            if let Some(t) = took {
                                c.on_pickup(i, &t);
                            }
                        }
                    }
                    k += step;
                }
            }
            c.inv.cursor = Some(cur);
        }
    }
    ret
}

/// `Container.onContainerClosed` and the crafting containers' overrides:
/// the cursor and the crafting grid are thrown out.
pub fn close(inv: &mut Inventory, window: Window) -> Vec<(Stack, bool)> {
    let mut out = Vec::new();
    if let Some(c) = inv.cursor.take() {
        out.push((c, false));
    }
    let w = window.grid_width();
    for k in 0..w * w {
        if let Some(s) = inv.grid[k].take() {
            out.push((s, false));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(id: u16, n: u32) -> Option<Stack> {
        Some(Stack::new(id, 0, n))
    }

    fn click(
        inv: &mut Inventory,
        tile: &mut [Option<Stack>],
        w: Window,
        s: i32,
        b: i32,
        m: i32,
    ) -> Effects {
        let mut d = Drag::default();
        let mut c = Ctx::new(inv, tile, w, false);
        slot_click(&mut c, &mut d, s, b, m);
        c.fx
    }

    #[test]
    fn pick_split_and_place() {
        let mut inv = Inventory::default();
        inv.main[0] = st(4, 33);
        // slot 36 is hotbar 0 in the player window
        click(&mut inv, &mut [], Window::Player, 36, 1, 0);
        assert_eq!(inv.cursor, st(4, 17));
        assert_eq!(inv.main[0], st(4, 16));
        click(&mut inv, &mut [], Window::Player, 9, 1, 0);
        assert_eq!(inv.main[9], st(4, 1));
        click(&mut inv, &mut [], Window::Player, 9, 0, 0);
        assert_eq!(inv.main[9], st(4, 17));
        assert_eq!(inv.cursor, None);
        // dropping outside
        inv.cursor = st(4, 5);
        let fx = click(&mut inv, &mut [], Window::Player, -999, 1, 0);
        assert_eq!(fx.drops, vec![(Stack::new(4, 0, 1), true)]);
        assert_eq!(inv.cursor, st(4, 4));
    }

    #[test]
    fn crafting_takes_and_shift_crafts() {
        let mut inv = Inventory::default();
        inv.grid[0] = st(17, 3);
        // the result slot offers four planks
        click(&mut inv, &mut [], Window::Player, 0, 0, 0);
        assert_eq!(inv.cursor, st(5, 4));
        assert_eq!(inv.grid[0], st(17, 2));
        // shift-click crafts the rest straight into the inventory
        click(&mut inv, &mut [], Window::Player, 0, 0, 1);
        assert_eq!(inv.grid[0], None);
        assert_eq!(inv.main[8], st(5, 8));
    }

    #[test]
    fn armor_shift_click_and_slot_rules() {
        let mut inv = Inventory::default();
        inv.main[3] = Some(Stack::new(306, 0, 1));
        click(&mut inv, &mut [], Window::Player, 39, 0, 1);
        assert_eq!(inv.armor[3], Some(Stack::new(306, 0, 1)));
        inv.main[0] = Some(Stack::new(307, 0, 1));
        click(&mut inv, &mut [], Window::Player, 36, 0, 1);
        assert_eq!(inv.armor[2], Some(Stack::new(307, 0, 1)));
        assert_eq!(inv.main[0], None);
        // a block cannot be put into the boots slot
        inv.cursor = st(1, 1);
        click(&mut inv, &mut [], Window::Player, 8, 0, 0);
        assert_eq!(inv.armor[0], None);
        assert_eq!(inv.cursor, st(1, 1));
    }

    #[test]
    fn drag_split_evenly() {
        let mut inv = Inventory::default();
        inv.cursor = st(4, 10);
        let mut d = Drag::default();
        let mut tile: [Option<Stack>; 0] = [];
        let mut c = Ctx::new(&mut inv, &mut tile, Window::Player, false);
        slot_click(&mut c, &mut d, -999, quickcraft_mask(0, 0), 5);
        for s in [9, 10, 11] {
            slot_click(&mut c, &mut d, s, quickcraft_mask(1, 0), 5);
        }
        slot_click(&mut c, &mut d, -999, quickcraft_mask(2, 0), 5);
        assert_eq!(inv.main[9], st(4, 3));
        assert_eq!(inv.main[11], st(4, 3));
        assert_eq!(inv.cursor, st(4, 1));
    }

    #[test]
    fn double_click_collects() {
        let mut inv = Inventory::default();
        inv.main[9] = st(4, 10);
        inv.main[20] = st(4, 64);
        inv.cursor = st(4, 5);
        click(&mut inv, &mut [], Window::Player, 12, 0, 6);
        // partial stacks first, then full ones
        assert_eq!(inv.cursor, st(4, 64));
        assert_eq!(inv.main[9], None);
        assert_eq!(inv.main[20], st(4, 15));
    }

    #[test]
    fn chest_transfer_and_hotbar_swap() {
        let mut inv = Inventory::default();
        let mut tile = [None; 27];
        tile[0] = st(4, 20);
        click(&mut inv, &mut tile, Window::Chest(3), 0, 0, 1);
        // into the hotbar end first (reverse merge)
        assert_eq!(inv.main[8], st(4, 20));
        assert_eq!(tile[0], None);
        // key 2 over chest slot 5 swaps with hotbar 1
        tile[5] = st(1, 3);
        click(&mut inv, &mut tile, Window::Chest(3), 5, 1, 2);
        assert_eq!(inv.main[1], st(1, 3));
        assert_eq!(tile[5], None);
    }
}
