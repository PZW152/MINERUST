//! `ModelRenderer` / `ModelBox` and the 1.7.10 mob models, drawn the way
//! `RendererLivingEntity.doRender` sets them up.

use super::mesher::Vertex;
use super::{mul, rot_x, rot_y, rot_z, transform, translate, Mat4};
use crate::entity::mob::Kind;
use crate::sim::MobView;

/// A texture cell of the atlas: uv rect and the pixel size of the cell.
#[derive(Clone, Copy)]
pub struct Tex {
    pub rect: [f32; 4],
    pub size: f32,
}

#[derive(Clone)]
struct Bx {
    u: f32,
    v: f32,
    o: [f32; 3],
    s: [f32; 3],
    g: f32,
    mirror: bool,
}

#[derive(Clone)]
pub(super) struct Part {
    pub(super) pivot: [f32; 3],
    pub(super) rot: [f32; 3],
    boxes: Vec<Bx>,
    pub(super) children: Vec<Part>,
}

impl Part {
    pub(super) fn new(pivot: [f32; 3]) -> Part {
        Part {
            pivot,
            rot: [0.0; 3],
            boxes: Vec::new(),
            children: Vec::new(),
        }
    }
    pub(super) fn bx(mut self, u: f32, v: f32, o: [f32; 3], s: [f32; 3], g: f32) -> Part {
        self.boxes.push(Bx {
            u,
            v,
            o,
            s,
            g,
            mirror: false,
        });
        self
    }
    pub(super) fn mbx(mut self, u: f32, v: f32, o: [f32; 3], s: [f32; 3], g: f32) -> Part {
        self.boxes.push(Bx {
            u,
            v,
            o,
            s,
            g,
            mirror: true,
        });
        self
    }
    pub(super) fn r(mut self, x: f32, y: f32, z: f32) -> Part {
        self.rot = [x, y, z];
        self
    }
}

pub struct Draw<'a> {
    pub out: &'a mut Vec<Vertex>,
    pub tex: Tex,
    pub light: [u8; 4],
    pub tint: [f32; 4],
}

const L0: [f32; 3] = [0.16169041, 0.80845207, -0.56591643];
const L1: [f32; 3] = [-0.16169041, 0.80845207, 0.56591643];

fn shade(n: [f32; 3]) -> f32 {
    let d0 = (n[0] * L0[0] + n[1] * L0[1] + n[2] * L0[2]).max(0.0);
    let d1 = (n[0] * L1[0] + n[1] * L1[1] + n[2] * L1[2]).max(0.0);
    (0.4 + 0.6 * (d0 + d1)).min(1.0)
}

/// `ModelBox` + `TexturedQuad.draw`, both windings (entities draw unculled).
fn draw_box(d: &mut Draw, m: &Mat4, b: &Bx) {
    let (w, h, dd) = (b.s[0], b.s[1], b.s[2]);
    let (mut x0, y0, z0) = (b.o[0] - b.g, b.o[1] - b.g, b.o[2] - b.g);
    let (mut x1, y1, z1) = (b.o[0] + w + b.g, b.o[1] + h + b.g, b.o[2] + dd + b.g);
    if b.mirror {
        std::mem::swap(&mut x0, &mut x1);
    }
    let v7 = [x0, y0, z0];
    let v0 = [x1, y0, z0];
    let v1 = [x1, y1, z0];
    let v2 = [x0, y1, z0];
    let v3 = [x0, y0, z1];
    let v4 = [x1, y0, z1];
    let v5 = [x1, y1, z1];
    let v6 = [x0, y1, z1];
    let (u, v) = (b.u, b.v);
    let quads = [
        (
            [v4, v0, v1, v5],
            [u + dd + w, v + dd, u + dd + w + dd, v + dd + h],
        ),
        ([v7, v3, v6, v2], [u, v + dd, u + dd, v + dd + h]),
        ([v4, v3, v7, v0], [u + dd, v, u + dd + w, v + dd]),
        ([v1, v2, v6, v5], [u + dd + w, v + dd, u + dd + w + w, v]),
        ([v0, v7, v2, v1], [u + dd, v + dd, u + dd + w, v + dd + h]),
        (
            [v3, v4, v5, v6],
            [u + dd + w + dd, v + dd, u + dd + w + dd + w, v + dd + h],
        ),
    ];
    let t = d.tex;
    let px = |a: f32, bb: f32| {
        [
            t.rect[0] + (t.rect[2] - t.rect[0]) * a / t.size,
            t.rect[1] + (t.rect[3] - t.rect[1]) * bb / t.size,
        ]
    };
    let s16 = 1.0 / 16.0;
    let cc = transform(
        m,
        [
            (x0 + x1) * 0.5 * s16,
            (y0 + y1) * 0.5 * s16,
            (z0 + z1) * 0.5 * s16,
            1.0,
        ],
    );
    let centre = [cc[0], cc[1], cc[2]];
    for (mut p, r) in quads {
        let mut uv = [
            px(r[2], r[1]),
            px(r[0], r[1]),
            px(r[0], r[3]),
            px(r[2], r[3]),
        ];
        if b.mirror {
            p.reverse();
            uv.reverse();
        }
        let s = 1.0 / 16.0;
        let q: Vec<[f32; 4]> = p
            .iter()
            .map(|c| transform(m, [c[0] * s, c[1] * s, c[2] * s, 1.0]))
            .collect();
        let e1 = [q[1][0] - q[0][0], q[1][1] - q[0][1], q[1][2] - q[0][2]];
        let e2 = [q[1][0] - q[2][0], q[1][1] - q[2][1], q[1][2] - q[2][2]];
        let mut n = [
            e2[1] * e1[2] - e2[2] * e1[1],
            e2[2] * e1[0] - e2[0] * e1[2],
            e2[0] * e1[1] - e2[1] * e1[0],
        ];
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if l < 1e-9 {
            continue;
        }
        for c in &mut n {
            *c /= l;
        }
        // orient the normal away from the box centre (both windings are drawn)
        let qc = [
            (q[0][0] + q[2][0]) * 0.5 - centre[0],
            (q[0][1] + q[2][1]) * 0.5 - centre[1],
            (q[0][2] + q[2][2]) * 0.5 - centre[2],
        ];
        if n[0] * qc[0] + n[1] * qc[1] + n[2] * qc[2] < 0.0 {
            n = [-n[0], -n[1], -n[2]];
        }
        let k = shade(n);
        let c = [
            (d.tint[0] * k * 255.0) as u8,
            (d.tint[1] * k * 255.0) as u8,
            (d.tint[2] * k * 255.0) as u8,
            (d.tint[3] * 255.0) as u8,
        ];
        let vs: Vec<Vertex> = (0..4)
            .map(|i| Vertex {
                pos: [q[i][0], q[i][1], q[i][2]],
                uv: uv[i],
                color: c,
                light: d.light,
            })
            .collect();
        d.out.extend_from_slice(&vs);
        d.out.extend_from_slice(&[vs[3], vs[2], vs[1], vs[0]]);
    }
}

/// `ModelRenderer.render`
pub(super) fn render(d: &mut Draw, parent: &Mat4, p: &Part) {
    let mut m = mul(
        parent,
        &translate(p.pivot[0] / 16.0, p.pivot[1] / 16.0, p.pivot[2] / 16.0),
    );
    let deg = 180.0 / std::f32::consts::PI;
    if p.rot[2] != 0.0 {
        m = mul(&m, &rot_z(p.rot[2] * deg));
    }
    if p.rot[1] != 0.0 {
        m = mul(&m, &rot_y(p.rot[1] * deg));
    }
    if p.rot[0] != 0.0 {
        m = mul(&m, &rot_x(p.rot[0] * deg));
    }
    for b in &p.boxes {
        draw_box(d, &m, b);
    }
    for c in &p.children {
        render(d, &m, c);
    }
}

pub(super) fn cos(x: f32) -> f32 {
    crate::util::math::cos(x)
}
pub(super) fn sin(x: f32) -> f32 {
    crate::util::math::sin(x)
}
pub(super) const PI: f32 = std::f32::consts::PI;

/// Animation inputs of `ModelBase.render`.
pub struct Anim {
    pub swing: f32,
    pub amount: f32,
    pub age: f32,
    pub head_yaw: f32,
    pub head_pitch: f32,
    pub attack: f32,
    pub wing: f32,
    pub sheep_head_y: f32,
    pub sheep_head_rot: Option<f32>,
}

fn quadruped(legh: f32, a: &Anim) -> Vec<Part> {
    let lh = legh;
    let mut v = vec![
        Part::new([0.0, 18.0 - lh, -6.0]).bx(0.0, 0.0, [-4.0, -4.0, -8.0], [8.0, 8.0, 8.0], 0.0),
        Part::new([0.0, 17.0 - lh, 2.0]).bx(28.0, 8.0, [-5.0, -10.0, -7.0], [10.0, 16.0, 8.0], 0.0),
    ];
    for (x, z) in [(-3.0, 7.0), (3.0, 7.0), (-3.0, -5.0), (3.0, -5.0)] {
        v.push(Part::new([x, 24.0 - lh, z]).bx(0.0, 16.0, [-2.0, 0.0, -2.0], [4.0, lh, 4.0], 0.0));
    }
    quad_angles(&mut v, a);
    v
}

fn quad_angles(v: &mut [Part], a: &Anim) {
    v[0].rot[0] = a.head_pitch / (180.0 / PI);
    v[0].rot[1] = a.head_yaw / (180.0 / PI);
    v[1].rot[0] = PI / 2.0;
    v[2].rot[0] = cos(a.swing * 0.6662) * 1.4 * a.amount;
    v[3].rot[0] = cos(a.swing * 0.6662 + PI) * 1.4 * a.amount;
    v[4].rot[0] = cos(a.swing * 0.6662 + PI) * 1.4 * a.amount;
    v[5].rot[0] = cos(a.swing * 0.6662) * 1.4 * a.amount;
}

/// `ModelBiped` with the zombie arm pose when `zombie_arms`.
fn biped(a: &Anim, thin: bool, zombie_arms: bool) -> Vec<Part> {
    let (aw, ao) = if thin { (2.0, -1.0) } else { (4.0, -2.0) };
    let head = Part::new([0.0, 0.0, 0.0]).bx(0.0, 0.0, [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], 0.0);
    let hat = Part::new([0.0, 0.0, 0.0]).bx(32.0, 0.0, [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], 0.5);
    let body = Part::new([0.0, 0.0, 0.0]).bx(16.0, 16.0, [-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], 0.0);
    let rarm = Part::new([-5.0, 2.0, 0.0]).bx(
        40.0,
        16.0,
        [if thin { -1.0 } else { -3.0 }, -2.0, ao],
        [aw, 12.0, aw],
        0.0,
    );
    let larm = Part::new([5.0, 2.0, 0.0]).mbx(40.0, 16.0, [-1.0, -2.0, ao], [aw, 12.0, aw], 0.0);
    let lx = if thin { 2.0 } else { 1.9 };
    let rleg = Part::new([-lx, 12.0, 0.1]).bx(0.0, 16.0, [ao, 0.0, ao], [aw, 12.0, aw], 0.0);
    let lleg = Part::new([lx, 12.0, 0.1]).mbx(0.0, 16.0, [ao, 0.0, ao], [aw, 12.0, aw], 0.0);
    let mut v = vec![head, body, rarm, larm, rleg, lleg, hat];
    let hy = a.head_yaw / (180.0 / PI);
    let hp = a.head_pitch / (180.0 / PI);
    v[0].rot = [hp, hy, 0.0];
    v[6].rot = [hp, hy, 0.0];
    v[2].rot[0] = cos(a.swing * 0.6662 + PI) * 2.0 * a.amount * 0.5;
    v[3].rot[0] = cos(a.swing * 0.6662) * 2.0 * a.amount * 0.5;
    v[4].rot[0] = cos(a.swing * 0.6662) * 1.4 * a.amount;
    v[5].rot[0] = cos(a.swing * 0.6662 + PI) * 1.4 * a.amount;
    if zombie_arms {
        let f8 = sin(a.attack * PI);
        let f9 = sin((1.0 - (1.0 - a.attack) * (1.0 - a.attack)) * PI);
        v[2].rot = [-PI / 2.0 - (f8 * 1.2 - f9 * 0.4), -(0.1 - f8 * 0.6), 0.0];
        v[3].rot = [-PI / 2.0 - (f8 * 1.2 - f9 * 0.4), 0.1 - f8 * 0.6, 0.0];
    }
    v[2].rot[2] += cos(a.age * 0.09) * 0.05 + 0.05;
    v[3].rot[2] -= cos(a.age * 0.09) * 0.05 + 0.05;
    v[2].rot[0] += sin(a.age * 0.067) * 0.05;
    v[3].rot[0] -= sin(a.age * 0.067) * 0.05;
    v
}

fn villager(a: &Anim, witch: bool, ticks: f32, id: u32) -> Vec<Part> {
    let mut nose =
        Part::new([0.0, -2.0, 0.0]).bx(24.0, 0.0, [-1.0, -1.0, -6.0], [2.0, 4.0, 2.0], 0.0);
    let mut head =
        Part::new([0.0, 0.0, 0.0]).bx(0.0, 0.0, [-4.0, -10.0, -4.0], [8.0, 10.0, 8.0], 0.0);
    if witch {
        nose.children.push(Part::new([0.0, -2.0, 0.0]).bx(
            0.0,
            0.0,
            [0.0, 3.0, -6.75],
            [1.0, 1.0, 1.0],
            -0.25,
        ));
        let f = 0.01 * (id % 10) as f32;
        nose.rot = [
            sin(ticks * f) * 4.5 * PI / 180.0,
            0.0,
            cos(ticks * f) * 2.5 * PI / 180.0,
        ];
        let tip = Part::new([1.75, -2.0, 2.0])
            .bx(0.0, 95.0, [0.0, 0.0, 0.0], [1.0, 2.0, 1.0], 0.25)
            .r(-PI / 15.0, 0.0, 0.10471976);
        let mut mid = Part::new([1.75, -4.0, 2.0])
            .bx(0.0, 87.0, [0.0, 0.0, 0.0], [4.0, 4.0, 4.0], 0.0)
            .r(-0.10471976, 0.0, 0.05235988);
        mid.children.push(tip);
        let mut low = Part::new([1.75, -4.0, 2.0])
            .bx(0.0, 76.0, [0.0, 0.0, 0.0], [7.0, 4.0, 7.0], 0.0)
            .r(-0.05235988, 0.0, 0.02617994);
        low.children.push(mid);
        let mut hat = Part::new([-5.0, -10.03125, -5.0]).bx(
            0.0,
            64.0,
            [0.0, 0.0, 0.0],
            [10.0, 2.0, 10.0],
            0.0,
        );
        hat.children.push(low);
        head.children.push(hat);
    }
    head.children.push(nose);
    head.rot = [a.head_pitch / (180.0 / PI), a.head_yaw / (180.0 / PI), 0.0];
    let body = Part::new([0.0, 0.0, 0.0])
        .bx(16.0, 20.0, [-4.0, 0.0, -3.0], [8.0, 12.0, 6.0], 0.0)
        .bx(0.0, 38.0, [-4.0, 0.0, -3.0], [8.0, 18.0, 6.0], 0.5);
    let arms = Part::new([0.0, 3.0, -1.0])
        .bx(44.0, 22.0, [-8.0, -2.0, -2.0], [4.0, 8.0, 4.0], 0.0)
        .bx(44.0, 22.0, [4.0, -2.0, -2.0], [4.0, 8.0, 4.0], 0.0)
        .bx(40.0, 38.0, [-4.0, 2.0, -2.0], [8.0, 4.0, 4.0], 0.0)
        .r(-0.75, 0.0, 0.0);
    let rl = Part::new([-2.0, 12.0, 0.0])
        .bx(0.0, 22.0, [-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], 0.0)
        .r(cos(a.swing * 0.6662) * 1.4 * a.amount * 0.5, 0.0, 0.0);
    let ll = Part::new([2.0, 12.0, 0.0])
        .mbx(0.0, 22.0, [-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], 0.0)
        .r(cos(a.swing * 0.6662 + PI) * 1.4 * a.amount * 0.5, 0.0, 0.0);
    vec![head, body, rl, ll, arms]
}

fn parts(mv: &MobView, a: &Anim, ticks: f32, layer: u8, partial: f32) -> Vec<Part> {
    let id = mv.id;
    match mv.kind {
        Kind::Slime
        | Kind::Enderman
        | Kind::Squid
        | Kind::Bat
        | Kind::Wolf
        | Kind::Ocelot
        | Kind::Horse
        | Kind::IronGolem => super::model2::parts(mv, a, layer, partial),
        Kind::Ghast => super::model3::ghast(ticks),
        Kind::Blaze => super::model3::blaze(a),
        Kind::MagmaCube => {
            super::model3::magma_cube(mv.squish[0] + (mv.squish[1] - mv.squish[0]) * partial)
        }
        Kind::PigZombie => biped(a, false, true),
        Kind::EnderDragon | Kind::EnderCrystal => Vec::new(),
        Kind::Pig => {
            let mut v = quadruped(6.0, a);
            v[0] = v[0]
                .clone()
                .bx(16.0, 16.0, [-2.0, 0.0, -9.0], [4.0, 3.0, 1.0], 0.0);
            v
        }
        Kind::Cow | Kind::Mooshroom => {
            let mut v = quadruped(12.0, a);
            v[0] = Part::new([0.0, 4.0, -8.0])
                .bx(0.0, 0.0, [-4.0, -4.0, -6.0], [8.0, 8.0, 6.0], 0.0)
                .bx(22.0, 0.0, [-5.0, -5.0, -4.0], [1.0, 3.0, 1.0], 0.0)
                .bx(22.0, 0.0, [4.0, -5.0, -4.0], [1.0, 3.0, 1.0], 0.0);
            v[1] = Part::new([0.0, 5.0, 2.0])
                .bx(18.0, 4.0, [-6.0, -10.0, -7.0], [12.0, 18.0, 10.0], 0.0)
                .bx(52.0, 0.0, [-2.0, 2.0, -8.0], [4.0, 6.0, 1.0], 0.0);
            v[2].pivot[0] -= 1.0;
            v[3].pivot[0] += 1.0;
            v[4].pivot[0] -= 1.0;
            v[5].pivot[0] += 1.0;
            v[4].pivot[2] -= 1.0;
            v[5].pivot[2] -= 1.0;
            quad_angles(&mut v, a);
            v
        }
        Kind::Sheep => {
            let mut v = quadruped(12.0, a);
            if layer == 1 {
                // ModelSheep1: the fleece
                v[0] = Part::new([0.0, 6.0, -8.0]).bx(
                    0.0,
                    0.0,
                    [-3.0, -4.0, -4.0],
                    [6.0, 6.0, 6.0],
                    0.6,
                );
                v[1] = Part::new([0.0, 5.0, 2.0]).bx(
                    28.0,
                    8.0,
                    [-4.0, -10.0, -7.0],
                    [8.0, 16.0, 6.0],
                    1.75,
                );
                for (i, (x, z)) in [(-3.0, 7.0), (3.0, 7.0), (-3.0, -5.0), (3.0, -5.0)]
                    .iter()
                    .enumerate()
                {
                    v[2 + i] = Part::new([*x, 12.0, *z]).bx(
                        0.0,
                        16.0,
                        [-2.0, 0.0, -2.0],
                        [4.0, 6.0, 4.0],
                        0.5,
                    );
                }
            } else {
                v[0] = Part::new([0.0, 6.0, -8.0]).bx(
                    0.0,
                    0.0,
                    [-3.0, -4.0, -6.0],
                    [6.0, 6.0, 8.0],
                    0.0,
                );
                v[1] = Part::new([0.0, 5.0, 2.0]).bx(
                    28.0,
                    8.0,
                    [-4.0, -10.0, -7.0],
                    [8.0, 16.0, 6.0],
                    0.0,
                );
            }
            quad_angles(&mut v, a);
            v[0].pivot[1] = 6.0 + a.sheep_head_y * 9.0;
            if let Some(r) = a.sheep_head_rot {
                v[0].rot[0] = r;
            }
            v
        }
        Kind::Chicken => {
            let hr = [a.head_pitch / (180.0 / PI), a.head_yaw / (180.0 / PI), 0.0];
            let mut head =
                Part::new([0.0, 15.0, -4.0]).bx(0.0, 0.0, [-2.0, -6.0, -2.0], [4.0, 6.0, 3.0], 0.0);
            head.rot = hr;
            let mut bill = Part::new([0.0, 15.0, -4.0]).bx(
                14.0,
                0.0,
                [-2.0, -4.0, -4.0],
                [4.0, 2.0, 2.0],
                0.0,
            );
            bill.rot = hr;
            let mut chin = Part::new([0.0, 15.0, -4.0]).bx(
                14.0,
                4.0,
                [-1.0, -2.0, -3.0],
                [2.0, 2.0, 2.0],
                0.0,
            );
            chin.rot = hr;
            vec![
                head,
                bill,
                chin,
                Part::new([0.0, 16.0, 0.0])
                    .bx(0.0, 9.0, [-3.0, -4.0, -3.0], [6.0, 8.0, 6.0], 0.0)
                    .r(PI / 2.0, 0.0, 0.0),
                Part::new([-2.0, 19.0, 1.0])
                    .bx(26.0, 0.0, [-1.0, 0.0, -3.0], [3.0, 5.0, 3.0], 0.0)
                    .r(cos(a.swing * 0.6662) * 1.4 * a.amount, 0.0, 0.0),
                Part::new([1.0, 19.0, 1.0])
                    .bx(26.0, 0.0, [-1.0, 0.0, -3.0], [3.0, 5.0, 3.0], 0.0)
                    .r(cos(a.swing * 0.6662 + PI) * 1.4 * a.amount, 0.0, 0.0),
                Part::new([-4.0, 13.0, 0.0])
                    .bx(24.0, 13.0, [0.0, 0.0, -3.0], [1.0, 4.0, 6.0], 0.0)
                    .r(0.0, 0.0, a.wing),
                Part::new([4.0, 13.0, 0.0])
                    .bx(24.0, 13.0, [-1.0, 0.0, -3.0], [1.0, 4.0, 6.0], 0.0)
                    .r(0.0, 0.0, -a.wing),
            ]
        }
        Kind::Zombie => biped(a, false, true),
        Kind::Skeleton => biped(a, mv.variant != 1, true),
        Kind::Creeper => {
            let hr = [a.head_pitch / (180.0 / PI), a.head_yaw / (180.0 / PI), 0.0];
            let mut head =
                Part::new([0.0, 4.0, 0.0]).bx(0.0, 0.0, [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], 0.0);
            head.rot = hr;
            let mut v = vec![
                head,
                Part::new([0.0, 4.0, 0.0]).bx(16.0, 16.0, [-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], 0.0),
            ];
            for (x, z, ph) in [
                (-2.0, 4.0, 0.0),
                (2.0, 4.0, PI),
                (-2.0, -4.0, PI),
                (2.0, -4.0, 0.0),
            ] {
                v.push(
                    Part::new([x, 16.0, z])
                        .bx(0.0, 16.0, [-2.0, 0.0, -2.0], [4.0, 6.0, 4.0], 0.0)
                        .r(cos(a.swing * 0.6662 + ph) * 1.4 * a.amount, 0.0, 0.0),
                );
            }
            v
        }
        Kind::Spider | Kind::CaveSpider => {
            let hr = [a.head_pitch / (180.0 / PI), a.head_yaw / (180.0 / PI), 0.0];
            let mut head = Part::new([0.0, 15.0, -3.0]).bx(
                32.0,
                4.0,
                [-4.0, -4.0, -8.0],
                [8.0, 8.0, 8.0],
                0.0,
            );
            head.rot = hr;
            let mut v = vec![
                head,
                Part::new([0.0, 15.0, 0.0]).bx(0.0, 0.0, [-3.0, -3.0, -3.0], [6.0, 6.0, 6.0], 0.0),
                Part::new([0.0, 15.0, 9.0]).bx(
                    0.0,
                    12.0,
                    [-5.0, -4.0, -6.0],
                    [10.0, 8.0, 12.0],
                    0.0,
                ),
            ];
            let q = PI / 4.0;
            let e = PI / 8.0;
            let zs = [2.0, 2.0, 1.0, 1.0, 0.0, 0.0, -1.0, -1.0];
            let rz = [-q, q, -q * 0.74, q * 0.74, -q * 0.74, q * 0.74, -q, q];
            let ry = [e * 2.0, -e * 2.0, e, -e, -e, e, -e * 2.0, e * 2.0];
            let s = a.swing * 0.6662;
            let yw = [
                -(cos(s * 2.0) * 0.4) * a.amount,
                -(cos(s * 2.0 + PI) * 0.4) * a.amount,
                -(cos(s * 2.0 + PI / 2.0) * 0.4) * a.amount,
                -(cos(s * 2.0 + PI * 1.5) * 0.4) * a.amount,
            ];
            let zw = [
                (sin(s) * 0.4).abs() * a.amount,
                (sin(s + PI) * 0.4).abs() * a.amount,
                (sin(s + PI / 2.0) * 0.4).abs() * a.amount,
                (sin(s + PI * 1.5) * 0.4).abs() * a.amount,
            ];
            for i in 0..8 {
                let left = i % 2 == 0;
                let sign = if left { 1.0 } else { -1.0 };
                let p = Part::new([if left { -4.0 } else { 4.0 }, 15.0, zs[i]])
                    .bx(
                        18.0,
                        0.0,
                        [if left { -15.0 } else { -1.0 }, -1.0, -1.0],
                        [16.0, 2.0, 2.0],
                        0.0,
                    )
                    .r(0.0, ry[i] + yw[i / 2] * sign, rz[i] + zw[i / 2] * sign);
                v.push(p);
            }
            v
        }
        Kind::Villager => villager(a, false, ticks, id),
        Kind::Witch => villager(a, true, ticks, id),
        Kind::Silverfish => {
            let len = [
                [3.0, 2.0, 2.0],
                [4.0, 3.0, 2.0],
                [6.0, 4.0, 3.0],
                [3.0, 3.0, 3.0],
                [2.0, 2.0, 3.0],
                [2.0, 1.0, 2.0],
                [1.0, 1.0, 2.0],
            ];
            let tp = [
                [0.0, 0.0],
                [0.0, 4.0],
                [0.0, 9.0],
                [0.0, 16.0],
                [0.0, 22.0],
                [11.0, 0.0],
                [13.0, 4.0],
            ];
            let mut z = -3.5f32;
            let mut zp = [0.0f32; 7];
            let mut v = Vec::new();
            for i in 0..7 {
                let l: [f32; 3] = len[i];
                let mut p = Part::new([0.0, 24.0 - l[1], z]).bx(
                    tp[i][0],
                    tp[i][1],
                    [l[0] * -0.5, 0.0, l[2] * -0.5],
                    l,
                    0.0,
                );
                p.rot[1] = cos(a.age * 0.9 + i as f32 * 0.15 * PI)
                    * PI
                    * 0.05
                    * (1 + (i as i32 - 2).abs()) as f32;
                p.pivot[0] = sin(a.age * 0.9 + i as f32 * 0.15 * PI)
                    * PI
                    * 0.2
                    * (i as i32 - 2).abs() as f32;
                zp[i] = z;
                if i < 6 {
                    z += (l[2] + len[i + 1][2]) * 0.5;
                }
                v.push(p);
            }
            let (r2, r4, r1) = (v[2].rot[1], v[4].rot[1], v[1].rot[1]);
            let (p4, p1) = (v[4].pivot[0], v[1].pivot[0]);
            v.push(
                Part::new([0.0, 16.0, zp[2]])
                    .bx(20.0, 0.0, [-5.0, 0.0, -1.5], [10.0, 8.0, 3.0], 0.0)
                    .r(0.0, r2, 0.0),
            );
            v.push(
                Part::new([p4, 20.0, zp[4]])
                    .bx(20.0, 11.0, [-3.0, 0.0, -1.5], [6.0, 4.0, 3.0], 0.0)
                    .r(0.0, r4, 0.0),
            );
            v.push(
                Part::new([p1, 19.0, zp[1]])
                    .bx(20.0, 18.0, [-3.0, 0.0, -1.5], [6.0, 5.0, 2.0], 0.0)
                    .r(0.0, r1, 0.0),
            );
            v
        }
    }
}

/// Atlas key of the texture a mob is drawn with.
pub fn texture(kind: Kind, variant: i32, layer: u8) -> &'static str {
    match kind {
        Kind::Pig => "pig/pig",
        Kind::Cow => "cow/cow",
        Kind::Mooshroom => "cow/mooshroom",
        Kind::Sheep => {
            if layer == 1 {
                "sheep/sheep_fur"
            } else {
                "sheep/sheep"
            }
        }
        Kind::Chicken => "chicken",
        Kind::Zombie => "zombie/zombie",
        Kind::Skeleton if variant == 1 => "skeleton/wither_skeleton",
        Kind::Skeleton => "skeleton/skeleton",
        Kind::Ghast => {
            if layer == 1 {
                "ghast/ghast_shooting"
            } else {
                "ghast/ghast"
            }
        }
        Kind::Blaze => "blaze",
        Kind::MagmaCube => "slime/magmacube",
        Kind::PigZombie => "zombie_pigman",
        Kind::EnderDragon => {
            if layer == 1 {
                "enderdragon/dragon_eyes"
            } else {
                "enderdragon/dragon"
            }
        }
        Kind::EnderCrystal => "endercrystal/endercrystal",
        Kind::Creeper => "creeper/creeper",
        Kind::Spider => "spider/spider",
        Kind::CaveSpider => "spider/cave_spider",
        Kind::Villager => [
            "villager/farmer",
            "villager/librarian",
            "villager/priest",
            "villager/smith",
            "villager/butcher",
        ][variant.clamp(0, 4) as usize],
        Kind::Witch => "witch",
        Kind::Silverfish => "silverfish",
        _ => "pig/pig",
    }
}

pub const TEXTURES: [&str; 25] = [
    "../particle/particles",
    "explosion",
    "../misc/underwater",
    "../environment/rain",
    "../environment/snow",
    "pig/pig",
    "cow/cow",
    "cow/mooshroom",
    "sheep/sheep",
    "sheep/sheep_fur",
    "chicken",
    "zombie/zombie",
    "skeleton/skeleton",
    "creeper/creeper",
    "spider/spider",
    "spider/cave_spider",
    "villager/farmer",
    "villager/librarian",
    "villager/priest",
    "villager/smith",
    "villager/butcher",
    "witch",
    "silverfish",
    "arrow",
    "minecart",
];

/// `EntitySheep.fleeceColorTable`
pub const FLEECE: [[f32; 3]; 16] = [
    [1.0, 1.0, 1.0],
    [0.85, 0.5, 0.2],
    [0.7, 0.3, 0.85],
    [0.4, 0.6, 0.85],
    [0.9, 0.9, 0.2],
    [0.5, 0.8, 0.1],
    [0.95, 0.5, 0.65],
    [0.3, 0.3, 0.3],
    [0.6, 0.6, 0.6],
    [0.3, 0.5, 0.6],
    [0.5, 0.25, 0.7],
    [0.2, 0.3, 0.7],
    [0.4, 0.3, 0.2],
    [0.4, 0.5, 0.2],
    [0.6, 0.2, 0.2],
    [0.1, 0.1, 0.1],
];

fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let mut d = b - a;
    while d < -180.0 {
        d += 360.0;
    }
    while d >= 180.0 {
        d -= 360.0;
    }
    a + t * d
}

/// `RendererLivingEntity.doRender` for one mob, at `rel` (camera-relative feet).
#[allow(clippy::too_many_arguments)]
pub fn mob(
    out: &mut Vec<Vertex>,
    translucent: &mut Vec<Vertex>,
    tex: &dyn Fn(&str) -> Option<Tex>,
    block: &dyn Fn(&mut Vec<Vertex>, u8, u8, &Mat4, [u8; 4]),
    m: &MobView,
    rel: [f32; 3],
    partial: f32,
    light: [u8; 4],
) {
    let body = lerp_angle(m.prev_body_yaw, m.body_yaw, partial);
    let head = lerp_angle(m.prev_head_yaw, m.head_yaw, partial);
    let pitch = m.prev_pitch + (m.pitch - m.prev_pitch) * partial;
    let age = m.ticks as f32 + partial;
    let mut mat = translate(rel[0], rel[1], rel[2]);
    match m.kind {
        Kind::Squid => {
            // RenderSquid.rotateCorpse
            let sp = m.sq[0][0] + (m.sq[1][0] - m.sq[0][0]) * partial;
            let sy = m.sq[0][1] + (m.sq[1][1] - m.sq[0][1]) * partial;
            mat = mul(&mat, &translate(0.0, 0.5, 0.0));
            mat = mul(&mat, &rot_y(180.0 - body));
            mat = mul(&mat, &super::rot_x(sp));
            mat = mul(&mat, &rot_y(sy));
            mat = mul(&mat, &translate(0.0, -1.2, 0.0));
        }
        Kind::Bat => {
            // RenderBat.rotateCorpse
            let dy = if m.flags & 8 != 0 {
                -0.1
            } else {
                cos(age * 0.3) * 0.1
            };
            mat = mul(&mat, &translate(0.0, dy, 0.0));
            mat = mul(&mat, &rot_y(180.0 - body));
        }
        _ => mat = mul(&mat, &rot_y(180.0 - body)),
    }
    if m.kind == Kind::EnderCrystal {
        // RenderEnderCrystal: not a living renderer at all
        let Some(t) = tex("endercrystal/endercrystal") else {
            return;
        };
        let mut d = Draw {
            out,
            tex: t,
            light,
            tint: [1.0; 4],
        };
        super::model3::crystal(&mut d, rel, m.sq[1][2] + partial);
        return;
    }
    if m.kind == Kind::EnderDragon {
        // RenderDragon.rotateCorpse
        let f3 = super::model3::ring(m, 7, partial)[0] as f32;
        let f4 =
            (super::model3::ring(m, 5, partial)[1] - super::model3::ring(m, 10, partial)[1]) as f32;
        mat = translate(rel[0], rel[1], rel[2]);
        mat = mul(&mat, &rot_y(-f3));
        mat = mul(&mat, &super::rot_x(f4 * 10.0));
        mat = mul(&mat, &translate(0.0, 0.0, 1.0));
        mat = mul(&mat, &super::entities::scale(-1.0, -1.0, 1.0));
        mat = mul(&mat, &translate(0.0, -24.0 / 16.0 - 0.0078125, 0.0));
        let hurt = m.hurt_time > 0;
        for layer in [0u8, 1] {
            let name = if layer == 0 {
                "enderdragon/dragon"
            } else {
                "enderdragon/dragon_eyes"
            };
            let Some(t) = tex(name) else { continue };
            let mut d = Draw {
                out,
                tex: t,
                light: if layer == 1 { [240, 240, 0, 0] } else { light },
                tint: if hurt && layer == 0 {
                    [1.0, 0.6, 0.6, 1.0]
                } else {
                    [1.0; 4]
                },
            };
            super::model3::dragon(&mut d, &mat, m, partial);
        }
        return;
    }
    if m.kind == Kind::IronGolem && m.limb_amount >= 0.01 {
        // RenderIronGolem.rotateCorpse: the swaying walk
        let p = 13.0f32;
        let f5 = m.limb_swing - m.limb_amount * (1.0 - partial) + 6.0;
        let f6 = ((f5 % p - p * 0.5).abs() - p * 0.25) / (p * 0.25);
        mat = mul(&mat, &rot_z(6.5 * f6));
    }
    if m.death_time > 0 && m.kind != Kind::EnderDragon {
        let f = ((m.death_time as f32 + partial - 1.0) / 20.0 * 1.6)
            .sqrt()
            .min(1.0);
        mat = mul(&mat, &rot_z(f * 90.0));
    }
    mat = mul(&mat, &super::entities::scale(-1.0, -1.0, 1.0));
    // preRenderCallback
    match m.kind {
        Kind::CaveSpider => mat = mul(&mat, &super::entities::scale(0.7, 0.7, 0.7)),
        Kind::Creeper => {
            let mut f = (m.prev_fuse as f32 + (m.fuse - m.prev_fuse) as f32 * partial) / 28.0;
            f = f.clamp(0.0, 1.0);
            let f2 = 1.0 + sin(f * 100.0) * f * 0.01;
            let f = f * f * f * f;
            let xz = (1.0 + f * 0.4) * f2;
            let y = (1.0 + f * 0.1) / f2;
            mat = mul(&mat, &super::entities::scale(xz, y, xz));
        }
        Kind::Witch | Kind::Villager => {
            mat = mul(&mat, &super::entities::scale(0.9375, 0.9375, 0.9375))
        }
        Kind::Slime => {
            // RenderSlime.preRenderCallback
            let size = m.variant.max(1) as f32;
            let sq = (m.squish[0] + (m.squish[1] - m.squish[0]) * partial) / (size * 0.5 + 1.0);
            let f3 = 1.0 / (sq + 1.0);
            mat = mul(
                &mat,
                &super::entities::scale(f3 * size, 1.0 / f3 * size, f3 * size),
            );
        }
        Kind::Bat => mat = mul(&mat, &super::entities::scale(0.35, 0.35, 0.35)),
        // RenderSkeleton: wither skeletons are 1.2 times larger
        Kind::Skeleton if m.variant == 1 => mat = mul(&mat, &super::entities::scale(1.2, 1.2, 1.2)),
        Kind::Ghast => {
            // RenderGhast.preRenderCallback: swells before shooting
            let mut f = (m.prev_fuse as f32 + (m.fuse - m.prev_fuse) as f32 * partial) / 20.0;
            if f < 0.0 {
                f = 0.0;
            }
            let f = 1.0 / (f * f * f * f * f * 2.0 + 1.0);
            let y = (8.0 + f) / 2.0;
            let xz = (8.0 + 1.0 / f) / 2.0;
            mat = mul(&mat, &super::entities::scale(xz, y, xz));
        }
        Kind::MagmaCube => {
            let size = m.variant.max(1) as f32;
            let sq = (m.squish[0] + (m.squish[1] - m.squish[0]) * partial) / (size * 0.5 + 1.0);
            let f = 1.0 / (sq + 1.0);
            mat = mul(
                &mat,
                &super::entities::scale(f * size, 1.0 / f * size, f * size),
            );
        }
        Kind::Ocelot if m.flags & 1 != 0 => mat = mul(&mat, &super::entities::scale(0.8, 0.8, 0.8)),
        Kind::Horse => {
            let f = match m.variant >> 16 {
                1 => 0.87,
                2 => 0.92,
                _ => 1.0,
            };
            mat = mul(&mat, &super::entities::scale(f, f, f));
        }
        _ => {}
    }
    mat = mul(&mat, &translate(0.0, -24.0 / 16.0 - 0.0078125, 0.0));
    let amount = (m.prev_limb_amount + (m.limb_amount - m.prev_limb_amount) * partial).min(1.0);
    let swing = m.limb_swing - m.limb_amount * (1.0 - partial);
    let wing = m.prev_wing + (m.wing - m.prev_wing) * partial;
    // EntitySheep head while eating
    let (shy, shr) = if m.kind == Kind::Sheep {
        let t = m.eat_timer as f32;
        let y = if t <= 0.0 {
            0.0
        } else if t >= 4.0 && t <= 36.0 {
            1.0
        } else if t < 4.0 {
            (t - partial) / 4.0
        } else {
            -(t - 40.0 - partial) / 4.0
        };
        let r = if t > 4.0 && t <= 36.0 {
            let f = (t - 4.0 - partial) / 32.0;
            Some(PI / 5.0 + PI * 7.0 / 100.0 * sin(f * 28.7))
        } else if t > 0.0 {
            Some(PI / 5.0)
        } else {
            None
        };
        (y, r)
    } else {
        (0.0, None)
    };
    let a = Anim {
        swing,
        amount,
        age,
        head_yaw: head - body,
        head_pitch: pitch,
        attack: m.prev_swing + (m.swing - m.prev_swing) * partial,
        wing: (sin(wing) + 1.0) * 1.0,
        sheep_head_y: shy,
        sheep_head_rot: shr.or(if m.kind == Kind::Sheep {
            Some(pitch / (180.0 / PI))
        } else {
            None
        }),
    };
    let hurt = m.hurt_time > 0 || m.death_time > 0;
    let tint = |c: [f32; 3]| {
        if hurt {
            [c[0] * 0.6 + 0.4, c[1] * 0.6, c[2] * 0.6, 1.0]
        } else {
            [c[0], c[1], c[2], 1.0]
        }
    };
    let newer = matches!(
        m.kind,
        Kind::Slime
            | Kind::Enderman
            | Kind::Squid
            | Kind::Bat
            | Kind::Wolf
            | Kind::Ocelot
            | Kind::Horse
            | Kind::IronGolem
    );
    let layers: &[u8] = if m.kind == Kind::Ghast && m.flags & 4 != 0 {
        &[1]
    } else if newer {
        super::model2::layers(m)
    } else if m.kind == Kind::Sheep && !m.sheared {
        &[0, 1]
    } else {
        &[0]
    };
    for &layer in layers {
        let name = if newer {
            match super::model2::texture(m, layer) {
                Some(n) => n,
                None => continue,
            }
        } else {
            texture(m.kind, m.variant, layer)
        };
        let Some(t) = tex(name) else {
            continue;
        };
        let col = if layer == 1 && m.kind == Kind::Sheep {
            FLEECE[(m.variant & 15) as usize]
        } else if layer == 1 && m.kind == Kind::Wolf {
            // the collar takes the dye colour
            FLEECE[(m.variant & 15) as usize]
        } else {
            [1.0; 3]
        };
        // RenderEnderman: the eyes glow at full brightness; blazes and
        // magma cubes are always lit (getBrightnessForRender).
        let lt = if (m.kind == Kind::Enderman && layer == 1)
            || matches!(m.kind, Kind::Blaze | Kind::MagmaCube)
        {
            [240, 240, 0, 0]
        } else {
            light
        };
        // RenderSlime: the outer jelly is blended.
        let target: &mut Vec<Vertex> = if m.kind == Kind::Slime && layer == 1 {
            &mut *translucent
        } else {
            &mut *out
        };
        let mut d = Draw {
            out: target,
            tex: t,
            light: lt,
            tint: tint(col),
        };
        // The young (ModelQuadruped.render / ModelChicken.render with
        // isChild): the head at full size, moved down and forward, the
        // rest at half size standing on the ground.
        let child = m.flags & 16 != 0;
        let heads = match m.kind {
            Kind::Pig | Kind::Cow | Kind::Mooshroom | Kind::Sheep => 1,
            Kind::Chicken => 3,
            _ => 0,
        };
        let (cy, cz) = match m.kind {
            Kind::Pig => (4.0, 4.0),
            Kind::Cow | Kind::Mooshroom => (8.0, 6.0),
            Kind::Chicken => (5.0, 2.0),
            _ => (8.0, 4.0),
        };
        for (i, p) in parts(m, &a, age, layer, partial).iter().enumerate() {
            if child && heads > 0 {
                let pm = if i < heads {
                    mul(&mat, &translate(0.0, cy * 0.0625, cz * 0.0625))
                } else {
                    mul(
                        &mul(&mat, &super::entities::scale(0.5, 0.5, 0.5)),
                        &translate(0.0, 24.0 * 0.0625, 0.0),
                    )
                };
                render(&mut d, &pm, p);
            } else {
                render(&mut d, &mat, p);
            }
        }
    }
    // RenderEnderman.renderEquippedItems: the carried block
    if m.kind == Kind::Enderman && m.carried[0] != 0 {
        let mut b = mul(&mat, &translate(0.0, 0.6875, -0.75));
        b = mul(&b, &super::rot_x(20.0));
        b = mul(&b, &rot_y(45.0));
        b = mul(&b, &super::entities::scale(-0.5, -0.5, 0.5));
        b = mul(&b, &translate(-0.5, -0.5, -0.5));
        block(out, m.carried[0], m.carried[1], &b, light);
    }
}

/// `RenderArrow`
pub fn arrow(out: &mut Vec<Vertex>, t: Tex, rel: [f32; 3], yaw: f32, pitch: f32, light: [u8; 4]) {
    let mut m = translate(rel[0], rel[1], rel[2]);
    m = mul(&m, &rot_y(yaw - 90.0));
    m = mul(&m, &rot_z(pitch));
    let s = 0.05625f32;
    m = mul(&m, &rot_x(45.0));
    m = mul(&m, &super::entities::scale(s, s, s));
    m = mul(&m, &translate(-4.0, 0.0, 0.0));
    let uv = |u: f32, v: f32| {
        [
            t.rect[0] + (t.rect[2] - t.rect[0]) * u * 32.0 / t.size,
            t.rect[1] + (t.rect[3] - t.rect[1]) * v * 32.0 / t.size,
        ]
    };
    let c = [255u8, 255, 255, 255];
    let mut quad = |p: [[f32; 3]; 4], u: [[f32; 2]; 4]| {
        let vs: Vec<Vertex> = (0..4)
            .map(|i| {
                let q = transform(&m, [p[i][0], p[i][1], p[i][2], 1.0]);
                Vertex {
                    pos: [q[0], q[1], q[2]],
                    uv: u[i],
                    color: c,
                    light,
                }
            })
            .collect();
        out.extend_from_slice(&vs);
        out.extend_from_slice(&[vs[3], vs[2], vs[1], vs[0]]);
    };
    let (f2, f3, f4, f5) = (0.0, 0.5, 0.0, 5.0 / 32.0);
    quad(
        [
            [-7.0, -2.0, -2.0],
            [-7.0, -2.0, 2.0],
            [-7.0, 2.0, 2.0],
            [-7.0, 2.0, -2.0],
        ],
        [
            uv(0.0, 5.0 / 32.0),
            uv(0.15625, 5.0 / 32.0),
            uv(0.15625, 10.0 / 32.0),
            uv(0.0, 10.0 / 32.0),
        ],
    );
    for i in 0..2 {
        let (a, b) = if i == 0 { (2.0, -2.0) } else { (-2.0, 2.0) };
        let _ = b;
        quad(
            [
                [-8.0, -2.0, 0.0],
                [8.0, -2.0, 0.0],
                [8.0, 2.0, 0.0],
                [-8.0, 2.0, 0.0],
            ]
            .map(|p| if i == 0 { p } else { [p[0], 0.0, p[1]] })
            .map(|p| [p[0], p[1] * a / 2.0, p[2]]),
            [uv(f2, f4), uv(f3, f4), uv(f3, f5), uv(f2, f5)],
        );
    }
}

/// `GuiInventory.func_147046_a` drawing the player through
/// `RendererLivingEntity.doRender` / `RenderPlayer`: `(x, y)` the feet in GUI
/// pixels, `scale` 30, `(dx, dy)` the mouse offsets that turn the body and
/// head, `age` `ticksExisted + partialTicks`. Returns GUI-space quads (four
/// vertices each, both windings) with the GUI lighting applied.
#[allow(clippy::too_many_arguments)]
pub fn player_gui(
    atlas: &super::atlas::Atlas,
    x: f32,
    y: f32,
    scale: f32,
    dx: f32,
    dy: f32,
    age: f32,
    armor: &[Option<crate::item::Stack>; 4],
    held: Option<crate::item::Stack>,
    sneak: bool,
) -> Vec<Vertex> {
    let a = (dx / 40.0).atan();
    let b = (dy / 40.0).atan();
    let yaw_offset = a * 20.0;
    let yaw = a * 40.0;
    let pitch = -b * 20.0;
    let pose = Pose {
        age,
        head_yaw: yaw - yaw_offset,
        head_pitch: pitch,
        held_right: held.is_some() as i32,
        sneak,
        ..Default::default()
    };
    let sc = super::entities::scale;
    let mut m = translate(x, y, 50.0);
    m = mul(&m, &sc(-scale, scale, scale));
    m = mul(&m, &rot_z(180.0));
    // RenderHelper.enableStandardItemLighting under glRotatef(135, 0, 1, 0)
    let light_m = mul(&mul(&sc(-1.0, 1.0, 1.0), &rot_z(180.0)), &rot_y(135.0));
    m = mul(&m, &rot_x(-b * 20.0));
    // glTranslatef(0, yOffset, 0) and RenderPlayer's `y - yOffset` cancel;
    // doRender at (0, 0, 0) with rotateCorpse
    m = mul(&m, &rot_y(180.0 - yaw_offset));
    m = mul(&m, &sc(-1.0, -1.0, 1.0));
    m = mul(&m, &sc(0.9375, 0.9375, 0.9375));
    m = mul(&m, &translate(0.0, -24.0 / 16.0 - 0.0078125, 0.0));
    let mut quads = Vec::new();
    for (raw, tint) in player_layers(atlas, &m, &pose, armor, held, false, [0; 4], [1.0; 4]) {
        quads.extend(gui_quads(raw, &light_m, tint));
    }
    sort_quads(quads)
}

/// Model quads (both windings, from `Draw`) as a GUI draws them: one
/// winding per face, lit by `RenderHelper.enableStandardItemLighting` set
/// up under `light_m`, sorted back to front for the depth-less UI pass.
pub fn gui_lit(raw: Vec<Vertex>, light_m: &Mat4) -> Vec<Vertex> {
    sort_quads(gui_quads(raw, light_m, Some([1.0; 4])))
}

/// Faces of `raw` (two windings each) seen from the GUI, relit with
/// `tint` (or keeping their colours when `None`).
pub fn gui_quads(raw: Vec<Vertex>, light_m: &Mat4, tint: Option<[f32; 4]>) -> Vec<[Vertex; 4]> {
    let norm = |v: [f32; 4]| {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        [v[0] / l, v[1] / l, v[2] / l]
    };
    let l0 = norm(transform(light_m, [L0[0], L0[1], L0[2], 0.0]));
    let l1 = norm(transform(light_m, [L1[0], L1[1], L1[2], 0.0]));
    // one quad per face (the reversed winding is dropped: the faces are
    // sorted back to front instead of depth tested)
    let mut quads: Vec<[Vertex; 4]> = Vec::new();
    for g in raw.chunks(8) {
        let q = [g[0], g[1], g[2], g[3]];
        let e1 = [
            q[1].pos[0] - q[0].pos[0],
            q[1].pos[1] - q[0].pos[1],
            q[1].pos[2] - q[0].pos[2],
        ];
        let e2 = [
            q[2].pos[0] - q[0].pos[0],
            q[2].pos[1] - q[0].pos[1],
            q[2].pos[2] - q[0].pos[2],
        ];
        let mut n = [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ];
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if l < 1e-6 {
            continue;
        }
        n = [n[0] / l, n[1] / l, n[2] / l];
        // the face towards the viewer (+z in the GUI's eye space)
        if n[2] < 0.0 {
            n = [-n[0], -n[1], -n[2]];
        }
        let d0 = (n[0] * l0[0] + n[1] * l0[1] + n[2] * l0[2]).max(0.0);
        let d1 = (n[0] * l1[0] + n[1] * l1[1] + n[2] * l1[2]).max(0.0);
        let k = (0.4 + 0.6 * (d0 + d1)).min(1.0);
        let mut q = q;
        if let Some(t) = tint {
            let c = [
                (t[0] * k * 255.0) as u8,
                (t[1] * k * 255.0) as u8,
                (t[2] * k * 255.0) as u8,
                255,
            ];
            for v in &mut q {
                v.color = c;
            }
        }
        quads.push(q);
    }
    quads
}

/// Single-winding faces lit like `gui_quads` but keeping their colours
/// (`glColor` times the lighting), sorted back to front.
pub fn gui_lit_faces(faces: Vec<[Vertex; 4]>, light_m: &Mat4) -> Vec<Vertex> {
    let mut raw = Vec::with_capacity(faces.len() * 8);
    for q in &faces {
        raw.extend_from_slice(q);
        raw.extend_from_slice(q);
    }
    let lit = gui_quads(raw, light_m, Some([1.0; 4]));
    // gui_quads drops degenerate faces; match the rest back by position
    let mut out = Vec::with_capacity(lit.len());
    let mut k = 0;
    for mut q in lit {
        while k < faces.len() && faces[k][0].pos != q[0].pos {
            k += 1;
        }
        if let Some(src) = faces.get(k) {
            for (v, s) in q.iter_mut().zip(src) {
                for c in 0..3 {
                    v.color[c] = (v.color[c] as u32 * s.color[c] as u32 / 255) as u8;
                }
            }
        }
        k += 1;
        out.push(q);
    }
    sort_quads(out)
}

/// Back to front for the UI pass, which has no depth test.
pub fn sort_quads(mut quads: Vec<[Vertex; 4]>) -> Vec<Vertex> {
    let depth = |q: &[Vertex; 4]| q.iter().map(|v| v.pos[2]).sum::<f32>();
    quads.sort_by(|a, b| depth(a).total_cmp(&depth(b)));
    let mut out = Vec::with_capacity(quads.len() * 4);
    for q in quads {
        out.extend_from_slice(&q);
    }
    out
}

/// The inputs of `ModelBiped.setRotationAngles` beyond `Anim`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Pose {
    pub limb_swing: f32,
    pub limb_amount: f32,
    pub age: f32,
    /// head yaw relative to the body, head pitch (degrees)
    pub head_yaw: f32,
    pub head_pitch: f32,
    /// `onGround`: the swing progress
    pub swing: f32,
    /// `heldItemRight`: 0 empty, 1 holding, 3 blocking
    pub held_right: i32,
    pub sneak: bool,
    pub riding: bool,
    pub aimed_bow: bool,
}

/// `ModelBiped(gap)` posed by `setRotationAngles`: head, body, right arm,
/// left arm, right leg, left leg, headwear.
pub fn biped_pose(p: &Pose, gap: f32) -> Vec<Part> {
    let head = Part::new([0.0, 0.0, 0.0]).bx(0.0, 0.0, [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], gap);
    let hat =
        Part::new([0.0, 0.0, 0.0]).bx(32.0, 0.0, [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], gap + 0.5);
    let body = Part::new([0.0, 0.0, 0.0]).bx(16.0, 16.0, [-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], gap);
    let rarm =
        Part::new([-5.0, 2.0, 0.0]).bx(40.0, 16.0, [-3.0, -2.0, -2.0], [4.0, 12.0, 4.0], gap);
    let larm =
        Part::new([5.0, 2.0, 0.0]).mbx(40.0, 16.0, [-1.0, -2.0, -2.0], [4.0, 12.0, 4.0], gap);
    let rleg = Part::new([-1.9, 12.0, 0.0]).bx(0.0, 16.0, [-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], gap);
    let lleg = Part::new([1.9, 12.0, 0.0]).mbx(0.0, 16.0, [-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], gap);
    let mut v = vec![head, body, rarm, larm, rleg, lleg, hat];
    let d = 180.0 / PI;
    let (s1, s2, s3) = (p.limb_swing, p.limb_amount, p.age);
    v[0].rot = [p.head_pitch / d, p.head_yaw / d, 0.0];
    v[6].rot = v[0].rot;
    v[2].rot = [cos(s1 * 0.6662 + PI) * 2.0 * s2 * 0.5, 0.0, 0.0];
    v[3].rot = [cos(s1 * 0.6662) * 2.0 * s2 * 0.5, 0.0, 0.0];
    v[4].rot = [cos(s1 * 0.6662) * 1.4 * s2, 0.0, 0.0];
    v[5].rot = [cos(s1 * 0.6662 + PI) * 1.4 * s2, 0.0, 0.0];
    if p.riding {
        v[2].rot[0] += -PI / 5.0;
        v[3].rot[0] += -PI / 5.0;
        v[4].rot[0] = -PI * 2.0 / 5.0;
        v[5].rot[0] = -PI * 2.0 / 5.0;
        v[4].rot[1] = PI / 10.0;
        v[5].rot[1] = -PI / 10.0;
    }
    if p.held_right != 0 {
        v[2].rot[0] = v[2].rot[0] * 0.5 - PI / 10.0 * p.held_right as f32;
    }
    // onGround > -9990: the swing
    let g = p.swing;
    let by = sin(g.sqrt() * PI * 2.0) * 0.2;
    v[1].rot[1] = by;
    v[2].pivot[2] = sin(by) * 5.0;
    v[2].pivot[0] = -cos(by) * 5.0;
    v[3].pivot[2] = -sin(by) * 5.0;
    v[3].pivot[0] = cos(by) * 5.0;
    v[2].rot[1] += by;
    v[3].rot[1] += by;
    v[3].rot[0] += by;
    let mut f = 1.0 - g;
    f *= f;
    f *= f;
    f = 1.0 - f;
    let f9 = sin(f * PI);
    let f10 = sin(g * PI) * -(v[0].rot[0] - 0.7) * 0.75;
    v[2].rot[0] = (v[2].rot[0] as f64 - (f9 as f64 * 1.2 + f10 as f64)) as f32;
    v[2].rot[1] += by * 2.0;
    v[2].rot[2] = sin(g * PI) * -0.4;
    if p.sneak {
        v[1].rot[0] = 0.5;
        v[2].rot[0] += 0.4;
        v[3].rot[0] += 0.4;
        v[4].pivot[2] = 4.0;
        v[5].pivot[2] = 4.0;
        v[4].pivot[1] = 9.0;
        v[5].pivot[1] = 9.0;
        v[0].pivot[1] = 1.0;
        v[6].pivot[1] = 1.0;
    } else {
        v[1].rot[0] = 0.0;
        v[4].pivot[2] = 0.1;
        v[5].pivot[2] = 0.1;
        v[4].pivot[1] = 12.0;
        v[5].pivot[1] = 12.0;
        v[0].pivot[1] = 0.0;
        v[6].pivot[1] = 0.0;
    }
    v[2].rot[2] += cos(s3 * 0.09) * 0.05 + 0.05;
    v[3].rot[2] -= cos(s3 * 0.09) * 0.05 + 0.05;
    v[2].rot[0] += sin(s3 * 0.067) * 0.05;
    v[3].rot[0] -= sin(s3 * 0.067) * 0.05;
    if p.aimed_bow {
        v[2].rot[2] = 0.0;
        v[3].rot[2] = 0.0;
        v[2].rot[1] = -0.1 + v[0].rot[1];
        v[3].rot[1] = 0.1 + v[0].rot[1] + 0.4;
        v[2].rot[0] = -PI / 2.0 + v[0].rot[0];
        v[3].rot[0] = -PI / 2.0 + v[0].rot[0];
        v[2].rot[2] += cos(s3 * 0.09) * 0.05 + 0.05;
        v[3].rot[2] -= cos(s3 * 0.09) * 0.05 + 0.05;
        v[2].rot[0] += sin(s3 * 0.067) * 0.05;
        v[3].rot[0] -= sin(s3 * 0.067) * 0.05;
    }
    v
}

/// `RendererLivingEntity.doRender` for a player model at `rel` (feet,
/// camera relative) turned by `body_yaw`, with `RenderPlayer`'s scale.
pub fn player_matrix(rel: [f32; 3], body_yaw: f32) -> Mat4 {
    let mut m = translate(rel[0], rel[1], rel[2]);
    m = mul(&m, &rot_y(180.0 - body_yaw));
    m = mul(&m, &super::entities::scale(-1.0, -1.0, 1.0));
    m = mul(&m, &super::entities::scale(0.9375, 0.9375, 0.9375));
    mul(&m, &translate(0.0, -24.0 / 16.0 - 0.0078125, 0.0))
}

/// The player in the world (third person).
pub fn player_world(d: &mut Draw, m: &Mat4, pose: &Pose) {
    for p in biped_pose(pose, 0.0) {
        render(d, m, &p);
    }
}

/// `RenderBiped.bipedArmorFilenamePrefix` textures: both layers of each
/// material and the leather overlays.
pub const ARMOR_TEXTURES: [&str; 12] = [
    "../models/armor/leather_layer_1",
    "../models/armor/leather_layer_2",
    "../models/armor/leather_layer_1_overlay",
    "../models/armor/leather_layer_2_overlay",
    "../models/armor/chainmail_layer_1",
    "../models/armor/chainmail_layer_2",
    "../models/armor/iron_layer_1",
    "../models/armor/iron_layer_2",
    "../models/armor/diamond_layer_1",
    "../models/armor/diamond_layer_2",
    "../models/armor/gold_layer_1",
    "../models/armor/gold_layer_2",
];

/// `RenderPlayer.shouldRenderPass` for pass 0..4 (helmet to boots): the
/// armour texture layers with their tint (`ItemArmor.getColor`, leather's
/// default 10511680 then the untinted overlay).
pub fn armor_layers(s: &crate::item::Stack, pass: usize) -> Vec<(String, [f32; 4])> {
    let id = s.id;
    if !(298..=317).contains(&id) {
        return Vec::new();
    }
    let mat = ["leather", "chainmail", "iron", "diamond", "gold"][(id as usize - 298) / 4];
    let layer = if pass == 2 { 2 } else { 1 };
    let base = format!("../models/armor/{mat}_layer_{layer}");
    if mat == "leather" {
        let c = s.dye_color().map_or(10511680u32, |c| c as u32);
        let t = [
            (c >> 16 & 255) as f32 / 255.0,
            (c >> 8 & 255) as f32 / 255.0,
            (c & 255) as f32 / 255.0,
            1.0,
        ];
        vec![(base.clone(), t), (format!("{base}_overlay"), [1.0; 4])]
    } else {
        vec![(base, [1.0; 4])]
    }
}

/// The armour model of a pass: `modelArmorChestplate` (1.0) or, for the
/// leggings, `modelArmor` (0.5), with only that pass's parts shown.
pub fn armor_parts(pose: &Pose, pass: usize) -> Vec<Part> {
    let gap = if pass == 2 { 0.5 } else { 1.0 };
    let v = biped_pose(pose, gap);
    // head, body, right arm, left arm, right leg, left leg, headwear
    let show = [
        pass == 0,
        pass == 1 || pass == 2,
        pass == 1,
        pass == 1,
        pass == 2 || pass == 3,
        pass == 2 || pass == 3,
        pass == 0,
    ];
    v.into_iter()
        .zip(show)
        .filter(|(_, s)| *s)
        .map(|(p, _)| p)
        .collect()
}

/// `ModelRenderer.postRender(0.0625)` of the posed right arm.
pub fn arm_matrix(m: &Mat4, pose: &Pose) -> Mat4 {
    let arm = &biped_pose(pose, 0.0)[2];
    let mut a = mul(
        m,
        &translate(
            arm.pivot[0] / 16.0,
            arm.pivot[1] / 16.0,
            arm.pivot[2] / 16.0,
        ),
    );
    let deg = 180.0 / PI;
    if arm.rot[2] != 0.0 {
        a = mul(&a, &rot_z(arm.rot[2] * deg));
    }
    if arm.rot[1] != 0.0 {
        a = mul(&a, &rot_y(arm.rot[1] * deg));
    }
    if arm.rot[0] != 0.0 {
        a = mul(&a, &rot_x(arm.rot[0] * deg));
    }
    a
}

/// `Item.isFull3D` and `shouldRotateAroundWhenRendering`.
pub fn full_3d(id: u16) -> (bool, bool) {
    if crate::item::tool(id).is_some() {
        return (true, false);
    }
    match id {
        280 | 352 | 369 => (true, false),
        346 | 398 => (true, true),
        _ => (false, false),
    }
}

/// `RenderPlayer.renderEquippedItems`: the held item's place from the
/// right arm's `postRender`.
pub fn held_matrix(arm: &Mat4, s: &crate::item::Stack, blocking: bool) -> Mat4 {
    let sc = super::entities::scale;
    let mut m = mul(arm, &translate(-0.0625, 0.4375, 0.0625));
    let block3d = s.block().is_some_and(|b| {
        matches!(
            crate::block::info(b).shape,
            crate::block::Shape::Cube
                | crate::block::Shape::Cactus
                | crate::block::Shape::Chest
                | crate::block::Shape::Slab
                | crate::block::Shape::SnowLayer
        )
    });
    let (full, around) = full_3d(s.id);
    if block3d {
        let k = 0.5 * 0.75;
        m = mul(&m, &translate(0.0, 0.1875, -0.3125));
        m = mul(&m, &rot_x(20.0));
        m = mul(&m, &rot_y(45.0));
        m = mul(&m, &sc(-k, -k, k));
    } else if s.id == 261 {
        let k = 0.625;
        m = mul(&m, &translate(0.0, 0.125, 0.3125));
        m = mul(&m, &rot_y(-20.0));
        m = mul(&m, &sc(k, -k, k));
        m = mul(&m, &rot_x(-100.0));
        m = mul(&m, &rot_y(45.0));
    } else if full {
        let k = 0.625;
        if around {
            m = mul(&m, &rot_z(180.0));
            m = mul(&m, &translate(0.0, -0.125, 0.0));
        }
        if blocking {
            m = mul(&m, &translate(0.05, 0.0, -0.1));
            m = mul(&m, &rot_y(-50.0));
            m = mul(&m, &rot_x(-10.0));
            m = mul(&m, &rot_z(-60.0));
        }
        m = mul(&m, &translate(0.0, 0.1875, 0.0));
        m = mul(&m, &sc(k, -k, k));
        m = mul(&m, &rot_x(-100.0));
        m = mul(&m, &rot_y(45.0));
    } else {
        let k = 0.375;
        m = mul(&m, &translate(0.25, 0.1875, -0.1875));
        m = mul(&m, &sc(k, k, k));
        m = mul(&m, &rot_z(60.0));
        m = mul(&m, &rot_x(-90.0));
        m = mul(&m, &rot_z(20.0));
    }
    m
}

/// A player model with its armour and held item, as layers: the skin,
/// `shouldRenderPass` for each armour slot (tinted), then
/// `renderEquippedItems` (its colours as drawn, no tint).
#[allow(clippy::too_many_arguments)]
pub fn player_layers(
    atlas: &super::atlas::Atlas,
    m: &Mat4,
    pose: &Pose,
    armor: &[Option<crate::item::Stack>; 4],
    held: Option<crate::item::Stack>,
    blocking: bool,
    light: [u8; 4],
    tint: [f32; 4],
) -> Vec<(Vec<Vertex>, Option<[f32; 4]>)> {
    let mut layers = Vec::new();
    {
        let mut raw = Vec::new();
        let mut d = Draw {
            out: &mut raw,
            tex: atlas.skin_tex(),
            light,
            tint,
        };
        for p in biped_pose(pose, 0.0) {
            render(&mut d, m, &p);
        }
        layers.push((raw, Some(tint)));
    }
    for pass in 0..4 {
        // armorItemInSlot(3 - pass)
        let Some(a) = armor[3 - pass] else { continue };
        for (name, t) in armor_layers(&a, pass) {
            let Some(tex) = atlas.mob_tex(&name) else {
                continue;
            };
            let c = [t[0] * tint[0], t[1] * tint[1], t[2] * tint[2], 1.0];
            let mut raw = Vec::new();
            let mut d = Draw {
                out: &mut raw,
                tex,
                light,
                tint: c,
            };
            for p in armor_parts(pose, pass) {
                render(&mut d, m, &p);
            }
            layers.push((raw, Some(c)));
        }
    }
    if let Some(s) = held {
        let am = arm_matrix(m, pose);
        let hm = held_matrix(&am, &s, blocking);
        let mut items = Vec::new();
        super::entities::render_item(&mut items, atlas, s, &hm, light);
        // renderItemIn2D draws one winding; models draw both
        let mut raw = Vec::new();
        for q in items.chunks(4) {
            raw.extend_from_slice(q);
            raw.extend_from_slice(&[q[3], q[2], q[1], q[0]]);
        }
        layers.push((raw, None));
    }
    layers
}

pub const BOOK_TEXTURE: &str = "enchanting_table_book";

/// `ModelBook` posed by `setRotationAngles(time, flipA, flipB, spread)`:
/// the covers, spine, page blocks and the two flipping pages.
pub fn book_parts(time: f32, flip_a: f32, flip_b: f32, spread: f32) -> Vec<Part> {
    let k = (sin(time * 0.02) * 0.1 + 1.25) * spread;
    let s = sin(k);
    vec![
        Part::new([0.0, 0.0, -1.0])
            .bx(0.0, 0.0, [-6.0, -5.0, 0.0], [6.0, 10.0, 0.0], 0.0)
            .r(0.0, PI + k, 0.0),
        Part::new([0.0, 0.0, 1.0])
            .bx(16.0, 0.0, [0.0, -5.0, 0.0], [6.0, 10.0, 0.0], 0.0)
            .r(0.0, -k, 0.0),
        Part::new([0.0, 0.0, 0.0])
            .bx(12.0, 0.0, [-1.0, -5.0, 0.0], [2.0, 10.0, 0.0], 0.0)
            .r(0.0, PI / 2.0, 0.0),
        Part::new([s, 0.0, 0.0])
            .bx(0.0, 10.0, [0.0, -4.0, -0.99], [5.0, 8.0, 1.0], 0.0)
            .r(0.0, k, 0.0),
        Part::new([s, 0.0, 0.0])
            .bx(12.0, 10.0, [0.0, -4.0, -0.01], [5.0, 8.0, 1.0], 0.0)
            .r(0.0, -k, 0.0),
        Part::new([s, 0.0, 0.0])
            .bx(24.0, 10.0, [0.0, -4.0, 0.0], [5.0, 8.0, 0.0], 0.0)
            .r(0.0, k - k * 2.0 * flip_a, 0.0),
        Part::new([s, 0.0, 0.0])
            .bx(24.0, 10.0, [0.0, -4.0, 0.0], [5.0, 8.0, 0.0], 0.0)
            .r(0.0, k - k * 2.0 * flip_b, 0.0),
    ]
}

/// The page angles `TileEntityEnchantmentTableRenderer` and `GuiEnchantment`
/// derive from the flip value.
pub fn book_flips(flip: f32) -> (f32, f32) {
    let f = |v: f32| ((v - v.trunc()) * 1.6 - 0.3).clamp(0.0, 1.0);
    (f(flip + 0.25), f(flip + 0.75))
}

/// `GuiEnchantment.drawGuiContainerBackgroundLayer`'s book: drawn through
/// its own 320x240 viewport with `gluPerspective(90, 4/3, 9, 80)` shifted by
/// (-0.34, 0.23), then mapped back to GUI pixels (`sw` x `sh`).
pub fn enchant_book_gui(
    atlas: &super::atlas::Atlas,
    sw: f32,
    sh: f32,
    open: f32,
    flip: f32,
) -> Vec<Vertex> {
    let Some(tex) = atlas.mob_tex(BOOK_TEXTURE) else {
        return Vec::new();
    };
    let sc = super::entities::scale;
    let mut m = translate(0.0, 3.3, -16.0);
    m = mul(&m, &sc(5.0, 5.0, 5.0));
    m = mul(&m, &rot_z(180.0));
    m = mul(&m, &rot_x(20.0));
    let o = 1.0 - open;
    m = mul(&m, &translate(o * 0.2, o * 0.1, o * 0.25));
    m = mul(&m, &rot_y(-o * 90.0 - 90.0));
    m = mul(&m, &rot_x(180.0));
    let (fa, fb) = book_flips(flip);
    let mut raw = Vec::new();
    {
        let mut d = Draw {
            out: &mut raw,
            tex,
            light: [0; 4],
            tint: [1.0; 4],
        };
        for p in book_parts(0.0, fa, fb, open) {
            render(&mut d, &m, &p);
        }
    }
    // enableStandardItemLighting under the identity model view
    let quads = gui_quads(raw, &super::IDENTITY, Some([1.0; 4]));
    let sorted = sort_quads(quads);
    // the projection and the viewport
    let f = 1.0 / (45.0f32).to_radians().tan();
    let a = 320.0 / 240.0;
    let vx = ((sw as i32 - 320) / 2) as f32;
    let vy = ((sh as i32 - 240) / 2) as f32;
    sorted
        .into_iter()
        .map(|mut v| {
            let (x, y, z) = (v.pos[0], v.pos[1], v.pos[2]);
            let w = -z;
            let cx = f / a * x + -0.34 * w;
            let cy = f * y + 0.23 * w;
            let (nx, ny) = (cx / w, cy / w);
            v.pos = [
                vx + (nx + 1.0) / 2.0 * 320.0,
                sh - (vy + (ny + 1.0) / 2.0 * 240.0),
                0.0,
            ];
            v
        })
        .collect()
}

/// `ModelBook.render` under `m`.
pub fn book_world(d: &mut Draw, m: &Mat4, time: f32, flip_a: f32, flip_b: f32, spread: f32) {
    for p in book_parts(time, flip_a, flip_b, spread) {
        render(d, m, &p);
    }
}
