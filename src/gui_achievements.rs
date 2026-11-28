//! `GuiAchievements`: the achievement map over its random rock background,
//! dragged around with the mouse.

use crate::gui::Ui;
use crate::lang::Lang;

/// `AchievementList`: id, column, row, special.
const MAP: &[(&str, i32, i32, bool)] = &[
    ("achievement.openInventory", 0, 0, false),
    ("achievement.mineWood", 2, 1, false),
    ("achievement.buildWorkBench", 4, -1, false),
    ("achievement.buildPickaxe", 4, 2, false),
    ("achievement.buildFurnace", 3, 4, false),
    ("achievement.acquireIron", 1, 4, false),
    ("achievement.buildHoe", 2, -3, false),
    ("achievement.makeBread", -1, -3, false),
    ("achievement.bakeCake", 0, -5, false),
    ("achievement.buildBetterPickaxe", 6, 2, false),
    ("achievement.cookFish", 2, 6, false),
    ("achievement.onARail", 2, 3, true),
    ("achievement.buildSword", 6, -1, false),
    ("achievement.killEnemy", 8, -1, false),
    ("achievement.killCow", 7, -3, false),
    ("achievement.flyPig", 9, -3, true),
    ("achievement.snipeSkeleton", 7, 0, true),
    ("achievement.diamonds", -1, 5, false),
    ("achievement.diamondsToYou", -1, 2, false),
    ("achievement.portal", -1, 7, false),
    ("achievement.ghast", -4, 8, true),
    ("achievement.blazeRod", 0, 9, false),
    ("achievement.potion", 2, 8, false),
    ("achievement.theEnd", 3, 10, true),
    ("achievement.theEnd2", 4, 13, true),
    ("achievement.enchantments", -4, 4, false),
    ("achievement.overkill", -4, 1, true),
    ("achievement.bookcase", -3, 6, false),
    ("achievement.breedCow", 7, -5, false),
    ("achievement.spawnWither", 7, 12, false),
    ("achievement.killWither", 7, 10, false),
    ("achievement.fullBeacon", 7, 8, true),
    ("achievement.exploreAllBiomes", 4, 8, true),
];

/// `textures/gui/achievement/achievement_background.png` on the GUI sheet.
const BG: (f32, f32) = (768.0, 1024.0);
const W: i32 = 256;
const H: i32 = 202;

fn bounds() -> (i32, i32, i32, i32) {
    let (mut c0, mut c1, mut r0, mut r1) = (i32::MAX, i32::MIN, i32::MAX, i32::MIN);
    for &(_, c, r, _) in MAP {
        c0 = c0.min(c);
        c1 = c1.max(c);
        r0 = r0.min(r);
        r1 = r1.max(r);
    }
    (c0 * 24 - 112, r0 * 24 - 112, c1 * 24 - 77, r1 * 24 - 77)
}

fn parent(id: &str) -> Option<&'static str> {
    crate::stats::ACHIEVEMENTS.iter().find(|a| a.0 == id).and_then(|a| a.1)
}

fn at(id: &str) -> Option<(i32, i32)> {
    MAP.iter().find(|a| a.0 == id).map(|a| (a.1, a.2))
}

/// `StatFileWriter.countRequirementsUntilAvailable`
fn requirements(id: &str, has: &dyn Fn(&str) -> bool) -> i32 {
    if has(id) {
        return 0;
    }
    let mut n = 0;
    let mut p = parent(id);
    while let Some(q) = p {
        if has(q) {
            break;
        }
        n += 1;
        p = parent(q);
    }
    n
}

/// `canUnlockAchievement`
fn can_unlock(id: &str, has: &dyn Fn(&str) -> bool) -> bool {
    parent(id).is_none_or(|p| has(p))
}

/// `String.hashCode`
fn java_hash(s: &str) -> i32 {
    s.encode_utf16().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32))
}

pub struct State {
    o: (f64, f64),
    p: (f64, f64),
    target: (f64, f64),
    scrolling: i32,
    last: (i32, i32),
}

impl State {
    pub fn new() -> State {
        // centred on openInventory
        let x = (0 * 24 - 141 / 2 - 12) as f64;
        let y = (0 * 24 - 141 / 2) as f64;
        State {
            o: (x, y),
            p: (x, y),
            target: (x, y),
            scrolling: 0,
            last: (0, 0),
        }
    }

    /// `updateScreen`: easing towards the target.
    pub fn tick(&mut self) {
        self.o = self.p;
        let (dx, dy) = (self.target.0 - self.p.0, self.target.1 - self.p.1);
        if dx * dx + dy * dy < 4.0 {
            self.p.0 += dx;
            self.p.1 += dy;
        } else {
            self.p.0 += dx * 0.85;
            self.p.1 += dy * 0.85;
        }
    }

    /// `drawScreen` with `achieved` the unlocked ones.
    pub fn draw(&mut self, ui: &mut Ui, lang: &Lang, achieved: &[String], mouse: (f32, f32), down: bool, partial: f32) {
        let has = |id: &str| achieved.iter().any(|a| a == id);
        let (w, h) = (ui.w as i32, ui.h as i32);
        let left = (w - W) / 2;
        let top = (h - H) / 2;
        let (mx, my) = (mouse.0 as i32, mouse.1 as i32);
        // dragging inside the window
        if down {
            let (ax, ay) = (left + 8, top + 17);
            if (self.scrolling == 0 || self.scrolling == 1) && mx >= ax && mx < ax + 224 && my >= ay && my < ay + 155 {
                if self.scrolling == 0 {
                    self.scrolling = 1;
                } else {
                    self.p.0 -= (mx - self.last.0) as f64;
                    self.p.1 -= (my - self.last.1) as f64;
                    self.target = self.p;
                    self.o = self.p;
                }
                self.last = (mx, my);
            }
        } else {
            self.scrolling = 0;
        }
        let (x_min, y_min, x_max, y_max) = bounds();
        self.target.0 = self.target.0.clamp(x_min as f64, (x_max - 1) as f64);
        self.target.1 = self.target.1.clamp(y_min as f64, (y_max - 1) as f64);
        crate::screens::dim_background(ui);
        let clip_from = ui.v.len();
        // drawAchievementScreen
        let sx = ((self.o.0 + (self.p.0 - self.o.0) * partial as f64).floor() as i32).clamp(x_min, x_max - 1);
        let sy = ((self.o.1 + (self.p.1 - self.o.1) * partial as f64).floor() as i32).clamp(y_min, y_max - 1);
        let (ox, oy) = (left + 16, top + 17);
        let (c0, r0) = ((sx + 288) >> 4, (sy + 288) >> 4);
        let (fx, fy) = ((sx + 288) % 16, (sy + 288) % 16);
        let seed_base = java_hash(&crate::stats::offline_uuid(crate::sim::PLAYER_NAME)) as i64;
        let mut row = 0;
        while row * 16 - fy < 155 {
            let shade = 0.6 - (r0 + row) as f32 / 25.0 * 0.3;
            let grey = (shade.clamp(0.0, 1.0) * 255.0) as u32;
            let tint = grey << 16 | grey << 8 | grey;
            let mut col = 0;
            while col * 16 - fx < 224 {
                let mut r = crate::java::JavaRandom::new(
                    seed_base + (c0 + col) as i64 + ((r0 + row) * 16) as i64,
                );
                let k = r.next_int_n(1 + r0 + row) + (r0 + row) / 2;
                let name = if k > 37 || r0 + row == 35 {
                    "bedrock"
                } else if k == 22 {
                    if r.next_int_n(2) == 0 {
                        "diamond_ore"
                    } else {
                        "redstone_ore"
                    }
                } else if k == 10 {
                    "iron_ore"
                } else if k == 8 {
                    "coal_ore"
                } else if k > 4 {
                    "stone"
                } else if k > 0 {
                    "dirt"
                } else {
                    "sand"
                };
                ui.named_icon_tinted(name, (ox + col * 16 - fx) as f32, (oy + row * 16 - fy) as f32, tint);
                col += 1;
            }
            row += 1;
        }
        // the lines to the parents and their arrows
        for &(id, c, r, _) in MAP {
            let Some(p) = parent(id).and_then(at) else { continue };
            let n = requirements(id, &has);
            if n > 4 {
                continue;
            }
            let (x1, y1) = (c * 24 - sx + 11 + ox, r * 24 - sy + 11 + oy);
            let (x2, y2) = (p.0 * 24 - sx + 11 + ox, p.1 * 24 - sy + 11 + oy);
            let colour = if has(id) {
                0xA0A0A0
            } else if can_unlock(id, &has) {
                0x00FF00
            } else {
                0x000000
            };
            let (a, b) = (x1.min(x2), x1.max(x2));
            ui.rect(a as f32, y1 as f32, (b - a + 1) as f32, 1.0, colour, 1.0);
            let (a, b) = (y1.min(y2), y1.max(y2));
            ui.rect(x2 as f32, a as f32 + 1.0, 1.0, (b - a - 1).max(0) as f32, colour, 1.0);
            if x1 > x2 {
                ui.sheet((x1 - 11 - 7) as f32, (y1 - 5) as f32, BG.0 + 114.0, BG.1 + 234.0, 7.0, 11.0);
            } else if x1 < x2 {
                ui.sheet((x1 + 11) as f32, (y1 - 5) as f32, BG.0 + 107.0, BG.1 + 234.0, 7.0, 11.0);
            } else if y1 > y2 {
                ui.sheet((x1 - 5) as f32, (y1 - 11 - 7) as f32, BG.0 + 96.0, BG.1 + 234.0, 11.0, 7.0);
            } else if y1 < y2 {
                ui.sheet((x1 - 5) as f32, (y1 + 11) as f32, BG.0 + 96.0, BG.1 + 241.0, 11.0, 7.0);
            }
        }
        // the icons
        let mut over = None;
        for &(id, c, r, special) in MAP {
            let (x, y) = (c * 24 - sx, r * 24 - sy);
            if !(x >= -24 && y >= -24 && x <= 224 && y <= 155) {
                continue;
            }
            let n = requirements(id, &has);
            let k = if has(id) {
                0.75
            } else if can_unlock(id, &has) {
                1.0
            } else if n < 3 {
                0.3
            } else if n == 3 {
                0.2
            } else if n == 4 {
                0.1
            } else {
                continue;
            };
            let g = (k * 255.0) as u32;
            let tint = g << 16 | g << 8 | g;
            let (px, py) = ((x + ox) as f32, (y + oy) as f32);
            let u = if special { 26.0 } else { 0.0 };
            ui.sheet_tinted(px - 2.0, py - 2.0, BG.0 + u, BG.1 + 202.0, 26.0, 26.0, 26.0, 26.0, tint);
            let (iid, imeta) = crate::stats::icon(id);
            crate::screens::draw_stack_with(ui, &crate::item::Stack::new(iid, imeta, 1), px + 3.0, py + 3.0, None);
            if !can_unlock(id, &has) {
                // renderWithColor = false: the item a dark silhouette
                ui.rect(px + 3.0, py + 3.0, 16.0, 16.0, 0x000000, 0.9);
            }
            if mx >= x + ox && mx <= x + ox + 22 && my >= y + oy && my <= y + oy + 22 {
                over = Some(id);
            }
        }
        // the depth trick of drawAchievementScreen: nothing shows outside
        // the window
        clip(ui, clip_from, [ox as f32, oy as f32, (ox + 224) as f32, (oy + 155) as f32]);
        // the frame over everything, the title
        ui.sheet(left as f32, top as f32, BG.0, BG.1, W as f32, H as f32);
        ui.text(lang.get("gui.achievements"), (left + 15) as f32, (top + 5) as f32, 4210752, false);
        crate::screens::button(ui, (w / 2 + 24) as f32, (h / 2 + 74) as f32, 80.0, lang.get("gui.done"), true, mouse);
        // the tooltip
        if let Some(id) = over {
            let special = MAP.iter().any(|a| a.0 == id && a.3);
            let (x, y) = ((mx + 12) as f32, (my - 4) as f32);
            let mut name = lang.get(id).to_string();
            let n = requirements(id, &has);
            let unlockable = can_unlock(id, &has);
            let mut desc_lines: Vec<String>;
            let width;
            let desc_colour;
            let mut taken = false;
            if !unlockable {
                if n > 3 {
                    return;
                }
                if n == 3 {
                    name = lang.get("achievement.unknown").to_string();
                }
                width = ui.text_width(&name).max(120.0);
                let pn = parent(id).map(|p| lang.get(p).to_string()).unwrap_or_default();
                let req = lang.format("achievement.requires", &[pn]);
                desc_lines = ui.wrap(&req, width);
                desc_colour = 0x705050;
            } else {
                width = ui.text_width(&name).max(120.0);
                let desc = lang.get(&format!("{id}.desc")).to_string();
                desc_lines = ui.wrap(&desc, width);
                desc_colour = 0xA0A0A0;
                taken = has(id);
            }
            let mut dh = desc_lines.len() as f32 * 9.0;
            if taken {
                dh += 12.0;
            }
            ui.gradient_rect(x - 3.0, y - 3.0, width + 6.0, dh + 12.0 + 6.0, 0xC0000000, 0xC0000000);
            for (i, l) in desc_lines.drain(..).enumerate() {
                ui.text(&l, x, y + 12.0 + i as f32 * 9.0, desc_colour, false);
            }
            if taken {
                ui.text(lang.get("achievement.taken"), x, y + dh + 4.0, 0x9090FF, true);
            }
            let colour = match (unlockable, special) {
                (true, true) => 0xFFFF80,
                (true, false) => 0xFFFFFF,
                (false, true) => 0x808040,
                (false, false) => 0x808080,
            };
            ui.text(&name, x, y, colour, true);
        }
    }
}

/// Is `p` on the Done button?
pub fn done_at(w: f32, h: f32, p: (f32, f32)) -> bool {
    let (x, y) = (((w as i32) / 2 + 24) as f32, ((h as i32) / 2 + 74) as f32);
    p.0 >= x && p.1 >= y && p.0 < x + 80.0 && p.1 < y + 20.0
}

/// Trims the axis-aligned quads (six vertices each) from `start` on to
/// `r` (x0, y0, x1, y1), their UVs with them; a quad wholly outside
/// collapses to nothing.
fn clip(ui: &mut Ui, start: usize, r: [f32; 4]) {
    let v = &mut ui.v[start..];
    if v.len() % 6 != 0 {
        return;
    }
    for q in v.chunks_mut(6) {
        // q[0], q[1], q[2], q[0], q[2], q[3] of the corners
        let c = [q[0], q[1], q[2], q[5]];
        let xs = c.iter().map(|p| p.pos[0]);
        let ys = c.iter().map(|p| p.pos[1]);
        let (x0, x1) = (xs.clone().fold(f32::MAX, f32::min), xs.fold(f32::MIN, f32::max));
        let (y0, y1) = (ys.clone().fold(f32::MAX, f32::min), ys.fold(f32::MIN, f32::max));
        if x1 <= x0 || y1 <= y0 {
            continue;
        }
        let (nx0, nx1) = (x0.max(r[0]), x1.min(r[2]));
        let (ny0, ny1) = (y0.max(r[1]), y1.min(r[3]));
        for p in q.iter_mut() {
            if nx0 >= nx1 || ny0 >= ny1 {
                p.pos[0] = r[0];
                p.pos[1] = r[1];
                continue;
            }
            let fx = (p.pos[0] - x0) / (x1 - x0);
            let fy = (p.pos[1] - y0) / (y1 - y0);
            let nx = if fx < 0.5 { nx0 } else { nx1 };
            let ny = if fy < 0.5 { ny0 } else { ny1 };
            p.pos[0] = nx;
            p.pos[1] = ny;
        }
        // the UVs follow the corners: find each corner's uv by its place
        let find = |xa: bool, ya: bool| {
            c.iter()
                .find(|p| ((p.pos[0] - x0).abs() < 1e-3) == xa && ((p.pos[1] - y0).abs() < 1e-3) == ya)
                .map(|p| p.uv)
        };
        let (Some(a), Some(b), Some(d)) = (find(true, true), find(false, true), find(true, false)) else {
            continue;
        };
        for p in q.iter_mut() {
            let fx = (p.pos[0] - x0) / (x1 - x0);
            let fy = (p.pos[1] - y0) / (y1 - y0);
            p.uv = [
                a[0] + (b[0] - a[0]) * fx + (d[0] - a[0]) * fy,
                a[1] + (b[1] - a[1]) * fx + (d[1] - a[1]) * fy,
            ];
        }
    }
}
