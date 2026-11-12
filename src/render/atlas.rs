//! Block texture atlas built at runtime from the game jar (`TextureMap` equivalent).

use crate::block;
use std::collections::HashMap;
use std::io::Read;

pub const ATLAS_SIZE: u32 = 1024;
/// Atlas width in 16px cells.
const GRID: usize = (ATLAS_SIZE / 16) as usize;
pub const MIP_LEVELS: u32 = 5;
pub const UI_SIZE: u32 = 1024;

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
    /// GUI sheet, 512x512 RGBA: `gui/widgets.png` at (0, 0), `gui/icons.png`
    /// at (256, 0), `font/ascii.png` at (0, 256).
    pub ui: Vec<u8>,
    /// `FontRenderer.charWidth` for the ASCII sheet.
    pub char_width: [u8; 256],
    /// Animated tiles (`.png.mcmeta`).
    pub anims: Vec<Anim>,
    /// `destroy_stage_0` .. `destroy_stage_9`.
    pub destroy: [u16; 10],
    /// Item sprites by `(id, meta)`.
    pub items: HashMap<(u16, i32), u16>,
    /// Entity textures (path under `textures/entity/`): tile and cell size.
    pub mobs: HashMap<&'static str, (u16, u32)>,
    /// `entity/steve.png` (64x32) in the top half of a 64x64 tile.
    pub skin: u16,
    /// `environment/clouds.png` as a 256x256 opacity mask.
    pub clouds: Vec<bool>,
}

/// One animated sprite (`TextureAtlasSprite` with animation metadata).
pub struct Anim {
    /// Top-left pixel of the tile in the atlas.
    pub x: u32,
    pub y: u32,
    pub size: u32,
    /// Ticks each frame is shown.
    pub frametime: u32,
    /// Frame indices in playback order.
    pub order: Vec<usize>,
    /// Mip chain of every frame.
    pub frames: Vec<Vec<Vec<u8>>>,
}

/// Mip chain for one tile, as it is stored in the atlas.
fn mip_chain(mut px: Vec<u8>, size: u32) -> Vec<Vec<u8>> {
    let transparent = px.chunks(4).any(|p| p[3] == 0);
    let mut out = Vec::new();
    let mut s = size;
    for level in 0..MIP_LEVELS {
        out.push(px.clone());
        if s > 1 && level + 1 < MIP_LEVELS {
            px = downsample(&px, s, transparent);
            s /= 2;
        }
    }
    out
}

/// Reads `frametime` and `frames` from an `.mcmeta` animation section.
fn parse_mcmeta(text: &str) -> (u32, Option<Vec<usize>>) {
    let number_after = |key: &str| -> Option<u32> {
        let i = text.find(key)? + key.len();
        let rest =
            text[i..].trim_start_matches(|c: char| c == ':' || c == '"' || c.is_whitespace());
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        digits.parse().ok()
    };
    let frametime = number_after("\"frametime\"").unwrap_or(1).max(1);
    let frames = text.find("\"frames\"").and_then(|i| {
        let open = text[i..].find('[')? + i + 1;
        let close = text[open..].find(']')? + open;
        let list: Vec<usize> = text[open..close]
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        if list.is_empty() {
            None
        } else {
            Some(list)
        }
    });
    (frametime, frames)
}

pub struct Image {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

pub type Jar = zip::ZipArchive<std::fs::File>;

pub fn read_png(jar: &mut Jar, path: &str) -> Result<Image, String> {
    let mut data = Vec::new();
    jar.by_name(path)
        .map_err(|e| format!("{path}: {e}"))?
        .read_to_end(&mut data)
        .map_err(|e| format!("{path}: {e}"))?;
    let mut dec = png::Decoder::new(std::io::Cursor::new(data));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = dec.read_info().map_err(|e| format!("{path}: {e}"))?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("{path}: {e}"))?;
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
    Ok(Image {
        w: info.width,
        h: info.height,
        rgba,
    })
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
    used: Vec<bool>,
    /// Top-left pixel of each tile.
    origins: Vec<(u32, u32)>,
}

impl Builder {
    /// Places a square tile of `cells` x `cells` 16px cells.
    fn add(&mut self, mut px: Vec<u8>, size: u32) -> u16 {
        let cells = (size / 16) as usize;
        let mut pos = None;
        'search: for cy in (0..GRID).step_by(cells) {
            for cx in (0..GRID).step_by(cells) {
                let free =
                    (0..cells).all(|a| (0..cells).all(|b| !self.used[(cy + a) * GRID + cx + b]));
                if free {
                    pos = Some((cx, cy));
                    break 'search;
                }
            }
        }
        let (cx, cy) = pos.expect("texture atlas full");
        for a in 0..cells {
            for b in 0..cells {
                self.used[(cy + a) * GRID + cx + b] = true;
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
                self.mips[level as usize][d..d + (s * 4) as usize]
                    .copy_from_slice(&px[sidx..sidx + (s * 4) as usize]);
            }
            if s > 1 && level + 1 < MIP_LEVELS {
                px = downsample(&px, s, transparent);
                s /= 2;
            }
        }
        let a = ATLAS_SIZE as f32;
        let (x0, y0) = (cx as f32 * 16.0, cy as f32 * 16.0);
        self.rects.push([
            x0 / a,
            y0 / a,
            (x0 + size as f32) / a,
            (y0 + size as f32) / a,
        ]);
        self.origins.push((cx as u32 * 16, cy as u32 * 16));
        (self.rects.len() - 1) as u16
    }
}

fn color_map(jar: &mut Jar, path: &str) -> Result<Vec<u32>, String> {
    let img = read_png(jar, path)?;
    Ok(img
        .rgba
        .chunks(4)
        .map(|p| (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32)
        .collect())
}

impl Atlas {
    pub fn load(jar: &mut Jar) -> Result<Atlas, String> {
        let mut b = Builder {
            mips: (0..MIP_LEVELS)
                .map(|l| vec![0u8; ((ATLAS_SIZE >> l) * (ATLAS_SIZE >> l) * 4) as usize])
                .collect(),
            rects: Vec::new(),
            used: vec![false; GRID * GRID],
            origins: Vec::new(),
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
        let mut destroy = [0u16; 10];
        for (i, d) in destroy.iter_mut().enumerate() {
            let p = block_tex(jar, &format!("destroy_stage_{i}"), 16)?;
            *d = b.add(p, 16);
        }
        let mut items = HashMap::new();
        for (id, meta) in crate::item::all_items() {
            if let Some(name) = crate::item::item_texture(id, meta) {
                let img = read_png(jar, &format!("assets/minecraft/textures/items/{name}.png"))?;
                items.insert((id, meta), b.add(crop(&img, 0, 0, 16), 16));
            }
        }
        let skin = {
            let img = read_png(jar, "assets/minecraft/textures/entity/steve.png")?;
            let mut px = vec![0u8; 64 * 64 * 4];
            for y in 0..img.h.min(64) {
                let s = (y * img.w * 4) as usize;
                let n = (img.w.min(64) * 4) as usize;
                px[(y * 64 * 4) as usize..(y * 64 * 4) as usize + n]
                    .copy_from_slice(&img.rgba[s..s + n]);
            }
            b.add(px, 64)
        };
        let mut mobs = HashMap::new();
        for name in crate::render::model::TEXTURES
            .into_iter()
            .chain(crate::render::model2::TEXTURES)
            .chain(crate::render::model2::TEXTURES2)
        {
            let path = match name.strip_prefix("../") {
                Some(rest) => format!("assets/minecraft/textures/{rest}.png"),
                None => format!("assets/minecraft/textures/entity/{name}.png"),
            };
            let img = read_png(jar, &path)?;
            let cell = img.w.max(img.h).next_power_of_two();
            let mut px = vec![0u8; (cell * cell * 4) as usize];
            for y in 0..img.h {
                let s = (y * img.w * 4) as usize;
                let n = (img.w * 4) as usize;
                let d = (y * cell * 4) as usize;
                px[d..d + n].copy_from_slice(&img.rgba[s..s + n]);
            }
            mobs.insert(name, (b.add(px, cell), cell));
        }
        // Animated sprites: every frame of the strip, in metadata order.
        let mut anims = Vec::new();
        for (name, tile, size) in [
            ("water_still", names["water_still"], 16u32),
            ("lava_still", names["lava_still"], 16),
            ("water_flow", water_flow, 32),
            ("lava_flow", lava_flow, 32),
        ] {
            let img = read_png(jar, &format!("assets/minecraft/textures/blocks/{name}.png"))?;
            let count = (img.h / size).max(1) as usize;
            let mut meta = String::new();
            if let Ok(mut f) = jar.by_name(&format!(
                "assets/minecraft/textures/blocks/{name}.png.mcmeta"
            )) {
                let _ = f.read_to_string(&mut meta);
            }
            let (frametime, order) = parse_mcmeta(&meta);
            let order: Vec<usize> = order
                .unwrap_or_else(|| (0..count).collect())
                .into_iter()
                .filter(|&i| i < count)
                .collect();
            let frames = (0..count)
                .map(|i| mip_chain(crop(&img, 0, i as u32 * size, size), size))
                .collect();
            let (x, y) = b.origins[tile as usize];
            anims.push(Anim {
                x,
                y,
                size,
                frametime,
                order,
                frames,
            });
        }
        let clouds = {
            let img = read_png(jar, "assets/minecraft/textures/environment/clouds.png")?;
            if img.w != 256 || img.h != 256 {
                return Err(format!("clouds.png: unexpected size {}x{}", img.w, img.h));
            }
            img.rgba.chunks(4).map(|p| p[3] > 25).collect()
        };
        // GUI sheet and font metrics (FontRenderer.readFontTexture).
        let mut ui = vec![0u8; UI_SIZE as usize * UI_SIZE as usize * 4];
        let mut blit = |img: &Image, ox: u32, oy: u32| {
            for y in 0..img.h.min(UI_SIZE - oy) {
                let s = (y * img.w * 4) as usize;
                let d = (((oy + y) * UI_SIZE + ox) * 4) as usize;
                let n = (img.w.min(UI_SIZE - ox) * 4) as usize;
                ui[d..d + n].copy_from_slice(&img.rgba[s..s + n]);
            }
        };
        blit(
            &read_png(jar, "assets/minecraft/textures/gui/widgets.png")?,
            0,
            0,
        );
        blit(
            &read_png(jar, "assets/minecraft/textures/gui/icons.png")?,
            256,
            0,
        );
        let font = read_png(jar, "assets/minecraft/textures/font/ascii.png")?;
        blit(&font, 0, 256);
        for (path, x, y) in [
            ("gui/container/inventory.png", 256, 256),
            ("gui/container/crafting_table.png", 512, 0),
            ("gui/container/furnace.png", 512, 256),
            ("gui/container/generic_54.png", 768, 0),
            ("gui/options_background.png", 768, 256),
            ("entity/steve.png", 768, 512),
        ] {
            blit(
                &read_png(jar, &format!("assets/minecraft/textures/{path}"))?,
                x,
                y,
            );
        }
        let mut char_width = [0u8; 256];
        let cell = font.w / 16;
        for (k, w) in char_width.iter_mut().enumerate() {
            let (cx, cy) = (k as u32 % 16, k as u32 / 16);
            let mut used = cell as i32 - 1;
            while used >= 0 {
                let x = cx * cell + used as u32;
                let empty = (0..cell)
                    .all(|y| font.rgba[(((cy * cell + y) * font.w + x) * 4 + 3) as usize] == 0);
                if !empty {
                    break;
                }
                used -= 1;
            }
            used += 1;
            if k == 32 {
                used = 3;
            }
            *w = ((0.5 + used as f64 * (8.0 / cell as f64)) as i32 + 1) as u8;
        }
        Ok(Atlas {
            skin,
            destroy,
            items,
            mobs,
            ui,
            char_width,
            anims,
            clouds,
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

    pub fn mob_tex(&self, name: &str) -> Option<crate::render::model::Tex> {
        let &(t, cell) = self.mobs.get(name)?;
        Some(crate::render::model::Tex {
            rect: self.rects[t as usize],
            size: cell as f32,
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
