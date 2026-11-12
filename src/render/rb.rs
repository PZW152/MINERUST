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
        Icon::new(
            self.atlas()
                .tile(self.id, self.meta as u8, side as usize)
                .unwrap_or([0.0; 4]),
        )
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
    fn mixAoBrightness(
        &self,
        a: i32,
        b: i32,
        c: i32,
        d: i32,
        e: f64,
        f: f64,
        g: f64,
        h: f64,
    ) -> i32 {
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

    /// `RenderBlocks.renderStandardBlockWithAmbientOcclusion`
    #[allow(
        unused_assignments,
        unused_mut,
        unused_variables,
        unused_parens,
        clippy::all
    )]
    pub fn std_ao(
        &mut self,
        mut var2: i32,
        mut var3: i32,
        mut var4: i32,
        var5: f32,
        var6: f32,
        var7: f32,
    ) -> bool {
        self.enableAO = true;
        let mut var8: bool = false;
        let mut var9: f32 = 0.0f32;
        let mut var10: f32 = 0.0f32;
        let mut var11: f32 = 0.0f32;
        let mut var12: f32 = 0.0f32;
        let mut var13: bool = true;
        let mut var14: i32 = self.mixed(var2, var3, var4);
        self.bright = 983055;
        if self.is_grass_top() {
            var13 = false;
        } else if self.overrideBlockTexture.is_some() {
            var13 = false;
        }
        if self.renderAllFaces || self.should_side(var2, var3 - 1, var4, 0) {
            if self.renderMinY <= 0.0f64 {
                var3 -= 1;
            }
            self.aoBrightnessXYNN = self.mixed(var2 - 1, var3, var4);
            self.aoBrightnessYZNN = self.mixed(var2, var3, var4 - 1);
            self.aoBrightnessYZNP = self.mixed(var2, var3, var4 + 1);
            self.aoBrightnessXYPN = self.mixed(var2 + 1, var3, var4);
            self.aoLightValueScratchXYNN = self.ao_light(var2 - 1, var3, var4);
            self.aoLightValueScratchYZNN = self.ao_light(var2, var3, var4 - 1);
            self.aoLightValueScratchYZNP = self.ao_light(var2, var3, var4 + 1);
            self.aoLightValueScratchXYPN = self.ao_light(var2 + 1, var3, var4);
            let mut var16: bool = self.can_block_grass(var2 + 1, var3 - 1, var4);
            let mut var17: bool = self.can_block_grass(var2 - 1, var3 - 1, var4);
            let mut var18: bool = self.can_block_grass(var2, var3 - 1, var4 + 1);
            let mut var19: bool = self.can_block_grass(var2, var3 - 1, var4 - 1);
            if !var19 && !var17 {
                self.aoLightValueScratchXYZNNN = self.aoLightValueScratchXYNN;
                self.aoBrightnessXYZNNN = self.aoBrightnessXYNN;
            } else {
                self.aoLightValueScratchXYZNNN = self.ao_light(var2 - 1, var3, var4 - 1);
                self.aoBrightnessXYZNNN = self.mixed(var2 - 1, var3, var4 - 1);
            }
            if !var18 && !var17 {
                self.aoLightValueScratchXYZNNP = self.aoLightValueScratchXYNN;
                self.aoBrightnessXYZNNP = self.aoBrightnessXYNN;
            } else {
                self.aoLightValueScratchXYZNNP = self.ao_light(var2 - 1, var3, var4 + 1);
                self.aoBrightnessXYZNNP = self.mixed(var2 - 1, var3, var4 + 1);
            }
            if !var19 && !var16 {
                self.aoLightValueScratchXYZPNN = self.aoLightValueScratchXYPN;
                self.aoBrightnessXYZPNN = self.aoBrightnessXYPN;
            } else {
                self.aoLightValueScratchXYZPNN = self.ao_light(var2 + 1, var3, var4 - 1);
                self.aoBrightnessXYZPNN = self.mixed(var2 + 1, var3, var4 - 1);
            }
            if !var18 && !var16 {
                self.aoLightValueScratchXYZPNP = self.aoLightValueScratchXYPN;
                self.aoBrightnessXYZPNP = self.aoBrightnessXYPN;
            } else {
                self.aoLightValueScratchXYZPNP = self.ao_light(var2 + 1, var3, var4 + 1);
                self.aoBrightnessXYZPNP = self.mixed(var2 + 1, var3, var4 + 1);
            }
            if self.renderMinY <= 0.0f64 {
                var3 += 1;
            }
            let mut var20: i32 = var14;
            if self.renderMinY <= 0.0f64 || !self.is_opaque(var2, var3 - 1, var4) {
                var20 = self.mixed(var2, var3 - 1, var4);
            }
            let mut var21: f32 = self.ao_light(var2, var3 - 1, var4);
            var9 = (self.aoLightValueScratchXYZNNP
                + self.aoLightValueScratchXYNN
                + self.aoLightValueScratchYZNP
                + var21)
                / 4.0f32;
            var12 = (self.aoLightValueScratchYZNP
                + var21
                + self.aoLightValueScratchXYZPNP
                + self.aoLightValueScratchXYPN)
                / 4.0f32;
            var11 = (var21
                + self.aoLightValueScratchYZNN
                + self.aoLightValueScratchXYPN
                + self.aoLightValueScratchXYZPNN)
                / 4.0f32;
            var10 = (self.aoLightValueScratchXYNN
                + self.aoLightValueScratchXYZNNN
                + var21
                + self.aoLightValueScratchYZNN)
                / 4.0f32;
            self.brightnessTopLeft = self.getAoBrightness(
                self.aoBrightnessXYZNNP,
                self.aoBrightnessXYNN,
                self.aoBrightnessYZNP,
                var20,
            );
            self.brightnessTopRight = self.getAoBrightness(
                self.aoBrightnessYZNP,
                self.aoBrightnessXYZPNP,
                self.aoBrightnessXYPN,
                var20,
            );
            self.brightnessBottomRight = self.getAoBrightness(
                self.aoBrightnessYZNN,
                self.aoBrightnessXYPN,
                self.aoBrightnessXYZPNN,
                var20,
            );
            self.brightnessBottomLeft = self.getAoBrightness(
                self.aoBrightnessXYNN,
                self.aoBrightnessXYZNNN,
                self.aoBrightnessYZNN,
                var20,
            );
            if var13 {
                self.colorRedTopRight = var5 * 0.5f32;
                self.colorRedBottomRight = self.colorRedTopRight;
                self.colorRedBottomLeft = self.colorRedBottomRight;
                self.colorRedTopLeft = self.colorRedBottomLeft;
                self.colorGreenTopRight = var6 * 0.5f32;
                self.colorGreenBottomRight = self.colorGreenTopRight;
                self.colorGreenBottomLeft = self.colorGreenBottomRight;
                self.colorGreenTopLeft = self.colorGreenBottomLeft;
                self.colorBlueTopRight = var7 * 0.5f32;
                self.colorBlueBottomRight = self.colorBlueTopRight;
                self.colorBlueBottomLeft = self.colorBlueBottomRight;
                self.colorBlueTopLeft = self.colorBlueBottomLeft;
            } else {
                self.colorRedTopRight = 0.5f32;
                self.colorRedBottomRight = self.colorRedTopRight;
                self.colorRedBottomLeft = self.colorRedBottomRight;
                self.colorRedTopLeft = self.colorRedBottomLeft;
                self.colorGreenTopRight = 0.5f32;
                self.colorGreenBottomRight = self.colorGreenTopRight;
                self.colorGreenBottomLeft = self.colorGreenBottomRight;
                self.colorGreenTopLeft = self.colorGreenBottomLeft;
                self.colorBlueTopRight = 0.5f32;
                self.colorBlueBottomRight = self.colorBlueTopRight;
                self.colorBlueBottomLeft = self.colorBlueBottomRight;
                self.colorBlueTopLeft = self.colorBlueBottomLeft;
            }
            self.colorRedTopLeft = self.colorRedTopLeft * (var9);
            self.colorGreenTopLeft = self.colorGreenTopLeft * (var9);
            self.colorBlueTopLeft = self.colorBlueTopLeft * (var9);
            self.colorRedBottomLeft = self.colorRedBottomLeft * (var10);
            self.colorGreenBottomLeft = self.colorGreenBottomLeft * (var10);
            self.colorBlueBottomLeft = self.colorBlueBottomLeft * (var10);
            self.colorRedBottomRight = self.colorRedBottomRight * (var11);
            self.colorGreenBottomRight = self.colorGreenBottomRight * (var11);
            self.colorBlueBottomRight = self.colorBlueBottomRight * (var11);
            self.colorRedTopRight = self.colorRedTopRight * (var12);
            self.colorGreenTopRight = self.colorGreenTopRight * (var12);
            self.colorBlueTopRight = self.colorBlueTopRight * (var12);
            self.face_y_neg(
                (var2 as f64),
                (var3 as f64),
                (var4 as f64),
                self.icon(var2, var3, var4, 0),
            );
            var8 = true;
        }
        if self.renderAllFaces || self.should_side(var2, var3 + 1, var4, 1) {
            if self.renderMaxY >= 1.0f64 {
                var3 += 1;
            }
            self.aoBrightnessXYNP = self.mixed(var2 - 1, var3, var4);
            self.aoBrightnessXYPP = self.mixed(var2 + 1, var3, var4);
            self.aoBrightnessYZPN = self.mixed(var2, var3, var4 - 1);
            self.aoBrightnessYZPP = self.mixed(var2, var3, var4 + 1);
            self.aoLightValueScratchXYNP = self.ao_light(var2 - 1, var3, var4);
            self.aoLightValueScratchXYPP = self.ao_light(var2 + 1, var3, var4);
            self.aoLightValueScratchYZPN = self.ao_light(var2, var3, var4 - 1);
            self.aoLightValueScratchYZPP = self.ao_light(var2, var3, var4 + 1);
            let mut var47: bool = self.can_block_grass(var2 + 1, var3 + 1, var4);
            let mut var52: bool = self.can_block_grass(var2 - 1, var3 + 1, var4);
            let mut var57: bool = self.can_block_grass(var2, var3 + 1, var4 + 1);
            let mut var62: bool = self.can_block_grass(var2, var3 + 1, var4 - 1);
            if !var62 && !var52 {
                self.aoLightValueScratchXYZNPN = self.aoLightValueScratchXYNP;
                self.aoBrightnessXYZNPN = self.aoBrightnessXYNP;
            } else {
                self.aoLightValueScratchXYZNPN = self.ao_light(var2 - 1, var3, var4 - 1);
                self.aoBrightnessXYZNPN = self.mixed(var2 - 1, var3, var4 - 1);
            }
            if !var62 && !var47 {
                self.aoLightValueScratchXYZPPN = self.aoLightValueScratchXYPP;
                self.aoBrightnessXYZPPN = self.aoBrightnessXYPP;
            } else {
                self.aoLightValueScratchXYZPPN = self.ao_light(var2 + 1, var3, var4 - 1);
                self.aoBrightnessXYZPPN = self.mixed(var2 + 1, var3, var4 - 1);
            }
            if !var57 && !var52 {
                self.aoLightValueScratchXYZNPP = self.aoLightValueScratchXYNP;
                self.aoBrightnessXYZNPP = self.aoBrightnessXYNP;
            } else {
                self.aoLightValueScratchXYZNPP = self.ao_light(var2 - 1, var3, var4 + 1);
                self.aoBrightnessXYZNPP = self.mixed(var2 - 1, var3, var4 + 1);
            }
            if !var57 && !var47 {
                self.aoLightValueScratchXYZPPP = self.aoLightValueScratchXYPP;
                self.aoBrightnessXYZPPP = self.aoBrightnessXYPP;
            } else {
                self.aoLightValueScratchXYZPPP = self.ao_light(var2 + 1, var3, var4 + 1);
                self.aoBrightnessXYZPPP = self.mixed(var2 + 1, var3, var4 + 1);
            }
            if self.renderMaxY >= 1.0f64 {
                var3 -= 1;
            }
            let mut var67: i32 = var14;
            if self.renderMaxY >= 1.0f64 || !self.is_opaque(var2, var3 + 1, var4) {
                var67 = self.mixed(var2, var3 + 1, var4);
            }
            let mut var72: f32 = self.ao_light(var2, var3 + 1, var4);
            var12 = (self.aoLightValueScratchXYZNPP
                + self.aoLightValueScratchXYNP
                + self.aoLightValueScratchYZPP
                + var72)
                / 4.0f32;
            var9 = (self.aoLightValueScratchYZPP
                + var72
                + self.aoLightValueScratchXYZPPP
                + self.aoLightValueScratchXYPP)
                / 4.0f32;
            var10 = (var72
                + self.aoLightValueScratchYZPN
                + self.aoLightValueScratchXYPP
                + self.aoLightValueScratchXYZPPN)
                / 4.0f32;
            var11 = (self.aoLightValueScratchXYNP
                + self.aoLightValueScratchXYZNPN
                + var72
                + self.aoLightValueScratchYZPN)
                / 4.0f32;
            self.brightnessTopRight = self.getAoBrightness(
                self.aoBrightnessXYZNPP,
                self.aoBrightnessXYNP,
                self.aoBrightnessYZPP,
                var67,
            );
            self.brightnessTopLeft = self.getAoBrightness(
                self.aoBrightnessYZPP,
                self.aoBrightnessXYZPPP,
                self.aoBrightnessXYPP,
                var67,
            );
            self.brightnessBottomLeft = self.getAoBrightness(
                self.aoBrightnessYZPN,
                self.aoBrightnessXYPP,
                self.aoBrightnessXYZPPN,
                var67,
            );
            self.brightnessBottomRight = self.getAoBrightness(
                self.aoBrightnessXYNP,
                self.aoBrightnessXYZNPN,
                self.aoBrightnessYZPN,
                var67,
            );
            self.colorRedTopRight = var5;
            self.colorRedBottomRight = self.colorRedTopRight;
            self.colorRedBottomLeft = self.colorRedBottomRight;
            self.colorRedTopLeft = self.colorRedBottomLeft;
            self.colorGreenTopRight = var6;
            self.colorGreenBottomRight = self.colorGreenTopRight;
            self.colorGreenBottomLeft = self.colorGreenBottomRight;
            self.colorGreenTopLeft = self.colorGreenBottomLeft;
            self.colorBlueTopRight = var7;
            self.colorBlueBottomRight = self.colorBlueTopRight;
            self.colorBlueBottomLeft = self.colorBlueBottomRight;
            self.colorBlueTopLeft = self.colorBlueBottomLeft;
            self.colorRedTopLeft = self.colorRedTopLeft * (var9);
            self.colorGreenTopLeft = self.colorGreenTopLeft * (var9);
            self.colorBlueTopLeft = self.colorBlueTopLeft * (var9);
            self.colorRedBottomLeft = self.colorRedBottomLeft * (var10);
            self.colorGreenBottomLeft = self.colorGreenBottomLeft * (var10);
            self.colorBlueBottomLeft = self.colorBlueBottomLeft * (var10);
            self.colorRedBottomRight = self.colorRedBottomRight * (var11);
            self.colorGreenBottomRight = self.colorGreenBottomRight * (var11);
            self.colorBlueBottomRight = self.colorBlueBottomRight * (var11);
            self.colorRedTopRight = self.colorRedTopRight * (var12);
            self.colorGreenTopRight = self.colorGreenTopRight * (var12);
            self.colorBlueTopRight = self.colorBlueTopRight * (var12);
            self.face_y_pos(
                (var2 as f64),
                (var3 as f64),
                (var4 as f64),
                self.icon(var2, var3, var4, 1),
            );
            var8 = true;
        }
        if self.renderAllFaces || self.should_side(var2, var3, var4 - 1, 2) {
            if self.renderMinZ <= 0.0f64 {
                var4 -= 1;
            }
            self.aoLightValueScratchXZNN = self.ao_light(var2 - 1, var3, var4);
            self.aoLightValueScratchYZNN = self.ao_light(var2, var3 - 1, var4);
            self.aoLightValueScratchYZPN = self.ao_light(var2, var3 + 1, var4);
            self.aoLightValueScratchXZPN = self.ao_light(var2 + 1, var3, var4);
            self.aoBrightnessXZNN = self.mixed(var2 - 1, var3, var4);
            self.aoBrightnessYZNN = self.mixed(var2, var3 - 1, var4);
            self.aoBrightnessYZPN = self.mixed(var2, var3 + 1, var4);
            self.aoBrightnessXZPN = self.mixed(var2 + 1, var3, var4);
            let mut var48: bool = self.can_block_grass(var2 + 1, var3, var4 - 1);
            let mut var53: bool = self.can_block_grass(var2 - 1, var3, var4 - 1);
            let mut var58: bool = self.can_block_grass(var2, var3 + 1, var4 - 1);
            let mut var63: bool = self.can_block_grass(var2, var3 - 1, var4 - 1);
            if !var53 && !var63 {
                self.aoLightValueScratchXYZNNN = self.aoLightValueScratchXZNN;
                self.aoBrightnessXYZNNN = self.aoBrightnessXZNN;
            } else {
                self.aoLightValueScratchXYZNNN = self.ao_light(var2 - 1, var3 - 1, var4);
                self.aoBrightnessXYZNNN = self.mixed(var2 - 1, var3 - 1, var4);
            }
            if !var53 && !var58 {
                self.aoLightValueScratchXYZNPN = self.aoLightValueScratchXZNN;
                self.aoBrightnessXYZNPN = self.aoBrightnessXZNN;
            } else {
                self.aoLightValueScratchXYZNPN = self.ao_light(var2 - 1, var3 + 1, var4);
                self.aoBrightnessXYZNPN = self.mixed(var2 - 1, var3 + 1, var4);
            }
            if !var48 && !var63 {
                self.aoLightValueScratchXYZPNN = self.aoLightValueScratchXZPN;
                self.aoBrightnessXYZPNN = self.aoBrightnessXZPN;
            } else {
                self.aoLightValueScratchXYZPNN = self.ao_light(var2 + 1, var3 - 1, var4);
                self.aoBrightnessXYZPNN = self.mixed(var2 + 1, var3 - 1, var4);
            }
            if !var48 && !var58 {
                self.aoLightValueScratchXYZPPN = self.aoLightValueScratchXZPN;
                self.aoBrightnessXYZPPN = self.aoBrightnessXZPN;
            } else {
                self.aoLightValueScratchXYZPPN = self.ao_light(var2 + 1, var3 + 1, var4);
                self.aoBrightnessXYZPPN = self.mixed(var2 + 1, var3 + 1, var4);
            }
            if self.renderMinZ <= 0.0f64 {
                var4 += 1;
            }
            let mut var68: i32 = var14;
            if self.renderMinZ <= 0.0f64 || !self.is_opaque(var2, var3, var4 - 1) {
                var68 = self.mixed(var2, var3, var4 - 1);
            }
            let mut var73: f32 = self.ao_light(var2, var3, var4 - 1);
            var9 = (self.aoLightValueScratchXZNN
                + self.aoLightValueScratchXYZNPN
                + var73
                + self.aoLightValueScratchYZPN)
                / 4.0f32;
            var10 = (var73
                + self.aoLightValueScratchYZPN
                + self.aoLightValueScratchXZPN
                + self.aoLightValueScratchXYZPPN)
                / 4.0f32;
            var11 = (self.aoLightValueScratchYZNN
                + var73
                + self.aoLightValueScratchXYZPNN
                + self.aoLightValueScratchXZPN)
                / 4.0f32;
            var12 = (self.aoLightValueScratchXYZNNN
                + self.aoLightValueScratchXZNN
                + self.aoLightValueScratchYZNN
                + var73)
                / 4.0f32;
            self.brightnessTopLeft = self.getAoBrightness(
                self.aoBrightnessXZNN,
                self.aoBrightnessXYZNPN,
                self.aoBrightnessYZPN,
                var68,
            );
            self.brightnessBottomLeft = self.getAoBrightness(
                self.aoBrightnessYZPN,
                self.aoBrightnessXZPN,
                self.aoBrightnessXYZPPN,
                var68,
            );
            self.brightnessBottomRight = self.getAoBrightness(
                self.aoBrightnessYZNN,
                self.aoBrightnessXYZPNN,
                self.aoBrightnessXZPN,
                var68,
            );
            self.brightnessTopRight = self.getAoBrightness(
                self.aoBrightnessXYZNNN,
                self.aoBrightnessXZNN,
                self.aoBrightnessYZNN,
                var68,
            );
            if var13 {
                self.colorRedTopRight = var5 * 0.8f32;
                self.colorRedBottomRight = self.colorRedTopRight;
                self.colorRedBottomLeft = self.colorRedBottomRight;
                self.colorRedTopLeft = self.colorRedBottomLeft;
                self.colorGreenTopRight = var6 * 0.8f32;
                self.colorGreenBottomRight = self.colorGreenTopRight;
                self.colorGreenBottomLeft = self.colorGreenBottomRight;
                self.colorGreenTopLeft = self.colorGreenBottomLeft;
                self.colorBlueTopRight = var7 * 0.8f32;
                self.colorBlueBottomRight = self.colorBlueTopRight;
                self.colorBlueBottomLeft = self.colorBlueBottomRight;
                self.colorBlueTopLeft = self.colorBlueBottomLeft;
            } else {
                self.colorRedTopRight = 0.8f32;
                self.colorRedBottomRight = self.colorRedTopRight;
                self.colorRedBottomLeft = self.colorRedBottomRight;
                self.colorRedTopLeft = self.colorRedBottomLeft;
                self.colorGreenTopRight = 0.8f32;
                self.colorGreenBottomRight = self.colorGreenTopRight;
                self.colorGreenBottomLeft = self.colorGreenBottomRight;
                self.colorGreenTopLeft = self.colorGreenBottomLeft;
                self.colorBlueTopRight = 0.8f32;
                self.colorBlueBottomRight = self.colorBlueTopRight;
                self.colorBlueBottomLeft = self.colorBlueBottomRight;
                self.colorBlueTopLeft = self.colorBlueBottomLeft;
            }
            self.colorRedTopLeft = self.colorRedTopLeft * (var9);
            self.colorGreenTopLeft = self.colorGreenTopLeft * (var9);
            self.colorBlueTopLeft = self.colorBlueTopLeft * (var9);
            self.colorRedBottomLeft = self.colorRedBottomLeft * (var10);
            self.colorGreenBottomLeft = self.colorGreenBottomLeft * (var10);
            self.colorBlueBottomLeft = self.colorBlueBottomLeft * (var10);
            self.colorRedBottomRight = self.colorRedBottomRight * (var11);
            self.colorGreenBottomRight = self.colorGreenBottomRight * (var11);
            self.colorBlueBottomRight = self.colorBlueBottomRight * (var11);
            self.colorRedTopRight = self.colorRedTopRight * (var12);
            self.colorGreenTopRight = self.colorGreenTopRight * (var12);
            self.colorBlueTopRight = self.colorBlueTopRight * (var12);
            let mut var22: Icon = self.icon(var2, var3, var4, 2);
            self.face_z_neg((var2 as f64), (var3 as f64), (var4 as f64), var22);
            if FANCY_GRASS && var22.grass_side && !self.overrideBlockTexture.is_some() {
                self.colorRedTopLeft = self.colorRedTopLeft * (var5);
                self.colorRedBottomLeft = self.colorRedBottomLeft * (var5);
                self.colorRedBottomRight = self.colorRedBottomRight * (var5);
                self.colorRedTopRight = self.colorRedTopRight * (var5);
                self.colorGreenTopLeft = self.colorGreenTopLeft * (var6);
                self.colorGreenBottomLeft = self.colorGreenBottomLeft * (var6);
                self.colorGreenBottomRight = self.colorGreenBottomRight * (var6);
                self.colorGreenTopRight = self.colorGreenTopRight * (var6);
                self.colorBlueTopLeft = self.colorBlueTopLeft * (var7);
                self.colorBlueBottomLeft = self.colorBlueBottomLeft * (var7);
                self.colorBlueBottomRight = self.colorBlueBottomRight * (var7);
                self.colorBlueTopRight = self.colorBlueTopRight * (var7);
                self.face_z_neg(
                    (var2 as f64),
                    (var3 as f64),
                    (var4 as f64),
                    self.grass_overlay(),
                );
            }
            var8 = true;
        }
        if self.renderAllFaces || self.should_side(var2, var3, var4 + 1, 3) {
            if self.renderMaxZ >= 1.0f64 {
                var4 += 1;
            }
            self.aoLightValueScratchXZNP = self.ao_light(var2 - 1, var3, var4);
            self.aoLightValueScratchXZPP = self.ao_light(var2 + 1, var3, var4);
            self.aoLightValueScratchYZNP = self.ao_light(var2, var3 - 1, var4);
            self.aoLightValueScratchYZPP = self.ao_light(var2, var3 + 1, var4);
            self.aoBrightnessXZNP = self.mixed(var2 - 1, var3, var4);
            self.aoBrightnessXZPP = self.mixed(var2 + 1, var3, var4);
            self.aoBrightnessYZNP = self.mixed(var2, var3 - 1, var4);
            self.aoBrightnessYZPP = self.mixed(var2, var3 + 1, var4);
            let mut var49: bool = self.can_block_grass(var2 + 1, var3, var4 + 1);
            let mut var54: bool = self.can_block_grass(var2 - 1, var3, var4 + 1);
            let mut var59: bool = self.can_block_grass(var2, var3 + 1, var4 + 1);
            let mut var64: bool = self.can_block_grass(var2, var3 - 1, var4 + 1);
            if !var54 && !var64 {
                self.aoLightValueScratchXYZNNP = self.aoLightValueScratchXZNP;
                self.aoBrightnessXYZNNP = self.aoBrightnessXZNP;
            } else {
                self.aoLightValueScratchXYZNNP = self.ao_light(var2 - 1, var3 - 1, var4);
                self.aoBrightnessXYZNNP = self.mixed(var2 - 1, var3 - 1, var4);
            }
            if !var54 && !var59 {
                self.aoLightValueScratchXYZNPP = self.aoLightValueScratchXZNP;
                self.aoBrightnessXYZNPP = self.aoBrightnessXZNP;
            } else {
                self.aoLightValueScratchXYZNPP = self.ao_light(var2 - 1, var3 + 1, var4);
                self.aoBrightnessXYZNPP = self.mixed(var2 - 1, var3 + 1, var4);
            }
            if !var49 && !var64 {
                self.aoLightValueScratchXYZPNP = self.aoLightValueScratchXZPP;
                self.aoBrightnessXYZPNP = self.aoBrightnessXZPP;
            } else {
                self.aoLightValueScratchXYZPNP = self.ao_light(var2 + 1, var3 - 1, var4);
                self.aoBrightnessXYZPNP = self.mixed(var2 + 1, var3 - 1, var4);
            }
            if !var49 && !var59 {
                self.aoLightValueScratchXYZPPP = self.aoLightValueScratchXZPP;
                self.aoBrightnessXYZPPP = self.aoBrightnessXZPP;
            } else {
                self.aoLightValueScratchXYZPPP = self.ao_light(var2 + 1, var3 + 1, var4);
                self.aoBrightnessXYZPPP = self.mixed(var2 + 1, var3 + 1, var4);
            }
            if self.renderMaxZ >= 1.0f64 {
                var4 -= 1;
            }
            let mut var69: i32 = var14;
            if self.renderMaxZ >= 1.0f64 || !self.is_opaque(var2, var3, var4 + 1) {
                var69 = self.mixed(var2, var3, var4 + 1);
            }
            let mut var74: f32 = self.ao_light(var2, var3, var4 + 1);
            var9 = (self.aoLightValueScratchXZNP
                + self.aoLightValueScratchXYZNPP
                + var74
                + self.aoLightValueScratchYZPP)
                / 4.0f32;
            var12 = (var74
                + self.aoLightValueScratchYZPP
                + self.aoLightValueScratchXZPP
                + self.aoLightValueScratchXYZPPP)
                / 4.0f32;
            var11 = (self.aoLightValueScratchYZNP
                + var74
                + self.aoLightValueScratchXYZPNP
                + self.aoLightValueScratchXZPP)
                / 4.0f32;
            var10 = (self.aoLightValueScratchXYZNNP
                + self.aoLightValueScratchXZNP
                + self.aoLightValueScratchYZNP
                + var74)
                / 4.0f32;
            self.brightnessTopLeft = self.getAoBrightness(
                self.aoBrightnessXZNP,
                self.aoBrightnessXYZNPP,
                self.aoBrightnessYZPP,
                var69,
            );
            self.brightnessTopRight = self.getAoBrightness(
                self.aoBrightnessYZPP,
                self.aoBrightnessXZPP,
                self.aoBrightnessXYZPPP,
                var69,
            );
            self.brightnessBottomRight = self.getAoBrightness(
                self.aoBrightnessYZNP,
                self.aoBrightnessXYZPNP,
                self.aoBrightnessXZPP,
                var69,
            );
            self.brightnessBottomLeft = self.getAoBrightness(
                self.aoBrightnessXYZNNP,
                self.aoBrightnessXZNP,
                self.aoBrightnessYZNP,
                var69,
            );
            if var13 {
                self.colorRedTopRight = var5 * 0.8f32;
                self.colorRedBottomRight = self.colorRedTopRight;
                self.colorRedBottomLeft = self.colorRedBottomRight;
                self.colorRedTopLeft = self.colorRedBottomLeft;
                self.colorGreenTopRight = var6 * 0.8f32;
                self.colorGreenBottomRight = self.colorGreenTopRight;
                self.colorGreenBottomLeft = self.colorGreenBottomRight;
                self.colorGreenTopLeft = self.colorGreenBottomLeft;
                self.colorBlueTopRight = var7 * 0.8f32;
                self.colorBlueBottomRight = self.colorBlueTopRight;
                self.colorBlueBottomLeft = self.colorBlueBottomRight;
                self.colorBlueTopLeft = self.colorBlueBottomLeft;
            } else {
                self.colorRedTopRight = 0.8f32;
                self.colorRedBottomRight = self.colorRedTopRight;
                self.colorRedBottomLeft = self.colorRedBottomRight;
                self.colorRedTopLeft = self.colorRedBottomLeft;
                self.colorGreenTopRight = 0.8f32;
                self.colorGreenBottomRight = self.colorGreenTopRight;
                self.colorGreenBottomLeft = self.colorGreenBottomRight;
                self.colorGreenTopLeft = self.colorGreenBottomLeft;
                self.colorBlueTopRight = 0.8f32;
                self.colorBlueBottomRight = self.colorBlueTopRight;
                self.colorBlueBottomLeft = self.colorBlueBottomRight;
                self.colorBlueTopLeft = self.colorBlueBottomLeft;
            }
            self.colorRedTopLeft = self.colorRedTopLeft * (var9);
            self.colorGreenTopLeft = self.colorGreenTopLeft * (var9);
            self.colorBlueTopLeft = self.colorBlueTopLeft * (var9);
            self.colorRedBottomLeft = self.colorRedBottomLeft * (var10);
            self.colorGreenBottomLeft = self.colorGreenBottomLeft * (var10);
            self.colorBlueBottomLeft = self.colorBlueBottomLeft * (var10);
            self.colorRedBottomRight = self.colorRedBottomRight * (var11);
            self.colorGreenBottomRight = self.colorGreenBottomRight * (var11);
            self.colorBlueBottomRight = self.colorBlueBottomRight * (var11);
            self.colorRedTopRight = self.colorRedTopRight * (var12);
            self.colorGreenTopRight = self.colorGreenTopRight * (var12);
            self.colorBlueTopRight = self.colorBlueTopRight * (var12);
            let mut var77: Icon = self.icon(var2, var3, var4, 3);
            self.face_z_pos(
                (var2 as f64),
                (var3 as f64),
                (var4 as f64),
                self.icon(var2, var3, var4, 3),
            );
            if FANCY_GRASS && var77.grass_side && !self.overrideBlockTexture.is_some() {
                self.colorRedTopLeft = self.colorRedTopLeft * (var5);
                self.colorRedBottomLeft = self.colorRedBottomLeft * (var5);
                self.colorRedBottomRight = self.colorRedBottomRight * (var5);
                self.colorRedTopRight = self.colorRedTopRight * (var5);
                self.colorGreenTopLeft = self.colorGreenTopLeft * (var6);
                self.colorGreenBottomLeft = self.colorGreenBottomLeft * (var6);
                self.colorGreenBottomRight = self.colorGreenBottomRight * (var6);
                self.colorGreenTopRight = self.colorGreenTopRight * (var6);
                self.colorBlueTopLeft = self.colorBlueTopLeft * (var7);
                self.colorBlueBottomLeft = self.colorBlueBottomLeft * (var7);
                self.colorBlueBottomRight = self.colorBlueBottomRight * (var7);
                self.colorBlueTopRight = self.colorBlueTopRight * (var7);
                self.face_z_pos(
                    (var2 as f64),
                    (var3 as f64),
                    (var4 as f64),
                    self.grass_overlay(),
                );
            }
            var8 = true;
        }
        if self.renderAllFaces || self.should_side(var2 - 1, var3, var4, 4) {
            if self.renderMinX <= 0.0f64 {
                var2 -= 1;
            }
            self.aoLightValueScratchXYNN = self.ao_light(var2, var3 - 1, var4);
            self.aoLightValueScratchXZNN = self.ao_light(var2, var3, var4 - 1);
            self.aoLightValueScratchXZNP = self.ao_light(var2, var3, var4 + 1);
            self.aoLightValueScratchXYNP = self.ao_light(var2, var3 + 1, var4);
            self.aoBrightnessXYNN = self.mixed(var2, var3 - 1, var4);
            self.aoBrightnessXZNN = self.mixed(var2, var3, var4 - 1);
            self.aoBrightnessXZNP = self.mixed(var2, var3, var4 + 1);
            self.aoBrightnessXYNP = self.mixed(var2, var3 + 1, var4);
            let mut var50: bool = self.can_block_grass(var2 - 1, var3 + 1, var4);
            let mut var55: bool = self.can_block_grass(var2 - 1, var3 - 1, var4);
            let mut var60: bool = self.can_block_grass(var2 - 1, var3, var4 - 1);
            let mut var65: bool = self.can_block_grass(var2 - 1, var3, var4 + 1);
            if !var60 && !var55 {
                self.aoLightValueScratchXYZNNN = self.aoLightValueScratchXZNN;
                self.aoBrightnessXYZNNN = self.aoBrightnessXZNN;
            } else {
                self.aoLightValueScratchXYZNNN = self.ao_light(var2, var3 - 1, var4 - 1);
                self.aoBrightnessXYZNNN = self.mixed(var2, var3 - 1, var4 - 1);
            }
            if !var65 && !var55 {
                self.aoLightValueScratchXYZNNP = self.aoLightValueScratchXZNP;
                self.aoBrightnessXYZNNP = self.aoBrightnessXZNP;
            } else {
                self.aoLightValueScratchXYZNNP = self.ao_light(var2, var3 - 1, var4 + 1);
                self.aoBrightnessXYZNNP = self.mixed(var2, var3 - 1, var4 + 1);
            }
            if !var60 && !var50 {
                self.aoLightValueScratchXYZNPN = self.aoLightValueScratchXZNN;
                self.aoBrightnessXYZNPN = self.aoBrightnessXZNN;
            } else {
                self.aoLightValueScratchXYZNPN = self.ao_light(var2, var3 + 1, var4 - 1);
                self.aoBrightnessXYZNPN = self.mixed(var2, var3 + 1, var4 - 1);
            }
            if !var65 && !var50 {
                self.aoLightValueScratchXYZNPP = self.aoLightValueScratchXZNP;
                self.aoBrightnessXYZNPP = self.aoBrightnessXZNP;
            } else {
                self.aoLightValueScratchXYZNPP = self.ao_light(var2, var3 + 1, var4 + 1);
                self.aoBrightnessXYZNPP = self.mixed(var2, var3 + 1, var4 + 1);
            }
            if self.renderMinX <= 0.0f64 {
                var2 += 1;
            }
            let mut var70: i32 = var14;
            if self.renderMinX <= 0.0f64 || !self.is_opaque(var2 - 1, var3, var4) {
                var70 = self.mixed(var2 - 1, var3, var4);
            }
            let mut var75: f32 = self.ao_light(var2 - 1, var3, var4);
            var12 = (self.aoLightValueScratchXYNN
                + self.aoLightValueScratchXYZNNP
                + var75
                + self.aoLightValueScratchXZNP)
                / 4.0f32;
            var9 = (var75
                + self.aoLightValueScratchXZNP
                + self.aoLightValueScratchXYNP
                + self.aoLightValueScratchXYZNPP)
                / 4.0f32;
            var10 = (self.aoLightValueScratchXZNN
                + var75
                + self.aoLightValueScratchXYZNPN
                + self.aoLightValueScratchXYNP)
                / 4.0f32;
            var11 = (self.aoLightValueScratchXYZNNN
                + self.aoLightValueScratchXYNN
                + self.aoLightValueScratchXZNN
                + var75)
                / 4.0f32;
            self.brightnessTopRight = self.getAoBrightness(
                self.aoBrightnessXYNN,
                self.aoBrightnessXYZNNP,
                self.aoBrightnessXZNP,
                var70,
            );
            self.brightnessTopLeft = self.getAoBrightness(
                self.aoBrightnessXZNP,
                self.aoBrightnessXYNP,
                self.aoBrightnessXYZNPP,
                var70,
            );
            self.brightnessBottomLeft = self.getAoBrightness(
                self.aoBrightnessXZNN,
                self.aoBrightnessXYZNPN,
                self.aoBrightnessXYNP,
                var70,
            );
            self.brightnessBottomRight = self.getAoBrightness(
                self.aoBrightnessXYZNNN,
                self.aoBrightnessXYNN,
                self.aoBrightnessXZNN,
                var70,
            );
            if var13 {
                self.colorRedTopRight = var5 * 0.6f32;
                self.colorRedBottomRight = self.colorRedTopRight;
                self.colorRedBottomLeft = self.colorRedBottomRight;
                self.colorRedTopLeft = self.colorRedBottomLeft;
                self.colorGreenTopRight = var6 * 0.6f32;
                self.colorGreenBottomRight = self.colorGreenTopRight;
                self.colorGreenBottomLeft = self.colorGreenBottomRight;
                self.colorGreenTopLeft = self.colorGreenBottomLeft;
                self.colorBlueTopRight = var7 * 0.6f32;
                self.colorBlueBottomRight = self.colorBlueTopRight;
                self.colorBlueBottomLeft = self.colorBlueBottomRight;
                self.colorBlueTopLeft = self.colorBlueBottomLeft;
            } else {
                self.colorRedTopRight = 0.6f32;
                self.colorRedBottomRight = self.colorRedTopRight;
                self.colorRedBottomLeft = self.colorRedBottomRight;
                self.colorRedTopLeft = self.colorRedBottomLeft;
                self.colorGreenTopRight = 0.6f32;
                self.colorGreenBottomRight = self.colorGreenTopRight;
                self.colorGreenBottomLeft = self.colorGreenBottomRight;
                self.colorGreenTopLeft = self.colorGreenBottomLeft;
                self.colorBlueTopRight = 0.6f32;
                self.colorBlueBottomRight = self.colorBlueTopRight;
                self.colorBlueBottomLeft = self.colorBlueBottomRight;
                self.colorBlueTopLeft = self.colorBlueBottomLeft;
            }
            self.colorRedTopLeft = self.colorRedTopLeft * (var9);
            self.colorGreenTopLeft = self.colorGreenTopLeft * (var9);
            self.colorBlueTopLeft = self.colorBlueTopLeft * (var9);
            self.colorRedBottomLeft = self.colorRedBottomLeft * (var10);
            self.colorGreenBottomLeft = self.colorGreenBottomLeft * (var10);
            self.colorBlueBottomLeft = self.colorBlueBottomLeft * (var10);
            self.colorRedBottomRight = self.colorRedBottomRight * (var11);
            self.colorGreenBottomRight = self.colorGreenBottomRight * (var11);
            self.colorBlueBottomRight = self.colorBlueBottomRight * (var11);
            self.colorRedTopRight = self.colorRedTopRight * (var12);
            self.colorGreenTopRight = self.colorGreenTopRight * (var12);
            self.colorBlueTopRight = self.colorBlueTopRight * (var12);
            let mut var78: Icon = self.icon(var2, var3, var4, 4);
            self.face_x_neg((var2 as f64), (var3 as f64), (var4 as f64), var78);
            if FANCY_GRASS && var78.grass_side && !self.overrideBlockTexture.is_some() {
                self.colorRedTopLeft = self.colorRedTopLeft * (var5);
                self.colorRedBottomLeft = self.colorRedBottomLeft * (var5);
                self.colorRedBottomRight = self.colorRedBottomRight * (var5);
                self.colorRedTopRight = self.colorRedTopRight * (var5);
                self.colorGreenTopLeft = self.colorGreenTopLeft * (var6);
                self.colorGreenBottomLeft = self.colorGreenBottomLeft * (var6);
                self.colorGreenBottomRight = self.colorGreenBottomRight * (var6);
                self.colorGreenTopRight = self.colorGreenTopRight * (var6);
                self.colorBlueTopLeft = self.colorBlueTopLeft * (var7);
                self.colorBlueBottomLeft = self.colorBlueBottomLeft * (var7);
                self.colorBlueBottomRight = self.colorBlueBottomRight * (var7);
                self.colorBlueTopRight = self.colorBlueTopRight * (var7);
                self.face_x_neg(
                    (var2 as f64),
                    (var3 as f64),
                    (var4 as f64),
                    self.grass_overlay(),
                );
            }
            var8 = true;
        }
        if self.renderAllFaces || self.should_side(var2 + 1, var3, var4, 5) {
            if self.renderMaxX >= 1.0f64 {
                var2 += 1;
            }
            self.aoLightValueScratchXYPN = self.ao_light(var2, var3 - 1, var4);
            self.aoLightValueScratchXZPN = self.ao_light(var2, var3, var4 - 1);
            self.aoLightValueScratchXZPP = self.ao_light(var2, var3, var4 + 1);
            self.aoLightValueScratchXYPP = self.ao_light(var2, var3 + 1, var4);
            self.aoBrightnessXYPN = self.mixed(var2, var3 - 1, var4);
            self.aoBrightnessXZPN = self.mixed(var2, var3, var4 - 1);
            self.aoBrightnessXZPP = self.mixed(var2, var3, var4 + 1);
            self.aoBrightnessXYPP = self.mixed(var2, var3 + 1, var4);
            let mut var51: bool = self.can_block_grass(var2 + 1, var3 + 1, var4);
            let mut var56: bool = self.can_block_grass(var2 + 1, var3 - 1, var4);
            let mut var61: bool = self.can_block_grass(var2 + 1, var3, var4 + 1);
            let mut var66: bool = self.can_block_grass(var2 + 1, var3, var4 - 1);
            if !var56 && !var66 {
                self.aoLightValueScratchXYZPNN = self.aoLightValueScratchXZPN;
                self.aoBrightnessXYZPNN = self.aoBrightnessXZPN;
            } else {
                self.aoLightValueScratchXYZPNN = self.ao_light(var2, var3 - 1, var4 - 1);
                self.aoBrightnessXYZPNN = self.mixed(var2, var3 - 1, var4 - 1);
            }
            if !var56 && !var61 {
                self.aoLightValueScratchXYZPNP = self.aoLightValueScratchXZPP;
                self.aoBrightnessXYZPNP = self.aoBrightnessXZPP;
            } else {
                self.aoLightValueScratchXYZPNP = self.ao_light(var2, var3 - 1, var4 + 1);
                self.aoBrightnessXYZPNP = self.mixed(var2, var3 - 1, var4 + 1);
            }
            if !var51 && !var66 {
                self.aoLightValueScratchXYZPPN = self.aoLightValueScratchXZPN;
                self.aoBrightnessXYZPPN = self.aoBrightnessXZPN;
            } else {
                self.aoLightValueScratchXYZPPN = self.ao_light(var2, var3 + 1, var4 - 1);
                self.aoBrightnessXYZPPN = self.mixed(var2, var3 + 1, var4 - 1);
            }
            if !var51 && !var61 {
                self.aoLightValueScratchXYZPPP = self.aoLightValueScratchXZPP;
                self.aoBrightnessXYZPPP = self.aoBrightnessXZPP;
            } else {
                self.aoLightValueScratchXYZPPP = self.ao_light(var2, var3 + 1, var4 + 1);
                self.aoBrightnessXYZPPP = self.mixed(var2, var3 + 1, var4 + 1);
            }
            if self.renderMaxX >= 1.0f64 {
                var2 -= 1;
            }
            let mut var71: i32 = var14;
            if self.renderMaxX >= 1.0f64 || !self.is_opaque(var2 + 1, var3, var4) {
                var71 = self.mixed(var2 + 1, var3, var4);
            }
            let mut var76: f32 = self.ao_light(var2 + 1, var3, var4);
            var9 = (self.aoLightValueScratchXYPN
                + self.aoLightValueScratchXYZPNP
                + var76
                + self.aoLightValueScratchXZPP)
                / 4.0f32;
            var10 = (self.aoLightValueScratchXYZPNN
                + self.aoLightValueScratchXYPN
                + self.aoLightValueScratchXZPN
                + var76)
                / 4.0f32;
            var11 = (self.aoLightValueScratchXZPN
                + var76
                + self.aoLightValueScratchXYZPPN
                + self.aoLightValueScratchXYPP)
                / 4.0f32;
            var12 = (var76
                + self.aoLightValueScratchXZPP
                + self.aoLightValueScratchXYPP
                + self.aoLightValueScratchXYZPPP)
                / 4.0f32;
            self.brightnessTopLeft = self.getAoBrightness(
                self.aoBrightnessXYPN,
                self.aoBrightnessXYZPNP,
                self.aoBrightnessXZPP,
                var71,
            );
            self.brightnessTopRight = self.getAoBrightness(
                self.aoBrightnessXZPP,
                self.aoBrightnessXYPP,
                self.aoBrightnessXYZPPP,
                var71,
            );
            self.brightnessBottomRight = self.getAoBrightness(
                self.aoBrightnessXZPN,
                self.aoBrightnessXYZPPN,
                self.aoBrightnessXYPP,
                var71,
            );
            self.brightnessBottomLeft = self.getAoBrightness(
                self.aoBrightnessXYZPNN,
                self.aoBrightnessXYPN,
                self.aoBrightnessXZPN,
                var71,
            );
            if var13 {
                self.colorRedTopRight = var5 * 0.6f32;
                self.colorRedBottomRight = self.colorRedTopRight;
                self.colorRedBottomLeft = self.colorRedBottomRight;
                self.colorRedTopLeft = self.colorRedBottomLeft;
                self.colorGreenTopRight = var6 * 0.6f32;
                self.colorGreenBottomRight = self.colorGreenTopRight;
                self.colorGreenBottomLeft = self.colorGreenBottomRight;
                self.colorGreenTopLeft = self.colorGreenBottomLeft;
                self.colorBlueTopRight = var7 * 0.6f32;
                self.colorBlueBottomRight = self.colorBlueTopRight;
                self.colorBlueBottomLeft = self.colorBlueBottomRight;
                self.colorBlueTopLeft = self.colorBlueBottomLeft;
            } else {
                self.colorRedTopRight = 0.6f32;
                self.colorRedBottomRight = self.colorRedTopRight;
                self.colorRedBottomLeft = self.colorRedBottomRight;
                self.colorRedTopLeft = self.colorRedBottomLeft;
                self.colorGreenTopRight = 0.6f32;
                self.colorGreenBottomRight = self.colorGreenTopRight;
                self.colorGreenBottomLeft = self.colorGreenBottomRight;
                self.colorGreenTopLeft = self.colorGreenBottomLeft;
                self.colorBlueTopRight = 0.6f32;
                self.colorBlueBottomRight = self.colorBlueTopRight;
                self.colorBlueBottomLeft = self.colorBlueBottomRight;
                self.colorBlueTopLeft = self.colorBlueBottomLeft;
            }
            self.colorRedTopLeft = self.colorRedTopLeft * (var9);
            self.colorGreenTopLeft = self.colorGreenTopLeft * (var9);
            self.colorBlueTopLeft = self.colorBlueTopLeft * (var9);
            self.colorRedBottomLeft = self.colorRedBottomLeft * (var10);
            self.colorGreenBottomLeft = self.colorGreenBottomLeft * (var10);
            self.colorBlueBottomLeft = self.colorBlueBottomLeft * (var10);
            self.colorRedBottomRight = self.colorRedBottomRight * (var11);
            self.colorGreenBottomRight = self.colorGreenBottomRight * (var11);
            self.colorBlueBottomRight = self.colorBlueBottomRight * (var11);
            self.colorRedTopRight = self.colorRedTopRight * (var12);
            self.colorGreenTopRight = self.colorGreenTopRight * (var12);
            self.colorBlueTopRight = self.colorBlueTopRight * (var12);
            let mut var79: Icon = self.icon(var2, var3, var4, 5);
            self.face_x_pos((var2 as f64), (var3 as f64), (var4 as f64), var79);
            if FANCY_GRASS && var79.grass_side && !self.overrideBlockTexture.is_some() {
                self.colorRedTopLeft = self.colorRedTopLeft * (var5);
                self.colorRedBottomLeft = self.colorRedBottomLeft * (var5);
                self.colorRedBottomRight = self.colorRedBottomRight * (var5);
                self.colorRedTopRight = self.colorRedTopRight * (var5);
                self.colorGreenTopLeft = self.colorGreenTopLeft * (var6);
                self.colorGreenBottomLeft = self.colorGreenBottomLeft * (var6);
                self.colorGreenBottomRight = self.colorGreenBottomRight * (var6);
                self.colorGreenTopRight = self.colorGreenTopRight * (var6);
                self.colorBlueTopLeft = self.colorBlueTopLeft * (var7);
                self.colorBlueBottomLeft = self.colorBlueBottomLeft * (var7);
                self.colorBlueBottomRight = self.colorBlueBottomRight * (var7);
                self.colorBlueTopRight = self.colorBlueTopRight * (var7);
                self.face_x_pos(
                    (var2 as f64),
                    (var3 as f64),
                    (var4 as f64),
                    self.grass_overlay(),
                );
            }
            var8 = true;
        }
        self.enableAO = false;
        return var8;
    }

    /// `RenderBlocks.renderStandardBlockWithAmbientOcclusionPartial`
    #[allow(
        unused_assignments,
        unused_mut,
        unused_variables,
        unused_parens,
        clippy::all
    )]
    pub fn std_ao_partial(
        &mut self,
        mut var2: i32,
        mut var3: i32,
        mut var4: i32,
        var5: f32,
        var6: f32,
        var7: f32,
    ) -> bool {
        self.enableAO = true;
        let mut var8: bool = false;
        let mut var9: f32 = 0.0f32;
        let mut var10: f32 = 0.0f32;
        let mut var11: f32 = 0.0f32;
        let mut var12: f32 = 0.0f32;
        let mut var13: bool = true;
        let mut var14: i32 = self.mixed(var2, var3, var4);
        self.bright = 983055;
        if self.is_grass_top() {
            var13 = false;
        } else if self.overrideBlockTexture.is_some() {
            var13 = false;
        }
        if self.renderAllFaces || self.should_side(var2, var3 - 1, var4, 0) {
            if self.renderMinY <= 0.0f64 {
                var3 -= 1;
            }
            self.aoBrightnessXYNN = self.mixed(var2 - 1, var3, var4);
            self.aoBrightnessYZNN = self.mixed(var2, var3, var4 - 1);
            self.aoBrightnessYZNP = self.mixed(var2, var3, var4 + 1);
            self.aoBrightnessXYPN = self.mixed(var2 + 1, var3, var4);
            self.aoLightValueScratchXYNN = self.ao_light(var2 - 1, var3, var4);
            self.aoLightValueScratchYZNN = self.ao_light(var2, var3, var4 - 1);
            self.aoLightValueScratchYZNP = self.ao_light(var2, var3, var4 + 1);
            self.aoLightValueScratchXYPN = self.ao_light(var2 + 1, var3, var4);
            let mut var16: bool = self.can_block_grass(var2 + 1, var3 - 1, var4);
            let mut var17: bool = self.can_block_grass(var2 - 1, var3 - 1, var4);
            let mut var18: bool = self.can_block_grass(var2, var3 - 1, var4 + 1);
            let mut var19: bool = self.can_block_grass(var2, var3 - 1, var4 - 1);
            if !var19 && !var17 {
                self.aoLightValueScratchXYZNNN = self.aoLightValueScratchXYNN;
                self.aoBrightnessXYZNNN = self.aoBrightnessXYNN;
            } else {
                self.aoLightValueScratchXYZNNN = self.ao_light(var2 - 1, var3, var4 - 1);
                self.aoBrightnessXYZNNN = self.mixed(var2 - 1, var3, var4 - 1);
            }
            if !var18 && !var17 {
                self.aoLightValueScratchXYZNNP = self.aoLightValueScratchXYNN;
                self.aoBrightnessXYZNNP = self.aoBrightnessXYNN;
            } else {
                self.aoLightValueScratchXYZNNP = self.ao_light(var2 - 1, var3, var4 + 1);
                self.aoBrightnessXYZNNP = self.mixed(var2 - 1, var3, var4 + 1);
            }
            if !var19 && !var16 {
                self.aoLightValueScratchXYZPNN = self.aoLightValueScratchXYPN;
                self.aoBrightnessXYZPNN = self.aoBrightnessXYPN;
            } else {
                self.aoLightValueScratchXYZPNN = self.ao_light(var2 + 1, var3, var4 - 1);
                self.aoBrightnessXYZPNN = self.mixed(var2 + 1, var3, var4 - 1);
            }
            if !var18 && !var16 {
                self.aoLightValueScratchXYZPNP = self.aoLightValueScratchXYPN;
                self.aoBrightnessXYZPNP = self.aoBrightnessXYPN;
            } else {
                self.aoLightValueScratchXYZPNP = self.ao_light(var2 + 1, var3, var4 + 1);
                self.aoBrightnessXYZPNP = self.mixed(var2 + 1, var3, var4 + 1);
            }
            if self.renderMinY <= 0.0f64 {
                var3 += 1;
            }
            let mut var20: i32 = var14;
            if self.renderMinY <= 0.0f64 || !self.is_opaque(var2, var3 - 1, var4) {
                var20 = self.mixed(var2, var3 - 1, var4);
            }
            let mut var21: f32 = self.ao_light(var2, var3 - 1, var4);
            var9 = (self.aoLightValueScratchXYZNNP
                + self.aoLightValueScratchXYNN
                + self.aoLightValueScratchYZNP
                + var21)
                / 4.0f32;
            var12 = (self.aoLightValueScratchYZNP
                + var21
                + self.aoLightValueScratchXYZPNP
                + self.aoLightValueScratchXYPN)
                / 4.0f32;
            var11 = (var21
                + self.aoLightValueScratchYZNN
                + self.aoLightValueScratchXYPN
                + self.aoLightValueScratchXYZPNN)
                / 4.0f32;
            var10 = (self.aoLightValueScratchXYNN
                + self.aoLightValueScratchXYZNNN
                + var21
                + self.aoLightValueScratchYZNN)
                / 4.0f32;
            self.brightnessTopLeft = self.getAoBrightness(
                self.aoBrightnessXYZNNP,
                self.aoBrightnessXYNN,
                self.aoBrightnessYZNP,
                var20,
            );
            self.brightnessTopRight = self.getAoBrightness(
                self.aoBrightnessYZNP,
                self.aoBrightnessXYZPNP,
                self.aoBrightnessXYPN,
                var20,
            );
            self.brightnessBottomRight = self.getAoBrightness(
                self.aoBrightnessYZNN,
                self.aoBrightnessXYPN,
                self.aoBrightnessXYZPNN,
                var20,
            );
            self.brightnessBottomLeft = self.getAoBrightness(
                self.aoBrightnessXYNN,
                self.aoBrightnessXYZNNN,
                self.aoBrightnessYZNN,
                var20,
            );
            if var13 {
                self.colorRedTopRight = var5 * 0.5f32;
                self.colorRedBottomRight = self.colorRedTopRight;
                self.colorRedBottomLeft = self.colorRedBottomRight;
                self.colorRedTopLeft = self.colorRedBottomLeft;
                self.colorGreenTopRight = var6 * 0.5f32;
                self.colorGreenBottomRight = self.colorGreenTopRight;
                self.colorGreenBottomLeft = self.colorGreenBottomRight;
                self.colorGreenTopLeft = self.colorGreenBottomLeft;
                self.colorBlueTopRight = var7 * 0.5f32;
                self.colorBlueBottomRight = self.colorBlueTopRight;
                self.colorBlueBottomLeft = self.colorBlueBottomRight;
                self.colorBlueTopLeft = self.colorBlueBottomLeft;
            } else {
                self.colorRedTopRight = 0.5f32;
                self.colorRedBottomRight = self.colorRedTopRight;
                self.colorRedBottomLeft = self.colorRedBottomRight;
                self.colorRedTopLeft = self.colorRedBottomLeft;
                self.colorGreenTopRight = 0.5f32;
                self.colorGreenBottomRight = self.colorGreenTopRight;
                self.colorGreenBottomLeft = self.colorGreenBottomRight;
                self.colorGreenTopLeft = self.colorGreenBottomLeft;
                self.colorBlueTopRight = 0.5f32;
                self.colorBlueBottomRight = self.colorBlueTopRight;
                self.colorBlueBottomLeft = self.colorBlueBottomRight;
                self.colorBlueTopLeft = self.colorBlueBottomLeft;
            }
            self.colorRedTopLeft = self.colorRedTopLeft * (var9);
            self.colorGreenTopLeft = self.colorGreenTopLeft * (var9);
            self.colorBlueTopLeft = self.colorBlueTopLeft * (var9);
            self.colorRedBottomLeft = self.colorRedBottomLeft * (var10);
            self.colorGreenBottomLeft = self.colorGreenBottomLeft * (var10);
            self.colorBlueBottomLeft = self.colorBlueBottomLeft * (var10);
            self.colorRedBottomRight = self.colorRedBottomRight * (var11);
            self.colorGreenBottomRight = self.colorGreenBottomRight * (var11);
            self.colorBlueBottomRight = self.colorBlueBottomRight * (var11);
            self.colorRedTopRight = self.colorRedTopRight * (var12);
            self.colorGreenTopRight = self.colorGreenTopRight * (var12);
            self.colorBlueTopRight = self.colorBlueTopRight * (var12);
            self.face_y_neg(
                (var2 as f64),
                (var3 as f64),
                (var4 as f64),
                self.icon(var2, var3, var4, 0),
            );
            var8 = true;
        }
        if self.renderAllFaces || self.should_side(var2, var3 + 1, var4, 1) {
            if self.renderMaxY >= 1.0f64 {
                var3 += 1;
            }
            self.aoBrightnessXYNP = self.mixed(var2 - 1, var3, var4);
            self.aoBrightnessXYPP = self.mixed(var2 + 1, var3, var4);
            self.aoBrightnessYZPN = self.mixed(var2, var3, var4 - 1);
            self.aoBrightnessYZPP = self.mixed(var2, var3, var4 + 1);
            self.aoLightValueScratchXYNP = self.ao_light(var2 - 1, var3, var4);
            self.aoLightValueScratchXYPP = self.ao_light(var2 + 1, var3, var4);
            self.aoLightValueScratchYZPN = self.ao_light(var2, var3, var4 - 1);
            self.aoLightValueScratchYZPP = self.ao_light(var2, var3, var4 + 1);
            let mut var54: bool = self.can_block_grass(var2 + 1, var3 + 1, var4);
            let mut var59: bool = self.can_block_grass(var2 - 1, var3 + 1, var4);
            let mut var64: bool = self.can_block_grass(var2, var3 + 1, var4 + 1);
            let mut var69: bool = self.can_block_grass(var2, var3 + 1, var4 - 1);
            if !var69 && !var59 {
                self.aoLightValueScratchXYZNPN = self.aoLightValueScratchXYNP;
                self.aoBrightnessXYZNPN = self.aoBrightnessXYNP;
            } else {
                self.aoLightValueScratchXYZNPN = self.ao_light(var2 - 1, var3, var4 - 1);
                self.aoBrightnessXYZNPN = self.mixed(var2 - 1, var3, var4 - 1);
            }
            if !var69 && !var54 {
                self.aoLightValueScratchXYZPPN = self.aoLightValueScratchXYPP;
                self.aoBrightnessXYZPPN = self.aoBrightnessXYPP;
            } else {
                self.aoLightValueScratchXYZPPN = self.ao_light(var2 + 1, var3, var4 - 1);
                self.aoBrightnessXYZPPN = self.mixed(var2 + 1, var3, var4 - 1);
            }
            if !var64 && !var59 {
                self.aoLightValueScratchXYZNPP = self.aoLightValueScratchXYNP;
                self.aoBrightnessXYZNPP = self.aoBrightnessXYNP;
            } else {
                self.aoLightValueScratchXYZNPP = self.ao_light(var2 - 1, var3, var4 + 1);
                self.aoBrightnessXYZNPP = self.mixed(var2 - 1, var3, var4 + 1);
            }
            if !var64 && !var54 {
                self.aoLightValueScratchXYZPPP = self.aoLightValueScratchXYPP;
                self.aoBrightnessXYZPPP = self.aoBrightnessXYPP;
            } else {
                self.aoLightValueScratchXYZPPP = self.ao_light(var2 + 1, var3, var4 + 1);
                self.aoBrightnessXYZPPP = self.mixed(var2 + 1, var3, var4 + 1);
            }
            if self.renderMaxY >= 1.0f64 {
                var3 -= 1;
            }
            let mut var74: i32 = var14;
            if self.renderMaxY >= 1.0f64 || !self.is_opaque(var2, var3 + 1, var4) {
                var74 = self.mixed(var2, var3 + 1, var4);
            }
            let mut var79: f32 = self.ao_light(var2, var3 + 1, var4);
            var12 = (self.aoLightValueScratchXYZNPP
                + self.aoLightValueScratchXYNP
                + self.aoLightValueScratchYZPP
                + var79)
                / 4.0f32;
            var9 = (self.aoLightValueScratchYZPP
                + var79
                + self.aoLightValueScratchXYZPPP
                + self.aoLightValueScratchXYPP)
                / 4.0f32;
            var10 = (var79
                + self.aoLightValueScratchYZPN
                + self.aoLightValueScratchXYPP
                + self.aoLightValueScratchXYZPPN)
                / 4.0f32;
            var11 = (self.aoLightValueScratchXYNP
                + self.aoLightValueScratchXYZNPN
                + var79
                + self.aoLightValueScratchYZPN)
                / 4.0f32;
            self.brightnessTopRight = self.getAoBrightness(
                self.aoBrightnessXYZNPP,
                self.aoBrightnessXYNP,
                self.aoBrightnessYZPP,
                var74,
            );
            self.brightnessTopLeft = self.getAoBrightness(
                self.aoBrightnessYZPP,
                self.aoBrightnessXYZPPP,
                self.aoBrightnessXYPP,
                var74,
            );
            self.brightnessBottomLeft = self.getAoBrightness(
                self.aoBrightnessYZPN,
                self.aoBrightnessXYPP,
                self.aoBrightnessXYZPPN,
                var74,
            );
            self.brightnessBottomRight = self.getAoBrightness(
                self.aoBrightnessXYNP,
                self.aoBrightnessXYZNPN,
                self.aoBrightnessYZPN,
                var74,
            );
            self.colorRedTopRight = var5;
            self.colorRedBottomRight = self.colorRedTopRight;
            self.colorRedBottomLeft = self.colorRedBottomRight;
            self.colorRedTopLeft = self.colorRedBottomLeft;
            self.colorGreenTopRight = var6;
            self.colorGreenBottomRight = self.colorGreenTopRight;
            self.colorGreenBottomLeft = self.colorGreenBottomRight;
            self.colorGreenTopLeft = self.colorGreenBottomLeft;
            self.colorBlueTopRight = var7;
            self.colorBlueBottomRight = self.colorBlueTopRight;
            self.colorBlueBottomLeft = self.colorBlueBottomRight;
            self.colorBlueTopLeft = self.colorBlueBottomLeft;
            self.colorRedTopLeft = self.colorRedTopLeft * (var9);
            self.colorGreenTopLeft = self.colorGreenTopLeft * (var9);
            self.colorBlueTopLeft = self.colorBlueTopLeft * (var9);
            self.colorRedBottomLeft = self.colorRedBottomLeft * (var10);
            self.colorGreenBottomLeft = self.colorGreenBottomLeft * (var10);
            self.colorBlueBottomLeft = self.colorBlueBottomLeft * (var10);
            self.colorRedBottomRight = self.colorRedBottomRight * (var11);
            self.colorGreenBottomRight = self.colorGreenBottomRight * (var11);
            self.colorBlueBottomRight = self.colorBlueBottomRight * (var11);
            self.colorRedTopRight = self.colorRedTopRight * (var12);
            self.colorGreenTopRight = self.colorGreenTopRight * (var12);
            self.colorBlueTopRight = self.colorBlueTopRight * (var12);
            self.face_y_pos(
                (var2 as f64),
                (var3 as f64),
                (var4 as f64),
                self.icon(var2, var3, var4, 1),
            );
            var8 = true;
        }
        if self.renderAllFaces || self.should_side(var2, var3, var4 - 1, 2) {
            if self.renderMinZ <= 0.0f64 {
                var4 -= 1;
            }
            self.aoLightValueScratchXZNN = self.ao_light(var2 - 1, var3, var4);
            self.aoLightValueScratchYZNN = self.ao_light(var2, var3 - 1, var4);
            self.aoLightValueScratchYZPN = self.ao_light(var2, var3 + 1, var4);
            self.aoLightValueScratchXZPN = self.ao_light(var2 + 1, var3, var4);
            self.aoBrightnessXZNN = self.mixed(var2 - 1, var3, var4);
            self.aoBrightnessYZNN = self.mixed(var2, var3 - 1, var4);
            self.aoBrightnessYZPN = self.mixed(var2, var3 + 1, var4);
            self.aoBrightnessXZPN = self.mixed(var2 + 1, var3, var4);
            let mut var55: bool = self.can_block_grass(var2 + 1, var3, var4 - 1);
            let mut var60: bool = self.can_block_grass(var2 - 1, var3, var4 - 1);
            let mut var65: bool = self.can_block_grass(var2, var3 + 1, var4 - 1);
            let mut var70: bool = self.can_block_grass(var2, var3 - 1, var4 - 1);
            if !var60 && !var70 {
                self.aoLightValueScratchXYZNNN = self.aoLightValueScratchXZNN;
                self.aoBrightnessXYZNNN = self.aoBrightnessXZNN;
            } else {
                self.aoLightValueScratchXYZNNN = self.ao_light(var2 - 1, var3 - 1, var4);
                self.aoBrightnessXYZNNN = self.mixed(var2 - 1, var3 - 1, var4);
            }
            if !var60 && !var65 {
                self.aoLightValueScratchXYZNPN = self.aoLightValueScratchXZNN;
                self.aoBrightnessXYZNPN = self.aoBrightnessXZNN;
            } else {
                self.aoLightValueScratchXYZNPN = self.ao_light(var2 - 1, var3 + 1, var4);
                self.aoBrightnessXYZNPN = self.mixed(var2 - 1, var3 + 1, var4);
            }
            if !var55 && !var70 {
                self.aoLightValueScratchXYZPNN = self.aoLightValueScratchXZPN;
                self.aoBrightnessXYZPNN = self.aoBrightnessXZPN;
            } else {
                self.aoLightValueScratchXYZPNN = self.ao_light(var2 + 1, var3 - 1, var4);
                self.aoBrightnessXYZPNN = self.mixed(var2 + 1, var3 - 1, var4);
            }
            if !var55 && !var65 {
                self.aoLightValueScratchXYZPPN = self.aoLightValueScratchXZPN;
                self.aoBrightnessXYZPPN = self.aoBrightnessXZPN;
            } else {
                self.aoLightValueScratchXYZPPN = self.ao_light(var2 + 1, var3 + 1, var4);
                self.aoBrightnessXYZPPN = self.mixed(var2 + 1, var3 + 1, var4);
            }
            if self.renderMinZ <= 0.0f64 {
                var4 += 1;
            }
            let mut var75: i32 = var14;
            if self.renderMinZ <= 0.0f64 || !self.is_opaque(var2, var3, var4 - 1) {
                var75 = self.mixed(var2, var3, var4 - 1);
            }
            let mut var80: f32 = self.ao_light(var2, var3, var4 - 1);
            let mut var22: f32 = (self.aoLightValueScratchXZNN
                + self.aoLightValueScratchXYZNPN
                + var80
                + self.aoLightValueScratchYZPN)
                / 4.0f32;
            let mut var23: f32 = (var80
                + self.aoLightValueScratchYZPN
                + self.aoLightValueScratchXZPN
                + self.aoLightValueScratchXYZPPN)
                / 4.0f32;
            let mut var24: f32 = (self.aoLightValueScratchYZNN
                + var80
                + self.aoLightValueScratchXYZPNN
                + self.aoLightValueScratchXZPN)
                / 4.0f32;
            let mut var25: f32 = (self.aoLightValueScratchXYZNNN
                + self.aoLightValueScratchXZNN
                + self.aoLightValueScratchYZNN
                + var80)
                / 4.0f32;
            var9 = (((var22 as f64) * self.renderMaxY * (1.0f64 - self.renderMinX)
                + (var23 as f64) * self.renderMaxY * self.renderMinX
                + (var24 as f64) * (1.0f64 - self.renderMaxY) * self.renderMinX
                + (var25 as f64) * (1.0f64 - self.renderMaxY) * (1.0f64 - self.renderMinX))
                as f32);
            var10 = (((var22 as f64) * self.renderMaxY * (1.0f64 - self.renderMaxX)
                + (var23 as f64) * self.renderMaxY * self.renderMaxX
                + (var24 as f64) * (1.0f64 - self.renderMaxY) * self.renderMaxX
                + (var25 as f64) * (1.0f64 - self.renderMaxY) * (1.0f64 - self.renderMaxX))
                as f32);
            var11 = (((var22 as f64) * self.renderMinY * (1.0f64 - self.renderMaxX)
                + (var23 as f64) * self.renderMinY * self.renderMaxX
                + (var24 as f64) * (1.0f64 - self.renderMinY) * self.renderMaxX
                + (var25 as f64) * (1.0f64 - self.renderMinY) * (1.0f64 - self.renderMaxX))
                as f32);
            var12 = (((var22 as f64) * self.renderMinY * (1.0f64 - self.renderMinX)
                + (var23 as f64) * self.renderMinY * self.renderMinX
                + (var24 as f64) * (1.0f64 - self.renderMinY) * self.renderMinX
                + (var25 as f64) * (1.0f64 - self.renderMinY) * (1.0f64 - self.renderMinX))
                as f32);
            let mut var26: i32 = self.getAoBrightness(
                self.aoBrightnessXZNN,
                self.aoBrightnessXYZNPN,
                self.aoBrightnessYZPN,
                var75,
            );
            let mut var27: i32 = self.getAoBrightness(
                self.aoBrightnessYZPN,
                self.aoBrightnessXZPN,
                self.aoBrightnessXYZPPN,
                var75,
            );
            let mut var28: i32 = self.getAoBrightness(
                self.aoBrightnessYZNN,
                self.aoBrightnessXYZPNN,
                self.aoBrightnessXZPN,
                var75,
            );
            let mut var29: i32 = self.getAoBrightness(
                self.aoBrightnessXYZNNN,
                self.aoBrightnessXZNN,
                self.aoBrightnessYZNN,
                var75,
            );
            self.brightnessTopLeft = self.mixAoBrightness(
                var26,
                var27,
                var28,
                var29,
                self.renderMaxY * (1.0f64 - self.renderMinX),
                self.renderMaxY * self.renderMinX,
                (1.0f64 - self.renderMaxY) * self.renderMinX,
                (1.0f64 - self.renderMaxY) * (1.0f64 - self.renderMinX),
            );
            self.brightnessBottomLeft = self.mixAoBrightness(
                var26,
                var27,
                var28,
                var29,
                self.renderMaxY * (1.0f64 - self.renderMaxX),
                self.renderMaxY * self.renderMaxX,
                (1.0f64 - self.renderMaxY) * self.renderMaxX,
                (1.0f64 - self.renderMaxY) * (1.0f64 - self.renderMaxX),
            );
            self.brightnessBottomRight = self.mixAoBrightness(
                var26,
                var27,
                var28,
                var29,
                self.renderMinY * (1.0f64 - self.renderMaxX),
                self.renderMinY * self.renderMaxX,
                (1.0f64 - self.renderMinY) * self.renderMaxX,
                (1.0f64 - self.renderMinY) * (1.0f64 - self.renderMaxX),
            );
            self.brightnessTopRight = self.mixAoBrightness(
                var26,
                var27,
                var28,
                var29,
                self.renderMinY * (1.0f64 - self.renderMinX),
                self.renderMinY * self.renderMinX,
                (1.0f64 - self.renderMinY) * self.renderMinX,
                (1.0f64 - self.renderMinY) * (1.0f64 - self.renderMinX),
            );
            if var13 {
                self.colorRedTopRight = var5 * 0.8f32;
                self.colorRedBottomRight = self.colorRedTopRight;
                self.colorRedBottomLeft = self.colorRedBottomRight;
                self.colorRedTopLeft = self.colorRedBottomLeft;
                self.colorGreenTopRight = var6 * 0.8f32;
                self.colorGreenBottomRight = self.colorGreenTopRight;
                self.colorGreenBottomLeft = self.colorGreenBottomRight;
                self.colorGreenTopLeft = self.colorGreenBottomLeft;
                self.colorBlueTopRight = var7 * 0.8f32;
                self.colorBlueBottomRight = self.colorBlueTopRight;
                self.colorBlueBottomLeft = self.colorBlueBottomRight;
                self.colorBlueTopLeft = self.colorBlueBottomLeft;
            } else {
                self.colorRedTopRight = 0.8f32;
                self.colorRedBottomRight = self.colorRedTopRight;
                self.colorRedBottomLeft = self.colorRedBottomRight;
                self.colorRedTopLeft = self.colorRedBottomLeft;
                self.colorGreenTopRight = 0.8f32;
                self.colorGreenBottomRight = self.colorGreenTopRight;
                self.colorGreenBottomLeft = self.colorGreenBottomRight;
                self.colorGreenTopLeft = self.colorGreenBottomLeft;
                self.colorBlueTopRight = 0.8f32;
                self.colorBlueBottomRight = self.colorBlueTopRight;
                self.colorBlueBottomLeft = self.colorBlueBottomRight;
                self.colorBlueTopLeft = self.colorBlueBottomLeft;
            }
            self.colorRedTopLeft = self.colorRedTopLeft * (var9);
            self.colorGreenTopLeft = self.colorGreenTopLeft * (var9);
            self.colorBlueTopLeft = self.colorBlueTopLeft * (var9);
            self.colorRedBottomLeft = self.colorRedBottomLeft * (var10);
            self.colorGreenBottomLeft = self.colorGreenBottomLeft * (var10);
            self.colorBlueBottomLeft = self.colorBlueBottomLeft * (var10);
            self.colorRedBottomRight = self.colorRedBottomRight * (var11);
            self.colorGreenBottomRight = self.colorGreenBottomRight * (var11);
            self.colorBlueBottomRight = self.colorBlueBottomRight * (var11);
            self.colorRedTopRight = self.colorRedTopRight * (var12);
            self.colorGreenTopRight = self.colorGreenTopRight * (var12);
            self.colorBlueTopRight = self.colorBlueTopRight * (var12);
            let mut var84: Icon = self.icon(var2, var3, var4, 2);
            self.face_z_neg((var2 as f64), (var3 as f64), (var4 as f64), var84);
            if FANCY_GRASS && var84.grass_side && !self.overrideBlockTexture.is_some() {
                self.colorRedTopLeft = self.colorRedTopLeft * (var5);
                self.colorRedBottomLeft = self.colorRedBottomLeft * (var5);
                self.colorRedBottomRight = self.colorRedBottomRight * (var5);
                self.colorRedTopRight = self.colorRedTopRight * (var5);
                self.colorGreenTopLeft = self.colorGreenTopLeft * (var6);
                self.colorGreenBottomLeft = self.colorGreenBottomLeft * (var6);
                self.colorGreenBottomRight = self.colorGreenBottomRight * (var6);
                self.colorGreenTopRight = self.colorGreenTopRight * (var6);
                self.colorBlueTopLeft = self.colorBlueTopLeft * (var7);
                self.colorBlueBottomLeft = self.colorBlueBottomLeft * (var7);
                self.colorBlueBottomRight = self.colorBlueBottomRight * (var7);
                self.colorBlueTopRight = self.colorBlueTopRight * (var7);
                self.face_z_neg(
                    (var2 as f64),
                    (var3 as f64),
                    (var4 as f64),
                    self.grass_overlay(),
                );
            }
            var8 = true;
        }
        if self.renderAllFaces || self.should_side(var2, var3, var4 + 1, 3) {
            if self.renderMaxZ >= 1.0f64 {
                var4 += 1;
            }
            self.aoLightValueScratchXZNP = self.ao_light(var2 - 1, var3, var4);
            self.aoLightValueScratchXZPP = self.ao_light(var2 + 1, var3, var4);
            self.aoLightValueScratchYZNP = self.ao_light(var2, var3 - 1, var4);
            self.aoLightValueScratchYZPP = self.ao_light(var2, var3 + 1, var4);
            self.aoBrightnessXZNP = self.mixed(var2 - 1, var3, var4);
            self.aoBrightnessXZPP = self.mixed(var2 + 1, var3, var4);
            self.aoBrightnessYZNP = self.mixed(var2, var3 - 1, var4);
            self.aoBrightnessYZPP = self.mixed(var2, var3 + 1, var4);
            let mut var56: bool = self.can_block_grass(var2 + 1, var3, var4 + 1);
            let mut var61: bool = self.can_block_grass(var2 - 1, var3, var4 + 1);
            let mut var66: bool = self.can_block_grass(var2, var3 + 1, var4 + 1);
            let mut var71: bool = self.can_block_grass(var2, var3 - 1, var4 + 1);
            if !var61 && !var71 {
                self.aoLightValueScratchXYZNNP = self.aoLightValueScratchXZNP;
                self.aoBrightnessXYZNNP = self.aoBrightnessXZNP;
            } else {
                self.aoLightValueScratchXYZNNP = self.ao_light(var2 - 1, var3 - 1, var4);
                self.aoBrightnessXYZNNP = self.mixed(var2 - 1, var3 - 1, var4);
            }
            if !var61 && !var66 {
                self.aoLightValueScratchXYZNPP = self.aoLightValueScratchXZNP;
                self.aoBrightnessXYZNPP = self.aoBrightnessXZNP;
            } else {
                self.aoLightValueScratchXYZNPP = self.ao_light(var2 - 1, var3 + 1, var4);
                self.aoBrightnessXYZNPP = self.mixed(var2 - 1, var3 + 1, var4);
            }
            if !var56 && !var71 {
                self.aoLightValueScratchXYZPNP = self.aoLightValueScratchXZPP;
                self.aoBrightnessXYZPNP = self.aoBrightnessXZPP;
            } else {
                self.aoLightValueScratchXYZPNP = self.ao_light(var2 + 1, var3 - 1, var4);
                self.aoBrightnessXYZPNP = self.mixed(var2 + 1, var3 - 1, var4);
            }
            if !var56 && !var66 {
                self.aoLightValueScratchXYZPPP = self.aoLightValueScratchXZPP;
                self.aoBrightnessXYZPPP = self.aoBrightnessXZPP;
            } else {
                self.aoLightValueScratchXYZPPP = self.ao_light(var2 + 1, var3 + 1, var4);
                self.aoBrightnessXYZPPP = self.mixed(var2 + 1, var3 + 1, var4);
            }
            if self.renderMaxZ >= 1.0f64 {
                var4 -= 1;
            }
            let mut var76: i32 = var14;
            if self.renderMaxZ >= 1.0f64 || !self.is_opaque(var2, var3, var4 + 1) {
                var76 = self.mixed(var2, var3, var4 + 1);
            }
            let mut var81: f32 = self.ao_light(var2, var3, var4 + 1);
            let mut var85: f32 = (self.aoLightValueScratchXZNP
                + self.aoLightValueScratchXYZNPP
                + var81
                + self.aoLightValueScratchYZPP)
                / 4.0f32;
            let mut var91: f32 = (var81
                + self.aoLightValueScratchYZPP
                + self.aoLightValueScratchXZPP
                + self.aoLightValueScratchXYZPPP)
                / 4.0f32;
            let mut var94: f32 = (self.aoLightValueScratchYZNP
                + var81
                + self.aoLightValueScratchXYZPNP
                + self.aoLightValueScratchXZPP)
                / 4.0f32;
            let mut var97: f32 = (self.aoLightValueScratchXYZNNP
                + self.aoLightValueScratchXZNP
                + self.aoLightValueScratchYZNP
                + var81)
                / 4.0f32;
            var9 = (((var85 as f64) * self.renderMaxY * (1.0f64 - self.renderMinX)
                + (var91 as f64) * self.renderMaxY * self.renderMinX
                + (var94 as f64) * (1.0f64 - self.renderMaxY) * self.renderMinX
                + (var97 as f64) * (1.0f64 - self.renderMaxY) * (1.0f64 - self.renderMinX))
                as f32);
            var10 = (((var85 as f64) * self.renderMinY * (1.0f64 - self.renderMinX)
                + (var91 as f64) * self.renderMinY * self.renderMinX
                + (var94 as f64) * (1.0f64 - self.renderMinY) * self.renderMinX
                + (var97 as f64) * (1.0f64 - self.renderMinY) * (1.0f64 - self.renderMinX))
                as f32);
            var11 = (((var85 as f64) * self.renderMinY * (1.0f64 - self.renderMaxX)
                + (var91 as f64) * self.renderMinY * self.renderMaxX
                + (var94 as f64) * (1.0f64 - self.renderMinY) * self.renderMaxX
                + (var97 as f64) * (1.0f64 - self.renderMinY) * (1.0f64 - self.renderMaxX))
                as f32);
            var12 = (((var85 as f64) * self.renderMaxY * (1.0f64 - self.renderMaxX)
                + (var91 as f64) * self.renderMaxY * self.renderMaxX
                + (var94 as f64) * (1.0f64 - self.renderMaxY) * self.renderMaxX
                + (var97 as f64) * (1.0f64 - self.renderMaxY) * (1.0f64 - self.renderMaxX))
                as f32);
            let mut var100: i32 = self.getAoBrightness(
                self.aoBrightnessXZNP,
                self.aoBrightnessXYZNPP,
                self.aoBrightnessYZPP,
                var76,
            );
            let mut var103: i32 = self.getAoBrightness(
                self.aoBrightnessYZPP,
                self.aoBrightnessXZPP,
                self.aoBrightnessXYZPPP,
                var76,
            );
            let mut var106: i32 = self.getAoBrightness(
                self.aoBrightnessYZNP,
                self.aoBrightnessXYZPNP,
                self.aoBrightnessXZPP,
                var76,
            );
            let mut var109: i32 = self.getAoBrightness(
                self.aoBrightnessXYZNNP,
                self.aoBrightnessXZNP,
                self.aoBrightnessYZNP,
                var76,
            );
            self.brightnessTopLeft = self.mixAoBrightness(
                var100,
                var109,
                var106,
                var103,
                self.renderMaxY * (1.0f64 - self.renderMinX),
                (1.0f64 - self.renderMaxY) * (1.0f64 - self.renderMinX),
                (1.0f64 - self.renderMaxY) * self.renderMinX,
                self.renderMaxY * self.renderMinX,
            );
            self.brightnessBottomLeft = self.mixAoBrightness(
                var100,
                var109,
                var106,
                var103,
                self.renderMinY * (1.0f64 - self.renderMinX),
                (1.0f64 - self.renderMinY) * (1.0f64 - self.renderMinX),
                (1.0f64 - self.renderMinY) * self.renderMinX,
                self.renderMinY * self.renderMinX,
            );
            self.brightnessBottomRight = self.mixAoBrightness(
                var100,
                var109,
                var106,
                var103,
                self.renderMinY * (1.0f64 - self.renderMaxX),
                (1.0f64 - self.renderMinY) * (1.0f64 - self.renderMaxX),
                (1.0f64 - self.renderMinY) * self.renderMaxX,
                self.renderMinY * self.renderMaxX,
            );
            self.brightnessTopRight = self.mixAoBrightness(
                var100,
                var109,
                var106,
                var103,
                self.renderMaxY * (1.0f64 - self.renderMaxX),
                (1.0f64 - self.renderMaxY) * (1.0f64 - self.renderMaxX),
                (1.0f64 - self.renderMaxY) * self.renderMaxX,
                self.renderMaxY * self.renderMaxX,
            );
            if var13 {
                self.colorRedTopRight = var5 * 0.8f32;
                self.colorRedBottomRight = self.colorRedTopRight;
                self.colorRedBottomLeft = self.colorRedBottomRight;
                self.colorRedTopLeft = self.colorRedBottomLeft;
                self.colorGreenTopRight = var6 * 0.8f32;
                self.colorGreenBottomRight = self.colorGreenTopRight;
                self.colorGreenBottomLeft = self.colorGreenBottomRight;
                self.colorGreenTopLeft = self.colorGreenBottomLeft;
                self.colorBlueTopRight = var7 * 0.8f32;
                self.colorBlueBottomRight = self.colorBlueTopRight;
                self.colorBlueBottomLeft = self.colorBlueBottomRight;
                self.colorBlueTopLeft = self.colorBlueBottomLeft;
            } else {
                self.colorRedTopRight = 0.8f32;
                self.colorRedBottomRight = self.colorRedTopRight;
                self.colorRedBottomLeft = self.colorRedBottomRight;
                self.colorRedTopLeft = self.colorRedBottomLeft;
                self.colorGreenTopRight = 0.8f32;
                self.colorGreenBottomRight = self.colorGreenTopRight;
                self.colorGreenBottomLeft = self.colorGreenBottomRight;
                self.colorGreenTopLeft = self.colorGreenBottomLeft;
                self.colorBlueTopRight = 0.8f32;
                self.colorBlueBottomRight = self.colorBlueTopRight;
                self.colorBlueBottomLeft = self.colorBlueBottomRight;
                self.colorBlueTopLeft = self.colorBlueBottomLeft;
            }
            self.colorRedTopLeft = self.colorRedTopLeft * (var9);
            self.colorGreenTopLeft = self.colorGreenTopLeft * (var9);
            self.colorBlueTopLeft = self.colorBlueTopLeft * (var9);
            self.colorRedBottomLeft = self.colorRedBottomLeft * (var10);
            self.colorGreenBottomLeft = self.colorGreenBottomLeft * (var10);
            self.colorBlueBottomLeft = self.colorBlueBottomLeft * (var10);
            self.colorRedBottomRight = self.colorRedBottomRight * (var11);
            self.colorGreenBottomRight = self.colorGreenBottomRight * (var11);
            self.colorBlueBottomRight = self.colorBlueBottomRight * (var11);
            self.colorRedTopRight = self.colorRedTopRight * (var12);
            self.colorGreenTopRight = self.colorGreenTopRight * (var12);
            self.colorBlueTopRight = self.colorBlueTopRight * (var12);
            let mut var86: Icon = self.icon(var2, var3, var4, 3);
            self.face_z_pos((var2 as f64), (var3 as f64), (var4 as f64), var86);
            if FANCY_GRASS && var86.grass_side && !self.overrideBlockTexture.is_some() {
                self.colorRedTopLeft = self.colorRedTopLeft * (var5);
                self.colorRedBottomLeft = self.colorRedBottomLeft * (var5);
                self.colorRedBottomRight = self.colorRedBottomRight * (var5);
                self.colorRedTopRight = self.colorRedTopRight * (var5);
                self.colorGreenTopLeft = self.colorGreenTopLeft * (var6);
                self.colorGreenBottomLeft = self.colorGreenBottomLeft * (var6);
                self.colorGreenBottomRight = self.colorGreenBottomRight * (var6);
                self.colorGreenTopRight = self.colorGreenTopRight * (var6);
                self.colorBlueTopLeft = self.colorBlueTopLeft * (var7);
                self.colorBlueBottomLeft = self.colorBlueBottomLeft * (var7);
                self.colorBlueBottomRight = self.colorBlueBottomRight * (var7);
                self.colorBlueTopRight = self.colorBlueTopRight * (var7);
                self.face_z_pos(
                    (var2 as f64),
                    (var3 as f64),
                    (var4 as f64),
                    self.grass_overlay(),
                );
            }
            var8 = true;
        }
        if self.renderAllFaces || self.should_side(var2 - 1, var3, var4, 4) {
            if self.renderMinX <= 0.0f64 {
                var2 -= 1;
            }
            self.aoLightValueScratchXYNN = self.ao_light(var2, var3 - 1, var4);
            self.aoLightValueScratchXZNN = self.ao_light(var2, var3, var4 - 1);
            self.aoLightValueScratchXZNP = self.ao_light(var2, var3, var4 + 1);
            self.aoLightValueScratchXYNP = self.ao_light(var2, var3 + 1, var4);
            self.aoBrightnessXYNN = self.mixed(var2, var3 - 1, var4);
            self.aoBrightnessXZNN = self.mixed(var2, var3, var4 - 1);
            self.aoBrightnessXZNP = self.mixed(var2, var3, var4 + 1);
            self.aoBrightnessXYNP = self.mixed(var2, var3 + 1, var4);
            let mut var57: bool = self.can_block_grass(var2 - 1, var3 + 1, var4);
            let mut var62: bool = self.can_block_grass(var2 - 1, var3 - 1, var4);
            let mut var67: bool = self.can_block_grass(var2 - 1, var3, var4 - 1);
            let mut var72: bool = self.can_block_grass(var2 - 1, var3, var4 + 1);
            if !var67 && !var62 {
                self.aoLightValueScratchXYZNNN = self.aoLightValueScratchXZNN;
                self.aoBrightnessXYZNNN = self.aoBrightnessXZNN;
            } else {
                self.aoLightValueScratchXYZNNN = self.ao_light(var2, var3 - 1, var4 - 1);
                self.aoBrightnessXYZNNN = self.mixed(var2, var3 - 1, var4 - 1);
            }
            if !var72 && !var62 {
                self.aoLightValueScratchXYZNNP = self.aoLightValueScratchXZNP;
                self.aoBrightnessXYZNNP = self.aoBrightnessXZNP;
            } else {
                self.aoLightValueScratchXYZNNP = self.ao_light(var2, var3 - 1, var4 + 1);
                self.aoBrightnessXYZNNP = self.mixed(var2, var3 - 1, var4 + 1);
            }
            if !var67 && !var57 {
                self.aoLightValueScratchXYZNPN = self.aoLightValueScratchXZNN;
                self.aoBrightnessXYZNPN = self.aoBrightnessXZNN;
            } else {
                self.aoLightValueScratchXYZNPN = self.ao_light(var2, var3 + 1, var4 - 1);
                self.aoBrightnessXYZNPN = self.mixed(var2, var3 + 1, var4 - 1);
            }
            if !var72 && !var57 {
                self.aoLightValueScratchXYZNPP = self.aoLightValueScratchXZNP;
                self.aoBrightnessXYZNPP = self.aoBrightnessXZNP;
            } else {
                self.aoLightValueScratchXYZNPP = self.ao_light(var2, var3 + 1, var4 + 1);
                self.aoBrightnessXYZNPP = self.mixed(var2, var3 + 1, var4 + 1);
            }
            if self.renderMinX <= 0.0f64 {
                var2 += 1;
            }
            let mut var77: i32 = var14;
            if self.renderMinX <= 0.0f64 || !self.is_opaque(var2 - 1, var3, var4) {
                var77 = self.mixed(var2 - 1, var3, var4);
            }
            let mut var82: f32 = self.ao_light(var2 - 1, var3, var4);
            let mut var87: f32 = (self.aoLightValueScratchXYNN
                + self.aoLightValueScratchXYZNNP
                + var82
                + self.aoLightValueScratchXZNP)
                / 4.0f32;
            let mut var92: f32 = (var82
                + self.aoLightValueScratchXZNP
                + self.aoLightValueScratchXYNP
                + self.aoLightValueScratchXYZNPP)
                / 4.0f32;
            let mut var95: f32 = (self.aoLightValueScratchXZNN
                + var82
                + self.aoLightValueScratchXYZNPN
                + self.aoLightValueScratchXYNP)
                / 4.0f32;
            let mut var98: f32 = (self.aoLightValueScratchXYZNNN
                + self.aoLightValueScratchXYNN
                + self.aoLightValueScratchXZNN
                + var82)
                / 4.0f32;
            var9 = (((var92 as f64) * self.renderMaxY * self.renderMaxZ
                + (var95 as f64) * self.renderMaxY * (1.0f64 - self.renderMaxZ)
                + (var98 as f64) * (1.0f64 - self.renderMaxY) * (1.0f64 - self.renderMaxZ)
                + (var87 as f64) * (1.0f64 - self.renderMaxY) * self.renderMaxZ)
                as f32);
            var10 = (((var92 as f64) * self.renderMaxY * self.renderMinZ
                + (var95 as f64) * self.renderMaxY * (1.0f64 - self.renderMinZ)
                + (var98 as f64) * (1.0f64 - self.renderMaxY) * (1.0f64 - self.renderMinZ)
                + (var87 as f64) * (1.0f64 - self.renderMaxY) * self.renderMinZ)
                as f32);
            var11 = (((var92 as f64) * self.renderMinY * self.renderMinZ
                + (var95 as f64) * self.renderMinY * (1.0f64 - self.renderMinZ)
                + (var98 as f64) * (1.0f64 - self.renderMinY) * (1.0f64 - self.renderMinZ)
                + (var87 as f64) * (1.0f64 - self.renderMinY) * self.renderMinZ)
                as f32);
            var12 = (((var92 as f64) * self.renderMinY * self.renderMaxZ
                + (var95 as f64) * self.renderMinY * (1.0f64 - self.renderMaxZ)
                + (var98 as f64) * (1.0f64 - self.renderMinY) * (1.0f64 - self.renderMaxZ)
                + (var87 as f64) * (1.0f64 - self.renderMinY) * self.renderMaxZ)
                as f32);
            let mut var101: i32 = self.getAoBrightness(
                self.aoBrightnessXYNN,
                self.aoBrightnessXYZNNP,
                self.aoBrightnessXZNP,
                var77,
            );
            let mut var104: i32 = self.getAoBrightness(
                self.aoBrightnessXZNP,
                self.aoBrightnessXYNP,
                self.aoBrightnessXYZNPP,
                var77,
            );
            let mut var107: i32 = self.getAoBrightness(
                self.aoBrightnessXZNN,
                self.aoBrightnessXYZNPN,
                self.aoBrightnessXYNP,
                var77,
            );
            let mut var110: i32 = self.getAoBrightness(
                self.aoBrightnessXYZNNN,
                self.aoBrightnessXYNN,
                self.aoBrightnessXZNN,
                var77,
            );
            self.brightnessTopLeft = self.mixAoBrightness(
                var104,
                var107,
                var110,
                var101,
                self.renderMaxY * self.renderMaxZ,
                self.renderMaxY * (1.0f64 - self.renderMaxZ),
                (1.0f64 - self.renderMaxY) * (1.0f64 - self.renderMaxZ),
                (1.0f64 - self.renderMaxY) * self.renderMaxZ,
            );
            self.brightnessBottomLeft = self.mixAoBrightness(
                var104,
                var107,
                var110,
                var101,
                self.renderMaxY * self.renderMinZ,
                self.renderMaxY * (1.0f64 - self.renderMinZ),
                (1.0f64 - self.renderMaxY) * (1.0f64 - self.renderMinZ),
                (1.0f64 - self.renderMaxY) * self.renderMinZ,
            );
            self.brightnessBottomRight = self.mixAoBrightness(
                var104,
                var107,
                var110,
                var101,
                self.renderMinY * self.renderMinZ,
                self.renderMinY * (1.0f64 - self.renderMinZ),
                (1.0f64 - self.renderMinY) * (1.0f64 - self.renderMinZ),
                (1.0f64 - self.renderMinY) * self.renderMinZ,
            );
            self.brightnessTopRight = self.mixAoBrightness(
                var104,
                var107,
                var110,
                var101,
                self.renderMinY * self.renderMaxZ,
                self.renderMinY * (1.0f64 - self.renderMaxZ),
                (1.0f64 - self.renderMinY) * (1.0f64 - self.renderMaxZ),
                (1.0f64 - self.renderMinY) * self.renderMaxZ,
            );
            if var13 {
                self.colorRedTopRight = var5 * 0.6f32;
                self.colorRedBottomRight = self.colorRedTopRight;
                self.colorRedBottomLeft = self.colorRedBottomRight;
                self.colorRedTopLeft = self.colorRedBottomLeft;
                self.colorGreenTopRight = var6 * 0.6f32;
                self.colorGreenBottomRight = self.colorGreenTopRight;
                self.colorGreenBottomLeft = self.colorGreenBottomRight;
                self.colorGreenTopLeft = self.colorGreenBottomLeft;
                self.colorBlueTopRight = var7 * 0.6f32;
                self.colorBlueBottomRight = self.colorBlueTopRight;
                self.colorBlueBottomLeft = self.colorBlueBottomRight;
                self.colorBlueTopLeft = self.colorBlueBottomLeft;
            } else {
                self.colorRedTopRight = 0.6f32;
                self.colorRedBottomRight = self.colorRedTopRight;
                self.colorRedBottomLeft = self.colorRedBottomRight;
                self.colorRedTopLeft = self.colorRedBottomLeft;
                self.colorGreenTopRight = 0.6f32;
                self.colorGreenBottomRight = self.colorGreenTopRight;
                self.colorGreenBottomLeft = self.colorGreenBottomRight;
                self.colorGreenTopLeft = self.colorGreenBottomLeft;
                self.colorBlueTopRight = 0.6f32;
                self.colorBlueBottomRight = self.colorBlueTopRight;
                self.colorBlueBottomLeft = self.colorBlueBottomRight;
                self.colorBlueTopLeft = self.colorBlueBottomLeft;
            }
            self.colorRedTopLeft = self.colorRedTopLeft * (var9);
            self.colorGreenTopLeft = self.colorGreenTopLeft * (var9);
            self.colorBlueTopLeft = self.colorBlueTopLeft * (var9);
            self.colorRedBottomLeft = self.colorRedBottomLeft * (var10);
            self.colorGreenBottomLeft = self.colorGreenBottomLeft * (var10);
            self.colorBlueBottomLeft = self.colorBlueBottomLeft * (var10);
            self.colorRedBottomRight = self.colorRedBottomRight * (var11);
            self.colorGreenBottomRight = self.colorGreenBottomRight * (var11);
            self.colorBlueBottomRight = self.colorBlueBottomRight * (var11);
            self.colorRedTopRight = self.colorRedTopRight * (var12);
            self.colorGreenTopRight = self.colorGreenTopRight * (var12);
            self.colorBlueTopRight = self.colorBlueTopRight * (var12);
            let mut var88: Icon = self.icon(var2, var3, var4, 4);
            self.face_x_neg((var2 as f64), (var3 as f64), (var4 as f64), var88);
            if FANCY_GRASS && var88.grass_side && !self.overrideBlockTexture.is_some() {
                self.colorRedTopLeft = self.colorRedTopLeft * (var5);
                self.colorRedBottomLeft = self.colorRedBottomLeft * (var5);
                self.colorRedBottomRight = self.colorRedBottomRight * (var5);
                self.colorRedTopRight = self.colorRedTopRight * (var5);
                self.colorGreenTopLeft = self.colorGreenTopLeft * (var6);
                self.colorGreenBottomLeft = self.colorGreenBottomLeft * (var6);
                self.colorGreenBottomRight = self.colorGreenBottomRight * (var6);
                self.colorGreenTopRight = self.colorGreenTopRight * (var6);
                self.colorBlueTopLeft = self.colorBlueTopLeft * (var7);
                self.colorBlueBottomLeft = self.colorBlueBottomLeft * (var7);
                self.colorBlueBottomRight = self.colorBlueBottomRight * (var7);
                self.colorBlueTopRight = self.colorBlueTopRight * (var7);
                self.face_x_neg(
                    (var2 as f64),
                    (var3 as f64),
                    (var4 as f64),
                    self.grass_overlay(),
                );
            }
            var8 = true;
        }
        if self.renderAllFaces || self.should_side(var2 + 1, var3, var4, 5) {
            if self.renderMaxX >= 1.0f64 {
                var2 += 1;
            }
            self.aoLightValueScratchXYPN = self.ao_light(var2, var3 - 1, var4);
            self.aoLightValueScratchXZPN = self.ao_light(var2, var3, var4 - 1);
            self.aoLightValueScratchXZPP = self.ao_light(var2, var3, var4 + 1);
            self.aoLightValueScratchXYPP = self.ao_light(var2, var3 + 1, var4);
            self.aoBrightnessXYPN = self.mixed(var2, var3 - 1, var4);
            self.aoBrightnessXZPN = self.mixed(var2, var3, var4 - 1);
            self.aoBrightnessXZPP = self.mixed(var2, var3, var4 + 1);
            self.aoBrightnessXYPP = self.mixed(var2, var3 + 1, var4);
            let mut var58: bool = self.can_block_grass(var2 + 1, var3 + 1, var4);
            let mut var63: bool = self.can_block_grass(var2 + 1, var3 - 1, var4);
            let mut var68: bool = self.can_block_grass(var2 + 1, var3, var4 + 1);
            let mut var73: bool = self.can_block_grass(var2 + 1, var3, var4 - 1);
            if !var63 && !var73 {
                self.aoLightValueScratchXYZPNN = self.aoLightValueScratchXZPN;
                self.aoBrightnessXYZPNN = self.aoBrightnessXZPN;
            } else {
                self.aoLightValueScratchXYZPNN = self.ao_light(var2, var3 - 1, var4 - 1);
                self.aoBrightnessXYZPNN = self.mixed(var2, var3 - 1, var4 - 1);
            }
            if !var63 && !var68 {
                self.aoLightValueScratchXYZPNP = self.aoLightValueScratchXZPP;
                self.aoBrightnessXYZPNP = self.aoBrightnessXZPP;
            } else {
                self.aoLightValueScratchXYZPNP = self.ao_light(var2, var3 - 1, var4 + 1);
                self.aoBrightnessXYZPNP = self.mixed(var2, var3 - 1, var4 + 1);
            }
            if !var58 && !var73 {
                self.aoLightValueScratchXYZPPN = self.aoLightValueScratchXZPN;
                self.aoBrightnessXYZPPN = self.aoBrightnessXZPN;
            } else {
                self.aoLightValueScratchXYZPPN = self.ao_light(var2, var3 + 1, var4 - 1);
                self.aoBrightnessXYZPPN = self.mixed(var2, var3 + 1, var4 - 1);
            }
            if !var58 && !var68 {
                self.aoLightValueScratchXYZPPP = self.aoLightValueScratchXZPP;
                self.aoBrightnessXYZPPP = self.aoBrightnessXZPP;
            } else {
                self.aoLightValueScratchXYZPPP = self.ao_light(var2, var3 + 1, var4 + 1);
                self.aoBrightnessXYZPPP = self.mixed(var2, var3 + 1, var4 + 1);
            }
            if self.renderMaxX >= 1.0f64 {
                var2 -= 1;
            }
            let mut var78: i32 = var14;
            if self.renderMaxX >= 1.0f64 || !self.is_opaque(var2 + 1, var3, var4) {
                var78 = self.mixed(var2 + 1, var3, var4);
            }
            let mut var83: f32 = self.ao_light(var2 + 1, var3, var4);
            let mut var89: f32 = (self.aoLightValueScratchXYPN
                + self.aoLightValueScratchXYZPNP
                + var83
                + self.aoLightValueScratchXZPP)
                / 4.0f32;
            let mut var93: f32 = (self.aoLightValueScratchXYZPNN
                + self.aoLightValueScratchXYPN
                + self.aoLightValueScratchXZPN
                + var83)
                / 4.0f32;
            let mut var96: f32 = (self.aoLightValueScratchXZPN
                + var83
                + self.aoLightValueScratchXYZPPN
                + self.aoLightValueScratchXYPP)
                / 4.0f32;
            let mut var99: f32 = (var83
                + self.aoLightValueScratchXZPP
                + self.aoLightValueScratchXYPP
                + self.aoLightValueScratchXYZPPP)
                / 4.0f32;
            var9 = (((var89 as f64) * (1.0f64 - self.renderMinY) * self.renderMaxZ
                + (var93 as f64) * (1.0f64 - self.renderMinY) * (1.0f64 - self.renderMaxZ)
                + (var96 as f64) * self.renderMinY * (1.0f64 - self.renderMaxZ)
                + (var99 as f64) * self.renderMinY * self.renderMaxZ) as f32);
            var10 = (((var89 as f64) * (1.0f64 - self.renderMinY) * self.renderMinZ
                + (var93 as f64) * (1.0f64 - self.renderMinY) * (1.0f64 - self.renderMinZ)
                + (var96 as f64) * self.renderMinY * (1.0f64 - self.renderMinZ)
                + (var99 as f64) * self.renderMinY * self.renderMinZ) as f32);
            var11 = (((var89 as f64) * (1.0f64 - self.renderMaxY) * self.renderMinZ
                + (var93 as f64) * (1.0f64 - self.renderMaxY) * (1.0f64 - self.renderMinZ)
                + (var96 as f64) * self.renderMaxY * (1.0f64 - self.renderMinZ)
                + (var99 as f64) * self.renderMaxY * self.renderMinZ) as f32);
            var12 = (((var89 as f64) * (1.0f64 - self.renderMaxY) * self.renderMaxZ
                + (var93 as f64) * (1.0f64 - self.renderMaxY) * (1.0f64 - self.renderMaxZ)
                + (var96 as f64) * self.renderMaxY * (1.0f64 - self.renderMaxZ)
                + (var99 as f64) * self.renderMaxY * self.renderMaxZ) as f32);
            let mut var102: i32 = self.getAoBrightness(
                self.aoBrightnessXYPN,
                self.aoBrightnessXYZPNP,
                self.aoBrightnessXZPP,
                var78,
            );
            let mut var105: i32 = self.getAoBrightness(
                self.aoBrightnessXZPP,
                self.aoBrightnessXYPP,
                self.aoBrightnessXYZPPP,
                var78,
            );
            let mut var108: i32 = self.getAoBrightness(
                self.aoBrightnessXZPN,
                self.aoBrightnessXYZPPN,
                self.aoBrightnessXYPP,
                var78,
            );
            let mut var111: i32 = self.getAoBrightness(
                self.aoBrightnessXYZPNN,
                self.aoBrightnessXYPN,
                self.aoBrightnessXZPN,
                var78,
            );
            self.brightnessTopLeft = self.mixAoBrightness(
                var102,
                var111,
                var108,
                var105,
                (1.0f64 - self.renderMinY) * self.renderMaxZ,
                (1.0f64 - self.renderMinY) * (1.0f64 - self.renderMaxZ),
                self.renderMinY * (1.0f64 - self.renderMaxZ),
                self.renderMinY * self.renderMaxZ,
            );
            self.brightnessBottomLeft = self.mixAoBrightness(
                var102,
                var111,
                var108,
                var105,
                (1.0f64 - self.renderMinY) * self.renderMinZ,
                (1.0f64 - self.renderMinY) * (1.0f64 - self.renderMinZ),
                self.renderMinY * (1.0f64 - self.renderMinZ),
                self.renderMinY * self.renderMinZ,
            );
            self.brightnessBottomRight = self.mixAoBrightness(
                var102,
                var111,
                var108,
                var105,
                (1.0f64 - self.renderMaxY) * self.renderMinZ,
                (1.0f64 - self.renderMaxY) * (1.0f64 - self.renderMinZ),
                self.renderMaxY * (1.0f64 - self.renderMinZ),
                self.renderMaxY * self.renderMinZ,
            );
            self.brightnessTopRight = self.mixAoBrightness(
                var102,
                var111,
                var108,
                var105,
                (1.0f64 - self.renderMaxY) * self.renderMaxZ,
                (1.0f64 - self.renderMaxY) * (1.0f64 - self.renderMaxZ),
                self.renderMaxY * (1.0f64 - self.renderMaxZ),
                self.renderMaxY * self.renderMaxZ,
            );
            if var13 {
                self.colorRedTopRight = var5 * 0.6f32;
                self.colorRedBottomRight = self.colorRedTopRight;
                self.colorRedBottomLeft = self.colorRedBottomRight;
                self.colorRedTopLeft = self.colorRedBottomLeft;
                self.colorGreenTopRight = var6 * 0.6f32;
                self.colorGreenBottomRight = self.colorGreenTopRight;
                self.colorGreenBottomLeft = self.colorGreenBottomRight;
                self.colorGreenTopLeft = self.colorGreenBottomLeft;
                self.colorBlueTopRight = var7 * 0.6f32;
                self.colorBlueBottomRight = self.colorBlueTopRight;
                self.colorBlueBottomLeft = self.colorBlueBottomRight;
                self.colorBlueTopLeft = self.colorBlueBottomLeft;
            } else {
                self.colorRedTopRight = 0.6f32;
                self.colorRedBottomRight = self.colorRedTopRight;
                self.colorRedBottomLeft = self.colorRedBottomRight;
                self.colorRedTopLeft = self.colorRedBottomLeft;
                self.colorGreenTopRight = 0.6f32;
                self.colorGreenBottomRight = self.colorGreenTopRight;
                self.colorGreenBottomLeft = self.colorGreenBottomRight;
                self.colorGreenTopLeft = self.colorGreenBottomLeft;
                self.colorBlueTopRight = 0.6f32;
                self.colorBlueBottomRight = self.colorBlueTopRight;
                self.colorBlueBottomLeft = self.colorBlueBottomRight;
                self.colorBlueTopLeft = self.colorBlueBottomLeft;
            }
            self.colorRedTopLeft = self.colorRedTopLeft * (var9);
            self.colorGreenTopLeft = self.colorGreenTopLeft * (var9);
            self.colorBlueTopLeft = self.colorBlueTopLeft * (var9);
            self.colorRedBottomLeft = self.colorRedBottomLeft * (var10);
            self.colorGreenBottomLeft = self.colorGreenBottomLeft * (var10);
            self.colorBlueBottomLeft = self.colorBlueBottomLeft * (var10);
            self.colorRedBottomRight = self.colorRedBottomRight * (var11);
            self.colorGreenBottomRight = self.colorGreenBottomRight * (var11);
            self.colorBlueBottomRight = self.colorBlueBottomRight * (var11);
            self.colorRedTopRight = self.colorRedTopRight * (var12);
            self.colorGreenTopRight = self.colorGreenTopRight * (var12);
            self.colorBlueTopRight = self.colorBlueTopRight * (var12);
            let mut var90: Icon = self.icon(var2, var3, var4, 5);
            self.face_x_pos((var2 as f64), (var3 as f64), (var4 as f64), var90);
            if FANCY_GRASS && var90.grass_side && !self.overrideBlockTexture.is_some() {
                self.colorRedTopLeft = self.colorRedTopLeft * (var5);
                self.colorRedBottomLeft = self.colorRedBottomLeft * (var5);
                self.colorRedBottomRight = self.colorRedBottomRight * (var5);
                self.colorRedTopRight = self.colorRedTopRight * (var5);
                self.colorGreenTopLeft = self.colorGreenTopLeft * (var6);
                self.colorGreenBottomLeft = self.colorGreenBottomLeft * (var6);
                self.colorGreenBottomRight = self.colorGreenBottomRight * (var6);
                self.colorGreenTopRight = self.colorGreenTopRight * (var6);
                self.colorBlueTopLeft = self.colorBlueTopLeft * (var7);
                self.colorBlueBottomLeft = self.colorBlueBottomLeft * (var7);
                self.colorBlueBottomRight = self.colorBlueBottomRight * (var7);
                self.colorBlueTopRight = self.colorBlueTopRight * (var7);
                self.face_x_pos(
                    (var2 as f64),
                    (var3 as f64),
                    (var4 as f64),
                    self.grass_overlay(),
                );
            }
            var8 = true;
        }
        self.enableAO = false;
        return var8;
    }

    /// `RenderBlocks.renderStandardBlockWithColorMultiplier`
    #[allow(
        unused_assignments,
        unused_mut,
        unused_variables,
        unused_parens,
        clippy::all
    )]
    pub fn std_flat(
        &mut self,
        mut var2: i32,
        mut var3: i32,
        mut var4: i32,
        var5: f32,
        var6: f32,
        var7: f32,
    ) -> bool {
        self.enableAO = false;
        let mut var9: bool = false;
        let mut var10: f32 = 0.5f32;
        let mut var11: f32 = 1.0f32;
        let mut var12: f32 = 0.8f32;
        let mut var13: f32 = 0.6f32;
        let mut var14: f32 = var11 * var5;
        let mut var15: f32 = var11 * var6;
        let mut var16: f32 = var11 * var7;
        let mut var17: f32 = var10;
        let mut var18: f32 = var12;
        let mut var19: f32 = var13;
        let mut var20: f32 = var10;
        let mut var21: f32 = var12;
        let mut var22: f32 = var13;
        let mut var23: f32 = var10;
        let mut var24: f32 = var12;
        let mut var25: f32 = var13;
        if self.id != block::GRASS {
            var17 = var17 * (var5);
            var18 = var18 * (var5);
            var19 = var19 * (var5);
            var20 = var20 * (var6);
            var21 = var21 * (var6);
            var22 = var22 * (var6);
            var23 = var23 * (var7);
            var24 = var24 * (var7);
            var25 = var25 * (var7);
        }
        let mut var26: i32 = self.mixed(var2, var3, var4);
        if self.renderAllFaces || self.should_side(var2, var3 - 1, var4, 0) {
            self.bright = if self.renderMinY > 0.0f64 {
                var26
            } else {
                self.mixed(var2, var3 - 1, var4)
            };
            self.color(var17, var20, var23);
            self.face_y_neg(
                (var2 as f64),
                (var3 as f64),
                (var4 as f64),
                self.icon(var2, var3, var4, 0),
            );
            var9 = true;
        }
        if self.renderAllFaces || self.should_side(var2, var3 + 1, var4, 1) {
            self.bright = if self.renderMaxY < 1.0f64 {
                var26
            } else {
                self.mixed(var2, var3 + 1, var4)
            };
            self.color(var14, var15, var16);
            self.face_y_pos(
                (var2 as f64),
                (var3 as f64),
                (var4 as f64),
                self.icon(var2, var3, var4, 1),
            );
            var9 = true;
        }
        if self.renderAllFaces || self.should_side(var2, var3, var4 - 1, 2) {
            self.bright = if self.renderMinZ > 0.0f64 {
                var26
            } else {
                self.mixed(var2, var3, var4 - 1)
            };
            self.color(var18, var21, var24);
            let mut var27: Icon = self.icon(var2, var3, var4, 2);
            self.face_z_neg((var2 as f64), (var3 as f64), (var4 as f64), var27);
            if FANCY_GRASS && var27.grass_side && !self.overrideBlockTexture.is_some() {
                self.color(var18 * var5, var21 * var6, var24 * var7);
                self.face_z_neg(
                    (var2 as f64),
                    (var3 as f64),
                    (var4 as f64),
                    self.grass_overlay(),
                );
            }
            var9 = true;
        }
        if self.renderAllFaces || self.should_side(var2, var3, var4 + 1, 3) {
            self.bright = if self.renderMaxZ < 1.0f64 {
                var26
            } else {
                self.mixed(var2, var3, var4 + 1)
            };
            self.color(var18, var21, var24);
            let mut var28: Icon = self.icon(var2, var3, var4, 3);
            self.face_z_pos((var2 as f64), (var3 as f64), (var4 as f64), var28);
            if FANCY_GRASS && var28.grass_side && !self.overrideBlockTexture.is_some() {
                self.color(var18 * var5, var21 * var6, var24 * var7);
                self.face_z_pos(
                    (var2 as f64),
                    (var3 as f64),
                    (var4 as f64),
                    self.grass_overlay(),
                );
            }
            var9 = true;
        }
        if self.renderAllFaces || self.should_side(var2 - 1, var3, var4, 4) {
            self.bright = if self.renderMinX > 0.0f64 {
                var26
            } else {
                self.mixed(var2 - 1, var3, var4)
            };
            self.color(var19, var22, var25);
            let mut var29: Icon = self.icon(var2, var3, var4, 4);
            self.face_x_neg((var2 as f64), (var3 as f64), (var4 as f64), var29);
            if FANCY_GRASS && var29.grass_side && !self.overrideBlockTexture.is_some() {
                self.color(var19 * var5, var22 * var6, var25 * var7);
                self.face_x_neg(
                    (var2 as f64),
                    (var3 as f64),
                    (var4 as f64),
                    self.grass_overlay(),
                );
            }
            var9 = true;
        }
        if self.renderAllFaces || self.should_side(var2 + 1, var3, var4, 5) {
            self.bright = if self.renderMaxX < 1.0f64 {
                var26
            } else {
                self.mixed(var2 + 1, var3, var4)
            };
            self.color(var19, var22, var25);
            let mut var30: Icon = self.icon(var2, var3, var4, 5);
            self.face_x_pos((var2 as f64), (var3 as f64), (var4 as f64), var30);
            if FANCY_GRASS && var30.grass_side && !self.overrideBlockTexture.is_some() {
                self.color(var19 * var5, var22 * var6, var25 * var7);
                self.face_x_pos(
                    (var2 as f64),
                    (var3 as f64),
                    (var4 as f64),
                    self.grass_overlay(),
                );
            }
            var9 = true;
        }
        return var9;
    }

    /// `RenderBlocks.renderFaceYNeg`
    #[allow(
        unused_assignments,
        unused_mut,
        unused_variables,
        unused_parens,
        clippy::all
    )]
    pub fn face_y_neg(&mut self, var2: f64, var4: f64, var6: f64, mut var8: Icon) {
        if self.overrideBlockTexture.is_some() {
            var8 = self.overrideBlockTexture.unwrap();
        }
        let mut var10: f64 = var8.u(self.renderMinX * 16.0f64);
        let mut var12: f64 = var8.u(self.renderMaxX * 16.0f64);
        let mut var14: f64 = var8.v(self.renderMinZ * 16.0f64);
        let mut var16: f64 = var8.v(self.renderMaxZ * 16.0f64);
        if self.renderMinX < 0.0f64 || self.renderMaxX > 1.0f64 {
            var10 = var8.min_u();
            var12 = var8.max_u();
        }
        if self.renderMinZ < 0.0f64 || self.renderMaxZ > 1.0f64 {
            var14 = var8.min_v();
            var16 = var8.max_v();
        }
        let mut var18: f64 = var12;
        let mut var20: f64 = var10;
        let mut var22: f64 = var14;
        let mut var24: f64 = var16;
        if self.uvRotateBottom == 2 {
            var10 = var8.u(self.renderMinZ * 16.0f64);
            var14 = var8.v(16.0f64 - self.renderMaxX * 16.0f64);
            var12 = var8.u(self.renderMaxZ * 16.0f64);
            var16 = var8.v(16.0f64 - self.renderMinX * 16.0f64);
            var22 = var14;
            var24 = var16;
            var18 = var10;
            var20 = var12;
            var14 = var16;
            var16 = var22;
        } else if self.uvRotateBottom == 1 {
            var10 = var8.u(16.0f64 - self.renderMaxZ * 16.0f64);
            var14 = var8.v(self.renderMinX * 16.0f64);
            var12 = var8.u(16.0f64 - self.renderMinZ * 16.0f64);
            var16 = var8.v(self.renderMaxX * 16.0f64);
            var18 = var12;
            var20 = var10;
            var10 = var18;
            var12 = var20;
            var22 = var16;
            var24 = var14;
        } else if self.uvRotateBottom == 3 {
            var10 = var8.u(16.0f64 - self.renderMinX * 16.0f64);
            var12 = var8.u(16.0f64 - self.renderMaxX * 16.0f64);
            var14 = var8.v(16.0f64 - self.renderMinZ * 16.0f64);
            var16 = var8.v(16.0f64 - self.renderMaxZ * 16.0f64);
            var18 = var12;
            var20 = var10;
            var22 = var14;
            var24 = var16;
        }
        let mut var26: f64 = var2 + self.renderMinX;
        let mut var28: f64 = var2 + self.renderMaxX;
        let mut var30: f64 = var4 + self.renderMinY;
        let mut var32: f64 = var6 + self.renderMinZ;
        let mut var34: f64 = var6 + self.renderMaxZ;
        if self.renderFromInside {
            var26 = var2 + self.renderMaxX;
            var28 = var2 + self.renderMinX;
        }
        if self.enableAO {
            self.color(
                self.colorRedTopLeft,
                self.colorGreenTopLeft,
                self.colorBlueTopLeft,
            );
            self.bright = self.brightnessTopLeft;
            self.vertex(var26, var30, var34, var20, var24);
            self.color(
                self.colorRedBottomLeft,
                self.colorGreenBottomLeft,
                self.colorBlueBottomLeft,
            );
            self.bright = self.brightnessBottomLeft;
            self.vertex(var26, var30, var32, var10, var14);
            self.color(
                self.colorRedBottomRight,
                self.colorGreenBottomRight,
                self.colorBlueBottomRight,
            );
            self.bright = self.brightnessBottomRight;
            self.vertex(var28, var30, var32, var18, var22);
            self.color(
                self.colorRedTopRight,
                self.colorGreenTopRight,
                self.colorBlueTopRight,
            );
            self.bright = self.brightnessTopRight;
            self.vertex(var28, var30, var34, var12, var16);
        } else {
            self.vertex(var26, var30, var34, var20, var24);
            self.vertex(var26, var30, var32, var10, var14);
            self.vertex(var28, var30, var32, var18, var22);
            self.vertex(var28, var30, var34, var12, var16);
        }
    }

    /// `RenderBlocks.renderFaceYPos`
    #[allow(
        unused_assignments,
        unused_mut,
        unused_variables,
        unused_parens,
        clippy::all
    )]
    pub fn face_y_pos(&mut self, var2: f64, var4: f64, var6: f64, mut var8: Icon) {
        if self.overrideBlockTexture.is_some() {
            var8 = self.overrideBlockTexture.unwrap();
        }
        let mut var10: f64 = var8.u(self.renderMinX * 16.0f64);
        let mut var12: f64 = var8.u(self.renderMaxX * 16.0f64);
        let mut var14: f64 = var8.v(self.renderMinZ * 16.0f64);
        let mut var16: f64 = var8.v(self.renderMaxZ * 16.0f64);
        if self.renderMinX < 0.0f64 || self.renderMaxX > 1.0f64 {
            var10 = var8.min_u();
            var12 = var8.max_u();
        }
        if self.renderMinZ < 0.0f64 || self.renderMaxZ > 1.0f64 {
            var14 = var8.min_v();
            var16 = var8.max_v();
        }
        let mut var18: f64 = var12;
        let mut var20: f64 = var10;
        let mut var22: f64 = var14;
        let mut var24: f64 = var16;
        if self.uvRotateTop == 1 {
            var10 = var8.u(self.renderMinZ * 16.0f64);
            var14 = var8.v(16.0f64 - self.renderMaxX * 16.0f64);
            var12 = var8.u(self.renderMaxZ * 16.0f64);
            var16 = var8.v(16.0f64 - self.renderMinX * 16.0f64);
            var22 = var14;
            var24 = var16;
            var18 = var10;
            var20 = var12;
            var14 = var16;
            var16 = var22;
        } else if self.uvRotateTop == 2 {
            var10 = var8.u(16.0f64 - self.renderMaxZ * 16.0f64);
            var14 = var8.v(self.renderMinX * 16.0f64);
            var12 = var8.u(16.0f64 - self.renderMinZ * 16.0f64);
            var16 = var8.v(self.renderMaxX * 16.0f64);
            var18 = var12;
            var20 = var10;
            var10 = var18;
            var12 = var20;
            var22 = var16;
            var24 = var14;
        } else if self.uvRotateTop == 3 {
            var10 = var8.u(16.0f64 - self.renderMinX * 16.0f64);
            var12 = var8.u(16.0f64 - self.renderMaxX * 16.0f64);
            var14 = var8.v(16.0f64 - self.renderMinZ * 16.0f64);
            var16 = var8.v(16.0f64 - self.renderMaxZ * 16.0f64);
            var18 = var12;
            var20 = var10;
            var22 = var14;
            var24 = var16;
        }
        let mut var26: f64 = var2 + self.renderMinX;
        let mut var28: f64 = var2 + self.renderMaxX;
        let mut var30: f64 = var4 + self.renderMaxY;
        let mut var32: f64 = var6 + self.renderMinZ;
        let mut var34: f64 = var6 + self.renderMaxZ;
        if self.renderFromInside {
            var26 = var2 + self.renderMaxX;
            var28 = var2 + self.renderMinX;
        }
        if self.enableAO {
            self.color(
                self.colorRedTopLeft,
                self.colorGreenTopLeft,
                self.colorBlueTopLeft,
            );
            self.bright = self.brightnessTopLeft;
            self.vertex(var28, var30, var34, var12, var16);
            self.color(
                self.colorRedBottomLeft,
                self.colorGreenBottomLeft,
                self.colorBlueBottomLeft,
            );
            self.bright = self.brightnessBottomLeft;
            self.vertex(var28, var30, var32, var18, var22);
            self.color(
                self.colorRedBottomRight,
                self.colorGreenBottomRight,
                self.colorBlueBottomRight,
            );
            self.bright = self.brightnessBottomRight;
            self.vertex(var26, var30, var32, var10, var14);
            self.color(
                self.colorRedTopRight,
                self.colorGreenTopRight,
                self.colorBlueTopRight,
            );
            self.bright = self.brightnessTopRight;
            self.vertex(var26, var30, var34, var20, var24);
        } else {
            self.vertex(var28, var30, var34, var12, var16);
            self.vertex(var28, var30, var32, var18, var22);
            self.vertex(var26, var30, var32, var10, var14);
            self.vertex(var26, var30, var34, var20, var24);
        }
    }

    /// `RenderBlocks.renderFaceZNeg`
    #[allow(
        unused_assignments,
        unused_mut,
        unused_variables,
        unused_parens,
        clippy::all
    )]
    pub fn face_z_neg(&mut self, var2: f64, var4: f64, var6: f64, mut var8: Icon) {
        if self.overrideBlockTexture.is_some() {
            var8 = self.overrideBlockTexture.unwrap();
        }
        let mut var10: f64 = var8.u(self.renderMinX * 16.0f64);
        let mut var12: f64 = var8.u(self.renderMaxX * 16.0f64);
        if self.field_152631_f {
            var12 = var8.u((1.0f64 - self.renderMinX) * 16.0f64);
            var10 = var8.u((1.0f64 - self.renderMaxX) * 16.0f64);
        }
        let mut var14: f64 = var8.v(16.0f64 - self.renderMaxY * 16.0f64);
        let mut var16: f64 = var8.v(16.0f64 - self.renderMinY * 16.0f64);
        if self.flipTexture {
            let mut var18: f64 = var10;
            var10 = var12;
            var12 = var18;
        }
        if self.renderMinX < 0.0f64 || self.renderMaxX > 1.0f64 {
            var10 = var8.min_u();
            var12 = var8.max_u();
        }
        if self.renderMinY < 0.0f64 || self.renderMaxY > 1.0f64 {
            var14 = var8.min_v();
            var16 = var8.max_v();
        }
        let mut var40: f64 = var12;
        let mut var20: f64 = var10;
        let mut var22: f64 = var14;
        let mut var24: f64 = var16;
        if self.uvRotateEast == 2 {
            var10 = var8.u(self.renderMinY * 16.0f64);
            var12 = var8.u(self.renderMaxY * 16.0f64);
            var14 = var8.v(16.0f64 - self.renderMinX * 16.0f64);
            var16 = var8.v(16.0f64 - self.renderMaxX * 16.0f64);
            var22 = var14;
            var24 = var16;
            var40 = var10;
            var20 = var12;
            var14 = var16;
            var16 = var22;
        } else if self.uvRotateEast == 1 {
            var10 = var8.u(16.0f64 - self.renderMaxY * 16.0f64);
            var12 = var8.u(16.0f64 - self.renderMinY * 16.0f64);
            var14 = var8.v(self.renderMaxX * 16.0f64);
            var16 = var8.v(self.renderMinX * 16.0f64);
            var40 = var12;
            var20 = var10;
            var10 = var40;
            var12 = var20;
            var22 = var16;
            var24 = var14;
        } else if self.uvRotateEast == 3 {
            var10 = var8.u(16.0f64 - self.renderMinX * 16.0f64);
            var12 = var8.u(16.0f64 - self.renderMaxX * 16.0f64);
            var14 = var8.v(self.renderMaxY * 16.0f64);
            var16 = var8.v(self.renderMinY * 16.0f64);
            var40 = var12;
            var20 = var10;
            var22 = var14;
            var24 = var16;
        }
        let mut var26: f64 = var2 + self.renderMinX;
        let mut var28: f64 = var2 + self.renderMaxX;
        let mut var30: f64 = var4 + self.renderMinY;
        let mut var32: f64 = var4 + self.renderMaxY;
        let mut var34: f64 = var6 + self.renderMinZ;
        if self.renderFromInside {
            var26 = var2 + self.renderMaxX;
            var28 = var2 + self.renderMinX;
        }
        if self.enableAO {
            self.color(
                self.colorRedTopLeft,
                self.colorGreenTopLeft,
                self.colorBlueTopLeft,
            );
            self.bright = self.brightnessTopLeft;
            self.vertex(var26, var32, var34, var40, var22);
            self.color(
                self.colorRedBottomLeft,
                self.colorGreenBottomLeft,
                self.colorBlueBottomLeft,
            );
            self.bright = self.brightnessBottomLeft;
            self.vertex(var28, var32, var34, var10, var14);
            self.color(
                self.colorRedBottomRight,
                self.colorGreenBottomRight,
                self.colorBlueBottomRight,
            );
            self.bright = self.brightnessBottomRight;
            self.vertex(var28, var30, var34, var20, var24);
            self.color(
                self.colorRedTopRight,
                self.colorGreenTopRight,
                self.colorBlueTopRight,
            );
            self.bright = self.brightnessTopRight;
            self.vertex(var26, var30, var34, var12, var16);
        } else {
            self.vertex(var26, var32, var34, var40, var22);
            self.vertex(var28, var32, var34, var10, var14);
            self.vertex(var28, var30, var34, var20, var24);
            self.vertex(var26, var30, var34, var12, var16);
        }
    }

    /// `RenderBlocks.renderFaceZPos`
    #[allow(
        unused_assignments,
        unused_mut,
        unused_variables,
        unused_parens,
        clippy::all
    )]
    pub fn face_z_pos(&mut self, var2: f64, var4: f64, var6: f64, mut var8: Icon) {
        if self.overrideBlockTexture.is_some() {
            var8 = self.overrideBlockTexture.unwrap();
        }
        let mut var10: f64 = var8.u(self.renderMinX * 16.0f64);
        let mut var12: f64 = var8.u(self.renderMaxX * 16.0f64);
        let mut var14: f64 = var8.v(16.0f64 - self.renderMaxY * 16.0f64);
        let mut var16: f64 = var8.v(16.0f64 - self.renderMinY * 16.0f64);
        if self.flipTexture {
            let mut var18: f64 = var10;
            var10 = var12;
            var12 = var18;
        }
        if self.renderMinX < 0.0f64 || self.renderMaxX > 1.0f64 {
            var10 = var8.min_u();
            var12 = var8.max_u();
        }
        if self.renderMinY < 0.0f64 || self.renderMaxY > 1.0f64 {
            var14 = var8.min_v();
            var16 = var8.max_v();
        }
        let mut var40: f64 = var12;
        let mut var20: f64 = var10;
        let mut var22: f64 = var14;
        let mut var24: f64 = var16;
        if self.uvRotateWest == 1 {
            var10 = var8.u(self.renderMinY * 16.0f64);
            var16 = var8.v(16.0f64 - self.renderMinX * 16.0f64);
            var12 = var8.u(self.renderMaxY * 16.0f64);
            var14 = var8.v(16.0f64 - self.renderMaxX * 16.0f64);
            var22 = var14;
            var24 = var16;
            var40 = var10;
            var20 = var12;
            var14 = var16;
            var16 = var22;
        } else if self.uvRotateWest == 2 {
            var10 = var8.u(16.0f64 - self.renderMaxY * 16.0f64);
            var14 = var8.v(self.renderMinX * 16.0f64);
            var12 = var8.u(16.0f64 - self.renderMinY * 16.0f64);
            var16 = var8.v(self.renderMaxX * 16.0f64);
            var40 = var12;
            var20 = var10;
            var10 = var40;
            var12 = var20;
            var22 = var16;
            var24 = var14;
        } else if self.uvRotateWest == 3 {
            var10 = var8.u(16.0f64 - self.renderMinX * 16.0f64);
            var12 = var8.u(16.0f64 - self.renderMaxX * 16.0f64);
            var14 = var8.v(self.renderMaxY * 16.0f64);
            var16 = var8.v(self.renderMinY * 16.0f64);
            var40 = var12;
            var20 = var10;
            var22 = var14;
            var24 = var16;
        }
        let mut var26: f64 = var2 + self.renderMinX;
        let mut var28: f64 = var2 + self.renderMaxX;
        let mut var30: f64 = var4 + self.renderMinY;
        let mut var32: f64 = var4 + self.renderMaxY;
        let mut var34: f64 = var6 + self.renderMaxZ;
        if self.renderFromInside {
            var26 = var2 + self.renderMaxX;
            var28 = var2 + self.renderMinX;
        }
        if self.enableAO {
            self.color(
                self.colorRedTopLeft,
                self.colorGreenTopLeft,
                self.colorBlueTopLeft,
            );
            self.bright = self.brightnessTopLeft;
            self.vertex(var26, var32, var34, var10, var14);
            self.color(
                self.colorRedBottomLeft,
                self.colorGreenBottomLeft,
                self.colorBlueBottomLeft,
            );
            self.bright = self.brightnessBottomLeft;
            self.vertex(var26, var30, var34, var20, var24);
            self.color(
                self.colorRedBottomRight,
                self.colorGreenBottomRight,
                self.colorBlueBottomRight,
            );
            self.bright = self.brightnessBottomRight;
            self.vertex(var28, var30, var34, var12, var16);
            self.color(
                self.colorRedTopRight,
                self.colorGreenTopRight,
                self.colorBlueTopRight,
            );
            self.bright = self.brightnessTopRight;
            self.vertex(var28, var32, var34, var40, var22);
        } else {
            self.vertex(var26, var32, var34, var10, var14);
            self.vertex(var26, var30, var34, var20, var24);
            self.vertex(var28, var30, var34, var12, var16);
            self.vertex(var28, var32, var34, var40, var22);
        }
    }

    /// `RenderBlocks.renderFaceXNeg`
    #[allow(
        unused_assignments,
        unused_mut,
        unused_variables,
        unused_parens,
        clippy::all
    )]
    pub fn face_x_neg(&mut self, var2: f64, var4: f64, var6: f64, mut var8: Icon) {
        if self.overrideBlockTexture.is_some() {
            var8 = self.overrideBlockTexture.unwrap();
        }
        let mut var10: f64 = var8.u(self.renderMinZ * 16.0f64);
        let mut var12: f64 = var8.u(self.renderMaxZ * 16.0f64);
        let mut var14: f64 = var8.v(16.0f64 - self.renderMaxY * 16.0f64);
        let mut var16: f64 = var8.v(16.0f64 - self.renderMinY * 16.0f64);
        if self.flipTexture {
            let mut var18: f64 = var10;
            var10 = var12;
            var12 = var18;
        }
        if self.renderMinZ < 0.0f64 || self.renderMaxZ > 1.0f64 {
            var10 = var8.min_u();
            var12 = var8.max_u();
        }
        if self.renderMinY < 0.0f64 || self.renderMaxY > 1.0f64 {
            var14 = var8.min_v();
            var16 = var8.max_v();
        }
        let mut var40: f64 = var12;
        let mut var20: f64 = var10;
        let mut var22: f64 = var14;
        let mut var24: f64 = var16;
        if self.uvRotateNorth == 1 {
            var10 = var8.u(self.renderMinY * 16.0f64);
            var14 = var8.v(16.0f64 - self.renderMaxZ * 16.0f64);
            var12 = var8.u(self.renderMaxY * 16.0f64);
            var16 = var8.v(16.0f64 - self.renderMinZ * 16.0f64);
            var22 = var14;
            var24 = var16;
            var40 = var10;
            var20 = var12;
            var14 = var16;
            var16 = var22;
        } else if self.uvRotateNorth == 2 {
            var10 = var8.u(16.0f64 - self.renderMaxY * 16.0f64);
            var14 = var8.v(self.renderMinZ * 16.0f64);
            var12 = var8.u(16.0f64 - self.renderMinY * 16.0f64);
            var16 = var8.v(self.renderMaxZ * 16.0f64);
            var40 = var12;
            var20 = var10;
            var10 = var40;
            var12 = var20;
            var22 = var16;
            var24 = var14;
        } else if self.uvRotateNorth == 3 {
            var10 = var8.u(16.0f64 - self.renderMinZ * 16.0f64);
            var12 = var8.u(16.0f64 - self.renderMaxZ * 16.0f64);
            var14 = var8.v(self.renderMaxY * 16.0f64);
            var16 = var8.v(self.renderMinY * 16.0f64);
            var40 = var12;
            var20 = var10;
            var22 = var14;
            var24 = var16;
        }
        let mut var26: f64 = var2 + self.renderMinX;
        let mut var28: f64 = var4 + self.renderMinY;
        let mut var30: f64 = var4 + self.renderMaxY;
        let mut var32: f64 = var6 + self.renderMinZ;
        let mut var34: f64 = var6 + self.renderMaxZ;
        if self.renderFromInside {
            var32 = var6 + self.renderMaxZ;
            var34 = var6 + self.renderMinZ;
        }
        if self.enableAO {
            self.color(
                self.colorRedTopLeft,
                self.colorGreenTopLeft,
                self.colorBlueTopLeft,
            );
            self.bright = self.brightnessTopLeft;
            self.vertex(var26, var30, var34, var40, var22);
            self.color(
                self.colorRedBottomLeft,
                self.colorGreenBottomLeft,
                self.colorBlueBottomLeft,
            );
            self.bright = self.brightnessBottomLeft;
            self.vertex(var26, var30, var32, var10, var14);
            self.color(
                self.colorRedBottomRight,
                self.colorGreenBottomRight,
                self.colorBlueBottomRight,
            );
            self.bright = self.brightnessBottomRight;
            self.vertex(var26, var28, var32, var20, var24);
            self.color(
                self.colorRedTopRight,
                self.colorGreenTopRight,
                self.colorBlueTopRight,
            );
            self.bright = self.brightnessTopRight;
            self.vertex(var26, var28, var34, var12, var16);
        } else {
            self.vertex(var26, var30, var34, var40, var22);
            self.vertex(var26, var30, var32, var10, var14);
            self.vertex(var26, var28, var32, var20, var24);
            self.vertex(var26, var28, var34, var12, var16);
        }
    }

    /// `RenderBlocks.renderFaceXPos`
    #[allow(
        unused_assignments,
        unused_mut,
        unused_variables,
        unused_parens,
        clippy::all
    )]
    pub fn face_x_pos(&mut self, var2: f64, var4: f64, var6: f64, mut var8: Icon) {
        if self.overrideBlockTexture.is_some() {
            var8 = self.overrideBlockTexture.unwrap();
        }
        let mut var10: f64 = var8.u(self.renderMinZ * 16.0f64);
        let mut var12: f64 = var8.u(self.renderMaxZ * 16.0f64);
        if self.field_152631_f {
            var12 = var8.u((1.0f64 - self.renderMinZ) * 16.0f64);
            var10 = var8.u((1.0f64 - self.renderMaxZ) * 16.0f64);
        }
        let mut var14: f64 = var8.v(16.0f64 - self.renderMaxY * 16.0f64);
        let mut var16: f64 = var8.v(16.0f64 - self.renderMinY * 16.0f64);
        if self.flipTexture {
            let mut var18: f64 = var10;
            var10 = var12;
            var12 = var18;
        }
        if self.renderMinZ < 0.0f64 || self.renderMaxZ > 1.0f64 {
            var10 = var8.min_u();
            var12 = var8.max_u();
        }
        if self.renderMinY < 0.0f64 || self.renderMaxY > 1.0f64 {
            var14 = var8.min_v();
            var16 = var8.max_v();
        }
        let mut var40: f64 = var12;
        let mut var20: f64 = var10;
        let mut var22: f64 = var14;
        let mut var24: f64 = var16;
        if self.uvRotateSouth == 2 {
            var10 = var8.u(self.renderMinY * 16.0f64);
            var14 = var8.v(16.0f64 - self.renderMinZ * 16.0f64);
            var12 = var8.u(self.renderMaxY * 16.0f64);
            var16 = var8.v(16.0f64 - self.renderMaxZ * 16.0f64);
            var22 = var14;
            var24 = var16;
            var40 = var10;
            var20 = var12;
            var14 = var16;
            var16 = var22;
        } else if self.uvRotateSouth == 1 {
            var10 = var8.u(16.0f64 - self.renderMaxY * 16.0f64);
            var14 = var8.v(self.renderMaxZ * 16.0f64);
            var12 = var8.u(16.0f64 - self.renderMinY * 16.0f64);
            var16 = var8.v(self.renderMinZ * 16.0f64);
            var40 = var12;
            var20 = var10;
            var10 = var40;
            var12 = var20;
            var22 = var16;
            var24 = var14;
        } else if self.uvRotateSouth == 3 {
            var10 = var8.u(16.0f64 - self.renderMinZ * 16.0f64);
            var12 = var8.u(16.0f64 - self.renderMaxZ * 16.0f64);
            var14 = var8.v(self.renderMaxY * 16.0f64);
            var16 = var8.v(self.renderMinY * 16.0f64);
            var40 = var12;
            var20 = var10;
            var22 = var14;
            var24 = var16;
        }
        let mut var26: f64 = var2 + self.renderMaxX;
        let mut var28: f64 = var4 + self.renderMinY;
        let mut var30: f64 = var4 + self.renderMaxY;
        let mut var32: f64 = var6 + self.renderMinZ;
        let mut var34: f64 = var6 + self.renderMaxZ;
        if self.renderFromInside {
            var32 = var6 + self.renderMaxZ;
            var34 = var6 + self.renderMinZ;
        }
        if self.enableAO {
            self.color(
                self.colorRedTopLeft,
                self.colorGreenTopLeft,
                self.colorBlueTopLeft,
            );
            self.bright = self.brightnessTopLeft;
            self.vertex(var26, var28, var34, var20, var24);
            self.color(
                self.colorRedBottomLeft,
                self.colorGreenBottomLeft,
                self.colorBlueBottomLeft,
            );
            self.bright = self.brightnessBottomLeft;
            self.vertex(var26, var28, var32, var12, var16);
            self.color(
                self.colorRedBottomRight,
                self.colorGreenBottomRight,
                self.colorBlueBottomRight,
            );
            self.bright = self.brightnessBottomRight;
            self.vertex(var26, var30, var32, var40, var22);
            self.color(
                self.colorRedTopRight,
                self.colorGreenTopRight,
                self.colorBlueTopRight,
            );
            self.bright = self.brightnessTopRight;
            self.vertex(var26, var30, var34, var10, var14);
        } else {
            self.vertex(var26, var28, var34, var20, var24);
            self.vertex(var26, var28, var32, var12, var16);
            self.vertex(var26, var30, var32, var40, var22);
            self.vertex(var26, var30, var34, var10, var14);
        }
    }

    fn reset_uv(&mut self) {
        self.uvRotateEast = 0;
        self.uvRotateWest = 0;
        self.uvRotateSouth = 0;
        self.uvRotateNorth = 0;
        self.uvRotateTop = 0;
        self.uvRotateBottom = 0;
    }

    /// `RenderBlocks.renderBlockLog` (render type 31: logs, hay)
    pub fn render_block_log(&mut self, x: i32, y: i32, z: i32) -> bool {
        let m = self.meta & 12;
        if m == 4 {
            self.uvRotateEast = 1;
            self.uvRotateWest = 1;
            self.uvRotateTop = 1;
            self.uvRotateBottom = 1;
        } else if m == 8 {
            self.uvRotateSouth = 1;
            self.uvRotateNorth = 1;
        }
        let r = self.render_standard(x, y, z);
        self.reset_uv();
        r
    }

    /// `RenderBlocks.renderBlockQuartz` (render type 39)
    pub fn render_block_quartz(&mut self, x: i32, y: i32, z: i32) -> bool {
        if self.meta == 3 {
            self.uvRotateEast = 1;
            self.uvRotateWest = 1;
            self.uvRotateTop = 1;
            self.uvRotateBottom = 1;
        } else if self.meta == 4 {
            self.uvRotateSouth = 1;
            self.uvRotateNorth = 1;
        }
        let r = self.render_standard(x, y, z);
        self.reset_uv();
        r
    }

    /// `RenderBlocks.renderPistonBase`; `all` is the extended look the
    /// moving-block renderer asks for.
    pub fn render_piston_base(&mut self, x: i32, y: i32, z: i32, all: bool) -> bool {
        let m = self.meta;
        let extended = all || m & 8 != 0;
        let o = m & 7;
        if extended {
            match o {
                0 => {
                    self.uvRotateEast = 3;
                    self.uvRotateWest = 3;
                    self.uvRotateSouth = 3;
                    self.uvRotateNorth = 3;
                    self.set_render_bounds(0.0, 0.25, 0.0, 1.0, 1.0, 1.0);
                }
                1 => self.set_render_bounds(0.0, 0.0, 0.0, 1.0, 0.75, 1.0),
                2 => {
                    self.uvRotateSouth = 1;
                    self.uvRotateNorth = 2;
                    self.set_render_bounds(0.0, 0.0, 0.25, 1.0, 1.0, 1.0);
                }
                3 => {
                    self.uvRotateSouth = 2;
                    self.uvRotateNorth = 1;
                    self.uvRotateTop = 3;
                    self.uvRotateBottom = 3;
                    self.set_render_bounds(0.0, 0.0, 0.0, 1.0, 1.0, 0.75);
                }
                4 => {
                    self.uvRotateEast = 1;
                    self.uvRotateWest = 2;
                    self.uvRotateTop = 2;
                    self.uvRotateBottom = 1;
                    self.set_render_bounds(0.25, 0.0, 0.0, 1.0, 1.0, 1.0);
                }
                5 => {
                    self.uvRotateEast = 2;
                    self.uvRotateWest = 1;
                    self.uvRotateTop = 1;
                    self.uvRotateBottom = 2;
                    self.set_render_bounds(0.0, 0.0, 0.0, 0.75, 1.0, 1.0);
                }
                _ => {}
            }
            // setShapeForRendering: the block bounds follow (as f32).
            let f = |v: f64| v as f32 as f64;
            let (a, b, c, d, e, g) = (
                f(self.renderMinX),
                f(self.renderMinY),
                f(self.renderMinZ),
                f(self.renderMaxX),
                f(self.renderMaxY),
                f(self.renderMaxZ),
            );
            self.set_render_bounds(a, b, c, d, e, g);
            self.render_standard(x, y, z);
            self.reset_uv();
            self.set_render_bounds(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        } else {
            match o {
                0 => {
                    self.uvRotateEast = 3;
                    self.uvRotateWest = 3;
                    self.uvRotateSouth = 3;
                    self.uvRotateNorth = 3;
                }
                2 => {
                    self.uvRotateSouth = 1;
                    self.uvRotateNorth = 2;
                }
                3 => {
                    self.uvRotateSouth = 2;
                    self.uvRotateNorth = 1;
                    self.uvRotateTop = 3;
                    self.uvRotateBottom = 3;
                }
                4 => {
                    self.uvRotateEast = 1;
                    self.uvRotateWest = 2;
                    self.uvRotateTop = 2;
                    self.uvRotateBottom = 1;
                }
                5 => {
                    self.uvRotateEast = 2;
                    self.uvRotateWest = 1;
                    self.uvRotateTop = 1;
                    self.uvRotateBottom = 2;
                }
                _ => {}
            }
            self.render_standard(x, y, z);
            self.reset_uv();
        }
        true
    }

    fn rod_icon(&self) -> Icon {
        self.overrideBlockTexture
            .unwrap_or_else(|| Icon::new(self.atlas().named("piston_side")))
    }

    /// `RenderBlocks.renderPistonRodUD`
    #[allow(clippy::too_many_arguments)]
    fn rod_ud(&mut self, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64, shade: f32, len: f64) {
        let i = self.rod_icon();
        let (u0, v0, u1, v1) = (i.min_u(), i.min_v(), i.u(len), i.v(4.0));
        self.color(shade, shade, shade);
        self.vertex(a, d, e, u1, v0);
        self.vertex(a, c, e, u0, v0);
        self.vertex(b, c, f, u0, v1);
        self.vertex(b, d, f, u1, v1);
    }

    /// `RenderBlocks.renderPistonRodSN`
    #[allow(clippy::too_many_arguments)]
    fn rod_sn(&mut self, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64, shade: f32, len: f64) {
        let i = self.rod_icon();
        let (u0, v0, u1, v1) = (i.min_u(), i.min_v(), i.u(len), i.v(4.0));
        self.color(shade, shade, shade);
        self.vertex(a, c, f, u1, v0);
        self.vertex(a, c, e, u0, v0);
        self.vertex(b, d, e, u0, v1);
        self.vertex(b, d, f, u1, v1);
    }

    /// `RenderBlocks.renderPistonRodEW`
    #[allow(clippy::too_many_arguments)]
    fn rod_ew(&mut self, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64, shade: f32, len: f64) {
        let i = self.rod_icon();
        let (u0, v0, u1, v1) = (i.min_u(), i.min_v(), i.u(len), i.v(4.0));
        self.color(shade, shade, shade);
        self.vertex(b, c, e, u1, v0);
        self.vertex(a, c, e, u0, v0);
        self.vertex(a, d, f, u0, v1);
        self.vertex(b, d, f, u1, v1);
    }

    /// `RenderBlocks.renderPistonExtension`; `long` gives the full-length arm.
    pub fn render_piston_extension(&mut self, x: i32, y: i32, z: i32, long: bool) -> bool {
        let o = self.meta & 7;
        let l = if long { 1.0f32 } else { 0.5f32 };
        let len = if long { 16.0 } else { 8.0 };
        // Java adds the float offsets to world coordinates.
        let (ox, oz) = (self.c().r.ox, self.c().r.oz);
        let (xf, yf, zf) = ((x + ox) as f32, y as f32, (z + oz) as f32);
        let p = |v: f32| v as f64;
        let px = |v: f32| v as f64 - ox as f64;
        let pz = |v: f32| v as f64 - oz as f64;
        match o {
            0 => {
                self.uvRotateEast = 3;
                self.uvRotateWest = 3;
                self.uvRotateSouth = 3;
                self.uvRotateNorth = 3;
                self.set_render_bounds(0.0, 0.0, 0.0, 1.0, 0.25, 1.0);
                self.render_standard(x, y, z);
                let (y0, y1) = (p(yf + 0.25), p(yf + 0.25 + l));
                self.rod_ud(
                    px(xf + 0.375),
                    px(xf + 0.625),
                    y0,
                    y1,
                    pz(zf + 0.625),
                    pz(zf + 0.625),
                    0.8,
                    len,
                );
                self.rod_ud(
                    px(xf + 0.625),
                    px(xf + 0.375),
                    y0,
                    y1,
                    pz(zf + 0.375),
                    pz(zf + 0.375),
                    0.8,
                    len,
                );
                self.rod_ud(
                    px(xf + 0.375),
                    px(xf + 0.375),
                    y0,
                    y1,
                    pz(zf + 0.375),
                    pz(zf + 0.625),
                    0.6,
                    len,
                );
                self.rod_ud(
                    px(xf + 0.625),
                    px(xf + 0.625),
                    y0,
                    y1,
                    pz(zf + 0.625),
                    pz(zf + 0.375),
                    0.6,
                    len,
                );
            }
            1 => {
                self.set_render_bounds(0.0, 0.75, 0.0, 1.0, 1.0, 1.0);
                self.render_standard(x, y, z);
                let (y0, y1) = (p(yf - 0.25 + 1.0 - l), p(yf - 0.25 + 1.0));
                self.rod_ud(
                    px(xf + 0.375),
                    px(xf + 0.625),
                    y0,
                    y1,
                    pz(zf + 0.625),
                    pz(zf + 0.625),
                    0.8,
                    len,
                );
                self.rod_ud(
                    px(xf + 0.625),
                    px(xf + 0.375),
                    y0,
                    y1,
                    pz(zf + 0.375),
                    pz(zf + 0.375),
                    0.8,
                    len,
                );
                self.rod_ud(
                    px(xf + 0.375),
                    px(xf + 0.375),
                    y0,
                    y1,
                    pz(zf + 0.375),
                    pz(zf + 0.625),
                    0.6,
                    len,
                );
                self.rod_ud(
                    px(xf + 0.625),
                    px(xf + 0.625),
                    y0,
                    y1,
                    pz(zf + 0.625),
                    pz(zf + 0.375),
                    0.6,
                    len,
                );
            }
            2 => {
                self.uvRotateSouth = 1;
                self.uvRotateNorth = 2;
                self.set_render_bounds(0.0, 0.0, 0.0, 1.0, 1.0, 0.25);
                self.render_standard(x, y, z);
                let (z0, z1) = (pz(zf + 0.25), pz(zf + 0.25 + l));
                self.rod_sn(
                    px(xf + 0.375),
                    px(xf + 0.375),
                    p(yf + 0.625),
                    p(yf + 0.375),
                    z0,
                    z1,
                    0.6,
                    len,
                );
                self.rod_sn(
                    px(xf + 0.625),
                    px(xf + 0.625),
                    p(yf + 0.375),
                    p(yf + 0.625),
                    z0,
                    z1,
                    0.6,
                    len,
                );
                self.rod_sn(
                    px(xf + 0.375),
                    px(xf + 0.625),
                    p(yf + 0.375),
                    p(yf + 0.375),
                    z0,
                    z1,
                    0.5,
                    len,
                );
                self.rod_sn(
                    px(xf + 0.625),
                    px(xf + 0.375),
                    p(yf + 0.625),
                    p(yf + 0.625),
                    z0,
                    z1,
                    1.0,
                    len,
                );
            }
            3 => {
                self.uvRotateSouth = 2;
                self.uvRotateNorth = 1;
                self.uvRotateTop = 3;
                self.uvRotateBottom = 3;
                self.set_render_bounds(0.0, 0.0, 0.75, 1.0, 1.0, 1.0);
                self.render_standard(x, y, z);
                let (z0, z1) = (pz(zf - 0.25 + 1.0 - l), pz(zf - 0.25 + 1.0));
                self.rod_sn(
                    px(xf + 0.375),
                    px(xf + 0.375),
                    p(yf + 0.625),
                    p(yf + 0.375),
                    z0,
                    z1,
                    0.6,
                    len,
                );
                self.rod_sn(
                    px(xf + 0.625),
                    px(xf + 0.625),
                    p(yf + 0.375),
                    p(yf + 0.625),
                    z0,
                    z1,
                    0.6,
                    len,
                );
                self.rod_sn(
                    px(xf + 0.375),
                    px(xf + 0.625),
                    p(yf + 0.375),
                    p(yf + 0.375),
                    z0,
                    z1,
                    0.5,
                    len,
                );
                self.rod_sn(
                    px(xf + 0.625),
                    px(xf + 0.375),
                    p(yf + 0.625),
                    p(yf + 0.625),
                    z0,
                    z1,
                    1.0,
                    len,
                );
            }
            4 => {
                self.uvRotateEast = 1;
                self.uvRotateWest = 2;
                self.uvRotateTop = 2;
                self.uvRotateBottom = 1;
                self.set_render_bounds(0.0, 0.0, 0.0, 0.25, 1.0, 1.0);
                self.render_standard(x, y, z);
                let (x0, x1) = (px(xf + 0.25), px(xf + 0.25 + l));
                self.rod_ew(
                    x0,
                    x1,
                    p(yf + 0.375),
                    p(yf + 0.375),
                    pz(zf + 0.625),
                    pz(zf + 0.375),
                    0.5,
                    len,
                );
                self.rod_ew(
                    x0,
                    x1,
                    p(yf + 0.625),
                    p(yf + 0.625),
                    pz(zf + 0.375),
                    pz(zf + 0.625),
                    1.0,
                    len,
                );
                self.rod_ew(
                    x0,
                    x1,
                    p(yf + 0.375),
                    p(yf + 0.625),
                    pz(zf + 0.375),
                    pz(zf + 0.375),
                    0.6,
                    len,
                );
                self.rod_ew(
                    x0,
                    x1,
                    p(yf + 0.625),
                    p(yf + 0.375),
                    pz(zf + 0.625),
                    pz(zf + 0.625),
                    0.6,
                    len,
                );
            }
            5 => {
                self.uvRotateEast = 2;
                self.uvRotateWest = 1;
                self.uvRotateTop = 1;
                self.uvRotateBottom = 2;
                self.set_render_bounds(0.75, 0.0, 0.0, 1.0, 1.0, 1.0);
                self.render_standard(x, y, z);
                let (x0, x1) = (px(xf - 0.25 + 1.0 - l), px(xf - 0.25 + 1.0));
                self.rod_ew(
                    x0,
                    x1,
                    p(yf + 0.375),
                    p(yf + 0.375),
                    pz(zf + 0.625),
                    pz(zf + 0.375),
                    0.5,
                    len,
                );
                self.rod_ew(
                    x0,
                    x1,
                    p(yf + 0.625),
                    p(yf + 0.625),
                    pz(zf + 0.375),
                    pz(zf + 0.625),
                    1.0,
                    len,
                );
                self.rod_ew(
                    x0,
                    x1,
                    p(yf + 0.375),
                    p(yf + 0.625),
                    pz(zf + 0.375),
                    pz(zf + 0.375),
                    0.6,
                    len,
                );
                self.rod_ew(
                    x0,
                    x1,
                    p(yf + 0.625),
                    p(yf + 0.375),
                    pz(zf + 0.625),
                    pz(zf + 0.625),
                    0.6,
                    len,
                );
            }
            _ => {}
        }
        self.reset_uv();
        self.set_render_bounds(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        true
    }

    /// `RenderBlocks.renderBlockSandFalling`: the block centred on the
    /// origin, flat shaded, lit with one brightness.
    pub fn render_sand_falling(&mut self, bright: i32) {
        self.set_render_bounds(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);
        self.bright = bright;
        self.color(0.5, 0.5, 0.5);
        let i = self.side_icon(0);
        self.face_y_neg(-0.5, -0.5, -0.5, i);
        self.color(1.0, 1.0, 1.0);
        let i = self.side_icon(1);
        self.face_y_pos(-0.5, -0.5, -0.5, i);
        self.color(0.8, 0.8, 0.8);
        let i = self.side_icon(2);
        self.face_z_neg(-0.5, -0.5, -0.5, i);
        self.color(0.8, 0.8, 0.8);
        let i = self.side_icon(3);
        self.face_z_pos(-0.5, -0.5, -0.5, i);
        self.color(0.6, 0.6, 0.6);
        let i = self.side_icon(4);
        self.face_x_neg(-0.5, -0.5, -0.5, i);
        self.color(0.6, 0.6, 0.6);
        let i = self.side_icon(5);
        self.face_x_pos(-0.5, -0.5, -0.5, i);
    }

    /// `RenderBlocks.renderBlockBrewingStand`
    pub fn render_brewing_stand(&mut self, x: i32, y: i32, z: i32) -> bool {
        self.set_render_bounds(0.4375, 0.0, 0.4375, 0.5625, 0.875, 0.5625);
        self.render_standard(x, y, z);
        self.overrideBlockTexture = Some(Icon::new(self.atlas().named("brewing_stand_base")));
        let all = self.renderAllFaces;
        self.renderAllFaces = true;
        self.set_render_bounds(0.5625, 0.0, 0.3125, 0.9375, 0.125, 0.6875);
        self.render_standard(x, y, z);
        self.set_render_bounds(0.125, 0.0, 0.0625, 0.5, 0.125, 0.4375);
        self.render_standard(x, y, z);
        self.set_render_bounds(0.125, 0.0, 0.5625, 0.5, 0.125, 0.9375);
        self.render_standard(x, y, z);
        self.renderAllFaces = all;
        self.overrideBlockTexture = None;
        self.bright = self.mixed(x, y, z);
        self.color(1.0, 1.0, 1.0);
        let icon = Icon::new(self.atlas().named("brewing_stand"));
        let (v0, v1) = (icon.min_v(), icon.max_v());
        let m = self.meta;
        let (xf, yf, zf) = (x as f64, y as f64, z as f64);
        for i in 0..3 {
            let a = i as f64 * std::f64::consts::PI * 2.0 / 3.0 + std::f64::consts::PI / 2.0;
            let u0 = icon.u(8.0);
            let mut u1 = icon.max_u();
            if m & 1 << i != 0 {
                u1 = icon.min_u();
            }
            let (x0, z0) = (xf + 0.5, zf + 0.5);
            let x1 = xf + 0.5 + a.sin() * 8.0 / 16.0;
            let z1 = zf + 0.5 + a.cos() * 8.0 / 16.0;
            self.vertex(x0, yf + 1.0, z0, u0, v0);
            self.vertex(x0, yf, z0, u0, v1);
            self.vertex(x1, yf, z1, u1, v1);
            self.vertex(x1, yf + 1.0, z1, u1, v0);
            self.vertex(x1, yf + 1.0, z1, u1, v0);
            self.vertex(x1, yf, z1, u1, v1);
            self.vertex(x0, yf, z0, u0, v1);
            self.vertex(x0, yf + 1.0, z0, u0, v0);
        }
        self.set_render_bounds(0.0, 0.0, 0.0, 1.0, 0.125, 1.0);
        true
    }
}
