//! `RenderPainting` and the frame of `RenderItemFrame`.

use super::mesher::Vertex;
use super::model::Tex;
use super::{mul, rot_y, transform, translate, Mat4};

pub const TEXTURES: [&str; 6] = [
    "../painting/paintings_kristoffer_zetterstrand",
    "lead_knot",
    "beacon_beam",
    "creeper/creeper_armor",
    "wither/wither",
    "wither/wither_invulnerable",
];

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

/// `RenderLeashKnot`: `ModelLeashKnot` (one 6 x 8 x 6 box) upside down at
/// the knot.
pub fn knot(out: &mut Vec<Vertex>, tex: Tex, rel: [f32; 3], light: [u8; 4]) {
    let m = mul(
        &translate(rel[0], rel[1], rel[2]),
        &super::entities::scale(-1.0, -1.0, 1.0),
    );
    let part = super::model::Part::new([0.0; 3]).bx(0.0, 0.0, [-3.0, -6.0, -3.0], [6.0, 8.0, 6.0], 0.0);
    let mut d = super::model::Draw {
        out,
        tex,
        light,
        tint: [1.0; 4],
    };
    super::model::render(&mut d, &m, &part);
}

/// One side of a beacon beam from `a` to `b` (x, z at the bottom and
/// top), `h` high, with u running `u0 -> u1` and v from `v0` at the
/// bottom to `v1` at the top, repeating (`GL_REPEAT`) inside the tile
/// `t`: cut where v crosses a whole number. Both sides are drawn.
fn beam_side(
    out: &mut Vec<Vertex>,
    t: [f32; 4],
    base: [f32; 3],
    a: [f32; 2],
    b: [f32; 2],
    h: f32,
    u: [f32; 2],
    v: [f32; 2],
    colour: [u8; 4],
) {
    let (v0, v1) = (v[0], v[1]);
    if h <= 0.0 || v1 <= v0 {
        return;
    }
    let tu = |x: f32| t[0] + (t[2] - t[0]) * x;
    let tv = |x: f32| t[1] + (t[3] - t[1]) * x;
    let mut lo = v0;
    while lo < v1 {
        let hi = (lo.floor() + 1.0).min(v1);
        let k = lo.floor();
        let (y0, y1) = ((lo - v0) / (v1 - v0) * h, (hi - v0) / (v1 - v0) * h);
        let (f0, f1) = (lo - k, hi - k);
        let q = [
            ([a[0], y1, a[1]], [tu(u[0]), tv(f1)]),
            ([a[0], y0, a[1]], [tu(u[0]), tv(f0)]),
            ([b[0], y0, b[1]], [tu(u[1]), tv(f0)]),
            ([b[0], y1, b[1]], [tu(u[1]), tv(f1)]),
        ];
        for order in [[0usize, 1, 2, 3], [3, 2, 1, 0]] {
            for i in order {
                let (p, uv) = q[i];
                out.push(Vertex {
                    pos: [base[0] + p[0], base[1] + p[1], base[2] + p[2]],
                    uv,
                    color: colour,
                    light: [240, 240, 0, 0],
                });
            }
        }
        lo = hi;
    }
}

/// `TileEntityBeaconRenderer`: the turning inner beam (drawn solid with
/// the alpha test) and the faint outer square (blended), 256 blocks high
/// times `scale`; `time` is the world time with the partial tick.
pub fn beacon_beam(
    solid: &mut Vec<Vertex>,
    blended: &mut Vec<Vertex>,
    t: [f32; 4],
    base: [f32; 3],
    time: f64,
    scale: f32,
) {
    let c = [255, 255, 255, 32];
    let f12 = (-time * 0.2 - (-time * 0.1).floor()) as f32;
    let a = time * 0.025 * (1.0 - 2.5);
    let r = 0.2f64;
    let pt = |k: f64| -> [f32; 2] {
        [
            (0.5 + (a + k).cos() * r) as f32,
            (0.5 + (a + k).sin() * r) as f32,
        ]
    };
    use std::f64::consts::PI;
    let (p18, p22, p26, p30) = (pt(PI * 3.0 / 4.0), pt(PI / 4.0), pt(PI * 5.0 / 4.0), pt(PI * 7.0 / 4.0));
    let h = 256.0 * scale;
    let v0 = -1.0 + f12;
    let v1 = 256.0 * scale * (0.5 / 0.2) as f32 + v0;
    for (x, y) in [(p18, p22), (p30, p26), (p22, p30), (p26, p18)] {
        beam_side(solid, t, base, x, y, h, [1.0, 0.0], [v0, v1], c);
    }
    let v1 = 256.0 * scale + v0;
    let (a0, a1, b0, b1) = ([0.2, 0.2], [0.8, 0.2], [0.8, 0.8], [0.2, 0.8]);
    for (x, y) in [(a0, a1), (b0, b1), (a1, b0), (b1, a0)] {
        beam_side(blended, t, base, x, y, h, [1.0, 0.0], [v0, v1], c);
    }
}

/// `TileEntitySkullRenderer.render`: `ModelSkeletonHead` (an 8 x 8 x 8
/// head) on the floor turned by `rot` degrees, or against the wall on
/// side `side`; `rel` is the block's corner.
pub fn skull(out: &mut Vec<Vertex>, tex: Tex, rel: [f32; 3], side: i32, rot: f32, light: [u8; 4]) {
    let (t, r) = match side {
        1 => ([0.5, 0.0, 0.5], rot),
        2 => ([0.5, 0.25, 0.74], rot),
        3 => ([0.5, 0.25, 0.26], 180.0),
        4 => ([0.74, 0.25, 0.5], 270.0),
        _ => ([0.26, 0.25, 0.5], 90.0),
    };
    let m = mul(
        &translate(rel[0] + t[0], rel[1] + t[1], rel[2] + t[2]),
        &super::entities::scale(-1.0, -1.0, 1.0),
    );
    let part = super::model::Part::new([0.0; 3])
        .bx(0.0, 0.0, [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], 0.0)
        .r(0.0, r.to_radians(), 0.0);
    let mut d = super::model::Draw {
        out,
        tex,
        light,
        tint: [1.0; 4],
    };
    super::model::render(&mut d, &m, &part);
}

/// The texture of head kind `kind` (`TileEntitySkullRenderer`).
pub fn skull_texture(kind: i32) -> &'static str {
    match kind {
        1 => "skeleton/wither_skeleton",
        2 => "zombie/zombie",
        3 => "steve",
        4 => "creeper/creeper",
        _ => "skeleton/skeleton",
    }
}

/// `RenderLightningBolt`: four widening layers of the main stroke and two
/// side branches, each eight 16-block segments, from the seed `vertex`.
pub fn lightning(out: &mut Vec<Vertex>, white: [f32; 4], rel: [f32; 3], vertex: i64) {
    use crate::java::JavaRandom;
    let uv = [(white[0] + white[2]) * 0.5, (white[1] + white[3]) * 0.5];
    let c = [(0.45f32 * 255.0) as u8, (0.45f32 * 255.0) as u8, (0.5f32 * 255.0) as u8, (0.3f32 * 255.0) as u8];
    let mut xs = [0f64; 8];
    let mut zs = [0f64; 8];
    let (mut x13, mut z15) = (0f64, 0f64);
    let mut r = JavaRandom::new(vertex);
    for i in (0..8).rev() {
        xs[i] = x13;
        zs[i] = z15;
        x13 += (r.next_int_n(11) - 5) as f64;
        z15 += (r.next_int_n(11) - 5) as f64;
    }
    let (x, y, z) = (rel[0] as f64, rel[1] as f64, rel[2] as f64);
    for layer in 0..4 {
        let mut r2 = JavaRandom::new(vertex);
        for branch in 0..3 {
            let mut top = 7i32;
            let mut bottom = 0i32;
            if branch > 0 {
                top = 7 - branch;
                bottom = top - 2;
            }
            let mut dx = xs[top as usize] - x13;
            let mut dz = zs[top as usize] - z15;
            let mut seg = top;
            while seg >= bottom {
                let (px, pz) = (dx, dz);
                if branch == 0 {
                    dx += (r2.next_int_n(11) - 5) as f64;
                    dz += (r2.next_int_n(11) - 5) as f64;
                } else {
                    dx += (r2.next_int_n(31) - 15) as f64;
                    dz += (r2.next_int_n(31) - 15) as f64;
                }
                let mut w1 = 0.1 + layer as f64 * 0.2;
                if branch == 0 {
                    w1 *= seg as f64 * 0.1 + 1.0;
                }
                let mut w2 = 0.1 + layer as f64 * 0.2;
                if branch == 0 {
                    w2 *= (seg - 1) as f64 * 0.1 + 1.0;
                }
                let mut strip = Vec::with_capacity(10);
                for k in 0..5 {
                    let mut ax = x + 0.5 - w1;
                    let mut az = z + 0.5 - w1;
                    if k == 1 || k == 2 {
                        ax += w1 * 2.0;
                    }
                    if k == 2 || k == 3 {
                        az += w1 * 2.0;
                    }
                    let mut bx = x + 0.5 - w2;
                    let mut bz = z + 0.5 - w2;
                    if k == 1 || k == 2 {
                        bx += w2 * 2.0;
                    }
                    if k == 2 || k == 3 {
                        bz += w2 * 2.0;
                    }
                    strip.push([bx + dx, y + (seg * 16) as f64, bz + dz]);
                    strip.push([ax + px, y + ((seg + 1) * 16) as f64, az + pz]);
                }
                // GL_TRIANGLE_STRIP as quads, both ways round
                for k in 0..4 {
                    let q = [strip[2 * k], strip[2 * k + 1], strip[2 * k + 3], strip[2 * k + 2]];
                    for order in [[0usize, 1, 2, 3], [3, 2, 1, 0]] {
                        for i in order {
                            let p = q[i];
                            out.push(Vertex {
                                pos: [p[0] as f32, p[1] as f32, p[2] as f32],
                                uv,
                                color: c,
                                light: [240, 240, 0, 0],
                            });
                        }
                    }
                }
                seg -= 1;
            }
        }
    }
}

/// `RenderWitherSkull`: `ModelSkeletonHead(0, 35, 64, 64)` turned with the
/// skull's flight.
pub fn wither_skull(out: &mut Vec<Vertex>, tex: Tex, rel: [f32; 3], yaw: f32, pitch: f32) {
    let m = mul(&translate(rel[0], rel[1], rel[2]), &super::entities::scale(-1.0, -1.0, 1.0));
    let part = super::model::Part::new([0.0; 3])
        .bx(0.0, 35.0, [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], 0.0)
        .r(pitch.to_radians(), yaw.to_radians(), 0.0);
    let mut d = super::model::Draw {
        out,
        tex,
        light: [240, 240, 0, 0],
        tint: [1.0; 4],
    };
    super::model::render(&mut d, &m, &part);
}
