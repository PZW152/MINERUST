//! `GuiRepair`: the anvil screen's name field, the cost line and the
//! parts of `anvil.png` drawn over the panel. The slots and the rest of
//! `GuiContainer` are in `gui_container.rs`.

use crate::gui::Ui;
use crate::item::Stack;
use crate::lang::Lang;
use crate::menu::widgets::TextField;
use crate::play::InvView;

/// `anvil.png` on the UI sheet.
pub const ANVIL: (f32, f32) = (1280.0, 768.0);
const XS: f32 = 176.0;
const YS: f32 = 166.0;

pub struct GuiRepair {
    /// `nameField`
    pub field: TextField,
    /// Slot 0 as `Container.detectAndSendChanges` last reported it
    /// (`None` before `addCraftingToCrafters`).
    last: Option<Option<Stack>>,
    /// `ContainerRepair.repairedItemName` of the client's container.
    pub name: String,
}

impl GuiRepair {
    /// `initGui`
    pub fn new(left: i32, top: i32) -> GuiRepair {
        let mut field = TextField::new(left as f32 + 62.0, top as f32 + 24.0, 103.0, 12.0);
        field.color = 0xFFFFFF;
        field.disabled_color = 0xFFFFFF;
        field.background = false;
        field.max_len = 40;
        GuiRepair {
            field,
            last: None,
            name: String::new(),
        }
    }

    /// The screen was laid out again.
    pub fn place(&mut self, left: i32, top: i32) {
        self.field.x = left as f32 + 62.0;
        self.field.y = top as f32 + 24.0;
    }

    /// `sendSlotContents(container, 0, stack)` whenever slot 0 changed:
    /// the field shows the stack's name. Returns the `MC|ItemName` to send.
    pub fn sync(&mut self, ui: &Ui, slot0: Option<Stack>, lang: &Lang) -> Option<String> {
        if self.last == Some(slot0) {
            return None;
        }
        self.last = Some(slot0);
        let shown = slot0.map_or(String::new(), |s| crate::item_text::display_name(&s, lang));
        self.field.set_text(ui, &shown);
        self.field.enabled = slot0.is_some();
        slot0.map(|s| self.rename(Some(s), lang))
    }

    /// `renameItem`: an unchanged default name is sent as "".
    fn rename(&mut self, slot0: Option<Stack>, lang: &Lang) -> String {
        let mut t = self.field.text();
        if let Some(s) = slot0 {
            if s.custom_name().is_none() && t == crate::item_text::display_name(&s, lang) {
                t.clear();
            }
        }
        self.name = t.clone();
        t
    }

    /// `keyTyped`: `Some(name)` when the field took the key.
    #[allow(clippy::too_many_arguments)]
    pub fn key(
        &mut self,
        ui: &Ui,
        c: char,
        code: i32,
        ctrl: bool,
        shift: bool,
        clip: &mut String,
        slot0: Option<Stack>,
        lang: &Lang,
    ) -> Option<String> {
        if self.field.key(ui, c, code, ctrl, shift, clip) {
            Some(self.rename(slot0, lang))
        } else {
            None
        }
    }

    /// `drawScreen` after the container: `nameField.drawTextBox`.
    pub fn draw(&self, ui: &mut Ui) {
        self.field.draw(ui);
    }
}

/// `drawGuiContainerBackgroundLayer`
pub fn background(ui: &mut Ui, view: &InvView, left: f32, top: f32) {
    let has = |k: usize| view.tile_slots.get(k).copied().flatten().is_some();
    ui.sheet(left, top, ANVIL.0, ANVIL.1, XS, YS);
    let v = YS + if has(0) { 0.0 } else { 16.0 };
    ui.sheet(left + 59.0, top + 20.0, ANVIL.0, ANVIL.1 + v, 110.0, 16.0);
    if (has(0) || has(1)) && view.result.is_none() {
        ui.sheet(left + 99.0, top + 45.0, ANVIL.0 + XS, ANVIL.1, 28.0, 21.0);
    }
}

/// `drawGuiContainerForegroundLayer`: the title and the cost.
pub fn foreground(
    ui: &mut Ui,
    view: &InvView,
    lang: &Lang,
    left: f32,
    top: f32,
    creative: bool,
    xp_level: i32,
) {
    ui.text(
        lang.get("container.repair"),
        left + 60.0,
        top + 6.0,
        4210752,
        false,
    );
    let cost = view.repair_cost;
    if cost <= 0 {
        return;
    }
    let mut color = 8453920u32;
    let mut show = true;
    let mut s = lang.format_raw("container.repair.cost", &[cost.to_string()]);
    if cost >= 40 && !creative {
        s = lang.get("container.repair.expensive").to_string();
        color = 16736352;
    } else if view.result.is_none() {
        show = false;
    } else if !((creative || xp_level >= cost) && cost > 0) {
        // ContainerRepair$2.canTakeStack
        color = 16736352;
    }
    if !show {
        return;
    }
    let shadow = (color & 16579836) >> 2;
    let x = (XS - 8.0 - ui.text_width(&s)).trunc();
    let y = 67.0;
    if ui.unicode_flag() {
        ui.rect(
            left + x - 3.0,
            top + y - 2.0,
            XS - 7.0 - (x - 3.0),
            12.0,
            0x000000,
            1.0,
        );
        ui.rect(
            left + x - 2.0,
            top + y - 1.0,
            XS - 8.0 - (x - 2.0),
            10.0,
            0x3B3B3B,
            1.0,
        );
    } else {
        ui.text(&s, left + x, top + y + 1.0, shadow, false);
        ui.text(&s, left + x + 1.0, top + y, shadow, false);
        ui.text(&s, left + x + 1.0, top + y + 1.0, shadow, false);
    }
    ui.text(&s, left + x, top + y, color, false);
}
