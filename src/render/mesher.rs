//! Chunk tessellation following `RenderBlocks`. Light comes from the world
//! (the vanilla light engine in `world::server`).

use super::atlas::Atlas;
use crate::block::{self, Material, Shape};
use crate::world::biome::{self, BiomeNoise};
use crate::world::ChunkData;
use std::sync::Arc;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub color: [u8; 4],
    /// `[block, sky, 0, 0]` in lightmap units (0..240).
    pub light: [u8; 4],
}

/// One 16x16x16 render chunk (`WorldRenderer`): pass 0 and pass 1 quads,
/// positions relative to the chunk column's origin.
pub struct SectionMesh {
    /// Section index (y / 16).
    pub sy: u8,
    pub opaque: Vec<Vertex>,
    pub translucent: Vec<Vertex>,
}

/// The sections of one chunk column that were built again.
pub struct ChunkMesh {
    pub cx: i32,
    pub cz: i32,
    pub sections: Vec<SectionMesh>,
}

/// One chunk of a mesh job's 3x3 neighbourhood.
#[derive(Clone)]
pub struct MeshChunk {
    pub data: Arc<ChunkData>,
    /// Existing 16-high sections (light of missing ones is implied).
    pub sections: u16,
    /// `heightMap[z << 4 | x]`
    pub height: Box<[i32; 256]>,
    /// `TileEntityFlowerPot` contents: local x, y, local z, the plant's
    /// block and data (block 0 for an empty pot).
    pub pots: Vec<([u8; 3], u8, u8)>,
}

/// `BlockFlowerPot.createNewTileEntity`: the plant a pot without a tile
/// entity gets from its metadata (as the witch hut's red mushroom).
pub fn pot_from_meta(meta: i32) -> Option<(u8, i32)> {
    Some(match meta {
        1 => (38, 0),
        2 => (37, 0),
        3..=6 => (6, meta - 3),
        7 => (40, 0),
        8 => (39, 0),
        9 => (81, 0),
        10 => (32, 0),
        11 => (31, 2),
        12 => (6, 4),
        13 => (6, 5),
        _ => return None,
    })
}

#[path = "rb.rs"]
pub(crate) mod rb;

const W: usize = 48;

struct Region<'a> {
    blocks: Vec<u8>,
    meta: Vec<u8>,
    sky: Vec<u8>,
    blk: Vec<u8>,
    chunks: &'a [MeshChunk; 9],
    ox: i32,
    oz: i32,
    top: usize,
}

#[inline]
fn idx(x: i32, y: i32, z: i32) -> usize {
    ((x as usize * W + z as usize) << 8) | y as usize
}

impl<'a> Region<'a> {
    /// The 3x3 chunks between heights `lo` and `hi` (the rest reads as air).
    fn new(chunks: &'a [MeshChunk; 9], lo: usize, hi: usize) -> Self {
        let (lo, hi) = (lo.min(256), hi.min(256));
        let n = W * W * 256;
        let mut blocks = vec![0u8; n];
        let mut meta = vec![0u8; n];
        let mut sky = vec![0u8; n];
        let mut blk = vec![0u8; n];
        let mut top = 0usize;
        for (ci, mc) in chunks.iter().enumerate() {
            let c = &mc.data;
            let (dx, dz) = (ci % 3, ci / 3);
            if mc.sections != 0 {
                top = top.max(((16 - mc.sections.leading_zeros() as usize) << 4).min(256));
            }
            for x in 0..16 {
                for z in 0..16 {
                    let s = (x * 16 + z) << 8;
                    let d = ((dx * 16 + x) * W + dz * 16 + z) << 8;
                    blocks[d + lo..d + hi].copy_from_slice(&c.blocks[s + lo..s + hi]);
                    meta[d + lo..d + hi].copy_from_slice(&c.meta[s + lo..s + hi]);
                    blk[d + lo..d + hi].copy_from_slice(&c.blk[s + lo..s + hi]);
                    sky[d + lo..d + hi].copy_from_slice(&c.sky[s + lo..s + hi]);
                    let h = mc.height[(z << 4) | x].clamp(0, 256) as usize;
                    for sec in (lo >> 4)..((hi + 15) >> 4) {
                        if mc.sections & (1 << sec) == 0 {
                            // Chunk.getSavedLightValue for a missing section.
                            for y in (sec << 4).max(lo)..((sec + 1) << 4).min(hi) {
                                sky[d + y] = if y >= h { 15 } else { 0 };
                                blk[d + y] = 0;
                            }
                        }
                    }
                }
            }
        }
        let c = &chunks[4].data;
        Region {
            blocks,
            meta,
            sky,
            blk,
            chunks,
            ox: (c.cx - 1) * 16,
            oz: (c.cz - 1) * 16,
            top: (top + 1).min(256),
        }
    }

    #[inline]
    fn block(&self, x: i32, y: i32, z: i32) -> u8 {
        if !(0..256).contains(&y) {
            0
        } else {
            self.blocks[idx(x, y, z)]
        }
    }

    #[inline]
    fn meta(&self, x: i32, y: i32, z: i32) -> i32 {
        if !(0..256).contains(&y) {
            0
        } else {
            self.meta[idx(x, y, z)] as i32
        }
    }

    #[inline]
    fn material(&self, x: i32, y: i32, z: i32) -> Material {
        block::material(self.block(x, y, z))
    }

    /// `World.getSkyBlockTypeBrightness`
    #[inline]
    fn light(&self, sky: bool, x: i32, y: i32, z: i32) -> i32 {
        if y >= 256 {
            return if sky { 15 } else { 0 };
        }
        let y = y.max(0);
        let arr = if sky { &self.sky } else { &self.blk };
        let i = idx(x, y, z);
        if block::info(self.blocks[i]).use_neighbor_brightness {
            let up = idx(x, (y + 1).min(255), z);
            let mut l = arr[up];
            l = l.max(arr[idx(x + 1, y, z)]);
            l = l.max(arr[idx(x - 1, y, z)]);
            l = l.max(arr[idx(x, y, z + 1)]);
            l = l.max(arr[idx(x, y, z - 1)]);
            l as i32
        } else {
            arr[i] as i32
        }
    }

    /// `Block.getMixedBrightnessForBlock`: `sky << 20 | block << 4`.
    #[inline]
    fn bright(&self, x: i32, y: i32, z: i32) -> i32 {
        let lv = block::info(self.block(x, y, z)).light_value as i32;
        let s = self.light(true, x, y, z);
        let b = self.light(false, x, y, z).max(lv);
        (s << 20) | (b << 4)
    }

    /// `BlockLiquid.getMixedBrightnessForBlock`
    fn liquid_bright(&self, x: i32, y: i32, z: i32) -> i32 {
        let a = self.bright(x, y, z);
        let b = self.bright(x, y + 1, z);
        let (j, k) = (a & 255, b & 255);
        let (l, m) = (a >> 16 & 255, b >> 16 & 255);
        (if j > k { j } else { k }) | (if l > m { l } else { m }) << 16
    }

    #[inline]
    fn ao(&self, x: i32, y: i32, z: i32) -> f32 {
        if block::info(self.block(x, y, z)).normal_cube() {
            0.2
        } else {
            1.0
        }
    }

    #[inline]
    fn can_block_grass(&self, x: i32, y: i32, z: i32) -> bool {
        block::info(self.block(x, y, z)).can_block_grass
    }

    fn biome(&self, x: i32, z: i32) -> &'static biome::Biome {
        let c = &self.chunks[(z as usize / 16) * 3 + x as usize / 16].data;
        biome::get_or_ocean(c.biomes[(z as usize & 15) * 16 + (x as usize & 15)] as i32)
    }

    /// `BlockLiquid.getEffectiveFlowDecay`
    fn flow_decay(&self, m: Material, x: i32, y: i32, z: i32) -> i32 {
        if self.material(x, y, z) != m {
            return -1;
        }
        let d = self.meta(x, y, z);
        if d >= 8 {
            0
        } else {
            d
        }
    }

    /// `BlockLiquid.isBlockSolid`
    fn liquid_solid(&self, m: Material, x: i32, y: i32, z: i32, side: usize) -> bool {
        let mat = self.material(x, y, z);
        if mat == m {
            false
        } else if side == 1 {
            true
        } else if mat == Material::Ice {
            false
        } else {
            mat.is_solid()
        }
    }

    /// `BlockLiquid.getFlowDirection`: angle, or `None` for still liquid.
    fn flow_direction(&self, m: Material, x: i32, y: i32, z: i32) -> Option<f32> {
        let (mut vx, mut vy, mut vz) = (0.0f64, 0.0f64, 0.0f64);
        let own = self.flow_decay(m, x, y, z);
        for (dx, dz) in [(-1, 0), (0, -1), (1, 0), (0, 1)] {
            let (nx, nz) = (x + dx, z + dz);
            let mut d = self.flow_decay(m, nx, y, nz);
            if d < 0 {
                if !self.material(nx, y, nz).blocks_movement() {
                    d = self.flow_decay(m, nx, y - 1, nz);
                    if d >= 0 {
                        let k = (d - (own - 8)) as f64;
                        vx += dx as f64 * k;
                        vz += dz as f64 * k;
                    }
                }
            } else {
                let k = (d - own) as f64;
                vx += dx as f64 * k;
                vz += dz as f64 * k;
            }
        }
        let norm = |x: f64, y: f64, z: f64| {
            let l = (x * x + y * y + z * z).sqrt();
            if l < 1.0e-4 {
                (0.0, 0.0, 0.0)
            } else {
                (x / l, y / l, z / l)
            }
        };
        if self.meta(x, y, z) >= 8 {
            let solid = self.liquid_solid(m, x, y, z - 1, 2)
                || self.liquid_solid(m, x, y, z + 1, 3)
                || self.liquid_solid(m, x - 1, y, z, 4)
                || self.liquid_solid(m, x + 1, y, z, 5)
                || self.liquid_solid(m, x, y + 1, z - 1, 2)
                || self.liquid_solid(m, x, y + 1, z + 1, 3)
                || self.liquid_solid(m, x - 1, y + 1, z, 4)
                || self.liquid_solid(m, x + 1, y + 1, z, 5);
            if solid {
                let n = norm(vx, vy, vz);
                vx = n.0;
                vy = n.1 - 6.0;
                vz = n.2;
            }
        }
        let n = norm(vx, vy, vz);
        if n.0 == 0.0 && n.2 == 0.0 {
            None
        } else {
            Some((n.2.atan2(n.0) - std::f64::consts::FRAC_PI_2) as f32)
        }
    }

    /// `BlockLiquid.getLiquidHeight` (corner height).
    fn liquid_height(&self, x: i32, y: i32, z: i32, m: Material) -> f32 {
        let mut n = 0i32;
        let mut f = 0.0f32;
        for i in 0..4 {
            let bx = x - (i & 1);
            let bz = z - (i >> 1 & 1);
            if self.material(bx, y + 1, bz) == m {
                return 1.0;
            }
            let mat = self.material(bx, y, bz);
            if mat == m {
                let meta = self.meta(bx, y, bz);
                let pct = (if meta >= 8 { 0 } else { meta } + 1) as f32 / 9.0f32;
                if meta >= 8 || meta == 0 {
                    f += pct * 10.0f32;
                    n += 10;
                }
                f += pct;
                n += 1;
            } else if !mat.is_solid() {
                f += 1.0;
                n += 1;
            }
        }
        1.0f32 - f / n as f32
    }
}

fn ao_brightness(mut a: i32, mut b: i32, mut c: i32, d: i32) -> i32 {
    if a == 0 {
        a = d;
    }
    if b == 0 {
        b = d;
    }
    if c == 0 {
        c = d;
    }
    (a + b + c + d) >> 2 & 16711935
}

const NORMALS: [[i32; 3]; 6] = [
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, -1],
    [0, 0, 1],
    [-1, 0, 0],
    [1, 0, 0],
];
const SHADE: [f32; 6] = [0.5, 1.0, 0.8, 0.8, 0.6, 0.6];
/// Tangent axes per face: (fallback side axis, other axis).
const TANGENTS: [(usize, usize); 6] = [(0, 2), (0, 2), (0, 1), (0, 1), (2, 1), (2, 1)];
/// Vertex positions (cube space) and tile UVs in `RenderBlocks.renderFace*` order.
const FACE_VERTS: [[([i32; 3], [f32; 2]); 4]; 6] = [
    [
        ([0, 0, 1], [0.0, 1.0]),
        ([0, 0, 0], [0.0, 0.0]),
        ([1, 0, 0], [1.0, 0.0]),
        ([1, 0, 1], [1.0, 1.0]),
    ],
    [
        ([1, 1, 1], [1.0, 1.0]),
        ([1, 1, 0], [1.0, 0.0]),
        ([0, 1, 0], [0.0, 0.0]),
        ([0, 1, 1], [0.0, 1.0]),
    ],
    [
        ([0, 1, 0], [1.0, 0.0]),
        ([1, 1, 0], [0.0, 0.0]),
        ([1, 0, 0], [0.0, 1.0]),
        ([0, 0, 0], [1.0, 1.0]),
    ],
    [
        ([0, 1, 1], [0.0, 0.0]),
        ([0, 0, 1], [0.0, 1.0]),
        ([1, 0, 1], [1.0, 1.0]),
        ([1, 1, 1], [1.0, 0.0]),
    ],
    [
        ([0, 1, 1], [1.0, 0.0]),
        ([0, 1, 0], [0.0, 0.0]),
        ([0, 0, 0], [0.0, 1.0]),
        ([0, 0, 1], [1.0, 1.0]),
    ],
    [
        ([1, 0, 1], [0.0, 1.0]),
        ([1, 0, 0], [1.0, 1.0]),
        ([1, 1, 0], [1.0, 0.0]),
        ([1, 1, 1], [0.0, 0.0]),
    ],
];

#[inline]
fn col(r: f32, g: f32, b: f32) -> [u8; 4] {
    [
        (r * 255.0) as i32 as u8,
        (g * 255.0) as i32 as u8,
        (b * 255.0) as i32 as u8,
        255,
    ]
}

#[inline]
fn light_bytes(b: i32) -> [u8; 4] {
    [(b & 255) as u8, (b >> 16 & 255) as u8, 0, 0]
}

pub fn rgb(c: u32) -> [f32; 3] {
    [
        (c >> 16 & 255) as f32 / 255.0,
        (c >> 8 & 255) as f32 / 255.0,
        (c & 255) as f32 / 255.0,
    ]
}

struct Ctx<'a> {
    r: Region<'a>,
    atlas: &'a Atlas,
    noise: &'a BiomeNoise,
    /// `RenderBlocks.renderAllFaces`
    all_faces: bool,
}

impl Ctx<'_> {
    /// 3x3 biome blend used by grass, leaves and water.
    fn blend(
        &self,
        x: i32,
        y: i32,
        z: i32,
        f: impl Fn(&biome::Biome, i32, i32, i32) -> u32,
    ) -> [f32; 3] {
        let (mut l, mut i1, mut j1) = (0u32, 0u32, 0u32);
        for dz in -1..=1 {
            for dx in -1..=1 {
                let c = f(
                    self.r.biome(x + dx, z + dz),
                    self.r.ox + x + dx,
                    y,
                    self.r.oz + z + dz,
                );
                l += c >> 16 & 255;
                i1 += c >> 8 & 255;
                j1 += c & 255;
            }
        }
        rgb((l / 9 & 255) << 16 | (i1 / 9 & 255) << 8 | (j1 / 9 & 255))
    }

    fn grass_tint(&self, x: i32, y: i32, z: i32) -> [f32; 3] {
        self.blend(x, y, z, |b, wx, wy, wz| {
            b.grass_color(self.noise, &self.atlas.grass_map, wx, wy, wz)
        })
    }

    fn foliage_tint(&self, x: i32, y: i32, z: i32) -> [f32; 3] {
        self.blend(x, y, z, |b, wx, wy, wz| {
            b.foliage_color(self.noise, &self.atlas.foliage_map, wx, wy, wz)
        })
    }

    /// Single-biome grass colour (tall grass, reeds, double plants).
    fn grass_color_at(&self, x: i32, y: i32, z: i32) -> [f32; 3] {
        rgb(self.r.biome(x, z).grass_color(
            self.noise,
            &self.atlas.grass_map,
            self.r.ox + x,
            y,
            self.r.oz + z,
        ))
    }

    fn rect(&self, id: u8, meta: i32, face: usize) -> Option<[f32; 4]> {
        self.atlas.tile(id, meta as u8, face)
    }
}

/// `x * 3129871 ^ z * 116129781L ^ y` scrambled, as used for plant offsets.
fn pos_hash(x: i32, y: i32, z: i32) -> i64 {
    let mut l = (x.wrapping_mul(3129871) as i64) ^ (z as i64).wrapping_mul(116129781) ^ y as i64;
    l = l
        .wrapping_mul(l)
        .wrapping_mul(42317861)
        .wrapping_add(l.wrapping_mul(11));
    l
}

/// Tessellates the sections of the middle chunk named by `mask`
/// (`WorldRenderer.updateRenderer` for each of them).
pub fn build(chunks: &[MeshChunk; 9], atlas: &Atlas, noise: &BiomeNoise, mask: u16) -> ChunkMesh {
    let c = &chunks[4].data;
    let mut sections = Vec::with_capacity(mask.count_ones() as usize);
    if mask == 0 {
        return ChunkMesh {
            cx: c.cx,
            cz: c.cz,
            sections,
        };
    }
    // The blocks a section's faces, light and shapes can reach: one section
    // either side is far more than the single block `ChunkCache` adds.
    let first = mask.trailing_zeros() as usize;
    let last = 15 - mask.leading_zeros() as usize;
    let cx = Ctx {
        r: Region::new(chunks, (first << 4).saturating_sub(16), (last << 4) + 32),
        atlas,
        noise,
        all_faces: false,
    };
    let r = &cx.r;
    for sy in first..=last {
        if mask & (1 << sy) == 0 {
            continue;
        }
        let mut opaque: Vec<Vertex> = Vec::new();
        let mut translucent: Vec<Vertex> = Vec::new();
        // ExtendedBlockStorage that was never created holds only air
        if chunks[4].sections & (1 << sy) != 0 {
            let (y0, y1) = ((sy << 4) as i32, ((sy << 4) + 16).min(r.top) as i32);
            // WorldRenderer.updateRenderer: y, then z, then x; the order the
            // translucent faces are drawn (and blended, depth writes off) in
            for y in y0..y1 {
                for lz in 0..16i32 {
                    for lx in 0..16i32 {
                        let (x, z) = (lx + 16, lz + 16);
                        let id = r.blocks[idx(x, y, z)];
                        if id == 0 {
                            continue;
                        }
                        let info = block::info(id);
                        let meta = r.meta[idx(x, y, z)] as i32;
                        let out = if info.translucent {
                            &mut translucent
                        } else {
                            &mut opaque
                        };
                        mesh_block(&cx, out, id, meta, x, y, z);
                    }
                }
            }
        }
        sections.push(SectionMesh {
            sy: sy as u8,
            opaque,
            translucent,
        });
    }
    ChunkMesh {
        cx: c.cx,
        cz: c.cz,
        sections,
    }
}

/// One block of the region (`RenderBlocks.renderBlockByRenderType`).
fn mesh_block(cx: &Ctx, out: &mut Vec<Vertex>, id: u8, meta: i32, x: i32, y: i32, z: i32) {
    let r = &cx.r;
    let (lx, lz) = (x - 16, z - 16);
    let info = block::info(id);
    match info.shape {
        Shape::Cube => {
            let mut rb = rb::Rb::new(cx, out, id, meta);
            match id {
                // renderPistonBase (16), renderBlockLog (31), renderBlockQuartz (39)
                block::PISTON | block::STICKY_PISTON => rb.render_piston_base(x, y, z, false),
                block::LOG | block::LOG2 | block::HAY_BLOCK => rb.render_block_log(x, y, z),
                block::QUARTZ_BLOCK => rb.render_block_quartz(x, y, z),
                _ => rb.render_standard(x, y, z),
            };
        }
        Shape::Boxes if id == block::PORTAL => {
            // render type 0 with BlockPortal.setBlockBoundsBasedOnState
            let get = |x: i32, y: i32, z: i32| (r.block(x, y, z), r.meta(x, y, z) as u8);
            let axis = crate::shapes::portal_axis(&get, x, y, z, meta as u8);
            let mut rb = rb::Rb::new(cx, out, id, meta);
            rb.set_bounds(crate::shapes::portal_bounds(axis));
            rb.render_standard(x, y, z);
        }
        Shape::Boxes if id == block::CAKE => {
            // render type 0 with BlockCake.setBlockBoundsBasedOnState
            let mut rb = rb::Rb::new(cx, out, id, meta);
            rb.set_bounds(crate::shapes::cake(meta as u8));
            rb.render_standard(x, y, z);
        }
        Shape::Hopper => {
            rb::Rb::new(cx, out, id, meta).render_block_hopper(x, y, z);
        }
        Shape::FenceGate => {
            rb::Rb::new(cx, out, id, meta).render_block_fence_gate(x, y, z);
        }
        Shape::Wall => {
            rb::Rb::new(cx, out, id, meta).render_block_wall(x, y, z);
        }
        Shape::Bed => {
            rb::Rb::new(cx, out, id, meta).render_block_bed(x, y, z);
        }
        Shape::Stem => stem(cx, out, id, meta, x, y, z),
        Shape::Boxes if id == block::PISTON_HEAD => {
            // renderPistonExtension (17)
            rb::Rb::new(cx, out, id, meta).render_piston_extension(x, y, z, true);
        }
        Shape::Liquid => liquid(&cx, out, id, x, y, z),
        Shape::Cross => {
            let tint = match id {
                block::TALLGRASS if meta != 0 => cx.grass_color_at(x, y, z),
                block::REEDS => cx.grass_color_at(x, y, z),
                _ => [1.0; 3],
            };
            let (mut px, mut py, mut pz) = (lx as f64, y as f64, lz as f64);
            let h = pos_hash(r.ox + x, y, r.oz + z);
            if id == block::TALLGRASS {
                px += ((h >> 16 & 15) as f32 / 15.0f32) as f64 * 0.5 - 0.25;
                py += (((h >> 20 & 15) as f32 / 15.0f32) as f64 - 1.0) * 0.2;
                pz += ((h >> 24 & 15) as f32 / 15.0f32) as f64 * 0.5 - 0.25;
            } else if id == block::RED_FLOWER || id == block::YELLOW_FLOWER {
                px += (((h >> 16 & 15) as f32 / 15.0f32) as f64 - 0.5) * 0.3;
                pz += (((h >> 24 & 15) as f32 / 15.0f32) as f64 - 0.5) * 0.3;
            }
            if let Some(t) = cx.rect(id, meta, 0) {
                crossed(
                    out,
                    t,
                    px,
                    py,
                    pz,
                    col(tint[0], tint[1], tint[2]),
                    light_bytes(r.bright(x, y, z)),
                );
            }
        }
        Shape::DoublePlant => double_plant(&cx, out, meta, x, y, z),
        Shape::Vine => vine(&cx, out, meta, x, y, z),
        Shape::LilyPad => lily_pad(&cx, out, x, y, z),
        // renderBlockCactus (13)
        Shape::Cactus => {
            rb::Rb::new(cx, out, id, meta).render_block_cactus(x, y, z);
        }
        // render type 0 with BlockSnow.setBlockBoundsForSnowDepth
        Shape::SnowLayer => {
            let h = (2 * (1 + (meta & 7))) as f32 / 16.0;
            let mut rb = rb::Rb::new(cx, out, id, meta);
            rb.set_bounds([0.0, 0.0, 0.0, 1.0, h, 1.0]);
            rb.render_standard(x, y, z);
        }
        // render type 0 with BlockSlab.setBlockBoundsBasedOnState
        Shape::Slab => {
            let b = if meta & 8 != 0 {
                [0.0, 0.5, 0.0, 1.0, 1.0, 1.0]
            } else {
                [0.0, 0.0, 0.0, 1.0, 0.5, 1.0]
            };
            let mut rb = rb::Rb::new(cx, out, id, meta);
            rb.set_bounds(b);
            rb.render_standard(x, y, z);
        }
        // render type 0 with setBlockBoundsBasedOnState: one box each
        Shape::Boxes
            if matches!(
                id,
                block::FARMLAND
                    | block::ENCHANTING_TABLE
                    | block::DAYLIGHT_DETECTOR
                    | block::STONE_PRESSURE_PLATE
                    | block::WOODEN_PRESSURE_PLATE
                    | block::LIGHT_WEIGHTED_PRESSURE_PLATE
                    | block::HEAVY_WEIGHTED_PRESSURE_PLATE
                    | block::STONE_BUTTON
                    | block::WOODEN_BUTTON
                    | block::CARPET
                    | block::TRAPDOOR
            ) =>
        {
            let list = crate::shapes::static_boxes(id, meta as u8);
            let mut rb = rb::Rb::new(cx, out, id, meta);
            rb.set_bounds(list[0]);
            rb.render_standard(x, y, z);
        }
        // render type 22: drawn by TileEntityChestRenderer, not the chunk
        Shape::Chest | Shape::Sign => {}
        Shape::Torch => {
            rb::Rb::new(cx, out, id, meta).render_block_torch(x, y, z);
        }
        Shape::Stairs | Shape::Fence | Shape::Pane | Shape::Door | Shape::Boxes => {
            shaped(&cx, out, id, meta, x, y, z)
        }
        Shape::Ladder => {
            rb::Rb::new(cx, out, id, meta).render_block_ladder(x, y, z);
        }
        Shape::Rail => {
            rb::Rb::new(cx, out, id, meta).render_block_minecart_track(x, y, z);
        }
        Shape::Crops => {
            rb::Rb::new(cx, out, id, meta).render_block_crops(x, y, z);
        }
        Shape::Wire => wire(&cx, out, meta, x, y, z),
        Shape::Fire => fire(&cx, out, x, y, z),
        Shape::Tripwire => tripwire(&cx, out, meta, x, y, z),
        Shape::BrewingStand => {
            rb::Rb::new(cx, out, id, meta).render_brewing_stand(x, y, z);
        }
        Shape::Anvil => {
            rb::Rb::new(cx, out, id, meta).render_anvil(x, y, z, false);
        }
        Shape::Beacon => {
            rb::Rb::new(cx, out, id, meta).render_block_beacon(x, y, z);
        }
        Shape::Cocoa => {
            rb::Rb::new(cx, out, id, meta).render_block_cocoa(x, y, z);
        }
        Shape::None => {}
    }
}

/// What `TileEntityRendererPiston` draws for one moving block.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum MovingDraw {
    /// `renderBlockAllFaces`
    Block,
    /// `renderPistonExtensionAllFaces(piston_head, .., long)`; `sprite`
    /// overrides the front face (`func_150086_a`) with the base's texture.
    Head {
        long: bool,
        sticky_sprite: Option<bool>,
    },
    /// `renderPistonBaseAllFaces`
    Base,
    /// `RenderFallingBlock`: an anvil drawn where the entity is, lit by
    /// the world there (which keeps its own block).
    Anvil,
}

/// `TileEntityRendererPiston`: a block drawn with every face where it
/// stands, lit by the world around it; the renderer adds the offset.
/// Vertices are relative to the chunk holding `pos`.
pub fn build_single(
    chunks: &[MeshChunk; 9],
    atlas: &Atlas,
    noise: &BiomeNoise,
    pos: [i32; 3],
    id: u8,
    meta: i32,
    draw: MovingDraw,
) -> Vec<Vertex> {
    let y = pos[1].clamp(0, 255) as usize;
    let mut cx = Ctx {
        r: Region::new(chunks, y.saturating_sub(16), y + 17),
        atlas,
        noise,
        all_faces: true,
    };
    let (x, y, z) = (pos[0] - cx.r.ox, pos[1], pos[2] - cx.r.oz);
    let mut out = Vec::new();
    if !(0..256).contains(&y) {
        return out;
    }
    // The world holds piston_extension with the stored metadata here.
    if draw != MovingDraw::Anvil {
        cx.r.blocks[idx(x, y, z)] = block::PISTON_EXTENSION;
        cx.r.meta[idx(x, y, z)] = meta as u8;
    }
    match draw {
        MovingDraw::Block => mesh_block(&cx, &mut out, id, meta, x, y, z),
        MovingDraw::Head {
            long,
            sticky_sprite,
        } => {
            let mut rb = rb::Rb::new(&cx, &mut out, block::PISTON_HEAD, meta);
            if let Some(st) = sticky_sprite {
                rb.head_sprite = Some(rb::Icon::new(atlas.named(if st {
                    "piston_top_sticky"
                } else {
                    "piston_top_normal"
                })));
            }
            rb.render_piston_extension(x, y, z, long);
        }
        MovingDraw::Base => {
            rb::Rb::new(&cx, &mut out, id, meta).render_piston_base(x, y, z, true);
        }
        MovingDraw::Anvil => {
            rb::Rb::new(&cx, &mut out, id, meta).render_anvil(x, y, z, false);
        }
    }
    out
}

/// `renderStandardBlockWithAmbientOcclusion`
fn cube(cx: &Ctx, out: &mut Vec<Vertex>, id: u8, meta: i32, x: i32, y: i32, z: i32) {
    let r = &cx.r;
    let (lx, lz) = (x - 16, z - 16);
    let mut tint: Option<[f32; 3]> = None;
    let grassy = id == block::GRASS || id == block::MYCELIUM;
    let snowed = grassy
        && matches!(
            r.material(x, y + 1, z),
            Material::Snow | Material::CraftedSnow
        );
    for f in 0..6 {
        let n = NORMALS[f];
        let (qx, qy, qz) = (x + n[0], y + n[1], z + n[2]);
        let nid = r.block(qx, qy, qz);
        let visible = match id {
            block::ICE | block::GLASS => nid != id && !block::opaque_cube(nid),
            _ => !block::opaque_cube(nid),
        };
        if !visible && !cx.all_faces {
            continue;
        }
        let rect = if snowed && f >= 2 {
            cx.atlas.rects[cx.atlas.grass_side_snowed as usize]
        } else {
            match cx.rect(id, meta, f) {
                Some(t) => t,
                None => continue,
            }
        };
        let leaves = id == block::LEAVES || id == block::LEAVES2;
        let tinted = (id == block::GRASS && f == 1) || leaves;
        if (tinted || id == block::GRASS) && tint.is_none() {
            tint = Some(if id == block::GRASS {
                cx.grass_tint(x, y, z)
            } else if id == block::LEAVES && meta & 3 == 1 {
                rgb(6396257)
            } else if id == block::LEAVES && meta & 3 == 2 {
                rgb(8431445)
            } else {
                cx.foliage_tint(x, y, z)
            });
        }
        let base = if tinted { tint.unwrap() } else { [1.0; 3] };
        let (t1, t2) = TANGENTS[f];
        let bq = r.bright(qx, qy, qz);
        let aq = r.ao(qx, qy, qz);
        let mut quad = [Vertex::default(); 4];
        let mut aos = [0f32; 4];
        for (vi, (p, uv)) in FACE_VERTS[f].iter().enumerate() {
            let s1 = if p[t1] == 1 { 1 } else { -1 };
            let s2 = if p[t2] == 1 { 1 } else { -1 };
            let mut a = [qx, qy, qz];
            a[t1] += s1;
            let mut b = [qx, qy, qz];
            b[t2] += s2;
            let (a1, b1) = (r.ao(a[0], a[1], a[2]), r.bright(a[0], a[1], a[2]));
            let (a2, b2) = (r.ao(b[0], b[1], b[2]), r.bright(b[0], b[1], b[2]));
            let f1 = r.can_block_grass(a[0] + n[0], a[1] + n[1], a[2] + n[2]);
            let f2 = r.can_block_grass(b[0] + n[0], b[1] + n[1], b[2] + n[2]);
            let (ac, bc) = if !f1 && !f2 {
                (a1, b1)
            } else {
                let mut cc = a;
                cc[t2] += s2;
                (r.ao(cc[0], cc[1], cc[2]), r.bright(cc[0], cc[1], cc[2]))
            };
            let ao = (a1 + a2 + ac + aq) / 4.0f32;
            aos[vi] = ao;
            let br = ao_brightness(b1, b2, bc, bq);
            let sh = SHADE[f] * ao;
            quad[vi] = Vertex {
                pos: [(lx + p[0]) as f32, (y + p[1]) as f32, (lz + p[2]) as f32],
                uv: [
                    rect[0] + (rect[2] - rect[0]) * uv[0],
                    rect[1] + (rect[3] - rect[1]) * uv[1],
                ],
                color: col(base[0] * sh, base[1] * sh, base[2] * sh),
                light: light_bytes(br),
            };
        }
        out.extend_from_slice(&quad);
        if id == block::GRASS && f >= 2 && !snowed {
            // Fancy grass: tinted side overlay.
            let o = cx.atlas.rects[cx.atlas.grass_overlay as usize];
            let t = tint.unwrap();
            for (vi, (_, uv)) in FACE_VERTS[f].iter().enumerate() {
                let sh = SHADE[f] * aos[vi];
                quad[vi].uv = [o[0] + (o[2] - o[0]) * uv[0], o[1] + (o[3] - o[1]) * uv[1]];
                quad[vi].color = col(t[0] * sh, t[1] * sh, t[2] * sh);
            }
            out.extend_from_slice(&quad);
        }
    }
}

/// Axis-aligned box with per-face flat lighting (cactus, snow layers, slabs,
/// chests). `inset` moves the side faces inwards without shrinking them, as
/// `renderBlockCactusImpl` does.
#[allow(clippy::too_many_arguments)]
fn boxed(
    cx: &Ctx,
    out: &mut Vec<Vertex>,
    id: u8,
    meta: i32,
    x: i32,
    y: i32,
    z: i32,
    b: [f32; 6],
    inset: f32,
) {
    let r = &cx.r;
    let (lx, lz) = ((x - 16) as f32, (z - 16) as f32);
    let own = r.bright(x, y, z);
    for f in 0..6 {
        let n = NORMALS[f];
        let axis = f / 2;
        let positive = f % 2 == 1;
        let on_boundary = if positive {
            b[axis + 3] >= 1.0
        } else {
            b[axis] <= 0.0
        };
        let (qx, qy, qz) = (x + n[0], y + n[1], z + n[2]);
        let nid = r.block(qx, qy, qz);
        if on_boundary && block::opaque_cube(nid) && !cx.all_faces {
            continue;
        }
        if id == block::SNOW_LAYER && f >= 2 && nid == id && r.meta(qx, qy, qz) >= meta {
            continue;
        }
        let Some(rect) = cx.rect(id, meta, f) else {
            continue;
        };
        let br = if on_boundary && inset == 0.0 || inset > 0.0 {
            r.bright(qx, qy, qz)
        } else {
            own
        };
        let sh = SHADE[f];
        let c = col(sh, sh, sh);
        let mut quad = [Vertex::default(); 4];
        for (vi, (p, _)) in FACE_VERTS[f].iter().enumerate() {
            let mut pos = [0f32; 3];
            for a in 0..3 {
                pos[a] = if p[a] == 1 { b[a + 3] } else { b[a] };
            }
            // Texture coordinates follow the block-space position.
            let (u, v) = match f {
                0 | 1 => (pos[0], pos[2]),
                2 => (1.0 - pos[0], 1.0 - pos[1]),
                3 => (pos[0], 1.0 - pos[1]),
                4 => (pos[2], 1.0 - pos[1]),
                _ => (1.0 - pos[2], 1.0 - pos[1]),
            };
            if inset > 0.0 && f >= 2 {
                pos[axis] += if positive { -inset } else { inset };
            }
            quad[vi] = Vertex {
                pos: [lx + pos[0], y as f32 + pos[1], lz + pos[2]],
                uv: [
                    rect[0] + (rect[2] - rect[0]) * u,
                    rect[1] + (rect[3] - rect[1]) * v,
                ],
                color: c,
                light: light_bytes(br),
            };
        }
        out.extend_from_slice(&quad);
    }
}

/// `drawCrossedSquares(icon, x, y, z, 1.0)`
fn crossed(
    out: &mut Vec<Vertex>,
    t: [f32; 4],
    x: f64,
    y: f64,
    z: f64,
    color: [u8; 4],
    light: [u8; 4],
) {
    let d = 0.45f64;
    let (x0, x1, z0, z1) = (
        (x + 0.5 - d) as f32,
        (x + 0.5 + d) as f32,
        (z + 0.5 - d) as f32,
        (z + 0.5 + d) as f32,
    );
    let (y0, y1) = (y as f32, (y + 1.0) as f32);
    let (u0, v0, u1, v1) = (t[0], t[1], t[2], t[3]);
    let v = |px: f32, py: f32, pz: f32, u: f32, vv: f32| Vertex {
        pos: [px, py, pz],
        uv: [u, vv],
        color,
        light,
    };
    out.extend_from_slice(&[
        v(x0, y1, z0, u0, v0),
        v(x0, y0, z0, u0, v1),
        v(x1, y0, z1, u1, v1),
        v(x1, y1, z1, u1, v0),
        v(x1, y1, z1, u0, v0),
        v(x1, y0, z1, u0, v1),
        v(x0, y0, z0, u1, v1),
        v(x0, y1, z0, u1, v0),
        v(x0, y1, z1, u0, v0),
        v(x0, y0, z1, u0, v1),
        v(x1, y0, z0, u1, v1),
        v(x1, y1, z0, u1, v0),
        v(x1, y1, z0, u0, v0),
        v(x1, y0, z0, u0, v1),
        v(x0, y0, z1, u1, v1),
        v(x0, y1, z1, u1, v0),
    ]);
}

/// `renderBlockDoublePlant`
fn double_plant(cx: &Ctx, out: &mut Vec<Vertex>, meta: i32, x: i32, y: i32, z: i32) {
    let r = &cx.r;
    let top = meta & 8 != 0;
    let variant = if top {
        if r.block(x, y - 1, z) != block::DOUBLE_PLANT {
            return;
        }
        r.meta(x, y - 1, z) & 7
    } else {
        meta & 7
    };
    // MC-30630: vanilla samples the biome colour at each half's own height, so
    // the two halves of tall grass and ferns can differ in colour.
    let tint_y = if top && crate::compat::fixed() {
        y - 1
    } else {
        y
    };
    let tint = if variant == 2 || variant == 3 {
        cx.grass_color_at(x, tint_y, z)
    } else {
        [1.0; 3]
    };
    let color = col(tint[0], tint[1], tint[2]);
    let light = light_bytes(r.bright(x, y, z));
    let (wx, wz) = (r.ox + x, r.oz + z);
    let mut h = (wx.wrapping_mul(3129871) as i64) ^ (wz as i64).wrapping_mul(116129781);
    h = h
        .wrapping_mul(h)
        .wrapping_mul(42317861)
        .wrapping_add(h.wrapping_mul(11));
    let px = (x - 16) as f64 + (((h >> 16 & 15) as f32 / 15.0f32) as f64 - 0.5) * 0.3;
    let pz = (z - 16) as f64 + (((h >> 24 & 15) as f32 / 15.0f32) as f64 - 0.5) * 0.3;
    let py = y as f64;
    let Some(t) = cx.rect(block::DOUBLE_PLANT, variant, if top { 1 } else { 0 }) else {
        return;
    };
    crossed(out, t, px, py, pz, color, light);
    if top && variant == 0 {
        let a = (h as f64 * 0.8).cos() * std::f64::consts::PI * 0.1;
        let (c, s) = (a.cos(), a.sin());
        let p1 = (0.5 + 0.3 * c - 0.5 * s, 0.5 + 0.5 * c + 0.3 * s);
        let p2 = (0.5 + 0.3 * c + 0.5 * s, 0.5 + -0.5 * c + 0.3 * s);
        let p3 = (0.5 + -0.05 * c + 0.5 * s, 0.5 + -0.5 * c + -0.05 * s);
        let p4 = (0.5 + -0.05 * c - 0.5 * s, 0.5 + 0.5 * c + -0.05 * s);
        let v = |p: (f64, f64), dy: f64, u: f32, vv: f32| Vertex {
            pos: [(px + p.0) as f32, (py + dy) as f32, (pz + p.1) as f32],
            uv: [u, vv],
            color,
            light,
        };
        let f = cx.atlas.rects[cx.atlas.sunflower_front as usize];
        out.extend_from_slice(&[
            v(p3, 1.0, f[0], f[3]),
            v(p4, 1.0, f[2], f[3]),
            v(p1, 0.0, f[2], f[1]),
            v(p2, 0.0, f[0], f[1]),
        ]);
        let b = cx.atlas.rects[cx.atlas.sunflower_back as usize];
        out.extend_from_slice(&[
            v(p4, 1.0, b[0], b[3]),
            v(p3, 1.0, b[2], b[3]),
            v(p2, 0.0, b[2], b[1]),
            v(p1, 0.0, b[0], b[1]),
        ]);
    }
}

/// `renderBlockVine`
fn vine(cx: &Ctx, out: &mut Vec<Vertex>, meta: i32, x: i32, y: i32, z: i32) {
    let r = &cx.r;
    let Some(t) = cx.rect(block::VINE, 0, 0) else {
        return;
    };
    let tint =
        rgb(r
            .biome(x, z)
            .foliage_color(cx.noise, &cx.atlas.foliage_map, r.ox + x, y, r.oz + z));
    let color = col(tint[0], tint[1], tint[2]);
    let light = light_bytes(r.bright(x, y, z));
    let (fx, fy, fz) = ((x - 16) as f32, y as f32, (z - 16) as f32);
    let (u0, v0, u1, v1) = (t[0], t[1], t[2], t[3]);
    let e = 0.05f32;
    let v = |px: f32, py: f32, pz: f32, u: f32, vv: f32| Vertex {
        pos: [fx + px, fy + py, fz + pz],
        uv: [u, vv],
        color,
        light,
    };
    let mut both = |q: [Vertex; 4]| {
        out.extend_from_slice(&q);
        out.extend_from_slice(&[q[3], q[2], q[1], q[0]]);
    };
    if meta & 2 != 0 {
        both([
            v(e, 1.0, 1.0, u0, v0),
            v(e, 0.0, 1.0, u0, v1),
            v(e, 0.0, 0.0, u1, v1),
            v(e, 1.0, 0.0, u1, v0),
        ]);
    }
    if meta & 8 != 0 {
        both([
            v(1.0 - e, 0.0, 1.0, u1, v1),
            v(1.0 - e, 1.0, 1.0, u1, v0),
            v(1.0 - e, 1.0, 0.0, u0, v0),
            v(1.0 - e, 0.0, 0.0, u0, v1),
        ]);
    }
    if meta & 4 != 0 {
        both([
            v(1.0, 0.0, e, u1, v1),
            v(1.0, 1.0, e, u1, v0),
            v(0.0, 1.0, e, u0, v0),
            v(0.0, 0.0, e, u0, v1),
        ]);
    }
    if meta & 1 != 0 {
        both([
            v(1.0, 1.0, 1.0 - e, u0, v0),
            v(1.0, 0.0, 1.0 - e, u0, v1),
            v(0.0, 0.0, 1.0 - e, u1, v1),
            v(0.0, 1.0, 1.0 - e, u1, v0),
        ]);
    }
    if block::info(r.block(x, y + 1, z)).normal_cube() {
        out.extend_from_slice(&[
            v(1.0, 1.0 - e, 0.0, u0, v0),
            v(1.0, 1.0 - e, 1.0, u0, v1),
            v(0.0, 1.0 - e, 1.0, u1, v1),
            v(0.0, 1.0 - e, 0.0, u1, v0),
        ]);
    }
}

/// `renderBlockLilyPad`
fn lily_pad(cx: &Ctx, out: &mut Vec<Vertex>, x: i32, y: i32, z: i32) {
    let r = &cx.r;
    let Some(t) = cx.rect(block::WATERLILY, 0, 1) else {
        return;
    };
    let h = pos_hash(r.ox + x, y, r.oz + z);
    let k = (h >> 16 & 3) as i32;
    let light = light_bytes(r.bright(x, y, z));
    let (fx, fz) = ((x - 16) as f32 + 0.5, (z - 16) as f32 + 0.5);
    let fy = y as f32 + 0.015625;
    let a = (k & 1) as f32 * 0.5 * (1 - k / 2 % 2 * 2) as f32;
    let b = ((k + 1) & 1) as f32 * 0.5 * (1 - (k + 1) / 2 % 2 * 2) as f32;
    let top = rgb(2129968);
    let c1 = col(top[0], top[1], top[2]);
    let under = rgb((2129968 & 16711422) >> 1);
    let c2 = col(under[0], under[1], under[2]);
    let v = |px: f32, pz: f32, u: f32, vv: f32, color: [u8; 4]| Vertex {
        pos: [px, fy, pz],
        uv: [u, vv],
        color,
        light,
    };
    let p = [
        (fx + a - b, fz + a + b, t[0], t[1]),
        (fx + a + b, fz - a + b, t[2], t[1]),
        (fx - a + b, fz - a - b, t[2], t[3]),
        (fx - a - b, fz + a - b, t[0], t[3]),
    ];
    out.extend_from_slice(&[
        v(p[0].0, p[0].1, p[0].2, p[0].3, c1),
        v(p[1].0, p[1].1, p[1].2, p[1].3, c1),
        v(p[2].0, p[2].1, p[2].2, p[2].3, c1),
        v(p[3].0, p[3].1, p[3].2, p[3].3, c1),
    ]);
    out.extend_from_slice(&[
        v(p[3].0, p[3].1, p[3].2, p[3].3, c2),
        v(p[2].0, p[2].1, p[2].2, p[2].3, c2),
        v(p[1].0, p[1].1, p[1].2, p[1].3, c2),
        v(p[0].0, p[0].1, p[0].2, p[0].3, c2),
    ]);
}

/// `RenderBlocks.renderBlockLiquid`
fn liquid(cx: &Ctx, out: &mut Vec<Vertex>, id: u8, x: i32, y: i32, z: i32) {
    let r = &cx.r;
    let atlas = cx.atlas;
    let m = block::material(id);
    let side = |nx: i32, ny: i32, nz: i32, s: usize| -> bool {
        let nid = r.block(nx, ny, nz);
        if block::material(nid) == m {
            false
        } else if s == 1 {
            true
        } else if nid == block::ICE {
            false
        } else {
            !block::opaque_cube(nid)
        }
    };
    let top = side(x, y + 1, z, 1);
    let bottom = side(x, y - 1, z, 0);
    let sides = [
        side(x, y, z - 1, 2),
        side(x, y, z + 1, 3),
        side(x - 1, y, z, 4),
        side(x + 1, y, z, 5),
    ];
    if !top && !bottom && !sides.iter().any(|&s| s) {
        return;
    }
    let tint = if m == Material::Water {
        cx.blend(x, y, z, |b, _, _, _| b.water_color)
    } else {
        [1.0; 3]
    };
    let (lx, lz, fy) = ((x - 16) as f32, (z - 16) as f32, y as f32);
    let mut d2 = r.liquid_height(x, y, z, m);
    let mut d3 = r.liquid_height(x, y, z + 1, m);
    let mut d4 = r.liquid_height(x + 1, y, z + 1, m);
    let mut d5 = r.liquid_height(x + 1, y, z, m);
    let still = atlas.tile(id, 0, 1).unwrap();
    let flow = atlas.rects[if m == Material::Water {
        atlas.water_flow
    } else {
        atlas.lava_flow
    } as usize];
    let mut push = |q: [Vertex; 4], double: bool| {
        out.extend_from_slice(&q);
        if double {
            out.extend_from_slice(&[q[3], q[2], q[1], q[0]]);
        }
    };
    if top {
        d2 -= 0.001;
        d3 -= 0.001;
        d4 -= 0.001;
        d5 -= 0.001;
        let c = col(tint[0], tint[1], tint[2]);
        let l = light_bytes(r.liquid_bright(x, y, z));
        // UVs in sprite units (0..16).
        let (t, uv) = match r.flow_direction(m, x, y, z) {
            None => (still, [(0.0, 0.0), (0.0, 16.0), (16.0, 16.0), (16.0, 0.0)]),
            Some(a) => {
                let s = a.sin() * 0.25;
                let co = a.cos() * 0.25;
                (
                    flow,
                    [
                        (8.0 + (-co - s) * 16.0, 8.0 + (-co + s) * 16.0),
                        (8.0 + (-co + s) * 16.0, 8.0 + (co + s) * 16.0),
                        (8.0 + (co + s) * 16.0, 8.0 + (co - s) * 16.0),
                        (8.0 + (co - s) * 16.0, 8.0 + (-co - s) * 16.0),
                    ],
                )
            }
        };
        let v = |px: f32, h: f32, pz: f32, uv: (f32, f32)| Vertex {
            pos: [lx + px, fy + h, lz + pz],
            uv: [
                t[0] + (t[2] - t[0]) * uv.0 / 16.0,
                t[1] + (t[3] - t[1]) * uv.1 / 16.0,
            ],
            color: c,
            light: l,
        };
        push(
            [
                v(0.0, d2, 0.0, uv[0]),
                v(0.0, d3, 1.0, uv[1]),
                v(1.0, d4, 1.0, uv[2]),
                v(1.0, d5, 0.0, uv[3]),
            ],
            true,
        );
    }
    if bottom {
        let c = col(0.5, 0.5, 0.5);
        let l = light_bytes(r.liquid_bright(x, y - 1, z));
        let mut q = [Vertex::default(); 4];
        for (vi, (p, uv)) in FACE_VERTS[0].iter().enumerate() {
            q[vi] = Vertex {
                pos: [lx + p[0] as f32, fy + 0.001, lz + p[2] as f32],
                uv: [
                    still[0] + (still[2] - still[0]) * uv[0],
                    still[1] + (still[3] - still[1]) * uv[1],
                ],
                color: c,
                light: l,
            };
        }
        push(q, false);
    }
    for (i, &vis) in sides.iter().enumerate() {
        if !vis {
            continue;
        }
        let e = 0.001f32;
        let (h1, h2, x1, x2, z1, z2, nx, nz) = match i {
            0 => (d2, d5, 0.0, 1.0, e, e, x, z - 1),
            1 => (d4, d3, 1.0, 0.0, 1.0 - e, 1.0 - e, x, z + 1),
            2 => (d3, d2, e, e, 1.0, 0.0, x - 1, z),
            _ => (d5, d4, 1.0 - e, 1.0 - e, 0.0, 1.0, x + 1, z),
        };
        let u0 = flow[0];
        let u1 = flow[0] + (flow[2] - flow[0]) * 0.5;
        let vh = |h: f32| flow[1] + (flow[3] - flow[1]) * ((1.0 - h) * 0.5);
        let vb = flow[1] + (flow[3] - flow[1]) * 0.5;
        let sh = if i < 2 { 0.8f32 } else { 0.6f32 };
        let c = col(sh * tint[0], sh * tint[1], sh * tint[2]);
        let l = light_bytes(r.liquid_bright(nx, y, nz));
        let v = |px: f32, py: f32, pz: f32, u: f32, vv: f32| Vertex {
            pos: [lx + px, fy + py, lz + pz],
            uv: [u, vv],
            color: c,
            light: l,
        };
        push(
            [
                v(x1, h1, z1, u0, vh(h1)),
                v(x2, h2, z2, u1, vh(h2)),
                v(x2, 0.0, z2, u1, vb),
                v(x1, 0.0, z1, u0, vb),
            ],
            true,
        );
    }
}

// ------------------------------------------------------------ shaped blocks

type TexFn<'a> = &'a dyn Fn(usize, usize) -> Option<([f32; 4], bool)>;

/// Renders a list of boxes with per-face textures. Faces on the cell boundary
/// next to an opaque block are culled and take the neighbour's brightness.
#[allow(clippy::too_many_arguments)]
fn boxes(
    cx: &Ctx,
    out: &mut Vec<Vertex>,
    x: i32,
    y: i32,
    z: i32,
    list: &[crate::shapes::Bx],
    tex: TexFn,
    tint: [f32; 3],
) {
    let r = &cx.r;
    let (lx, lz) = ((x - 16) as f32, (z - 16) as f32);
    let own = r.bright(x, y, z);
    for (bi, b) in list.iter().enumerate() {
        for f in 0..6 {
            let n = NORMALS[f];
            let axis = f / 2;
            let positive = f % 2 == 1;
            let on_boundary = if positive {
                b[axis + 3] >= 1.0
            } else {
                b[axis] <= 0.0
            };
            let (qx, qy, qz) = (x + n[0], y + n[1], z + n[2]);
            if on_boundary && block::opaque_cube(r.block(qx, qy, qz)) && !cx.all_faces {
                continue;
            }
            let Some((rect, flip)) = tex(bi, f) else {
                continue;
            };
            let br = if on_boundary {
                r.bright(qx, qy, qz)
            } else {
                own
            };
            let sh = SHADE[f];
            let c = col(tint[0] * sh, tint[1] * sh, tint[2] * sh);
            let mut q = [Vertex::default(); 4];
            for (vi, (p, _)) in FACE_VERTS[f].iter().enumerate() {
                let mut pos = [0f32; 3];
                for a in 0..3 {
                    pos[a] = if p[a] == 1 { b[a + 3] } else { b[a] };
                }
                // RenderBlocks.renderFaceZNeg / renderFaceXPos (without
                // field_152631_f, which only fences and stairs set): the U
                // runs backwards within the box's own range, so a thin box
                // samples the near edge of the icon, not the far one
                let (mut u, v) = match f {
                    0 | 1 => (pos[0], pos[2]),
                    2 => (b[0] + b[3] - pos[0], 1.0 - pos[1]),
                    3 => (pos[0], 1.0 - pos[1]),
                    4 => (pos[2], 1.0 - pos[1]),
                    _ => (b[2] + b[5] - pos[2], 1.0 - pos[1]),
                };
                if flip {
                    u = 1.0 - u;
                }
                q[vi] = Vertex {
                    pos: [lx + pos[0], y as f32 + pos[1], lz + pos[2]],
                    uv: [
                        rect[0] + (rect[2] - rect[0]) * u,
                        rect[1] + (rect[3] - rect[1]) * v.clamp(0.0, 1.0),
                    ],
                    color: c,
                    light: light_bytes(br),
                };
            }
            out.extend_from_slice(&q);
        }
    }
}

/// A free-standing quad; `two_sided` adds the reversed winding.
fn quad(
    out: &mut Vec<Vertex>,
    p: [[f32; 3]; 4],
    uv: [[f32; 2]; 4],
    color: [u8; 4],
    light: [u8; 4],
    two_sided: bool,
) {
    let v = [0, 1, 2, 3].map(|i| Vertex {
        pos: p[i],
        uv: uv[i],
        color,
        light,
    });
    out.extend_from_slice(&v);
    if two_sided {
        out.extend_from_slice(&[v[3], v[2], v[1], v[0]]);
    }
}

fn shaped(cx: &Ctx, out: &mut Vec<Vertex>, id: u8, meta: i32, x: i32, y: i32, z: i32) {
    use crate::shapes;
    let r = &cx.r;
    let get = |gx: i32, gy: i32, gz: i32| (r.block(gx, gy, gz), r.meta(gx, gy, gz) as u8);
    let m = meta as u8;
    let plain = |f: usize| cx.rect(id, meta, f).map(|t| (t, false));
    let white = [1.0f32; 3];
    match block::info(id).shape {
        Shape::Stairs => {
            let list = shapes::stairs(&get, x, y, z, m);
            rb::Rb::new(cx, out, id, meta).render_block_stairs(x, y, z, &list);
        }
        Shape::Fence => {
            let [n, s, w, e] = shapes::fence_links(&get, x, y, z);
            rb::Rb::new(cx, out, id, meta).render_block_fence(x, y, z, [w, e, n, s]);
        }
        // BlockPane.getRenderType: 41 for glass, 18 for iron bars
        Shape::Pane => {
            let links = shapes::pane_links(&get, id, x, y, z);
            let mut rb = rb::Rb::new(cx, out, id, meta);
            if block::material(id) == block::Material::Glass {
                rb.render_block_stained_glass_pane(x, y, z, links);
            } else {
                rb.render_block_pane(x, y, z, links);
            }
        }
        Shape::Door => {
            let full = shapes::door_full_meta(&get, x, y, z, m);
            let bx = shapes::door(full);
            let tex_meta = if full & 8 != 0 { 8 } else { 0 };
            let tex = |_: usize, f: usize| {
                if f < 2 {
                    return cx.rect(id, 0, f).map(|t| (t, false));
                }
                // BlockDoor.getIcon(world, x, y, z, side)
                let d = full & 3;
                let open = full & 4 != 0;
                let mut flip = false;
                if open {
                    if (d == 0 && f == 2)
                        || (d == 1 && f == 5)
                        || (d == 2 && f == 3)
                        || (d == 3 && f == 4)
                    {
                        flip = !flip;
                    }
                } else {
                    if (d == 0 && f == 5)
                        || (d == 1 && f == 3)
                        || (d == 2 && f == 4)
                        || (d == 3 && f == 2)
                    {
                        flip = !flip;
                    }
                    if full & 16 != 0 {
                        flip = !flip;
                    }
                }
                cx.rect(id, tex_meta, f).map(|t| (t, flip))
            };
            boxes(cx, out, x, y, z, &[bx], &tex, white);
        }
        _ => match id {
            block::CAULDRON => cauldron(cx, out, meta, x, y, z),
            // renderBlockEndPortalFrame: the top turned to the facing, the
            // eye (all faces) on it when inserted
            block::END_PORTAL_FRAME => {
                let eye = cx.rect(id, 8, 1).map(rb::Icon::new);
                let mut rb = rb::Rb::new(cx, out, id, meta & 3);
                rb.uvRotateTop = match meta & 3 {
                    0 => 3,
                    3 => 1,
                    1 => 2,
                    _ => 0,
                };
                rb.set_render_bounds(0.0, 0.0, 0.0, 1.0, 0.8125, 1.0);
                rb.render_standard(x, y, z);
                if meta & 4 != 0 {
                    rb.renderAllFaces = true;
                    rb.overrideBlockTexture = eye;
                    rb.set_render_bounds(0.25, 0.8125, 0.25, 0.75, 1.0, 0.75);
                    rb.render_standard(x, y, z);
                }
            }
            // renderBlockDragonEgg: eight slices narrowing to the top
            block::DRAGON_EGG => {
                let mut rb = rb::Rb::new(cx, out, id, meta);
                let mut k = 0i32;
                for (w, h) in [(2, 1), (3, 1), (4, 1), (5, 2), (6, 3), (7, 5), (6, 2), (3, 1)] {
                    let r = w as f64 / 16.0;
                    let top = 1.0 - k as f64 / 16.0;
                    let bottom = 1.0 - (k + h) as f64 / 16.0;
                    k += h;
                    rb.set_render_bounds(0.5 - r, bottom, 0.5 - r, 0.5 + r, top, 0.5 + r);
                    rb.render_standard(x, y, z);
                }
            }
            block::FLOWER_POT => flower_pot(cx, out, meta, x, y, z),
            block::LEVER => {
                rb::Rb::new(cx, out, id, meta).render_block_lever(x, y, z);
            }
            block::UNPOWERED_REPEATER | block::POWERED_REPEATER => {
                rb::Rb::new(cx, out, id, meta).render_block_repeater(x, y, z);
            }
            block::UNPOWERED_COMPARATOR | block::POWERED_COMPARATOR => {
                rb::Rb::new(cx, out, id, meta).render_block_redstone_comparator(x, y, z);
            }
            _ => {
                let list = shapes::static_boxes(id, m);
                boxes(cx, out, x, y, z, &list, &|_, f| plain(f), white);
            }
        },
    }
}

/// `renderBlockCauldron`: a standard block (the legs and the hollow are
/// the textures' holes), the four inner walls with the side texture, the
/// inner floor both ways, and the water at its level.
fn cauldron(cx: &Ctx, out: &mut Vec<Vertex>, meta: i32, x: i32, y: i32, z: i32) {
    let mut rb = rb::Rb::new(cx, out, block::CAULDRON, 0);
    rb.render_standard(x, y, z);
    let bright = rb.mixed_brightness(x, y, z);
    rb.set_color_brightness(1.0, bright);
    let side = rb.side_icon(2);
    let e = 0.125f64;
    let (fx, fy, fz) = (x as f64, y as f64, z as f64);
    rb.face_x_pos(fx - 1.0 + e, fy, fz, side);
    rb.face_x_neg(fx + 1.0 - e, fy, fz, side);
    rb.face_z_pos(fx, fy, fz - 1.0 + e, side);
    rb.face_z_neg(fx, fy, fz + 1.0 - e, side);
    let inner = rb::Icon::new(cx.rect(block::CAULDRON, 8, 2).unwrap_or([0.0; 4]));
    rb.face_y_pos(fx, fy - 1.0 + 0.25, fz, inner);
    rb.face_y_neg(fx, fy + 1.0 - 0.75, fz, inner);
    if meta > 0 {
        // BlockCauldron.getRenderLiquidLevel, water_still
        let water = rb::Icon::new(cx.rect(block::WATER, 0, 1).unwrap_or([0.0; 4]));
        let level = (6 + 3 * meta.clamp(0, 3)) as f64 / 16.0;
        rb.face_y_pos(fx, fy - 1.0 + level, fz, water);
    }
}

/// `renderBlockFlowerpot`: the pot as a standard block, its four inner
/// walls and the dirt, then the plant of its tile entity.
fn flower_pot(cx: &Ctx, out: &mut Vec<Vertex>, meta: i32, x: i32, y: i32, z: i32) {
    let r = &cx.r;
    {
        let mut rb = rb::Rb::new(cx, out, block::FLOWER_POT, 0);
        rb.set_bounds([0.3125, 0.0, 0.3125, 0.6875, 0.375, 0.6875]);
        rb.render_standard(x, y, z);
        let bright = rb.mixed_brightness(x, y, z);
        rb.set_color_brightness(1.0, bright);
        let icon = rb.side_icon(0);
        let e = 0.1865f64;
        let (fx, fy, fz) = (x as f64, y as f64, z as f64);
        rb.face_x_pos(fx - 0.5 + e, fy, fz, icon);
        rb.face_x_neg(fx + 0.5 - e, fy, fz, icon);
        rb.face_z_pos(fx, fy, fz - 0.5 + e, icon);
        rb.face_z_neg(fx, fy, fz + 0.5 - e, icon);
        let dirt = rb::Icon::new(cx.rect(block::DIRT, 0, 1).unwrap_or([0.0; 4]));
        rb.face_y_pos(fx, fy - 0.5 + e + 0.1875, fz, dirt);
    }
    let light = light_bytes(r.bright(x, y, z));
    let (lx, lz) = ((x - 16) as f32, (z - 16) as f32);
    let fy = y as f32;
    // the plant in it (TileEntityFlowerPot), lifted by 4/16
    let at = [(x - 16) as u8, y as u8, (z - 16) as u8];
    let plant = match cx.r.chunks[4].pots.iter().find(|p| p.0 == at) {
        Some(&(_, b, d)) => (b != 0).then_some((b, d as i32)),
        None => pot_from_meta(meta),
    };
    match plant {
        Some((block::CACTUS, _)) => {
            // render type 13: three stacked boxes a quarter wide, all faces
            let mut rb = rb::Rb::new(cx, out, block::CACTUS, 0);
            rb.renderAllFaces = true;
            rb.origin[1] -= 0.25;
            let e = 0.125;
            for k in 0..3 {
                let y0 = k as f64 * 0.25;
                rb.set_render_bounds(0.5 - e, y0, 0.5 - e, 0.5 + e, y0 + 0.25, 0.5 + e);
                rb.render_standard(x, y, z);
            }
        }
        Some((b, d)) => {
            // render type 1: drawCrossedSquares at three quarters' size,
            // in the plant's colour (a fern takes the grass colour)
            if let Some(t) = cx.rect(b, d, 0) {
                let c = if b == block::TALLGRASS {
                    let g = cx.grass_color_at(x, y, z);
                    col(g[0], g[1], g[2])
                } else {
                    [255; 4]
                };
                let s = 0.75f32;
                let h = 0.45 * s;
                let (x0, x1, z0, z1) = (lx + 0.5 - h, lx + 0.5 + h, lz + 0.5 - h, lz + 0.5 + h);
                let (y0, y1) = (fy + 0.25, fy + 0.25 + s);
                let uv = [[t[0], t[1]], [t[0], t[3]], [t[2], t[3]], [t[2], t[1]]];
                for q in [
                    [[x0, y1, z0], [x0, y0, z0], [x1, y0, z1], [x1, y1, z1]],
                    [[x1, y1, z1], [x1, y0, z1], [x0, y0, z0], [x0, y1, z0]],
                    [[x0, y1, z1], [x0, y0, z1], [x1, y0, z0], [x1, y1, z0]],
                    [[x1, y1, z0], [x1, y0, z0], [x0, y0, z1], [x0, y1, z1]],
                ] {
                    quad(out, q, uv, c, light, false);
                }
            }
        }
        None => {}
    }
}

/// A standing torch displaced inside the cell (repeater torches).
#[allow(clippy::too_many_arguments)]

/// `renderBlockStem` (render type 19): `renderBlockStemSmall` up to the
/// stem's height, and with a fruit beside a grown stem the bent
/// `renderBlockStemBig` towards it; tinted by `BlockStem.getRenderColor`.
fn stem(cx: &Ctx, out: &mut Vec<Vertex>, id: u8, meta: i32, x: i32, y: i32, z: i32) {
    let r = &cx.r;
    let light = light_bytes(r.bright(x, y, z));
    let col = [
        (meta * 32) as u8,
        (255 - meta * 8) as u8,
        (meta * 4) as u8,
        255,
    ];
    let height = crate::shapes::stem(meta as u8)[4] as f64;
    let fruit = if id == block::PUMPKIN_STEM {
        block::PUMPKIN
    } else {
        block::MELON_BLOCK
    };
    // BlockStem.getState
    let state = if meta < 7 {
        -1
    } else if r.block(x - 1, y, z) == fruit {
        0
    } else if r.block(x + 1, y, z) == fruit {
        1
    } else if r.block(x, y, z - 1) == fruit {
        2
    } else if r.block(x, y, z + 1) == fruit {
        3
    } else {
        -1
    };
    let (bx, by, bz) = (
        (x - 16) as f64,
        y as f64 - 0.0625f32 as f64,
        (z - 16) as f64,
    );
    let mut v = |p: [f64; 3], uv: [f32; 2]| {
        out.push(Vertex {
            pos: [p[0] as f32, p[1] as f32, p[2] as f32],
            uv,
            color: col,
            light,
        });
    };
    // renderBlockStemSmall
    let small = if state < 0 { height } else { 0.5 };
    if let Some(t) = cx.rect(id, meta, 0) {
        let (u0, v0, u1) = (t[0], t[1], t[2]);
        let v1 = (t[1] as f64 + (t[3] - t[1]) as f64 * (small * 16.0) / 16.0) as f32;
        let x0 = bx + 0.5 - 0.45f32 as f64;
        let x1 = bx + 0.5 + 0.45f32 as f64;
        let z0 = bz + 0.5 - 0.45f32 as f64;
        let z1 = bz + 0.5 + 0.45f32 as f64;
        let quads = [
            [
                (x0, z0, u0, v0, true),
                (x0, z0, u0, v1, false),
                (x1, z1, u1, v1, false),
                (x1, z1, u1, v0, true),
            ],
            [
                (x1, z1, u1, v0, true),
                (x1, z1, u1, v1, false),
                (x0, z0, u0, v1, false),
                (x0, z0, u0, v0, true),
            ],
            [
                (x0, z1, u0, v0, true),
                (x0, z1, u0, v1, false),
                (x1, z0, u1, v1, false),
                (x1, z0, u1, v0, true),
            ],
            [
                (x1, z0, u1, v0, true),
                (x1, z0, u1, v1, false),
                (x0, z1, u0, v1, false),
                (x0, z1, u0, v0, true),
            ],
        ];
        for q in quads {
            for (px, pz, u, vv, top) in q {
                v([px, by + if top { small } else { 0.0 }, pz], [u, vv]);
            }
        }
    }
    // renderBlockStemBig
    if state >= 0 {
        let t = cx.atlas.named(if id == block::PUMPKIN_STEM {
            "pumpkin_stem_connected"
        } else {
            "melon_stem_connected"
        });
        let (mut u0, v0, mut u1, v1) = (t[0], t[1], t[2], t[3]);
        if (state + 1) / 2 % 2 == 1 {
            std::mem::swap(&mut u0, &mut u1);
        }
        let (x0, x1, z0, z1) = (bx, bx + 1.0, bz, bz + 1.0);
        let (xm, zm) = (bx + 0.5, bz + 0.5);
        let top = by + height;
        if state < 2 {
            v([x0, top, zm], [u0, v0]);
            v([x0, by, zm], [u0, v1]);
            v([x1, by, zm], [u1, v1]);
            v([x1, top, zm], [u1, v0]);
            v([x1, top, zm], [u1, v0]);
            v([x1, by, zm], [u1, v1]);
            v([x0, by, zm], [u0, v1]);
            v([x0, top, zm], [u0, v0]);
        } else {
            v([xm, top, z1], [u0, v0]);
            v([xm, by, z1], [u0, v1]);
            v([xm, by, z0], [u1, v1]);
            v([xm, top, z0], [u1, v0]);
            v([xm, top, z0], [u1, v0]);
            v([xm, by, z0], [u1, v1]);
            v([xm, by, z1], [u0, v1]);
            v([xm, top, z1], [u0, v0]);
        }
    }
}

/// `BlockRedstoneWire.isPowerProviderOrWire` (without the repeater facing test).
/// `Block.canProvidePower` of everything but the wire and the repeaters.
fn provides_power(id: u8) -> bool {
    // detector rail, lever, pressure plates, redstone torches, buttons,
    // tripwire hook, trapped chest, weighted plates, comparators, daylight
    // sensor, redstone block
    matches!(
        id,
        28 | 69 | 70 | 72 | 75 | 76 | 77 | 131 | 143 | 146 | 147 | 148 | 149 | 150 | 151 | 152
    )
}

/// `BlockRedstoneWire.isPowerProviderOrWire(world, x, y, z, direction)`
fn wire_link(r: &Region, x: i32, y: i32, z: i32, dir: i32) -> bool {
    let id = r.block(x, y, z);
    if id == block::REDSTONE_WIRE {
        return true;
    }
    if id != block::UNPOWERED_REPEATER && id != block::POWERED_REPEATER {
        return provides_power(id) && dir != -1;
    }
    let m = r.meta(x, y, z) & 3;
    // Direction.rotateOpposite
    dir == m || dir == [2, 3, 0, 1][m as usize]
}

/// `RenderBlocks.renderBlockRedstoneWire`
fn wire(cx: &Ctx, out: &mut Vec<Vertex>, meta: i32, x: i32, y: i32, z: i32) {
    let r = &cx.r;
    let light = light_bytes(r.bright(x, y, z));
    let normal = |x: i32, y: i32, z: i32| block::info(r.block(x, y, z)).normal_cube();
    let (Some(cross), Some(line), Some(cross_o), Some(line_o)) = (
        cx.rect(block::REDSTONE_WIRE, 0, 0),
        cx.rect(block::REDSTONE_WIRE, 0, 1),
        cx.rect(block::REDSTONE_WIRE, 0, 2),
        cx.rect(block::REDSTONE_WIRE, 0, 3),
    ) else {
        return;
    };
    // BlockRedstoneWire.colorMultiplier
    let f = meta as f32 / 15.0;
    let mut red = f * 0.6 + 0.4;
    if meta == 0 {
        red = 0.3;
    }
    let green = (f * f * 0.7 - 0.5).max(0.0);
    let blue = (f * f * 0.6 - 0.7).max(0.0);
    let c = col(red, green, blue);
    let white = [255u8; 4];

    let side = |dx: i32, dz: i32, dir: i32| {
        wire_link(r, x + dx, y, z + dz, dir)
            || !normal(x + dx, y, z + dz) && wire_link(r, x + dx, y - 1, z + dz, -1)
    };
    let (mut w, mut e, mut n, mut so) =
        (side(-1, 0, 1), side(1, 0, 3), side(0, -1, 2), side(0, 1, 0));
    let open_above = !normal(x, y + 1, z);
    if open_above {
        let up =
            |dx: i32, dz: i32| normal(x + dx, y, z + dz) && wire_link(r, x + dx, y + 1, z + dz, -1);
        w |= up(-1, 0);
        e |= up(1, 0);
        n |= up(0, -1);
        so |= up(0, 1);
    }

    let (bx, by, bz) = ((x - 16) as f32, y as f32, (z - 16) as f32);
    let (mut x0, mut x1, mut z0, mut z1) = (bx, bx + 1.0, bz, bz + 1.0);
    let yy = (by as f64 + 0.015625) as f32;
    let mut kind = 0;
    if (w || e) && !n && !so {
        kind = 1;
    }
    if (n || so) && !e && !w {
        kind = 2;
    }
    // `getInterpolatedU` / `getInterpolatedV`
    let iu = |t: [f32; 4], d: i32| t[0] + (t[2] - t[0]) * d as f32 / 16.0;
    let iv = |t: [f32; 4], d: i32| t[1] + (t[3] - t[1]) * d as f32 / 16.0;
    if kind == 0 {
        let (mut u0, mut v0, mut u1, mut v1) = (0, 0, 16, 16);
        if !w {
            x0 += 0.3125;
            u0 += 5;
        }
        if !e {
            x1 -= 0.3125;
            u1 -= 5;
        }
        if !n {
            z0 += 0.3125;
            v0 += 5;
        }
        if !so {
            z1 -= 0.3125;
            v1 -= 5;
        }
        for (t, color) in [(cross, c), (cross_o, white)] {
            quad(
                out,
                [[x1, yy, z1], [x1, yy, z0], [x0, yy, z0], [x0, yy, z1]],
                [
                    [iu(t, u1), iv(t, v1)],
                    [iu(t, u1), iv(t, v0)],
                    [iu(t, u0), iv(t, v0)],
                    [iu(t, u0), iv(t, v1)],
                ],
                color,
                light,
                false,
            );
        }
    } else {
        for (t, color) in [(line, c), (line_o, white)] {
            let (u0, v0, u1, v1) = (t[0], t[1], t[2], t[3]);
            let uv = if kind == 1 {
                [[u1, v1], [u1, v0], [u0, v0], [u0, v1]]
            } else {
                [[u1, v1], [u0, v1], [u0, v0], [u1, v0]]
            };
            quad(
                out,
                [[x1, yy, z1], [x1, yy, z0], [x0, yy, z0], [x0, yy, z1]],
                uv,
                color,
                light,
                false,
            );
        }
    }

    // the wire climbing the faces of the blocks beside it
    if open_above {
        let top = by + 1.0 + 0.021875;
        let o = 0.015625f32;
        let climbs = |dx: i32, dz: i32| {
            normal(x + dx, y, z + dz) && r.block(x + dx, y + 1, z + dz) == block::REDSTONE_WIRE
        };
        for (t, color) in [(line, c), (line_o, white)] {
            let (u0, v0, u1, v1) = (t[0], t[1], t[2], t[3]);
            if climbs(-1, 0) {
                quad(
                    out,
                    [
                        [bx + o, top, bz + 1.0],
                        [bx + o, by, bz + 1.0],
                        [bx + o, by, bz],
                        [bx + o, top, bz],
                    ],
                    [[u1, v0], [u0, v0], [u0, v1], [u1, v1]],
                    color,
                    light,
                    false,
                );
            }
            if climbs(1, 0) {
                quad(
                    out,
                    [
                        [bx + 1.0 - o, by, bz + 1.0],
                        [bx + 1.0 - o, top, bz + 1.0],
                        [bx + 1.0 - o, top, bz],
                        [bx + 1.0 - o, by, bz],
                    ],
                    [[u0, v1], [u1, v1], [u1, v0], [u0, v0]],
                    color,
                    light,
                    false,
                );
            }
            if climbs(0, -1) {
                quad(
                    out,
                    [
                        [bx + 1.0, by, bz + o],
                        [bx + 1.0, top, bz + o],
                        [bx, top, bz + o],
                        [bx, by, bz + o],
                    ],
                    [[u0, v1], [u1, v1], [u1, v0], [u0, v0]],
                    color,
                    light,
                    false,
                );
            }
            if climbs(0, 1) {
                quad(
                    out,
                    [
                        [bx + 1.0, top, bz + 1.0 - o],
                        [bx + 1.0, by, bz + 1.0 - o],
                        [bx, by, bz + 1.0 - o],
                        [bx, top, bz + 1.0 - o],
                    ],
                    [[u1, v0], [u0, v0], [u0, v1], [u1, v1]],
                    color,
                    light,
                    false,
                );
            }
        }
    }
}

/// `renderBlockFire`: four slanted sheets per layer over a burning floor,
/// or sheets leaning against the burning sides.
fn fire(cx: &Ctx, out: &mut Vec<Vertex>, x: i32, y: i32, z: i32) {
    let r = &cx.r;
    let (Some(t0), Some(t1)) = (cx.rect(block::FIRE, 0, 0), cx.rect(block::FIRE, 0, 1)) else {
        return;
    };
    let light = light_bytes(r.bright(x, y, z));
    let c = [255u8; 4];
    let (bx, by, bz) = ((x - 16) as f32, y as f32, (z - 16) as f32);
    let h = 1.4f32;
    let below = r.block(x, y - 1, z);
    let flammable = |id: u8| {
        matches!(
            id,
            block::PLANKS
                | block::FENCE
                | block::LOG
                | block::LOG2
                | block::LEAVES
                | block::LEAVES2
                | block::WOOL
                | block::BOOKSHELF
                | block::TNT
                | block::TALLGRASS
                | block::VINE
                | block::COAL_BLOCK
                | block::HAY_BLOCK
                | block::CARPET
        ) || block::is_stairs(id) && block::material(id) == block::Material::Wood
    };
    let uv = |t: [f32; 4]| {
        let (u0, v0, u1, v1) = (t[0], t[1], t[2], t[3]);
        [[u1, v0], [u1, v1], [u0, v1], [u0, v0]]
    };
    let solid_below = block::info(below).normal_cube() || flammable(below);
    if solid_below {
        let (a, b) = if (x + y + z) & 1 == 1 {
            (t1, t0)
        } else {
            (t0, t1)
        };
        let (d5, d6, d7, d8) = (bx + 0.7, bx + 0.3, bz + 0.7, bz + 0.3);
        let (d9, d10, d11, d12) = (bx + 0.2, bx + 0.8, bz + 0.2, bz + 0.8);
        quad(
            out,
            [
                [d9, by + h, bz + 1.0],
                [d5, by, bz + 1.0],
                [d5, by, bz],
                [d9, by + h, bz],
            ],
            uv(a),
            c,
            light,
            true,
        );
        quad(
            out,
            [
                [d10, by + h, bz],
                [d6, by, bz],
                [d6, by, bz + 1.0],
                [d10, by + h, bz + 1.0],
            ],
            uv(a),
            c,
            light,
            true,
        );
        quad(
            out,
            [
                [bx + 1.0, by + h, d12],
                [bx + 1.0, by, d8],
                [bx, by, d8],
                [bx, by + h, d12],
            ],
            uv(b),
            c,
            light,
            true,
        );
        quad(
            out,
            [
                [bx, by + h, d11],
                [bx, by, d7],
                [bx + 1.0, by, d7],
                [bx + 1.0, by + h, d11],
            ],
            uv(b),
            c,
            light,
            true,
        );
        let (d9, d10, d11, d12) = (bx, bx + 1.0, bz, bz + 1.0);
        let (d5, d6, d7, d8) = (bx + 0.1, bx + 0.9, bz + 0.1, bz + 0.9);
        quad(
            out,
            [
                [d5, by + h, bz],
                [d9, by, bz],
                [d9, by, bz + 1.0],
                [d5, by + h, bz + 1.0],
            ],
            uv(b),
            c,
            light,
            true,
        );
        quad(
            out,
            [
                [d6, by + h, bz + 1.0],
                [d10, by, bz + 1.0],
                [d10, by, bz],
                [d6, by + h, bz],
            ],
            uv(b),
            c,
            light,
            true,
        );
        quad(
            out,
            [
                [bx, by + h, d8],
                [bx, by, d12],
                [bx + 1.0, by, d12],
                [bx + 1.0, by + h, d8],
            ],
            uv(a),
            c,
            light,
            true,
        );
        quad(
            out,
            [
                [bx + 1.0, by + h, d7],
                [bx + 1.0, by, d11],
                [bx, by, d11],
                [bx, by + h, d7],
            ],
            uv(a),
            c,
            light,
            true,
        );
    } else {
        // sheets leaning against each burning neighbour
        let f = 0.2f32;
        let hh = h;
        let side = |dx: i32, dz: i32| flammable(r.block(x + dx, y, z + dz));
        if side(-1, 0) {
            quad(
                out,
                [
                    [bx + f, by + hh, bz + 1.0],
                    [bx, by, bz + 1.0],
                    [bx, by, bz],
                    [bx + f, by + hh, bz],
                ],
                uv(t0),
                c,
                light,
                true,
            );
        }
        if side(1, 0) {
            quad(
                out,
                [
                    [bx + 1.0 - f, by + hh, bz],
                    [bx + 1.0, by, bz],
                    [bx + 1.0, by, bz + 1.0],
                    [bx + 1.0 - f, by + hh, bz + 1.0],
                ],
                uv(t0),
                c,
                light,
                true,
            );
        }
        if side(0, -1) {
            quad(
                out,
                [
                    [bx, by + hh, bz + f],
                    [bx, by, bz],
                    [bx + 1.0, by, bz],
                    [bx + 1.0, by + hh, bz + f],
                ],
                uv(t1),
                c,
                light,
                true,
            );
        }
        if side(0, 1) {
            quad(
                out,
                [
                    [bx + 1.0, by + hh, bz + 1.0 - f],
                    [bx + 1.0, by, bz + 1.0],
                    [bx, by, bz + 1.0],
                    [bx, by + hh, bz + 1.0 - f],
                ],
                uv(t1),
                c,
                light,
                true,
            );
        }
        if flammable(r.block(x, y + 1, z)) {
            let top = by + 1.0;
            quad(
                out,
                [
                    [bx, top, bz],
                    [bx + 1.0, top - 0.2, bz],
                    [bx + 1.0, top - 0.2, bz + 1.0],
                    [bx, top, bz + 1.0],
                ],
                uv(t0),
                c,
                light,
                true,
            );
        }
    }
}

/// `renderBlockTripWire`: thin strips towards connected tripwire / hooks.
fn tripwire(cx: &Ctx, out: &mut Vec<Vertex>, meta: i32, x: i32, y: i32, z: i32) {
    let r = &cx.r;
    let Some(t) = cx.rect(block::TRIPWIRE, 0, 0) else {
        return;
    };
    let light = light_bytes(r.bright(x, y, z));
    let h = if meta & 4 != 0 { 0.015625f32 } else { 0.0625 };
    let (bx, by, bz) = ((x - 16) as f32, y as f32 + h, (z - 16) as f32);
    let link = |dx: i32, dz: i32| {
        matches!(
            r.block(x + dx, y, z + dz),
            block::TRIPWIRE | block::TRIPWIRE_HOOK
        )
    };
    let (w, e, n, s) = (link(-1, 0), link(1, 0), link(0, -1), link(0, 1));
    let ew = w || e || !(n || s);
    let ns = n || s;
    let half = 0.03125f32;
    let (u0, u1) = (t[0], t[2]);
    let (v0, v1) = (t[1] + (t[3] - t[1]) * 0.375, t[1] + (t[3] - t[1]) * 0.5);
    let c = [255u8; 4];
    if ew {
        quad(
            out,
            [
                [bx, by, bz + 0.5 - half],
                [bx, by, bz + 0.5 + half],
                [bx + 1.0, by, bz + 0.5 + half],
                [bx + 1.0, by, bz + 0.5 - half],
            ],
            [[u0, v0], [u0, v1], [u1, v1], [u1, v0]],
            c,
            light,
            true,
        );
    }
    if ns {
        quad(
            out,
            [
                [bx + 0.5 - half, by, bz + 1.0],
                [bx + 0.5 + half, by, bz + 1.0],
                [bx + 0.5 + half, by, bz],
                [bx + 0.5 - half, by, bz],
            ],
            [[u0, v0], [u0, v1], [u1, v1], [u1, v0]],
            c,
            light,
            true,
        );
    }
}
