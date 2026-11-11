//! Port of `net.minecraft.util.MathHelper` (table-based sin/cos and Java-style floors).

use std::sync::OnceLock;

static SIN_TABLE: OnceLock<Box<[f32; 65536]>> = OnceLock::new();

fn table() -> &'static [f32; 65536] {
    SIN_TABLE.get_or_init(|| {
        let mut t = Box::new([0f32; 65536]);
        for i in 0..65536 {
            t[i] = (i as f64 * std::f64::consts::PI * 2.0 / 65536.0).sin() as f32;
        }
        t
    })
}

pub const PI: f32 = std::f64::consts::PI as f32;

#[inline]
pub fn sin(f: f32) -> f32 {
    table()[((f * 10430.378f32) as i32 & 65535) as usize]
}

#[inline]
pub fn cos(f: f32) -> f32 {
    table()[((f * 10430.378f32 + 16384.0f32) as i32 & 65535) as usize]
}

#[inline]
pub fn sqrt_float(f: f32) -> f32 {
    (f as f64).sqrt() as f32
}

#[inline]
pub fn sqrt_double(d: f64) -> f32 {
    d.sqrt() as f32
}

#[inline]
pub fn floor_float(f: f32) -> i32 {
    let i = f as i32;
    if f < i as f32 { i - 1 } else { i }
}

#[inline]
pub fn floor_double(d: f64) -> i32 {
    let i = d as i32;
    if d < i as f64 { i - 1 } else { i }
}

#[inline]
pub fn floor_double_long(d: f64) -> i64 {
    let i = d as i64;
    if d < i as f64 { i - 1 } else { i }
}

#[inline]
pub fn denormalize_clamp(a: f64, b: f64, t: f64) -> f64 {
    if t < 0.0 {
        a
    } else if t > 1.0 {
        b
    } else {
        a + (b - a) * t
    }
}

#[inline]
pub fn clamp_int(v: i32, lo: i32, hi: i32) -> i32 {
    if v < lo { lo } else if v > hi { hi } else { v }
}

#[inline]
pub fn clamp_float(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo { lo } else if v > hi { hi } else { v }
}

/// `MathHelper.wrapAngleTo180_float`
pub fn wrap_angle_180(mut a: f32) -> f32 {
    a %= 360.0;
    if a >= 180.0 {
        a -= 360.0;
    }
    if a < -180.0 {
        a += 360.0;
    }
    a
}

/// Java `Math.round(double)`: floor(x + 0.5) as long.
#[inline]
pub fn java_round(d: f64) -> i64 {
    (d + 0.5).floor() as i64
}
