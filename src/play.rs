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
    /// `TileEntityDispenser`; `TileEntityDropper` when `dropper`
    Dispenser {
        slots: [Option<Stack>; 9],
        dropper: bool,
    },
    /// `TileEntityComparator.outputSignal` (the live value is
    /// `ServerWorld::comparators`; this copy is what gets saved).
    Comparator { output: i32 },
    /// `BlockJukebox$TileEntityJukebox`: the record in it.
    Jukebox { record: Option<Stack> },
    /// `TileEntityDaylightDetector` (no data; the live set is
    /// `ServerWorld::daylight_detectors`).
    Daylight,
    /// `TileEntityHopper`
    Hopper {
        slots: [Option<Stack>; 5],
        /// `transferCooldown`
        cooldown: i32,
    },
    /// `TileEntityMobSpawner` (`MobSpawnerBaseLogic.mobID`)
    Spawner { mob: String },
    /// `TileEntitySign.signText`; `editable` until the placer's update.
    Sign { text: [String; 4], editable: bool },
    /// `TileEntitySkull`: `SkullType` and `Rot` (sixteenths, on the floor).
    Skull { kind: i32, rot: i32 },
    /// `TileEntityBeacon`: the pyramid's levels, the chosen powers, whether
    /// the sky is open (`isComplete`) and the payment slot.
    Beacon {
        levels: i32,
        primary: i32,
        secondary: i32,
        complete: bool,
        payment: Option<Stack>,
    },
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
    /// `InventoryEnderChest` through the ender chest at this place.
    Ender(Pos),
    /// An `EntityMinecartChest` (`displayGUIChest`).
    Cart(u32),
    /// A dispenser or dropper (`TileEntityDispenser`).
    Dispenser(Pos),
    /// `ContainerHopper`
    Hopper(Pos),
    /// `ContainerEnchantment` at the table
    Enchant(Pos),
    /// `ContainerRepair` at an anvil
    Anvil(Pos),
    /// `ContainerMerchant` with the villager of this id
    Merchant(u32),
    /// `ContainerHorseInventory` of the horse with this id: whether it
    /// carries a chest and can wear armour.
    Horse(u32, bool, bool),
    /// `ContainerBeacon`
    Beacon(Pos),
}

/// The `Container` class an open window uses.
pub fn window_of(o: &Open) -> Window {
    match o {
        Open::None | Open::Inventory => Window::Player,
        Open::Workbench(_) => Window::Workbench,
        Open::Furnace(_) => Window::Furnace,
        Open::Brewing(_) => Window::Brewing,
        Open::Chest(ps) => Window::Chest(ps.len() * 3),
        Open::Cart(_) | Open::Ender(_) => Window::Chest(3),
        Open::Dispenser(_) => Window::Dispenser,
        Open::Hopper(_) => Window::Hopper,
        Open::Enchant(_) => Window::Enchant,
        Open::Anvil(_) => Window::Repair,
        Open::Merchant(_) => Window::Merchant,
        Open::Horse(_, chest, armor) => Window::Horse {
            chest: *chest,
            armor: *armor,
        },
        Open::Beacon(_) => Window::Beacon,
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
    /// `MC|TrList`: the villager's offers, and the window's title (its
    /// custom name, empty for `entity.Villager.name`).
    pub offers: Vec<crate::merchant::Recipe>,
    pub merchant_name: String,
    /// `GuiMerchant.selectedMerchantRecipe`
    pub merchant_index: usize,
    /// The open dispenser window is a dropper's (`container.dropper`).
    pub dropper: bool,
    /// `ContainerBeacon`'s bars: levels, primary and secondary power.
    pub beacon: [i32; 3],
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
    /// The player's own random (`EntityPlayerMP.getRNG()`), lent by the
    /// server player while a dig or use runs, for `ItemStack.damageItem`.
    pub wear_rand: JavaRandom,
    /// Stacks that broke (`renderBrokenItemStack`) during that time.
    pub broke: Vec<Stack>,
    /// `EntityPlayer.canEat(false)` of the server player, and the
    /// `FoodStats.addStats` calls a dig or use made (cake slices).
    pub can_eat: bool,
    pub ate: Vec<(i32, f32)>,
    pub inv: Inventory,
    pub open: Open,
    pub mode: GameMode,
    pub items: Vec<ItemEntity>,
    /// `EntityFallingBlock`s
    pub falling: Vec<crate::entity::falling::FallingBlock>,
    /// `EntityFallingBlock.fall` this tick: the block's box, the damage
    /// and whether it is an anvil, for the owner of the other entities.
    pub falling_hits: Vec<(Aabb, f32, bool)>,
    /// Experience orbs to spawn (`dropXpOnBlockBreak`, `SlotFurnace`).
    pub orbs: Vec<([f64; 3], i32)>,
    /// Boats and minecarts with their trackers.
    pub vehicles: Vec<crate::entity::vehicle::Tracked>,
    pub next_item: u32,
    /// Sounds and particles of the item entities this tick.
    pub fx: Vec<crate::entity::base::Fx>,
    /// Items picked up this tick (`S0DPacketCollectItem`).
    pub collected: Vec<ItemView>,
    /// Items mobs picked up this tick, with the mob (`onItemPickup`).
    pub mob_collected: Vec<(ItemView, u32)>,
    /// The player is alive (`getHealth() > 0`).
    pub alive: bool,
    /// `rotationPitch` of the player.
    pub pitch: f32,
    /// `EntityPlayer.rand`
    pub prand: JavaRandom,
    pub tiles: BTreeMap<Pos, Tile>,
    /// The container slots last seen, to tell when a container changed
    /// (`TileEntity.markDirty` -> `World.func_147453_f`).
    pub(crate) slot_snap: std::collections::HashMap<Pos, Vec<Option<Stack>>>,
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
    /// `InventoryMerchant`: two prices and the goods, the selected offer,
    /// the villager's offers as last read, and what the last click did
    /// (`useRecipe` indexes, `verifySellingItem` results).
    pub merchant_slots: [Option<Stack>; 3],
    pub merchant_index: usize,
    pub offers: Vec<crate::merchant::Recipe>,
    pub merchant_name: String,
    pub merchant_fx: (Vec<usize>, Vec<bool>),
    /// `AnimalChest` of the horse whose window is open, as last read.
    pub horse_slots: Vec<Option<Stack>>,
    /// `EntityPlayer.theInventoryEnderChest`
    pub ender: [Option<Stack>; 27],
    pub repair_name: String,
    /// `experienceLevel` of the player, kept here for the containers.
    pub xp_level: i32,
    /// `S36PacketSignEditorOpen` to send, and sign texts that changed.
    pub sign_open: Option<Pos>,
    pub sign_updates: Vec<(Pos, [String; 4])>,
    /// Heads placed since the last tick (`S35PacketUpdateTileEntity` 4).
    pub skull_updates: Vec<(Pos, i32, i32)>,
    /// `addStat(dropStat)` calls since the last tick.
    pub drops_made: i32,
    /// `triggerAchievement` calls since the last tick.
    pub achievements: Vec<&'static str>,
    /// `addStat` calls since the last tick.
    pub stat_adds: Vec<(String, i32)>,
    /// Withers made by the last skull (position, yaw), and whether the
    /// difficulty is Peaceful.
    pub wither_spawns: Vec<([f64; 3], f32)>,
    pub difficulty_peaceful: bool,
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
            beacon: [0; 3],
            inv: Inventory::default(),
            open: Open::None,
            result: None,
            tile_slots: Vec::new(),
            furnace: None,
            clicks: 0,
            enchant: [0; 3],
            name_seed: 0,
            repair_cost: 0,
            offers: Vec::new(),
            merchant_name: String::new(),
            merchant_index: 0,
            dropper: false,
        }
    }
}

impl Play {
    pub fn new(mode: GameMode) -> Play {
        Play {
            wear_rand: JavaRandom::new(0),
            broke: Vec::new(),
            can_eat: false,
            ate: Vec::new(),
            inv: Inventory::default(),
            open: Open::None,
            mode,
            items: Vec::new(),
            falling: Vec::new(),
            vehicles: Vec::new(),
            orbs: Vec::new(),
            falling_hits: Vec::new(),
            mob_collected: Vec::new(),
            next_item: 1,
            pitch: 0.0,
            fx: Vec::new(),
            collected: Vec::new(),
            alive: true,
            prand: JavaRandom::new(0x706C6179),
            tiles: BTreeMap::new(),
            slot_snap: Default::default(),
            player: ([0.0; 3], 0.0),
            inv_dirty: true,
            drag: Drag::default(),
            clicks: 0,
            creative_drops: 0,
            sign_open: None,
            enchant_slot: [None],
            repair_slots: [None; 2],
            merchant_slots: [None; 3],
            merchant_index: 0,
            offers: Vec::new(),
            merchant_name: String::new(),
            merchant_fx: (Vec::new(), Vec::new()),
            horse_slots: Vec::new(),
            ender: [None; 27],
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
            skull_updates: Vec::new(),
            drops_made: 0,
            achievements: Vec::new(),
            stat_adds: Vec::new(),
            wither_spawns: Vec::new(),
            difficulty_peaceful: false,
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
            offers: if matches!(self.open, Open::Merchant(_)) {
                self.offers.clone()
            } else {
                Vec::new()
            },
            merchant_name: self.merchant_name.clone(),
            merchant_index: self.merchant_index,
            dropper: matches!(&self.open, Open::Dispenser(p)
                if matches!(self.tiles.get(p), Some(Tile::Dispenser { dropper: true, .. }))),
            beacon: match &self.open {
                Open::Beacon(p) => match self.tiles.get(p) {
                    Some(Tile::Beacon {
                        levels,
                        primary,
                        secondary,
                        ..
                    }) => [*levels, *primary, *secondary],
                    _ => [0; 3],
                },
                _ => [0; 3],
            },
        }
    }

    /// `BlockJukebox.removeRecord`: the record stops and pops out of the top.
    pub fn eject_record(&mut self, w: &mut ServerWorld, p: Pos) {
        let Some(Tile::Jukebox { record: Some(r) }) = self.tiles.get(&p).cloned() else {
            return;
        };
        w.redstone_effects
            .push(crate::world::redstone::Effect::Record(p, 0));
        self.tiles.insert(p, Tile::Jukebox { record: None });
        if w.block(p[0], p[1], p[2]) == block::JUKEBOX {
            w.set_meta(p[0], p[1], p[2], 0, 2);
        }
        let f = 0.7f32;
        let ox = (w.rand.next_float() * f) as f64 + ((1.0 - f) * 0.5) as f64;
        let oy = (w.rand.next_float() * f) as f64 + ((1.0 - f) as f64 * 0.2 + 0.6);
        let oz = (w.rand.next_float() * f) as f64 + ((1.0 - f) * 0.5) as f64;
        let id = self.next_item;
        self.next_item += 1;
        let mut e = ItemEntity::new(
            id,
            r,
            [p[0] as f64 + ox, p[1] as f64 + oy, p[2] as f64 + oz],
        );
        e.pickup_delay = 10;
        self.spawn(e);
    }

    pub(crate) fn spawn(&mut self, e: ItemEntity) {
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
                self.tiles.insert(
                    pos,
                    Tile::Dispenser {
                        slots: a,
                        dropper: false,
                    },
                );
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
            GenEntity::Xp { pos, amount } => self.orbs.push((pos, amount)),
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
        // ItemInWorldManager.onBlockClicked: a creative player harvests at
        // once, unbreakable blocks (bedrock, end portal frames) included;
        // in survival a hardness below 0 never finishes
        let unbreakable = crate::item::hardness(id) < 0.0 && self.mode != GameMode::Creative;
        if id == block::AIR || unbreakable || block::material(id).is_liquid() {
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
        // BlockBed.onBlockHarvested: in creative the head takes the foot
        // with it (or the foot would drop the bed)
        if id == block::BED && self.mode == GameMode::Creative && meta & 8 != 0 {
            let (dx, dz) = [(0, 1), (-1, 0), (0, -1), (1, 0)][(meta & 3) as usize];
            if w.block(p[0] - dx, p[1], p[2] - dz) == block::BED {
                w.set_air(p[0] - dx, p[1], p[2] - dz);
            }
        }
        // BlockDoor.onBlockHarvested: in creative the top takes the lower
        // half with it (or the lower half would drop the door)
        if (id == block::WOODEN_DOOR || id == block::IRON_DOOR)
            && self.mode == GameMode::Creative
            && meta & 8 != 0
            && w.block(p[0], p[1] - 1, p[2]) == id
        {
            w.set_air(p[0], p[1] - 1, p[2]);
        }
        // BlockPistonExtension.onBlockHarvested: in creative the head takes
        // its piston
        if id == block::PISTON_HEAD && self.mode == GameMode::Creative {
            let d = [[0, -1, 0], [0, 1, 0], [0, 0, -1], [0, 0, 1], [-1, 0, 0], [1, 0, 0]]
                [(meta & 7).min(5) as usize];
            let b = [p[0] - d[0], p[1] - d[1], p[2] - d[2]];
            let base = w.block(b[0], b[1], b[2]);
            if base == block::PISTON || base == block::STICKY_PISTON {
                w.set_air(b[0], b[1], b[2]);
            }
        }
        // BlockTripWire.onBlockHarvested: shears disarm the wire first
        if id == block::TRIPWIRE && held.is_some_and(|s| s.id == 359) {
            w.set_meta(p[0], p[1], p[2], meta | 8, 4);
        }
        // BlockJukebox.breakBlock: removeRecord
        if id == block::JUKEBOX {
            self.eject_record(w, p);
        }
        // Container contents spill out.
        if let Some(t) = self.tiles.remove(&p) {
            let slots: Vec<Option<Stack>> = match t {
                Tile::Furnace { slots, .. } => slots.to_vec(),
                Tile::Chest { slots } => slots.to_vec(),
                Tile::Dispenser { slots, .. } => slots.to_vec(),
                Tile::Hopper { slots, .. } => slots.to_vec(),
                Tile::Brewing { slots, .. } => slots.to_vec(),
                // BlockBeacon keeps no drops of its payment
                Tile::Beacon { .. } => Vec::new(),
                // BlockSkull.getDrops: the head of its kind (not after a
                // creative player's hit, onBlockHarvested's bit 8)
                Tile::Skull { kind, .. } => {
                    if self.mode == GameMode::Creative {
                        Vec::new()
                    } else {
                        vec![Some(Stack::new(397, kind, 1))]
                    }
                }
                Tile::Spawner { .. }
                | Tile::Sign { .. }
                | Tile::Comparator { .. }
                | Tile::Daylight
                | Tile::Jukebox { .. } => Vec::new(),
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
                // Block.harvestBlock
                let ench = held.map(|s| s.enchantments()).unwrap_or_default();
                let level = |e: i32| ench.iter().find(|x| x.0 == e).map_or(0, |x| x.1);
                if id == block::SNOW_LAYER {
                    // BlockSnow.harvestBlock: the layers as snowballs
                    for s in item::drops(id, meta, &mut rnd) {
                        self.drop_at_block(s, p, &mut rnd);
                    }
                } else {
                    // Silk Touch (33) takes the block itself; otherwise
                    // dropBlockAsItem with the Fortune (35) level
                    let silk = crate::drops::can_silk_harvest(id) && level(33) > 0;
                    let v = if silk {
                        match crate::drops::create_stacked_block(id, meta) {
                            Some(s) => crate::drops::drop_stack(&mut rnd, s),
                            None => Vec::new(),
                        }
                    } else {
                        crate::drops::drop_block_as_item_with_chance(
                            id,
                            meta,
                            1.0,
                            level(35),
                            &mut rnd,
                        )
                    };
                    w.spawn_drops(v, p[0], p[1], p[2]);
                }
            }
            w.rand = rnd;
            // Item.onBlockDestroyed: ItemTool 1 and ItemSword 2 for a block
            // of non-zero hardness (ItemHoe is neither), ItemShears 1 for
            // leaves, cobweb, grass, vines and tripwire
            if let Some(s) = held {
                let cost = match item::tool(s.id).map(|t| t.kind) {
                    Some(item::ToolKind::Sword) if item::hardness(id) != 0.0 => 2,
                    Some(item::ToolKind::Hoe) | None => 0,
                    Some(_) if item::hardness(id) != 0.0 => 1,
                    _ => 0,
                };
                let cost = if s.id == 359 {
                    match block::material(id) {
                        block::Material::Leaves => 1,
                        _ if matches!(
                            id,
                            block::WEB | block::TALLGRASS | block::VINE | block::TRIPWIRE
                        ) =>
                        {
                            1
                        }
                        _ => 0,
                    }
                } else {
                    cost
                };
                if cost > 0 {
                    self.damage_held(cost);
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
        hit: [f32; 3],
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
                block::ENDER_CHEST => {
                    // BlockEnderChest.onBlockActivated: not under a normal cube
                    if !block::info(w.block(target[0], target[1] + 1, target[2])).normal_cube() {
                        self.open = Open::Ender(target);
                        self.inv_dirty = true;
                    }
                    return true;
                }
                block::CHEST | block::TRAPPED_CHEST => {
                    // BlockChest.func_149951_m: the north/west half comes first.
                    let cid = w.block(target[0], target[1], target[2]);
                    let mut halves = vec![target];
                    for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                        let q = [target[0] + dx, target[1], target[2] + dz];
                        if w.block(q[0], q[1], q[2]) == cid {
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
                block::DISPENSER | block::DROPPER => {
                    // BlockDispenser.onBlockActivated: displayGUIDispenser
                    self.tiles.entry(target).or_insert(Tile::Dispenser {
                        slots: [None; 9],
                        dropper: tid == block::DROPPER,
                    });
                    self.open = Open::Dispenser(target);
                    self.inv_dirty = true;
                    return true;
                }
                block::HOPPER => {
                    // BlockHopper.onBlockActivated: openHopperMenu
                    self.tiles.entry(target).or_insert(Tile::Hopper {
                        slots: [None; 5],
                        cooldown: -1,
                    });
                    self.open = Open::Hopper(target);
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
                block::BEACON => {
                    // BlockBeacon.onBlockActivated: openBeaconMenu
                    self.tiles.entry(target).or_insert_with(Tile::beacon);
                    self.open = Open::Beacon(target);
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
                // BlockJukebox.onBlockActivated, else ItemRecord.onItemUse
                block::JUKEBOX => {
                    if w.meta(target[0], target[1], target[2]) != 0 {
                        self.eject_record(w, target);
                        return true;
                    }
                    if let Some(s) = self.inv.held().filter(|s| (2256..=2267).contains(&s.id)) {
                        self.tiles.insert(
                            target,
                            Tile::Jukebox {
                                record: Some(Stack { count: 1, ..s }),
                            },
                        );
                        w.set_meta(target[0], target[1], target[2], 1, 2);
                        w.redstone_effects
                            .push(crate::world::redstone::Effect::Record(target, s.id as i32));
                        if self.mode == GameMode::Survival {
                            self.inv.consume_held();
                            self.inv_dirty = true;
                        }
                        return true;
                    }
                }
                block::UNPOWERED_COMPARATOR | block::POWERED_COMPARATOR => {
                    // BlockRedstoneComparator.onBlockActivated
                    w.comparator_toggle(target[0], target[1], target[2]);
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
                block::FENCE_GATE => {
                    // BlockFenceGate.onBlockActivated: opens away from the
                    // player
                    let [x, y, z] = target;
                    let mut m = w.meta(x, y, z);
                    if m & 4 != 0 {
                        w.set_meta(x, y, z, m & !4, 2);
                    } else {
                        let face = ((self.player.1 * 4.0 / 360.0) as f64 + 0.5).floor() as i32 & 3;
                        if m & 3 == (face + 2) % 4 {
                            m = face;
                        }
                        w.set_meta(x, y, z, m | 4, 2);
                    }
                    w.door_sound(x, y, z);
                    return true;
                }
                block::CAKE => {
                    // BlockCake.onBlockActivated -> func_150036_b
                    self.eat_cake(w, target);
                    return true;
                }
                block::DISPENSER | block::DROPPER => {
                    self.tiles.entry(target).or_insert(Tile::Dispenser {
                        slots: [None; 9],
                        dropper: tid == block::DROPPER,
                    });
                    return true;
                }
                block::HOPPER => {
                    self.tiles.entry(target).or_insert(Tile::Hopper {
                        slots: [None; 5],
                        cooldown: -1,
                    });
                    return true;
                }
                block::BEACON => {
                    self.tiles.entry(target).or_insert_with(Tile::beacon);
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
        if held.id == 397 {
            return self.place_skull(w, target, face, held);
        }
        // ItemFireball.onItemUse: fire on the clicked side, the charge used up
        if held.id == 385 {
            let p = [target[0] + face[0], target[1] + face[1], target[2] + face[2]];
            if w.is_air(p[0], p[1], p[2]) {
                let pitch = (w.rand.next_float() - w.rand.next_float()) * 0.2 + 1.0;
                w.redstone_effects.push(crate::world::redstone::Effect::Sound(
                    "fire.ignite",
                    [p[0] as f64 + 0.5, p[1] as f64 + 0.5, p[2] as f64 + 0.5],
                    1.0,
                    pitch,
                ));
                w.set_block(p[0], p[1], p[2], block::FIRE, 0, 3);
            }
            if self.mode != GameMode::Creative {
                self.inv.consume_held();
                self.inv_dirty = true;
            }
            return true;
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
        let side = match face {
            [0, -1, 0] => 0,
            [0, 1, 0] => 1,
            [0, 0, -1] => 2,
            [0, 0, 1] => 3,
            [-1, 0, 0] => 4,
            _ => 5,
        };
        let [tx, ty, tz] = target;
        // ItemHoe.onItemUse
        if (290..=294).contains(&held.id) {
            if side != 0
                && w.material(tx, ty + 1, tz) == block::Material::Air
                && (tid == block::GRASS || tid == block::DIRT)
            {
                // Blocks.farmland.stepSound (gravel), as ItemBlock plays it
                let st = block::step_sound(block::FARMLAND);
                w.redstone_effects
                    .push(crate::world::redstone::Effect::Sound(
                        st.step,
                        [tx as f64 + 0.5, ty as f64 + 0.5, tz as f64 + 0.5],
                        (st.volume + 1.0) / 2.0,
                        st.pitch * 0.8,
                    ));
                w.set_block(tx, ty, tz, block::FARMLAND, 0, 3);
                self.damage_held(1);
                return true;
            }
            return false;
        }
        if held.id == 355 {
            // ItemBed.onItemUse: the foot above the clicked top, the head
            // one block the way the player looks
            if side != 1 {
                return false;
            }
            let y = ty + 1;
            let d = ((self.player.1 * 4.0 / 360.0) as f64 + 0.5).floor() as i32 & 3;
            let (dx, dz) = [(0, 1), (-1, 0), (0, -1), (1, 0)][d as usize];
            if w.is_air(tx, y, tz)
                && w.is_air(tx + dx, y, tz + dz)
                && w.solid_top_surface(tx, y - 1, tz)
                && w.solid_top_surface(tx + dx, y - 1, tz + dz)
            {
                w.set_block(tx, y, tz, block::BED, d, 3);
                if w.block(tx, y, tz) == block::BED {
                    w.set_block(tx + dx, y, tz + dz, block::BED, d + 8, 3);
                }
                if self.mode == GameMode::Survival {
                    self.inv.consume_held();
                    self.inv_dirty = true;
                }
                return true;
            }
            return false;
        }
        // ItemSeeds / ItemSeedFood: the crop on top of its soil
        let seed = match held.id {
            295 => Some((block::WHEAT, block::FARMLAND)),
            361 => Some((block::PUMPKIN_STEM, block::FARMLAND)),
            362 => Some((block::MELON_STEM, block::FARMLAND)),
            372 => Some((block::NETHER_WART, block::SOUL_SAND)),
            391 => Some((block::CARROTS, block::FARMLAND)),
            392 => Some((block::POTATOES, block::FARMLAND)),
            _ => None,
        };
        if let Some((crop, soil)) = seed {
            if side != 1 {
                return false;
            }
            if tid == soil && w.is_air(tx, ty + 1, tz) {
                w.set_block(tx, ty + 1, tz, crop, 0, 3);
                if self.mode == GameMode::Survival {
                    self.inv.consume_held();
                    self.inv_dirty = true;
                }
                return true;
            }
            return false;
        }
        if held.id == 351 {
            // ItemDye.onItemUse
            if held.meta == 15 {
                if w.bone_meal(tx, ty, tz) {
                    // playAuxSFX(2005): ItemDye.func_150918_a on the client
                    w.redstone_effects
                        .push(crate::world::redstone::Effect::BoneMeal([tx, ty, tz]));
                    if self.mode == GameMode::Survival {
                        self.inv.consume_held();
                        self.inv_dirty = true;
                    }
                    return true;
                }
            } else if held.meta == 3 {
                // cocoa beans on the side of a jungle log
                if tid == block::LOG && w.meta(tx, ty, tz) & 3 == 3 {
                    if side < 2 {
                        return false;
                    }
                    let p = [tx + face[0], ty, tz + face[2]];
                    if w.is_air(p[0], p[1], p[2]) {
                        // BlockCocoa.onBlockPlaced: rotateOpposite[facingToDirection[side]]
                        const FACING_TO_DIR: [i32; 6] = [-1, -1, 2, 0, 1, 3];
                        let d = (FACING_TO_DIR[side as usize] + 2) & 3;
                        w.set_block(p[0], p[1], p[2], block::COCOA, d, 2);
                        if self.mode == GameMode::Survival {
                            self.inv.consume_held();
                            self.inv_dirty = true;
                        }
                    }
                    return true;
                }
            }
            return false;
        }
        let id = match held.id {
            331 => block::REDSTONE_WIRE,
            356 => block::UNPOWERED_REPEATER,
            // ItemReed(Blocks.unpowered_comparator)
            404 => block::UNPOWERED_COMPARATOR,
            // ItemReed for the brewing stand, the cauldron and the cake
            379 => block::BREWING_STAND,
            380 => block::CAULDRON,
            354 => block::CAKE,
            _ => match held.block() {
                Some(id) => id,
                None => return false,
            },
        };
        // ItemSlab.onItemUse: a slab on the matching half makes the double
        // slab, on the clicked block or on the one beside it (func_150946_a)
        if id == block::STONE_SLAB || id == block::WOODEN_SLAB {
            let double = if id == block::STONE_SLAB {
                block::DOUBLE_STONE_SLAB
            } else {
                block::DOUBLE_WOODEN_SLAB
            };
            let kind = held.meta & 7;
            let tm = w.meta(target[0], target[1], target[2]);
            let top = tm & 8 != 0;
            let beside = [
                target[0] + face[0],
                target[1] + face[1],
                target[2] + face[2],
            ];
            let at = if (face == [0, 1, 0] && !top || face == [0, -1, 0] && top)
                && tid == id
                && tm & 7 == kind
            {
                Some(target)
            } else if w.block(beside[0], beside[1], beside[2]) == id
                && w.meta(beside[0], beside[1], beside[2]) & 7 == kind
            {
                Some(beside)
            } else {
                None
            };
            if let Some(q) = at {
                // checkNoEntityCollision with the full block
                let ([px, py, pz], _) = self.player;
                let pbb = Aabb {
                    min: [px - 0.3, py, pz - 0.3],
                    max: [px + 0.3, py + 1.8, pz + 0.3],
                };
                let bb = Aabb {
                    min: [q[0] as f64, q[1] as f64, q[2] as f64],
                    max: [q[0] as f64 + 1.0, q[1] as f64 + 1.0, q[2] as f64 + 1.0],
                };
                if !bb.intersects(&pbb) && w.set_block(q[0], q[1], q[2], double, kind, 3) {
                    if self.mode == GameMode::Survival {
                        self.inv.consume_held();
                        self.inv_dirty = true;
                    }
                }
                return true;
            }
        }
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
            block::FENCE_GATE => {
                // BlockFenceGate.canPlaceBlockAt + onBlockPlacedBy
                if !w.material(p[0], p[1] - 1, p[2]).is_solid() {
                    return false;
                }
                meta = yaw4;
            }
            block::CAKE => {
                // BlockCake.canPlaceBlockAt: canBlockStay
                if !w.material(p[0], p[1] - 1, p[2]).is_solid() {
                    return false;
                }
                meta = 0;
            }
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
                // BlockTrapDoor.onBlockPlaced: the upper half of the side
                if hit[1] > 0.5 {
                    meta |= 8;
                }
            }
            // BlockStairs.onBlockPlaced (upside down under a ceiling or on
            // the upper half of a side) and onBlockPlacedBy (facing)
            id if block::is_stairs(id) => {
                let k = if side != 0 && (side == 1 || hit[1] as f64 <= 0.5) {
                    0
                } else {
                    4
                };
                meta = [2, 1, 3, 0][yaw4 as usize] | k;
            }
            // BlockLadder.onBlockPlaced
            block::LADDER => {
                let mut j = 0;
                if (j == 0 || side == 2) && nc(w, p[0], p[1], p[2] + 1) {
                    j = 2;
                }
                if (j == 0 || side == 3) && nc(w, p[0], p[1], p[2] - 1) {
                    j = 3;
                }
                if (j == 0 || side == 4) && nc(w, p[0] + 1, p[1], p[2]) {
                    j = 4;
                }
                if (j == 0 || side == 5) && nc(w, p[0] - 1, p[1], p[2]) {
                    j = 5;
                }
                if j == 0 {
                    // canPlaceBlockAt: needs a block behind it
                    return false;
                }
                meta = j;
            }
            // BlockPumpkin.onBlockPlacedBy
            block::PUMPKIN | block::LIT_PUMPKIN => {
                meta = ((self.player.1 * 4.0 / 360.0) as f64 + 2.5).floor() as i32 & 3;
            }
            // BlockVine.onBlockPlaced
            block::VINE => {
                meta = match side {
                    2 => 1,
                    3 => 4,
                    4 => 8,
                    5 => 2,
                    _ => meta,
                };
            }
            // BlockTripWireHook.onBlockPlaced
            block::TRIPWIRE_HOOK => {
                meta = match side {
                    2 if nc(w, p[0], p[1], p[2] + 1) => 2,
                    3 if nc(w, p[0], p[1], p[2] - 1) => 0,
                    4 if nc(w, p[0] + 1, p[1], p[2]) => 1,
                    5 if nc(w, p[0] - 1, p[1], p[2]) => 3,
                    _ => 0,
                };
            }
            // BlockQuartz.onBlockPlaced: the pillar lies along the clicked axis
            block::QUARTZ_BLOCK => {
                if meta == 2 {
                    meta = match side {
                        2 | 3 => 4,
                        4 | 5 => 3,
                        _ => 2,
                    };
                }
            }
            block::STONE_PRESSURE_PLATE
            | block::WOODEN_PRESSURE_PLATE
            | block::LIGHT_WEIGHTED_PRESSURE_PLATE
            | block::HEAVY_WEIGHTED_PRESSURE_PLATE => {
                if !w.solid_top_surface(p[0], p[1] - 1, p[2])
                    && !matches!(
                        w.block(p[0], p[1] - 1, p[2]),
                        block::FENCE | block::NETHER_BRICK_FENCE
                    )
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
            block::UNPOWERED_REPEATER | block::UNPOWERED_COMPARATOR => {
                if !w.solid_top_surface(p[0], p[1] - 1, p[2]) {
                    return false;
                }
                meta = (yaw4 + 2) % 4;
            }
            block::PISTON | block::STICKY_PISTON | block::DISPENSER | block::DROPPER => {
                meta = self.orientation(p);
            }
            // BlockHopper.onBlockPlaced: away from the clicked face, never up
            block::HOPPER => {
                let side = match face {
                    [0, -1, 0] => 0,
                    [0, 1, 0] => 1,
                    [0, 0, -1] => 2,
                    [0, 0, 1] => 3,
                    [-1, 0, 0] => 4,
                    _ => 5,
                };
                let opp = [1, 0, 3, 2, 5, 4][side];
                meta = if opp == 1 { 0 } else { opp };
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
            block::FURNACE | block::ENDER_CHEST => meta = facing_meta(self.player.1),
            block::CHEST | block::TRAPPED_CHEST => meta = facing_meta(self.player.1),
            // ItemAnvilBlock.getMetadata (damage << 2), then
            // BlockAnvil.onBlockPlacedBy
            block::ANVIL => {
                let k = (yaw4 + 1) % 4;
                meta = [2, 3, 0, 1][k as usize] | (meta << 2) & 12;
            }
            block::LOG | block::LOG2 | block::HAY_BLOCK => {
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
            // BlockSlab.onBlockPlaced: the upper half under a ceiling or
            // on the upper half of a side
            block::STONE_SLAB | block::WOODEN_SLAB => {
                meta &= 7;
                if !(side != 0 && (side == 1 || hit[1] as f64 <= 0.5)) {
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
        // World.canPlaceEntityOnSide: the block's own collision box
        if let Some(c) = block::collision_box(id, meta as u8) {
            let bb = Aabb {
                min: [
                    p[0] as f64 + c[0] as f64,
                    p[1] as f64 + c[1] as f64,
                    p[2] as f64 + c[2] as f64,
                ],
                max: [
                    p[0] as f64 + c[3] as f64,
                    p[1] as f64 + c[4] as f64,
                    p[2] as f64 + c[5] as f64,
                ],
            };
            if bb.intersects(&pbb) {
                return false;
            }
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
                block::DISPENSER | block::DROPPER => {
                    w.set_meta(p[0], p[1], p[2], meta, 2);
                }
                block::UNPOWERED_REPEATER => w.repeater_placed(p[0], p[1], p[2]),
                block::UNPOWERED_COMPARATOR => w.comparator_placed(p[0], p[1], p[2]),
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

    /// `BlockCake.func_150036_b`: a slice when the player may eat.
    fn eat_cake(&mut self, w: &mut ServerWorld, p: Pos) {
        if self.can_eat {
            self.ate.push((2, 0.1));
            self.can_eat = false;
            let m = w.meta(p[0], p[1], p[2]) + 1;
            if m >= 6 {
                w.set_air(p[0], p[1], p[2]);
            } else {
                w.set_meta(p[0], p[1], p[2], m, 2);
            }
        }
    }

    /// `ItemInWorldManager.onBlockClicked` in survival: a fire on the
    /// clicked side goes out (`World.extinguishFire`), then
    /// `Block.onBlockClicked` (cake, note block).
    pub fn click_block(&mut self, w: &mut ServerWorld, p: Pos, side: i32) {
        const OFF: [[i32; 3]; 6] = [
            [0, -1, 0],
            [0, 1, 0],
            [0, 0, -1],
            [0, 0, 1],
            [-1, 0, 0],
            [1, 0, 0],
        ];
        if (0..6).contains(&side) {
            let o = OFF[side as usize];
            let f = [p[0] + o[0], p[1] + o[1], p[2] + o[2]];
            if w.block(f[0], f[1], f[2]) == block::FIRE {
                // playAuxSFXAtEntity(player, 1004): random.fizz
                let r = || crate::util::math::math_random() as f32;
                let pitch = 2.6 + (r() - r()) * 0.8;
                w.redstone_effects
                    .push(crate::world::redstone::Effect::Sound(
                        "random.fizz",
                        [f[0] as f64 + 0.5, f[1] as f64 + 0.5, f[2] as f64 + 0.5],
                        0.5,
                        pitch,
                    ));
                w.set_air(f[0], f[1], f[2]);
            }
        }
        match w.block(p[0], p[1], p[2]) {
            block::CAKE => self.eat_cake(w, p),
            block::NOTEBLOCK => w.note_trigger(p[0], p[1], p[2]),
            _ => {}
        }
    }

    /// Wears the held item by `n` uses (`ItemStack.damageItem` with the
    /// player's random, so Unbreaking can spare it).
    fn damage_held(&mut self, n: i32) {
        let i = self.inv.selected;
        if let Some(s) = self.inv.main[i] {
            let creative = self.mode == GameMode::Creative;
            let (left, broke) = crate::enchant::damage_item(s, n, creative, &mut self.wear_rand);
            if broke {
                self.broke.push(s);
            }
            self.inv.main[i] = left;
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

    /// `ItemSkull.onItemUse`: on top of or beside a solid block, the head
    /// turned with the player on the floor.
    fn place_skull(&mut self, w: &mut ServerWorld, t: Pos, face: [i32; 3], held: Stack) -> bool {
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
        w.set_block(p[0], p[1], p[2], block::SKULL, side, 2);
        let rot = if side == 1 {
            ((self.player.1 * 16.0 / 360.0) as f64 + 0.5).floor() as i32 & 15
        } else {
            0
        };
        let kind = held.meta.clamp(0, 4);
        self.tiles.insert(p, Tile::Skull { kind, rot });
        self.skull_updates.push((p, kind, rot));
        // BlockSkull.makeWither (not on Peaceful)
        if kind == 1 && self.difficulty_peaceful == false {
            let tiles = &self.tiles;
            let shape = {
                let at = |x: i32, y: i32, z: i32| w.peek_block(x, y, z);
                let skull = |x: i32, y: i32, z: i32| {
                    w.peek_block(x, y, z) == block::SKULL
                        && matches!(tiles.get(&[x, y, z]), Some(Tile::Skull { kind: 1, .. }))
                };
                crate::entity::mob::wither::summon_shape(&at, &skull, p)
            };
            if let Some((clear, at, yaw)) = shape {
                for c in &clear {
                    self.tiles.remove(c);
                    w.set_block(c[0], c[1], c[2], block::AIR, 0, 2);
                }
                self.wither_spawns.push((at, yaw));
            }
        }
        if self.mode != GameMode::Creative {
            self.inv.consume_held();
        }
        self.inv_dirty = true;
        true
    }

    /// The heads in chunk (cx, cz) for the client.
    pub fn skulls_in(&self, cx: i32, cz: i32) -> Vec<(Pos, i32, i32)> {
        self.tiles
            .iter()
            .filter(|(p, _)| p[0] >> 4 == cx && p[2] >> 4 == cz)
            .filter_map(|(p, t)| match t {
                Tile::Skull { kind, rot } => Some((*p, *kind, *rot)),
                _ => None,
            })
            .collect()
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
                Some(Tile::Dispenser { slots, .. }) => slots.to_vec(),
                _ => Vec::new(),
            },
            Open::Hopper(p) => match self.tiles.get(p) {
                Some(Tile::Hopper { slots, .. }) => slots.to_vec(),
                _ => Vec::new(),
            },
            Open::Beacon(p) => match self.tiles.get(p) {
                Some(Tile::Beacon { payment, .. }) => vec![*payment],
                _ => vec![None],
            },
            Open::Enchant(_) => self.enchant_slot.to_vec(),
            Open::Anvil(_) => self.repair_slots.to_vec(),
            Open::Merchant(_) => self.merchant_slots.to_vec(),
            Open::Horse(..) => self.horse_slots.clone(),
            Open::Ender(_) => self.ender.to_vec(),
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
                if let Some(Tile::Dispenser { slots, .. }) = self.tiles.get_mut(&p) {
                    copy(slots, v);
                }
            }
            Open::Hopper(p) => {
                if let Some(Tile::Hopper { slots, .. }) = self.tiles.get_mut(&p) {
                    copy(slots, v);
                }
            }
            Open::Beacon(p) => {
                if let Some(Tile::Beacon { payment, .. }) = self.tiles.get_mut(&p) {
                    *payment = v.first().copied().flatten();
                }
            }
            Open::Enchant(_) => copy(&mut self.enchant_slot, v),
            Open::Anvil(_) => copy(&mut self.repair_slots, v),
            Open::Merchant(_) => copy(&mut self.merchant_slots, v),
            Open::Horse(..) => copy(&mut self.horse_slots, v),
            Open::Ender(_) => copy(&mut self.ender, v),
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
                .with_repair(&self.repair_name, self.xp_level)
                .with_merchant(&self.offers, self.merchant_index);
            container::slot_click(&mut c, &mut drag, slot, button, mode);
            c.fx
        };
        self.merchant_fx.0.extend(fx.traded.iter().copied());
        self.merchant_fx.1.extend(fx.verified.iter().copied());
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
        // SlotCrafting.onCrafting / SlotFurnace.onCrafting /
        // ContainerBrewingStand$Potion: their achievements
        // ItemStack.onCrafting: addStat(objectCraftStats, amount)
        for (s, n) in fx.crafted.iter().chain(fx.smelted.iter()) {
            self.stat_adds.push((format!("stat.craftItem.{}", s.id), (*n).max(s.count) as i32));
        }
        for (s, _) in &fx.crafted {
            let a = match s.id {
                58 => "achievement.buildWorkBench",
                270 => "achievement.buildPickaxe",
                61 => "achievement.buildFurnace",
                290 => "achievement.buildHoe",
                297 => "achievement.makeBread",
                354 => "achievement.bakeCake",
                274 => "achievement.buildBetterPickaxe",
                268 => "achievement.buildSword",
                116 => "achievement.enchantments",
                47 => "achievement.bookcase",
                _ => continue,
            };
            self.achievements.push(a);
        }
        for (s, _) in &fx.smelted {
            match s.id {
                265 => self.achievements.push("achievement.acquireIron"),
                350 => self.achievements.push("achievement.cookFish"),
                _ => {}
            }
        }
        if fx.brewed {
            self.achievements.push("achievement.potion");
        }
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

    /// `MC|TrSel`: `ContainerMerchant.setCurrentRecipeIndex`.
    pub fn select_offer(&mut self, index: usize) {
        if !matches!(self.open, Open::Merchant(_)) {
            return;
        }
        self.merchant_index = index;
        let mut inv = self.inv.clone();
        let mut tile = self.merchant_slots.to_vec();
        let creative = self.mode == GameMode::Creative;
        let fx = {
            let mut c = Ctx::new(&mut inv, &mut tile, Window::Merchant, creative)
                .with_merchant(&self.offers, index);
            c.set_offer_index(index);
            c.fx
        };
        for k in 0..3 {
            self.merchant_slots[k] = tile[k];
        }
        self.merchant_fx.1.extend(fx.verified);
        self.inv_dirty = true;
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
        // ContainerMerchant.onContainerClosed: the prices go back out
        for k in 0..2 {
            if let Some(s) = self.merchant_slots[k].take() {
                self.throw(s, &mut rnd);
            }
        }
        self.merchant_slots[2] = None;
        self.merchant_index = 0;
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
        self.drops_made += 1;
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

    /// `Minecraft.middleClickMouse` on a block: the item it is picked as
    /// (`Block.getItem(World..)`, `getDamageValue`), then
    /// `InventoryPlayer.setCurrentItem`: a hotbar slot already holding it
    /// is selected (in survival too); creative brings it to the hand.
    pub fn pick_block(&mut self, w: &ServerWorld, p: Pos) {
        let id = w.peek_block(p[0], p[1], p[2]);
        let meta = w.peek_meta(p[0], p[1], p[2]) as i32;
        let skull = match self.tiles.get(&p) {
            Some(Tile::Skull { kind, .. }) => Some(*kind as i32),
            _ => None,
        };
        let below = w.peek_meta(p[0], p[1] - 1, p[2]) as i32;
        let Some((item, dmg, subtypes)) = pick_item(id, meta, below, skull) else { return };
        self.set_current_item(item, dmg, subtypes);
    }

    /// `InventoryPlayer.setCurrentItem` and `creativePickItem`.
    pub fn set_current_item(&mut self, item: ItemId, dmg: i32, subtypes: bool) {
        let creative = self.mode == GameMode::Creative;
        let main = &self.inv.main;
        let find = |item: ItemId, dmg: Option<i32>| {
            (0..main.len()).find(|&i| {
                matches!(main[i], Some(s) if s.id == item && dmg.map_or(true, |d| s.meta == d))
            })
        };
        let held = main[self.inv.selected];
        let found = find(item, subtypes.then_some(dmg));
        match found {
            Some(i) if i < 9 => self.inv.selected = i,
            _ if creative => {
                if let Some(e) = (0..main.len()).find(|&i| main[i].is_none()) {
                    if e < 9 {
                        self.inv.selected = e;
                    }
                }
                // creativePickItem: an unenchanted tool in hand stays
                let sel = self.inv.selected;
                if let Some(h) = held {
                    if crate::enchant::enchantable(&h) && find(h.id, Some(h.meta)) == Some(sel) {
                        self.inv_dirty = true;
                        return;
                    }
                }
                match find(item, Some(dmg)) {
                    Some(k) => {
                        let n = self.inv.main[k].map_or(1, |s| s.count);
                        self.inv.main[k] = self.inv.main[sel];
                        self.inv.main[sel] = Some(Stack::new(item, dmg, n));
                    }
                    None => self.inv.main[sel] = Some(Stack::new(item, dmg, 1)),
                }
            }
            _ => {}
        }
        self.inv_dirty = true;
    }

    /// `TileEntityChest.openInventory` / `closeInventory`: a chest whose
    /// window opens or closes tells its neighbours and the block below
    /// (a trapped chest's power changes with it).
    fn sync_chest_users(&mut self, w: &mut ServerWorld) {
        let now: std::collections::BTreeSet<Pos> = match &self.open {
            Open::Chest(ps) => ps.iter().copied().collect(),
            _ => Default::default(),
        };
        if now == w.chest_users {
            return;
        }
        let changed: Vec<Pos> = now.symmetric_difference(&w.chest_users).copied().collect();
        w.chest_users = now;
        for p in changed {
            let id = w.block(p[0], p[1], p[2]);
            if block::is_chest(id) {
                w.notify_neighbours(p[0], p[1], p[2], id);
                w.notify_neighbours(p[0], p[1] - 1, p[2], id);
            }
        }
    }

    /// Items, furnaces and pickup for one tick.
    pub fn tick(&mut self, w: &mut ServerWorld) {
        // TileEntityHopper.updateEntity
        self.tick_hoppers(w);
        self.sync_comparators(w);
        self.container_signals(w);
        self.sync_chest_users(w);
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
            if let Some((bb, amount)) = f.hurt.take() {
                // the items it lands on are entities of the list too
                // (EntityItem.attackEntityFrom)
                for e in self.items.iter_mut() {
                    if !e.e.dead && e.bb().intersects(&bb) {
                        e.attack(None, false, amount);
                    }
                }
                self.falling_hits
                    .push((bb, amount, f.block == block::ANVIL));
            }
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
                    // EntityItem.onCollideWithPlayer: the pick-up achievements
                    match shown.id {
                        17 | 162 => self.achievements.push("achievement.mineWood"),
                        334 => self.achievements.push("achievement.killCow"),
                        264 => self.achievements.push("achievement.diamonds"),
                        369 => self.achievements.push("achievement.blazeRod"),
                        _ => {}
                    }
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
                    Tile::Dispenser { slots, dropper } => (
                        if *dropper { "Dropper" } else { "Trap" },
                        vec![("Items", items(slots))],
                    ),
                    Tile::Comparator { output } => {
                        ("Comparator", vec![("OutputSignal", Nbt::Int(*output))])
                    }
                    Tile::Daylight => ("DLDetector", vec![]),
                    Tile::Skull { kind, rot } => (
                        "Skull",
                        vec![
                            ("SkullType", Nbt::Byte(*kind as i8)),
                            ("Rot", Nbt::Byte(*rot as i8)),
                            ("ExtraType", Nbt::String(String::new())),
                        ],
                    ),
                    // TileEntityBeacon.writeToNBT
                    Tile::Beacon {
                        levels,
                        primary,
                        secondary,
                        ..
                    } => (
                        "Beacon",
                        vec![
                            ("Primary", Nbt::Int(*primary)),
                            ("Secondary", Nbt::Int(*secondary)),
                            ("Levels", Nbt::Int(*levels)),
                        ],
                    ),
                    // TileEntityJukebox.writeToNBT: Record and RecordItem
                    Tile::Jukebox { record } => (
                        "RecordPlayer",
                        match record {
                            Some(s) => vec![
                                ("Record", Nbt::Int(s.id as i32)),
                                ("RecordItem", item_nbt(None, s)),
                            ],
                            None => vec![],
                        },
                    ),
                    Tile::Hopper { slots, cooldown } => (
                        "Hopper",
                        vec![
                            ("Items", items(slots)),
                            ("TransferCooldown", Nbt::Int(*cooldown)),
                        ],
                    ),
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
            "Trap" | "Dropper" => {
                let v = fill(9);
                let mut a = [None; 9];
                a.copy_from_slice(&v);
                Tile::Dispenser {
                    slots: a,
                    dropper: id == "Dropper",
                }
            }
            "DLDetector" => Tile::Daylight,
            "Skull" => Tile::Skull {
                kind: t.int("SkullType").unwrap_or(0) as i32,
                rot: t.int("Rot").unwrap_or(0) as i32,
            },
            "Beacon" => Tile::Beacon {
                levels: t.int("Levels").unwrap_or(0) as i32,
                primary: t.int("Primary").unwrap_or(0) as i32,
                secondary: t.int("Secondary").unwrap_or(0) as i32,
                complete: false,
                payment: None,
            },
            "RecordPlayer" => Tile::Jukebox {
                record: t
                    .get("RecordItem")
                    .and_then(crate::world::anvil::item_from_nbt)
                    .or_else(|| {
                        t.int("Record")
                            .filter(|&r| r > 0)
                            .map(|r| Stack::new(r as crate::item::ItemId, 0, 1))
                    }),
            },
            "Comparator" => Tile::Comparator {
                output: t.int("OutputSignal").unwrap_or(0) as i32,
            },
            "Hopper" => {
                let v = fill(5);
                let mut a = [None; 5];
                a.copy_from_slice(&v);
                Tile::Hopper {
                    slots: a,
                    cooldown: t.int("TransferCooldown").unwrap_or(-1) as i32,
                }
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
        list.extend(vehicles.iter().filter(|b| b.saved()).map(|b| b.to_nbt(dim)));
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
                b.saved() && (p[0].floor() as i32) >> 4 == cx && (p[2].floor() as i32) >> 4 == cz
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
                Tile::Dispenser { slots, dropper } => format!(
                    "{},{},{},{},{}",
                    if *dropper { "R" } else { "D" },
                    p[0],
                    p[1],
                    p[2],
                    enc(slots)
                ),
                Tile::Hopper { slots, cooldown } => {
                    format!("H,{},{},{},{},{}", p[0], p[1], p[2], cooldown, enc(slots))
                }
                Tile::Comparator { output } => format!("K,{},{},{},{}", p[0], p[1], p[2], output),
                Tile::Daylight => format!("L,{},{},{}", p[0], p[1], p[2]),
                Tile::Skull { kind, rot } => format!("U,{},{},{},{},{}", p[0], p[1], p[2], kind, rot),
                Tile::Beacon {
                    levels,
                    primary,
                    secondary,
                    ..
                } => format!("B,{},{},{},{},{},{}", p[0], p[1], p[2], levels, primary, secondary),
                Tile::Jukebox { record } => {
                    format!("J,{},{},{},{}", p[0], p[1], p[2], enc(&[*record]))
                }
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
            "D" | "R" => {
                let f: Vec<&str> = line.splitn(5, ',').collect();
                let s = dec(f.get(4)?, 9)?;
                let mut slots = [None; 9];
                slots.copy_from_slice(&s);
                self.tiles.insert(
                    pos(f[1], f[2], f[3])?,
                    Tile::Dispenser {
                        slots,
                        dropper: f[0] == "R",
                    },
                );
            }
            "J" => {
                let f: Vec<&str> = line.splitn(5, ',').collect();
                let s = dec(f.get(4)?, 1)?;
                self.tiles
                    .insert(pos(f[1], f[2], f[3])?, Tile::Jukebox { record: s[0] });
            }
            "B" => {
                let f: Vec<&str> = line.splitn(7, ',').collect();
                self.tiles.insert(
                    pos(f[1], f[2], f[3])?,
                    Tile::Beacon {
                        levels: f.get(4)?.parse().ok()?,
                        primary: f.get(5)?.parse().ok()?,
                        secondary: f.get(6)?.trim().parse().ok()?,
                        complete: false,
                        payment: None,
                    },
                );
            }
            "U" => {
                let f: Vec<&str> = line.splitn(6, ',').collect();
                self.tiles.insert(
                    pos(f[1], f[2], f[3])?,
                    Tile::Skull {
                        kind: f.get(4)?.parse().ok()?,
                        rot: f.get(5)?.trim().parse().ok()?,
                    },
                );
            }
            "L" => {
                let f: Vec<&str> = line.splitn(4, ',').collect();
                self.tiles
                    .insert(pos(f[1], f[2], f[3].trim())?, Tile::Daylight);
            }
            "K" => {
                let f: Vec<&str> = line.splitn(5, ',').collect();
                self.tiles.insert(
                    pos(f[1], f[2], f[3])?,
                    Tile::Comparator {
                        output: f.get(4)?.parse().ok()?,
                    },
                );
            }
            "H" => {
                let f: Vec<&str> = line.splitn(6, ',').collect();
                let s = dec(f.get(5)?, 5)?;
                let mut slots = [None; 5];
                slots.copy_from_slice(&s);
                self.tiles.insert(
                    pos(f[1], f[2], f[3])?,
                    Tile::Hopper {
                        slots,
                        cooldown: f[4].parse().ok()?,
                    },
                );
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

/// `TileEntityBeacon.effectsList` by tier.
pub const BEACON_EFFECTS: [&[i32]; 4] = [
    &[crate::potion::MOVE_SPEED, crate::potion::DIG_SPEED],
    &[crate::potion::RESISTANCE, crate::potion::JUMP],
    &[crate::potion::DAMAGE_BOOST],
    &[crate::potion::REGENERATION],
];

/// `TileEntityBeacon.setPrimaryEffect`: a power of a tier the pyramid
/// reaches (the first three), else none.
pub fn beacon_primary(levels: i32, id: i32) -> i32 {
    for (t, list) in BEACON_EFFECTS.iter().enumerate().take(3) {
        if (t as i32) < levels && list.contains(&id) {
            return id;
        }
    }
    0
}

/// `TileEntityBeacon.setSecondaryEffect`: any listed power, from level 4.
pub fn beacon_secondary(levels: i32, id: i32) -> i32 {
    if levels >= 4 && BEACON_EFFECTS.iter().any(|l| l.contains(&id)) {
        id
    } else {
        0
    }
}

impl Tile {
    /// `new TileEntityBeacon()`: levels -1 until the first update.
    pub fn beacon() -> Tile {
        Tile::Beacon {
            levels: -1,
            primary: 0,
            secondary: 0,
            complete: false,
            payment: None,
        }
    }
}

/// `Block.getItem(World, x, y, z)` and the damage `middleClickMouse` gives
/// it: the item, its damage, and `getHasSubtypes`. `None` for the blocks
/// with no item (a spawner, portals, a lit redstone ore, a dragon egg).
pub fn pick_item(id: u8, meta: i32, below: i32, skull: Option<i32>) -> Option<(ItemId, i32, bool)> {
    let it = |i: ItemId| Some((i, 0, false));
    match id {
        0 => None,
        // BlockDoor, BlockBed, signs, cake, cauldron, brewing stand
        64 => it(324),
        71 => it(330),
        26 => it(355),
        63 | 68 => it(323),
        92 => it(354),
        118 => it(380),
        117 => it(379),
        // crops give their seed, stems the seeds, wart, reeds
        59 => it(295),
        141 => it(391),
        142 => it(392),
        104 => it(361),
        105 => it(362),
        115 => it(372),
        83 => it(338),
        // redstone parts
        55 => it(331),
        93 | 94 => it(356),
        149 | 150 => it(404),
        75 | 76 => Some((76, 0, false)),
        123 | 124 => Some((123, 0, false)),
        132 => it(287),
        // BlockCocoa: cocoa beans
        127 => Some((351, 3, true)),
        // BlockSkull: the tile entity's type
        144 => Some((397, skull.unwrap_or(meta), true)),
        140 => it(390),
        // BlockFarmland gives dirt, and dirt reads the moisture as its damage
        60 => Some((3, if meta == 1 { 0 } else { meta }, true)),
        61 | 62 => Some((61, 0, false)),
        99 | 100 => Some((id as ItemId - 60, 0, false)),
        34 => Some((if meta & 8 != 0 { 29 } else { 33 }, 0, false)),
        // the double slabs give their slab
        43 => Some((44, meta & 7, true)),
        125 => Some((126, meta & 7, true)),
        // no item: spawner, piston moving, portals, dragon egg, lit redstone ore
        52 | 36 | 90 | 119 | 122 | 74 => None,
        _ => {
            // Item.registerItems: the ItemBlocks with subtypes
            let subtypes = matches!(
                id,
                3 | 5 | 6 | 12 | 17 | 18 | 24 | 31 | 35 | 37 | 38 | 44 | 78 | 95 | 97 | 98 | 126
                    | 139 | 145 | 155 | 159 | 160 | 161 | 162 | 171 | 175
            );
            if !subtypes {
                return Some((id as ItemId, 0, false));
            }
            let dmg = match id {
                // BlockDirt.getDamageValue: coarse dirt (1) reads as dirt
                3 => {
                    if meta == 1 {
                        0
                    } else {
                        meta
                    }
                }
                161 => meta & 3,
                31 | 97 => meta,
                44 | 126 => meta & 7,
                // BlockDoublePlant: the top half asks the bottom
                175 => {
                    if meta & 8 != 0 {
                        below & 7
                    } else {
                        meta & 7
                    }
                }
                _ => crate::drops::damage_dropped(id, meta),
            };
            Some((id as ItemId, dmg, true))
        }
    }
}

#[cfg(test)]
mod pick_tests {
    use super::pick_item;

    /// `Minecraft.middleClickMouse` against `Block.getItem` / `getDamageValue`.
    #[test]
    fn middle_click_picks_what_the_game_picks() {
        // either half of a door is the door item
        assert_eq!(pick_item(64, 8, 3, None), Some((324, 0, false)));
        assert_eq!(pick_item(64, 3, 0, None), Some((324, 0, false)));
        assert_eq!(pick_item(71, 9, 0, None), Some((330, 0, false)));
        // stairs and torches lose their facing; logs keep the wood
        assert_eq!(pick_item(53, 3, 0, None), Some((53, 0, false)));
        assert_eq!(pick_item(17, 5, 0, None), Some((17, 1, true)));
        // a double slab is its slab; the top of a tall flower asks below
        assert_eq!(pick_item(43, 9, 0, None), Some((44, 1, true)));
        assert_eq!(pick_item(175, 8, 4, None), Some((175, 4, true)));
        assert_eq!(pick_item(127, 9, 0, None), Some((351, 3, true)));
        assert_eq!(pick_item(144, 1, 0, Some(4)), Some((397, 4, true)));
        assert_eq!(pick_item(52, 0, 0, None), None);
    }
}
