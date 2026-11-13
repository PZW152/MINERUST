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
        SlotKind::Armor(_) | SlotKind::Potion | SlotKind::Enchant => 1,
        _ => 64,
    }
}

fn slot_valid(d: &SlotDef, s: &Stack) -> bool {
    match d.kind {
        SlotKind::Plain | SlotKind::Enchant => true,
        SlotKind::CraftResult | SlotKind::FurnaceOut | SlotKind::RepairOut => false,
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
        if s.id == c.id && s.meta == c.meta && s.tag == c.tag {
            let add = if ignore_size { 0 } else { s.count };
            ok |= c.count + add <= max_stack(s.id);
        }
    }
    ok
}

/// `Container.canDragIntoSlot`
fn can_drag_into(f: &Frame, d: &SlotDef) -> bool {
    match f.layout {
        Some(_) => d.inv == container::Inv::Player || d.y > 90 && d.x <= 162,
        None => true,
    }
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

/// A screen's own slot list and placement (`GuiContainerCreative`), with
/// `ContainerCreative`'s `canMergeSlot` / `canDragIntoSlot` rules.
pub struct Layout {
    pub slots: Vec<SlotDef>,
    pub left: i32,
    pub top: i32,
    pub xs: i32,
    pub ys: i32,
}

pub struct Frame<'a> {
    pub view: &'a InvView,
    pub layout: Option<&'a Layout>,
    pub w: i32,
    pub h: i32,
    pub effects: bool,
    pub mouse: (i32, i32),
    /// `ticksExisted + partialTicks` of the player.
    pub age: f32,
    /// `GuiEnchantment`: the player's level, creative mode, and the book's
    /// interpolated `open` and `flip`.
    pub ench: Option<EnchantState>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EnchantState {
    pub xp_level: i32,
    pub creative: bool,
    pub open: f32,
    pub flip: f32,
}

const ENCHANT: (f32, f32) = (1024.0, 768.0);

/// The enchanting button `i` under the mouse (`GuiEnchantment.mouseClicked`).
pub fn enchant_button_at(open: &Open, w: i32, h: i32, mouse: (i32, i32)) -> Option<usize> {
    let (left, top) = origin(open, w, h, false);
    (0..3).find(|&i| {
        let (x, y) = (mouse.0 - (left + 60), mouse.1 - (top + 14 + 19 * i as i32));
        x >= 0 && y >= 0 && x < 108 && y < 19
    })
}

impl GuiContainer {
    fn geometry(f: &Frame) -> (Vec<SlotDef>, i32, i32, i32, i32) {
        if let Some(l) = f.layout {
            return (l.slots.clone(), l.left, l.top, l.xs, l.ys);
        }
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
            && can_drag_into(f, &slots[i])
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
        // canMergeSlot: not the crafting result; ContainerCreative: the lower rows
        let mergeable = hit.is_some_and(|i| {
            if f.layout.is_some() {
                slots[i].y > 90
            } else {
                slots[i].inv != container::Inv::Result
            }
        });
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
        let (slots, ..) = Self::geometry(f);
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
        self.draw_with(ui, f, lang, effects, &|s| {
            crate::screens::tooltip_lines(s, lang)
        });
    }

    /// `drawScreen` with the screen's own `renderToolTip` lines; a screen
    /// with a `Layout` draws its background itself first.
    pub fn draw_with(
        &mut self,
        ui: &mut Ui,
        f: &Frame,
        lang: &Lang,
        effects: &[EffectLine],
        tooltip: &dyn Fn(&Stack) -> Vec<String>,
    ) {
        let view = f.view;
        let (slots, left, top, xs, ys) = Self::geometry(f);
        let (lf, tf) = (left as f32, top as f32);
        let own = f.layout.is_none();
        if own {
            dim_background(ui);
        }
        if !effects.is_empty() && (!own || matches!(view.open, Open::Inventory | Open::None)) {
            draw_effects(ui, lf, tf, effects);
        }
        if own {
            background(ui, view, lf, tf, xs as f32, ys as f32);
            if let (Open::Enchant(_), Some(e)) = (&view.open, f.ench) {
                enchant_background(ui, view, &e, lf, tf, f.mouse);
            }
        }
        if own && matches!(view.open, Open::Inventory | Open::None) {
            // GuiInventory: the player, turned towards the mouse
            let (px, py) = (lf + 51.0, tf + 75.0);
            let (mx, my) = (f.mouse.0 as f32, f.mouse.1 as f32);
            ui.player_model(px, py, 30.0, px - mx, py - 50.0 - my, f.age, &view.inv);
        }
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
                    } else if can_add(view, Some(d), Some(&c), true) && can_drag_into(f, d) {
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
        if own {
            foreground(ui, view, lang, lf, tf, xs as f32, ys as f32);
            if let (Open::Anvil(_), Some(e)) = (&view.open, f.ench) {
                crate::gui_repair::foreground(ui, view, lang, lf, tf, e.creative, e.xp_level);
            }
        }
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
                let lines = tooltip(&st);
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
        Open::Enchant(_) => ui.sheet(left, top, ENCHANT.0, ENCHANT.1, xs, ys),
        Open::Anvil(_) => crate::gui_repair::background(ui, view, left, top),
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
        // GuiRepair draws its own title and no inventory label
        Open::Anvil(_) => return,
        Open::Furnace(_) => centered(ui, lang.get("container.furnace")),
        Open::Brewing(_) => centered(ui, lang.get("container.brewing")),
        Open::Dispenser(_) => centered(ui, lang.get("container.dispenser")),
        Open::Enchant(_) => {
            ui.text(
                lang.get("container.enchant"),
                left + 12.0,
                top + 5.0,
                fg,
                false,
            );
        }
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

/// `GuiEnchantment.drawGuiContainerBackgroundLayer` after the panel: the
/// book and the three buttons with their galactic names and levels.
fn enchant_background(
    ui: &mut Ui,
    view: &InvView,
    e: &EnchantState,
    left: f32,
    top: f32,
    mouse: (i32, i32),
) {
    // the book model in its own perspective viewport
    let (w, h) = (ui.w, ui.h);
    let atlas = ui.atlas();
    let book = crate::render::model::enchant_book_gui(atlas, w, h, e.open, e.flip);
    ui.push_model(&book);
    // EnchantmentNameParts.reseedRandomGenerator(nameSeed)
    let mut r = crate::java::JavaRandom::new(view.name_seed);
    for i in 0..3 {
        let name = crate::enchant::random_name(&mut r);
        let lvl = view.enchant[i];
        let (x, y) = (left + 60.0, top + 14.0 + 19.0 * i as f32);
        if lvl == 0 {
            ui.sheet(x, y, ENCHANT.0, ENCHANT.1 + 185.0, 108.0, 19.0);
            continue;
        }
        let num = lvl.to_string();
        let nw = ui.text_width(&num);
        if e.xp_level < lvl && !e.creative {
            ui.sheet(x, y, ENCHANT.0, ENCHANT.1 + 185.0, 108.0, 19.0);
            ui.sga_split(
                &name,
                left + 62.0,
                y + 2.0,
                104.0,
                (6839882 & 16711422) >> 1,
            );
            ui.text(&num, left + 62.0 + 104.0 - nw, y + 2.0 + 7.0, 4226832, true);
        } else {
            let (mx, my) = (mouse.0 as f32 - x, mouse.1 as f32 - y);
            let mut color = 6839882;
            if mx >= 0.0 && my >= 0.0 && mx < 108.0 && my < 19.0 {
                ui.sheet(x, y, ENCHANT.0, ENCHANT.1 + 204.0, 108.0, 19.0);
                color = 16777088;
            } else {
                ui.sheet(x, y, ENCHANT.0, ENCHANT.1 + 166.0, 108.0, 19.0);
            }
            ui.sga_split(&name, left + 62.0, y + 2.0, 104.0, color);
            ui.text(&num, left + 62.0 + 104.0 - nw, y + 2.0 + 7.0, 8453920, true);
        }
    }
}

/// `GuiEnchantment`'s book: `flip`, `flipT`, `flipA`, `open` and their
/// previous values, its own `random` and the last stack (`tickBook`).
#[derive(Clone, Debug)]
pub struct BookAnim {
    pub flip: f32,
    pub o_flip: f32,
    flip_t: f32,
    flip_a: f32,
    pub open: f32,
    pub o_open: f32,
    rand: crate::java::JavaRandom,
    last: Option<Stack>,
}

impl Default for BookAnim {
    fn default() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as i64);
        BookAnim {
            flip: 0.0,
            o_flip: 0.0,
            flip_t: 0.0,
            flip_a: 0.0,
            open: 0.0,
            o_open: 0.0,
            rand: crate::java::JavaRandom::new(seed),
            last: None,
        }
    }
}

impl BookAnim {
    /// `GuiEnchantment.tickBook`
    pub fn tick(&mut self, item: Option<Stack>, levels: [i32; 3]) {
        if item != self.last {
            self.last = item;
            loop {
                self.flip_t += (self.rand.next_int_n(4) - self.rand.next_int_n(4)) as f32;
                if !(self.flip <= self.flip_t + 1.0 && self.flip >= self.flip_t - 1.0) {
                    break;
                }
            }
        }
        self.o_flip = self.flip;
        self.o_open = self.open;
        if levels.iter().any(|&l| l != 0) {
            self.open += 0.2;
        } else {
            self.open -= 0.2;
        }
        self.open = self.open.clamp(0.0, 1.0);
        let d = ((self.flip_t - self.flip) * 0.4).clamp(-0.2, 0.2);
        self.flip_a += (d - self.flip_a) * 0.9;
        self.flip += self.flip_a;
    }
}
