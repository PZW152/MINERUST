//! `ContainerRepair.updateRepairOutput`: what the anvil makes of its two
//! inputs and the typed name, and what it costs.

use crate::enchant;
use crate::item::{self, Stack};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Repair {
    /// The output slot.
    pub out: Option<Stack>,
    /// `maximumCost`
    pub cost: i32,
    /// `stackSizeToBeUsedInRepair`
    pub use_count: u32,
}

fn damageable(s: &Stack) -> bool {
    item::max_damage(s.id) > 0
}

/// `Item.getIsRepairable`: tools, swords and armour with their material.
fn repairable(a: &Stack, b: &Stack) -> bool {
    if let Some(t) = item::tool(a.id) {
        if t.kind == item::ToolKind::Hoe {
            return false;
        }
        // ToolMaterial.getBaseItemForRepair
        let m = match t.max_uses {
            59 => 5,
            131 => 4,
            250 => 265,
            1561 => 264,
            _ => 266,
        };
        return b.id == m;
    }
    if (298..=317).contains(&a.id) {
        // ArmorMaterial.getBaseItemForRepair: leather, chain, iron, diamond, gold
        let m = [334, 265, 265, 264, 266][(a.id as usize - 298) / 4];
        return b.id == m;
    }
    false
}

/// The cost per level of an enchantment by its weight.
fn weight_cost(id: i32, book: bool) -> i32 {
    let c = match enchant::weight(id) {
        1 => 8,
        2 => 4,
        5 => 2,
        10 => 1,
        _ => 0,
    };
    if book {
        (c / 2).max(1)
    } else {
        c
    }
}

/// `EnchantmentHelper.setEnchantments`
fn set_enchantments(map: &[(i32, i32)], s: Stack) -> Stack {
    if s.id == 403 || !map.is_empty() {
        return s.with_enchantments(map);
    }
    // removeTag("ench"): the compound itself stays
    if s.tag.is_none() {
        return s;
    }
    s.edit_tag(|t| {
        if let crate::world::anvil::Nbt::Compound(v) = t {
            v.retain(|e| e.0 != "ench");
        }
    })
}

/// `StringUtils.isBlank`
fn blank(s: &str) -> bool {
    s.chars().all(char::is_whitespace)
}

/// `ContainerRepair.updateRepairOutput`. `name_of` is `getDisplayName`
/// as the server reads it.
pub fn update(
    in0: Option<Stack>,
    in1: Option<Stack>,
    name: &str,
    creative: bool,
    name_of: &dyn Fn(&Stack) -> String,
) -> Repair {
    let Some(first) = in0 else {
        return Repair::default();
    };
    let mut out = first;
    let mut map = out.enchantments();
    let (mut cost, mut base, mut rename) = (0i32, 0i32, 0i32);
    base += first.repair_cost() + in1.map_or(0, |s| s.repair_cost());
    let mut use_count = 0;
    let mut book = false;
    if let Some(second) = in1 {
        book = second.id == 403 && !second.enchantments().is_empty();
        if damageable(&out) && repairable(&first, &second) {
            let md = item::max_damage(out.id);
            let mut k = out.meta.min(md / 4);
            if k <= 0 {
                return Repair::default();
            }
            let mut n = 0;
            while k > 0 && n < second.count {
                out.meta -= k;
                cost += (k / 100).max(1) + map.len() as i32;
                k = out.meta.min(md / 4);
                n += 1;
            }
            use_count = n;
        } else {
            if !book && (out.id != second.id || !damageable(&out)) {
                return Repair::default();
            }
            if damageable(&out) && !book {
                let md = item::max_damage(out.id);
                let a = md - first.meta;
                let b = item::max_damage(second.id) - second.meta;
                let c = b + md * 12 / 100;
                let d = (md - (a + c)).max(0);
                if d < out.meta {
                    out.meta = d;
                    cost += (c / 100).max(1);
                }
            }
            for (k, lvl) in second.enchantments() {
                let old = map.iter().find(|e| e.0 == k).map_or(0, |e| e.1);
                let mut level = if old == lvl { lvl + 1 } else { lvl.max(old) };
                let gain = level - old;
                let mut ok = enchant::can_apply(k, &first);
                if creative || first.id == 403 {
                    ok = true;
                }
                for &(other, _) in &map {
                    if other != k && !enchant::can_apply_together(k, other) {
                        ok = false;
                        cost += gain;
                    }
                }
                if ok {
                    level = level.min(enchant::max_level_of(k));
                    match map.iter_mut().find(|e| e.0 == k) {
                        Some(e) => e.1 = level,
                        None => map.push((k, level)),
                    }
                    cost += weight_cost(k, book) * gain;
                }
            }
        }
    }
    // the rename; the price is a byte in the original
    let rename_cost = |s: &Stack| (if damageable(s) { 7 } else { s.count as i32 * 5 }) as i8 as i32;
    if blank(name) {
        if first.custom_name().is_some() {
            rename = rename_cost(&first);
            cost += rename;
            out = out.with_name(None);
        }
    } else if name != name_of(&first) {
        rename = rename_cost(&first);
        cost += rename;
        if first.custom_name().is_some() {
            base += rename / 2;
        }
        out = out.with_name(Some(name));
    }
    for (i, &(k, lvl)) in map.iter().enumerate() {
        base += (i as i32 + 1) + lvl * weight_cost(k, book);
    }
    if book {
        base = (base / 2).max(1);
    }
    let mut total = base + cost;
    let mut result = Some(out);
    if cost <= 0 {
        result = None;
    }
    if rename == cost && rename > 0 && total >= 40 {
        total = 39;
    }
    if total >= 40 && !creative {
        result = None;
    }
    if let Some(o) = result {
        let mut rc = o.repair_cost();
        if let Some(s) = in1 {
            if rc < s.repair_cost() {
                rc = s.repair_cost();
            }
        }
        if o.custom_name().is_some() {
            rc -= 9;
        }
        rc = rc.max(0) + 2;
        result = Some(set_enchantments(&map, o.with_repair_cost(rc)));
    }
    Repair {
        out: result,
        cost: total,
        use_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack(s: &str) -> Option<Stack> {
        if s == "-" {
            return None;
        }
        let f: Vec<&str> = s.split(':').collect();
        let mut st = Stack::new(
            f[0].parse().unwrap(),
            f[1].parse().unwrap(),
            f[2].parse().unwrap(),
        );
        for x in &f[3..] {
            if let Some(v) = x.strip_prefix("rc=") {
                st = st.with_repair_cost(v.parse().unwrap());
            } else if let Some(v) = x.strip_prefix("name=") {
                st = st.with_name(Some(v));
            } else if let Some(v) = x.strip_prefix("ench=") {
                let mut l = st.enchantments();
                for e in v.split('+') {
                    let (a, b) = e.split_once('.').unwrap();
                    l.push((a.parse().unwrap(), b.parse().unwrap()));
                }
                st = st.with_enchantments(&l);
            }
        }
        Some(st)
    }

    #[test]
    fn anvil_matches_the_game() {
        let name_of = |s: &Stack| s.custom_name().unwrap_or_else(|| item::display_name(s));
        let mut n = 0;
        for line in include_str!("../tools/repair_1710.txt").lines() {
            let (case, want) = line.split_once(" => ").unwrap();
            let f: Vec<&str> = case.split('|').collect();
            let r = update(stack(f[0]), stack(f[1]), f[2], f[3] == "1", &name_of);
            let mut got = format!("{}", r.cost);
            match r.out {
                None => got += " none",
                Some(o) => {
                    let name = o
                        .custom_name()
                        .map_or("-".to_string(), |s| s.replace(' ', "_"));
                    got += &format!(
                        " {} {} {} {} {} {}",
                        r.use_count,
                        o.id,
                        o.meta,
                        o.count,
                        o.repair_cost(),
                        name
                    );
                    for (k, l) in o.enchantments() {
                        got += &format!(" {k}:{l}");
                    }
                }
            }
            assert_eq!(got, want, "{case}");
            n += 1;
        }
        assert_eq!(n, 25);
    }
}
