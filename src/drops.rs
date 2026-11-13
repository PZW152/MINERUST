//! What a block leaves behind: `Block.dropBlockAsItemWithChance` and the
//! methods it is built from (`quantityDropped`, `quantityDroppedWithBonus`,
//! `getItemDropped`, `damageDropped`), `canSilkHarvest` and
//! `createStackedBlock`, class by class. Every call on the world's random
//! generator is made in the original order, the three floats that place
//! each item included; `tools/DropHarness.java` checks all of it.

use crate::item::{ItemId, Stack};
use crate::java::JavaRandom;

/// Something `dropBlockAsItemWithChance` spawns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Spawn {
    /// An `EntityItem` and its offset inside the block
    /// (`Block.dropBlockAsItem(World, int, int, int, ItemStack)`).
    Item(Stack, [f64; 3]),
    /// One `EntityXPOrb` of `dropXpOnBlockBreak`.
    Xp(i32),
    /// `BlockSilverfish`: the silverfish that was hiding inside.
    Silverfish,
}

/// Blocks without an `ItemBlock` (`Item.registerItems`): their
/// `Item.getItemFromBlock` is null.
fn item_of(id: u8) -> Option<ItemId> {
    match id {
        0 | 26 | 34 | 36 | 55 | 59 | 63 | 64 | 68 | 71 | 74 | 75 | 83 | 92 | 93 | 94 | 104
        | 105 | 115 | 117 | 118 | 124 | 132 | 140 | 144 | 149 | 150 => None,
        _ => Some(id as ItemId),
    }
}

/// `MathHelper.getRandomIntegerInRange`
fn range(r: &mut JavaRandom, a: i32, b: i32) -> i32 {
    if a >= b {
        a
    } else {
        r.next_int_n(b - a + 1) + a
    }
}

/// `BlockOre`
fn is_ore(id: u8) -> bool {
    matches!(id, 14 | 15 | 16 | 21 | 56 | 129 | 153)
}

/// `BlockCrops.getSeed` and `getCrop`
fn crop_items(id: u8) -> (ItemId, ItemId) {
    match id {
        141 => (391, 391),
        142 => (392, 392),
        _ => (295, 296),
    }
}

/// `Block.quantityDropped(Random)`
fn quantity_dropped(id: u8, r: &mut JavaRandom) -> i32 {
    match id {
        // liquids, glass, piston parts, fire, mob spawner, snow layer, ice,
        // portals, cake, stained glass, monster egg, vine, nether wart,
        // command block, packed ice
        8..=11 | 20 | 34 | 51 | 52 | 78 | 79 | 90 | 92 | 95 | 97 | 106 | 115 | 119 | 137 | 174 => 0,
        // BlockLeaves
        18 | 161 => {
            if r.next_int_n(20) == 0 {
                1
            } else {
                0
            }
        }
        21 => 4 + r.next_int_n(5),
        // double slabs
        43 | 125 => 2,
        47 => 3,
        73 | 74 => 4 + r.next_int_n(2),
        80 | 82 => 4,
        89 => 2 + r.next_int_n(3),
        // BlockHugeMushroom
        99 | 100 => (r.next_int_n(10) - 7).max(0),
        103 => 3 + r.next_int_n(5),
        130 => 8,
        _ => 1,
    }
}

/// `Block.quantityDroppedWithBonus(fortune, Random)`
fn quantity_dropped_with_bonus(id: u8, fortune: i32, r: &mut JavaRandom) -> i32 {
    match id {
        // BlockOre: only the ores that drop something else multiply
        _ if is_ore(id) => {
            if fortune > 0 && item_of(id) != get_item_dropped(id, 0, r, fortune) {
                let j = (r.next_int_n(fortune + 2) - 1).max(0);
                quantity_dropped(id, r) * (j + 1)
            } else {
                quantity_dropped(id, r)
            }
        }
        31 => 1 + r.next_int_n(fortune * 2 + 1),
        73 | 74 => quantity_dropped(id, r) + r.next_int_n(fortune + 1),
        89 => (quantity_dropped(id, r) + r.next_int_n(fortune + 1)).clamp(1, 4),
        103 => (quantity_dropped(id, r) + r.next_int_n(1 + fortune)).min(9),
        _ => quantity_dropped(id, r),
    }
}

/// `Block.getItemDropped(meta, Random, fortune)`
fn get_item_dropped(id: u8, meta: i32, r: &mut JavaRandom, fortune: i32) -> Option<ItemId> {
    Some(match id {
        1 => 4,
        // grass, farmland, mycelium: Blocks.dirt.getItemDropped
        2 | 60 | 110 => 3,
        8..=11 | 32 | 36 | 52 | 92 | 104 | 105 | 106 | 115 | 120 => return None,
        13 => {
            let f = fortune.min(3);
            if r.next_int_n(10 - f * 3) == 0 {
                318
            } else {
                13
            }
        }
        16 => 263,
        21 => 351,
        56 => 264,
        129 => 388,
        153 => 406,
        // BlockLeaves: a sapling
        18 | 161 => 6,
        // BlockBed: only the foot drops
        26 => {
            if meta & 8 != 0 {
                return None;
            }
            355
        }
        30 | 132 => 287,
        31 => {
            if r.next_int_n(8) == 0 {
                295
            } else {
                return None;
            }
        }
        43 | 44 => 44,
        125 | 126 => 126,
        47 => 340,
        55 | 73 | 74 => 331,
        59 | 141 | 142 => {
            let (seed, crop) = crop_items(id);
            if meta == 7 {
                crop
            } else {
                seed
            }
        }
        61 | 62 => 61,
        63 | 68 => 323,
        64 | 71 => {
            if meta & 8 != 0 {
                return None;
            }
            if id == 71 {
                330
            } else {
                324
            }
        }
        75 | 76 => 76,
        78 | 80 => 332,
        82 => 337,
        83 => 338,
        89 => 348,
        93 | 94 => 356,
        // BlockHugeMushroom: the small mushroom of its kind
        99 => 39,
        100 => 40,
        // BlockPane: glass panes cannot drop
        102 | 160 => return None,
        103 => 360,
        117 => 379,
        118 => 380,
        123 | 124 => 123,
        130 => 49,
        140 => 390,
        144 => 397,
        149 | 150 => 404,
        // BlockDoublePlant: the upper half and the two grasses drop nothing
        175 => {
            if meta & 8 != 0 || matches!(meta & 7, 2 | 3) {
                return None;
            }
            175
        }
        _ => return item_of(id),
    })
}

/// `Block.damageDropped(meta)`
fn damage_dropped(id: u8, meta: i32) -> i32 {
    match id {
        // planks, sand, sandstone, wool, flowers, stained glass and panes,
        // stone brick, walls, skull, stained clay, carpet
        5 | 12 | 24 | 35 | 37 | 38 | 95 | 98 | 139 | 144 | 159 | 160 | 171 => meta,
        6 => (meta & 7).clamp(0, 5),
        // BlockRotatedPillar, BlockOldLeaf
        17 | 162 | 170 | 18 => meta & 3,
        161 => (meta & 3) + 4,
        21 => 4,
        43 | 44 | 125 | 126 => meta & 7,
        145 => meta >> 2,
        155 => {
            if meta == 3 || meta == 4 {
                2
            } else {
                meta
            }
        }
        175 => {
            if meta & 8 != 0 {
                0
            } else {
                meta & 7
            }
        }
        _ => 0,
    }
}

/// `Block.dropBlockAsItem(World, x, y, z, ItemStack)`
fn drop_item(out: &mut Vec<Spawn>, r: &mut JavaRandom, s: Stack) {
    let f = 0.7f32;
    let mut at = [0.0f64; 3];
    for a in &mut at {
        *a = (r.next_float() * f) as f64 + (1.0f32 - f) as f64 * 0.5;
    }
    out.push(Spawn::Item(s, at));
}

/// `Block.dropBlockAsItem(World, x, y, z, ItemStack)` for one stack, as
/// `harvestBlock` uses it for Silk Touch.
pub fn drop_stack(r: &mut JavaRandom, s: Stack) -> Vec<Spawn> {
    let mut v = Vec::new();
    drop_item(&mut v, r, s);
    v
}

/// `Block.dropXpOnBlockBreak`
fn drop_xp(out: &mut Vec<Spawn>, mut amount: i32) {
    while amount > 0 {
        let split = crate::entity::xp_orb::xp_split(amount);
        amount -= split;
        out.push(Spawn::Xp(split));
    }
}

/// `Block.dropBlockAsItemWithChance` of the base class.
fn base(id: u8, meta: i32, chance: f32, fortune: i32, r: &mut JavaRandom, out: &mut Vec<Spawn>) {
    let n = quantity_dropped_with_bonus(id, fortune, r);
    for _ in 0..n {
        if !(r.next_float() > chance) {
            if let Some(item) = get_item_dropped(id, meta, r, fortune) {
                drop_item(out, r, Stack::new(item, damage_dropped(id, meta), 1));
            }
        }
    }
}

/// `Block.dropBlockAsItemWithChance(world, x, y, z, meta, chance, fortune)`
/// on the server, with `r` as `World.rand`.
pub fn drop_block_as_item_with_chance(
    id: u8,
    meta: i32,
    chance: f32,
    fortune: i32,
    r: &mut JavaRandom,
) -> Vec<Spawn> {
    let mut v = Vec::new();
    let out = &mut v;
    match id {
        0 | 144 => {}
        // BlockOre
        _ if is_ore(id) => {
            base(id, meta, chance, fortune, r, out);
            if get_item_dropped(id, meta, r, fortune) != item_of(id) {
                let xp = match id {
                    16 => range(r, 0, 2),
                    56 | 129 => range(r, 3, 7),
                    21 | 153 => range(r, 2, 5),
                    _ => 0,
                };
                drop_xp(out, xp);
            }
        }
        // BlockLeaves
        18 | 161 => {
            let mut c = if id == 18 && meta & 3 == 3 { 40 } else { 20 };
            if fortune > 0 {
                c -= 2 << fortune;
                if c < 10 {
                    c = 10;
                }
            }
            if r.next_int_n(c) == 0 {
                drop_item(out, r, Stack::new(6, damage_dropped(id, meta), 1));
            }
            c = 200;
            if fortune > 0 {
                c -= 10 << fortune;
                if c < 40 {
                    c = 40;
                }
            }
            // dropAppleWithChance: oak and dark oak
            let apple = if id == 18 { 0 } else { 1 };
            if meta & 3 == apple && r.next_int_n(c) == 0 {
                drop_item(out, r, Stack::new(260, 0, 1));
            }
        }
        // BlockBed: the head leaves it to the foot
        26 => {
            if meta & 8 == 0 {
                base(id, meta, chance, 0, r, out);
            }
        }
        // BlockMobSpawner
        52 => {
            base(id, meta, chance, fortune, r, out);
            let xp = 15 + r.next_int_n(15) + r.next_int_n(15);
            drop_xp(out, xp);
        }
        // BlockCrops, BlockPotato
        59 | 141 | 142 => {
            base(id, meta, chance, 0, r, out);
            if meta >= 7 {
                for _ in 0..3 + fortune {
                    if r.next_int_n(15) <= meta {
                        drop_item(out, r, Stack::new(crop_items(id).0, 0, 1));
                    }
                }
            }
            if id == 142 && meta >= 7 && r.next_int_n(50) == 0 {
                drop_item(out, r, Stack::new(394, 0, 1));
            }
        }
        // BlockRedstoneOre
        73 | 74 => {
            base(id, meta, chance, fortune, r, out);
            let xp = 1 + r.next_int_n(5);
            drop_xp(out, xp);
        }
        // BlockJukebox
        84 => base(id, meta, chance, 0, r, out),
        // BlockSilverfish
        97 => out.push(Spawn::Silverfish),
        // BlockStem
        104 | 105 => {
            base(id, meta, chance, fortune, r, out);
            let seeds = if id == 104 { 361 } else { 362 };
            for _ in 0..3 {
                if r.next_int_n(15) <= meta {
                    drop_item(out, r, Stack::new(seeds, 0, 1));
                }
            }
        }
        // BlockNetherWart
        115 => {
            let mut n = 1;
            if meta >= 3 {
                n = 2 + r.next_int_n(3);
                if fortune > 0 {
                    n += r.next_int_n(fortune + 1);
                }
            }
            for _ in 0..n {
                drop_item(out, r, Stack::new(372, 0, 1));
            }
        }
        // BlockCocoa: getNextGrowthStage
        127 => {
            let n = if (meta & 12) >> 2 >= 2 { 3 } else { 1 };
            for _ in 0..n {
                drop_item(out, r, Stack::new(351, 3, 1));
            }
        }
        _ => base(id, meta, chance, fortune, r, out),
    }
    v
}

/// `Block.canSilkHarvest`: `renderAsNormalBlock() && !isBlockContainer`
/// unless the class says otherwise; as reported by the game for every block.
pub fn can_silk_harvest(id: u8) -> bool {
    SILK.contains(&id)
}

include!("drops_silk.rs");

/// `Block.createStackedBlock(meta)`: what Silk Touch yields.
pub fn create_stacked_block(id: u8, meta: i32) -> Option<Stack> {
    Some(match id {
        // BlockDirt: coarse dirt is not kept
        3 => Stack::new(3, if meta == 1 { 0 } else { meta }, 1),
        // BlockLeaves, BlockRotatedPillar
        18 | 161 | 17 | 162 | 170 => Stack::new(id as ItemId, meta & 3, 1),
        43 | 44 => Stack::new(44, meta & 7, 2),
        125 | 126 => Stack::new(126, meta & 7, 2),
        73 | 74 => Stack::new(73, 0, 1),
        123 | 124 => Stack::new(123, 0, 1),
        // BlockSilverfish: the block it imitates
        97 => match meta {
            1 => Stack::new(4, 0, 1),
            2 => Stack::new(98, 0, 1),
            3 => Stack::new(98, 1, 1),
            4 => Stack::new(98, 2, 1),
            5 => Stack::new(98, 3, 1),
            _ => Stack::new(1, 0, 1),
        },
        // BlockPane
        101 | 102 | 160 => Stack::new(id as ItemId, meta, 1),
        155 if meta == 3 || meta == 4 => Stack::new(155, 2, 1),
        _ => {
            let item = item_of(id)?;
            // `Item.getHasSubtypes`
            let sub = SUBTYPES.contains(&id);
            Stack::new(item, if sub { meta } else { 0 }, 1)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn show(v: &[Spawn]) -> String {
        v.iter()
            .map(|s| match s {
                Spawn::Item(s, _) => format!("i{}:{}:{}", s.id, s.meta, s.count),
                Spawn::Xp(n) => format!("x{n}"),
                Spawn::Silverfish => "eEntitySilverfish".to_string(),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// `tools/DropHarness.java`: every block and metadata, fortune 0 to 3,
    /// four seeds each; the last number is the next `rand.nextInt()`.
    #[test]
    fn drops_match_the_game() {
        let (mut id, mut meta) = (0u8, 0i32);
        let mut bad = Vec::new();
        let mut checked = 0;
        for line in include_str!("../tools/drops_1710.txt").lines() {
            if !line.starts_with(' ') {
                let f: Vec<&str> = line.split(' ').collect();
                id = f[0].parse().unwrap();
                meta = f[1].parse().unwrap();
                let silk = f[2].strip_prefix("silk=").unwrap();
                if silk != "E" && (silk == "1") != can_silk_harvest(id) {
                    bad.push(format!("{id}: canSilkHarvest {silk}"));
                }
                let st = f[3].strip_prefix("st=").unwrap();
                if st != "E" {
                    let got = match create_stacked_block(id, meta) {
                        Some(s) => format!("{}:{}", s.id, s.meta),
                        None => "-".to_string(),
                    };
                    if got != st
                        && !bad
                            .iter()
                            .any(|b: &String| b.starts_with(&format!("{id}:")))
                    {
                        bad.push(format!("{id}:{meta} createStackedBlock {got} (game: {st})"));
                    }
                }
                continue;
            }
            let f: Vec<&str> = line.trim_start().splitn(3, ' ').collect();
            let (fortune, seed): (i32, i64) = (f[0].parse().unwrap(), f[1].parse().unwrap());
            let rest = f.get(2).copied().unwrap_or("");
            // the stand-in world cannot run these (they read the world)
            if rest.starts_with("ERR") {
                continue;
            }
            let mut r = JavaRandom::new(seed * 7919 + fortune as i64 * 31 + meta as i64);
            let v = drop_block_as_item_with_chance(id, meta, 1.0, fortune, &mut r);
            let got = format!("{} |{}", show(&v), r.next_int());
            checked += 1;
            if got != rest
                && !bad
                    .iter()
                    .any(|b: &String| b.starts_with(&format!("{id}:")))
            {
                bad.push(format!(
                    "{id}:{meta} fortune {fortune} seed {seed}: {got} (game: {rest})"
                ));
            }
        }
        for b in &bad {
            eprintln!("{b}");
        }
        assert!(bad.is_empty(), "{} differences", bad.len());
        assert!(checked > 40000);
    }
}
