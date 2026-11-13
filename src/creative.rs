//! `GuiContainerCreative`: the twelve tabs, the scrolling item list, the
//! search tab, the survival-inventory tab with the delete slot, and
//! `handleMouseClick`'s client-side item handling that reaches the server as
//! `C10PacketCreativeInventoryAction`.

use crate::container::{self, Ctx, Drag, Inv, SlotDef, SlotKind, Window};
use crate::gui::Ui;
use crate::gui_container::{Click, EffectLine, Frame, GuiContainer, Keys, Layout};
use crate::item::{max_stack, Stack};
use crate::item_data::{CREATIVE, SEARCH, TABS};
use crate::lang::Lang;
use crate::menu::widgets::TextField;
use crate::play::InvView;

const TABS_TEX: (f32, f32) = (1792.0, 256.0);
const ITEMS_TEX: (f32, f32) = (1024.0, 512.0);
const INVENTORY_TEX: (f32, f32) = (1280.0, 512.0);
const SEARCH_TEX: (f32, f32) = (1536.0, 512.0);

pub const SEARCH_TAB: usize = 5;
pub const INVENTORY_TAB: usize = 11;
const XS: i32 = 195;
const YS: i32 = 136;

/// `C10PacketCreativeInventoryAction`: a slot of `ContainerPlayer` set to a
/// stack, or (slot -1) a stack thrown out.
#[derive(Clone, Debug, PartialEq)]
pub enum Out {
    Slot(i32, Option<Stack>),
    Drop(Stack),
}

/// `CreativeTabs.getIconItemStack`
fn tab_icon(t: usize) -> Stack {
    let (id, meta) = [
        (45, 0),
        (175, 5),
        (331, 0),
        (27, 0),
        (327, 0),
        (345, 0),
        (260, 0),
        (258, 0),
        (283, 0),
        (373, 0),
        (280, 0),
        (54, 0),
    ][t];
    Stack::new(id, meta, 1)
}

fn entry(id: u16, meta: i32, count: u32, ench: &[(i32, i32)]) -> Stack {
    let s = Stack::new(id, meta, count);
    if ench.is_empty() {
        s
    } else {
        s.with_enchantments(ench)
    }
}

/// `CreativeTabs.displayAllReleventItems`
fn tab_items(t: usize) -> Vec<Stack> {
    CREATIVE
        .iter()
        .filter(|e| e.0 as usize == t)
        .map(|e| entry(e.1, e.2, e.3, e.4))
        .collect()
}

/// The tab an item is listed under, or for a one-enchantment book the tab
/// with that enchantment's type (`renderToolTip` in the search tab).
fn item_tab(s: &Stack) -> Option<usize> {
    let t = crate::item_text::creative_tab(s.id);
    if t >= 0 {
        return Some(t as usize);
    }
    if s.id == 403 {
        let e = s.enchantments();
        if e.len() == 1 {
            // tabTools: digger, fishing_rod, breakable; tabCombat: armour,
            // bow, weapon
            return Some(if matches!(e[0].0, 32..=35 | 61 | 62) {
                7
            } else {
                8
            });
        }
    }
    None
}

pub struct Creative {
    /// `selectedTabIndex` (kept between openings)
    pub tab: usize,
    scroll: f32,
    is_scrolling: bool,
    was_clicking: bool,
    pub search: TextField,
    clear_search: bool,
    items: Vec<Stack>,
    /// `basicInventory`: the 45 visible list slots.
    basic: Vec<Option<Stack>>,
    gui: GuiContainer,
    /// The drag state of `ContainerCreative` and of `ContainerPlayer`.
    drag: Drag,
    player_drag: Drag,
    pub out: Vec<Out>,
}

impl Creative {
    pub fn new(tab: usize, ui: &Ui, lang: &Lang) -> Creative {
        let mut search = TextField::new(0.0, 0.0, 89.0, 9.0);
        search.max_len = 15;
        search.background = false;
        search.visible = false;
        search.color = 0xFFFFFF;
        let mut c = Creative {
            tab: 0,
            scroll: 0.0,
            is_scrolling: false,
            was_clicking: false,
            search,
            clear_search: false,
            items: Vec::new(),
            basic: vec![None; 45],
            gui: GuiContainer::default(),
            drag: Drag::default(),
            player_drag: Drag::default(),
            out: Vec::new(),
        };
        c.set_tab(tab, ui, lang);
        c
    }

    fn origin(w: i32, h: i32, effects: bool) -> (i32, i32) {
        let mut left = (w - XS) / 2;
        if effects {
            left = 160 + (w - XS - 200) / 2;
        }
        (left, (h - YS) / 2)
    }

    /// The slot list of the current tab.
    fn slots(&self) -> Vec<SlotDef> {
        if self.tab != INVENTORY_TAB {
            return Window::Creative.slots();
        }
        // CreativeSlot wrappers of ContainerPlayer at their creative places
        let mut v = Window::Player.slots();
        for (k, d) in v.iter_mut().enumerate() {
            if (5..9).contains(&k) {
                let i = k as i32 - 5;
                d.x = 9 + i / 2 * 54;
                d.y = 6 + i % 2 * 27;
            } else if k < 5 {
                d.x = -2000;
                d.y = -2000;
            } else {
                let i = k as i32 - 9;
                d.x = 9 + i % 9 * 18;
                d.y = if k >= 36 { 112 } else { 54 + i / 9 * 18 };
            }
        }
        // destroyItemSlot: Slot(basicInventory, 0, 173, 112)
        v.push(SlotDef {
            inv: Inv::Tile,
            index: 0,
            kind: SlotKind::Plain,
            x: 173,
            y: 112,
        });
        v
    }

    fn destroy_slot(&self) -> Option<usize> {
        (self.tab == INVENTORY_TAB).then_some(45)
    }

    fn layout(&self, w: i32, h: i32, effects: bool) -> Layout {
        let (left, top) = Self::origin(w, h, effects);
        Layout {
            slots: self.slots(),
            left,
            top,
            xs: XS,
            ys: YS,
        }
    }

    /// The window as the screen shows it: the list in the tile slots.
    fn shown(&self, view: &InvView) -> InvView {
        let mut v = view.clone();
        v.tile_slots = self.basic.clone();
        v
    }

    /// `ContainerCreative.scrollTo`
    fn scroll_to(&mut self, f: f32) {
        let rows = self.items.len() as i32 / 9 - 5 + 1;
        let off = ((f * rows as f32) as f64 + 0.5) as i32;
        let off = off.max(0);
        for r in 0..5 {
            for c in 0..9 {
                let i = c + (r + off) * 9;
                self.basic[(c + r * 9) as usize] = self.items.get(i as usize).copied();
            }
        }
    }

    fn needs_scroll_bars(&self) -> bool {
        self.tab != INVENTORY_TAB && self.items.len() > 45
    }

    /// `setCurrentCreativeTab`
    pub fn set_tab(&mut self, t: usize, ui: &Ui, lang: &Lang) {
        self.tab = t;
        self.gui = GuiContainer::default();
        self.items = if t == SEARCH_TAB || t == INVENTORY_TAB {
            Vec::new()
        } else {
            tab_items(t)
        };
        if t == SEARCH_TAB {
            self.search.visible = true;
            self.search.can_lose_focus = false;
            self.search.focused = true;
            self.search.set_text(ui, "");
            self.update_search(lang);
        } else {
            self.search.visible = false;
            self.search.can_lose_focus = true;
            self.search.focused = false;
        }
        self.scroll = 0.0;
        self.scroll_to(0.0);
    }

    /// `updateCreativeSearch`: every item whose tooltip holds the text.
    fn update_search(&mut self, lang: &Lang) {
        let q = self.search.text().to_lowercase();
        self.items = SEARCH
            .iter()
            .map(|e| entry(e.0, e.1, e.2, e.3))
            .filter(|s| {
                crate::item_text::tooltip(s, lang)
                    .iter()
                    .any(|l| l.to_lowercase().contains(&q))
            })
            .collect();
        self.scroll = 0.0;
        self.scroll_to(0.0);
    }

    /// `isMouseOverTab` and `renderCreativeInventoryHoveringText`: the tab
    /// rectangle relative to the window.
    fn tab_rect(t: usize) -> (i32, i32) {
        let col = (t % 6) as i32;
        let mut x = 28 * col;
        if col == 5 {
            x = XS - 28 + 2;
        } else if col > 0 {
            x += col;
        }
        let y = if t < 6 { -32 } else { YS };
        (x, y)
    }

    fn tab_at(rx: i32, ry: i32) -> Option<usize> {
        (0..12).find(|&t| {
            let (x, y) = Self::tab_rect(t);
            rx >= x && rx <= x + 28 && ry >= y && ry <= y + 32
        })
    }

    /// `handleMouseClick`
    fn handle(&mut self, view: &mut InvView, c: Click) {
        self.clear_search = true;
        let slots = self.slots();
        let shift = c.mode == 1;
        let mode = if c.slot == -999 && c.mode == 0 {
            4
        } else {
            c.mode
        };
        let slot = (c.slot >= 0 && (c.slot as usize) < slots.len()).then_some(c.slot as usize);
        let inv = &mut view.inv;
        if slot.is_none() && self.tab != INVENTORY_TAB && mode != 5 {
            if let Some(cur) = inv.cursor {
                if c.button == 0 {
                    self.out.push(Out::Drop(cur));
                    inv.cursor = None;
                }
                if c.button == 1 {
                    self.out.push(Out::Drop(Stack { count: 1, ..cur }));
                    inv.cursor = (cur.count > 1).then_some(Stack {
                        count: cur.count - 1,
                        ..cur
                    });
                }
            }
        } else if slot.is_some() && slot == self.destroy_slot() && shift {
            // shift on the bin clears the whole inventory
            for k in 0..45 {
                self.out.push(Out::Slot(k, None));
            }
            inv.main = [None; 36];
            inv.armor = [None; 4];
        } else if self.tab == INVENTORY_TAB {
            if slot.is_some() && slot == self.destroy_slot() {
                inv.cursor = None;
            } else if mode == 4 && slot.is_some_and(|i| self.get(view, &slots[i]).is_some()) {
                let i = slot.unwrap();
                let s = self.get(view, &slots[i]).unwrap();
                let n = if c.button == 0 { 1 } else { max_stack(s.id) };
                let mut tile: [Option<Stack>; 0] = [];
                let mut ctx = Ctx::new(&mut view.inv, &mut tile, Window::Player, true);
                let before = player_slots(&ctx);
                if let Some(t) = take(&mut ctx, i, n) {
                    self.out.push(Out::Drop(t));
                }
                self.send_changes(&before, &ctx);
            } else if mode == 4 && view.inv.cursor.is_some() {
                self.out.push(Out::Drop(view.inv.cursor.unwrap()));
                view.inv.cursor = None;
            } else {
                let n = slot.map_or(c.slot, |i| i as i32);
                let mut tile: [Option<Stack>; 0] = [];
                let mut ctx = Ctx::new(&mut view.inv, &mut tile, Window::Player, true);
                let before = player_slots(&ctx);
                container::slot_click(&mut ctx, &mut self.player_drag, n, c.button, mode);
                self.send_changes(&before, &ctx);
                for (s, _) in std::mem::take(&mut ctx.fx.drops) {
                    self.out.push(Out::Drop(s));
                }
            }
        } else if mode != 5 && slot.is_some_and(|i| slots[i].inv == Inv::Tile) {
            let i = slot.unwrap();
            let here = self.basic[slots[i].index];
            let cur = inv.cursor;
            match mode {
                2 => {
                    if let (Some(h), true) = (here, (0..9).contains(&c.button)) {
                        let s = Stack {
                            count: max_stack(h.id),
                            ..h
                        };
                        inv.main[c.button as usize] = Some(s);
                        self.out.push(Out::Slot(36 + c.button, Some(s)));
                    }
                    return;
                }
                3 => {
                    if let (None, Some(h)) = (cur, here) {
                        inv.cursor = Some(Stack {
                            count: max_stack(h.id),
                            ..h
                        });
                    }
                    return;
                }
                4 => {
                    if let Some(h) = here {
                        let n = if c.button == 0 { 1 } else { max_stack(h.id) };
                        self.out.push(Out::Drop(Stack { count: n, ..h }));
                    }
                    return;
                }
                _ => {}
            }
            match (cur, here) {
                (Some(mut k), Some(h)) if k.id == h.id && k.meta == h.meta => {
                    if c.button == 0 {
                        if shift {
                            k.count = max_stack(k.id);
                        } else if k.count < max_stack(k.id) {
                            k.count += 1;
                        }
                        inv.cursor = Some(k);
                    } else if k.count <= 1 {
                        inv.cursor = None;
                    } else {
                        k.count -= 1;
                        inv.cursor = Some(k);
                    }
                }
                (None, Some(h)) => {
                    let mut k = h;
                    if shift {
                        k.count = max_stack(k.id);
                    }
                    inv.cursor = Some(k);
                }
                _ => inv.cursor = None,
            }
        } else {
            let n = slot.map_or(c.slot, |i| i as i32);
            let mut basic = std::mem::take(&mut self.basic);
            {
                let mut ctx = Ctx::new(&mut view.inv, &mut basic, Window::Creative, true);
                container::slot_click(&mut ctx, &mut self.drag, n, c.button, mode);
            }
            self.basic = basic;
            if mode == 5 && c.button & 3 == 2 {
                for k in 0..9 {
                    self.out.push(Out::Slot(36 + k, view.inv.main[k as usize]));
                }
            } else if let Some(i) = slot {
                if slots[i].inv == Inv::Player {
                    let k = slots[i].index;
                    self.out.push(Out::Slot(36 + k as i32, view.inv.main[k]));
                }
            }
        }
    }

    fn get(&self, view: &InvView, d: &SlotDef) -> Option<Stack> {
        crate::gui_container::slot_stack(&self.shown(view), d)
    }

    /// `CreativeCrafting.sendSlotContents` for what a click changed.
    fn send_changes(&mut self, before: &[Option<Stack>], ctx: &Ctx) {
        let after = player_slots(ctx);
        for k in 0..45 {
            if before[k] != after[k] {
                self.out.push(Out::Slot(k as i32, after[k]));
            }
        }
    }

    fn frame<'a>(view: &'a InvView, layout: &'a Layout, mouse: (i32, i32), age: f32) -> Frame<'a> {
        Frame {
            view,
            layout: Some(layout),
            w: 0,
            h: 0,
            effects: false,
            mouse,
            age,
        }
    }

    /// `mouseClicked`: a press on a tab only selects it on release.
    #[allow(clippy::too_many_arguments)]
    pub fn mouse_clicked(
        &mut self,
        view: &mut InvView,
        dims: (i32, i32, bool),
        mouse: (i32, i32),
        button: i32,
        shift: bool,
        now: u64,
        keys: &Keys,
    ) {
        let layout = self.layout(dims.0, dims.1, dims.2);
        if button == 0 && Self::tab_at(mouse.0 - layout.left, mouse.1 - layout.top).is_some() {
            return;
        }
        let shown = self.shown(view);
        let mut gui = std::mem::take(&mut self.gui);
        {
            let f = Self::frame(&shown, &layout, mouse, 0.0);
            gui.mouse_clicked(&f, button, shift, now, keys, &mut |c| {
                self.handle(view, c);
                self.shown(view)
            });
        }
        self.gui = gui;
    }

    pub fn mouse_dragged(&mut self, view: &InvView, dims: (i32, i32, bool), mouse: (i32, i32)) {
        let layout = self.layout(dims.0, dims.1, dims.2);
        let shown = self.shown(view);
        let f = Self::frame(&shown, &layout, mouse, 0.0);
        self.gui.mouse_dragged(&f);
    }

    /// `mouseMovedOrUp`: releasing over a tab selects it.
    #[allow(clippy::too_many_arguments)]
    pub fn mouse_released(
        &mut self,
        view: &mut InvView,
        dims: (i32, i32, bool),
        mouse: (i32, i32),
        button: i32,
        shift: bool,
        keys: &Keys,
        ui: &Ui,
        lang: &Lang,
    ) {
        let layout = self.layout(dims.0, dims.1, dims.2);
        if button == 0 {
            if let Some(t) = Self::tab_at(mouse.0 - layout.left, mouse.1 - layout.top) {
                self.set_tab(t, ui, lang);
                return;
            }
        }
        let shown = self.shown(view);
        let mut gui = std::mem::take(&mut self.gui);
        {
            let f = Self::frame(&shown, &layout, mouse, 0.0);
            gui.mouse_released(&f, button, shift, keys, &mut |c| {
                self.handle(view, c);
                self.shown(view)
            });
        }
        self.gui = gui;
    }

    /// `handleMouseInput`: the wheel scrolls the list.
    pub fn wheel(&mut self, up: bool) {
        if !self.needs_scroll_bars() {
            return;
        }
        let rows = self.items.len() as i32 / 9 - 5 + 1;
        let d = if up { 1.0 } else { -1.0 };
        self.scroll = ((self.scroll as f64 - d / rows as f64) as f32).clamp(0.0, 1.0);
        self.scroll_to(self.scroll);
    }

    /// `keyTyped`; true when the screen closes.
    #[allow(clippy::too_many_arguments)]
    pub fn key_typed(
        &mut self,
        view: &mut InvView,
        dims: (i32, i32, bool),
        mouse: (i32, i32),
        ch: char,
        code: i32,
        ctrl: bool,
        shift: bool,
        keys: &Keys,
        chat_key: i32,
        ui: &Ui,
        lang: &Lang,
        clip: &mut String,
    ) -> bool {
        let layout = self.layout(dims.0, dims.1, dims.2);
        let shown = self.shown(view);
        if self.tab != SEARCH_TAB {
            if code == chat_key {
                self.set_tab(SEARCH_TAB, ui, lang);
                return false;
            }
            let mut gui = std::mem::take(&mut self.gui);
            let close = {
                let f = Self::frame(&shown, &layout, mouse, 0.0);
                gui.key_typed(&f, code, ctrl, keys, &mut |c| {
                    self.handle(view, c);
                    self.shown(view)
                })
            };
            self.gui = gui;
            return close;
        }
        if self.clear_search {
            self.clear_search = false;
            self.search.set_text(ui, "");
        }
        // checkHotbarKeys
        if view.inv.cursor.is_none() {
            if let Some(h) = self.gui.hovered {
                if let Some(k) = keys.hotbar.iter().position(|&c| c == code) {
                    self.handle(
                        view,
                        Click {
                            slot: h as i32,
                            button: k as i32,
                            mode: 2,
                        },
                    );
                    return false;
                }
            }
        }
        if self.search.key(ui, ch, code, ctrl, shift, clip) {
            self.update_search(lang);
            return false;
        }
        let mut gui = std::mem::take(&mut self.gui);
        let close = {
            let f = Self::frame(&shown, &layout, mouse, 0.0);
            gui.key_typed(&f, code, ctrl, keys, &mut |c| {
                self.handle(view, c);
                self.shown(view)
            })
        };
        self.gui = gui;
        close
    }

    pub fn tick(&mut self) {
        self.search.cursor_counter += 1;
    }

    /// `drawScreen`
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        ui: &mut Ui,
        view: &InvView,
        dims: (i32, i32, bool),
        mouse: (i32, i32),
        mouse_down: bool,
        age: f32,
        lang: &Lang,
        effects: &[EffectLine],
    ) {
        let layout = self.layout(dims.0, dims.1, dims.2);
        let (left, top) = (layout.left, layout.top);
        // the scroll bar drag
        let (x0, y0) = (left + 175, top + 18);
        let (x1, y1) = (x0 + 14, y0 + 112);
        if !self.was_clicking
            && mouse_down
            && mouse.0 >= x0
            && mouse.1 >= y0
            && mouse.0 < x1
            && mouse.1 < y1
        {
            self.is_scrolling = self.needs_scroll_bars();
        }
        if !mouse_down {
            self.is_scrolling = false;
        }
        self.was_clicking = mouse_down;
        if self.is_scrolling {
            self.scroll = ((mouse.1 - y0) as f32 - 7.5) / ((y1 - y0) as f32 - 15.0);
            self.scroll = self.scroll.clamp(0.0, 1.0);
            self.scroll_to(self.scroll);
        }
        crate::screens::dim_background(ui);
        // drawGuiContainerBackgroundLayer
        let (lf, tf) = (left as f32, top as f32);
        for t in 0..12 {
            if t != self.tab {
                self.draw_tab(ui, t, lf, tf);
            }
        }
        let bg = match self.tab {
            INVENTORY_TAB => INVENTORY_TEX,
            SEARCH_TAB => SEARCH_TEX,
            _ => ITEMS_TEX,
        };
        ui.sheet(lf, tf, bg.0, bg.1, XS as f32, YS as f32);
        self.search.x = lf + 82.0;
        self.search.y = tf + 6.0;
        self.search.draw(ui);
        if self.tab != INVENTORY_TAB {
            let u = 232.0 + if self.needs_scroll_bars() { 0.0 } else { 12.0 };
            ui.sheet(
                lf + 175.0,
                tf + 18.0 + ((112 - 17) as f32 * self.scroll).trunc(),
                TABS_TEX.0 + u,
                TABS_TEX.1,
                12.0,
                15.0,
            );
        }
        self.draw_tab(ui, self.tab, lf, tf);
        if self.tab == INVENTORY_TAB {
            let (px, py) = (lf + 43.0, tf + 45.0);
            ui.player_model(
                px,
                py,
                20.0,
                px - mouse.0 as f32,
                py - 30.0 - mouse.1 as f32,
                age,
                &view.inv,
            );
        }
        // drawGuiContainerForegroundLayer: the tab title
        if self.tab != INVENTORY_TAB {
            ui.text(
                lang.get(&format!("itemGroup.{}", TABS[self.tab])),
                lf + 8.0,
                tf + 6.0,
                4210752,
                false,
            );
        }
        let shown = self.shown(view);
        let f = Frame {
            view: &shown,
            layout: Some(&layout),
            w: dims.0,
            h: dims.1,
            effects: dims.2,
            mouse,
            age,
        };
        let search = self.tab == SEARCH_TAB;
        let tooltip = |s: &Stack| -> Vec<String> {
            if !search {
                return crate::screens::tooltip_lines(s, lang);
            }
            // the tab name under the item name
            let mut v = crate::item_text::tooltip(s, lang);
            if let Some(t) = item_tab(s) {
                v.insert(
                    1,
                    format!(
                        "\u{a7}l\u{a7}9{}",
                        lang.get(&format!("itemGroup.{}", TABS[t]))
                    ),
                );
            }
            let c = crate::item_text::rarity_color(crate::item_text::rarity(s));
            v.into_iter()
                .enumerate()
                .map(|(i, l)| {
                    if i == 0 {
                        format!("\u{a7}{c}{l}")
                    } else {
                        format!("\u{a7}7{l}")
                    }
                })
                .collect()
        };
        self.gui.draw_with(ui, &f, lang, effects, &tooltip);
        for t in 0..12 {
            let (x, y) = Self::tab_rect(t);
            let (rx, ry) = (mouse.0 - left, mouse.1 - top);
            if rx >= x + 3 - 1 && rx < x + 3 + 23 + 1 && ry >= y + 3 - 1 && ry < y + 3 + 27 + 1 {
                let l = lang.get(&format!("itemGroup.{}", TABS[t])).to_string();
                crate::screens::hovering_text(ui, &[l], mouse.0, mouse.1);
                break;
            }
        }
        if self.tab == INVENTORY_TAB {
            let (rx, ry) = (mouse.0 - left, mouse.1 - top);
            if rx >= 172 && rx < 190 && ry >= 111 && ry < 129 {
                let l = lang.get("inventory.binSlot").to_string();
                crate::screens::hovering_text(ui, &[l], mouse.0, mouse.1);
            }
        }
    }

    /// `drawTab`
    fn draw_tab(&self, ui: &mut Ui, t: usize, left: f32, top: f32) {
        let sel = t == self.tab;
        let first = t < 6;
        let col = (t % 6) as f32;
        let u = col * 28.0;
        let mut v = if sel { 32.0 } else { 0.0 };
        let mut x = left + 28.0 * col;
        let mut y = top;
        if col == 5.0 {
            x = left + XS as f32 - 28.0;
        } else if col > 0.0 {
            x += col;
        }
        if first {
            y -= 28.0;
        } else {
            v += 64.0;
            y += YS as f32 - 4.0;
        }
        ui.sheet(x, y, TABS_TEX.0 + u, TABS_TEX.1 + v, 28.0, 32.0);
        let icon = tab_icon(t);
        crate::screens::draw_stack(ui, &icon, x + 6.0, y + 8.0 + if first { 1.0 } else { -1.0 });
    }
}

/// The 45 `ContainerPlayer` slot stacks.
fn player_slots(ctx: &Ctx) -> Vec<Option<Stack>> {
    (0..45).map(|i| ctx.get(i)).collect()
}

/// `Slot.decrStackSize`
fn take(ctx: &mut Ctx, i: usize, n: u32) -> Option<Stack> {
    let s = ctx.get(i)?;
    let k = n.min(s.count);
    ctx.set_slot(
        i,
        (s.count > k).then_some(Stack {
            count: s.count - k,
            ..s
        }),
    );
    Some(Stack { count: k, ..s })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(tab: usize) -> Creative {
        let mut c = Creative {
            tab,
            scroll: 0.0,
            is_scrolling: false,
            was_clicking: false,
            search: TextField::new(0.0, 0.0, 89.0, 9.0),
            clear_search: false,
            items: tab_items(tab),
            basic: vec![None; 45],
            gui: GuiContainer::default(),
            drag: Drag::default(),
            player_drag: Drag::default(),
            out: Vec::new(),
        };
        c.scroll_to(0.0);
        c
    }

    fn click(slot: i32, button: i32, mode: i32) -> Click {
        Click { slot, button, mode }
    }

    #[test]
    fn picking_from_the_list_and_placing() {
        let mut c = screen(0);
        let mut v = InvView::empty();
        let first = c.basic[0].unwrap();
        assert_eq!(first, Stack::new(1, 0, 1));
        // left click takes one, shift-click a full stack
        c.handle(&mut v, click(0, 0, 0));
        assert_eq!(v.inv.cursor, Some(Stack::new(1, 0, 1)));
        c.handle(&mut v, click(0, 0, 0));
        assert_eq!(v.inv.cursor.unwrap().count, 2);
        // into hotbar slot 3 (container slot 48)
        c.handle(&mut v, click(48, 0, 0));
        assert_eq!(v.inv.main[3], Some(Stack::new(1, 0, 2)));
        assert_eq!(c.out, vec![Out::Slot(39, Some(Stack::new(1, 0, 2)))]);
        // a list slot with something else on the cursor clears the cursor
        v.inv.cursor = Some(Stack::new(4, 0, 5));
        c.handle(&mut v, click(1, 0, 0));
        assert_eq!(v.inv.cursor, None);
        // number key over the list fills the hotbar slot
        c.out.clear();
        c.handle(&mut v, click(2, 4, 2));
        assert_eq!(v.inv.main[4].unwrap().count, 64);
        assert_eq!(c.out.len(), 1);
        // outside the window throws the cursor
        v.inv.cursor = Some(Stack::new(4, 0, 5));
        c.out.clear();
        c.handle(&mut v, click(-999, 0, 0));
        assert_eq!(c.out, vec![Out::Drop(Stack::new(4, 0, 5))]);
    }

    #[test]
    fn inventory_tab_bin() {
        let mut c = screen(INVENTORY_TAB);
        let mut v = InvView::empty();
        v.inv.main[0] = Some(Stack::new(1, 0, 3));
        // ContainerPlayer slot 36 is hotbar 0
        c.handle(&mut v, click(36, 0, 0));
        assert_eq!(v.inv.cursor, Some(Stack::new(1, 0, 3)));
        assert_eq!(c.out, vec![Out::Slot(36, None)]);
        // the bin swallows the cursor
        c.handle(&mut v, click(45, 0, 0));
        assert_eq!(v.inv.cursor, None);
        // shift on the bin clears everything
        v.inv.main[5] = Some(Stack::new(1, 0, 3));
        c.out.clear();
        c.handle(&mut v, click(45, 0, 1));
        assert_eq!(v.inv.main[5], None);
        assert_eq!(c.out.len(), 45);
    }
}
