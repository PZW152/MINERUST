//! `InventoryPlayer`: 36 main slots (0..9 the hotbar), four armour slots,
//! the stack on the mouse cursor, plus the crafting grid of the open
//! `ContainerPlayer` / `ContainerWorkbench`. Clicks are `container.rs`.

use crate::item::{self, max_stack, Stack};

pub const MAIN: usize = 36;

#[derive(Clone, Debug, PartialEq)]
pub struct Inventory {
    pub main: [Option<Stack>; MAIN],
    /// `armorInventory`: boots, leggings, chestplate, helmet.
    pub armor: [Option<Stack>; 4],
    /// `InventoryCrafting`, row-major with `grid_size` columns.
    pub grid: [Option<Stack>; 9],
    pub cursor: Option<Stack>,
    pub selected: usize,
    /// 3 at a crafting table, 2 in the player inventory.
    pub grid_size: usize,
}

impl Default for Inventory {
    fn default() -> Self {
        Inventory {
            main: [None; MAIN],
            armor: [None; 4],
            grid: [None; 9],
            cursor: None,
            selected: 0,
            grid_size: 2,
        }
    }
}

/// `ItemStack.isItemDamaged`
fn damaged(s: &Stack) -> bool {
    item::max_damage(s.id) > 0 && s.meta > 0
}

impl Inventory {
    pub fn held(&self) -> Option<Stack> {
        self.main[self.selected]
    }

    /// `getFirstEmptyStack`
    pub fn first_empty(&self) -> i32 {
        self.main
            .iter()
            .position(|s| s.is_none())
            .map_or(-1, |i| i as i32)
    }

    /// `storeItemStack`: the first stack `s` can be added to.
    fn store_item_stack(&self, s: &Stack) -> Option<usize> {
        self.main.iter().position(|t| {
            t.is_some_and(|t| {
                t.id == s.id
                    && crate::container::stackable(&t)
                    && t.count < max_stack(t.id)
                    && t.count < 64
                    && (!item::has_subtypes(t.id) || t.meta == s.meta)
                    && t.tag == s.tag
            })
        })
    }

    /// `storePartialItemStack`: returns what is left of `s.count`.
    fn store_partial(&mut self, s: &Stack) -> u32 {
        let n = s.count;
        if max_stack(s.id) == 1 {
            let k = self.first_empty();
            if k < 0 {
                return n;
            }
            let k = k as usize;
            if self.main[k].is_none() {
                self.main[k] = Some(*s);
            }
            return 0;
        }
        let k = match self.store_item_stack(s) {
            Some(k) => k as i32,
            None => self.first_empty(),
        };
        if k < 0 {
            return n;
        }
        let k = k as usize;
        let t = self.main[k].unwrap_or(Stack { count: 0, ..*s });
        let mut add = n;
        add = add.min(max_stack(t.id) - t.count);
        add = add.min(64u32.saturating_sub(t.count));
        if add == 0 {
            self.main[k] = Some(t).filter(|t| t.count > 0);
            return n;
        }
        self.main[k] = Some(Stack {
            count: t.count + add,
            ..t
        });
        n - add
    }

    /// `addItemStackToInventory`: `s.count` is left with what did not fit;
    /// the result says whether the last attempt stored anything (and is
    /// what decides the pickup sound).
    pub fn add_stack(&mut self, s: &mut Stack, creative: bool) -> bool {
        if s.count == 0 {
            return false;
        }
        if damaged(s) {
            let k = self.first_empty();
            if k >= 0 {
                self.main[k as usize] = Some(*s);
                s.count = 0;
                return true;
            }
            if creative {
                s.count = 0;
                return true;
            }
            return false;
        }
        let mut before;
        loop {
            before = s.count;
            s.count = self.store_partial(s);
            if !(s.count > 0 && s.count < before) {
                break;
            }
        }
        if s.count == before && creative {
            s.count = 0;
            return true;
        }
        s.count < before
    }

    /// `addItemStackToInventory` outside creative mode; returns the rest.
    pub fn add(&mut self, mut s: Stack) -> Option<Stack> {
        self.add_stack(&mut s, false);
        if s.count == 0 {
            None
        } else {
            Some(s)
        }
    }

    /// Takes one item from the selected hotbar slot (block placement).
    pub fn consume_held(&mut self) {
        if let Some(s) = &mut self.main[self.selected] {
            s.count -= 1;
            if s.count == 0 {
                self.main[self.selected] = None;
            }
        }
    }

    /// `InventoryPlayer.damageArmor`: every worn piece loses a quarter of
    /// the damage (at least 1) and breaks past its durability.
    pub fn damage_armor(&mut self, amount: f32) -> bool {
        let mut n = amount / 4.0;
        if n < 1.0 {
            n = 1.0;
        }
        let mut changed = false;
        for slot in &mut self.armor {
            if let Some(s) = slot {
                if armor_points(s.id).is_some() {
                    s.meta += n as i32;
                    changed = true;
                    if s.meta > item::max_damage(s.id) {
                        *slot = None;
                    }
                }
            }
        }
        changed
    }

    /// `getTotalArmorValue`: the `damageReduceAmount` of the worn pieces.
    pub fn armor_value(&self) -> i32 {
        self.armor
            .iter()
            .flatten()
            .filter_map(|s| armor_points(s.id))
            .sum()
    }

    /// Serialised form for the level file: `slot:id:meta:count` entries,
    /// armour as slots 100..104 like the `Inventory` NBT list.
    pub fn encode(&self) -> String {
        let mut v = vec![format!("sel{}", self.selected)];
        for (i, s) in self.main.iter().enumerate() {
            if let Some(s) = s {
                v.push(format!("{i}:{}", s.to_code()));
            }
        }
        for (i, s) in self.armor.iter().enumerate() {
            if let Some(s) = s {
                v.push(format!("{}:{}", 100 + i, s.to_code()));
            }
        }
        v.join(";")
    }

    pub fn decode(text: &str) -> Option<Inventory> {
        let mut inv = Inventory::default();
        for part in text.split(';') {
            if let Some(s) = part.strip_prefix("sel") {
                inv.selected = s.parse::<usize>().ok()?.min(8);
                continue;
            }
            let (slot, rest) = part.split_once(':')?;
            let i: usize = slot.parse().ok()?;
            let st = Stack::from_code(rest)?;
            if i < MAIN {
                inv.main[i] = Some(st);
            } else if (100..104).contains(&i) {
                inv.armor[i - 100] = Some(st);
            } else {
                return None;
            }
        }
        Some(inv)
    }
}

/// `ItemArmor.damageReduceAmount`
pub fn armor_points(id: item::ItemId) -> Option<i32> {
    // ItemArmor$ArmorMaterial: CLOTH, CHAIN, IRON, DIAMOND, GOLD
    const R: [[i32; 4]; 5] = [
        [1, 3, 2, 1],
        [2, 5, 4, 1],
        [2, 6, 5, 2],
        [3, 8, 6, 3],
        [2, 5, 3, 1],
    ];
    if (298..=317).contains(&id) {
        let k = (id - 298) as usize;
        Some(R[k / 4][k % 4])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_prefers_partial_stacks() {
        let mut inv = Inventory::default();
        inv.main[3] = Some(Stack::new(1, 0, 60));
        assert_eq!(inv.add(Stack::new(1, 0, 10)), None);
        assert_eq!(inv.main[3].unwrap().count, 64);
        assert_eq!(inv.main[0].unwrap().count, 6);
        // damaged tools take the first empty slot
        let mut t = Stack::new(257, 3, 1);
        assert!(inv.add_stack(&mut t, false));
        assert_eq!(inv.main[1], Some(Stack::new(257, 3, 1)));
    }

    #[test]
    fn full_inventory_reports_failure() {
        let mut inv = Inventory::default();
        for i in 0..MAIN {
            inv.main[i] = Some(Stack::new(3, 0, 64));
        }
        inv.main[7] = Some(Stack::new(4, 0, 60));
        let mut s = Stack::new(4, 0, 10);
        // four fit, then nothing: the last attempt stored nothing
        assert!(!inv.add_stack(&mut s, false));
        assert_eq!(s.count, 6);
        let mut c = Stack::new(5, 0, 3);
        assert!(inv.add_stack(&mut c, true));
        assert_eq!(c.count, 0);
    }

    #[test]
    fn encode_round_trips_and_rejects_garbage() {
        let mut inv = Inventory::default();
        inv.main[2] = Some(Stack::new(278, 17, 1));
        inv.main[30] = Some(Stack::new(4, 0, 64));
        inv.armor[3] = Some(Stack::new(310, 5, 1));
        inv.selected = 2;
        assert_eq!(Inventory::decode(&inv.encode()), Some(inv));
        assert!(Inventory::decode("5:1:0").is_none());
        assert!(Inventory::decode("99:1:0:1").is_none());
    }
}
