//! `WorldGenDungeons`, and the enchanted-book roll shared by every loot chest.

use crate::block;
use crate::java::JavaRandom;
use crate::world::loot;
use crate::world::server::{GenTile, ServerWorld};

#[derive(Clone, Copy, PartialEq)]
enum Ench {
    Protection(usize),
    Oxygen,
    WaterWorker,
    Thorns,
    Damage(usize),
    Knockback,
    FireAspect,
    LootBonus,
    Digging,
    Untouching,
    Durability,
    ArrowDamage,
    ArrowKnockback,
    ArrowFire,
    ArrowInfinite,
    FishingSpeed,
}

/// `(effectId, weight, kind)` in `Enchantment.enchantmentsList` order.
const ENCHANTMENTS: [(i32, i32, Ench); 24] = [
    (0, 10, Ench::Protection(0)),
    (1, 5, Ench::Protection(1)),
    (2, 5, Ench::Protection(2)),
    (3, 2, Ench::Protection(3)),
    (4, 5, Ench::Protection(4)),
    (5, 2, Ench::Oxygen),
    (6, 2, Ench::WaterWorker),
    (7, 1, Ench::Thorns),
    (16, 10, Ench::Damage(0)),
    (17, 5, Ench::Damage(1)),
    (18, 5, Ench::Damage(2)),
    (19, 5, Ench::Knockback),
    (20, 2, Ench::FireAspect),
    (21, 2, Ench::LootBonus),
    (32, 10, Ench::Digging),
    (33, 1, Ench::Untouching),
    (34, 5, Ench::Durability),
    (35, 2, Ench::LootBonus),
    (48, 10, Ench::ArrowDamage),
    (49, 2, Ench::ArrowKnockback),
    (50, 2, Ench::ArrowFire),
    (51, 1, Ench::ArrowInfinite),
    (61, 2, Ench::LootBonus),
    (62, 2, Ench::FishingSpeed),
];

fn max_level(e: Ench) -> i32 {
    match e {
        Ench::Protection(_) => 4,
        Ench::Oxygen | Ench::Thorns | Ench::LootBonus | Ench::Durability | Ench::FishingSpeed => 3,
        Ench::Damage(_) | Ench::Digging | Ench::ArrowDamage => 5,
        Ench::Knockback | Ench::FireAspect | Ench::ArrowKnockback => 2,
        Ench::WaterWorker | Ench::Untouching | Ench::ArrowFire | Ench::ArrowInfinite => 1,
    }
}

/// `(getMinEnchantability(l), getMaxEnchantability(l))`
fn range(e: Ench, l: i32) -> (i32, i32) {
    let sup = 1 + l * 10 + 50;
    match e {
        Ench::Protection(t) => {
            let min = [1, 10, 5, 5, 3][t] + (l - 1) * [11, 8, 6, 8, 6][t];
            (min, min + [20, 12, 10, 12, 15][t])
        }
        Ench::Oxygen => (10 * l, 10 * l + 30),
        Ench::WaterWorker => (1, 41),
        Ench::Thorns => (10 + 20 * (l - 1), sup),
        Ench::Damage(t) => {
            let min = [1, 5, 5][t] + (l - 1) * [11, 8, 8][t];
            (min, min + 20)
        }
        Ench::Knockback => (5 + 20 * (l - 1), sup),
        Ench::FireAspect => (10 + 20 * (l - 1), sup),
        Ench::LootBonus | Ench::FishingSpeed => (15 + (l - 1) * 9, sup),
        Ench::Digging => (1 + 10 * (l - 1), sup),
        Ench::Untouching => (15, sup),
        Ench::Durability => (5 + (l - 1) * 8, sup),
        Ench::ArrowDamage => {
            let min = 1 + (l - 1) * 10;
            (min, min + 15)
        }
        Ench::ArrowKnockback => {
            let min = 12 + (l - 1) * 20;
            (min, min + 25)
        }
        Ench::ArrowFire | Ench::ArrowInfinite => (20, 50),
    }
}

/// `a.canApplyTogether(b)`
fn compatible(a: (i32, Ench), b: (i32, Ench)) -> bool {
    match a.1 {
        Ench::Protection(t) => match b.1 {
            Ench::Protection(u) => {
                if t == u {
                    false
                } else {
                    t == 2 || u == 2
                }
            }
            _ => a.0 != b.0,
        },
        Ench::Damage(_) => !matches!(b.1, Ench::Damage(_)),
        Ench::LootBonus => a.0 != b.0 && b.0 != 33,
        Ench::Untouching => a.0 != b.0 && b.0 != 35,
        _ => a.0 != b.0,
    }
}

/// `EnchantmentHelper.buildEnchantmentList(rand, book, 30)`: the
/// `(id, level)` list a random enchanted book receives.
pub fn roll_enchanted_book(r: &mut JavaRandom) -> Vec<(i32, i32)> {
    // Book enchantability 1: `1 + nextInt(1) + nextInt(1)`.
    let k = 1 + r.next_int_n(1) + r.next_int_n(1);
    let level = k + 30;
    let f = (r.next_float() + r.next_float() - 1.0f32) * 0.15f32;
    let mut power = (level as f32 * (1.0f32 + f) + 0.5f32) as i32;
    if power < 1 {
        power = 1;
    }
    // mapEnchantmentData: one entry per enchantment (highest matching level).
    let mut map: Vec<(i32, i32, Ench)> = Vec::new();
    let mut levels: Vec<(i32, i32)> = Vec::new();
    for &(id, weight, e) in ENCHANTMENTS.iter() {
        let mut found = 0;
        for l in 1..=max_level(e) {
            let (lo, hi) = range(e, l);
            if power >= lo && power <= hi {
                found = l;
            }
        }
        if found > 0 {
            map.push((id, weight, e));
            levels.push((id, found));
        }
    }
    if map.is_empty() {
        return Vec::new();
    }
    // java.util.HashMap iteration order for Integer keys: by bucket, then by
    // insertion; the table never shrinks when entries are removed.
    let cap = if map.len() <= 12 {
        16
    } else if map.len() <= 24 {
        32
    } else {
        64
    };
    map.sort_by_key(|e| e.0 & (cap - 1));
    let pick = |r: &mut JavaRandom, map: &[(i32, i32, Ench)]| -> Option<(i32, Ench)> {
        let total: i32 = map.iter().map(|e| e.1).sum();
        let mut n = r.next_int_n(total);
        for e in map {
            n -= e.1;
            if n < 0 {
                return Some((e.0, e.2));
            }
        }
        None
    };
    let Some(first) = pick(r, &map) else {
        return Vec::new();
    };
    let mut chosen = vec![first];
    let mut i = power;
    while r.next_int_n(50) <= i {
        map.retain(|e| chosen.iter().all(|c| compatible(*c, (e.0, e.2))));
        if !map.is_empty() {
            if let Some(e) = pick(r, &map) {
                chosen.push(e);
            }
        }
        i >>= 1;
    }
    chosen
        .iter()
        .map(|c| (c.0, levels.iter().find(|l| l.0 == c.0).map_or(1, |l| l.1)))
        .collect()
}

pub fn dungeon(w: &mut ServerWorld, r: &mut JavaRandom, x: i32, y: i32, z: i32) -> bool {
    let h = 3;
    let rx = r.next_int_n(2) + 2;
    let rz = r.next_int_n(2) + 2;
    let mut openings = 0;
    for bx in (x - rx - 1)..=(x + rx + 1) {
        for by in (y - 1)..=(y + h + 1) {
            for bz in (z - rz - 1)..=(z + rz + 1) {
                let solid = w.material(bx, by, bz).is_solid();
                if by == y - 1 && !solid {
                    return false;
                }
                if by == y + h + 1 && !solid {
                    return false;
                }
                if (bx == x - rx - 1 || bx == x + rx + 1 || bz == z - rz - 1 || bz == z + rz + 1)
                    && by == y
                    && w.is_air(bx, by, bz)
                    && w.is_air(bx, by + 1, bz)
                {
                    openings += 1;
                }
            }
        }
    }
    if !(1..=5).contains(&openings) {
        return false;
    }
    for bx in (x - rx - 1)..=(x + rx + 1) {
        let mut by = y + h;
        while by >= y - 1 {
            for bz in (z - rz - 1)..=(z + rz + 1) {
                if bx != x - rx - 1
                    && by != y - 1
                    && bz != z - rz - 1
                    && bx != x + rx + 1
                    && by != y + h + 1
                    && bz != z + rz + 1
                {
                    w.set_air(bx, by, bz);
                } else if by >= 0 && !w.material(bx, by - 1, bz).is_solid() {
                    w.set_air(bx, by, bz);
                } else if w.material(bx, by, bz).is_solid() {
                    if by == y - 1 && r.next_int_n(4) != 0 {
                        w.set_block(bx, by, bz, block::MOSSY_COBBLESTONE, 0, 2);
                    } else {
                        w.set_block(bx, by, bz, block::COBBLESTONE, 0, 2);
                    }
                }
            }
            by -= 1;
        }
    }
    for _ in 0..2 {
        for _ in 0..3 {
            let cx = x + r.next_int_n(rx * 2 + 1) - rx;
            let cz = z + r.next_int_n(rz * 2 + 1) - rz;
            if w.is_air(cx, y, cz) {
                let mut walls = 0;
                for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    if w.material(cx + dx, y, cz + dz).is_solid() {
                        walls += 1;
                    }
                }
                if walls == 1 {
                    w.set_block(cx, y, cz, block::CHEST, 0, 2);
                    let book = loot::enchanted_book(r, 1, 1, 1);
                    let mut slots = vec![None; 27];
                    loot::fill(r, &[loot::DUNGEON, &[book]].concat(), &mut slots, 8);
                    w.gen_tiles.push(GenTile::Chest {
                        pos: [cx, y, cz],
                        slots,
                    });
                    break;
                }
            }
        }
    }
    w.set_block(x, y, z, block::MOB_SPAWNER, 0, 2);
    // func_76543_b
    let mob = ["Skeleton", "Zombie", "Zombie", "Spider"][r.next_int_n(4) as usize];
    w.gen_tiles.push(GenTile::Spawner {
        pos: [x, y, z],
        mob,
    });
    true
}
