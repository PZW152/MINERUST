//! `GuiButton`, `GuiOptionButton`, `GuiOptionSlider`,
//! `GuiScreenOptionsSounds$Button`, `GuiButtonLanguage`, `GuiTextField`,
//! `GuiSlot` and the backgrounds of `GuiScreen`.

use crate::gui::Ui;
use crate::lang::Lang;
use crate::render::atlas::gui_pos;
use crate::settings::{GameSettings, Opt};

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Plain,
    /// `GuiOptionButton` of a cycling option
    Option(Opt),
    /// `GuiOptionSlider`
    Slider {
        opt: Opt,
        value: f32,
        dragging: bool,
    },
    /// `GuiScreenOptionsSounds$Button`
    Sound {
        cat: usize,
        value: f32,
        pressed: bool,
    },
    /// `GuiButtonLanguage`
    Language,
}

#[derive(Clone, Debug)]
pub struct Button {
    pub id: i32,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub label: String,
    pub enabled: bool,
    pub visible: bool,
    pub kind: Kind,
}

impl Button {
    /// `new GuiButton(id, x, y, w, h, text)`
    pub fn new(id: i32, x: f32, y: f32, w: f32, h: f32, label: impl Into<String>) -> Button {
        Button {
            id,
            x,
            y,
            w,
            h,
            label: label.into(),
            enabled: true,
            visible: true,
            kind: Kind::Plain,
        }
    }
    /// `new GuiButton(id, x, y, text)`: 200 by 20
    pub fn wide(id: i32, x: f32, y: f32, label: impl Into<String>) -> Button {
        Button::new(id, x, y, 200.0, 20.0, label)
    }
    /// `GuiOptionButton` / `GuiOptionSlider` for an option, 150 by 20.
    pub fn option(x: f32, y: f32, opt: Opt, s: &GameSettings, lang: &Lang) -> Button {
        let id = opt as i32;
        let mut b = Button::new(id, x, y, 150.0, 20.0, s.label(opt, lang));
        b.kind = if opt.is_float() {
            Kind::Slider {
                opt,
                value: opt.normalize(s.get_float(opt)),
                dragging: false,
            }
        } else {
            Kind::Option(opt)
        };
        b
    }
    /// `GuiButtonLanguage`
    pub fn language(id: i32, x: f32, y: f32) -> Button {
        let mut b = Button::new(id, x, y, 20.0, 20.0, "");
        b.kind = Kind::Language;
        b
    }

    pub fn inside(&self, m: (f32, f32)) -> bool {
        m.0 >= self.x && m.1 >= self.y && m.0 < self.x + self.w && m.1 < self.y + self.h
    }

    /// `mousePressed`
    pub fn pressed(&self, m: (f32, f32)) -> bool {
        self.enabled && self.visible && self.inside(m)
    }

    /// `getHoverState`: 0 disabled, 1 normal, 2 hovered.
    fn hover_state(&self, hovered: bool) -> f32 {
        match self.kind {
            Kind::Slider { .. } | Kind::Sound { .. } => 0.0,
            _ => {
                if !self.enabled {
                    0.0
                } else if hovered {
                    2.0
                } else {
                    1.0
                }
            }
        }
    }

    /// `drawButton` (with `mouseDragged` of the sliders).
    pub fn draw(&mut self, ui: &mut Ui, m: (f32, f32), s: &mut GameSettings, lang: &Lang) {
        if !self.visible {
            return;
        }
        let hovered = self.inside(m);
        if let Kind::Language = self.kind {
            let v = 106.0 + if hovered { self.h } else { 0.0 };
            ui.widget(self.x, self.y, 0.0, v, self.w, self.h);
            return;
        }
        let k = self.hover_state(hovered);
        let hw = (self.w as i32 / 2) as f32;
        ui.widget(self.x, self.y, 0.0, 46.0 + k * 20.0, hw, self.h);
        ui.widget(self.x + hw, self.y, 200.0 - hw, 46.0 + k * 20.0, hw, self.h);
        // mouseDragged
        let x0 = self.x;
        let w = self.w;
        match &mut self.kind {
            Kind::Slider {
                opt,
                value,
                dragging,
            } => {
                if *dragging {
                    *value = ((m.0 - (x0 + 4.0)) / (w - 8.0)).clamp(0.0, 1.0);
                    let v = opt.denormalize(*value);
                    s.set_float(*opt, v);
                    *value = opt.normalize(v);
                    self.label = s.label(*opt, lang);
                }
                let kx = x0 + (*value * (w - 8.0)) as i32 as f32;
                ui.widget(kx, self.y, 0.0, 66.0, 4.0, 20.0);
                ui.widget(kx + 4.0, self.y, 196.0, 66.0, 4.0, 20.0);
            }
            Kind::Sound {
                cat,
                value,
                pressed,
            } => {
                if *pressed {
                    *value = ((m.0 - (x0 + 4.0)) / (w - 8.0)).clamp(0.0, 1.0);
                    s.sound_levels[*cat] = *value;
                    s.save();
                    self.label = sound_label(*cat, s, lang);
                }
                let kx = x0 + (*value * (w - 8.0)) as i32 as f32;
                ui.widget(kx, self.y, 0.0, 66.0, 4.0, 20.0);
                ui.widget(kx + 4.0, self.y, 196.0, 66.0, 4.0, 20.0);
            }
            _ => {}
        }
        let color = if !self.enabled {
            10526880
        } else if hovered {
            16777120
        } else {
            14737632
        };
        ui.centered(
            &self.label,
            self.x + hw,
            self.y + ((self.h - 8.0) / 2.0).floor(),
            color,
        );
    }

    /// The press of a slider: the value jumps to the mouse.
    pub fn press_slider(&mut self, m: (f32, f32), s: &mut GameSettings, lang: &Lang) {
        let x0 = self.x;
        let w = self.w;
        match &mut self.kind {
            Kind::Slider {
                opt,
                value,
                dragging,
            } => {
                *value = ((m.0 - (x0 + 4.0)) / (w - 8.0)).clamp(0.0, 1.0);
                s.set_float(*opt, opt.denormalize(*value));
                self.label = s.label(*opt, lang);
                *dragging = true;
            }
            Kind::Sound {
                cat,
                value,
                pressed,
            } => {
                *value = ((m.0 - (x0 + 4.0)) / (w - 8.0)).clamp(0.0, 1.0);
                s.sound_levels[*cat] = *value;
                s.save();
                self.label = sound_label(*cat, s, lang);
                *pressed = true;
            }
            _ => {}
        }
    }

    /// `mouseReleased`; true for a sound slider that was held (it plays the
    /// click on release instead of on press).
    pub fn release(&mut self) -> bool {
        match &mut self.kind {
            Kind::Slider { dragging, .. } => {
                *dragging = false;
                false
            }
            Kind::Sound { pressed, .. } => {
                let was = *pressed;
                *pressed = false;
                was
            }
            _ => false,
        }
    }
}

/// `GuiScreenOptionsSounds$Button` text.
pub fn sound_label(cat: usize, s: &GameSettings, lang: &Lang) -> String {
    let v = s.sound_levels[cat];
    let name = lang
        .get(&format!(
            "soundCategory.{}",
            crate::settings::SOUND_CATEGORIES[cat]
        ))
        .to_string();
    let val = if v == 0.0 {
        lang.get("options.off").to_string()
    } else {
        format!("{}%", (v * 100.0) as i32)
    };
    format!("{name}: {val}")
}

/// `ChatAllowedCharacters.isAllowedCharacter`
pub fn allowed_char(c: char) -> bool {
    c != '\u{a7}' && c as u32 >= 32 && c as u32 != 127
}

/// `GuiTextField`
#[derive(Clone, Debug)]
pub struct TextField {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub text: Vec<char>,
    pub max_len: usize,
    pub cursor_counter: i32,
    pub background: bool,
    pub can_lose_focus: bool,
    pub focused: bool,
    pub enabled: bool,
    pub visible: bool,
    pub scroll: usize,
    pub cursor: usize,
    pub selection_end: usize,
    pub color: u32,
    pub disabled_color: u32,
}

impl TextField {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> TextField {
        TextField {
            x,
            y,
            w,
            h,
            text: Vec::new(),
            max_len: 32,
            cursor_counter: 0,
            background: true,
            can_lose_focus: true,
            focused: false,
            enabled: true,
            visible: true,
            scroll: 0,
            cursor: 0,
            selection_end: 0,
            color: 14737632,
            disabled_color: 7368816,
        }
    }

    pub fn text(&self) -> String {
        self.text.iter().collect()
    }

    /// `setText`
    pub fn set_text(&mut self, ui: &Ui, s: &str) {
        let mut t: Vec<char> = s.chars().collect();
        t.truncate(self.max_len);
        self.text = t;
        self.set_cursor_end(ui);
    }

    /// `setFocused`
    pub fn set_focused(&mut self, f: bool) {
        if f && !self.focused {
            self.cursor_counter = 0;
        }
        self.focused = f;
    }

    /// `getWidth`
    fn inner_width(&self) -> f32 {
        if self.background {
            self.w - 8.0
        } else {
            self.w
        }
    }

    fn slice(&self, a: usize) -> String {
        self.text[a.min(self.text.len())..].iter().collect()
    }

    /// `writeText`
    pub fn write(&mut self, ui: &Ui, s: &str) {
        let add: Vec<char> = s.chars().filter(|&c| allowed_char(c)).collect();
        let a = self.cursor.min(self.selection_end);
        let b = self.cursor.max(self.selection_end);
        let room = self.max_len as isize
            - self.text.len() as isize
            - (a as isize - self.selection_end as isize);
        let mut out: Vec<char> = Vec::new();
        if !self.text.is_empty() {
            out.extend_from_slice(&self.text[..a]);
        }
        let n = if room < add.len() as isize {
            let r = room.max(0) as usize;
            out.extend_from_slice(&add[..r]);
            r
        } else {
            out.extend_from_slice(&add);
            add.len()
        };
        if !self.text.is_empty() && b < self.text.len() {
            out.extend_from_slice(&self.text[b..]);
        }
        self.text = out;
        let d = a as isize - self.selection_end as isize + n as isize;
        self.move_cursor_by(ui, d);
    }

    /// `deleteFromCursor`
    fn delete_from_cursor(&mut self, ui: &Ui, n: isize) {
        if self.text.is_empty() {
            return;
        }
        if self.selection_end != self.cursor {
            self.write(ui, "");
            return;
        }
        let back = n < 0;
        let a = if back {
            self.cursor as isize + n
        } else {
            self.cursor as isize
        };
        let b = if back {
            self.cursor as isize
        } else {
            self.cursor as isize + n
        };
        let mut out: Vec<char> = Vec::new();
        if a >= 0 {
            out.extend_from_slice(&self.text[..a as usize]);
        }
        if (b as usize) < self.text.len() {
            out.extend_from_slice(&self.text[b as usize..]);
        }
        self.text = out;
        if back {
            self.move_cursor_by(ui, n);
        }
    }

    /// `deleteWords`
    fn delete_words(&mut self, ui: &Ui, n: isize) {
        if self.text.is_empty() {
            return;
        }
        if self.selection_end != self.cursor {
            self.write(ui, "");
        } else {
            let w = self.nth_word(n) as isize - self.cursor as isize;
            self.delete_from_cursor(ui, w);
        }
    }

    /// `getNthWordFromPos` (which always starts at the cursor)
    fn nth_word(&self, n: isize) -> usize {
        let mut p = self.cursor;
        let back = n < 0;
        for _ in 0..n.unsigned_abs() {
            if !back {
                let len = self.text.len();
                match self.text[p.min(len)..].iter().position(|&c| c == ' ') {
                    None => p = len,
                    Some(i) => {
                        p += i;
                        while p < len && self.text[p] == ' ' {
                            p += 1;
                        }
                    }
                }
            } else {
                while p > 0 && self.text[p - 1] == ' ' {
                    p -= 1;
                }
                while p > 0 && self.text[p - 1] != ' ' {
                    p -= 1;
                }
            }
        }
        p
    }

    fn move_cursor_by(&mut self, ui: &Ui, d: isize) {
        let p = self.selection_end as isize + d;
        self.set_cursor(ui, p);
    }

    /// `setCursorPosition`
    fn set_cursor(&mut self, ui: &Ui, p: isize) {
        self.cursor = p.clamp(0, self.text.len() as isize) as usize;
        let c = self.cursor;
        self.set_selection(ui, c as isize);
    }

    fn set_cursor_end(&mut self, ui: &Ui) {
        let n = self.text.len() as isize;
        self.set_cursor(ui, n);
    }

    /// `setSelectionPos`
    fn set_selection(&mut self, ui: &Ui, p: isize) {
        let len = self.text.len();
        let p = p.clamp(0, len as isize) as usize;
        self.selection_end = p;
        if self.scroll > len {
            self.scroll = len;
        }
        let w = self.inner_width();
        let vis = ui
            .trim_to_width(&self.slice(self.scroll), w, false)
            .chars()
            .count();
        let end = vis + self.scroll;
        if p == self.scroll {
            let back = ui.trim_to_width(&self.text(), w, true).chars().count();
            self.scroll = self.scroll.saturating_sub(back);
        }
        if p > end {
            self.scroll += p - end;
        } else if p <= self.scroll {
            self.scroll -= self.scroll - p;
        }
        self.scroll = self.scroll.min(len);
    }

    fn selected(&self) -> String {
        let a = self.cursor.min(self.selection_end);
        let b = self.cursor.max(self.selection_end);
        self.text[a..b].iter().collect()
    }

    /// `textboxKeyTyped`: `c` the character (or '\0'), `code` the LWJGL key.
    pub fn key(
        &mut self,
        ui: &Ui,
        c: char,
        code: i32,
        ctrl: bool,
        shift: bool,
        clip: &mut String,
    ) -> bool {
        if !self.focused {
            return false;
        }
        match c {
            '\u{1}' => {
                self.set_cursor_end(ui);
                self.set_selection(ui, 0);
                return true;
            }
            '\u{3}' => {
                *clip = self.selected();
                return true;
            }
            '\u{16}' => {
                if self.enabled {
                    let s = clip.clone();
                    self.write(ui, &s);
                }
                return true;
            }
            '\u{18}' => {
                *clip = self.selected();
                if self.enabled {
                    self.write(ui, "");
                }
                return true;
            }
            _ => {}
        }
        match code {
            14 => {
                if self.enabled {
                    if ctrl {
                        self.delete_words(ui, -1);
                    } else {
                        self.delete_from_cursor(ui, -1);
                    }
                }
                true
            }
            199 => {
                if shift {
                    self.set_selection(ui, 0);
                } else {
                    self.set_cursor(ui, 0);
                }
                true
            }
            203 | 205 => {
                let d: isize = if code == 203 { -1 } else { 1 };
                if shift {
                    if ctrl {
                        let p = self.nth_word(d) as isize;
                        self.set_selection(ui, p);
                    } else {
                        let p = self.selection_end as isize + d;
                        self.set_selection(ui, p);
                    }
                } else if ctrl {
                    let p = self.nth_word(d) as isize;
                    self.set_cursor(ui, p);
                } else {
                    self.move_cursor_by(ui, d);
                }
                true
            }
            207 => {
                if shift {
                    let n = self.text.len() as isize;
                    self.set_selection(ui, n);
                } else {
                    self.set_cursor_end(ui);
                }
                true
            }
            211 => {
                if self.enabled {
                    if ctrl {
                        self.delete_words(ui, 1);
                    } else {
                        self.delete_from_cursor(ui, 1);
                    }
                }
                true
            }
            _ => {
                if allowed_char(c) {
                    if self.enabled {
                        self.write(ui, &c.to_string());
                    }
                    true
                } else {
                    false
                }
            }
        }
    }

    /// `mouseClicked`
    pub fn click(&mut self, ui: &Ui, m: (f32, f32), button: u8) {
        let inside =
            m.0 >= self.x && m.0 < self.x + self.w && m.1 >= self.y && m.1 < self.y + self.h;
        if self.can_lose_focus {
            self.set_focused(inside);
        }
        if self.focused && button == 0 {
            let mut dx = m.0 - self.x;
            if self.background {
                dx -= 4.0;
            }
            let vis = ui.trim_to_width(&self.slice(self.scroll), self.inner_width(), false);
            let n = ui.trim_to_width(&vis, dx, false).chars().count();
            let p = (n + self.scroll) as isize;
            self.set_cursor(ui, p);
        }
    }

    /// `drawTextBox`
    pub fn draw(&self, ui: &mut Ui) {
        if !self.visible {
            return;
        }
        if self.background {
            ui.rect(
                self.x - 1.0,
                self.y - 1.0,
                self.w + 2.0,
                self.h + 2.0,
                0xA0A0A0,
                1.0,
            );
            ui.rect(self.x, self.y, self.w, self.h, 0x000000, 1.0);
        }
        let color = if self.enabled {
            self.color
        } else {
            self.disabled_color
        };
        let c = self.cursor as isize - self.scroll as isize;
        let mut sel = self.selection_end as isize - self.scroll as isize;
        let vis: Vec<char> = ui
            .trim_to_width(&self.slice(self.scroll), self.inner_width(), false)
            .chars()
            .collect();
        let in_view = c >= 0 && c <= vis.len() as isize;
        let blink = self.focused && self.cursor_counter / 6 % 2 == 0 && in_view;
        let x0 = if self.background {
            self.x + 4.0
        } else {
            self.x
        };
        let y0 = if self.background {
            self.y + ((self.h - 8.0) / 2.0).floor()
        } else {
            self.y
        };
        let mut x = x0;
        if sel > vis.len() as isize {
            sel = vis.len() as isize;
        }
        if !vis.is_empty() {
            let part: String = if in_view {
                vis[..c as usize].iter().collect()
            } else {
                vis.iter().collect()
            };
            x = ui.text(&part, x, y0, color, true);
        }
        let mid = self.cursor < self.text.len() || self.text.len() >= self.max_len;
        let mut cx = x;
        if !in_view {
            cx = if c > 0 { x0 + self.w } else { x0 };
        } else if mid {
            cx -= 1.0;
            x -= 1.0;
        }
        if !vis.is_empty() && in_view && (c as usize) < vis.len() {
            let rest: String = vis[c as usize..].iter().collect();
            ui.text(&rest, x, y0, color, true);
        }
        if blink {
            if mid {
                ui.rect(cx, y0 - 1.0, 1.0, 1.0 + 9.0 + 1.0, 0xD0D0D0, 1.0);
            } else {
                ui.text("_", cx, y0, color, true);
            }
        }
        if sel != c {
            let head: String = vis[..sel.max(0) as usize].iter().collect();
            let sx = x0 + ui.text_width(&head);
            // drawCursorVertical: an inverted (logic op) blue box
            let (mut a, mut b) = (cx, sx - 1.0);
            if a > b {
                std::mem::swap(&mut a, &mut b);
            }
            let r = self.x + self.w;
            let b = b.min(r);
            let a = a.min(r);
            ui.rect(a, y0 - 1.0, b - a, 9.0 + 2.0, 0x0000FF, 0.5);
        }
    }
}

/// `GuiScreen.drawBackground`: the dirt tiles, 32 GUI pixels each, tinted.
pub fn tiled_background(
    ui: &mut Ui,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    v_off: f32,
    tint: u32,
    alpha_top: f32,
    alpha_bottom: f32,
) {
    let t = 32.0;
    let mut y = ((y0 + v_off) / t).floor() * t - v_off;
    while y < y1 {
        let mut x = (x0 / t).floor() * t;
        while x < x1 {
            let ax = x.max(x0);
            let bx = (x + t).min(x1);
            let ay = y.max(y0);
            let by = (y + t).min(y1);
            if bx > ax && by > ay {
                let u0 = (ax - x) / t * 16.0;
                let u1 = (bx - x) / t * 16.0;
                let v0 = (ay - y) / t * 16.0;
                let v1 = (by - y) / t * 16.0;
                let fa = |yy: f32| {
                    if y1 > y0 {
                        alpha_top + (alpha_bottom - alpha_top) * ((yy - y0) / (y1 - y0))
                    } else {
                        alpha_top
                    }
                };
                ui.sheet_gradient(
                    ax,
                    ay,
                    bx - ax,
                    by - ay,
                    768.0 + u0,
                    256.0 + v0,
                    u1 - u0,
                    v1 - v0,
                    tint,
                    fa(ay),
                    fa(by),
                );
            }
            x += t;
        }
        y += t;
    }
}

/// `GuiScreen.drawDefaultBackground` / `drawWorldBackground(0)`
pub fn default_background(ui: &mut Ui, in_world: bool) {
    if in_world {
        crate::screens::gradient(ui, 0.0, 0.0, ui.w, ui.h, 0xC0101010, 0xD0101010);
    } else {
        let (w, h) = (ui.w, ui.h);
        tiled_background(ui, 0.0, 0.0, w, h, 0.0, 0x404040, 1.0, 1.0);
    }
}

/// `GuiSlot`
#[derive(Clone, Debug)]
pub struct SlotList {
    pub width: f32,
    pub height: f32,
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
    pub slot_height: f32,
    pub center_vertically: bool,
    initial_click_y: f32,
    scroll_multiplier: f32,
    pub amount: f32,
    pub selected: i32,
    last_clicked: Option<std::time::Instant>,
    pub show_selection_box: bool,
    pub header_padding: f32,
    pub enabled: bool,
    pub list_width: f32,
    pub scrollbar_x: f32,
}

/// What a frame of `GuiSlot.drawScreen` did with the mouse.
pub enum SlotEvent {
    /// `elementClicked(index, doubleClick, x, y)`
    Clicked(usize, bool),
}

impl SlotList {
    pub fn new(width: f32, height: f32, top: f32, bottom: f32, slot_height: f32) -> SlotList {
        SlotList {
            width,
            height,
            top,
            bottom,
            left: 0.0,
            right: width,
            slot_height,
            center_vertically: true,
            initial_click_y: -2.0,
            scroll_multiplier: 0.0,
            amount: 0.0,
            selected: -1,
            last_clicked: None,
            show_selection_box: true,
            header_padding: 0.0,
            enabled: true,
            list_width: 220.0,
            scrollbar_x: width / 2.0 + 124.0,
        }
    }

    fn max_scroll(&self, content: f32) -> f32 {
        content - (self.bottom - self.top - 4.0)
    }

    /// `bindAmountScrolled`
    fn bind(&mut self, content: f32) {
        let mut m = (self.max_scroll(content) as i32) as f32;
        if m < 0.0 {
            m = ((m as i32) / 2) as f32;
        }
        if !self.center_vertically && m < 0.0 {
            m = 0.0;
        }
        if self.amount < 0.0 {
            self.amount = 0.0;
        }
        if self.amount > m {
            self.amount = m;
        }
    }

    /// `getSlotIndexFromScreenCoords`
    pub fn index_at(&self, m: (f32, f32), size: usize) -> Option<usize> {
        let a = self.left + self.width / 2.0 - self.list_width / 2.0;
        let b = self.left + self.width / 2.0 + self.list_width / 2.0;
        let y = (m.1 - self.top - self.header_padding + self.amount as i32 as f32 - 4.0) as i32;
        let i = y / self.slot_height as i32;
        (m.0 < self.scrollbar_x && m.0 >= a && m.0 <= b && i >= 0 && y >= 0 && (i as usize) < size)
            .then_some(i as usize)
    }

    /// Mouse wheel over the list (`Mouse.getEventDWheel`).
    pub fn wheel(&mut self, m: (f32, f32), up: bool) {
        if m.0 > self.left && m.0 < self.right && m.1 > self.top && m.1 < self.bottom {
            let d = if up { -1.0 } else { 1.0 };
            self.amount += (d * self.slot_height / 2.0) as i32 as f32;
        }
    }

    /// `drawScreen`: the input part, then the drawing. `slot` draws entry
    /// `i` at (x, y) with height `h`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        ui: &mut Ui,
        m: (f32, f32),
        down: bool,
        size: usize,
        content: Option<f32>,
        events: &mut Vec<SlotEvent>,
        slot: &mut dyn FnMut(&mut Ui, usize, f32, f32, f32),
    ) {
        let content = content.unwrap_or(size as f32 * self.slot_height + self.header_padding);
        let sx = self.scrollbar_x;
        let sx1 = sx + 6.0;
        if m.0 > self.left && m.0 < self.right && m.1 > self.top && m.1 < self.bottom {
            if !down || !self.enabled {
                self.initial_click_y = -1.0;
            } else if self.initial_click_y == -1.0 {
                let mut drag = true;
                if m.1 >= self.top && m.1 <= self.bottom {
                    let a = self.width / 2.0 - self.list_width / 2.0;
                    let b = self.width / 2.0 + self.list_width / 2.0;
                    let y = (m.1 - self.top - self.header_padding + self.amount as i32 as f32 - 4.0)
                        as i32;
                    let i = y / self.slot_height as i32;
                    if m.0 >= a && m.0 <= b && i >= 0 && y >= 0 && (i as usize) < size {
                        let dbl = i == self.selected
                            && self
                                .last_clicked
                                .is_some_and(|t| t.elapsed().as_millis() < 250);
                        events.push(SlotEvent::Clicked(i as usize, dbl));
                        self.selected = i;
                        self.last_clicked = Some(std::time::Instant::now());
                    } else if m.0 >= a && m.0 <= b && y < 0 {
                        drag = false;
                    }
                    if m.0 >= sx && m.0 <= sx1 {
                        self.scroll_multiplier = -1.0;
                        let mut ms = self.max_scroll(content) as i32;
                        if ms < 1 {
                            ms = 1;
                        }
                        let span = self.bottom - self.top;
                        let mut k = ((span * span) / content) as i32;
                        k = k.max(32).min((span - 8.0) as i32);
                        self.scroll_multiplier /= (span - k as f32) / ms as f32;
                    } else {
                        self.scroll_multiplier = 1.0;
                    }
                    self.initial_click_y = if drag { m.1 } else { -2.0 };
                } else {
                    self.initial_click_y = -2.0;
                }
            } else if self.initial_click_y >= 0.0 {
                self.amount -= (m.1 - self.initial_click_y) * self.scroll_multiplier;
                self.initial_click_y = m.1;
            }
        }
        self.bind(content);
        let am = self.amount as i32 as f32;
        // the list's dirt, darker
        tiled_background(
            ui,
            self.left,
            self.top,
            self.right,
            self.bottom,
            am,
            0x202020,
            1.0,
            1.0,
        );
        let x = self.left + self.width / 2.0 - self.list_width / 2.0 + 2.0;
        let y = self.top + 4.0 - am;
        // drawSelectionBox
        for i in 0..size {
            let ey = y + i as f32 * self.slot_height + self.header_padding;
            let eh = self.slot_height - 4.0;
            if ey <= self.bottom && ey + eh >= self.top {
                if self.show_selection_box && i as i32 == self.selected {
                    let a = self.left + (self.width / 2.0 - self.list_width / 2.0);
                    let b = self.left + self.width / 2.0 + self.list_width / 2.0;
                    ui.rect(a, ey - 2.0, b - a, eh + 4.0, 0x808080, 1.0);
                    ui.rect(a + 1.0, ey - 1.0, b - a - 2.0, eh + 2.0, 0x000000, 1.0);
                }
                slot(ui, i, x, ey, eh);
            }
        }
        // overlayBackground above and below, then the 4-pixel shadows
        let (w, h) = (self.width, self.height);
        tiled_background(
            ui,
            self.left,
            0.0,
            self.left + w,
            self.top,
            0.0,
            0x404040,
            1.0,
            1.0,
        );
        tiled_background(
            ui,
            self.left,
            self.bottom,
            self.left + w,
            h,
            0.0,
            0x404040,
            1.0,
            1.0,
        );
        crate::screens::gradient(
            ui,
            self.left,
            self.top,
            self.right - self.left,
            4.0,
            0xFF000000,
            0x00000000,
        );
        crate::screens::gradient(
            ui,
            self.left,
            self.bottom - 4.0,
            self.right - self.left,
            4.0,
            0x00000000,
            0xFF000000,
        );
        let ms = self.max_scroll(content) as i32;
        if ms > 0 {
            let span = self.bottom - self.top;
            let mut k = ((span * span) / content) as i32;
            k = k.max(32).min((span - 8.0) as i32);
            let mut ky = (am as i32 * (span as i32 - k) / ms) as f32 + self.top;
            if ky < self.top {
                ky = self.top;
            }
            ui.rect(sx, self.top, 6.0, span, 0x000000, 1.0);
            ui.rect(sx, ky, 6.0, k as f32, 0x808080, 1.0);
            ui.rect(sx, ky, 5.0, k as f32 - 1.0, 0xC0C0C0, 1.0);
        }
    }
}

/// `drawTexturedModalRect` of a sheet texture placed by `gui_pos`.
pub fn tex_rect(ui: &mut Ui, path: &str, x: f32, y: f32, u: f32, v: f32, w: f32, h: f32) {
    let (ox, oy) = gui_pos(path);
    ui.sheet(x, y, ox + u, oy + v, w, h);
}
