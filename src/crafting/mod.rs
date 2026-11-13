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
            Recipe::Special("RecipesArmorDyes") => armor_dyes(g).is_some(),
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
            Recipe::Special("RecipesArmorDyes") => armor_dyes(g),
            Recipe::Special(_) => None,
        }
    }
}

/// `RecipesArmorDyes`: one leather piece and dyes, the colours averaged
/// and brightened to the average maximum channel.
fn armor_dyes(g: &Grid) -> Option<Stack> {
    let mut out: Option<Stack> = None;
    let mut sum = [0i32; 3];
    let (mut max_sum, mut n) = (0i32, 0i32);
    let mut armor = false;
    for i in 0..g.len() {
        let Some(s) = g.slot(i) else { continue };
        if (298..=317).contains(&s.id) {
            armor = true;
            // only leather (ArmorMaterial.CLOTH), and only one piece
            if (298..=301).contains(&s.id) && out.is_none() {
                let mut c = s;
                c.count = 1;
                if let Some(col) = s.dye_color() {
                    let (r, gg, b) = (
                        (col >> 16 & 255) as f32 / 255.0,
                        (col >> 8 & 255) as f32 / 255.0,
                        (col & 255) as f32 / 255.0,
                    );
                    max_sum = (max_sum as f32 + r.max(gg.max(b)) * 255.0) as i32;
                    sum[0] = (sum[0] as f32 + r * 255.0) as i32;
                    sum[1] = (sum[1] as f32 + gg * 255.0) as i32;
                    sum[2] = (sum[2] as f32 + b * 255.0) as i32;
                    n += 1;
                }
                out = Some(c);
            } else {
                return None;
            }
        } else if s.id == 351 {
            // EntitySheep.fleeceColorTable[BlockColored.func_150032_b(damage)]
            let f = crate::render::model::FLEECE[(!s.meta & 15) as usize];
            let (r, gg, b) = (
                (f[0] * 255.0) as i32,
                (f[1] * 255.0) as i32,
                (f[2] * 255.0) as i32,
            );
            max_sum += r.max(gg.max(b));
            sum[0] += r;
            sum[1] += gg;
            sum[2] += b;
            n += 1;
        } else {
            return None;
        }
    }
    if !armor || n == 0 {
        return None;
    }
    let mut out = out?;
    let (mut r, mut gg, mut b) = (sum[0] / n, sum[1] / n, sum[2] / n);
    let avg = max_sum as f32 / n as f32;
    let m = r.max(gg.max(b)) as f32;
    r = (r as f32 * avg / m) as i32;
    gg = (gg as f32 * avg / m) as i32;
    b = (b as f32 * avg / m) as i32;
    let color = ((r << 8) + gg << 8) + b;
    // ItemArmor.func_82813_b: display.color
    out = out.edit_tag(|t| {
        use crate::world::anvil::Nbt;
        if !matches!(t.get("display"), Some(Nbt::Compound(_))) {
            t.set("display", Nbt::Compound(Vec::new()));
        }
        if let Some(d) = t.get_mut("display") {
            d.set("color", Nbt::Int(color));
        }
    });
    Some(out)
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

    /// Against `tools/DyeHarness.java`.
    #[test]
    fn armor_dyes_match_the_game() {
        for l in include_str!("../../tools/dyes_1710.txt").lines() {
            let (grid, want) = l.split_once('|').unwrap();
            let mut cells = vec![None; 9];
            for (i, part) in grid.split(' ').enumerate() {
                let f: Vec<i32> = part.split(':').map(|x| x.parse().unwrap()).collect();
                let mut st = Stack::new(f[0] as ItemId, f[1], 1);
                if f.len() > 2 {
                    let c = f[2];
                    st = st.edit_tag(|t| {
                        t.set(
                            "display",
                            crate::world::anvil::Nbt::Compound(vec![(
                                "color".into(),
                                crate::world::anvil::Nbt::Int(c),
                            )]),
                        )
                    });
                }
                cells[i] = Some(st);
            }
            let got = find_matching(&Grid {
                cells: &cells,
                width: 3,
            });
            let got = match got {
                Some(s) => s
                    .dye_color()
                    .map_or_else(|| format!("item{}", s.id), |c| c.to_string()),
                None => "none".into(),
            };
            assert_eq!(got, want, "{grid}");
        }
    }
}
