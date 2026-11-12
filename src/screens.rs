//! `GuiContainer` windows, `GuiButton`, `GuiIngameMenu`, `GuiGameOver` and the
//! survival part of `GuiIngame` (hearts, food, air, experience bar).

use crate::entity::survival::Stats;
use crate::gui::Ui;
use crate::inventory::Slot;
use crate::item::Stack;
use crate::play::{InvView, Open};

/// Sheet origins of the container backgrounds (see `Atlas::load`).
const INVENTORY: (f32, f32) = (256.0, 256.0);
const WORKBENCH: (f32, f32) = (512.0, 0.0);
const FURNACE: (f32, f32) = (512.0, 256.0);
const CHEST: (f32, f32) = (768.0, 0.0);
const OPTIONS_BG: (f32, f32) = (768.0, 256.0);

/// A clickable slot of the open window.
#[derive(Clone, Copy, Debug)]
pub struct SlotHit {
    pub x: f32,
    pub y: f32,
    pub slot: Slot,
    pub tile: Option<usize>,
}

/// Window size and slot layout (`Container` slot coordinates).
pub fn layout(view: &InvView, w: f32, h: f32) -> (f32, f32, f32, f32, Vec<SlotHit>) {
    let rows = match &view.open {
        Open::Chest(ps) => ps.len() * 3,
        _ => 0,
    };
    let (xs, ys) = match &view.open {
        Open::Chest(_) => (176.0, 114.0 + rows as f32 * 18.0),
        _ => (176.0, 166.0),
    };
    let (left, top) = (((w - xs) / 2.0).floor(), ((h - ys) / 2.0).floor());
    let mut v = Vec::new();
    let main = |v: &mut Vec<SlotHit>, y0: f32| {
        for i in 0..3 {
            for j in 0..9 {
                v.push(SlotHit {
                    x: 8.0 + j as f32 * 18.0,
                    y: y0 + i as f32 * 18.0,
                    slot: Slot::Main(9 + i * 9 + j),
                    tile: None,
                });
            }
        }
        for j in 0..9 {
            v.push(SlotHit {
                x: 8.0 + j as f32 * 18.0,
                y: y0 + 58.0,
                slot: Slot::Main(j),
                tile: None,
            });
        }
    };
    match &view.open {
        Open::Inventory | Open::None => {
            v.push(SlotHit {
                x: 144.0,
                y: 36.0,
                slot: Slot::Result,
                tile: None,
            });
            for i in 0..2 {
                for j in 0..2 {
                    v.push(SlotHit {
                        x: 88.0 + j as f32 * 18.0,
                        y: 26.0 + i as f32 * 18.0,
                        slot: Slot::Craft(i * 3 + j),
                        tile: None,
                    });
                }
            }
            main(&mut v, 84.0);
        }
        Open::Workbench(_) => {
            v.push(SlotHit {
                x: 124.0,
                y: 35.0,
                slot: Slot::Result,
                tile: None,
            });
            for i in 0..3 {
                for j in 0..3 {
                    v.push(SlotHit {
                        x: 30.0 + j as f32 * 18.0,
                        y: 17.0 + i as f32 * 18.0,
                        slot: Slot::Craft(i * 3 + j),
                        tile: None,
                    });
                }
            }
            main(&mut v, 84.0);
        }
        Open::Furnace(_) => {
            for (i, (x, y)) in [(56.0, 17.0), (56.0, 53.0), (116.0, 35.0)]
                .iter()
                .enumerate()
            {
                v.push(SlotHit {
                    x: *x,
                    y: *y,
                    slot: Slot::Outside,
                    tile: Some(i),
                });
            }
            main(&mut v, 84.0);
        }
        Open::Chest(_) => {
            for i in 0..rows {
                for j in 0..9 {
                    v.push(SlotHit {
                        x: 8.0 + j as f32 * 18.0,
                        y: 18.0 + i as f32 * 18.0,
                        slot: Slot::Outside,
                        tile: Some(i * 9 + j),
                    });
                }
            }
            main(&mut v, 103.0 + (rows as f32 - 4.0) * 18.0);
        }
    }
    (left, top, xs, ys, v)
}

fn slot_stack(view: &InvView, s: &SlotHit) -> Option<Stack> {
    if let Some(t) = s.tile {
        return view.tile_slots.get(t).copied().flatten();
    }
    match s.slot {
        Slot::Main(i) => view.inv.main[i],
        Slot::Craft(i) => view.inv.grid[i],
        Slot::Result => view.result,
        Slot::Outside => None,
    }
}

/// `GuiScreen.drawDefaultBackground` (in-game variant).
pub fn dim_background(ui: &mut Ui) {
    gradient(ui, 0.0, 0.0, ui.w, ui.h, 0xC0101010, 0xD0101010);
}

/// `Gui.drawGradientRect`
pub fn gradient(ui: &mut Ui, x: f32, y: f32, w: f32, h: f32, top: u32, bottom: u32) {
    // Two rectangles approximate the vertical blend closely enough at GUI scale.
    let steps = 16;
    for i in 0..steps {
        let t = i as f32 / (steps - 1) as f32;
        let lerp = |s: u32| -> f32 {
            let a = (top >> s & 255) as f32;
            let b = (bottom >> s & 255) as f32;
            a + (b - a) * t
        };
        let c = (lerp(16) as u32) << 16 | (lerp(8) as u32) << 8 | lerp(0) as u32;
        ui.rect(
            x,
            y + h * i as f32 / steps as f32,
            w,
            h / steps as f32 + 0.01,
            c,
            lerp(24) / 255.0,
        );
    }
}

pub fn draw_stack(ui: &mut Ui, s: &Stack, x: f32, y: f32) {
    ui.stack_icon(s, x, y);
    if s.count > 1 {
        let t = s.count.to_string();
        let tw = ui.text_width(&t);
        ui.text(&t, x + 19.0 - 2.0 - tw, y + 6.0 + 3.0, 0xFFFFFF, true);
    }
    if let Some(t) = crate::item::tool(s.id) {
        if s.meta > 0 {
            // RenderItem.renderItemOverlayIntoGUI: durability bar.
            let frac = 1.0 - s.meta as f32 / t.max_uses as f32;
            let len = (13.0 - (s.meta as f32 * 13.0 / t.max_uses as f32)).round();
            let c = (255.0 * frac).round() as u32;
            let color = ((255 - c) << 16) | (c << 8);
            ui.rect(x + 2.0, y + 13.0, 13.0, 2.0, 0x000000, 1.0);
            ui.rect(
                x + 2.0,
                y + 13.0,
                12.0,
                1.0,
                ((255 - c) / 4) << 16 | 0x3F00,
                1.0,
            );
            ui.rect(x + 2.0, y + 13.0, len, 1.0, color, 1.0);
        }
    }
}

/// Draws the open container window; returns the hovered slot.
pub fn container(ui: &mut Ui, view: &InvView, mouse: (f32, f32)) -> Option<SlotHit> {
    dim_background(ui);
    let (left, top, xs, ys, slots) = layout(view, ui.w, ui.h);
    match &view.open {
        Open::Inventory | Open::None => ui.sheet(left, top, INVENTORY.0, INVENTORY.1, xs, ys),
        Open::Workbench(_) => ui.sheet(left, top, WORKBENCH.0, WORKBENCH.1, xs, ys),
        Open::Furnace(_) => {
            ui.sheet(left, top, FURNACE.0, FURNACE.1, xs, ys);
            if let Some((burn, cook)) = view.furnace {
                if burn > 0 || view.tile_slots.first().copied().flatten().is_some() {
                    let b = burn.clamp(0, 13) as f32;
                    if burn > 0 {
                        ui.sheet(
                            left + 56.0,
                            top + 36.0 + 12.0 - b,
                            FURNACE.0 + 176.0,
                            FURNACE.1 + 12.0 - b,
                            14.0,
                            b + 2.0,
                        );
                    }
                }
                ui.sheet(
                    left + 79.0,
                    top + 34.0,
                    FURNACE.0 + 176.0,
                    FURNACE.1 + 14.0,
                    cook.clamp(0, 24) as f32 + 1.0,
                    16.0,
                );
            }
        }
        Open::Chest(_) => {
            let rows = view.tile_slots.len() / 9;
            ui.sheet(left, top, CHEST.0, CHEST.1, xs, rows as f32 * 18.0 + 17.0);
            ui.sheet(
                left,
                top + rows as f32 * 18.0 + 17.0,
                CHEST.0,
                CHEST.1 + 126.0,
                xs,
                96.0,
            );
        }
    }
    // Titles (drawGuiContainerForegroundLayer), colour 0x404040.
    let fg = 0x404040;
    match &view.open {
        Open::Inventory | Open::None => ui.text("Crafting", left + 86.0, top + 16.0, fg, false),
        Open::Workbench(_) => {
            ui.text("Crafting", left + 28.0, top + 6.0, fg, false);
            ui.text("Inventory", left + 8.0, top + ys - 96.0 + 2.0, fg, false);
        }
        Open::Furnace(_) => {
            let t = "Furnace";
            ui.text(
                t,
                left + xs / 2.0 - ui.text_width(t) / 2.0,
                top + 6.0,
                fg,
                false,
            );
            ui.text("Inventory", left + 8.0, top + ys - 96.0 + 2.0, fg, false);
        }
        Open::Chest(ps) => {
            ui.text(
                if ps.len() > 1 { "Large chest" } else { "Chest" },
                left + 8.0,
                top + 6.0,
                fg,
                false,
            );
            ui.text("Inventory", left + 8.0, top + ys - 96.0 + 2.0, fg, false);
        }
    }
    let mut hovered = None;
    for s in &slots {
        let (x, y) = (left + s.x, top + s.y);
        if let Some(st) = slot_stack(view, s) {
            draw_stack(ui, &st, x, y);
        }
        if mouse.0 >= x - 1.0 && mouse.0 < x + 17.0 && mouse.1 >= y - 1.0 && mouse.1 < y + 17.0 {
            ui.rect(x, y, 16.0, 16.0, 0xFFFFFF, 0.5);
            hovered = Some(*s);
        }
    }
    if let Some(c) = view.inv.cursor {
        draw_stack(ui, &c, mouse.0 - 8.0, mouse.1 - 8.0);
    } else if let Some(h) = hovered {
        if let Some(st) = slot_stack(view, &h) {
            tooltip(ui, &crate::item::display_name(&st), mouse.0, mouse.1);
        }
    }
    hovered
}

/// `GuiContainer.drawHoveringText` for one line.
pub fn tooltip(ui: &mut Ui, text: &str, mx: f32, my: f32) {
    let w = ui.text_width(text);
    let (mut x, mut y) = (mx + 12.0, my - 12.0);
    if x + w > ui.w {
        x -= 28.0 + w;
    }
    if y + 8.0 + 6.0 > ui.h {
        y = ui.h - 8.0 - 6.0;
    }
    let bg = 0xF0100010u32;
    let (h, a) = (8.0, ((bg >> 24) & 255) as f32 / 255.0);
    ui.rect(x - 3.0, y - 4.0, w + 6.0, 1.0, bg & 0xFFFFFF, a);
    ui.rect(x - 3.0, y + h + 3.0, w + 6.0, 1.0, bg & 0xFFFFFF, a);
    ui.rect(x - 3.0, y - 3.0, w + 6.0, h + 6.0, bg & 0xFFFFFF, a);
    ui.rect(x - 4.0, y - 3.0, 1.0, h + 6.0, bg & 0xFFFFFF, a);
    ui.rect(x + w + 3.0, y - 3.0, 1.0, h + 6.0, bg & 0xFFFFFF, a);
    gradient(ui, x - 3.0, y - 2.0, 1.0, h + 4.0, 0x505000FF, 0x5028007F);
    gradient(
        ui,
        x + w + 2.0,
        y - 2.0,
        1.0,
        h + 4.0,
        0x505000FF,
        0x5028007F,
    );
    ui.rect(x - 3.0, y - 3.0, w + 6.0, 1.0, 0x5000FF, 0.31);
    ui.rect(x - 3.0, y + h + 2.0, w + 6.0, 1.0, 0x28007F, 0.31);
    ui.text(text, x, y, 0xFFFFFF, true);
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
    let left = w / 2.0 - 91.0;
    let right = w / 2.0 + 91.0;
    let top = h - 39.0;
    let health = s.health.ceil() as i32;
    let flash = s.hurt_resistance >= 10 && s.hurt_resistance / 3 % 2 == 1;
    for i in 0..10 {
        let x = left + i as f32 * 8.0;
        let mut y = top;
        if health <= 4 {
            y += jitter.next_int_n(2) as f32;
        }
        ui.icon(x, y, 16.0 + if flash { 9.0 } else { 0.0 }, 0.0, 9.0, 9.0);
        let base = if flash { 54.0 } else { 36.0 };
        if i * 2 + 1 < health {
            ui.icon(x, y, 16.0 + base, 0.0, 9.0, 9.0);
        } else if i * 2 + 1 == health {
            ui.icon(x, y, 16.0 + base + 9.0, 0.0, 9.0, 9.0);
        }
    }
    for i in 0..10 {
        let x = right - i as f32 * 8.0 - 9.0;
        let mut y = top;
        if s.saturation <= 0.0 && tick % (s.food as u32 * 3 + 1) == 0 {
            y += (jitter.next_int_n(3) - 1) as f32;
        }
        ui.icon(x, y, 16.0, 27.0, 9.0, 9.0);
        if i * 2 + 1 < s.food {
            ui.icon(x, y, 52.0, 27.0, 9.0, 9.0);
        } else if i * 2 + 1 == s.food {
            ui.icon(x, y, 61.0, 27.0, 9.0, 9.0);
        }
    }
    if underwater && s.air < 300 {
        let full = ((s.air - 2) as f32 * 10.0 / 300.0).ceil() as i32;
        let popping = (s.air as f32 * 10.0 / 300.0).ceil() as i32 - full;
        for i in 0..(full + popping) {
            let x = right - i as f32 * 8.0 - 9.0;
            ui.icon(
                x,
                h - 49.0,
                if i < full { 16.0 } else { 25.0 },
                18.0,
                9.0,
                9.0,
            );
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
