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

pub struct StatsScreen {
    pub list: SlotList,
}

impl StatsScreen {
    pub fn new() -> StatsScreen {
        StatsScreen {
            list: SlotList::new(0.0, 0.0, 0.0, 0.0, 10.0),
        }
    }

    /// `initGui`: the list between 32 and height - 64.
    pub fn resize(&mut self, w: f32, h: f32) {
        let amount = self.list.amount;
        self.list = SlotList::new(w, h, 32.0, h - 64.0, 10.0);
        self.list.show_selection_box = false;
        self.list.amount = amount;
    }

    /// `drawScreen`: the list, then the title.
    pub fn draw(&mut self, ui: &mut Ui, lang: &Lang, values: &[(String, i32)], m: (f32, f32), down: bool) {
        let mut events = Vec::new();
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
