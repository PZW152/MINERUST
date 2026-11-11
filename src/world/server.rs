//! Server-side world: `World` / `WorldServer` / `Chunk` / `ChunkProviderServer`
//! semantics needed by terrain population and block updates. Lighting follows
//! the vanilla incremental engine exactly, including its dependence on which
//! neighbouring chunks are loaded.

use crate::block::{self, Material};
use crate::java::JavaRandom;
use crate::world::biome::{self, Biome};
use crate::world::{ChunkData, ChunkProvider};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::Arc;

#[derive(Default)]
pub struct FastHasher(u64);

impl Hasher for FastHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0.rotate_left(5) ^ b as u64).wrapping_mul(0x517cc1b727220a95);
        }
    }
    fn write_i32(&mut self, i: i32) {
        self.0 = (self.0.rotate_left(5) ^ i as u32 as u64).wrapping_mul(0x517cc1b727220a95);
    }
    fn write_u8(&mut self, i: u8) {
        self.0 = (self.0.rotate_left(5) ^ i as u64).wrapping_mul(0x517cc1b727220a95);
    }
}

pub type FastMap<K, V> = HashMap<K, V, BuildHasherDefault<FastHasher>>;
pub type FastSet<K> = HashSet<K, BuildHasherDefault<FastHasher>>;

/// `Facing.offsets{X,Y,Z}ForSide`
pub const FACING: [[i32; 3]; 6] = [[0, -1, 0], [0, 1, 0], [0, 0, -1], [0, 0, 1], [-1, 0, 0], [1, 0, 0]];

pub struct Chunk {
    pub data: Arc<ChunkData>,
    /// Which 16-high `ExtendedBlockStorage` sections exist.
    pub sections: u16,
    /// `heightMap[z << 4 | x]`
    pub height: [i32; 256],
    /// `precipitationHeightMap[x | z << 4]`
    precip: [i32; 256],
    pub height_min: i32,
    update_sky_cols: [bool; 256],
    gap_updated: bool,
    pub terrain_populated: bool,
    pub light_populated: bool,
    pub ticked: bool,
    queued_light_checks: i32,
}

#[inline]
fn bidx(x: i32, y: i32, z: i32) -> usize {
    (((x as usize) << 4 | z as usize) << 8) | y as usize
}

impl Chunk {
    /// `new Chunk(world, blocks, meta, x, z)`
    fn new(data: ChunkData) -> Chunk {
        let mut sections = 0u16;
        for (i, &b) in data.blocks.iter().enumerate() {
            if b != 0 {
                sections |= 1 << ((i & 255) >> 4);
            }
        }
        Chunk {
            data: Arc::new(data),
            sections,
            height: [0; 256],
            precip: [-999; 256],
            height_min: 0,
            update_sky_cols: [false; 256],
            gap_updated: false,
            terrain_populated: false,
            light_populated: false,
            ticked: false,
            queued_light_checks: 0,
        }
    }

    #[inline]
    fn has_section(&self, y: i32) -> bool {
        self.sections & (1 << (y >> 4)) != 0
    }

    #[inline]
    pub fn block(&self, x: i32, y: i32, z: i32) -> u8 {
        if (y >> 4) < 16 && self.has_section(y) {
            self.data.blocks[bidx(x, y, z)]
        } else {
            block::AIR
        }
    }

    #[inline]
    pub fn meta(&self, x: i32, y: i32, z: i32) -> u8 {
        if (y >> 4) < 16 && self.has_section(y) {
            self.data.meta[bidx(x, y, z)]
        } else {
            0
        }
    }

    #[inline]
    fn opacity(&self, x: i32, y: i32, z: i32) -> i32 {
        block::info(self.block(x, y, z)).light_opacity as i32
    }

    pub fn top_filled_segment(&self) -> i32 {
        if self.sections == 0 {
            0
        } else {
            (15 - self.sections.leading_zeros() as i32) << 4
        }
    }

    /// `Chunk.generateSkylightMap`
    fn generate_skylight_map(&mut self) {
        let top = self.top_filled_segment();
        self.height_min = i32::MAX;
        // MC-7508: vanilla starts one block too low and never sees a block on
        // the top level of the highest section.
        let first = if crate::compat::fixed() { (top + 16).min(256) } else { top + 16 - 1 };
        for x in 0..16 {
            for z in 0..16 {
                self.precip[(x + (z << 4)) as usize] = -999;
                let mut y = first;
                while y > 0 {
                    if self.opacity(x, y - 1, z) != 0 {
                        self.height[(z << 4 | x) as usize] = y;
                        if y < self.height_min {
                            self.height_min = y;
                        }
                        break;
                    }
                    y -= 1;
                }
                let mut l = 15i32;
                let mut y = top + 16 - 1;
                loop {
                    let mut op = self.opacity(x, y, z);
                    if op == 0 && l != 15 {
                        op = 1;
                    }
                    l -= op;
                    if l > 0 && self.has_section(y) {
                        Arc::make_mut(&mut self.data).sky[bidx(x, y, z)] = l as u8;
                    }
                    y -= 1;
                    if y <= 0 || l <= 0 {
                        break;
                    }
                }
            }
        }
    }

    #[inline]
    fn can_see_sky(&self, x: i32, y: i32, z: i32) -> bool {
        y >= self.height[(z << 4 | x) as usize]
    }

    /// `Chunk.getSavedLightValue`
    #[inline]
    pub fn saved_light(&self, sky: bool, x: i32, y: i32, z: i32) -> i32 {
        if !self.has_section(y) {
            if self.can_see_sky(x, y, z) && sky {
                15
            } else {
                0
            }
        } else if sky {
            self.data.sky[bidx(x, y, z)] as i32
        } else {
            self.data.blk[bidx(x, y, z)] as i32
        }
    }

    /// `Chunk.setLightValue`
    fn set_light(&mut self, sky: bool, x: i32, y: i32, z: i32, v: i32) {
        if !self.has_section(y) {
            self.sections |= 1 << (y >> 4);
            self.generate_skylight_map();
        }
        let d = Arc::make_mut(&mut self.data);
        if sky {
            d.sky[bidx(x, y, z)] = v as u8;
        } else {
            d.blk[bidx(x, y, z)] = v as u8;
        }
    }

    /// `Chunk.getBlockLightValue(x, y, z, skylightSubtracted)`
    pub fn block_light_value(&self, x: i32, y: i32, z: i32, sub: i32) -> i32 {
        if !self.has_section(y) {
            return if sub < 15 { 15 - sub } else { 0 };
        }
        let mut l = self.data.sky[bidx(x, y, z)] as i32 - sub;
        let b = self.data.blk[bidx(x, y, z)] as i32;
        if b > l {
            l = b;
        }
        l
    }

    /// `Chunk.getPrecipitationHeight`
    fn precipitation_height(&mut self, x: i32, z: i32) -> i32 {
        let i = (x | z << 4) as usize;
        let mut h = self.precip[i];
        if h == -999 {
            let mut y = self.top_filled_segment() + 15;
            h = -1;
            while y > 0 && h == -1 {
                let m = block::material(self.block(x, y, z));
                if !m.blocks_movement() && !m.is_liquid() {
                    y -= 1;
                } else {
                    h = y + 1;
                }
            }
            self.precip[i] = h;
        }
        h
    }
}

fn rle_encode(src: &[u8], out: &mut Vec<u8>) {
    let mut i = 0;
    while i < src.len() {
        let v = src[i];
        let mut n = 1usize;
        while i + n < src.len() && src[i + n] == v && n < 65535 {
            n += 1;
        }
        out.extend_from_slice(&(n as u16).to_le_bytes());
        out.push(v);
        i += n;
    }
}

/// Bounds-checked: malformed input yields `None`, never a panic.
fn rle_decode(src: &[u8], pos: &mut usize, dst: &mut [u8]) -> Option<()> {
    let mut i = 0;
    while i < dst.len() {
        let run = src.get(*pos..*pos + 3)?;
        let n = u16::from_le_bytes([run[0], run[1]]) as usize;
        if n == 0 || i + n > dst.len() {
            return None;
        }
        dst[i..i + n].fill(run[2]);
        *pos += 3;
        i += n;
    }
    Some(())
}

impl Chunk {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(16384);
        out.extend_from_slice(&self.sections.to_le_bytes());
        out.push(self.terrain_populated as u8 | (self.light_populated as u8) << 1 | (self.ticked as u8) << 2 | (self.gap_updated as u8) << 3);
        out.extend_from_slice(&self.queued_light_checks.to_le_bytes());
        out.extend_from_slice(&self.height_min.to_le_bytes());
        for h in self.height {
            out.extend_from_slice(&(h as i16).to_le_bytes());
        }
        for b in self.update_sky_cols {
            out.push(b as u8);
        }
        out.extend_from_slice(&self.data.biomes);
        rle_encode(&self.data.blocks[..], &mut out);
        rle_encode(&self.data.meta[..], &mut out);
        rle_encode(&self.data.sky[..], &mut out);
        rle_encode(&self.data.blk[..], &mut out);
        out
    }

    /// Decodes a stored chunk; `None` if the data is truncated or malformed.
    fn from_bytes(cx: i32, cz: i32, b: &[u8]) -> Option<Chunk> {
        use crate::world::gen::chunk_provider::boxed;
        if b.len() < 11 + 512 + 256 + 256 {
            return None;
        }
        let sections = u16::from_le_bytes([b[0], b[1]]);
        let flags = b[2];
        let queued = i32::from_le_bytes([b[3], b[4], b[5], b[6]]);
        let height_min = i32::from_le_bytes([b[7], b[8], b[9], b[10]]);
        let mut pos = 11;
        let mut height = [0i32; 256];
        for h in height.iter_mut() {
            *h = i16::from_le_bytes([b[pos], b[pos + 1]]) as i32;
            pos += 2;
        }
        let mut cols = [false; 256];
        for c in cols.iter_mut() {
            *c = b[pos] != 0;
            pos += 1;
        }
        let mut biomes = [0u8; 256];
        biomes.copy_from_slice(&b[pos..pos + 256]);
        pos += 256;
        let mut data = ChunkData { cx, cz, blocks: boxed(), meta: boxed(), biomes, sky: boxed(), blk: boxed() };
        rle_decode(b, &mut pos, &mut data.blocks[..])?;
        rle_decode(b, &mut pos, &mut data.meta[..])?;
        rle_decode(b, &mut pos, &mut data.sky[..])?;
        rle_decode(b, &mut pos, &mut data.blk[..])?;
        Some(Chunk {
            data: Arc::new(data),
            sections,
            height,
            precip: [-999; 256],
            height_min,
            update_sky_cols: cols,
            gap_updated: flags & 8 != 0,
            terrain_populated: flags & 1 != 0,
            light_populated: flags & 2 != 0,
            ticked: flags & 4 != 0,
            queued_light_checks: queued,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Tick {
    time: i64,
    priority: i32,
    id: u64,
    x: i32,
    y: i32,
    z: i32,
    block: u8,
}

pub struct ServerWorld {
    pub provider: Arc<ChunkProvider>,
    pub chunks: FastMap<(i32, i32), Box<Chunk>>,
    /// Chunks whose blocks or light changed since the renderer last looked.
    pub dirty: FastSet<(i32, i32)>,
    pub rand: JavaRandom,
    pub total_time: i64,
    pub scheduled_immediate: bool,
    pub fall_instantly: bool,
    /// `World.skylightSubtracted`
    pub skylight_subtracted: i32,
    light_queue: Vec<i32>,
    ticks: BTreeSet<Tick>,
    tick_keys: FastSet<(i32, i32, i32, u8)>,
    next_tick_id: u64,
    /// `ChunkProviderGenerate.rand`
    pub gen_rand: JavaRandom,
    pub(crate) update_lcg: i32,
    /// `WorldGenBigTree.heightLimit` of each biome object's generator.
    pub big_tree_limit: [i32; 256],
    /// `BlockDynamicLiquid.field_149814_b` for flowing water / flowing lava.
    spread_dirs: [[bool; 4]; 2],
    /// Unloaded chunks kept in memory when there is no save directory.
    stash: FastMap<(i32, i32), Vec<u8>>,
    /// World directory; chunks are written here when they unload.
    pub save_dir: Option<std::path::PathBuf>,
    /// Number of chunks populated so far (statistics).
    pub populated: u64,
}

impl ServerWorld {
    pub fn new(provider: Arc<ChunkProvider>) -> ServerWorld {
        ServerWorld {
            provider,
            chunks: FastMap::default(),
            dirty: FastSet::default(),
            rand: JavaRandom::new(0x1D2E3F4A5B6C),
            total_time: 0,
            scheduled_immediate: false,
            fall_instantly: false,
            skylight_subtracted: 0,
            light_queue: vec![0; 32768],
            ticks: BTreeSet::new(),
            tick_keys: FastSet::default(),
            next_tick_id: 0,
            gen_rand: JavaRandom::new(0),
            update_lcg: 0x2F6E2B1,
            big_tree_limit: [0; 256],
            spread_dirs: [[false; 4]; 2],
            stash: FastMap::default(),
            save_dir: None,
            populated: 0,
        }
    }

    pub fn seed(&self) -> i64 {
        self.provider.seed
    }

    // ---------------------------------------------------------------- chunks

    #[inline]
    pub fn chunk_exists(&self, cx: i32, cz: i32) -> bool {
        self.chunks.contains_key(&(cx, cz))
    }

    #[inline]
    fn ch(&self, cx: i32, cz: i32) -> Option<&Chunk> {
        self.chunks.get(&(cx, cz)).map(|c| &**c)
    }

    /// `ChunkProviderServer.provideChunk`: loads (generates) on demand.
    pub fn chunk(&mut self, cx: i32, cz: i32) -> &mut Chunk {
        if !self.chunks.contains_key(&(cx, cz)) {
            self.load_chunk(cx, cz);
        }
        self.chunks.get_mut(&(cx, cz)).unwrap()
    }

    /// `ChunkProviderServer.loadChunk`
    pub fn load_chunk(&mut self, cx: i32, cz: i32) {
        if self.chunks.contains_key(&(cx, cz)) {
            return;
        }
        let stored = match self.stash.remove(&(cx, cz)) {
            Some(b) => Some(b),
            None => self.save_dir.as_ref().and_then(|d| crate::world::save::read_chunk(d, cx, cz)),
        };
        if let Some(bytes) = stored {
            if let Some(c) = Chunk::from_bytes(cx, cz, &bytes) {
                self.insert_restored(Box::new(c));
                return;
            }
            eprintln!("chunk {cx},{cz}: stored data is damaged, regenerating");
        }
        let data = self.provider.provide_chunk(cx, cz);
        self.insert_generated(data);
    }

    /// Adds a freshly generated chunk (`provideChunk` result) and runs the
    /// population checks of `Chunk.populateChunk`. Returns false if present.
    pub fn insert_generated(&mut self, data: ChunkData) -> bool {
        let (cx, cz) = (data.cx, data.cz);
        if self.chunks.contains_key(&(cx, cz)) {
            return false;
        }
        let mut c = Box::new(Chunk::new(data));
        c.generate_skylight_map();
        self.chunks.insert((cx, cz), c);
        self.mark_dirty_chunk(cx, cz);
        self.populate_chunk_checks(cx, cz);
        true
    }

    /// Adds a chunk without running any population (tests, flat worlds).
    pub fn insert_raw(&mut self, data: ChunkData) {
        let key = (data.cx, data.cz);
        let mut c = Box::new(Chunk::new(data));
        c.generate_skylight_map();
        c.terrain_populated = true;
        self.chunks.insert(key, c);
    }

    /// Unloads a chunk: to its file when the world has a save directory,
    /// otherwise into the compressed in-memory store.
    pub fn unload_chunk(&mut self, cx: i32, cz: i32) {
        if let Some(c) = self.chunks.remove(&(cx, cz)) {
            let bytes = c.to_bytes();
            match &self.save_dir {
                Some(dir) => {
                    if let Err(e) = crate::world::save::write_chunk(dir, cx, cz, &bytes) {
                        eprintln!("failed to save chunk {cx},{cz}: {e}");
                        self.stash.insert((cx, cz), bytes);
                    }
                }
                None => {
                    self.stash.insert((cx, cz), bytes);
                }
            }
        }
    }

    /// Writes every loaded chunk to disk without unloading it.
    pub fn save_all(&self) -> std::io::Result<usize> {
        let Some(dir) = &self.save_dir else { return Ok(0) };
        let mut n = 0;
        let mut keys: Vec<(i32, i32)> = self.chunks.keys().copied().collect();
        keys.sort();
        for key in keys {
            crate::world::save::write_chunk(dir, key.0, key.1, &self.chunks[&key].to_bytes())?;
            n += 1;
        }
        for (key, bytes) in &self.stash {
            crate::world::save::write_chunk(dir, key.0, key.1, bytes)?;
            n += 1;
        }
        Ok(n)
    }

    /// Whether the chunk can be restored without generating it.
    pub fn is_stored(&self, cx: i32, cz: i32) -> bool {
        self.stash.contains_key(&(cx, cz)) || self.save_dir.as_ref().map_or(false, |d| crate::world::save::chunk_path(d, cx, cz).is_file())
    }

    /// Pending scheduled updates as `(x, y, z, block, delay)`.
    pub fn pending_tick_list(&self) -> Vec<(i32, i32, i32, u8, i64)> {
        self.ticks.iter().map(|t| (t.x, t.y, t.z, t.block, t.time - self.total_time)).collect()
    }

    /// Re-inserts a chunk restored from storage (already populated or not).
    pub fn insert_restored(&mut self, c: Box<Chunk>) {
        let (cx, cz) = (c.data.cx, c.data.cz);
        self.chunks.insert((cx, cz), c);
        self.mark_dirty_chunk(cx, cz);
        self.populate_chunk_checks(cx, cz);
    }

    pub fn remove_chunk(&mut self, cx: i32, cz: i32) -> Option<Box<Chunk>> {
        self.chunks.remove(&(cx, cz))
    }

    /// `Chunk.populateChunk`
    fn populate_chunk_checks(&mut self, x: i32, z: i32) {
        let pop = |w: &ServerWorld, x: i32, z: i32| w.ch(x, z).map_or(false, |c| c.terrain_populated);
        if !pop(self, x, z) && self.chunk_exists(x + 1, z + 1) && self.chunk_exists(x, z + 1) && self.chunk_exists(x + 1, z) {
            self.populate(x, z);
        }
        if self.chunk_exists(x - 1, z) && !pop(self, x - 1, z) && self.chunk_exists(x - 1, z + 1) && self.chunk_exists(x, z + 1) && self.chunk_exists(x - 1, z + 1) {
            self.populate(x - 1, z);
        }
        if self.chunk_exists(x, z - 1) && !pop(self, x, z - 1) && self.chunk_exists(x + 1, z - 1) && self.chunk_exists(x + 1, z - 1) && self.chunk_exists(x + 1, z) {
            self.populate(x, z - 1);
        }
        if self.chunk_exists(x - 1, z - 1) && !pop(self, x - 1, z - 1) && self.chunk_exists(x, z - 1) && self.chunk_exists(x - 1, z) {
            self.populate(x - 1, z - 1);
        }
    }

    /// `ChunkProviderServer.populate`
    fn populate(&mut self, cx: i32, cz: i32) {
        if !self.chunk(cx, cz).terrain_populated {
            self.check_light(cx, cz);
            super::populate::populate(self, cx, cz);
            self.populated += 1;
            // The four chunks it can write into may have just become meshable.
            for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                self.dirty.insert((cx + dx, cz + dz));
            }
        }
    }

    #[inline]
    fn mark_dirty_chunk(&mut self, cx: i32, cz: i32) {
        self.dirty.insert((cx, cz));
    }

    /// Marks the chunk containing a changed block (and edge neighbours).
    #[inline]
    fn mark_dirty(&mut self, x: i32, z: i32) {
        let (cx, cz) = (x >> 4, z >> 4);
        self.dirty.insert((cx, cz));
        let (lx, lz) = (x & 15, z & 15);
        if lx == 0 || lx == 15 || lz == 0 || lz == 15 {
            let dx = if lx == 0 { -1 } else if lx == 15 { 1 } else { 0 };
            let dz = if lz == 0 { -1 } else if lz == 15 { 1 } else { 0 };
            if dx != 0 {
                self.dirty.insert((cx + dx, cz));
            }
            if dz != 0 {
                self.dirty.insert((cx, cz + dz));
            }
            if dx != 0 && dz != 0 {
                self.dirty.insert((cx + dx, cz + dz));
            }
        }
    }

    /// `World.checkChunksExist`
    pub fn check_chunks_exist(&self, x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32) -> bool {
        if y1 >= 0 && y0 < 256 {
            for cx in (x0 >> 4)..=(x1 >> 4) {
                for cz in (z0 >> 4)..=(z1 >> 4) {
                    if !self.chunk_exists(cx, cz) {
                        return false;
                    }
                }
            }
            true
        } else {
            false
        }
    }

    #[inline]
    fn chunks_near_exist(&self, x: i32, y: i32, z: i32, r: i32) -> bool {
        self.check_chunks_exist(x - r, y - r, z - r, x + r, y + r, z + r)
    }

    // ---------------------------------------------------------------- blocks

    /// `World.getBlock` (generates missing chunks).
    #[inline]
    pub fn block(&mut self, x: i32, y: i32, z: i32) -> u8 {
        if !(0..256).contains(&y) {
            return block::AIR;
        }
        if let Some(c) = self.chunks.get(&(x >> 4, z >> 4)) {
            return c.block(x & 15, y, z & 15);
        }
        self.chunk(x >> 4, z >> 4).block(x & 15, y, z & 15)
    }

    /// Whether the chunk containing block column (x, z) is loaded.
    #[inline]
    pub fn loaded(&self, x: i32, z: i32) -> bool {
        self.chunk_exists(x >> 4, z >> 4)
    }

    /// Block without loading anything (air when unloaded).
    #[inline]
    pub fn peek_block(&self, x: i32, y: i32, z: i32) -> u8 {
        if !(0..256).contains(&y) {
            return block::AIR;
        }
        self.ch(x >> 4, z >> 4).map_or(block::AIR, |c| c.block(x & 15, y, z & 15))
    }

    #[inline]
    pub fn peek_meta(&self, x: i32, y: i32, z: i32) -> u8 {
        if !(0..256).contains(&y) {
            return 0;
        }
        self.ch(x >> 4, z >> 4).map_or(0, |c| c.meta(x & 15, y, z & 15))
    }

    #[inline]
    pub fn meta(&mut self, x: i32, y: i32, z: i32) -> i32 {
        if !(0..256).contains(&y) {
            return 0;
        }
        if let Some(c) = self.chunks.get(&(x >> 4, z >> 4)) {
            return c.meta(x & 15, y, z & 15) as i32;
        }
        self.chunk(x >> 4, z >> 4).meta(x & 15, y, z & 15) as i32
    }

    #[inline]
    pub fn material(&mut self, x: i32, y: i32, z: i32) -> Material {
        block::material(self.block(x, y, z))
    }

    #[inline]
    pub fn is_air(&mut self, x: i32, y: i32, z: i32) -> bool {
        self.material(x, y, z) == Material::Air
    }

    /// `World.getBiomeGenForCoords`
    pub fn biome(&mut self, x: i32, z: i32) -> &'static Biome {
        match self.ch(x >> 4, z >> 4) {
            Some(c) => biome::get_or_ocean(c.data.biomes[((z & 15) << 4 | (x & 15)) as usize] as i32),
            None => biome::get_or_ocean(self.provider.biome_at(x, z)),
        }
    }

    /// `World.getHeightValue`
    pub fn height(&mut self, x: i32, z: i32) -> i32 {
        match self.ch(x >> 4, z >> 4) {
            Some(c) => c.height[((z & 15) << 4 | (x & 15)) as usize],
            None => 0,
        }
    }

    fn height_min(&self, x: i32, z: i32) -> i32 {
        self.ch(x >> 4, z >> 4).map_or(0, |c| c.height_min)
    }

    /// `World.getPrecipitationHeight`
    pub fn precipitation_height(&mut self, x: i32, z: i32) -> i32 {
        self.chunk(x >> 4, z >> 4).precipitation_height(x & 15, z & 15)
    }

    /// `World.getTopSolidOrLiquidBlock`
    pub fn top_solid_or_liquid(&mut self, x: i32, z: i32) -> i32 {
        let c = self.chunk(x >> 4, z >> 4);
        let mut y = c.top_filled_segment() + 15;
        let (lx, lz) = (x & 15, z & 15);
        while y > 0 {
            let m = block::material(c.block(lx, y, lz));
            if m.blocks_movement() && m != Material::Leaves {
                return y + 1;
            }
            y -= 1;
        }
        -1
    }

    /// `World.canBlockSeeTheSky`
    pub fn can_see_sky(&mut self, x: i32, y: i32, z: i32) -> bool {
        self.chunk(x >> 4, z >> 4).can_see_sky(x & 15, y, z & 15)
    }

    /// `World.getSavedLightValue`
    #[inline]
    pub fn saved_light(&self, sky: bool, x: i32, y: i32, z: i32) -> i32 {
        let y = y.clamp(0, 255);
        match self.ch(x >> 4, z >> 4) {
            Some(c) => c.saved_light(sky, x & 15, y, z & 15),
            None => {
                if sky {
                    15
                } else {
                    0
                }
            }
        }
    }

    /// `World.getFullBlockLightValue`
    pub fn full_block_light(&mut self, x: i32, y: i32, z: i32) -> i32 {
        if y < 0 {
            return 0;
        }
        let y = y.min(255);
        self.chunk(x >> 4, z >> 4).block_light_value(x & 15, y, z & 15, 0)
    }

    /// `World.getBlockLightValue_do`
    pub fn block_light_value(&mut self, x: i32, y: i32, z: i32, neighbours: bool) -> i32 {
        if neighbours && block::info(self.block(x, y, z)).use_neighbor_brightness {
            let mut l = self.block_light_value(x, y + 1, z, false);
            for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let v = self.block_light_value(x + dx, y, z + dz, false);
                if v > l {
                    l = v;
                }
            }
            return l;
        }
        if y < 0 {
            return 0;
        }
        let y = y.min(255);
        let sub = self.skylight_subtracted;
        self.chunk(x >> 4, z >> 4).block_light_value(x & 15, y, z & 15, sub)
    }

    fn set_light(&mut self, sky: bool, x: i32, y: i32, z: i32, v: i32) {
        if (0..256).contains(&y) {
            if let Some(c) = self.chunks.get_mut(&(x >> 4, z >> 4)) {
                c.set_light(sky, x & 15, y, z & 15, v);
                self.mark_dirty(x, z);
            }
        }
    }

    /// `World.setBlock(x, y, z, block, meta, flags)`
    pub fn set_block(&mut self, x: i32, y: i32, z: i32, id: u8, meta: i32, flags: i32) -> bool {
        if !(0..256).contains(&y) {
            return false;
        }
        self.chunk(x >> 4, z >> 4);
        let old = if flags & 1 != 0 { self.peek_block(x, y, z) } else { 0 };
        let changed = self.chunk_set_block(x, y, z, id, meta);
        self.update_all_light(x, y, z);
        if changed {
            self.mark_dirty(x, z);
            if flags & 1 != 0 {
                self.notify_neighbours(x, y, z, old);
            }
        }
        changed
    }

    pub fn set_air(&mut self, x: i32, y: i32, z: i32) -> bool {
        self.set_block(x, y, z, block::AIR, 0, 3)
    }

    /// `World.setBlockMetadataWithNotify`
    pub fn set_meta(&mut self, x: i32, y: i32, z: i32, meta: i32, flags: i32) -> bool {
        if !(0..256).contains(&y) {
            return false;
        }
        let c = self.chunk(x >> 4, z >> 4);
        if !c.has_section(y) {
            return false;
        }
        let i = bidx(x & 15, y, z & 15);
        if c.data.meta[i] as i32 == meta {
            return false;
        }
        Arc::make_mut(&mut c.data).meta[i] = meta as u8;
        let id = c.data.blocks[i];
        self.mark_dirty(x, z);
        if flags & 1 != 0 {
            self.notify_neighbours(x, y, z, id);
        }
        true
    }

    /// `Chunk.setBlockIDWithMetadata` (func_150807_a)
    fn chunk_set_block(&mut self, wx: i32, y: i32, wz: i32, id: u8, meta: i32) -> bool {
        let (x, z) = (wx & 15, wz & 15);
        let key = (wx >> 4, wz >> 4);
        let col = (z << 4 | x) as usize;
        let (old, old_meta, height, mut regen);
        {
            let c = self.chunks.get_mut(&key).unwrap();
            if y >= c.precip[col] - 1 {
                c.precip[col] = -999;
            }
            height = c.height[col];
            old = c.block(x, y, z);
            old_meta = c.meta(x, y, z) as i32;
            if old == id && old_meta == meta {
                return false;
            }
            regen = false;
            if !c.has_section(y) {
                if id == block::AIR {
                    return false;
                }
                c.sections |= 1 << (y >> 4);
                regen = y >= height;
                if !regen && crate::compat::fixed() {
                    // MC-29490: a section created below the height map starts
                    // with sky light 0 everywhere, which shows up as black
                    // patches under tall trees. Give open columns their light.
                    let base = (y >> 4) << 4;
                    let heights = c.height;
                    let d = Arc::make_mut(&mut c.data);
                    for cx in 0..16 {
                        for cz in 0..16 {
                            let h = heights[(cz << 4 | cx) as usize];
                            for yy in base.max(h)..base + 16 {
                                d.sky[bidx(cx, yy, cz)] = 15;
                            }
                        }
                    }
                }
            }
            Arc::make_mut(&mut c.data).blocks[bidx(x, y, z)] = id;
        }
        self.break_block(old, old_meta, wx, y, wz);
        {
            let c = self.chunks.get_mut(&key).unwrap();
            if c.data.blocks[bidx(x, y, z)] != id {
                return false;
            }
            Arc::make_mut(&mut c.data).meta[bidx(x, y, z)] = meta as u8;
        }
        if regen {
            self.chunks.get_mut(&key).unwrap().generate_skylight_map();
            self.mark_dirty_chunk(key.0, key.1);
        } else {
            let new_op = block::info(id).light_opacity as i32;
            let old_op = block::info(old).light_opacity as i32;
            if new_op > 0 {
                if y >= height {
                    self.relight_block(key, x, y + 1, z);
                }
            } else if y == height - 1 {
                self.relight_block(key, x, y, z);
            }
            let c = self.chunks.get_mut(&key).unwrap();
            if new_op != old_op && (new_op < old_op || c.saved_light(true, x, y, z) > 0 || c.saved_light(false, x, y, z) > 0) {
                c.update_sky_cols[(x + z * 16) as usize] = true;
                c.gap_updated = true;
            }
        }
        self.on_block_added(id, wx, y, wz);
        true
    }

    /// `Chunk.relightBlock`
    fn relight_block(&mut self, key: (i32, i32), x: i32, y: i32, z: i32) {
        let col = (z << 4 | x) as usize;
        let (old_h, new_h);
        {
            let c = self.chunks.get_mut(&key).unwrap();
            old_h = c.height[col] & 255;
            let mut h = old_h;
            if y > old_h {
                h = y;
            }
            while h > 0 && c.opacity(x, h - 1, z) == 0 {
                h -= 1;
            }
            if h == old_h {
                return;
            }
            c.height[col] = h;
            new_h = h;
            if h < old_h {
                for yy in h..old_h {
                    if c.has_section(yy) {
                        Arc::make_mut(&mut c.data).sky[bidx(x, yy, z)] = 15;
                    }
                }
            } else {
                for yy in old_h..h {
                    if c.has_section(yy) {
                        Arc::make_mut(&mut c.data).sky[bidx(x, yy, z)] = 0;
                    }
                }
            }
            let mut l = 15i32;
            let mut yy = h;
            while yy > 0 && l > 0 {
                yy -= 1;
                let mut op = c.opacity(x, yy, z);
                if op == 0 {
                    op = 1;
                }
                l -= op;
                if l < 0 {
                    l = 0;
                }
                if c.has_section(yy) {
                    Arc::make_mut(&mut c.data).sky[bidx(x, yy, z)] = l as u8;
                }
            }
            if new_h < c.height_min {
                c.height_min = new_h;
            }
        }
        self.mark_dirty_chunk(key.0, key.1);
        let (wx, wz) = (key.0 * 16 + x, key.1 * 16 + z);
        let (mut lo, mut hi) = (old_h, new_h);
        if hi < lo {
            std::mem::swap(&mut lo, &mut hi);
        }
        self.update_skylight_neighbor_height(wx - 1, wz, lo, hi);
        self.update_skylight_neighbor_height(wx + 1, wz, lo, hi);
        self.update_skylight_neighbor_height(wx, wz - 1, lo, hi);
        self.update_skylight_neighbor_height(wx, wz + 1, lo, hi);
        self.update_skylight_neighbor_height(wx, wz, lo, hi);
    }

    fn update_skylight_neighbor_height(&mut self, x: i32, z: i32, lo: i32, hi: i32) {
        if hi > lo && self.chunks_near_exist(x, 0, z, 16) {
            for y in lo..hi {
                self.update_light(true, x, y, z);
            }
        }
    }

    fn check_skylight_neighbor_height(&mut self, x: i32, z: i32, h: i32) {
        let n = self.height(x, z);
        if n > h {
            self.update_skylight_neighbor_height(x, z, h, n + 1);
        } else if n < h {
            self.update_skylight_neighbor_height(x, z, n, h + 1);
        }
    }

    /// `Chunk.recheckGaps(false)`
    fn recheck_gaps(&mut self, cx: i32, cz: i32) {
        if !self.chunks_near_exist(cx * 16 + 8, 0, cz * 16 + 8, 16) {
            return;
        }
        for x in 0..16 {
            for z in 0..16 {
                let i = (x + z * 16) as usize;
                let c = self.chunks.get_mut(&(cx, cz)).unwrap();
                if c.update_sky_cols[i] {
                    c.update_sky_cols[i] = false;
                    let h = c.height[(z << 4 | x) as usize];
                    let (wx, wz) = (cx * 16 + x, cz * 16 + z);
                    let mut m = self.height_min(wx - 1, wz);
                    m = m.min(self.height_min(wx + 1, wz));
                    m = m.min(self.height_min(wx, wz - 1));
                    m = m.min(self.height_min(wx, wz + 1));
                    self.check_skylight_neighbor_height(wx, wz, m);
                    self.check_skylight_neighbor_height(wx - 1, wz, h);
                    self.check_skylight_neighbor_height(wx + 1, wz, h);
                    self.check_skylight_neighbor_height(wx, wz - 1, h);
                    self.check_skylight_neighbor_height(wx, wz + 1, h);
                }
            }
        }
        self.chunks.get_mut(&(cx, cz)).unwrap().gap_updated = false;
    }

    /// Test hook: runs `checkLight` and leaves the chunk unpopulated.
    pub fn debug_check_light(&mut self, cx: i32, cz: i32) {
        self.check_light(cx, cz);
        self.chunks.get_mut(&(cx, cz)).unwrap().terrain_populated = false;
    }

    /// `Chunk.func_150809_p` (checkLight)
    fn check_light(&mut self, cx: i32, cz: i32) {
        {
            let c = self.chunk(cx, cz);
            c.terrain_populated = true;
            c.light_populated = true;
        }
        if self.check_chunks_exist(cx * 16 - 1, 0, cz * 16 - 1, cx * 16 + 1, 63, cz * 16 + 1) {
            'outer: for x in 0..16 {
                for z in 0..16 {
                    if !self.check_light_column(cx, cz, x, z) {
                        self.chunks.get_mut(&(cx, cz)).unwrap().light_populated = false;
                        // Vanilla only breaks the inner loop.
                        continue 'outer;
                    }
                }
            }
            if self.chunks[&(cx, cz)].light_populated {
                self.check_border_light(cx - 1, cz, 3);
                self.check_border_light(cx + 1, cz, 1);
                self.check_border_light(cx, cz - 1, 0);
                self.check_border_light(cx, cz + 1, 2);
            }
        } else {
            self.chunks.get_mut(&(cx, cz)).unwrap().light_populated = false;
        }
    }

    fn check_border_light(&mut self, cx: i32, cz: i32, side: i32) {
        if !self.chunk(cx, cz).terrain_populated {
            return;
        }
        for i in 0..16 {
            match side {
                3 => self.check_light_column(cx, cz, 15, i),
                1 => self.check_light_column(cx, cz, 0, i),
                0 => self.check_light_column(cx, cz, i, 15),
                _ => self.check_light_column(cx, cz, i, 0),
            };
        }
    }

    /// `Chunk.func_150811_f`
    fn check_light_column(&mut self, cx: i32, cz: i32, x: i32, z: i32) -> bool {
        let top = self.chunks[&(cx, cz)].top_filled_segment();
        let mut seen_opaque = false;
        let mut below_solid = false;
        let mut y = top + 16 - 1;
        while y > 63 || y > 0 && !below_solid {
            let op = self.chunks[&(cx, cz)].opacity(x, y, z);
            if op == 255 && y < 63 {
                below_solid = true;
            }
            if !seen_opaque && op > 0 {
                seen_opaque = true;
            } else if seen_opaque && op == 0 && !self.update_all_light(cx * 16 + x, y, cz * 16 + z) {
                return false;
            }
            y -= 1;
        }
        while y > 0 {
            if block::info(self.chunks[&(cx, cz)].block(x, y, z)).light_value > 0 {
                self.update_all_light(cx * 16 + x, y, cz * 16 + z);
            }
            y -= 1;
        }
        true
    }

    /// `Chunk.enqueueRelightChecks`
    fn enqueue_relight_checks(&mut self, cx: i32, cz: i32) {
        for _ in 0..8 {
            let q = self.chunks[&(cx, cz)].queued_light_checks;
            if q >= 4096 {
                return;
            }
            let sy = q % 16;
            let lx = q / 16 % 16;
            let lz = q / 256;
            self.chunks.get_mut(&(cx, cz)).unwrap().queued_light_checks += 1;
            let (wx, wz) = ((cx << 4) + lx, (cz << 4) + lz);
            for i in 0..16 {
                let y = (sy << 4) + i;
                let c = &self.chunks[&(cx, cz)];
                let has = c.has_section(y);
                let edge = i == 0 || i == 15 || lx == 0 || lx == 15 || lz == 0 || lz == 15;
                if (!has && edge) || (has && block::material(c.data.blocks[bidx(lx, y, lz)]) == Material::Air) {
                    for f in FACING {
                        let (nx, ny, nz) = (wx + f[0], y + f[1], wz + f[2]);
                        if block::info(self.block(nx, ny, nz)).light_value > 0 {
                            self.update_all_light(nx, ny, nz);
                        }
                    }
                    self.update_all_light(wx, y, wz);
                }
            }
        }
    }

    /// `Chunk.func_150804_b` (per-tick chunk maintenance) plus the relight
    /// queue from `World.func_147467_a`.
    pub fn tick_chunk(&mut self, cx: i32, cz: i32) {
        if !self.chunk_exists(cx, cz) {
            return;
        }
        self.enqueue_relight_checks(cx, cz);
        if self.chunks[&(cx, cz)].gap_updated {
            self.recheck_gaps(cx, cz);
        }
        let c = self.chunks.get_mut(&(cx, cz)).unwrap();
        c.ticked = true;
        if !c.light_populated && c.terrain_populated {
            self.check_light(cx, cz);
        }
    }

    // ---------------------------------------------------------------- light

    /// `World.func_147451_t` (updateAllLightTypes)
    pub fn update_all_light(&mut self, x: i32, y: i32, z: i32) -> bool {
        let a = self.update_light(true, x, y, z);
        let b = self.update_light(false, x, y, z);
        a | b
    }

    /// `World.computeLightValue`
    fn compute_light(&mut self, x: i32, y: i32, z: i32, sky: bool) -> i32 {
        if sky && self.can_see_sky(x, y, z) {
            return 15;
        }
        let info = block::info(self.block(x, y, z));
        let mut l = if sky { 0 } else { info.light_value as i32 };
        let mut op = info.light_opacity as i32;
        if op >= 15 && info.light_value > 0 {
            op = 1;
        }
        if op < 1 {
            op = 1;
        }
        if op >= 15 {
            return 0;
        }
        if l >= 14 {
            return l;
        }
        for f in FACING {
            let v = self.saved_light(sky, x + f[0], y + f[1], z + f[2]) - op;
            if v > l {
                l = v;
            }
            if l >= 14 {
                return l;
            }
        }
        l
    }

    /// `World.updateLightByType`
    pub fn update_light(&mut self, sky: bool, x: i32, y: i32, z: i32) -> bool {
        if !self.chunks_near_exist(x, y, z, 17) {
            return false;
        }
        let mut q = std::mem::take(&mut self.light_queue);
        let mut head = 0usize;
        let mut tail = 0usize;
        let saved = self.saved_light(sky, x, y, z);
        let computed = self.compute_light(x, y, z, sky);
        if computed > saved {
            q[tail] = 133152;
            tail += 1;
        } else if computed < saved {
            q[tail] = 133152 | saved << 18;
            tail += 1;
            while head < tail {
                let e = q[head];
                head += 1;
                let px = (e & 63) - 32 + x;
                let py = (e >> 6 & 63) - 32 + y;
                let pz = (e >> 12 & 63) - 32 + z;
                let lvl = e >> 18 & 15;
                let cur = self.saved_light(sky, px, py, pz);
                if cur == lvl {
                    self.set_light(sky, px, py, pz, 0);
                    if lvl > 0 {
                        let d = (px - x).abs() + (py - y).abs() + (pz - z).abs();
                        if d < 17 {
                            for f in FACING {
                                let (nx, ny, nz) = (px + f[0], py + f[1], pz + f[2]);
                                let op = (block::info(self.block(nx, ny, nz)).light_opacity as i32).max(1);
                                let nl = self.saved_light(sky, nx, ny, nz);
                                if nl == lvl - op && tail < q.len() {
                                    q[tail] = (nx - x + 32) | (ny - y + 32) << 6 | (nz - z + 32) << 12 | (lvl - op) << 18;
                                    tail += 1;
                                }
                            }
                        }
                    }
                }
            }
            head = 0;
        }
        while head < tail {
            let e = q[head];
            head += 1;
            let px = (e & 63) - 32 + x;
            let py = (e >> 6 & 63) - 32 + y;
            let pz = (e >> 12 & 63) - 32 + z;
            let cur = self.saved_light(sky, px, py, pz);
            let new = self.compute_light(px, py, pz, sky);
            if new != cur {
                self.set_light(sky, px, py, pz, new);
                if new > cur {
                    let d = (px - x).abs() + (py - y).abs() + (pz - z).abs();
                    if d < 17 && tail < q.len() - 6 {
                        for (dx, dy, dz) in [(-1, 0, 0), (1, 0, 0), (0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1)] {
                            if self.saved_light(sky, px + dx, py + dy, pz + dz) < new {
                                q[tail] = (px + dx - x + 32) + ((py + dy - y + 32) << 6) + ((pz + dz - z + 32) << 12);
                                tail += 1;
                            }
                        }
                    }
                }
            }
        }
        self.light_queue = q;
        true
    }

    // ---------------------------------------------------------------- updates

    /// `World.notifyBlocksOfNeighborChange`
    pub fn notify_neighbours(&mut self, x: i32, y: i32, z: i32, src: u8) {
        self.on_neighbor_change(x - 1, y, z, src);
        self.on_neighbor_change(x + 1, y, z, src);
        self.on_neighbor_change(x, y - 1, z, src);
        self.on_neighbor_change(x, y + 1, z, src);
        self.on_neighbor_change(x, y, z - 1, src);
        self.on_neighbor_change(x, y, z + 1, src);
    }

    /// `WorldServer.scheduleBlockUpdateWithPriority`
    pub fn schedule_update(&mut self, x: i32, y: i32, z: i32, id: u8, mut delay: i32) {
        let mut r = 0;
        if self.scheduled_immediate && block::material(id) != Material::Air {
            // Every block except fire `requiresUpdates()`.
            if id != block::FIRE {
                r = 8;
                if self.check_chunks_exist(x - r, y - r, z - r, x + r, y + r, z + r) {
                    let b = self.block(x, y, z);
                    if block::material(b) != Material::Air && b == id {
                        let mut rnd = std::mem::replace(&mut self.rand, JavaRandom::new(0));
                        self.update_tick(b, x, y, z, &mut rnd);
                        self.rand = rnd;
                    }
                }
                return;
            }
            delay = 1;
        }
        if self.check_chunks_exist(x - r, y - r, z - r, x + r, y + r, z + r) {
            if self.tick_keys.insert((x, y, z, id)) {
                let time = if block::material(id) != Material::Air { delay as i64 + self.total_time } else { 0 };
                self.next_tick_id += 1;
                self.ticks.insert(Tick { time, priority: 0, id: self.next_tick_id, x, y, z, block: id });
            }
        }
    }

    /// `WorldServer.tickUpdates(false)`
    pub fn tick_updates(&mut self) {
        let n = self.ticks.len().min(1000);
        let mut now = Vec::new();
        for _ in 0..n {
            let Some(&t) = self.ticks.first() else { break };
            if t.time > self.total_time {
                break;
            }
            self.ticks.remove(&t);
            self.tick_keys.remove(&(t.x, t.y, t.z, t.block));
            now.push(t);
        }
        for t in now {
            if self.check_chunks_exist(t.x, t.y, t.z, t.x, t.y, t.z) {
                let b = self.block(t.x, t.y, t.z);
                if block::material(b) != Material::Air && b == t.block {
                    let mut rnd = std::mem::replace(&mut self.rand, JavaRandom::new(0));
                    self.update_tick(b, t.x, t.y, t.z, &mut rnd);
                    self.rand = rnd;
                }
            } else {
                self.schedule_update(t.x, t.y, t.z, t.block, 0);
            }
        }
    }

    /// Re-queues a scheduled update read from a save file.
    pub fn restore_tick(&mut self, x: i32, y: i32, z: i32, id: u8, delay: i64) {
        if self.tick_keys.insert((x, y, z, id)) {
            self.next_tick_id += 1;
            self.ticks.insert(Tick { time: self.total_time + delay, priority: 0, id: self.next_tick_id, x, y, z, block: id });
        }
    }

    pub fn pending_ticks(&self) -> usize {
        self.ticks.len()
    }

    // ---------------------------------------------------------------- block hooks

    /// `Block.breakBlock` of the replaced block.
    fn break_block(&mut self, old: u8, _old_meta: i32, x: i32, y: i32, z: i32) {
        let (r, only_unset) = match old {
            block::LEAVES | block::LEAVES2 => (1, false),
            block::LOG | block::LOG2 => (4, true),
            _ => return,
        };
        let e = r + 1;
        if !self.check_chunks_exist(x - e, y - e, z - e, x + e, y + e, z + e) {
            return;
        }
        for dx in -r..=r {
            for dy in -r..=r {
                for dz in -r..=r {
                    if self.material(x + dx, y + dy, z + dz) == Material::Leaves {
                        let m = self.meta(x + dx, y + dy, z + dz);
                        if !only_unset || m & 8 == 0 {
                            self.set_meta(x + dx, y + dy, z + dz, m | 8, 4);
                        }
                    }
                }
            }
        }
    }

    /// `Block.onBlockAdded`
    fn on_block_added(&mut self, id: u8, x: i32, y: i32, z: i32) {
        match id {
            block::FLOWING_WATER | block::FLOWING_LAVA => {
                self.check_liquid_collisions(id, x, y, z);
                if self.block(x, y, z) == id {
                    let rate = liquid_tick_rate(id);
                    self.schedule_update(x, y, z, id, rate);
                }
            }
            block::WATER | block::LAVA => self.check_liquid_collisions(id, x, y, z),
            block::SAND | block::GRAVEL => self.schedule_update(x, y, z, id, 2),
            block::CHEST => {
                self.chest_init_meta(x, y, z);
                for (dx, dz) in [(0, -1), (0, 1), (-1, 0), (1, 0)] {
                    if self.block(x + dx, y, z + dz) == block::CHEST {
                        self.chest_init_meta(x + dx, y, z + dz);
                    }
                }
            }
            _ => {}
        }
    }

    /// `BlockChest.func_149954_e`
    fn chest_init_meta(&mut self, x: i32, y: i32, z: i32) {
        let full = |w: &mut ServerWorld, x: i32, y: i32, z: i32| block::info(w.block(x, y, z)).full_block;
        let n = self.block(x, y, z - 1);
        let s = self.block(x, y, z + 1);
        let w = self.block(x - 1, y, z);
        let e = self.block(x + 1, y, z);
        let c = block::CHEST;
        let (fn_, fs, fw, fe) = (full(self, x, y, z - 1), full(self, x, y, z + 1), full(self, x - 1, y, z), full(self, x + 1, y, z));
        let mut m;
        if n != c && s != c {
            if w != c && e != c {
                m = 3;
                if fn_ && !fs {
                    m = 3;
                }
                if fs && !fn_ {
                    m = 2;
                }
                if fw && !fe {
                    m = 5;
                }
                if fe && !fw {
                    m = 4;
                }
            } else {
                let ox = if w == c { x - 1 } else { x + 1 };
                let on = full(self, ox, y, z - 1);
                let os = full(self, ox, y, z + 1);
                m = 3;
                let other = self.meta(ox, y, z);
                if other == 2 {
                    m = 2;
                }
                if (fn_ || on) && !fs && !os {
                    m = 3;
                }
                if (fs || os) && !fn_ && !on {
                    m = 2;
                }
            }
        } else {
            let oz = if n == c { z - 1 } else { z + 1 };
            let ow = full(self, x - 1, y, oz);
            let oe = full(self, x + 1, y, oz);
            m = 5;
            let other = self.meta(x, y, oz);
            if other == 4 {
                m = 4;
            }
            if (fw || ow) && !fe && !oe {
                m = 5;
            }
            if (fe || oe) && !fw && !ow {
                m = 4;
            }
        }
        self.set_meta(x, y, z, m, 3);
    }

    /// `BlockLiquid.func_149805_n`
    fn check_liquid_collisions(&mut self, id: u8, x: i32, y: i32, z: i32) {
        if self.block(x, y, z) != id || block::material(id) != Material::Lava {
            return;
        }
        let water = self.material(x, y, z - 1) == Material::Water
            || self.material(x, y, z + 1) == Material::Water
            || self.material(x - 1, y, z) == Material::Water
            || self.material(x + 1, y, z) == Material::Water
            || self.material(x, y + 1, z) == Material::Water;
        if water {
            let m = self.meta(x, y, z);
            if m == 0 {
                self.set_block(x, y, z, block::OBSIDIAN, 0, 3);
            } else if m <= 4 {
                self.set_block(x, y, z, block::COBBLESTONE, 0, 3);
            }
        }
    }

    /// `Block.canBlockStay`
    pub fn can_block_stay(&mut self, id: u8, x: i32, y: i32, z: i32) -> bool {
        let below = self.block(x, y - 1, z);
        match id {
            block::TALLGRASS | block::YELLOW_FLOWER | block::RED_FLOWER => below == block::GRASS || below == block::DIRT || below == block::FARMLAND,
            block::DEADBUSH => below == block::SAND || below == block::HARDENED_CLAY || below == block::STAINED_HARDENED_CLAY || below == block::DIRT,
            block::BROWN_MUSHROOM | block::RED_MUSHROOM => {
                if (0..256).contains(&y) {
                    below == block::MYCELIUM
                        || (below == block::DIRT && self.meta(x, y - 1, z) == 2)
                        || (self.full_block_light(x, y, z) < 13 && block::info(below).full_block)
                } else {
                    false
                }
            }
            block::REEDS => self.reed_can_place(x, y, z),
            block::CACTUS => {
                for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    if self.material(x + dx, y, z + dz).is_solid() {
                        return false;
                    }
                }
                below == block::CACTUS || below == block::SAND
            }
            block::WATERLILY => (0..256).contains(&y) && block::material(below) == Material::Water && self.meta(x, y - 1, z) == 0,
            block::DOUBLE_PLANT => {
                let m = self.meta(x, y, z);
                if m & 8 != 0 {
                    below == block::DOUBLE_PLANT
                } else {
                    self.block(x, y + 1, z) == block::DOUBLE_PLANT && (below == block::GRASS || below == block::DIRT || below == block::FARMLAND)
                }
            }
            _ => true,
        }
    }

    /// `BlockReed.canPlaceBlockAt`
    fn reed_can_place(&mut self, x: i32, y: i32, z: i32) -> bool {
        let below = self.block(x, y - 1, z);
        if below == block::REEDS {
            return true;
        }
        if below != block::GRASS && below != block::DIRT && below != block::SAND {
            return false;
        }
        self.material(x - 1, y - 1, z) == Material::Water
            || self.material(x + 1, y - 1, z) == Material::Water
            || self.material(x, y - 1, z - 1) == Material::Water
            || self.material(x, y - 1, z + 1) == Material::Water
    }

    /// `Block.canPlaceBlockAt`
    pub fn can_place_block_at(&mut self, id: u8, x: i32, y: i32, z: i32) -> bool {
        let replaceable = self.material(x, y, z).replaceable();
        match id {
            block::WATERLILY => replaceable && self.block(x, y - 1, z) == block::WATER,
            block::DOUBLE_PLANT => {
                let below = self.block(x, y - 1, z);
                replaceable && (below == block::GRASS || below == block::DIRT || below == block::FARMLAND) && self.is_air(x, y + 1, z)
            }
            block::PUMPKIN => replaceable && self.solid_top_surface(x, y - 1, z),
            block::MELON_BLOCK => replaceable,
            block::SNOW_LAYER => self.snow_can_place(x, y, z),
            block::CACTUS => replaceable && self.can_block_stay(id, x, y, z),
            block::REEDS => self.reed_can_place(x, y, z),
            _ => replaceable,
        }
    }

    /// `BlockSnow.canPlaceBlockAt`
    fn snow_can_place(&mut self, x: i32, y: i32, z: i32) -> bool {
        let below = self.block(x, y - 1, z);
        if below == block::ICE || below == block::PACKED_ICE {
            return false;
        }
        let info = block::info(below);
        if info.material == Material::Leaves {
            return true;
        }
        if below == block::SNOW_LAYER && self.meta(x, y - 1, z) & 7 == 7 {
            return true;
        }
        info.opaque && info.material.blocks_movement()
    }

    /// `World.doesBlockHaveSolidTopSurface`
    pub fn solid_top_surface(&mut self, x: i32, y: i32, z: i32) -> bool {
        let id = self.block(x, y, z);
        let info = block::info(id);
        if info.normal_cube() {
            return true;
        }
        let m = self.meta(x, y, z);
        match id {
            block::STONE_SLAB => m & 8 == 8,
            block::SNOW_LAYER => m & 7 == 7,
            _ => false,
        }
    }

    /// `BlockVine.func_150093_a` (canPlaceOn)
    pub fn vine_can_attach(&mut self, x: i32, y: i32, z: i32) -> bool {
        let info = block::info(self.block(x, y, z));
        info.normal && info.material.blocks_movement()
    }

    /// `Block.onNeighborBlockChange`
    fn on_neighbor_change(&mut self, x: i32, y: i32, z: i32, _src: u8) {
        let id = self.block(x, y, z);
        match id {
            block::WATER | block::LAVA => {
                self.check_liquid_collisions(id, x, y, z);
                if self.block(x, y, z) == id {
                    // setNotStationary
                    let m = self.meta(x, y, z);
                    self.set_block(x, y, z, id - 1, m, 2);
                    self.schedule_update(x, y, z, id - 1, liquid_tick_rate(id));
                }
            }
            block::FLOWING_WATER | block::FLOWING_LAVA => self.check_liquid_collisions(id, x, y, z),
            block::SAND | block::GRAVEL => self.schedule_update(x, y, z, id, 2),
            block::TALLGRASS | block::DEADBUSH | block::YELLOW_FLOWER | block::RED_FLOWER | block::BROWN_MUSHROOM | block::RED_MUSHROOM | block::WATERLILY => {
                if !self.can_block_stay(id, x, y, z) {
                    self.set_block(x, y, z, block::AIR, 0, 2);
                }
            }
            block::DOUBLE_PLANT => {
                if !self.can_block_stay(id, x, y, z) {
                    let m = self.meta(x, y, z);
                    if m & 8 == 0 && self.block(x, y + 1, z) == id {
                        self.set_block(x, y + 1, z, block::AIR, 0, 2);
                    }
                    self.set_block(x, y, z, block::AIR, 0, 2);
                }
            }
            block::REEDS => {
                if !self.reed_can_place(x, y, z) {
                    self.set_air(x, y, z);
                }
            }
            block::CACTUS => {
                if !self.can_block_stay(id, x, y, z) {
                    self.set_air(x, y, z);
                }
            }
            block::SNOW_LAYER => {
                if !self.snow_can_place(x, y, z) {
                    self.set_air(x, y, z);
                }
            }
            block::VINE => {
                if !self.vine_has_connections(x, y, z) {
                    self.set_air(x, y, z);
                }
            }
            _ => {}
        }
    }

    /// `BlockVine.func_150094_e`
    fn vine_has_connections(&mut self, x: i32, y: i32, z: i32) -> bool {
        // Direction.offsetX / offsetZ
        const OX: [i32; 4] = [0, -1, 0, 1];
        const OZ: [i32; 4] = [1, 0, -1, 0];
        let m = self.meta(x, y, z);
        let mut k = m;
        if k > 0 {
            for d in 0..4 {
                let bit = 1 << d;
                if m & bit != 0
                    && !self.vine_can_attach(x + OX[d], y, z + OZ[d])
                    && (self.block(x, y + 1, z) != block::VINE || self.meta(x, y + 1, z) & bit == 0)
                {
                    k &= !bit;
                }
            }
        }
        if k == 0 && !self.vine_can_attach(x, y + 1, z) {
            return false;
        }
        if k != m {
            self.set_meta(x, y, z, k, 2);
        }
        true
    }

    /// `Block.updateTick`
    pub fn update_tick(&mut self, id: u8, x: i32, y: i32, z: i32, rand: &mut JavaRandom) {
        match id {
            block::FLOWING_WATER | block::FLOWING_LAVA => self.liquid_tick(id, x, y, z, rand),
            block::SAND | block::GRAVEL => self.try_fall(id, x, y, z),
            _ => {}
        }
    }

    fn can_fall_below(&mut self, x: i32, y: i32, z: i32) -> bool {
        let b = self.block(x, y, z);
        let m = block::material(b);
        m == Material::Air || b == block::FIRE || m == Material::Water || m == Material::Lava
    }

    /// `BlockFalling.func_149830_m`. Falling-block entities are resolved
    /// immediately to their resting position.
    fn try_fall(&mut self, id: u8, x: i32, mut y: i32, z: i32) {
        if self.can_fall_below(x, y - 1, z) && y >= 0 {
            let meta = self.meta(x, y, z);
            self.set_air(x, y, z);
            while self.can_fall_below(x, y - 1, z) && y > 0 {
                y -= 1;
            }
            if y > 0 {
                self.set_block(x, y, z, id, meta, 3);
            }
        }
    }

    // ---------------------------------------------------------------- liquids

    fn liquid_level(&mut self, mat: Material, x: i32, y: i32, z: i32) -> i32 {
        if self.material(x, y, z) == mat {
            self.meta(x, y, z)
        } else {
            -1
        }
    }

    fn blocks_spreading(&mut self, x: i32, y: i32, z: i32) -> bool {
        let b = self.block(x, y, z);
        if b == block::REEDS {
            return true;
        }
        block::material(b).blocks_movement()
    }

    fn can_spread_to(&mut self, mat: Material, x: i32, y: i32, z: i32) -> bool {
        let m = self.material(x, y, z);
        if m == mat || m == Material::Lava {
            false
        } else {
            !self.blocks_spreading(x, y, z)
        }
    }

    fn spread_to(&mut self, id: u8, x: i32, y: i32, z: i32, level: i32) {
        if self.can_spread_to(block::material(id), x, y, z) {
            self.set_block(x, y, z, id, level, 3);
        }
    }

    fn distance_to_gap(&mut self, mat: Material, x: i32, y: i32, z: i32, depth: i32, from: i32) -> i32 {
        let mut best = 1000;
        for d in 0..4 {
            if (d == 0 && from == 1) || (d == 1 && from == 0) || (d == 2 && from == 3) || (d == 3 && from == 2) {
                continue;
            }
            let (nx, nz) = match d {
                0 => (x - 1, z),
                1 => (x + 1, z),
                2 => (x, z - 1),
                _ => (x, z + 1),
            };
            if !self.blocks_spreading(nx, y, nz) && (self.material(nx, y, nz) != mat || self.meta(nx, y, nz) != 0) {
                if !self.blocks_spreading(nx, y - 1, nz) {
                    return depth;
                }
                if depth < 4 {
                    let v = self.distance_to_gap(mat, nx, y, nz, depth + 1, d);
                    if v < best {
                        best = v;
                    }
                }
            }
        }
        best
    }

    fn spread_directions(&mut self, mat: Material, x: i32, y: i32, z: i32) -> [bool; 4] {
        let mut dist = [1000i32; 4];
        for d in 0..4 {
            let (nx, nz) = match d {
                0 => (x - 1, z),
                1 => (x + 1, z),
                2 => (x, z - 1),
                _ => (x, z + 1),
            };
            if !self.blocks_spreading(nx, y, nz) && (self.material(nx, y, nz) != mat || self.meta(nx, y, nz) != 0) {
                if self.blocks_spreading(nx, y - 1, nz) {
                    dist[d] = self.distance_to_gap(mat, nx, y, nz, 1, d as i32);
                } else {
                    dist[d] = 0;
                }
            }
        }
        let m = *dist.iter().min().unwrap();
        [dist[0] == m, dist[1] == m, dist[2] == m, dist[3] == m]
    }

    /// `BlockDynamicLiquid.updateTick`
    fn liquid_tick(&mut self, id: u8, x: i32, y: i32, z: i32, rand: &mut JavaRandom) {
        let mat = block::material(id);
        let mut level = self.liquid_level(mat, x, y, z);
        let step = if mat == Material::Lava { 2 } else { 1 };
        let mut rate = liquid_tick_rate(id);
        if level > 0 {
            let mut sources = 0;
            let mut min = -100;
            for (nx, nz) in [(x - 1, z), (x + 1, z), (x, z - 1), (x, z + 1)] {
                // func_149810_a
                let mut l = self.liquid_level(mat, nx, y, nz);
                if l < 0 {
                    continue;
                }
                if l == 0 {
                    sources += 1;
                }
                if l >= 8 {
                    l = 0;
                }
                min = if min >= 0 && l >= min { min } else { l };
            }
            let mut new = min + step;
            if new >= 8 || min < 0 {
                new = -1;
            }
            let above = self.liquid_level(mat, x, y + 1, z);
            if above >= 0 {
                new = if above >= 8 { above } else { above + 8 };
            }
            if sources >= 2 && mat == Material::Water {
                if self.material(x, y - 1, z).is_solid() {
                    new = 0;
                } else if self.material(x, y - 1, z) == mat && self.meta(x, y - 1, z) == 0 {
                    new = 0;
                }
            }
            if mat == Material::Lava && level < 8 && new < 8 && new > level && rand.next_int_n(4) != 0 {
                rate *= 4;
            }
            if new == level {
                // func_149811_n: become still
                let m = self.meta(x, y, z);
                self.set_block(x, y, z, id + 1, m, 2);
            } else {
                level = new;
                if level < 0 {
                    self.set_air(x, y, z);
                } else {
                    self.set_meta(x, y, z, level, 2);
                    self.schedule_update(x, y, z, id, rate);
                    self.notify_neighbours(x, y, z, id);
                }
            }
        } else {
            let m = self.meta(x, y, z);
            self.set_block(x, y, z, id + 1, m, 2);
        }
        if self.can_spread_to(mat, x, y - 1, z) {
            if mat == Material::Lava && self.material(x, y - 1, z) == Material::Water {
                self.set_block(x, y - 1, z, block::STONE, 0, 3);
                return;
            }
            if level >= 8 {
                self.spread_to(id, x, y - 1, z, level);
            } else {
                self.spread_to(id, x, y - 1, z, level + 8);
            }
        } else if level >= 0 && (level == 0 || self.blocks_spreading(x, y - 1, z)) {
            // Vanilla keeps the result in an instance field of the block
            // (`field_149814_b`). With immediate updates the nested ticks
            // triggered by each spread overwrite it while this loop is still
            // reading it, so the array must be shared and re-read every time.
            let slot = if mat == Material::Water { 0 } else { 1 };
            self.spread_dirs[slot] = self.spread_directions(mat, x, y, z);
            let mut next = level + step;
            if level >= 8 {
                next = 1;
            }
            if next >= 8 {
                return;
            }
            if self.spread_dirs[slot][0] {
                self.spread_to(id, x - 1, y, z, next);
            }
            if self.spread_dirs[slot][1] {
                self.spread_to(id, x + 1, y, z, next);
            }
            if self.spread_dirs[slot][2] {
                self.spread_to(id, x, y, z - 1, next);
            }
            if self.spread_dirs[slot][3] {
                self.spread_to(id, x, y, z + 1, next);
            }
        }
    }

    // ---------------------------------------------------------------- climate

    /// `World.canBlockFreeze`
    pub fn can_block_freeze(&mut self, x: i32, y: i32, z: i32, natural: bool) -> bool {
        let b = self.biome(x, z);
        let t = b.float_temperature(&self.provider.terrain.noise, x, y, z);
        if t > 0.15f32 {
            return false;
        }
        if (0..256).contains(&y) && self.saved_light(false, x, y, z) < 10 {
            let id = self.block(x, y, z);
            if (id == block::WATER || id == block::FLOWING_WATER) && self.meta(x, y, z) == 0 {
                if !natural {
                    return true;
                }
                let all_water = self.material(x - 1, y, z) == Material::Water
                    && self.material(x + 1, y, z) == Material::Water
                    && self.material(x, y, z - 1) == Material::Water
                    && self.material(x, y, z + 1) == Material::Water;
                if !all_water {
                    return true;
                }
            }
        }
        false
    }

    /// `World.func_147478_e` (canSnowAt)
    pub fn can_snow_at(&mut self, x: i32, y: i32, z: i32, check_light: bool) -> bool {
        let b = self.biome(x, z);
        let t = b.float_temperature(&self.provider.terrain.noise, x, y, z);
        if t > 0.15f32 {
            return false;
        }
        if !check_light {
            return true;
        }
        if (0..256).contains(&y) && self.saved_light(false, x, y, z) < 10 {
            return self.is_air(x, y, z) && self.snow_can_place(x, y, z);
        }
        false
    }

    /// One server tick of world-level block processing.
    pub fn tick(&mut self) {
        self.total_time += 1;
        self.tick_updates();
    }
}

/// `BlockLiquid.tickRate` (overworld)
fn liquid_tick_rate(id: u8) -> i32 {
    if block::material(id) == Material::Water {
        5
    } else {
        30
    }
}
