//! Server-side gameplay state: the player's inventory, dropped items,
//! furnaces and chests, and what happens when blocks are dug or used.

use crate::block;
use crate::entity::item::ItemEntity;
use crate::entity::player::Aabb;
use crate::entity::survival::GameMode;
use crate::inventory::{Inventory, Slot, MAIN};
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
}

/// The window the player has open.
#[derive(Clone, Debug, PartialEq)]
pub enum Open {
    None,
    Inventory,
    Workbench(Pos),
    Furnace(Pos),
    /// One or two chest halves; the first is the top half of the window.
    Chest(Vec<Pos>),
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
    next_item: u32,
    pub tiles: BTreeMap<Pos, Tile>,
    /// Feet position and yaw of the player.
    pub player: ([f64; 3], f32),
    pub inv_dirty: bool,
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
    let b = |id: u8| id as ItemId;
    Some(match s.id {
        x if x == b(block::IRON_ORE) => Stack::new(item::IRON_INGOT, 0, 1),
        x if x == b(block::GOLD_ORE) => Stack::new(item::GOLD_INGOT, 0, 1),
        x if x == b(block::DIAMOND_ORE) => Stack::new(item::DIAMOND, 0, 1),
        x if x == b(block::EMERALD_ORE) => Stack::new(item::EMERALD, 0, 1),
        x if x == b(block::COAL_ORE) => Stack::new(item::COAL, 0, 1),
        x if x == b(block::REDSTONE_ORE) => Stack::new(item::REDSTONE, 0, 1),
        x if x == b(block::LAPIS_ORE) => Stack::new(item::DYE, 4, 1),
        x if x == b(block::SAND) => Stack::new(b(block::GLASS), 0, 1),
        x if x == b(block::COBBLESTONE) => Stack::new(b(block::STONE), 0, 1),
        x if x == b(block::LOG) || x == b(block::LOG2) => Stack::new(item::COAL, 1, 1),
        x if x == b(block::CACTUS) => Stack::new(item::DYE, 2, 1),
        x if x == b(block::CLAY) => Stack::new(b(block::HARDENED_CLAY), 0, 1),
        item::CLAY_BALL => Stack::new(item::BRICK, 0, 1),
        _ => return None,
    })
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

/// Left/right click on a plain slot (`Container.slotClick` mode 0).
fn click_slot(
    target: &mut Option<Stack>,
    cursor: &mut Option<Stack>,
    right: bool,
    accepts: bool,
    take_only: bool,
) {
    match (*target, *cursor) {
        (Some(s), None) => {
            if right && !take_only {
                let take = (s.count + 1) / 2;
                *cursor = Some(Stack { count: take, ..s });
                *target = if s.count > take {
                    Some(Stack {
                        count: s.count - take,
                        ..s
                    })
                } else {
                    None
                };
            } else {
                *cursor = Some(s);
                *target = None;
            }
        }
        (None, Some(c)) if accepts && !take_only => {
            let n = if right {
                1
            } else {
                c.count.min(max_stack(c.id))
            };
            *target = Some(Stack { count: n, ..c });
            *cursor = if c.count > n {
                Some(Stack {
                    count: c.count - n,
                    ..c
                })
            } else {
                None
            };
        }
        (Some(s), Some(c)) if s.same_item(&c) && s.meta == c.meta => {
            if take_only {
                if s.count + c.count <= max_stack(c.id) {
                    *cursor = Some(Stack {
                        count: s.count + c.count,
                        ..c
                    });
                    *target = None;
                }
            } else {
                let room = max_stack(s.id).saturating_sub(s.count);
                let n = if right {
                    room.min(1)
                } else {
                    room.min(c.count)
                };
                *target = Some(Stack {
                    count: s.count + n,
                    ..s
                });
                *cursor = if c.count > n {
                    Some(Stack {
                        count: c.count - n,
                        ..c
                    })
                } else {
                    None
                };
            }
        }
        (Some(s), Some(c)) if accepts && !take_only => {
            *target = Some(c);
            *cursor = Some(s);
        }
        _ => {}
    }
}

/// `BlockChest` / `BlockFurnace` facing from the placer's yaw.
fn facing_meta(yaw: f32) -> i32 {
    match (((yaw * 4.0 / 360.0) as f64 + 0.5).floor() as i32) & 3 {
        0 => 2,
        1 => 5,
        2 => 3,
        _ => 4,
    }
}

impl Play {
    pub fn new(mode: GameMode) -> Play {
        Play {
            inv: Inventory::default(),
            open: Open::None,
            mode,
            items: Vec::new(),
            next_item: 1,
            tiles: BTreeMap::new(),
            player: ([0.0; 3], 0.0),
            inv_dirty: true,
            pending: Vec::new(),
        }
    }

    pub fn view(&self) -> InvView {
        let (tile_slots, furnace) = match &self.open {
            Open::Furnace(p) => match self.tiles.get(p) {
                Some(Tile::Furnace {
                    slots,
                    burn,
                    item_burn,
                    cook,
                }) => {
                    let ib = if *item_burn == 0 { 200 } else { *item_burn };
                    (slots.to_vec(), Some((burn * 13 / ib, cook * 24 / 200)))
                }
                _ => (Vec::new(), None),
            },
            Open::Chest(ps) => {
                let mut v = Vec::new();
                for p in ps {
                    if let Some(Tile::Chest { slots }) = self.tiles.get(p) {
                        v.extend_from_slice(slots);
                    }
                }
                (v, None)
            }
            _ => (Vec::new(), None),
        };
        InvView {
            inv: self.inv.clone(),
            open: self.open.clone(),
            result: self.inv.result(),
            tile_slots,
            furnace,
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
            other => self.pending.push(other),
        }
    }

    /// `EntityPlayer.dropPlayerItemWithRandomChoice`: thrown in front of the player.
    pub fn throw(&mut self, s: Stack, r: &mut JavaRandom) {
        let ([x, y, z], yaw) = self.player;
        let id = self.next_item;
        self.next_item += 1;
        let mut e = ItemEntity::new(id, s, [x, y + 1.62 - 0.3, z], r);
        let pitch = 0.0f32;
        let f = 0.3f32;
        let yr = yaw / 180.0 * std::f32::consts::PI;
        e.motion = [
            (-yr.sin() * pitch.cos() * f) as f64,
            (-pitch.sin() * f + 0.1) as f64,
            (yr.cos() * pitch.cos() * f) as f64,
        ];
        e.pickup_delay = 40;
        self.spawn(e);
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
                Tile::Spawner { .. } => Vec::new(),
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
    pub fn use_block(&mut self, w: &mut ServerWorld, target: Pos, face: [i32; 3], sneaking: bool) {
        let tid = w.block(target[0], target[1], target[2]);
        if !sneaking || self.inv.held().is_none() {
            match tid {
                block::CRAFTING_TABLE => {
                    self.open = Open::Workbench(target);
                    self.inv.grid_size = 3;
                    self.inv_dirty = true;
                    return;
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
                    return;
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
                        return;
                    }
                    for q in &halves {
                        self.tiles
                            .entry(*q)
                            .or_insert(Tile::Chest { slots: [None; 27] });
                    }
                    self.open = Open::Chest(halves);
                    self.inv_dirty = true;
                    return;
                }
                block::LEVER => {
                    w.toggle_lever(target[0], target[1], target[2]);
                    return;
                }
                block::STONE_BUTTON | block::WOODEN_BUTTON => {
                    w.press_button(tid, target[0], target[1], target[2]);
                    return;
                }
                block::UNPOWERED_REPEATER | block::POWERED_REPEATER => {
                    // BlockRedstoneRepeater.onBlockActivated: next delay.
                    let m = w.meta(target[0], target[1], target[2]);
                    let d = (((m & 12) >> 2) + 1) << 2 & 12;
                    w.set_meta(target[0], target[1], target[2], d | (m & 3), 3);
                    return;
                }
                block::NOTEBLOCK => {
                    w.note_click(target[0], target[1], target[2]);
                    return;
                }
                block::WOODEN_DOOR => {
                    // BlockDoor.onBlockActivated
                    let [x, y, z] = target;
                    let ly = if w.meta(x, y, z) & 8 != 0 { y - 1 } else { y };
                    let m = w.meta(x, ly, z);
                    w.set_meta(x, ly, z, (m & 7) ^ 4, 2);
                    w.door_sound(x, y, z);
                    return;
                }
                block::TRAPDOOR => {
                    let [x, y, z] = target;
                    let m = w.meta(x, y, z);
                    w.set_meta(x, y, z, m ^ 4, 2);
                    w.door_sound(x, y, z);
                    return;
                }
                block::DISPENSER => {
                    self.tiles
                        .entry(target)
                        .or_insert(Tile::Dispenser { slots: [None; 9] });
                    return;
                }
                block::TNT if self.inv.held().map(|s| s.id) == Some(259) => {
                    // BlockTNT.onBlockActivated with flint and steel.
                    w.prime_tnt(target[0], target[1], target[2], 80);
                    self.damage_held(1);
                    return;
                }
                _ => {}
            }
        }
        let Some(held) = self.inv.held() else { return };
        if held.id == 324 || held.id == 330 {
            self.place_door(w, target, face, held.id);
            return;
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
            return;
        }
        let id = match held.id {
            331 => block::REDSTONE_WIRE,
            356 => block::UNPOWERED_REPEATER,
            _ => match held.block() {
                Some(id) => id,
                None => return,
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
            return;
        }
        if !w.material(p[0], p[1], p[2]).replaceable() {
            return;
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
                    _ => return,
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
                    _ => return,
                };
            }
            block::TRAPDOOR => {
                meta = match side {
                    2 if nc(w, p[0], p[1], p[2] + 1) => 0,
                    3 if nc(w, p[0], p[1], p[2] - 1) => 1,
                    4 if nc(w, p[0] + 1, p[1], p[2]) => 2,
                    5 if nc(w, p[0] - 1, p[1], p[2]) => 3,
                    _ => return,
                };
            }
            block::STONE_PRESSURE_PLATE | block::WOODEN_PRESSURE_PLATE => {
                if !w.solid_top_surface(p[0], p[1] - 1, p[2])
                    && w.block(p[0], p[1] - 1, p[2]) != block::FENCE
                {
                    return;
                }
                meta = 0;
            }
            block::REDSTONE_WIRE => {
                if !w.solid_top_surface(p[0], p[1] - 1, p[2])
                    && w.block(p[0], p[1] - 1, p[2]) != block::GLOWSTONE
                {
                    return;
                }
                meta = 0;
            }
            block::UNPOWERED_REPEATER => {
                if !w.solid_top_surface(p[0], p[1] - 1, p[2]) {
                    return;
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
                    return;
                }
            }
            block::FURNACE | block::CHEST => meta = facing_meta(self.player.1),
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
            return;
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
            return;
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
            return;
        }
        if id == block::DOUBLE_PLANT {
            if !w.is_air(p[0], p[1] + 1, p[2]) {
                return;
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
        let mut e = ItemEntity::new(id, one, at, r);
        let d3 = r.next_double() * 0.1 + 0.2;
        e.motion = [dir[0] * d3, 0.20000000298023224, dir[2] * d3];
        for v in &mut e.motion {
            *v += r.next_gaussian() * 0.007499999832361937 * 6.0;
        }
        self.spawn(e);
        sounds.push(("random.click", c, 1.0, 1.0));
        None
    }

    /// A click inside the open window. `Slot::Main` and the crafting slots
    /// go to the inventory; `tile` indexes the chest or furnace slots.
    pub fn click(
        &mut self,
        w: &mut ServerWorld,
        slot: Slot,
        tile: Option<usize>,
        right: bool,
        shift: bool,
    ) {
        self.inv_dirty = true;
        let mut rnd = std::mem::replace(&mut w.rand, JavaRandom::new(0));
        if let Some(t) = tile {
            self.click_tile(t, right, shift);
        } else if let (Slot::Main(i), true, Open::Chest(_) | Open::Furnace(_)) =
            (slot, shift, &self.open)
        {
            self.shift_into_tile(i);
        } else {
            for s in self.inv.click(slot, right, shift) {
                self.throw(s, &mut rnd);
            }
        }
        w.rand = rnd;
    }

    fn tile_slots_mut(&mut self) -> Vec<&mut Option<Stack>> {
        match &self.open {
            Open::Furnace(p) => match self.tiles.get_mut(p) {
                Some(Tile::Furnace { slots, .. }) => slots.iter_mut().collect(),
                _ => Vec::new(),
            },
            Open::Chest(ps) => {
                let ps = ps.clone();
                let mut out: Vec<&mut Option<Stack>> = Vec::new();
                // Split borrows: collect raw pointers to each half's slots.
                let mut ptrs: Vec<*mut [Option<Stack>; 27]> = Vec::new();
                for p in &ps {
                    if let Some(Tile::Chest { slots }) = self.tiles.get_mut(p) {
                        ptrs.push(slots as *mut _);
                    }
                }
                for ptr in ptrs {
                    // SAFETY: distinct map entries, alive for the borrow of self.
                    out.extend(unsafe { (*ptr).iter_mut() });
                }
                out
            }
            _ => Vec::new(),
        }
    }

    fn click_tile(&mut self, t: usize, right: bool, shift: bool) {
        let furnace = matches!(self.open, Open::Furnace(_));
        let mut cursor = self.inv.cursor;
        if shift {
            let s = {
                let mut slots = self.tile_slots_mut();
                match slots.get_mut(t) {
                    Some(slot) => slot.take(),
                    None => None,
                }
            };
            if let Some(s) = s {
                let rest = {
                    let mut tmp = self.inv.clone();
                    let r = merge_back(&mut tmp, s);
                    self.inv = tmp;
                    r
                };
                if let Some(r) = rest {
                    let mut slots = self.tile_slots_mut();
                    *slots[t] = Some(r);
                }
            }
            return;
        }
        {
            let mut slots = self.tile_slots_mut();
            let Some(slot) = slots.get_mut(t) else { return };
            // SlotFurnace: the output only gives items out; fuel and input accept.
            let take_only = furnace && t == 2;
            click_slot(slot, &mut cursor, right, true, take_only);
        }
        self.inv.cursor = cursor;
    }

    /// Shift-click from the player inventory into the open container.
    fn shift_into_tile(&mut self, i: usize) {
        let Some(s) = self.inv.main[i].take() else {
            return;
        };
        let furnace = matches!(self.open, Open::Furnace(_));
        let mut rest = Some(s);
        {
            let mut slots = self.tile_slots_mut();
            if furnace {
                let target = if smelt(&s).is_some() {
                    Some(0)
                } else if burn_time(&s) > 0 {
                    Some(1)
                } else {
                    None
                };
                if let Some(ti) = target {
                    if merge_slot(&mut *slots[ti], s) {
                        rest = None;
                    }
                }
            } else {
                let mut left = s;
                for slot in slots.iter_mut() {
                    if let Some(t) = slot {
                        if t.same_item(&left) && t.meta == left.meta {
                            let n = (max_stack(t.id) - t.count).min(left.count);
                            t.count += n;
                            left.count -= n;
                        }
                    }
                    if left.count == 0 {
                        break;
                    }
                }
                if left.count > 0 {
                    for slot in slots.iter_mut() {
                        if slot.is_none() {
                            **slot = Some(left);
                            left.count = 0;
                            break;
                        }
                    }
                }
                rest = if left.count > 0 { Some(left) } else { None };
            }
        }
        if furnace && rest.is_some() {
            // Not smeltable or fuel: hotbar <-> main.
            self.inv.main[i] = rest;
            self.inv.click(Slot::Main(i), false, true);
            return;
        }
        self.inv.main[i] = rest;
    }

    pub fn close(&mut self, w: &mut ServerWorld) {
        let mut rnd = std::mem::replace(&mut w.rand, JavaRandom::new(0));
        for s in self.inv.close() {
            self.throw(s, &mut rnd);
        }
        w.rand = rnd;
        self.open = Open::None;
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
        // TileEntityFurnace.updateEntity
        let keys: Vec<Pos> = self.tiles.keys().copied().collect();
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

        // EntityItem.onUpdate, merging, pickup.
        let mut alive = Vec::with_capacity(self.items.len());
        for mut e in std::mem::take(&mut self.items) {
            if !w.chunk_exists(
                crate::util::math::floor_double(e.pos[0]) >> 4,
                crate::util::math::floor_double(e.pos[2]) >> 4,
            ) {
                alive.push(e);
                continue;
            }
            if e.tick(&*w as &dyn BlockView) {
                alive.push(e);
            }
        }
        // searchForOtherItemsNearby every 25 ticks of age.
        let n = alive.len();
        for i in 0..n {
            if alive[i].stack.count == 0 || alive[i].age % 25 != 0 {
                continue;
            }
            for j in 0..n {
                if i == j || alive[j].stack.count == 0 {
                    continue;
                }
                let (a, b) = (alive[i].clone(), alive[j].clone());
                let near = (a.pos[0] - b.pos[0]).abs() < 0.5 + 0.25
                    && (a.pos[1] - b.pos[1]).abs() < 0.25
                    && (a.pos[2] - b.pos[2]).abs() < 0.5 + 0.25;
                if near
                    && a.stack.same_item(&b.stack)
                    && a.stack.meta == b.stack.meta
                    && a.stack.count + b.stack.count <= max_stack(a.stack.id)
                    && item::tool(a.stack.id).is_none()
                {
                    alive[j].stack.count += a.stack.count;
                    alive[j].pickup_delay = alive[j].pickup_delay.max(a.pickup_delay);
                    alive[j].age = alive[j].age.min(a.age);
                    alive[i].stack.count = 0;
                    break;
                }
            }
        }
        alive.retain(|e| e.stack.count > 0);
        let ([px, py, pz], _) = self.player;
        let reach = Aabb {
            min: [px - 0.3 - 1.0, py - 0.5, pz - 0.3 - 1.0],
            max: [px + 0.3 + 1.0, py + 1.8 + 0.5, pz + 0.3 + 1.0],
        };
        for e in alive.iter_mut() {
            if e.pickup_delay == 0 && e.bb().intersects(&reach) {
                match self.inv.add(e.stack) {
                    None => e.stack.count = 0,
                    Some(rest) => e.stack = rest,
                }
                self.inv_dirty = true;
            }
        }
        alive.retain(|e| e.stack.count > 0);
        self.items = alive;
    }

    pub fn item_views(&self) -> Vec<ItemView> {
        self.items
            .iter()
            .map(|e| ItemView {
                id: e.id,
                stack: e.stack,
                pos: e.pos,
                prev: e.prev,
                age: e.age,
                hover: e.hover,
            })
            .collect()
    }

    /// Level-file lines for the tile entities.
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

/// Shift-click from a container into the player inventory (hotbar last).
fn merge_back(inv: &mut Inventory, s: Stack) -> Option<Stack> {
    let mut left = s;
    for i in (0..MAIN).rev() {
        if let Some(t) = &mut inv.main[i] {
            if t.same_item(&left) && t.meta == left.meta && t.count < max_stack(t.id) {
                let n = (max_stack(t.id) - t.count).min(left.count);
                t.count += n;
                left.count -= n;
                if left.count == 0 {
                    return None;
                }
            }
        }
    }
    for i in (0..MAIN).rev() {
        if inv.main[i].is_none() {
            inv.main[i] = Some(left);
            return None;
        }
    }
    Some(left)
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
