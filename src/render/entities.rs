//! Geometry for things that are not part of the chunk meshes: dropped items
//! (`RenderItem.doRender`), the block-breaking overlay
//! (`RenderGlobal.drawBlockDamageTexture`) and the first-person hand
//! (`ItemRenderer.renderItemInFirstPerson`).

use super::atlas::Atlas;
use super::mesher::Vertex;
use super::{mul, rot_x, rot_y, rot_z, transform, translate, Mat4};
use crate::block::{self, Shape};
use crate::item::Stack;
use crate::java::JavaRandom;

pub fn scale(x: f32, y: f32, z: f32) -> Mat4 {
    let mut m = super::IDENTITY;
    m[0] = x;
    m[5] = y;
    m[10] = z;
    m
}

#[inline]
fn light_bytes(sky: i32, blk: i32) -> [u8; 4] {
    [(blk << 4) as u8, (sky << 4) as u8, 0, 0]
}

/// Corners and tile UVs of a unit cube's faces, in `RenderBlocks` order.
const FACES: [(usize, [[f32; 3]; 4], [[f32; 2]; 4], f32); 6] = [
    (
        0,
        [
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 1.0],
        ],
        [[0.0, 1.0], [0.0, 0.0], [1.0, 0.0], [1.0, 1.0]],
        0.5,
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
        1.0,
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
        0.8,
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
        0.8,
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
        0.6,
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
        0.6,
    ),
];

fn tint_for(id: u8, meta: i32, face: usize) -> [f32; 3] {
    let c = match id {
        block::GRASS if face == 1 => 0x7CBD6B,
        block::LEAVES if meta & 3 == 1 => 6396257,
        block::LEAVES if meta & 3 == 2 => 8431445,
        block::LEAVES | block::LEAVES2 | block::VINE | block::WATERLILY => 0x48B518,
        block::TALLGRASS if meta != 0 => 0x7CBD6B,
        _ => 0xFFFFFF,
    };
    [
        (c >> 16 & 255) as f32 / 255.0,
        (c >> 8 & 255) as f32 / 255.0,
        (c & 255) as f32 / 255.0,
    ]
}

/// A unit cube (`0..1`) of block `id`, transformed by `m`.
fn block_cube(
    out: &mut Vec<Vertex>,
    atlas: &Atlas,
    id: u8,
    meta: i32,
    m: &Mat4,
    light: [u8; 4],
    height: f32,
) {
    for (face, corners, uvs, shade) in FACES {
        let Some(t) = atlas.tile(id, meta as u8, face) else {
            continue;
        };
        let tint = tint_for(id, meta, face);
        let c = [
            (tint[0] * shade * 255.0) as u8,
            (tint[1] * shade * 255.0) as u8,
            (tint[2] * shade * 255.0) as u8,
            255,
        ];
        for k in 0..4 {
            let p = corners[k];
            let o = transform(m, [p[0], p[1] * height, p[2], 1.0]);
            let mut uv = uvs[k];
            if face >= 2 && height < 1.0 {
                uv[1] = 1.0 - (1.0 - uv[1]) * height;
            }
            out.push(Vertex {
                pos: [o[0], o[1], o[2]],
                uv: [t[0] + (t[2] - t[0]) * uv[0], t[1] + (t[3] - t[1]) * uv[1]],
                color: c,
                light,
            });
        }
    }
}

/// `ItemRenderer.renderItemIn2D`: a sprite extruded to `thickness`.
fn extruded(
    out: &mut Vec<Vertex>,
    t: [f32; 4],
    m: &Mat4,
    light: [u8; 4],
    tint: [f32; 3],
    thickness: f32,
) {
    let c = |s: f32| {
        [
            (tint[0] * s * 255.0) as u8,
            (tint[1] * s * 255.0) as u8,
            (tint[2] * s * 255.0) as u8,
            255,
        ]
    };
    let mut quad = |p: [[f32; 3]; 4], uv: [[f32; 2]; 4], s: f32| {
        for k in 0..4 {
            let o = transform(m, [p[k][0], p[k][1], p[k][2], 1.0]);
            out.push(Vertex {
                pos: [o[0], o[1], o[2]],
                uv: uv[k],
                color: c(s),
                light,
            });
        }
    };
    let (u0, v0, u1, v1) = (t[0], t[1], t[2], t[3]);
    // Front and back faces of the sprite.
    quad(
        [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ],
        [[u1, v1], [u0, v1], [u0, v0], [u1, v0]],
        1.0,
    );
    quad(
        [
            [0.0, 1.0, -thickness],
            [1.0, 1.0, -thickness],
            [1.0, 0.0, -thickness],
            [0.0, 0.0, -thickness],
        ],
        [[u1, v0], [u0, v0], [u0, v1], [u1, v1]],
        0.8,
    );
    // One strip per texel column and row for the edges.
    let n = 16;
    for i in 0..n {
        let f = i as f32 / n as f32;
        let u = u1 + (u0 - u1) * f - (u1 - u0) / 32.0;
        quad(
            [
                [f, 0.0, -thickness],
                [f, 0.0, 0.0],
                [f, 1.0, 0.0],
                [f, 1.0, -thickness],
            ],
            [[u, v1], [u, v1], [u, v0], [u, v0]],
            0.6,
        );
        let f2 = (i + 1) as f32 / n as f32;
        quad(
            [
                [f2, 1.0, -thickness],
                [f2, 1.0, 0.0],
                [f2, 0.0, 0.0],
                [f2, 0.0, -thickness],
            ],
            [[u, v0], [u, v0], [u, v1], [u, v1]],
            0.6,
        );
        let v = v1 + (v0 - v1) * f - (v1 - v0) / 32.0;
        quad(
            [
                [0.0, f2, 0.0],
                [1.0, f2, 0.0],
                [1.0, f2, -thickness],
                [0.0, f2, -thickness],
            ],
            [[u1, v], [u0, v], [u0, v], [u1, v]],
            0.9,
        );
        quad(
            [
                [1.0, f, 0.0],
                [0.0, f, 0.0],
                [0.0, f, -thickness],
                [1.0, f, -thickness],
            ],
            [[u0, v], [u1, v], [u1, v], [u0, v]],
            0.9,
        );
    }
}

fn is_3d(id: u8) -> bool {
    matches!(
        block::info(id).shape,
        Shape::Cube | Shape::Cactus | Shape::Chest | Shape::Slab | Shape::SnowLayer
    )
}

fn sprite_rect(atlas: &Atlas, s: &Stack) -> Option<[f32; 4]> {
    if let Some(b) = s.block() {
        let face = if b == block::DOUBLE_PLANT { 1 } else { 0 };
        return atlas.tile(b, s.meta as u8, face);
    }
    let meta = if crate::item::tool(s.id).is_some() {
        0
    } else {
        s.meta
    };
    atlas
        .items
        .get(&(s.id, meta))
        .map(|&t| atlas.rects[t as usize])
}

/// One dropped item, relative to the camera. `light` is `(sky, block)` at
/// the item, already combined with the sky darkening by the shader.
#[allow(clippy::too_many_arguments)]
/// `RenderFireball`: the fire charge sprite facing the camera.
pub fn billboard(
    out: &mut Vec<Vertex>,
    rect: [f32; 4],
    c: [f32; 3],
    size: f32,
    yaw: f32,
    pitch: f32,
) {
    let mut m = translate(c[0], c[1], c[2]);
    m = mul(&m, &rot_y(180.0 - yaw));
    m = mul(&m, &rot_x(-pitch));
    m = mul(&m, &scale(size, size, size));
    let p = |x: f32, y: f32| transform(&m, [x, y, 0.0, 1.0]);
    let (u0, v0, u1, v1) = (rect[0], rect[1], rect[2], rect[3]);
    let l = [240u8, 240, 0, 0];
    let c4 = [255u8; 4];
    let q = [
        (p(-0.5, -0.25), [u0, v1]),
        (p(0.5, -0.25), [u1, v1]),
        (p(0.5, 0.75), [u1, v0]),
        (p(-0.5, 0.75), [u0, v0]),
    ];
    let vs: Vec<Vertex> = q
        .iter()
        .map(|(o, uv)| Vertex {
            pos: [o[0], o[1], o[2]],
            uv: *uv,
            color: c4,
            light: l,
        })
        .collect();
    out.extend_from_slice(&vs);
    out.extend_from_slice(&[vs[3], vs[2], vs[1], vs[0]]);
}

/// `RenderSnowball.func_77026_a`: an item icon facing the camera, lit by
/// the entity's brightness and tinted.
#[allow(clippy::too_many_arguments)]
pub fn billboard_lit(
    out: &mut Vec<Vertex>,
    rect: [f32; 4],
    c: [f32; 3],
    size: f32,
    yaw: f32,
    pitch: f32,
    light: (i32, i32),
    tint: [f32; 3],
) {
    let start = out.len();
    billboard(out, rect, c, size, yaw, pitch);
    let l = light_bytes(light.0, light.1);
    let col = [
        (tint[0] * 255.0) as u8,
        (tint[1] * 255.0) as u8,
        (tint[2] * 255.0) as u8,
        255,
    ];
    for v in &mut out[start..] {
        v.light = l;
        v.color = col;
    }
}

/// A unit block (`RenderBlocks.renderBlockAsItem`) transformed by `m`.
pub fn block_model(
    out: &mut Vec<Vertex>,
    atlas: &Atlas,
    id: u8,
    meta: u8,
    m: &Mat4,
    light: [u8; 4],
) {
    block_cube(out, atlas, id, meta as i32, m, light, 1.0);
}

/// `RenderTNTPrimed.doRender`: the block swells in its last ten ticks and
/// flashes white every five.
pub fn primed_tnt(
    out: &mut Vec<Vertex>,
    atlas: &Atlas,
    centre: [f32; 3],
    fuse: i32,
    partial: f32,
    light: (i32, i32),
) {
    let l = light_bytes(light.0, light.1);
    let left = fuse as f32 - partial + 1.0;
    let mut m = translate(centre[0], centre[1], centre[2]);
    if left < 10.0 {
        let mut f = (1.0 - left / 10.0).clamp(0.0, 1.0);
        f *= f;
        f *= f;
        let s = 1.0 + f * 0.3;
        m = mul(&m, &scale(s, s, s));
    }
    m = mul(&m, &translate(-0.5, -0.5, -0.5));
    let start = out.len();
    block_cube(out, atlas, block::TNT, 0, &m, l, 1.0);
    if fuse / 5 % 2 == 0 {
        let a = (1.0 - left / 100.0) * 0.8;
        for v in &mut out[start..] {
            for c in 0..3 {
                let x = v.color[c] as f32 / 255.0;
                v.color[c] = ((x + (1.0 - x) * a).min(1.0) * 255.0) as u8;
            }
            v.light = [240, 240, 0, 0];
        }
    }
}

pub fn item_entity(
    out: &mut Vec<Vertex>,
    atlas: &Atlas,
    s: &Stack,
    pos: [f32; 3],
    age: f32,
    hover: f32,
    view_yaw: f32,
    light: (i32, i32),
) {
    let l = light_bytes(light.0, light.1);
    let bob = ((age / 10.0 + hover).sin() * 0.1 + 0.1) as f32;
    let spin = ((age / 20.0 + hover) * (180.0 / std::f32::consts::PI)) as f32;
    let copies = if s.count > 20 {
        4
    } else if s.count > 5 {
        3
    } else if s.count > 1 {
        2
    } else {
        1
    };
    let mut r = JavaRandom::new(187);
    let base = mul(&translate(pos[0], pos[1] + bob, pos[2]), &rot_y(spin));
    match s.block().filter(|&b| is_3d(b)) {
        Some(b) => {
            let h = if block::info(b).shape == Shape::Slab {
                0.5
            } else {
                1.0
            };
            for i in 0..copies {
                let mut m = base;
                if i > 0 {
                    let dx = (r.next_float() * 2.0 - 1.0) * 0.2 / 0.25;
                    let dy = (r.next_float() * 2.0 - 1.0) * 0.2 / 0.25;
                    let dz = (r.next_float() * 2.0 - 1.0) * 0.2 / 0.25;
                    m = mul(&m, &scale(0.25, 0.25, 0.25));
                    m = mul(&m, &translate(dx, dy, dz));
                } else {
                    m = mul(&m, &scale(0.25, 0.25, 0.25));
                }
                m = mul(&m, &translate(-0.5, -0.5, -0.5));
                block_cube(out, atlas, b, s.meta, &m, l, h);
            }
        }
        None => {
            let passes: Vec<([f32; 4], [f32; 3])> = if s.block().is_some() {
                match sprite_rect(atlas, s) {
                    Some(t) => vec![(t, s.block().map_or([1.0; 3], |b| tint_for(b, s.meta, 0)))],
                    None => return,
                }
            } else {
                atlas.item_passes(s.id, s.meta)
            };
            // Fancy graphics: 3D sprites that spin like blocks.
            for (t, tint) in passes {
                for i in 0..copies {
                    let mut m = mul(&translate(pos[0], pos[1] + bob, pos[2]), &rot_y(spin));
                    m = mul(&m, &scale(0.5, 0.5, 0.5));
                    if i > 0 {
                        let off = 0.0625 * 1.5 * i as f32 * 0.5;
                        m = mul(&m, &translate(0.0, 0.0, off));
                    }
                    m = mul(&m, &translate(-0.5, -0.25, 0.0));
                    extruded(out, t, &m, l, tint, 0.0625);
                }
            }
            let _ = view_yaw;
        }
    }
}

/// `drawBlockDamageTexture`: the crack texture on every face of the block,
/// pushed out a little so it wins the depth test.
pub fn crack(out: &mut Vec<Vertex>, atlas: &Atlas, stage: usize, rel: [f32; 3], bounds: [f32; 6]) {
    let t = atlas.rects[atlas.destroy[stage.min(9)] as usize];
    let e = 0.002f32;
    let b = [
        bounds[0] - e,
        bounds[1] - e,
        bounds[2] - e,
        bounds[3] + e,
        bounds[4] + e,
        bounds[5] + e,
    ];
    for (_, corners, uvs, _) in FACES {
        for k in 0..4 {
            let p = corners[k];
            let q = [
                if p[0] > 0.5 { b[3] } else { b[0] },
                if p[1] > 0.5 { b[4] } else { b[1] },
                if p[2] > 0.5 { b[5] } else { b[2] },
            ];
            out.push(Vertex {
                pos: [rel[0] + q[0], rel[1] + q[1], rel[2] + q[2]],
                uv: [
                    t[0] + (t[2] - t[0]) * uvs[k][0],
                    t[1] + (t[3] - t[1]) * uvs[k][1],
                ],
                color: [255; 4],
                light: [240, 240, 0, 0],
            });
        }
    }
}

/// `ModelBox` for a 64x32 skin placed at `skin` in the atlas.
fn model_box(
    out: &mut Vec<Vertex>,
    m: &Mat4,
    skin: [f32; 4],
    tex: (f32, f32),
    o: [f32; 3],
    d: [f32; 3],
    light: [u8; 4],
) {
    let (u, v) = tex;
    let (w, h, dd) = (d[0], d[1], d[2]);
    let px = |x: f32, y: f32| {
        [
            skin[0] + (skin[2] - skin[0]) * x / 64.0,
            skin[1] + (skin[3] - skin[1]) * y / 64.0,
        ]
    };
    let (x0, y0, z0) = (o[0], o[1], o[2]);
    let (x1, y1, z1) = (x0 + w, y0 + h, z0 + dd);
    let s = 1.0 / 16.0;
    let mut quad = |p: [[f32; 3]; 4], uv: [[f32; 2]; 2], shade: f32| {
        let c = (shade * 255.0) as u8;
        let uvs = [
            [uv[1][0], uv[0][1]],
            [uv[0][0], uv[0][1]],
            [uv[0][0], uv[1][1]],
            [uv[1][0], uv[1][1]],
        ];
        for k in 0..4 {
            let q = transform(m, [p[k][0] * s, p[k][1] * s, p[k][2] * s, 1.0]);
            out.push(Vertex {
                pos: [q[0], q[1], q[2]],
                uv: uvs[k],
                color: [c, c, c, 255],
                light,
            });
        }
    };
    // Order as ModelBox.quadList: +X, -X, -Y, +Y, -Z, +Z.
    quad(
        [[x1, y0, z1], [x1, y0, z0], [x1, y1, z0], [x1, y1, z1]],
        [px(u + dd + w, v + dd), px(u + dd + w + dd, v + dd + h)],
        0.6,
    );
    quad(
        [[x0, y0, z0], [x0, y0, z1], [x0, y1, z1], [x0, y1, z0]],
        [px(u, v + dd), px(u + dd, v + dd + h)],
        0.6,
    );
    quad(
        [[x1, y0, z1], [x0, y0, z1], [x0, y0, z0], [x1, y0, z0]],
        [px(u + dd, v), px(u + dd + w, v + dd)],
        1.0,
    );
    quad(
        [[x1, y1, z0], [x0, y1, z0], [x0, y1, z1], [x1, y1, z1]],
        [px(u + dd + w, v + dd), px(u + dd + w + w, v)],
        0.5,
    );
    quad(
        [[x1, y0, z0], [x0, y0, z0], [x0, y1, z0], [x1, y1, z0]],
        [px(u + dd, v + dd), px(u + dd + w, v + dd + h)],
        0.8,
    );
    quad(
        [[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]],
        [
            px(u + dd + w + dd, v + dd),
            px(u + dd + w + dd + w, v + dd + h),
        ],
        0.8,
    );
}

/// `ItemRenderer.renderItemInFirstPerson`, in view space (camera at the
/// origin looking down -Z). `equip` is the interpolated equip progress and
/// `swing` the swing progress (0..1).
pub fn hand(
    out: &mut Vec<Vertex>,
    atlas: &Atlas,
    held: Option<Stack>,
    equip: f32,
    swing: f32,
    light: (i32, i32),
    using: Option<(crate::item_use::Action, f32, i32)>,
) {
    use crate::item_use::Action;
    let l = light_bytes(light.0, light.1);
    let pi = std::f32::consts::PI;
    let f13 = 0.8f32;
    let f5 = swing;
    match held {
        Some(s) => {
            let mut m;
            if let Some((act, count, max)) = using {
                m = crate::render::IDENTITY;
                if act == Action::Eat || act == Action::Drink {
                    // the item comes up to the mouth and bobs
                    let f34 = count;
                    let f42 = 1.0 - f34 / max as f32;
                    let mut f16 = 1.0 - f42;
                    f16 = f16 * f16 * f16;
                    f16 = f16 * f16 * f16;
                    f16 = f16 * f16 * f16;
                    let f17 = 1.0 - f16;
                    let bob = (f34 / 4.0 * pi).cos().abs()
                        * 0.1
                        * if f42 as f64 > 0.2 { 1.0 } else { 0.0 };
                    m = mul(&m, &translate(0.0, bob, 0.0));
                    m = mul(&m, &translate(f17 * 0.6, -f17 * 0.5, 0.0));
                    m = mul(&m, &rot_y(f17 * 90.0));
                    m = mul(&m, &rot_x(f17 * 10.0));
                    m = mul(&m, &rot_z(f17 * 30.0));
                }
            } else {
                let f6 = (f5 * pi).sin();
                let f7 = (f5.sqrt() * pi).sin();
                m = translate(-f7 * 0.4, (f5.sqrt() * pi * 2.0).sin() * 0.2, -f6 * 0.2);
            }
            m = mul(
                &m,
                &translate(0.7 * f13, -0.65 * f13 - (1.0 - equip) * 0.6, -0.9 * f13),
            );
            m = mul(&m, &rot_y(45.0));
            let f6 = (f5 * f5 * pi).sin();
            let f7 = (f5.sqrt() * pi).sin();
            m = mul(&m, &rot_y(-f6 * 20.0));
            m = mul(&m, &rot_z(-f7 * 20.0));
            m = mul(&m, &rot_x(-f7 * 80.0));
            m = mul(&m, &scale(0.4, 0.4, 0.4));
            if let Some((act, count, max)) = using {
                if act == Action::Block {
                    m = mul(&m, &translate(-0.5, 0.2, 0.0));
                    m = mul(&m, &rot_y(30.0));
                    m = mul(&m, &rot_x(-80.0));
                    m = mul(&m, &rot_y(60.0));
                } else if act == Action::Bow {
                    m = mul(&m, &rot_z(-18.0));
                    m = mul(&m, &rot_y(-12.0));
                    m = mul(&m, &rot_x(-8.0));
                    m = mul(&m, &translate(-0.9, 0.2, 0.0));
                    let f18 = max as f32 - count;
                    let mut f19 = f18 / 20.0;
                    f19 = (f19 * f19 + f19 * 2.0) / 3.0;
                    if f19 > 1.0 {
                        f19 = 1.0;
                    }
                    if f19 > 0.1 {
                        m = mul(
                            &m,
                            &translate(0.0, ((f18 - 0.1) * 1.3).sin() * 0.01 * (f19 - 0.1), 0.0),
                        );
                    }
                    m = mul(&m, &translate(0.0, 0.0, f19 * 0.1));
                    m = mul(&m, &rot_z(-335.0));
                    m = mul(&m, &rot_y(-50.0));
                    m = mul(&m, &translate(0.0, 0.5, 0.0));
                    m = mul(&m, &scale(1.0, 1.0, 1.0 + f19 * 0.2));
                    m = mul(&m, &translate(0.0, -0.5, 0.0));
                    m = mul(&m, &rot_y(50.0));
                    m = mul(&m, &rot_z(335.0));
                }
            }
            // ItemBow.getItemIconForUseDuration
            let mut s = s;
            if let Some((Action::Bow, count, max)) = using {
                let j = max - count as i32 + 1;
                let frame = if j >= 18 {
                    2
                } else if j > 13 {
                    1
                } else if j > 0 {
                    0
                } else {
                    -1
                };
                if frame >= 0 {
                    s.meta = 1000 + frame;
                }
            }
            match s.block().filter(|&b| is_3d(b)) {
                Some(b) => {
                    let h = if block::info(b).shape == Shape::Slab {
                        0.5
                    } else {
                        1.0
                    };
                    let mm = mul(&m, &translate(-0.5, -0.5, -0.5));
                    block_cube(out, atlas, b, s.meta, &mm, l, h);
                }
                None => {
                    let passes: Vec<([f32; 4], [f32; 3])> = if s.block().is_some() || s.id == 261 {
                        match sprite_rect(atlas, &s) {
                            Some(t) => {
                                vec![(t, s.block().map_or([1.0; 3], |b| tint_for(b, s.meta, 0)))]
                            }
                            None => return,
                        }
                    } else {
                        atlas.item_passes(s.id, s.meta)
                    };
                    for (t, tint) in passes {
                        let mut mm = mul(&m, &translate(0.0, -0.3, 0.0));
                        mm = mul(&mm, &scale(1.5, 1.5, 1.5));
                        mm = mul(&mm, &rot_y(50.0));
                        mm = mul(&mm, &rot_z(335.0));
                        mm = mul(&mm, &translate(-0.9375, -0.0625, 0.0));
                        extruded(out, t, &mm, l, tint, 0.0625);
                    }
                }
            }
        }
        None => {
            let f6 = (f5 * pi).sin();
            let f7 = (f5.sqrt() * pi).sin();
            let mut m = translate(-f7 * 0.3, (f5.sqrt() * pi * 2.0).sin() * 0.4, -f6 * 0.4);
            m = mul(
                &m,
                &translate(0.8 * f13, -0.75 * f13 - (1.0 - equip) * 0.6, -0.9 * f13),
            );
            m = mul(&m, &rot_y(45.0));
            let f6 = (f5 * f5 * pi).sin();
            let f7 = (f5.sqrt() * pi).sin();
            m = mul(&m, &rot_y(f7 * 70.0));
            m = mul(&m, &rot_z(-f6 * 20.0));
            m = mul(&m, &translate(-1.0, 3.6, 3.5));
            m = mul(&m, &rot_z(120.0));
            m = mul(&m, &rot_x(200.0));
            m = mul(&m, &rot_y(-135.0));
            m = mul(&m, &translate(5.6, 0.0, 0.0));
            // ModelBiped.bipedRightArm: rotation point (-5, 2, 0), box (-3, -2, -2, 4, 12, 4).
            m = mul(&m, &translate(-5.0 / 16.0, 2.0 / 16.0, 0.0));
            model_box(
                out,
                &m,
                atlas.rects[atlas.skin as usize],
                (40.0, 16.0),
                [-3.0, -2.0, -2.0],
                [4.0, 12.0, 4.0],
                l,
            );
        }
    }
}
