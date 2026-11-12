//! Models of the ghast, blaze, magma cube, ender dragon and ender crystal
//! (`ModelGhast`, `ModelBlaze`, `ModelMagmaCube`, `ModelDragon`,
//! `ModelEnderCrystal`).

use super::model::{cos, render, sin, Anim, Draw, Part, PI};
use super::{mul, rot_x, rot_y, rot_z, translate, Mat4};
use crate::java::JavaRandom;
use crate::sim::MobView;

fn rad(deg: f32) -> f32 {
    deg / (180.0 / PI)
}

/// `ModelGhast`: the body and nine tentacles of fixed random lengths.
pub(super) fn ghast(age: f32) -> Vec<Part> {
    let mut v =
        vec![Part::new([0.0, 8.0 + 9.6, 0.0]).bx(0.0, 0.0, [-8.0, -8.0, -8.0], [16.0; 3], 0.0)];
    let mut r = JavaRandom::new(1660);
    for i in 0..9 {
        let fx = ((i % 3) as f32 - (i / 3 % 2) as f32 * 0.5 + 0.25) / 2.0 * 2.0 - 1.0;
        let x = fx * 5.0;
        let z = ((i / 3) as f32 / 2.0 * 2.0 - 1.0) * 5.0;
        let len = (r.next_int_n(7) + 8) as f32;
        v.push(
            Part::new([x, 15.0 + 9.6, z])
                .bx(0.0, 0.0, [-1.0, 0.0, -1.0], [2.0, len, 2.0], 0.0)
                .r(0.2 * sin(age * 0.3 + i as f32) + 0.4, 0.0, 0.0),
        );
    }
    v
}

/// `ModelBlaze`: a head and three rings of rods.
pub(super) fn blaze(a: &Anim) -> Vec<Part> {
    let t = a.age;
    let mut v = vec![Part::new([0.0; 3])
        .bx(0.0, 0.0, [-4.0, -4.0, -4.0], [8.0; 3], 0.0)
        .r(rad(a.head_pitch), rad(a.head_yaw), 0.0)];
    let rod =
        |x: f32, y: f32, z: f32| Part::new([x, y, z]).bx(0.0, 16.0, [0.0; 3], [2.0, 8.0, 2.0], 0.0);
    let mut ang = t * PI * -0.1;
    for i in 0..4 {
        v.push(rod(
            cos(ang) * 9.0,
            -2.0 + cos((i as f32 * 2.0 + t) * 0.25),
            sin(ang) * 9.0,
        ));
        ang += PI / 2.0;
    }
    let mut ang = PI / 4.0 + t * PI * 0.03;
    for i in 4..8 {
        v.push(rod(
            cos(ang) * 7.0,
            2.0 + cos((i as f32 * 2.0 + t) * 0.25),
            sin(ang) * 7.0,
        ));
        ang += PI / 2.0;
    }
    let mut ang = 0.47123894 + t * PI * -0.05;
    for i in 8..12 {
        v.push(rod(
            cos(ang) * 5.0,
            11.0 + cos((i as f32 * 1.5 + t) * 0.5),
            sin(ang) * 5.0,
        ));
        ang += PI / 2.0;
    }
    v
}

/// `ModelMagmaCube`: eight slices that spread apart when it squashes.
pub(super) fn magma_cube(squish: f32) -> Vec<Part> {
    let s = squish.max(0.0);
    let mut v = vec![Part::new([0.0; 3]).bx(0.0, 16.0, [-2.0, 18.0, -2.0], [4.0; 3], 0.0)];
    for i in 0..8 {
        let (u, w) = match i {
            2 => (24.0, 10.0),
            3 => (24.0, 19.0),
            _ => (0.0, i as f32),
        };
        v.push(Part::new([0.0, -(4.0 - i as f32) * s * 1.7, 0.0]).bx(
            u,
            w,
            [-4.0, 16.0 + i as f32, -4.0],
            [8.0, 1.0, 8.0],
            0.0,
        ));
    }
    v
}

/// `EntityDragon.getMovementOffsets` on the client copy.
pub(super) fn ring(m: &MobView, back: i32, partial: f32) -> [f64; 2] {
    if m.ring.len() < 64 {
        return [m.body_yaw as f64, m.pos[1]];
    }
    let partial = if m.health <= 0.0 { 0.0 } else { partial };
    let p = 1.0 - partial as f64;
    let i = ((m.ring_index - back) & 63) as usize;
    let j = ((m.ring_index - back - 1) & 63) as usize;
    let (a, b) = (m.ring[i], m.ring[j]);
    [a[0] + wrap(b[0] - a[0]) * p, a[1] + (b[1] - a[1]) * p]
}

fn wrap(mut d: f64) -> f64 {
    while d >= 180.0 {
        d -= 360.0;
    }
    while d < -180.0 {
        d += 360.0;
    }
    d
}

fn neck() -> Part {
    Part::new([0.0; 3])
        .bx(192.0, 104.0, [-5.0, -5.0, -5.0], [10.0; 3], 0.0)
        .bx(48.0, 0.0, [-1.0, -9.0, -3.0], [2.0, 4.0, 6.0], 0.0)
}

/// `ModelDragon.render`, from the matrix after the usual entity transforms.
pub(super) fn dragon(d: &mut Draw, mat: &Mat4, m: &MobView, partial: f32) {
    let anim = m.prev_wing + (m.wing - m.prev_wing) * partial;
    let jaw = ((anim * PI * 2.0).sin() + 1.0) * 0.2;
    let mut f10 = (anim * PI * 2.0 - 1.0).sin() + 1.0;
    f10 = (f10 * f10 + f10 * 2.0) * 0.05;
    let mut base = mul(mat, &translate(0.0, f10 - 2.0, -3.0));
    base = mul(&base, &rot_x(f10 * 2.0));
    let k = 1.5f32;
    let r6 = ring(m, 6, partial);
    let f16 = wrap(ring(m, 5, partial)[0] - ring(m, 10, partial)[0]) as f32;
    let f17 = wrap(ring(m, 5, partial)[0] + f16 as f64 / 2.0) as f32;
    let t = anim * PI * 2.0;
    let (mut y, mut z, mut x) = (20.0f32, -12.0f32, 0.0f32);
    for i in 0..5 {
        let o = ring(m, 5 - i, partial);
        let wob = (i as f32 * 0.45 + t).cos() * 0.15;
        let mut s = neck();
        let ry = wrap(o[0] - r6[0]) as f32 * PI / 180.0 * k;
        let rx = wob + (o[1] - r6[1]) as f32 * PI / 180.0 * k * 5.0;
        let rz = -wrap(o[0] - f17 as f64) as f32 * PI / 180.0 * k;
        s.rot = [rx, ry, rz];
        s.pivot = [x, y, z];
        y += rx.sin() * 10.0;
        z -= ry.cos() * rx.cos() * 10.0;
        x -= ry.sin() * rx.cos() * 10.0;
        render(d, &base, &s);
    }
    let o0 = ring(m, 0, partial);
    let mut head = Part::new([x, y, z])
        .bx(176.0, 44.0, [-6.0, -1.0, -24.0], [12.0, 5.0, 16.0], 0.0)
        .bx(112.0, 30.0, [-8.0, -8.0, -10.0], [16.0; 3], 0.0)
        .mbx(0.0, 0.0, [-5.0, -12.0, -4.0], [2.0, 4.0, 6.0], 0.0)
        .mbx(112.0, 0.0, [-5.0, -3.0, -22.0], [2.0, 2.0, 4.0], 0.0)
        .bx(0.0, 0.0, [3.0, -12.0, -4.0], [2.0, 4.0, 6.0], 0.0)
        .bx(112.0, 0.0, [3.0, -3.0, -22.0], [2.0, 2.0, 4.0], 0.0);
    head.rot = [
        0.0,
        wrap(o0[0] - r6[0]) as f32 * PI / 180.0,
        -wrap(o0[0] - f17 as f64) as f32 * PI / 180.0,
    ];
    head.children.push(
        Part::new([0.0, 4.0, -8.0])
            .bx(176.0, 65.0, [-6.0, 0.0, -16.0], [12.0, 4.0, 16.0], 0.0)
            .r(jaw, 0.0, 0.0),
    );
    render(d, &base, &head);
    let mut body_m = mul(&base, &translate(0.0, 1.0, 0.0));
    body_m = mul(&body_m, &rot_z(-f16 * k));
    body_m = mul(&body_m, &translate(0.0, -1.0, 0.0));
    let body = Part::new([0.0, 4.0, 8.0])
        .bx(0.0, 0.0, [-12.0, 0.0, -16.0], [24.0, 24.0, 64.0], 0.0)
        .bx(220.0, 53.0, [-1.0, -6.0, -10.0], [2.0, 6.0, 12.0], 0.0)
        .bx(220.0, 53.0, [-1.0, -6.0, 10.0], [2.0, 6.0, 12.0], 0.0)
        .bx(220.0, 53.0, [-1.0, -6.0, 30.0], [2.0, 6.0, 12.0], 0.0);
    render(d, &body_m, &body);
    let mut side = body_m;
    for _ in 0..2 {
        let a = anim * PI * 2.0;
        let mut wing = Part::new([-12.0, 5.0, 2.0])
            .bx(112.0, 88.0, [-56.0, -4.0, -4.0], [56.0, 8.0, 8.0], 0.0)
            .bx(-56.0, 88.0, [-56.0, 0.0, 2.0], [56.0, 0.0, 56.0], 0.0)
            .r(0.125 - a.cos() * 0.2, 0.25, (a.sin() + 0.125) * 0.8);
        wing.children.push(
            Part::new([-56.0, 0.0, 0.0])
                .bx(112.0, 136.0, [-56.0, -2.0, -2.0], [56.0, 4.0, 4.0], 0.0)
                .bx(-56.0, 144.0, [-56.0, 0.0, 2.0], [56.0, 0.0, 56.0], 0.0)
                .r(0.0, 0.0, -((a + 2.0).sin() + 0.5) * 0.75),
        );
        let mut front = Part::new([-12.0, 20.0, 2.0])
            .bx(112.0, 104.0, [-4.0, -4.0, -4.0], [8.0, 24.0, 8.0], 0.0)
            .r(1.3 + f10 * 0.1, 0.0, 0.0);
        let mut ftip = Part::new([0.0, 20.0, -1.0])
            .bx(226.0, 138.0, [-3.0, -1.0, -3.0], [6.0, 24.0, 6.0], 0.0)
            .r(-0.5 - f10 * 0.1, 0.0, 0.0);
        ftip.children.push(
            Part::new([0.0, 23.0, 0.0])
                .bx(144.0, 104.0, [-4.0, 0.0, -12.0], [8.0, 4.0, 16.0], 0.0)
                .r(0.75 + f10 * 0.1, 0.0, 0.0),
        );
        front.children.push(ftip);
        let mut rear = Part::new([-16.0, 16.0, 42.0])
            .bx(0.0, 0.0, [-8.0, -4.0, -8.0], [16.0, 32.0, 16.0], 0.0)
            .r(1.0 + f10 * 0.1, 0.0, 0.0);
        let mut rtip = Part::new([0.0, 32.0, -4.0])
            .bx(196.0, 0.0, [-6.0, -2.0, 0.0], [12.0, 32.0, 12.0], 0.0)
            .r(0.5 + f10 * 0.1, 0.0, 0.0);
        rtip.children.push(
            Part::new([0.0, 31.0, 4.0])
                .bx(112.0, 0.0, [-9.0, 0.0, -20.0], [18.0, 6.0, 24.0], 0.0)
                .r(0.75 + f10 * 0.1, 0.0, 0.0),
        );
        rear.children.push(rtip);
        render(d, &side, &wing);
        render(d, &side, &front);
        render(d, &side, &rear);
        side = mul(&side, &super::entities::scale(-1.0, 1.0, 1.0));
    }
    // the tail
    let r11 = ring(m, 11, partial);
    let (mut y, mut z, mut x) = (10.0f32, 60.0f32, 0.0f32);
    let mut wob = 0.0f32;
    for i in 0..12 {
        let o = ring(m, 12 + i, partial);
        wob += (i as f32 * 0.45 + t).sin() * 0.05;
        let mut s = neck();
        let ry = (wrap(o[0] - r11[0]) as f32 * k + 180.0) * PI / 180.0;
        let rx = wob + (o[1] - r11[1]) as f32 * PI / 180.0 * k * 5.0;
        let rz = wrap(o[0] - f17 as f64) as f32 * PI / 180.0 * k;
        s.rot = [rx, ry, rz];
        s.pivot = [x, y, z];
        y += rx.sin() * 10.0;
        z -= ry.cos() * rx.cos() * 10.0;
        x -= ry.sin() * rx.cos() * 10.0;
        render(d, &base, &s);
    }
}

/// Rotation by `deg` about the unit axis `(a, b, c)` (`glRotatef`).
fn rot_axis(deg: f32, a: f32, b: f32, c: f32) -> Mat4 {
    let l = (a * a + b * b + c * c).sqrt();
    let (x, y, z) = (a / l, b / l, c / l);
    let (s, co) = (deg.to_radians().sin(), deg.to_radians().cos());
    let t = 1.0 - co;
    [
        t * x * x + co,
        t * x * y + s * z,
        t * x * z - s * y,
        0.0,
        t * x * y - s * z,
        t * y * y + co,
        t * y * z + s * x,
        0.0,
        t * x * z + s * y,
        t * y * z - s * x,
        t * z * z + co,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
    ]
}

/// `RenderEnderCrystal` + `ModelEnderCrystal` (with its base).
pub(super) fn crystal(d: &mut Draw, rel: [f32; 3], spin: f32) {
    let mut bob = (spin * 0.2).sin() / 2.0 + 0.5;
    bob = bob * bob + bob;
    let r = spin * 3.0;
    let mut m = translate(rel[0], rel[1] + 1.0, rel[2]);
    m = mul(&m, &super::entities::scale(2.0, 2.0, 2.0));
    m = mul(&m, &translate(0.0, -0.5, 0.0));
    let base = Part::new([0.0; 3]).bx(0.0, 16.0, [-6.0, 0.0, -6.0], [12.0, 4.0, 12.0], 0.0);
    render(d, &m, &base);
    m = mul(&m, &rot_y(r));
    m = mul(&m, &translate(0.0, 0.8 + bob * 0.2, 0.0));
    m = mul(&m, &rot_axis(60.0, 0.7071, 0.0, 0.7071));
    let glass = Part::new([0.0; 3]).bx(0.0, 0.0, [-4.0, -4.0, -4.0], [8.0; 3], 0.0);
    render(d, &m, &glass);
    let f = 0.875;
    m = mul(&m, &super::entities::scale(f, f, f));
    m = mul(&m, &rot_axis(60.0, 0.7071, 0.0, 0.7071));
    m = mul(&m, &rot_y(r));
    render(d, &m, &glass);
    m = mul(&m, &super::entities::scale(f, f, f));
    m = mul(&m, &rot_axis(60.0, 0.7071, 0.0, 0.7071));
    m = mul(&m, &rot_y(r));
    let cube = Part::new([0.0; 3]).bx(32.0, 0.0, [-4.0, -4.0, -4.0], [8.0; 3], 0.0);
    render(d, &m, &cube);
}

pub const TEXTURES: [&str; 11] = [
    "../items/ender_eye",
    "ghast/ghast",
    "ghast/ghast_shooting",
    "blaze",
    "slime/magmacube",
    "zombie_pigman",
    "skeleton/wither_skeleton",
    "enderdragon/dragon",
    "enderdragon/dragon_eyes",
    "endercrystal/endercrystal",
    "../items/fireball",
];
