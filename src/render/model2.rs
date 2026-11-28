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
pub(super) fn wolf(a: &Anim, sitting: bool, angry: bool, tail: f32, tilt: [f32; 4]) -> Vec<Part> {
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
    // setLivingAnimations: the begging tilt and the shake
    head.rot[2] = tilt[0];
    mane.rot[2] = tilt[1];
    body.rot[2] = tilt[2];
    tl.rot[2] = tilt[3];
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

/// `ModelHorse` posed by `setLivingAnimations` for `EntityHorse` `mv`:
/// each part with its group for `render`'s child scaling (0 saddle and
/// reins, 1 legs, 2 body, 3 head, 4 chests).
pub(super) fn horse_groups(mv: &MobView, a: &Anim, partial: f32) -> Vec<(u8, Part)> {
    let htype = mv.variant >> 16;
    let lerp = |p: f32, c: f32| p + (c - p) * partial;
    let lean = lerp(mv.hanim[0], mv.hanim[1]);
    let rear = lerp(mv.hanim[2], mv.hanim[3]);
    let mouth = lerp(mv.hanim[4], mv.hanim[5]);
    let not_rear = 1.0 - rear;
    let tail_wag = mv.tail_counter != 0;
    let saddled = mv.horse_flags & 4 != 0;
    let ridden = mv.ridden;
    let adult = mv.flags & 16 == 0;
    let chested = adult && mv.horse_flags & 8 != 0;
    let saddle_shown = adult && saddled;
    let mule = htype == 1 || htype == 2;
    let (swing, amount, age) = (a.swing, a.amount, a.age);
    let yaw_d = a.head_yaw.clamp(-20.0, 20.0);
    let mut pitch = rad(a.head_pitch);
    if amount > 0.2 {
        pitch += cos(swing * 0.4) * 0.15 * amount;
    }
    let v19 = cos(swing * 0.6662 + 3.141593);
    let v20 = v19 * 0.8 * amount;
    let mx = rear.max(lean);
    let head_rx = rear * (PI / 12.0 + pitch) + lean * 2.18166 + (1.0 - mx) * (PI / 6.0 + pitch);
    let head_ry = rear * rad(yaw_d) + (1.0 - mx) * rad(yaw_d);
    let head_py = rear * -6.0 + lean * 11.0 + (1.0 - mx) * 4.0;
    let head_pz = rear * -1.0 + lean * -10.0 + (1.0 - mx) * -10.0;
    let tail_py = rear * 9.0 + not_rear * 3.0;
    let chest_r_py = rear * 5.5 + not_rear * 3.0;
    let chest_r_pz = rear * 15.0 + not_rear * 10.0;
    let body_rx = rear * -0.7853981;
    let mut v: Vec<(u8, Part)> = Vec::new();
    let hp = [0.0, head_py, head_pz];
    let mut head = Part::new(hp)
        .bx(0.0, 0.0, [-2.5, -10.0, -1.5], [5.0, 5.0, 7.0], 0.0)
        .r(head_rx, head_ry, 0.0);
    head.children.push(
        Part::new([0.0, 0.02, 0.02 - mouth])
            .bx(24.0, 18.0, [-2.0, -10.0, -7.0], [4.0, 3.0, 6.0], 0.0)
            .r(-0.09424778 * mouth, 0.0, 0.0),
    );
    head.children.push(
        Part::new([0.0, 0.0, mouth])
            .bx(24.0, 27.0, [-2.0, -7.0, -6.5], [4.0, 2.0, 5.0], 0.0)
            .r(PI / 20.0 * mouth, 0.0, 0.0),
    );
    // render(): the saddle, its straps and the reins, unscaled
    if saddle_shown {
        let sy = rear * 0.5 + not_rear * 2.0;
        let sz = rear * 11.0 + not_rear * 2.0;
        let (rope_rx, rope_rz) = if ridden { (-PI / 3.0, 0.0) } else { (v20 / 3.0, v20 / 5.0) };
        let hr = |u: f32, w: f32, o: [f32; 3], s: [f32; 3], g: f32| {
            Part::new(hp).bx(u, w, o, s, g).r(head_rx, head_ry, 0.0)
        };
        let sd = |u: f32, w: f32, o: [f32; 3], s: [f32; 3]| {
            Part::new([0.0, sy, sz]).bx(u, w, o, s, 0.0).r(body_rx, 0.0, 0.0)
        };
        v.push((0, hr(80.0, 12.0, [-2.5, -10.1, -7.0], [5.0, 5.0, 12.0], 0.2)));
        v.push((0, sd(80.0, 0.0, [-5.0, 0.0, -3.0], [10.0, 1.0, 8.0])));
        v.push((0, sd(106.0, 9.0, [-1.5, -1.0, -3.0], [3.0, 1.0, 2.0])));
        v.push((0, sd(80.0, 9.0, [-4.0, -1.0, 3.0], [8.0, 1.0, 2.0])));
        let rope = |x: f32, u: f32, w: f32, o: [f32; 3], s: [f32; 3], rz: f32| {
            Part::new([x, sy, sz]).bx(u, w, o, s, 0.0).r(rope_rx, 0.0, rz)
        };
        v.push((0, rope(5.0, 70.0, 0.0, [-0.5, 0.0, -0.5], [1.0, 6.0, 1.0], rope_rz)));
        v.push((0, rope(5.0, 74.0, 0.0, [-0.5, 6.0, -1.0], [1.0, 2.0, 2.0], rope_rz)));
        v.push((0, rope(-5.0, 80.0, 0.0, [-0.5, 0.0, -0.5], [1.0, 6.0, 1.0], -rope_rz)));
        v.push((0, rope(-5.0, 74.0, 4.0, [-0.5, 6.0, -1.0], [1.0, 2.0, 2.0], -rope_rz)));
        v.push((0, hr(74.0, 13.0, [1.5, -8.0, -4.0], [1.0, 2.0, 2.0], 0.0)));
        v.push((0, hr(74.0, 13.0, [-2.5, -8.0, -4.0], [1.0, 2.0, 2.0], 0.0)));
        if ridden {
            v.push((0, Part::new(hp).bx(44.0, 10.0, [2.6, -6.0, -6.0], [0.0, 3.0, 16.0], 0.0).r(pitch, head_ry, 0.0)));
            v.push((0, Part::new(hp).bx(44.0, 5.0, [-2.6, -6.0, -6.0], [0.0, 3.0, 16.0], 0.0).r(pitch, head_ry, 0.0)));
        }
    }
    // the legs
    let v24 = PI / 12.0 * rear;
    let v25 = cos(age * 0.6 + 3.141593);
    let h = v19 * 0.5 * amount;
    let fl_py = -2.0 * rear + 9.0 * not_rear;
    let fl_pz = -2.0 * rear + -8.0 * not_rear;
    let bl_s_py = 9.0 + sin(PI / 2.0 + v24 + not_rear * -h) * 7.0;
    let bl_s_pz = 11.0 + cos(PI * 3.0 / 2.0 + v24 + not_rear * -h) * 7.0;
    let br_s_py = 9.0 + sin(PI / 2.0 + v24 + not_rear * h) * 7.0;
    let br_s_pz = 11.0 + cos(PI * 3.0 / 2.0 + v24 + not_rear * h) * 7.0;
    let v26 = (-PI / 3.0 + v25) * rear + v20 * not_rear;
    let v27 = (-PI / 3.0 - v25) * rear - v20 * not_rear;
    let fl_s_py = fl_py + sin(PI / 2.0 + v26) * 7.0;
    let fl_s_pz = fl_pz + cos(PI * 3.0 / 2.0 + v26) * 7.0;
    let fr_s_py = fl_py + sin(PI / 2.0 + v27) * 7.0;
    let fr_s_pz = fl_pz + cos(PI * 3.0 / 2.0 + v27) * 7.0;
    let bl_rx = v24 - h * not_rear;
    let bl_s_rx = -0.08726646 * rear + (-h - h.max(0.0)) * not_rear;
    let br_rx = v24 + h * not_rear;
    let br_s_rx = -0.08726646 * rear + (h - (-h).max(0.0)) * not_rear;
    let fl_s_rx = (v26 + PI * (0.2 + v25 * 0.2).max(0.0)) * rear + (v20 + h.max(0.0)) * not_rear;
    let fr_s_rx = (v27 + PI * (0.2 - v25 * 0.2).max(0.0)) * rear + (-v20 + (-h).max(0.0)) * not_rear;
    let leg = |u: f32, w: f32, p: [f32; 3], o: [f32; 3], s: [f32; 3], rx: f32| {
        Part::new(p).bx(u, w, o, s, 0.0).r(rx, 0.0, 0.0)
    };
    v.push((1, leg(78.0, 29.0, [4.0, 9.0, 11.0], [-2.5, -2.0, -2.5], [4.0, 9.0, 5.0], bl_rx)));
    v.push((1, leg(78.0, 43.0, [4.0, bl_s_py, bl_s_pz], [-2.0, 0.0, -1.5], [3.0, 5.0, 3.0], bl_s_rx)));
    v.push((1, leg(78.0, 51.0, [4.0, bl_s_py, bl_s_pz], [-2.5, 5.1, -2.0], [4.0, 3.0, 4.0], bl_s_rx)));
    v.push((1, leg(96.0, 29.0, [-4.0, 9.0, 11.0], [-1.5, -2.0, -2.5], [4.0, 9.0, 5.0], br_rx)));
    v.push((1, leg(96.0, 43.0, [-4.0, br_s_py, br_s_pz], [-1.0, 0.0, -1.5], [3.0, 5.0, 3.0], br_s_rx)));
    v.push((1, leg(96.0, 51.0, [-4.0, br_s_py, br_s_pz], [-1.5, 5.1, -2.0], [4.0, 3.0, 4.0], br_s_rx)));
    v.push((1, leg(44.0, 29.0, [4.0, fl_py, fl_pz], [-1.9, -1.0, -2.1], [3.0, 8.0, 4.0], v26)));
    v.push((1, leg(44.0, 41.0, [4.0, fl_s_py, fl_s_pz], [-1.9, 0.0, -1.6], [3.0, 5.0, 3.0], fl_s_rx)));
    v.push((1, leg(44.0, 51.0, [4.0, fl_s_py, fl_s_pz], [-2.4, 5.1, -2.1], [4.0, 3.0, 4.0], fl_s_rx)));
    v.push((1, leg(60.0, 29.0, [-4.0, fl_py, fl_pz], [-1.1, -1.0, -2.1], [3.0, 8.0, 4.0], v27)));
    v.push((1, leg(60.0, 41.0, [-4.0, fr_s_py, fr_s_pz], [-1.1, 0.0, -1.6], [3.0, 5.0, 3.0], fr_s_rx)));
    v.push((1, leg(60.0, 51.0, [-4.0, fr_s_py, fr_s_pz], [-1.6, 5.1, -2.1], [4.0, 3.0, 4.0], fr_s_rx)));
    // the body, the tail (all three at tailBase's point), neck and mane
    let mut tail = (-1.3089 + amount * 1.5).min(0.0);
    let tail_ry = if tail_wag {
        tail = 0.0;
        cos(age * 0.7)
    } else {
        0.0
    };
    let tp = [0.0, tail_py, 14.0];
    v.push((2, Part::new([0.0, 11.0, 9.0]).bx(0.0, 34.0, [-5.0, -8.0, -19.0], [10.0, 10.0, 24.0], 0.0).r(body_rx, 0.0, 0.0)));
    v.push((2, Part::new(tp).bx(44.0, 0.0, [-1.0, -1.0, 0.0], [2.0, 2.0, 3.0], 0.0).r(tail, tail_ry, 0.0)));
    v.push((2, Part::new(tp).bx(38.0, 7.0, [-1.5, -2.0, 3.0], [3.0, 4.0, 7.0], 0.0).r(tail, tail_ry, 0.0)));
    v.push((2, Part::new(tp).bx(24.0, 3.0, [-1.5, -4.5, 9.0], [3.0, 4.0, 7.0], 0.0).r(-0.2618 + tail, tail_ry, 0.0)));
    v.push((2, Part::new(hp).bx(0.0, 12.0, [-2.05, -9.8, -2.0], [4.0, 14.0, 8.0], 0.0).r(head_rx, head_ry, 0.0)));
    v.push((2, Part::new(hp).bx(58.0, 0.0, [-1.0, -11.5, 5.0], [2.0, 16.0, 4.0], 0.0).r(head_rx, head_ry, 0.0)));
    // the ears and the head
    if mule {
        v.push((3, Part::new(hp).bx(0.0, 12.0, [-2.0, -16.0, 4.0], [2.0, 7.0, 1.0], 0.0).r(head_rx, head_ry, PI / 12.0)));
        v.push((3, Part::new(hp).bx(0.0, 12.0, [0.0, -16.0, 4.0], [2.0, 7.0, 1.0], 0.0).r(head_rx, head_ry, -PI / 12.0)));
    } else {
        v.push((3, Part::new(hp).bx(0.0, 0.0, [0.45, -12.0, 4.0], [2.0, 3.0, 1.0], 0.0).r(head_rx, head_ry, 0.0)));
        v.push((3, Part::new(hp).bx(0.0, 0.0, [-2.45, -12.0, 4.0], [2.0, 3.0, 1.0], 0.0).r(head_rx, head_ry, 0.0)));
    }
    v.push((3, head));
    // the donkey's and mule's chests (the left one's point only follows
    // the right while saddled)
    if chested {
        let (lpy, lpz) = if saddled { (chest_r_py, chest_r_pz) } else { (3.0, 10.0) };
        v.push((4, Part::new([-7.5, lpy, lpz]).bx(0.0, 34.0, [-3.0, 0.0, 0.0], [8.0, 8.0, 3.0], 0.0).r(v20 / 5.0, 1.570796, 0.0)));
        v.push((4, Part::new([4.5, chest_r_py, chest_r_pz]).bx(0.0, 47.0, [-3.0, 0.0, 0.0], [8.0, 8.0, 3.0], 0.0).r(-v20 / 5.0, 1.570796, 0.0)));
    }
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
            // getShakeAngle(partial, offset) and getInterestedAngle
            let t = m.shake[0] + (m.shake[1] - m.shake[0]) * partial;
            let shake = |off: f32| {
                let k = ((t + off) / 1.8).clamp(0.0, 1.0);
                sin(k * PI) * sin(k * PI * 11.0) * 0.15 * PI
            };
            let interested = (m.head_course[0] + (m.head_course[1] - m.head_course[0]) * partial) * 0.15 * PI;
            wolf(
                a,
                sitting,
                angry,
                tail,
                [interested + shake(0.0), shake(-0.08), shake(-0.16), shake(-0.2)],
            )
        }
        Kind::Ocelot => ocelot(a, sitting),
        Kind::Horse => horse_groups(m, a, partial).into_iter().map(|g| g.1).collect(),
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
            3 => "horse/horse_zombie",
            4 => "horse/horse_skeleton",
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
        // RenderHorse: the armour layer of the layered texture
        (Kind::Horse, 2) => match m.horse_armor {
            1 => "horse/armor/horse_armor_iron",
            2 => "horse/armor/horse_armor_gold",
            3 => "horse/armor/horse_armor_diamond",
            _ => return None,
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
        Kind::Horse if (m.variant >> 16) == 0 => {
            match ((m.variant >> 8) & 7 != 0, m.horse_armor > 0) {
                (true, true) => &[0, 1, 2],
                (true, false) => &[0, 1],
                (false, true) => &[0, 2],
                (false, false) => &[0],
            }
        }
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

pub const TEXTURES2: [&str; 7] = [
    "horse/mule",
    "iron_golem",
    "horse/horse_zombie",
    "horse/horse_skeleton",
    "horse/armor/horse_armor_iron",
    "horse/armor/horse_armor_gold",
    "horse/armor/horse_armor_diamond",
];
