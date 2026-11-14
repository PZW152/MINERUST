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
pub const NETHER_STAR: ItemId = 399;

pub use crate::item_tag::Tag;
use crate::world::anvil::Nbt;

/// An inventory stack: item id, damage/metadata, count and its NBT tag.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stack {
    pub id: ItemId,
    pub meta: i32,
    pub count: u32,
    /// `stackTagCompound` (interned; `Tag::NONE` for none).
    pub tag: Tag,
}

impl Stack {
    pub fn new(id: ItemId, meta: i32, count: u32) -> Stack {
        Stack {
            id,
            meta,
            count,
            tag: Tag::NONE,
        }
    }

    /// `getTagCompound`
    pub fn nbt(&self) -> Option<std::sync::Arc<Nbt>> {
        self.tag.get()
    }

    /// The stack with its tag changed by `f` (an empty compound when there
    /// was none, like `setTagCompound(new NBTTagCompound())`).
    pub fn edit_tag(mut self, f: impl FnOnce(&mut Nbt)) -> Stack {
        let mut n = self
            .nbt()
            .map_or(Nbt::Compound(Vec::new()), |t| (*t).clone());
        f(&mut n);
        self.tag = Tag::of(&n);
        self
    }

    /// `setTagCompound(null)` when the compound became empty.
    fn drop_empty_tag(mut self) -> Stack {
        if matches!(self.nbt().as_deref(), Some(Nbt::Compound(v)) if v.is_empty()) {
            self.tag = Tag::NONE;
        }
        self
    }

    /// The list key enchantments live under: `StoredEnchantments` for the
    /// enchanted book, `ench` otherwise.
    fn ench_key(&self) -> &'static str {
        if self.id == 403 {
            "StoredEnchantments"
        } else {
            "ench"
        }
    }

    /// Replaces the enchantment list with `(id, level)` pairs.
    pub fn with_enchantments(self, list: &[(i32, i32)]) -> Stack {
        let key = self.ench_key();
        let items: Vec<Nbt> = list
            .iter()
            .map(|&(id, lvl)| {
                Nbt::Compound(vec![
                    ("id".into(), Nbt::Short(id as i16)),
                    ("lvl".into(), Nbt::Short(lvl as i16)),
                ])
            })
            .collect();
        if items.is_empty() {
            return self
                .edit_tag(|n| {
                    if let Nbt::Compound(v) = n {
                        v.retain(|e| e.0 != key);
                    }
                })
                .drop_empty_tag();
        }
        self.edit_tag(|n| n.set(key, Nbt::List(10, items)))
    }

    /// `(enchantment id, level)` pairs (`getEnchantmentTagList`, or the
    /// stored ones of an enchanted book).
    pub fn enchantments(&self) -> Vec<(i32, i32)> {
        let Some(t) = self.nbt() else {
            return Vec::new();
        };
        t.list(self.ench_key())
            .iter()
            .map(|e| {
                (
                    e.int("id").unwrap_or(0) as i32,
                    e.int("lvl").unwrap_or(0) as i32,
                )
            })
            .collect()
    }

    /// `isItemEnchanted`: an `ench` list is present.
    pub fn is_enchanted(&self) -> bool {
        self.nbt()
            .is_some_and(|t| matches!(t.get("ench"), Some(Nbt::List(..))))
    }

    /// `hasDisplayName` / `getDisplayName`'s custom part.
    pub fn custom_name(&self) -> Option<String> {
        let t = self.nbt()?;
        t.get("display")?.string("Name").map(str::to_string)
    }

    /// `setStackDisplayName`, or `func_135074_t` (clearing) with `None`.
    pub fn with_name(self, name: Option<&str>) -> Stack {
        match name {
            Some(s) => self.edit_tag(|n| {
                if !matches!(n.get("display"), Some(Nbt::Compound(_))) {
                    n.set("display", Nbt::Compound(Vec::new()));
                }
                if let Some(d) = n.get_mut("display") {
                    d.set("Name", Nbt::String(s.to_string()));
                }
            }),
            None => {
                if self.custom_name().is_none() {
                    return self;
                }
                self.edit_tag(|n| {
                    let mut empty = false;
                    if let Some(Nbt::Compound(d)) = n.get_mut("display") {
                        d.retain(|e| e.0 != "Name");
                        empty = d.is_empty();
                    }
                    if empty {
                        if let Nbt::Compound(v) = n {
                            v.retain(|e| e.0 != "display");
                        }
                    }
                })
                .drop_empty_tag()
            }
        }
    }

    /// `getRepairCost`
    pub fn repair_cost(&self) -> i32 {
        self.nbt().and_then(|t| t.int("RepairCost")).unwrap_or(0) as i32
    }

    /// `setRepairCost`
    pub fn with_repair_cost(self, c: i32) -> Stack {
        self.edit_tag(|n| n.set("RepairCost", Nbt::Int(c)))
    }

    /// `ItemArmor.getColor`'s stored `display.color`.
    pub fn dye_color(&self) -> Option<i32> {
        let t = self.nbt()?;
        t.get("display")?.int("color").map(|c| c as i32)
    }

    /// Save-file form: `id:meta:count[:t<hex of the tag NBT>]`.
    pub fn to_code(&self) -> String {
        match self.nbt() {
            Some(t) => {
                let b = crate::world::anvil::write_nbt(&t);
                let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
                format!("{}:{}:{}:t{}", self.id, self.meta, self.count, hex)
            }
            None => format!("{}:{}:{}", self.id, self.meta, self.count),
        }
    }

    pub fn from_code(s: &str) -> Option<Stack> {
        let f: Vec<&str> = s.split(':').collect();
        if f.len() != 3 && f.len() != 4 {
            return None;
        }
        let mut st = Stack::new(f[0].parse().ok()?, f[1].parse().ok()?, f[2].parse().ok()?);
        if f.len() == 4 {
            if let Some(hex) = f[3].strip_prefix('t') {
                if hex.len() % 2 != 0 {
                    return None;
                }
                let bytes: Option<Vec<u8>> = (0..hex.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
                    .collect();
                let n = crate::world::anvil::read_nbt(&bytes?).ok()?;
                st.tag = Tag::of(&n);
            } else {
                // the older packed enchantment form: (id: 6 bits, level: 3) x 7
                let mut v: u64 = f[3].parse().ok()?;
                let mut list = Vec::new();
                while v != 0 {
                    let e = v & 511;
                    list.push(((e & 63) as i32, (e >> 6) as i32));
                    v >>= 9;
                }
                st = st.with_enchantments(&list);
            }
        }
        if st.count == 0 || st.count > 64 {
            return None;
        }
        Some(st)
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
        self.id == o.id && (self.meta == o.meta || max_damage(self.id) > 0) && self.tag == o.tag
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

const MATERIALS: [(i32, i32, f32, &str); 5] = [
    (0, 59, 2.0, "wood"),
    (1, 131, 4.0, "stone"),
    (2, 250, 6.0, "iron"),
    (3, 1561, 8.0, "diamond"),
    (0, 32, 12.0, "gold"),
];

/// Tool item ids: `[sword, shovel, pickaxe, axe, hoe]` per material.
const TOOL_IDS: [[ItemId; 5]; 5] = [
    [268, 269, 270, 271, 290],
    [272, 273, 274, 275, 291],
    [267, 256, 257, 258, 292],
    [276, 277, 278, 279, 293],
    [283, 284, 285, 286, 294],
];
const KINDS: [(ToolKind, &str); 5] = [
    (ToolKind::Sword, "sword"),
    (ToolKind::Shovel, "shovel"),
    (ToolKind::Pickaxe, "pickaxe"),
    (ToolKind::Axe, "axe"),
    (ToolKind::Hoe, "hoe"),
];

/// The "Weapon modifier" of `Item.getItemAttributeModifiers` (operation 0
/// on `attackDamage`): `ItemSword` 4 + the material's
/// `getDamageVsEntity`, `ItemTool` its base (axe 3, pickaxe 2, spade 1)
/// plus the same. Hoes are not `ItemTool`s and have none.
pub fn weapon_damage(id: ItemId) -> Option<f32> {
    let t = tool(id)?;
    // ToolMaterial.getDamageVsEntity: wood 0, stone 1, iron 2, emerald 3, gold 0
    let mat = match t.max_uses {
        131 => 1.0,
        250 => 2.0,
        1561 => 3.0,
        _ => 0.0,
    };
    match t.kind {
        ToolKind::Sword => Some(4.0 + mat),
        ToolKind::Axe => Some(3.0 + mat),
        ToolKind::Pickaxe => Some(2.0 + mat),
        ToolKind::Shovel => Some(1.0 + mat),
        ToolKind::Hoe => None,
    }
}

pub fn tool(id: ItemId) -> Option<ToolInfo> {
    for (m, row) in TOOL_IDS.iter().enumerate() {
        for (k, &tid) in row.iter().enumerate() {
            if tid == id {
                let (level, max_uses, efficiency, _) = MATERIALS[m];
                return Some(ToolInfo {
                    kind: KINDS[k].0,
                    level,
                    max_uses,
                    efficiency,
                });
            }
        }
    }
    None
}

/// `Item.getItemStackLimit`
pub fn max_stack(id: ItemId) -> u32 {
    if id < 256 {
        // Block items: doors and beds are items; signs and cakes too.
        return 64;
    }
    crate::item_table::get(id).map_or(64, |d| d.stack)
}

/// `Item.getHasSubtypes`: the damage value tells items apart. Damageable
/// items use it as wear instead.
pub fn has_subtypes(id: ItemId) -> bool {
    max_damage(id) == 0
}

/// `Item.getMaxDamage`
pub fn max_damage(id: ItemId) -> i32 {
    if id < 256 {
        0
    } else {
        crate::item_table::get(id).map_or(0, |d| d.max_damage)
    }
}

/// Texture file stem under `textures/items/` for non-block items.
pub fn item_texture(id: ItemId, meta: i32) -> Option<String> {
    if let Some((icon, _)) = crate::item_table::variant(id, meta) {
        return Some(icon.to_string());
    }
    if let Some(d) = crate::item_table::get(id) {
        return Some(d.icon.to_string());
    }
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
            DYE => match meta {
                2 => "dye_powder_green",
                3 => "dye_powder_brown",
                _ => "dye_powder_blue",
            },
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
    if let Some((_, name)) = crate::item_table::variant(s.id, s.meta) {
        return name.to_string();
    }
    if let Some(d) = crate::item_table::get(s.id) {
        return d.name.to_string();
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
            RED_FLOWER => [
                "Poppy",
                "Blue Orchid",
                "Allium",
                "Azure Bluet",
                "Red Tulip",
                "Orange Tulip",
                "White Tulip",
                "Pink Tulip",
                "Oxeye Daisy",
            ][m.min(8)]
            .into(),
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
            DOUBLE_PLANT => [
                "Sunflower",
                "Lilac",
                "Double Tallgrass",
                "Large Fern",
                "Rose Bush",
                "Peony",
            ][m.min(5)]
            .into(),
            SAPLING => format!("{} Sapling", wood[(m & 7).min(5)]),
            SPONGE => "Sponge".into(),
            LAPIS_BLOCK => "Lapis Lazuli Block".into(),
            DISPENSER => "Dispenser".into(),
            NOTEBLOCK => "Note Block".into(),
            GOLDEN_RAIL => "Powered Rail".into(),
            DETECTOR_RAIL => "Detector Rail".into(),
            STICKY_PISTON => "Sticky Piston".into(),
            WEB => "Cobweb".into(),
            PISTON => "Piston".into(),
            WOOL => format!("{} Wool", COLOR_NAMES[m]),
            CARPET => format!("{} Carpet", COLOR_NAMES[m]),
            GOLD_BLOCK => "Block of Gold".into(),
            IRON_BLOCK => "Block of Iron".into(),
            DOUBLE_STONE_SLAB => "Stone Slab".into(),
            BRICK_BLOCK => "Bricks".into(),
            TNT => "TNT".into(),
            BOOKSHELF => "Bookshelf".into(),
            OAK_STAIRS => "Oak Wood Stairs".into(),
            SPRUCE_STAIRS => "Spruce Wood Stairs".into(),
            BIRCH_STAIRS => "Birch Wood Stairs".into(),
            JUNGLE_STAIRS => "Jungle Wood Stairs".into(),
            ACACIA_STAIRS => "Acacia Wood Stairs".into(),
            DARK_OAK_STAIRS => "Dark Oak Wood Stairs".into(),
            STONE_STAIRS => "Cobblestone Stairs".into(),
            BRICK_STAIRS => "Brick Stairs".into(),
            STONE_BRICK_STAIRS => "Stone Brick Stairs".into(),
            NETHER_BRICK_STAIRS => "Nether Brick Stairs".into(),
            SANDSTONE_STAIRS => "Sandstone Stairs".into(),
            REDSTONE_WIRE => "Redstone Dust".into(),
            DIAMOND_BLOCK => "Block of Diamond".into(),
            WHEAT => "Crops".into(),
            FARMLAND => "Farmland".into(),
            WOODEN_DOOR => "Wooden Door".into(),
            LADDER => "Ladder".into(),
            RAIL => "Rail".into(),
            LEVER => "Lever".into(),
            STONE_PRESSURE_PLATE => "Stone Pressure Plate".into(),
            IRON_DOOR => "Iron Door".into(),
            WOODEN_PRESSURE_PLATE => "Wooden Pressure Plate".into(),
            LIT_REDSTONE_ORE => "Redstone Ore".into(),
            UNLIT_REDSTONE_TORCH | REDSTONE_TORCH => "Redstone Torch".into(),
            STONE_BUTTON => "Button".into(),
            WOODEN_BUTTON => "Button".into(),
            JUKEBOX => "Jukebox".into(),
            FENCE => "Fence".into(),
            NETHERRACK => "Netherrack".into(),
            SOUL_SAND => "Soul Sand".into(),
            GLOWSTONE => "Glowstone".into(),
            LIT_PUMPKIN => "Jack o'Lantern".into(),
            UNPOWERED_REPEATER | POWERED_REPEATER => "Redstone Repeater".into(),
            STONEBRICK => [
                "Stone Bricks",
                "Mossy Stone Bricks",
                "Cracked Stone Bricks",
                "Chiseled Stone Bricks",
            ][m & 3]
                .into(),
            IRON_BARS => "Iron Bars".into(),
            GLASS_PANE => "Glass Pane".into(),
            NETHER_BRICK => "Nether Brick".into(),
            CAULDRON => "Cauldron".into(),
            END_PORTAL_FRAME => "End Portal".into(),
            END_STONE => "End Stone".into(),
            TRIPWIRE_HOOK => "Tripwire Hook".into(),
            TRIPWIRE => "Tripwire".into(),
            EMERALD_BLOCK => "Block of Emerald".into(),
            FLOWER_POT => "Flower Pot".into(),
            CARROTS => "Carrots".into(),
            POTATOES => "Potatoes".into(),
            REDSTONE_BLOCK => "Block of Redstone".into(),
            TRAPDOOR => "Trapdoor".into(),
            PORTAL => "Portal".into(),
            NETHER_BRICK_FENCE => "Nether Brick Fence".into(),
            NETHER_WART => "Nether Wart".into(),
            END_PORTAL => "End Portal".into(),
            DRAGON_EGG => "Dragon Egg".into(),
            QUARTZ_ORE => "Nether Quartz Ore".into(),
            REDSTONE_LAMP | LIT_REDSTONE_LAMP => "Redstone Lamp".into(),
            QUARTZ_BLOCK => "Block of Quartz".into(),
            HAY_BLOCK => "Hay Bale".into(),
            COAL_BLOCK => "Block of Coal".into(),
            _ => format!("tile.{id}"),
        },
        id => format!("item.{id}"),
    };
    name
}

/// Every non-block item id that has a texture (for the atlas).
pub fn all_items() -> Vec<(ItemId, i32)> {
    let mut v: Vec<(ItemId, i32)> = crate::item_table::ITEMS.iter().map(|d| (d.id, 0)).collect();
    v.push((COAL, 1));
    for m in 1..16 {
        v.push((DYE, m));
    }
    for m in 1..4 {
        v.push((349, m));
    }
    v.push((350, 1));
    for m in 1..5 {
        v.push((397, m));
    }
    // ItemBow.getItemIconForUseDuration: bow_pulling_0..2
    for m in 1000..1003 {
        v.push((261, m));
    }
    v
}

pub const COLOR_NAMES: [&str; 16] = [
    "White",
    "Orange",
    "Magenta",
    "Light Blue",
    "Yellow",
    "Lime",
    "Pink",
    "Gray",
    "Light Gray",
    "Cyan",
    "Purple",
    "Blue",
    "Brown",
    "Green",
    "Red",
    "Black",
];

/// `Block.blockHardness`; negative means unbreakable.
pub fn hardness(id: u8) -> f32 {
    use block::*;
    match id {
        STONE => 1.5,
        GRASS | GRAVEL | CLAY | MYCELIUM => 0.6,
        DIRT | SAND | ICE | PACKED_ICE => 0.5,
        COBBLESTONE | PLANKS | LOG | LOG2 | STONE_SLAB | MOSSY_COBBLESTONE | WOODEN_SLAB
        | DOUBLE_WOODEN_SLAB => 2.0,
        BEDROCK | PISTON_EXTENSION => -1.0,
        FLOWING_WATER | WATER | FLOWING_LAVA | LAVA => 100.0,
        GOLD_ORE | IRON_ORE | COAL_ORE | LAPIS_ORE | DIAMOND_ORE | REDSTONE_ORE | EMERALD_ORE => {
            3.0
        }
        LEAVES | LEAVES2 | SNOW | VINE | BROWN_MUSHROOM_BLOCK | RED_MUSHROOM_BLOCK | COCOA => 0.2,
        GLASS | STAINED_GLASS => 0.3,
        SANDSTONE | QUARTZ_STAIRS => 0.8,
        OBSIDIAN => 50.0,
        MOB_SPAWNER => 5.0,
        CHEST => 2.5,
        SNOW_LAYER => 0.1,
        CACTUS => 0.4,
        PUMPKIN | MELON_BLOCK => 1.0,
        MONSTER_EGG => 0.75,
        STAINED_HARDENED_CLAY | HARDENED_CLAY => 1.25,
        SPONGE | FARMLAND => 0.6,
        LAPIS_BLOCK | GOLD_BLOCK | END_STONE | WOODEN_DOOR | LIT_REDSTONE_ORE => 3.0,
        DISPENSER => 3.5,
        NOTEBLOCK | WOOL | SANDSTONE_STAIRS | QUARTZ_BLOCK => 0.8,
        RAIL | GOLDEN_RAIL | DETECTOR_RAIL => 0.7,
        PISTON
        | STICKY_PISTON
        | LEVER
        | STONE_PRESSURE_PLATE
        | WOODEN_PRESSURE_PLATE
        | STONE_BUTTON
        | WOODEN_BUTTON
        | SOUL_SAND
        | HAY_BLOCK => 0.5,
        WEB => 4.0,
        IRON_BLOCK | DIAMOND_BLOCK | EMERALD_BLOCK | IRON_DOOR | IRON_BARS | REDSTONE_BLOCK
        | COAL_BLOCK => 5.0,
        DOUBLE_STONE_SLAB | BRICK_BLOCK | OAK_STAIRS | SPRUCE_STAIRS | BIRCH_STAIRS
        | JUNGLE_STAIRS | ACACIA_STAIRS | DARK_OAK_STAIRS | STONE_STAIRS | BRICK_STAIRS
        | NETHER_BRICK_STAIRS | JUKEBOX | FENCE | NETHER_BRICK | CAULDRON => 2.0,
        BREWING_STAND => 0.5,
        BOOKSHELF | STONEBRICK | STONE_BRICK_STAIRS => 1.5,
        LADDER | NETHERRACK => 0.4,
        STANDING_SIGN | WALL_SIGN => 1.0,
        ENCHANTING_TABLE => 5.0,
        ANVIL => 5.0,
        GLOWSTONE | GLASS_PANE => 0.3,
        LIT_PUMPKIN => 1.0,
        END_PORTAL_FRAME => -1.0,
        CARPET => 0.1,
        REDSTONE_LAMP | LIT_REDSTONE_LAMP => 0.3,
        TRAPDOOR => 3.0,
        PORTAL | END_PORTAL => -1.0,
        NETHER_BRICK_FENCE => 2.0,
        DRAGON_EGG => 3.0,
        QUARTZ_ORE => 3.0,
        PISTON_HEAD => 0.5,
        _ => 0.0,
    }
}

/// Harvest level a pickaxe needs (`ItemPickaxe.canHarvestBlock`).
fn pickaxe_level(id: u8) -> i32 {
    use block::*;
    match id {
        OBSIDIAN => 3,
        DIAMOND_ORE | EMERALD_ORE | GOLD_ORE | REDSTONE_ORE | LIT_REDSTONE_ORE | DIAMOND_BLOCK
        | EMERALD_BLOCK | GOLD_BLOCK => 2,
        IRON_ORE | LAPIS_ORE | IRON_BLOCK | LAPIS_BLOCK => 1,
        _ => 0,
    }
}

fn effective(kind: ToolKind, id: u8) -> bool {
    use block::*;
    let m = block::material(id);
    match kind {
        ToolKind::Pickaxe => {
            matches!(
                m,
                Material::Rock | Material::PackedIce | Material::Iron | Material::Anvil
            ) || id == ICE
                || is_rail(id)
                || matches!(id, STONE_BUTTON | STONE_PRESSURE_PLATE)
        }
        ToolKind::Shovel => matches!(
            id,
            GRASS
                | DIRT
                | SAND
                | GRAVEL
                | SNOW_LAYER
                | SNOW
                | CLAY
                | MYCELIUM
                | FARMLAND
                | SOUL_SAND
        ),
        ToolKind::Axe => {
            matches!(m, Material::Wood | Material::Plants | Material::Vine)
                || matches!(id, PUMPKIN | MELON_BLOCK)
        }
        _ => false,
    }
}

/// `EntityPlayer.canHarvestBlock`
pub fn can_harvest(held: Option<Stack>, id: u8) -> bool {
    let m = block::material(id);
    // Material.isToolNotRequired
    let needs_tool = matches!(
        m,
        Material::Rock
            | Material::Snow
            | Material::CraftedSnow
            | Material::Iron
            | Material::Anvil
            | Material::Web
    );
    if m == Material::Web {
        // ItemSword / ItemShears.func_150897_b
        return held.map_or(false, |s| {
            s.id == 359 || tool(s.id).map_or(false, |t| t.kind == ToolKind::Sword)
        });
    }
    if !needs_tool {
        return true;
    }
    match held.and_then(|s| tool(s.id)) {
        Some(t) => match t.kind {
            ToolKind::Pickaxe => {
                matches!(m, Material::Rock | Material::Iron | Material::Anvil)
                    && t.level >= pickaxe_level(id)
            }
            ToolKind::Shovel => id == block::SNOW_LAYER || id == block::SNOW,
            _ => false,
        },
        None => false,
    }
}

/// `EntityPlayer.getCurrentPlayerStrVsBlock` before the water/air penalties.
pub fn dig_speed(held: Option<Stack>, id: u8) -> f32 {
    match held.and_then(|s| tool(s.id)) {
        _ if id == block::WEB && held.map_or(false, |s| s.id == 359) => 15.0,
        Some(t) if t.kind == ToolKind::Sword => {
            if id == block::WEB {
                15.0
            } else if matches!(
                block::material(id),
                Material::Plants | Material::Vine | Material::Leaves | Material::Gourd
            ) {
                1.5
            } else {
                1.0
            }
        }
        Some(t) if effective(t.kind, id) => t.efficiency,
        _ => 1.0,
    }
}

/// What `EntityPlayer.getCurrentPlayerStrVsBlock` reads off the player
/// besides the held stack.
#[derive(Clone, Copy, Debug, Default)]
pub struct Digger {
    /// `isInsideOfMaterial(Material.water)` at the eyes
    pub in_water: bool,
    pub on_ground: bool,
    /// amplifiers of Haste (`digSpeed`) and Mining Fatigue (`digSlowdown`)
    pub haste: Option<i32>,
    pub fatigue: Option<i32>,
    /// `EnchantmentHelper.getAquaAffinityModifier` of the worn armour
    pub aqua_affinity: bool,
}

/// `EntityPlayer.getCurrentPlayerStrVsBlock`
pub fn player_dig_speed(held: Option<Stack>, id: u8, p: &Digger) -> f32 {
    let mut f = dig_speed(held, id);
    if f > 1.0 {
        let e = crate::enchant::level(crate::enchant::EFFICIENCY, held.as_ref());
        if e > 0 && held.is_some() {
            let k = (e * e + 1) as f32;
            // !canItemHarvestBlock && !(f > 1): never true inside f > 1
            f += k;
        }
    }
    if let Some(a) = p.haste {
        f *= 1.0 + (a + 1) as f32 * 0.2;
    }
    if let Some(a) = p.fatigue {
        f *= 1.0 - (a + 1) as f32 * 0.2;
    }
    if p.in_water && !p.aqua_affinity {
        f /= 5.0;
    }
    if !p.on_ground {
        f /= 5.0;
    }
    f
}

/// `break_progress_by` for a player with no effects or armour.
pub fn break_progress(held: Option<Stack>, id: u8, in_water: bool, on_ground: bool) -> f32 {
    break_progress_by(
        held,
        id,
        &Digger {
            in_water,
            on_ground,
            ..Default::default()
        },
    )
}

/// `Block.getPlayerRelativeBlockHardness`: progress per tick.
pub fn break_progress_by(held: Option<Stack>, id: u8, p: &Digger) -> f32 {
    let h = hardness(id);
    if h < 0.0 {
        return 0.0;
    }
    let speed = player_dig_speed(held, id, p);
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
        CHEST | FURNACE | CRAFTING_TABLE | TORCH | ENCHANTING_TABLE => one(id as ItemId, 0),
        // BlockAnvil.damageDropped
        ANVIL => one(id as ItemId, meta >> 2),
        // BlockSign.getItemDropped
        STANDING_SIGN | WALL_SIGN => one(323, 0),
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
                v.push(Stack::new(
                    6,
                    if id == LEAVES {
                        meta & 3
                    } else {
                        4 + (meta & 1)
                    },
                    1,
                ));
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
        GLASS | DEADBUSH | MOB_SPAWNER | ICE | PACKED_ICE | VINE | MONSTER_EGG | FLOWING_WATER
        | WATER | FLOWING_LAVA | LAVA | FIRE => Vec::new(),
        SNOW_LAYER => vec![Stack::new(SNOWBALL, 0, (meta & 7) as u32 + 1)],
        SNOW => vec![Stack::new(SNOWBALL, 0, 4)],
        CLAY => vec![Stack::new(CLAY_BALL, 0, 4)],
        REEDS => one(REEDS_ITEM, 0),
        MELON_BLOCK => vec![Stack::new(MELON, 0, 3 + r.next_int_n(5) as u32)],
        BROWN_MUSHROOM_BLOCK | RED_MUSHROOM_BLOCK => {
            let n = (r.next_int_n(10) - 7).max(0) as u32;
            if n > 0 {
                vec![Stack::new(
                    if id == BROWN_MUSHROOM_BLOCK {
                        BROWN_MUSHROOM
                    } else {
                        RED_MUSHROOM
                    } as ItemId,
                    0,
                    n,
                )]
            } else {
                Vec::new()
            }
        }
        COCOA => vec![Stack::new(
            DYE,
            3,
            if (meta & 12) >> 2 >= 2 { 3 } else { 1 },
        )],
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
        SAND
        | PLANKS
        | RED_FLOWER
        | STAINED_HARDENED_CLAY
        | SANDSTONE
        | WOOL
        | CARPET
        | STONEBRICK
        | QUARTZ_BLOCK => one(id as ItemId, meta),
        SAPLING => one(id as ItemId, meta & 7),
        DOUBLE_STONE_SLAB => vec![Stack::new(STONE_SLAB as ItemId, meta & 7, 2)],
        WOODEN_DOOR => {
            if meta & 8 == 0 {
                one(324, 0)
            } else {
                Vec::new()
            }
        }
        IRON_DOOR => {
            if meta & 8 == 0 {
                one(330, 0)
            } else {
                Vec::new()
            }
        }
        WEB | TRIPWIRE => one(STRING, 0),
        REDSTONE_WIRE => one(REDSTONE, 0),
        LIT_REDSTONE_ORE => vec![Stack::new(REDSTONE, 0, 4 + r.next_int_n(2) as u32)],
        UNLIT_REDSTONE_TORCH | REDSTONE_TORCH => one(REDSTONE_TORCH as ItemId, 0),
        UNPOWERED_REPEATER | POWERED_REPEATER => one(356, 0),
        CAULDRON => one(380, 0),
        // BlockBrewingStand.getItemDropped
        BREWING_STAND => one(379, 0),
        REDSTONE_LAMP | LIT_REDSTONE_LAMP => one(REDSTONE_LAMP as ItemId, 0),
        PISTON_HEAD | PORTAL | END_PORTAL => Vec::new(),
        QUARTZ_ORE => one(406, 0),
        NETHER_WART => {
            // BlockNetherWart.getDrops: 1, or 2-4 when ripe
            let n = if meta >= 3 { 2 + r.next_int_n(3) } else { 1 };
            vec![Stack::new(372, 0, n as u32)]
        }
        FLOWER_POT => one(390, 0),
        BOOKSHELF => vec![Stack::new(340, 0, 3)],
        GLOWSTONE => vec![Stack::new(348, 0, 2 + r.next_int_n(3) as u32)],
        FARMLAND => one(DIRT as ItemId, 0),
        GLASS_PANE | END_PORTAL_FRAME => Vec::new(),
        WHEAT | CARROTS | POTATOES => {
            // BlockCrops.getDrops (fortune 0)
            let mut v = Vec::new();
            let (crop, seed) = match id {
                WHEAT => (296, SEEDS),
                CARROTS => (391, 391),
                _ => (392, 392),
            };
            if meta >= 7 {
                v.push(Stack::new(crop, 0, 1));
                for _ in 0..3 {
                    if r.next_int_n(15) <= meta {
                        v.push(Stack::new(seed, 0, 1));
                    }
                }
                if id == POTATOES && r.next_int_n(50) == 0 {
                    v.push(Stack::new(394, 0, 1));
                }
            } else {
                v.push(Stack::new(seed, 0, 1));
            }
            v
        }
        LEVER
        | STONE_BUTTON
        | WOODEN_BUTTON
        | STONE_PRESSURE_PLATE
        | WOODEN_PRESSURE_PLATE
        | TRIPWIRE_HOOK
        | LADDER
        | FENCE
        | IRON_BARS => one(id as ItemId, 0),
        RAIL | GOLDEN_RAIL | DETECTOR_RAIL | DISPENSER | PISTON | STICKY_PISTON | TNT
        | LIT_PUMPKIN => one(id as ItemId, 0),
        id if is_stairs(id) => one(id as ItemId, 0),
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
        let ticks = |held: Option<Stack>, id: u8| {
            (1.0 / break_progress(held, id, false, true)).ceil() as i32
        };
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
