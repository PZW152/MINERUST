//! `TileEntityChestRenderer` with `ModelChest` / `ModelLargeChest`, and
//! `TileEntityRendererChestHelper` for the chest item.

use super::mesher::Vertex;
use super::model::{render, Draw, Part, Tex};
use super::{mul, rot_y, translate, Mat4};

pub const TEXTURES: [&str; 6] = [
    "chest/normal",
    "chest/normal_double",
    "chest/trapped",
    "chest/trapped_double",
    "chest/christmas",
    "chest/christmas_double",
];

/// `ModelChest` (14 wide) or `ModelLargeChest` (30 wide) with the lid
/// opened by `lid` (`1 - (1 - f)^3` already applied).
fn parts(large: bool, lid: f32) -> Vec<Part> {
    let w = if large { 30.0 } else { 14.0 };
    let knob_x = if large { 16.0 } else { 8.0 };
    let a = -(lid * std::f32::consts::PI / 2.0);
    vec![
        Part::new([1.0, 7.0, 15.0])
            .bx(0.0, 0.0, [0.0, -5.0, -14.0], [w, 5.0, 14.0], 0.0)
            .r(a, 0.0, 0.0),
        Part::new([knob_x, 7.0, 15.0])
            .bx(0.0, 0.0, [-1.0, -2.0, -15.0], [2.0, 4.0, 1.0], 0.0)
            .r(a, 0.0, 0.0),
        Part::new([1.0, 6.0, 1.0]).bx(0.0, 19.0, [0.0, 0.0, 0.0], [w, 10.0, 14.0], 0.0),
    ]
}

/// The texture name for a chest: trapped or not, single or double, and
/// the 24th-26th of December.
pub fn texture(trapped: bool, large: bool, christmas: bool) -> &'static str {
    match (christmas, trapped, large) {
        (true, _, false) => "chest/christmas",
        (true, _, true) => "chest/christmas_double",
        (false, true, false) => "chest/trapped",
        (false, true, true) => "chest/trapped_double",
        (false, false, false) => "chest/normal",
        (false, false, true) => "chest/normal_double",
    }
}

/// `TileEntityChestRenderer.renderTileEntityAt` for the chest whose block
/// corner is at `rel` (camera relative). `x_pos` / `z_pos`: the other half
/// of a double chest is east / south (this half renders the pair).
#[allow(clippy::too_many_arguments)]
pub fn world(
    out: &mut Vec<Vertex>,
    tex: Tex,
    rel: [f32; 3],
    meta: i32,
    x_pos: bool,
    z_pos: bool,
    lid: f32,
    light: [u8; 4],
) {
    let mut m = translate(rel[0], rel[1] + 1.0, rel[2] + 1.0);
    m = mul(&m, &super::entities::scale(1.0, -1.0, -1.0));
    m = mul(&m, &translate(0.5, 0.5, 0.5));
    let angle = match meta {
        2 => 180.0,
        4 => 90.0,
        5 => -90.0,
        _ => 0.0,
    };
    if meta == 2 && x_pos {
        m = mul(&m, &translate(1.0, 0.0, 0.0));
    }
    if meta == 5 && z_pos {
        m = mul(&m, &translate(0.0, 0.0, -1.0));
    }
    m = mul(&m, &rot_y(angle));
    m = mul(&m, &translate(-0.5, -0.5, -0.5));
    let f = 1.0 - lid;
    let f = 1.0 - f * f * f;
    let mut d = Draw {
        out,
        tex,
        light,
        tint: [1.0; 4],
    };
    for p in parts(x_pos || z_pos, f) {
        render(&mut d, &m, &p);
    }
}

/// `RenderItem.renderItemIntoGUI` of a chest (`renderBlockAsItem` type
/// 22): the closed single chest at the item's 16x16 place.
pub fn gui_icon(tex: Tex, base: &Mat4) -> Vec<Vertex> {
    let mut m = mul(base, &rot_y(90.0));
    m = mul(&m, &translate(-0.5, -0.5, -0.5));
    // TileEntityRendererChestHelper: renderTileEntityAt(chest, 0, 0, 0, 0)
    m = mul(&m, &translate(0.0, 1.0, 1.0));
    m = mul(&m, &super::entities::scale(1.0, -1.0, -1.0));
    m = mul(&m, &translate(0.5, 0.5, 0.5));
    m = mul(&m, &translate(-0.5, -0.5, -0.5));
    let mut raw = Vec::new();
    {
        let mut d = Draw {
            out: &mut raw,
            tex,
            light: [0; 4],
            tint: [1.0; 4],
        };
        for p in parts(false, 0.0) {
            render(&mut d, &m, &p);
        }
    }
    // RenderHelper.enableGUIStandardItemLighting
    let lm = mul(&super::rot_y(-30.0), &super::rot_x(165.0));
    super::model::gui_lit(raw, &lm)
}
