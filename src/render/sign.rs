//! `TileEntitySignRenderer` with `ModelSign`, and the sign text drawn by
//! `FontRenderer` from `font/ascii.png` (kept in the block atlas so the 3D
//! passes can sample it).

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
/// quad per ASCII glyph from the atlas copy of `ascii.png`.
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
    let mut px = x;
    for c in s.chars() {
        if c == ' ' {
            px += 4.0;
            continue;
        }
        let Some(i) = crate::gui::ascii_index(c) else {
            continue;
        };
        let cw = atlas.char_width[i] as f32;
        let (tx, ty) = ((i % 16 * 8) as f32, (i / 16 * 8) as f32);
        let w = cw - 0.01;
        // renderDefaultChar's triangle strip as a quad
        let corners = [
            ([px, y], [tx, ty]),
            ([px, y + 7.99], [tx, ty + 7.99]),
            ([px + w - 1.0, y + 7.99], [tx + w - 1.0, ty + 7.99]),
            ([px + w - 1.0, y], [tx + w - 1.0, ty]),
        ];
        for (p, t) in corners {
            let o = transform(m, [p[0], p[1], 0.0, 1.0]);
            out.push(Vertex {
                pos: [o[0], o[1], o[2]],
                uv: [u0 + t[0] * du, v0 + t[1] * dv],
                color,
                light,
            });
        }
        px += cw;
    }
}

/// `FontRenderer.getStringWidth` for the default font.
pub fn width(atlas: &Atlas, s: &str) -> f32 {
    s.chars()
        .map(|c| {
            if c == ' ' {
                4.0
            } else {
                crate::gui::ascii_index(c).map_or(0.0, |i| atlas.char_width[i] as f32)
            }
        })
        .sum()
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
