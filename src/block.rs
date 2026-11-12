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
pub const TORCH: u8 = 50;
pub const FIRE: u8 = 51;
pub const MOB_SPAWNER: u8 = 52;
pub const CHEST: u8 = 54;
pub const DIAMOND_ORE: u8 = 56;
pub const CRAFTING_TABLE: u8 = 58;
pub const FARMLAND: u8 = 60;
pub const FURNACE: u8 = 61;
pub const LIT_FURNACE: u8 = 62;
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

pub const SAPLING: u8 = 6;
pub const SPONGE: u8 = 19;
pub const LAPIS_BLOCK: u8 = 22;
pub const DISPENSER: u8 = 23;
pub const NOTEBLOCK: u8 = 25;
pub const GOLDEN_RAIL: u8 = 27;
pub const DETECTOR_RAIL: u8 = 28;
pub const STICKY_PISTON: u8 = 29;
pub const WEB: u8 = 30;
pub const PISTON: u8 = 33;
pub const PISTON_HEAD: u8 = 34;
pub const TRAPDOOR: u8 = 96;
pub const PORTAL: u8 = 90;
pub const NETHER_BRICK_FENCE: u8 = 113;
pub const NETHER_WART: u8 = 115;
pub const END_PORTAL: u8 = 119;
pub const DRAGON_EGG: u8 = 122;
pub const QUARTZ_ORE: u8 = 153;
pub const REDSTONE_LAMP: u8 = 123;
pub const LIT_REDSTONE_LAMP: u8 = 124;
pub const WOOL: u8 = 35;
pub const GOLD_BLOCK: u8 = 41;
pub const IRON_BLOCK: u8 = 42;
pub const DOUBLE_STONE_SLAB: u8 = 43;
pub const BRICK_BLOCK: u8 = 45;
pub const TNT: u8 = 46;
pub const BOOKSHELF: u8 = 47;
pub const OAK_STAIRS: u8 = 53;
pub const REDSTONE_WIRE: u8 = 55;
pub const DIAMOND_BLOCK: u8 = 57;
pub const WHEAT: u8 = 59;
pub const WOODEN_DOOR: u8 = 64;
pub const LADDER: u8 = 65;
pub const RAIL: u8 = 66;
pub const STONE_STAIRS: u8 = 67;
pub const LEVER: u8 = 69;
pub const STONE_PRESSURE_PLATE: u8 = 70;
pub const IRON_DOOR: u8 = 71;
pub const WOODEN_PRESSURE_PLATE: u8 = 72;
pub const LIT_REDSTONE_ORE: u8 = 74;
pub const UNLIT_REDSTONE_TORCH: u8 = 75;
pub const REDSTONE_TORCH: u8 = 76;
pub const STONE_BUTTON: u8 = 77;
pub const JUKEBOX: u8 = 84;
pub const FENCE: u8 = 85;
pub const NETHERRACK: u8 = 87;
pub const SOUL_SAND: u8 = 88;
pub const GLOWSTONE: u8 = 89;
pub const LIT_PUMPKIN: u8 = 91;
pub const UNPOWERED_REPEATER: u8 = 93;
pub const POWERED_REPEATER: u8 = 94;
pub const STONEBRICK: u8 = 98;
pub const IRON_BARS: u8 = 101;
pub const GLASS_PANE: u8 = 102;
pub const BRICK_STAIRS: u8 = 108;
pub const STONE_BRICK_STAIRS: u8 = 109;
pub const NETHER_BRICK: u8 = 112;
pub const NETHER_BRICK_STAIRS: u8 = 114;
pub const CAULDRON: u8 = 118;
pub const END_PORTAL_FRAME: u8 = 120;
pub const END_STONE: u8 = 121;
pub const SANDSTONE_STAIRS: u8 = 128;
pub const TRIPWIRE_HOOK: u8 = 131;
pub const TRIPWIRE: u8 = 132;
pub const EMERALD_BLOCK: u8 = 133;
pub const SPRUCE_STAIRS: u8 = 134;
pub const BIRCH_STAIRS: u8 = 135;
pub const JUNGLE_STAIRS: u8 = 136;
pub const FLOWER_POT: u8 = 140;
pub const CARROTS: u8 = 141;
pub const POTATOES: u8 = 142;
pub const WOODEN_BUTTON: u8 = 143;
pub const REDSTONE_BLOCK: u8 = 152;
pub const QUARTZ_BLOCK: u8 = 155;
pub const ACACIA_STAIRS: u8 = 163;
pub const DARK_OAK_STAIRS: u8 = 164;
pub const HAY_BLOCK: u8 = 170;
pub const CARPET: u8 = 171;
pub const COAL_BLOCK: u8 = 173;

/// `BlockStairs` instances.
#[inline]
pub fn is_stairs(id: u8) -> bool {
    matches!(
        id,
        OAK_STAIRS
            | STONE_STAIRS
            | BRICK_STAIRS
            | STONE_BRICK_STAIRS
            | NETHER_BRICK_STAIRS
            | SANDSTONE_STAIRS
            | SPRUCE_STAIRS
            | BIRCH_STAIRS
            | JUNGLE_STAIRS
            | ACACIA_STAIRS
            | DARK_OAK_STAIRS
    )
}

/// `BlockRailBase` instances.
#[inline]
pub fn is_rail(id: u8) -> bool {
    matches!(id, RAIL | GOLDEN_RAIL | DETECTOR_RAIL)
}

/// The block whose textures a stair block borrows (`BlockStairs.field_150149_b`)
/// with its metadata.
pub fn stair_model(id: u8) -> (u8, u8) {
    match id {
        OAK_STAIRS => (PLANKS, 0),
        SPRUCE_STAIRS => (PLANKS, 1),
        BIRCH_STAIRS => (PLANKS, 2),
        JUNGLE_STAIRS => (PLANKS, 3),
        ACACIA_STAIRS => (PLANKS, 4),
        DARK_OAK_STAIRS => (PLANKS, 5),
        STONE_STAIRS => (COBBLESTONE, 0),
        BRICK_STAIRS => (BRICK_BLOCK, 0),
        STONE_BRICK_STAIRS => (STONEBRICK, 0),
        NETHER_BRICK_STAIRS => (NETHER_BRICK, 0),
        _ => (SANDSTONE, 0),
    }
}

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
    Torch,
    /// `renderBlockStairs` (render type 10)
    Stairs,
    /// `renderBlockFence` (11)
    Fence,
    /// `renderBlockPane` (18)
    Pane,
    /// `renderBlockDoor` (7)
    Door,
    /// `renderBlockLadder` (8)
    Ladder,
    /// `renderBlockMinecartTrack` (9)
    Rail,
    /// `renderBlockCrops` (6)
    Crops,
    /// `renderBlockRedstoneWire` (5)
    Wire,
    /// `renderBlockTripWire` (30)
    Tripwire,
    /// Blocks drawn as a fixed set of boxes (`shapes::static_boxes`).
    Boxes,
    /// `renderBlockFire` (3)
    Fire,
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
    /// `Material.circuits`
    Circuits,
    /// `Material.web`: solid but does not block movement.
    Web,
    Cloth,
    Iron,
    Tnt,
    Piston,
    Carpet,
    Sponge,
    Portal,
    /// `Material.dragonEgg`
    DragonEgg,
}

impl Material {
    #[inline]
    pub fn is_liquid(self) -> bool {
        matches!(self, Material::Water | Material::Lava)
    }
    /// `Material.isSolid()`
    #[inline]
    pub fn is_solid(self) -> bool {
        !matches!(
            self,
            Material::Air
                | Material::Water
                | Material::Lava
                | Material::Plants
                | Material::Vine
                | Material::Snow
                | Material::Fire
                | Material::Circuits
                | Material::Carpet
                | Material::Portal
        )
    }
    /// `Material.blocksMovement()`
    #[inline]
    pub fn blocks_movement(self) -> bool {
        self.is_solid() && self != Material::Web
    }
    #[inline]
    pub fn replaceable(self) -> bool {
        matches!(
            self,
            Material::Air
                | Material::Water
                | Material::Lava
                | Material::Vine
                | Material::Snow
                | Material::Fire
        )
    }
    #[inline]
    fn translucent(self) -> bool {
        matches!(
            self,
            Material::Leaves
                | Material::Glass
                | Material::Ice
                | Material::Snow
                | Material::Cactus
                | Material::Tnt
        )
    }
    /// `Material.isOpaque()`
    #[inline]
    pub fn is_opaque(self) -> bool {
        !self.translucent() && self.blocks_movement()
    }
    #[inline]
    pub fn can_burn(self) -> bool {
        matches!(
            self,
            Material::Wood
                | Material::Leaves
                | Material::Vine
                | Material::Cloth
                | Material::Tnt
                | Material::Carpet
        )
    }
    /// `Material.getCanBlockGrass()`
    #[inline]
    fn can_block_grass(self) -> bool {
        !matches!(
            self,
            Material::Air
                | Material::Plants
                | Material::Vine
                | Material::Snow
                | Material::Fire
                | Material::Circuits
                | Material::Carpet
                | Material::Portal
        )
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
        (CRAFTING_TABLE, M::Wood),
        (FURNACE, M::Rock),
        (LIT_FURNACE, M::Rock),
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
    t[LIT_FURNACE as usize].light_value = 13;
    let mut torch = base(Shape::Torch, M::Circuits, false);
    torch.light_value = 14;
    t[TORCH as usize] = torch;
    t[FIRE as usize] = base(Shape::Fire, M::Fire, false);
    t[FIRE as usize].light_value = 15;

    let mut slab = base(Shape::Slab, M::Rock, false);
    slab.light_opacity = 255;
    t[STONE_SLAB as usize] = slab;
    t[CHEST as usize] = base(Shape::Chest, M::Wood, false);

    for (id, m) in [
        (SPONGE, M::Sponge),
        (LAPIS_BLOCK, M::Iron),
        (DISPENSER, M::Rock),
        (NOTEBLOCK, M::Wood),
        (WOOL, M::Cloth),
        (GOLD_BLOCK, M::Iron),
        (IRON_BLOCK, M::Iron),
        (DOUBLE_STONE_SLAB, M::Rock),
        (BRICK_BLOCK, M::Rock),
        (BOOKSHELF, M::Wood),
        (DIAMOND_BLOCK, M::Iron),
        (LIT_REDSTONE_ORE, M::Rock),
        (JUKEBOX, M::Wood),
        (NETHERRACK, M::Rock),
        (LIT_PUMPKIN, M::Gourd),
        (STONEBRICK, M::Rock),
        (NETHER_BRICK, M::Rock),
        (END_STONE, M::Rock),
        (EMERALD_BLOCK, M::Iron),
        (REDSTONE_BLOCK, M::Iron),
        (QUARTZ_BLOCK, M::Rock),
        (HAY_BLOCK, M::Grass),
        (COAL_BLOCK, M::Rock),
    ] {
        t[id as usize] = cube(m);
    }
    // BlockPistonBase.isOpaqueCube() is false, so light passes through.
    for id in [PISTON, STICKY_PISTON] {
        let mut p = base(Shape::Cube, M::Piston, false);
        p.solid = true;
        p.full_block = false;
        t[id as usize] = p;
    }
    // BlockRedstoneLight: Material.redstoneLight (a plain solid material).
    t[REDSTONE_LAMP as usize] = cube(M::Iron);
    t[LIT_REDSTONE_LAMP as usize] = cube(M::Iron);
    t[LIT_REDSTONE_LAMP as usize].light_value = 15;
    t[PISTON_HEAD as usize] = base(Shape::Boxes, M::Piston, false);
    // BlockPortal: a thin translucent pane that glows.
    let mut portal = base(Shape::Boxes, M::Portal, false);
    portal.light_value = 11;
    portal.translucent = true;
    t[PORTAL as usize] = portal;
    t[NETHER_BRICK_FENCE as usize] = base(Shape::Fence, M::Rock, false);
    t[NETHER_WART as usize] = base(Shape::Crops, M::Plants, false);
    // BlockEndPortal: 0.75 high, light 15, no collision.
    let mut end_portal = base(Shape::Boxes, M::Portal, false);
    end_portal.light_value = 15;
    t[END_PORTAL as usize] = end_portal;
    let mut egg = base(Shape::Boxes, M::DragonEgg, false);
    egg.light_value = 1;
    t[DRAGON_EGG as usize] = egg;
    t[QUARTZ_ORE as usize] = cube(M::Rock);
    t[TRAPDOOR as usize] = base(Shape::Boxes, M::Wood, false);
    // TNT uses Material.tnt, which is translucent: not an opaque cube for
    // `Material.isOpaque` but still rendered and lit as a full block.
    t[TNT as usize] = cube(M::Tnt);
    let mut soul = cube(M::Sand);
    soul.solid = false;
    t[SOUL_SAND as usize] = soul;
    let mut glow = base(Shape::Cube, M::Glass, true);
    glow.light_value = 15;
    t[GLOWSTONE as usize] = glow;
    t[LIT_PUMPKIN as usize].light_value = 15;
    t[LIT_REDSTONE_ORE as usize].light_value = 9;

    t[SAPLING as usize] = base(Shape::Cross, M::Plants, false);
    let mut web = base(Shape::Cross, M::Web, false);
    web.light_opacity = 1;
    t[WEB as usize] = web;

    for id in [
        OAK_STAIRS,
        SPRUCE_STAIRS,
        BIRCH_STAIRS,
        JUNGLE_STAIRS,
        ACACIA_STAIRS,
        DARK_OAK_STAIRS,
    ] {
        t[id as usize] = base(Shape::Stairs, M::Wood, false);
    }
    for id in [
        STONE_STAIRS,
        BRICK_STAIRS,
        STONE_BRICK_STAIRS,
        NETHER_BRICK_STAIRS,
        SANDSTONE_STAIRS,
    ] {
        t[id as usize] = base(Shape::Stairs, M::Rock, false);
    }
    for id in 0..256usize {
        if is_stairs(id as u8) {
            // BlockStairs: setLightOpacity(255)
            t[id].light_opacity = 255;
        }
    }
    t[FENCE as usize] = base(Shape::Fence, M::Wood, false);
    t[GLASS_PANE as usize] = base(Shape::Pane, M::Glass, false);
    t[IRON_BARS as usize] = base(Shape::Pane, M::Iron, false);
    t[WOODEN_DOOR as usize] = base(Shape::Door, M::Wood, false);
    t[IRON_DOOR as usize] = base(Shape::Door, M::Iron, false);
    t[LADDER as usize] = base(Shape::Ladder, M::Circuits, false);
    for id in [RAIL, GOLDEN_RAIL, DETECTOR_RAIL] {
        t[id as usize] = base(Shape::Rail, M::Circuits, false);
    }
    for id in [WHEAT, CARROTS, POTATOES] {
        t[id as usize] = base(Shape::Crops, M::Plants, false);
    }
    t[REDSTONE_WIRE as usize] = base(Shape::Wire, M::Circuits, false);
    t[TRIPWIRE as usize] = base(Shape::Tripwire, M::Circuits, false);
    let mut farmland = base(Shape::Boxes, M::Ground, false);
    farmland.light_opacity = 255;
    t[FARMLAND as usize] = farmland;
    for (id, m) in [
        (LEVER, M::Circuits),
        (STONE_PRESSURE_PLATE, M::Rock),
        (WOODEN_PRESSURE_PLATE, M::Wood),
        (STONE_BUTTON, M::Circuits),
        (WOODEN_BUTTON, M::Circuits),
        (UNPOWERED_REPEATER, M::Circuits),
        (POWERED_REPEATER, M::Circuits),
        (CAULDRON, M::Iron),
        (END_PORTAL_FRAME, M::Rock),
        (TRIPWIRE_HOOK, M::Circuits),
        (FLOWER_POT, M::Circuits),
        (CARPET, M::Carpet),
    ] {
        t[id as usize] = base(Shape::Boxes, m, false);
    }
    t[END_PORTAL_FRAME as usize].light_value = 1;
    t[POWERED_REPEATER as usize].light_value = 9;
    t[UNLIT_REDSTONE_TORCH as usize] = base(Shape::Torch, M::Circuits, false);
    t[REDSTONE_TORCH as usize] = base(Shape::Torch, M::Circuits, false);
    t[REDSTONE_TORCH as usize].light_value = 7;
    let mut dslab = cube(M::Rock);
    dslab.light_opacity = 255;
    t[DOUBLE_STONE_SLAB as usize] = dslab;

    // Block.registerBlocks: useNeighborBrightness.
    for (id, b) in t.iter_mut().enumerate() {
        if b.material == M::Air {
            continue;
        }
        b.use_neighbor_brightness = is_stairs(id as u8)
            || id == STONE_SLAB as usize
            || id == DOUBLE_STONE_SLAB as usize
            || id == FARMLAND as usize
            || b.can_block_grass
            || b.light_opacity == 0;
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
        SOUL_SAND => Some([0.0, 0.0, 0.0, 1.0, 0.875, 1.0]),
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

const LOGS: [(&str, &str); 4] = [
    ("log_oak", "log_oak_top"),
    ("log_spruce", "log_spruce_top"),
    ("log_birch", "log_birch_top"),
    ("log_jungle", "log_jungle_top"),
];
const LOGS2: [(&str, &str); 4] = [
    ("log_acacia", "log_acacia_top"),
    ("log_big_oak", "log_big_oak_top"),
    ("log_acacia", "log_acacia_top"),
    ("log_big_oak", "log_big_oak_top"),
];
const LEAVES_TEX: [&str; 4] = [
    "leaves_oak",
    "leaves_spruce",
    "leaves_birch",
    "leaves_jungle",
];
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
    (
        "double_plant_sunflower_bottom",
        "double_plant_sunflower_top",
    ),
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
        STONE => "stone",
        MONSTER_EGG => [
            "stone",
            "cobblestone",
            "stonebrick",
            "stonebrick_mossy",
            "stonebrick_cracked",
            "stonebrick_carved",
            "stone",
            "stone",
        ][(meta & 7) as usize],
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
        SANDSTONE => {
            // BlockSandStone.getIcon
            if face == 1 || (face == 0 && (meta == 1 || meta == 2)) {
                "sandstone_top"
            } else if face == 0 {
                "sandstone_bottom"
            } else {
                [
                    "sandstone_normal",
                    "sandstone_carved",
                    "sandstone_smooth",
                    "sandstone_normal",
                ][(meta & 3) as usize]
            }
        }
        STONE_SLAB | DOUBLE_STONE_SLAB => match meta & 7 {
            // BlockStoneSlab.getIcon
            1 => match face {
                1 => "sandstone_top",
                0 => "sandstone_bottom",
                _ => "sandstone_normal",
            },
            2 => "planks_oak",
            3 => "cobblestone",
            4 => "brick",
            5 => "stonebrick",
            6 => "nether_brick",
            7 => match face {
                1 => "quartz_block_top",
                0 => "quartz_block_bottom",
                _ => "quartz_block_side",
            },
            _ => {
                if face < 2 || (id == DOUBLE_STONE_SLAB && meta & 8 != 0) {
                    "stone_slab_top"
                } else {
                    "stone_slab_side"
                }
            }
        },
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
        TORCH => "torch_on",
        CRAFTING_TABLE => match face {
            1 => "crafting_table_top",
            0 => "planks_oak",
            2 | 4 => "crafting_table_front",
            _ => "crafting_table_side",
        },
        FURNACE | LIT_FURNACE => {
            // BlockFurnace.getIcon: the front faces the metadata direction;
            // metadata 0 (item form) shows the front on side 3.
            let front = if meta == 0 { 3 } else { meta as usize };
            if face < 2 {
                "furnace_top"
            } else if face == front {
                if id == LIT_FURNACE {
                    "furnace_front_on"
                } else {
                    "furnace_front_off"
                }
            } else {
                "furnace_side"
            }
        }
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
            } else if (meta == 2 && face == 2)
                || (meta == 3 && face == 5)
                || (meta == 0 && face == 3)
                || (meta == 1 && face == 4)
            {
                "pumpkin_face_off"
            } else {
                "pumpkin_side"
            }
        }
        LIT_PUMPKIN => {
            if face < 2 {
                "pumpkin_top"
            } else if (meta == 2 && face == 2)
                || (meta == 3 && face == 5)
                || (meta == 0 && face == 3)
                || (meta == 1 && face == 4)
            {
                "pumpkin_face_on"
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
        SAPLING => [
            "sapling_oak",
            "sapling_spruce",
            "sapling_birch",
            "sapling_jungle",
            "sapling_acacia",
            "sapling_roofed_oak",
            "sapling_oak",
            "sapling_oak",
        ][(meta & 7) as usize],
        SPONGE => "sponge",
        LAPIS_BLOCK => "lapis_block",
        DISPENSER => {
            // BlockDispenser.getIcon
            let k = (meta & 7) as usize;
            let vertical = k == 0 || k == 1;
            if face == k {
                if vertical {
                    "dispenser_front_vertical"
                } else {
                    "dispenser_front_horizontal"
                }
            } else if vertical || face < 2 {
                "furnace_top"
            } else {
                "furnace_side"
            }
        }
        NOTEBLOCK => "noteblock",
        RAIL => {
            if meta >= 6 {
                "rail_normal_turned"
            } else {
                "rail_normal"
            }
        }
        GOLDEN_RAIL => {
            if meta & 8 != 0 {
                "rail_golden_powered"
            } else {
                "rail_golden"
            }
        }
        DETECTOR_RAIL => {
            if meta & 8 != 0 {
                "rail_detector_powered"
            } else {
                "rail_detector"
            }
        }
        PISTON | STICKY_PISTON => {
            // BlockPistonBase.getIcon
            let k = (meta & 7) as usize;
            let opposite = [1, 0, 3, 2, 5, 4];
            if k <= 5 && face == k {
                if meta & 8 != 0 {
                    "piston_inner"
                } else if id == STICKY_PISTON {
                    "piston_top_sticky"
                } else {
                    "piston_top_normal"
                }
            } else if k <= 5 && face == opposite[k] {
                "piston_bottom"
            } else {
                "piston_side"
            }
        }
        PISTON_HEAD => {
            let k = (meta & 7) as usize;
            if face == k {
                if meta & 8 != 0 {
                    "piston_top_sticky"
                } else {
                    "piston_top_normal"
                }
            } else {
                "piston_side"
            }
        }
        TRAPDOOR => "trapdoor",
        FIRE => {
            if face == 1 {
                "fire_layer_1"
            } else {
                "fire_layer_0"
            }
        }
        PORTAL => "portal",
        NETHER_BRICK_FENCE => "nether_brick",
        NETHER_WART => match meta {
            0 => "nether_wart_stage_0",
            1 | 2 => "nether_wart_stage_1",
            _ => "nether_wart_stage_2",
        },
        END_PORTAL => "portal",
        DRAGON_EGG => "dragon_egg",
        QUARTZ_ORE => "quartz_ore",
        REDSTONE_LAMP => "redstone_lamp_off",
        LIT_REDSTONE_LAMP => "redstone_lamp_on",
        WEB => "web",
        WOOL | CARPET => WOOL_COLORS[(meta & 15) as usize],
        GOLD_BLOCK => "gold_block",
        IRON_BLOCK => "iron_block",
        BRICK_BLOCK => "brick",
        TNT => match face {
            0 => "tnt_bottom",
            1 => "tnt_top",
            _ => "tnt_side",
        },
        BOOKSHELF => {
            if face < 2 {
                "planks_oak"
            } else {
                "bookshelf"
            }
        }
        OAK_STAIRS | SPRUCE_STAIRS | BIRCH_STAIRS | JUNGLE_STAIRS | ACACIA_STAIRS
        | DARK_OAK_STAIRS | STONE_STAIRS | BRICK_STAIRS | STONE_BRICK_STAIRS
        | NETHER_BRICK_STAIRS | SANDSTONE_STAIRS => {
            let (m, mm) = stair_model(id);
            return texture_name(m, mm, face);
        }
        REDSTONE_WIRE => [
            "redstone_dust_cross",
            "redstone_dust_line",
            "redstone_dust_cross_overlay",
            "redstone_dust_line_overlay",
            "redstone_dust_cross",
            "redstone_dust_cross",
        ][face],
        DIAMOND_BLOCK => "diamond_block",
        WHEAT => [
            "wheat_stage_0",
            "wheat_stage_1",
            "wheat_stage_2",
            "wheat_stage_3",
            "wheat_stage_4",
            "wheat_stage_5",
            "wheat_stage_6",
            "wheat_stage_7",
        ][(meta & 7) as usize],
        CARROTS | POTATOES => {
            // BlockCarrot / BlockPotato.getIcon
            let m = (meta & 7) as usize;
            let stage = if m < 7 {
                (if m == 6 { 5 } else { m }) >> 1
            } else {
                3
            };
            if id == CARROTS {
                [
                    "carrots_stage_0",
                    "carrots_stage_1",
                    "carrots_stage_2",
                    "carrots_stage_3",
                ][stage]
            } else {
                [
                    "potatoes_stage_0",
                    "potatoes_stage_1",
                    "potatoes_stage_2",
                    "potatoes_stage_3",
                ][stage]
            }
        }
        FARMLAND => {
            if face == 1 {
                if meta > 0 {
                    "farmland_wet"
                } else {
                    "farmland_dry"
                }
            } else {
                "dirt"
            }
        }
        WOODEN_DOOR => {
            if meta & 8 != 0 {
                "door_wood_upper"
            } else {
                "door_wood_lower"
            }
        }
        IRON_DOOR => {
            if meta & 8 != 0 {
                "door_iron_upper"
            } else {
                "door_iron_lower"
            }
        }
        LADDER => "ladder",
        LEVER => "lever",
        STONE_PRESSURE_PLATE | STONE_BUTTON => "stone",
        WOODEN_PRESSURE_PLATE | WOODEN_BUTTON | FENCE => "planks_oak",
        LIT_REDSTONE_ORE => "redstone_ore",
        UNLIT_REDSTONE_TORCH => "redstone_torch_off",
        REDSTONE_TORCH => "redstone_torch_on",
        JUKEBOX => {
            if face == 1 {
                "jukebox_top"
            } else {
                "jukebox_side"
            }
        }
        NETHERRACK => "netherrack",
        SOUL_SAND => "soul_sand",
        GLOWSTONE => "glowstone",
        UNPOWERED_REPEATER | POWERED_REPEATER => {
            if face == 1 {
                if id == POWERED_REPEATER {
                    "repeater_on"
                } else {
                    "repeater_off"
                }
            } else {
                "stone_slab_top"
            }
        }
        STONEBRICK => [
            "stonebrick",
            "stonebrick_mossy",
            "stonebrick_cracked",
            "stonebrick_carved",
        ][(meta & 3) as usize],
        IRON_BARS => "iron_bars",
        GLASS_PANE => {
            if face < 2 {
                "glass_pane_top"
            } else {
                "glass"
            }
        }
        NETHER_BRICK => "nether_brick",
        CAULDRON => {
            if meta >= 8 {
                "cauldron_inner"
            } else {
                match face {
                    0 => "cauldron_bottom",
                    1 => "cauldron_top",
                    _ => "cauldron_side",
                }
            }
        }
        END_PORTAL_FRAME => {
            if meta >= 8 {
                "endframe_eye"
            } else {
                match face {
                    0 => "end_stone",
                    1 => "endframe_top",
                    _ => "endframe_side",
                }
            }
        }
        END_STONE => "end_stone",
        TRIPWIRE_HOOK => "trip_wire_source",
        TRIPWIRE => "trip_wire",
        EMERALD_BLOCK => "emerald_block",
        FLOWER_POT => {
            if meta >= 15 {
                "dirt"
            } else {
                "flower_pot"
            }
        }
        REDSTONE_BLOCK => "redstone_block",
        QUARTZ_BLOCK => match face {
            0 => "quartz_block_bottom",
            1 => "quartz_block_top",
            _ => "quartz_block_side",
        },
        HAY_BLOCK => {
            if face < 2 {
                "hay_block_top"
            } else {
                "hay_block_side"
            }
        }
        COAL_BLOCK => "coal_block",
        _ => return None,
    })
}

pub const WOOL_COLORS: [&str; 16] = [
    "wool_colored_white",
    "wool_colored_orange",
    "wool_colored_magenta",
    "wool_colored_light_blue",
    "wool_colored_yellow",
    "wool_colored_lime",
    "wool_colored_pink",
    "wool_colored_gray",
    "wool_colored_silver",
    "wool_colored_cyan",
    "wool_colored_purple",
    "wool_colored_blue",
    "wool_colored_brown",
    "wool_colored_green",
    "wool_colored_red",
    "wool_colored_black",
];

/// Extra textures that are not a direct face of any block.
pub const EXTRA_TEXTURES: [&str; 3] = ["grass_side_overlay", "water_flow", "lava_flow"];
