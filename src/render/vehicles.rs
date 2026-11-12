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
