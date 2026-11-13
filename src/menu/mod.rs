//! The menus: `GuiMainMenu`, `GuiSelectWorld`, `GuiCreateWorld`,
//! `GuiRenameWorld`, `GuiYesNo`, `GuiMultiplayer`, `GuiOptions`,
//! `GuiVideoSettings`, `GuiScreenOptionsSounds`, `GuiControls`,
//! `GuiLanguage`, `GuiIngameMenu`, and the screens they lead to, as a stack
//! in place of `Minecraft.displayGuiScreen`.

pub mod widgets;

use crate::gui::Ui;
use crate::lang::{Lang, Language};
use crate::settings::{GameSettings, Opt};
use std::path::{Path, PathBuf};
use widgets::{default_background, Button, Kind, SlotEvent, SlotList, TextField};

/// What the menus ask of the game.
#[derive(Clone, Debug)]
pub enum Action {
    None,
    /// `Minecraft.shutdown`
    Quit,
    /// `launchIntegratedServer(folder, name, settings)`
    Play {
        folder: PathBuf,
        new: Option<NewWorld>,
    },
    /// `displayGuiScreen(null)` in a world: back to the game.
    Resume,
    /// `menu.returnToMenu`: save, leave the world, show the title screen.
    QuitToTitle,
    /// The language or the font changed: reload `Lang`.
    Language,
    /// `options.txt` changed (render distance, GUI scale, FOV, ...).
    Settings,
}

/// `WorldSettings` of `GuiCreateWorld`.
#[derive(Clone, Debug)]
pub struct NewWorld {
    pub name: String,
    pub seed: i64,
    /// survival, hardcore or creative
    pub mode: String,
    pub structures: bool,
    pub bonus_chest: bool,
    pub cheats: bool,
    /// `WorldType.worldTypes` index
    pub world_type: usize,
}

/// `WorldType`: the creatable ones in `worldTypes` order.
pub const WORLD_TYPES: [(&str, &str); 4] = [
    ("default", "generator.default"),
    ("flat", "generator.flat"),
    ("largeBiomes", "generator.largeBiomes"),
    ("amplified", "generator.amplified"),
];

/// What the screens need of the game.
pub struct Ctx<'a> {
    pub settings: &'a mut GameSettings,
    pub lang: &'a Lang,
    pub languages: &'a [Language],
    pub saves: &'a Path,
    /// `mc.theWorld != null`
    pub in_world: bool,
    pub hardcore: bool,
    pub clipboard: &'a mut String,
    /// Sounds to play (`gui.button.press`).
    pub sounds: &'a mut Vec<&'static str>,
    pub ctrl: bool,
    pub shift: bool,
}

/// `SaveFormatComparator`
#[derive(Clone, Debug)]
pub struct SaveInfo {
    pub folder: String,
    pub name: String,
    pub last_played: i64,
    pub game_type: i32,
    pub hardcore: bool,
    pub cheats: bool,
    pub seed: i64,
    pub generator: String,
    pub structures: bool,
}

/// `AnvilSaveConverter.getSaveList`, sorted (`compareTo`).
pub fn save_list(saves: &Path) -> Vec<SaveInfo> {
    let mut v = Vec::new();
    let Ok(rd) = std::fs::read_dir(saves) else {
        return v;
    };
    for e in rd.flatten() {
        if !e.path().is_dir() {
            continue;
        }
        let folder = e.file_name().to_string_lossy().to_string();
        let Some(root) = crate::world::anvil::read_level_dat(&e.path()) else {
            continue;
        };
        let Some(d) = root.get("Data") else { continue };
        let ver = d.int("version").unwrap_or(0);
        if ver != 19132 && ver != 19133 {
            continue;
        }
        let mut name = d.string("LevelName").unwrap_or("").to_string();
        if name.is_empty() {
            name = folder.clone();
        }
        v.push(SaveInfo {
            folder,
            name,
            last_played: d.int("LastPlayed").unwrap_or(0),
            game_type: d.int("GameType").unwrap_or(0) as i32,
            hardcore: d.int("hardcore").unwrap_or(0) != 0,
            cheats: d.int("allowCommands").unwrap_or(0) != 0,
            seed: d.int("RandomSeed").unwrap_or(0),
            generator: d.string("generatorName").unwrap_or("default").to_string(),
            structures: d.int("MapFeatures").unwrap_or(1) != 0,
        });
    }
    v.sort_by(|a, b| {
        b.last_played
            .cmp(&a.last_played)
            .then_with(|| a.folder.cmp(&b.folder))
    });
    v
}

/// `SimpleDateFormat()` in the en_US locale: `M/d/yy h:mm a`.
fn short_date(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86400);
    let tod = secs.rem_euclid(86400);
    // civil from days (Howard Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let (hh, mm) = (tod / 3600, tod % 3600 / 60);
    let (h12, ap) = match hh {
        0 => (12, "AM"),
        1..=11 => (hh, "AM"),
        12 => (12, "PM"),
        _ => (hh - 12, "PM"),
    };
    format!("{m}/{d}/{:02} {h12}:{mm:02} {ap}", y.rem_euclid(100))
}

pub enum Screen {
    Main(MainMenu),
    SelectWorld(SelectWorld),
    CreateWorld(Box<CreateWorld>),
    Rename(Rename),
    YesNo(YesNo),
    Multiplayer(Basic),
    Options(Basic),
    Video(OptionList),
    Sounds(Basic),
    Controls(Controls),
    Language(LanguageScreen),
    Ingame(Basic),
    /// A screen not ported yet with its title (Achievements, Statistics,
    /// Resource Packs, Snooper, Chat Settings, Twitch).
    Todo(Basic, String),
    /// `ScreenChatOptions` and the y of its "Multiplayer Settings" title.
    ChatOptions(Basic, f32),
}

/// A screen of plain and option buttons.
pub struct Basic {
    pub buttons: Vec<Button>,
}

/// The stack of screens (`currentScreen` and the `parentScreen`s).
pub struct Menu {
    pub stack: Vec<Screen>,
    pub w: f32,
    pub h: f32,
    pub mouse: (f32, f32),
    pub down: bool,
    /// `GuiScreen.selectedButton`
    selected: Option<usize>,
    pending: Option<Action>,
}

impl Menu {
    pub fn title() -> Menu {
        Menu {
            stack: vec![Screen::Main(MainMenu::new(None))],
            w: 0.0,
            h: 0.0,
            mouse: (0.0, 0.0),
            down: false,
            selected: None,
            pending: None,
        }
    }

    pub fn ingame() -> Menu {
        Menu {
            stack: vec![Screen::Ingame(Basic {
                buttons: Vec::new(),
            })],
            w: 0.0,
            h: 0.0,
            mouse: (0.0, 0.0),
            down: false,
            selected: None,
            pending: None,
        }
    }

    pub fn with_splash(splashes: &[String], r: &mut crate::java::JavaRandom) -> Menu {
        let mut m = Menu::title();
        m.stack[0] = Screen::Main(MainMenu::new(Some((splashes, r))));
        m
    }

    /// Opens a screen by name over the title screen (`--menu`, for captures).
    pub fn open_named(&mut self, name: &str, lang: &Lang) {
        let b = || Basic {
            buttons: Vec::new(),
        };
        let s = match name {
            "select" => Screen::SelectWorld(SelectWorld::new()),
            "create" => Screen::CreateWorld(Box::new(CreateWorld::new(lang))),
            "multiplayer" => Screen::Multiplayer(b()),
            "options" => Screen::Options(b()),
            "video" => Screen::Video(OptionList::new()),
            "sounds" => Screen::Sounds(b()),
            "controls" => Screen::Controls(Controls::new()),
            "language" => Screen::Language(LanguageScreen::new()),
            "chat" => Screen::ChatOptions(b(), 0.0),
            _ => return,
        };
        self.stack.push(s);
    }

    /// `setWorldAndResolution`: the size changed or the screen is new.
    pub fn resize(&mut self, ui: &Ui, ctx: &mut Ctx, w: f32, h: f32) {
        self.w = w;
        self.h = h;
        if let Some(s) = self.stack.last_mut() {
            init(s, ui, ctx, w, h);
        }
    }

    fn open(&mut self, s: Screen, ui: &Ui, ctx: &mut Ctx) {
        self.stack.push(s);
        let (w, h) = (self.w, self.h);
        init(self.stack.last_mut().unwrap(), ui, ctx, w, h);
        self.selected = None;
    }

    /// `displayGuiScreen(parentScreen)`
    fn back(&mut self, ui: &Ui, ctx: &mut Ctx) -> Action {
        self.stack.pop();
        self.selected = None;
        if self.stack.is_empty() {
            return Action::Resume;
        }
        let (w, h) = (self.w, self.h);
        init(self.stack.last_mut().unwrap(), ui, ctx, w, h);
        Action::None
    }

    /// `displayGuiScreen(null)`: in a world back to the game, otherwise the
    /// title screen.
    fn close(&mut self, ui: &Ui, ctx: &mut Ctx) -> Action {
        self.selected = None;
        if ctx.in_world {
            self.stack.clear();
            Action::Resume
        } else {
            self.stack.truncate(1);
            let (w, h) = (self.w, self.h);
            init(self.stack.last_mut().unwrap(), ui, ctx, w, h);
            Action::None
        }
    }

    pub fn tick(&mut self) {
        match self.stack.last_mut() {
            Some(Screen::Main(m)) => m.panorama_timer += 1,
            Some(Screen::CreateWorld(c)) => {
                c.name.cursor_counter += 1;
                c.seed.cursor_counter += 1;
            }
            Some(Screen::Rename(r)) => r.field.cursor_counter += 1,
            _ => {}
        }
    }

    fn buttons(&mut self) -> Option<&mut Vec<Button>> {
        Some(match self.stack.last_mut()? {
            Screen::Main(s) => &mut s.buttons,
            Screen::SelectWorld(s) => &mut s.buttons,
            Screen::CreateWorld(s) => &mut s.buttons,
            Screen::Rename(s) => &mut s.buttons,
            Screen::YesNo(s) => &mut s.buttons,
            Screen::Multiplayer(s) => &mut s.buttons,
            Screen::Options(s) | Screen::Sounds(s) | Screen::Ingame(s) => &mut s.buttons,
            Screen::Todo(s, _) => &mut s.buttons,
            Screen::ChatOptions(s, _) => &mut s.buttons,
            Screen::Video(s) => &mut s.buttons,
            Screen::Controls(s) => &mut s.buttons,
            Screen::Language(s) => &mut s.buttons,
        })
    }

    /// `drawScreen`
    pub fn draw(&mut self, ui: &mut Ui, ctx: &mut Ctx, partial: f32) {
        let m = self.mouse;
        let down = self.down;
        let (w, h) = (self.w, self.h);
        let mut events = Vec::new();
        let Some(top) = self.stack.last_mut() else {
            return;
        };
        match top {
            Screen::Main(s) => s.draw(ui, ctx, w, h, partial),
            Screen::SelectWorld(s) => {
                let saves = &s.saves;
                let gm = [
                    ctx.lang.get("gameMode.survival").to_string(),
                    ctx.lang.get("gameMode.creative").to_string(),
                    ctx.lang.get("gameMode.adventure").to_string(),
                ];
                let lang = ctx.lang;
                let in_world = ctx.in_world;
                default_background(ui, in_world);
                s.list.draw(
                    ui,
                    m,
                    down,
                    saves.len(),
                    Some(saves.len() as f32 * 36.0),
                    &mut events,
                    &mut |ui, i, x, y, _h| {
                        let e = &saves[i];
                        let mut name = e.name.clone();
                        if name.is_empty() {
                            name = format!("{} {}", lang.get("selectWorld.world"), i + 1);
                        }
                        let line2 = format!("{} ({})", e.folder, short_date(e.last_played));
                        let mut line3 = gm.get(e.game_type as usize).cloned().unwrap_or_default();
                        if e.hardcore {
                            line3 = format!("\u{a7}4{}\u{a7}r", lang.get("gameMode.hardcore"));
                        }
                        if e.cheats {
                            line3 = format!("{line3}, {}", lang.get("selectWorld.cheats"));
                        }
                        ui.text(&name, x + 2.0, y + 1.0, 0xFFFFFF, true);
                        ui.text(&line2, x + 2.0, y + 12.0, 0x808080, true);
                        ui.text(&line3, x + 2.0, y + 22.0, 0x808080, true);
                    },
                );
                ui.centered(ctx.lang.get("selectWorld.title"), w / 2.0, 20.0, 0xFFFFFF);
            }
            Screen::CreateWorld(s) => s.draw(ui, ctx, w),
            Screen::Rename(s) => {
                default_background(ui, ctx.in_world);
                ui.centered(
                    ctx.lang.get("selectWorld.renameTitle"),
                    w / 2.0,
                    20.0,
                    0xFFFFFF,
                );
                ui.text(
                    ctx.lang.get("selectWorld.enterName"),
                    w / 2.0 - 100.0,
                    47.0,
                    0xA0A0A0,
                    true,
                );
                s.field.draw(ui);
            }
            Screen::YesNo(s) => {
                default_background(ui, ctx.in_world);
                ui.centered(&s.line1, w / 2.0, 70.0, 0xFFFFFF);
                ui.centered(&s.line2, w / 2.0, 90.0, 0xFFFFFF);
            }
            Screen::Multiplayer(_) => {
                default_background(ui, ctx.in_world);
                // GuiMultiplayer: the (empty) server list and LAN scan line
                ui.centered(ctx.lang.get("multiplayer.title"), w / 2.0, 20.0, 0xFFFFFF);
                ui.centered(
                    ctx.lang.get("lanServer.scanning"),
                    w / 2.0,
                    32.0 + 10.0,
                    0x808080,
                );
            }
            Screen::Options(_) => {
                default_background(ui, ctx.in_world);
                ui.centered(ctx.lang.get("options.title"), w / 2.0, 15.0, 0xFFFFFF);
            }
            Screen::Sounds(_) => {
                default_background(ui, ctx.in_world);
                ui.centered(
                    ctx.lang.get("options.sounds.title"),
                    w / 2.0,
                    15.0,
                    0xFFFFFF,
                );
            }
            Screen::Ingame(_) => {
                default_background(ui, true);
                ui.centered(ctx.lang.get("menu.game"), w / 2.0, 40.0, 0xFFFFFF);
            }
            Screen::Todo(_, title) => {
                default_background(ui, ctx.in_world);
                ui.centered(title, w / 2.0, 15.0, 0xFFFFFF);
            }
            Screen::ChatOptions(_, sep) => {
                default_background(ui, ctx.in_world);
                ui.centered(ctx.lang.get("options.chat.title"), w / 2.0, 20.0, 0xFFFFFF);
                ui.centered(
                    ctx.lang.get("options.multiplayer.title"),
                    w / 2.0,
                    *sep + 7.0,
                    0xFFFFFF,
                );
            }
            Screen::Video(s) => {
                default_background(ui, ctx.in_world);
                s.draw(ui, ctx, m, down);
                ui.centered(ctx.lang.get("options.videoTitle"), w / 2.0, 5.0, 0xFFFFFF);
            }
            Screen::Controls(s) => {
                default_background(ui, ctx.in_world);
                s.draw(ui, ctx, m, down);
                ui.centered(ctx.lang.get("controls.title"), w / 2.0, 8.0, 0xFFFFFF);
                let all_default = ctx.settings.keys.iter().all(|k| k.code == k.default);
                if let Some(b) = s.buttons.iter_mut().find(|b| b.id == 201) {
                    b.enabled = !all_default;
                }
            }
            Screen::Language(s) => {
                default_background(ui, ctx.in_world);
                let langs = ctx.languages;
                let cur = ctx.settings.language.clone();
                s.list.selected = langs
                    .iter()
                    .position(|l| l.code == cur)
                    .map_or(-1, |i| i as i32);
                s.list.draw(
                    ui,
                    m,
                    down,
                    langs.len(),
                    None,
                    &mut events,
                    &mut |ui, i, _x, y, _h| {
                        ui.centered(&langs[i].label(), w / 2.0, y + 1.0, 0xFFFFFF);
                    },
                );
                ui.centered(ctx.lang.get("options.language"), w / 2.0, 16.0, 0xFFFFFF);
                let warn = format!("({})", ctx.lang.get("options.languageWarning"));
                ui.centered(&warn, w / 2.0, h - 56.0, 0x808080);
            }
        }
        // super.drawScreen: the buttons
        let lang = ctx.lang;
        if let Some(bs) = self.buttons() {
            for b in bs.iter_mut() {
                b.draw(ui, m, ctx.settings, lang);
            }
        }
        for e in events {
            self.slot_clicked(e, ui, ctx);
        }
    }

    /// The list click events found while drawing.
    fn slot_clicked(&mut self, e: SlotEvent, _ui: &Ui, ctx: &mut Ctx) {
        let SlotEvent::Clicked(i, dbl) = e;
        match self.stack.last_mut() {
            Some(Screen::SelectWorld(s)) => {
                s.selected = Some(i);
                let ok = i < s.saves.len();
                for b in s.buttons.iter_mut() {
                    if matches!(b.id, 1 | 2 | 6 | 7) {
                        b.enabled = ok;
                    }
                }
                if dbl && ok {
                    self.pending = Some(Action::Play {
                        folder: ctx.saves.join(&s.saves[i].folder),
                        new: None,
                    });
                }
            }
            Some(Screen::Language(_)) => {
                if let Some(l) = ctx.languages.get(i) {
                    if ctx.settings.language != l.code {
                        ctx.settings.language = l.code.clone();
                        ctx.settings.save();
                        self.pending = Some(Action::Language);
                    }
                }
                if let Some(Screen::Language(s)) = self.stack.last_mut() {
                    for b in s.buttons.iter_mut() {
                        if b.id == 6 {
                            b.label = ctx.lang.get("gui.done").to_string();
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// `handleMouseInput`: a press.
    pub fn mouse_down(&mut self, ui: &Ui, ctx: &mut Ctx, button: u8) -> Action {
        self.down = button == 0 || self.down;
        let m = self.mouse;
        // GuiControls waits for a mouse button
        if let Some(Screen::Controls(c)) = self.stack.last_mut() {
            if let Some(k) = c.waiting.take() {
                ctx.settings.keys[k].code = -100 + button as i32;
                ctx.settings.save();
                return Action::None;
            }
            if button == 0 {
                if let Some(a) = c.list_press(m, ctx) {
                    return a;
                }
            }
        }
        if let Some(Screen::Video(v)) = self.stack.last_mut() {
            if button == 0 {
                let before = ctx.settings.gui_scale;
                let hit = v.list_press(m, ctx);
                if hit {
                    ctx.sounds.push("gui.button.press");
                    if ctx.settings.gui_scale != before {
                        return Action::Settings;
                    }
                    return Action::Settings;
                }
            }
        }
        if button == 0 {
            let lang = ctx.lang;
            let mut hit = None;
            if let Some(bs) = self.buttons() {
                for (i, b) in bs.iter_mut().enumerate() {
                    if b.pressed(m) {
                        match b.kind {
                            Kind::Slider { .. } | Kind::Sound { .. } => {
                                b.press_slider(m, ctx.settings, lang)
                            }
                            _ => {}
                        }
                        hit = Some(i);
                        break;
                    }
                }
            }
            if let Some(i) = hit {
                self.selected = Some(i);
                let b = self.buttons().unwrap()[i].clone();
                if !matches!(b.kind, Kind::Sound { .. }) {
                    ctx.sounds.push("gui.button.press");
                }
                return self.action(b, ui, ctx);
            }
        }
        // text fields
        match self.stack.last_mut() {
            Some(Screen::CreateWorld(c)) => {
                if c.more {
                    c.seed.click(ui, m, button);
                } else {
                    c.name.click(ui, m, button);
                }
            }
            Some(Screen::Rename(r)) => r.field.click(ui, m, button),
            _ => {}
        }
        Action::None
    }

    /// `mouseMovedOrUp` with a button.
    pub fn mouse_up(&mut self, ctx: &mut Ctx, button: u8) {
        if button == 0 {
            self.down = false;
            if let Some(i) = self.selected.take() {
                let mut click = false;
                if let Some(bs) = self.buttons() {
                    if let Some(b) = bs.get_mut(i) {
                        click = b.release();
                    }
                }
                if click {
                    ctx.sounds.push("gui.button.press");
                }
            }
            if let Some(Screen::Video(v)) = self.stack.last_mut() {
                v.release();
            }
            if matches!(
                self.stack.last(),
                Some(Screen::Options(_)) | Some(Screen::Video(_)) | Some(Screen::Controls(_))
            ) {
                ctx.settings.save();
            }
        }
    }

    pub fn wheel(&mut self, up: bool) {
        let m = self.mouse;
        match self.stack.last_mut() {
            Some(Screen::SelectWorld(s)) => s.list.wheel(m, up),
            Some(Screen::Language(s)) => s.list.wheel(m, up),
            Some(Screen::Video(s)) => s.list.wheel(m, up),
            Some(Screen::Controls(s)) => s.list.wheel(m, up),
            _ => {}
        }
    }

    /// `keyTyped(char, LWJGL code)`
    pub fn key(&mut self, ui: &Ui, ctx: &mut Ctx, c: char, code: i32) -> Action {
        match self.stack.last_mut() {
            Some(Screen::Main(_)) => return Action::None,
            Some(Screen::Controls(s)) => {
                if let Some(k) = s.waiting.take() {
                    ctx.settings.keys[k].code = if code == 1 { 0 } else { code };
                    ctx.settings.save();
                    return Action::None;
                }
            }
            Some(Screen::CreateWorld(s)) => {
                let (ctrl, shift) = (ctx.ctrl, ctx.shift);
                if s.name.focused && !s.more {
                    s.name.key(ui, c, code, ctrl, shift, ctx.clipboard);
                } else if s.seed.focused && s.more {
                    s.seed.key(ui, c, code, ctrl, shift, ctx.clipboard);
                }
                if code == 28 || code == 156 {
                    let b = s.buttons[0].clone();
                    return self.action(b, ui, ctx);
                }
                if let Some(Screen::CreateWorld(s)) = self.stack.last_mut() {
                    s.buttons[0].enabled = !s.name.text.is_empty();
                    s.calc_save_dir(ctx.saves);
                }
                return Action::None;
            }
            Some(Screen::Rename(s)) => {
                let (ctrl, shift) = (ctx.ctrl, ctx.shift);
                s.field.key(ui, c, code, ctrl, shift, ctx.clipboard);
                s.buttons[0].enabled = !s.field.text().trim().is_empty();
                if code == 28 || code == 156 {
                    let b = s.buttons[0].clone();
                    return self.action(b, ui, ctx);
                }
                return Action::None;
            }
            Some(Screen::Ingame(_)) => {
                if code == 1 {
                    self.stack.clear();
                    return Action::Resume;
                }
                return Action::None;
            }
            _ => {}
        }
        // GuiScreen.keyTyped: Escape
        if code == 1 {
            return self.close(ui, ctx);
        }
        Action::None
    }

    /// `actionPerformed`
    fn action(&mut self, b: Button, ui: &Ui, ctx: &mut Ctx) -> Action {
        let lang = ctx.lang;
        let Some(top) = self.stack.last_mut() else {
            return Action::None;
        };
        match top {
            Screen::Main(_) => match b.id {
                0 => self.open(
                    Screen::Options(Basic {
                        buttons: Vec::new(),
                    }),
                    ui,
                    ctx,
                ),
                5 => self.open(Screen::Language(LanguageScreen::new()), ui, ctx),
                1 => self.open(Screen::SelectWorld(SelectWorld::new()), ui, ctx),
                2 => self.open(
                    Screen::Multiplayer(Basic {
                        buttons: Vec::new(),
                    }),
                    ui,
                    ctx,
                ),
                4 => return Action::Quit,
                _ => {}
            },
            Screen::SelectWorld(s) => match b.id {
                2 => {
                    if let Some(i) = s.selected {
                        let name = s.saves[i].name.clone();
                        let folder = s.saves[i].folder.clone();
                        let y = YesNo {
                            buttons: Vec::new(),
                            line1: lang.get("selectWorld.deleteQuestion").to_string(),
                            line2: format!("'{name}' {}", lang.get("selectWorld.deleteWarning")),
                            yes: lang.get("selectWorld.deleteButton").to_string(),
                            no: lang.get("gui.cancel").to_string(),
                            folder,
                        };
                        self.open(Screen::YesNo(y), ui, ctx);
                    }
                }
                1 => {
                    if let Some(i) = s.selected {
                        return Action::Play {
                            folder: ctx.saves.join(&s.saves[i].folder),
                            new: None,
                        };
                    }
                }
                3 => self.open(
                    Screen::CreateWorld(Box::new(CreateWorld::new(lang))),
                    ui,
                    ctx,
                ),
                6 => {
                    if let Some(i) = s.selected {
                        let r = Rename {
                            buttons: Vec::new(),
                            field: TextField::new(0.0, 0.0, 200.0, 20.0),
                            folder: s.saves[i].folder.clone(),
                            name: s.saves[i].name.clone(),
                        };
                        self.open(Screen::Rename(r), ui, ctx);
                    }
                }
                0 => return self.back(ui, ctx),
                7 => {
                    if let Some(i) = s.selected {
                        let e = s.saves[i].clone();
                        let mut c = CreateWorld::new(lang);
                        c.recreate_from(&e, lang);
                        self.open(Screen::CreateWorld(Box::new(c)), ui, ctx);
                    }
                }
                _ => {}
            },
            Screen::YesNo(y) => {
                if b.id == 0 {
                    let _ = std::fs::remove_dir_all(ctx.saves.join(&y.folder));
                }
                self.stack.pop();
                if let Some(Screen::SelectWorld(s)) = self.stack.last_mut() {
                    s.saves = save_list(ctx.saves);
                    s.selected = None;
                }
                let (w, h) = (self.w, self.h);
                if let Some(t) = self.stack.last_mut() {
                    init(t, ui, ctx, w, h);
                }
            }
            Screen::Rename(r) => {
                if b.id == 1 {
                    return self.back(ui, ctx);
                }
                if b.id == 0 {
                    // AnvilSaveConverter.renameWorld: LevelName in level.dat
                    let dir = ctx.saves.join(&r.folder);
                    if let Some(mut root) = crate::world::anvil::read_level_dat(&dir) {
                        if let Some(d) = root.get_mut("Data") {
                            d.set(
                                "LevelName",
                                crate::world::anvil::Nbt::String(r.field.text().trim().to_string()),
                            );
                        }
                        let _ = crate::world::anvil::write_level_dat(&dir, &root);
                    }
                    self.stack.pop();
                    if let Some(Screen::SelectWorld(s)) = self.stack.last_mut() {
                        s.saves = save_list(ctx.saves);
                        s.selected = None;
                    }
                    let (w, h) = (self.w, self.h);
                    if let Some(t) = self.stack.last_mut() {
                        init(t, ui, ctx, w, h);
                    }
                }
            }
            Screen::CreateWorld(c) => {
                return match c.action(b, ui, ctx) {
                    Some(a) => a,
                    None => self.back(ui, ctx),
                }
            }
            Screen::Multiplayer(_) => {
                if b.id == 0 {
                    return self.back(ui, ctx);
                }
            }
            Screen::Options(bs) => {
                if b.id < 100 {
                    if let Kind::Option(o) = b.kind {
                        ctx.settings.set_option(o, 1);
                        let label = ctx.settings.label(o, lang);
                        if let Some(x) = bs.buttons.iter_mut().find(|x| x.id == b.id) {
                            x.label = label;
                        }
                        return Action::Settings;
                    }
                }
                ctx.settings.save();
                match b.id {
                    101 => self.open(Screen::Video(OptionList::new()), ui, ctx),
                    100 => self.open(Screen::Controls(Controls::new()), ui, ctx),
                    102 => self.open(Screen::Language(LanguageScreen::new()), ui, ctx),
                    106 => self.open(
                        Screen::Sounds(Basic {
                            buttons: Vec::new(),
                        }),
                        ui,
                        ctx,
                    ),
                    103 => self.open(
                        Screen::ChatOptions(
                            Basic {
                                buttons: Vec::new(),
                            },
                            0.0,
                        ),
                        ui,
                        ctx,
                    ),
                    104 => self.open(
                        Screen::Todo(
                            Basic {
                                buttons: Vec::new(),
                            },
                            lang.get("options.snooper.title").to_string(),
                        ),
                        ui,
                        ctx,
                    ),
                    105 => self.open(
                        Screen::Todo(
                            Basic {
                                buttons: Vec::new(),
                            },
                            lang.get("resourcePack.title").to_string(),
                        ),
                        ui,
                        ctx,
                    ),
                    107 => self.open(
                        Screen::Todo(
                            Basic {
                                buttons: Vec::new(),
                            },
                            lang.get("stream.unavailable.title").to_string(),
                        ),
                        ui,
                        ctx,
                    ),
                    200 => return self.back(ui, ctx),
                    _ => {}
                }
            }
            Screen::ChatOptions(c, _) => {
                if b.id < 100 {
                    if let Kind::Option(o) = b.kind {
                        ctx.settings.set_option(o, 1);
                        let label = ctx.settings.label(o, lang);
                        if let Some(x) = c.buttons.iter_mut().find(|x| x.id == b.id) {
                            x.label = label;
                        }
                        return Action::Settings;
                    }
                }
                if b.id == 200 {
                    ctx.settings.save();
                    return self.back(ui, ctx);
                }
            }
            Screen::Sounds(_) | Screen::Todo(..) => {
                if b.id == 200 {
                    ctx.settings.save();
                    return self.back(ui, ctx);
                }
            }
            Screen::Video(_) => {
                if b.id == 200 {
                    ctx.settings.save();
                    return self.back(ui, ctx);
                }
            }
            Screen::Controls(c) => match b.id {
                200 => return self.back(ui, ctx),
                201 => {
                    for k in ctx.settings.keys.iter_mut() {
                        k.code = k.default;
                    }
                    ctx.settings.save();
                }
                id if id < 100 => {
                    if let Kind::Option(o) = b.kind {
                        ctx.settings.set_option(o, 1);
                        let label = ctx.settings.label(o, lang);
                        if let Some(x) = c.buttons.iter_mut().find(|x| x.id == id) {
                            x.label = label;
                        }
                    }
                }
                _ => {}
            },
            Screen::Language(l) => match b.id {
                6 => return self.back(ui, ctx),
                100 => {
                    ctx.settings.set_option(Opt::ForceUnicodeFont, 1);
                    let label = ctx.settings.label(Opt::ForceUnicodeFont, lang);
                    if let Some(x) = l.buttons.iter_mut().find(|x| x.id == 100) {
                        x.label = label;
                    }
                    return Action::Language;
                }
                _ => {}
            },
            Screen::Ingame(_) => match b.id {
                0 => self.open(
                    Screen::Options(Basic {
                        buttons: Vec::new(),
                    }),
                    ui,
                    ctx,
                ),
                1 => return Action::QuitToTitle,
                4 => {
                    self.stack.clear();
                    return Action::Resume;
                }
                5 => self.open(
                    Screen::Todo(
                        Basic {
                            buttons: Vec::new(),
                        },
                        lang.get("gui.achievements").to_string(),
                    ),
                    ui,
                    ctx,
                ),
                6 => self.open(
                    Screen::Todo(
                        Basic {
                            buttons: Vec::new(),
                        },
                        lang.get("gui.stats").to_string(),
                    ),
                    ui,
                    ctx,
                ),
                _ => {}
            },
        }
        Action::None
    }

    /// The action found while drawing (a double click).
    pub fn take_pending(&mut self) -> Action {
        self.pending.take().unwrap_or(Action::None)
    }
}

/// `initGui` of each screen.
fn init(s: &mut Screen, ui: &Ui, ctx: &mut Ctx, w: f32, h: f32) {
    let lang = ctx.lang;
    let t = |k: &str| lang.get(k).to_string();
    let cx = (w as i32 / 2) as f32;
    match s {
        Screen::Main(m) => {
            let y = (h as i32 / 4 + 48) as f32;
            m.buttons = vec![
                Button::wide(1, cx - 100.0, y, t("menu.singleplayer")),
                Button::wide(2, cx - 100.0, y + 24.0, t("menu.multiplayer")),
                Button::wide(14, cx - 100.0, y + 48.0, t("menu.online")),
                Button::new(
                    0,
                    cx - 100.0,
                    y + 72.0 + 12.0,
                    98.0,
                    20.0,
                    t("menu.options"),
                ),
                Button::new(4, cx + 2.0, y + 72.0 + 12.0, 98.0, 20.0, t("menu.quit")),
                Button::language(5, cx - 124.0, y + 72.0 + 12.0),
            ];
            // Minecraft Realms needs an account and a network
            m.buttons[2].enabled = false;
        }
        Screen::SelectWorld(s) => {
            s.saves = save_list(ctx.saves);
            s.list = SlotList::new(w, h, 32.0, h - 64.0, 36.0);
            if let Some(i) = s.selected {
                s.list.selected = i as i32;
            }
            let ok = s.selected.is_some_and(|i| i < s.saves.len());
            s.buttons = vec![
                Button::new(
                    1,
                    cx - 154.0,
                    h - 52.0,
                    150.0,
                    20.0,
                    t("selectWorld.select"),
                ),
                Button::new(3, cx + 4.0, h - 52.0, 150.0, 20.0, t("selectWorld.create")),
                Button::new(6, cx - 154.0, h - 28.0, 72.0, 20.0, t("selectWorld.rename")),
                Button::new(2, cx - 76.0, h - 28.0, 72.0, 20.0, t("selectWorld.delete")),
                Button::new(7, cx + 4.0, h - 28.0, 72.0, 20.0, t("selectWorld.recreate")),
                Button::new(0, cx + 82.0, h - 28.0, 72.0, 20.0, t("gui.cancel")),
            ];
            for b in s.buttons.iter_mut() {
                if matches!(b.id, 1 | 2 | 6 | 7) {
                    b.enabled = ok;
                }
            }
        }
        Screen::CreateWorld(c) => c.init(ui, ctx, w, h),
        Screen::Rename(r) => {
            r.buttons = vec![
                Button::wide(
                    0,
                    cx - 100.0,
                    (h as i32 / 4 + 96 + 12) as f32,
                    t("selectWorld.renameButton"),
                ),
                Button::wide(
                    1,
                    cx - 100.0,
                    (h as i32 / 4 + 120 + 12) as f32,
                    t("gui.cancel"),
                ),
            ];
            let text = r.field.text();
            r.field = TextField::new(cx - 100.0, 60.0, 200.0, 20.0);
            let name = if text.is_empty() {
                r.name.clone()
            } else {
                text
            };
            r.field.set_text(ui, &name);
            r.field.set_focused(true);
        }
        Screen::YesNo(y) => {
            y.buttons = vec![
                Button::new(
                    0,
                    cx - 155.0,
                    (h as i32 / 6 + 96) as f32,
                    150.0,
                    20.0,
                    y.yes.clone(),
                ),
                Button::new(
                    1,
                    cx - 155.0 + 160.0,
                    (h as i32 / 6 + 96) as f32,
                    150.0,
                    20.0,
                    y.no.clone(),
                ),
            ];
        }
        Screen::Multiplayer(m) => {
            m.buttons = vec![
                Button::new(7, cx - 154.0, h - 28.0, 70.0, 20.0, t("selectServer.edit")),
                Button::new(2, cx - 74.0, h - 28.0, 70.0, 20.0, t("selectServer.delete")),
                Button::new(
                    1,
                    cx - 154.0,
                    h - 52.0,
                    100.0,
                    20.0,
                    t("selectServer.select"),
                ),
                Button::new(
                    4,
                    cx - 50.0,
                    h - 52.0,
                    100.0,
                    20.0,
                    t("selectServer.direct"),
                ),
                Button::new(
                    3,
                    cx + 4.0 + 50.0,
                    h - 52.0,
                    100.0,
                    20.0,
                    t("selectServer.add"),
                ),
                Button::new(8, cx + 4.0, h - 28.0, 70.0, 20.0, t("selectServer.refresh")),
                Button::new(0, cx + 4.0 + 76.0, h - 28.0, 75.0, 20.0, t("gui.cancel")),
            ];
            // no network protocol yet
            for b in m.buttons.iter_mut() {
                if b.id != 0 && b.id != 8 {
                    b.enabled = false;
                }
            }
            m.buttons[0].enabled = false;
            m.buttons[1].enabled = false;
            m.buttons[2].enabled = false;
        }
        Screen::Options(o) => {
            let h6 = (h as i32 / 6) as f32;
            let mut v = Vec::new();
            for (i, opt) in [Opt::Fov, Opt::Difficulty].iter().enumerate() {
                let x = cx - 155.0 + (i % 2) as f32 * 160.0;
                let y = h6 - 12.0 + 24.0 * (i >> 1) as f32;
                let mut b = Button::option(x, y, *opt, ctx.settings, lang);
                if *opt == Opt::Difficulty && ctx.in_world && ctx.hardcore {
                    b.enabled = false;
                    b.label = format!(
                        "{}: {}",
                        t("options.difficulty"),
                        t("options.difficulty.hardcore")
                    );
                }
                v.push(b);
            }
            v.push(Button::new(
                8675309,
                cx + 5.0,
                h6 + 48.0 - 6.0,
                150.0,
                20.0,
                "Super Secret Settings...",
            ));
            v.push(Button::new(
                106,
                cx - 155.0,
                h6 + 72.0 - 6.0,
                150.0,
                20.0,
                t("options.sounds"),
            ));
            v.push(Button::new(
                107,
                cx + 5.0,
                h6 + 72.0 - 6.0,
                150.0,
                20.0,
                t("options.stream"),
            ));
            v.push(Button::new(
                101,
                cx - 155.0,
                h6 + 96.0 - 6.0,
                150.0,
                20.0,
                t("options.video"),
            ));
            v.push(Button::new(
                100,
                cx + 5.0,
                h6 + 96.0 - 6.0,
                150.0,
                20.0,
                t("options.controls"),
            ));
            v.push(Button::new(
                102,
                cx - 155.0,
                h6 + 120.0 - 6.0,
                150.0,
                20.0,
                t("options.language"),
            ));
            v.push(Button::new(
                103,
                cx + 5.0,
                h6 + 120.0 - 6.0,
                150.0,
                20.0,
                t("options.multiplayer.title"),
            ));
            v.push(Button::new(
                105,
                cx - 155.0,
                h6 + 144.0 - 6.0,
                150.0,
                20.0,
                t("options.resourcepack"),
            ));
            v.push(Button::new(
                104,
                cx + 5.0,
                h6 + 144.0 - 6.0,
                150.0,
                20.0,
                t("options.snooper.view"),
            ));
            v.push(Button::wide(200, cx - 100.0, h6 + 168.0, t("gui.done")));
            // the shader cycling of entityRenderer.activateNextShader needs
            // the post-processing shaders
            v[2].enabled = false;
            o.buttons = v;
        }
        Screen::Sounds(o) => {
            let h6 = (h as i32 / 6) as f32;
            let mut v = Vec::new();
            let mk = |cat: usize, i: usize, master: bool, s: &GameSettings| {
                let x = cx - 155.0 + (i % 2) as f32 * 160.0;
                let y = h6 - 12.0 + 24.0 * (i >> 1) as f32;
                let mut b = Button::new(
                    cat as i32,
                    x,
                    y,
                    if master { 310.0 } else { 150.0 },
                    20.0,
                    widgets::sound_label(cat, s, lang),
                );
                b.kind = Kind::Sound {
                    cat,
                    value: s.sound_levels[cat],
                    pressed: false,
                };
                b
            };
            v.push(mk(0, 0, true, ctx.settings));
            let mut i = 2;
            for cat in 1..9 {
                v.push(mk(cat, i, false, ctx.settings));
                i += 1;
            }
            v.push(Button::wide(200, cx - 100.0, h6 + 168.0, t("gui.done")));
            o.buttons = v;
        }
        Screen::Video(v) => v.init(ctx, w, h),
        Screen::Controls(c) => c.init(ctx, w, h),
        Screen::Language(l) => {
            l.buttons = vec![
                {
                    let mut b = Button::option(
                        cx - 155.0,
                        h - 38.0,
                        Opt::ForceUnicodeFont,
                        ctx.settings,
                        lang,
                    );
                    b.id = 100;
                    b
                },
                Button::new(6, cx - 155.0 + 160.0, h - 38.0, 150.0, 20.0, t("gui.done")),
            ];
            let amount = l.list.amount;
            l.list = SlotList::new(w, h, 32.0, h - 65.0 + 4.0, 18.0);
            l.list.amount = amount;
        }
        Screen::Ingame(g) => {
            let b = -16.0;
            let h4 = (h as i32 / 4) as f32;
            g.buttons = vec![
                Button::wide(1, cx - 100.0, h4 + 120.0 + b, t("menu.returnToMenu")),
                Button::wide(4, cx - 100.0, h4 + 24.0 + b, t("menu.returnToGame")),
                Button::new(0, cx - 100.0, h4 + 96.0 + b, 98.0, 20.0, t("menu.options")),
                Button::new(7, cx + 2.0, h4 + 96.0 + b, 98.0, 20.0, t("menu.shareToLan")),
                Button::new(
                    5,
                    cx - 100.0,
                    h4 + 48.0 + b,
                    98.0,
                    20.0,
                    t("gui.achievements"),
                ),
                Button::new(6, cx + 2.0, h4 + 48.0 + b, 98.0, 20.0, t("gui.stats")),
            ];
            // Open to LAN needs the network server
            g.buttons[3].enabled = false;
        }
        Screen::Todo(b, _) => {
            b.buttons = vec![Button::wide(200, cx - 100.0, h - 27.0, t("gui.done"))];
        }
        Screen::ChatOptions(b, sep) => {
            // ScreenChatOptions.initGui
            let h6 = (h as i32 / 6) as f32;
            let mut v = Vec::new();
            let mut i = 0usize;
            let place = |i: usize| {
                (
                    cx - 155.0 + (i % 2) as f32 * 160.0,
                    h6 + 24.0 * (i >> 1) as f32,
                )
            };
            for o in [
                Opt::ChatVisibility,
                Opt::ChatColor,
                Opt::ChatLinks,
                Opt::ChatOpacity,
                Opt::ChatLinksPrompt,
                Opt::ChatScale,
                Opt::ChatHeightFocused,
                Opt::ChatHeightUnfocused,
                Opt::ChatWidth,
            ] {
                let (x, y) = place(i);
                v.push(Button::option(x, y, o, ctx.settings, lang));
                i += 1;
            }
            if i % 2 == 1 {
                i += 1;
            }
            *sep = h6 + 24.0 * (i >> 1) as f32;
            i += 2;
            let (x, y) = place(i);
            v.push(Button::option(x, y, Opt::ShowCape, ctx.settings, lang));
            v.push(Button::wide(200, cx - 100.0, h6 + 168.0, t("gui.done")));
            b.buttons = v;
        }
    }
}

/// `GuiMainMenu`
pub struct MainMenu {
    pub buttons: Vec<Button>,
    pub splash: String,
    /// `updateCounter`: below 1e-4 the logo reads "Minceraft".
    pub update_counter: f32,
    pub panorama_timer: i32,
    pub panorama_tex: Option<u32>,
}

impl MainMenu {
    fn new(src: Option<(&[String], &mut crate::java::JavaRandom)>) -> MainMenu {
        let mut splash = "missingno".to_string();
        let mut uc = 0.5;
        if let Some((list, r)) = src {
            if !list.is_empty() {
                loop {
                    splash = list[r.next_int_n(list.len() as i32) as usize].clone();
                    // "This message will never appear on the splash screen, isn't that weird?"
                    if java_hash(&splash) != 125780783 {
                        break;
                    }
                }
            }
            uc = r.next_float();
        }
        MainMenu {
            buttons: Vec::new(),
            splash,
            update_counter: uc,
            panorama_timer: 0,
            panorama_tex: None,
        }
    }

    /// `drawScreen` after the panorama, which the owner draws first.
    fn draw(&mut self, ui: &mut Ui, ctx: &Ctx, w: f32, h: f32, _partial: f32) {
        let _ = ctx;
        crate::screens::gradient(ui, 0.0, 0.0, w, h, 0x80FFFFFF, 0x00FFFFFF);
        crate::screens::gradient(ui, 0.0, 0.0, w, h, 0x00000000, 0x80000000);
        let lx = (w as i32 / 2 - 274 / 2) as f32;
        let ly = 30.0;
        let logo = "gui/title/minecraft.png";
        if self.update_counter < 1.0e-4 {
            widgets::tex_rect(ui, logo, lx, ly, 0.0, 0.0, 99.0, 44.0);
            widgets::tex_rect(ui, logo, lx + 99.0, ly, 129.0, 0.0, 27.0, 44.0);
            widgets::tex_rect(ui, logo, lx + 99.0 + 26.0, ly, 126.0, 0.0, 3.0, 44.0);
            widgets::tex_rect(ui, logo, lx + 99.0 + 26.0 + 3.0, ly, 99.0, 0.0, 26.0, 44.0);
            widgets::tex_rect(ui, logo, lx + 155.0, ly, 0.0, 45.0, 155.0, 44.0);
        } else {
            widgets::tex_rect(ui, logo, lx, ly, 0.0, 0.0, 155.0, 44.0);
            widgets::tex_rect(ui, logo, lx + 155.0, ly, 0.0, 45.0, 155.0, 44.0);
        }
        // the splash: rotated -20 degrees and pulsing
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        let k = (ms % 1000) as f32 / 1000.0 * std::f32::consts::PI * 2.0;
        let mut sc = 1.8 - (crate::util::math::sin(k) * 0.1).abs();
        sc = sc * 100.0 / (ui.text_width(&self.splash) + 32.0);
        let start = ui.v.len();
        let tw = ui.text_width(&self.splash);
        ui.text(&self.splash, -(tw as i32 / 2) as f32, -8.0, 0xFFFF00, true);
        let (s, c) = (-20f32).to_radians().sin_cos();
        let ox = (w as i32 / 2 + 90) as f32;
        for v in &mut ui.v[start..] {
            let (x, y) = (v.pos[0] * sc, v.pos[1] * sc);
            v.pos[0] = ox + x * c - y * s;
            v.pos[1] = 70.0 + x * s + y * c;
        }
        ui.text("Minecraft 1.7.10", 2.0, h - 10.0, 0xFFFFFF, true);
        let cr = "Copyright Mojang AB. Do not distribute!";
        let cw = ui.text_width(cr);
        ui.text(cr, w - cw - 2.0, h - 10.0, 0xFFFFFF, true);
    }
}

/// `String.hashCode`
fn java_hash(s: &str) -> i32 {
    let mut h: i32 = 0;
    for u in s.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(u as i32);
    }
    h
}

/// `GuiSelectWorld`
pub struct SelectWorld {
    pub buttons: Vec<Button>,
    pub saves: Vec<SaveInfo>,
    pub list: SlotList,
    pub selected: Option<usize>,
}

impl SelectWorld {
    fn new() -> SelectWorld {
        SelectWorld {
            buttons: Vec::new(),
            saves: Vec::new(),
            list: SlotList::new(0.0, 0.0, 0.0, 0.0, 36.0),
            selected: None,
        }
    }
}

/// `GuiRenameWorld`
pub struct Rename {
    pub buttons: Vec<Button>,
    pub field: TextField,
    pub folder: String,
    pub name: String,
}

/// `GuiYesNo` (the delete question)
pub struct YesNo {
    pub buttons: Vec<Button>,
    pub line1: String,
    pub line2: String,
    pub yes: String,
    pub no: String,
    pub folder: String,
}

/// `GuiCreateWorld`
pub struct CreateWorld {
    pub buttons: Vec<Button>,
    pub name: TextField,
    pub seed: TextField,
    world_name: String,
    world_seed: String,
    save_dir: String,
    mode: String,
    structures: bool,
    cheats: bool,
    cheats_by_user: bool,
    bonus: bool,
    hardcore: bool,
    generated: bool,
    pub more: bool,
    world_type: usize,
    desc1: String,
    desc2: String,
}

/// `GuiCreateWorld.disallowedFilenames`
const DISALLOWED: [&str; 24] = [
    "CON", "COM", "PRN", "AUX", "CLOCK$", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6",
    "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// `ChatAllowedCharacters.allowedCharacters`
const FILE_CHARS: [char; 15] = [
    '/', '\n', '\r', '\t', '\0', '\u{c}', '`', '?', '*', '\\', '<', '>', '|', '"', ':',
];

impl CreateWorld {
    fn new(lang: &Lang) -> CreateWorld {
        CreateWorld {
            buttons: Vec::new(),
            name: TextField::new(0.0, 0.0, 200.0, 20.0),
            seed: TextField::new(0.0, 0.0, 200.0, 20.0),
            world_name: lang.get("selectWorld.newWorld").to_string(),
            world_seed: String::new(),
            save_dir: String::new(),
            mode: "survival".into(),
            structures: true,
            cheats: false,
            cheats_by_user: false,
            bonus: false,
            hardcore: false,
            generated: false,
            more: false,
            world_type: 0,
            desc1: String::new(),
            desc2: String::new(),
        }
    }

    /// `recreateFromExistingWorld`
    fn recreate_from(&mut self, e: &SaveInfo, lang: &Lang) {
        self.world_name = lang.format("selectWorld.newWorld.copyOf", &[e.name.clone()]);
        self.world_seed = e.seed.to_string();
        self.world_type = WORLD_TYPES
            .iter()
            .position(|t| t.0.eq_ignore_ascii_case(&e.generator))
            .unwrap_or(0);
        self.structures = e.structures;
        self.cheats = e.cheats;
        self.mode = if e.hardcore {
            "hardcore".into()
        } else if e.game_type == 1 {
            "creative".into()
        } else {
            "survival".into()
        };
    }

    fn init(&mut self, ui: &Ui, ctx: &mut Ctx, w: f32, h: f32) {
        let lang = ctx.lang;
        let t = |k: &str| lang.get(k).to_string();
        let cx = (w as i32 / 2) as f32;
        let mut v = vec![
            Button::new(
                0,
                cx - 155.0,
                h - 28.0,
                150.0,
                20.0,
                t("selectWorld.create"),
            ),
            Button::new(1, cx + 5.0, h - 28.0, 150.0, 20.0, t("gui.cancel")),
            Button::new(2, cx - 75.0, 115.0, 150.0, 20.0, t("selectWorld.gameMode")),
            Button::new(
                3,
                cx - 75.0,
                187.0,
                150.0,
                20.0,
                t("selectWorld.moreWorldOptions"),
            ),
            Button::new(
                4,
                cx - 155.0,
                100.0,
                150.0,
                20.0,
                t("selectWorld.mapFeatures"),
            ),
            Button::new(7, cx + 5.0, 151.0, 150.0, 20.0, t("selectWorld.bonusItems")),
            Button::new(5, cx + 5.0, 100.0, 150.0, 20.0, t("selectWorld.mapType")),
            Button::new(
                6,
                cx - 155.0,
                151.0,
                150.0,
                20.0,
                t("selectWorld.allowCommands"),
            ),
            Button::new(
                8,
                cx + 5.0,
                120.0,
                150.0,
                20.0,
                t("selectWorld.customizeType"),
            ),
        ];
        for b in v.iter_mut().skip(4) {
            b.visible = false;
        }
        self.buttons = v;
        self.name = TextField::new(cx - 100.0, 60.0, 200.0, 20.0);
        self.name.set_focused(true);
        let n = self.world_name.clone();
        self.name.set_text(ui, &n);
        self.seed = TextField::new(cx - 100.0, 60.0, 200.0, 20.0);
        let s = self.world_seed.clone();
        self.seed.set_text(ui, &s);
        let more = self.more;
        self.show_more(more, lang);
        self.calc_save_dir(ctx.saves);
        self.update_display(lang);
        if self.hardcore {
            self.btn(6).enabled = false;
            self.btn(7).enabled = false;
        }
    }

    fn btn(&mut self, id: i32) -> &mut Button {
        self.buttons.iter_mut().find(|b| b.id == id).unwrap()
    }

    /// `calcSaveDirName`
    fn calc_save_dir(&mut self, saves: &Path) {
        let mut s: String = self.name.text().trim().to_string();
        for c in FILE_CHARS {
            s = s.replace(c, "_");
        }
        if s.is_empty() {
            s = "World".into();
        }
        // getUncollidingSaveDirName
        s = s.replace(['.', '/', '"'], "_");
        for d in DISALLOWED {
            if s.eq_ignore_ascii_case(d) {
                s = format!("_{s}_");
            }
        }
        while crate::world::anvil::read_level_dat(&saves.join(&s)).is_some() {
            s.push('-');
        }
        self.save_dir = s;
    }

    /// `updateDisplayState`
    fn update_display(&mut self, lang: &Lang) {
        let t = |k: &str| lang.get(k).to_string();
        let on_off = |b: bool| if b { t("options.on") } else { t("options.off") };
        let mode = self.mode.clone();
        let (st, bonus, hc, cheats, wt) = (
            self.structures,
            self.bonus,
            self.hardcore,
            self.cheats,
            self.world_type,
        );
        self.btn(2).label = format!(
            "{} {}",
            t("selectWorld.gameMode"),
            t(&format!("selectWorld.gameMode.{mode}"))
        );
        self.desc1 = t(&format!("selectWorld.gameMode.{mode}.line1"));
        self.desc2 = t(&format!("selectWorld.gameMode.{mode}.line2"));
        self.btn(4).label = format!("{} {}", t("selectWorld.mapFeatures"), on_off(st));
        self.btn(7).label = format!("{} {}", t("selectWorld.bonusItems"), on_off(bonus && !hc));
        self.btn(5).label = format!("{} {}", t("selectWorld.mapType"), t(WORLD_TYPES[wt].1));
        self.btn(6).label = format!(
            "{} {}",
            t("selectWorld.allowCommands"),
            on_off(cheats && !hc)
        );
    }

    /// `showMoreWorldOptions`
    fn show_more(&mut self, more: bool, lang: &Lang) {
        self.more = more;
        self.btn(2).visible = !more;
        for id in [4, 7, 5, 6] {
            self.btn(id).visible = more;
        }
        let flat = self.world_type == 1;
        self.btn(8).visible = more && flat;
        self.btn(3).label = if more {
            lang.get("gui.done").to_string()
        } else {
            lang.get("selectWorld.moreWorldOptions").to_string()
        };
    }

    /// `actionPerformed`; `None` lets the menu go back (Cancel).
    fn action(&mut self, b: Button, _ui: &Ui, ctx: &mut Ctx) -> Option<Action> {
        let lang = ctx.lang;
        if !b.enabled {
            return Some(Action::None);
        }
        match b.id {
            1 => return None,
            0 => {
                if self.generated {
                    return Some(Action::None);
                }
                self.generated = true;
                let mut seed = crate::java::JavaRandom::new(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_nanos() as i64),
                )
                .next_long();
                let s = self.seed.text();
                if !s.is_empty() {
                    match s.parse::<i64>() {
                        Ok(v) if v != 0 => seed = v,
                        Ok(_) => {}
                        Err(_) => seed = java_hash(&s) as i64,
                    }
                }
                let nw = NewWorld {
                    name: self.name.text().trim().to_string(),
                    seed,
                    mode: self.mode.clone(),
                    structures: self.structures,
                    bonus_chest: self.bonus && !self.hardcore,
                    cheats: self.cheats && !self.hardcore,
                    world_type: self.world_type,
                };
                return Some(Action::Play {
                    folder: ctx.saves.join(&self.save_dir),
                    new: Some(nw),
                });
            }
            3 => {
                let m = !self.more;
                self.show_more(m, lang);
            }
            2 => {
                if self.mode == "survival" {
                    if !self.cheats_by_user {
                        self.cheats = false;
                    }
                    self.mode = "hardcore".into();
                    self.hardcore = true;
                    self.btn(6).enabled = false;
                    self.btn(7).enabled = false;
                } else if self.mode == "hardcore" {
                    if !self.cheats_by_user {
                        self.cheats = true;
                    }
                    self.hardcore = false;
                    self.mode = "creative".into();
                    self.btn(6).enabled = true;
                    self.btn(7).enabled = true;
                } else {
                    if !self.cheats_by_user {
                        self.cheats = false;
                    }
                    self.mode = "survival".into();
                    self.btn(6).enabled = true;
                    self.btn(7).enabled = true;
                    self.hardcore = false;
                }
                self.update_display(lang);
            }
            4 => {
                self.structures = !self.structures;
                self.update_display(lang);
            }
            7 => {
                self.bonus = !self.bonus;
                self.update_display(lang);
            }
            5 => {
                self.world_type = (self.world_type + 1) % WORLD_TYPES.len();
                self.update_display(lang);
                let m = self.more;
                self.show_more(m, lang);
            }
            6 => {
                self.cheats_by_user = true;
                self.cheats = !self.cheats;
                self.update_display(lang);
            }
            _ => {}
        }
        Some(Action::None)
    }

    fn draw(&mut self, ui: &mut Ui, ctx: &Ctx, w: f32) {
        let lang = ctx.lang;
        let t = |k: &str| lang.get(k).to_string();
        let cx = (w as i32 / 2) as f32;
        default_background(ui, ctx.in_world);
        ui.centered(&t("selectWorld.create"), cx, 20.0, 0xFFFFFF);
        let g = 0xA0A0A0;
        if self.more {
            ui.text(&t("selectWorld.enterSeed"), cx - 100.0, 47.0, g, true);
            ui.text(&t("selectWorld.seedInfo"), cx - 100.0, 85.0, g, true);
            ui.text(
                &t("selectWorld.mapFeatures.info"),
                cx - 150.0,
                122.0,
                g,
                true,
            );
            ui.text(
                &t("selectWorld.allowCommands.info"),
                cx - 150.0,
                172.0,
                g,
                true,
            );
            self.seed.draw(ui);
            // WorldType.showWorldInfoNotice: amplified
            if self.world_type == 3 {
                let b = self.buttons.iter().find(|b| b.id == 5).unwrap();
                let (x, y, bw) = (b.x + 2.0, b.y + 22.0, b.w);
                let mut yy = y;
                for line in ui.wrap(&t("generator.amplified.info"), bw) {
                    ui.text(&line, x, yy, 10526880, false);
                    yy += 9.0;
                }
            }
        } else {
            ui.text(&t("selectWorld.enterName"), cx - 100.0, 47.0, g, true);
            let rf = format!("{} {}", t("selectWorld.resultFolder"), self.save_dir);
            ui.text(&rf, cx - 100.0, 85.0, g, true);
            self.name.draw(ui);
            ui.text(&self.desc1, cx - 100.0, 137.0, g, true);
            ui.text(&self.desc2, cx - 100.0, 149.0, g, true);
        }
        self.world_name = self.name.text();
        self.world_seed = self.seed.text();
    }
}

/// `GuiVideoSettings` with its `GuiOptionsRowList`.
pub struct OptionList {
    pub buttons: Vec<Button>,
    pub rows: Vec<(Button, Option<Button>)>,
    pub list: SlotList,
}

/// `GuiVideoSettings.videoOptions` without `ADVANCED_OPENGL` (not an
/// NVIDIA driver path).
const VIDEO: [Opt; 14] = [
    Opt::Graphics,
    Opt::RenderDistance,
    Opt::AmbientOcclusion,
    Opt::FramerateLimit,
    Opt::Anaglyph,
    Opt::ViewBobbing,
    Opt::GuiScale,
    Opt::Gamma,
    Opt::RenderClouds,
    Opt::Particles,
    Opt::UseFullscreen,
    Opt::EnableVsync,
    Opt::MipmapLevels,
    Opt::AnisotropicFiltering,
];

impl OptionList {
    fn new() -> OptionList {
        OptionList {
            buttons: Vec::new(),
            rows: Vec::new(),
            list: SlotList::new(0.0, 0.0, 0.0, 0.0, 25.0),
        }
    }

    fn init(&mut self, ctx: &mut Ctx, w: f32, h: f32) {
        let lang = ctx.lang;
        let cx = (w as i32 / 2) as f32;
        self.buttons = vec![Button::wide(
            200,
            cx - 100.0,
            h - 27.0,
            lang.get("gui.done"),
        )];
        let amount = self.list.amount;
        self.list = SlotList::new(w, h, 32.0, h - 32.0, 25.0);
        self.list.center_vertically = false;
        self.list.list_width = 400.0;
        self.list.scrollbar_x = w / 2.0 + 124.0 + 32.0;
        self.list.amount = amount;
        self.rows.clear();
        let mut i = 0;
        while i < VIDEO.len() {
            let a = Button::option(cx - 155.0, 0.0, VIDEO[i], ctx.settings, lang);
            let b = VIDEO
                .get(i + 1)
                .map(|o| Button::option(cx - 155.0 + 160.0, 0.0, *o, ctx.settings, lang));
            self.rows.push((a, b));
            i += 2;
        }
    }

    fn draw(&mut self, ui: &mut Ui, ctx: &mut Ctx, m: (f32, f32), down: bool) {
        let mut events = Vec::new();
        let n = self.rows.len();
        let rows = &mut self.rows;
        let settings = &mut *ctx.settings;
        let lang = ctx.lang;
        self.list.draw(
            ui,
            m,
            down,
            n,
            None,
            &mut events,
            &mut |ui, i, _x, y, _h| {
                let (a, b) = &mut rows[i];
                a.y = y;
                a.draw(ui, m, settings, lang);
                if let Some(b) = b {
                    b.y = y;
                    b.draw(ui, m, settings, lang);
                }
            },
        );
    }

    /// `mouseClickedVanilla`: a row's buttons.
    fn list_press(&mut self, m: (f32, f32), ctx: &mut Ctx) -> bool {
        if !(m.1 >= self.list.top && m.1 <= self.list.bottom) {
            return false;
        }
        let Some(i) = self.list.index_at(m, self.rows.len()) else {
            return false;
        };
        let lang = ctx.lang;
        let (a, b) = &mut self.rows[i];
        for btn in std::iter::once(a).chain(b.iter_mut()) {
            if btn.pressed(m) {
                match btn.kind {
                    Kind::Option(o) => {
                        ctx.settings.set_option(o, 1);
                        btn.label = ctx.settings.label(o, lang);
                    }
                    Kind::Slider { .. } => btn.press_slider(m, ctx.settings, lang),
                    _ => {}
                }
                self.list.enabled = false;
                return true;
            }
        }
        false
    }

    fn release(&mut self) {
        for (a, b) in self.rows.iter_mut() {
            a.release();
            if let Some(b) = b {
                b.release();
            }
        }
        self.list.enabled = true;
    }
}

/// `GuiControls` with its `GuiKeyBindingList`.
pub struct Controls {
    pub buttons: Vec<Button>,
    pub list: SlotList,
    /// `buttonId`: the binding waiting for a key.
    pub waiting: Option<usize>,
    /// Entries: a category label or a key index.
    entries: Vec<Result<usize, String>>,
    max_label: f32,
}

impl Controls {
    fn new() -> Controls {
        Controls {
            buttons: Vec::new(),
            list: SlotList::new(0.0, 0.0, 0.0, 0.0, 20.0),
            waiting: None,
            entries: Vec::new(),
            max_label: 0.0,
        }
    }

    fn init(&mut self, ctx: &mut Ctx, w: f32, h: f32) {
        let lang = ctx.lang;
        let cx = (w as i32 / 2) as f32;
        self.buttons = vec![
            Button::new(200, cx - 155.0, h - 29.0, 150.0, 20.0, lang.get("gui.done")),
            Button::new(
                201,
                cx - 155.0 + 160.0,
                h - 29.0,
                150.0,
                20.0,
                lang.get("controls.resetAll"),
            ),
        ];
        for (i, o) in [Opt::InvertMouse, Opt::Sensitivity, Opt::Touchscreen]
            .iter()
            .enumerate()
        {
            let x = cx - 155.0 + (i % 2) as f32 * 160.0;
            let y = 18.0 + 24.0 * (i >> 1) as f32;
            self.buttons
                .push(Button::option(x, y, *o, ctx.settings, lang));
        }
        let amount = self.list.amount;
        self.list = SlotList::new(w, h, 63.0, h - 32.0, 20.0);
        self.list.list_width = 220.0 + 32.0;
        self.list.scrollbar_x = w / 2.0 + 124.0 + 15.0;
        self.list.amount = amount;
        // Arrays.sort(keyBindings): by translated category, then description
        let mut idx: Vec<usize> = (0..ctx.settings.keys.len()).collect();
        let keys = &ctx.settings.keys;
        idx.sort_by(|&a, &b| {
            let (ka, kb) = (&keys[a], &keys[b]);
            java_cmp(lang.get(ka.category), lang.get(kb.category))
                .then_with(|| java_cmp(lang.get(ka.description), lang.get(kb.description)))
        });
        self.entries.clear();
        let mut cat = "";
        self.max_label = 0.0;
        for i in idx {
            let k = &keys[i];
            if k.category != cat {
                cat = k.category;
                self.entries.push(Err(lang.get(cat).to_string()));
            }
            self.entries.push(Ok(i));
        }
        let _ = &self.max_label;
    }

    fn draw(&mut self, ui: &mut Ui, ctx: &mut Ctx, m: (f32, f32), down: bool) {
        let lang = ctx.lang;
        if self.max_label == 0.0 {
            for e in &self.entries {
                if let Ok(i) = e {
                    let w = ui.text_width(lang.get(ctx.settings.keys[*i].description));
                    if w > self.max_label {
                        self.max_label = w;
                    }
                }
            }
        }
        let mut events = Vec::new();
        let entries = &self.entries;
        let keys = &ctx.settings.keys;
        let waiting = self.waiting;
        let max = self.max_label;
        let w = self.list.width;
        let n = entries.len();
        self.list.show_selection_box = false;
        let mut dummy = GameSettings::default();
        self.list.draw(
            ui,
            m,
            down,
            n,
            None,
            &mut events,
            &mut |ui, i, x, y, h| match &entries[i] {
                Err(label) => {
                    let lw = ui.text_width(label);
                    ui.text(
                        label,
                        w / 2.0 - (lw as i32 / 2) as f32,
                        y + h - 9.0 - 1.0,
                        0xFFFFFF,
                        false,
                    );
                }
                Ok(k) => {
                    let kb = &keys[*k];
                    ui.text(
                        lang.get(kb.description),
                        x + 90.0 - max,
                        y + (h as i32 / 2 - 9 / 2) as f32,
                        0xFFFFFF,
                        false,
                    );
                    let mut reset =
                        Button::new(0, x + 190.0, y, 50.0, 18.0, lang.get("controls.reset"));
                    reset.enabled = kb.code != kb.default;
                    reset.draw(ui, m, &mut dummy, lang);
                    let mut label = crate::settings::key_display(kb.code, lang);
                    let dup = kb.code != 0
                        && keys
                            .iter()
                            .enumerate()
                            .any(|(j, o)| j != *k && o.code == kb.code);
                    if waiting == Some(*k) {
                        label = format!("\u{a7}f> \u{a7}e{label}\u{a7}f <");
                    } else if dup {
                        label = format!("\u{a7}c{label}");
                    }
                    let mut change = Button::new(0, x + 105.0, y, 75.0, 18.0, label);
                    change.draw(ui, m, &mut dummy, lang);
                }
            },
        );
    }

    /// `mouseClickedVanilla` on the key list.
    fn list_press(&mut self, m: (f32, f32), ctx: &mut Ctx) -> Option<Action> {
        if !(m.1 >= self.list.top && m.1 <= self.list.bottom) {
            return None;
        }
        let i = self.list.index_at(m, self.entries.len())?;
        let Ok(k) = self.entries[i] else { return None };
        let x = self.list.left + self.list.width / 2.0 - self.list.list_width / 2.0 + 2.0;
        let y = self.list.top + 4.0 - self.list.amount as i32 as f32
            + i as f32 * self.list.slot_height
            + self.list.header_padding;
        let change = Button::new(0, x + 105.0, y, 75.0, 18.0, "");
        let mut reset = Button::new(0, x + 190.0, y, 50.0, 18.0, "");
        let kb = &ctx.settings.keys[k];
        reset.enabled = kb.code != kb.default;
        if change.pressed(m) {
            self.waiting = Some(k);
            ctx.sounds.push("gui.button.press");
            return Some(Action::None);
        }
        if reset.pressed(m) {
            let d = ctx.settings.keys[k].default;
            ctx.settings.keys[k].code = d;
            ctx.settings.save();
            ctx.sounds.push("gui.button.press");
            return Some(Action::None);
        }
        None
    }
}

/// `String.compareTo`: UTF-16 code units.
fn java_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// `GuiLanguage`
pub struct LanguageScreen {
    pub buttons: Vec<Button>,
    pub list: SlotList,
}

impl LanguageScreen {
    fn new() -> LanguageScreen {
        LanguageScreen {
            buttons: Vec::new(),
            list: SlotList::new(0.0, 0.0, 0.0, 0.0, 18.0),
        }
    }
}
