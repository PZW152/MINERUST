//! `Enchantment` (the 24 of 1.7.10), `EnumEnchantmentType.canEnchantItem`,
//! `Item.getItemEnchantability` and `EnchantmentHelper`'s
//! `calcItemStackEnchantability`, `mapEnchantmentData` and
//! `buildEnchantmentList`.

use crate::item::{ItemId, Stack};
use crate::java::JavaRandom;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Type {
    Armor,
    ArmorFeet,
    ArmorHead,
    ArmorTorso,
    Weapon,
    Digger,
    Breakable,
    Bow,
    FishingRod,
}

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

/// `(effectId, weight, kind, type)` in `Enchantment.enchantmentsList` order.
const ENCHANTMENTS: [(i32, i32, Ench, Type); 24] = [
    (0, 10, Ench::Protection(0), Type::Armor),
    (1, 5, Ench::Protection(1), Type::Armor),
    (2, 5, Ench::Protection(2), Type::ArmorFeet),
    (3, 2, Ench::Protection(3), Type::Armor),
    (4, 5, Ench::Protection(4), Type::Armor),
    (5, 2, Ench::Oxygen, Type::ArmorHead),
    (6, 2, Ench::WaterWorker, Type::ArmorHead),
    (7, 1, Ench::Thorns, Type::ArmorTorso),
    (16, 10, Ench::Damage(0), Type::Weapon),
    (17, 5, Ench::Damage(1), Type::Weapon),
    (18, 5, Ench::Damage(2), Type::Weapon),
    (19, 5, Ench::Knockback, Type::Weapon),
    (20, 2, Ench::FireAspect, Type::Weapon),
    (21, 2, Ench::LootBonus, Type::Weapon),
    (32, 10, Ench::Digging, Type::Digger),
    (33, 1, Ench::Untouching, Type::Digger),
    (34, 5, Ench::Durability, Type::Breakable),
    (35, 2, Ench::LootBonus, Type::Digger),
    (48, 10, Ench::ArrowDamage, Type::Bow),
    (49, 2, Ench::ArrowKnockback, Type::Bow),
    (50, 2, Ench::ArrowFire, Type::Bow),
    (51, 1, Ench::ArrowInfinite, Type::Bow),
    (61, 2, Ench::LootBonus, Type::FishingRod),
    (62, 2, Ench::FishingSpeed, Type::FishingRod),
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

/// `EnumEnchantmentType.canEnchantItem`
fn can_enchant(t: Type, id: ItemId) -> bool {
    // breakable takes any Item.isDamageable first
    if t == Type::Breakable && crate::item::max_damage(id) > 0 && !crate::item::has_subtypes(id) {
        return true;
    }
    if let Some(a) = crate::container::armor_type(id) {
        // armor_legs has no enchantment in 1.7.10
        return t == Type::Armor
            || match a {
                0 => t == Type::ArmorHead,
                1 => t == Type::ArmorTorso,
                2 => false,
                _ => t == Type::ArmorFeet,
            };
    }
    if let Some(tool) = crate::item::tool(id) {
        return match tool.kind {
            crate::item::ToolKind::Sword => t == Type::Weapon,
            // ItemHoe is not an ItemTool
            crate::item::ToolKind::Hoe => false,
            _ => t == Type::Digger,
        };
    }
    match id {
        261 => t == Type::Bow,
        346 => t == Type::FishingRod,
        _ => false,
    }
}

/// `Item.getItemEnchantability`
pub fn enchantability(id: ItemId) -> i32 {
    if let Some(t) = crate::item::tool(id) {
        if t.kind == crate::item::ToolKind::Hoe {
            return 0;
        }
        // ToolMaterial: wood, stone, iron, diamond, gold
        return match t.max_uses {
            59 => 15,
            131 => 5,
            250 => 14,
            1561 => 10,
            _ => 22,
        };
    }
    if (298..=317).contains(&id) {
        // ArmorMaterial: cloth, chain, iron, diamond, gold
        return [15, 12, 9, 10, 25][(id as usize - 298) / 4];
    }
    match id {
        261 | 346 | 340 => 1,
        _ => 0,
    }
}

/// `ItemStack.isItemEnchantable`: a tool (`isItemTool`, or a single book)
/// without enchantments.
pub fn enchantable(s: &Stack) -> bool {
    let tool = if s.id == 340 {
        s.count == 1
    } else {
        crate::item::max_stack(s.id) == 1 && crate::item::max_damage(s.id) > 0
    };
    tool && !s.is_enchanted()
}

/// `EnchantmentHelper.calcItemStackEnchantability`
pub fn calc_level(r: &mut JavaRandom, slot: usize, shelves: i32, s: &Stack) -> i32 {
    if enchantability(s.id) <= 0 {
        return 0;
    }
    let b = shelves.min(15);
    let k = r.next_int_n(8) + 1 + (b >> 1) + r.next_int_n(b + 1);
    match slot {
        0 => (k / 3).max(1),
        1 => k * 2 / 3 + 1,
        _ => k.max(b * 2),
    }
}

/// `EnchantmentHelper.buildEnchantmentList(rand, stack, level)`: the
/// `(id, level)` list, empty for `null`.
pub fn build_list(r: &mut JavaRandom, s: &Stack, level: i32) -> Vec<(i32, i32)> {
    let mut k = enchantability(s.id);
    if k <= 0 {
        return Vec::new();
    }
    k /= 2;
    let k = 1 + r.next_int_n((k >> 1) + 1) + r.next_int_n((k >> 1) + 1);
    let lv = k + level;
    let f = (r.next_float() + r.next_float() - 1.0f32) * 0.15f32;
    let mut power = (lv as f32 * (1.0f32 + f) + 0.5f32) as i32;
    if power < 1 {
        power = 1;
    }
    // mapEnchantmentData: the highest matching level of each enchantment
    let book = s.id == 340;
    let mut map: Vec<(i32, i32, Ench)> = Vec::new();
    let mut levels: Vec<(i32, i32)> = Vec::new();
    for &(id, weight, e, t) in ENCHANTMENTS.iter() {
        if !(book || can_enchant(t, s.id)) {
            continue;
        }
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

/// `EnchantmentNameParts`: the words of the table's random names.
const WORDS: &str = "the elder scrolls klaatu berata niktu xyzzy bless curse light darkness fire air earth water hot dry cold wet ignite snuff embiggen twist shorten stretch fiddle destroy imbue galvanize enchant free limited range of towards inside sphere cube self other ball mental physical grow shrink demon elemental spirit animal creature beast humanoid undead fresh stale ";

/// `EnchantmentNameParts.generateNewRandomName`
pub fn random_name(r: &mut JavaRandom) -> String {
    // String.split drops the trailing empty part
    let words: Vec<&str> = WORDS.trim_end().split(' ').collect();
    let n = r.next_int_n(2) + 3;
    let mut s = String::new();
    for i in 0..n {
        if i > 0 {
            s.push(' ');
        }
        s.push_str(words[r.next_int_n(words.len() as i32) as usize]);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Against `tools/EnchantHarness.java`.
    #[test]
    fn lists_and_levels_match_the_game() {
        let want = include_str!("../tools/enchant_1710.txt");
        for l in want.lines() {
            let f: Vec<&str> = l.split(' ').collect();
            let id: u16 = f[1].parse().unwrap();
            let s = Stack::new(id, 0, 1);
            match f[0] {
                "L" => {
                    let level: i32 = f[2].parse().unwrap();
                    let seed: i64 = f[3].parse().unwrap();
                    let got = build_list(&mut JavaRandom::new(seed), &s, level);
                    let got: Vec<String> = got.iter().map(|(a, b)| format!("{a}:{b}")).collect();
                    assert_eq!(got.join(" "), f[4..].join(" "), "{l}");
                }
                _ => {
                    let shelves: i32 = f[2].parse().unwrap();
                    let seed: i64 = f[3].parse().unwrap();
                    let mut r = JavaRandom::new(seed);
                    let got: Vec<String> = (0..3)
                        .map(|k| calc_level(&mut r, k, shelves, &s).to_string())
                        .collect();
                    assert_eq!(got.join(" "), f[4..].join(" "), "{l}");
                }
            }
        }
    }
}
