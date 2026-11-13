//! `GuiMainMenu.renderSkybox`: `drawPanorama` into a 256x256 viewport (64
//! jittered passes of the six `panorama_N.png` faces), the
//! `rotateAndBlurSkybox` passes, as an 8-bit framebuffer holds them. The
//! result is drawn stretched over the screen with linear filtering.

use crate::render::atlas::{gui_pos, UI_SIZE};

/// Where the blurred background is kept in the GUI sheet.
pub const BG_X: u32 = 768;
pub const BG_Y: u32 = 1792;

pub struct Panorama {
    /// The six faces, 256x256 RGB each.
    faces: Vec<Vec<[u8; 3]>>,
}

fn rx(t: f64, v: [f64; 3]) -> [f64; 3] {
    let (s, c) = t.to_radians().sin_cos();
    [v[0], v[1] * c - v[2] * s, v[1] * s + v[2] * c]
}
fn ry(t: f64, v: [f64; 3]) -> [f64; 3] {
    let (s, c) = t.to_radians().sin_cos();
    [v[0] * c + v[2] * s, v[1], -v[0] * s + v[2] * c]
}
fn rz(t: f64, v: [f64; 3]) -> [f64; 3] {
    let (s, c) = t.to_radians().sin_cos();
    [v[0] * c - v[1] * s, v[0] * s + v[1] * c, v[2]]
}

impl Panorama {
    pub fn new(sheet: &[u8]) -> Panorama {
        let mut faces = Vec::new();
        for k in 0..6 {
            let (x0, y0) = gui_pos(&format!("gui/title/background/panorama_{k}.png"));
            let mut f = vec![[0u8; 3]; 256 * 256];
            for y in 0..256 {
                for x in 0..256 {
                    let i = (((y0 as u32 + y) * UI_SIZE + x0 as u32 + x) * 4) as usize;
                    f[(y * 256 + x) as usize] = [sheet[i], sheet[i + 1], sheet[i + 2]];
                }
            }
            faces.push(f);
        }
        Panorama { faces }
    }

    /// The face and texel a ray from `o` along `d` (cube space) hits.
    fn sample(&self, o: [f64; 3], d: [f64; 3]) -> [u8; 3] {
        // the nearest face plane the ray leaves through
        let mut best = f64::INFINITY;
        let mut hit = [0.0; 3];
        for a in 0..3 {
            if d[a] == 0.0 {
                continue;
            }
            let plane = if d[a] > 0.0 { 1.0 } else { -1.0 };
            let t = (plane - o[a]) / d[a];
            if t > 0.0 && t < best {
                best = t;
                hit = [o[0] + d[0] * t, o[1] + d[1] * t, o[2] + d[2] * t];
                hit[a] = plane;
            }
        }
        let q = hit;
        // face k: Rface_k * (x, y, 1)
        let (k, x, y) = if q[2] >= 1.0 {
            (0, q[0], q[1])
        } else if q[0] >= 1.0 {
            (1, -q[2], q[1])
        } else if q[2] <= -1.0 {
            (2, -q[0], q[1])
        } else if q[0] <= -1.0 {
            (3, q[2], q[1])
        } else if q[1] <= -1.0 {
            (4, q[0], q[2])
        } else {
            (5, q[0], -q[2])
        };
        let u = (x + 1.0) / 2.0;
        let v = (y + 1.0) / 2.0;
        let ix = ((u * 256.0).floor() as i32).clamp(0, 255) as usize;
        let iy = ((v * 256.0).floor() as i32).clamp(0, 255) as usize;
        self.faces[k][iy * 256 + ix]
    }

    /// `drawPanorama` and seven `rotateAndBlurSkybox`; `t` is
    /// `panoramaTimer + partialTicks`. Returns the 256x256 RGBA texture, row
    /// 0 being the framebuffer's bottom row.
    ///
    /// The final quad binds no texture of its own: it samples the
    /// background texture as the seventh pass's `glCopyTexSubImage2D` left
    /// it, the image after six passes (the seventh only draws into the
    /// discarded default framebuffer).
    pub fn render(&self, t: f32) -> Vec<u8> {
        self.render_passes(t, 6)
    }

    pub fn render_passes(&self, t: f32, passes: usize) -> Vec<u8> {
        let a = ((t / 400.0).sin() * 25.0 + 20.0) as f64;
        let b = (-t * 0.1) as f64;
        // gluPerspective(120, 1, ...)
        let f = 1.0 / (60.0f64).to_radians().tan();
        let mut fb = vec![[0u8; 3]; 256 * 256];
        let threads = std::thread::available_parallelism()
            .map_or(4, |n| n.get())
            .min(16);
        let rows = 256usize.div_ceil(threads);
        std::thread::scope(|sc| {
            for (n, chunk) in fb.chunks_mut(rows * 256).enumerate() {
                sc.spawn(move || {
                    for (r, px) in chunk.iter_mut().enumerate() {
                        let x = r % 256;
                        let y = n * rows + r / 256;
                        let nx = (x as f64 + 0.5) / 128.0 - 1.0;
                        let ny = (y as f64 + 0.5) / 128.0 - 1.0;
                        let dv = [nx / f, ny / f, -1.0];
                        // view = Rx180 * Rz90 * (j + Rx(a) * Ry(b) * p)
                        let w = rz(-90.0, rx(-180.0, dv));
                        let d = ry(-b, rx(-a, w));
                        let mut acc = [0f32; 3];
                        for i in 0..64 {
                            let jx = ((i % 8) as f32 / 8.0 - 0.5) / 64.0;
                            let jy = ((i / 8) as f32 / 8.0 - 0.5) / 64.0;
                            let o = ry(-b, rx(-a, [-jx as f64, -jy as f64, 0.0]));
                            let c = self.sample(o, d);
                            // setColorRGBA_I(0xFFFFFF, 255 / (i + 1)), SRC_ALPHA blending
                            let al = (255 / (i + 1)) as f32 / 255.0;
                            for k in 0..3 {
                                let v = c[k] as f32 / 255.0 * al + acc[k] * (1.0 - al);
                                acc[k] = (v * 255.0).round() / 255.0;
                            }
                        }
                        *px = [
                            (acc[0] * 255.0).round() as u8,
                            (acc[1] * 255.0).round() as u8,
                            (acc[2] * 255.0).round() as u8,
                        ];
                    }
                });
            }
        });
        for _ in 0..passes {
            // glCopyTexSubImage2D, then three quads at u offsets -1, 0, +1
            // texels with alpha 1, 1/2, 1/3: the texture lands transposed
            let src = fb.clone();
            for py in 0..256usize {
                for px in 0..256usize {
                    let mut out = [0f32; 3];
                    for k in 0..3 {
                        let sx = (py as i32 + k - 1).rem_euclid(256) as usize;
                        let c = src[px * 256 + sx];
                        let al = 1.0 / (k as f32 + 1.0);
                        for ch in 0..3 {
                            let v = c[ch] as f32 / 255.0 * al + out[ch] * (1.0 - al);
                            out[ch] = (v * 255.0).round() / 255.0;
                        }
                    }
                    fb[py * 256 + px] = [
                        (out[0] * 255.0).round() as u8,
                        (out[1] * 255.0).round() as u8,
                        (out[2] * 255.0).round() as u8,
                    ];
                }
            }
        }
        let mut rgba = vec![255u8; 256 * 256 * 4];
        for (i, p) in fb.iter().enumerate() {
            rgba[i * 4..i * 4 + 3].copy_from_slice(p);
        }
        rgba
    }
}

/// The final quad of `renderSkybox`: the background texture's uv at the
/// screen corners (top-left, bottom-left, bottom-right, top-right) in sheet
/// coordinates.
pub fn screen_uv(w: f32, h: f32) -> [[f32; 2]; 4] {
    let k = if w > h { 120.0 / w } else { 120.0 / h };
    let a = h * k / 256.0;
    let b = w * k / 256.0;
    let s = UI_SIZE as f32;
    let at = |u: f32, v: f32| [(BG_X as f32 + u * 256.0) / s, (BG_Y as f32 + v * 256.0) / s];
    [
        at(0.5 + a, 0.5 + b),
        at(0.5 - a, 0.5 + b),
        at(0.5 - a, 0.5 - b),
        at(0.5 + a, 0.5 - b),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes the raw and blurred frames as PPM (`MINERUST_PANO_OUT`).
    #[test]
    fn dump_frames() {
        let (Ok(jar), Ok(out)) = (
            std::env::var("MC1710_JAR"),
            std::env::var("MINERUST_PANO_OUT"),
        ) else {
            return;
        };
        let f = std::fs::File::open(jar).unwrap();
        let mut j = zip::ZipArchive::new(f).unwrap();
        let atlas = crate::render::atlas::Atlas::load(&mut j).unwrap();
        let p = Panorama::new(&atlas.ui);
        for (name, n) in [("raw", 0), ("blur", 7)] {
            let px = p.render_passes(0.0, n);
            let mut v = b"P6 256 256 255
"
            .to_vec();
            // row 0 is the bottom: flip for the image
            for y in (0..256).rev() {
                for x in 0..256 {
                    let i = (y * 256 + x) * 4;
                    v.extend_from_slice(&px[i..i + 3]);
                }
            }
            std::fs::write(format!("{out}/{name}.ppm"), v).unwrap();
        }
    }
}
