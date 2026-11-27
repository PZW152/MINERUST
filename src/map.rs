//! Maps: `MapData` (the 128 x 128 colour bytes, centre, scale and the
//! players' markers), `ItemEmptyMap`, `ItemMap.updateMapData` and
//! `MapColor`, with `World.getUniqueDataId("map")` and the `data/map_N.dat`
//! and `data/idcounts.dat` files.

use crate::block;
use crate::util::math;
use crate::world::anvil::{compound, Nbt};
use crate::world::server::ServerWorld;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// `Block.getMapColor(meta).colorIndex` for every id and metadata, and
/// `MapColor.colorValue` by index (from `tools/MapColorHarness.java`).
struct Table {
    index: Vec<u8>,
    colours: Vec<i32>,
}

fn table() -> &'static Table {
    static T: std::sync::OnceLock<Table> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut index = Vec::with_capacity(4096);
        let mut colours = Vec::new();
        for (i, l) in include_str!("../tools/mapcolor_1710.txt").lines().enumerate() {
            let v = l.split_whitespace().map(|t| t.parse::<i32>().unwrap_or(0));
            if i < 256 {
                index.extend(v.map(|c| c as u8));
            } else {
                colours.extend(v);
            }
        }
        Table { index, colours }
    })
}

/// `Block.getMapColor(meta).colorIndex`
pub fn block_colour(id: u8, meta: u8) -> u8 {
    table().index[id as usize * 16 + (meta & 15) as usize]
}

pub const WATER: u8 = 12;

/// `MapColor.getMapColor(shade)` as RGBA, and the faint checker
/// `MapItemRenderer` gives unexplored pixels (index 0).
pub fn pixel(i: usize, c: u8) -> [u8; 4] {
    let idx = (c / 4) as usize;
    if idx == 0 {
        let a = ((i + i / 128) & 1) * 8 + 16;
        return [0, 0, 0, a as u8];
    }
    let v = table().colours.get(idx).copied().unwrap_or(0).max(0);
    let k = match c & 3 {
        3 => 135,
        2 => 255,
        0 => 180,
        _ => 220,
    };
    [
        ((v >> 16 & 255) * k / 255) as u8,
        ((v >> 8 & 255) * k / 255) as u8,
        ((v & 255) * k / 255) as u8,
        255,
    ]
}

/// The map's texture (`MapItemRenderer.Instance.updateMapTexture`).
pub fn texture(colours: &[u8]) -> Vec<u8> {
    let mut px = Vec::with_capacity(128 * 128 * 4);
    for (i, &c) in colours.iter().enumerate().take(16384) {
        px.extend_from_slice(&pixel(i, c));
    }
    px
}

/// `MapData.MapCoord`: icon (0 player, 1 frame, 6 player off the map),
/// position in half pixels from the centre, and rotation in sixteenths.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mark {
    pub icon: u8,
    pub x: i8,
    pub z: i8,
    pub rot: u8,
}

#[derive(Clone, Debug)]
pub struct MapData {
    pub scale: u8,
    pub x_center: i32,
    pub z_center: i32,
    pub dim: i8,
    pub colours: Vec<u8>,
    pub marks: Vec<Mark>,
    /// `MapInfo.step` of the (only) player.
    pub step: i32,
    /// Changed since the client last got it, and since the last save.
    pub sent: bool,
    pub saved: bool,
}

impl MapData {
    pub fn new(scale: u8, x: f64, z: f64, dim: i32) -> MapData {
        let w = 128 * (1i64 << scale);
        // Math.round(posX / w) * w (ItemEmptyMap: a double, rounded half up)
        let c = |p: f64| ((p / w as f64 + 0.5).floor() as i64 * w) as i32;
        MapData {
            scale,
            x_center: c(x),
            z_center: c(z),
            dim: dim as i8,
            colours: vec![0; 16384],
            marks: Vec::new(),
            step: 0,
            sent: false,
            saved: false,
        }
    }

    fn to_nbt(&self) -> Nbt {
        compound(vec![(
            "data",
            compound(vec![
                ("scale", Nbt::Byte(self.scale as i8)),
                ("dimension", Nbt::Byte(self.dim)),
                ("xCenter", Nbt::Int(self.x_center)),
                ("zCenter", Nbt::Int(self.z_center)),
                ("width", Nbt::Short(128)),
                ("height", Nbt::Short(128)),
                ("colors", Nbt::ByteArray(self.colours.clone())),
            ]),
        )])
    }

    /// `MapData.readFromNBT` (maps of other sizes are centred in 128 x 128).
    fn from_nbt(root: &Nbt) -> Option<MapData> {
        let d = root.get("data")?;
        let mut m = MapData::new(0, 0.0, 0.0, 0);
        m.dim = d.int("dimension").unwrap_or(0) as i8;
        m.x_center = d.int("xCenter").unwrap_or(0) as i32;
        m.z_center = d.int("zCenter").unwrap_or(0) as i32;
        m.scale = (d.int("scale").unwrap_or(0) as i32).clamp(0, 4) as u8;
        let w = d.int("width").unwrap_or(128) as i32;
        let h = d.int("height").unwrap_or(128) as i32;
        let c = d.bytes("colors")?;
        if w == 128 && h == 128 {
            m.colours = c.to_vec();
            m.colours.resize(16384, 0);
        } else {
            let (ox, oz) = ((128 - w) / 2, (128 - h) / 2);
            for z in 0..h {
                let zz = z + oz;
                if !(0..128).contains(&zz) {
                    continue;
                }
                for x in 0..w {
                    let xx = x + ox;
                    if (0..128).contains(&xx) {
                        if let Some(&b) = c.get((x + z * w) as usize) {
                            m.colours[(xx + zz * 128) as usize] = b;
                        }
                    }
                }
            }
        }
        m.saved = true;
        Some(m)
    }

    /// `MapData.updateDecorations` for the player.
    pub fn update_player_mark(&mut self, x: f64, z: f64, yaw: f32, time: i64) {
        let s = (1i32 << self.scale) as f32;
        let fx = (x - self.x_center as f64) as f32 / s;
        let fz = (z - self.z_center as f64) as f32 / s;
        let mut mx = (fx * 2.0 + 0.5) as i32 as i8;
        let mut mz = (fz * 2.0 + 0.5) as i32 as i8;
        let r = 63.0f32;
        let (icon, rot);
        if fx >= -r && fz >= -r && fx <= r && fz <= r {
            let mut y = yaw as f64;
            y += if y < 0.0 { -8.0 } else { 8.0 };
            let mut rr = (y * 16.0 / 360.0) as i32 as i8 as u8;
            if self.dim < 0 {
                let t = (time / 10) as i32;
                rr = (t.wrapping_mul(t).wrapping_mul(34187121).wrapping_add(t.wrapping_mul(121)) >> 15 & 15)
                    as u8;
            }
            icon = 0;
            rot = rr;
        } else {
            if fx.abs() >= 320.0 || fz.abs() >= 320.0 {
                self.marks.clear();
                return;
            }
            icon = 6;
            rot = 0;
            if fx <= -r {
                mx = (r * 2.0 + 2.5) as i32 as i8;
            }
            if fz <= -r {
                mz = (r * 2.0 + 2.5) as i32 as i8;
            }
            if fx >= r {
                mx = (r * 2.0 + 1.0) as i32 as i8;
            }
            if fz >= r {
                mz = (r * 2.0 + 1.0) as i32 as i8;
            }
        }
        let m = Mark {
            icon,
            x: mx,
            z: mz,
            rot: rot & 15,
        };
        if self.marks != [m] {
            self.marks = vec![m];
            self.sent = false;
        }
    }

    /// `ItemMap.updateMapData`: one sixteenth of the columns around the
    /// player each tick.
    pub fn update(&mut self, w: &ServerWorld, px: f64, pz: f64) {
        if w.dim as i8 != self.dim {
            return;
        }
        let no_sky = w.dim == -1;
        let s = 1i32 << self.scale;
        let (xc, zc) = (self.x_center, self.z_center);
        let x0 = math::floor_double(px - xc as f64) / s + 64;
        let z0 = math::floor_double(pz - zc as f64) / s + 64;
        let mut r = 128 / s;
        if no_sky {
            r /= 2;
        }
        self.step += 1;
        for x in (x0 - r + 1)..(x0 + r) {
            if (x & 15) != (self.step & 15) {
                continue;
            }
            let mut prev_h = 0.0f64;
            for z in (z0 - r - 1)..(z0 + r) {
                if !(x >= 0 && z >= -1 && x < 128 && z < 128) {
                    continue;
                }
                let (dx, dz) = (x - x0, z - z0);
                let edge = dx * dx + dz * dz > (r - 2) * (r - 2);
                let bx = (xc / s + x - 64) * s;
                let bz = (zc / s + z - 64) * s;
                if !w.chunk_exists(bx >> 4, bz >> 4) {
                    continue;
                }
                // HashMultiset: the most frequent colour (first seen wins a tie)
                let mut counts: Vec<(u8, i32)> = Vec::new();
                let mut add = |c: u8, n: i32| match counts.iter_mut().find(|e| e.0 == c) {
                    Some(e) => e.1 += n,
                    None => counts.push((c, n)),
                };
                let mut depth = 0i32;
                let mut h = 0.0f64;
                if no_sky {
                    let mut k = bx.wrapping_add(bz.wrapping_mul(231871));
                    k = k.wrapping_mul(k).wrapping_mul(31287121).wrapping_add(k.wrapping_mul(11));
                    if (k >> 20 & 1) == 0 {
                        add(block_colour(block::DIRT, 0), 10);
                    } else {
                        add(block_colour(block::STONE, 0), 100);
                    }
                    h = 100.0;
                } else {
                    for a in 0..s {
                        for b in 0..s {
                            let (cx, cz) = (bx + a, bz + b);
                            let mut y = w.peek_height(cx, cz) + 1;
                            let mut id = block::AIR;
                            let mut meta = 0u8;
                            if y > 1 {
                                loop {
                                    y -= 1;
                                    id = w.peek_block(cx, y, cz);
                                    meta = w.peek_meta(cx, y, cz);
                                    if !(block_colour(id, meta) == 0 && y > 0) {
                                        break;
                                    }
                                }
                                if y > 0 && block::material(id).is_liquid() {
                                    let mut yy = y - 1;
                                    loop {
                                        let below = w.peek_block(cx, yy, cz);
                                        yy -= 1;
                                        depth += 1;
                                        if !(yy > 0 && block::material(below).is_liquid()) {
                                            break;
                                        }
                                    }
                                }
                            }
                            h += y as f64 / (s * s) as f64;
                            add(block_colour(id, meta), 1);
                        }
                    }
                }
                depth /= s * s;
                let parity = ((x + z) & 1) as f64;
                let mut d = (h - prev_h) * 4.0 / (s + 4) as f64 + (parity - 0.5) * 0.4;
                let mut shade = 1u8;
                if d > 0.6 {
                    shade = 2;
                }
                if d < -0.6 {
                    shade = 0;
                }
                let mut best = (0u8, 0i32);
                for &(c, n) in &counts {
                    if n > best.1 {
                        best = (c, n);
                    }
                }
                let colour = best.0;
                if colour == WATER {
                    d = depth as f64 * 0.1 + parity * 0.2;
                    shade = 1;
                    if d < 0.5 {
                        shade = 2;
                    }
                    if d > 0.9 {
                        shade = 0;
                    }
                }
                prev_h = h;
                if z >= 0 && dx * dx + dz * dz < r * r && (!edge || (x + z) & 1 != 0) {
                    let i = (x + z * 128) as usize;
                    let v = colour * 4 + shade;
                    if self.colours[i] != v {
                        self.colours[i] = v;
                        self.sent = false;
                        self.saved = false;
                    }
                }
            }
        }
    }
}

/// Every map of the world, loaded from `data/` as they are needed.
#[derive(Default)]
pub struct Maps {
    pub list: HashMap<i32, MapData>,
    /// `idcounts.dat`'s "map": the last id handed out (-1 none).
    last_id: Option<i32>,
    dir: Option<PathBuf>,
    counts_dirty: bool,
}

fn read_gz(path: &Path) -> Option<Nbt> {
    use std::io::Read;
    let data = std::fs::read(path).ok()?;
    let mut raw = Vec::new();
    flate2::read::GzDecoder::new(&data[..]).read_to_end(&mut raw).ok()?;
    crate::world::anvil::read_nbt(&raw).ok()
}

fn write_gz(path: &Path, root: &Nbt) -> std::io::Result<()> {
    use std::io::Write;
    let mut z = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(&crate::world::anvil::write_nbt(root))?;
    crate::world::save::write_atomic(path, &z.finish()?)
}

impl Maps {
    pub fn new(world_dir: Option<PathBuf>) -> Maps {
        Maps {
            dir: world_dir.map(|d| d.join("data")),
            ..Default::default()
        }
    }

    /// `World.getUniqueDataId("map")`
    pub fn unique_id(&mut self) -> i32 {
        let last = match self.last_id {
            Some(v) => v,
            None => self
                .dir
                .as_ref()
                .and_then(|d| read_gz(&d.join("idcounts.dat")))
                .and_then(|n| n.int("map").map(|v| v as i32))
                .unwrap_or(-1),
        };
        let id = last + 1;
        self.last_id = Some(id);
        self.counts_dirty = true;
        id
    }

    /// `World.loadItemData(MapData.class, "map_" + id)`
    pub fn get(&mut self, id: i32) -> Option<&mut MapData> {
        if !self.list.contains_key(&id) {
            let m = self
                .dir
                .as_ref()
                .and_then(|d| read_gz(&d.join(format!("map_{id}.dat"))))
                .and_then(|n| MapData::from_nbt(&n))?;
            self.list.insert(id, m);
        }
        self.list.get_mut(&id)
    }

    /// `ItemMap.getMapData` on the server: a map that does not exist yet
    /// is made at scale 3 around the spawn, under a new id.
    pub fn get_or_make(&mut self, id: i32, spawn: [i32; 2], dim: i32) -> (i32, &mut MapData) {
        if self.get(id).is_some() {
            return (id, self.list.get_mut(&id).unwrap());
        }
        let nid = self.unique_id();
        let mut m = MapData::new(3, 0.0, 0.0, dim);
        let w = 128 * 8;
        // Math.round((float) spawnX / w) * w, but Math.round(spawnZ / w)
        // divides the ints first
        m.x_center = ((spawn[0] as f32 / w as f32 + 0.5).floor() as i32) * w;
        m.z_center = spawn[1] / w * w;
        self.list.insert(nid, m);
        (nid, self.list.get_mut(&nid).unwrap())
    }

    pub fn save(&mut self) -> std::io::Result<()> {
        let Some(d) = self.dir.clone() else {
            return Ok(());
        };
        if self.counts_dirty || self.list.values().any(|m| !m.saved) {
            std::fs::create_dir_all(&d)?;
        }
        for (id, m) in self.list.iter_mut() {
            if !m.saved {
                write_gz(&d.join(format!("map_{id}.dat")), &m.to_nbt())?;
                m.saved = true;
            }
        }
        if self.counts_dirty {
            if let Some(v) = self.last_id {
                write_gz(
                    &d.join("idcounts.dat"),
                    &compound(vec![("map", Nbt::Short(v as i16))]),
                )?;
            }
            self.counts_dirty = false;
        }
        Ok(())
    }
}

/// What the client keeps of a map (`S34PacketMaps`).
#[derive(Clone, Debug)]
pub struct MapView {
    pub id: i32,
    pub colours: Vec<u8>,
    pub marks: Vec<Mark>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_match_the_game() {
        assert_eq!(block_colour(block::GRASS, 0), 1);
        assert_eq!(block_colour(block::STONE, 0), 11);
        assert_eq!(block_colour(block::WATER, 0), WATER);
        // wool by colour
        assert_eq!(block_colour(35, 14), 28);
        // grass in full light (shade 2)
        assert_eq!(pixel(0, 1 * 4 + 2), [127, 178, 56, 255]);
        let m = MapData::new(0, 70.0, -70.0, 0);
        assert_eq!((m.x_center, m.z_center), (128, -128));
    }
}
