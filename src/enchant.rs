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

fn by_id(id: i32) -> Option<&'static (i32, i32, Ench, Type)> {
    ENCHANTMENTS.iter().find(|e| e.0 == id)
}

/// `Enchantment.getWeight`
pub fn weight(id: i32) -> i32 {
    by_id(id).map_or(0, |e| e.1)
}

/// `Enchantment.getMaxLevel`
pub fn max_level_of(id: i32) -> i32 {
    by_id(id).map_or(1, |e| max_level(e.2))
}

/// `Enchantment.canApplyTogether`
pub fn can_apply_together(a: i32, b: i32) -> bool {
    match (by_id(a), by_id(b)) {
        (Some(x), Some(y)) => compatible((x.0, x.2), (y.0, y.2)),
        _ => a != b,
    }
}

/// `Enchantment.canApply(stack)` with its overrides (axes take the damage
/// enchantments, shears efficiency and silk touch, anything damageable
/// unbreaking, any armour thorns).
pub fn can_apply(id: i32, s: &Stack) -> bool {
    let Some(e) = by_id(id) else { return false };
    let axe = crate::item::tool(s.id).is_some_and(|t| t.kind == crate::item::ToolKind::Axe);
    let special = match e.2 {
        Ench::Damage(_) => axe,
        Ench::Digging | Ench::Untouching => s.id == 359,
        Ench::Durability => crate::item::max_damage(s.id) > 0,
        Ench::Thorns => crate::container::armor_type(s.id).is_some(),
        _ => false,
    };
    special || can_enchant(e.3, s.id)
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

// ------------------------------------------------------------ effects

pub const PROTECTION: i32 = 0;
pub const FIRE_PROTECTION: i32 = 1;
pub const FEATHER_FALLING: i32 = 2;
pub const BLAST_PROTECTION: i32 = 3;
pub const PROJECTILE_PROTECTION: i32 = 4;
pub const RESPIRATION: i32 = 5;
pub const AQUA_AFFINITY: i32 = 6;
pub const THORNS: i32 = 7;
pub const SHARPNESS: i32 = 16;
pub const SMITE: i32 = 17;
pub const BANE_OF_ARTHROPODS: i32 = 18;
pub const KNOCKBACK: i32 = 19;
pub const FIRE_ASPECT: i32 = 20;
pub const LOOTING: i32 = 21;
pub const EFFICIENCY: i32 = 32;
pub const SILK_TOUCH: i32 = 33;
pub const UNBREAKING: i32 = 34;
pub const FORTUNE: i32 = 35;
pub const POWER: i32 = 48;
pub const PUNCH: i32 = 49;
pub const FLAME: i32 = 50;
pub const INFINITY: i32 = 51;
pub const LUCK_OF_THE_SEA: i32 = 61;
pub const LURE: i32 = 62;

/// `EnumCreatureAttribute`
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Creature {
    Undefined,
    Undead,
    Arthropod,
}

/// `ItemStack.getEnchantmentTagList` as `(id, lvl)` pairs read with
/// `getShort`: the `ench` list only (an enchanted book's
/// `StoredEnchantments` do nothing).
fn tag_list(s: &Stack) -> Vec<(i32, i32)> {
    let Some(t) = s.nbt() else {
        return Vec::new();
    };
    t.list("ench")
        .iter()
        .map(|e| {
            (
                e.int("id").unwrap_or(0) as i16 as i32,
                e.int("lvl").unwrap_or(0) as i16 as i32,
            )
        })
        .collect()
}

/// `EnchantmentHelper.getEnchantmentLevel`: the first entry with this id.
pub fn level(id: i32, s: Option<&Stack>) -> i32 {
    let Some(s) = s else { return 0 };
    tag_list(s)
        .into_iter()
        .find(|e| e.0 == id)
        .map_or(0, |e| e.1)
}

/// `EnchantmentHelper.getMaxEnchantmentLevel` over worn items.
pub fn max_level_in(id: i32, items: &[Option<Stack>]) -> i32 {
    items
        .iter()
        .map(|s| level(id, s.as_ref()))
        .fold(0, |a, l| if l > a { l } else { a })
}

/// The enchantments that exist (`Enchantment.enchantmentsList[id] != null`)
/// in the stack's list order, as `applyEnchantmentModifier` visits them.
fn known(s: &Stack) -> impl Iterator<Item = (Ench, i32)> {
    tag_list(s)
        .into_iter()
        .filter_map(|(id, l)| by_id(id).map(|e| (e.2, l)))
}

static ENCHANTMENT_RAND: std::sync::Mutex<Option<JavaRandom>> = std::sync::Mutex::new(None);

/// `EnchantmentHelper.enchantmentRand`: a static `new Random()` of its own.
fn enchantment_rand<T>(f: impl FnOnce(&mut JavaRandom) -> T) -> T {
    let mut g = ENCHANTMENT_RAND.lock().unwrap();
    let r = g.get_or_insert_with(|| {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as i64);
        JavaRandom::new(t.wrapping_mul(181783497276652981) ^ 0x5DEECE66D)
    });
    f(r)
}

/// `Enchantment.calcModifierDamage` (only `EnchantmentProtection`
/// overrides it).
fn calc_modifier_damage(e: Ench, l: i32, src: &crate::entity::living::Source) -> i32 {
    use crate::entity::living::SourceKind;
    let Ench::Protection(t) = e else { return 0 };
    // DamageSource.canHarmInCreative: only outOfWorld
    if src.kind == SourceKind::OutOfWorld {
        return 0;
    }
    let f = (6 + l * l) as f32 / 3.0;
    let floor = |x: f32| crate::util::math::floor_float(x);
    if t == 0 {
        floor(f * 0.75)
    } else if t == 1 && src.fire {
        floor(f * 1.25)
    } else if t == 2 && src.kind == SourceKind::Fall {
        floor(f * 2.5)
    } else if t == 3 && src.explosion {
        floor(f * 1.5)
    } else if t == 4 && src.projectile {
        floor(f * 1.5)
    } else {
        0
    }
}

/// `EnchantmentHelper.getEnchantmentModifierDamage(getLastActiveItems(), source)`:
/// the protection total capped at 25, then halved with a random part.
pub fn modifier_damage(items: &[Option<Stack>], src: &crate::entity::living::Source) -> i32 {
    let mut m = 0;
    for s in items.iter().flatten() {
        for (e, l) in known(s) {
            m += calc_modifier_damage(e, l, src);
        }
    }
    if m > 25 {
        m = 25;
    }
    (m + 1 >> 1) + enchantment_rand(|r| r.next_int_n((m >> 1) + 1))
}

/// `EnchantmentHelper.getModifierForCreature`: the extra melee damage of
/// Sharpness, Smite and Bane of Arthropods (`calcDamageByCreature`).
pub fn modifier_living(s: Option<&Stack>, c: Creature) -> f32 {
    let mut m = 0.0f32;
    let Some(s) = s else { return m };
    for (e, l) in known(s) {
        m += match e {
            Ench::Damage(0) => l as f32 * 1.25,
            Ench::Damage(1) if c == Creature::Undead => l as f32 * 2.5,
            Ench::Damage(2) if c == Creature::Arthropod => l as f32 * 2.5,
            _ => 0.0,
        };
    }
    m
}

/// The `onEntityDamaged` calls of `applyArthropodEnchantments` with the
/// attacker's held item: Bane of Arthropods slows an arthropod down for
/// `20 + rand.nextInt(10 * level)` ticks at amplifier 3. Returns the
/// durations, one per enchantment that applies.
pub fn arthropod_slowness(s: Option<&Stack>, c: Creature, r: &mut JavaRandom) -> Vec<i32> {
    let mut v = Vec::new();
    let Some(s) = s else { return v };
    for (e, l) in known(s) {
        if e == Ench::Damage(2) && c == Creature::Arthropod {
            v.push(20 + r.next_int_n(10 * l));
        }
    }
    v
}

/// `EnchantmentThorns.shouldHit`
pub fn thorns_should_hit(level: i32, r: &mut JavaRandom) -> bool {
    if level <= 0 {
        false
    } else {
        r.next_float() < 0.15 * level as f32
    }
}

/// `EnchantmentThorns.getDamage`
pub fn thorns_damage(level: i32, r: &mut JavaRandom) -> i32 {
    if level > 10 {
        level - 10
    } else {
        1 + r.next_int_n(4)
    }
}

/// `EnchantmentDurability.negateDamage`
pub fn negate_damage(s: &Stack, level: i32, r: &mut JavaRandom) -> bool {
    if crate::container::armor_type(s.id).is_some() && r.next_float() < 0.6 {
        false
    } else {
        r.next_int_n(level + 1) > 0
    }
}

/// `ItemStack.attemptDamageItem`: wears the stack by `n` uses less what
/// Unbreaking saves; true when it broke (its damage passed the maximum).
pub fn attempt_damage_item(s: &mut Stack, n: i32, r: &mut JavaRandom) -> bool {
    // isItemStackDamageable
    let max = crate::item::max_damage(s.id);
    if max <= 0 {
        return false;
    }
    let mut n = n;
    if n > 0 {
        let u = level(UNBREAKING, Some(s));
        let mut saved = 0;
        let mut i = 0;
        while u > 0 && i < n {
            if negate_damage(s, u, r) {
                saved += 1;
            }
            i += 1;
        }
        n -= saved;
        if n <= 0 {
            return false;
        }
    }
    s.meta += n;
    s.meta > max
}

/// `ItemStack.damageItem` for a player (creative mode does nothing): the
/// stack afterwards, `None` when it broke (`renderBrokenItemStack`,
/// `stackSize--`).
pub fn damage_item(s: Stack, n: i32, creative: bool, r: &mut JavaRandom) -> (Option<Stack>, bool) {
    if creative {
        return (Some(s), false);
    }
    let mut s = s;
    if attempt_damage_item(&mut s, n, r) {
        if s.count <= 1 {
            return (None, true);
        }
        s.count -= 1;
        s.meta = 0;
        return (Some(s), true);
    }
    (Some(s), false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Against `tools/EnchantFxHarness.java`: Protection by source, the
    /// creature damage, Unbreaking and Thorns with seeded randoms.
    #[test]
    fn effects_match_the_game() {
        use crate::entity::living::{Source, SourceKind};
        let src = |n: &str| match n {
            "generic" => Source::named(SourceKind::Generic),
            "inFire" => Source::named(SourceKind::InFire),
            "onFire" => Source::named(SourceKind::OnFire),
            "lava" => Source::named(SourceKind::Lava),
            "fall" => Source::named(SourceKind::Fall),
            "outOfWorld" => Source::named(SourceKind::OutOfWorld),
            "magic" => Source::named(SourceKind::Magic),
            "explosion" => Source::explosion(None),
            "arrow" => Source::arrow(None),
            "fireball" => Source::fireball(None),
            _ => panic!("{n}"),
        };
        let mut n = [0; 4];
        for l in include_str!("../tools/enchant_fx_1710.txt").lines() {
            let f: Vec<&str> = l.split(' ').collect();
            let int = |i: usize| f[i].parse::<i32>().unwrap();
            match f[0] {
                "p" => {
                    let e = by_id(int(1)).unwrap().2;
                    let got = calc_modifier_damage(e, int(2), &src(f[3]));
                    assert_eq!(got, int(4), "{l}");
                    n[0] += 1;
                }
                "c" => {
                    let c = [Creature::Undefined, Creature::Undead, Creature::Arthropod]
                        [int(3) as usize];
                    let s = Stack::new(276, 0, 1).with_enchantments(&[(int(1), int(2))]);
                    let want: f32 = f[4].parse().unwrap();
                    assert_eq!(modifier_living(Some(&s), c), want, "{l}");
                    n[1] += 1;
                }
                "u" => {
                    let id: u16 = f[1].parse().unwrap();
                    let max = crate::item::max_damage(id);
                    let mut s = Stack::new(id, max - 2, 1);
                    if int(2) > 0 {
                        s = s.with_enchantments(&[(UNBREAKING, int(2))]);
                    }
                    let mut r = JavaRandom::new(int(4) as i64);
                    let broke = attempt_damage_item(&mut s, int(3), &mut r);
                    let got = format!("{} {} |{}", s.meta, broke, r.next_int());
                    assert_eq!(got, f[5..].join(" "), "{l}");
                    n[2] += 1;
                }
                "t" => {
                    let mut r = JavaRandom::new(int(2) as i64);
                    let hit = thorns_should_hit(int(1), &mut r);
                    let d = thorns_damage(int(1), &mut r);
                    let got = format!("{} {} |{}", hit, d, r.next_int());
                    assert_eq!(got, f[3..].join(" "), "{l}");
                    n[3] += 1;
                }
                _ => {}
            }
        }
        assert_eq!(n, [300, 54, 576, 80]);
    }

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
