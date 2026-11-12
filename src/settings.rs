//! `GameSettings`, `GameSettings$Options` and `KeyBinding`: the fields and
//! defaults, `options.txt` in vanilla's order and format, and the option
//! buttons' labels (`getKeyBinding`) and changes (`setOptionValue`,
//! `setOptionFloatValue`).

use crate::lang::Lang;
use std::path::PathBuf;

/// `GameSettings$Options`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opt {
    InvertMouse,
    Sensitivity,
    Fov,
    Gamma,
    Saturation,
    RenderDistance,
    ViewBobbing,
    Anaglyph,
    AdvancedOpengl,
    FramerateLimit,
    FboEnable,
    Difficulty,
    Graphics,
    AmbientOcclusion,
    GuiScale,
    RenderClouds,
    Particles,
    ChatVisibility,
    ChatColor,
    ChatLinks,
    ChatOpacity,
    ChatLinksPrompt,
    SnooperEnabled,
    UseFullscreen,
    EnableVsync,
    ShowCape,
    Touchscreen,
    ChatScale,
    ChatWidth,
    ChatHeightFocused,
    ChatHeightUnfocused,
    MipmapLevels,
    AnisotropicFiltering,
    ForceUnicodeFont,
}

impl Opt {
    /// `enumString`, `enumFloat`, `enumBoolean`, min, max, step
    pub fn meta(self) -> (&'static str, bool, bool, f32, f32, f32) {
        use Opt::*;
        match self {
            InvertMouse => ("options.invertMouse", false, true, 0.0, 1.0, 0.0),
            Sensitivity => ("options.sensitivity", true, false, 0.0, 1.0, 0.0),
            Fov => ("options.fov", true, false, 30.0, 110.0, 1.0),
            Gamma => ("options.gamma", true, false, 0.0, 1.0, 0.0),
            Saturation => ("options.saturation", true, false, 0.0, 1.0, 0.0),
            // GameSettings(): RENDER_DISTANCE.setValueMax(16.0F)
            RenderDistance => ("options.renderDistance", true, false, 2.0, 16.0, 1.0),
            ViewBobbing => ("options.viewBobbing", false, true, 0.0, 1.0, 0.0),
            Anaglyph => ("options.anaglyph", false, true, 0.0, 1.0, 0.0),
            AdvancedOpengl => ("options.advancedOpengl", false, true, 0.0, 1.0, 0.0),
            FramerateLimit => ("options.framerateLimit", true, false, 10.0, 260.0, 10.0),
            FboEnable => ("options.fboEnable", false, true, 0.0, 1.0, 0.0),
            Difficulty => ("options.difficulty", false, false, 0.0, 1.0, 0.0),
            Graphics => ("options.graphics", false, false, 0.0, 1.0, 0.0),
            AmbientOcclusion => ("options.ao", false, false, 0.0, 1.0, 0.0),
            GuiScale => ("options.guiScale", false, false, 0.0, 1.0, 0.0),
            RenderClouds => ("options.renderClouds", false, true, 0.0, 1.0, 0.0),
            Particles => ("options.particles", false, false, 0.0, 1.0, 0.0),
            ChatVisibility => ("options.chat.visibility", false, false, 0.0, 1.0, 0.0),
            ChatColor => ("options.chat.color", false, true, 0.0, 1.0, 0.0),
            ChatLinks => ("options.chat.links", false, true, 0.0, 1.0, 0.0),
            ChatOpacity => ("options.chat.opacity", true, false, 0.0, 1.0, 0.0),
            ChatLinksPrompt => ("options.chat.links.prompt", false, true, 0.0, 1.0, 0.0),
            SnooperEnabled => ("options.snooper", false, true, 0.0, 1.0, 0.0),
            UseFullscreen => ("options.fullscreen", false, true, 0.0, 1.0, 0.0),
            EnableVsync => ("options.vsync", false, true, 0.0, 1.0, 0.0),
            ShowCape => ("options.showCape", false, true, 0.0, 1.0, 0.0),
            Touchscreen => ("options.touchscreen", false, true, 0.0, 1.0, 0.0),
            ChatScale => ("options.chat.scale", true, false, 0.0, 1.0, 0.0),
            ChatWidth => ("options.chat.width", true, false, 0.0, 1.0, 0.0),
            ChatHeightFocused => ("options.chat.height.focused", true, false, 0.0, 1.0, 0.0),
            ChatHeightUnfocused => ("options.chat.height.unfocused", true, false, 0.0, 1.0, 0.0),
            MipmapLevels => ("options.mipmapLevels", true, false, 0.0, 4.0, 1.0),
            AnisotropicFiltering => ("options.anisotropicFiltering", true, false, 1.0, 16.0, 0.0),
            ForceUnicodeFont => ("options.forceUnicodeFont", false, true, 0.0, 1.0, 0.0),
        }
    }
    pub fn is_float(self) -> bool {
        self.meta().1
    }
    pub fn is_bool(self) -> bool {
        self.meta().2
    }
    fn snap_to_step(self, v: f32) -> f32 {
        let step = self.meta().5;
        if step > 0.0 {
            step * (v / step).round()
        } else {
            v
        }
    }
    pub fn snap_to_step_clamp(self, v: f32) -> f32 {
        let (_, _, _, lo, hi, _) = self.meta();
        self.snap_to_step(v).clamp(lo, hi)
    }
    pub fn normalize(self, v: f32) -> f32 {
        let (_, _, _, lo, hi, _) = self.meta();
        ((self.snap_to_step_clamp(v) - lo) / (hi - lo)).clamp(0.0, 1.0)
    }
    pub fn denormalize(self, v: f32) -> f32 {
        let (_, _, _, lo, hi, _) = self.meta();
        self.snap_to_step_clamp(lo + (hi - lo) * v.clamp(0.0, 1.0))
    }
}

/// `SoundCategory` names, in order.
pub const SOUND_CATEGORIES: [&str; 9] = [
    "master", "music", "record", "weather", "block", "hostile", "neutral", "player", "ambient",
];

/// `KeyBinding`: description, LWJGL key code (negative: mouse button - 100),
/// category.
#[derive(Clone, Debug)]
pub struct KeyBinding {
    pub description: &'static str,
    pub code: i32,
    pub default: i32,
    pub category: &'static str,
}

fn kb(d: &'static str, c: i32, cat: &'static str) -> KeyBinding {
    KeyBinding {
        description: d,
        code: c,
        default: c,
        category: cat,
    }
}

/// `GameSettings.keyBindings`, in its order.
pub fn default_keys() -> Vec<KeyBinding> {
    let mut v = vec![
        kb("key.attack", -100, "key.categories.gameplay"),
        kb("key.use", -99, "key.categories.gameplay"),
        kb("key.forward", 17, "key.categories.movement"),
        kb("key.left", 30, "key.categories.movement"),
        kb("key.back", 31, "key.categories.movement"),
        kb("key.right", 32, "key.categories.movement"),
        kb("key.jump", 57, "key.categories.movement"),
        kb("key.sneak", 42, "key.categories.movement"),
        kb("key.drop", 16, "key.categories.gameplay"),
        kb("key.inventory", 18, "key.categories.inventory"),
        kb("key.chat", 20, "key.categories.multiplayer"),
        kb("key.playerlist", 15, "key.categories.multiplayer"),
        kb("key.pickItem", -98, "key.categories.gameplay"),
        kb("key.command", 53, "key.categories.multiplayer"),
        kb("key.screenshot", 60, "key.categories.misc"),
        kb("key.togglePerspective", 63, "key.categories.misc"),
        kb("key.smoothCamera", 0, "key.categories.misc"),
        kb("key.sprint", 29, "key.categories.gameplay"),
        kb("key.streamStartStop", 64, "key.categories.stream"),
        kb("key.streamPauseUnpause", 65, "key.categories.stream"),
        kb("key.streamCommercial", 0, "key.categories.stream"),
        kb("key.streamToggleMic", 0, "key.categories.stream"),
        kb("key.fullscreen", 87, "key.categories.misc"),
    ];
    const HOTBAR: [&str; 9] = [
        "key.hotbar.1",
        "key.hotbar.2",
        "key.hotbar.3",
        "key.hotbar.4",
        "key.hotbar.5",
        "key.hotbar.6",
        "key.hotbar.7",
        "key.hotbar.8",
        "key.hotbar.9",
    ];
    for (i, d) in HOTBAR.iter().enumerate() {
        v.push(kb(d, 2 + i as i32, "key.categories.inventory"));
    }
    v
}

/// `GameSettings`
#[derive(Clone, Debug)]
pub struct GameSettings {
    pub mouse_sensitivity: f32,
    pub invert_mouse: bool,
    pub render_distance: i32,
    pub view_bobbing: bool,
    pub anaglyph: bool,
    pub advanced_opengl: bool,
    pub fbo_enable: bool,
    pub limit_framerate: i32,
    pub fancy_graphics: bool,
    pub ambient_occlusion: i32,
    pub clouds: bool,
    pub resource_packs: Vec<String>,
    /// `EntityPlayer$EnumChatVisibility`: 0 full, 1 system, 2 hidden
    pub chat_visibility: i32,
    pub chat_colours: bool,
    pub chat_links: bool,
    pub chat_links_prompt: bool,
    pub chat_opacity: f32,
    pub snooper_enabled: bool,
    pub full_screen: bool,
    pub enable_vsync: bool,
    pub hide_server_address: bool,
    pub advanced_item_tooltips: bool,
    pub pause_on_lost_focus: bool,
    pub show_cape: bool,
    pub touchscreen: bool,
    pub override_width: i32,
    pub override_height: i32,
    pub held_item_tooltips: bool,
    pub chat_scale: f32,
    pub chat_width: f32,
    pub chat_height_unfocused: f32,
    pub chat_height_focused: f32,
    pub show_inventory_achievement_hint: bool,
    pub mipmap_levels: i32,
    pub anisotropic_filtering: i32,
    pub sound_levels: [f32; 9],
    pub stream: [String; 11],
    pub keys: Vec<KeyBinding>,
    /// `EnumDifficulty` id
    pub difficulty: i32,
    pub hide_gui: bool,
    pub third_person_view: i32,
    pub show_debug_info: bool,
    pub last_server: String,
    pub smooth_camera: bool,
    pub fov: f32,
    pub gamma: f32,
    pub saturation: f32,
    pub gui_scale: i32,
    pub particle_setting: i32,
    pub language: String,
    pub force_unicode_font: bool,
    pub file: Option<PathBuf>,
}

impl Default for GameSettings {
    fn default() -> Self {
        GameSettings {
            mouse_sensitivity: 0.5,
            invert_mouse: false,
            // Minecraft.isJava64bit() ? 12 : 8
            render_distance: 12,
            view_bobbing: true,
            anaglyph: false,
            advanced_opengl: false,
            fbo_enable: true,
            limit_framerate: 120,
            fancy_graphics: true,
            ambient_occlusion: 2,
            clouds: true,
            resource_packs: Vec::new(),
            chat_visibility: 0,
            chat_colours: true,
            chat_links: true,
            chat_links_prompt: true,
            chat_opacity: 1.0,
            snooper_enabled: true,
            full_screen: false,
            enable_vsync: true,
            hide_server_address: false,
            advanced_item_tooltips: false,
            pause_on_lost_focus: true,
            show_cape: true,
            touchscreen: false,
            override_width: 0,
            override_height: 0,
            held_item_tooltips: true,
            chat_scale: 1.0,
            chat_width: 1.0,
            chat_height_unfocused: 0.44366196,
            chat_height_focused: 1.0,
            show_inventory_achievement_hint: true,
            mipmap_levels: 4,
            anisotropic_filtering: 1,
            sound_levels: [1.0; 9],
            stream: [
                "0.5".into(),
                "1.0".into(),
                "1.0".into(),
                "0.5412844".into(),
                "0.31690142".into(),
                "1".into(),
                "true".into(),
                "".into(),
                "0".into(),
                "0".into(),
                "0".into(),
            ],
            keys: default_keys(),
            difficulty: 2,
            hide_gui: false,
            third_person_view: 0,
            show_debug_info: false,
            last_server: String::new(),
            smooth_camera: false,
            fov: 70.0,
            gamma: 0.0,
            saturation: 0.0,
            gui_scale: 0,
            particle_setting: 0,
            language: "en_US".into(),
            force_unicode_font: false,
            file: None,
        }
    }
}

const STREAM_KEYS: [&str; 11] = [
    "streamBytesPerPixel",
    "streamMicVolume",
    "streamSystemVolume",
    "streamKbps",
    "streamFps",
    "streamCompression",
    "streamSendMetadata",
    "streamPreferredServer",
    "streamChatEnabled",
    "streamChatUserFilter",
    "streamMicToggleBehavior",
];

/// Java's `Float.toString` for the values options.txt holds.
pub fn jf(v: f32) -> String {
    format!("{v:?}")
}

/// `GameSettings.parseFloat`: "true" is 1, "false" 0.
fn pf(s: &str) -> f32 {
    match s {
        "true" => 1.0,
        "false" => 0.0,
        _ => s.trim().parse().unwrap_or(0.0),
    }
}

impl GameSettings {
    /// `new GameSettings(mc, dir)`: defaults, then `loadOptions`.
    pub fn load(dir: &std::path::Path) -> GameSettings {
        let mut s = GameSettings {
            file: Some(dir.join("options.txt")),
            ..Default::default()
        };
        if let Ok(text) = std::fs::read_to_string(dir.join("options.txt")) {
            for line in text.lines() {
                s.load_line(line);
            }
        }
        s
    }

    /// One line of `loadOptions`.
    fn load_line(&mut self, line: &str) {
        let p: Vec<&str> = line.split(':').collect();
        if p.len() < 2 {
            return;
        }
        let (k, v) = (p[0], p[1]);
        let b = v == "true";
        let int = |v: &str| v.trim().parse::<i32>().ok();
        match k {
            "mouseSensitivity" => self.mouse_sensitivity = pf(v),
            "invertYMouse" => self.invert_mouse = b,
            "fov" => self.fov = pf(v) * 40.0 + 70.0,
            "gamma" => self.gamma = pf(v),
            "saturation" => self.saturation = pf(v),
            "renderDistance" => {
                if let Some(i) = int(v) {
                    self.render_distance = i;
                }
            }
            "guiScale" => self.gui_scale = int(v).unwrap_or(self.gui_scale),
            "particles" => self.particle_setting = int(v).unwrap_or(self.particle_setting),
            "bobView" => self.view_bobbing = b,
            "anaglyph3d" => self.anaglyph = b,
            "advancedOpengl" => self.advanced_opengl = b,
            "maxFps" => self.limit_framerate = int(v).unwrap_or(self.limit_framerate),
            "fboEnable" => self.fbo_enable = b,
            "difficulty" => self.difficulty = int(v).unwrap_or(self.difficulty) & 3,
            "fancyGraphics" => self.fancy_graphics = b,
            "ao" => {
                self.ambient_occlusion = match v {
                    "true" => 2,
                    "false" => 0,
                    _ => int(v).unwrap_or(2),
                }
            }
            "clouds" => self.clouds = b,
            "resourcePacks" => {
                // gson: ["a","b"]; the value may itself contain ':'
                let rest = line.splitn(2, ':').nth(1).unwrap_or("");
                self.resource_packs = rest
                    .trim()
                    .trim_start_matches('[')
                    .trim_end_matches(']')
                    .split(',')
                    .map(|x| x.trim().trim_matches('"').to_string())
                    .filter(|x| !x.is_empty())
                    .collect();
            }
            "lastServer" => self.last_server = line.splitn(2, ':').nth(1).unwrap_or("").to_string(),
            "lang" => self.language = v.to_string(),
            "chatVisibility" => self.chat_visibility = int(v).unwrap_or(0).rem_euclid(3),
            "chatColors" => self.chat_colours = b,
            "chatLinks" => self.chat_links = b,
            "chatLinksPrompt" => self.chat_links_prompt = b,
            "chatOpacity" => self.chat_opacity = pf(v),
            "snooperEnabled" => self.snooper_enabled = b,
            "fullscreen" => self.full_screen = b,
            "enableVsync" => self.enable_vsync = b,
            "hideServerAddress" => self.hide_server_address = b,
            "advancedItemTooltips" => self.advanced_item_tooltips = b,
            "pauseOnLostFocus" => self.pause_on_lost_focus = b,
            "showCape" => self.show_cape = b,
            "touchscreen" => self.touchscreen = b,
            "overrideHeight" => self.override_height = int(v).unwrap_or(0),
            "overrideWidth" => self.override_width = int(v).unwrap_or(0),
            "heldItemTooltips" => self.held_item_tooltips = b,
            "chatHeightFocused" => self.chat_height_focused = pf(v),
            "chatHeightUnfocused" => self.chat_height_unfocused = pf(v),
            "chatScale" => self.chat_scale = pf(v),
            "chatWidth" => self.chat_width = pf(v),
            "showInventoryAchievementHint" => self.show_inventory_achievement_hint = b,
            "mipmapLevels" => self.mipmap_levels = int(v).unwrap_or(4),
            "anisotropicFiltering" => self.anisotropic_filtering = int(v).unwrap_or(1),
            "forceUnicodeFont" => self.force_unicode_font = b,
            _ => {
                if let Some(i) = STREAM_KEYS.iter().position(|s| *s == k) {
                    self.stream[i] = v.to_string();
                } else if let Some(d) = k.strip_prefix("key_") {
                    if let Some(key) = self.keys.iter_mut().find(|x| x.description == d) {
                        if let Some(c) = int(v) {
                            key.code = c;
                        }
                    }
                } else if let Some(c) = k.strip_prefix("soundCategory_") {
                    if let Some(i) = SOUND_CATEGORIES.iter().position(|s| *s == c) {
                        self.sound_levels[i] = pf(v);
                    }
                }
            }
        }
    }

    /// `saveOptions`
    pub fn save(&self) {
        let Some(path) = &self.file else { return };
        let mut o = Vec::new();
        let b = |v: bool| if v { "true" } else { "false" };
        o.push(format!("invertYMouse:{}", b(self.invert_mouse)));
        o.push(format!("mouseSensitivity:{}", jf(self.mouse_sensitivity)));
        o.push(format!("fov:{}", jf((self.fov - 70.0) / 40.0)));
        o.push(format!("gamma:{}", jf(self.gamma)));
        o.push(format!("saturation:{}", jf(self.saturation)));
        o.push(format!("renderDistance:{}", self.render_distance));
        o.push(format!("guiScale:{}", self.gui_scale));
        o.push(format!("particles:{}", self.particle_setting));
        o.push(format!("bobView:{}", b(self.view_bobbing)));
        o.push(format!("anaglyph3d:{}", b(self.anaglyph)));
        o.push(format!("advancedOpengl:{}", b(self.advanced_opengl)));
        o.push(format!("maxFps:{}", self.limit_framerate));
        o.push(format!("fboEnable:{}", b(self.fbo_enable)));
        o.push(format!("difficulty:{}", self.difficulty));
        o.push(format!("fancyGraphics:{}", b(self.fancy_graphics)));
        o.push(format!("ao:{}", self.ambient_occlusion));
        o.push(format!("clouds:{}", b(self.clouds)));
        let packs: Vec<String> = self
            .resource_packs
            .iter()
            .map(|p| format!("\"{p}\""))
            .collect();
        o.push(format!("resourcePacks:[{}]", packs.join(",")));
        o.push(format!("lastServer:{}", self.last_server));
        o.push(format!("lang:{}", self.language));
        o.push(format!("chatVisibility:{}", self.chat_visibility));
        o.push(format!("chatColors:{}", b(self.chat_colours)));
        o.push(format!("chatLinks:{}", b(self.chat_links)));
        o.push(format!("chatLinksPrompt:{}", b(self.chat_links_prompt)));
        o.push(format!("chatOpacity:{}", jf(self.chat_opacity)));
        o.push(format!("snooperEnabled:{}", b(self.snooper_enabled)));
        o.push(format!("fullscreen:{}", b(self.full_screen)));
        o.push(format!("enableVsync:{}", b(self.enable_vsync)));
        o.push(format!("hideServerAddress:{}", b(self.hide_server_address)));
        o.push(format!(
            "advancedItemTooltips:{}",
            b(self.advanced_item_tooltips)
        ));
        o.push(format!("pauseOnLostFocus:{}", b(self.pause_on_lost_focus)));
        o.push(format!("showCape:{}", b(self.show_cape)));
        o.push(format!("touchscreen:{}", b(self.touchscreen)));
        o.push(format!("overrideWidth:{}", self.override_width));
        o.push(format!("overrideHeight:{}", self.override_height));
        o.push(format!("heldItemTooltips:{}", b(self.held_item_tooltips)));
        o.push(format!(
            "chatHeightFocused:{}",
            jf(self.chat_height_focused)
        ));
        o.push(format!(
            "chatHeightUnfocused:{}",
            jf(self.chat_height_unfocused)
        ));
        o.push(format!("chatScale:{}", jf(self.chat_scale)));
        o.push(format!("chatWidth:{}", jf(self.chat_width)));
        o.push(format!(
            "showInventoryAchievementHint:{}",
            b(self.show_inventory_achievement_hint)
        ));
        o.push(format!("mipmapLevels:{}", self.mipmap_levels));
        o.push(format!(
            "anisotropicFiltering:{}",
            self.anisotropic_filtering
        ));
        for (k, v) in STREAM_KEYS.iter().zip(self.stream.iter()) {
            o.push(format!("{k}:{v}"));
        }
        o.push(format!("forceUnicodeFont:{}", b(self.force_unicode_font)));
        for k in &self.keys {
            o.push(format!("key_{}:{}", k.description, k.code));
        }
        for (c, v) in SOUND_CATEGORIES.iter().zip(self.sound_levels.iter()) {
            o.push(format!("soundCategory_{c}:{}", jf(*v)));
        }
        let mut text = o.join("\n");
        text.push('\n');
        let _ = std::fs::write(path, text);
    }

    pub fn key(&self, d: &str) -> i32 {
        self.keys
            .iter()
            .find(|k| k.description == d)
            .map_or(0, |k| k.code)
    }

    /// `getOptionFloatValue`
    pub fn get_float(&self, o: Opt) -> f32 {
        use Opt::*;
        match o {
            Fov => self.fov,
            Gamma => self.gamma,
            Saturation => self.saturation,
            Sensitivity => self.mouse_sensitivity,
            ChatOpacity => self.chat_opacity,
            ChatHeightFocused => self.chat_height_focused,
            ChatHeightUnfocused => self.chat_height_unfocused,
            ChatScale => self.chat_scale,
            ChatWidth => self.chat_width,
            FramerateLimit => self.limit_framerate as f32,
            AnisotropicFiltering => self.anisotropic_filtering as f32,
            MipmapLevels => self.mipmap_levels as f32,
            RenderDistance => self.render_distance as f32,
            _ => 0.0,
        }
    }

    /// `setOptionFloatValue`
    pub fn set_float(&mut self, o: Opt, v: f32) {
        use Opt::*;
        match o {
            Sensitivity => self.mouse_sensitivity = v,
            Fov => self.fov = v,
            Gamma => self.gamma = v,
            FramerateLimit => self.limit_framerate = v as i32,
            ChatOpacity => self.chat_opacity = v,
            ChatHeightFocused => self.chat_height_focused = v,
            ChatHeightUnfocused => self.chat_height_unfocused = v,
            ChatWidth => self.chat_width = v,
            ChatScale => self.chat_scale = v,
            AnisotropicFiltering => self.anisotropic_filtering = v as i32,
            MipmapLevels => self.mipmap_levels = v as i32,
            RenderDistance => self.render_distance = v as i32,
            _ => {}
        }
    }

    /// `getOptionOrdinalValue`
    pub fn get_bool(&self, o: Opt) -> bool {
        use Opt::*;
        match o {
            InvertMouse => self.invert_mouse,
            ViewBobbing => self.view_bobbing,
            Anaglyph => self.anaglyph,
            AdvancedOpengl => self.advanced_opengl,
            FboEnable => self.fbo_enable,
            RenderClouds => self.clouds,
            ChatColor => self.chat_colours,
            ChatLinks => self.chat_links,
            ChatLinksPrompt => self.chat_links_prompt,
            SnooperEnabled => self.snooper_enabled,
            UseFullscreen => self.full_screen,
            EnableVsync => self.enable_vsync,
            ShowCape => self.show_cape,
            Touchscreen => self.touchscreen,
            ForceUnicodeFont => self.force_unicode_font,
            _ => false,
        }
    }

    /// `setOptionValue(option, 1)`, then `saveOptions`.
    pub fn set_option(&mut self, o: Opt, d: i32) {
        use Opt::*;
        match o {
            InvertMouse => self.invert_mouse = !self.invert_mouse,
            GuiScale => self.gui_scale = (self.gui_scale + d) & 3,
            Particles => self.particle_setting = (self.particle_setting + d) % 3,
            ViewBobbing => self.view_bobbing = !self.view_bobbing,
            RenderClouds => self.clouds = !self.clouds,
            ForceUnicodeFont => self.force_unicode_font = !self.force_unicode_font,
            AdvancedOpengl => self.advanced_opengl = !self.advanced_opengl,
            FboEnable => self.fbo_enable = !self.fbo_enable,
            Anaglyph => self.anaglyph = !self.anaglyph,
            Difficulty => self.difficulty = (self.difficulty + d) & 3,
            Graphics => self.fancy_graphics = !self.fancy_graphics,
            AmbientOcclusion => self.ambient_occlusion = (self.ambient_occlusion + d) % 3,
            ChatVisibility => self.chat_visibility = (self.chat_visibility + d) % 3,
            ChatColor => self.chat_colours = !self.chat_colours,
            ChatLinks => self.chat_links = !self.chat_links,
            ChatLinksPrompt => self.chat_links_prompt = !self.chat_links_prompt,
            SnooperEnabled => self.snooper_enabled = !self.snooper_enabled,
            ShowCape => self.show_cape = !self.show_cape,
            Touchscreen => self.touchscreen = !self.touchscreen,
            UseFullscreen => self.full_screen = !self.full_screen,
            EnableVsync => self.enable_vsync = !self.enable_vsync,
            _ => {}
        }
        self.save();
    }

    /// `getKeyBinding(option)`: the button's text.
    pub fn label(&self, o: Opt, lang: &Lang) -> String {
        use Opt::*;
        let tr = |k: &str| lang.get(k).to_string();
        let head = format!("{}: ", tr(o.meta().0));
        if o.is_float() {
            let v = self.get_float(o);
            let n = o.normalize(v);
            return match o {
                Sensitivity if n == 0.0 => head + &tr("options.sensitivity.min"),
                Sensitivity if n == 1.0 => head + &tr("options.sensitivity.max"),
                Sensitivity => format!("{head}{}%", (n * 200.0) as i32),
                Fov if v == 70.0 => head + &tr("options.fov.min"),
                Fov if v == 110.0 => head + &tr("options.fov.max"),
                Fov => format!("{head}{}", v as i32),
                FramerateLimit if v == o.meta().4 => head + &tr("options.framerateLimit.max"),
                FramerateLimit => format!("{head}{} fps", v as i32),
                Gamma if n == 0.0 => head + &tr("options.gamma.min"),
                Gamma if n == 1.0 => head + &tr("options.gamma.max"),
                Gamma => format!("{head}+{}%", (n * 100.0) as i32),
                Saturation => format!("{head}{}%", (n * 400.0) as i32),
                ChatOpacity => format!("{head}{}%", (n * 90.0 + 10.0) as i32),
                ChatHeightUnfocused | ChatHeightFocused => {
                    // GuiNewChat.calculateChatboxHeight
                    let px = crate::util::math::floor_float(n * 160.0 + 20.0);
                    format!("{head}{px}px")
                }
                ChatWidth => {
                    let px = crate::util::math::floor_float(n * 280.0 + 40.0);
                    format!("{head}{px}px")
                }
                RenderDistance => format!("{head}{} chunks", v as i32),
                AnisotropicFiltering if v == 1.0 => head + &tr("options.off"),
                AnisotropicFiltering => format!("{head}{}", v as i32),
                MipmapLevels if v == 0.0 => head + &tr("options.off"),
                MipmapLevels => format!("{head}{}", v as i32),
                _ if n == 0.0 => head + &tr("options.off"),
                _ => format!("{head}{}%", (n * 100.0) as i32),
            };
        }
        if o.is_bool() {
            return head
                + &tr(if self.get_bool(o) {
                    "options.on"
                } else {
                    "options.off"
                });
        }
        let pick = |a: &[&str], i: i32| {
            let i = if i < 0 || i as usize >= a.len() {
                0
            } else {
                i as usize
            };
            tr(a[i])
        };
        match o {
            Difficulty => head + &tr(DIFFICULTY_KEYS[self.difficulty as usize & 3]),
            GuiScale => head + &pick(&GUISCALES, self.gui_scale),
            ChatVisibility => head + &pick(&CHAT_VISIBILITY, self.chat_visibility),
            Particles => head + &pick(&PARTICLES, self.particle_setting),
            AmbientOcclusion => head + &pick(&AMBIENT_OCCLUSIONS, self.ambient_occlusion),
            Graphics => {
                head + &tr(if self.fancy_graphics {
                    "options.graphics.fancy"
                } else {
                    "options.graphics.fast"
                })
            }
            _ => head,
        }
    }
}

pub const GUISCALES: [&str; 4] = [
    "options.guiScale.auto",
    "options.guiScale.small",
    "options.guiScale.normal",
    "options.guiScale.large",
];
pub const PARTICLES: [&str; 3] = [
    "options.particles.all",
    "options.particles.decreased",
    "options.particles.minimal",
];
pub const AMBIENT_OCCLUSIONS: [&str; 3] = ["options.ao.off", "options.ao.min", "options.ao.max"];
pub const CHAT_VISIBILITY: [&str; 3] = [
    "options.chat.visibility.full",
    "options.chat.visibility.system",
    "options.chat.visibility.hidden",
];
/// `EnumDifficulty.getDifficultyResourceKey`
pub const DIFFICULTY_KEYS: [&str; 4] = [
    "options.difficulty.peaceful",
    "options.difficulty.easy",
    "options.difficulty.normal",
    "options.difficulty.hard",
];

/// `Keyboard.getKeyName` for the LWJGL key codes.
pub fn lwjgl_key_name(code: i32) -> &'static str {
    match code {
        1 => "ESCAPE",
        2 => "1",
        3 => "2",
        4 => "3",
        5 => "4",
        6 => "5",
        7 => "6",
        8 => "7",
        9 => "8",
        10 => "9",
        11 => "0",
        12 => "MINUS",
        13 => "EQUALS",
        14 => "BACK",
        15 => "TAB",
        16 => "Q",
        17 => "W",
        18 => "E",
        19 => "R",
        20 => "T",
        21 => "Y",
        22 => "U",
        23 => "I",
        24 => "O",
        25 => "P",
        26 => "LBRACKET",
        27 => "RBRACKET",
        28 => "RETURN",
        29 => "LCONTROL",
        30 => "A",
        31 => "S",
        32 => "D",
        33 => "F",
        34 => "G",
        35 => "H",
        36 => "J",
        37 => "K",
        38 => "L",
        39 => "SEMICOLON",
        40 => "APOSTROPHE",
        41 => "GRAVE",
        42 => "LSHIFT",
        43 => "BACKSLASH",
        44 => "Z",
        45 => "X",
        46 => "C",
        47 => "V",
        48 => "B",
        49 => "N",
        50 => "M",
        51 => "COMMA",
        52 => "PERIOD",
        53 => "SLASH",
        54 => "RSHIFT",
        55 => "MULTIPLY",
        56 => "LMENU",
        57 => "SPACE",
        58 => "CAPITAL",
        59 => "F1",
        60 => "F2",
        61 => "F3",
        62 => "F4",
        63 => "F5",
        64 => "F6",
        65 => "F7",
        66 => "F8",
        67 => "F9",
        68 => "F10",
        69 => "NUMLOCK",
        70 => "SCROLL",
        71 => "NUMPAD7",
        72 => "NUMPAD8",
        73 => "NUMPAD9",
        74 => "SUBTRACT",
        75 => "NUMPAD4",
        76 => "NUMPAD5",
        77 => "NUMPAD6",
        78 => "ADD",
        79 => "NUMPAD1",
        80 => "NUMPAD2",
        81 => "NUMPAD3",
        82 => "NUMPAD0",
        83 => "DECIMAL",
        87 => "F11",
        88 => "F12",
        156 => "NUMPADENTER",
        157 => "RCONTROL",
        181 => "DIVIDE",
        184 => "RMENU",
        197 => "PAUSE",
        199 => "HOME",
        200 => "UP",
        201 => "PRIOR",
        203 => "LEFT",
        205 => "RIGHT",
        207 => "END",
        208 => "DOWN",
        209 => "NEXT",
        210 => "INSERT",
        211 => "DELETE",
        219 => "LMETA",
        220 => "RMETA",
        _ => "NONE",
    }
}

/// `GameSettings.getKeyDisplayString`
pub fn key_display(code: i32, lang: &Lang) -> String {
    if code < 0 {
        lang.format("key.mouseButton", &[(code + 101).to_string()])
    } else {
        lwjgl_key_name(code).to_string()
    }
}

/// winit's physical keys as LWJGL key codes.
pub fn lwjgl_code(k: winit::keyboard::KeyCode) -> i32 {
    use winit::keyboard::KeyCode::*;
    match k {
        Escape => 1,
        Digit1 => 2,
        Digit2 => 3,
        Digit3 => 4,
        Digit4 => 5,
        Digit5 => 6,
        Digit6 => 7,
        Digit7 => 8,
        Digit8 => 9,
        Digit9 => 10,
        Digit0 => 11,
        Minus => 12,
        Equal => 13,
        Backspace => 14,
        Tab => 15,
        KeyQ => 16,
        KeyW => 17,
        KeyE => 18,
        KeyR => 19,
        KeyT => 20,
        KeyY => 21,
        KeyU => 22,
        KeyI => 23,
        KeyO => 24,
        KeyP => 25,
        BracketLeft => 26,
        BracketRight => 27,
        Enter => 28,
        ControlLeft => 29,
        KeyA => 30,
        KeyS => 31,
        KeyD => 32,
        KeyF => 33,
        KeyG => 34,
        KeyH => 35,
        KeyJ => 36,
        KeyK => 37,
        KeyL => 38,
        Semicolon => 39,
        Quote => 40,
        Backquote => 41,
        ShiftLeft => 42,
        Backslash => 43,
        KeyZ => 44,
        KeyX => 45,
        KeyC => 46,
        KeyV => 47,
        KeyB => 48,
        KeyN => 49,
        KeyM => 50,
        Comma => 51,
        Period => 52,
        Slash => 53,
        ShiftRight => 54,
        NumpadMultiply => 55,
        AltLeft => 56,
        Space => 57,
        CapsLock => 58,
        F1 => 59,
        F2 => 60,
        F3 => 61,
        F4 => 62,
        F5 => 63,
        F6 => 64,
        F7 => 65,
        F8 => 66,
        F9 => 67,
        F10 => 68,
        NumLock => 69,
        ScrollLock => 70,
        Numpad7 => 71,
        Numpad8 => 72,
        Numpad9 => 73,
        NumpadSubtract => 74,
        Numpad4 => 75,
        Numpad5 => 76,
        Numpad6 => 77,
        NumpadAdd => 78,
        Numpad1 => 79,
        Numpad2 => 80,
        Numpad3 => 81,
        Numpad0 => 82,
        NumpadDecimal => 83,
        F11 => 87,
        F12 => 88,
        NumpadEnter => 156,
        ControlRight => 157,
        NumpadDivide => 181,
        AltRight => 184,
        Pause => 197,
        Home => 199,
        ArrowUp => 200,
        PageUp => 201,
        ArrowLeft => 203,
        ArrowRight => 205,
        End => 207,
        ArrowDown => 208,
        PageDown => 209,
        Insert => 210,
        Delete => 211,
        SuperLeft => 219,
        SuperRight => 220,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_round_trip() {
        let dir = std::env::temp_dir().join(format!("minerust-opts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = GameSettings::load(&dir);
        s.fov = 90.0;
        s.gui_scale = 2;
        s.keys[2].code = 200;
        s.language = "zh_TW".into();
        s.save();
        let t = GameSettings::load(&dir);
        assert_eq!(t.fov, 90.0);
        assert_eq!(t.gui_scale, 2);
        assert_eq!(t.key("key.forward"), 200);
        assert_eq!(t.language, "zh_TW");
        let text = std::fs::read_to_string(dir.join("options.txt")).unwrap();
        assert!(text.starts_with("invertYMouse:false\nmouseSensitivity:0.5\nfov:0.5\n"));
        assert!(text.contains("chatHeightUnfocused:0.44366196\n"));
    }
}
