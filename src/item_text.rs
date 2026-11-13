//! `ItemStack.getDisplayName`, `getRarity` and `getTooltip` with the item
//! overrides that do not read NBT (`ItemPotion`, `ItemMonsterPlacer`,
//! `ItemRecord`, `ItemEnchantedBook`, `ItemAppleGold`), and
//! `GuiScreen.renderToolTip`'s colouring.

use crate::item::Stack;
use crate::item_data::{EGGS, ENCHANTMENTS, NAMES};
use crate::lang::Lang;

fn entry(s: &Stack) -> Option<&'static (u16, i32, i8, &'static str, u8, f32)> {
    NAMES
        .iter()
        .find(|e| e.0 == s.id && e.1 == s.meta)
        .or_else(|| NAMES.iter().find(|e| e.0 == s.id))
}

/// `getUnlocalizedName(stack)`
pub fn unlocalized(s: &Stack) -> Option<&'static str> {
    entry(s).map(|e| e.3)
}

/// The creative tab of the item, -1 for none.
pub fn creative_tab(id: crate::item::ItemId) -> i8 {
    NAMES.iter().find(|e| e.0 == id).map_or(-1, |e| e.2)
}

fn tr(lang: &Lang, k: &str) -> String {
    lang.get(k).trim().to_string()
}

/// `Item.getItemStackDisplayName`
pub fn display_name(s: &Stack, lang: &Lang) -> String {
    match s.id {
        373 => return potion_name(s, lang),
        383 => {
            let mut n = tr(lang, "item.monsterPlacer.name");
            if let Some((_, e, _, _)) = EGGS.iter().find(|e| e.0 == s.meta) {
                n = format!("{} {}", n, lang.get(&format!("entity.{e}.name")));
            }
            return n;
        }
        _ => {}
    }
    match unlocalized(s) {
        Some(k) => tr(lang, &format!("{k}.name")),
        None => crate::item::display_name(s),
    }
}

/// `ItemPotion.getItemStackDisplayName`
fn potion_name(s: &Stack, lang: &Lang) -> String {
    if s.meta == 0 {
        return tr(lang, "item.emptyPotion.name");
    }
    let mut pre = String::new();
    if s.meta & 16384 != 0 {
        pre = format!("{} ", tr(lang, "potion.prefix.grenade"));
    }
    let fx = crate::potion::effects_of(s.meta, false);
    if let Some(e) = fx.first() {
        return pre + &tr(lang, &format!("{}.postfix", e.potion().name));
    }
    let p = crate::potion::PREFIXES[crate::potion::prefix_index(s.meta)];
    format!("{} {}", tr(lang, p), tr(lang, "item.potion.name"))
}

/// `EnumRarity` index: common, uncommon, rare, epic.
pub fn rarity(s: &Stack) -> u8 {
    let base = entry(s).map_or(0, |e| e.4);
    match s.id {
        // ItemAppleGold and ItemRecord ignore enchantments
        322 | 2256..=2267 => base,
        // ItemEnchantedBook: stored enchantments make it uncommon
        403 if s.ench != 0 => 1,
        // Item.getRarity
        _ if s.ench != 0 => 2,
        _ => base,
    }
}

/// `EnumRarity.rarityColor`
pub fn rarity_color(r: u8) -> char {
    ['f', 'e', 'b', 'd'][r as usize & 3]
}

/// `ItemStack.DECIMALFORMAT` (`#.###`)
pub fn decimal(v: f64) -> String {
    let s = format!("{:.3}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".into()
    } else {
        s.to_string()
    }
}

/// `StringUtils.ticksToElapsedTime`
pub fn elapsed(ticks: i32) -> String {
    let s = ticks / 20;
    let m = s / 60;
    let s = s % 60;
    if s < 10 {
        format!("{m}:0{s}")
    } else {
        format!("{m}:{s}")
    }
}

/// `Enchantment.getTranslatedName`
pub fn enchantment_name(id: i32, lvl: i32, lang: &Lang) -> String {
    let n = ENCHANTMENTS
        .iter()
        .find(|e| e.0 == id)
        .map_or("enchantment.unknown", |e| e.1);
    format!(
        "{} {}",
        lang.get(n),
        lang.get(&format!("enchantment.level.{lvl}"))
    )
}

const RECORDS: [&str; 12] = [
    "13", "cat", "blocks", "chirp", "far", "mall", "mellohi", "stal", "strad", "ward", "11", "wait",
];

fn attr_name(a: crate::potion::Attr) -> &'static str {
    use crate::potion::Attr;
    match a {
        Attr::MaxHealth => "generic.maxHealth",
        Attr::MovementSpeed => "generic.movementSpeed",
        Attr::AttackDamage => "generic.attackDamage",
    }
}

fn modifier_line(lang: &Lang, op: i32, amount: f64, attr: &str) -> Option<String> {
    let shown = if op == 1 || op == 2 {
        amount * 100.0
    } else {
        amount
    };
    let name = lang.get(&format!("attribute.name.{attr}")).to_string();
    if amount > 0.0 {
        Some(format!(
            "\u{a7}9{}",
            lang.format(
                &format!("attribute.modifier.plus.{op}"),
                &[decimal(shown), name]
            )
        ))
    } else if amount < 0.0 {
        Some(format!(
            "\u{a7}c{}",
            lang.format(
                &format!("attribute.modifier.take.{op}"),
                &[decimal(-shown), name]
            )
        ))
    } else {
        None
    }
}

/// `ItemStack.getTooltip(player, false)`
pub fn tooltip(s: &Stack, lang: &Lang) -> Vec<String> {
    let mut v = Vec::new();
    let mut name = display_name(s, lang);
    if s.id == 358 {
        name = format!("{name} #{}", s.meta);
    }
    v.push(name);
    // Item.addInformation
    match s.id {
        373 if s.meta != 0 => {
            let fx = crate::potion::effects_of(s.meta, false);
            let mut mods = Vec::new();
            if fx.is_empty() {
                v.push(format!("\u{a7}7{}", tr(lang, "potion.empty")));
            }
            for e in &fx {
                let p = e.potion();
                let mut t = tr(lang, p.name);
                if let Some((attr, _, base, op)) = p.modifier {
                    mods.push((attr_name(attr), p.modifier_amount(e.amplifier, base), op));
                }
                if e.amplifier > 0 {
                    t = format!(
                        "{} {}",
                        t,
                        tr(lang, &format!("potion.potency.{}", e.amplifier))
                    );
                }
                if e.duration > 20 {
                    let d = if e.duration_max {
                        "**:**".to_string()
                    } else {
                        elapsed(e.duration)
                    };
                    t = format!("{t} ({d})");
                }
                v.push(format!("\u{a7}{}{t}", if p.bad { 'c' } else { '7' }));
            }
            if !mods.is_empty() {
                v.push(String::new());
                v.push(format!("\u{a7}5{}", lang.get("potion.effects.whenDrank")));
                for (attr, amount, op) in mods {
                    if let Some(l) = modifier_line(lang, op, amount, attr) {
                        v.push(l);
                    }
                }
            }
        }
        2256..=2267 => {
            let r = RECORDS[(s.id - 2256) as usize];
            v.push(format!(
                "\u{a7}7{}",
                lang.get(&format!("item.record.{r}.desc"))
            ));
        }
        _ => {}
    }
    for (id, lvl) in s.enchantments() {
        v.push(enchantment_name(id, lvl, lang));
    }
    // getAttributeModifiers: the weapon damage, with Sharpness
    let atk = entry(s).map_or(0.0, |e| e.5) as f64;
    if atk != 0.0 {
        v.push(String::new());
        let sharp = s
            .enchantments()
            .iter()
            .find(|e| e.0 == 16)
            .map_or(0.0, |e| e.1 as f32 * 1.25);
        if let Some(l) = modifier_line(lang, 0, atk + sharp as f64, "generic.attackDamage") {
            v.push(l);
        }
    }
    v
}

/// `GuiScreen.renderToolTip`: the first line in the rarity colour, the
/// rest grey.
pub fn render_lines(s: &Stack, lang: &Lang) -> Vec<String> {
    let c = rarity_color(rarity(s));
    tooltip(s, lang)
        .into_iter()
        .enumerate()
        .map(|(i, l)| {
            if i == 0 {
                format!("\u{a7}{c}{l}")
            } else {
                format!("\u{a7}7{l}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(decimal(7.0), "7");
        assert_eq!(decimal(8.25), "8.25");
        assert_eq!(decimal(0.15000000596046448), "0.15");
        assert_eq!(elapsed(3600), "3:00");
        assert_eq!(elapsed(900), "0:45");
    }

    /// Against `getDisplayName` of every sub-item with the game's en_US.
    #[test]
    fn english_names_match_the_game() {
        let Ok(jar) = std::env::var("MC1710_JAR") else {
            return;
        };
        let Ok(f) = std::fs::File::open(&jar) else {
            return;
        };
        let Ok(mut j) = zip::ZipArchive::new(f) else {
            return;
        };
        let lang = Lang::load(&mut j);
        assert_eq!(lang.get("tile.stone.name"), "Stone");
        let want = include_str!("../tools/item_names_en.txt");
        let mut bad = Vec::new();
        for l in want.lines() {
            let mut f = l.splitn(3, ' ');
            let id: u16 = f.next().unwrap().parse().unwrap();
            let meta: i32 = f.next().unwrap().parse().unwrap();
            let name = f.next().unwrap_or("");
            let got = display_name(&Stack::new(id, meta, 1), &lang);
            if got != name {
                bad.push(format!("{id}:{meta} {got:?} != {name:?}"));
            }
        }
        eprintln!("checked {} names", want.lines().count());
        assert!(bad.is_empty(), "{}", bad.join("\n"));
    }
}
