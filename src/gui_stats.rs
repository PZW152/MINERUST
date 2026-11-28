//! `GuiStats`: the General page of the statistics (`StatList.generalStats`).

use crate::gui::Ui;
use crate::lang::Lang;
use crate::menu::widgets::SlotList;

/// How a statistic is shown (`IStatType`).
#[derive(Clone, Copy)]
enum Ty {
    Simple,
    Time,
    Distance,
    Damage,
}

/// `StatList.generalStats` in registration order.
const GENERAL: &[(&str, Ty)] = &[
    ("stat.leaveGame", Ty::Simple),
    ("stat.playOneMinute", Ty::Time),
    ("stat.walkOneCm", Ty::Distance),
    ("stat.swimOneCm", Ty::Distance),
    ("stat.fallOneCm", Ty::Distance),
    ("stat.climbOneCm", Ty::Distance),
    ("stat.flyOneCm", Ty::Distance),
    ("stat.diveOneCm", Ty::Distance),
    ("stat.minecartOneCm", Ty::Distance),
    ("stat.boatOneCm", Ty::Distance),
    ("stat.pigOneCm", Ty::Distance),
    ("stat.horseOneCm", Ty::Distance),
    ("stat.jump", Ty::Simple),
    ("stat.drop", Ty::Simple),
    ("stat.damageDealt", Ty::Damage),
    ("stat.damageTaken", Ty::Damage),
    ("stat.deaths", Ty::Simple),
    ("stat.mobKills", Ty::Simple),
    ("stat.animalsBred", Ty::Simple),
    ("stat.playerKills", Ty::Simple),
    ("stat.fishCaught", Ty::Simple),
    ("stat.junkFished", Ty::Simple),
    ("stat.treasureFished", Ty::Simple),
];

/// `NumberFormat.getIntegerInstance(Locale.US)`
fn grouped(v: i64) -> String {
    let s = v.abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if v < 0 {
        format!("-{out}")
    } else {
        out
    }
}

/// `DecimalFormat("########0.00")`
fn two(v: f64) -> String {
    format!("{v:.2}")
}

/// Java's `Double.toString` for the small values a time shows in seconds.
fn java_double(v: f64) -> String {
    if v == v.trunc() {
        format!("{v:.1}")
    } else {
        let s = format!("{v}");
        s
    }
}

fn format(ty: Ty, v: i32) -> String {
    match ty {
        Ty::Simple => grouped(v as i64),
        Ty::Time => {
            let s = v as f64 / 20.0;
            let m = s / 60.0;
            let h = m / 60.0;
            let d = h / 24.0;
            let y = d / 365.0;
            if y > 0.5 {
                two(y) + " y"
            } else if d > 0.5 {
                two(d) + " d"
            } else if h > 0.5 {
                two(h) + " h"
            } else if m > 0.5 {
                two(m) + " m"
            } else {
                java_double(s) + " s"
            }
        }
        Ty::Distance => {
            let m = v as f64 / 100.0;
            let km = m / 1000.0;
            if km > 0.5 {
                two(km) + " km"
            } else if m > 0.5 {
                two(m) + " m"
            } else {
                format!("{v} cm")
            }
        }
        Ty::Damage => two(v as f64 * 0.1),
    }
}

/// `EntityList.entityEggs` in id order: the mobs the Mobs page can list.
const MOBS: &[&str] = &[
    "Creeper", "Skeleton", "Spider", "Zombie", "Slime", "Ghast", "PigZombie", "Enderman",
    "CaveSpider", "Silverfish", "Blaze", "LavaSlime", "Bat", "Witch", "Pig", "Sheep", "Cow",
    "Chicken", "Squid", "Wolf", "MushroomCow", "Ozelot", "EntityHorse", "Villager",
];

/// `%s` and `%d` in order.
fn fill(pattern: &str, args: &[String]) -> String {
    let mut out = String::new();
    let mut it = args.iter();
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' && matches!(chars.peek(), Some('s') | Some('d')) {
            chars.next();
            out.push_str(it.next().map_or("", |s| s.as_str()));
        } else {
            out.push(c);
        }
    }
    out
}

/// `textures/gui/container/stats_icons.png` on the GUI sheet.
const ICONS: (f32, f32) = (0.0, 1280.0);

pub struct StatsScreen {
    pub list: SlotList,
    /// 0 General, 3 Mobs
    pub page: u8,
}

impl StatsScreen {
    pub fn new() -> StatsScreen {
        StatsScreen {
            list: SlotList::new(0.0, 0.0, 0.0, 0.0, 10.0),
            page: 0,
        }
    }

    /// `initGui`: the list between 32 and height - 64.
    pub fn resize(&mut self, w: f32, h: f32) {
        let amount = self.list.amount;
        let slot = match self.page {
            3 => 36.0,
            1 | 2 => 20.0,
            _ => 10.0,
        };
        self.list = SlotList::new(w, h, 32.0, h - 64.0, slot);
        if self.page == 1 || self.page == 2 {
            // setHasListHeader(true, 20)
            self.list.header_padding = 20.0;
        }
        self.list.show_selection_box = false;
        self.list.amount = amount;
    }

    /// `drawScreen`: the list, then the title.
    pub fn draw(&mut self, ui: &mut Ui, lang: &Lang, values: &[(String, i32)], m: (f32, f32), down: bool) {
        let mut events = Vec::new();
        if self.page == 1 || self.page == 2 {
            // GuiStats$StatsBlock / $StatsItem: crafted, used, mined for the
            // blocks; depleted, crafted, used for the items
            let get = |k: String| values.iter().find(|e| e.0 == k).map_or(0, |e| e.1);
            let blocks = self.page == 1;
            let ids: Vec<u16> = if blocks { (1u16..256).collect() } else { (256u16..2300).collect() };
            let rows: Vec<(u16, [i32; 3])> = ids
                .into_iter()
                .filter_map(|id| {
                    let c = get(format!("stat.craftItem.{id}"));
                    let u = get(format!("stat.useItem.{id}"));
                    let v = if blocks {
                        [c, u, get(format!("stat.mineBlock.{id}"))]
                    } else {
                        [get(format!("stat.breakItem.{id}")), c, u]
                    };
                    (v.iter().any(|&x| x > 0)).then_some((id, v))
                })
                .collect();
            let n = rows.len();
            let icons = if blocks { [18.0, 36.0, 54.0] } else { [72.0, 18.0, 36.0] };
            let x0 = (ui.w / 2.0).trunc() - 92.0 - 16.0;
            let hy = self.list.top + 4.0 - self.list.amount as i32 as f32;
            self.list.draw(ui, m, down, n, None, &mut events, &mut |ui, i, x, y, _h| {
                let (id, v) = rows[i];
                // drawStatsScreen: the slot and the item
                ui.sheet(x + 40.0 + 1.0, y + 1.0, ICONS.0, ICONS.1, 18.0, 18.0);
                crate::screens::draw_stack_with(ui, &crate::item::Stack::new(id, 0, 1), x + 40.0 + 2.0, y + 2.0, None);
                let c = if i % 2 == 0 { 0xFFFFFF } else { 0x909090 };
                for (k, col) in [115.0, 165.0, 215.0].into_iter().enumerate() {
                    let t = grouped(v[k] as i64);
                    let w = ui.text_width(&t);
                    ui.text(&t, x + col - w, y + 5.0, c, true);
                }
            });
            // drawListHeader
            if hy + 19.0 > self.list.top && hy < self.list.bottom {
                for (k, col) in [115.0, 165.0, 215.0].into_iter().enumerate() {
                    ui.sheet(x0 + col - 18.0, hy + 1.0, ICONS.0, ICONS.1 + 18.0, 18.0, 18.0);
                    ui.sheet(x0 + col - 18.0, hy + 1.0, ICONS.0 + icons[k], ICONS.1 + 18.0, 18.0, 18.0);
                }
            }
            ui.centered(lang.get("gui.stats"), ui.w / 2.0, 20.0, 0xFFFFFF);
            return;
        }
        if self.page == 3 {
            // GuiStats$StatsMobsList
            let get = |k: String| values.iter().find(|e| e.0 == k).map_or(0, |e| e.1);
            let rows: Vec<(String, i32, i32)> = MOBS
                .iter()
                .map(|n| {
                    (
                        lang.get(&format!("entity.{n}.name")).to_string(),
                        get(format!("stat.killEntity.{n}")),
                        get(format!("stat.entityKilledBy.{n}")),
                    )
                })
                .filter(|r| r.1 > 0 || r.2 > 0)
                .collect();
            let n = rows.len();
            self.list.draw(ui, m, down, n, None, &mut events, &mut |ui, i, x, y, _h| {
                let (name, kills, by) = &rows[i];
                let a = if *kills == 0 {
                    fill(lang.get("stat.entityKills.none"), &[name.clone()])
                } else {
                    fill(lang.get("stat.entityKills"), &[kills.to_string(), name.clone()])
                };
                let b = if *by == 0 {
                    fill(lang.get("stat.entityKilledBy.none"), &[name.clone()])
                } else {
                    fill(lang.get("stat.entityKilledBy"), &[name.clone(), by.to_string()])
                };
                ui.text(name, x + 2.0 - 10.0, y + 1.0, 0xFFFFFF, true);
                ui.text(&a, x + 2.0, y + 1.0 + 9.0, if *kills == 0 { 0x606060 } else { 0x909090 }, true);
                ui.text(&b, x + 2.0, y + 1.0 + 18.0, if *by == 0 { 0x606060 } else { 0x909090 }, true);
            });
            ui.centered(lang.get("gui.stats"), ui.w / 2.0, 20.0, 0xFFFFFF);
            return;
        }
        let rows: Vec<(String, String)> = GENERAL
            .iter()
            .map(|&(k, ty)| {
                let v = values.iter().find(|e| e.0 == k).map_or(0, |e| e.1);
                (lang.get(k).to_string(), format(ty, v))
            })
            .collect();
        let n = rows.len();
        self.list.draw(ui, m, down, n, None, &mut events, &mut |ui, i, x, y, _h| {
            let c = if i % 2 == 0 { 0xFFFFFF } else { 0x909090 };
            let (name, val) = &rows[i];
            ui.text(name, x + 2.0, y + 1.0, c, true);
            let w = ui.text_width(val);
            ui.text(val, x + 2.0 + 213.0 - w, y + 1.0, c, true);
        });
        ui.centered(lang.get("gui.stats"), ui.w / 2.0, 20.0, 0xFFFFFF);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_read_as_the_game_shows_them() {
        assert_eq!(format(Ty::Simple, 1234567), "1,234,567");
        assert_eq!(format(Ty::Time, 20), "1.0 s");
        assert_eq!(format(Ty::Time, 1200 * 30), "30.00 m");
        assert_eq!(format(Ty::Distance, 40), "40 cm");
        assert_eq!(format(Ty::Distance, 12345), "123.45 m");
        assert_eq!(format(Ty::Damage, 75), "7.50");
    }
}
