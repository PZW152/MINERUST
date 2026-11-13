//! Server-side gameplay state: the player's inventory, dropped items,
//! furnaces and chests, and what happens when blocks are dug or used.

use crate::block;
use crate::container::{self, Ctx, Drag, Window};
use crate::entity::item::ItemEntity;
use crate::entity::player::Aabb;
use crate::entity::survival::GameMode;
use crate::inventory::Inventory;
use crate::item::{self, max_stack, ItemId, Stack};
use crate::java::JavaRandom;
use crate::world::{BlockView, ServerWorld};
use std::collections::BTreeMap;

pub type Pos = [i32; 3];

#[derive(Clone, Debug, PartialEq)]
pub enum Tile {
    /// `TileEntityFurnace`: input, fuel, output.
    Furnace {
        slots: [Option<Stack>; 3],
        burn: i32,
        item_burn: i32,
        cook: i32,
    },
    /// `TileEntityChest`
    Chest { slots: [Option<Stack>; 27] },
    /// `TileEntityDispenser`
    Dispenser { slots: [Option<Stack>; 9] },
    /// `TileEntityMobSpawner` (`MobSpawnerBaseLogic.mobID`)
    Spawner { mob: String },
    /// `TileEntitySign.signText`; `editable` until the placer's update.
    Sign { text: [String; 4], editable: bool },
    /// `TileEntityBrewingStand`: three bottles and the ingredient.
    Brewing {
        slots: [Option<Stack>; 4],
        brew_time: i32,
        /// `filledSlots` (the block's metadata)
        filled: i32,
        /// `ingredientID`
        ingredient: u16,
    },
}

/// `Item.isPotionIngredient` (`getPotionEffect` is set), with nether wart.
pub fn is_potion_ingredient(s: &Stack) -> bool {
    s.id == 372 || crate::potion::ingredient_effect(s.id, s.meta).is_some()
}

/// `TileEntityBrewingStand.func_145936_c` (getPotionResult)
fn potion_result(meta: i32, ing: Option<Stack>) -> i32 {
    match ing {
        None => meta,
        Some(i) if is_potion_ingredient(&i) => {
            let e = if i.id == 372 {
                "+4"
            } else {
                crate::potion::ingredient_effect(i.id, i.meta).unwrap()
            };
            crate::potion::apply_ingredient(meta, e)
        }
        _ => meta,
    }
}

/// `ItemPotion.getEffects(meta)` compared the way the brewing code does:
/// the cached lists are the same object only for the same metadata.
fn brew_changes(j: i32, k: i32) -> bool {
    let a = crate::potion::effects_of(j, false);
    let b = crate::potion::effects_of(k, false);
    let (la, lb) = (
        if a.is_empty() { None } else { Some(&a) },
        if b.is_empty() { None } else { Some(&b) },
    );
    let differ = j != k && (la.is_some() || lb.is_some());
    (j <= 0 || differ) && (la.is_none() || (la != lb && lb.is_some()))
}

/// `TileEntityBrewingStand.canBrew`
fn can_brew(slots: &[Option<Stack>; 4]) -> bool {
    let Some(ing) = slots[3] else { return false };
    if ing.count == 0 || !is_potion_ingredient(&ing) {
        return false;
    }
    for s in slots[..3].iter().flatten() {
        if s.id == 373 {
            let j = s.meta;
            let k = potion_result(j, Some(ing));
            if !crate::item_use::is_splash(j) && crate::item_use::is_splash(k) {
                return true;
            }
            if brew_changes(j, k) && j != k {
                return true;
            }
        }
    }
    false
}

/// `TileEntityBrewingStand.brewPotions`
fn brew_potions(slots: &mut [Option<Stack>; 4]) {
    if !can_brew(slots) {
        return;
    }
    let ing = slots[3];
    for s in slots[..3].iter_mut().flatten() {
        if s.id == 373 {
            let j = s.meta;
            let k = potion_result(j, ing);
            if brew_changes(j, k) {
                if j != k {
                    s.meta = k;
                }
            } else if !crate::item_use::is_splash(j) && crate::item_use::is_splash(k) {
                s.meta = k;
            }
        }
    }
    let ing = slots[3].unwrap();
    // Item.hasContainerItem: none of the ingredients has one
    slots[3] = if ing.count > 1 {
        Some(Stack {
            count: ing.count - 1,
            ..ing
        })
    } else {
        None
    };
}

/// The window the player has open.
#[derive(Clone, Debug, PartialEq)]
pub enum Open {
    None,
    Inventory,
    Workbench(Pos),
    Furnace(Pos),
    Brewing(Pos),
    /// One or two chest halves; the first is the top half of the window.
    Chest(Vec<Pos>),
    /// An `EntityMinecartChest` (`displayGUIChest`).
    Cart(u32),
    /// A dispenser or dropper (`TileEntityDispenser`).
    Dispenser(Pos),
    /// `ContainerEnchantment` at the table
    Enchant(Pos),
    /// `ContainerRepair` at an anvil
    Anvil(Pos),
}

/// The `Container` class an open window uses.
pub fn window_of(o: &Open) -> Window {
    match o {
        Open::None | Open::Inventory => Window::Player,
        Open::Workbench(_) => Window::Workbench,
        Open::Furnace(_) => Window::Furnace,
        Open::Brewing(_) => Window::Brewing,
        Open::Chest(ps) => Window::Chest(ps.len() * 3),
        Open::Cart(_) => Window::Chest(3),
        Open::Dispenser(_) => Window::Dispenser,
        Open::Enchant(_) => Window::Enchant,
        Open::Anvil(_) => Window::Repair,
    }
}

/// What the client needs to draw the inventory and an open window.
#[derive(Clone, Debug, PartialEq)]
pub struct InvView {
    pub inv: Inventory,
    pub open: Open,
    pub result: Option<Stack>,
    /// Container slots (chest or furnace).
    pub tile_slots: Vec<Option<Stack>>,
    /// Furnace progress: `(burn scaled 0..13, cook scaled 0..24)`.
    pub furnace: Option<(i32, i32)>,
    /// How many `WindowClick`s the server had handled: the client drops
    /// views older than its own predicted clicks.
    pub clicks: u32,
    /// `ContainerEnchantment.enchantLevels` and `nameSeed` (`S31` bars).
    pub enchant: [i32; 3],
    pub name_seed: i64,
    /// `ContainerRepair.maximumCost` (`S31` bar 0).
    pub repair_cost: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct ItemView {
    pub id: u32,
    pub stack: Stack,
    pub pos: [f64; 3],
    pub prev: [f64; 3],
    pub age: i32,
    pub hover: f32,
}

pub struct Play {
    pub inv: Inventory,
    pub open: Open,
    pub mode: GameMode,
    pub items: Vec<ItemEntity>,
    /// `EntityFallingBlock`s
    pub falling: Vec<crate::entity::falling::FallingBlock>,
    /// Experience orbs to spawn (`dropXpOnBlockBreak`, `SlotFurnace`).
    pub orbs: Vec<([f64; 3], i32)>,
    /// Boats and minecarts with their trackers.
    pub vehicles: Vec<crate::entity::vehicle::Tracked>,
    pub next_item: u32,
    /// Sounds and particles of the item entities this tick.
    pub fx: Vec<crate::entity::base::Fx>,
    /// Items picked up this tick (`S0DPacketCollectItem`).
    pub collected: Vec<ItemView>,
    /// The player is alive (`getHealth() > 0`).
    pub alive: bool,
    /// `rotationPitch` of the player.
    pub pitch: f32,
    /// `EntityPlayer.rand`
    pub prand: JavaRandom,
    pub tiles: BTreeMap<Pos, Tile>,
    /// Feet position and yaw of the player.
    pub player: ([f64; 3], f32),
    pub inv_dirty: bool,
    /// The open container's drag-splitting state.
    pub drag: Drag,
    /// `WindowClick`s handled so far.
    pub clicks: u32,
    /// `field_147375_m`: the creative-mode drop throttle.
    pub creative_drops: i32,
    /// `ContainerEnchantment`: `tableInventory`, `enchantLevels`, `nameSeed`
    /// and its `rand` (`new Random()`).
    pub enchant_slot: [Option<Stack>; 1],
    pub enchant_levels: [i32; 3],
    pub name_seed: i64,
    pub enchant_rand: JavaRandom,
    /// `ContainerRepair.inputSlots` and `repairedItemName`.
    pub repair_slots: [Option<Stack>; 2],
    pub repair_name: String,
    /// `experienceLevel` of the player, kept here for the containers.
    pub xp_level: i32,
    /// `S36PacketSignEditorOpen` to send, and sign texts that changed.
    pub sign_open: Option<Pos>,
    pub sign_updates: Vec<(Pos, [String; 4])>,
    /// Generated entities (villagers, witches, chest minecarts).
    pub pending: Vec<crate::world::server::GenEntity>,
}

/// `TileEntityFurnace.getItemBurnTime`
pub fn burn_time(s: &Stack) -> i32 {
    if let Some(b) = s.block() {
        if b == block::STONE_SLAB {
            return 0;
        }
        if block::material(b) == block::Material::Wood {
            return 300;
        }
        if b == 6 {
            return 100;
        }
    }
    if let Some(t) = item::tool(s.id) {
        // ItemTool / ItemSword / ItemHoe made of wood.
        if t.efficiency == 2.0 && t.max_uses == 59 {
            return 200;
        }
    }
    match s.id {
        item::STICK => 100,
        item::COAL => 1600,
        _ => 0,
    }
}

/// `FurnaceRecipes.getSmeltingResult`
pub fn smelt(s: &Stack) -> Option<Stack> {
    smelting(s).map(|r| r.0)
}

/// `FurnaceRecipes`: the result and `func_151398_b`'s experience.
pub fn smelting(s: &Stack) -> Option<(Stack, f32)> {
    let b = |id: u8| id as ItemId;
    Some(match s.id {
        x if x == b(block::IRON_ORE) => (Stack::new(item::IRON_INGOT, 0, 1), 0.7),
        x if x == b(block::GOLD_ORE) => (Stack::new(item::GOLD_INGOT, 0, 1), 1.0),
        x if x == b(block::DIAMOND_ORE) => (Stack::new(item::DIAMOND, 0, 1), 1.0),
        x if x == b(block::SAND) => (Stack::new(b(block::GLASS), 0, 1), 0.1),
        319 => (Stack::new(320, 0, 1), 0.35),
        363 => (Stack::new(364, 0, 1), 0.35),
        365 => (Stack::new(366, 0, 1), 0.35),
        x if x == b(block::COBBLESTONE) => (Stack::new(b(block::STONE), 0, 1), 0.1),
        item::CLAY_BALL => (Stack::new(item::BRICK, 0, 1), 0.3),
        x if x == b(block::CLAY) => (Stack::new(b(block::HARDENED_CLAY), 0, 1), 0.35),
        x if x == b(block::CACTUS) => (Stack::new(item::DYE, 2, 1), 0.2),
        x if x == b(block::LOG) || x == b(block::LOG2) => (Stack::new(item::COAL, 1, 1), 0.15),
        x if x == b(block::EMERALD_ORE) => (Stack::new(item::EMERALD, 0, 1), 1.0),
        392 => (Stack::new(393, 0, 1), 0.35),
        87 => (Stack::new(405, 0, 1), 0.1),
        // ItemFishFood.FishType cod and salmon
        349 if s.meta == 0 || s.meta == 1 => (Stack::new(350, s.meta, 1), 0.35),
        x if x == b(block::COAL_ORE) => (Stack::new(item::COAL, 0, 1), 0.1),
        x if x == b(block::REDSTONE_ORE) => (Stack::new(item::REDSTONE, 0, 1), 0.7),
        x if x == b(block::LAPIS_ORE) => (Stack::new(item::DYE, 4, 1), 0.2),
        153 => (Stack::new(406, 0, 1), 0.2),
        _ => return None,
    })
}

/// `FurnaceRecipes.func_151398_b`: the experience of a smelted output.
pub fn smelting_xp(out: &Stack) -> f32 {
    // the first entry of experienceList whose output matches
    let inputs: [(u16, i32); 21] = [
        (15, 0),
        (14, 0),
        (56, 0),
        (12, 0),
        (319, 0),
        (363, 0),
        (365, 0),
        (4, 0),
        (337, 0),
        (82, 0),
        (81, 0),
        (17, 0),
        (129, 0),
        (392, 0),
        (87, 0),
        (349, 0),
        (349, 1),
        (16, 0),
        (73, 0),
        (21, 0),
        (153, 0),
    ];
    for (id, meta) in inputs {
        if let Some((r, xp)) = smelting(&Stack::new(id, meta, 1)) {
            if r.id == out.id && r.meta == out.meta {
                return xp;
            }
        }
    }
    0.0
}

/// `SlotFurnace.onCrafting`: the experience for `n` items taken;
/// `math_random` stands for `Math.random()`.
pub fn furnace_xp(out: &Stack, n: i32, math_random: &mut JavaRandom) -> i32 {
    let f = smelting_xp(out);
    if f == 0.0 {
        return 0;
    }
    if f < 1.0 {
        let v = n as f32 * f;
        let mut k = crate::util::math::floor_float(v);
        if (k as f32) < v.ceil() && (math_random.next_double() as f32) < v - k as f32 {
            k += 1;
        }
        return k;
    }
    n
}

fn merge_slot(slot: &mut Option<Stack>, add: Stack) -> bool {
    match slot {
        None => {
            *slot = Some(add);
            true
        }
        Some(s)
            if s.same_item(&add)
                && s.meta == add.meta
                && s.count + add.count <= max_stack(s.id) =>
        {
            s.count += add.count;
            true
        }
        _ => false,
    }
}

/// `BlockChest` / `BlockFurnace` facing from the placer's yaw.
/// `ItemEnderEye.onItemUse`: with all twelve eyes in place, the 3x3 inside
/// of the ring becomes an End portal.
fn fill_end_portal(w: &mut ServerWorld, x: i32, y: i32, z: i32, dir: i32) {
    const OX: [i32; 4] = [0, -1, 0, 1];
    const OZ: [i32; 4] = [1, 0, -1, 0];
    const ROT_RIGHT: [usize; 4] = [1, 2, 3, 0];
    let d = dir as usize;
    let r = ROT_RIGHT[d];
    let eye = |w: &mut ServerWorld, x: i32, z: i32| {
        w.block(x, y, z) == block::END_PORTAL_FRAME && w.meta(x, y, z) & 4 != 0
    };
    let (mut first, mut last, mut seen, mut ok) = (0, 0, false, true);
    for l in -2..=2 {
        let (px, pz) = (x + OX[r] * l, z + OZ[r] * l);
        if w.block(px, y, pz) == block::END_PORTAL_FRAME {
            if !eye(w, px, pz) {
                ok = false;
                break;
            }
            last = l;
            if !seen {
                first = l;
                seen = true;
            }
        }
    }
    if !ok || last != first + 2 {
        return;
    }
    for l in first..=last {
        let px = x + OX[r] * l + OX[d] * 4;
        let pz = z + OZ[r] * l + OZ[d] * 4;
        if !eye(w, px, pz) {
            return;
        }
    }
    let mut l = first - 1;
    while l <= last + 1 {
        for i in 1..=3 {
            let px = x + OX[r] * l + OX[d] * i;
            let pz = z + OZ[r] * l + OZ[d] * i;
            if !eye(w, px, pz) {
                return;
            }
        }
        l += 4;
    }
    for l in first..=last {
        for i in 1..=3 {
            let px = x + OX[r] * l + OX[d] * i;
            let pz = z + OZ[r] * l + OZ[d] * i;
            w.set_block(px, y, pz, block::END_PORTAL, 0, 2);
        }
    }
}

fn facing_meta(yaw: f32) -> i32 {
    match (((yaw * 4.0 / 360.0) as f64 + 0.5).floor() as i32) & 3 {
        0 => 2,
        1 => 5,
        2 => 3,
        _ => 4,
    }
}

impl InvView {
    pub fn empty() -> InvView {
        InvView {
            inv: Inventory::default(),
            open: Open::None,
            result: None,
            tile_slots: Vec::new(),
            furnace: None,
            clicks: 0,
            enchant: [0; 3],
            name_seed: 0,
            repair_cost: 0,
        }
    }
}

impl Play {
    pub fn new(mode: GameMode) -> Play {
        Play {
            inv: Inventory::default(),
            open: Open::None,
            mode,
            items: Vec::new(),
            falling: Vec::new(),
            vehicles: Vec::new(),
            orbs: Vec::new(),
            next_item: 1,
            pitch: 0.0,
            fx: Vec::new(),
            collected: Vec::new(),
            alive: true,
            prand: JavaRandom::new(0x706C6179),
            tiles: BTreeMap::new(),
            player: ([0.0; 3], 0.0),
            inv_dirty: true,
            drag: Drag::default(),
            clicks: 0,
            creative_drops: 0,
            sign_open: None,
            enchant_slot: [None],
            repair_slots: [None; 2],
            repair_name: String::new(),
            xp_level: 0,
            enchant_levels: [0; 3],
            name_seed: 0,
            enchant_rand: JavaRandom::new(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_nanos() as i64),
            ),
            sign_updates: Vec::new(),
            pending: Vec::new(),
        }
    }

    pub fn view(&self) -> InvView {
        let furnace = match &self.open {
            Open::Furnace(p) => match self.tiles.get(p) {
                Some(Tile::Furnace {
                    burn,
                    item_burn,
                    cook,
                    ..
                }) => {
                    let ib = if *item_burn == 0 { 200 } else { *item_burn };
                    Some((burn * 13 / ib, cook * 24 / 200))
                }
                _ => None,
            },
            Open::Brewing(p) => match self.tiles.get(p) {
                Some(Tile::Brewing { brew_time, .. }) => Some((*brew_time, -1)),
                _ => None,
            },
            _ => None,
        };
        let mut inv = self.inv.clone();
        let mut tile = self.tile_vec();
        let creative = self.mode == GameMode::Creative;
        let c = Ctx::new(&mut inv, &mut tile, self.window(), creative)
            .with_repair(&self.repair_name, self.xp_level);
        let (result, repair_cost) = (c.result(), c.repair_cost());
        InvView {
            inv: self.inv.clone(),
            open: self.open.clone(),
            result,
            tile_slots: self.tile_vec(),
            furnace,
            clicks: self.clicks,
            enchant: self.enchant_levels,
            name_seed: self.name_seed,
            repair_cost,
        }
    }

    fn spawn(&mut self, e: ItemEntity) {
        self.items.push(e);
    }

    /// Takes over a tile entity created by world generation.
    pub fn add_gen_tile(&mut self, t: crate::world::server::GenTile) {
        use crate::world::server::GenTile;
        match t {
            GenTile::Chest { pos, slots } => {
                let mut a = [None; 27];
                for (d, s) in a.iter_mut().zip(slots) {
                    *d = s;
                }
                self.tiles.insert(pos, Tile::Chest { slots: a });
            }
            GenTile::Dispenser { pos, slots } => {
                let mut a = [None; 9];
                for (d, s) in a.iter_mut().zip(slots) {
                    *d = s;
                }
                self.tiles.insert(pos, Tile::Dispenser { slots: a });
            }
            GenTile::Spawner { pos, mob } => {
                self.tiles.insert(
                    pos,
                    Tile::Spawner {
                        mob: mob.to_string(),
                    },
                );
            }
        }
    }

    /// Takes over an entity created by world generation. Items become item
    /// entities; mobs and minecarts wait in `pending` for their entity types.
    pub fn add_gen_entity(&mut self, e: crate::world::server::GenEntity, r: &mut JavaRandom) {
        use crate::world::server::GenEntity;
        match e {
            GenEntity::FallingBlock { pos, block, meta } => {
                let id = self.next_item;
                self.next_item += 1;
                self.falling.push(crate::entity::falling::FallingBlock::new(
                    id, pos, block, meta,
                ));
            }
            GenEntity::Item { pos, stack } => {
                let id = self.next_item;
                self.next_item += 1;
                let it = ItemEntity::from_block(
                    id,
                    stack,
                    pos[0].floor() as i32,
                    pos[1].floor() as i32,
                    pos[2].floor() as i32,
                    r,
                );
                self.spawn(it);
            }
            GenEntity::ScatteredItem { pos, stack } => {
                let id = self.next_item;
                self.next_item += 1;
                let mut pr = std::mem::replace(&mut self.prand, JavaRandom::new(0));
                let mut it = ItemEntity::new(id, stack, pos);
                self.prand = pr;
                it.pickup_delay = 10;
                self.spawn(it);
            }
            GenEntity::MinecartChest { pos, slots } => {
                use crate::entity::minecart::{CartKind, Minecart};
                let id = crate::entity::vehicle::VEHICLE_ID | self.next_item;
                self.next_item += 1;
                let seed = self.prand.next_long();
                let mut c = Minecart::at(id, CartKind::Chest, pos[0], pos[1], pos[2], seed);
                for (d, s) in c.slots.iter_mut().zip(slots) {
                    *d = s;
                }
                self.vehicles.push(crate::entity::vehicle::Tracked::new(
                    crate::entity::vehicle::Vehicle::Cart(c),
                ));
            }
            other => self.pending.push(other),
        }
    }

    /// `EntityPlayer.dropItem(stack, randomChoice, false)`: thrown from the
    /// player's eyes, or scattered (`randomChoice`, on death).
    pub fn throw_with(&mut self, s: Stack, random_choice: bool, r: &mut JavaRandom) {
        if s.count == 0 {
            return;
        }
        let ([x, y, z], yaw) = self.player;
        let pitch = self.pitch;
        let id = self.next_item;
        self.next_item += 1;
        // posY is the eye position; getEyeHeight() is 0.12.
        let eye = y + 1.62f32 as f64;
        let mut e = ItemEntity::new(id, s, [x, eye - 0.3f32 as f64 + 0.12f32 as f64, z]);
        e.pickup_delay = 40;
        let pr = &mut self.prand;
        if random_choice {
            let f6 = pr.next_float() * 0.5;
            let f7 = pr.next_float() * std::f32::consts::PI * 2.0;
            e.e.motion[0] = (-crate::util::math::sin(f7) * f6) as f64;
            e.e.motion[2] = (crate::util::math::cos(f7) * f6) as f64;
            e.e.motion[1] = 0.2f32 as f64;
        } else {
            use crate::util::math::{cos, sin};
            let pi = std::f32::consts::PI;
            let f = 0.3f32;
            e.e.motion[0] = (-sin(yaw / 180.0 * pi) * cos(pitch / 180.0 * pi) * f) as f64;
            e.e.motion[2] = (cos(yaw / 180.0 * pi) * cos(pitch / 180.0 * pi) * f) as f64;
            e.e.motion[1] = (-sin(pitch / 180.0 * pi) * f + 0.1) as f64;
            let mut f = 0.02f32;
            let f11 = pr.next_float() * pi * 2.0;
            f *= pr.next_float();
            e.e.motion[0] += (f11 as f64).cos() * f as f64;
            e.e.motion[1] += ((pr.next_float() - pr.next_float()) * 0.1) as f64;
            e.e.motion[2] += (f11 as f64).sin() * f as f64;
        }
        self.spawn(e);
    }

    pub fn throw(&mut self, s: Stack, r: &mut JavaRandom) {
        self.throw_with(s, false, r);
    }

    pub fn drop_at_block(&mut self, s: Stack, p: Pos, r: &mut JavaRandom) {
        let id = self.next_item;
        self.next_item += 1;
        let e = ItemEntity::from_block(id, s, p[0], p[1], p[2], r);
        self.spawn(e);
    }

    /// The player finished digging `p` (or clicked it in creative).
    pub fn dig(&mut self, w: &mut ServerWorld, p: Pos) {
        let id = w.block(p[0], p[1], p[2]);
        if id == block::AIR || crate::item::hardness(id) < 0.0 || block::material(id).is_liquid() {
            return;
        }
        let meta = w.meta(p[0], p[1], p[2]);
        let held = self.inv.held();
        if self.mode == GameMode::Creative {
            if let Some(t) = held.and_then(|s| item::tool(s.id)) {
                if t.kind == item::ToolKind::Sword {
                    // ItemSword cannot break blocks in creative.
                    return;
                }
            }
        }
        // Container contents spill out.
        if let Some(t) = self.tiles.remove(&p) {
            let slots: Vec<Option<Stack>> = match t {
                Tile::Furnace { slots, .. } => slots.to_vec(),
                Tile::Chest { slots } => slots.to_vec(),
                Tile::Dispenser { slots } => slots.to_vec(),
                Tile::Brewing { slots, .. } => slots.to_vec(),
                Tile::Spawner { .. } | Tile::Sign { .. } => Vec::new(),
            };
            for s in slots.into_iter().flatten() {
                let mut rnd = std::mem::replace(&mut w.rand, JavaRandom::new(0));
                self.drop_at_block(s, p, &mut rnd);
                w.rand = rnd;
            }
            if self.open != Open::None && self.open != Open::Inventory {
                self.open = Open::None;
                self.inv_dirty = true;
            }
        }
        // The upper half of a double plant goes with the lower one.
        if id == block::DOUBLE_PLANT
            && meta & 8 != 0
            && w.block(p[0], p[1] - 1, p[2]) == block::DOUBLE_PLANT
        {
            let lower = w.meta(p[0], p[1] - 1, p[2]);
            w.set_block(p[0], p[1] - 1, p[2], block::AIR, 0, 3);
            if self.mode == GameMode::Survival {
                let mut rnd = std::mem::replace(&mut w.rand, JavaRandom::new(0));
                for s in item::drops(id, lower, &mut rnd) {
                    self.drop_at_block(s, [p[0], p[1] - 1, p[2]], &mut rnd);
                }
                w.rand = rnd;
            }
        }
        w.set_block(p[0], p[1], p[2], block::AIR, 0, 3);
        if self.mode == GameMode::Survival {
            let mut rnd = std::mem::replace(&mut w.rand, JavaRandom::new(0));
            if item::can_harvest(held, id) {
                for s in item::drops(id, meta, &mut rnd) {
                    self.drop_at_block(s, p, &mut rnd);
                }
                // BlockOre / BlockRedstoneOre.dropBlockAsItemWithChance:
                // dropXpOnBlockBreak
                let range = |r: &mut JavaRandom, a: i32, b: i32| {
                    if a >= b {
                        a
                    } else {
                        r.next_int_n(b - a + 1) + a
                    }
                };
                let xp = match id {
                    block::COAL_ORE => range(&mut rnd, 0, 2),
                    block::DIAMOND_ORE | block::EMERALD_ORE => range(&mut rnd, 3, 7),
                    block::LAPIS_ORE | 153 => range(&mut rnd, 2, 5),
                    block::REDSTONE_ORE | block::LIT_REDSTONE_ORE => 1 + rnd.next_int_n(5),
                    _ => 0,
                };
                if xp > 0 {
                    let c = [p[0] as f64 + 0.5, p[1] as f64 + 0.5, p[2] as f64 + 0.5];
                    self.orbs.push((c, xp));
                }
            }
            w.rand = rnd;
            // Tools lose durability (ItemTool.onBlockDestroyed).
            if let Some(s) = held {
                if let Some(t) = item::tool(s.id) {
                    if crate::item::hardness(id) != 0.0 {
                        let cost = if t.kind == item::ToolKind::Sword {
                            2
                        } else {
                            1
                        };
                        let d = s.meta + cost;
                        self.inv.main[self.inv.selected] = if d > t.max_uses {
                            None
                        } else {
                            Some(Stack { meta: d, ..s })
                        };
                        self.inv_dirty = true;
                    }
                }
            }
            // Breaking ice leaves water unless it rests on air.
            if id == block::ICE && block::material(w.block(p[0], p[1] - 1, p[2])).blocks_movement()
            {
                w.set_block(p[0], p[1], p[2], block::FLOWING_WATER, 0, 3);
            }
        }
    }

    /// Right click on `target` with `face` the side that was hit.
    /// `PlayerControllerMP.onPlayerRightClick`: false when neither the block
    /// nor the item (`Item.onItemUse`) took the click, and the client goes on
    /// to `sendUseItem` (`onItemRightClick`).
    pub fn use_block(
        &mut self,
        w: &mut ServerWorld,
        target: Pos,
        face: [i32; 3],
        sneaking: bool,
    ) -> bool {
        let tid = w.block(target[0], target[1], target[2]);
        if !sneaking || self.inv.held().is_none() {
            match tid {
                block::CRAFTING_TABLE => {
                    self.open = Open::Workbench(target);
                    self.inv.grid_size = 3;
                    self.inv_dirty = true;
                    return true;
                }
                block::FURNACE | block::LIT_FURNACE => {
                    self.tiles.entry(target).or_insert(Tile::Furnace {
                        slots: [None; 3],
                        burn: 0,
                        item_burn: 0,
                        cook: 0,
                    });
                    self.open = Open::Furnace(target);
                    self.inv_dirty = true;
                    return true;
                }
                block::CHEST => {
                    // BlockChest.func_149951_m: the north/west half comes first.
                    let mut halves = vec![target];
                    for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                        let q = [target[0] + dx, target[1], target[2] + dz];
                        if w.block(q[0], q[1], q[2]) == block::CHEST {
                            if dx < 0 || dz < 0 {
                                halves.insert(0, q);
                            } else {
                                halves.push(q);
                            }
                            break;
                        }
                    }
                    if halves
                        .iter()
                        .any(|q| block::info(w.block(q[0], q[1] + 1, q[2])).normal_cube())
                    {
                        return true;
                    }
                    for q in &halves {
                        self.tiles
                            .entry(*q)
                            .or_insert(Tile::Chest { slots: [None; 27] });
                    }
                    self.open = Open::Chest(halves);
                    self.inv_dirty = true;
                    return true;
                }
                block::ANVIL => {
                    // BlockAnvil.onBlockActivated: displayGUIAnvil
                    self.open = Open::Anvil(target);
                    self.repair_name.clear();
                    self.inv_dirty = true;
                    return true;
                }
                block::ENCHANTING_TABLE => {
                    // BlockEnchantmentTable.onBlockActivated
                    self.open = Open::Enchant(target);
                    self.enchant_levels = [0; 3];
                    self.inv_dirty = true;
                    return true;
                }
                block::DISPENSER => {
                    // BlockDispenser.onBlockActivated: displayGUIDispenser
                    self.tiles
                        .entry(target)
                        .or_insert(Tile::Dispenser { slots: [None; 9] });
                    self.open = Open::Dispenser(target);
                    self.inv_dirty = true;
                    return true;
                }
                block::BREWING_STAND => {
                    self.tiles.entry(target).or_insert(Tile::Brewing {
                        slots: [None; 4],
                        brew_time: 0,
                        filled: 0,
                        ingredient: 0,
                    });
                    self.open = Open::Brewing(target);
                    self.inv_dirty = true;
                    return true;
                }
                block::LEVER => {
                    w.toggle_lever(target[0], target[1], target[2]);
                    return true;
                }
                block::STONE_BUTTON | block::WOODEN_BUTTON => {
                    w.press_button(tid, target[0], target[1], target[2]);
                    return true;
                }
                block::UNPOWERED_REPEATER | block::POWERED_REPEATER => {
                    // BlockRedstoneRepeater.onBlockActivated: next delay.
                    let m = w.meta(target[0], target[1], target[2]);
                    let d = (((m & 12) >> 2) + 1) << 2 & 12;
                    w.set_meta(target[0], target[1], target[2], d | (m & 3), 3);
                    return true;
                }
                block::NOTEBLOCK => {
                    w.note_click(target[0], target[1], target[2]);
                    return true;
                }
                block::WOODEN_DOOR => {
                    // BlockDoor.onBlockActivated
                    let [x, y, z] = target;
                    let ly = if w.meta(x, y, z) & 8 != 0 { y - 1 } else { y };
                    let m = w.meta(x, ly, z);
                    w.set_meta(x, ly, z, (m & 7) ^ 4, 2);
                    w.door_sound(x, y, z);
                    return true;
                }
                block::TRAPDOOR => {
                    let [x, y, z] = target;
                    let m = w.meta(x, y, z);
                    w.set_meta(x, y, z, m ^ 4, 2);
                    w.door_sound(x, y, z);
                    return true;
                }
                block::DISPENSER => {
                    self.tiles
                        .entry(target)
                        .or_insert(Tile::Dispenser { slots: [None; 9] });
                    return true;
                }
                block::END_PORTAL_FRAME if self.inv.held().map(|s| s.id) == Some(381) => {
                    let [x, y, z] = target;
                    let m = w.meta(x, y, z);
                    if m & 4 == 0 {
                        w.set_meta(x, y, z, m | 4, 2);
                        if self.mode == GameMode::Survival {
                            self.inv.consume_held();
                            self.inv_dirty = true;
                        }
                        fill_end_portal(w, x, y, z, m & 3);
                    }
                    return true;
                }
                block::TNT if self.inv.held().map(|s| s.id) == Some(259) => {
                    // BlockTNT.onBlockActivated with flint and steel.
                    w.prime_tnt(target[0], target[1], target[2], 80);
                    self.damage_held(1);
                    return true;
                }
                _ => {}
            }
        }
        let Some(held) = self.inv.held() else {
            return false;
        };
        if held.id == 324 || held.id == 330 {
            self.place_door(w, target, face, held.id);
            return true;
        }
        if held.id == 323 {
            return self.place_sign(w, target, face);
        }
        if held.id == 259 {
            // ItemFlintAndSteel.onItemUse: fire on the clicked side.
            let p = [
                target[0] + face[0],
                target[1] + face[1],
                target[2] + face[2],
            ];
            if w.is_air(p[0], p[1], p[2]) {
                let pitch = w.rand.next_float() * 0.4 + 0.8;
                w.redstone_effects
                    .push(crate::world::redstone::Effect::Sound(
                        "fire.ignite",
                        [p[0] as f64 + 0.5, p[1] as f64 + 0.5, p[2] as f64 + 0.5],
                        1.0,
                        pitch,
                    ));
                w.set_block(p[0], p[1], p[2], block::FIRE, 0, 3);
            }
            self.damage_held(1);
            return true;
        }
        let id = match held.id {
            331 => block::REDSTONE_WIRE,
            356 => block::UNPOWERED_REPEATER,
            // ItemReed for the brewing stand and the cauldron
            379 => block::BREWING_STAND,
            380 => block::CAULDRON,
            _ => match held.block() {
                Some(id) => id,
                None => return false,
            },
        };
        // ItemBlock.onItemUse: replaceable targets are replaced in place.
        let tmat = block::material(tid);
        let replace_in_place = tid == block::SNOW_LAYER
            && w.meta(target[0], target[1], target[2]) & 7 < 1
            || tid == block::VINE
            || tid == block::TALLGRASS
            || tid == block::DEADBUSH
            || (tmat.replaceable() && !tmat.is_liquid() && tid != block::AIR);
        let p = if replace_in_place {
            target
        } else {
            [
                target[0] + face[0],
                target[1] + face[1],
                target[2] + face[2],
            ]
        };
        if !(0..256).contains(&p[1]) {
            return false;
        }
        if !w.material(p[0], p[1], p[2]).replaceable() {
            return false;
        }
        let mut meta = held.meta;
        let side = match face {
            [0, -1, 0] => 0,
            [0, 1, 0] => 1,
            [0, 0, -1] => 2,
            [0, 0, 1] => 3,
            [-1, 0, 0] => 4,
            _ => 5,
        };
        let nc = |w: &mut ServerWorld, x: i32, y: i32, z: i32| {
            block::info(w.block(x, y, z)).normal_cube()
        };
        let yaw4 = ((self.player.1 * 4.0 / 360.0) as f64 + 0.5).floor() as i32 & 3;
        match id {
            block::LEVER => {
                // BlockLever.onBlockPlaced + onBlockPlacedBy
                let k = match side {
                    0 if nc(w, p[0], p[1] + 1, p[2]) => 0,
                    1 if w.solid_top_surface(p[0], p[1] - 1, p[2]) => 5,
                    2 if nc(w, p[0], p[1], p[2] + 1) => 4,
                    3 if nc(w, p[0], p[1], p[2] - 1) => 3,
                    4 if nc(w, p[0] + 1, p[1], p[2]) => 2,
                    5 if nc(w, p[0] - 1, p[1], p[2]) => 1,
                    _ => return false,
                };
                meta = match k {
                    5 if yaw4 & 1 != 0 => 6,
                    0 if yaw4 & 1 == 0 => 7,
                    k => k,
                };
            }
            block::STONE_BUTTON | block::WOODEN_BUTTON => {
                meta = match side {
                    2 if nc(w, p[0], p[1], p[2] + 1) => 4,
                    3 if nc(w, p[0], p[1], p[2] - 1) => 3,
                    4 if nc(w, p[0] + 1, p[1], p[2]) => 2,
                    5 if nc(w, p[0] - 1, p[1], p[2]) => 1,
                    _ => return false,
                };
            }
            block::TRAPDOOR => {
                meta = match side {
                    2 if nc(w, p[0], p[1], p[2] + 1) => 0,
                    3 if nc(w, p[0], p[1], p[2] - 1) => 1,
                    4 if nc(w, p[0] + 1, p[1], p[2]) => 2,
                    5 if nc(w, p[0] - 1, p[1], p[2]) => 3,
                    _ => return false,
                };
            }
            block::STONE_PRESSURE_PLATE | block::WOODEN_PRESSURE_PLATE => {
                if !w.solid_top_surface(p[0], p[1] - 1, p[2])
                    && w.block(p[0], p[1] - 1, p[2]) != block::FENCE
                {
                    return false;
                }
                meta = 0;
            }
            block::REDSTONE_WIRE => {
                if !w.solid_top_surface(p[0], p[1] - 1, p[2])
                    && w.block(p[0], p[1] - 1, p[2]) != block::GLOWSTONE
                {
                    return false;
                }
                meta = 0;
            }
            block::UNPOWERED_REPEATER => {
                if !w.solid_top_surface(p[0], p[1] - 1, p[2]) {
                    return false;
                }
                meta = (yaw4 + 2) % 4;
            }
            block::PISTON | block::STICKY_PISTON | block::DISPENSER => {
                meta = self.orientation(p);
            }
            block::TORCH | block::REDSTONE_TORCH => {
                // BlockTorch.onBlockPlaced: attach to the clicked face.
                let side_ok = |w: &mut ServerWorld, x: i32, y: i32, z: i32| {
                    block::info(w.block(x, y, z)).normal_cube()
                };
                meta = match face {
                    [0, 1, 0] => 5,
                    [0, 0, -1] => 4,
                    [0, 0, 1] => 3,
                    [-1, 0, 0] => 2,
                    [1, 0, 0] => 1,
                    _ => 0,
                };
                let support = match meta {
                    1 => side_ok(w, p[0] - 1, p[1], p[2]),
                    2 => side_ok(w, p[0] + 1, p[1], p[2]),
                    3 => side_ok(w, p[0], p[1], p[2] - 1),
                    4 => side_ok(w, p[0], p[1], p[2] + 1),
                    5 => w.solid_top_surface(p[0], p[1] - 1, p[2]),
                    _ => false,
                };
                if !support {
                    return false;
                }
            }
            block::FURNACE | block::CHEST => meta = facing_meta(self.player.1),
            // ItemAnvilBlock.getMetadata (damage << 2), then
            // BlockAnvil.onBlockPlacedBy
            block::ANVIL => {
                let k = (yaw4 + 1) % 4;
                meta = [2, 3, 0, 1][k as usize] | (meta << 2) & 12;
            }
            block::LOG | block::LOG2 => {
                // BlockRotatedPillar.onBlockPlaced
                meta = (meta & 3)
                    | if face[0] != 0 {
                        4
                    } else if face[2] != 0 {
                        8
                    } else {
                        0
                    };
            }
            block::STONE_SLAB => {
                if face == [0, -1, 0] {
                    meta |= 8;
                }
            }
            _ => {}
        }
        if !w.can_place_block_at(id, p[0], p[1], p[2]) {
            return false;
        }
        let needs_support = matches!(
            block::info(id).material,
            block::Material::Plants | block::Material::Vine | block::Material::Cactus
        );
        if needs_support
            && id != block::DOUBLE_PLANT
            && id != block::VINE
            && !w.can_block_stay(id, p[0], p[1], p[2])
        {
            return false;
        }
        // Never place a solid block inside the player.
        let ([px, py, pz], _) = self.player;
        let pbb = Aabb {
            min: [px - 0.3, py, pz - 0.3],
            max: [px + 0.3, py + 1.8, pz + 0.3],
        };
        let bb = Aabb {
            min: [p[0] as f64, p[1] as f64, p[2] as f64],
            max: [p[0] as f64 + 1.0, p[1] as f64 + 1.0, p[2] as f64 + 1.0],
        };
        if block::collision_box(id, meta as u8).is_some() && bb.intersects(&pbb) {
            return false;
        }
        if id == block::DOUBLE_PLANT {
            if !w.is_air(p[0], p[1] + 1, p[2]) {
                return false;
            }
            w.set_block(p[0], p[1], p[2], id, meta & 7, 3);
            w.set_block(p[0], p[1] + 1, p[2], id, 8, 3);
        } else {
            w.set_block(p[0], p[1], p[2], id, meta, 3);
            match id {
                // onBlockPlacedBy runs after the facing chosen in onBlockAdded.
                block::DISPENSER => {
                    w.set_meta(p[0], p[1], p[2], meta, 2);
                }
                block::UNPOWERED_REPEATER => w.repeater_placed(p[0], p[1], p[2]),
                _ => {}
            }
        }
        if self.mode == GameMode::Survival {
            self.inv.consume_held();
            self.inv_dirty = true;
        }
        true
    }

    /// `BlockPistonBase.determineOrientation`
    fn orientation(&self, p: Pos) -> i32 {
        let ([px, py, pz], yaw) = self.player;
        if (px - p[0] as f64).abs() < 2.0 && (pz - p[2] as f64).abs() < 2.0 {
            let d = py + 1.82;
            if d - p[1] as f64 > 2.0 {
                return 1;
            }
            if p[1] as f64 - d > 0.0 {
                return 0;
            }
        }
        match (((yaw * 4.0 / 360.0) as f64 + 0.5).floor() as i32) & 3 {
            0 => 2,
            1 => 5,
            2 => 3,
            _ => 4,
        }
    }

    /// Wears the held item by `n` uses (`ItemStack.damageItem`).
    fn damage_held(&mut self, n: i32) {
        if self.mode != GameMode::Survival {
            return;
        }
        let i = self.inv.selected;
        if let Some(s) = self.inv.main[i] {
            let max = crate::item::max_damage(s.id);
            let d = s.meta + n;
            self.inv.main[i] = if max > 0 && d > max {
                None
            } else {
                Some(Stack { meta: d, ..s })
            };
            self.inv_dirty = true;
        }
    }

    /// `ItemDoor.onItemUse` + `placeDoorBlock`
    fn place_door(&mut self, w: &mut ServerWorld, target: Pos, face: [i32; 3], item_id: u16) {
        if face != [0, 1, 0] {
            return;
        }
        let door = if item_id == 324 {
            block::WOODEN_DOOR
        } else {
            block::IRON_DOOR
        };
        let [x, y, z] = [target[0], target[1] + 1, target[2]];
        if y >= 255
            || !w.solid_top_surface(x, y - 1, z)
            || !w.material(x, y, z).replaceable()
            || !w.material(x, y + 1, z).replaceable()
        {
            return;
        }
        let dir = (((self.player.1 + 180.0) * 4.0 / 360.0) as f64 - 0.5).floor() as i32 & 3;
        let (b0, b1) = match dir {
            0 => (0, 1),
            1 => (-1, 0),
            2 => (0, -1),
            _ => (1, 0),
        };
        let nc = |w: &mut ServerWorld, x: i32, y: i32, z: i32| {
            block::info(w.block(x, y, z)).normal_cube() as i32
        };
        let i1 = nc(w, x - b0, y, z - b1) + nc(w, x - b0, y + 1, z - b1);
        let j1 = nc(w, x + b0, y, z + b1) + nc(w, x + b0, y + 1, z + b1);
        let flag = w.block(x - b0, y, z - b1) == door || w.block(x - b0, y + 1, z - b1) == door;
        let flag1 = w.block(x + b0, y, z + b1) == door || w.block(x + b0, y + 1, z + b1) == door;
        let hinge = (flag && !flag1) || j1 > i1;
        w.set_block(x, y, z, door, dir, 2);
        w.set_block(x, y + 1, z, door, 8 | hinge as i32, 2);
        w.notify_neighbours(x, y, z, door);
        w.notify_neighbours(x, y + 1, z, door);
        if self.mode == GameMode::Survival {
            self.inv.consume_held();
            self.inv_dirty = true;
        }
    }

    /// `BlockDispenser.func_149941_e`: fires one random stack. Arrows and
    /// TNT come back for the caller to spawn as entities.
    pub fn dispense(
        &mut self,
        pos: Pos,
        facing: i32,
        r: &mut JavaRandom,
        sounds: &mut Vec<(&'static str, [f64; 3], f32, f32)>,
    ) -> Option<(Stack, [f64; 3], [f64; 3])> {
        const FX: [i32; 6] = [0, 0, 0, 0, -1, 1];
        const FY: [i32; 6] = [-1, 1, 0, 0, 0, 0];
        const FZ: [i32; 6] = [0, 0, -1, 1, 0, 0];
        let f = (facing as usize).min(5);
        let c = [
            pos[0] as f64 + 0.5,
            pos[1] as f64 + 0.5,
            pos[2] as f64 + 0.5,
        ];
        let Some(Tile::Dispenser { slots }) = self.tiles.get_mut(&pos) else {
            sounds.push(("random.click", c, 1.0, 1.2));
            return None;
        };
        // TileEntityDispenser.func_146017_i
        let mut pick = None;
        let mut j = 1;
        for (k, s) in slots.iter().enumerate() {
            if s.is_some() {
                if r.next_int_n(j) == 0 {
                    pick = Some(k);
                }
                j += 1;
            }
        }
        let Some(k) = pick else {
            sounds.push(("random.click", c, 1.0, 1.2));
            return None;
        };
        let mut s = slots[k].unwrap();
        let one = Stack { count: 1, ..s };
        s.count -= 1;
        slots[k] = if s.count == 0 { None } else { Some(s) };
        let dir = [FX[f] as f64, FY[f] as f64, FZ[f] as f64];
        let mut at = [
            c[0] + 0.7 * dir[0],
            c[1] + 0.7 * dir[1],
            c[2] + 0.7 * dir[2],
        ];
        if one.id == 262 || one.id == block::TNT as u16 {
            let snd = if one.id == 262 { 1.2 } else { 1.0 };
            sounds.push(("random.click", c, 1.0, snd));
            return Some((one, at, dir));
        }
        // BehaviorDefaultDispenseItem.doDispense
        at[1] -= if FY[f] != 0 { 0.125 } else { 0.15625 };
        let id = self.next_item;
        self.next_item += 1;
        let mut e = ItemEntity::new(id, one, at);
        let d3 = r.next_double() * 0.1 + 0.2;
        e.e.motion = [dir[0] * d3, 0.20000000298023224, dir[2] * d3];
        for v in &mut e.e.motion {
            *v += r.next_gaussian() * 0.007499999832361937 * 6.0;
        }
        self.spawn(e);
        sounds.push(("random.click", c, 1.0, 1.0));
        None
    }

    /// `ItemSign.onItemUse`: a standing sign on top, a wall sign on a side,
    /// then the sign editor (`func_146100_a`).
    fn place_sign(&mut self, w: &mut ServerWorld, t: Pos, face: [i32; 3]) -> bool {
        let side = match face {
            [0, -1, 0] => 0,
            [0, 1, 0] => 1,
            [0, 0, -1] => 2,
            [0, 0, 1] => 3,
            [-1, 0, 0] => 4,
            _ => 5,
        };
        if side == 0 || !w.material(t[0], t[1], t[2]).is_solid() {
            return false;
        }
        let p = [t[0] + face[0], t[1] + face[1], t[2] + face[2]];
        if !(0..256).contains(&p[1]) || !w.material(p[0], p[1], p[2]).replaceable() {
            return false;
        }
        if side == 1 {
            let m = (((self.player.1 + 180.0) * 16.0 / 360.0) as f64 + 0.5).floor() as i32 & 15;
            w.set_block(p[0], p[1], p[2], block::STANDING_SIGN, m, 3);
        } else {
            w.set_block(p[0], p[1], p[2], block::WALL_SIGN, side, 3);
        }
        self.tiles.insert(
            p,
            Tile::Sign {
                text: Default::default(),
                editable: true,
            },
        );
        if self.mode != GameMode::Creative {
            self.inv.consume_held();
        }
        self.inv_dirty = true;
        self.sign_open = Some(p);
        true
    }

    /// `NetHandlerPlayServer.processUpdateSign`
    pub fn update_sign(&mut self, p: Pos, lines: [String; 4]) {
        if let Some(Tile::Sign { text, editable }) = self.tiles.get_mut(&p) {
            if !*editable {
                return;
            }
            *editable = false;
            for (i, l) in lines.into_iter().enumerate() {
                // ChatAllowedCharacters, fifteen at most
                let ok = l.chars().count() <= 15
                    && l.chars()
                        .all(|c| c != '\u{a7}' && c >= ' ' && c != '\u{7f}');
                text[i] = if ok { l } else { "!?".to_string() };
            }
            self.sign_updates.push((p, text.clone()));
        }
    }

    /// The text of the signs in a chunk (`S33PacketUpdateSign` on load).
    pub fn signs_in(&self, cx: i32, cz: i32) -> Vec<(Pos, [String; 4])> {
        self.tiles
            .iter()
            .filter(|(p, _)| p[0] >> 4 == cx && p[2] >> 4 == cz)
            .filter_map(|(p, t)| match t {
                Tile::Sign { text, .. } => Some((*p, text.clone())),
                _ => None,
            })
            .collect()
    }

    /// The `Container` of the open window.
    pub fn window(&self) -> Window {
        window_of(&self.open)
    }

    /// The tile entity (or minecart) slots behind the open window.
    pub fn tile_vec(&self) -> Vec<Option<Stack>> {
        match &self.open {
            Open::Furnace(p) => match self.tiles.get(p) {
                Some(Tile::Furnace { slots, .. }) => slots.to_vec(),
                _ => Vec::new(),
            },
            Open::Brewing(p) => match self.tiles.get(p) {
                Some(Tile::Brewing { slots, .. }) => slots.to_vec(),
                _ => Vec::new(),
            },
            Open::Dispenser(p) => match self.tiles.get(p) {
                Some(Tile::Dispenser { slots }) => slots.to_vec(),
                _ => Vec::new(),
            },
            Open::Enchant(_) => self.enchant_slot.to_vec(),
            Open::Anvil(_) => self.repair_slots.to_vec(),
            Open::Chest(ps) => {
                let mut v = Vec::new();
                for p in ps {
                    if let Some(Tile::Chest { slots }) = self.tiles.get(p) {
                        v.extend_from_slice(slots);
                    }
                }
                v
            }
            Open::Cart(id) => match self.vehicles.iter().find(|b| b.v.id() == *id) {
                Some(crate::entity::vehicle::Tracked {
                    v: crate::entity::vehicle::Vehicle::Cart(c),
                    ..
                }) => c.slots.clone(),
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    fn store_tile_vec(&mut self, v: &[Option<Stack>]) {
        let copy = |d: &mut [Option<Stack>], v: &[Option<Stack>]| {
            for (a, b) in d.iter_mut().zip(v) {
                *a = *b;
            }
        };
        match self.open.clone() {
            Open::Furnace(p) => {
                if let Some(Tile::Furnace { slots, .. }) = self.tiles.get_mut(&p) {
                    copy(slots, v);
                }
            }
            Open::Brewing(p) => {
                if let Some(Tile::Brewing { slots, .. }) = self.tiles.get_mut(&p) {
                    copy(slots, v);
                }
            }
            Open::Dispenser(p) => {
                if let Some(Tile::Dispenser { slots }) = self.tiles.get_mut(&p) {
                    copy(slots, v);
                }
            }
            Open::Enchant(_) => copy(&mut self.enchant_slot, v),
            Open::Anvil(_) => copy(&mut self.repair_slots, v),
            Open::Chest(ps) => {
                for (k, p) in ps.iter().enumerate() {
                    if let Some(Tile::Chest { slots }) = self.tiles.get_mut(p) {
                        copy(slots, &v[(k * 27).min(v.len())..]);
                    }
                }
            }
            Open::Cart(id) => {
                if let Some(crate::entity::vehicle::Tracked {
                    v: crate::entity::vehicle::Vehicle::Cart(c),
                    ..
                }) = self.vehicles.iter_mut().find(|b| b.v.id() == id)
                {
                    copy(&mut c.slots, v);
                }
            }
            _ => {}
        }
    }

    /// `C0EPacketClickWindow`: `Container.slotClick` on the open window.
    /// Returns the experience levels spent (the anvil).
    pub fn window_click(&mut self, w: &mut ServerWorld, slot: i32, button: i32, mode: i32) -> i32 {
        self.clicks = self.clicks.wrapping_add(1);
        if self.open == Open::None {
            return 0;
        }
        self.inv_dirty = true;
        let window = self.window();
        let mut tile = self.tile_vec();
        let creative = self.mode == GameMode::Creative;
        let mut drag = std::mem::take(&mut self.drag);
        let fx = {
            let mut c = Ctx::new(&mut self.inv, &mut tile, window, creative)
                .with_repair(&self.repair_name, self.xp_level);
            container::slot_click(&mut c, &mut drag, slot, button, mode);
            c.fx
        };
        self.drag = drag;
        let enchant_before = self.enchant_slot[0];
        self.store_tile_vec(&tile);
        if let Open::Enchant(p) = self.open {
            if self.enchant_slot[0] != enchant_before {
                self.enchant_changed(w, p);
            }
        }
        let mut rnd = std::mem::replace(&mut w.rand, JavaRandom::new(0));
        // dropPlayerItemWithRandomChoice(stack, flag) never scatters
        for (s, _) in fx.drops {
            self.throw(s, &mut rnd);
        }
        w.rand = rnd;
        for (out, n) in fx.smelted {
            // SlotFurnace.onCrafting: experience for what was taken
            let mut pr = std::mem::replace(&mut self.prand, JavaRandom::new(0));
            let mut xp = furnace_xp(&out, n as i32, &mut pr);
            self.prand = pr;
            let (f, _) = self.player;
            while xp > 0 {
                let k = crate::entity::xp_orb::xp_split(xp);
                xp -= k;
                self.orbs.push(([f[0], f[1] + 0.5, f[2] + 0.5], k));
            }
        }
        let mut spent = 0;
        if let (Some(cost), Open::Anvil(p)) = (fx.repaired, self.open.clone()) {
            if !creative {
                spent = cost;
            }
            self.anvil_used(w, p, creative);
        }
        spent
    }

    /// The rest of `ContainerRepair$2.onPickupFromSlot`: a 12% chance
    /// to wear the anvil a step (and break it after the third), with
    /// `playAuxSFX` 1020 (break) or 1021 (use).
    fn anvil_used(&mut self, w: &mut ServerWorld, p: Pos, creative: bool) {
        let (x, y, z) = (p[0], p[1], p[2]);
        let mut sound = "random.anvil_use";
        if !creative && w.block(x, y, z) == block::ANVIL && self.prand.next_float() < 0.12 {
            let m = w.meta(x, y, z);
            let d = (m >> 2) + 1;
            if d > 2 {
                w.set_air(x, y, z);
                sound = "random.anvil_break";
            } else {
                w.set_meta(x, y, z, m & 3 | d << 2, 2);
            }
        }
        // RenderGlobal.playAuxSFX: the client's world.rand picks the pitch
        let pitch = w.rand.next_float() * 0.1 + 0.9;
        self.fx.push(crate::entity::base::Fx::Sound(
            sound,
            [x as f64 + 0.5, y as f64 + 0.5, z as f64 + 0.5],
            1.0,
            pitch,
        ));
    }

    /// `MC|ItemName`: `ContainerRepair.updateItemName` with the name
    /// cut to the allowed characters, ignored above 30 of them.
    pub fn rename(&mut self, name: &str) {
        if !matches!(self.open, Open::Anvil(_)) {
            return;
        }
        let s: String = name
            .chars()
            .filter(|&c| crate::menu::widgets::allowed_char(c))
            .collect();
        if s.encode_utf16().count() <= 30 {
            self.repair_name = s;
            self.inv_dirty = true;
        }
    }

    /// `NetHandlerPlayServer.processCreativeInventoryAction`
    pub fn creative_slot(&mut self, w: &mut ServerWorld, slot: i32, stack: Option<Stack>) {
        self.clicks = self.clicks.wrapping_add(1);
        if self.mode != GameMode::Creative {
            return;
        }
        let ok = stack.is_none_or(|s| s.meta >= 0 && s.count <= 64 && s.count > 0);
        if (1..45).contains(&slot) && ok {
            let mut tile: [Option<Stack>; 0] = [];
            let mut c = Ctx::new(&mut self.inv, &mut tile, Window::Player, true);
            c.set_slot(slot as usize, stack);
            self.inv_dirty = true;
        } else if slot < 0 && ok && self.creative_drops < 200 {
            self.creative_drops += 20;
            if let Some(s) = stack {
                let mut rnd = std::mem::replace(&mut w.rand, JavaRandom::new(0));
                self.throw(s, &mut rnd);
                w.rand = rnd;
                // setAgeToCreativeDespawnTime
                if let Some(e) = self.items.last_mut() {
                    e.age = 4800;
                }
            }
        }
    }

    /// `ContainerEnchantment.onCraftMatrixChanged`: a new name seed and the
    /// three levels from the bookshelves around the table.
    fn enchant_changed(&mut self, w: &mut ServerWorld, p: Pos) {
        let Some(s) = self.enchant_slot[0].filter(crate::enchant::enchantable) else {
            self.enchant_levels = [0; 3];
            return;
        };
        self.name_seed = self.enchant_rand.next_long();
        let shelf =
            |w: &mut ServerWorld, x: i32, y: i32, z: i32| w.block(x, y, z) == block::BOOKSHELF;
        let (x, y, z) = (p[0], p[1], p[2]);
        let mut n = 0;
        for dz in -1..=1 {
            for dx in -1..=1 {
                if (dz != 0 || dx != 0)
                    && w.is_air(x + dx, y, z + dz)
                    && w.is_air(x + dx, y + 1, z + dz)
                {
                    n += shelf(w, x + dx * 2, y, z + dz * 2) as i32;
                    n += shelf(w, x + dx * 2, y + 1, z + dz * 2) as i32;
                    if dx != 0 && dz != 0 {
                        n += shelf(w, x + dx * 2, y, z + dz) as i32;
                        n += shelf(w, x + dx * 2, y + 1, z + dz) as i32;
                        n += shelf(w, x + dx, y, z + dz * 2) as i32;
                        n += shelf(w, x + dx, y + 1, z + dz * 2) as i32;
                    }
                }
            }
        }
        for k in 0..3 {
            self.enchant_levels[k] = crate::enchant::calc_level(&mut self.enchant_rand, k, n, &s);
        }
        self.inv_dirty = true;
    }

    /// `ContainerEnchantment.enchantItem`: `xp_level` is the player's
    /// level; returns the levels spent.
    pub fn enchant_item(&mut self, w: &mut ServerWorld, k: usize, xp_level: i32) -> Option<i32> {
        let Open::Enchant(p) = self.open else {
            return None;
        };
        let cost = *self.enchant_levels.get(k)?;
        let s = self.enchant_slot[0]?;
        let creative = self.mode == GameMode::Creative;
        if cost <= 0 || !(xp_level >= cost || creative) {
            return None;
        }
        let list = crate::enchant::build_list(&mut self.enchant_rand, &s, cost);
        if list.is_empty() {
            return Some(0);
        }
        let book = s.id == 340;
        let mut out = s;
        if book {
            out.id = 403;
        }
        let skip = if book && list.len() > 1 {
            self.enchant_rand.next_int_n(list.len() as i32) as usize
        } else {
            usize::MAX
        };
        let mut have = out.enchantments();
        for (i, e) in list.iter().enumerate() {
            if !book || i != skip {
                have.push(*e);
            }
        }
        out = out.with_enchantments(&have);
        self.enchant_slot[0] = Some(out);
        self.enchant_changed(w, p);
        self.inv_dirty = true;
        Some(cost)
    }

    /// `closeScreen` / `C0DPacketCloseWindow`: `onContainerClosed` throws
    /// out the cursor stack and the crafting grid.
    pub fn close(&mut self, w: &mut ServerWorld) {
        let window = self.window();
        let mut rnd = std::mem::replace(&mut w.rand, JavaRandom::new(0));
        for (s, _) in container::close(&mut self.inv, window) {
            self.throw(s, &mut rnd);
        }
        // ContainerEnchantment.onContainerClosed
        if let Some(s) = self.enchant_slot[0].take() {
            self.throw(s, &mut rnd);
        }
        // ContainerRepair.onContainerClosed
        for k in 0..2 {
            if let Some(s) = self.repair_slots[k].take() {
                self.throw(s, &mut rnd);
            }
        }
        self.repair_name.clear();
        self.enchant_levels = [0; 3];
        w.rand = rnd;
        self.open = Open::None;
        self.drag = Drag::default();
        self.inv.grid_size = 2;
        self.inv_dirty = true;
    }

    /// Q: throws one item (or the stack) from the selected slot.
    pub fn drop_held(&mut self, w: &mut ServerWorld, all: bool) {
        let sel = self.inv.selected;
        let Some(s) = self.inv.main[sel] else { return };
        let n = if all { s.count } else { 1 };
        self.inv.main[sel] = if s.count > n {
            Some(Stack {
                count: s.count - n,
                ..s
            })
        } else {
            None
        };
        let mut rnd = std::mem::replace(&mut w.rand, JavaRandom::new(0));
        self.throw(Stack { count: n, ..s }, &mut rnd);
        w.rand = rnd;
        self.inv_dirty = true;
    }

    /// Creative middle click: put the block in the hotbar.
    pub fn pick_block(&mut self, id: u8, meta: i32) {
        if self.mode != GameMode::Creative || !block::info(id).exists || id == block::AIR {
            return;
        }
        let want = Stack::new(
            id as ItemId,
            if id == block::LEAVES || id == block::LEAVES2 || id == block::LOG || id == block::LOG2
            {
                meta & 3
            } else {
                meta
            },
            1,
        );
        if let Some(i) = (0..9).find(
            |&i| matches!(self.inv.main[i], Some(s) if s.id == want.id && s.meta == want.meta),
        ) {
            self.inv.selected = i;
        } else {
            self.inv.main[self.inv.selected] = Some(want);
        }
        self.inv_dirty = true;
    }

    /// Items, furnaces and pickup for one tick.
    pub fn tick(&mut self, w: &mut ServerWorld) {
        // TileEntityBrewingStand.updateEntity
        let keys: Vec<Pos> = self.tiles.keys().copied().collect();
        for p in &keys {
            if !w.chunk_exists(p[0] >> 4, p[2] >> 4) {
                continue;
            }
            if let Some(Tile::Brewing {
                slots,
                brew_time,
                filled,
                ingredient,
            }) = self.tiles.get_mut(p)
            {
                if *brew_time > 0 {
                    *brew_time -= 1;
                    if *brew_time == 0 {
                        brew_potions(slots);
                    } else if !can_brew(slots) || Some(*ingredient) != slots[3].map(|s| s.id) {
                        *brew_time = 0;
                    }
                } else if can_brew(slots) {
                    *brew_time = 400;
                    *ingredient = slots[3].unwrap().id;
                }
                // getFilledSlots: a bit per potion bottle
                let mut i = 0;
                for (k, s) in slots[..3].iter().enumerate() {
                    if s.is_some_and(|s| s.id == 373) {
                        i |= 1 << k;
                    }
                }
                if i != *filled {
                    *filled = i;
                    w.set_meta(p[0], p[1], p[2], i, 2);
                }
                if self.open == Open::Brewing(*p) {
                    self.inv_dirty = true;
                }
            }
        }
        // TileEntityFurnace.updateEntity
        for p in keys {
            if !w.chunk_exists(p[0] >> 4, p[2] >> 4) {
                continue;
            }
            let mut lit_change = None;
            if let Some(Tile::Furnace {
                slots,
                burn,
                item_burn,
                cook,
            }) = self.tiles.get_mut(&p)
            {
                let was_burning = *burn > 0;
                if *burn > 0 {
                    *burn -= 1;
                }
                let can_smelt = |slots: &[Option<Stack>; 3]| match slots[0].and_then(|s| smelt(&s))
                {
                    None => false,
                    Some(r) => match slots[2] {
                        None => true,
                        Some(o) => {
                            o.same_item(&r)
                                && o.meta == r.meta
                                && o.count + r.count <= max_stack(o.id)
                        }
                    },
                };
                if *burn == 0 && can_smelt(slots) {
                    if let Some(f) = slots[1] {
                        let t = burn_time(&f);
                        if t > 0 {
                            *burn = t;
                            *item_burn = t;
                            slots[1] = if f.count > 1 {
                                Some(Stack {
                                    count: f.count - 1,
                                    ..f
                                })
                            } else {
                                None
                            };
                        }
                    }
                }
                if *burn > 0 && can_smelt(slots) {
                    *cook += 1;
                    if *cook == 200 {
                        *cook = 0;
                        let r = smelt(&slots[0].unwrap()).unwrap();
                        merge_slot(&mut slots[2], r);
                        let i = slots[0].unwrap();
                        slots[0] = if i.count > 1 {
                            Some(Stack {
                                count: i.count - 1,
                                ..i
                            })
                        } else {
                            None
                        };
                    }
                } else {
                    *cook = 0;
                }
                if was_burning != (*burn > 0) {
                    lit_change = Some(*burn > 0);
                }
                if self.open == Open::Furnace(p) {
                    self.inv_dirty = true;
                }
            }
            if let Some(lit) = lit_change {
                // BlockFurnace.updateFurnaceBlockState keeps the tile entity.
                let m = w.meta(p[0], p[1], p[2]);
                w.set_block(
                    p[0],
                    p[1],
                    p[2],
                    if lit {
                        block::LIT_FURNACE
                    } else {
                        block::FURNACE
                    },
                    m,
                    3,
                );
            }
        }

        // EntityFallingBlock.onUpdate
        let mut falling = std::mem::take(&mut self.falling);
        let mut drops = Vec::new();
        for f in falling.iter_mut() {
            f.tick(w, &mut drops, &mut self.fx);
        }
        falling.retain(|f| !f.e.dead);
        self.falling = falling;
        // Entity.entityDropItem(stack, 0): delayBeforeCanPickup 10
        let mut rnd = std::mem::replace(&mut w.rand, JavaRandom::new(0));
        for (p, st) in drops {
            let id = self.next_item;
            self.next_item += 1;
            let mut e = ItemEntity::new(id, st, p);
            e.pickup_delay = 10;
            self.spawn(e);
        }
        w.rand = rnd;
        // EntityItem.onUpdate (merging included), then the player's
        // EntityPlayer.onLivingUpdate pickup of the items in reach.
        let mut items = std::mem::take(&mut self.items);
        crate::entity::item::tick_all(&mut items, w, &mut self.fx);
        if self.alive {
            let ([px, py, pz], _) = self.player;
            // bb.expand(1.0, 0.5, 1.0)
            let reach = Aabb {
                min: [px - 0.3 - 1.0, py - 0.5, pz - 0.3 - 1.0],
                max: [px + 0.3 + 1.0, py + 1.8 + 0.5, pz + 0.3 + 1.0],
            };
            for e in items.iter_mut() {
                if e.e.dead || !e.bb().intersects(&reach) {
                    continue;
                }
                // EntityItem.onCollideWithPlayer
                if e.pickup_delay != 0 {
                    continue;
                }
                let creative = self.mode == GameMode::Creative;
                let mut st = e.stack;
                let added = self.inv.add_stack(&mut st, creative);
                let rest = st.count;
                let shown = e.stack;
                if rest != e.stack.count {
                    // the entity's own stack is what was stored from
                    e.stack.count = rest;
                    self.inv_dirty = true;
                }
                if added {
                    let p = ((e.e.rand.next_float() - e.e.rand.next_float()) * 0.7 + 1.0) * 2.0;
                    self.fx.push(crate::entity::base::Fx::Sound(
                        "random.pop",
                        [px, py + 1.62, pz],
                        0.2,
                        p,
                    ));
                    self.collected.push(ItemView {
                        id: e.id,
                        stack: shown,
                        pos: e.e.pos,
                        prev: e.e.prev,
                        age: e.age,
                        hover: e.hover,
                    });
                    if rest == 0 {
                        e.e.dead = true;
                    }
                    self.inv_dirty = true;
                }
            }
            items.retain(|e| !e.e.dead);
        }
        self.items = items;
    }

    pub fn item_views(&self) -> Vec<ItemView> {
        self.items
            .iter()
            .map(|e| ItemView {
                id: e.id,
                stack: e.stack,
                pos: e.e.pos,
                prev: e.e.prev,
                age: e.age,
                hover: e.hover,
            })
            .collect()
    }

    /// Level-file lines for the tile entities.
    /// `TileEntity.writeToNBT` for the tiles in one chunk.
    pub fn tiles_nbt(&self, cx: i32, cz: i32) -> Vec<crate::world::anvil::Nbt> {
        use crate::world::anvil::{compound, item_nbt, Nbt};
        let items = |slots: &[Option<Stack>]| {
            Nbt::List(
                10,
                slots
                    .iter()
                    .enumerate()
                    .filter_map(|(i, s)| s.as_ref().map(|s| item_nbt(Some(i as i8), s)))
                    .collect(),
            )
        };
        self.tiles
            .iter()
            .filter(|(p, _)| p[0] >> 4 == cx && p[2] >> 4 == cz)
            .map(|(p, t)| {
                let (id, mut extra) = match t {
                    Tile::Chest { slots } => ("Chest", vec![("Items", items(slots))]),
                    Tile::Dispenser { slots } => ("Trap", vec![("Items", items(slots))]),
                    Tile::Furnace {
                        slots,
                        burn,
                        item_burn,
                        cook,
                    } => {
                        let _ = item_burn;
                        (
                            "Furnace",
                            vec![
                                ("Items", items(slots)),
                                ("BurnTime", Nbt::Short(*burn as i16)),
                                ("CookTime", Nbt::Short(*cook as i16)),
                            ],
                        )
                    }
                    Tile::Brewing {
                        slots, brew_time, ..
                    } => (
                        // TileEntity.addMapping(TileEntityBrewingStand, "Cauldron")
                        "Cauldron",
                        vec![
                            ("Items", items(slots)),
                            ("BrewTime", Nbt::Short(*brew_time as i16)),
                        ],
                    ),
                    Tile::Spawner { mob } => (
                        "MobSpawner",
                        vec![
                            ("EntityId", Nbt::String(mob.clone())),
                            ("Delay", Nbt::Short(20)),
                        ],
                    ),
                    Tile::Sign { text, .. } => (
                        "Sign",
                        vec![
                            ("Text1", Nbt::String(text[0].clone())),
                            ("Text2", Nbt::String(text[1].clone())),
                            ("Text3", Nbt::String(text[2].clone())),
                            ("Text4", Nbt::String(text[3].clone())),
                        ],
                    ),
                };
                let mut v = vec![
                    ("id", Nbt::String(id.into())),
                    ("x", Nbt::Int(p[0])),
                    ("y", Nbt::Int(p[1])),
                    ("z", Nbt::Int(p[2])),
                ];
                v.append(&mut extra);
                compound(v)
            })
            .collect()
    }

    /// `TileEntity.createAndLoadEntity` for the tiles this port keeps.
    pub fn tile_from_nbt(&mut self, t: &crate::world::anvil::Nbt) {
        use crate::world::anvil::slots;
        let (Some(id), Some(x), Some(y), Some(z)) =
            (t.string("id"), t.int("x"), t.int("y"), t.int("z"))
        else {
            return;
        };
        let pos = [x as i32, y as i32, z as i32];
        let got = slots(t.list("Items"));
        let fill = |n: usize| {
            let mut v = vec![None; n];
            for (&k, s) in &got {
                if (k as usize) < n {
                    v[k as usize] = Some(*s);
                }
            }
            v
        };
        let tile = match id {
            "Chest" => {
                let v = fill(27);
                let mut a = [None; 27];
                a.copy_from_slice(&v);
                Tile::Chest { slots: a }
            }
            "Trap" => {
                let v = fill(9);
                let mut a = [None; 9];
                a.copy_from_slice(&v);
                Tile::Dispenser { slots: a }
            }
            "Furnace" => {
                let v = fill(3);
                Tile::Furnace {
                    slots: [v[0], v[1], v[2]],
                    burn: t.int("BurnTime").unwrap_or(0) as i32,
                    item_burn: t.int("BurnTime").unwrap_or(0) as i32,
                    cook: t.int("CookTime").unwrap_or(0) as i32,
                }
            }
            "Cauldron" => {
                let v = fill(4);
                Tile::Brewing {
                    slots: [v[0], v[1], v[2], v[3]],
                    brew_time: t.int("BrewTime").unwrap_or(0) as i32,
                    filled: 0,
                    ingredient: v[3].map_or(0, |s| s.id),
                }
            }
            "MobSpawner" => Tile::Spawner {
                mob: t.string("EntityId").unwrap_or("Pig").to_string(),
            },
            // TileEntitySign.readFromNBT: at most 15 characters a line
            "Sign" => {
                let mut text: [String; 4] = Default::default();
                for (i, l) in text.iter_mut().enumerate() {
                    let s = t.string(&format!("Text{}", i + 1)).unwrap_or("");
                    *l = s.chars().take(15).collect();
                }
                Tile::Sign {
                    text,
                    editable: false,
                }
            }
            _ => return,
        };
        self.tiles.insert(pos, tile);
    }

    /// Dropped items in one chunk as `EntityItem` NBT (`Entity.writeToNBT`
    /// + `EntityItem.writeEntityToNBT`).
    pub fn items_nbt(&self, cx: i32, cz: i32, dim: i32) -> Vec<crate::world::anvil::Nbt> {
        use crate::world::anvil::{compound, item_nbt, Nbt};
        self.items
            .iter()
            .filter(|e| {
                (e.e.pos[0].floor() as i32) >> 4 == cx && (e.e.pos[2].floor() as i32) >> 4 == cz
            })
            .map(|e| {
                let mut v = vec![("id", Nbt::String("Item".into()))];
                v.extend(e.e.nbt_fields(dim, e.id));
                v.push(("Health", Nbt::Short(e.health as u8 as i16)));
                v.push(("Age", Nbt::Short(e.age as i16)));
                v.push(("Item", item_nbt(None, &e.stack)));
                compound(v)
            })
            .collect()
    }

    /// Dropped items and falling blocks of one dimension for the project's
    /// own save format (`data/drops.dat`), as entity NBT.
    pub fn drops_nbt(
        items: &[ItemEntity],
        falling: &[crate::entity::falling::FallingBlock],
        vehicles: &[crate::entity::vehicle::Tracked],
        dim: i32,
    ) -> Vec<u8> {
        use crate::world::anvil::{compound, Nbt};
        let mut tmp = Play::new(GameMode::Survival);
        tmp.items = items.to_vec();
        tmp.falling = falling.to_vec();
        let mut keys = std::collections::BTreeSet::new();
        for e in &tmp.items {
            keys.insert((
                (e.e.pos[0].floor() as i32) >> 4,
                (e.e.pos[2].floor() as i32) >> 4,
            ));
        }
        for f in &tmp.falling {
            keys.insert((
                (f.e.pos[0].floor() as i32) >> 4,
                (f.e.pos[2].floor() as i32) >> 4,
            ));
        }
        let mut list = Vec::new();
        for (cx, cz) in keys {
            list.extend(tmp.items_nbt(cx, cz, dim));
            list.extend(tmp.falling_nbt(cx, cz, dim));
        }
        list.extend(vehicles.iter().map(|b| b.to_nbt(dim)));
        crate::world::anvil::write_nbt(&compound(vec![("Entities", Nbt::List(10, list))]))
    }

    /// Reads `data/drops.dat` back.
    pub fn load_drops(&mut self, bytes: &[u8], r: &mut JavaRandom) {
        let Ok(root) = crate::world::anvil::read_nbt(bytes) else {
            return;
        };
        for e in root.list("Entities") {
            match e.string("id") {
                Some("Item") => self.item_from_nbt(e, r),
                Some("FallingSand") => self.falling_from_nbt(e),
                Some(_) => {
                    self.vehicle_from_nbt(e);
                }
                _ => {}
            }
        }
    }

    /// The boats and minecarts of a chunk.
    pub fn vehicles_nbt(&self, cx: i32, cz: i32, dim: i32) -> Vec<crate::world::anvil::Nbt> {
        self.vehicles
            .iter()
            .filter(|b| {
                let p = b.v.e().pos;
                (p[0].floor() as i32) >> 4 == cx && (p[2].floor() as i32) >> 4 == cz
            })
            .map(|b| b.to_nbt(dim))
            .collect()
    }

    /// An `EntityBoat` or `EntityMinecart` from a save; false for other ids.
    pub fn vehicle_from_nbt(&mut self, t: &crate::world::anvil::Nbt) -> bool {
        let id = crate::entity::vehicle::VEHICLE_ID | self.next_item;
        let seed = self.prand.next_long();
        match crate::entity::vehicle::Tracked::from_nbt(t, id, seed) {
            Some(v) => {
                self.next_item += 1;
                self.vehicles.push(v);
                true
            }
            None => false,
        }
    }

    /// `EntityFallingBlock.writeEntityToNBT` for the falling blocks of a chunk.
    pub fn falling_nbt(&self, cx: i32, cz: i32, dim: i32) -> Vec<crate::world::anvil::Nbt> {
        use crate::world::anvil::{compound, Nbt};
        self.falling
            .iter()
            .filter(|f| {
                (f.e.pos[0].floor() as i32) >> 4 == cx && (f.e.pos[2].floor() as i32) >> 4 == cz
            })
            .map(|f| {
                let mut v = vec![("id", Nbt::String("FallingSand".into()))];
                v.extend(f.e.nbt_fields(dim, f.id));
                v.push(("Tile", Nbt::Byte(f.block as i8)));
                v.push(("TileID", Nbt::Int(f.block as i32)));
                v.push(("Data", Nbt::Byte(f.meta as i8)));
                v.push(("Time", Nbt::Byte(f.fall_time as i8)));
                v.push(("DropItem", Nbt::Byte(f.drop_item as i8)));
                v.push(("HurtEntities", Nbt::Byte(f.hurt_entities as i8)));
                v.push(("FallHurtAmount", Nbt::Float(f.fall_hurt_amount)));
                v.push(("FallHurtMax", Nbt::Int(f.fall_hurt_max)));
                compound(v)
            })
            .collect()
    }

    /// `EntityFallingBlock.readEntityFromNBT`
    pub fn falling_from_nbt(&mut self, t: &crate::world::anvil::Nbt) {
        use crate::world::anvil::Nbt;
        let pos = t.list("Pos");
        let p = |i: usize| match pos.get(i) {
            Some(Nbt::Double(v)) => *v,
            _ => 0.0,
        };
        let mut block = match t.get("TileID") {
            Some(Nbt::Int(v)) => *v as u8,
            _ => (t.int("Tile").unwrap_or(12) & 255) as u8,
        };
        if block::material(block) == block::Material::Air {
            block = block::SAND;
        }
        let id = self.next_item;
        self.next_item += 1;
        let meta = (t.int("Data").unwrap_or(0) & 255) as i32;
        let mut f = crate::entity::falling::FallingBlock::new(id, [p(0), p(1), p(2)], block, meta);
        let mo = t.list("Motion");
        let m = |i: usize| match mo.get(i) {
            Some(Nbt::Double(v)) => *v,
            _ => 0.0,
        };
        f.e.motion = [m(0), m(1), m(2)];
        f.fall_time = (t.int("Time").unwrap_or(0) & 255) as i32;
        if t.get("HurtEntities").is_some() {
            f.hurt_entities = t.int("HurtEntities").unwrap_or(0) != 0;
            f.fall_hurt_amount = t.float("FallHurtAmount").unwrap_or(2.0) as f32;
            f.fall_hurt_max = t.int("FallHurtMax").unwrap_or(40) as i32;
        }
        if t.get("DropItem").is_some() {
            f.drop_item = t.int("DropItem").unwrap_or(1) != 0;
        }
        self.falling.push(f);
    }

    /// An `EntityItem` from a region file.
    pub fn item_from_nbt(&mut self, t: &crate::world::anvil::Nbt, r: &mut JavaRandom) {
        use crate::world::anvil::Nbt;
        let Some(stack) = t.get("Item").and_then(crate::world::anvil::item_from_nbt) else {
            return;
        };
        let pos = t.list("Pos");
        let p = |i: usize| match pos.get(i) {
            Some(Nbt::Double(v)) => *v,
            _ => 0.0,
        };
        let id = self.next_item;
        self.next_item += 1;
        let mut e = ItemEntity::new(id, stack, [p(0), p(1), p(2)]);
        let mo = t.list("Motion");
        let m = |i: usize| match mo.get(i) {
            Some(Nbt::Double(v)) => *v,
            _ => 0.0,
        };
        e.e.motion = [m(0), m(1), m(2)];
        e.e.fall_distance = t.float("FallDistance").unwrap_or(0.0) as f32;
        e.e.fire = t.int("Fire").unwrap_or(0) as i32;
        e.age = t.int("Age").unwrap_or(0) as i32;
        e.health = (t.int("Health").unwrap_or(5) & 255) as i32;
        e.pickup_delay = 0;
        self.spawn(e);
    }

    pub fn encode_tiles(&self) -> Vec<String> {
        let enc = |s: &[Option<Stack>]| {
            s.iter()
                .map(|x| x.map_or("-".to_string(), |s| s.to_code()))
                .collect::<Vec<_>>()
                .join(";")
        };
        self.tiles
            .iter()
            .map(|(p, t)| match t {
                Tile::Furnace {
                    slots,
                    burn,
                    item_burn,
                    cook,
                } => format!(
                    "F,{},{},{},{},{},{},{}",
                    p[0],
                    p[1],
                    p[2],
                    burn,
                    item_burn,
                    cook,
                    enc(slots)
                ),
                Tile::Chest { slots } => format!("C,{},{},{},{}", p[0], p[1], p[2], enc(slots)),
                Tile::Dispenser { slots } => format!("D,{},{},{},{}", p[0], p[1], p[2], enc(slots)),
                Tile::Spawner { mob } => format!("S,{},{},{},{}", p[0], p[1], p[2], mob),
                Tile::Sign { text, .. } => {
                    format!("G,{},{},{},{}", p[0], p[1], p[2], text.join("\u{1f}"))
                }
                Tile::Brewing {
                    slots,
                    brew_time,
                    ingredient,
                    ..
                } => {
                    format!(
                        "B,{},{},{},{},{},{}",
                        p[0],
                        p[1],
                        p[2],
                        brew_time,
                        ingredient,
                        enc(slots)
                    )
                }
            })
            .collect()
    }

    pub fn decode_tile(&mut self, line: &str) -> Option<()> {
        let f: Vec<&str> = line.splitn(8, ',').collect();
        let dec = |s: &str, n: usize| -> Option<Vec<Option<Stack>>> {
            let v: Vec<Option<Stack>> = s
                .split(';')
                .map(|x| {
                    if x == "-" {
                        Some(None)
                    } else {
                        Some(Some(Stack::from_code(x)?))
                    }
                })
                .collect::<Option<Vec<_>>>()?;
            if v.len() == n {
                Some(v)
            } else {
                None
            }
        };
        let pos = |a: &str, b: &str, c: &str| -> Option<Pos> {
            Some([a.parse().ok()?, b.parse().ok()?, c.parse().ok()?])
        };
        match *f.first()? {
            "G" => {
                let f: Vec<&str> = line.splitn(5, ',').collect();
                let mut text: [String; 4] = Default::default();
                for (i, l) in f.get(4)?.split('\u{1f}').take(4).enumerate() {
                    text[i] = l.to_string();
                }
                self.tiles.insert(
                    pos(f[1], f[2], f[3])?,
                    Tile::Sign {
                        text,
                        editable: false,
                    },
                );
            }
            "F" if f.len() == 8 => {
                let s = dec(f[7], 3)?;
                self.tiles.insert(
                    pos(f[1], f[2], f[3])?,
                    Tile::Furnace {
                        slots: [s[0], s[1], s[2]],
                        burn: f[4].parse().ok()?,
                        item_burn: f[5].parse().ok()?,
                        cook: f[6].parse().ok()?,
                    },
                );
            }
            "B" => {
                let f: Vec<&str> = line.splitn(7, ',').collect();
                let v = dec(f.get(6)?, 4)?;
                self.tiles.insert(
                    pos(f[1], f[2], f[3])?,
                    Tile::Brewing {
                        slots: [v[0], v[1], v[2], v[3]],
                        brew_time: f[4].parse().ok()?,
                        filled: 0,
                        ingredient: f[5].parse().ok()?,
                    },
                );
            }
            "C" => {
                let f: Vec<&str> = line.splitn(5, ',').collect();
                let s = dec(f.get(4)?, 27)?;
                let mut slots = [None; 27];
                slots.copy_from_slice(&s);
                self.tiles
                    .insert(pos(f[1], f[2], f[3])?, Tile::Chest { slots });
            }
            "D" => {
                let f: Vec<&str> = line.splitn(5, ',').collect();
                let s = dec(f.get(4)?, 9)?;
                let mut slots = [None; 9];
                slots.copy_from_slice(&s);
                self.tiles
                    .insert(pos(f[1], f[2], f[3])?, Tile::Dispenser { slots });
            }
            "S" => {
                let f: Vec<&str> = line.splitn(5, ',').collect();
                self.tiles.insert(
                    pos(f[1], f[2], f[3])?,
                    Tile::Spawner {
                        mob: f.get(4)?.to_string(),
                    },
                );
            }
            _ => return None,
        }
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn furnace_smelts_iron_with_coal() {
        let mut slots = [
            Some(Stack::new(block::IRON_ORE as ItemId, 0, 2)),
            Some(Stack::new(item::COAL, 0, 1)),
            None,
        ];
        let mut p = Play::new(GameMode::Survival);
        p.tiles.insert(
            [0, 64, 0],
            Tile::Furnace {
                slots,
                burn: 0,
                item_burn: 0,
                cook: 0,
            },
        );
        let mut w = ServerWorld::new(std::sync::Arc::new(crate::world::ChunkProvider::new(0)));
        w.insert_raw(crate::world::ChunkData {
            cx: 0,
            cz: 0,
            blocks: crate::world::gen::chunk_provider::boxed(),
            meta: crate::world::gen::chunk_provider::boxed(),
            biomes: [1; 256],
            sky: crate::world::gen::chunk_provider::boxed(),
            blk: crate::world::gen::chunk_provider::boxed(),
        });
        w.set_block(0, 64, 0, block::FURNACE, 2, 3);
        for _ in 0..400 {
            p.tick(&mut w);
        }
        if let Some(Tile::Furnace { slots: s, .. }) = p.tiles.get(&[0, 64, 0]) {
            slots = *s;
        }
        assert_eq!(slots[2], Some(Stack::new(item::IRON_INGOT, 0, 2)));
        assert!(slots[0].is_none());
        assert!(slots[1].is_none());
    }

    #[test]
    fn burn_times() {
        assert_eq!(burn_time(&Stack::new(item::COAL, 0, 1)), 1600);
        assert_eq!(burn_time(&Stack::new(block::PLANKS as ItemId, 0, 1)), 300);
        assert_eq!(burn_time(&Stack::new(item::STICK, 0, 1)), 100);
        assert_eq!(burn_time(&Stack::new(270, 0, 1)), 200);
        assert_eq!(burn_time(&Stack::new(block::STONE as ItemId, 0, 1)), 0);
    }
}
