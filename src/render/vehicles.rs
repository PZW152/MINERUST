//! `ModelBoat` drawn as `RenderBoat.doRender` sets it up.

use super::entities::scale;
use super::mesher::Vertex;
use super::model::{render, Draw, Part, Tex, PI};
use super::{mul, rot_x, rot_y, translate};

pub const TEXTURES: [&str; 1] = ["boat"];

/// `ModelBoat.boatSides`
fn boat_sides() -> Vec<Part> {
    let (w, h, d, y) = (24.0f32, 6.0f32, 20.0f32, 4.0f32);
    let side = |p: [f32; 3]| {
        Part::new(p).bx(
            0.0,
            0.0,
            [-w / 2.0 + 2.0, -h - 1.0, -1.0],
            [w - 4.0, h, 2.0],
            0.0,
        )
    };
    vec![
        Part::new([0.0, y, 0.0])
            .bx(
                0.0,
                8.0,
                [-w / 2.0, -d / 2.0 + 2.0, -3.0],
                [w, d - 4.0, 4.0],
                0.0,
            )
            .r(PI / 2.0, 0.0, 0.0),
        side([-w / 2.0 + 1.0, y, 0.0]).r(0.0, (std::f64::consts::PI * 3.0 / 2.0) as f32, 0.0),
        side([w / 2.0 - 1.0, y, 0.0]).r(0.0, PI / 2.0, 0.0),
        side([0.0, y, -d / 2.0 + 1.0]).r(0.0, PI, 0.0),
        side([0.0, y, d / 2.0 - 1.0]),
    ]
}

/// `RenderBoat.doRender`: `rel` is the camera-relative `posX/Y/Z`, `yaw`
/// the interpolated `rotationYaw`; `hit`, `damage` and `forward` are the
/// data watcher's time since hit, damage taken and forward direction.
#[allow(clippy::too_many_arguments)]
pub fn boat(
    out: &mut Vec<Vertex>,
    tex: Tex,
    rel: [f32; 3],
    yaw: f32,
    hit: i32,
    damage: f32,
    forward: i32,
    partial: f32,
    light: [u8; 4],
) {
    let mut m = translate(rel[0], rel[1], rel[2]);
    m = mul(&m, &rot_y(180.0 - yaw));
    let f10 = hit as f32 - partial;
    let mut f11 = damage - partial;
    if f11 < 0.0 {
        f11 = 0.0;
    }
    if f10 > 0.0 {
        m = mul(
            &m,
            &rot_x(crate::util::math::sin(f10) * f10 * f11 / 10.0 * forward as f32),
        );
    }
    let k = 0.75f32;
    m = mul(&m, &scale(k, k, k));
    m = mul(&m, &scale(1.0 / k, 1.0 / k, 1.0 / k));
    m = mul(&m, &scale(-1.0, -1.0, 1.0));
    let mut d = Draw {
        out,
        tex,
        light,
        tint: [1.0; 4],
    };
    for p in boat_sides() {
        render(&mut d, &m, &p);
    }
}

/// `ModelMinecart.sideModels`, with `render`'s `rotationPointY` of the
/// floor (`4.0F - var4`, `var4` being -0.1).
fn minecart_sides() -> Vec<Part> {
    let (w, h, d, y) = (20.0f32, 8.0f32, 16.0f32, 4.0f32);
    let side = |p: [f32; 3]| {
        Part::new(p).bx(
            0.0,
            0.0,
            [-w / 2.0 + 2.0, -h - 1.0, -1.0],
            [w - 4.0, h, 2.0],
            0.0,
        )
    };
    vec![
        Part::new([0.0, y, 0.0])
            .bx(0.0, 10.0, [-w / 2.0, -d / 2.0, -1.0], [w, d, 2.0], 0.0)
            .r(PI / 2.0, 0.0, 0.0),
        side([-w / 2.0 + 1.0, y, 0.0]).r(0.0, (std::f64::consts::PI * 3.0 / 2.0) as f32, 0.0),
        side([w / 2.0 - 1.0, y, 0.0]).r(0.0, PI / 2.0, 0.0),
        side([0.0, y, -d / 2.0 + 1.0]).r(0.0, PI, 0.0),
        side([0.0, y, d / 2.0 - 1.0]),
        Part::new([0.0, 4.0 - -0.1f32, 0.0])
            .bx(
                44.0,
                10.0,
                [-w / 2.0 + 1.0, -d / 2.0 + 1.0, -1.0],
                [w - 2.0, d - 2.0, 1.0],
                0.0,
            )
            .r(-PI / 2.0, 0.0, 0.0),
    ]
}

/// `RenderMinecart.doRender`. `pos` is the interpolated position, `rel`
/// the same relative to the camera; the cart follows the rails it is on
/// (`getPos` / `getPosOffset`), and its display tile is drawn by `block`
/// (`renderBlockAsItem` with the entity's brightness).
#[allow(clippy::too_many_arguments)]
pub fn minecart(
    out: &mut Vec<Vertex>,
    tex: Tex,
    w: &dyn crate::world::BlockView,
    c: &crate::entity::minecart::Minecart,
    pos: [f64; 3],
    rel: [f32; 3],
    mut yaw: f32,
    mut pitch: f32,
    partial: f32,
    light: [u8; 4],
    block: &dyn Fn(&mut Vec<Vertex>, u8, u8, &super::Mat4, [u8; 4]),
) {
    use crate::entity::minecart::{get_pos, get_pos_offset};
    let mut k = (c.id as i64).wrapping_mul(493286711);
    k = k
        .wrapping_mul(k)
        .wrapping_mul(4392167121)
        .wrapping_add(k.wrapping_mul(98761));
    let j = |s: i64| (((k >> s & 7) as f32 + 0.5) / 8.0 - 0.5) * 0.004;
    let mut x = rel[0] as f64;
    let mut y = rel[1] as f64;
    let mut z = rel[2] as f64;
    if let Some(v) = get_pos(w, pos[0], pos[1], pos[2]) {
        let d = 0.3f32 as f64;
        let a = get_pos_offset(w, pos[0], pos[1], pos[2], d).unwrap_or(v);
        let b = get_pos_offset(w, pos[0], pos[1], pos[2], -d).unwrap_or(v);
        x += v[0] - pos[0];
        y += (a[1] + b[1]) / 2.0 - pos[1];
        z += v[2] - pos[2];
        let mut dv = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let l = (dv[0] * dv[0] + dv[1] * dv[1] + dv[2] * dv[2]).sqrt();
        if l != 0.0 {
            // Vec3.normalize
            if l < 1.0e-4 {
                dv = [0.0; 3];
            } else {
                dv = [dv[0] / l, dv[1] / l, dv[2] / l];
            }
            yaw = (dv[2].atan2(dv[0]) * 180.0 / std::f64::consts::PI) as f32;
            pitch = (dv[1].atan() * 73.0) as f32;
        }
    }
    let mut m = translate(j(16), j(20), j(24));
    m = mul(&m, &translate(x as f32, y as f32, z as f32));
    m = mul(&m, &rot_y(180.0 - yaw));
    m = mul(&m, &super::rot_z(-pitch));
    let f32_ = c.rolling_amplitude as f32 - partial;
    let mut f33 = c.damage - partial;
    if f33 < 0.0 {
        f33 = 0.0;
    }
    if f32_ > 0.0 {
        m = mul(
            &m,
            &rot_x(crate::util::math::sin(f32_) * f32_ * f33 / 10.0 * c.rolling_direction as f32),
        );
    }
    let (tile, data, offset) = c.display_tile();
    // getRenderType() != -1
    if crate::block::info(tile).shape != crate::block::Shape::None {
        let s = 0.75f32;
        let mut bm = mul(&m, &scale(s, s, s));
        bm = mul(&bm, &translate(0.0, offset as f32 / 16.0, 0.0));
        // renderBlockAsItem draws the block centred on the origin
        bm = mul(&bm, &translate(-0.5, -0.5, -0.5));
        block(out, tile, data as u8, &bm, light);
    }
    m = mul(&m, &scale(-1.0, -1.0, 1.0));
    let mut d = Draw {
        out,
        tex,
        light,
        tint: [1.0; 4],
    };
    for p in minecart_sides() {
        render(&mut d, &m, &p);
    }
}
