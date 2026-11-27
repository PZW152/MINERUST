//! `MerchantRecipe`, `MerchantRecipeList` and the villagers' trades
//! (`EntityVillager.addDefaultEquipmentAndRecipies`).

use crate::item::Stack;
use crate::java::JavaRandom;
use crate::world::anvil::Nbt;

const EMERALD: u16 = 388;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Recipe {
    pub buy: Stack,
    pub buy_b: Option<Stack>,
    pub sell: Stack,
    /// `toolUses`, `maxTradeUses`
    pub uses: i32,
    pub max_uses: i32,
}

impl Recipe {
    pub fn new(buy: Stack, buy_b: Option<Stack>, sell: Stack) -> Recipe {
        Recipe {
            buy,
            buy_b,
            sell,
            uses: 0,
            max_uses: 7,
        }
    }

    /// `hasSameIDsAs`
    pub fn same_ids(&self, o: &Recipe) -> bool {
        self.buy.id == o.buy.id
            && self.sell.id == o.sell.id
            && match (self.buy_b, o.buy_b) {
                (None, None) => true,
                (Some(a), Some(b)) => a.id == b.id,
                _ => false,
            }
    }

    /// `hasSameItemsAs`: the same items, this one cheaper
    pub fn same_items(&self, o: &Recipe) -> bool {
        self.same_ids(o)
            && (self.buy.count < o.buy.count
                || matches!((self.buy_b, o.buy_b), (Some(a), Some(b)) if a.count < b.count))
    }

    /// `isRecipeDisabled`
    pub fn disabled(&self) -> bool {
        self.uses >= self.max_uses
    }

    /// `writeToTags`
    pub fn to_nbt(&self) -> Nbt {
        use crate::world::anvil::item_nbt;
        let mut v = vec![
            ("buy".to_string(), item_nbt(None, &self.buy)),
            ("sell".to_string(), item_nbt(None, &self.sell)),
        ];
        if let Some(b) = &self.buy_b {
            v.push(("buyB".to_string(), item_nbt(None, b)));
        }
        v.push(("uses".to_string(), Nbt::Int(self.uses)));
        v.push(("maxUses".to_string(), Nbt::Int(self.max_uses)));
        Nbt::Compound(v)
    }

    /// `readFromTags`
    pub fn from_nbt(t: &Nbt) -> Option<Recipe> {
        use crate::world::anvil::item_from_nbt;
        let buy = item_from_nbt(t.get("buy")?)?;
        let sell = item_from_nbt(t.get("sell")?)?;
        let buy_b = t.get("buyB").and_then(item_from_nbt);
        Some(Recipe {
            buy,
            buy_b,
            sell,
            uses: t.int("uses").unwrap_or(0) as i32,
            max_uses: t.int("maxUses").map_or(7, |v| v as i32),
        })
    }
}

/// `MerchantRecipeList.getRecipiesAsTags`
pub fn list_to_nbt(list: &[Recipe]) -> Nbt {
    Nbt::Compound(vec![(
        "Recipes".to_string(),
        Nbt::List(10, list.iter().map(Recipe::to_nbt).collect()),
    )])
}

/// `MerchantRecipeList(NBTTagCompound)`
pub fn list_from_nbt(t: &Nbt) -> Vec<Recipe> {
    t.list("Recipes")
        .iter()
        .filter_map(Recipe::from_nbt)
        .collect()
}

/// `MerchantRecipeList.canRecipeBeUsed`: the selected recipe (index > 0)
/// or the first whose prices the two stacks meet.
pub fn can_recipe_be_used(
    list: &[Recipe],
    a: Stack,
    b: Option<Stack>,
    index: usize,
) -> Option<usize> {
    if index > 0 && index < list.len() {
        let r = &list[index];
        let refused = a.id != r.buy.id
            || (b.is_some() || r.buy_b.is_some())
                && (r.buy_b.is_none() || b.is_none() || r.buy_b.unwrap().id != b.unwrap().id)
            || a.count < r.buy.count
            || r.buy_b
                .is_some_and(|rb| b.is_some_and(|b| b.count < rb.count));
        return (!refused).then_some(index);
    }
    list.iter().position(|r| {
        a.id == r.buy.id
            && a.count >= r.buy.count
            && match (r.buy_b, b) {
                (None, None) => true,
                (Some(rb), Some(b)) => rb.id == b.id && b.count >= rb.count,
                _ => false,
            }
    })
}

/// `MerchantRecipeList.addToListWithCheck`
pub fn add_with_check(list: &mut Vec<Recipe>, r: Recipe) {
    for i in 0..list.len() {
        if r.same_ids(&list[i]) {
            if r.same_items(&list[i]) {
                list[i] = r;
            }
            return;
        }
    }
    list.push(r);
}

/// `villagersSellingList`: how many of an item a villager buys.
fn buy_range(id: u16) -> Option<(i32, i32)> {
    Some(match id {
        263 => (16, 24),
        265 | 266 => (8, 10),
        264 => (4, 6),
        339 => (24, 36),
        340 => (11, 13),
        387 => (1, 1),
        368 => (3, 4),
        381 => (2, 3),
        319 | 363 | 365 => (14, 18),
        350 => (9, 13),
        295 => (34, 48),
        362 | 361 => (30, 38),
        296 => (18, 22),
        35 => (14, 22),
        367 => (36, 64),
        _ => return None,
    })
}

/// `blacksmithSellingList`: emeralds asked, or (negative) items per emerald.
fn sell_range(id: u16) -> Option<(i32, i32)> {
    Some(match id {
        259 | 359 => (3, 4),
        267 => (7, 11),
        276 => (12, 14),
        258 => (6, 8),
        279 => (9, 12),
        257 => (7, 9),
        278 => (10, 12),
        256 => (4, 6),
        277 => (7, 8),
        292 => (4, 6),
        293 => (7, 8),
        309 => (4, 6),
        313 => (7, 8),
        306 => (4, 6),
        310 => (7, 8),
        307 => (10, 14),
        311 => (16, 19),
        308 => (8, 10),
        312 => (11, 14),
        305 | 302 => (5, 7),
        303 => (11, 15),
        304 => (9, 11),
        297 => (-4, -2),
        360 | 260 => (-8, -4),
        357 => (-10, -7),
        20 => (-5, -3),
        47 => (3, 4),
        299 => (4, 5),
        301 | 298 | 300 => (2, 4),
        329 => (6, 8),
        384 | 331 => (-4, -1),
        345 | 347 => (10, 12),
        89 => (-3, -1),
        320 | 364 => (-7, -5),
        366 => (-8, -6),
        381 => (7, 11),
        262 => (-12, -8),
        _ => return None,
    })
}

fn amount(range: Option<(i32, i32)>, r: &mut JavaRandom) -> i32 {
    match range {
        None => 1,
        Some((a, b)) if a >= b => a,
        Some((a, b)) => a + r.next_int_n(b - a),
    }
}

/// `makeBuyTradeOffer`: emeralds for some of the item.
fn buy(v: &mut Vec<Recipe>, id: u16, r: &mut JavaRandom, p: f32) {
    if r.next_float() < p {
        let n = amount(buy_range(id), r);
        v.push(Recipe::new(
            Stack::new(id, 0, n as u32),
            None,
            Stack::new(EMERALD, 0, 1),
        ));
    }
}

/// `makeSellTradeOffer`: the item for emeralds.
fn sell(v: &mut Vec<Recipe>, id: u16, r: &mut JavaRandom, p: f32) {
    if r.next_float() < p {
        let n = amount(sell_range(id), r);
        let (price, item) = if n < 0 {
            (Stack::new(EMERALD, 0, 1), Stack::new(id, 0, (-n) as u32))
        } else {
            (Stack::new(EMERALD, 0, n as u32), Stack::new(id, 0, 1))
        };
        v.push(Recipe::new(price, None, item));
    }
}

/// `Collections.shuffle(list)` with its own random.
fn shuffle(v: &mut [Recipe], r: &mut JavaRandom) {
    for i in (1..v.len()).rev() {
        let j = r.next_int_n(i as i32 + 1) as usize;
        v.swap(i, j);
    }
}

/// `EntityVillager.addDefaultEquipmentAndRecipies(n)`: candidate offers for
/// the profession, shuffled, and the first `n` added to the list. `r` is
/// the villager's random, `shuffle_rand` `Collections`' own.
pub fn add_default(
    list: &mut Option<Vec<Recipe>>,
    profession: i32,
    n: usize,
    r: &mut JavaRandom,
    shuffle_rand: &mut JavaRandom,
) {
    let size = if let Some(l) = list {
        (l.len() as f32).sqrt() * 0.2
    } else {
        0.0
    };
    // adjustProbability
    let adj = |p: f32| {
        let v = p + size;
        if v > 0.9 {
            0.9 - (v - 0.9)
        } else {
            v
        }
    };
    let mut v: Vec<Recipe> = Vec::new();
    match profession {
        0 => {
            buy(&mut v, 296, r, adj(0.9));
            buy(&mut v, 35, r, adj(0.5));
            buy(&mut v, 365, r, adj(0.5));
            buy(&mut v, 350, r, adj(0.4));
            sell(&mut v, 297, r, adj(0.9));
            sell(&mut v, 360, r, adj(0.3));
            sell(&mut v, 260, r, adj(0.3));
            sell(&mut v, 357, r, adj(0.3));
            sell(&mut v, 359, r, adj(0.3));
            sell(&mut v, 259, r, adj(0.3));
            sell(&mut v, 366, r, adj(0.3));
            sell(&mut v, 262, r, adj(0.5));
            if r.next_float() < adj(0.5) {
                let n = 4 + r.next_int_n(2);
                v.push(Recipe::new(
                    Stack::new(13, 0, 10),
                    Some(Stack::new(EMERALD, 0, 1)),
                    Stack::new(318, 0, n as u32),
                ));
            }
        }
        1 => {
            buy(&mut v, 339, r, adj(0.8));
            buy(&mut v, 340, r, adj(0.8));
            buy(&mut v, 387, r, adj(0.3));
            sell(&mut v, 47, r, adj(0.8));
            sell(&mut v, 20, r, adj(0.2));
            sell(&mut v, 345, r, adj(0.2));
            sell(&mut v, 347, r, adj(0.2));
            if r.next_float() < adj(0.07) {
                // Enchantment.enchantmentsBookList: every enchantment
                let ids = crate::enchant::all_ids();
                let id = ids[r.next_int_n(ids.len() as i32) as usize];
                let (lo, hi) = (1, crate::enchant::max_level_of(id));
                // MathHelper.getRandomIntegerInRange
                let lvl = if lo >= hi {
                    lo
                } else {
                    r.next_int_n(hi - lo + 1) + lo
                };
                let book = Stack::new(403, 0, 1).with_enchantments(&[(id, lvl)]);
                let price = 2 + r.next_int_n(5 + lvl * 10) + 3 * lvl;
                v.push(Recipe::new(
                    Stack::new(340, 0, 1),
                    Some(Stack::new(EMERALD, 0, price as u32)),
                    book,
                ));
            }
        }
        2 => {
            sell(&mut v, 381, r, adj(0.3));
            sell(&mut v, 384, r, adj(0.2));
            sell(&mut v, 331, r, adj(0.4));
            sell(&mut v, 89, r, adj(0.3));
            for id in [267u16, 276, 307, 311, 258, 279, 257, 278] {
                if r.next_float() < adj(0.05) {
                    let price = 2 + r.next_int_n(3);
                    let level = 5 + r.next_int_n(15);
                    // EnchantmentHelper.addRandomEnchantment
                    let base = Stack::new(id, 0, 1);
                    let list = crate::enchant::build_list(r, &base, level);
                    let item = base.with_enchantments(&list);
                    v.push(Recipe::new(
                        Stack::new(id, 0, 1),
                        Some(Stack::new(EMERALD, 0, price as u32)),
                        item,
                    ));
                }
            }
        }
        3 => {
            buy(&mut v, 263, r, adj(0.7));
            buy(&mut v, 265, r, adj(0.5));
            buy(&mut v, 266, r, adj(0.5));
            buy(&mut v, 264, r, adj(0.5));
            for (id, p) in [
                (267u16, 0.5f32),
                (276, 0.5),
                (258, 0.3),
                (279, 0.3),
                (257, 0.5),
                (278, 0.5),
                (256, 0.2),
                (277, 0.2),
                (292, 0.2),
                (293, 0.2),
                (309, 0.2),
                (313, 0.2),
                (306, 0.2),
                (310, 0.2),
                (307, 0.2),
                (311, 0.2),
                (308, 0.2),
                (312, 0.2),
                (305, 0.1),
                (302, 0.1),
                (303, 0.1),
                (304, 0.1),
            ] {
                sell(&mut v, id, r, adj(p));
            }
        }
        4 => {
            buy(&mut v, 263, r, adj(0.7));
            buy(&mut v, 319, r, adj(0.5));
            buy(&mut v, 363, r, adj(0.5));
            sell(&mut v, 329, r, adj(0.1));
            sell(&mut v, 299, r, adj(0.3));
            sell(&mut v, 301, r, adj(0.3));
            sell(&mut v, 298, r, adj(0.3));
            sell(&mut v, 300, r, adj(0.3));
            sell(&mut v, 320, r, adj(0.3));
            sell(&mut v, 364, r, adj(0.3));
        }
        _ => {}
    }
    if v.is_empty() {
        buy(&mut v, 266, r, 1.0);
    }
    shuffle(&mut v, shuffle_rand);
    let l = list.get_or_insert_with(Vec::new);
    for rec in v.into_iter().take(n) {
        add_with_check(l, rec);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipes_match_and_save() {
        let mut r = JavaRandom::new(1);
        let mut s = JavaRandom::new(2);
        let mut list = None;
        for _ in 0..5 {
            add_default(&mut list, 0, 1, &mut r, &mut s);
        }
        let list = list.unwrap();
        assert!(!list.is_empty());
        let back = list_from_nbt(&list_to_nbt(&list));
        assert_eq!(back, list);
        // wheat for an emerald
        let w = vec![Recipe::new(
            Stack::new(296, 0, 20),
            None,
            Stack::new(EMERALD, 0, 1),
        )];
        assert_eq!(
            can_recipe_be_used(&w, Stack::new(296, 0, 25), None, 0),
            Some(0)
        );
        assert_eq!(
            can_recipe_be_used(&w, Stack::new(296, 0, 19), None, 0),
            None
        );
        assert_eq!(
            can_recipe_be_used(&w, Stack::new(296, 0, 25), Some(Stack::new(1, 0, 1)), 0),
            None
        );
    }
}
