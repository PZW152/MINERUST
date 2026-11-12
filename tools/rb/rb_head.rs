//! A literal port of the parts of `RenderBlocks` that draw standard
//! blocks: `renderStandardBlock` with its smooth-lighting variants and the
//! six `renderFace*` methods, plus the piston renderers. Field and local
//! names follow the decompiled source so the two compare line by line.

#![allow(non_snake_case)]

use super::{light_bytes, Ctx, Vertex};
use crate::block::{self, Material};

/// `RenderBlocks.fancyGrass`
const FANCY_GRASS: bool = true;
/// `GameSettings.fancyGraphics` (default on).
pub const FANCY_GRAPHICS: bool = true;
/// `GameSettings.ambientOcclusion` (default 2, "Maximum").
pub const AMBIENT_OCCLUSION: i32 = 2;

/// `IIcon` (a `TextureAtlasSprite`'s UV rectangle).
#[derive(Clone, Copy, Debug)]
pub struct Icon {
    pub r: [f32; 4],
    /// `getIconName().equals("grass_side")`
    pub grass_side: bool,
}

impl Icon {
    pub fn new(r: [f32; 4]) -> Icon {
        Icon {
            r,
            grass_side: false,
        }
    }
    /// `getInterpolatedU`
    pub fn u(&self, d: f64) -> f64 {
        let f = self.r[2] - self.r[0];
        (self.r[0] + f * d as f32 / 16.0) as f64
    }
    /// `getInterpolatedV`
    pub fn v(&self, d: f64) -> f64 {
        let f = self.r[3] - self.r[1];
        (self.r[1] + f * d as f32 / 16.0) as f64
    }
    pub fn min_u(&self) -> f64 {
        self.r[0] as f64
    }
    pub fn max_u(&self) -> f64 {
        self.r[2] as f64
    }
    pub fn min_v(&self) -> f64 {
        self.r[1] as f64
    }
    pub fn max_v(&self) -> f64 {
        self.r[3] as f64
    }
}

/// `RenderBlocks` state while drawing one block.
pub struct Rb<'a, 'b> {
    /// The world around the block; `None` for entity renderers.
    pub cx: Option<&'a Ctx<'b>>,
    /// Subtracted from vertex positions (region to chunk-local).
    pub origin: [f64; 3],
    /// `Tessellator` colour and brightness when there is no world.
    pub atlas_only: Option<&'a super::Atlas>,
    pub out: &'a mut Vec<Vertex>,
    /// The block being drawn (`var1`).
    pub id: u8,
    pub meta: i32,
    pub overrideBlockTexture: Option<Icon>,
    pub renderAllFaces: bool,
    pub renderFromInside: bool,
    pub flipTexture: bool,
    /// `BlockPistonExtension.sprite` (`func_150086_a`)
    pub head_sprite: Option<Icon>,
    pub field_152631_f: bool,
    pub renderMinX: f64,
    pub renderMaxX: f64,
    pub renderMinY: f64,
    pub renderMaxY: f64,
    pub renderMinZ: f64,
    pub renderMaxZ: f64,
    pub partialRenderBounds: bool,
    pub uvRotateEast: i32,
    pub uvRotateWest: i32,
    pub uvRotateSouth: i32,
    pub uvRotateNorth: i32,
    pub uvRotateTop: i32,
    pub uvRotateBottom: i32,
    pub enableAO: bool,
    aoLightValueScratchXYZNNN: f32,
    aoLightValueScratchXYNN: f32,
    aoLightValueScratchXYZNNP: f32,
    aoLightValueScratchYZNN: f32,
    aoLightValueScratchYZNP: f32,
    aoLightValueScratchXYZPNN: f32,
    aoLightValueScratchXYPN: f32,
    aoLightValueScratchXYZPNP: f32,
    aoLightValueScratchXYZNPN: f32,
    aoLightValueScratchXYNP: f32,
    aoLightValueScratchXYZNPP: f32,
    aoLightValueScratchYZPN: f32,
    aoLightValueScratchXYZPPN: f32,
    aoLightValueScratchXYPP: f32,
    aoLightValueScratchYZPP: f32,
    aoLightValueScratchXYZPPP: f32,
    aoLightValueScratchXZNN: f32,
    aoLightValueScratchXZPN: f32,
    aoLightValueScratchXZNP: f32,
    aoLightValueScratchXZPP: f32,
    aoBrightnessXYZNNN: i32,
    aoBrightnessXYNN: i32,
    aoBrightnessXYZNNP: i32,
    aoBrightnessYZNN: i32,
    aoBrightnessYZNP: i32,
    aoBrightnessXYZPNN: i32,
    aoBrightnessXYPN: i32,
    aoBrightnessXYZPNP: i32,
    aoBrightnessXYZNPN: i32,
    aoBrightnessXYNP: i32,
    aoBrightnessXYZNPP: i32,
    aoBrightnessYZPN: i32,
    aoBrightnessXYZPPN: i32,
    aoBrightnessXYPP: i32,
    aoBrightnessYZPP: i32,
    aoBrightnessXYZPPP: i32,
    aoBrightnessXZNN: i32,
    aoBrightnessXZPN: i32,
    aoBrightnessXZNP: i32,
    aoBrightnessXZPP: i32,
    brightnessTopLeft: i32,
    brightnessBottomLeft: i32,
    brightnessBottomRight: i32,
    brightnessTopRight: i32,
    colorRedTopLeft: f32,
    colorRedBottomLeft: f32,
    colorRedBottomRight: f32,
    colorRedTopRight: f32,
    colorGreenTopLeft: f32,
    colorGreenBottomLeft: f32,
    colorGreenBottomRight: f32,
    colorGreenTopRight: f32,
    colorBlueTopLeft: f32,
    colorBlueBottomLeft: f32,
    colorBlueBottomRight: f32,
    colorBlueTopRight: f32,
    /// `Tessellator` colour and brightness.
    tcolor: [u8; 4],
    pub bright: i32,
}

impl<'a, 'b> Rb<'a, 'b> {
    pub fn new(cx: &'a Ctx<'b>, out: &'a mut Vec<Vertex>, id: u8, meta: i32) -> Self {
        let mut r = Self::bare(cx.atlas, out, id, meta);
        r.cx = Some(cx);
        r.renderAllFaces = cx.all_faces;
        r.origin = [16.0, 0.0, 16.0];
        r
    }

    /// A `RenderBlocks` with no `blockAccess` (entity renderers).
    pub fn bare(atlas: &'a super::Atlas, out: &'a mut Vec<Vertex>, id: u8, meta: i32) -> Self {
        Rb {
            cx: None,
            origin: [0.0; 3],
            atlas_only: Some(atlas),
            out,
            id,
            meta,
            overrideBlockTexture: None,
            renderAllFaces: false,
            renderFromInside: false,
            flipTexture: false,
            head_sprite: None,
            field_152631_f: false,
            renderMinX: 0.0,
            renderMaxX: 1.0,
            renderMinY: 0.0,
            renderMaxY: 1.0,
            renderMinZ: 0.0,
            renderMaxZ: 1.0,
            partialRenderBounds: false,
            uvRotateEast: 0,
            uvRotateWest: 0,
            uvRotateSouth: 0,
            uvRotateNorth: 0,
            uvRotateTop: 0,
            uvRotateBottom: 0,
            enableAO: false,
            aoLightValueScratchXYZNNN: 0.0,
            aoLightValueScratchXYNN: 0.0,
            aoLightValueScratchXYZNNP: 0.0,
            aoLightValueScratchYZNN: 0.0,
            aoLightValueScratchYZNP: 0.0,
            aoLightValueScratchXYZPNN: 0.0,
            aoLightValueScratchXYPN: 0.0,
            aoLightValueScratchXYZPNP: 0.0,
            aoLightValueScratchXYZNPN: 0.0,
            aoLightValueScratchXYNP: 0.0,
            aoLightValueScratchXYZNPP: 0.0,
            aoLightValueScratchYZPN: 0.0,
            aoLightValueScratchXYZPPN: 0.0,
            aoLightValueScratchXYPP: 0.0,
            aoLightValueScratchYZPP: 0.0,
            aoLightValueScratchXYZPPP: 0.0,
            aoLightValueScratchXZNN: 0.0,
            aoLightValueScratchXZPN: 0.0,
            aoLightValueScratchXZNP: 0.0,
            aoLightValueScratchXZPP: 0.0,
            aoBrightnessXYZNNN: 0,
            aoBrightnessXYNN: 0,
            aoBrightnessXYZNNP: 0,
            aoBrightnessYZNN: 0,
            aoBrightnessYZNP: 0,
            aoBrightnessXYZPNN: 0,
            aoBrightnessXYPN: 0,
            aoBrightnessXYZPNP: 0,
            aoBrightnessXYZNPN: 0,
            aoBrightnessXYNP: 0,
            aoBrightnessXYZNPP: 0,
            aoBrightnessYZPN: 0,
            aoBrightnessXYZPPN: 0,
            aoBrightnessXYPP: 0,
            aoBrightnessYZPP: 0,
            aoBrightnessXYZPPP: 0,
            aoBrightnessXZNN: 0,
            aoBrightnessXZPN: 0,
            aoBrightnessXZNP: 0,
            aoBrightnessXZPP: 0,
            brightnessTopLeft: 0,
            brightnessBottomLeft: 0,
            brightnessBottomRight: 0,
            brightnessTopRight: 0,
            colorRedTopLeft: 0.0,
            colorRedBottomLeft: 0.0,
            colorRedBottomRight: 0.0,
            colorRedTopRight: 0.0,
            colorGreenTopLeft: 0.0,
            colorGreenBottomLeft: 0.0,
            colorGreenBottomRight: 0.0,
            colorGreenTopRight: 0.0,
            colorBlueTopLeft: 0.0,
            colorBlueBottomLeft: 0.0,
            colorBlueBottomRight: 0.0,
            colorBlueTopRight: 0.0,
            tcolor: [255; 4],
            bright: 0,
        }
    }

    /// `setRenderBounds`
    pub fn set_render_bounds(&mut self, x0: f64, y0: f64, z0: f64, x1: f64, y1: f64, z1: f64) {
        self.renderMinX = x0;
        self.renderMaxX = x1;
        self.renderMinY = y0;
        self.renderMaxY = y1;
        self.renderMinZ = z0;
        self.renderMaxZ = z1;
        self.partialRenderBounds = AMBIENT_OCCLUSION >= 2
            && (x0 > 0.0 || x1 < 1.0 || y0 > 0.0 || y1 < 1.0 || z0 > 0.0 || z1 < 1.0);
    }

    /// `setRenderBoundsFromBlock` with `[minX, minY, minZ, maxX, maxY, maxZ]`.
    pub fn set_bounds(&mut self, b: [f32; 6]) {
        self.set_render_bounds(
            b[0] as f64,
            b[1] as f64,
            b[2] as f64,
            b[3] as f64,
            b[4] as f64,
            b[5] as f64,
        );
    }

    fn c(&self) -> &'a Ctx<'b> {
        self.cx.expect("RenderBlocks without a world")
    }

    fn atlas(&self) -> &'a super::Atlas {
        self.atlas_only.unwrap()
    }

    /// `Tessellator.setColorOpaque_F` + `setBrightness` for callers.
    pub fn set_color_brightness(&mut self, c: f32, bright: i32) {
        self.color(c, c, c);
        self.bright = bright;
    }

    /// `getBlockIconFromSideAndMetadata`
    pub fn side_icon(&self, side: i32) -> Icon {
        Icon::new(self.atlas().tile(self.id, self.meta as u8, side as usize).unwrap_or([0.0; 4]))
    }

    fn block_at(&self, x: i32, y: i32, z: i32) -> u8 {
        self.c().r.block(x, y, z)
    }

    /// `Block.getMixedBrightnessForBlock` of the block being drawn.
    fn mixed(&self, x: i32, y: i32, z: i32) -> i32 {
        self.c().r.bright(x, y, z)
    }

    /// `Block.getAmbientOcclusionLightValue`
    fn ao_light(&self, x: i32, y: i32, z: i32) -> f32 {
        self.c().r.ao(x, y, z)
    }

    fn can_block_grass(&self, x: i32, y: i32, z: i32) -> bool {
        self.c().r.can_block_grass(x, y, z)
    }

    fn is_opaque(&self, x: i32, y: i32, z: i32) -> bool {
        block::info(self.block_at(x, y, z)).opaque
    }

    fn meta_at(&self, x: i32, y: i32, z: i32) -> i32 {
        self.c().r.meta(x, y, z)
    }

    /// `Block.shouldSideBeRendered` with the overrides of the blocks drawn
    /// through `renderStandardBlock`. `(x, y, z)` is the neighbour.
    fn should_side(&self, x: i32, y: i32, z: i32, side: i32) -> bool {
        let n = self.block_at(x, y, z);
        match self.id {
            // BlockBreakable (glass, stained glass)
            block::GLASS | 95 => {
                const OX: [i32; 6] = [0, 0, 0, 0, -1, 1];
                const OY: [i32; 6] = [-1, 1, 0, 0, 0, 0];
                const OZ: [i32; 6] = [0, 0, -1, 1, 0, 0];
                let s = side as usize;
                if self.meta_at(x, y, z) != self.meta_at(x - OX[s], y - OY[s], z - OZ[s]) {
                    return true;
                }
                if n == self.id {
                    return false;
                }
                self.default_side(x, y, z, side)
            }
            // BlockIce passes the opposite side on to BlockBreakable.
            block::ICE => {
                if n == self.id {
                    false
                } else {
                    self.default_side(x, y, z, 1 - side)
                }
            }
            // BlockLeavesBase
            block::LEAVES | block::LEAVES2 => {
                if !FANCY_GRAPHICS && n == self.id {
                    false
                } else {
                    self.default_side(x, y, z, side)
                }
            }
            // BlockPortal.shouldSideBeRendered
            block::PORTAL => {
                let mut f = 0;
                if n == block::PORTAL {
                    f = self.meta_at(x, y, z) & 3;
                    if f == 0 {
                        return false;
                    }
                    if f == 2 && side != 5 && side != 4 {
                        return false;
                    }
                    if f == 1 && side != 3 && side != 2 {
                        return false;
                    }
                }
                let p = |dx: i32, dz: i32| self.block_at(x + dx, y, z + dz) == block::PORTAL;
                let xn = p(-1, 0) && !p(-2, 0);
                let xp = p(1, 0) && !p(2, 0);
                let zn = p(0, -1) && !p(0, -2);
                let zp = p(0, 1) && !p(0, 2);
                let ax = xn || xp || f == 1;
                let az = zn || zp || f == 2;
                (ax && side == 4) || (ax && side == 5) || (az && side == 2) || (az && side == 3)
            }
            block::SNOW_LAYER | block::CARPET if side == 1 => true,
            block::DRAGON_EGG => true,
            _ => self.default_side(x, y, z, side),
        }
    }

    /// `Block.shouldSideBeRendered`
    fn default_side(&self, x: i32, y: i32, z: i32, side: i32) -> bool {
        match side {
            0 if self.renderMinY > 0.0 => true,
            1 if self.renderMaxY < 1.0 => true,
            2 if self.renderMinZ > 0.0 => true,
            3 if self.renderMaxZ < 1.0 => true,
            4 if self.renderMinX > 0.0 => true,
            5 if self.renderMaxX < 1.0 => true,
            _ => !self.is_opaque(x, y, z),
        }
    }

    /// `getBlockIcon(block, world, x, y, z, side)`
    fn icon(&self, x: i32, y: i32, z: i32, side: i32) -> Icon {
        if let Some(o) = self.overrideBlockTexture {
            return o;
        }
        let atlas = self.atlas();
        if self.id == block::PISTON || self.id == block::STICKY_PISTON {
            // BlockPistonBase.getIcon (the block bounds are the render bounds)
            let o = self.meta & 7;
            let top = if self.id == block::STICKY_PISTON {
                "piston_top_sticky"
            } else {
                "piston_top_normal"
            };
            let name = if o > 5 {
                top
            } else if side == o {
                if self.meta & 8 == 0
                    && !(self.renderMinX > 0.0)
                    && !(self.renderMinY > 0.0)
                    && !(self.renderMinZ > 0.0)
                    && !(self.renderMaxX < 1.0)
                    && !(self.renderMaxY < 1.0)
                    && !(self.renderMaxZ < 1.0)
                {
                    top
                } else {
                    "piston_inner"
                }
            } else if side == (o ^ 1) {
                "piston_bottom"
            } else {
                "piston_side"
            };
            return Icon::new(atlas.named(name));
        }
        if self.id == block::PISTON_HEAD {
            // BlockPistonExtension.getIcon
            let o = self.meta & 7;
            if side == o {
                if let Some(s) = self.head_sprite {
                    return s;
                }
                return Icon::new(atlas.named(if self.meta & 8 != 0 {
                    "piston_top_sticky"
                } else {
                    "piston_top_normal"
                }));
            }
            return Icon::new(atlas.named(if o < 6 && side == (o ^ 1) {
                "piston_top_normal"
            } else {
                "piston_side"
            }));
        }
        if self.id == block::GRASS && side >= 2 {
            // BlockGrass.getIcon: snowed sides
            let m = block::material(self.block_at(x, y + 1, z));
            if m == Material::Snow || m == Material::CraftedSnow {
                return Icon::new(atlas.rects[atlas.grass_side_snowed as usize]);
            }
            let r = self
                .c()
                .rect(self.id, self.meta, side as usize)
                .unwrap_or([0.0; 4]);
            return Icon {
                r,
                grass_side: true,
            };
        }
        Icon::new(
            self.c()
                .rect(self.id, self.meta, side as usize)
                .unwrap_or([0.0; 4]),
        )
    }

    /// `getBlockIcon(block).getIconName().equals("grass_top")`
    fn is_grass_top(&self) -> bool {
        self.id == block::GRASS
    }

    /// `BlockGrass.getIconSideOverlay`
    fn grass_overlay(&self) -> Icon {
        Icon::new(self.atlas().rects[self.atlas().grass_overlay as usize])
    }

    /// `Tessellator.setColorOpaque_F`
    fn color(&mut self, r: f32, g: f32, b: f32) {
        let c = |v: f32| ((v * 255.0) as i32).clamp(0, 255) as u8;
        self.tcolor = [c(r), c(g), c(b), 255];
    }

    /// `Tessellator.addVertexWithUV` in region coordinates.
    fn vertex(&mut self, x: f64, y: f64, z: f64, u: f64, v: f64) {
        self.out.push(Vertex {
            pos: [
                (x - self.origin[0]) as f32,
                (y - self.origin[1]) as f32,
                (z - self.origin[2]) as f32,
            ],
            uv: [u as f32, v as f32],
            color: self.tcolor,
            light: light_bytes(self.bright),
        });
    }

    /// `RenderBlocks.getAoBrightness`
    fn getAoBrightness(&self, mut a: i32, mut b: i32, mut c: i32, d: i32) -> i32 {
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

    /// `RenderBlocks.mixAoBrightness`
    #[allow(clippy::too_many_arguments)]
    fn mixAoBrightness(&self, a: i32, b: i32, c: i32, d: i32, e: f64, f: f64, g: f64, h: f64) -> i32 {
        let hi = ((a >> 16 & 255) as f64 * e
            + (b >> 16 & 255) as f64 * f
            + (c >> 16 & 255) as f64 * g
            + (d >> 16 & 255) as f64 * h) as i32
            & 255;
        let lo = ((a & 255) as f64 * e
            + (b & 255) as f64 * f
            + (c & 255) as f64 * g
            + (d & 255) as f64 * h) as i32
            & 255;
        hi << 16 | lo
    }

    /// `Block.colorMultiplier` for the blocks with a tint.
    fn color_multiplier(&self, x: i32, y: i32, z: i32) -> [f32; 3] {
        match self.id {
            block::GRASS => self.c().grass_tint(x, y, z),
            block::LEAVES if self.meta & 3 == 1 => super::rgb(6396257),
            block::LEAVES if self.meta & 3 == 2 => super::rgb(8431445),
            block::LEAVES | block::LEAVES2 | block::VINE => self.c().foliage_tint(x, y, z),
            _ => [1.0; 3],
        }
    }

    /// `RenderBlocks.renderStandardBlock`
    pub fn render_standard(&mut self, x: i32, y: i32, z: i32) -> bool {
        let c = self.color_multiplier(x, y, z);
        if AMBIENT_OCCLUSION == 0 || block::info(self.id).light_value != 0 {
            self.std_flat(x, y, z, c[0], c[1], c[2])
        } else if self.partialRenderBounds {
            self.std_ao_partial(x, y, z, c[0], c[1], c[2])
        } else {
            self.std_ao(x, y, z, c[0], c[1], c[2])
        }
    }

