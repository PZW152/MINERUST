//! Port of `ChunkProviderGenerate` (terrain shape, surface, caves and ravines)
//! and the parts of `WorldChunkManager` it needs.

use super::caves::{self, Carver};
use super::layer::GenLayer;
use super::noise::{NoiseGeneratorOctaves, NoiseGeneratorPerlin};
use crate::block;
use crate::java::JavaRandom;
use crate::util::math;
use crate::world::biome::{self, Biome, Terrain, TerrainState};
use std::sync::Arc;

/// Raw output of `provideChunk`. Block index is `(x * 16 + z) * 256 + y`.
pub struct ChunkData {
    pub cx: i32,
    pub cz: i32,
    pub blocks: Box<[u8; 65536]>,
    pub meta: Box<[u8; 65536]>,
    /// Biome ids, index `z * 16 + x`.
    pub biomes: [u8; 256],
    /// Sky light (0..15), same indexing as `blocks`.
    pub sky: Box<[u8; 65536]>,
    /// Block light (0..15).
    pub blk: Box<[u8; 65536]>,
}

impl Clone for ChunkData {
    fn clone(&self) -> Self {
        ChunkData {
            cx: self.cx,
            cz: self.cz,
            blocks: self.blocks.clone(),
            meta: self.meta.clone(),
            biomes: self.biomes,
            sky: self.sky.clone(),
            blk: self.blk.clone(),
        }
    }
}

pub struct ChunkProvider {
    pub seed: i64,
    min_limit: NoiseGeneratorOctaves,
    max_limit: NoiseGeneratorOctaves,
    main: NoiseGeneratorOctaves,
    surface: NoiseGeneratorPerlin,
    depth: NoiseGeneratorOctaves,
    parabolic: [f32; 25],
    pub gen_biomes: Arc<GenLayer>,
    pub biome_index: Arc<GenLayer>,
    pub terrain: TerrainState,
    /// `WorldProvider.dimensionId`: 0 overworld, -1 Nether, 1 End.
    pub dim: i32,
    pub hell: Option<Box<super::hell::HellGen>>,
    pub end: Option<Box<super::end::EndGen>>,
}

pub fn boxed() -> Box<[u8; 65536]> {
    vec![0u8; 65536].into_boxed_slice().try_into().unwrap()
}

/// Key of the Java object whose mutable `topBlock` field a biome id reads/writes.
fn top_keys(b: &Biome) -> (Option<usize>, usize) {
    // Generic `BiomeGenMutated` wrappers delegate `genTerrainBlocks` to the base
    // biome object, so the write lands on the base and their own field is static.
    let generic_mutated = b.color_zy_quirk && b.terrain != Terrain::SavannaMutated;
    if generic_mutated {
        (None, (b.id as usize).wrapping_sub(128) & 255)
    } else {
        (Some(b.id as usize), b.id as usize)
    }
}

impl ChunkProvider {
    pub fn new(seed: i64) -> Self {
        let mut rand = JavaRandom::new(seed);
        let min_limit = NoiseGeneratorOctaves::new(&mut rand, 16);
        let max_limit = NoiseGeneratorOctaves::new(&mut rand, 16);
        let main = NoiseGeneratorOctaves::new(&mut rand, 8);
        let surface = NoiseGeneratorPerlin::new(&mut rand, 4);
        let _noise5 = NoiseGeneratorOctaves::new(&mut rand, 10);
        let depth = NoiseGeneratorOctaves::new(&mut rand, 16);
        let _mob_spawner = NoiseGeneratorOctaves::new(&mut rand, 8);
        let mut parabolic = [0f32; 25];
        for i in -2i32..=2 {
            for j in -2i32..=2 {
                parabolic[(i + 2 + (j + 2) * 5) as usize] =
                    10.0f32 / math::sqrt_float((i * i + j * j) as f32 + 0.2f32);
            }
        }
        let (gen_biomes, biome_index) = GenLayer::build(seed);
        ChunkProvider {
            seed,
            min_limit,
            max_limit,
            main,
            surface,
            depth,
            parabolic,
            gen_biomes,
            biome_index,
            terrain: TerrainState::new(seed),
            dim: 0,
            hell: None,
            end: None,
        }
    }

    /// The generator of another dimension (`WorldProviderHell` / `WorldProviderEnd`).
    pub fn new_dim(seed: i64, dim: i32) -> Self {
        let mut p = Self::new(seed);
        p.dim = dim;
        match dim {
            -1 => p.hell = Some(Box::new(super::hell::HellGen::new(seed))),
            1 => p.end = Some(Box::new(super::end::EndGen::new(seed))),
            _ => {}
        }
        p
    }

    /// `WorldChunkManager.getBiomeGenAt(x, z)` id.
    pub fn biome_at(&self, x: i32, z: i32) -> i32 {
        match self.dim {
            -1 => return 8,
            1 => return 9,
            _ => {}
        }
        self.biome_index.get_ints(x, z, 1, 1)[0]
    }

    fn height_field(&self, cx: i32, cz: i32, biomes: &[&'static Biome]) -> Vec<f64> {
        let (x, z) = (cx * 4, cz * 4);
        let mut depth = vec![0f64; 25];
        let mut main = vec![0f64; 825];
        let mut lower = vec![0f64; 825];
        let mut upper = vec![0f64; 825];
        self.depth
            .generate_2d(&mut depth, x, z, 5, 5, 200.0, 200.0, 0.5);
        self.main.generate(
            &mut main,
            x,
            0,
            z,
            5,
            33,
            5,
            8.555150000000001,
            4.277575000000001,
            8.555150000000001,
        );
        self.min_limit
            .generate(&mut lower, x, 0, z, 5, 33, 5, 684.412, 684.412, 684.412);
        self.max_limit
            .generate(&mut upper, x, 0, z, 5, 33, 5, 684.412, 684.412, 684.412);
        let mut hm = vec![0f64; 825];
        let mut idx3 = 0usize;
        let mut idx2 = 0usize;
        for i in 0..5usize {
            for j in 0..5usize {
                let mut var18 = 0.0f32;
                let mut var19 = 0.0f32;
                let mut var20 = 0.0f32;
                let centre = biomes[i + 2 + (j + 2) * 10];
                for di in -2i32..=2 {
                    for dj in -2i32..=2 {
                        let b = biomes[(i as i32 + di + 2 + (j as i32 + dj + 2) * 10) as usize];
                        let root = b.root_height;
                        let variation = b.height_variation;
                        let mut w =
                            self.parabolic[(di + 2 + (dj + 2) * 5) as usize] / (root + 2.0f32);
                        if b.root_height > centre.root_height {
                            w /= 2.0f32;
                        }
                        var18 += variation * w;
                        var19 += root * w;
                        var20 += w;
                    }
                }
                var18 /= var20;
                var19 /= var20;
                var18 = var18 * 0.9f32 + 0.1f32;
                var19 = (var19 * 4.0f32 - 1.0f32) / 8.0f32;
                let mut d = depth[idx2] / 8000.0;
                if d < 0.0 {
                    d = -d * 0.3;
                }
                d = d * 3.0 - 2.0;
                if d < 0.0 {
                    d /= 2.0;
                    if d < -1.0 {
                        d = -1.0;
                    }
                    d /= 1.4;
                    d /= 2.0;
                } else {
                    if d > 1.0 {
                        d = 1.0;
                    }
                    d /= 8.0;
                }
                idx2 += 1;
                let mut base = var19 as f64;
                let var = var18 as f64;
                base += d * 0.2;
                base = base * 8.5 / 8.0;
                let h = 8.5 + base * 4.0;
                for y in 0..33 {
                    let mut off = (y as f64 - h) * 12.0 * 128.0 / 256.0 / var;
                    if off < 0.0 {
                        off *= 4.0;
                    }
                    let lo = lower[idx3] / 512.0;
                    let hi = upper[idx3] / 512.0;
                    let t = (main[idx3] / 10.0 + 1.0) / 2.0;
                    let mut v = math::denormalize_clamp(lo, hi, t) - off;
                    if y > 29 {
                        let t = ((y - 29) as f32 / 3.0f32) as f64;
                        v = v * (1.0 - t) + -10.0 * t;
                    }
                    hm[idx3] = v;
                    idx3 += 1;
                }
            }
        }
        hm
    }

    /// `provideChunk(cx, cz)` (terrain, surface, caves, ravines; no structures).
    pub fn provide_chunk(&self, cx: i32, cz: i32) -> ChunkData {
        if let Some(h) = &self.hell {
            return h.provide(cx, cz);
        }
        if let Some(e) = &self.end {
            return e.provide(cx, cz);
        }
        let mut rand = JavaRandom::new(0);
        rand.set_seed(
            (cx as i64)
                .wrapping_mul(341873128712)
                .wrapping_add((cz as i64).wrapping_mul(132897987541)),
        );
        let mut blocks = boxed();
        let mut meta = boxed();

        // func_147424_a
        let gen_ids = self.gen_biomes.get_ints(cx * 4 - 2, cz * 4 - 2, 10, 10);
        let gen_biomes: Vec<&'static Biome> =
            gen_ids.iter().map(|&i| biome::get_or_ocean(i)).collect();
        let hm = self.height_field(cx, cz, &gen_biomes);
        for xi in 0..4usize {
            for zi in 0..4usize {
                let mut i1 = (xi * 5 + zi) * 33;
                let mut i2 = (xi * 5 + zi + 1) * 33;
                let mut i3 = ((xi + 1) * 5 + zi) * 33;
                let mut i4 = ((xi + 1) * 5 + zi + 1) * 33;
                for yi in 0..32usize {
                    let mut d1 = hm[i1 + yi];
                    let mut d2 = hm[i2 + yi];
                    let mut d3 = hm[i3 + yi];
                    let mut d4 = hm[i4 + yi];
                    let s1 = (hm[i1 + yi + 1] - d1) * 0.125;
                    let s2 = (hm[i2 + yi + 1] - d2) * 0.125;
                    let s3 = (hm[i3 + yi + 1] - d3) * 0.125;
                    let s4 = (hm[i4 + yi + 1] - d4) * 0.125;
                    for yy in 0..8usize {
                        let mut a = d1;
                        let mut b = d2;
                        let da = (d3 - d1) * 0.25;
                        let db = (d4 - d2) * 0.25;
                        let y = yi * 8 + yy;
                        for xx in 0..4usize {
                            let base = ((xx + xi * 4) << 12) | ((zi * 4) << 8) | y;
                            let dv = (b - a) * 0.25;
                            let mut v = a - dv;
                            for zz in 0..4usize {
                                v += dv;
                                let idx = base + zz * 256;
                                blocks[idx] = if v > 0.0 {
                                    block::STONE
                                } else if y < 63 {
                                    block::WATER
                                } else {
                                    block::AIR
                                };
                            }
                            a += da;
                            b += db;
                        }
                        d1 += s1;
                        d2 += s2;
                        d3 += s3;
                        d4 += s4;
                    }
                }
                let _ = (&mut i1, &mut i2, &mut i3, &mut i4);
            }
        }

        // replaceBlocksForBiome
        let ids = self.biome_index.get_ints(cx * 16, cz * 16, 16, 16);
        let mut stone_noise = vec![0f64; 256];
        self.surface.get_region(
            &mut stone_noise,
            (cx * 16) as f64,
            (cz * 16) as f64,
            16,
            16,
            0.0625,
            0.0625,
            1.0,
        );
        let mut tops = [0u8; 256];
        for (i, t) in tops.iter_mut().enumerate() {
            *t = biome::get(i as i32).map_or(block::GRASS, |b| b.top_block);
        }
        for a in 0..16i32 {
            for b in 0..16i32 {
                let bi = biome::get_or_ocean(ids[(b + a * 16) as usize]);
                let top = bi.gen_terrain_blocks(
                    &self.terrain,
                    &mut rand,
                    &mut blocks[..],
                    &mut meta[..],
                    cx * 16 + a,
                    cz * 16 + b,
                    stone_noise[(b + a * 16) as usize],
                );
                tops[top_keys(bi).1] = top;
            }
        }

        // Caves / ravines read `getBiomeGenForCoords(x, z).topBlock`.
        let mut col_tops = [0u8; 256];
        for x in 0..16usize {
            for z in 0..16usize {
                let bi = biome::get_or_ocean(ids[x + z * 16]);
                col_tops[x * 16 + z] = match top_keys(bi) {
                    (None, _) => bi.top_block,
                    (Some(k), _) => tops[k],
                };
            }
        }
        {
            let mut c = Carver {
                blocks: &mut blocks[..],
                tops: &col_tops,
                cx,
                cz,
            };
            caves::generate_caves(self.seed, &mut c);
            caves::generate_ravines(self.seed, &mut c);
        }

        // `new Chunk(world, blocks, meta, x, z)` only stores metadata of non-air
        // blocks, so metadata left behind under carved-out cells disappears.
        for i in 0..65536 {
            if blocks[i] == block::AIR {
                meta[i] = 0;
            }
        }

        let mut biomes = [0u8; 256];
        for i in 0..256 {
            // The chunk stores `biomeID` of the object, which for the aliased
            // slot 161 (Redwood Taiga Hills M) is 160.
            biomes[i] = biome::get_or_ocean(ids[i]).id;
        }
        ChunkData {
            cx,
            cz,
            blocks,
            meta,
            biomes,
            sky: boxed(),
            blk: boxed(),
        }
    }

    /// `WorldChunkManager.findBiomePosition` + the search loop of
    /// `WorldServer.createSpawnPosition`. `top_is_grass(x, z)` stands in for
    /// `WorldProvider.canCoordinateBeSpawn`.
    pub fn find_spawn(&self, top_is_grass: &mut dyn FnMut(i32, i32) -> bool) -> (i32, i32) {
        const SPAWN: [i32; 7] = [4, 1, 5, 19, 18, 21, 22];
        let mut rand = JavaRandom::new(self.seed);
        let l = (0 - 256) >> 2;
        let i1 = (0 - 256) >> 2;
        let j1 = (0 + 256) >> 2;
        let k1 = (0 + 256) >> 2;
        let w = j1 - l + 1;
        let h = k1 - i1 + 1;
        let ints = self.gen_biomes.get_ints(l, i1, w, h);
        let mut pos: Option<(i32, i32)> = None;
        let mut count = 0;
        for k in 0..(w * h) {
            let x = (l + k % w) << 2;
            let z = (i1 + k / w) << 2;
            let id = biome::get_or_ocean(ints[k as usize]).id as i32;
            if SPAWN.contains(&id) && (pos.is_none() || rand.next_int_n(count + 1) == 0) {
                pos = Some((x, z));
                count += 1;
            }
        }
        let (mut x, mut z) = pos.unwrap_or((0, 0));
        let mut tries = 0;
        while !top_is_grass(x, z) {
            x += rand.next_int_n(64) - rand.next_int_n(64);
            z += rand.next_int_n(64) - rand.next_int_n(64);
            tries += 1;
            if tries == 1000 {
                break;
            }
        }
        (x, z)
    }
}
