//! In-game GUI drawing: `FontRenderer`, `Gui.drawTexturedModalRect`,
//! `RenderItem.renderItemIntoGUI` and the `GuiIngame` layout.

use crate::block::{self, Shape};
use crate::render::atlas::{Atlas, UI_SIZE};
use crate::render::mesher::Vertex;
use crate::render::{self, Mat4};

/// Texture selector stored in `Vertex::light[0]`.
const BLOCKS: u8 = 0;
const SHEET: u8 = 1;
const PLAIN: u8 = 2;

pub struct Ui<'a> {
    pub v: Vec<Vertex>,
    atlas: &'a Atlas,
    /// Scaled GUI size (`ScaledResolution`).
    pub w: f32,
    pub h: f32,
}

fn rgba(c: u32, a: f32) -> [u8; 4] {
    [
        (c >> 16 & 255) as u8,
        (c >> 8 & 255) as u8,
        (c & 255) as u8,
        (a * 255.0) as u8,
    ]
}

impl<'a> Ui<'a> {
    pub fn new(atlas: &'a Atlas, w: f32, h: f32) -> Ui<'a> {
        Ui {
            v: Vec::new(),
            atlas,
            w,
            h,
        }
    }

    fn quad(&mut self, p: [[f32; 2]; 4], uv: [[f32; 2]; 4], color: [u8; 4], sel: u8) {
        let vert = |i: usize| Vertex {
            pos: [p[i][0], p[i][1], 0.0],
            uv: uv[i],
            color,
            light: [sel, 0, 0, 0],
        };
        self.v
            .extend_from_slice(&[vert(0), vert(1), vert(2), vert(0), vert(2), vert(3)]);
    }

    /// A block-atlas region (`uv` = u0, v0, u1, v1) drawn into a GUI rectangle.
    pub fn atlas_rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        uv: [f32; 4],
        color: u32,
        alpha: f32,
    ) {
        self.quad(
            [[x, y], [x, y + h], [x + w, y + h], [x + w, y]],
            [
                [uv[0], uv[1]],
                [uv[0], uv[3]],
                [uv[2], uv[3]],
                [uv[2], uv[1]],
            ],
            rgba(color, alpha),
            BLOCKS,
        );
    }

    /// `Gui.drawRect`
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: u32, alpha: f32) {
        self.quad(
            [[x, y], [x, y + h], [x + w, y + h], [x + w, y]],
            [[0.0; 2]; 4],
            rgba(color, alpha),
            PLAIN,
        );
    }

    /// `Gui.drawGradientRect`: vertical blend of two ARGB colours.
    pub fn gradient_rect(&mut self, x: f32, y: f32, w: f32, h: f32, top: u32, bottom: u32) {
        let c = |v: u32| {
            [
                (v >> 16 & 255) as u8,
                (v >> 8 & 255) as u8,
                (v & 255) as u8,
                (v >> 24 & 255) as u8,
            ]
        };
        let (t, b) = (c(top), c(bottom));
        let vert = |p: [f32; 2], col: [u8; 4]| Vertex {
            pos: [p[0], p[1], 0.0],
            uv: [0.0; 2],
            color: col,
            light: [PLAIN, 0, 0, 0],
        };
        let q = [
            vert([x, y], t),
            vert([x, y + h], b),
            vert([x + w, y + h], b),
            vert([x + w, y], t),
        ];
        self.v
            .extend_from_slice(&[q[0], q[1], q[2], q[0], q[2], q[3]]);
    }

    /// `GuiInventory.func_147046_a`: the player model at `(x, y)`.
    pub fn player_model(&mut self, x: f32, y: f32, scale: f32, dx: f32, dy: f32, age: f32) {
        let q = render::model::player_gui(self.atlas.skin_tex(), x, y, scale, dx, dy, age);
        for c in q.chunks(4) {
            let mut c = [c[0], c[1], c[2], c[3]];
            for v in &mut c {
                v.light = [BLOCKS, 0, 0, 0];
            }
            self.v
                .extend_from_slice(&[c[0], c[1], c[2], c[0], c[2], c[3]]);
        }
    }

    /// A 16x16 icon from the block/item atlas by texture name
    /// (`drawTexturedModelRectFromIcon`).
    pub fn named_icon(&mut self, name: &str, x: f32, y: f32) {
        let t = self.atlas.named(name);
        self.quad(
            [[x, y], [x, y + 16.0], [x + 16.0, y + 16.0], [x + 16.0, y]],
            [[t[0], t[1]], [t[0], t[3]], [t[2], t[3]], [t[2], t[1]]],
            [255; 4],
            BLOCKS,
        );
    }

    /// `drawTexturedModalRect` from the GUI sheet; `(u, v)` in sheet pixels.
    pub fn sheet(&mut self, x: f32, y: f32, u: f32, v: f32, w: f32, h: f32) {
        let s = UI_SIZE as f32;
        let (u0, v0, u1, v1) = (u / s, v / s, (u + w) / s, (v + h) / s);
        self.quad(
            [[x, y], [x, y + h], [x + w, y + h], [x + w, y]],
            [[u0, v0], [u0, v1], [u1, v1], [u1, v0]],
            [255; 4],
            SHEET,
        );
    }

    /// A quad sampling the GUI sheet with linear filtering (`GL_LINEAR`).
    pub fn linear_quad(&mut self, p: [[f32; 2]; 4], uv: [[f32; 2]; 4]) {
        self.quad(p, uv, [255; 4], 3);
    }

    /// Sheet region scaled to `w` x `h`, tinted, with the alpha going
    /// from `a0` at the top to `a1` at the bottom.
    #[allow(clippy::too_many_arguments)]
    pub fn sheet_gradient(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        u: f32,
        v: f32,
        uw: f32,
        vh: f32,
        tint: u32,
        a0: f32,
        a1: f32,
    ) {
        let s = UI_SIZE as f32;
        let (u0, v0, u1, v1) = (u / s, v / s, (u + uw) / s, (v + vh) / s);
        let top = rgba(tint, a0);
        let bot = rgba(tint, a1);
        let vert = |p: [f32; 2], t: [f32; 2], c: [u8; 4]| Vertex {
            pos: [p[0], p[1], 0.0],
            uv: t,
            color: c,
            light: [SHEET, 0, 0, 0],
        };
        let q = [
            vert([x, y], [u0, v0], top),
            vert([x, y + h], [u0, v1], bot),
            vert([x + w, y + h], [u1, v1], bot),
            vert([x + w, y], [u1, v0], top),
        ];
        self.v
            .extend_from_slice(&[q[0], q[1], q[2], q[0], q[2], q[3]]);
    }

    /// Sheet region scaled to `w` x `h`, tinted.
    pub fn sheet_tinted(
        &mut self,
        x: f32,
        y: f32,
        u: f32,
        v: f32,
        w: f32,
        h: f32,
        uw: f32,
        vh: f32,
        tint: u32,
    ) {
        let s = UI_SIZE as f32;
        let (u0, v0, u1, v1) = (u / s, v / s, (u + uw) / s, (v + vh) / s);
        self.quad(
            [[x, y], [x, y + h], [x + w, y + h], [x + w, y]],
            [[u0, v0], [u0, v1], [u1, v1], [u1, v0]],
            rgba(tint, 1.0),
            SHEET,
        );
    }

    /// Text drawn at an integer scale (`glScalef` around the origin).
    pub fn text_scaled(&mut self, s: &str, x: f32, y: f32, scale: f32, color: u32) {
        let start = self.v.len();
        self.text(s, x, y, color, true);
        for v in &mut self.v[start..] {
            v.pos[0] *= scale;
            v.pos[1] *= scale;
        }
    }

    /// Any stack: block icons in 3D, items as flat sprites.
    pub fn stack_icon(&mut self, s: &crate::item::Stack, x: f32, y: f32) {
        if let Some(b) = s.block() {
            if crate::block::info(b).shape != crate::block::Shape::None
                || b == crate::block::FLOWING_WATER
            {
                self.block_icon(b, s.meta, x, y);
                return;
            }
        }
        // RenderItem.renderItemIntoGUI: one quad per render pass
        for (t, c) in self.atlas.item_passes(s.id, s.meta) {
            let col = [
                (c[0] * 255.0) as u8,
                (c[1] * 255.0) as u8,
                (c[2] * 255.0) as u8,
                255,
            ];
            self.quad(
                [[x, y], [x, y + 16.0], [x + 16.0, y + 16.0], [x + 16.0, y]],
                [[t[0], t[1]], [t[0], t[3]], [t[2], t[3]], [t[2], t[1]]],
                col,
                BLOCKS,
            );
        }
    }

    /// `widgets.png` region.
    pub fn widget(&mut self, x: f32, y: f32, u: f32, v: f32, w: f32, h: f32) {
        self.sheet(x, y, u, v, w, h);
    }

    /// `icons.png` region.
    pub fn icon(&mut self, x: f32, y: f32, u: f32, v: f32, w: f32, h: f32) {
        self.sheet(x, y, u + 256.0, v, w, h);
    }

    /// `FontRenderer.getStringWidth`
    /// `RenderItem.renderItemIntoGUI` for a block item in a 16x16 slot.
    pub fn block_icon(&mut self, id: u8, meta: i32, x: f32, y: f32) {
        if id == block::CHEST {
            // renderItemIntoGUI's 3D block place, then type 22
            let mut m: Mat4 = render::translate(x - 2.0, y + 3.0, 0.0);
            m = render::mul(&m, &scale(10.0, 10.0, 10.0));
            m = render::mul(&m, &render::translate(1.0, 0.5, 1.0));
            m = render::mul(&m, &scale(1.0, 1.0, -1.0));
            m = render::mul(&m, &render::rot_x(210.0));
            m = render::mul(&m, &render::rot_y(45.0));
            m = render::mul(&m, &render::rot_y(-90.0));
            if let Some(t) = self
                .atlas
                .mob_tex(render::chest::texture(false, false, christmas()))
            {
                let q = render::chest::gui_icon(t, &m);
                for c in q.chunks(4) {
                    let mut c = [c[0], c[1], c[2], c[3]];
                    for v in &mut c {
                        v.light = [BLOCKS, 0, 0, 0];
                    }
                    self.v
                        .extend_from_slice(&[c[0], c[1], c[2], c[0], c[2], c[3]]);
                }
            }
            return;
        }
        let info = block::info(id);
        let cube = matches!(
            info.shape,
            Shape::Cube | Shape::Cactus | Shape::Chest | Shape::Slab
        );
        if !cube {
            // Flat sprite (`renderIcon`).
            let face = if id == block::DOUBLE_PLANT { 1 } else { 0 };
            if let Some(t) = self.atlas.tile(id, meta as u8, face) {
                let tint = if id == block::TALLGRASS || id == block::VINE || id == block::WATERLILY
                {
                    0x48B518
                } else {
                    0xFFFFFF
                };
                self.quad(
                    [[x, y], [x, y + 16.0], [x + 16.0, y + 16.0], [x + 16.0, y]],
                    [[t[0], t[1]], [t[0], t[3]], [t[2], t[3]], [t[2], t[1]]],
                    rgba(tint, 1.0),
                    BLOCKS,
                );
            }
            return;
        }
        // The fixed inventory view of a block.
        let mut m: Mat4 = render::translate(x - 2.0, y + 3.0, 0.0);
        m = render::mul(&m, &scale(10.0, 10.0, 10.0));
        m = render::mul(&m, &render::translate(1.0, 0.5, 1.0));
        m = render::mul(&m, &scale(1.0, 1.0, -1.0));
        m = render::mul(&m, &render::rot_x(210.0));
        m = render::mul(&m, &render::rot_y(45.0));
        m = render::mul(&m, &render::rot_y(90.0));
        m = render::mul(&m, &render::translate(-0.5, -0.5, -0.5));
        let top = if info.shape == Shape::Slab { 0.5 } else { 1.0 };
        let project = |p: [f32; 3]| {
            let o = render::transform(&m, [p[0], p[1] * top, p[2], 1.0]);
            [o[0], o[1], o[2]]
        };
        // Corners per face (same order as the world mesher) with tile UVs.
        const FACES: [(usize, [[f32; 3]; 4], [[f32; 2]; 4]); 6] = [
            (
                0,
                [
                    [0.0, 0.0, 1.0],
                    [0.0, 0.0, 0.0],
                    [1.0, 0.0, 0.0],
                    [1.0, 0.0, 1.0],
                ],
                [[0.0, 1.0], [0.0, 0.0], [1.0, 0.0], [1.0, 1.0]],
            ),
            (
                1,
                [
                    [1.0, 1.0, 1.0],
                    [1.0, 1.0, 0.0],
                    [0.0, 1.0, 0.0],
                    [0.0, 1.0, 1.0],
                ],
                [[1.0, 1.0], [1.0, 0.0], [0.0, 0.0], [0.0, 1.0]],
            ),
            (
                2,
                [
                    [0.0, 1.0, 0.0],
                    [1.0, 1.0, 0.0],
                    [1.0, 0.0, 0.0],
                    [0.0, 0.0, 0.0],
                ],
                [[1.0, 0.0], [0.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
            ),
            (
                3,
                [
                    [0.0, 1.0, 1.0],
                    [0.0, 0.0, 1.0],
                    [1.0, 0.0, 1.0],
                    [1.0, 1.0, 1.0],
                ],
                [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
            ),
            (
                4,
                [
                    [0.0, 1.0, 1.0],
                    [0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0],
                ],
                [[1.0, 0.0], [0.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
            ),
            (
                5,
                [
                    [1.0, 0.0, 1.0],
                    [1.0, 0.0, 0.0],
                    [1.0, 1.0, 0.0],
                    [1.0, 1.0, 1.0],
                ],
                [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
            ),
        ];
        // The three faces nearest to the viewer.
        let mut order: Vec<(f32, usize)> = FACES
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let z: f32 = f.1.iter().map(|&p| project(p)[2]).sum();
                (z, i)
            })
            .collect();
        order.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        let centre_x = x + 8.0;
        let nearest: Vec<usize> = order.iter().take(3).map(|o| o.1).collect();
        for &fi in nearest.iter().rev() {
            let (face, corners, uvs) = &FACES[fi];
            let Some(t) = self.atlas.tile(id, meta as u8, *face) else {
                continue;
            };
            let p: Vec<[f32; 3]> = corners.iter().map(|&c| project(c)).collect();
            let cx: f32 = p.iter().map(|q| q[0]).sum::<f32>() / 4.0;
            let shade = if *face == 1 {
                1.0
            } else if cx < centre_x {
                0.8
            } else {
                0.6
            };
            let tint = match id {
                block::GRASS if *face == 1 => 0x7CBD6B,
                block::LEAVES | block::LEAVES2 => 0x48B518,
                _ => 0xFFFFFF,
            };
            let c = rgba(tint, 1.0);
            let color = [
                (c[0] as f32 * shade) as u8,
                (c[1] as f32 * shade) as u8,
                (c[2] as f32 * shade) as u8,
                255,
            ];
            let uv: Vec<[f32; 2]> = uvs
                .iter()
                .map(|u| {
                    [
                        t[0] + (t[2] - t[0]) * u[0],
                        t[1] + (t[3] - t[1])
                            * (if info.shape == Shape::Slab && *face >= 2 {
                                0.5 + u[1] * 0.5
                            } else {
                                u[1]
                            }),
                    ]
                })
                .collect();
            self.quad(
                [
                    [p[0][0], p[0][1]],
                    [p[1][0], p[1][1]],
                    [p[2][0], p[2][1]],
                    [p[3][0], p[3][1]],
                ],
                [uv[0], uv[1], uv[2], uv[3]],
                color,
                BLOCKS,
            );
        }
    }
}

fn scale(x: f32, y: f32, z: f32) -> Mat4 {
    let mut m = render::IDENTITY;
    m[0] = x;
    m[5] = y;
    m[10] = z;
    m
}

/// One inventory slot: block id, metadata, count.
pub type Stack = (u8, i32, u32);

/// `GuiIngame.renderHotbar`-style hotbar with the selected slot highlighted.
pub fn hotbar(ui: &mut Ui, slots: &[Option<Stack>; 9], selected: usize, show_counts: bool) {
    let (w, h) = (ui.w, ui.h);
    ui.widget(w / 2.0 - 91.0, h - 22.0, 0.0, 0.0, 182.0, 22.0);
    ui.widget(
        w / 2.0 - 91.0 - 1.0 + selected as f32 * 20.0,
        h - 22.0 - 1.0,
        0.0,
        22.0,
        24.0,
        22.0,
    );
    for (i, s) in slots.iter().enumerate() {
        if let Some((id, meta, count)) = *s {
            let (x, y) = (w / 2.0 - 90.0 + i as f32 * 20.0 + 2.0, h - 16.0 - 3.0);
            ui.block_icon(id, meta, x, y);
            if show_counts && count > 1 {
                let t = count.to_string();
                let tw = ui.text_width(&t);
                ui.text(&t, x + 19.0 - 2.0 - tw, y + 6.0 + 3.0, 0xFFFFFF, true);
            }
        }
    }
}

/// The F3 overlay lines, top-left and top-right.
pub fn debug_overlay(ui: &mut Ui, left: &[String], right: &[String]) {
    for (i, l) in left.iter().enumerate() {
        ui.text(l, 2.0, 2.0 + i as f32 * 10.0, 0xFFFFFF, true);
    }
    for (i, l) in right.iter().enumerate() {
        let w = ui.text_width(l);
        ui.text(l, ui.w - w - 2.0, 2.0 + i as f32 * 10.0, 0xE0E0E0, true);
    }
}

/// `java.awt.Color.HSBtoRGB`
pub fn hsb_to_rgb(hue: f32, saturation: f32, brightness: f32) -> u32 {
    let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
    if saturation == 0.0 {
        let v = (brightness * 255.0 + 0.5) as u32;
        r = v;
        g = v;
        b = v;
    } else {
        let h = (hue - hue.floor()) * 6.0;
        let f = h - h.floor();
        let p = brightness * (1.0 - saturation);
        let q = brightness * (1.0 - saturation * f);
        let t = brightness * (1.0 - saturation * (1.0 - f));
        let c = |v: f32| (v * 255.0 + 0.5) as u32;
        match h as i32 {
            0 => (r, g, b) = (c(brightness), c(t), c(p)),
            1 => (r, g, b) = (c(q), c(brightness), c(p)),
            2 => (r, g, b) = (c(p), c(brightness), c(t)),
            3 => (r, g, b) = (c(p), c(q), c(brightness)),
            4 => (r, g, b) = (c(t), c(p), c(brightness)),
            5 => (r, g, b) = (c(brightness), c(p), c(q)),
            _ => {}
        }
    }
    0xFF000000 | (r << 16) | (g << 8) | b
}

/// `FontRenderer`'s table of the characters in `font/ascii.png`.
pub const ASCII_CHARS: &str = "\u{c0}\u{c1}\u{c2}\u{c8}\u{ca}\u{cb}\u{cd}\u{d3}\u{d4}\u{d5}\u{da}\u{df}\u{e3}\u{f5}\u{11f}\u{130}\u{131}\u{152}\u{153}\u{15e}\u{15f}\u{174}\u{175}\u{17e}\u{207}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0}\u{0} !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~\u{0}\u{c7}\u{fc}\u{e9}\u{e2}\u{e4}\u{e0}\u{e5}\u{e7}\u{ea}\u{eb}\u{e8}\u{ef}\u{ee}\u{ec}\u{c4}\u{c5}\u{c9}\u{e6}\u{c6}\u{f4}\u{f6}\u{f2}\u{fb}\u{f9}\u{ff}\u{d6}\u{dc}\u{f8}\u{a3}\u{d8}\u{d7}\u{192}\u{e1}\u{ed}\u{f3}\u{fa}\u{f1}\u{d1}\u{aa}\u{ba}\u{bf}\u{ae}\u{ac}\u{bd}\u{bc}\u{a1}\u{ab}\u{bb}\u{2591}\u{2592}\u{2593}\u{2502}\u{2524}\u{2561}\u{2562}\u{2556}\u{2555}\u{2563}\u{2551}\u{2557}\u{255d}\u{255c}\u{255b}\u{2510}\u{2514}\u{2534}\u{252c}\u{251c}\u{2500}\u{253c}\u{255e}\u{255f}\u{255a}\u{2554}\u{2569}\u{2566}\u{2560}\u{2550}\u{256c}\u{2567}\u{2568}\u{2564}\u{2565}\u{2559}\u{2558}\u{2552}\u{2553}\u{256b}\u{256a}\u{2518}\u{250c}\u{2588}\u{2584}\u{258c}\u{2590}\u{2580}\u{3b1}\u{3b2}\u{393}\u{3c0}\u{3a3}\u{3c3}\u{3bc}\u{3c4}\u{3a6}\u{398}\u{3a9}\u{3b4}\u{221e}\u{2205}\u{2208}\u{2229}\u{2261}\u{b1}\u{2265}\u{2264}\u{2320}\u{2321}\u{f7}\u{2248}\u{b0}\u{2219}\u{b7}\u{221a}\u{207f}\u{b2}\u{25a0}\u{0}";

/// `String.indexOf` in `ASCII_CHARS`.
pub fn ascii_index(c: char) -> Option<usize> {
    ASCII_CHARS.chars().position(|x| x == c)
}

/// `FontRenderer.colorCode`
pub fn color_code(i: usize) -> u32 {
    let k = (i >> 3 & 1) as u32 * 85;
    let mut r = (i >> 2 & 1) as u32 * 170 + k;
    let mut g = (i >> 1 & 1) as u32 * 170 + k;
    let mut b = (i & 1) as u32 * 170 + k;
    if i == 6 {
        r += 85;
    }
    if i >= 16 {
        r /= 4;
        g /= 4;
        b /= 4;
    }
    (r & 255) << 16 | (g & 255) << 8 | (b & 255)
}

pub const FONT_HEIGHT: f32 = 9.0;

impl<'a> Ui<'a> {
    fn unicode_flag(&self) -> bool {
        self.atlas.glyphs.lock().map_or(false, |g| g.unicode)
    }

    /// `FontRenderer.getCharWidth`
    pub fn char_width(&self, c: char) -> i32 {
        if c == '\u{a7}' {
            return -1;
        }
        if c == ' ' {
            return 4;
        }
        let i = ascii_index(c);
        if c != '\u{0}' && i.is_some() && !self.unicode_flag() {
            return self.atlas.char_width[i.unwrap()] as i32;
        }
        let g = self.atlas.glyphs.lock().unwrap();
        let w = g.sizes.get(c as usize).copied().unwrap_or(0);
        if w != 0 {
            let mut a = (w >> 4) as i32;
            let mut b = (w & 15) as i32;
            if b > 7 {
                b = 15;
                a = 0;
            }
            b += 1;
            (b - a) / 2 + 1
        } else {
            0
        }
    }

    /// `FontRenderer.getStringWidth`
    pub fn text_width(&self, s: &str) -> f32 {
        let mut w = 0;
        let mut bold = false;
        let mut it = s.chars().peekable();
        while let Some(c) = it.next() {
            let mut k = self.char_width(c);
            if k < 0 && it.peek().is_some() {
                let n = it.next().unwrap();
                if n == 'l' || n == 'L' {
                    bold = true;
                } else if n == 'r' || n == 'R' {
                    bold = false;
                }
                k = 0;
            }
            w += k;
            if bold && k > 0 {
                w += 1;
            }
        }
        w as f32
    }

    /// `renderDefaultChar`: one cell of `font/ascii.png`.
    fn default_char(&mut self, i: usize, x: f32, y: f32, italic: bool, color: [u8; 4]) -> f32 {
        let sz = UI_SIZE as f32;
        let (cu, cv) = ((i % 16 * 8) as f32, (i / 16 * 8) as f32 + 256.0);
        let k = if italic { 1.0 } else { 0.0 };
        let w = self.atlas.char_width[i] as f32 - 0.01;
        let (u0, v0, u1, v1) = (cu / sz, cv / sz, (cu + w - 1.0) / sz, (cv + 7.99) / sz);
        self.quad(
            [
                [x + k, y],
                [x - k, y + 7.99],
                [x + w - 1.0 - k, y + 7.99],
                [x + w - 1.0 + k, y],
            ],
            [[u0, v0], [u0, v1], [u1, v1], [u1, v0]],
            color,
            SHEET,
        );
        self.atlas.char_width[i] as f32
    }

    /// `renderUnicodeChar`: half of a 16-pixel glyph cell.
    fn unicode_char(&mut self, c: char, x: f32, y: f32, italic: bool, color: [u8; 4]) -> f32 {
        let (w, cell) = {
            let mut g = self.atlas.glyphs.lock().unwrap();
            let w = g.sizes.get(c as usize).copied().unwrap_or(0);
            if w == 0 {
                return 0.0;
            }
            (w, g.cell(c as u32))
        };
        let a = (w >> 4) as f32;
        let b = (w & 15) as f32 + 1.0;
        if let Some((cu, cv)) = cell {
            let sz = UI_SIZE as f32;
            let u = cu + a;
            let d = b - a - 0.02;
            let k = if italic { 1.0 } else { 0.0 };
            let (u0, v0, u1, v1) = (u / sz, cv / sz, (u + d) / sz, (cv + 15.98) / sz);
            self.quad(
                [
                    [x + k, y],
                    [x - k, y + 7.99],
                    [x + d / 2.0 - k, y + 7.99],
                    [x + d / 2.0 + k, y],
                ],
                [[u0, v0], [u0, v1], [u1, v1], [u1, v0]],
                color,
                SHEET,
            );
        }
        (b - a) / 2.0 + 1.0
    }

    /// `renderCharAtPos`
    fn char_at(
        &mut self,
        i: Option<usize>,
        c: char,
        x: f32,
        y: f32,
        italic: bool,
        color: [u8; 4],
    ) -> f32 {
        if c == ' ' {
            return 4.0;
        }
        match i {
            Some(i) if !self.unicode_flag() => self.default_char(i, x, y, italic, color),
            _ => self.unicode_char(c, x, y, italic, color),
        }
    }

    /// `renderString` / `renderStringAtPos` with the formatting codes;
    /// returns the end x.
    fn render_string(
        &mut self,
        s: &str,
        x: f32,
        y: f32,
        color: u32,
        alpha: f32,
        shadow: bool,
    ) -> f32 {
        let mut c = color & 0xFFFFFF;
        if shadow {
            c = (c & 0xFCFCFC) >> 2;
        }
        let base = c;
        let mut cur = rgba(c, alpha);
        let (mut px, mut py) = (x, y);
        let uni = self.unicode_flag();
        let (mut bold, mut italic, mut under, mut strike) = (false, false, false, false);
        let chars: Vec<char> = s.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let ch = chars[i];
            if ch == '\u{a7}' && i + 1 < chars.len() {
                let n = chars[i + 1].to_ascii_lowercase();
                let k = "0123456789abcdefklmnor".find(n).map_or(-1, |k| k as i32);
                if k < 16 {
                    bold = false;
                    italic = false;
                    under = false;
                    strike = false;
                    let mut k = if (0..=15).contains(&k) { k } else { 15 } as usize;
                    if shadow {
                        k += 16;
                    }
                    cur = rgba(color_code(k), alpha);
                } else if k == 17 {
                    bold = true;
                } else if k == 18 {
                    strike = true;
                } else if k == 19 {
                    under = true;
                } else if k == 20 {
                    italic = true;
                } else if k == 21 {
                    bold = false;
                    italic = false;
                    under = false;
                    strike = false;
                    cur = rgba(base, alpha);
                }
                // 16 (obfuscated) picks random glyphs of the same width
                i += 2;
                continue;
            }
            let idx = ascii_index(ch);
            let step = if uni { 0.5 } else { 1.0 };
            let back = (ch == '\u{0}' || idx.is_none() || uni) && shadow;
            if back {
                px -= step;
                py -= step;
            }
            let mut w = self.char_at(idx, ch, px, py, italic, cur);
            if back {
                px += step;
                py += step;
            }
            if bold {
                px += step;
                if back {
                    px -= step;
                    py -= step;
                }
                self.char_at(idx, ch, px, py, italic, cur);
                px -= step;
                if back {
                    px += step;
                    py += step;
                }
                w += 1.0;
            }
            if strike {
                self.solid(px, py + (FONT_HEIGHT / 2.0).floor() - 1.0, w, 1.0, cur);
            }
            if under {
                self.solid(px - 1.0, py + FONT_HEIGHT - 1.0, w + 1.0, 1.0, cur);
            }
            px += (w as i32) as f32;
            i += 1;
        }
        px
    }

    fn solid(&mut self, x: f32, y: f32, w: f32, h: f32, c: [u8; 4]) {
        self.quad(
            [[x, y], [x, y + h], [x + w, y + h], [x + w, y]],
            [[0.0; 2]; 4],
            c,
            PLAIN,
        );
    }

    /// `FontRenderer.drawString`, optionally with the drop shadow.
    pub fn text(&mut self, s: &str, x: f32, y: f32, color: u32, shadow: bool) -> f32 {
        self.text_alpha_shadow(s, x, y, color, 1.0, shadow)
    }

    /// `drawString` with the alpha in the colour's top byte.
    pub fn text_alpha(&mut self, s: &str, x: f32, y: f32, color: u32, alpha: f32) {
        self.text_alpha_shadow(s, x, y, color, alpha, false);
    }

    pub fn text_alpha_shadow(
        &mut self,
        s: &str,
        x: f32,
        y: f32,
        color: u32,
        alpha: f32,
        shadow: bool,
    ) -> f32 {
        if shadow {
            let a = self.render_string(s, x + 1.0, y + 1.0, color, alpha, true);
            let b = self.render_string(s, x, y, color, alpha, false);
            a.max(b)
        } else {
            self.render_string(s, x, y, color, alpha, false)
        }
    }

    /// `drawCenteredString`
    pub fn centered(&mut self, s: &str, x: f32, y: f32, color: u32) {
        let w = self.text_width(s);
        self.text(s, x - (w as i32 / 2) as f32, y, color, true);
    }

    /// `trimStringToWidth`
    pub fn trim_to_width(&self, s: &str, width: f32, reverse: bool) -> String {
        let chars: Vec<char> = s.chars().collect();
        let mut out: Vec<char> = Vec::new();
        let mut w = 0.0;
        let (mut code, mut bold) = (false, false);
        let order: Vec<usize> = if reverse {
            (0..chars.len()).rev().collect()
        } else {
            (0..chars.len()).collect()
        };
        for i in order {
            if w >= width {
                break;
            }
            let c = chars[i];
            let k = self.char_width(c);
            if code {
                code = false;
                if c == 'l' || c == 'L' {
                    bold = true;
                } else if c == 'r' || c == 'R' {
                    bold = false;
                }
            } else if k < 0 {
                code = true;
            } else {
                w += k as f32;
                if bold {
                    w += 1.0;
                }
            }
            if w > width {
                break;
            }
            if reverse {
                out.insert(0, c);
            } else {
                out.push(c);
            }
        }
        out.into_iter().collect()
    }

    /// `listFormattedStringToWidth`
    pub fn wrap(&self, s: &str, width: f32) -> Vec<String> {
        self.wrap_formatted(s, width)
            .split('\n')
            .map(|x| x.to_string())
            .collect()
    }

    fn wrap_formatted(&self, s: &str, width: f32) -> String {
        let chars: Vec<char> = s.chars().collect();
        let n = self.size_to_width(&chars, width);
        if chars.len() <= n {
            return s.to_string();
        }
        let head: String = chars[..n].iter().collect();
        let c = chars[n];
        let skip = c == ' ' || c == '\n';
        let rest: String = chars[n + if skip { 1 } else { 0 }..].iter().collect();
        let next = format_from_string(&head) + &rest;
        head + "\n" + &self.wrap_formatted(&next, width)
    }

    /// `sizeStringToWidth`
    fn size_to_width(&self, s: &[char], width: f32) -> usize {
        let len = s.len();
        let mut w = 0.0;
        let mut i = 0usize;
        let mut space: isize = -1;
        let mut bold = false;
        while i < len {
            let c = s[i];
            match c {
                '\n' => {
                    // var5-- then the newline check below
                }
                '\u{a7}' => {
                    if i < len - 1 {
                        i += 1;
                        let n = s[i];
                        if n == 'l' || n == 'L' {
                            bold = true;
                        } else if n == 'r' || n == 'R' || n.is_ascii_hexdigit() {
                            bold = false;
                        }
                    }
                }
                _ => {
                    if c == ' ' {
                        space = i as isize;
                    }
                    w += self.char_width(c) as f32;
                    if bold {
                        w += 1.0;
                    }
                }
            }
            if c == '\n' {
                return i;
            }
            if w > width {
                break;
            }
            i += 1;
        }
        if i != len && space != -1 && (space as usize) < i {
            space as usize
        } else {
            i
        }
    }
}

/// `getFormatFromString`
fn format_from_string(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    for i in 0..chars.len() {
        if chars[i] == '\u{a7}' && i + 1 < chars.len() {
            let c = chars[i + 1];
            if c.is_ascii_hexdigit() {
                out = format!("\u{a7}{c}");
            } else if ('k'..='o').contains(&c) || ('K'..='O').contains(&c) || c == 'r' || c == 'R' {
                out.push('\u{a7}');
                out.push(c);
            }
        }
    }
    out
}

/// `TileEntityChestRenderer`'s December 24th to 26th check.
pub fn christmas() -> bool {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    // civil date from days since 1970-01-01 (UTC)
    let z = secs.div_euclid(86400) + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    m == 12 && (24..=26).contains(&d)
}
