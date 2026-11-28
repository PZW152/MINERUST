//! `GuiScreenBook`: reading a written book, and writing, paging and
//! signing a book and quill (`MC|BEdit`, `MC|BSign`).

use crate::gui::Ui;
use crate::item::Stack;
use crate::lang::Lang;
use crate::world::anvil::Nbt;

/// `textures/gui/book.png` on the GUI sheet.
const BOOK: (f32, f32) = (512.0, 1024.0);
const W: f32 = 192.0;

/// What the screen asks for after an input.
#[derive(Clone, Debug, PartialEq)]
pub enum BookAction {
    None,
    /// Close; with the book to send (pages, and the title when signed).
    Close(Option<(Vec<String>, Option<String>)>),
}

pub struct BookScreen {
    pub pages: Vec<String>,
    pub page: usize,
    pub unsigned: bool,
    pub signing: bool,
    pub title: String,
    pub modified: bool,
    pub ticks: u32,
    pub author: String,
}

/// `ChatAllowedCharacters.isAllowedCharacter`
fn allowed(c: char) -> bool {
    c != '\u{a7}' && c >= ' ' && c != '\u{7f}'
}

impl BookScreen {
    /// `new GuiScreenBook(player, stack, unsigned)`
    pub fn open(s: &Stack, author: &str) -> BookScreen {
        let unsigned = s.id == 386;
        let mut pages: Vec<String> = s
            .nbt()
            .map(|t| {
                t.list("pages")
                    .iter()
                    .filter_map(|p| match p {
                        Nbt::String(x) => Some(x.clone()),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        if pages.is_empty() && unsigned {
            pages.push(String::new());
        }
        let author = s
            .nbt()
            .and_then(|t| t.string("author").map(|a| a.to_string()))
            .unwrap_or_else(|| author.to_string());
        BookScreen {
            pages,
            page: 0,
            unsigned,
            signing: false,
            title: String::new(),
            modified: false,
            ticks: 0,
            author,
        }
    }

    fn total(&self) -> usize {
        self.pages.len().max(1)
    }

    /// The buttons: id, x, y, width, label key, visible, enabled.
    fn buttons(&self, w: f32) -> Vec<(i32, f32, f32, f32, &'static str, bool, bool)> {
        let x = (w / 2.0).trunc();
        let y = 4.0 + W;
        let mut v = Vec::new();
        if self.unsigned {
            v.push((3, x - 100.0, y, 98.0, "book.signButton", !self.signing, true));
            v.push((0, x + 2.0, y, 98.0, "gui.done", !self.signing, true));
            v.push((
                5,
                x - 100.0,
                y,
                98.0,
                "book.finalizeButton",
                self.signing,
                !self.title.trim().is_empty(),
            ));
            v.push((4, x + 2.0, y, 98.0, "gui.cancel", self.signing, true));
        } else {
            v.push((0, x - 100.0, y, 200.0, "gui.done", true, true));
        }
        v
    }

    /// The page turners: forward and back, (x, y), visible.
    fn turners(&self, w: f32) -> [(bool, f32, f32, bool); 2] {
        let left = ((w - W) / 2.0).trunc();
        [
            (
                true,
                left + 120.0,
                156.0,
                !self.signing && (self.page + 1 < self.total() || self.unsigned),
            ),
            (false, left + 38.0, 156.0, !self.signing && self.page > 0),
        ]
    }

    fn send(&mut self, sign: bool) -> Option<(Vec<String>, Option<String>)> {
        if !(self.unsigned && self.modified) {
            return None;
        }
        // sendBookToServer: empty pages at the end go
        while self.pages.len() > 1 && self.pages.last().is_some_and(|p| p.is_empty()) {
            self.pages.pop();
        }
        Some((self.pages.clone(), sign.then(|| self.title.trim().to_string())))
    }

    /// `GuiScreenBook.mouseClicked` / `actionPerformed`; true when a
    /// button was pressed (for its sound).
    pub fn click(&mut self, w: f32, m: (f32, f32)) -> (bool, BookAction) {
        for (fwd, x, y, vis) in self.turners(w) {
            if vis && m.0 >= x && m.1 >= y && m.0 < x + 23.0 && m.1 < y + 13.0 {
                if fwd {
                    if self.page + 1 < self.total() {
                        self.page += 1;
                    } else if self.unsigned {
                        // addNewPage
                        if self.pages.len() < 50 {
                            self.pages.push(String::new());
                            self.modified = true;
                        }
                        if self.page + 1 < self.total() {
                            self.page += 1;
                        }
                    }
                } else if self.page > 0 {
                    self.page -= 1;
                }
                return (true, BookAction::None);
            }
        }
        for (id, x, y, bw, _, vis, en) in self.buttons(w) {
            if !(vis && en && m.0 >= x && m.1 >= y && m.0 < x + bw && m.1 < y + 20.0) {
                continue;
            }
            return match id {
                0 => (true, BookAction::Close(self.send(false))),
                3 => {
                    self.signing = true;
                    (true, BookAction::None)
                }
                5 => (true, BookAction::Close(self.send(true))),
                4 => {
                    self.signing = false;
                    (true, BookAction::None)
                }
                _ => (true, BookAction::None),
            };
        }
        (false, BookAction::None)
    }

    /// `pageInsertIntoCurrent`: only while the page still fits its 118
    /// pixels with the cursor, and under 256 characters.
    fn insert(&mut self, ui: &Ui, s: &str) {
        let Some(cur) = self.pages.get(self.page) else {
            return;
        };
        let next = format!("{cur}{s}");
        let h = ui.wrap(&format!("{next}\u{a7}0_"), 118.0).len() as f32 * 9.0;
        if h <= 118.0 && next.chars().count() < 256 {
            self.pages[self.page] = next;
            self.modified = true;
        }
    }

    /// `GuiScreenBook.keyTyped` (Escape closes without sending).
    pub fn key(&mut self, ui: &Ui, ch: char, code: i32, clipboard: &str) -> BookAction {
        if code == 1 {
            return BookAction::Close(None);
        }
        if !self.unsigned {
            return BookAction::None;
        }
        if self.signing {
            match code {
                14 => {
                    self.title.pop();
                }
                28 | 156 => {
                    if !self.title.is_empty() {
                        return BookAction::Close(self.send(true));
                    }
                }
                _ => {
                    if self.title.chars().count() < 16 && allowed(ch) {
                        self.title.push(ch);
                        self.modified = true;
                    }
                }
            }
            return BookAction::None;
        }
        if ch == '\u{16}' {
            self.insert(ui, clipboard);
            return BookAction::None;
        }
        match code {
            14 => {
                if let Some(p) = self.pages.get_mut(self.page) {
                    if p.pop().is_some() {
                        self.modified = true;
                    }
                }
            }
            28 | 156 => self.insert(ui, "\n"),
            _ => {
                if allowed(ch) {
                    self.insert(ui, &ch.to_string());
                }
            }
        }
        BookAction::None
    }

    /// `drawSplitString`: the lines of `s` wrapped to `width`, 9 apart.
    fn split(ui: &mut Ui, s: &str, x: f32, y: f32, width: f32) {
        let t = s.trim_end_matches('\n');
        for (i, l) in ui.wrap(t, width).iter().enumerate() {
            ui.text(l, x, y + i as f32 * 9.0, 0, false);
        }
    }

    /// `GuiScreenBook.drawScreen`
    pub fn draw(&self, ui: &mut Ui, lang: &Lang, w: f32, m: (f32, f32)) {
        let left = ((w - W) / 2.0).trunc();
        let top = 2.0;
        ui.sheet(left, top, BOOK.0, BOOK.1, W, W);
        let cursor = if self.ticks / 6 % 2 == 0 {
            "\u{a7}0_"
        } else {
            "\u{a7}7_"
        };
        if self.signing {
            let mut t = self.title.clone();
            if self.unsigned {
                t.push_str(cursor);
            }
            let head = lang.get("book.editTitle");
            let hw = ui.text_width(head);
            ui.text(head, left + 36.0 + ((116.0 - hw) / 2.0).trunc(), top + 32.0, 0, false);
            let tw = ui.text_width(&t);
            ui.text(&t, left + 36.0 + ((116.0 - tw) / 2.0).trunc(), top + 48.0, 0, false);
            let by = lang.format("book.byAuthor", &[self.author.clone()]);
            let bw = ui.text_width(&by);
            ui.text(
                &format!("\u{a7}8{by}"),
                left + 36.0 + ((116.0 - bw) / 2.0).trunc(),
                top + 58.0,
                0,
                false,
            );
            Self::split(ui, lang.get("book.finalizeWarning"), left + 36.0, top + 80.0, 116.0);
        } else {
            let ind = lang.format(
                "book.pageIndicator",
                &[(self.page + 1).to_string(), self.total().to_string()],
            );
            let iw = ui.text_width(&ind);
            ui.text(&ind, left - iw + W - 44.0, top + 16.0, 0, false);
            let mut p = self.pages.get(self.page).cloned().unwrap_or_default();
            if self.unsigned {
                p.push_str(cursor);
            }
            Self::split(ui, &p, left + 36.0, top + 32.0, 116.0);
        }
        for (fwd, x, y, vis) in self.turners(w) {
            if !vis {
                continue;
            }
            let over = m.0 >= x && m.1 >= y && m.0 < x + 23.0 && m.1 < y + 13.0;
            let u = if over { 23.0 } else { 0.0 };
            let v = if fwd { 192.0 } else { 205.0 };
            ui.sheet(x, y, BOOK.0 + u, BOOK.1 + v, 23.0, 13.0);
        }
        for (_, x, y, bw, key, vis, en) in self.buttons(w) {
            if vis {
                crate::screens::button(ui, x, y, bw, lang.get(key), en, m);
            }
        }
    }
}

/// The book as it goes back into the hand: `MC|BEdit` keeps a book and
/// quill with new pages, `MC|BSign` makes it a written book with the
/// author and title.
pub fn apply(held: Stack, pages: &[String], sign: Option<&str>, author: &str) -> Option<Stack> {
    if held.id != 386 {
        return None;
    }
    let list = Nbt::List(8, pages.iter().map(|p| Nbt::String(p.clone())).collect());
    let mut s = held.edit_tag(|t| t.set("pages", list));
    if let Some(title) = sign {
        s = s.edit_tag(|t| {
            t.set("author", Nbt::String(author.to_string()));
            t.set("title", Nbt::String(title.to_string()));
        });
        s.id = 387;
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn books_are_written_and_signed() {
        let s = apply(Stack::new(386, 0, 1), &["Hello".into(), "World".into()], None, "Player").unwrap();
        assert_eq!(s.id, 386);
        let b = BookScreen::open(&s, "Player");
        assert_eq!(b.pages, vec!["Hello".to_string(), "World".to_string()]);
        assert!(b.unsigned);
        let w = apply(s, &["Hello".into()], Some("Diary"), "Player").unwrap();
        assert_eq!(w.id, 387);
        let t = w.nbt().unwrap();
        assert_eq!(t.string("title"), Some("Diary"));
        assert_eq!(t.string("author"), Some("Player"));
        assert!(!BookScreen::open(&w, "x").unsigned);
    }
}
