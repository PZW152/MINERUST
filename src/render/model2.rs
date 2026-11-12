//! Models of the slime, enderman, squid, bat, wolf, ocelot, horse and iron
//! golem (`ModelSlime`, `ModelEnderman`, `ModelSquid`, `ModelBat`,
//! `ModelWolf`, `ModelOcelot`, `ModelHorse`, `ModelIronGolem`).

use super::model::{cos, sin, Anim, Part, PI};
use crate::entity::mob::Kind;
use crate::sim::MobView;

fn rad(deg: f32) -> f32 {
    deg / (180.0 / PI)
}

/// `ModelSlime(16)` (inner, layer 0) or `ModelSlime(0)` (outer, layer 1).
pub(super) fn slime(layer: u8) -> Vec<Part> {
    if layer == 1 {
        return vec![Part::new([0.0; 3]).bx(0.0, 0.0, [-4.0, 16.0, -4.0], [8.0; 3], 0.0)];
    }
    vec![
        Part::new([0.0; 3]).bx(0.0, 16.0, [-3.0, 17.0, -3.0], [6.0; 3], 0.0),
        Part::new([0.0; 3]).bx(32.0, 0.0, [-3.25, 18.0, -3.5], [2.0; 3], 0.0),
        Part::new([0.0; 3]).bx(32.0, 4.0, [1.25, 18.0, -3.5], [2.0; 3], 0.0),
        Part::new([0.0; 3]).bx(32.0, 8.0, [0.0, 21.0, -3.5], [1.0; 3], 0.0),
    ]
}

/// `ModelEnderman`: a `ModelBiped` with long limbs.
pub(super) fn enderman(a: &Anim, carrying: bool, screaming: bool) -> Vec<Part> {
    let y0 = -14.0f32;
    let hy = if screaming { y0 + 1.0 - 5.0 } else { y0 + 1.0 };
    let hr = [rad(a.head_pitch), rad(a.head_yaw), 0.0];
    let head = Part::new([0.0, hy, 0.0]).bx(0.0, 0.0, [-4.0, -8.0, -4.0], [8.0; 3], 0.0);
    let hat = Part::new([0.0, hy, 0.0]).bx(0.0, 16.0, [-4.0, -8.0, -4.0], [8.0; 3], -0.5);
    let body = Part::new([0.0, y0, 0.0]).bx(32.0, 16.0, [-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], 0.0);
    let limb = |p: [f32; 3], mirror: bool, top: f32| {
        let q = Part::new(p);
        if mirror {
            q.mbx(56.0, 0.0, [-1.0, top, -1.0], [2.0, 30.0, 2.0], 0.0)
        } else {
            q.bx(56.0, 0.0, [-1.0, top, -1.0], [2.0, 30.0, 2.0], 0.0)
        }
    };
    let clamp = |x: f32| (x * 0.5).clamp(-0.4, 0.4);
    let mut ra = limb([-3.0, 2.0 + y0, 0.0], false, -2.0);
    let mut la = limb([5.0, 2.0 + y0, 0.0], true, -2.0);
    let mut rl = limb([-2.0, 9.0 + y0, 0.0], false, 0.0);
    let mut ll = limb([2.0, 9.0 + y0, 0.0], true, 0.0);
    ra.rot[0] =
        clamp(cos(a.swing * 0.6662 + PI) * 2.0 * a.amount * 0.5 + sin(a.age * 0.067) * 0.05);
    la.rot[0] = clamp(cos(a.swing * 0.6662) * 2.0 * a.amount * 0.5 - sin(a.age * 0.067) * 0.05);
    rl.rot[0] = clamp(cos(a.swing * 0.6662) * 1.4 * a.amount);
    ll.rot[0] = clamp(cos(a.swing * 0.6662 + PI) * 1.4 * a.amount);
    ra.rot[2] = cos(a.age * 0.09) * 0.05 + 0.05;
    la.rot[2] = -(cos(a.age * 0.09) * 0.05 + 0.05);
    if carrying {
        ra.rot[0] = -0.5;
        la.rot[0] = -0.5;
        ra.rot[2] = 0.05;
        la.rot[2] = -0.05;
    }
    vec![
        head.r(hr[0], hr[1], 0.0),
        body,
        ra,
        la,
        rl,
        ll,
        hat.r(hr[0], hr[1], 0.0),
    ]
}

/// `ModelSquid` with the tentacle angle.
pub(super) fn squid(tentacle: f32) -> Vec<Part> {
    let mut v =
        vec![Part::new([0.0, 8.0, 0.0]).bx(0.0, 0.0, [-6.0, -8.0, -6.0], [12.0, 16.0, 12.0], 0.0)];
    for i in 0..8 {
        let a = i as f64 * std::f64::consts::PI * 2.0 / 8.0;
        let (x, z) = ((a.cos() * 5.0) as f32, (a.sin() * 5.0) as f32);
        let ry = (i as f64 * std::f64::consts::PI * -2.0 / 8.0 + std::f64::consts::PI / 2.0) as f32;
        v.push(
            Part::new([x, 15.0, z])
                .bx(48.0, 0.0, [-1.0, 0.0, -1.0], [2.0, 18.0, 2.0], 0.0)
                .r(tentacle, ry, 0.0),
        );
    }
    v
}

/// `ModelBat`, hanging upside down or flapping.
pub(super) fn bat(a: &Anim, hanging: bool) -> Vec<Part> {
    let ears = |p: Part| {
        p.bx(24.0, 0.0, [-4.0, -6.0, -2.0], [3.0, 4.0, 1.0], 0.0)
            .mbx(24.0, 0.0, [1.0, -6.0, -2.0], [3.0, 4.0, 1.0], 0.0)
    };
    let mut head = ears(Part::new([0.0; 3]).bx(0.0, 0.0, [-3.0, -3.0, -3.0], [6.0; 3], 0.0));
    let mut body = Part::new([0.0; 3])
        .bx(0.0, 16.0, [-3.0, 4.0, -3.0], [6.0, 12.0, 6.0], 0.0)
        .bx(0.0, 34.0, [-5.0, 16.0, 0.0], [10.0, 6.0, 1.0], 0.0);
    let mut rw = Part::new([0.0; 3]).bx(42.0, 0.0, [-12.0, 1.0, 1.5], [10.0, 16.0, 1.0], 0.0);
    let mut rwo =
        Part::new([-12.0, 1.0, 1.5]).bx(24.0, 16.0, [-8.0, 1.0, 0.0], [8.0, 12.0, 1.0], 0.0);
    let mut lw = Part::new([0.0; 3]).mbx(42.0, 0.0, [2.0, 1.0, 1.5], [10.0, 16.0, 1.0], 0.0);
    let mut lwo =
        Part::new([12.0, 1.0, 1.5]).mbx(24.0, 16.0, [0.0, 1.0, 0.0], [8.0, 12.0, 1.0], 0.0);
    if hanging {
        head.rot = [rad(a.head_pitch), PI - rad(a.head_yaw), PI];
        head.pivot = [0.0, -2.0, 0.0];
        rw.pivot = [-3.0, 0.0, 3.0];
        lw.pivot = [3.0, 0.0, 3.0];
        body.rot[0] = PI;
        rw.rot = [-PI / 20.0, -PI * 2.0 / 5.0, 0.0];
        rwo.rot[1] = -1.7278761;
        lw.rot = [rw.rot[0], -rw.rot[1], 0.0];
        lwo.rot[1] = 1.7278761;
    } else {
        head.rot = [rad(a.head_pitch), rad(a.head_yaw), 0.0];
        body.rot = [PI / 4.0 + cos(a.age * 0.1) * 0.15, 0.0, 0.0];
        rw.rot[1] = cos(a.age * 1.3) * PI * 0.25;
        lw.rot[1] = -rw.rot[1];
        rwo.rot[1] = rw.rot[1] * 0.5;
        lwo.rot[1] = -rw.rot[1] * 0.5;
    }
    rw.children.push(rwo);
    lw.children.push(lwo);
    body.children.push(rw);
    body.children.push(lw);
    vec![head, body]
}

/// `ModelWolf`, standing or sitting.
pub(super) fn wolf(a: &Anim, sitting: bool, angry: bool, tail: f32) -> Vec<Part> {
    let mut head = Part::new([-1.0, 13.5, -7.0])
        .bx(0.0, 0.0, [-3.0, -3.0, -2.0], [6.0, 6.0, 4.0], 0.0)
        .bx(16.0, 14.0, [-3.0, -5.0, 0.0], [2.0, 2.0, 1.0], 0.0)
        .bx(16.0, 14.0, [1.0, -5.0, 0.0], [2.0, 2.0, 1.0], 0.0)
        .bx(0.0, 10.0, [-1.5, 0.0, -5.0], [3.0, 3.0, 4.0], 0.0);
    head.rot = [rad(a.head_pitch), rad(a.head_yaw), 0.0];
    let mut body =
        Part::new([0.0, 14.0, 2.0]).bx(18.0, 14.0, [-4.0, -2.0, -3.0], [6.0, 9.0, 6.0], 0.0);
    let mut mane =
        Part::new([-1.0, 14.0, 2.0]).bx(21.0, 0.0, [-4.0, -3.0, -3.0], [8.0, 6.0, 7.0], 0.0);
    let leg = |p: [f32; 3]| Part::new(p).bx(0.0, 18.0, [-1.0, 0.0, -1.0], [2.0, 8.0, 2.0], 0.0);
    let mut l1 = leg([-2.5, 16.0, 7.0]);
    let mut l2 = leg([0.5, 16.0, 7.0]);
    let mut l3 = leg([-2.5, 16.0, -4.0]);
    let mut l4 = leg([0.5, 16.0, -4.0]);
    let mut tl =
        Part::new([-1.0, 12.0, 8.0]).bx(9.0, 18.0, [-1.0, 0.0, -1.0], [2.0, 8.0, 2.0], 0.0);
    tl.rot[1] = if angry {
        0.0
    } else {
        cos(a.swing * 0.6662) * 1.4 * a.amount
    };
    if sitting {
        mane.pivot = [-1.0, 16.0, -3.0];
        mane.rot = [PI * 2.0 / 5.0, 0.0, 0.0];
        body.pivot = [0.0, 18.0, 0.0];
        body.rot[0] = PI / 4.0;
        tl.pivot = [-1.0, 21.0, 6.0];
        l1.pivot = [-2.5, 22.0, 2.0];
        l1.rot[0] = PI * 3.0 / 2.0;
        l2.pivot = [0.5, 22.0, 2.0];
        l2.rot[0] = PI * 3.0 / 2.0;
        l3.rot[0] = 5.811947;
        l3.pivot = [-2.49, 17.0, -4.0];
        l4.rot[0] = 5.811947;
        l4.pivot = [0.51, 17.0, -4.0];
    } else {
        body.rot[0] = PI / 2.0;
        mane.pivot = [-1.0, 14.0, -3.0];
        mane.rot[0] = PI / 2.0;
        l1.rot[0] = cos(a.swing * 0.6662) * 1.4 * a.amount;
        l2.rot[0] = cos(a.swing * 0.6662 + PI) * 1.4 * a.amount;
        l3.rot[0] = cos(a.swing * 0.6662 + PI) * 1.4 * a.amount;
        l4.rot[0] = cos(a.swing * 0.6662) * 1.4 * a.amount;
    }
    tl.rot[0] = tail;
    vec![head, body, l1, l2, l3, l4, tl, mane]
}

/// `ModelOcelot`, walking (state 1) or sitting (state 3).
pub(super) fn ocelot(a: &Anim, sitting: bool) -> Vec<Part> {
    let mut head = Part::new([0.0, 15.0, -9.0])
        .bx(0.0, 0.0, [-2.5, -2.0, -3.0], [5.0, 4.0, 5.0], 0.0)
        .bx(0.0, 24.0, [-1.5, 0.0, -4.0], [3.0, 2.0, 2.0], 0.0)
        .bx(0.0, 10.0, [-2.0, -3.0, 0.0], [1.0, 1.0, 2.0], 0.0)
        .bx(6.0, 10.0, [1.0, -3.0, 0.0], [1.0, 1.0, 2.0], 0.0);
    let mut body =
        Part::new([0.0, 12.0, -10.0]).bx(20.0, 0.0, [-2.0, 3.0, -8.0], [4.0, 16.0, 6.0], 0.0);
    let mut t1 = Part::new([0.0, 15.0, 8.0]).bx(0.0, 15.0, [-0.5, 0.0, 0.0], [1.0, 8.0, 1.0], 0.0);
    let mut t2 = Part::new([0.0, 20.0, 14.0]).bx(4.0, 15.0, [-0.5, 0.0, 0.0], [1.0, 8.0, 1.0], 0.0);
    let mut bl = Part::new([1.1, 18.0, 5.0]).bx(8.0, 13.0, [-1.0, 0.0, 1.0], [2.0, 6.0, 2.0], 0.0);
    let mut br = Part::new([-1.1, 18.0, 5.0]).bx(8.0, 13.0, [-1.0, 0.0, 1.0], [2.0, 6.0, 2.0], 0.0);
    let mut fl =
        Part::new([1.2, 13.8, -5.0]).bx(40.0, 0.0, [-1.0, 0.0, 0.0], [2.0, 10.0, 2.0], 0.0);
    let mut fr =
        Part::new([-1.2, 13.8, -5.0]).bx(40.0, 0.0, [-1.0, 0.0, 0.0], [2.0, 10.0, 2.0], 0.0);
    t1.rot[0] = 0.9;
    if sitting {
        body.rot[0] = PI / 4.0;
        body.pivot[1] += -4.0;
        body.pivot[2] += 5.0;
        head.pivot[1] += -3.3;
        head.pivot[2] += 1.0;
        t1.pivot[1] += 8.0;
        t1.pivot[2] += -2.0;
        t2.pivot[1] += 2.0;
        t2.pivot[2] += -0.8;
        t1.rot[0] = 1.7278761;
        t2.rot[0] = 2.670354;
        fl.rot[0] = -PI / 20.0;
        fr.rot[0] = -PI / 20.0;
        fl.pivot = [1.2, 15.8, -7.0];
        fr.pivot = [-1.2, 15.8, -7.0];
        bl.rot[0] = -PI / 2.0;
        br.rot[0] = -PI / 2.0;
        bl.pivot = [1.1, 21.0, 1.0];
        br.pivot = [-1.1, 21.0, 1.0];
    } else {
        body.rot[0] = PI / 2.0;
        bl.rot[0] = cos(a.swing * 0.6662) * a.amount;
        br.rot[0] = cos(a.swing * 0.6662 + PI) * a.amount;
        fl.rot[0] = cos(a.swing * 0.6662 + PI) * a.amount;
        fr.rot[0] = cos(a.swing * 0.6662) * a.amount;
        t2.rot[0] = 1.7278761 + PI / 4.0 * cos(a.swing) * a.amount;
    }
    head.rot = [rad(a.head_pitch), rad(a.head_yaw), 0.0];
    vec![head, body, t1, t2, bl, br, fl, fr]
}

/// `ModelHorse` for an adult without saddle or chests.
pub(super) fn horse(a: &Anim, mule_ears: bool) -> Vec<Part> {
    let mut yaw = a.head_yaw.clamp(-20.0, 20.0);
    yaw = rad(yaw);
    let mut pitch = rad(a.head_pitch);
    if a.amount > 0.2 {
        pitch += cos(a.swing * 0.4) * 0.15 * a.amount;
    }
    let hrx = PI / 6.0 + pitch;
    let hp = [0.0, 4.0, -10.0];
    let head = Part::new(hp)
        .bx(0.0, 0.0, [-2.5, -10.0, -1.5], [5.0, 5.0, 7.0], 0.0)
        .r(hrx, yaw, 0.0);
    let mut head = head;
    head.children.push(Part::new([0.0, 0.02, 0.02]).bx(
        24.0,
        18.0,
        [-2.0, -10.0, -7.0],
        [4.0, 3.0, 6.0],
        0.0,
    ));
    head.children.push(Part::new([0.0, 0.0, 0.0]).bx(
        24.0,
        27.0,
        [-2.0, -7.0, -6.5],
        [4.0, 2.0, 5.0],
        0.0,
    ));
    let ears = if mule_ears {
        vec![
            Part::new(hp)
                .bx(0.0, 12.0, [-2.0, -16.0, 4.0], [2.0, 7.0, 1.0], 0.0)
                .r(hrx, yaw, PI / 12.0),
            Part::new(hp)
                .bx(0.0, 12.0, [0.0, -16.0, 4.0], [2.0, 7.0, 1.0], 0.0)
                .r(hrx, yaw, -PI / 12.0),
        ]
    } else {
        vec![
            Part::new(hp)
                .bx(0.0, 0.0, [0.45, -12.0, 4.0], [2.0, 3.0, 1.0], 0.0)
                .r(hrx, yaw, 0.0),
            Part::new(hp)
                .bx(0.0, 0.0, [-2.45, -12.0, 4.0], [2.0, 3.0, 1.0], 0.0)
                .r(hrx, yaw, 0.0),
        ]
    };
    let neck = Part::new(hp)
        .bx(0.0, 12.0, [-2.05, -9.8, -2.0], [4.0, 14.0, 8.0], 0.0)
        .r(hrx, yaw, 0.0);
    let mane = Part::new(hp)
        .bx(58.0, 0.0, [-1.0, -11.5, 5.0], [2.0, 16.0, 4.0], 0.0)
        .r(hrx, yaw, 0.0);
    let body =
        Part::new([0.0, 11.0, 9.0]).bx(0.0, 34.0, [-5.0, -8.0, -19.0], [10.0, 10.0, 24.0], 0.0);
    let mut tail = (-1.3089 + a.amount * 1.5).min(0.0);
    if tail > 0.0 {
        tail = 0.0;
    }
    let tp = [0.0, 3.0, 14.0];
    let tails = vec![
        Part::new(tp)
            .bx(44.0, 0.0, [-1.0, -1.0, 0.0], [2.0, 2.0, 3.0], 0.0)
            .r(tail, 0.0, 0.0),
        Part::new(tp)
            .bx(38.0, 7.0, [-1.5, -2.0, 3.0], [3.0, 4.0, 7.0], 0.0)
            .r(tail, 0.0, 0.0),
        Part::new(tp)
            .bx(24.0, 3.0, [-1.5, -4.5, 9.0], [3.0, 4.0, 7.0], 0.0)
            .r(-0.2618 + tail, 0.0, 0.0),
    ];
    let v19 = cos(a.swing * 0.6662 + 3.141593);
    let v20 = v19 * 0.8 * a.amount;
    let half = v19 * 0.5 * a.amount;
    let mut v = Vec::new();
    // back legs
    for (x, u, sign, leg_box, shin_box, hoof_box) in [
        (
            4.0f32,
            78.0f32,
            -1.0f32,
            [-2.5f32, -2.0, -2.5],
            [-2.0f32, 0.0, -1.5],
            [-2.5f32, 5.1, -2.0],
        ),
        (
            -4.0,
            96.0,
            1.0,
            [-1.5, -2.0, -2.5],
            [-1.0, 0.0, -1.5],
            [-1.5, 5.1, -2.0],
        ),
    ] {
        let lrx = sign * half;
        let srx = sign * half - (-sign * half).max(0.0);
        let sy = 9.0 + sin(PI / 2.0 + lrx) * 7.0;
        let sz = 11.0 + cos(PI * 3.0 / 2.0 + lrx) * 7.0;
        v.push(
            Part::new([x, 9.0, 11.0])
                .bx(u, 29.0, leg_box, [4.0, 9.0, 5.0], 0.0)
                .r(lrx, 0.0, 0.0),
        );
        v.push(
            Part::new([x, sy, sz])
                .bx(u, 43.0, shin_box, [3.0, 5.0, 3.0], 0.0)
                .r(srx, 0.0, 0.0),
        );
        v.push(
            Part::new([x, sy, sz])
                .bx(u, 51.0, hoof_box, [4.0, 3.0, 4.0], 0.0)
                .r(srx, 0.0, 0.0),
        );
    }
    // front legs
    for (x, u, sign, leg_box, shin_box, hoof_box) in [
        (
            4.0f32,
            44.0f32,
            1.0f32,
            [-1.9f32, -1.0, -2.1],
            [-1.9f32, 0.0, -1.6],
            [-2.4f32, 5.1, -2.1],
        ),
        (
            -4.0,
            60.0,
            -1.0,
            [-1.1, -1.0, -2.1],
            [-1.1, 0.0, -1.6],
            [-1.6, 5.1, -2.1],
        ),
    ] {
        let lrx = sign * v20;
        let srx = sign * v20 + (sign * half).max(0.0);
        let sy = 9.0 + sin(PI / 2.0 + lrx) * 7.0;
        let sz = -8.0 + cos(PI * 3.0 / 2.0 + lrx) * 7.0;
        v.push(
            Part::new([x, 9.0, -8.0])
                .bx(u, 29.0, leg_box, [3.0, 8.0, 4.0], 0.0)
                .r(lrx, 0.0, 0.0),
        );
        v.push(
            Part::new([x, sy, sz])
                .bx(u, 41.0, shin_box, [3.0, 5.0, 3.0], 0.0)
                .r(srx, 0.0, 0.0),
        );
        v.push(
            Part::new([x, sy, sz])
                .bx(u, 51.0, hoof_box, [4.0, 3.0, 4.0], 0.0)
                .r(srx, 0.0, 0.0),
        );
    }
    v.push(body);
    v.extend(tails);
    v.push(neck);
    v.push(mane);
    v.extend(ears);
    v.push(head);
    v
}

/// `ModelIronGolem.triangleWave`
fn tri(x: f32, p: f32) -> f32 {
    ((x % p - p * 0.5).abs() - p * 0.25) / (p * 0.25)
}

/// `ModelIronGolem`
pub(super) fn iron_golem(a: &Anim, attack: f32) -> Vec<Part> {
    let head = Part::new([0.0, -7.0, -2.0])
        .bx(0.0, 0.0, [-4.0, -12.0, -5.5], [8.0, 10.0, 8.0], 0.0)
        .bx(24.0, 0.0, [-1.0, -5.0, -7.5], [2.0, 4.0, 2.0], 0.0)
        .r(rad(a.head_pitch), rad(a.head_yaw), 0.0);
    let body = Part::new([0.0, -7.0, 0.0])
        .bx(0.0, 40.0, [-9.0, -2.0, -6.0], [18.0, 12.0, 11.0], 0.0)
        .bx(0.0, 70.0, [-4.5, 10.0, -3.0], [9.0, 5.0, 6.0], 0.5);
    let (rax, lax) = if attack > 0.0 {
        let t = -2.0 + 1.5 * tri(attack, 10.0);
        (t, t)
    } else {
        (
            (-0.2 + 1.5 * tri(a.swing, 13.0)) * a.amount,
            (-0.2 - 1.5 * tri(a.swing, 13.0)) * a.amount,
        )
    };
    let ra = Part::new([0.0, -7.0, 0.0])
        .bx(60.0, 21.0, [-13.0, -2.5, -3.0], [4.0, 30.0, 6.0], 0.0)
        .r(rax, 0.0, 0.0);
    let la = Part::new([0.0, -7.0, 0.0])
        .bx(60.0, 58.0, [9.0, -2.5, -3.0], [4.0, 30.0, 6.0], 0.0)
        .r(lax, 0.0, 0.0);
    let ll = Part::new([-4.0, 11.0, 0.0])
        .bx(37.0, 0.0, [-3.5, -3.0, -3.0], [6.0, 16.0, 5.0], 0.0)
        .r(-1.5 * tri(a.swing, 13.0) * a.amount, 0.0, 0.0);
    let rl = Part::new([5.0, 11.0, 0.0])
        .mbx(60.0, 0.0, [-3.5, -3.0, -3.0], [6.0, 16.0, 5.0], 0.0)
        .r(1.5 * tri(a.swing, 13.0) * a.amount, 0.0, 0.0);
    vec![head, body, ll, rl, ra, la]
}

/// Parts of the newer mobs for one texture layer.
pub(super) fn parts(m: &MobView, a: &Anim, layer: u8, partial: f32) -> Vec<Part> {
    let tamed = m.flags & 1 != 0;
    let sitting = m.flags & 2 != 0;
    let angry = m.flags & 4 != 0;
    match m.kind {
        Kind::Slime => slime(layer),
        Kind::Enderman => enderman(a, m.carried[0] != 0, angry),
        Kind::Squid => squid(m.sq[0][3] + (m.sq[1][3] - m.sq[0][3]) * partial),
        Kind::Bat => bat(a, m.flags & 8 != 0),
        Kind::Wolf => {
            // EntityWolf.getTailRotation
            let tail = if angry {
                1.5393804
            } else if tamed {
                (0.55 - (20.0 - m.health) * 0.02) * PI
            } else {
                PI / 5.0
            };
            wolf(a, sitting, angry, tail)
        }
        Kind::Ocelot => ocelot(a, sitting),
        Kind::Horse => horse(a, (m.variant >> 16) == 1 || (m.variant >> 16) == 2),
        Kind::IronGolem => {
            let t = if m.attack_timer > 0 {
                m.attack_timer as f32 - partial
            } else {
                0.0
            };
            iron_golem(a, t)
        }
        _ => Vec::new(),
    }
}

/// Texture of each layer: base and overlay (slime outer shell, enderman
/// eyes, wolf collar, horse markings).
pub fn texture(m: &MobView, layer: u8) -> Option<&'static str> {
    let tamed = m.flags & 1 != 0;
    let angry = m.flags & 4 != 0;
    Some(match (m.kind, layer) {
        (Kind::Slime, _) => "slime/slime",
        (Kind::Enderman, 0) => "enderman/enderman",
        (Kind::Enderman, _) => "enderman/enderman_eyes",
        (Kind::Squid, _) => "squid",
        (Kind::Bat, _) => "bat",
        (Kind::Wolf, 0) => {
            if tamed {
                "wolf/wolf_tame"
            } else if angry {
                "wolf/wolf_angry"
            } else {
                "wolf/wolf"
            }
        }
        (Kind::Wolf, _) => "wolf/wolf_collar",
        (Kind::Ocelot, _) => {
            ["cat/ocelot", "cat/black", "cat/red", "cat/siamese"][m.variant.clamp(0, 3) as usize]
        }
        (Kind::Horse, 0) => match m.variant >> 16 {
            1 => "horse/donkey",
            2 => "horse/mule",
            _ => [
                "horse/horse_white",
                "horse/horse_creamy",
                "horse/horse_chestnut",
                "horse/horse_brown",
                "horse/horse_black",
                "horse/horse_gray",
                "horse/horse_darkbrown",
            ][(m.variant & 7).min(6) as usize],
        },
        (Kind::Horse, _) => match (m.variant >> 8) & 7 {
            1 => "horse/horse_markings_white",
            2 => "horse/horse_markings_whitefield",
            3 => "horse/horse_markings_whitedots",
            4 => "horse/horse_markings_blackdots",
            _ => return None,
        },
        (Kind::IronGolem, _) => "iron_golem",
        _ => return None,
    })
}

/// Layers drawn for a mob.
pub fn layers(m: &MobView) -> &'static [u8] {
    let tamed = m.flags & 1 != 0;
    match m.kind {
        Kind::Slime | Kind::Enderman => &[0, 1],
        Kind::Wolf if tamed => &[0, 1],
        Kind::Horse if (m.variant >> 16) == 0 && (m.variant >> 8) & 7 != 0 => &[0, 1],
        _ => &[0],
    }
}

pub const TEXTURES: [&str; 25] = [
    "slime/slime",
    "enderman/enderman",
    "enderman/enderman_eyes",
    "squid",
    "bat",
    "wolf/wolf",
    "wolf/wolf_tame",
    "wolf/wolf_angry",
    "wolf/wolf_collar",
    "cat/ocelot",
    "cat/black",
    "cat/red",
    "cat/siamese",
    "horse/horse_white",
    "horse/horse_creamy",
    "horse/horse_chestnut",
    "horse/horse_brown",
    "horse/horse_black",
    "horse/horse_gray",
    "horse/horse_darkbrown",
    "horse/horse_markings_white",
    "horse/horse_markings_whitefield",
    "horse/horse_markings_whitedots",
    "horse/horse_markings_blackdots",
    "horse/donkey",
];

pub const TEXTURES2: [&str; 2] = ["horse/mule", "iron_golem"];
