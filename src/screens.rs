//! `GuiContainer` windows, `GuiButton`, `GuiIngameMenu`, `GuiGameOver` and the
//! survival part of `GuiIngame` (hearts, food, air, experience bar).

use crate::entity::survival::Stats;
use crate::gui::Ui;
use crate::item::Stack;

const OPTIONS_BG: (f32, f32) = (768.0, 256.0);

pub use crate::gui_container::EffectLine;

/// `GuiScreen.drawDefaultBackground` (in-game variant).
pub fn dim_background(ui: &mut Ui) {
    gradient(ui, 0.0, 0.0, ui.w, ui.h, 0xC0101010, 0xD0101010);
}

/// `Gui.drawGradientRect`: ARGB colours at the top and bottom edge.
pub fn gradient(ui: &mut Ui, x: f32, y: f32, w: f32, h: f32, top: u32, bottom: u32) {
    ui.gradient_rect(x, y, w, h, top, bottom);
}

pub fn draw_stack(ui: &mut Ui, s: &Stack, x: f32, y: f32) {
    draw_stack_with(ui, s, x, y, None);
}

/// `RenderItem.renderItemAndEffectIntoGUI` and `renderItemOverlayIntoGUI`
/// with its optional count text.
pub fn draw_stack_with(ui: &mut Ui, s: &Stack, x: f32, y: f32, label: Option<&str>) {
    ui.stack_icon(s, x, y);
    if s.count > 1 || label.is_some() {
        let t = label.map_or_else(|| s.count.to_string(), |l| l.to_string());
        let tw = ui.text_width(&t);
        ui.text(&t, x + 19.0 - 2.0 - tw, y + 6.0 + 3.0, 0xFFFFFF, true);
    }
    let md = crate::item::max_damage(s.id);
    if md > 0 && s.meta > 0 {
        // the durability bar
        let d = s.meta as f64;
        let len = (13.0 - d * 13.0 / md as f64).round() as f32;
        let k = (255.0 - d * 255.0 / md as f64).round() as i32;
        let color = ((255 - k) << 16 | k << 8) as u32;
        let shade = (((255 - k) / 4) << 16 | 16128) as u32;
        ui.rect(x + 2.0, y + 13.0, 13.0, 2.0, 0x000000, 1.0);
        ui.rect(x + 2.0, y + 13.0, 12.0, 1.0, shade, 1.0);
        ui.rect(x + 2.0, y + 13.0, len, 1.0, color, 1.0);
    }
}

/// `GuiScreen.renderToolTip`'s lines.
pub fn tooltip_lines(s: &Stack, lang: &crate::lang::Lang) -> Vec<String> {
    crate::item_text::render_lines(s, lang)
}

/// `GuiScreen.drawHoveringText`
pub fn hovering_text(ui: &mut Ui, lines: &[String], mx: i32, my: i32) {
    if lines.is_empty() {
        return;
    }
    let w = lines
        .iter()
        .map(|l| ui.text_width(l) as i32)
        .max()
        .unwrap_or(0);
    let mut x = mx + 12;
    let mut y = my - 12;
    let mut h = 8;
    if lines.len() > 1 {
        h += 2 + (lines.len() as i32 - 1) * 10;
    }
    if x + w > ui.w as i32 {
        x -= 28 + w;
    }
    if y + h + 6 > ui.h as i32 {
        y = ui.h as i32 - h - 6;
    }
    let g = |ui: &mut Ui, x0: i32, y0: i32, x1: i32, y1: i32, a: u32, b: u32| {
        gradient(
            ui,
            x0 as f32,
            y0 as f32,
            (x1 - x0) as f32,
            (y1 - y0) as f32,
            a,
            b,
        );
    };
    let bg = 0xF0100010u32;
    g(ui, x - 3, y - 4, x + w + 3, y - 3, bg, bg);
    g(ui, x - 3, y + h + 3, x + w + 3, y + h + 4, bg, bg);
    g(ui, x - 3, y - 3, x + w + 3, y + h + 3, bg, bg);
    g(ui, x - 4, y - 3, x - 3, y + h + 3, bg, bg);
    g(ui, x + w + 3, y - 3, x + w + 4, y + h + 3, bg, bg);
    let b0 = 0x505000FFu32;
    let b1 = (b0 & 0xFEFEFE) >> 1 | b0 & 0xFF000000;
    g(ui, x - 3, y - 3 + 1, x - 3 + 1, y + h + 3 - 1, b0, b1);
    g(ui, x + w + 2, y - 3 + 1, x + w + 3, y + h + 3 - 1, b0, b1);
    g(ui, x - 3, y - 3, x + w + 3, y - 3 + 1, b0, b0);
    g(ui, x - 3, y + h + 2, x + w + 3, y + h + 3, b1, b1);
    let mut ty = y;
    for (i, l) in lines.iter().enumerate() {
        ui.text(l, x as f32, ty as f32, 0xFFFFFF, true);
        if i == 0 {
            ty += 2;
        }
        ty += 10;
    }
}

/// `GuiButton.drawButton`; returns whether the mouse is over it.
pub fn button(
    ui: &mut Ui,
    x: f32,
    y: f32,
    w: f32,
    label: &str,
    enabled: bool,
    mouse: (f32, f32),
) -> bool {
    let hover = mouse.0 >= x && mouse.1 >= y && mouse.0 < x + w && mouse.1 < y + 20.0;
    let k = if !enabled {
        0.0
    } else if hover {
        2.0
    } else {
        1.0
    };
    ui.widget(x, y, 0.0, 46.0 + k * 20.0, w / 2.0, 20.0);
    ui.widget(
        x + w / 2.0,
        y,
        200.0 - w / 2.0,
        46.0 + k * 20.0,
        w / 2.0,
        20.0,
    );
    let color = if !enabled {
        0xA0A0A0
    } else if hover {
        0xFFFFA0
    } else {
        0xE0E0E0
    };
    let tw = ui.text_width(label);
    ui.text(
        label,
        x + w / 2.0 - tw / 2.0,
        y + (20.0 - 8.0) / 2.0,
        color,
        true,
    );
    hover && enabled
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuAction {
    BackToGame,
    SaveAndQuit,
    Respawn,
}

/// `GuiIngameMenu`
pub fn pause_menu(ui: &mut Ui, mouse: (f32, f32)) -> Option<MenuAction> {
    dim_background(ui);
    let (w, h) = (ui.w, ui.h);
    let t = "Game menu";
    ui.text(t, w / 2.0 - ui.text_width(t) / 2.0, 40.0, 0xFFFFFF, true);
    let b = -16.0;
    let mut hit = None;
    if button(
        ui,
        w / 2.0 - 100.0,
        h / 4.0 + 24.0 + b,
        200.0,
        "Back to Game",
        true,
        mouse,
    ) {
        hit = Some(MenuAction::BackToGame);
    }
    button(
        ui,
        w / 2.0 - 100.0,
        h / 4.0 + 48.0 + b,
        98.0,
        "Achievements",
        false,
        mouse,
    );
    button(
        ui,
        w / 2.0 + 2.0,
        h / 4.0 + 48.0 + b,
        98.0,
        "Statistics",
        false,
        mouse,
    );
    button(
        ui,
        w / 2.0 - 100.0,
        h / 4.0 + 96.0 + b,
        98.0,
        "Options...",
        false,
        mouse,
    );
    button(
        ui,
        w / 2.0 + 2.0,
        h / 4.0 + 96.0 + b,
        98.0,
        "Open to LAN",
        false,
        mouse,
    );
    if button(
        ui,
        w / 2.0 - 100.0,
        h / 4.0 + 120.0 + b,
        200.0,
        "Save and Quit to Title",
        true,
        mouse,
    ) {
        hit = Some(MenuAction::SaveAndQuit);
    }
    hit
}

/// `GuiGameOver`
pub fn death_screen(ui: &mut Ui, stats: &Stats, mouse: (f32, f32)) -> Option<MenuAction> {
    let (w, h) = (ui.w, ui.h);
    gradient(ui, 0.0, 0.0, w, h, 0x60500000, 0xA0803030);
    let title = "Game over!";
    ui.text_scaled(
        title,
        w / 2.0 / 2.0 - ui.text_width(title) / 2.0,
        30.0,
        2.0,
        0xFFFFFF,
    );
    let msg: &str = if stats.death_text.is_empty() {
        stats.death_message()
    } else {
        &stats.death_text
    };
    ui.text(
        msg,
        w / 2.0 - ui.text_width(msg) / 2.0,
        85.0,
        0xFFFFFF,
        true,
    );
    let score = "Score: 0";
    ui.text(
        score,
        w / 2.0 - ui.text_width(score) / 2.0,
        100.0,
        0xFFFFFF,
        true,
    );
    let enabled = stats.death_time >= 20;
    let mut hit = None;
    if button(
        ui,
        w / 2.0 - 100.0,
        h / 4.0 + 72.0,
        200.0,
        "Respawn",
        enabled,
        mouse,
    ) {
        hit = Some(MenuAction::Respawn);
    }
    if button(
        ui,
        w / 2.0 - 100.0,
        h / 4.0 + 96.0,
        200.0,
        "Title screen",
        enabled,
        mouse,
    ) {
        hit = Some(MenuAction::SaveAndQuit);
    }
    hit
}

/// Hearts, food, air and the experience bar (`GuiIngame.func_110327_a`).
/// `tick` is the GUI update counter, `jitter` a per-heart random source.
pub fn survival_bars(
    ui: &mut Ui,
    s: &Stats,
    tick: u32,
    jitter: &mut crate::java::JavaRandom,
    underwater: bool,
) {
    let (w, h) = (ui.w, ui.h);
    // Experience bar (GuiIngame.renderTooltip: the xp part).
    ui.icon(w / 2.0 - 91.0, h - 32.0 + 3.0, 0.0, 64.0, 182.0, 5.0);
    let filled = (s.xp * 183.0) as i32;
    if filled > 0 {
        ui.icon(
            w / 2.0 - 91.0,
            h - 32.0 + 3.0,
            0.0,
            69.0,
            filled as f32,
            5.0,
        );
    }
    if s.xp_level > 0 {
        let t = s.xp_level.to_string();
        let x = (w - ui.text_width(&t)) / 2.0;
        let y = h - 31.0 - 4.0;
        for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
            ui.text(&t, x + dx, y + dy, 0x000000, false);
        }
        ui.text(&t, x, y, 0x80FF20, false);
    }
    render_icons(ui, s, tick, jitter, underwater);
}

/// `GuiIngame.renderIcons`: armour, health (with poison, wither,
/// absorption, regeneration and Health Boost rows), food (hunger) and air.
fn render_icons(
    ui: &mut Ui,
    s: &Stats,
    tick: u32,
    rand: &mut crate::java::JavaRandom,
    underwater: bool,
) {
    use crate::potion;
    use crate::util::math::ceiling_float_int;
    let (w, h) = (ui.w as i32, ui.h as i32);
    let active = |id: i32| s.effects.iter().any(|e| e.id == id);
    let mut flash = s.hurt_resistance / 3 % 2 == 1;
    if s.hurt_resistance < 10 {
        flash = false;
    }
    let health = ceiling_float_int(s.health);
    // prevHealth is only set by the server's attackEntityFrom: 0 here
    let prev_health = 0;
    let food = s.food;
    let left = w / 2 - 91;
    let right = w / 2 + 91;
    let top = h - 39;
    let max = s.max_health;
    let absorption = s.absorption;
    let rows = ceiling_float_int((max + absorption) / 2.0 / 10.0);
    let step = (10 - (rows - 2)).max(3);
    let armor_y = top - (rows - 1) * step - 10;
    let mut left_abs = absorption;
    let armor = 0;
    let mut regen = -1;
    if active(potion::REGENERATION) {
        regen = tick as i32 % ceiling_float_int(max + 5.0);
    }
    let icon = |ui: &mut Ui, x: i32, y: i32, u: i32, v: i32| {
        ui.icon(x as f32, y as f32, u as f32, v as f32, 9.0, 9.0);
    };
    for i in 0..10 {
        if armor > 0 {
            let x = left + i * 8;
            if i * 2 + 1 < armor {
                icon(ui, x, armor_y, 34, 9);
            }
            if i * 2 + 1 == armor {
                icon(ui, x, armor_y, 25, 9);
            }
            if i * 2 + 1 > armor {
                icon(ui, x, armor_y, 16, 9);
            }
        }
    }
    let mut i = ceiling_float_int((max + absorption) / 2.0) - 1;
    while i >= 0 {
        let mut u = 16;
        if active(potion::POISON) {
            u += 36;
        } else if active(potion::WITHER) {
            u += 72;
        }
        let f = if flash { 1 } else { 0 };
        let row = ceiling_float_int((i + 1) as f32 / 10.0) - 1;
        let x = left + i % 10 * 8;
        let mut y = top - row * step;
        if health <= 4 {
            y += rand.next_int_n(2);
        }
        if i == regen {
            y -= 2;
        }
        let hc = 0;
        icon(ui, x, y, 16 + f * 9, 9 * hc);
        if flash {
            if i * 2 + 1 < prev_health {
                icon(ui, x, y, u + 54, 9 * hc);
            }
            if i * 2 + 1 == prev_health {
                icon(ui, x, y, u + 63, 9 * hc);
            }
        }
        if left_abs > 0.0 {
            if left_abs == absorption && absorption % 2.0 == 1.0 {
                icon(ui, x, y, u + 153, 9 * hc);
            } else {
                icon(ui, x, y, u + 144, 9 * hc);
            }
            left_abs -= 2.0;
        } else {
            if i * 2 + 1 < health {
                icon(ui, x, y, u + 36, 9 * hc);
            }
            if i * 2 + 1 == health {
                icon(ui, x, y, u + 45, 9 * hc);
            }
        }
        i -= 1;
    }
    for i in 0..10 {
        let mut y = top;
        let mut u = 16;
        let mut bg = 0;
        if active(potion::HUNGER) {
            u += 36;
            bg = 13;
        }
        if s.saturation <= 0.0 && tick % (food as u32 * 3 + 1) == 0 {
            y += rand.next_int_n(3) - 1;
        }
        let x = right - i * 8 - 9;
        icon(ui, x, y, 16 + bg * 9, 27);
        if i * 2 + 1 < food {
            icon(ui, x, y, u + 36, 27);
        }
        if i * 2 + 1 == food {
            icon(ui, x, y, u + 45, 27);
        }
    }
    if underwater {
        let air = s.air;
        let full = ((air - 2) as f64 * 10.0 / 300.0).ceil() as i32;
        let popping = (air as f64 * 10.0 / 300.0).ceil() as i32 - full;
        for i in 0..(full + popping) {
            if i < full {
                icon(ui, right - i * 8 - 9, armor_y, 16, 18);
            } else {
                icon(ui, right - i * 8 - 9, armor_y, 25, 18);
            }
        }
    }
}

/// The dirt background of the loading screen (`drawBackground`).
pub fn dirt_background(ui: &mut Ui) {
    let mut y = 0.0;
    while y < ui.h {
        let mut x = 0.0;
        while x < ui.w {
            ui.sheet_tinted(
                x,
                y,
                OPTIONS_BG.0,
                OPTIONS_BG.1,
                32.0,
                32.0,
                16.0,
                16.0,
                0x404040,
            );
            x += 32.0;
        }
        y += 32.0;
    }
}

/// `GuiIngame.renderTooltip`: the hotbar with its items.
pub fn hotbar(ui: &mut Ui, inv: &crate::inventory::Inventory) {
    let (w, h) = (ui.w, ui.h);
    ui.widget(w / 2.0 - 91.0, h - 22.0, 0.0, 0.0, 182.0, 22.0);
    ui.widget(
        w / 2.0 - 91.0 - 1.0 + inv.selected as f32 * 20.0,
        h - 22.0 - 1.0,
        0.0,
        22.0,
        24.0,
        22.0,
    );
    for i in 0..9 {
        if let Some(s) = inv.main[i] {
            draw_stack(
                ui,
                &s,
                w / 2.0 - 90.0 + i as f32 * 20.0 + 2.0,
                h - 16.0 - 3.0,
            );
        }
    }
}
