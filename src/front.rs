//! What `Minecraft` holds outside a world: the settings, the languages,
//! the asset store and the title screen, drawn without a world.

use crate::assets::Assets;
use crate::lang::{Lang, Language};
use crate::menu::{Action, Ctx, Menu, NewWorld};
use crate::render::atlas::Atlas;
use crate::render::vk::Renderer;
use crate::settings::GameSettings;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

/// The parts of `Minecraft` that outlive a world.
pub struct Shell {
    pub settings: GameSettings,
    pub languages: Vec<Language>,
    pub assets: Option<Assets>,
    pub jar_path: PathBuf,
    /// `saves/`
    pub saves: PathBuf,
    pub clipboard: String,
    /// `texts/splashes.txt`
    pub splashes: Vec<String>,
    pub ctrl: bool,
    pub shift: bool,
}

impl Shell {
    /// `LanguageManager.onResourceManagerReload`: the language and the
    /// `FontRenderer` unicode flag.
    pub fn load_lang(&self, atlas: &Atlas) -> Lang {
        let lang = match zip::ZipArchive::new(match std::fs::File::open(&self.jar_path) {
            Ok(f) => f,
            Err(_) => return Lang::default(),
        }) {
            Ok(mut jar) => {
                Lang::load_language(&mut jar, self.assets.as_ref(), &self.settings.language)
            }
            Err(_) => Lang::default(),
        };
        if let Ok(mut g) = atlas.glyphs.lock() {
            g.unicode = lang.is_unicode() || self.settings.force_unicode_font;
        }
        lang
    }

    /// `ScaledResolution`: the GUI scale for a window size.
    pub fn scale(&self, atlas: &Atlas, size: (u32, u32)) -> u32 {
        let unicode = atlas.glyphs.lock().map_or(false, |g| g.unicode);
        let mut gs = self.settings.gui_scale as u32;
        if gs == 0 {
            gs = 1000;
        }
        let mut sf = 1u32;
        while sf < gs && size.0 / (sf + 1) >= 320 && size.1 / (sf + 1) >= 240 {
            sf += 1;
        }
        if unicode && sf % 2 != 0 && sf != 1 {
            sf -= 1;
        }
        sf
    }
}

/// The title screen and the menus before a world.
pub struct Front {
    pub renderer: Renderer,
    pub atlas: Arc<Atlas>,
    pub shell: Shell,
    pub lang: Lang,
    pub menu: Menu,
    pub audio: Option<crate::audio::Audio>,
    panorama: crate::panorama::Panorama,
    pub mouse_px: (f32, f32),
    last: Instant,
    tick_acc: f64,
    scale: u32,
    size: (u32, u32),
    pub screenshot: Option<PathBuf>,
}

impl Front {
    pub fn new(
        renderer: Renderer,
        atlas: Arc<Atlas>,
        shell: Shell,
        lang: Lang,
        audio: Option<crate::audio::Audio>,
    ) -> Front {
        let panorama = crate::panorama::Panorama::new(&atlas.ui);
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as i64);
        let mut r = crate::java::JavaRandom::new(seed);
        let menu = Menu::with_splash(&shell.splashes, &mut r);
        Front {
            renderer,
            atlas,
            shell,
            lang,
            menu,
            audio,
            panorama,
            mouse_px: (0.0, 0.0),
            last: Instant::now(),
            tick_acc: 0.0,
            scale: 0,
            size: (0, 0),
            screenshot: None,
        }
    }

    fn ctx<'a>(shell: &'a mut Shell, lang: &'a Lang, sounds: &'a mut Vec<&'static str>) -> Ctx<'a> {
        Ctx {
            settings: &mut shell.settings,
            lang,
            languages: &shell.languages,
            saves: &shell.saves,
            in_world: false,
            hardcore: false,
            clipboard: &mut shell.clipboard,
            sounds,
            ctrl: shell.ctrl,
            shift: shell.shift,
        }
    }

    fn gui_mouse(&self) -> (f32, f32) {
        let s = self.scale.max(1) as f32;
        ((self.mouse_px.0 / s).floor(), (self.mouse_px.1 / s).floor())
    }

    fn play_sounds(&mut self, sounds: Vec<&'static str>) {
        if let Some(a) = self.audio.as_mut() {
            for s in sounds {
                a.play(s, None, 1.0, 1.0);
            }
        }
    }

    /// Applies what a menu action changed; returns the action for the app.
    fn handle(&mut self, a: Action) -> Action {
        match a {
            Action::Language => {
                self.lang = self.shell.load_lang(&self.atlas);
                self.relayout();
                Action::None
            }
            Action::Settings => {
                self.relayout();
                Action::None
            }
            other => other,
        }
    }

    fn relayout(&mut self) {
        self.scale = 0;
    }

    /// One frame: the menu ticks at 20 per second, then the screen.
    pub fn frame(&mut self, size: (u32, u32)) -> Action {
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f64().clamp(0.0, 1.0);
        self.last = now;
        self.tick_acc += dt * 20.0;
        let n = (self.tick_acc as i32).min(10);
        self.tick_acc -= self.tick_acc.floor();
        for _ in 0..n {
            self.menu.tick();
            if let Some(a) = self.audio.as_mut() {
                a.tick_music();
            }
        }
        let partial = self.tick_acc as f32;
        let sf = self.shell.scale(&self.atlas, size);
        let w = (size.0 as f64 / sf as f64).ceil() as f32;
        let h = (size.1 as f64 / sf as f64).ceil() as f32;
        let mut sounds = Vec::new();
        {
            let ui = crate::gui::Ui::new(&self.atlas, w, h);
            if sf != self.scale || size != self.size {
                self.scale = sf;
                self.size = size;
                let mut ctx = Self::ctx(&mut self.shell, &self.lang, &mut sounds);
                self.menu.resize(&ui, &mut ctx, w, h);
            }
        }
        self.menu.mouse = self.gui_mouse();
        let mut ui = crate::gui::Ui::new(&self.atlas, w, h);
        if let Some(crate::menu::Screen::Main(m)) = self.menu.stack.last() {
            // GuiMainMenu.renderSkybox
            let t = m.panorama_timer as f32 + partial;
            let px = self.panorama.render(t);
            self.renderer.upload_ui_region(
                crate::panorama::BG_X,
                crate::panorama::BG_Y,
                256,
                256,
                &px,
            );
            let uv = crate::panorama::screen_uv(w, h);
            ui.linear_quad([[0.0, 0.0], [0.0, h], [w, h], [w, 0.0]], uv);
        }
        {
            let mut ctx = Self::ctx(&mut self.shell, &self.lang, &mut sounds);
            self.menu.draw(&mut ui, &mut ctx, partial);
        }
        let mut pending = self.menu.take_pending();
        // MINERUST_AUTOCREATE=<name>: "Create New World" with the defaults
        if let Ok(name) = std::env::var("MINERUST_AUTOCREATE") {
            std::env::remove_var("MINERUST_AUTOCREATE");
            pending = Action::Play {
                folder: self.shell.saves.join(&name),
                new: Some(NewWorld {
                    name: name.clone(),
                    seed: 12345,
                    mode: "survival".into(),
                    structures: true,
                    bonus_chest: false,
                    cheats: false,
                    world_type: 0,
                }),
            };
        }
        let verts = ui.v;
        self.renderer.sync_glyphs(&self.atlas);
        crate::game::draw_ui_only(
            &mut self.renderer,
            &self.atlas,
            &verts,
            sf as f32,
            size,
            self.screenshot.take(),
        );
        self.play_sounds(sounds);
        self.handle(pending)
    }

    pub fn mouse_button(&mut self, button: u8, pressed: bool) -> Action {
        let mut sounds = Vec::new();
        self.menu.mouse = self.gui_mouse();
        let (w, h) = (self.menu.w, self.menu.h);
        let ui = crate::gui::Ui::new(&self.atlas, w, h);
        let a = {
            let mut ctx = Self::ctx(&mut self.shell, &self.lang, &mut sounds);
            if pressed {
                self.menu.mouse_down(&ui, &mut ctx, button)
            } else {
                self.menu.mouse_up(&mut ctx, button);
                Action::None
            }
        };
        drop(ui);
        self.play_sounds(sounds);
        self.handle(a)
    }

    pub fn wheel(&mut self, up: bool) {
        self.menu.wheel(up);
    }

    /// A key press: its character (or '\0') and LWJGL code.
    pub fn key(&mut self, c: char, code: i32) -> Action {
        let mut sounds = Vec::new();
        let (w, h) = (self.menu.w, self.menu.h);
        let ui = crate::gui::Ui::new(&self.atlas, w, h);
        let a = {
            let mut ctx = Self::ctx(&mut self.shell, &self.lang, &mut sounds);
            self.menu.key(&ui, &mut ctx, c, code)
        };
        drop(ui);
        self.play_sounds(sounds);
        self.handle(a)
    }
}

/// `WorldInfo` of a new world (`new WorldInfo(settings, name)`), as
/// `level.dat` before the server initializes it.
pub fn new_level_dat(dir: &std::path::Path, w: &NewWorld) -> std::io::Result<()> {
    use crate::world::anvil::{compound, Nbt};
    std::fs::create_dir_all(dir)?;
    let (gen, ver) = match crate::menu::WORLD_TYPES[w.world_type].0 {
        "default" => ("default", 1),
        "flat" => ("flat", 0),
        "largeBiomes" => ("largeBiomes", 0),
        _ => ("amplified", 0),
    };
    let game_type = if w.mode == "creative" { 1 } else { 0 };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |t| t.as_millis() as i64);
    let root = compound(vec![(
        "Data",
        compound(vec![
            ("RandomSeed", Nbt::Long(w.seed)),
            ("generatorName", Nbt::String(gen.into())),
            ("generatorVersion", Nbt::Int(ver)),
            ("generatorOptions", Nbt::String(String::new())),
            ("GameType", Nbt::Int(game_type)),
            ("MapFeatures", Nbt::Byte(w.structures as i8)),
            ("SpawnX", Nbt::Int(0)),
            ("SpawnY", Nbt::Int(0)),
            ("SpawnZ", Nbt::Int(0)),
            ("Time", Nbt::Long(0)),
            ("DayTime", Nbt::Long(0)),
            ("SizeOnDisk", Nbt::Long(0)),
            ("LastPlayed", Nbt::Long(now)),
            ("LevelName", Nbt::String(w.name.clone())),
            ("version", Nbt::Int(19133)),
            ("rainTime", Nbt::Int(0)),
            ("raining", Nbt::Byte(0)),
            ("thunderTime", Nbt::Int(0)),
            ("thundering", Nbt::Byte(0)),
            ("hardcore", Nbt::Byte((w.mode == "hardcore") as i8)),
            ("initialized", Nbt::Byte(0)),
            ("allowCommands", Nbt::Byte(w.cheats as i8)),
        ]),
    )]);
    crate::world::anvil::write_level_dat(dir, &root)
}
