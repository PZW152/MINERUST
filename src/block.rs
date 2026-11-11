//! Block registry: the subset of `Block.registerBlocks` that world generation
//! can produce, with the properties the generator, lighting and renderer read.

use std::sync::LazyLock;

pub const AIR: u8 = 0;
pub const STONE: u8 = 1;
pub const GRASS: u8 = 2;
pub const DIRT: u8 = 3;
pub const COBBLESTONE: u8 = 4;
pub const PLANKS: u8 = 5;
pub const BEDROCK: u8 = 7;
pub const FLOWING_WATER: u8 = 8;
pub const WATER: u8 = 9;
pub const FLOWING_LAVA: u8 = 10;
pub const LAVA: u8 = 11;
pub const SAND: u8 = 12;
pub const GRAVEL: u8 = 13;
pub const GOLD_ORE: u8 = 14;
pub const IRON_ORE: u8 = 15;
pub const COAL_ORE: u8 = 16;
pub const LOG: u8 = 17;
pub const LEAVES: u8 = 18;
pub const GLASS: u8 = 20;
pub const LAPIS_ORE: u8 = 21;
pub const SANDSTONE: u8 = 24;
pub const TALLGRASS: u8 = 31;
pub const DEADBUSH: u8 = 32;
pub const YELLOW_FLOWER: u8 = 37;
pub const RED_FLOWER: u8 = 38;
pub const BROWN_MUSHROOM: u8 = 39;
pub const RED_MUSHROOM: u8 = 40;
pub const STONE_SLAB: u8 = 44;
pub const MOSSY_COBBLESTONE: u8 = 48;
pub const OBSIDIAN: u8 = 49;
pub const FIRE: u8 = 51;
pub const MOB_SPAWNER: u8 = 52;
pub const CHEST: u8 = 54;
pub const DIAMOND_ORE: u8 = 56;
pub const FARMLAND: u8 = 60;
pub const REDSTONE_ORE: u8 = 73;
pub const SNOW_LAYER: u8 = 78;
pub const ICE: u8 = 79;
pub const SNOW: u8 = 80;
pub const CACTUS: u8 = 81;
pub const CLAY: u8 = 82;
pub const REEDS: u8 = 83;
pub const PUMPKIN: u8 = 86;
pub const MONSTER_EGG: u8 = 97;
pub const BROWN_MUSHROOM_BLOCK: u8 = 99;
pub const RED_MUSHROOM_BLOCK: u8 = 100;
pub const MELON_BLOCK: u8 = 103;
pub const VINE: u8 = 106;
pub const MYCELIUM: u8 = 110;
pub const WATERLILY: u8 = 111;
pub const COCOA: u8 = 127;
pub const EMERALD_ORE: u8 = 129;
pub const STAINED_HARDENED_CLAY: u8 = 159;
pub const LEAVES2: u8 = 161;
pub const LOG2: u8 = 162;
pub const HARDENED_CLAY: u8 = 172;
pub const PACKED_ICE: u8 = 174;
pub const DOUBLE_PLANT: u8 = 175;

/// How the block is tessellated.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    None,
    Cube,
    Liquid,
    /// Two crossed quads (render type 1).
    Cross,
    /// Double plant (render type 40).
    DoublePlant,
    Cactus,
    Vine,
    LilyPad,
    SnowLayer,
    Slab,
    Chest,
    Cocoa,
}

/// `net.minecraft.block.material.Material`
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Material {
    Air,
    Grass,
    Ground,
    Wood,
    Rock,
    Water,
    Lava,
    Leaves,
    Plants,
    Vine,
    Sand,
    Glass,
    Ice,
    PackedIce,
    Snow,
    CraftedSnow,
    Cactus,
    Clay,
    Gourd,
    Fire,
}

impl Material {
    #[inline]
    pub fn is_liquid(self) -> bool {
        matches!(self, Material::Water | Material::Lava)
    }
    /// `Material.isSolid()`
    #[inline]
    pub fn is_solid(self) -> bool {
        !matches!(self, Material::Air | Material::Water | Material::Lava | Material::Plants | Material::Vine | Material::Snow | Material::Fire)
    }
    /// `Material.blocksMovement()`
    #[inline]
    pub fn blocks_movement(self) -> bool {
        self.is_solid()
    }
    #[inline]
    pub fn replaceable(self) -> bool {
        matches!(self, Material::Air | Material::Water | Material::Lava | Material::Vine | Material::Snow | Material::Fire)
    }
    #[inline]
    fn translucent(self) -> bool {
        matches!(self, Material::Leaves | Material::Glass | Material::Ice | Material::Snow | Material::Cactus)
    }
    /// `Material.isOpaque()`
    #[inline]
    pub fn is_opaque(self) -> bool {
        !self.translucent() && self.blocks_movement()
    }
    #[inline]
    pub fn can_burn(self) -> bool {
        matches!(self, Material::Wood | Material::Leaves | Material::Vine)
    }
    /// `Material.getCanBlockGrass()`
    #[inline]
    fn can_block_grass(self) -> bool {
        !matches!(self, Material::Air | Material::Plants | Material::Vine | Material::Snow | Material::Fire)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BlockInfo {
    pub shape: Shape,
    pub material: Material,
    /// `Block.isOpaqueCube()` with fancy graphics.
    pub opaque: bool,
    /// `Block.func_149730_j()` (isFullBlock): `isOpaqueCube()` as evaluated in the
    /// constructor. Leaves report true there because the fancy-graphics flag
    /// is not set yet.
    pub full_block: bool,
    /// `Block.renderAsNormalBlock()`
    pub normal: bool,
    /// `Block.getLightOpacity()`
    pub light_opacity: u8,
    /// `Block.getLightValue()`
    pub light_value: u8,
    /// Has a full-cube collision box.
    pub solid: bool,
    /// Rendered in the translucent pass (render pass 1).
    pub translucent: bool,
    /// `Block.slipperiness`
    pub slipperiness: f32,
    /// `Block.getCanBlockGrass()`
    pub can_block_grass: bool,
    /// `Block.getUseNeighborBrightness()`
    pub use_neighbor_brightness: bool,
    /// Registered at all.
    pub exists: bool,
}

impl BlockInfo {
    /// `Block.isNormalCube()`
    #[inline]
    pub fn normal_cube(&self) -> bool {
        self.material.is_opaque() && self.normal
    }
}

fn base(shape: Shape, material: Material, opaque: bool) -> BlockInfo {
    BlockInfo {
        shape,
        material,
        opaque,
        full_block: opaque,
        normal: opaque,
        light_opacity: if opaque { 255 } else { 0 },
        light_value: 0,
        solid: opaque,
        translucent: false,
        slipperiness: 0.6,
        can_block_grass: !material.can_block_grass(),
        use_neighbor_brightness: false,
        exists: true,
    }
}

fn build() -> [BlockInfo; 256] {
    use Material as M;
    let mut none = base(Shape::None, M::Air, false);
    none.exists = false;
    let mut t = [none; 256];
    t[AIR as usize].exists = true;
    let cube = |m: Material| base(Shape::Cube, m, true);
    for (id, m) in [
        (STONE, M::Rock),
        (GRASS, M::Grass),
        (DIRT, M::Ground),
        (COBBLESTONE, M::Rock),
        (PLANKS, M::Wood),
        (BEDROCK, M::Rock),
        (SAND, M::Sand),
        (GRAVEL, M::Sand),
        (GOLD_ORE, M::Rock),
        (IRON_ORE, M::Rock),
        (COAL_ORE, M::Rock),
        (LOG, M::Wood),
        (LOG2, M::Wood),
        (LAPIS_ORE, M::Rock),
        (SANDSTONE, M::Rock),
        (MOSSY_COBBLESTONE, M::Rock),
        (OBSIDIAN, M::Rock),
        (DIAMOND_ORE, M::Rock),
        (REDSTONE_ORE, M::Rock),
        (SNOW, M::CraftedSnow),
        (CLAY, M::Clay),
        (PUMPKIN, M::Gourd),
        (MONSTER_EGG, M::Clay),
        (BROWN_MUSHROOM_BLOCK, M::Wood),
        (RED_MUSHROOM_BLOCK, M::Wood),
        (MELON_BLOCK, M::Gourd),
        (MYCELIUM, M::Grass),
        (EMERALD_ORE, M::Rock),
        (STAINED_HARDENED_CLAY, M::Rock),
        (HARDENED_CLAY, M::Rock),
        (PACKED_ICE, M::PackedIce),
    ] {
        t[id as usize] = cube(m);
    }
    t[PACKED_ICE as usize].slipperiness = 0.98;

    let mut water = base(Shape::Liquid, M::Water, false);
    water.light_opacity = 3;
    water.translucent = true;
    t[FLOWING_WATER as usize] = water;
    t[WATER as usize] = water;
    let mut lava = base(Shape::Liquid, M::Lava, false);
    lava.light_value = 15;
    t[FLOWING_LAVA as usize] = lava;
    t[LAVA as usize] = lava;

    let mut ice = base(Shape::Cube, M::Ice, false);
    ice.light_opacity = 3;
    ice.solid = true;
    ice.translucent = true;
    ice.slipperiness = 0.98;
    t[ICE as usize] = ice;

    let mut leaves = base(Shape::Cube, M::Leaves, false);
    leaves.light_opacity = 1;
    leaves.solid = true;
    leaves.full_block = true;
    t[LEAVES as usize] = leaves;
    t[LEAVES2 as usize] = leaves;

    let mut glass = base(Shape::Cube, M::Glass, false);
    glass.solid = true;
    t[GLASS as usize] = glass;

    let mut spawner = base(Shape::Cube, M::Rock, false);
    spawner.solid = true;
    t[MOB_SPAWNER as usize] = spawner;

    let plant = base(Shape::Cross, M::Plants, false);
    for id in [DEADBUSH, YELLOW_FLOWER, RED_FLOWER, RED_MUSHROOM, REEDS] {
        t[id as usize] = plant;
    }
    // BlockTallGrass uses Material.vine (replaceable).
    t[TALLGRASS as usize] = base(Shape::Cross, M::Vine, false);
    let mut brown = plant;
    brown.light_value = 1;
    t[BROWN_MUSHROOM as usize] = brown;
    // BlockDoublePlant: Material.plants; replaceability is a block override.
    t[DOUBLE_PLANT as usize] = base(Shape::DoublePlant, M::Plants, false);
    t[VINE as usize] = base(Shape::Vine, M::Vine, false);
    t[WATERLILY as usize] = base(Shape::LilyPad, M::Plants, false);
    t[SNOW_LAYER as usize] = base(Shape::SnowLayer, M::Snow, false);
    t[CACTUS as usize] = base(Shape::Cactus, M::Cactus, false);
    t[COCOA as usize] = base(Shape::Cocoa, M::Plants, false);
    t[FIRE as usize] = base(Shape::None, M::Fire, false);
    t[FIRE as usize].light_value = 15;

    let mut slab = base(Shape::Slab, M::Rock, false);
    slab.light_opacity = 255;
    t[STONE_SLAB as usize] = slab;
    t[CHEST as usize] = base(Shape::Chest, M::Wood, false);

    // Block.registerBlocks: useNeighborBrightness.
    for (id, b) in t.iter_mut().enumerate() {
        if b.material == M::Air {
            continue;
        }
        b.use_neighbor_brightness = id == STONE_SLAB as usize || id == FARMLAND as usize || b.can_block_grass || b.light_opacity == 0;
    }
    t
}

static TABLE: LazyLock<[BlockInfo; 256]> = LazyLock::new(build);

#[inline]
pub fn info(id: u8) -> &'static BlockInfo {
    &TABLE[id as usize]
}

#[inline]
pub fn material(id: u8) -> Material {
    TABLE[id as usize].material
}

/// Collision box (block-local) for `getCollisionBoundingBoxFromPool`.
pub fn collision_box(id: u8, meta: u8) -> Option<[f32; 6]> {
    let b = info(id);
    if b.solid {
        return Some([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    }
    match id {
        CACTUS => Some([0.0625, 0.0, 0.0625, 0.9375, 0.9375, 0.9375]),
        CHEST => Some([0.0625, 0.0, 0.0625, 0.9375, 0.875, 0.9375]),
        STONE_SLAB => {
            if meta & 8 != 0 {
                Some([0.0, 0.5, 0.0, 1.0, 1.0, 1.0])
            } else {
                Some([0.0, 0.0, 0.0, 1.0, 0.5, 1.0])
            }
        }
        WATERLILY => Some([0.0, 0.0, 0.0, 1.0, 0.015625, 1.0]),
        SNOW_LAYER => {
            let l = (meta & 7) as f32;
            if l == 0.0 {
                None
            } else {
                Some([0.0, 0.0, 0.0, 1.0, l * 0.125, 1.0])
            }
        }
        _ => None,
    }
}

/// Whether the crosshair can target this block.
pub fn selectable(id: u8) -> bool {
    let b = info(id);
    b.exists && id != AIR && !b.material.is_liquid() && id != FIRE
}

const CLAY_COLORS: [&str; 16] = [
    "hardened_clay_stained_white",
    "hardened_clay_stained_orange",
    "hardened_clay_stained_magenta",
    "hardened_clay_stained_light_blue",
    "hardened_clay_stained_yellow",
    "hardened_clay_stained_lime",
    "hardened_clay_stained_pink",
    "hardened_clay_stained_gray",
    "hardened_clay_stained_silver",
    "hardened_clay_stained_cyan",
    "hardened_clay_stained_purple",
    "hardened_clay_stained_blue",
    "hardened_clay_stained_brown",
    "hardened_clay_stained_green",
    "hardened_clay_stained_red",
    "hardened_clay_stained_black",
];

const LOGS: [(&str, &str); 4] = [("log_oak", "log_oak_top"), ("log_spruce", "log_spruce_top"), ("log_birch", "log_birch_top"), ("log_jungle", "log_jungle_top")];
const LOGS2: [(&str, &str); 4] = [("log_acacia", "log_acacia_top"), ("log_big_oak", "log_big_oak_top"), ("log_acacia", "log_acacia_top"), ("log_big_oak", "log_big_oak_top")];
const LEAVES_TEX: [&str; 4] = ["leaves_oak", "leaves_spruce", "leaves_birch", "leaves_jungle"];
const FLOWERS: [&str; 9] = [
    "flower_rose",
    "flower_blue_orchid",
    "flower_allium",
    "flower_houstonia",
    "flower_tulip_red",
    "flower_tulip_orange",
    "flower_tulip_white",
    "flower_tulip_pink",
    "flower_oxeye_daisy",
];
pub const DOUBLE_PLANTS: [(&str, &str); 6] = [
    ("double_plant_sunflower_bottom", "double_plant_sunflower_top"),
    ("double_plant_syringa_bottom", "double_plant_syringa_top"),
    ("double_plant_grass_bottom", "double_plant_grass_top"),
    ("double_plant_fern_bottom", "double_plant_fern_top"),
    ("double_plant_rose_bottom", "double_plant_rose_top"),
    ("double_plant_paeonia_bottom", "double_plant_paeonia_top"),
];

/// `BlockHugeMushroom.getIcon`
fn mushroom_face(skin: &'static str, meta: u8, face: usize) -> &'static str {
    let cap = "mushroom_block_inside";
    if meta == 10 && face > 1 {
        "mushroom_block_skin_stem"
    } else if (1..=9).contains(&meta) && face == 1 {
        skin
    } else if (1..=3).contains(&meta) && face == 2 {
        skin
    } else if (7..=9).contains(&meta) && face == 3 {
        skin
    } else if (meta == 1 || meta == 4 || meta == 7) && face == 4 {
        skin
    } else if (meta == 3 || meta == 6 || meta == 9) && face == 5 {
        skin
    } else if meta == 14 {
        skin
    } else if meta == 15 {
        "mushroom_block_skin_stem"
    } else {
        cap
    }
}

/// Texture (file stem under `textures/blocks/`) for a block face.
/// Faces: 0 down, 1 up, 2 north(-Z), 3 south(+Z), 4 west(-X), 5 east(+X).
pub fn texture_name(id: u8, meta: u8, face: usize) -> Option<&'static str> {
    let log = |set: &[(&'static str, &'static str); 4]| {
        let (side, top) = set[(meta & 3) as usize];
        let end = match meta & 12 {
            0 => face < 2,
            4 => face == 4 || face == 5,
            8 => face == 2 || face == 3,
            _ => false,
        };
        if end {
            top
        } else {
            side
        }
    };
    Some(match id {
        STONE | MONSTER_EGG => "stone",
        GRASS => match face {
            1 => "grass_top",
            0 => "dirt",
            _ => "grass_side",
        },
        DIRT => {
            if meta == 2 {
                match face {
                    1 => "dirt_podzol_top",
                    0 => "dirt",
                    _ => "dirt_podzol_side",
                }
            } else {
                "dirt"
            }
        }
        COBBLESTONE => "cobblestone",
        PLANKS | CHEST => "planks_oak",
        BEDROCK => "bedrock",
        FLOWING_WATER | WATER => "water_still",
        FLOWING_LAVA | LAVA => "lava_still",
        SAND => {
            if meta == 1 {
                "red_sand"
            } else {
                "sand"
            }
        }
        GRAVEL => "gravel",
        GOLD_ORE => "gold_ore",
        IRON_ORE => "iron_ore",
        COAL_ORE => "coal_ore",
        LOG => log(&LOGS),
        LOG2 => log(&LOGS2),
        LEAVES => LEAVES_TEX[(meta & 3) as usize],
        LEAVES2 => {
            if meta & 3 == 1 {
                "leaves_big_oak"
            } else {
                "leaves_acacia"
            }
        }
        GLASS => "glass",
        LAPIS_ORE => "lapis_ore",
        SANDSTONE => match face {
            1 => "sandstone_top",
            0 => "sandstone_bottom",
            _ => "sandstone_normal",
        },
        STONE_SLAB => {
            if meta & 7 == 1 {
                match face {
                    1 => "sandstone_top",
                    0 => "sandstone_bottom",
                    _ => "sandstone_normal",
                }
            } else if face < 2 {
                "stone_slab_top"
            } else {
                "stone_slab_side"
            }
        }
        TALLGRASS => match meta {
            1 => "tallgrass",
            2 => "fern",
            _ => "deadbush",
        },
        DEADBUSH => "deadbush",
        YELLOW_FLOWER => "flower_dandelion",
        RED_FLOWER => FLOWERS[(meta as usize).min(8)],
        BROWN_MUSHROOM => "mushroom_brown",
        RED_MUSHROOM => "mushroom_red",
        MOSSY_COBBLESTONE => "cobblestone_mossy",
        OBSIDIAN => "obsidian",
        MOB_SPAWNER => "mob_spawner",
        DIAMOND_ORE => "diamond_ore",
        REDSTONE_ORE => "redstone_ore",
        SNOW_LAYER | SNOW => "snow",
        ICE => "ice",
        CACTUS => match face {
            1 => "cactus_top",
            0 => "cactus_bottom",
            _ => "cactus_side",
        },
        CLAY => "clay",
        REEDS => "reeds",
        PUMPKIN => {
            // BlockPumpkin.getIcon
            if face < 2 {
                "pumpkin_top"
            } else if (meta == 2 && face == 2) || (meta == 3 && face == 5) || (meta == 0 && face == 3) || (meta == 1 && face == 4) {
                "pumpkin_face_off"
            } else {
                "pumpkin_side"
            }
        }
        BROWN_MUSHROOM_BLOCK => mushroom_face("mushroom_block_skin_brown", meta, face),
        RED_MUSHROOM_BLOCK => mushroom_face("mushroom_block_skin_red", meta, face),
        MELON_BLOCK => {
            if face < 2 {
                "melon_top"
            } else {
                "melon_side"
            }
        }
        VINE => "vine",
        MYCELIUM => match face {
            1 => "mycelium_top",
            0 => "dirt",
            _ => "mycelium_side",
        },
        WATERLILY => "waterlily",
        COCOA => match (meta & 12) >> 2 {
            0 => "cocoa_stage_0",
            1 => "cocoa_stage_1",
            _ => "cocoa_stage_2",
        },
        EMERALD_ORE => "emerald_ore",
        STAINED_HARDENED_CLAY => CLAY_COLORS[(meta & 15) as usize],
        HARDENED_CLAY => "hardened_clay",
        PACKED_ICE => "ice_packed",
        DOUBLE_PLANT => {
            let (bottom, top) = DOUBLE_PLANTS[((meta & 7) as usize).min(5)];
            if face == 1 {
                top
            } else {
                bottom
            }
        }
        _ => return None,
    })
}

/// Extra textures that are not a direct face of any block.
pub const EXTRA_TEXTURES: [&str; 3] = ["grass_side_overlay", "water_flow", "lava_flow"];
