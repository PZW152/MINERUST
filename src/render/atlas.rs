//! Block texture atlas built at runtime from the game jar (`TextureMap` equivalent).

use crate::block;
use std::collections::HashMap;
use std::io::Read;

pub const ATLAS_SIZE: u32 = 256;
pub const MIP_LEVELS: u32 = 5;

pub struct Atlas {
    /// RGBA8 pixels per mip level.
    pub mips: Vec<Vec<u8>>,
    /// `[u0, v0, u1, v1]` per tile.
    pub rects: Vec<[f32; 4]>,
    /// Tile for `(id, meta, face)`: index `(id * 16 + meta) * 6 + face`; `u16::MAX` if none.
    pub face_tile: Vec<u16>,
    pub grass_overlay: u16,
    pub grass_side_snowed: u16,
    pub sunflower_front: u16,
    pub sunflower_back: u16,
    pub water_flow: u16,
    pub lava_flow: u16,
    pub sun: u16,
    pub moon: [u16; 8],
    /// `colormap/grass.png` as 256x256 RGB.
    pub grass_map: Vec<u32>,
    /// `colormap/foliage.png`
    pub foliage_map: Vec<u32>,
}

pub struct Image {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

pub type Jar = zip::ZipArchive<std::fs::File>;

pub fn read_png(jar: &mut Jar, path: &str) -> Result<Image, String> {
    let mut data = Vec::new();
    jar.by_name(path).map_err(|e| format!("{path}: {e}"))?.read_to_end(&mut data).map_err(|e| format!("{path}: {e}"))?;
    let mut dec = png::Decoder::new(std::io::Cursor::new(data));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = dec.read_info().map_err(|e| format!("{path}: {e}"))?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| format!("{path}: {e}"))?;
    let n = (info.width * info.height) as usize;
    let mut rgba = Vec::with_capacity(n * 4);
    match info.color_type {
        png::ColorType::Rgba => rgba.extend_from_slice(&buf[..n * 4]),
        png::ColorType::Rgb => {
            for p in buf[..n * 3].chunks(3) {
                rgba.extend_from_slice(&[p[0], p[1], p[2], 255]);
            }
        }
        png::ColorType::Grayscale => {
            for &g in &buf[..n] {
                rgba.extend_from_slice(&[g, g, g, 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for p in buf[..n * 2].chunks(2) {
                rgba.extend_from_slice(&[p[0], p[0], p[0], p[1]]);
            }
        }
        png::ColorType::Indexed => return Err(format!("{path}: unexpected indexed output")),
    }
    Ok(Image { w: info.width, h: info.height, rgba })
}

fn crop(img: &Image, x: u32, y: u32, s: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((s * s * 4) as usize);
    for r in 0..s {
        let o = (((y + r) * img.w + x) * 4) as usize;
        out.extend_from_slice(&img.rgba[o..o + (s * 4) as usize]);
    }
    out
}

/// One mip reduction step, following `TextureUtil.generateMipmapData`.
fn downsample(src: &[u8], s: u32, transparent: bool) -> Vec<u8> {
    let d = s / 2;
    let mut out = vec![0u8; (d * d * 4) as usize];
    for y in 0..d {
        for x in 0..d {
            let px = |dx: u32, dy: u32| {
                let o = (((y * 2 + dy) * s + x * 2 + dx) * 4) as usize;
                [src[o], src[o + 1], src[o + 2], src[o + 3]]
            };
            let p = [px(0, 0), px(1, 0), px(0, 1), px(1, 1)];
            let o = ((y * d + x) * 4) as usize;
            let g = |v: u8| (v as f32 / 255.0).powf(2.2);
            let ug = |v: f32| (v.powf(1.0 / 2.2) * 255.0) as u8;
            if !transparent {
                for c in 0..4 {
                    let s: f32 = p.iter().map(|q| g(q[c])).sum();
                    out[o + c] = ug(s / 4.0);
                }
            } else {
                let mut acc = [0f32; 4];
                let mut cnt = 0f32;
                for q in &p {
                    if q[3] != 0 {
                        for c in 0..4 {
                            acc[c] += g(q[c]);
                        }
                        cnt += 1.0;
                    }
                }
                let mut a = ug(acc[3] / 4.0);
                if a < 96 {
                    a = 0;
                }
                if cnt > 0.0 {
                    for c in 0..3 {
                        out[o + c] = ug(acc[c] / cnt);
                    }
                }
                out[o + 3] = a;
            }
        }
    }
    out
}

struct Builder {
    mips: Vec<Vec<u8>>,
    rects: Vec<[f32; 4]>,
    used: [bool; 256],
}

impl Builder {
    /// Places a square tile of `cells` x `cells` 16px cells.
    fn add(&mut self, mut px: Vec<u8>, size: u32) -> u16 {
        let cells = (size / 16) as usize;
        let mut pos = None;
        'search: for cy in (0..16).step_by(cells) {
            for cx in (0..16).step_by(cells) {
                let free = (0..cells).all(|a| (0..cells).all(|b| !self.used[(cy + a) * 16 + cx + b]));
                if free {
                    pos = Some((cx, cy));
                    break 'search;
                }
            }
        }
        let (cx, cy) = pos.expect("texture atlas full");
        for a in 0..cells {
            for b in 0..cells {
                self.used[(cy + a) * 16 + cx + b] = true;
            }
        }
        let transparent = px.chunks(4).any(|p| p[3] == 0);
        let mut s = size;
        for level in 0..MIP_LEVELS {
            let aw = ATLAS_SIZE >> level;
            let ox = (cx as u32 * 16) >> level;
            let oy = (cy as u32 * 16) >> level;
            for r in 0..s {
                let d = (((oy + r) * aw + ox) * 4) as usize;
                let sidx = (r * s * 4) as usize;
                self.mips[level as usize][d..d + (s * 4) as usize].copy_from_slice(&px[sidx..sidx + (s * 4) as usize]);
            }
            if s > 1 && level + 1 < MIP_LEVELS {
                px = downsample(&px, s, transparent);
                s /= 2;
            }
        }
        let a = ATLAS_SIZE as f32;
        let (x0, y0) = (cx as f32 * 16.0, cy as f32 * 16.0);
        self.rects.push([x0 / a, y0 / a, (x0 + size as f32) / a, (y0 + size as f32) / a]);
        (self.rects.len() - 1) as u16
    }
}

fn color_map(jar: &mut Jar, path: &str) -> Result<Vec<u32>, String> {
    let img = read_png(jar, path)?;
    Ok(img.rgba.chunks(4).map(|p| (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32).collect())
}

impl Atlas {
    pub fn load(jar: &mut Jar) -> Result<Atlas, String> {
        let mut b = Builder {
            mips: (0..MIP_LEVELS).map(|l| vec![0u8; ((ATLAS_SIZE >> l) * (ATLAS_SIZE >> l) * 4) as usize]).collect(),
            rects: Vec::new(),
            used: [false; 256],
        };
        let block_tex = |jar: &mut Jar, name: &str, size: u32| -> Result<Vec<u8>, String> {
            let img = read_png(jar, &format!("assets/minecraft/textures/blocks/{name}.png"))?;
            if img.w < size || img.h < size {
                return Err(format!("{name}: unexpected size {}x{}", img.w, img.h));
            }
            // Animated strips: frame 0.
            Ok(crop(&img, 0, 0, size))
        };
        // 32px tiles first so they align to 2x2 cells.
        let water_flow = {
            let p = block_tex(jar, "water_flow", 32)?;
            b.add(p, 32)
        };
        let lava_flow = {
            let p = block_tex(jar, "lava_flow", 32)?;
            b.add(p, 32)
        };
        let sun = {
            let img = read_png(jar, "assets/minecraft/textures/environment/sun.png")?;
            b.add(crop(&img, 0, 0, 32), 32)
        };
        let mut moon = [0u16; 8];
        {
            let img = read_png(jar, "assets/minecraft/textures/environment/moon_phases.png")?;
            for (i, m) in moon.iter_mut().enumerate() {
                *m = b.add(crop(&img, (i as u32 % 4) * 32, (i as u32 / 4) * 32, 32), 32);
            }
        }
        let mut names: HashMap<&'static str, u16> = HashMap::new();
        let mut face_tile = vec![u16::MAX; 256 * 16 * 6];
        for id in 0..=255u8 {
            for meta in 0..16u8 {
                for face in 0..6usize {
                    if let Some(name) = block::texture_name(id, meta, face) {
                        let t = match names.get(name) {
                            Some(&t) => t,
                            None => {
                                let p = block_tex(jar, name, 16)?;
                                let t = b.add(p, 16);
                                names.insert(name, t);
                                t
                            }
                        };
                        face_tile[(id as usize * 16 + meta as usize) * 6 + face] = t;
                    }
                }
            }
        }
        let grass_overlay = {
            let p = block_tex(jar, "grass_side_overlay", 16)?;
            b.add(p, 16)
        };
        let mut extra = |jar: &mut Jar, name: &str| -> Result<u16, String> {
            let p = block_tex(jar, name, 16)?;
            Ok(b.add(p, 16))
        };
        let grass_side_snowed = extra(jar, "grass_side_snowed")?;
        let sunflower_front = extra(jar, "double_plant_sunflower_front")?;
        let sunflower_back = extra(jar, "double_plant_sunflower_back")?;
        Ok(Atlas {
            mips: b.mips,
            rects: b.rects,
            face_tile,
            grass_overlay,
            grass_side_snowed,
            sunflower_front,
            sunflower_back,
            water_flow,
            lava_flow,
            sun,
            moon,
            grass_map: color_map(jar, "assets/minecraft/textures/colormap/grass.png")?,
            foliage_map: color_map(jar, "assets/minecraft/textures/colormap/foliage.png")?,
        })
    }

    #[inline]
    pub fn tile(&self, id: u8, meta: u8, face: usize) -> Option<[f32; 4]> {
        let t = self.face_tile[(id as usize * 16 + (meta & 15) as usize) * 6 + face];
        if t == u16::MAX {
            None
        } else {
            Some(self.rects[t as usize])
        }
    }
}
