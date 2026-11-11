//! `InventoryPlayer`, `ContainerPlayer` / `ContainerWorkbench` click handling
//! and `CraftingManager` recipes.

use crate::block;
use crate::item::{self, max_stack, ItemId, Stack};

pub const MAIN: usize = 36;
/// Metadata wildcard (`OreDictionary`-less `32767` in recipes).
const ANY: i32 = 32767;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slot {
    /// 0..9 is the hotbar, 9..36 the main inventory.
    Main(usize),
    /// Crafting grid, row-major; 2x2 uses 0, 1, 3, 4 of the 3x3 layout.
    Craft(usize),
    Result,
    /// Outside the window: drop the cursor stack.
    Outside,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Inventory {
    pub main: [Option<Stack>; MAIN],
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
            grid: [None; 9],
            cursor: None,
            selected: 0,
            grid_size: 2,
        }
    }
}

fn limit(s: &Stack) -> u32 {
    max_stack(s.id)
}

impl Inventory {
    pub fn held(&self) -> Option<Stack> {
        self.main[self.selected]
    }

    /// `InventoryPlayer.addItemStackToInventory`: merges into existing stacks,
    /// then fills empty slots. Returns what did not fit.
    pub fn add(&mut self, mut s: Stack) -> Option<Stack> {
        for slot in self.main.iter_mut().flatten() {
            if s.count == 0 {
                break;
            }
            if slot.same_item(&s) && slot.meta == s.meta && slot.count < limit(slot) {
                let n = (limit(slot) - slot.count).min(s.count);
                slot.count += n;
                s.count -= n;
            }
        }
        for slot in self.main.iter_mut() {
            if s.count == 0 {
                break;
            }
            if slot.is_none() {
                let n = s.count.min(limit(&s));
                *slot = Some(Stack { count: n, ..s });
                s.count -= n;
            }
        }
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

    fn grid_slots(&self) -> Vec<usize> {
        if self.grid_size == 3 {
            (0..9).collect()
        } else {
            vec![0, 1, 3, 4]
        }
    }

    /// The current crafting result.
    pub fn result(&self) -> Option<Stack> {
        let g = &self.grid;
        let cells: Vec<Option<(ItemId, i32)>> =
            (0..9).map(|i| g[i].map(|s| (s.id, s.meta))).collect();
        find_recipe(&cells, self.grid_size)
    }

    fn consume_grid(&mut self) {
        for i in self.grid_slots() {
            if let Some(s) = &mut self.grid[i] {
                s.count -= 1;
                if s.count == 0 {
                    self.grid[i] = None;
                }
            }
        }
    }

    fn slot_mut(&mut self, s: Slot) -> Option<&mut Option<Stack>> {
        match s {
            Slot::Main(i) if i < MAIN => Some(&mut self.main[i]),
            Slot::Craft(i) if i < 9 && self.grid_slots().contains(&i) => Some(&mut self.grid[i]),
            _ => None,
        }
    }

    /// `mergeItemStack`: moves `s` into `range` of the main inventory, filling
    /// matching stacks first. Returns the remainder.
    fn merge_into(
        &mut self,
        mut s: Stack,
        range: std::ops::Range<usize>,
        reverse: bool,
    ) -> Option<Stack> {
        let idx: Vec<usize> = if reverse {
            range.clone().rev().collect()
        } else {
            range.clone().collect()
        };
        for &i in &idx {
            if let Some(t) = &mut self.main[i] {
                if t.same_item(&s) && t.meta == s.meta && t.count < limit(t) {
                    let n = (limit(t) - t.count).min(s.count);
                    t.count += n;
                    s.count -= n;
                    if s.count == 0 {
                        return None;
                    }
                }
            }
        }
        for &i in &idx {
            if self.main[i].is_none() {
                self.main[i] = Some(s);
                return None;
            }
        }
        Some(s)
    }

    /// `Container.slotClick`. `right` is the right mouse button; `shift`
    /// transfers between sections. Returns stacks to drop into the world.
    pub fn click(&mut self, slot: Slot, right: bool, shift: bool) -> Vec<Stack> {
        let mut drops = Vec::new();
        match slot {
            Slot::Outside => {
                if let Some(c) = self.cursor {
                    if right {
                        drops.push(Stack { count: 1, ..c });
                        self.cursor = if c.count > 1 {
                            Some(Stack {
                                count: c.count - 1,
                                ..c
                            })
                        } else {
                            None
                        };
                    } else {
                        drops.push(c);
                        self.cursor = None;
                    }
                }
            }
            Slot::Result => {
                if shift {
                    // Craft as many as fit.
                    while let Some(r) = self.result() {
                        let mut trial = self.clone();
                        if trial.merge_into(r, 0..MAIN, true).is_some() {
                            break;
                        }
                        *self = trial;
                        self.consume_grid();
                    }
                } else if let Some(r) = self.result() {
                    match self.cursor {
                        None => {
                            self.cursor = Some(r);
                            self.consume_grid();
                        }
                        Some(c)
                            if c.same_item(&r)
                                && c.meta == r.meta
                                && c.count + r.count <= limit(&c) =>
                        {
                            self.cursor = Some(Stack {
                                count: c.count + r.count,
                                ..c
                            });
                            self.consume_grid();
                        }
                        _ => {}
                    }
                }
            }
            Slot::Main(i) if shift => {
                if let Some(s) = self.main[i].take() {
                    // Hotbar <-> main inventory (ContainerPlayer.transferStackInSlot).
                    let rest = if i < 9 {
                        self.merge_into(s, 9..MAIN, false)
                    } else {
                        self.merge_into(s, 0..9, false)
                    };
                    self.main[i] = rest;
                }
            }
            Slot::Craft(i) if shift => {
                if let Some(s) = self.grid[i].take() {
                    let rest = self
                        .merge_into(s, 9..MAIN, false)
                        .and_then(|r| self.merge_into(r, 0..9, false));
                    self.grid[i] = rest;
                }
            }
            _ => {
                let cursor = self.cursor;
                let Some(target) = self.slot_mut(slot) else {
                    return drops;
                };
                let cur = *target;
                let (new_slot, new_cursor) = match (cur, cursor) {
                    (None, None) => (None, None),
                    (Some(s), None) => {
                        if right {
                            let take = (s.count + 1) / 2;
                            let left = s.count - take;
                            (
                                if left > 0 {
                                    Some(Stack { count: left, ..s })
                                } else {
                                    None
                                },
                                Some(Stack { count: take, ..s }),
                            )
                        } else {
                            (None, Some(s))
                        }
                    }
                    (None, Some(c)) => {
                        if right {
                            (
                                Some(Stack { count: 1, ..c }),
                                if c.count > 1 {
                                    Some(Stack {
                                        count: c.count - 1,
                                        ..c
                                    })
                                } else {
                                    None
                                },
                            )
                        } else {
                            let n = c.count.min(limit(&c));
                            (
                                Some(Stack { count: n, ..c }),
                                if c.count > n {
                                    Some(Stack {
                                        count: c.count - n,
                                        ..c
                                    })
                                } else {
                                    None
                                },
                            )
                        }
                    }
                    (Some(s), Some(c)) => {
                        if s.same_item(&c) && s.meta == c.meta {
                            let room = limit(&s).saturating_sub(s.count);
                            let n = if right {
                                1.min(room).min(c.count)
                            } else {
                                room.min(c.count)
                            };
                            let rest = c.count - n;
                            (
                                Some(Stack {
                                    count: s.count + n,
                                    ..s
                                }),
                                if rest > 0 {
                                    Some(Stack { count: rest, ..c })
                                } else {
                                    None
                                },
                            )
                        } else {
                            (Some(c), Some(s))
                        }
                    }
                };
                *target = new_slot;
                self.cursor = new_cursor;
            }
        }
        drops
    }

    /// `ContainerPlayer.onContainerClosed`: the grid and cursor go back into
    /// the inventory; what does not fit is dropped.
    pub fn close(&mut self) -> Vec<Stack> {
        let mut drops = Vec::new();
        let mut back: Vec<Stack> = self.grid.iter_mut().filter_map(|s| s.take()).collect();
        if let Some(c) = self.cursor.take() {
            back.push(c);
        }
        for s in back {
            if let Some(r) = self.add(s) {
                drops.push(r);
            }
        }
        self.grid_size = 2;
        drops
    }

    /// Serialised form for the level file: `slot:id:meta:count` entries.
    pub fn encode(&self) -> String {
        let mut v = vec![format!("sel{}", self.selected)];
        for (i, s) in self.main.iter().enumerate() {
            if let Some(s) = s {
                v.push(format!("{i}:{}", s.to_code()));
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
            if i >= MAIN {
                return None;
            }
            inv.main[i] = Some(Stack::from_code(rest)?);
        }
        Some(inv)
    }
}

/// One ingredient: item and metadata (`ANY` matches every metadata).
type Ing = Option<(ItemId, i32)>;

struct Shaped {
    w: usize,
    h: usize,
    cells: Vec<Ing>,
    out: Stack,
}

fn shaped(rows: &[&str], key: &[(char, ItemId, i32)], out: Stack) -> Shaped {
    let h = rows.len();
    let w = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut cells = Vec::with_capacity(w * h);
    for r in rows {
        let chars: Vec<char> = r.chars().collect();
        for x in 0..w {
            let c = chars.get(x).copied().unwrap_or(' ');
            cells.push(key.iter().find(|k| k.0 == c).map(|k| (k.1, k.2)));
        }
    }
    Shaped { w, h, cells, out }
}

fn recipes() -> &'static [Shaped] {
    use std::sync::OnceLock;
    static R: OnceLock<Vec<Shaped>> = OnceLock::new();
    R.get_or_init(|| {
        let b = |id: u8| id as ItemId;
        let mut r = Vec::new();
        for m in 0..4 {
            r.push(shaped(
                &["#"],
                &[('#', b(block::LOG), m)],
                Stack::new(b(block::PLANKS), m, 4),
            ));
        }
        for m in 0..2 {
            r.push(shaped(
                &["#"],
                &[('#', b(block::LOG2), m)],
                Stack::new(b(block::PLANKS), 4 + m, 4),
            ));
        }
        r.push(shaped(
            &["#", "#"],
            &[('#', b(block::PLANKS), ANY)],
            Stack::new(item::STICK, 0, 4),
        ));
        r.push(shaped(
            &["##", "##"],
            &[('#', b(block::PLANKS), ANY)],
            Stack::new(b(block::CRAFTING_TABLE), 0, 1),
        ));
        r.push(shaped(
            &["###", "# #", "###"],
            &[('#', b(block::PLANKS), ANY)],
            Stack::new(b(block::CHEST), 0, 1),
        ));
        r.push(shaped(
            &["###", "# #", "###"],
            &[('#', b(block::COBBLESTONE), ANY)],
            Stack::new(b(block::FURNACE), 0, 1),
        ));
        r.push(shaped(
            &["X", "#"],
            &[('X', item::COAL, ANY), ('#', item::STICK, ANY)],
            Stack::new(b(block::TORCH), 0, 4),
        ));
        r.push(shaped(
            &["##", "##"],
            &[('#', b(block::SAND), 0)],
            Stack::new(b(block::SANDSTONE), 0, 1),
        ));
        r.push(shaped(
            &["##", "##"],
            &[('#', item::SNOWBALL, ANY)],
            Stack::new(b(block::SNOW), 0, 1),
        ));
        r.push(shaped(
            &["##", "##"],
            &[('#', item::CLAY_BALL, ANY)],
            Stack::new(b(block::CLAY), 0, 1),
        ));
        r.push(shaped(
            &["###"],
            &[('#', b(block::STONE), ANY)],
            Stack::new(b(block::STONE_SLAB), 0, 6),
        ));
        r.push(shaped(
            &["###"],
            &[('#', b(block::SANDSTONE), ANY)],
            Stack::new(b(block::STONE_SLAB), 1, 6),
        ));
        r.push(shaped(
            &["###"],
            &[('#', b(block::COBBLESTONE), ANY)],
            Stack::new(b(block::STONE_SLAB), 3, 6),
        ));
        r.push(shaped(
            &["MMM", "MMM", "MMM"],
            &[('M', item::MELON, ANY)],
            Stack::new(b(block::MELON_BLOCK), 0, 1),
        ));
        // RecipesTools / RecipesWeapons
        let mats: [(ItemId, i32); 5] = [
            (b(block::PLANKS), ANY),
            (b(block::COBBLESTONE), ANY),
            (item::IRON_INGOT, ANY),
            (item::DIAMOND, ANY),
            (item::GOLD_INGOT, ANY),
        ];
        let ids: [[ItemId; 5]; 5] = [
            [268, 269, 270, 271, 290],
            [272, 273, 274, 275, 291],
            [267, 256, 257, 258, 292],
            [276, 277, 278, 279, 293],
            [283, 284, 285, 286, 294],
        ];
        let shapes: [&[&str]; 5] = [
            &["X", "X", "#"],
            &["X", "#", "#"],
            &["XXX", " # ", " # "],
            &["XX", "X#", " #"],
            &["XX", " #", " #"],
        ];
        for (mi, &(mat, mm)) in mats.iter().enumerate() {
            for (k, sh) in shapes.iter().enumerate() {
                r.push(shaped(
                    sh,
                    &[('X', mat, mm), ('#', item::STICK, ANY)],
                    Stack::new(ids[mi][k], 0, 1),
                ));
            }
        }
        r
    })
}

fn matches_at(
    rec: &Shaped,
    cells: &[Ing],
    size: usize,
    ox: usize,
    oy: usize,
    mirror: bool,
) -> bool {
    for y in 0..size {
        for x in 0..size {
            let want = if x >= ox && y >= oy && x - ox < rec.w && y - oy < rec.h {
                let rx = if mirror { rec.w - 1 - (x - ox) } else { x - ox };
                rec.cells[(y - oy) * rec.w + rx]
            } else {
                None
            };
            let have = cells[y * 3 + x];
            match (want, have) {
                (None, None) => {}
                (Some((wi, wm)), Some((hi, hm))) => {
                    if wi != hi || (wm != ANY && wm != hm) {
                        return false;
                    }
                }
                _ => return false,
            }
        }
    }
    true
}

/// `CraftingManager.findMatchingRecipe` for a `size` x `size` grid laid out
/// in the top-left of a 3x3 array.
pub fn find_recipe(cells: &[Ing], size: usize) -> Option<Stack> {
    // Two of the same damaged tool repair each other (shapeless rule).
    let filled: Vec<(ItemId, i32)> = (0..size)
        .flat_map(|y| (0..size).map(move |x| y * 3 + x))
        .filter_map(|i| cells[i])
        .collect();
    if filled.len() == 2 && filled[0].0 == filled[1].0 && item::tool(filled[0].0).is_some() {
        let t = item::tool(filled[0].0).unwrap();
        let left = |d: i32| t.max_uses - d;
        let sum = left(filled[0].1) + left(filled[1].1) + t.max_uses * 5 / 100;
        let damage = (t.max_uses - sum).max(0);
        return Some(Stack::new(filled[0].0, damage, 1));
    }
    for rec in recipes() {
        if rec.w > size || rec.h > size {
            continue;
        }
        for ox in 0..=(size - rec.w) {
            for oy in 0..=(size - rec.h) {
                if matches_at(rec, cells, size, ox, oy, false)
                    || matches_at(rec, cells, size, ox, oy, true)
                {
                    return Some(rec.out);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(items: &[(usize, ItemId, i32)]) -> Vec<Ing> {
        let mut g = vec![None; 9];
        for &(i, id, m) in items {
            g[i] = Some((id, m));
        }
        g
    }

    #[test]
    fn recipes_match_in_any_position_and_mirrored() {
        let p = block::PLANKS as ItemId;
        assert_eq!(
            find_recipe(&grid(&[(0, block::LOG as ItemId, 2)]), 2),
            Some(Stack::new(p, 2, 4))
        );
        assert_eq!(
            find_recipe(&grid(&[(4, block::LOG as ItemId, 2)]), 2),
            Some(Stack::new(p, 2, 4))
        );
        assert_eq!(
            find_recipe(&grid(&[(1, p, 0), (4, p, 3)]), 2),
            Some(Stack::new(item::STICK, 0, 4))
        );
        // Axe and its mirror image.
        let c = block::COBBLESTONE as ItemId;
        let axe = grid(&[
            (0, c, 0),
            (1, c, 0),
            (3, c, 0),
            (4, item::STICK, 0),
            (7, item::STICK, 0),
        ]);
        let mirrored = grid(&[
            (1, c, 0),
            (2, c, 0),
            (5, c, 0),
            (4, item::STICK, 0),
            (7, item::STICK, 0),
        ]);
        assert_eq!(find_recipe(&axe, 3).map(|s| s.id), Some(275));
        assert_eq!(find_recipe(&mirrored, 3).map(|s| s.id), Some(275));
        // A 3x3 recipe does not fit the 2x2 grid.
        assert_eq!(find_recipe(&axe, 2), None);
    }

    #[test]
    fn add_merges_then_fills() {
        let mut inv = Inventory::default();
        inv.main[3] = Some(Stack::new(1, 0, 60));
        assert_eq!(inv.add(Stack::new(1, 0, 10)), None);
        assert_eq!(inv.main[3].unwrap().count, 64);
        assert_eq!(inv.main[0].unwrap().count, 6);
    }

    #[test]
    fn clicks_follow_container_rules() {
        let mut inv = Inventory::default();
        inv.main[0] = Some(Stack::new(3, 0, 9));
        inv.click(Slot::Main(0), true, false);
        assert_eq!(inv.cursor.unwrap().count, 5);
        assert_eq!(inv.main[0].unwrap().count, 4);
        inv.click(Slot::Main(1), true, false);
        assert_eq!(inv.main[1].unwrap().count, 1);
        inv.click(Slot::Main(0), false, false);
        assert_eq!(inv.main[0].unwrap().count, 8);
        assert!(inv.cursor.is_none());
        // Crafting: two planks in a column make sticks, consuming one each.
        inv.main[5] = Some(Stack::new(block::PLANKS as ItemId, 0, 2));
        inv.grid[0] = Some(Stack::new(block::PLANKS as ItemId, 0, 2));
        inv.grid[3] = Some(Stack::new(block::PLANKS as ItemId, 0, 2));
        inv.click(Slot::Result, false, true);
        assert!(inv.grid[0].is_none());
        assert_eq!(
            inv.main
                .iter()
                .flatten()
                .filter(|s| s.id == item::STICK)
                .map(|s| s.count)
                .sum::<u32>(),
            8
        );
    }

    #[test]
    fn encode_round_trips_and_rejects_garbage() {
        let mut inv = Inventory::default();
        inv.main[2] = Some(Stack::new(278, 17, 1));
        inv.main[30] = Some(Stack::new(4, 0, 64));
        inv.selected = 2;
        assert_eq!(Inventory::decode(&inv.encode()), Some(inv));
        assert!(Inventory::decode("5:1:0").is_none());
        assert!(Inventory::decode("99:1:0:1").is_none());
    }
}
