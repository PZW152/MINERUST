//! `GuiAchievement`: the box in the top right corner that announces a new
//! achievement, or keeps showing the inventory hint in a new world. It is
//! drawn after everything else (`Minecraft.runGameLoop` calls
//! `updateAchievementWindow` last), so F1 does not hide it and it stays over
//! open screens.

use crate::gui::Ui;
use crate::item::Stack;
use crate::lang::Lang;
use std::sync::OnceLock;

/// `Minecraft.getSystemTime`: milliseconds.
fn system_time() -> i64 {
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis() as i64
}

#[derive(Default)]
pub struct GuiAchievement {
    title: String,
    description: String,
    /// `theAchievement` (its stat id)
    achievement: Option<String>,
    notification_time: i64,
    permanent_notification: bool,
}

impl GuiAchievement {
    /// `displayAchievement`: "Achievement get!" for three seconds.
    pub fn display_achievement(&mut self, lang: &Lang, id: &str) {
        self.title = lang.get("achievement.get").to_string();
        self.description = lang.get(id).to_string();
        self.notification_time = system_time();
        self.achievement = Some(id.to_string());
        self.permanent_notification = false;
    }

    /// `displayUnformattedAchievement`: the achievement's description stays
    /// until replaced. `key` is what `Minecraft$2` formats into the
    /// inventory hint (`GameSettings.getKeyDisplayString` of the inventory key).
    pub fn display_unformatted_achievement(&mut self, lang: &Lang, id: &str, key: &str) {
        self.title = lang.get(id).to_string();
        // Achievement.getDescription: the formatter, else the plain text
        self.description = lang.format_raw(&format!("{id}.desc"), &[key.to_string()]);
        self.notification_time = system_time() + 2500;
        self.achievement = Some(id.to_string());
        self.permanent_notification = true;
    }

    /// `clearAchievements`
    pub fn clear(&mut self) {
        self.achievement = None;
        self.notification_time = 0;
    }

    /// `updateAchievementWindow`, in GUI coordinates.
    pub fn draw(&mut self, ui: &mut Ui, has_player: bool) {
        let Some(id) = &self.achievement else { return };
        if self.notification_time == 0 || !has_player {
            return;
        }
        let mut t = (system_time() - self.notification_time) as f64 / 3000.0;
        if !self.permanent_notification {
            if !(0.0..=1.0).contains(&t) {
                self.notification_time = 0;
                return;
            }
        } else if t > 0.5 {
            t = 0.5;
        }
        let mut s = t * 2.0;
        if s > 1.0 {
            s = 2.0 - s;
        }
        s *= 4.0;
        s = 1.0 - s;
        if s < 0.0 {
            s = 0.0;
        }
        s *= s;
        s *= s;
        let x = ui.w.floor() as i32 - 160;
        let y = -((s * 36.0) as i32);
        let (x, y) = (x as f32, y as f32);
        crate::menu::widgets::tex_rect(
            ui,
            "gui/achievement/achievement_background.png",
            x,
            y,
            96.0,
            202.0,
            160.0,
            32.0,
        );
        if self.permanent_notification {
            // FontRenderer.drawSplitString, 120 wide, white, no shadow,
            // FONT_HEIGHT (9) per line
            for (i, line) in ui.wrap(&self.description, 120.0).iter().enumerate() {
                ui.text(line, x + 30.0, y + 7.0 + i as f32 * 9.0, 0xFFFFFF, false);
            }
        } else {
            ui.text(&self.title, x + 30.0, y + 7.0, 0xFFFF00, false);
            ui.text(&self.description, x + 30.0, y + 18.0, 0xFFFFFF, false);
        }
        let (item, meta) = crate::stats::icon(id);
        if item != 0 {
            crate::screens::draw_stack(ui, &Stack::new(item, meta, 1), x + 8.0, y + 8.0);
        }
    }
}
