//! `RenderPainting` and the frame of `RenderItemFrame`.

use super::mesher::Vertex;
use super::model::Tex;
use super::{mul, rot_y, transform, translate, Mat4};

pub const TEXTURES: [&str; 1] = ["../painting/paintings_kristoffer_zetterstrand"];

/// The world-space normal `n` under the rotation of `m`, lit as
/// `RenderHelper.enableStandardItemLighting` does.
fn lit(m: &Mat4, n: [f32; 3]) -> u8 {
    let w = [
        m[0] * n[0] + m[4] * n[1] + m[8] * n[2],
        m[1] * n[0] + m[5] * n[1] + m[9] * n[2],
        m[2] * n[0] + m[6] * n[1] + m[10] * n[2],
    ];
    let l = (w[0] * w[0] + w[1] * w[1] + w[2] * w[2]).sqrt().max(1e-6);
    let s = super::model::shade([w[0] / l, w[1] / l, w[2] / l]);
    (s * 255.0) as u8
}

/// One quad in Tessellator order, pushed in both windings (entities are
/// drawn unculled here).
fn quad(
    out: &mut Vec<Vertex>,
    m: &Mat4,
    v: [([f32; 3], [f32; 2]); 4],
    n: [f32; 3],
    light: [u8; 4],
) {
    let c = lit(m, n);
    let q: Vec<Vertex> = v
        .iter()
        .map(|(p, uv)| {
            let o = transform(m, [p[0], p[1], p[2], 1.0]);
            Vertex {
                pos: [o[0], o[1], o[2]],
                uv: *uv,
                color: [c, c, c, 255],
                light,
            }
        })
        .collect();
    out.extend_from_slice(&q);
    out.extend_from_slice(&[q[3], q[2], q[1], q[0]]);
}

/// `RenderPainting.doRender` at `rel` (camera-relative position) turned by
/// `yaw`; `light(cx, cy)` is `setLightmap` for the tile whose centre is at
/// (cx, cy) pixels.
pub fn painting(
    out: &mut Vec<Vertex>,
    tex: Tex,
    rel: [f32; 3],
    yaw: f32,
    art: (&str, i32, i32, i32, i32),
    light: &dyn Fn(f32, f32) -> [u8; 4],
) {
    let (_, w, h, ox, oy) = art;
    let mut m = mul(&translate(rel[0], rel[1], rel[2]), &rot_y(yaw));
    let k = 0.0625;
    m = mul(&m, &super::entities::scale(k, k, k));
    // texture pixel (u, v) of the 256 x 256 sheet in the atlas
    let r = tex.rect;
    let sz = tex.size;
    let uv = |u: f32, v: f32| {
        [
            r[0] + (r[2] - r[0]) * u * 256.0 / sz,
            r[1] + (r[3] - r[1]) * v * 256.0 / sz,
        ]
    };
    let x0 = -w as f32 / 2.0;
    let y0 = -h as f32 / 2.0;
    let d = 0.5f32;
    let (bu0, bu1, bv0, bv1) = (0.75f32, 0.8125f32, 0.0f32, 0.0625f32);
    let (tu0, tu1, tv0, tv1) = (0.75f32, 0.8125f32, 0.001953125f32, 0.001953125f32);
    let (su0, su1, sv0, sv1) = (0.7519531f32, 0.7519531f32, 0.0f32, 0.0625f32);
    for i in 0..w / 16 {
        for j in 0..h / 16 {
            let xa = x0 + ((i + 1) * 16) as f32;
            let xb = x0 + (i * 16) as f32;
            let ya = y0 + ((j + 1) * 16) as f32;
            let yb = y0 + (j * 16) as f32;
            let l = light((xa + xb) / 2.0, (ya + yb) / 2.0);
            let fu0 = (ox + w - i * 16) as f32 / 256.0;
            let fu1 = (ox + w - (i + 1) * 16) as f32 / 256.0;
            let fv0 = (oy + h - j * 16) as f32 / 256.0;
            let fv1 = (oy + h - (j + 1) * 16) as f32 / 256.0;
            quad(
                out,
                &m,
                [
                    ([xa, yb, -d], uv(fu1, fv0)),
                    ([xb, yb, -d], uv(fu0, fv0)),
                    ([xb, ya, -d], uv(fu0, fv1)),
                    ([xa, ya, -d], uv(fu1, fv1)),
                ],
                [0.0, 0.0, -1.0],
                l,
            );
            quad(
                out,
                &m,
                [
                    ([xa, ya, d], uv(bu0, bv0)),
                    ([xb, ya, d], uv(bu1, bv0)),
                    ([xb, yb, d], uv(bu1, bv1)),
                    ([xa, yb, d], uv(bu0, bv1)),
                ],
                [0.0, 0.0, 1.0],
                l,
            );
            quad(
                out,
                &m,
                [
                    ([xa, ya, -d], uv(tu0, tv0)),
                    ([xb, ya, -d], uv(tu1, tv0)),
                    ([xb, ya, d], uv(tu1, tv1)),
                    ([xa, ya, d], uv(tu0, tv1)),
                ],
                [0.0, 1.0, 0.0],
                l,
            );
            quad(
                out,
                &m,
                [
                    ([xa, yb, d], uv(tu0, tv0)),
                    ([xb, yb, d], uv(tu1, tv0)),
                    ([xb, yb, -d], uv(tu1, tv1)),
                    ([xa, yb, -d], uv(tu0, tv1)),
                ],
                [0.0, -1.0, 0.0],
                l,
            );
            quad(
                out,
                &m,
                [
                    ([xa, ya, d], uv(su1, sv0)),
                    ([xa, yb, d], uv(su1, sv1)),
                    ([xa, yb, -d], uv(su0, sv1)),
                    ([xa, ya, -d], uv(su0, sv0)),
                ],
                [-1.0, 0.0, 0.0],
                l,
            );
            quad(
                out,
                &m,
                [
                    ([xb, ya, -d], uv(su1, sv0)),
                    ([xb, yb, -d], uv(su1, sv1)),
                    ([xb, yb, d], uv(su0, sv1)),
                    ([xb, ya, d], uv(su0, sv0)),
                ],
                [1.0, 0.0, 0.0],
                l,
            );
        }
    }
}

/// `RenderBlocks.renderBlockAsItem` of a box with `overrideBlockBounds`
/// and one override icon `t` (atlas rect): six faces, UVs from the bounds.
fn block_box(out: &mut Vec<Vertex>, m: &Mat4, b: [f32; 6], t: [f32; 4], light: [u8; 4]) {
    let [x0, y0, z0, x1, y1, z1] = b;
    let u = |f: f32| t[0] + (t[2] - t[0]) * f;
    let v = |f: f32| t[1] + (t[3] - t[1]) * f;
    // bottom and top: u from x, v from z
    quad(
        out,
        m,
        [
            ([x0, y0, z1], [u(x0), v(z1)]),
            ([x0, y0, z0], [u(x0), v(z0)]),
            ([x1, y0, z0], [u(x1), v(z0)]),
            ([x1, y0, z1], [u(x1), v(z1)]),
        ],
        [0.0, -1.0, 0.0],
        light,
    );
    quad(
        out,
        m,
        [
            ([x1, y1, z1], [u(x1), v(z1)]),
            ([x1, y1, z0], [u(x1), v(z0)]),
            ([x0, y1, z0], [u(x0), v(z0)]),
            ([x0, y1, z1], [u(x0), v(z1)]),
        ],
        [0.0, 1.0, 0.0],
        light,
    );
    // north and south: u from x, v from 1 - y
    quad(
        out,
        m,
        [
            ([x0, y1, z0], [u(1.0 - x0), v(1.0 - y1)]),
            ([x1, y1, z0], [u(1.0 - x1), v(1.0 - y1)]),
            ([x1, y0, z0], [u(1.0 - x1), v(1.0 - y0)]),
            ([x0, y0, z0], [u(1.0 - x0), v(1.0 - y0)]),
        ],
        [0.0, 0.0, -1.0],
        light,
    );
    quad(
        out,
        m,
        [
            ([x0, y1, z1], [u(x0), v(1.0 - y1)]),
            ([x0, y0, z1], [u(x0), v(1.0 - y0)]),
            ([x1, y0, z1], [u(x1), v(1.0 - y0)]),
            ([x1, y1, z1], [u(x1), v(1.0 - y1)]),
        ],
        [0.0, 0.0, 1.0],
        light,
    );
    // west and east: u from z, v from 1 - y
    quad(
        out,
        m,
        [
            ([x0, y1, z1], [u(z1), v(1.0 - y1)]),
            ([x0, y1, z0], [u(z0), v(1.0 - y1)]),
            ([x0, y0, z0], [u(z0), v(1.0 - y0)]),
            ([x0, y0, z1], [u(z1), v(1.0 - y0)]),
        ],
        [-1.0, 0.0, 0.0],
        light,
    );
    quad(
        out,
        m,
        [
            ([x1, y0, z1], [u(1.0 - z1), v(1.0 - y0)]),
            ([x1, y0, z0], [u(1.0 - z0), v(1.0 - y0)]),
            ([x1, y1, z0], [u(1.0 - z0), v(1.0 - y1)]),
            ([x1, y1, z1], [u(1.0 - z1), v(1.0 - y1)]),
        ],
        [1.0, 0.0, 0.0],
        light,
    );
}

/// `RenderItemFrame.renderFrameItemAsBlock` with the block centre at
/// `centre` (camera-relative): the backing (`itemframe_background`) and
/// four birch-plank bars, turned by `yaw` (`renderBlockAsItem` adds 90).
pub fn frame(
    out: &mut Vec<Vertex>,
    planks: [f32; 4],
    background: [f32; 4],
    centre: [f32; 3],
    yaw: f32,
    light: [u8; 4],
    map: bool,
) {
    let mut m = mul(&translate(centre[0], centre[1], centre[2]), &rot_y(yaw));
    m = mul(&m, &rot_y(90.0));
    m = mul(&m, &translate(-0.5, -0.5, -0.5));
    let t = 0.0625f32;
    // renderFrameForMap: the whole block's face, a full-depth backing
    let h = if map { 0.5f32 } else { 0.75f32 / 2.0 };
    let back = if map { t } else { t * 0.5 };
    block_box(
        out,
        &m,
        [
            0.0,
            0.5 - h + t,
            0.5 - h + t,
            back,
            0.5 + h - t,
            0.5 + h - t,
        ],
        background,
        light,
    );
    for b in [
        [0.0, 0.5 - h, 0.5 - h, t + 1.0e-4, t + 0.5 - h, 0.5 + h],
        [0.0, 0.5 + h - t, 0.5 - h, t + 1.0e-4, 0.5 + h, 0.5 + h],
        [0.0, 0.5 - h, 0.5 - h, t, 0.5 + h, t + 0.5 - h],
        [0.0, 0.5 - h, 0.5 + h - t, t, 0.5 + h, 0.5 + h],
    ] {
        block_box(out, &m, b, planks, light);
    }
}
