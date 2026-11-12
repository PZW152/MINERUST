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

pub struct ChunkMesh {
    pub cx: i32,
    pub cz: i32,
    pub opaque: Vec<Vertex>,
    pub translucent: Vec<Vertex>,
    pub max_y: f32,
}

/// One chunk of a mesh job's 3x3 neighbourhood.
#[derive(Clone)]
pub struct MeshChunk {
    pub data: Arc<ChunkData>,
    /// Existing 16-high sections (light of missing ones is implied).
    pub sections: u16,
    /// `heightMap[z << 4 | x]`
    pub height: Box<[i32; 256]>,
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
    fn new(chunks: &'a [MeshChunk; 9]) -> Self {
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
                    blocks[d..d + 256].copy_from_slice(&c.blocks[s..s + 256]);
                    meta[d..d + 256].copy_from_slice(&c.meta[s..s + 256]);
                    blk[d..d + 256].copy_from_slice(&c.blk[s..s + 256]);
                    sky[d..d + 256].copy_from_slice(&c.sky[s..s + 256]);
                    let h = mc.height[(z << 4) | x].clamp(0, 256) as usize;
                    for sec in 0..16usize {
                        if mc.sections & (1 << sec) == 0 {
                            // Chunk.getSavedLightValue for a missing section.
                            for y in (sec << 4)..((sec + 1) << 4) {
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

fn rgb(c: u32) -> [f32; 3] {
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

pub fn build(chunks: &[MeshChunk; 9], atlas: &Atlas, noise: &BiomeNoise) -> ChunkMesh {
    let cx = Ctx {
        r: Region::new(chunks),
        atlas,
        noise,
        all_faces: false,
    };
    let r = &cx.r;
    let c = &chunks[4].data;
    let mut opaque: Vec<Vertex> = Vec::new();
    let mut translucent: Vec<Vertex> = Vec::new();
    let mut max_y = 0f32;

    for lx in 0..16i32 {
        for lz in 0..16i32 {
            let (x, z) = (lx + 16, lz + 16);
            for y in 0..r.top as i32 {
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
                let before = out.len();
                mesh_block(&cx, out, id, meta, x, y, z);
                if out.len() != before {
                    max_y = max_y.max((y + 1) as f32);
                }
            }
        }
    }
    ChunkMesh {
        cx: c.cx,
        cz: c.cz,
        opaque,
        translucent,
        max_y,
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
        Shape::Cactus => boxed(
            &cx,
            out,
            id,
            meta,
            x,
            y,
            z,
            [0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            0.0625,
        ),
        Shape::SnowLayer => {
            let h = ((meta & 7) + 1) as f32 * 0.125;
            boxed(
                &cx,
                out,
                id,
                meta,
                x,
                y,
                z,
                [0.0, 0.0, 0.0, 1.0, h, 1.0],
                0.0,
            )
        }
        Shape::Slab => {
            let b = if meta & 8 != 0 {
                [0.0, 0.5, 0.0, 1.0, 1.0, 1.0]
            } else {
                [0.0, 0.0, 0.0, 1.0, 0.5, 1.0]
            };
            boxed(&cx, out, id, meta, x, y, z, b, 0.0)
        }
        Shape::Chest => boxed(
            &cx,
            out,
            id,
            meta,
            x,
            y,
            z,
            [0.0625, 0.0, 0.0625, 0.9375, 0.875, 0.9375],
            0.0,
        ),
        Shape::Torch => torch(&cx, out, id, meta, x, y, z),
        Shape::Stairs | Shape::Fence | Shape::Pane | Shape::Door | Shape::Boxes => {
            shaped(&cx, out, id, meta, x, y, z)
        }
        Shape::Ladder => ladder(&cx, out, meta, x, y, z),
        Shape::Rail => rail(&cx, out, id, meta, x, y, z),
        Shape::Crops => crops(&cx, out, id, meta, x, y, z),
        Shape::Wire => wire(&cx, out, meta, x, y, z),
        Shape::Fire => fire(&cx, out, x, y, z),
        Shape::Tripwire => tripwire(&cx, out, meta, x, y, z),
        Shape::Cocoa | Shape::None => {}
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
    let mut cx = Ctx {
        r: Region::new(chunks),
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
    cx.r.blocks[idx(x, y, z)] = block::PISTON_EXTENSION;
    cx.r.meta[idx(x, y, z)] = meta as u8;
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
            block::ICE | block::GLASS => nid != id && !block::info(nid).opaque,
            _ => !block::info(nid).opaque,
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
        if on_boundary && block::info(nid).opaque && !cx.all_faces {
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

/// `renderBlockTorch` / `renderTorchAtAngle`
fn torch(cx: &Ctx, out: &mut Vec<Vertex>, id: u8, meta: i32, x: i32, y: i32, z: i32) {
    let r = &cx.r;
    let Some(t) = cx.rect(id, 0, 0) else { return };
    let light = light_bytes(r.bright(x, y, z));
    let color = [255u8; 4];
    let (bx, by, bz) = ((x - 16) as f64, y as f64, (z - 16) as f64);
    let (d7, d9, d11) = (0.4f32 as f64, 0.5 - 0.4f32 as f64, 0.2f32 as f64);
    let (mut px, py, mut pz, dx, dz) = match meta {
        1 => (bx - d9, by + d11, bz, -d7, 0.0),
        2 => (bx + d9, by + d11, bz, d7, 0.0),
        3 => (bx, by + d11, bz - d9, 0.0, -d7),
        4 => (bx, by + d11, bz + d9, 0.0, d7),
        _ => (bx, by, bz, 0.0, 0.0),
    };
    let u = |p: f32| t[0] + (t[2] - t[0]) * p / 16.0;
    let v = |p: f32| t[1] + (t[3] - t[1]) * p / 16.0;
    let (u0, v0, u1, v1) = (t[0], t[1], t[2], t[3]);
    px += 0.5;
    pz += 0.5;
    let (x0, x1, z0, z1) = (px - 0.5, px + 0.5, pz - 0.5, pz + 0.5);
    let e = 0.0625f64;
    let h = 0.625f64;
    let vt = |x: f64, y: f64, z: f64, uu: f32, vv: f32| Vertex {
        pos: [x as f32, y as f32, z as f32],
        uv: [uu, vv],
        color,
        light,
    };
    let tx = px + dx * (1.0 - h);
    let tz = pz + dz * (1.0 - h);
    out.extend_from_slice(&[
        vt(tx - e, py + h, tz - e, u(7.0), v(6.0)),
        vt(tx - e, py + h, tz + e, u(7.0), v(8.0)),
        vt(tx + e, py + h, tz + e, u(9.0), v(8.0)),
        vt(tx + e, py + h, tz - e, u(9.0), v(6.0)),
        vt(px + e + dx, py, pz - e + dz, u(9.0), v(13.0)),
        vt(px + e + dx, py, pz + e + dz, u(9.0), v(15.0)),
        vt(px - e + dx, py, pz + e + dz, u(7.0), v(15.0)),
        vt(px - e + dx, py, pz - e + dz, u(7.0), v(13.0)),
        vt(px - e, py + 1.0, z0, u0, v0),
        vt(px - e + dx, py, z0 + dz, u0, v1),
        vt(px - e + dx, py, z1 + dz, u1, v1),
        vt(px - e, py + 1.0, z1, u1, v0),
        vt(px + e, py + 1.0, z1, u0, v0),
        vt(px + dx + e, py, z1 + dz, u0, v1),
        vt(px + dx + e, py, z0 + dz, u1, v1),
        vt(px + e, py + 1.0, z0, u1, v0),
        vt(x0, py + 1.0, pz + e, u0, v0),
        vt(x0 + dx, py, pz + e + dz, u0, v1),
        vt(x1 + dx, py, pz + e + dz, u1, v1),
        vt(x1, py + 1.0, pz + e, u1, v0),
        vt(x1, py + 1.0, pz - e, u0, v0),
        vt(x1 + dx, py, pz - e + dz, u0, v1),
        vt(x0 + dx, py, pz - e + dz, u1, v1),
        vt(x0, py + 1.0, pz - e, u1, v0),
    ]);
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
            !block::info(nid).opaque
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
            if on_boundary && block::info(r.block(qx, qy, qz)).opaque && !cx.all_faces {
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
                let (mut u, v) = match f {
                    0 | 1 => (pos[0], pos[2]),
                    2 => (1.0 - pos[0], 1.0 - pos[1]),
                    3 => (pos[0], 1.0 - pos[1]),
                    4 => (pos[2], 1.0 - pos[1]),
                    _ => (1.0 - pos[2], 1.0 - pos[1]),
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
            boxes(cx, out, x, y, z, &list, &|_, f| plain(f), white);
        }
        Shape::Fence => {
            let list = shapes::fence_render(&get, x, y, z);
            boxes(cx, out, x, y, z, &list, &|_, f| plain(f), white);
        }
        Shape::Pane => {
            let list = shapes::pane(&get, id, x, y, z);
            let edge = cx.rect(id, meta, 0);
            let face = cx.rect(id, meta, 2);
            let tex = |bi: usize, f: usize| {
                let b = list[bi];
                let thin_x = b[3] - b[0] < 0.25;
                let main = if thin_x {
                    f == 4 || f == 5
                } else {
                    f == 2 || f == 3
                };
                if main { face } else { edge }.map(|t| (t, false))
            };
            boxes(cx, out, x, y, z, &list, &tex, white);
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
            block::END_PORTAL_FRAME => {
                let list = shapes::static_boxes(id, m);
                let tex = |bi: usize, f: usize| {
                    if bi == 0 {
                        plain(f)
                    } else {
                        cx.rect(id, 8, f).map(|t| (t, false))
                    }
                };
                boxes(cx, out, x, y, z, &list, &tex, white);
            }
            block::FLOWER_POT => flower_pot(cx, out, meta, x, y, z),
            block::LEVER => {
                // Cobblestone base and the handle (RenderBlocks.renderBlockLever).
                let k = m & 7;
                let on = m & 8 != 0;
                let base: shapes::Bx = match k {
                    1 => [0.0, 0.3125, 0.25, 0.1875, 0.6875, 0.75],
                    2 => [0.8125, 0.3125, 0.25, 1.0, 0.6875, 0.75],
                    3 => [0.25, 0.3125, 0.0, 0.75, 0.6875, 0.1875],
                    4 => [0.25, 0.3125, 0.8125, 0.75, 0.6875, 1.0],
                    5 | 6 => [0.25, 0.0, 0.3125, 0.75, 0.1875, 0.6875],
                    _ => [0.25, 0.8125, 0.3125, 0.75, 1.0, 0.6875],
                };
                boxes(
                    cx,
                    out,
                    x,
                    y,
                    z,
                    &[base],
                    &|_, f| cx.rect(block::COBBLESTONE, 0, f).map(|t| (t, false)),
                    white,
                );
                let lean = if on { 0.15 } else { -0.15 };
                let handle: shapes::Bx = match k {
                    1 => [0.1875, 0.4375 + lean, 0.4375, 0.625, 0.5625 + lean, 0.5625],
                    2 => [0.375, 0.4375 + lean, 0.4375, 0.8125, 0.5625 + lean, 0.5625],
                    3 => [0.4375, 0.4375 + lean, 0.1875, 0.5625, 0.5625 + lean, 0.625],
                    4 => [0.4375, 0.4375 + lean, 0.375, 0.5625, 0.5625 + lean, 0.8125],
                    5 | 6 => [
                        0.4375 + lean * 0.5,
                        0.1875,
                        0.4375,
                        0.5625 + lean * 0.5,
                        0.625,
                        0.5625,
                    ],
                    _ => [
                        0.4375 + lean * 0.5,
                        0.375,
                        0.4375,
                        0.5625 + lean * 0.5,
                        0.8125,
                        0.5625,
                    ],
                };
                boxes(cx, out, x, y, z, &[handle], &|_, f| plain(f), white);
            }
            block::UNPOWERED_REPEATER | block::POWERED_REPEATER => {
                let list = shapes::static_boxes(id, m);
                boxes(cx, out, x, y, z, &list, &|_, f| plain(f), white);
                // The two torches of RenderBlocks.renderBlockRepeater.
                let torch_id = if id == block::POWERED_REPEATER {
                    block::REDSTONE_TORCH
                } else {
                    block::UNLIT_REDSTONE_TORCH
                };
                let d = (m & 3) as usize;
                let delay = ((m >> 2) & 3) as f32;
                let back = [
                    (0.0f32, 0.3125f32),
                    (-0.3125, 0.0),
                    (0.0, -0.3125),
                    (0.3125, 0.0),
                ][d];
                let fo = -0.0625 + delay * 0.125;
                let front = [(0.0f32, -fo), (fo, 0.0), (0.0, fo), (-fo, 0.0)][d];
                for (ox, oz) in [back, front] {
                    small_torch(cx, out, torch_id, x, y, z, ox, -0.1875, oz);
                }
            }
            _ => {
                let list = shapes::static_boxes(id, m);
                boxes(cx, out, x, y, z, &list, &|_, f| plain(f), white);
            }
        },
    }
}

/// `renderBlockCauldron`
fn cauldron(cx: &Ctx, out: &mut Vec<Vertex>, meta: i32, x: i32, y: i32, z: i32) {
    let r = &cx.r;
    let plain = |f: usize| cx.rect(block::CAULDRON, 0, f).map(|t| (t, false));
    boxes(
        cx,
        out,
        x,
        y,
        z,
        &[[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]],
        &|_, f| plain(f),
        [1.0; 3],
    );
    let Some(inner) = cx.rect(block::CAULDRON, 8, 2) else {
        return;
    };
    let (lx, lz) = ((x - 16) as f32, (z - 16) as f32);
    let fy = y as f32;
    let light = light_bytes(r.bright(x, y, z));
    let e = 0.125f32;
    let (u0, v0, u1, v1) = (inner[0], inner[1], inner[2], inner[3]);
    let uv = [[u0, v0], [u0, v1], [u1, v1], [u1, v0]];
    let side_x = col(0.6, 0.6, 0.6);
    let side_z = col(0.8, 0.8, 0.8);
    quad(
        out,
        [
            [lx + e, fy + 1.0, lz],
            [lx + e, fy, lz],
            [lx + e, fy, lz + 1.0],
            [lx + e, fy + 1.0, lz + 1.0],
        ],
        uv,
        side_x,
        light,
        false,
    );
    quad(
        out,
        [
            [lx + 1.0 - e, fy + 1.0, lz + 1.0],
            [lx + 1.0 - e, fy, lz + 1.0],
            [lx + 1.0 - e, fy, lz],
            [lx + 1.0 - e, fy + 1.0, lz],
        ],
        uv,
        side_x,
        light,
        false,
    );
    quad(
        out,
        [
            [lx + 1.0, fy + 1.0, lz + e],
            [lx + 1.0, fy, lz + e],
            [lx, fy, lz + e],
            [lx, fy + 1.0, lz + e],
        ],
        uv,
        side_z,
        light,
        false,
    );
    quad(
        out,
        [
            [lx, fy + 1.0, lz + 1.0 - e],
            [lx, fy, lz + 1.0 - e],
            [lx + 1.0, fy, lz + 1.0 - e],
            [lx + 1.0, fy + 1.0, lz + 1.0 - e],
        ],
        uv,
        side_z,
        light,
        false,
    );
    let fl = 0.25f32;
    quad(
        out,
        [
            [lx, fy + fl, lz],
            [lx, fy + fl, lz + 1.0],
            [lx + 1.0, fy + fl, lz + 1.0],
            [lx + 1.0, fy + fl, lz],
        ],
        uv,
        [255; 4],
        light,
        false,
    );
    if meta > 0 {
        if let Some(w) = cx.rect(block::WATER, 0, 1) {
            let h = (6 + 3 * meta.min(3)) as f32 / 16.0;
            quad(
                out,
                [
                    [lx, fy + h, lz],
                    [lx, fy + h, lz + 1.0],
                    [lx + 1.0, fy + h, lz + 1.0],
                    [lx + 1.0, fy + h, lz],
                ],
                [[w[0], w[1]], [w[0], w[3]], [w[2], w[3]], [w[2], w[1]]],
                [255; 4],
                light,
                false,
            );
        }
    }
}

/// `renderBlockFlowerpot`
fn flower_pot(cx: &Ctx, out: &mut Vec<Vertex>, meta: i32, x: i32, y: i32, z: i32) {
    let r = &cx.r;
    let list = crate::shapes::static_boxes(block::FLOWER_POT, meta as u8);
    boxes(
        cx,
        out,
        x,
        y,
        z,
        &list,
        &|_, f| {
            if f == 1 {
                None
            } else {
                cx.rect(block::FLOWER_POT, 0, f).map(|t| (t, false))
            }
        },
        [1.0; 3],
    );
    let light = light_bytes(r.bright(x, y, z));
    let (lx, lz) = ((x - 16) as f32, (z - 16) as f32);
    let fy = y as f32;
    if let Some(d) = cx.rect(block::FLOWER_POT, 15, 1) {
        let (a, b, h) = (0.375f32, 0.625f32, 0.25f32);
        quad(
            out,
            [
                [lx + a, fy + h, lz + a],
                [lx + a, fy + h, lz + b],
                [lx + b, fy + h, lz + b],
                [lx + b, fy + h, lz + a],
            ],
            [[d[0], d[1]], [d[0], d[3]], [d[2], d[3]], [d[2], d[1]]],
            [255; 4],
            light,
            false,
        );
        // The rim.
        if let Some(p) = cx.rect(block::FLOWER_POT, 0, 1) {
            let (o0, o1, i0, i1, hy) = (0.3125f32, 0.6875f32, 0.375f32, 0.625f32, 0.375f32);
            let uvq = |x0: f32, z0: f32, x1: f32, z1: f32| {
                let u = |a: f32| p[0] + (p[2] - p[0]) * a;
                let v = |a: f32| p[1] + (p[3] - p[1]) * a;
                [
                    [u(x0), v(z0)],
                    [u(x0), v(z1)],
                    [u(x1), v(z1)],
                    [u(x1), v(z0)],
                ]
            };
            for (x0, z0, x1, z1) in [
                (o0, o0, o1, i0),
                (o0, i1, o1, o1),
                (o0, i0, i0, i1),
                (i1, i0, o1, i1),
            ] {
                quad(
                    out,
                    [
                        [lx + x0, fy + hy, lz + z0],
                        [lx + x0, fy + hy, lz + z1],
                        [lx + x1, fy + hy, lz + z1],
                        [lx + x1, fy + hy, lz + z0],
                    ],
                    uvq(x0, z0, x1, z1),
                    [255; 4],
                    light,
                    false,
                );
            }
        }
    }
    // TileEntityFlowerPot contents by metadata (BlockFlowerPot.createNewTileEntity).
    let plant = match meta {
        1 => Some((block::RED_FLOWER, 0)),
        2 => Some((block::YELLOW_FLOWER, 0)),
        3..=6 => Some((block::SAPLING, meta - 3)),
        7 => Some((block::RED_MUSHROOM, 0)),
        8 => Some((block::BROWN_MUSHROOM, 0)),
        10 => Some((block::DEADBUSH, 0)),
        11 => Some((block::TALLGRASS, 2)),
        12 => Some((block::SAPLING, 4)),
        13 => Some((block::SAPLING, 5)),
        _ => None,
    };
    if let Some((pid, pm)) = plant {
        if let Some(t) = cx.rect(pid, pm, 0) {
            let tint = if pid == block::TALLGRASS {
                cx.grass_color_at(x, y, z)
            } else {
                [1.0; 3]
            };
            let s = 0.75f32;
            let d = 0.45 * s;
            let (cx0, cz0) = (lx + 0.5, lz + 0.5);
            let (y0, y1) = (fy + 0.25, fy + 0.25 + s);
            let uv = [[t[0], t[1]], [t[0], t[3]], [t[2], t[3]], [t[2], t[1]]];
            let c = col(tint[0], tint[1], tint[2]);
            quad(
                out,
                [
                    [cx0 - d, y1, cz0 - d],
                    [cx0 - d, y0, cz0 - d],
                    [cx0 + d, y0, cz0 + d],
                    [cx0 + d, y1, cz0 + d],
                ],
                uv,
                c,
                light,
                true,
            );
            quad(
                out,
                [
                    [cx0 - d, y1, cz0 + d],
                    [cx0 - d, y0, cz0 + d],
                    [cx0 + d, y0, cz0 - d],
                    [cx0 + d, y1, cz0 - d],
                ],
                uv,
                c,
                light,
                true,
            );
        }
    } else if meta == 9 {
        let cactus = [[0.375f32, 0.25, 0.375, 0.625, 1.0, 0.625]];
        boxes(
            cx,
            out,
            x,
            y,
            z,
            &cactus,
            &|_, f| cx.rect(block::CACTUS, 0, f.max(2)).map(|t| (t, false)),
            [1.0; 3],
        );
    }
}

/// A standing torch displaced inside the cell (repeater torches).
#[allow(clippy::too_many_arguments)]
fn small_torch(
    cx: &Ctx,
    out: &mut Vec<Vertex>,
    id: u8,
    x: i32,
    y: i32,
    z: i32,
    ox: f32,
    oy: f32,
    oz: f32,
) {
    let Some(t) = cx.rect(id, 0, 0) else { return };
    let light = light_bytes(cx.r.bright(x, y, z));
    let (px, py, pz) = (
        (x - 16) as f32 + 0.5 + ox,
        y as f32 + oy,
        (z - 16) as f32 + 0.5 + oz,
    );
    let e = 0.0625f32;
    let uv = [[t[0], t[1]], [t[0], t[3]], [t[2], t[3]], [t[2], t[1]]];
    let c = [255u8; 4];
    quad(
        out,
        [
            [px - e, py + 1.0, pz - 0.5],
            [px - e, py, pz - 0.5],
            [px - e, py, pz + 0.5],
            [px - e, py + 1.0, pz + 0.5],
        ],
        uv,
        c,
        light,
        false,
    );
    quad(
        out,
        [
            [px + e, py + 1.0, pz + 0.5],
            [px + e, py, pz + 0.5],
            [px + e, py, pz - 0.5],
            [px + e, py + 1.0, pz - 0.5],
        ],
        uv,
        c,
        light,
        false,
    );
    quad(
        out,
        [
            [px - 0.5, py + 1.0, pz + e],
            [px - 0.5, py, pz + e],
            [px + 0.5, py, pz + e],
            [px + 0.5, py + 1.0, pz + e],
        ],
        uv,
        c,
        light,
        false,
    );
    quad(
        out,
        [
            [px + 0.5, py + 1.0, pz - e],
            [px + 0.5, py, pz - e],
            [px - 0.5, py, pz - e],
            [px - 0.5, py + 1.0, pz - e],
        ],
        uv,
        c,
        light,
        false,
    );
}

/// `renderBlockLadder`
fn ladder(cx: &Ctx, out: &mut Vec<Vertex>, meta: i32, x: i32, y: i32, z: i32) {
    let Some(t) = cx.rect(block::LADDER, 0, 0) else {
        return;
    };
    let light = light_bytes(cx.r.bright(x, y, z));
    let (lx, ly, lz) = ((x - 16) as f32, y as f32, (z - 16) as f32);
    let f = 0.05f32;
    let (u0, v0, u1, v1) = (t[0], t[1], t[2], t[3]);
    let p = match meta {
        5 => [
            [lx + f, ly + 1.0, lz + 1.0],
            [lx + f, ly, lz + 1.0],
            [lx + f, ly, lz],
            [lx + f, ly + 1.0, lz],
        ],
        4 => [
            [lx + 1.0 - f, ly, lz + 1.0],
            [lx + 1.0 - f, ly + 1.0, lz + 1.0],
            [lx + 1.0 - f, ly + 1.0, lz],
            [lx + 1.0 - f, ly, lz],
        ],
        3 => [
            [lx + 1.0, ly, lz + f],
            [lx + 1.0, ly + 1.0, lz + f],
            [lx, ly + 1.0, lz + f],
            [lx, ly, lz + f],
        ],
        _ => [
            [lx + 1.0, ly + 1.0, lz + 1.0 - f],
            [lx + 1.0, ly, lz + 1.0 - f],
            [lx, ly, lz + 1.0 - f],
            [lx, ly + 1.0, lz + 1.0 - f],
        ],
    };
    let uv = match meta {
        5 | 2 => [[u0, v0], [u0, v1], [u1, v1], [u1, v0]],
        _ => [[u1, v1], [u1, v0], [u0, v0], [u0, v1]],
    };
    quad(out, p, uv, [255; 4], light, true);
}

/// `renderBlockMinecartTrack`
fn rail(cx: &Ctx, out: &mut Vec<Vertex>, id: u8, meta: i32, x: i32, y: i32, z: i32) {
    let Some(t) = cx.rect(id, meta, 0) else {
        return;
    };
    let light = light_bytes(cx.r.bright(x, y, z));
    let l = if id == block::RAIL { meta } else { meta & 7 };
    let (bx, by, bz) = ((x - 16) as f32, y as f32, (z - 16) as f32);
    let (d0, d1, d2, d3) = (t[0], t[1], t[2], t[3]);
    let e = 0.0625f32;
    let (mut f1, mut f2, mut f3, mut f4) = (bx + 1.0, bx + 1.0, bx, bx);
    let (mut f5, mut f6, mut f7, mut f8) = (bz, bz + 1.0, bz + 1.0, bz);
    let (mut f9, mut f10, mut f11, mut f12) = (by + e, by + e, by + e, by + e);
    if l != 1 && l != 2 && l != 3 && l != 7 {
        if l == 8 {
            f1 = bx;
            f2 = bx;
            f3 = bx + 1.0;
            f4 = bx + 1.0;
            f5 = bz + 1.0;
            f8 = bz + 1.0;
            f6 = bz;
            f7 = bz;
        } else if l == 9 {
            f1 = bx;
            f4 = bx;
            f2 = bx + 1.0;
            f3 = bx + 1.0;
            f5 = bz;
            f6 = bz;
            f7 = bz + 1.0;
            f8 = bz + 1.0;
        }
    } else {
        f1 = bx + 1.0;
        f4 = bx + 1.0;
        f2 = bx;
        f3 = bx;
        f5 = bz + 1.0;
        f6 = bz + 1.0;
        f7 = bz;
        f8 = bz;
    }
    if l == 2 || l == 4 {
        f9 += 1.0;
        f12 += 1.0;
    } else if l == 3 || l == 5 {
        f10 += 1.0;
        f11 += 1.0;
    }
    quad(
        out,
        [[f1, f9, f5], [f2, f10, f6], [f3, f11, f7], [f4, f12, f8]],
        [[d2, d1], [d2, d3], [d0, d3], [d0, d1]],
        [255; 4],
        light,
        true,
    );
}

/// `renderBlockCrops` / `renderBlockCropsImpl`
fn crops(cx: &Ctx, out: &mut Vec<Vertex>, id: u8, meta: i32, x: i32, y: i32, z: i32) {
    let Some(t) = cx.rect(id, meta, 0) else {
        return;
    };
    let light = light_bytes(cx.r.bright(x, y, z));
    let (bx, by, bz) = ((x - 16) as f32, y as f32 - 0.0625, (z - 16) as f32);
    let uv = [[t[0], t[1]], [t[0], t[3]], [t[2], t[3]], [t[2], t[1]]];
    let c = [255u8; 4];
    for o in [0.25f32, 0.75] {
        quad(
            out,
            [
                [bx + o, by + 1.0, bz],
                [bx + o, by, bz],
                [bx + o, by, bz + 1.0],
                [bx + o, by + 1.0, bz + 1.0],
            ],
            uv,
            c,
            light,
            true,
        );
        quad(
            out,
            [
                [bx, by + 1.0, bz + o],
                [bx, by, bz + o],
                [bx + 1.0, by, bz + o],
                [bx + 1.0, by + 1.0, bz + o],
            ],
            uv,
            c,
            light,
            true,
        );
    }
}

/// `BlockRedstoneWire.isPowerProviderOrWire` (without the repeater facing test).
fn wire_link(id: u8) -> bool {
    matches!(
        id,
        block::REDSTONE_WIRE
            | block::REDSTONE_TORCH
            | block::UNLIT_REDSTONE_TORCH
            | block::LEVER
            | block::STONE_BUTTON
            | block::WOODEN_BUTTON
            | block::STONE_PRESSURE_PLATE
            | block::WOODEN_PRESSURE_PLATE
            | block::UNPOWERED_REPEATER
            | block::POWERED_REPEATER
            | block::TRIPWIRE_HOOK
            | block::REDSTONE_BLOCK
            | block::DETECTOR_RAIL
    )
}

/// `renderBlockRedstoneWire` (flat part).
fn wire(cx: &Ctx, out: &mut Vec<Vertex>, meta: i32, x: i32, y: i32, z: i32) {
    let r = &cx.r;
    let light = light_bytes(r.bright(x, y, z));
    let link = |dx: i32, dz: i32| {
        let n = r.block(x + dx, y, z + dz);
        wire_link(n)
            || (!block::info(n).normal_cube()
                && r.block(x + dx, y - 1, z + dz) == block::REDSTONE_WIRE)
            || (!block::info(r.block(x, y + 1, z)).normal_cube()
                && r.block(x + dx, y + 1, z + dz) == block::REDSTONE_WIRE)
    };
    let (w, e, n, s) = (link(-1, 0), link(1, 0), link(0, -1), link(0, 1));
    // BlockRedstoneWire.colorMultiplier
    let f = meta as f32 / 15.0;
    let mut red = f * 0.6 + 0.4;
    if meta == 0 {
        red = 0.3;
    }
    let green = (f * f * 0.7 - 0.5).max(0.0);
    let blue = (f * f * 0.6 - 0.7).max(0.0);
    let c = col(red, green, blue);
    let (bx, by, bz) = ((x - 16) as f32, y as f32 + 0.015625, (z - 16) as f32);
    let line_ew = (w || e) && !n && !s;
    let line_ns = (n || s) && !w && !e;
    if line_ew || line_ns {
        let (Some(t), Some(o)) = (
            cx.rect(block::REDSTONE_WIRE, 0, 1),
            cx.rect(block::REDSTONE_WIRE, 0, 3),
        ) else {
            return;
        };
        for (tex, color) in [(t, c), (o, [255u8; 4])] {
            let (u0, v0, u1, v1) = (tex[0], tex[1], tex[2], tex[3]);
            let p = [
                [bx + 1.0, by, bz + 1.0],
                [bx + 1.0, by, bz],
                [bx, by, bz],
                [bx, by, bz + 1.0],
            ];
            let uv = if line_ns {
                [[u1, v1], [u1, v0], [u0, v0], [u0, v1]]
            } else {
                [[u1, v0], [u0, v0], [u0, v1], [u1, v1]]
            };
            quad(out, p, uv, color, light, false);
        }
        return;
    }
    let (Some(t), Some(o)) = (
        cx.rect(block::REDSTONE_WIRE, 0, 0),
        cx.rect(block::REDSTONE_WIRE, 0, 2),
    ) else {
        return;
    };
    let any = w || e || n || s;
    let x0 = if w || !any { 0.0 } else { 0.3125 };
    let x1 = if e || !any { 1.0 } else { 0.6875 };
    let z0 = if n || !any { 0.0 } else { 0.3125 };
    let z1 = if s || !any { 1.0 } else { 0.6875 };
    for (tex, color) in [(t, c), (o, [255u8; 4])] {
        let u = |a: f32| tex[0] + (tex[2] - tex[0]) * a;
        let v = |a: f32| tex[1] + (tex[3] - tex[1]) * a;
        quad(
            out,
            [
                [bx + x1, by, bz + z1],
                [bx + x1, by, bz + z0],
                [bx + x0, by, bz + z0],
                [bx + x0, by, bz + z1],
            ],
            [
                [u(x1), v(z1)],
                [u(x1), v(z0)],
                [u(x0), v(z0)],
                [u(x0), v(z1)],
            ],
            color,
            light,
            false,
        );
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
