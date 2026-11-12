//! `CraftingManager`, `ShapedRecipes`, `ShapelessRecipes` and the special
//! recipe classes, matched against an `InventoryCrafting` grid.

mod data;

use crate::item::{self, ItemId, Stack};

/// The metadata wildcard of recipe ingredients.
pub const ANY: i32 = 32767;

pub enum Recipe {
    Shaped {
        w: usize,
        h: usize,
        out: (ItemId, i32, u32),
        items: &'static [Option<(ItemId, i32)>],
    },
    Shapeless {
        out: (ItemId, i32, u32),
        items: &'static [(ItemId, i32)],
    },
    /// An `IRecipe` class with its own matching code.
    Special(&'static str),
}

pub use data::RECIPES;

/// `InventoryCrafting`: `width` columns, row-major cells.
pub struct Grid<'a> {
    pub cells: &'a [Option<Stack>],
    pub width: usize,
}

impl Grid<'_> {
    fn len(&self) -> usize {
        self.cells.len()
    }
    fn slot(&self, i: usize) -> Option<Stack> {
        self.cells.get(i).copied().flatten()
    }
    /// `getStackInRowAndColumn(column, row)`
    fn at(&self, x: usize, y: usize) -> Option<Stack> {
        if x < self.width {
            self.slot(x + y * self.width)
        } else {
            None
        }
    }
}

fn ing_match(want: (ItemId, i32), have: &Stack) -> bool {
    want.0 == have.id && (want.1 == ANY || want.1 == have.meta)
}

/// `ShapedRecipes.checkMatch`
fn check_match(
    g: &Grid,
    w: usize,
    h: usize,
    items: &[Option<(ItemId, i32)>],
    ox: usize,
    oy: usize,
    mirror: bool,
) -> bool {
    for x in 0..3 {
        for y in 0..3 {
            let (rx, ry) = (x as isize - ox as isize, y as isize - oy as isize);
            let mut want = None;
            if rx >= 0 && ry >= 0 && (rx as usize) < w && (ry as usize) < h {
                let (rx, ry) = (rx as usize, ry as usize);
                want = if mirror {
                    items[w - rx - 1 + ry * w]
                } else {
                    items[rx + ry * w]
                };
            }
            let have = g.at(x, y);
            match (want, have) {
                (None, None) => {}
                (Some(wt), Some(hv)) => {
                    if !ing_match(wt, &hv) {
                        return false;
                    }
                }
                _ => return false,
            }
        }
    }
    true
}

impl Recipe {
    /// `IRecipe.matches`
    pub fn matches(&self, g: &Grid) -> bool {
        match self {
            Recipe::Shaped { w, h, items, .. } => {
                for ox in 0..=(3 - w) {
                    for oy in 0..=(3 - h) {
                        if check_match(g, *w, *h, items, ox, oy, true)
                            || check_match(g, *w, *h, items, ox, oy, false)
                        {
                            return true;
                        }
                    }
                }
                false
            }
            Recipe::Shapeless { items, .. } => {
                let mut left: Vec<(ItemId, i32)> = items.to_vec();
                for y in 0..3 {
                    for x in 0..3 {
                        if let Some(s) = g.at(x, y) {
                            match left.iter().position(|&wt| ing_match(wt, &s)) {
                                Some(k) => {
                                    left.remove(k);
                                }
                                None => return false,
                            }
                        }
                    }
                }
                left.is_empty()
            }
            Recipe::Special("RecipesMapCloning") => map_cloning(g).is_some(),
            // The others read item NBT (dye colours, firework and book
            // data, map scale) that stacks do not carry yet.
            Recipe::Special(_) => false,
        }
    }

    /// `IRecipe.getCraftingResult`
    pub fn result(&self, g: &Grid) -> Option<Stack> {
        match self {
            Recipe::Shaped { out, .. } | Recipe::Shapeless { out, .. } => {
                Some(Stack::new(out.0, out.1, out.2))
            }
            Recipe::Special("RecipesMapCloning") => map_cloning(g),
            Recipe::Special(_) => None,
        }
    }
}

const FILLED_MAP: ItemId = 358;
const EMPTY_MAP: ItemId = 395;

/// `RecipesMapCloning`: one filled map and some empty maps.
fn map_cloning(g: &Grid) -> Option<Stack> {
    let mut n = 0;
    let mut map: Option<Stack> = None;
    for i in 0..g.len() {
        if let Some(s) = g.slot(i) {
            if s.id == FILLED_MAP {
                if map.is_some() {
                    return None;
                }
                map = Some(s);
            } else if s.id == EMPTY_MAP {
                n += 1;
            } else {
                return None;
            }
        }
    }
    let m = map?;
    if n == 0 {
        return None;
    }
    Some(Stack::new(FILLED_MAP, m.meta, n + 1))
}

/// `CraftingManager.findMatchingRecipe`: two of the same damageable item
/// repair each other; otherwise the first matching recipe in list order.
pub fn find_matching(g: &Grid) -> Option<Stack> {
    let mut n = 0;
    let (mut a, mut b) = (None, None);
    for i in 0..g.len() {
        if let Some(s) = g.slot(i) {
            if n == 0 {
                a = Some(s);
            }
            if n == 1 {
                b = Some(s);
            }
            n += 1;
        }
    }
    if n == 2 {
        let (a, b) = (a.unwrap(), b.unwrap());
        let md = item::max_damage(a.id);
        // Item.isDamageable: maxDamage > 0 and no subtypes
        if a.id == b.id && a.count == 1 && b.count == 1 && md > 0 {
            let sum = (md - a.meta) + (md - b.meta) + md * 5 / 100;
            let d = (md - sum).max(0);
            return Some(Stack::new(a.id, d, 1));
        }
    }
    for r in RECIPES {
        if r.matches(g) {
            return r.result(g);
        }
    }
    None
}

/// `Item.getContainerItem`: what a crafting ingredient leaves behind.
pub fn container_item(id: ItemId) -> Option<ItemId> {
    match id {
        // water, lava and milk buckets
        326 | 327 | 335 => Some(325),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(w: usize, cells: &[(usize, ItemId, i32)]) -> Vec<Option<Stack>> {
        let mut v = vec![None; w * w];
        for &(i, id, m) in cells {
            v[i] = Some(Stack::new(id, m, 1));
        }
        v
    }

    fn find(w: usize, cells: &[(usize, ItemId, i32)]) -> Option<Stack> {
        let v = grid(w, cells);
        find_matching(&Grid {
            cells: &v,
            width: w,
        })
    }

    #[test]
    fn list_matches_the_game() {
        assert_eq!(RECIPES.len(), 317);
    }

    #[test]
    fn shaped_in_any_position_and_mirrored() {
        // planks from a log in any cell of the 2x2 grid
        for i in 0..4 {
            assert_eq!(find(2, &[(i, 17, 2)]), Some(Stack::new(5, 2, 4)));
        }
        // crafting table in the 2x2 grid
        let t = find(2, &[(0, 5, 0), (1, 5, 1), (2, 5, 3), (3, 5, 0)]);
        assert_eq!(t, Some(Stack::new(58, 0, 1)));
        // a stick column in the right column of the 3x3 grid
        assert_eq!(
            find(3, &[(2, 5, 0), (5, 5, 0)]),
            Some(Stack::new(280, 0, 4))
        );
        // axe, both orientations
        let axe = [(0, 4, 0), (1, 4, 0), (3, 4, 0), (4, 280, 0), (7, 280, 0)];
        assert_eq!(find(3, &axe), Some(Stack::new(275, 0, 1)));
        let mirrored = [(1, 4, 0), (2, 4, 0), (5, 4, 0), (4, 280, 0), (7, 280, 0)];
        assert_eq!(find(3, &mirrored), Some(Stack::new(275, 0, 1)));
        // a pickaxe does not fit the 2x2 grid
        assert_eq!(find(2, &[(0, 4, 0), (1, 4, 0)]), None);
    }

    #[test]
    fn shapeless_and_repair() {
        // book: three paper and a leather in any cells
        let b = find(3, &[(8, 339, 0), (0, 334, 0), (4, 339, 0), (2, 339, 0)]);
        assert_eq!(b, Some(Stack::new(340, 0, 1)));
        // two worn iron pickaxes (250 uses)
        let r = find(2, &[(0, 257, 200), (3, 257, 100)]);
        assert_eq!(r, Some(Stack::new(257, 250 - (50 + 150 + 12), 1)));
        // repair even works for bows
        assert_eq!(
            find(2, &[(1, 261, 0), (2, 261, 0)]),
            Some(Stack::new(261, 0, 1))
        );
        // map cloning
        let m = find(3, &[(0, 358, 7), (1, 395, 0), (5, 395, 0)]);
        assert_eq!(m, Some(Stack::new(358, 7, 3)));
    }
}
