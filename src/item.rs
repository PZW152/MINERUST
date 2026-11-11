//! Items, tools and the block properties that mining depends on
//! (`Item`, `ItemTool`, `Block.blockHardness`, `Block.getDrops`).

use crate::block::{self, Material};
use crate::java::JavaRandom;

/// Item id; ids below 256 are block items with the block's id.
pub type ItemId = u16;

pub const COAL: ItemId = 263;
pub const DIAMOND: ItemId = 264;
pub const IRON_INGOT: ItemId = 265;
pub const GOLD_INGOT: ItemId = 266;
pub const STICK: ItemId = 280;
pub const STRING: ItemId = 287;
pub const SEEDS: ItemId = 295;
pub const FLINT: ItemId = 318;
pub const APPLE: ItemId = 260;
pub const REDSTONE: ItemId = 331;
pub const SNOWBALL: ItemId = 332;
pub const CLAY_BALL: ItemId = 337;
pub const BRICK: ItemId = 336;
pub const REEDS_ITEM: ItemId = 338;
pub const DYE: ItemId = 351;
pub const MELON: ItemId = 360;
pub const EMERALD: ItemId = 388;

/// An inventory stack: item id, damage/metadata, count.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stack {
    pub id: ItemId,
    pub meta: i32,
    pub count: u32,
}

impl Stack {
    pub fn new(id: ItemId, meta: i32, count: u32) -> Stack {
        Stack { id, meta, count }
    }
    pub fn block(&self) -> Option<u8> {
        if self.id < 256 && block::info(self.id as u8).exists {
            Some(self.id as u8)
        } else {
            None
        }
    }
    /// `ItemStack.areItemStacksEqual` without the count (stackable together).
    pub fn same_item(&self, o: &Stack) -> bool {
        self.id == o.id && (self.meta == o.meta || tool(self.id).is_some())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToolKind {
    Sword,
    Shovel,
    Pickaxe,
    Axe,
    Hoe,
}

/// `Item.ToolMaterial`: harvest level, uses, efficiency.
#[derive(Clone, Copy, Debug)]
pub struct ToolInfo {
    pub kind: ToolKind,
    pub level: i32,
    pub max_uses: i32,
    pub efficiency: f32,
}

const MATERIALS: [(i32, i32, f32, &str); 5] = [(0, 59, 2.0, "wood"), (1, 131, 4.0, "stone"), (2, 250, 6.0, "iron"), (3, 1561, 8.0, "diamond"), (0, 32, 12.0, "gold")];

/// Tool item ids: `[sword, shovel, pickaxe, axe, hoe]` per material.
const TOOL_IDS: [[ItemId; 5]; 5] = [[268, 269, 270, 271, 290], [272, 273, 274, 275, 291], [267, 256, 257, 258, 292], [276, 277, 278, 279, 293], [283, 284, 285, 286, 294]];
const KINDS: [(ToolKind, &str); 5] = [(ToolKind::Sword, "sword"), (ToolKind::Shovel, "shovel"), (ToolKind::Pickaxe, "pickaxe"), (ToolKind::Axe, "axe"), (ToolKind::Hoe, "hoe")];

pub fn tool(id: ItemId) -> Option<ToolInfo> {
    for (m, row) in TOOL_IDS.iter().enumerate() {
        for (k, &tid) in row.iter().enumerate() {
            if tid == id {
                let (level, max_uses, efficiency, _) = MATERIALS[m];
                return Some(ToolInfo { kind: KINDS[k].0, level, max_uses, efficiency });
            }
        }
    }
    None
}

/// `Item.getItemStackLimit`
pub fn max_stack(id: ItemId) -> u32 {
    if tool(id).is_some() {
        1
    } else if id == SNOWBALL {
        16
    } else {
        64
    }
}

/// Texture file stem under `textures/items/` for non-block items.
pub fn item_texture(id: ItemId, meta: i32) -> Option<String> {
    for (m, row) in TOOL_IDS.iter().enumerate() {
        for (k, &tid) in row.iter().enumerate() {
            if tid == id {
                return Some(format!("{}_{}", MATERIALS[m].3, KINDS[k].1));
            }
        }
    }
    Some(
        match id {
            COAL => {
                if meta == 1 {
                    "charcoal"
                } else {
                    "coal"
                }
            }
            BRICK => "brick",
            DIAMOND => "diamond",
            IRON_INGOT => "iron_ingot",
            GOLD_INGOT => "gold_ingot",
            STICK => "stick",
            STRING => "string",
            SEEDS => "seeds_wheat",
            FLINT => "flint",
            APPLE => "apple",
            REDSTONE => "redstone_dust",
            SNOWBALL => "snowball",
            CLAY_BALL => "clay_ball",
            REEDS_ITEM => "reeds",
            MELON => "melon",
            EMERALD => "emerald",
            DYE => {
                match meta {
                    2 => "dye_powder_green",
                    3 => "dye_powder_brown",
                    _ => "dye_powder_blue",
                }
            }
            _ => return None,
        }
        .to_string(),
    )
}

/// English display name (`en_US.lang`).
pub fn display_name(s: &Stack) -> String {
    use block::*;
    for (m, row) in TOOL_IDS.iter().enumerate() {
        for (k, &tid) in row.iter().enumerate() {
            if tid == s.id {
                let mat = ["Wooden", "Stone", "Iron", "Diamond", "Golden"][m];
                let kind = ["Sword", "Shovel", "Pickaxe", "Axe", "Hoe"][k];
                return format!("{mat} {kind}");
            }
        }
    }
    let wood = ["Oak", "Spruce", "Birch", "Jungle", "Acacia", "Dark Oak"];
    let m = s.meta.clamp(0, 15) as usize;
    let name: String = match s.id {
        COAL => if s.meta == 1 { "Charcoal" } else { "Coal" }.into(),
        DIAMOND => "Diamond".into(),
        IRON_INGOT => "Iron Ingot".into(),
        GOLD_INGOT => "Gold Ingot".into(),
        STICK => "Stick".into(),
        STRING => "String".into(),
        SEEDS => "Seeds".into(),
        FLINT => "Flint".into(),
        APPLE => "Apple".into(),
        REDSTONE => "Redstone".into(),
        SNOWBALL => "Snowball".into(),
        CLAY_BALL => "Clay".into(),
        BRICK => "Brick".into(),
        REEDS_ITEM => "Sugar Canes".into(),
        MELON => "Melon".into(),
        EMERALD => "Emerald".into(),
        DYE => match s.meta {
            2 => "Cactus Green",
            3 => "Cocoa Beans",
            _ => "Lapis Lazuli",
        }
        .into(),
        id if id < 256 => match id as u8 {
            STONE => "Stone".into(),
            GRASS => "Grass Block".into(),
            DIRT => if s.meta == 2 { "Podzol" } else { "Dirt" }.into(),
            COBBLESTONE => "Cobblestone".into(),
            PLANKS => format!("{} Wood Planks", wood[m.min(5)]),
            6 => "Sapling".into(),
            BEDROCK => "Bedrock".into(),
            FLOWING_WATER | WATER => "Water".into(),
            FLOWING_LAVA | LAVA => "Lava".into(),
            SAND => if s.meta == 1 { "Red Sand" } else { "Sand" }.into(),
            GRAVEL => "Gravel".into(),
            GOLD_ORE => "Gold Ore".into(),
            IRON_ORE => "Iron Ore".into(),
            COAL_ORE => "Coal Ore".into(),
            LOG => format!("{} Wood", wood[m & 3]),
            LOG2 => format!("{} Wood", wood[4 + (m & 1)]),
            LEAVES => format!("{} Leaves", wood[m & 3]),
            LEAVES2 => format!("{} Leaves", wood[4 + (m & 1)]),
            GLASS => "Glass".into(),
            LAPIS_ORE => "Lapis Lazuli Ore".into(),
            SANDSTONE => "Sandstone".into(),
            TALLGRASS => if s.meta == 2 { "Fern" } else { "Grass" }.into(),
            DEADBUSH => "Dead Bush".into(),
            YELLOW_FLOWER => "Dandelion".into(),
            RED_FLOWER => ["Poppy", "Blue Orchid", "Allium", "Azure Bluet", "Red Tulip", "Orange Tulip", "White Tulip", "Pink Tulip", "Oxeye Daisy"][m.min(8)].into(),
            BROWN_MUSHROOM | RED_MUSHROOM => "Mushroom".into(),
            STONE_SLAB => match s.meta & 7 {
                1 => "Sandstone Slab",
                3 => "Cobblestone Slab",
                _ => "Stone Slab",
            }
            .into(),
            MOSSY_COBBLESTONE => "Moss Stone".into(),
            OBSIDIAN => "Obsidian".into(),
            TORCH => "Torch".into(),
            MOB_SPAWNER => "Monster Spawner".into(),
            CHEST => "Chest".into(),
            DIAMOND_ORE => "Diamond Ore".into(),
            CRAFTING_TABLE => "Crafting Table".into(),
            FURNACE | LIT_FURNACE => "Furnace".into(),
            REDSTONE_ORE => "Redstone Ore".into(),
            SNOW_LAYER => "Snow".into(),
            ICE => "Ice".into(),
            SNOW => "Snow".into(),
            CACTUS => "Cactus".into(),
            CLAY => "Clay".into(),
            REEDS => "Sugar Canes".into(),
            PUMPKIN => "Pumpkin".into(),
            MONSTER_EGG => "Stone Monster Egg".into(),
            BROWN_MUSHROOM_BLOCK | RED_MUSHROOM_BLOCK => "Mushroom".into(),
            MELON_BLOCK => "Melon".into(),
            VINE => "Vines".into(),
            MYCELIUM => "Mycelium".into(),
            WATERLILY => "Lily Pad".into(),
            EMERALD_ORE => "Emerald Ore".into(),
            STAINED_HARDENED_CLAY => "Stained Clay".into(),
            HARDENED_CLAY => "Hardened Clay".into(),
            PACKED_ICE => "Packed Ice".into(),
            DOUBLE_PLANT => ["Sunflower", "Lilac", "Double Tallgrass", "Large Fern", "Rose Bush", "Peony"][m.min(5)].into(),
            _ => format!("tile.{id}"),
        },
        id => format!("item.{id}"),
    };
    name
}

/// Every non-block item id that has a texture (for the atlas).
pub fn all_items() -> Vec<(ItemId, i32)> {
    let mut v: Vec<(ItemId, i32)> = TOOL_IDS.iter().flatten().map(|&id| (id, 0)).collect();
    for id in [COAL, DIAMOND, IRON_INGOT, GOLD_INGOT, STICK, STRING, SEEDS, FLINT, APPLE, REDSTONE, SNOWBALL, CLAY_BALL, REEDS_ITEM, MELON, EMERALD, BRICK] {
        v.push((id, 0));
    }
    v.push((COAL, 1));
    for m in [2, 3, 4] {
        v.push((DYE, m));
    }
    v
}

/// `Block.blockHardness`; negative means unbreakable.
pub fn hardness(id: u8) -> f32 {
    use block::*;
    match id {
        STONE => 1.5,
        GRASS | GRAVEL | CLAY | MYCELIUM => 0.6,
        DIRT | SAND | ICE | PACKED_ICE => 0.5,
        COBBLESTONE | PLANKS | LOG | LOG2 | STONE_SLAB | MOSSY_COBBLESTONE => 2.0,
        BEDROCK => -1.0,
        FLOWING_WATER | WATER | FLOWING_LAVA | LAVA => 100.0,
        GOLD_ORE | IRON_ORE | COAL_ORE | LAPIS_ORE | DIAMOND_ORE | REDSTONE_ORE | EMERALD_ORE => 3.0,
        LEAVES | LEAVES2 | SNOW | VINE | BROWN_MUSHROOM_BLOCK | RED_MUSHROOM_BLOCK | COCOA => 0.2,
        GLASS => 0.3,
        SANDSTONE => 0.8,
        OBSIDIAN => 50.0,
        MOB_SPAWNER => 5.0,
        CHEST => 2.5,
        SNOW_LAYER => 0.1,
        CACTUS => 0.4,
        PUMPKIN | MELON_BLOCK => 1.0,
        MONSTER_EGG => 0.75,
        STAINED_HARDENED_CLAY | HARDENED_CLAY => 1.25,
        _ => 0.0,
    }
}

/// Harvest level a pickaxe needs (`ItemPickaxe.canHarvestBlock`).
fn pickaxe_level(id: u8) -> i32 {
    use block::*;
    match id {
        OBSIDIAN => 3,
        DIAMOND_ORE | EMERALD_ORE | GOLD_ORE | REDSTONE_ORE => 2,
        IRON_ORE | LAPIS_ORE => 1,
        _ => 0,
    }
}

fn effective(kind: ToolKind, id: u8) -> bool {
    use block::*;
    let m = block::material(id);
    match kind {
        ToolKind::Pickaxe => matches!(m, Material::Rock | Material::PackedIce) || id == ICE,
        ToolKind::Shovel => matches!(id, GRASS | DIRT | SAND | GRAVEL | SNOW_LAYER | SNOW | CLAY | MYCELIUM),
        ToolKind::Axe => matches!(m, Material::Wood | Material::Plants | Material::Vine) || matches!(id, PUMPKIN | MELON_BLOCK),
        _ => false,
    }
}

/// `EntityPlayer.canHarvestBlock`
pub fn can_harvest(held: Option<Stack>, id: u8) -> bool {
    let m = block::material(id);
    // Material.isToolNotRequired
    let needs_tool = matches!(m, Material::Rock | Material::Snow | Material::CraftedSnow);
    if !needs_tool {
        return true;
    }
    match held.and_then(|s| tool(s.id)) {
        Some(t) => match t.kind {
            ToolKind::Pickaxe => matches!(m, Material::Rock) && t.level >= pickaxe_level(id),
            ToolKind::Shovel => id == block::SNOW_LAYER || id == block::SNOW,
            _ => false,
        },
        None => false,
    }
}

/// `EntityPlayer.getCurrentPlayerStrVsBlock` before the water/air penalties.
pub fn dig_speed(held: Option<Stack>, id: u8) -> f32 {
    match held.and_then(|s| tool(s.id)) {
        Some(t) if t.kind == ToolKind::Sword => {
            if matches!(block::material(id), Material::Plants | Material::Vine | Material::Leaves | Material::Gourd) {
                1.5
            } else {
                1.0
            }
        }
        Some(t) if effective(t.kind, id) => t.efficiency,
        _ => 1.0,
    }
}

/// `Block.getPlayerRelativeBlockHardness`: progress per tick.
pub fn break_progress(held: Option<Stack>, id: u8, in_water: bool, on_ground: bool) -> f32 {
    let h = hardness(id);
    if h < 0.0 {
        return 0.0;
    }
    let mut speed = dig_speed(held, id);
    if in_water {
        speed /= 5.0;
    }
    if !on_ground {
        speed /= 5.0;
    }
    if h == 0.0 {
        return 1.0;
    }
    if can_harvest(held, id) {
        speed / h / 30.0
    } else {
        speed / h / 100.0
    }
}

/// `Block.getDrops` (fortune 0): what breaking the block yields.
pub fn drops(id: u8, meta: i32, r: &mut JavaRandom) -> Vec<Stack> {
    use block::*;
    let one = |i: ItemId, m: i32| vec![Stack::new(i, m, 1)];
    match id {
        STONE => one(COBBLESTONE as ItemId, 0),
        GRASS | MYCELIUM => one(DIRT as ItemId, 0),
        DIRT => one(DIRT as ItemId, 0),
        GRAVEL => {
            if r.next_int_n(10) == 0 {
                one(FLINT, 0)
            } else {
                one(GRAVEL as ItemId, 0)
            }
        }
        COAL_ORE => one(COAL, 0),
        LIT_FURNACE => one(FURNACE as ItemId, 0),
        CHEST | FURNACE | CRAFTING_TABLE | TORCH => one(id as ItemId, 0),
        DIAMOND_ORE => one(DIAMOND, 0),
        EMERALD_ORE => one(EMERALD, 0),
        LAPIS_ORE => vec![Stack::new(DYE, 4, 4 + r.next_int_n(5) as u32)],
        REDSTONE_ORE => vec![Stack::new(REDSTONE, 0, 4 + r.next_int_n(2) as u32)],
        LOG | LOG2 => one(id as ItemId, meta & 3),
        LEAVES | LEAVES2 => {
            // BlockLeaves.dropBlockAsItemWithChance
            let mut v = Vec::new();
            let jungle = id == LEAVES && meta & 3 == 3;
            if r.next_int_n(if jungle { 40 } else { 20 }) == 0 {
                v.push(Stack::new(6, if id == LEAVES { meta & 3 } else { 4 + (meta & 1) }, 1));
            }
            let apple = (id == LEAVES && meta & 3 == 0) || (id == LEAVES2 && meta & 3 == 1);
            if apple && r.next_int_n(200) == 0 {
                v.push(Stack::new(APPLE, 0, 1));
            }
            v
        }
        TALLGRASS => {
            if r.next_int_n(8) == 0 {
                one(SEEDS, 0)
            } else {
                Vec::new()
            }
        }
        GLASS | DEADBUSH | MOB_SPAWNER | ICE | PACKED_ICE | VINE | MONSTER_EGG | FLOWING_WATER | WATER | FLOWING_LAVA | LAVA | FIRE => Vec::new(),
        SNOW_LAYER => vec![Stack::new(SNOWBALL, 0, (meta & 7) as u32 + 1)],
        SNOW => vec![Stack::new(SNOWBALL, 0, 4)],
        CLAY => vec![Stack::new(CLAY_BALL, 0, 4)],
        REEDS => one(REEDS_ITEM, 0),
        MELON_BLOCK => vec![Stack::new(MELON, 0, 3 + r.next_int_n(5) as u32)],
        BROWN_MUSHROOM_BLOCK | RED_MUSHROOM_BLOCK => {
            let n = (r.next_int_n(10) - 7).max(0) as u32;
            if n > 0 {
                vec![Stack::new(if id == BROWN_MUSHROOM_BLOCK { BROWN_MUSHROOM } else { RED_MUSHROOM } as ItemId, 0, n)]
            } else {
                Vec::new()
            }
        }
        COCOA => vec![Stack::new(DYE, 3, if (meta & 12) >> 2 >= 2 { 3 } else { 1 })],
        DOUBLE_PLANT => {
            let variant = meta & 7;
            if meta & 8 != 0 {
                Vec::new()
            } else if variant == 2 || variant == 3 {
                if r.next_int_n(8) == 0 {
                    one(SEEDS, 0)
                } else {
                    Vec::new()
                }
            } else {
                one(DOUBLE_PLANT as ItemId, variant)
            }
        }
        SAND | PLANKS | RED_FLOWER | STAINED_HARDENED_CLAY | SANDSTONE => one(id as ItemId, meta),
        STONE_SLAB => one(id as ItemId, meta & 7),
        _ => one(id as ItemId, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Break times in ticks, from `ceil(1 / progress)`, against the wiki values
    /// for 1.7 (seconds * 20).
    #[test]
    fn break_times_match_vanilla() {
        let ticks = |held: Option<Stack>, id: u8| (1.0 / break_progress(held, id, false, true)).ceil() as i32;
        let pick = |m: usize| Some(Stack::new(TOOL_IDS[m][2], 0, 1));
        assert_eq!(ticks(None, block::DIRT), 15); // 0.75 s
        assert_eq!(ticks(None, block::STONE), 150); // 7.5 s
        assert_eq!(ticks(pick(0), block::STONE), 23); // wood: 1.15 s
        assert_eq!(ticks(pick(1), block::STONE), 12); // stone: 0.6 s
        assert_eq!(ticks(pick(2), block::STONE), 8); // iron: 0.4 s
        assert_eq!(ticks(pick(3), block::STONE), 6); // diamond: 0.3 s
        assert_eq!(ticks(None, block::LOG), 60); // 3 s
        assert_eq!(ticks(pick(3), block::OBSIDIAN), 188); // 9.4 s
        assert_eq!(ticks(None, block::OBSIDIAN), 5000); // 250 s
        assert_eq!(break_progress(None, block::BEDROCK, false, true), 0.0);
        assert_eq!(break_progress(None, block::TALLGRASS, false, true), 1.0);
    }

    #[test]
    fn harvest_rules() {
        let pick = |m: usize| Some(Stack::new(TOOL_IDS[m][2], 0, 1));
        assert!(!can_harvest(None, block::STONE));
        assert!(can_harvest(pick(0), block::STONE));
        assert!(!can_harvest(pick(0), block::IRON_ORE));
        assert!(can_harvest(pick(1), block::IRON_ORE));
        assert!(!can_harvest(pick(2), block::OBSIDIAN));
        assert!(can_harvest(pick(3), block::OBSIDIAN));
        assert!(can_harvest(None, block::LOG));
        assert!(!can_harvest(None, block::SNOW));
    }
}
