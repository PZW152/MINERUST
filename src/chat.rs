//! `GuiNewChat` (the chat lines over the hotbar) and `GuiChat` (the input
//! line), and the chat message the server sends (`S02PacketChat` with a
//! `ChatComponentTranslation`).

use crate::gui::Ui;
use crate::lang::Lang;
use crate::menu::widgets::TextField;
use crate::settings::GameSettings;

/// A `ChatComponentTranslation` (or plain text when `key` is not a
/// translation key) with its `ChatStyle` colour.
#[derive(Clone, Debug, PartialEq)]
pub struct ChatMsg {
    pub key: String,
    pub args: Vec<String>,
    /// `EnumChatFormatting` code (`'c'` red, `'7'` grey, ...).
    pub color: Option<char>,
    pub italic: bool,
}

impl ChatMsg {
    pub fn tr(key: &str, args: Vec<String>) -> ChatMsg {
        ChatMsg {
            key: key.to_string(),
            args,
            color: None,
            italic: false,
        }
    }
    pub fn red(key: &str, args: Vec<String>) -> ChatMsg {
        ChatMsg {
            color: Some('c'),
            ..ChatMsg::tr(key, args)
        }
    }

    /// `getFormattedText`
    pub fn text(&self, lang: &Lang) -> String {
        let mut s = String::new();
        if let Some(c) = self.color {
            s.push('\u{a7}');
            s.push(c);
        }
        if self.italic {
            s.push_str("\u{a7}o");
        }
        let args: Vec<String> = self.args.iter().map(|a| arg_text(a, lang)).collect();
        s.push_str(&lang.format_raw(&self.key, &args));
        s
    }
}

/// An argument: `\u{1}key` is a translation, `\u{2}<colour><id>:<meta>`
/// an item's `[name]`, anything else plain text.
fn arg_text(a: &str, lang: &Lang) -> String {
    if let Some(k) = a.strip_prefix('\u{1}') {
        return lang.get(k).to_string();
    }
    if let Some(r) = a.strip_prefix('\u{2}') {
        let mut it = r.chars();
        let c = it.next().unwrap_or('f');
        let rest: String = it.collect();
        let (id, meta) = rest.split_once(':').unwrap_or(("0", "0"));
        let st = crate::item::Stack::new(id.parse().unwrap_or(0), meta.parse().unwrap_or(0), 1);
        let name = crate::item_text::display_name(&st, lang);
        return if c == 'f' {
            format!("[{name}]")
        } else {
            format!("\u{a7}{c}[{name}]\u{a7}r")
        };
    }
    a.to_string()
}

struct Line {
    counter: i32,
    text: String,
}

#[derive(Default)]
pub struct ChatGui {
    /// `chatLines` (newest first) and `drawnChatLines` (wrapped).
    lines: Vec<Line>,
    drawn: Vec<Line>,
    /// `sentMessages`
    pub sent: Vec<String>,
    scroll: i32,
    scrolled: bool,
}

/// `calculateChatboxWidth`
fn chat_width(s: &GameSettings) -> i32 {
    (s.chat_width * (320.0 - 40.0) + 40.0).floor() as i32
}

/// `calculateChatboxHeight`
fn chat_height(s: &GameSettings, open: bool) -> i32 {
    let v = if open {
        s.chat_height_focused
    } else {
        s.chat_height_unfocused
    };
    (v * (180.0 - 20.0) + 20.0).floor() as i32
}

fn line_count(s: &GameSettings, open: bool) -> i32 {
    chat_height(s, open) / 9
}

/// `GuiNewChat.formatColors`
fn format_colors(s: &str, colours: bool) -> String {
    if colours {
        s.to_string()
    } else {
        // EnumChatFormatting.getTextWithoutFormattingCodes
        let mut out = String::new();
        let mut it = s.chars();
        while let Some(c) = it.next() {
            if c == '\u{a7}' {
                it.next();
            } else {
                out.push(c);
            }
        }
        out
    }
}

impl ChatGui {
    /// `printChatMessage`: wrapped to the chat width (`setChatLine`).
    pub fn print(&mut self, ui: &Ui, s: &GameSettings, text: &str, counter: i32, open: bool) {
        self.set_line(ui, s, text, counter, open, false);
    }

    fn set_line(
        &mut self,
        ui: &Ui,
        s: &GameSettings,
        text: &str,
        counter: i32,
        open: bool,
        refresh: bool,
    ) {
        let width = (chat_width(s) as f32 / s.chat_scale).floor();
        let mut pieces = vec![format_colors(text, s.chat_colours)];
        let mut out: Vec<String> = Vec::new();
        let mut cur = String::new();
        let mut used = 0.0f32;
        let mut k = 0;
        while k < pieces.len() {
            let mut p = pieces[k].clone();
            let mut w = ui.text_width(&p);
            let mut brk = false;
            if used + w > width {
                let mut head = ui.trim_to_width(&p, width - used, false);
                let mut tail = if head.chars().count() < p.chars().count() {
                    Some(p.chars().skip(head.chars().count()).collect::<String>())
                } else {
                    None
                };
                if tail.as_ref().is_some_and(|t| !t.is_empty()) {
                    if let Some(sp) = head.rfind(' ') {
                        if ui.text_width(&p[..sp]) > 0.0 {
                            head = p[..sp].to_string();
                            tail = Some(p[sp..].to_string());
                        }
                    }
                    pieces.insert(k + 1, tail.unwrap());
                }
                p = head;
                w = ui.text_width(&p);
                brk = true;
            }
            if used + w <= width {
                used += w;
                cur.push_str(&p);
            } else {
                brk = true;
            }
            if brk {
                out.push(std::mem::take(&mut cur));
                used = 0.0;
            }
            k += 1;
        }
        out.push(cur);
        for l in out {
            if open && self.scroll > 0 {
                self.scrolled = true;
                self.scroll_by(s, open, 1);
            }
            self.drawn.insert(0, Line { counter, text: l });
        }
        self.drawn.truncate(100);
        if !refresh {
            self.lines.insert(
                0,
                Line {
                    counter,
                    text: text.to_string(),
                },
            );
            self.lines.truncate(100);
        }
    }

    /// `refreshChat` (after the chat settings change)
    pub fn refresh(&mut self, ui: &Ui, s: &GameSettings) {
        self.drawn.clear();
        self.reset_scroll();
        let lines: Vec<(i32, String)> = self
            .lines
            .iter()
            .rev()
            .map(|l| (l.counter, l.text.clone()))
            .collect();
        for (c, t) in lines {
            self.set_line(ui, s, &t, c, false, true);
        }
    }

    /// `addToSentMessages`
    pub fn add_sent(&mut self, m: &str) {
        if self.sent.last().map(String::as_str) != Some(m) {
            self.sent.push(m.to_string());
        }
    }

    pub fn reset_scroll(&mut self) {
        self.scroll = 0;
        self.scrolled = false;
    }

    /// `scroll`
    pub fn scroll_by(&mut self, s: &GameSettings, open: bool, n: i32) {
        self.scroll += n;
        let max = self.drawn.len() as i32 - line_count(s, open);
        if self.scroll > max {
            self.scroll = max;
        }
        if self.scroll <= 0 {
            self.scroll = 0;
            self.scrolled = false;
        }
    }

    pub fn lines_per_page(s: &GameSettings, open: bool) -> i32 {
        line_count(s, open)
    }

    /// `drawChat`, under `glTranslatef(0, height - 48, 0)`.
    pub fn draw(&self, ui: &mut Ui, s: &GameSettings, counter: i32, open: bool) {
        if s.chat_visibility == 2 || self.drawn.is_empty() {
            return;
        }
        let base_y = ui.h - 48.0;
        let lines = line_count(s, open);
        let opacity = s.chat_opacity * 0.9 + 0.1;
        let scale = s.chat_scale;
        let w = (chat_width(s) as f32 / scale).ceil();
        let mut shown = 0;
        let start = ui.v.len();
        let mut i = 0;
        while (i + self.scroll) < self.drawn.len() as i32 && i < lines {
            let l = &self.drawn[(i + self.scroll) as usize];
            let age = counter - l.counter;
            if age < 200 || open {
                let mut f = 1.0 - age as f64 / 200.0;
                f *= 10.0;
                f = f.clamp(0.0, 1.0);
                f *= f;
                let mut a = (255.0 * f) as i32;
                if open {
                    a = 255;
                }
                a = (a as f32 * opacity) as i32;
                shown += 1;
                if a > 3 {
                    let y = -i * 9;
                    ui.rect(0.0, (y - 9) as f32, w + 4.0, 9.0, 0, (a / 2) as f32 / 255.0);
                    ui.text_alpha_shadow(
                        &l.text,
                        0.0,
                        (y - 8) as f32,
                        0xFFFFFF,
                        a as f32 / 255.0,
                        true,
                    );
                }
            }
            i += 1;
        }
        if open {
            let n = self.drawn.len() as i32;
            let all = n * 9 + n;
            let vis = shown * 9 + shown;
            let off = self.scroll * vis / n.max(1);
            let bar = vis * vis / all.max(1);
            if all != vis {
                let a = if off > 0 { 170 } else { 96 } as f32 / 255.0;
                let c = if self.scrolled { 13382451 } else { 3355562 };
                ui.rect(-3.0, (-off - bar) as f32, 2.0, bar as f32, c, a);
                ui.rect(-2.0, (-off - bar) as f32, 1.0, bar as f32, 13421772, a);
            }
        }
        // glTranslatef(2, 20) and glScalef(scale) under the hotbar offset
        for v in &mut ui.v[start..] {
            v.pos[0] = 2.0 + v.pos[0] * scale;
            v.pos[1] = base_y + 20.0 + v.pos[1] * scale;
        }
    }
}

/// `GuiChat`
pub struct ChatScreen {
    pub field: TextField,
    history_cursor: usize,
    history_buffer: String,
}

/// What a key in the chat screen did.
pub enum ChatKey {
    None,
    Close,
    Send(String),
}

impl ChatScreen {
    /// `initGui` with `defaultInputFieldText`
    pub fn new(ui: &Ui, chat: &ChatGui, text: &str) -> ChatScreen {
        let mut field = TextField::new(4.0, ui.h - 12.0, ui.w - 4.0, 12.0);
        field.max_len = 100;
        field.background = false;
        field.focused = true;
        field.can_lose_focus = false;
        field.set_text(ui, text);
        ChatScreen {
            field,
            history_cursor: chat.sent.len(),
            history_buffer: String::new(),
        }
    }

    /// `getSentHistory`
    fn history(&mut self, ui: &Ui, chat: &ChatGui, d: i32) {
        let n = chat.sent.len() as i32;
        let k = (self.history_cursor as i32 + d).clamp(0, n);
        if k as usize != self.history_cursor {
            if k == n {
                self.history_cursor = n as usize;
                let b = self.history_buffer.clone();
                self.field.set_text(ui, &b);
            } else {
                if self.history_cursor as i32 == n {
                    self.history_buffer = self.field.text();
                }
                self.field.set_text(ui, &chat.sent[k as usize]);
                self.history_cursor = k as usize;
            }
        }
    }

    /// `keyTyped`
    #[allow(clippy::too_many_arguments)]
    pub fn key(
        &mut self,
        ui: &Ui,
        chat: &mut ChatGui,
        s: &GameSettings,
        ch: char,
        code: i32,
        ctrl: bool,
        shift: bool,
        clip: &mut String,
    ) -> ChatKey {
        match code {
            1 => ChatKey::Close,
            28 | 156 => {
                let t = self.field.text().trim().to_string();
                if t.is_empty() {
                    ChatKey::Close
                } else {
                    chat.add_sent(&t);
                    ChatKey::Send(t)
                }
            }
            200 => {
                self.history(ui, chat, -1);
                ChatKey::None
            }
            208 => {
                self.history(ui, chat, 1);
                ChatKey::None
            }
            201 => {
                chat.scroll_by(s, true, ChatGui::lines_per_page(s, true) - 1);
                ChatKey::None
            }
            209 => {
                chat.scroll_by(s, true, -ChatGui::lines_per_page(s, true) + 1);
                ChatKey::None
            }
            _ => {
                self.field.key(ui, ch, code, ctrl, shift, clip);
                ChatKey::None
            }
        }
    }

    /// `handleMouseInput`: the wheel scrolls seven lines (one with shift).
    pub fn wheel(&mut self, chat: &mut ChatGui, s: &GameSettings, up: bool, shift: bool) {
        let mut d = if up { 1 } else { -1 };
        if !shift {
            d *= 7;
        }
        chat.scroll_by(s, true, d);
    }

    /// `drawScreen`
    pub fn draw(&mut self, ui: &mut Ui) {
        ui.rect(2.0, ui.h - 14.0, ui.w - 4.0, 12.0, 0, 0x80 as f32 / 255.0);
        self.field.x = 4.0;
        self.field.y = ui.h - 12.0;
        self.field.w = ui.w - 4.0;
        self.field.draw(ui);
    }
}
