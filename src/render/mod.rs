pub mod atlas;
pub mod entities;
pub mod mesher;
pub mod model;
pub mod vk;

/// Column-major 4x4 matrix (OpenGL convention).
pub type Mat4 = [f32; 16];

pub const IDENTITY: Mat4 = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

/// `a * b`
pub fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut m = [0f32; 16];
    for c in 0..4 {
        for r in 0..4 {
            let mut s = 0.0;
            for k in 0..4 {
                s += a[k * 4 + r] * b[c * 4 + k];
            }
            m[c * 4 + r] = s;
        }
    }
    m
}

/// `gluPerspective` adapted to Vulkan clip space (Y down, depth 0..1).
pub fn perspective(fovy_deg: f32, aspect: f32, zn: f32, zf: f32) -> Mat4 {
    let f = 1.0 / (fovy_deg.to_radians() * 0.5).tan();
    let mut m = [0f32; 16];
    m[0] = f / aspect;
    m[5] = -f;
    m[10] = zf / (zn - zf);
    m[11] = -1.0;
    m[14] = zn * zf / (zn - zf);
    m
}

/// `glRotatef(deg, 1, 0, 0)`
pub fn rot_x(deg: f32) -> Mat4 {
    let (s, c) = deg.to_radians().sin_cos();
    let mut m = IDENTITY;
    m[5] = c;
    m[6] = s;
    m[9] = -s;
    m[10] = c;
    m
}

/// `glRotatef(deg, 0, 1, 0)`
pub fn rot_y(deg: f32) -> Mat4 {
    let (s, c) = deg.to_radians().sin_cos();
    let mut m = IDENTITY;
    m[0] = c;
    m[2] = -s;
    m[8] = s;
    m[10] = c;
    m
}

/// `glRotatef(deg, 0, 0, 1)`
pub fn rot_z(deg: f32) -> Mat4 {
    let (s, c) = deg.to_radians().sin_cos();
    let mut m = IDENTITY;
    m[0] = c;
    m[1] = s;
    m[4] = -s;
    m[5] = c;
    m
}

pub fn translate(x: f32, y: f32, z: f32) -> Mat4 {
    let mut m = IDENTITY;
    m[12] = x;
    m[13] = y;
    m[14] = z;
    m
}

pub fn transform(m: &Mat4, v: [f32; 4]) -> [f32; 4] {
    let mut o = [0f32; 4];
    for r in 0..4 {
        o[r] = m[r] * v[0] + m[4 + r] * v[1] + m[8 + r] * v[2] + m[12 + r] * v[3];
    }
    o
}
