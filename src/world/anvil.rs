//! The vanilla Anvil format: NBT (`CompressedStreamTools`), region files
//! (`RegionFile`, `.mca`), chunk sections (`AnvilChunkLoader`) and
//! `level.dat`, so worlds can be opened from and written back for 1.7.10.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// An NBT tag.
#[derive(Clone, Debug, PartialEq)]
pub enum Nbt {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<u8>),
    String(String),
    List(u8, Vec<Nbt>),
    Compound(Vec<(String, Nbt)>),
    IntArray(Vec<i32>),
}

impl Nbt {
    pub fn id(&self) -> u8 {
        match self {
            Nbt::Byte(_) => 1,
            Nbt::Short(_) => 2,
            Nbt::Int(_) => 3,
            Nbt::Long(_) => 4,
            Nbt::Float(_) => 5,
            Nbt::Double(_) => 6,
            Nbt::ByteArray(_) => 7,
            Nbt::String(_) => 8,
            Nbt::List(..) => 9,
            Nbt::Compound(_) => 10,
            Nbt::IntArray(_) => 11,
        }
    }
    pub fn get(&self, key: &str) -> Option<&Nbt> {
        match self {
            Nbt::Compound(v) => v.iter().find(|(k, _)| k == key).map(|(_, t)| t),
            _ => None,
        }
    }
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Nbt> {
        match self {
            Nbt::Compound(v) => v.iter_mut().find(|(k, _)| k == key).map(|(_, t)| t),
            _ => None,
        }
    }
    /// Sets (or adds) a key of a compound.
    pub fn set(&mut self, key: &str, val: Nbt) {
        if let Nbt::Compound(v) = self {
            if let Some(e) = v.iter_mut().find(|(k, _)| k == key) {
                e.1 = val;
            } else {
                v.push((key.to_string(), val));
            }
        }
    }
    pub fn int(&self, key: &str) -> Option<i64> {
        match self.get(key)? {
            Nbt::Byte(v) => Some(*v as i64),
            Nbt::Short(v) => Some(*v as i64),
            Nbt::Int(v) => Some(*v as i64),
            Nbt::Long(v) => Some(*v),
            _ => None,
        }
    }
    pub fn float(&self, key: &str) -> Option<f64> {
        match self.get(key)? {
            Nbt::Float(v) => Some(*v as f64),
            Nbt::Double(v) => Some(*v),
            _ => self.int(key).map(|v| v as f64),
        }
    }
    pub fn bytes(&self, key: &str) -> Option<&[u8]> {
        match self.get(key)? {
            Nbt::ByteArray(v) => Some(v),
            _ => None,
        }
    }
    pub fn list(&self, key: &str) -> &[Nbt] {
        match self.get(key) {
            Some(Nbt::List(_, v)) => v,
            _ => &[],
        }
    }
    pub fn string(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Nbt::String(s) => Some(s),
            _ => None,
        }
    }
}

pub fn compound(v: Vec<(&str, Nbt)>) -> Nbt {
    Nbt::Compound(v.into_iter().map(|(k, t)| (k.to_string(), t)).collect())
}

pub fn list(items: Vec<Nbt>) -> Nbt {
    let id = items.first().map_or(10, |t| t.id());
    Nbt::List(id, items)
}

// ---------------------------------------------------------------- reading

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> io::Result<&[u8]> {
        if self.p + n > self.b.len() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "truncated NBT",
            ));
        }
        let s = &self.b[self.p..self.p + n];
        self.p += n;
        Ok(s)
    }
    fn u8(&mut self) -> io::Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn i16(&mut self) -> io::Result<i16> {
        Ok(i16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> io::Result<i32> {
        Ok(i32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i64(&mut self) -> io::Result<i64> {
        Ok(i64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn string(&mut self) -> io::Result<String> {
        let n = self.i16()? as u16 as usize;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
    fn len(&mut self) -> io::Result<usize> {
        let n = self.i32()?;
        if n < 0 || n as usize > self.b.len() * 8 + 1024 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "bad NBT length"));
        }
        Ok(n as usize)
    }
    fn payload(&mut self, id: u8, depth: u32) -> io::Result<Nbt> {
        if depth > 512 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "NBT too deep"));
        }
        Ok(match id {
            1 => Nbt::Byte(self.u8()? as i8),
            2 => Nbt::Short(self.i16()?),
            3 => Nbt::Int(self.i32()?),
            4 => Nbt::Long(self.i64()?),
            5 => Nbt::Float(f32::from_bits(self.i32()? as u32)),
            6 => Nbt::Double(f64::from_bits(self.i64()? as u64)),
            7 => {
                let n = self.len()?;
                Nbt::ByteArray(self.take(n)?.to_vec())
            }
            8 => Nbt::String(self.string()?),
            9 => {
                let t = self.u8()?;
                let n = self.len()?;
                let mut v = Vec::with_capacity(n.min(65536));
                for _ in 0..n {
                    v.push(self.payload(t, depth + 1)?);
                }
                Nbt::List(t, v)
            }
            10 => {
                let mut v = Vec::new();
                loop {
                    let t = self.u8()?;
                    if t == 0 {
                        break;
                    }
                    let k = self.string()?;
                    v.push((k, self.payload(t, depth + 1)?));
                }
                Nbt::Compound(v)
            }
            11 => {
                let n = self.len()?;
                let mut v = Vec::with_capacity(n.min(65536));
                for _ in 0..n {
                    v.push(self.i32()?);
                }
                Nbt::IntArray(v)
            }
            _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "bad NBT tag")),
        })
    }
}

/// Reads a named root compound (`CompressedStreamTools.read`).
pub fn read_nbt(b: &[u8]) -> io::Result<Nbt> {
    let mut r = Reader { b, p: 0 };
    let t = r.u8()?;
    if t != 10 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "root is not a compound",
        ));
    }
    let _name = r.string()?;
    r.payload(10, 0)
}

// ---------------------------------------------------------------- writing

fn put_string(o: &mut Vec<u8>, s: &str) {
    o.extend_from_slice(&(s.len() as u16).to_be_bytes());
    o.extend_from_slice(s.as_bytes());
}

fn put_payload(o: &mut Vec<u8>, t: &Nbt) {
    match t {
        Nbt::Byte(v) => o.push(*v as u8),
        Nbt::Short(v) => o.extend_from_slice(&v.to_be_bytes()),
        Nbt::Int(v) => o.extend_from_slice(&v.to_be_bytes()),
        Nbt::Long(v) => o.extend_from_slice(&v.to_be_bytes()),
        Nbt::Float(v) => o.extend_from_slice(&v.to_bits().to_be_bytes()),
        Nbt::Double(v) => o.extend_from_slice(&v.to_bits().to_be_bytes()),
        Nbt::ByteArray(v) => {
            o.extend_from_slice(&(v.len() as i32).to_be_bytes());
            o.extend_from_slice(v);
        }
        Nbt::String(s) => put_string(o, s),
        Nbt::List(id, v) => {
            o.push(if v.is_empty() { *id } else { v[0].id() });
            o.extend_from_slice(&(v.len() as i32).to_be_bytes());
            for e in v {
                put_payload(o, e);
            }
        }
        Nbt::Compound(v) => {
            for (k, e) in v {
                o.push(e.id());
                put_string(o, k);
                put_payload(o, e);
            }
            o.push(0);
        }
        Nbt::IntArray(v) => {
            o.extend_from_slice(&(v.len() as i32).to_be_bytes());
            for e in v {
                o.extend_from_slice(&e.to_be_bytes());
            }
        }
    }
}

/// Writes a named root compound.
pub fn write_nbt(root: &Nbt) -> Vec<u8> {
    let mut o = Vec::new();
    o.push(10);
    put_string(&mut o, "");
    put_payload(&mut o, root);
    o
}

// ---------------------------------------------------------------- region files

pub fn region_path(dir: &Path, cx: i32, cz: i32) -> PathBuf {
    dir.join("region")
        .join(format!("r.{}.{}.mca", cx >> 5, cz >> 5))
}

/// `RegionFile.getChunkDataInputStream`: the chunk's NBT, if present.
pub fn read_region_chunk(dir: &Path, cx: i32, cz: i32) -> Option<Nbt> {
    let mut f = fs::File::open(region_path(dir, cx, cz)).ok()?;
    let i = ((cx & 31) + (cz & 31) * 32) as u64;
    let mut loc = [0u8; 4];
    f.seek(SeekFrom::Start(i * 4)).ok()?;
    f.read_exact(&mut loc).ok()?;
    let offset = u32::from_be_bytes([0, loc[0], loc[1], loc[2]]) as u64;
    let count = loc[3] as u64;
    if offset < 2 || count == 0 {
        return None;
    }
    f.seek(SeekFrom::Start(offset * 4096)).ok()?;
    let mut head = [0u8; 5];
    f.read_exact(&mut head).ok()?;
    let len = u32::from_be_bytes(head[0..4].try_into().ok()?) as usize;
    if len == 0 || len as u64 > count * 4096 {
        return None;
    }
    let mut data = vec![0u8; len - 1];
    f.read_exact(&mut data).ok()?;
    let mut raw = Vec::new();
    match head[4] {
        1 => {
            flate2::read::GzDecoder::new(&data[..])
                .read_to_end(&mut raw)
                .ok()?;
        }
        2 => {
            flate2::read::ZlibDecoder::new(&data[..])
                .read_to_end(&mut raw)
                .ok()?;
        }
        _ => return None,
    }
    read_nbt(&raw).ok()
}

/// `RegionFile.write`: stores a chunk (zlib), reusing or appending sectors.
pub fn write_region_chunk(dir: &Path, cx: i32, cz: i32, root: &Nbt, time: u32) -> io::Result<()> {
    let path = region_path(dir, cx, cz);
    if let Some(p) = path.parent() {
        fs::create_dir_all(p)?;
    }
    let mut f = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)?;
    let mut header = vec![0u8; 8192];
    let size = f.metadata()?.len();
    if size >= 8192 {
        f.seek(SeekFrom::Start(0))?;
        f.read_exact(&mut header)?;
    } else {
        f.seek(SeekFrom::Start(0))?;
        f.write_all(&header)?;
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(&write_nbt(root))?;
    let data = z.finish()?;
    let need = (data.len() + 5).div_ceil(4096) as u32;
    if need >= 256 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "chunk too large for a region",
        ));
    }
    let i = ((cx & 31) + (cz & 31) * 32) as usize;
    // sectors in use by the other chunks
    let total = (f.metadata()?.len().div_ceil(4096)).max(2) as u32;
    let mut used = vec![false; total as usize + need as usize + 2];
    used[0] = true;
    used[1] = true;
    for k in 0..1024 {
        if k == i {
            continue;
        }
        let o = u32::from_be_bytes([0, header[k * 4], header[k * 4 + 1], header[k * 4 + 2]]);
        let c = header[k * 4 + 3] as u32;
        for s in o..o + c {
            if (s as usize) < used.len() {
                used[s as usize] = true;
            }
        }
    }
    let old = u32::from_be_bytes([0, header[i * 4], header[i * 4 + 1], header[i * 4 + 2]]);
    let old_n = header[i * 4 + 3] as u32;
    let free = |at: u32, used: &[bool]| {
        (at..at + need).all(|s| !used.get(s as usize).copied().unwrap_or(false))
    };
    let at = if old >= 2 && need <= old_n && free(old, &used) {
        old
    } else {
        let mut s = 2;
        while !free(s, &used) {
            s += 1;
        }
        s
    };
    let mut sector = Vec::with_capacity(need as usize * 4096);
    sector.extend_from_slice(&((data.len() + 1) as u32).to_be_bytes());
    sector.push(2);
    sector.extend_from_slice(&data);
    sector.resize(need as usize * 4096, 0);
    f.seek(SeekFrom::Start(at as u64 * 4096))?;
    f.write_all(&sector)?;
    header[i * 4] = (at >> 16) as u8;
    header[i * 4 + 1] = (at >> 8) as u8;
    header[i * 4 + 2] = at as u8;
    header[i * 4 + 3] = need as u8;
    header[4096 + i * 4..4096 + i * 4 + 4].copy_from_slice(&time.to_be_bytes());
    f.seek(SeekFrom::Start(0))?;
    f.write_all(&header)?;
    f.sync_all()
}

pub fn region_has(dir: &Path, cx: i32, cz: i32) -> bool {
    let Ok(mut f) = fs::File::open(region_path(dir, cx, cz)) else {
        return false;
    };
    let i = ((cx & 31) + (cz & 31) * 32) as u64;
    let mut loc = [0u8; 4];
    f.seek(SeekFrom::Start(i * 4)).is_ok() && f.read_exact(&mut loc).is_ok() && loc != [0; 4]
}

// ---------------------------------------------------------------- chunks

/// A chunk in this project's layout, converted to or from Anvil.
pub struct AChunk {
    pub cx: i32,
    pub cz: i32,
    /// `(x * 16 + z) * 256 + y`
    pub blocks: Box<[u8; 65536]>,
    pub meta: Box<[u8; 65536]>,
    pub sky: Box<[u8; 65536]>,
    pub blk: Box<[u8; 65536]>,
    pub biomes: [u8; 256],
    /// `[z << 4 | x]`
    pub height: [i32; 256],
    pub sections: u16,
    pub populated: bool,
    pub light_populated: bool,
    pub entities: Vec<Nbt>,
    pub tiles: Vec<Nbt>,
    pub ticks: Vec<Nbt>,
}

fn boxed() -> Box<[u8; 65536]> {
    vec![0u8; 65536].into_boxed_slice().try_into().unwrap()
}

fn nibble(a: &[u8], i: usize) -> u8 {
    let b = a.get(i >> 1).copied().unwrap_or(0);
    if i & 1 == 0 {
        b & 15
    } else {
        b >> 4
    }
}

/// `AnvilChunkLoader.readChunkFromNBT`
pub fn chunk_from_nbt(root: &Nbt) -> Option<AChunk> {
    let level = root.get("Level")?;
    let cx = level.int("xPos")? as i32;
    let cz = level.int("zPos")? as i32;
    let mut c = AChunk {
        cx,
        cz,
        blocks: boxed(),
        meta: boxed(),
        sky: boxed(),
        blk: boxed(),
        biomes: [0; 256],
        height: [0; 256],
        sections: 0,
        populated: level.int("TerrainPopulated").unwrap_or(0) != 0,
        light_populated: level.int("LightPopulated").unwrap_or(0) != 0,
        entities: level.list("Entities").to_vec(),
        tiles: level.list("TileEntities").to_vec(),
        ticks: level.list("TileTicks").to_vec(),
    };
    for s in level.list("Sections") {
        let y0 = s.int("Y")? as i32;
        if !(0..16).contains(&y0) {
            continue;
        }
        let blocks = s.bytes("Blocks")?;
        let data = s.bytes("Data").unwrap_or(&[]);
        let sky = s.bytes("SkyLight").unwrap_or(&[]);
        let bl = s.bytes("BlockLight").unwrap_or(&[]);
        c.sections |= 1 << y0;
        for i in 0..4096usize {
            let (x, z, y) = (i & 15, (i >> 4) & 15, i >> 8);
            let j = (x * 16 + z) * 256 + (y0 as usize * 16 + y);
            c.blocks[j] = blocks.get(i).copied().unwrap_or(0);
            c.meta[j] = nibble(data, i);
            c.sky[j] = nibble(sky, i);
            c.blk[j] = nibble(bl, i);
        }
    }
    if let Some(b) = level.bytes("Biomes") {
        for (i, v) in b.iter().take(256).enumerate() {
            c.biomes[i] = *v;
        }
    }
    if let Some(Nbt::IntArray(h)) = level.get("HeightMap") {
        for (i, v) in h.iter().take(256).enumerate() {
            c.height[i] = *v;
        }
    }
    Some(c)
}

/// `AnvilChunkLoader.writeChunkToNBT`
pub fn chunk_to_nbt(c: &AChunk, time: i64) -> Nbt {
    let mut sections = Vec::new();
    for y0 in 0..16usize {
        if c.sections & (1 << y0) == 0 {
            continue;
        }
        let mut blocks = vec![0u8; 4096];
        let mut data = vec![0u8; 2048];
        let mut sky = vec![0u8; 2048];
        let mut bl = vec![0u8; 2048];
        for i in 0..4096usize {
            let (x, z, y) = (i & 15, (i >> 4) & 15, i >> 8);
            let j = (x * 16 + z) * 256 + (y0 * 16 + y);
            blocks[i] = c.blocks[j];
            let sh = if i & 1 == 0 { 0 } else { 4 };
            data[i >> 1] |= (c.meta[j] & 15) << sh;
            sky[i >> 1] |= (c.sky[j] & 15) << sh;
            bl[i >> 1] |= (c.blk[j] & 15) << sh;
        }
        sections.push(compound(vec![
            ("Y", Nbt::Byte(y0 as i8)),
            ("Blocks", Nbt::ByteArray(blocks)),
            ("Data", Nbt::ByteArray(data)),
            ("BlockLight", Nbt::ByteArray(bl)),
            ("SkyLight", Nbt::ByteArray(sky)),
        ]));
    }
    let level = compound(vec![
        ("xPos", Nbt::Int(c.cx)),
        ("zPos", Nbt::Int(c.cz)),
        ("LastUpdate", Nbt::Long(time)),
        ("LightPopulated", Nbt::Byte(c.light_populated as i8)),
        ("TerrainPopulated", Nbt::Byte(c.populated as i8)),
        ("V", Nbt::Byte(1)),
        ("InhabitedTime", Nbt::Long(0)),
        ("Sections", Nbt::List(10, sections)),
        ("Biomes", Nbt::ByteArray(c.biomes.to_vec())),
        ("HeightMap", Nbt::IntArray(c.height.to_vec())),
        ("Entities", Nbt::List(10, c.entities.clone())),
        ("TileEntities", Nbt::List(10, c.tiles.clone())),
        ("TileTicks", Nbt::List(10, c.ticks.clone())),
    ]);
    compound(vec![("Level", level)])
}

// ---------------------------------------------------------------- level.dat

pub fn read_level_dat(dir: &Path) -> Option<Nbt> {
    let data = fs::read(dir.join("level.dat")).ok()?;
    let mut raw = Vec::new();
    flate2::read::GzDecoder::new(&data[..])
        .read_to_end(&mut raw)
        .ok()?;
    read_nbt(&raw).ok()
}

pub fn write_level_dat(dir: &Path, root: &Nbt) -> io::Result<()> {
    let mut z = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(&write_nbt(root))?;
    let data = z.finish()?;
    // keep the previous file as level.dat_old, like SaveHandler
    let path = dir.join("level.dat");
    if path.exists() {
        let _ = fs::copy(&path, dir.join("level.dat_old"));
    }
    super::save::write_atomic(&path, &data)
}

/// An item stack in NBT (`ItemStack.writeToNBT`).
pub fn item_nbt(slot: Option<i8>, s: &crate::item::Stack) -> Nbt {
    let mut v = vec![
        ("id", Nbt::Short(s.id as i16)),
        ("Count", Nbt::Byte(s.count as i8)),
        ("Damage", Nbt::Short(s.meta as i16)),
    ];
    if let Some(sl) = slot {
        v.insert(0, ("Slot", Nbt::Byte(sl)));
    }
    compound(v)
}

/// `ItemStack.loadItemStackFromNBT`
pub fn item_from_nbt(t: &Nbt) -> Option<crate::item::Stack> {
    let id = t.int("id")? as u16;
    let count = t.int("Count")?.max(0) as u32;
    if count == 0 || id == 0 {
        return None;
    }
    Some(crate::item::Stack::new(
        id,
        t.int("Damage").unwrap_or(0) as i32,
        count,
    ))
}

/// Ordered map helper for slot lists.
pub fn slots(items: &[Nbt]) -> BTreeMap<i32, crate::item::Stack> {
    let mut m = BTreeMap::new();
    for t in items {
        if let (Some(sl), Some(s)) = (t.int("Slot"), item_from_nbt(t)) {
            m.insert(sl as i32 & 255, s);
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nbt_round_trip_and_region() {
        let t = compound(vec![
            ("a", Nbt::Int(7)),
            ("b", Nbt::String("hi".into())),
            ("c", list(vec![Nbt::Short(1), Nbt::Short(2)])),
            ("d", Nbt::IntArray(vec![1, -2])),
        ]);
        assert_eq!(read_nbt(&write_nbt(&t)).unwrap(), t);
        let dir = std::env::temp_dir().join(format!("minerust-anvil-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        write_region_chunk(&dir, -1, 33, &t, 5).unwrap();
        write_region_chunk(&dir, 0, 0, &compound(vec![("x", Nbt::Long(9))]), 5).unwrap();
        assert_eq!(read_region_chunk(&dir, -1, 33).unwrap(), t);
        assert!(region_has(&dir, 0, 0));
        assert!(!region_has(&dir, 1, 0));
        let _ = fs::remove_dir_all(&dir);
    }
}
