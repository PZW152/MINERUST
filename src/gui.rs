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
        let meta = if crate::item::tool(s.id).is_some() {
            0
        } else {
            s.meta
        };
        if let Some(&t) = self.atlas.items.get(&(s.id, meta)) {
            let t = self.atlas.rects[t as usize];
            self.quad(
                [[x, y], [x, y + 16.0], [x + 16.0, y + 16.0], [x + 16.0, y]],
                [[t[0], t[1]], [t[0], t[3]], [t[2], t[3]], [t[2], t[1]]],
                [255; 4],
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
    pub fn text_width(&self, s: &str) -> f32 {
        s.chars().map(|c| self.char_width(c)).sum()
    }

    fn char_width(&self, c: char) -> f32 {
        let i = c as u32;
        if (32..127).contains(&i) {
            self.atlas.char_width[i as usize] as f32
        } else {
            self.atlas.char_width[b'?' as usize] as f32
        }
    }

    fn glyphs(&mut self, s: &str, mut x: f32, y: f32, color: [u8; 4]) {
        let sz = UI_SIZE as f32;
        for c in s.chars() {
            let mut i = c as u32;
            if !(32..127).contains(&i) {
                i = b'?' as u32;
            }
            let w = self.atlas.char_width[i as usize] as f32;
            if i != 32 {
                let (cu, cv) = ((i % 16 * 8) as f32, (i / 16 * 8) as f32 + 256.0);
                let gw = w - 1.01;
                let (u0, v0, u1, v1) = (cu / sz, cv / sz, (cu + gw) / sz, (cv + 7.99) / sz);
                self.quad(
                    [[x, y], [x, y + 7.99], [x + gw, y + 7.99], [x + gw, y]],
                    [[u0, v0], [u0, v1], [u1, v1], [u1, v0]],
                    color,
                    SHEET,
                );
            }
            x += w;
        }
    }

    /// `FontRenderer.drawString`, optionally with the drop shadow.
    pub fn text(&mut self, s: &str, x: f32, y: f32, color: u32, shadow: bool) {
        if shadow {
            self.glyphs(s, x + 1.0, y + 1.0, rgba((color & 0xFCFCFC) >> 2, 1.0));
        }
        self.glyphs(s, x, y, rgba(color, 1.0));
    }

    /// `RenderItem.renderItemIntoGUI` for a block item in a 16x16 slot.
    pub fn block_icon(&mut self, id: u8, meta: i32, x: f32, y: f32) {
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
