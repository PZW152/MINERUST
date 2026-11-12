//! `GuiContainer` and its subclasses (`GuiInventory`, `GuiCrafting`,
//! `GuiFurnace`, `GuiBrewingStand`, `GuiChest`, `GuiDispenser`): drawing,
//! slot hover, and the mouse and key handling that turns into
//! `windowClick(slot, button, mode)` calls.

use crate::container::{self, SlotDef, SlotKind, Window};
use crate::gui::Ui;
use crate::item::{max_stack, Stack};
use crate::lang::Lang;
use crate::play::{InvView, Open};
use crate::screens::{dim_background, draw_stack_with};

/// Sheet origins of the container backgrounds (see `Atlas::load`).
const INVENTORY: (f32, f32) = (256.0, 256.0);
const WORKBENCH: (f32, f32) = (512.0, 0.0);
const FURNACE: (f32, f32) = (512.0, 256.0);
const CHEST: (f32, f32) = (768.0, 0.0);
const BREWING: (f32, f32) = (256.0, 512.0);
const DISPENSER: (f32, f32) = (1792.0, 512.0);

/// One active effect as `InventoryEffectRenderer` draws it: status icon
/// index, the name line and the duration line.
pub type EffectLine = (i32, String, String);

/// A `windowClick` the screen asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Click {
    pub slot: i32,
    pub button: i32,
    pub mode: i32,
}

/// The per-screen state of `GuiContainer`.
#[derive(Clone, Debug, Default)]
pub struct GuiContainer {
    /// `theSlot`: the slot under the mouse at the last draw.
    pub hovered: Option<usize>,
    /// `lastClickSlot`, `lastClickTime`, `lastClickButton`
    last_slot: Option<Option<usize>>,
    last_time: u64,
    last_button: i32,
    double_click: bool,
    ignore_mouse_up: bool,
    shift_clicked: Option<Stack>,
    drag_splitting: bool,
    drag_button: i32,
    drag_limit: i32,
    drag_slots: Vec<usize>,
    drag_remnant: u32,
    /// `GuiScreen.eventButton`: the button held for `mouseClickMove`.
    held: Option<i32>,
}

/// `xSize`, `ySize` of the window's screen.
pub fn size(open: &Open) -> (i32, i32) {
    match container_window(open) {
        Window::Chest(rows) => (176, 114 + rows as i32 * 18),
        _ => (176, 166),
    }
}

pub fn container_window(open: &Open) -> Window {
    crate::play::window_of(open)
}

/// `guiLeft`, `guiTop`; `InventoryEffectRenderer` moves the player's
/// inventory right while effects are active.
pub fn origin(open: &Open, w: i32, h: i32, effects: bool) -> (i32, i32) {
    let (xs, ys) = size(open);
    let mut left = (w - xs) / 2;
    let top = (h - ys) / 2;
    if effects && matches!(open, Open::Inventory | Open::None) {
        left = 160 + (w - xs - 200) / 2;
    }
    (left, top)
}

/// `Slot.getStack` on the client's copy of the window.
pub fn slot_stack(view: &InvView, d: &SlotDef) -> Option<Stack> {
    use container::Inv;
    match d.inv {
        Inv::Player => {
            if d.index < 36 {
                view.inv.main[d.index]
            } else {
                view.inv.armor[d.index - 36]
            }
        }
        Inv::Craft => view.inv.grid[d.index],
        Inv::Result => view.result,
        Inv::Tile => view.tile_slots.get(d.index).copied().flatten(),
    }
}

fn slot_limit(d: &SlotDef) -> u32 {
    match d.kind {
        SlotKind::Armor(_) | SlotKind::Potion => 1,
        _ => 64,
    }
}

fn slot_valid(d: &SlotDef, s: &Stack) -> bool {
    match d.kind {
        SlotKind::Plain => true,
        SlotKind::CraftResult | SlotKind::FurnaceOut => false,
        SlotKind::Armor(t) => match container::armor_type(s.id) {
            Some(a) => a == t,
            None => (s.id == 86 || s.id == 397) && t == 0,
        },
        SlotKind::Potion => container::can_hold_potion(s),
        SlotKind::Ingredient => crate::play::is_potion_ingredient(s),
    }
}

/// `Container.canAddItemToSlot`
fn can_add(view: &InvView, d: Option<&SlotDef>, s: Option<&Stack>, ignore_size: bool) -> bool {
    let cur = d.and_then(|d| slot_stack(view, d));
    let mut ok = cur.is_none();
    if let (Some(c), Some(s)) = (cur, s) {
        if s.id == c.id && s.meta == c.meta && s.ench == c.ench {
            let add = if ignore_size { 0 } else { s.count };
            ok |= c.count + add <= max_stack(s.id);
        }
    }
    ok
}

fn over(d: &SlotDef, left: i32, top: i32, mx: i32, my: i32) -> bool {
    let (x, y) = (mx - left, my - top);
    x >= d.x - 1 && x < d.x + 16 + 1 && y >= d.y - 1 && y < d.y + 16 + 1
}

/// The `key.hotbar.N`, `key.drop`, `key.pickItem` and `key.inventory`
/// codes the screen reacts to.
pub struct Keys {
    pub inventory: i32,
    pub drop: i32,
    pub pick: i32,
    pub hotbar: [i32; 9],
}

pub struct Frame<'a> {
    pub view: &'a InvView,
    pub w: i32,
    pub h: i32,
    pub effects: bool,
    pub mouse: (i32, i32),
}

impl GuiContainer {
    fn geometry(f: &Frame) -> (Vec<SlotDef>, i32, i32, i32, i32) {
        let slots = container_window(&f.view.open).slots();
        let (left, top) = origin(&f.view.open, f.w, f.h, f.effects);
        let (xs, ys) = size(&f.view.open);
        (slots, left, top, xs, ys)
    }

    /// `getSlotAtPosition`
    fn slot_at(slots: &[SlotDef], left: i32, top: i32, mx: i32, my: i32) -> Option<usize> {
        slots.iter().position(|d| over(d, left, top, mx, my))
    }

    /// `updateDragSplitting`
    fn update_drag(&mut self, view: &InvView, slots: &[SlotDef]) {
        let Some(cur) = view.inv.cursor else { return };
        if !self.drag_splitting {
            return;
        }
        let mut rem = cur.count as i64;
        for &i in &self.drag_slots {
            let mut s = cur;
            let have = slot_stack(view, &slots[i]).map_or(0, |s| s.count);
            container::compute_stack_size(self.drag_slots.len(), self.drag_limit, &mut s, have);
            s.count = s.count.min(max_stack(s.id)).min(slot_limit(&slots[i]));
            rem -= s.count as i64 - have as i64;
        }
        self.drag_remnant = rem.max(0) as u32;
    }

    /// `GuiContainer.mouseClicked`
    pub fn mouse_clicked(
        &mut self,
        f: &Frame,
        button: i32,
        shift: bool,
        now_ms: u64,
        keys: &Keys,
        send: &mut dyn FnMut(Click) -> InvView,
    ) {
        self.held = Some(button);
        let (slots, left, top, xs, ys) = Self::geometry(f);
        let (mx, my) = f.mouse;
        let pick = button == keys.pick + 100;
        let hit = Self::slot_at(&slots, left, top, mx, my);
        self.double_click = self.last_slot == Some(hit)
            && now_ms.wrapping_sub(self.last_time) < 250
            && self.last_button == button;
        self.ignore_mouse_up = false;
        if button == 0 || button == 1 || pick {
            let outside = mx < left || my < top || mx >= left + xs || my >= top + ys;
            let mut n = hit.map_or(-1, |i| i as i32);
            if outside {
                n = -999;
            }
            if n != -1 && !self.drag_splitting {
                if f.view.inv.cursor.is_none() {
                    if pick {
                        send(Click {
                            slot: n,
                            button,
                            mode: 3,
                        });
                    } else {
                        let sh = n != -999 && shift;
                        let mut mode = 0;
                        if sh {
                            self.shift_clicked = hit.and_then(|i| slot_stack(f.view, &slots[i]));
                            mode = 1;
                        } else if n == -999 {
                            mode = 4;
                        }
                        send(Click {
                            slot: n,
                            button,
                            mode,
                        });
                    }
                    self.ignore_mouse_up = true;
                } else {
                    self.drag_splitting = true;
                    self.drag_button = button;
                    self.drag_slots.clear();
                    if button == 0 {
                        self.drag_limit = 0;
                    } else if button == 1 {
                        self.drag_limit = 1;
                    }
                }
            }
        }
        self.last_slot = Some(hit);
        self.last_time = now_ms;
        self.last_button = button;
    }

    /// `GuiContainer.mouseClickMove`
    pub fn mouse_dragged(&mut self, f: &Frame) {
        if self.held.is_none() {
            return;
        }
        let (slots, left, top, _, _) = Self::geometry(f);
        let (mx, my) = f.mouse;
        let Some(i) = Self::slot_at(&slots, left, top, mx, my) else {
            return;
        };
        let Some(cur) = f.view.inv.cursor else { return };
        if self.drag_splitting
            && cur.count as usize > self.drag_slots.len()
            && can_add(f.view, Some(&slots[i]), Some(&cur), true)
            && slot_valid(&slots[i], &cur)
            && !self.drag_slots.contains(&i)
        {
            self.drag_slots.push(i);
            self.update_drag(f.view, &slots);
        }
    }

    /// `GuiContainer.mouseMovedOrUp` for a released button.
    pub fn mouse_released(
        &mut self,
        f: &Frame,
        button: i32,
        shift: bool,
        keys: &Keys,
        send: &mut dyn FnMut(Click) -> InvView,
    ) {
        self.held = None;
        let (slots, left, top, xs, ys) = Self::geometry(f);
        let (mx, my) = f.mouse;
        let hit = Self::slot_at(&slots, left, top, mx, my);
        let outside = mx < left || my < top || mx >= left + xs || my >= top + ys;
        let mut n = hit.map_or(-1, |i| i as i32);
        if outside {
            n = -999;
        }
        let mut view = f.view.clone();
        // canMergeSlot: not the crafting result
        let mergeable = hit.is_some_and(|i| slots[i].inv != container::Inv::Result);
        if self.double_click && hit.is_some() && button == 0 && mergeable {
            let h = hit.unwrap();
            if shift {
                if let Some(sc) = self.shift_clicked {
                    for (k, d) in slots.iter().enumerate() {
                        if slot_stack(&view, d).is_some()
                            && d.inv == slots[h].inv
                            && can_add(&view, Some(d), Some(&sc), true)
                        {
                            view = send(Click {
                                slot: k as i32,
                                button,
                                mode: 1,
                            });
                        }
                    }
                }
            } else {
                view = send(Click {
                    slot: n,
                    button,
                    mode: 6,
                });
            }
            self.double_click = false;
            self.last_time = 0;
        } else {
            if self.drag_splitting && self.drag_button != button {
                self.drag_splitting = false;
                self.drag_slots.clear();
                self.ignore_mouse_up = true;
                return;
            }
            if self.ignore_mouse_up {
                self.ignore_mouse_up = false;
                return;
            }
            if self.drag_splitting && !self.drag_slots.is_empty() {
                let lim = self.drag_limit;
                send(Click {
                    slot: -999,
                    button: container::quickcraft_mask(0, lim),
                    mode: 5,
                });
                for &i in &self.drag_slots.clone() {
                    send(Click {
                        slot: i as i32,
                        button: container::quickcraft_mask(1, lim),
                        mode: 5,
                    });
                }
                view = send(Click {
                    slot: -999,
                    button: container::quickcraft_mask(2, lim),
                    mode: 5,
                });
            } else if view.inv.cursor.is_some() {
                if button == keys.pick + 100 {
                    view = send(Click {
                        slot: n,
                        button,
                        mode: 3,
                    });
                } else {
                    let sh = n != -999 && shift;
                    if sh {
                        self.shift_clicked = hit.and_then(|i| slot_stack(&view, &slots[i]));
                    }
                    view = send(Click {
                        slot: n,
                        button,
                        mode: if sh { 1 } else { 0 },
                    });
                }
            }
        }
        if view.inv.cursor.is_none() {
            self.last_time = 0;
        }
        self.drag_splitting = false;
    }

    /// `GuiContainer.keyTyped`; true when the screen closes.
    pub fn key_typed(
        &mut self,
        f: &Frame,
        code: i32,
        ctrl: bool,
        keys: &Keys,
        send: &mut dyn FnMut(Click) -> InvView,
    ) -> bool {
        if code == 1 || code == keys.inventory {
            return true;
        }
        let slots = container_window(&f.view.open).slots();
        // checkHotbarKeys
        if f.view.inv.cursor.is_none() {
            if let Some(h) = self.hovered {
                if let Some(k) = keys.hotbar.iter().position(|&c| c == code) {
                    send(Click {
                        slot: h as i32,
                        button: k as i32,
                        mode: 2,
                    });
                    return false;
                }
            }
        }
        if let Some(h) = self.hovered {
            if slot_stack(f.view, &slots[h]).is_some() {
                if code == keys.pick {
                    send(Click {
                        slot: h as i32,
                        button: 0,
                        mode: 3,
                    });
                } else if code == keys.drop {
                    send(Click {
                        slot: h as i32,
                        button: if ctrl { 1 } else { 0 },
                        mode: 4,
                    });
                }
            }
        }
        false
    }

    /// `GuiContainer.drawScreen`; returns the tooltip lines to draw.
    pub fn draw(&mut self, ui: &mut Ui, f: &Frame, lang: &Lang, effects: &[EffectLine]) {
        let view = f.view;
        let (slots, left, top, xs, ys) = Self::geometry(f);
        let (lf, tf) = (left as f32, top as f32);
        dim_background(ui);
        if !effects.is_empty() && matches!(view.open, Open::Inventory | Open::None) {
            draw_effects(ui, lf, tf, effects);
        }
        background(ui, view, lf, tf, xs as f32, ys as f32);
        // the slots
        self.hovered = None;
        let cursor = view.inv.cursor;
        for (i, d) in slots.iter().enumerate() {
            let (x, y) = (lf + d.x as f32, tf + d.y as f32);
            let mut st = slot_stack(view, d);
            let mut highlight = false;
            let mut label: Option<String> = None;
            let mut skip = false;
            if self.drag_splitting && self.drag_slots.contains(&i) {
                if let Some(c) = cursor {
                    if self.drag_slots.len() == 1 {
                        skip = true;
                    } else if can_add(view, Some(d), Some(&c), true) {
                        let mut s = c;
                        highlight = true;
                        let have = st.map_or(0, |s| s.count);
                        container::compute_stack_size(
                            self.drag_slots.len(),
                            self.drag_limit,
                            &mut s,
                            have,
                        );
                        if s.count > max_stack(s.id) {
                            label = Some(format!("\u{a7}e{}", max_stack(s.id)));
                            s.count = max_stack(s.id);
                        }
                        if s.count > slot_limit(d) {
                            label = Some(format!("\u{a7}e{}", slot_limit(d)));
                            s.count = slot_limit(d);
                        }
                        st = Some(s);
                    }
                }
            }
            if !skip {
                if st.is_none() {
                    if let SlotKind::Armor(t) = d.kind {
                        // ItemArmor.getBackgroundIcon
                        let n = [
                            "empty_armor_slot_helmet",
                            "empty_armor_slot_chestplate",
                            "empty_armor_slot_leggings",
                            "empty_armor_slot_boots",
                        ][t];
                        ui.named_icon(n, x, y);
                    }
                }
                if highlight {
                    ui.rect(x, y, 16.0, 16.0, 0xFFFFFF, 0x80 as f32 / 255.0);
                }
                if let Some(s) = st {
                    draw_stack_with(ui, &s, x, y, label.as_deref());
                }
            }
            if over(d, left, top, f.mouse.0, f.mouse.1) {
                self.hovered = Some(i);
                ui.rect(x, y, 16.0, 16.0, 0xFFFFFF, 0x80 as f32 / 255.0);
            }
        }
        foreground(ui, view, lang, lf, tf, xs as f32, ys as f32);
        if let Some(c) = cursor {
            let mut s = c;
            let mut label = None;
            if self.drag_splitting && self.drag_slots.len() > 1 {
                s.count = self.drag_remnant;
                if s.count == 0 {
                    label = Some("\u{a7}e0".to_string());
                }
            }
            let (mx, my) = (f.mouse.0 as f32, f.mouse.1 as f32);
            draw_stack_with(ui, &s, mx - 8.0, my - 8.0, label.as_deref());
        } else if let Some(h) = self.hovered {
            if let Some(st) = slot_stack(view, &slots[h]) {
                let lines = crate::screens::tooltip_lines(&st, lang);
                crate::screens::hovering_text(ui, &lines, f.mouse.0, f.mouse.1);
            }
        }
    }
}

/// `InventoryEffectRenderer.drawActivePotionEffects`
fn draw_effects(ui: &mut Ui, left: f32, top: f32, effects: &[EffectLine]) {
    let x = left - 124.0;
    let mut y = top;
    let step = if effects.len() > 5 {
        (132 / (effects.len() as i32 - 1)) as f32
    } else {
        33.0
    };
    for (icon, name, dur) in effects {
        ui.sheet(x, y, INVENTORY.0, INVENTORY.1 + 166.0, 140.0, 32.0);
        if *icon >= 0 {
            ui.sheet(
                x + 6.0,
                y + 7.0,
                INVENTORY.0 + (icon % 8 * 18) as f32,
                INVENTORY.1 + 198.0 + (icon / 8 * 18) as f32,
                18.0,
                18.0,
            );
        }
        ui.text(name, x + 10.0 + 18.0, y + 6.0, 16777215, true);
        ui.text(dur, x + 10.0 + 18.0, y + 6.0 + 10.0, 8355711, true);
        y += step;
    }
}

/// `drawGuiContainerBackgroundLayer`
fn background(ui: &mut Ui, view: &InvView, left: f32, top: f32, xs: f32, ys: f32) {
    match &view.open {
        Open::Inventory | Open::None => ui.sheet(left, top, INVENTORY.0, INVENTORY.1, xs, ys),
        Open::Workbench(_) => ui.sheet(left, top, WORKBENCH.0, WORKBENCH.1, xs, ys),
        Open::Dispenser(_) => ui.sheet(left, top, DISPENSER.0, DISPENSER.1, xs, ys),
        Open::Furnace(_) => {
            ui.sheet(left, top, FURNACE.0, FURNACE.1, xs, ys);
            if let Some((burn, cook)) = view.furnace {
                // isBurning: getBurnTimeRemainingScaled(13)
                if burn > 0 {
                    let b = burn as f32;
                    ui.sheet(
                        left + 56.0,
                        top + 36.0 + 12.0 - b,
                        FURNACE.0 + 176.0,
                        FURNACE.1 + 12.0 - b,
                        14.0,
                        b + 2.0,
                    );
                }
                ui.sheet(
                    left + 79.0,
                    top + 34.0,
                    FURNACE.0 + 176.0,
                    FURNACE.1 + 14.0,
                    cook as f32 + 1.0,
                    16.0,
                );
            }
        }
        Open::Brewing(_) => {
            // GuiBrewingStand.drawGuiContainerBackgroundLayer
            ui.sheet(left, top, BREWING.0, BREWING.1, xs, ys);
            let t = view.furnace.map_or(0, |f| f.0);
            if t > 0 {
                let j = (28.0f32 * (1.0 - t as f32 / 400.0)) as i32;
                if j > 0 {
                    ui.sheet(
                        left + 97.0,
                        top + 16.0,
                        BREWING.0 + 176.0,
                        BREWING.1,
                        9.0,
                        j as f32,
                    );
                }
                let j = [29, 24, 20, 16, 11, 6, 0][(t / 2 % 7) as usize];
                if j > 0 {
                    ui.sheet(
                        left + 65.0,
                        top + 14.0 + 29.0 - j as f32,
                        BREWING.0 + 185.0,
                        BREWING.1 + 29.0 - j as f32,
                        12.0,
                        j as f32,
                    );
                }
            }
        }
        Open::Chest(_) | Open::Cart(_) => {
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
}

/// `drawGuiContainerForegroundLayer`: the titles in 0x404040.
fn foreground(ui: &mut Ui, view: &InvView, lang: &Lang, left: f32, top: f32, xs: f32, ys: f32) {
    let fg = 4210752;
    let inv = lang.get("container.inventory");
    let centered = |ui: &mut Ui, t: &str| {
        let w = ui.text_width(t);
        ui.text(t, left + xs / 2.0 - (w / 2.0).trunc(), top + 6.0, fg, false);
    };
    match &view.open {
        Open::Inventory | Open::None => {
            ui.text(
                lang.get("container.crafting"),
                left + 86.0,
                top + 16.0,
                fg,
                false,
            );
            return;
        }
        Open::Workbench(_) => {
            ui.text(
                lang.get("container.crafting"),
                left + 28.0,
                top + 6.0,
                fg,
                false,
            );
        }
        Open::Furnace(_) => centered(ui, lang.get("container.furnace")),
        Open::Brewing(_) => centered(ui, lang.get("container.brewing")),
        Open::Dispenser(_) => centered(ui, lang.get("container.dispenser")),
        Open::Cart(_) => {
            ui.text(
                lang.get("container.minecart"),
                left + 8.0,
                top + 6.0,
                fg,
                false,
            );
        }
        Open::Chest(ps) => {
            let k = if ps.len() > 1 {
                "container.chestDouble"
            } else {
                "container.chest"
            };
            ui.text(lang.get(k), left + 8.0, top + 6.0, fg, false);
        }
    }
    ui.text(inv, left + 8.0, top + ys - 96.0 + 2.0, fg, false);
}
