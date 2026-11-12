//! `ChunkProviderGenerate.populate`, `BiomeDecorator` and the per-biome
//! `decorate` overrides. Structures (`mapFeaturesEnabled`) are not generated.

use crate::block;
use crate::java::JavaRandom;
use crate::world::biome::{Biome, Terrain};
use crate::world::features::{bigtrees, misc, trees};
use crate::world::server::ServerWorld;

/// `BiomeDecorator` tunables.
#[derive(Clone, Copy)]
struct Deco {
    waterlily: i32,
    trees: i32,
    flowers: i32,
    grass: i32,
    dead_bush: i32,
    mushrooms: i32,
    reeds: i32,
    cacti: i32,
    sand: i32,
    sand2: i32,
    clay: i32,
    big_mushrooms: i32,
}

const DEFAULT: Deco = Deco {
    waterlily: 0,
    trees: 0,
    flowers: 2,
    grass: 1,
    dead_bush: 0,
    mushrooms: 0,
    reeds: 0,
    cacti: 0,
    sand: 1,
    sand2: 3,
    clay: 1,
    big_mushrooms: 0,
};

/// Java class of a biome object, as far as decoration is concerned.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Base,
    Plains {
        sunflowers: bool,
    },
    Desert,
    Hills {
        more_trees: bool,
    },
    Forest(u8),
    Taiga(u8),
    Swamp,
    Jungle {
        edge: bool,
    },
    Mushroom,
    Beach,
    Snow {
        icy: bool,
    },
    Savanna,
    Mesa {
        forest: bool,
    },
    /// `BiomeGenMutated` wrapping biome `id - 128`.
    Mutated,
    /// `BiomeGenForest$1`
    BirchMutated,
    /// `BiomeGenForest$2`
    RoofedMutated,
    /// `BiomeGenSavanna$Mutated`
    SavannaMutated,
}

fn kind(id: u8) -> Kind {
    match id {
        1 => Kind::Plains { sunflowers: false },
        129 => Kind::Plains { sunflowers: true },
        2 | 17 => Kind::Desert,
        3 | 131 | 162 => Kind::Hills { more_trees: false },
        20 | 34 => Kind::Hills { more_trees: true },
        4 | 18 => Kind::Forest(0),
        132 => Kind::Forest(1),
        27 | 28 => Kind::Forest(2),
        29 => Kind::Forest(3),
        5 | 19 | 30 | 31 => Kind::Taiga(0),
        32 | 33 => Kind::Taiga(1),
        160 | 161 => Kind::Taiga(2),
        6 => Kind::Swamp,
        21 | 22 => Kind::Jungle { edge: false },
        23 => Kind::Jungle { edge: true },
        14 | 15 => Kind::Mushroom,
        16 | 25 | 26 => Kind::Beach,
        12 | 13 => Kind::Snow { icy: false },
        140 => Kind::Snow { icy: true },
        35 | 36 => Kind::Savanna,
        37 | 39 | 165 | 167 => Kind::Mesa { forest: false },
        38 | 166 => Kind::Mesa { forest: true },
        155 | 156 => Kind::BirchMutated,
        157 => Kind::RoofedMutated,
        163 | 164 => Kind::SavannaMutated,
        130 | 133 | 134 | 149 | 151 | 158 => Kind::Mutated,
        _ => Kind::Base,
    }
}

/// Decorator settings of the biome object `id` (constructor state).
fn deco(id: u8) -> Deco {
    let mut d = DEFAULT;
    match kind(id) {
        Kind::Plains { .. } => {
            d.trees = -999;
            d.flowers = 4;
            d.grass = 10;
        }
        Kind::Desert => {
            d.trees = -999;
            d.dead_bush = 2;
            d.reeds = 50;
            d.cacti = 10;
        }
        Kind::Hills { more_trees } => {
            if more_trees {
                d.trees = 3;
            }
        }
        Kind::Forest(t) => {
            d.trees = 10;
            d.grass = 2;
            if t == 1 {
                d.trees = 6;
                d.flowers = 100;
                d.grass = 1;
            }
            if t == 3 {
                d.trees = -999;
            }
        }
        Kind::Taiga(t) => {
            d.trees = 10;
            if t != 1 && t != 2 {
                d.grass = 1;
                d.mushrooms = 1;
            } else {
                d.grass = 7;
                d.dead_bush = 1;
                d.mushrooms = 3;
            }
        }
        Kind::Swamp => {
            d.trees = 2;
            d.flowers = 1;
            d.dead_bush = 1;
            d.mushrooms = 8;
            d.reeds = 10;
            d.clay = 1;
            d.waterlily = 4;
            d.sand2 = 0;
            d.sand = 0;
            d.grass = 5;
        }
        Kind::Jungle { edge } => {
            d.trees = if edge { 2 } else { 50 };
            d.grass = 25;
            d.flowers = 4;
        }
        Kind::Mushroom => {
            d.trees = -100;
            d.flowers = -100;
            d.grass = -100;
            d.mushrooms = 1;
            d.big_mushrooms = 1;
        }
        Kind::Beach => {
            d.trees = -999;
        }
        Kind::Savanna => {
            d.trees = 1;
            d.flowers = 4;
            d.grass = 20;
        }
        Kind::SavannaMutated => {
            d.trees = 2;
            d.flowers = 2;
            d.grass = 5;
        }
        Kind::Mesa { forest } => {
            d.trees = -999;
            d.dead_bush = 20;
            d.reeds = 3;
            d.cacti = 5;
            d.flowers = 0;
            if forest {
                d.trees = 5;
            }
        }
        Kind::Base
        | Kind::Snow { .. }
        | Kind::Mutated
        | Kind::BirchMutated
        | Kind::RoofedMutated => {}
    }
    d
}

enum Tree {
    Trees {
        min: i32,
        wood: i32,
        leaf: i32,
        vines: bool,
    },
    Big(u8),
    Swamp,
    Birch(bool),
    Canopy,
    Pine,
    Spruce,
    MegaPine(bool),
    Shrub,
    MegaJungle,
    Savanna,
}

const OAK: Tree = Tree::Trees {
    min: 4,
    wood: 0,
    leaf: 0,
    vines: false,
};

/// `BiomeGenBase.func_150567_a` for the biome object `id`.
fn pick_tree(id: u8, r: &mut JavaRandom) -> Tree {
    let default = |id: u8, r: &mut JavaRandom| {
        if r.next_int_n(10) == 0 {
            Tree::Big(id)
        } else {
            OAK
        }
    };
    match kind(id) {
        Kind::Hills { .. } => {
            if r.next_int_n(3) > 0 {
                Tree::Spruce
            } else {
                default(id, r)
            }
        }
        Kind::Forest(t) => {
            if t == 3 && r.next_int_n(3) > 0 {
                Tree::Canopy
            } else if t != 2 && r.next_int_n(5) != 0 {
                OAK
            } else {
                Tree::Birch(false)
            }
        }
        Kind::Taiga(t) => {
            if (t == 1 || t == 2) && r.next_int_n(3) == 0 {
                if t != 2 && r.next_int_n(13) != 0 {
                    Tree::MegaPine(false)
                } else {
                    Tree::MegaPine(true)
                }
            } else if r.next_int_n(3) == 0 {
                Tree::Pine
            } else {
                Tree::Spruce
            }
        }
        Kind::Swamp => Tree::Swamp,
        Kind::Jungle { edge } => {
            if r.next_int_n(10) == 0 {
                Tree::Big(id)
            } else if r.next_int_n(2) == 0 {
                Tree::Shrub
            } else if !edge && r.next_int_n(3) == 0 {
                Tree::MegaJungle
            } else {
                Tree::Trees {
                    min: 4 + r.next_int_n(7),
                    wood: 3,
                    leaf: 3,
                    vines: true,
                }
            }
        }
        Kind::Snow { .. } => Tree::Spruce,
        Kind::Savanna => {
            if r.next_int_n(5) > 0 {
                Tree::Savanna
            } else {
                OAK
            }
        }
        Kind::Mesa { .. } => OAK,
        Kind::BirchMutated => {
            if r.next_boolean() {
                Tree::Birch(true)
            } else {
                Tree::Birch(false)
            }
        }
        Kind::Mutated | Kind::RoofedMutated | Kind::SavannaMutated => pick_tree(id - 128, r),
        _ => default(id, r),
    }
}

fn grow_tree(w: &mut ServerWorld, r: &mut JavaRandom, t: Tree, x: i32, y: i32, z: i32) {
    match t {
        Tree::Trees {
            min,
            wood,
            leaf,
            vines,
        } => {
            trees::trees(w, r, x, y, z, min, wood, leaf, vines);
        }
        Tree::Big(owner) => {
            let mut limit = w.big_tree_limit[owner as usize];
            bigtrees::big_tree(w, r, x, y, z, &mut limit);
            w.big_tree_limit[owner as usize] = limit;
        }
        Tree::Swamp => {
            trees::swamp(w, r, x, y, z);
        }
        Tree::Birch(tall) => {
            trees::birch(w, r, x, y, z, tall);
        }
        Tree::Canopy => {
            misc::canopy(w, r, x, y, z);
        }
        Tree::Pine => {
            trees::taiga1(w, r, x, y, z);
        }
        Tree::Spruce => {
            trees::taiga2(w, r, x, y, z);
        }
        Tree::MegaPine(base) => {
            if bigtrees::mega_pine(w, r, x, y, z, base) {
                bigtrees::mega_pine_soil(w, r, x, y, z);
            }
        }
        Tree::Shrub => {
            trees::shrub(w, r, x, y, z, 3, 0);
        }
        Tree::MegaJungle => {
            bigtrees::mega_jungle(w, r, x, y, z, 10, 20, 3, 3);
        }
        Tree::Savanna => {
            trees::savanna(w, r, x, y, z);
        }
    }
}

/// `func_150572_a`: returns `(block, meta)`.
fn pick_flower(w: &ServerWorld, id: u8, r: &mut JavaRandom, x: i32, z: i32) -> (u8, i32) {
    let plant = &w.provider.terrain.noise.plant;
    match kind(id) {
        Kind::Forest(1) => {
            let d = ((1.0 + plant.get_value(x as f64 / 48.0, z as f64 / 48.0)) / 2.0)
                .clamp(0.0, 0.9999);
            let mut i = (d * 9.0) as i32;
            if i == 1 {
                i = 0;
            }
            (block::RED_FLOWER, i)
        }
        Kind::Plains { .. } => {
            let d = plant.get_value(x as f64 / 200.0, z as f64 / 200.0);
            if d < -0.8 {
                let i = r.next_int_n(4);
                return (block::RED_FLOWER, 4 + i);
            }
            if r.next_int_n(3) > 0 {
                match r.next_int_n(3) {
                    0 => (block::RED_FLOWER, 0),
                    1 => (block::RED_FLOWER, 3),
                    _ => (block::RED_FLOWER, 8),
                }
            } else {
                (block::YELLOW_FLOWER, 0)
            }
        }
        Kind::Swamp => (block::RED_FLOWER, 1),
        _ => {
            if r.next_int_n(3) > 0 {
                (block::YELLOW_FLOWER, 0)
            } else {
                (block::RED_FLOWER, 0)
            }
        }
    }
}

/// `getRandomWorldGenForGrass`: tall grass metadata.
fn pick_grass(id: u8, r: &mut JavaRandom) -> i32 {
    match kind(id) {
        Kind::Jungle { .. } => {
            if r.next_int_n(4) == 0 {
                2
            } else {
                1
            }
        }
        Kind::Taiga(_) => {
            if r.next_int_n(5) > 0 {
                2
            } else {
                1
            }
        }
        _ => 1,
    }
}

fn ore1(
    w: &mut ServerWorld,
    r: &mut JavaRandom,
    cx: i32,
    cz: i32,
    n: i32,
    id: u8,
    size: i32,
    lo: i32,
    hi: i32,
) {
    for _ in 0..n {
        let x = cx + r.next_int_n(16);
        let y = r.next_int_n(hi - lo) + lo;
        let z = cz + r.next_int_n(16);
        misc::minable(w, r, x, y, z, id, size, block::STONE);
    }
}

/// `BiomeDecorator.generateOres`
pub fn generate_ores(w: &mut ServerWorld, r: &mut JavaRandom, cx: i32, cz: i32) {
    ore1(w, r, cx, cz, 20, block::DIRT, 32, 0, 256);
    ore1(w, r, cx, cz, 10, block::GRAVEL, 32, 0, 256);
    ore1(w, r, cx, cz, 20, block::COAL_ORE, 16, 0, 128);
    ore1(w, r, cx, cz, 20, block::IRON_ORE, 8, 0, 64);
    ore1(w, r, cx, cz, 2, block::GOLD_ORE, 8, 0, 32);
    ore1(w, r, cx, cz, 8, block::REDSTONE_ORE, 7, 0, 16);
    ore1(w, r, cx, cz, 1, block::DIAMOND_ORE, 7, 0, 16);
    let x = cx + r.next_int_n(16);
    let y = r.next_int_n(16) + r.next_int_n(16) + (16 - 16);
    let z = cz + r.next_int_n(16);
    misc::minable(w, r, x, y, z, block::LAPIS_ORE, 6, block::STONE);
}

/// `BiomeDecorator.genDecorations(biome)` with settings `d`.
fn gen_decorations(w: &mut ServerWorld, r: &mut JavaRandom, biome: u8, d: &Deco, cx: i32, cz: i32) {
    generate_ores(w, r, cx, cz);

    let xz = |r: &mut JavaRandom| {
        let x = cx + r.next_int_n(16) + 8;
        let z = cz + r.next_int_n(16) + 8;
        (x, z)
    };
    for _ in 0..d.sand2 {
        let (x, z) = xz(r);
        let y = w.top_solid_or_liquid(x, z);
        misc::disk(w, r, x, y, z, block::SAND, 7);
    }
    for _ in 0..d.clay {
        let (x, z) = xz(r);
        let y = w.top_solid_or_liquid(x, z);
        misc::disk(w, r, x, y, z, block::CLAY, 4);
    }
    for _ in 0..d.sand {
        let (x, z) = xz(r);
        let y = w.top_solid_or_liquid(x, z);
        misc::disk(w, r, x, y, z, block::GRAVEL, 6);
    }
    let mut n = d.trees;
    if r.next_int_n(10) == 0 {
        n += 1;
    }
    for _ in 0..n {
        let (x, z) = xz(r);
        let y = w.height(x, z);
        let t = pick_tree(biome, r);
        grow_tree(w, r, t, x, y, z);
    }
    for _ in 0..d.big_mushrooms {
        let (x, z) = xz(r);
        let y = w.height(x, z);
        misc::big_mushroom(w, r, x, y, z, -1);
    }
    for _ in 0..d.flowers {
        let (x, z) = xz(r);
        let y = r.next_int_n(w.height(x, z) + 32);
        let (id, meta) = pick_flower(w, biome, r, x, z);
        misc::flowers(w, r, x, y, z, id, meta);
    }
    for _ in 0..d.grass {
        let (x, z) = xz(r);
        let y = r.next_int_n(w.height(x, z) * 2);
        let meta = pick_grass(biome, r);
        misc::tall_grass(w, r, x, y, z, meta);
    }
    for _ in 0..d.dead_bush {
        let (x, z) = xz(r);
        let y = r.next_int_n(w.height(x, z) * 2);
        misc::dead_bush(w, r, x, y, z);
    }
    for _ in 0..d.waterlily {
        let (x, z) = xz(r);
        let mut y = r.next_int_n(w.height(x, z) * 2);
        while y > 0 && w.is_air(x, y - 1, z) {
            y -= 1;
        }
        misc::waterlily(w, r, x, y, z);
    }
    for _ in 0..d.mushrooms {
        if r.next_int_n(4) == 0 {
            let (x, z) = xz(r);
            let y = w.height(x, z);
            misc::flowers(w, r, x, y, z, block::BROWN_MUSHROOM, 0);
        }
        if r.next_int_n(8) == 0 {
            let (x, z) = xz(r);
            let y = r.next_int_n(w.height(x, z) * 2);
            misc::flowers(w, r, x, y, z, block::RED_MUSHROOM, 0);
        }
    }
    if r.next_int_n(4) == 0 {
        let (x, z) = xz(r);
        let y = r.next_int_n(w.height(x, z) * 2);
        misc::flowers(w, r, x, y, z, block::BROWN_MUSHROOM, 0);
    }
    if r.next_int_n(8) == 0 {
        let (x, z) = xz(r);
        let y = r.next_int_n(w.height(x, z) * 2);
        misc::flowers(w, r, x, y, z, block::RED_MUSHROOM, 0);
    }
    for _ in 0..d.reeds {
        let (x, z) = xz(r);
        let y = r.next_int_n(w.height(x, z) * 2);
        misc::reed(w, r, x, y, z);
    }
    for _ in 0..10 {
        let (x, z) = xz(r);
        let y = r.next_int_n(w.height(x, z) * 2);
        misc::reed(w, r, x, y, z);
    }
    if r.next_int_n(32) == 0 {
        let (x, z) = xz(r);
        let y = r.next_int_n(w.height(x, z) * 2);
        misc::pumpkin(w, r, x, y, z);
    }
    for _ in 0..d.cacti {
        let (x, z) = xz(r);
        let y = r.next_int_n(w.height(x, z) * 2);
        misc::cactus(w, r, x, y, z);
    }
    // generateLakes is true for every overworld biome.
    for _ in 0..50 {
        let x = cx + r.next_int_n(16) + 8;
        let inner = r.next_int_n(248) + 8;
        let y = r.next_int_n(inner);
        let z = cz + r.next_int_n(16) + 8;
        misc::spring(w, r, x, y, z, block::FLOWING_WATER);
    }
    for _ in 0..20 {
        let x = cx + r.next_int_n(16) + 8;
        let a = r.next_int_n(240) + 8;
        let b = r.next_int_n(a) + 8;
        let y = r.next_int_n(b);
        let z = cz + r.next_int_n(16) + 8;
        misc::spring(w, r, x, y, z, block::FLOWING_LAVA);
    }
}

/// `biome.decorate(world, rand, x, z)` for the biome object `id`.
fn decorate(w: &mut ServerWorld, r: &mut JavaRandom, id: u8, cx: i32, cz: i32) {
    let k = kind(id);
    let mut d = deco(id);
    let tall = |w: &mut ServerWorld, r: &mut JavaRandom, variant: i32| -> bool {
        let x = cx + r.next_int_n(16) + 8;
        let z = cz + r.next_int_n(16) + 8;
        let y = r.next_int_n(w.height(x, z) + 32);
        misc::double_plant(w, r, x, y, z, variant)
    };
    match k {
        Kind::Mutated | Kind::BirchMutated => {
            let d = deco(id - 128);
            gen_decorations(w, r, id, &d, cx, cz);
            return;
        }
        Kind::RoofedMutated => {
            decorate(w, r, id - 128, cx, cz);
            return;
        }
        Kind::Forest(t) => {
            if t == 3 {
                for i in 0..4 {
                    for j in 0..4 {
                        let x = cx + i * 4 + 1 + 8 + r.next_int_n(3);
                        let z = cz + j * 4 + 1 + 8 + r.next_int_n(3);
                        let y = w.height(x, z);
                        if r.next_int_n(20) == 0 {
                            misc::big_mushroom(w, r, x, y, z, -1);
                        } else {
                            let t = pick_tree(id, r);
                            grow_tree(w, r, t, x, y, z);
                        }
                    }
                }
            }
            let mut n = r.next_int_n(5) - 3;
            if t == 1 {
                n += 2;
            }
            for _ in 0..n {
                let variant = match r.next_int_n(3) {
                    0 => 1,
                    1 => 4,
                    _ => 5,
                };
                for _ in 0..5 {
                    if tall(w, r, variant) {
                        break;
                    }
                }
            }
        }
        Kind::Plains { sunflowers } => {
            let v = w
                .provider
                .terrain
                .noise
                .plant
                .get_value((cx + 8) as f64 / 200.0, (cz + 8) as f64 / 200.0);
            if v < -0.8 {
                d.flowers = 15;
                d.grass = 5;
            } else {
                d.flowers = 4;
                d.grass = 10;
                for _ in 0..7 {
                    tall(w, r, 2);
                }
            }
            if sunflowers {
                for _ in 0..10 {
                    tall(w, r, 0);
                }
            }
        }
        Kind::Taiga(t) => {
            if t == 1 || t == 2 {
                let n = r.next_int_n(3);
                for _ in 0..n {
                    let x = cx + r.next_int_n(16) + 8;
                    let z = cz + r.next_int_n(16) + 8;
                    let y = w.height(x, z);
                    misc::block_blob(w, r, x, y, z, block::MOSSY_COBBLESTONE, 0);
                }
            }
            for _ in 0..7 {
                tall(w, r, 3);
            }
        }
        Kind::Savanna => {
            for _ in 0..7 {
                tall(w, r, 2);
            }
        }
        Kind::Snow { icy } => {
            if icy {
                for _ in 0..3 {
                    let x = cx + r.next_int_n(16) + 8;
                    let z = cz + r.next_int_n(16) + 8;
                    let y = w.height(x, z);
                    misc::ice_spike(w, r, x, y, z);
                }
                for _ in 0..2 {
                    let x = cx + r.next_int_n(16) + 8;
                    let z = cz + r.next_int_n(16) + 8;
                    let y = w.height(x, z);
                    misc::ice_path(w, r, x, y, z, 4);
                }
            }
        }
        _ => {}
    }
    gen_decorations(w, r, id, &d, cx, cz);
    match k {
        Kind::Hills { .. } => {
            let n = 3 + r.next_int_n(6);
            for _ in 0..n {
                let x = cx + r.next_int_n(16);
                let y = r.next_int_n(28) + 4;
                let z = cz + r.next_int_n(16);
                if w.block(x, y, z) == block::STONE {
                    w.set_block(x, y, z, block::EMERALD_ORE, 0, 2);
                }
            }
            for _ in 0..7 {
                let x = cx + r.next_int_n(16);
                let y = r.next_int_n(64);
                let z = cz + r.next_int_n(16);
                misc::minable(w, r, x, y, z, block::MONSTER_EGG, 8, block::STONE);
            }
        }
        Kind::Jungle { .. } => {
            let x = cx + r.next_int_n(16) + 8;
            let z = cz + r.next_int_n(16) + 8;
            let y = r.next_int_n(w.height(x, z) * 2);
            misc::melon(w, r, x, y, z);
            for _ in 0..50 {
                let x = cx + r.next_int_n(16) + 8;
                let z = cz + r.next_int_n(16) + 8;
                misc::vines(w, r, x, 128, z);
            }
        }
        Kind::Desert => {
            if r.next_int_n(1000) == 0 {
                let x = cx + r.next_int_n(16) + 8;
                let z = cz + r.next_int_n(16) + 8;
                let y = w.height(x, z) + 1;
                misc::desert_well(w, r, x, y, z);
            }
        }
        _ => {}
    }
}

/// `ChunkProviderGenerate.populate(provider, cx, cz)`
pub fn populate(w: &mut ServerWorld, cx: i32, cz: i32) {
    w.fall_instantly = true;
    let (bx, bz) = (cx * 16, cz * 16);
    let biome: &'static Biome = w.biome(bx + 16, bz + 16);
    let seed = w.seed();
    let mut r = JavaRandom::new(seed);
    let a = r.next_long() / 2 * 2 + 1;
    let b = r.next_long() / 2 * 2 + 1;
    r.set_seed(
        (cx as i64)
            .wrapping_mul(a)
            .wrapping_add((cz as i64).wrapping_mul(b))
            ^ seed,
    );

    let village = w.map_features && super::structure::generate_in_chunk(w, &mut r, cx, cz);
    if biome.id != 2 && biome.id != 17 && !village && r.next_int_n(4) == 0 {
        let x = bx + r.next_int_n(16) + 8;
        let y = r.next_int_n(256);
        let z = bz + r.next_int_n(16) + 8;
        misc::lake(w, &mut r, x, y, z, block::WATER);
    }
    if !village && r.next_int_n(8) == 0 {
        let x = bx + r.next_int_n(16) + 8;
        let inner = r.next_int_n(248) + 8;
        let y = r.next_int_n(inner);
        let z = bz + r.next_int_n(16) + 8;
        if y < 63 || r.next_int_n(10) == 0 {
            misc::lake(w, &mut r, x, y, z, block::LAVA);
        }
    }
    for _ in 0..8 {
        let x = bx + r.next_int_n(16) + 8;
        let y = r.next_int_n(256);
        let z = bz + r.next_int_n(16) + 8;
        super::features::dungeon::dungeon(w, &mut r, x, y, z);
    }
    decorate(w, &mut r, biome.id, bx, bz);
    crate::entity::mob::world_gen_spawning(w, biome.id as i32, bx + 8, bz + 8, &mut r);
    let (sx, sz) = (bx + 8, bz + 8);
    for i in 0..16 {
        for j in 0..16 {
            let y = w.precipitation_height(sx + i, sz + j);
            if w.can_block_freeze(sx + i, y - 1, sz + j, false) {
                w.set_block(sx + i, y - 1, sz + j, block::ICE, 0, 2);
            }
            if w.can_snow_at(sx + i, y, sz + j, true) {
                w.set_block(sx + i, y, sz + j, block::SNOW_LAYER, 0, 2);
            }
        }
    }
    w.fall_instantly = false;
    let _ = Terrain::Default;
}
