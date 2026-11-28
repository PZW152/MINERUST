//! `TileEntitySignRenderer` with `ModelSign`, and the sign text drawn by
//! `FontRenderer` from `font/ascii.png` (kept in the block atlas so the 3D
//! passes can sample it) or from the Unicode pages.

use super::atlas::Atlas;
use super::mesher::Vertex;
use super::model::{render, Draw, Part};
use super::{mul, rot_y, transform, translate, Mat4};

pub const TEXTURES: [&str; 1] = ["sign"];

/// `ModelSign`: the board and, for standing signs, the stick.
fn parts(standing: bool) -> Vec<Part> {
    let mut v =
        vec![Part::new([0.0, 0.0, 0.0]).bx(0.0, 0.0, [-12.0, -14.0, -1.0], [24.0, 12.0, 2.0], 0.0)];
    if standing {
        v.push(Part::new([0.0, 0.0, 0.0]).bx(0.0, 14.0, [-1.0, -2.0, -1.0], [2.0, 14.0, 2.0], 0.0));
    }
    v
}

/// `FontRenderer.drawString` (no shadow) in the text plane of `m`: one
/// quad per glyph, from the atlas copy of `ascii.png` or, for characters
/// outside it and whenever the Unicode font is on, from the Unicode glyph
/// cache in the GUI sheet (`fs_terrain` reads U from 2 up there).
pub fn text(
    out: &mut Vec<Vertex>,
    atlas: &Atlas,
    s: &str,
    x: f32,
    y: f32,
    m: &Mat4,
    color: [u8; 4],
    light: [u8; 4],
) {
    let font = atlas.named("font_ascii");
    let (u0, v0) = (font[0], font[1]);
    let (du, dv) = ((font[2] - font[0]) / 128.0, (font[3] - font[1]) / 128.0);
    let uni = crate::gui::unicode_flag(atlas);
    let sheet = super::atlas::UI_SIZE as f32;
    let mut px = x;
    let base = color;
    let mut color = color;
    let quad = |out: &mut Vec<Vertex>, color: [u8; 4], corners: [([f32; 2], [f32; 2]); 4]| {
        for (p, uv) in corners {
            let o = transform(m, [p[0], p[1], 0.0, 1.0]);
            out.push(Vertex {
                pos: [o[0], o[1], o[2]],
                uv,
                color,
                light,
            });
        }
    };
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\u{a7}' {
            // renderStringAtPos: a colour code sets the colour (keeping the
            // alpha), r resets it; the style codes draw as nothing here
            if let Some(n) = chars.next() {
                let k = "0123456789abcdefklmnor".find(n.to_ascii_lowercase());
                match k {
                    Some(k) if k < 16 => {
                        let v = crate::gui::color_code(k);
                        color = [(v >> 16) as u8, (v >> 8) as u8, v as u8, base[3]];
                    }
                    Some(21) => color = base,
                    _ => {}
                }
            }
            continue;
        }
        if c == ' ' {
            px += 4.0;
            continue;
        }
        match crate::gui::ascii_index(c) {
            Some(i) if !uni && c != '\u{0}' => {
                let cw = atlas.char_width[i] as f32;
                let (tx, ty) = ((i % 16 * 8) as f32, (i / 16 * 8) as f32);
                let w = cw - 0.01;
                let t = |a: f32, b: f32| [u0 + a * du, v0 + b * dv];
                // renderDefaultChar's triangle strip as a quad
                quad(
                    out,
                    color,
                    [
                        ([px, y], t(tx, ty)),
                        ([px, y + 7.99], t(tx, ty + 7.99)),
                        ([px + w - 1.0, y + 7.99], t(tx + w - 1.0, ty + 7.99)),
                        ([px + w - 1.0, y], t(tx + w - 1.0, ty)),
                    ],
                );
                px += cw;
            }
            _ => {
                // renderUnicodeChar
                let (w, cell) = {
                    let mut g = atlas.glyphs.lock().unwrap();
                    let w = g.sizes.get(c as usize).copied().unwrap_or(0);
                    if w == 0 {
                        continue;
                    }
                    (w, g.cell(c as u32))
                };
                let a = (w >> 4) as f32;
                let b = (w & 15) as f32 + 1.0;
                if let Some((cu, cv)) = cell {
                    let u = cu + a;
                    let d = b - a - 0.02;
                    let t = |a: f32, b: f32| [2.0 + a / sheet, b / sheet];
                    quad(
                        out,
                        color,
                        [
                            ([px, y], t(u, cv)),
                            ([px, y + 7.99], t(u, cv + 15.98)),
                            ([px + d / 2.0, y + 7.99], t(u + d, cv + 15.98)),
                            ([px + d / 2.0, y], t(u + d, cv)),
                        ],
                    );
                }
                px += (b - a) / 2.0 + 1.0;
            }
        }
    }
}

/// `FontRenderer.getStringWidth`
pub fn width(atlas: &Atlas, s: &str) -> f32 {
    crate::gui::text_width(atlas, s)
}

/// `TileEntitySignRenderer.renderTileEntityAt` with the block corner at
/// `base` (`glTranslatef(x, y, z)` already applied). Returns the board
/// quads (both windings) and the text quads (one winding) separately.
pub fn sign(
    atlas: &Atlas,
    base: &Mat4,
    standing: bool,
    meta: i32,
    lines: &[String; 4],
    edit_line: Option<usize>,
    light: [u8; 4],
) -> (Vec<Vertex>, Vec<Vertex>) {
    let k = 0.666_666_7f32;
    let mut m = mul(base, &translate(0.5, 0.75 * k, 0.5));
    if standing {
        m = mul(&m, &rot_y(-(meta as f32 * 360.0 / 16.0)));
    } else {
        let a = match meta {
            2 => 180.0,
            4 => 90.0,
            5 => -90.0,
            _ => 0.0,
        };
        m = mul(&m, &rot_y(-a));
        m = mul(&m, &translate(0.0, -0.3125, -0.4375));
    }
    let mut board = Vec::new();
    if let Some(tex) = atlas.mob_tex("sign") {
        let mb = mul(&m, &super::entities::scale(k, -k, -k));
        let mut d = Draw {
            out: &mut board,
            tex,
            light,
            tint: [1.0; 4],
        };
        for p in parts(standing) {
            render(&mut d, &mb, &p);
        }
    }
    let s = 0.016_666_668 * k;
    let mut mt = mul(&m, &translate(0.0, 0.5 * k, 0.07 * k));
    mt = mul(&mt, &super::entities::scale(s, -s, s));
    let mut words = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let shown = if edit_line == Some(i) {
            format!("> {l} <")
        } else {
            l.clone()
        };
        let w = width(atlas, &shown);
        text(
            &mut words,
            atlas,
            &shown,
            -((w as i32) / 2) as f32,
            (i as i32 * 10 - 4 * 5) as f32,
            &mt,
            [0, 0, 0, 255],
            light,
        );
    }
    (board, words)
}
