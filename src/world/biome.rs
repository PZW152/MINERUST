//! Port of `BiomeGenBase` and its subclasses: the biome table, mutations and
//! the per-column surface builder (`genTerrainBlocks`).

use crate::block;
use crate::java::JavaRandom;
use crate::util::math;
use crate::world::gen::noise::NoiseGeneratorPerlin;
use std::sync::OnceLock;

/// Equivalent of `getBiomeClass()`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    Ocean,
    Plains,
    Desert,
    Hills,
    Forest,
    Taiga,
    Swamp,
    River,
    Hell,
    End,
    Snow,
    Mushroom,
    Beach,
    Jungle,
    StoneBeach,
    Savanna,
    Mesa,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TempCategory {
    Ocean,
    Cold,
    Medium,
    Warm,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Terrain {
    Default,
    /// 0 normal, 1 more trees, 2 mutated
    Hills(u8),
    /// 0 normal, 1 mega, 2 mega spruce
    Taiga(u8),
    Swamp,
    SavannaMutated,
    Mesa {
        bryce: bool,
        forest: bool,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GrassKind {
    Default,
    Swamp,
    Roofed,
    Mesa,
}

#[derive(Clone, Debug)]
pub struct Biome {
    pub id: u8,
    pub name: String,
    pub class: Class,
    pub root_height: f32,
    pub height_variation: f32,
    pub temperature: f32,
    pub rainfall: f32,
    pub top_block: u8,
    pub top_meta: u8,
    pub filler_block: u8,
    pub enable_snow: bool,
    pub enable_rain: bool,
    pub terrain: Terrain,
    pub temp_category: TempCategory,
    pub water_color: u32,
    pub grass_kind: GrassKind,
    /// Temperature used for the grass/foliage colour lookup (base biome's for `BiomeGenMutated`).
    pub color_temperature: f32,
    /// `BiomeGenMutated.getBiomeGrassColor` passes `y` in place of `z` to its base biome.
    pub color_zy_quirk: bool,
}

const H_DEFAULT: (f32, f32) = (0.1, 0.2);
const H_SHALLOW_WATERS: (f32, f32) = (-0.5, 0.0);
const H_OCEANS: (f32, f32) = (-1.0, 0.1);
const H_DEEP_OCEANS: (f32, f32) = (-1.8, 0.1);
const H_LOW_PLAINS: (f32, f32) = (0.125, 0.05);
const H_MID_PLAINS: (f32, f32) = (0.2, 0.2);
const H_LOW_HILLS: (f32, f32) = (0.45, 0.3);
const H_HIGH_PLATEAUS: (f32, f32) = (1.5, 0.025);
const H_MID_HILLS: (f32, f32) = (1.0, 0.5);
const H_SHORES: (f32, f32) = (0.0, 0.025);
const H_ROCKY_WATERS: (f32, f32) = (0.1, 0.8);
const H_LOW_ISLANDS: (f32, f32) = (0.2, 0.3);
const H_PARTIALLY_SUBMERGED: (f32, f32) = (-0.2, 0.1);

impl Biome {
    fn new(id: u8, name: &str, class: Class) -> Biome {
        Biome {
            id,
            name: name.to_string(),
            class,
            root_height: H_DEFAULT.0,
            height_variation: H_DEFAULT.1,
            temperature: 0.5,
            rainfall: 0.5,
            top_block: block::GRASS,
            top_meta: 0,
            filler_block: block::DIRT,
            enable_snow: false,
            enable_rain: true,
            terrain: Terrain::Default,
            temp_category: TempCategory::Medium,
            water_color: 0xFFFFFF,
            grass_kind: GrassKind::Default,
            color_temperature: 0.5,
            color_zy_quirk: false,
        }
    }
    fn h(mut self, h: (f32, f32)) -> Self {
        self.root_height = h.0;
        self.height_variation = h.1;
        self
    }
    fn tr(mut self, t: f32, r: f32) -> Self {
        self.temperature = t;
        self.rainfall = r;
        self
    }
    fn snow(mut self) -> Self {
        self.enable_snow = true;
        self
    }
    fn no_rain(mut self) -> Self {
        self.enable_rain = false;
        self
    }
    fn blocks(mut self, top: u8, meta: u8, filler: u8) -> Self {
        self.top_block = top;
        self.top_meta = meta;
        self.filler_block = filler;
        self
    }
    fn terrain(mut self, t: Terrain) -> Self {
        self.terrain = t;
        self
    }
    fn grass(mut self, g: GrassKind) -> Self {
        self.grass_kind = g;
        if g == GrassKind::Swamp {
            self.water_color = 14745518;
        }
        self
    }
    fn finish(mut self) -> Self {
        self.temp_category = if self.class == Class::Ocean {
            TempCategory::Ocean
        } else if (self.temperature as f64) < 0.2 {
            TempCategory::Cold
        } else if (self.temperature as f64) < 1.0 {
            TempCategory::Medium
        } else {
            TempCategory::Warm
        };
        self.color_temperature = self.temperature;
        self
    }

    /// `getEnableSnow() || canSpawnLightningBolt()`
    pub fn can_rain_or_snow(&self) -> bool {
        self.enable_snow || self.enable_rain
    }

    pub fn is_mesa(&self) -> bool {
        self.class == Class::Mesa
    }

    /// `BiomeGenBase.getFloatTemperature`
    pub fn float_temperature(&self, noise: &BiomeNoise, x: i32, y: i32, z: i32) -> f32 {
        float_temperature(self.temperature, noise, x, y, z)
    }

    /// `getBiomeGrassColor(x, y, z)`; `grass_map` is `colormap/grass.png` as 256x256 RGB.
    pub fn grass_color(
        &self,
        noise: &BiomeNoise,
        grass_map: &[u32],
        x: i32,
        y: i32,
        z: i32,
    ) -> u32 {
        let z = if self.color_zy_quirk { y } else { z };
        match self.grass_kind {
            GrassKind::Mesa => 9470285,
            GrassKind::Swamp => {
                let d = noise.plant.get_value(x as f64 * 0.0225, z as f64 * 0.0225);
                if d < -0.1 {
                    5011004
                } else {
                    6975545
                }
            }
            GrassKind::Default | GrassKind::Roofed => {
                let t = math::clamp_float(
                    float_temperature(self.color_temperature, noise, x, y, z),
                    0.0,
                    1.0,
                ) as f64;
                let mut r = math::clamp_float(self.rainfall, 0.0, 1.0) as f64;
                r *= t;
                let i = ((1.0 - t) * 255.0) as i32;
                let j = ((1.0 - r) * 255.0) as i32;
                let c = grass_map
                    .get(((j << 8) | i) as usize)
                    .copied()
                    .unwrap_or(0x7FB238);
                if self.grass_kind == GrassKind::Roofed {
                    ((c & 16711422) + 2634762) >> 1
                } else {
                    c
                }
            }
        }
    }
}

impl Biome {
    /// `getBiomeFoliageColor(x, y, z)`; `foliage_map` is `colormap/foliage.png`.
    pub fn foliage_color(
        &self,
        noise: &BiomeNoise,
        foliage_map: &[u32],
        x: i32,
        y: i32,
        z: i32,
    ) -> u32 {
        let z = if self.color_zy_quirk { y } else { z };
        match self.grass_kind {
            GrassKind::Mesa => 10387789,
            GrassKind::Swamp => 6975545,
            GrassKind::Default | GrassKind::Roofed => {
                let t = math::clamp_float(
                    float_temperature(self.color_temperature, noise, x, y, z),
                    0.0,
                    1.0,
                ) as f64;
                let mut r = math::clamp_float(self.rainfall, 0.0, 1.0) as f64;
                r *= t;
                let i = ((1.0 - t) * 255.0) as i32;
                let j = ((1.0 - r) * 255.0) as i32;
                foliage_map
                    .get(((j << 8) | i) as usize)
                    .copied()
                    .unwrap_or(0x48B518)
            }
        }
    }
}

fn float_temperature(temperature: f32, noise: &BiomeNoise, x: i32, y: i32, z: i32) -> f32 {
    if y > 64 {
        let n = noise
            .temperature
            .get_value(x as f64 * 1.0 / 8.0, z as f64 * 1.0 / 8.0) as f32
            * 4.0f32;
        temperature - (n + y as f32 - 64.0f32) * 0.05f32 / 30.0f32
    } else {
        temperature
    }
}

fn generic_mutation(base: &Biome) -> Biome {
    let mut b = base.clone();
    b.id = base.id + 128;
    b.name = format!("{} M", base.name);
    b.root_height = base.root_height + 0.1f32;
    b.height_variation = base.height_variation + 0.2f32;
    b.color_temperature = base.color_temperature;
    b.color_zy_quirk = true;
    b
}

fn build_table() -> Vec<Option<Biome>> {
    use Class as C;
    let mesa_t = |forest| Terrain::Mesa {
        bryce: false,
        forest,
    };
    let mesa = |id, name: &str, forest| {
        Biome::new(id, name, C::Mesa)
            .no_rain()
            .tr(2.0, 0.0)
            .blocks(block::SAND, 1, block::STAINED_HARDENED_CLAY)
            .terrain(mesa_t(forest))
            .grass(GrassKind::Mesa)
    };
    let plains = |id, name: &str| Biome::new(id, name, C::Plains).tr(0.8, 0.4).h(H_LOW_PLAINS);
    let desert = |id, name: &str| {
        Biome::new(id, name, C::Desert)
            .blocks(block::SAND, 0, block::SAND)
            .no_rain()
            .tr(2.0, 0.0)
    };
    let forest = |id, name: &str, ty: u8| {
        let b = Biome::new(id, name, C::Forest).tr(0.7, 0.8);
        match ty {
            2 => b.tr(0.6, 0.6),
            3 => b.grass(GrassKind::Roofed),
            _ => b,
        }
    };
    let taiga = |id, name: &str, ty: u8| Biome::new(id, name, C::Taiga).terrain(Terrain::Taiga(ty));
    let hills = |id, name: &str, mode: u8| {
        Biome::new(id, name, C::Hills)
            .terrain(Terrain::Hills(mode))
            .tr(0.2, 0.3)
    };
    let jungle = |id, name: &str| Biome::new(id, name, C::Jungle);
    let beach = |id, name: &str| Biome::new(id, name, C::Beach).blocks(block::SAND, 0, block::SAND);
    let mushroom = |id, name: &str| {
        Biome::new(id, name, C::Mushroom)
            .blocks(block::MYCELIUM, 0, block::DIRT)
            .tr(0.9, 1.0)
    };
    let savanna = |id, name: &str| Biome::new(id, name, C::Savanna).no_rain();

    let attenuated = (H_MID_HILLS.0 * 0.8f32, H_MID_HILLS.1 * 0.6f32);

    let list: Vec<Biome> = vec![
        Biome::new(0, "Ocean", C::Ocean).h(H_OCEANS),
        plains(1, "Plains"),
        desert(2, "Desert").h(H_LOW_PLAINS),
        hills(3, "Extreme Hills", 0).h(H_MID_HILLS),
        forest(4, "Forest", 0),
        taiga(5, "Taiga", 0).tr(0.25, 0.8).h(H_MID_PLAINS),
        Biome::new(6, "Swampland", C::Swamp)
            .h(H_PARTIALLY_SUBMERGED)
            .tr(0.8, 0.9)
            .terrain(Terrain::Swamp)
            .grass(GrassKind::Swamp),
        Biome::new(7, "River", C::River).h(H_SHALLOW_WATERS),
        Biome::new(8, "Hell", C::Hell).no_rain().tr(2.0, 0.0),
        Biome::new(9, "Sky", C::End).no_rain(),
        Biome::new(10, "FrozenOcean", C::Ocean)
            .snow()
            .h(H_OCEANS)
            .tr(0.0, 0.5),
        Biome::new(11, "FrozenRiver", C::River)
            .snow()
            .h(H_SHALLOW_WATERS)
            .tr(0.0, 0.5),
        Biome::new(12, "Ice Plains", C::Snow)
            .snow()
            .tr(0.0, 0.5)
            .h(H_LOW_PLAINS),
        Biome::new(13, "Ice Mountains", C::Snow)
            .snow()
            .h(H_LOW_HILLS)
            .tr(0.0, 0.5),
        mushroom(14, "MushroomIsland").h(H_LOW_ISLANDS),
        mushroom(15, "MushroomIslandShore").h(H_SHORES),
        beach(16, "Beach").tr(0.8, 0.4).h(H_SHORES),
        desert(17, "DesertHills").h(H_LOW_HILLS),
        forest(18, "ForestHills", 0).h(H_LOW_HILLS),
        taiga(19, "TaigaHills", 0).tr(0.25, 0.8).h(H_LOW_HILLS),
        hills(20, "Extreme Hills Edge", 1).h(attenuated),
        jungle(21, "Jungle").tr(0.95, 0.9),
        jungle(22, "JungleHills").tr(0.95, 0.9).h(H_LOW_HILLS),
        jungle(23, "JungleEdge").tr(0.95, 0.8),
        Biome::new(24, "Deep Ocean", C::Ocean).h(H_DEEP_OCEANS),
        Biome::new(25, "Stone Beach", C::StoneBeach)
            .blocks(block::STONE, 0, block::STONE)
            .tr(0.2, 0.3)
            .h(H_ROCKY_WATERS),
        beach(26, "Cold Beach").tr(0.05, 0.3).h(H_SHORES).snow(),
        forest(27, "Birch Forest", 2),
        forest(28, "Birch Forest Hills", 2).h(H_LOW_HILLS),
        forest(29, "Roofed Forest", 3),
        taiga(30, "Cold Taiga", 0)
            .snow()
            .tr(-0.5, 0.4)
            .h(H_MID_PLAINS),
        taiga(31, "Cold Taiga Hills", 0)
            .snow()
            .tr(-0.5, 0.4)
            .h(H_LOW_HILLS),
        taiga(32, "Mega Taiga", 1).tr(0.3, 0.8).h(H_MID_PLAINS),
        taiga(33, "Mega Taiga Hills", 1).tr(0.3, 0.8).h(H_LOW_HILLS),
        hills(34, "Extreme Hills+", 1).h(H_MID_HILLS),
        savanna(35, "Savanna").tr(1.2, 0.0).h(H_LOW_PLAINS),
        savanna(36, "Savanna Plateau")
            .tr(1.0, 0.0)
            .h(H_HIGH_PLATEAUS),
        mesa(37, "Mesa", false),
        mesa(38, "Mesa Plateau F", true).h(H_HIGH_PLATEAUS),
        mesa(39, "Mesa Plateau", false).h(H_HIGH_PLATEAUS),
    ];

    let mut table: Vec<Option<Biome>> = vec![None; 256];
    for b in list {
        let id = b.id as usize;
        table[id] = Some(b.finish());
    }

    // createMutation() calls, in the order of the static initialiser.
    for id in [
        1usize, 2, 4, 5, 6, 12, 21, 23, 30, 35, 36, 37, 38, 39, 27, 28, 29, 32, 3, 34,
    ] {
        let base = table[id].clone().unwrap();
        let mid = (id + 128) as u8;
        let m = match id {
            1 => plains(mid, "Sunflower Plains").finish(),
            4 => {
                let mut b = forest(mid, "Flower Forest", 1).finish();
                b.root_height = base.root_height;
                b.height_variation = base.height_variation + 0.2f32;
                b
            }
            12 => {
                let mut b = Biome::new(mid, "Ice Plains Spikes", C::Snow)
                    .snow()
                    .tr(0.0, 0.5)
                    .blocks(block::SNOW, 0, block::DIRT)
                    .finish();
                b.root_height = base.root_height + 0.3f32;
                b.height_variation = base.height_variation + 0.4f32;
                b
            }
            35 | 36 => {
                let mut b = generic_mutation(&base);
                b.terrain = Terrain::SavannaMutated;
                b.temperature = (base.temperature + 1.0f32) * 0.5f32;
                b.root_height = base.root_height * 0.5f32 + 0.3f32;
                b.height_variation = base.height_variation * 0.5f32 + 1.2f32;
                b
            }
            37 | 38 | 39 => {
                let is_mesa = id == 37;
                let forest_flag = matches!(base.terrain, Terrain::Mesa { forest: true, .. });
                let mut b = mesa(mid, "", forest_flag).finish();
                b.terrain = Terrain::Mesa {
                    bryce: is_mesa,
                    forest: forest_flag,
                };
                if !is_mesa {
                    b.root_height = H_LOW_HILLS.0;
                    b.height_variation = H_LOW_HILLS.1;
                    b.name = format!("{} M", base.name);
                } else {
                    b.name = format!("{} (Bryce)", base.name);
                }
                b
            }
            32 => {
                let mut b = taiga(mid, "Mega Spruce Taiga", 2).tr(0.3, 0.8).finish();
                b.root_height = base.root_height;
                b.height_variation = base.height_variation;
                b
            }
            3 | 34 => {
                let mut b = hills(mid, "", 2).finish();
                b.name = format!("{} M", base.name);
                b.root_height = base.root_height;
                b.height_variation = base.height_variation;
                b.temperature = base.temperature;
                b.rainfall = base.rainfall;
                b
            }
            _ => generic_mutation(&base),
        };
        table[id + 128] = Some(m);
    }
    // biomeList[megaTaigaHills.biomeID + 128] = biomeList[megaTaiga.biomeID + 128]
    table[161] = table[160].clone();
    table
}

static TABLE: OnceLock<Vec<Option<Biome>>> = OnceLock::new();

fn table() -> &'static Vec<Option<Biome>> {
    TABLE.get_or_init(build_table)
}

/// `BiomeGenBase.getBiome(id)` without the ocean fallback (returns `None` for holes).
pub fn get(id: i32) -> Option<&'static Biome> {
    if !(0..256).contains(&id) {
        return None;
    }
    table()[id as usize].as_ref()
}

/// `BiomeGenBase.getBiome(id)`: falls back to ocean for unknown ids.
pub fn get_or_ocean(id: i32) -> &'static Biome {
    get(id).unwrap_or_else(|| get(0).unwrap())
}

/// The static noise generators of `BiomeGenBase`.
pub struct BiomeNoise {
    pub temperature: NoiseGeneratorPerlin,
    pub plant: NoiseGeneratorPerlin,
}

impl BiomeNoise {
    pub fn new() -> Self {
        BiomeNoise {
            temperature: NoiseGeneratorPerlin::new(&mut JavaRandom::new(1234), 1),
            plant: NoiseGeneratorPerlin::new(&mut JavaRandom::new(2345), 1),
        }
    }
}

struct MesaState {
    bands: [u8; 64],
    offset_noise: NoiseGeneratorPerlin,
    pillar_noise: NoiseGeneratorPerlin,
    pillar_roof_noise: NoiseGeneratorPerlin,
}

impl MesaState {
    fn new(seed: i64) -> Self {
        // generateBands(seed)
        let mut bands = [16u8; 64];
        let mut r = JavaRandom::new(seed);
        let offset_noise = NoiseGeneratorPerlin::new(&mut r, 1);
        let mut i = 0i32;
        while i < 64 {
            i += r.next_int_n(5) + 1;
            if i < 64 {
                bands[i as usize] = 1;
            }
            i += 1;
        }
        let mut stripes =
            |r: &mut JavaRandom, count_bound: i32, count_add: i32, len_add: i32, colour: u8| {
                let n = r.next_int_n(count_bound) + count_add;
                for _ in 0..n {
                    let len = r.next_int_n(3) + len_add;
                    let s = r.next_int_n(64);
                    let mut j = 0;
                    while s + j < 64 && j < len {
                        bands[(s + j) as usize] = colour;
                        j += 1;
                    }
                }
            };
        stripes(&mut r, 4, 2, 1, 4);
        stripes(&mut r, 4, 2, 2, 12);
        stripes(&mut r, 4, 2, 1, 14);
        let n = r.next_int_n(3) + 3;
        let mut pos = 0i32;
        for _ in 0..n {
            pos += r.next_int_n(16) + 4;
            if pos < 64 {
                bands[pos as usize] = 0;
                if pos > 1 && r.next_boolean() {
                    bands[(pos - 1) as usize] = 8;
                }
                if pos < 63 && r.next_boolean() {
                    bands[(pos + 1) as usize] = 8;
                }
            }
        }
        // The pillar noise is seeded from the not-yet-assigned `worldSeed` field (0).
        let mut pr = JavaRandom::new(0);
        let pillar_noise = NoiseGeneratorPerlin::new(&mut pr, 4);
        let pillar_roof_noise = NoiseGeneratorPerlin::new(&mut pr, 1);
        MesaState {
            bands,
            offset_noise,
            pillar_noise,
            pillar_roof_noise,
        }
    }

    fn clay_layer(&self, x: i32, y: i32, _z: i32) -> u8 {
        let v = self
            .offset_noise
            .get_value(x as f64 * 1.0 / 512.0, x as f64 * 1.0 / 512.0)
            * 2.0;
        let o = math::java_round(v) as i32;
        self.bands[(y + o + 64).rem_euclid(64) as usize]
    }
}

/// Per-world state used by `genTerrainBlocks`.
pub struct TerrainState {
    pub noise: BiomeNoise,
    mesa: MesaState,
}

impl TerrainState {
    pub fn new(seed: i64) -> Self {
        TerrainState {
            noise: BiomeNoise::new(),
            mesa: MesaState::new(seed),
        }
    }
}

impl Biome {
    /// `genTerrainBlocks(world, rand, blocks, meta, x, z, noise)`; returns the resulting `topBlock` field.
    pub fn gen_terrain_blocks(
        &self,
        st: &TerrainState,
        rand: &mut JavaRandom,
        blocks: &mut [u8],
        meta: &mut [u8],
        x: i32,
        z: i32,
        noise: f64,
    ) -> u8 {
        let (mut top, mut top_meta, mut filler) =
            (self.top_block, self.top_meta, self.filler_block);
        match self.terrain {
            Terrain::Default => {}
            Terrain::Hills(mode) => {
                top = block::GRASS;
                top_meta = 0;
                filler = block::DIRT;
                if (noise < -1.0 || noise > 2.0) && mode == 2 {
                    top = block::GRAVEL;
                    filler = block::GRAVEL;
                } else if noise > 1.0 && mode != 1 {
                    top = block::STONE;
                    filler = block::STONE;
                }
            }
            Terrain::Taiga(ty) => {
                if ty == 1 || ty == 2 {
                    top = block::GRASS;
                    top_meta = 0;
                    filler = block::DIRT;
                    if noise > 1.75 {
                        top = block::DIRT;
                        top_meta = 1;
                    } else if noise > -0.95 {
                        top = block::DIRT;
                        top_meta = 2;
                    }
                }
            }
            Terrain::Swamp => {
                // Vanilla quirk: the generator stores air as `null`, and the swamp
                // scan stops at the first entry that is `null || material != air`.
                // That is always y = 255, so the "y == 62 -> water + lily pad" branch
                // never runs in 1.7.10. Verified against the reference: no-op.
            }
            Terrain::SavannaMutated => {
                top = block::GRASS;
                top_meta = 0;
                filler = block::DIRT;
                if noise > 1.75 {
                    top = block::STONE;
                    filler = block::STONE;
                } else if noise > -0.5 {
                    top = block::DIRT;
                    top_meta = 1;
                }
            }
            Terrain::Mesa { bryce, forest } => {
                self.gen_mesa(st, rand, blocks, meta, x, z, noise, bryce, forest);
                return self.top_block;
            }
        }
        self.gen_biome_terrain(st, rand, blocks, meta, x, z, noise, top, top_meta, filler);
        // Value left in the (mutable) `topBlock` field, which cave carving reads back later.
        top
    }

    #[allow(clippy::too_many_arguments)]
    fn gen_biome_terrain(
        &self,
        st: &TerrainState,
        rand: &mut JavaRandom,
        blocks: &mut [u8],
        meta: &mut [u8],
        x: i32,
        z: i32,
        noise: f64,
        top_block: u8,
        top_block_meta: u8,
        filler_block: u8,
    ) {
        let mut top = top_block;
        let mut top_meta = top_block_meta;
        let mut filler = filler_block;
        let mut k: i32 = -1;
        let depth = (noise / 3.0 + 3.0 + rand.next_double() * 0.25) as i32;
        let lx = (x & 15) as usize;
        let lz = (z & 15) as usize;
        for y in (0..256i32).rev() {
            let idx = (lz * 16 + lx) * 256 + y as usize;
            if y <= rand.next_int_n(5) {
                blocks[idx] = block::BEDROCK;
                continue;
            }
            let b = blocks[idx];
            if b == block::AIR {
                k = -1;
            } else if b == block::STONE {
                if k == -1 {
                    if depth <= 0 {
                        top = block::AIR;
                        top_meta = 0;
                        filler = block::STONE;
                    } else if (59..=64).contains(&y) {
                        top = top_block;
                        top_meta = top_block_meta;
                        filler = filler_block;
                    }
                    if y < 63 && top == block::AIR {
                        if self.float_temperature(&st.noise, x, y, z) < 0.15f32 {
                            top = block::ICE;
                            top_meta = 0;
                        } else {
                            top = block::WATER;
                            top_meta = 0;
                        }
                    }
                    k = depth;
                    if y >= 62 {
                        blocks[idx] = top;
                        meta[idx] = top_meta;
                    } else if y < 56 - depth {
                        top = block::AIR;
                        filler = block::STONE;
                        blocks[idx] = block::GRAVEL;
                    } else {
                        blocks[idx] = filler;
                    }
                } else if k > 0 {
                    k -= 1;
                    blocks[idx] = filler;
                    if k == 0 && filler == block::SAND {
                        k = rand.next_int_n(4) + (y - 63).max(0);
                        filler = block::SANDSTONE;
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn gen_mesa(
        &self,
        st: &TerrainState,
        rand: &mut JavaRandom,
        blocks: &mut [u8],
        meta: &mut [u8],
        x: i32,
        z: i32,
        noise: f64,
        bryce: bool,
        has_forest: bool,
    ) {
        let ms = &st.mesa;
        let mut pillar = 0.0f64;
        if bryce {
            let a = (x & -16) + (z & 15);
            let b = (z & -16) + (x & 15);
            let m = noise
                .abs()
                .min(ms.pillar_noise.get_value(a as f64 * 0.25, b as f64 * 0.25));
            if m > 0.0 {
                let s = 0.001953125f64;
                let r = ms
                    .pillar_roof_noise
                    .get_value(a as f64 * s, b as f64 * s)
                    .abs();
                pillar = m * m * 2.5;
                let cap = (r * 50.0).ceil() + 14.0;
                if pillar > cap {
                    pillar = cap;
                }
                pillar += 64.0;
            }
        }
        let lx = (x & 15) as usize;
        let lz = (z & 15) as usize;
        let mut top = block::STAINED_HARDENED_CLAY;
        let mut filler = self.filler_block;
        let depth = (noise / 3.0 + 3.0 + rand.next_double() * 0.25) as i32;
        let cos_pos = (noise / 3.0 * std::f64::consts::PI).cos() > 0.0;
        let mut k: i32 = -1;
        let mut flag = false;
        for y in (0..256i32).rev() {
            let idx = (lz * 16 + lx) * 256 + y as usize;
            if blocks[idx] == block::AIR && y < pillar as i32 {
                blocks[idx] = block::STONE;
            }
            if y <= rand.next_int_n(5) {
                blocks[idx] = block::BEDROCK;
                continue;
            }
            let b = blocks[idx];
            if b == block::AIR {
                k = -1;
            } else if b == block::STONE {
                if k == -1 {
                    flag = false;
                    if depth <= 0 {
                        top = block::AIR;
                        filler = block::STONE;
                    } else if (59..=64).contains(&y) {
                        top = block::STAINED_HARDENED_CLAY;
                        filler = self.filler_block;
                    }
                    if y < 63 && top == block::AIR {
                        top = block::WATER;
                    }
                    k = depth + (y - 63).max(0);
                    if y >= 62 {
                        if has_forest && y > 86 + depth * 2 {
                            if cos_pos {
                                blocks[idx] = block::DIRT;
                                meta[idx] = 1;
                            } else {
                                blocks[idx] = block::GRASS;
                            }
                        } else if y > 66 + depth {
                            let mut c = 16u8;
                            if !(64..=127).contains(&y) {
                                c = 1;
                            } else if !cos_pos {
                                c = ms.clay_layer(x, y, z);
                            }
                            if c < 16 {
                                blocks[idx] = block::STAINED_HARDENED_CLAY;
                                meta[idx] = c;
                            } else {
                                blocks[idx] = block::HARDENED_CLAY;
                            }
                        } else {
                            blocks[idx] = self.top_block;
                            meta[idx] = self.top_meta;
                            flag = true;
                        }
                    } else {
                        blocks[idx] = filler;
                        if filler == block::STAINED_HARDENED_CLAY {
                            meta[idx] = 1;
                        }
                    }
                } else if k > 0 {
                    k -= 1;
                    if flag {
                        blocks[idx] = block::STAINED_HARDENED_CLAY;
                        meta[idx] = 1;
                    } else {
                        let c = ms.clay_layer(x, y, z);
                        if c < 16 {
                            blocks[idx] = block::STAINED_HARDENED_CLAY;
                            meta[idx] = c;
                        } else {
                            blocks[idx] = block::HARDENED_CLAY;
                        }
                    }
                }
            }
        }
    }
}
