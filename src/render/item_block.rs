//! `RenderBlocks.renderBlockAsItem` for the render types that
//! `renderItemIn3d` accepts: the faces of a block item in block space,
//! with the `glRotatef` / `glTranslatef` the method applies before them.
//! The GUI, dropped items and held items light these themselves.

use super::atlas::Atlas;
use super::mesher::rb::{Icon, Rb};
use super::mesher::Vertex;
use super::{mul, rot_y, translate, Mat4};
use crate::block::{self, Shape};

/// One face: its side (0 bottom .. 5 east) and its four vertices.
pub struct ItemFace {
    pub side: usize,
    pub v: [Vertex; 4],
}

/// `Block.getRenderType` of the block items drawn in 3D
/// (`RenderBlocks.renderItemIn3d`); `None` for the flat ones.
pub fn render_type(id: u8) -> Option<i32> {
    let info = block::info(id);
    Some(match info.shape {
        Shape::Cube => match id {
            block::LOG | block::LOG2 | block::HAY_BLOCK => 31,
            block::QUARTZ_BLOCK => 39,
            block::PISTON | block::STICKY_PISTON => 16,
            _ => 0,
        },
        Shape::Slab | Shape::SnowLayer => 0,
        Shape::Cactus => 13,
        Shape::Stairs => 10,
        Shape::Chest => 22,
        Shape::Anvil => 35,
        Shape::Fence => 11,
        Shape::FenceGate => 21,
        Shape::Wall => 32,
        Shape::Boxes => match id {
            block::ENCHANTING_TABLE
            | block::TRAPDOOR
            | block::STONE_PRESSURE_PLATE
            | block::WOODEN_PRESSURE_PLATE
            | block::STONE_BUTTON
            | block::WOODEN_BUTTON
            | block::CARPET
            | block::FARMLAND => 0,
            block::END_PORTAL_FRAME => 26,
            block::DRAGON_EGG => 27,
            _ => return None,
        },
        _ => return None,
    })
}

/// The block bounds `renderBlockAsItem` reads: `setBlockBoundsForItemRender`
/// where the block has one, else the bounds the block keeps.
fn item_bounds(id: u8, meta: i32) -> [f64; 6] {
    let full = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    let f = |v: [f32; 6]| v.map(|x| x as f64);
    if block::info(id).shape == Shape::Slab {
        // BlockSlab.setBlockBoundsForItemRender (the single slabs)
        return f([0.0, 0.0, 0.0, 1.0, 0.5, 1.0]);
    }
    match id {
        block::SNOW_LAYER => f([0.0, 0.0, 0.0, 1.0, 0.125, 1.0]),
        block::CARPET => f([0.0, 0.0, 0.0, 1.0, 0.0625, 1.0]),
        block::ENCHANTING_TABLE => f([0.0, 0.0, 0.0, 1.0, 0.75, 1.0]),
        block::FARMLAND => f([0.0, 0.0, 0.0, 1.0, 0.9375, 1.0]),
        block::END_PORTAL_FRAME => f([0.0, 0.0, 0.0, 1.0, 0.8125, 1.0]),
        block::TRAPDOOR => {
            let a = 0.1875f32;
            f([0.0, 0.5 - a / 2.0, 0.0, 1.0, 0.5 + a / 2.0, 1.0])
        }
        block::STONE_PRESSURE_PLATE | block::WOODEN_PRESSURE_PLATE => {
            let (a, b, c) = (0.5f32, 0.125f32, 0.5f32);
            f([0.5 - a, 0.5 - b, 0.5 - c, 0.5 + a, 0.5 + b, 0.5 + c])
        }
        block::STONE_BUTTON | block::WOODEN_BUTTON => {
            let (a, b, c) = (0.1875f32, 0.125f32, 0.125f32);
            f([0.5 - a, 0.5 - b, 0.5 - c, 0.5 + a, 0.5 + b, 0.5 + c])
        }
        _ => {
            let _ = meta;
            full
        }
    }
}

/// `getRenderColor(meta)` as `glColor`, per face: grass colours only its
/// top in the inventory.
fn tint(id: u8, meta: i32, side: usize) -> [f32; 3] {
    let c = match id {
        block::GRASS if side == 1 => 0x7CBD6B,
        block::LEAVES if meta & 3 == 1 => 6396257,
        block::LEAVES if meta & 3 == 2 => 8431445,
        block::LEAVES | block::LEAVES2 => 0x48B518,
        _ => 0xFFFFFF,
    };
    [
        (c >> 16 & 255) as f32 / 255.0,
        (c >> 8 & 255) as f32 / 255.0,
        (c & 255) as f32 / 255.0,
    ]
}

struct Faces<'r, 'a, 'b> {
    rb: &'r mut Rb<'a, 'b>,
    out: Vec<ItemFace>,
    id: u8,
    meta: i32,
}

impl Faces<'_, '_, '_> {
    /// The six `renderFace*` calls at the origin, `icon(side)` choosing the
    /// texture and `shift` the cactus' `addTranslation`.
    fn six(&mut self, icon: &dyn Fn(&Rb, i32) -> Icon, shift: f64) {
        for side in 0..6usize {
            let start = self.rb.out.len();
            let t = tint(self.id, self.meta, side);
            self.rb.set_color(t[0], t[1], t[2]);
            let i = icon(self.rb, side as i32);
            match side {
                0 => self.rb.face_y_neg(0.0, 0.0, 0.0, i),
                1 => self.rb.face_y_pos(0.0, 0.0, 0.0, i),
                2 => self.rb.face_z_neg(0.0, 0.0, 0.0, i),
                3 => self.rb.face_z_pos(0.0, 0.0, 0.0, i),
                4 => self.rb.face_x_neg(0.0, 0.0, 0.0, i),
                _ => self.rb.face_x_pos(0.0, 0.0, 0.0, i),
            }
            let d = [
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0],
                [0.0, 0.0, shift],
                [0.0, 0.0, -shift],
                [shift, 0.0, 0.0],
                [-shift, 0.0, 0.0],
            ][side];
            let vs: Vec<Vertex> = self.rb.out.drain(start..).collect();
            for q in vs.chunks(4) {
                let mut v = [q[0], q[1], q[2], q[3]];
                for p in &mut v {
                    for k in 0..3 {
                        p.pos[k] += d[k] as f32;
                    }
                }
                self.out.push(ItemFace { side, v });
            }
        }
    }

    fn bounds(&mut self, b: [f64; 6]) {
        self.rb
            .set_render_bounds(b[0], b[1], b[2], b[3], b[4], b[5]);
    }
}

/// The faces of block item `id:meta` as `renderBlockAsItem` draws them,
/// and the transform it applies first. `None` for flat items and chests
/// (`TileEntityRendererChestHelper`, drawn by `render::chest`).
pub fn faces(atlas: &Atlas, id: u8, meta: i32) -> Option<(Vec<ItemFace>, Mat4)> {
    let ty = render_type(id)?;
    if ty == 22 {
        return None;
    }
    let mut raw = Vec::new();
    let mut rb = Rb::bare(atlas, &mut raw, id, meta);
    let side_meta = |rb: &Rb, s: i32| rb.side_icon(s);
    let mut f = Faces {
        rb: &mut rb,
        out: Vec::new(),
        id,
        meta,
    };
    let half = translate(-0.5, -0.5, -0.5);
    let pre = match ty {
        // the standard path: glRotatef(90, 0, 1, 0), then -0.5
        0 | 31 | 39 | 16 | 26 => {
            if ty == 16 {
                f.meta = 1;
                f.rb.meta = 1;
            }
            f.bounds(item_bounds(id, meta));
            f.six(&side_meta, 0.0);
            mul(&rot_y(90.0), &half)
        }
        13 => {
            // getBlockIconFromSide: metadata 0
            f.rb.meta = 0;
            f.bounds([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
            f.six(&side_meta, 0.0625);
            half
        }
        10 => {
            f.rb.meta = 0;
            for b in [
                [0.0, 0.0, 0.0, 1.0, 1.0, 0.5],
                [0.0, 0.0, 0.5, 1.0, 0.5, 1.0],
            ] {
                f.bounds(b);
                f.six(&side_meta, 0.0);
            }
            half
        }
        11 => {
            f.rb.meta = 0;
            let a = 0.125f32;
            let c = 0.0625f32;
            for b in [
                [0.5 - a, 0.0, 0.0, 0.5 + a, 1.0, a * 2.0],
                [0.5 - a, 0.0, 1.0 - a * 2.0, 0.5 + a, 1.0, 1.0],
                [
                    0.5 - c,
                    1.0 - c * 3.0,
                    -c * 2.0,
                    0.5 + c,
                    1.0 - c,
                    1.0 + c * 2.0,
                ],
                [
                    0.5 - c,
                    0.5 - c * 3.0,
                    -c * 2.0,
                    0.5 + c,
                    0.5 - c,
                    1.0 + c * 2.0,
                ],
            ] {
                f.bounds(b.map(|x| x as f64));
                f.six(&side_meta, 0.0);
            }
            half
        }
        21 => {
            // the fence gate: two posts and a bar, metadata 0
            f.rb.meta = 0;
            let c = 0.0625f32;
            for b in [
                [0.5 - c, 0.3, 0.0, 0.5 + c, 1.0, c * 2.0],
                [0.5 - c, 0.3, 1.0 - c * 2.0, 0.5 + c, 1.0, 1.0],
                [0.5 - c, 0.5, 0.0, 0.5 + c, 1.0 - c, 1.0],
            ] {
                f.bounds(b.map(|x| x as f64));
                f.six(&side_meta, 0.0);
            }
            half
        }
        32 => {
            // the wall: a straight piece and the post, with the item's metadata
            for b in [
                [0.0, 0.0, 0.3125, 1.0, 0.8125, 0.6875],
                [0.25, 0.0, 0.25, 0.75, 1.0, 0.75],
            ] {
                f.bounds(b);
                f.six(&side_meta, 0.0);
            }
            half
        }
        27 => {
            // the dragon egg's eight layers
            f.rb.meta = 0;
            let mut y = 0i32;
            for (w, h) in [
                (2, 1),
                (3, 1),
                (4, 1),
                (5, 2),
                (6, 3),
                (7, 5),
                (6, 2),
                (3, 1),
            ] {
                let a = w as f32 / 16.0;
                let top = 1.0 - y as f32 / 16.0;
                let bottom = 1.0 - (y + h) as f32 / 16.0;
                y += h;
                f.bounds([0.5 - a, bottom, 0.5 - a, 0.5 + a, top, 0.5 + a].map(|x| x as f64));
                f.six(&side_meta, 0.0);
            }
            half
        }
        35 => {
            let start = f.rb.out.len();
            f.rb.meta = meta << 2;
            f.rb.render_anvil(0, 0, 0, true);
            // the anvil draws its own six faces per box in side order
            let vs: Vec<Vertex> = f.rb.out.drain(start..).collect();
            for (k, q) in vs.chunks(4).enumerate() {
                f.out.push(ItemFace {
                    side: k % 6,
                    v: [q[0], q[1], q[2], q[3]],
                });
            }
            half
        }
        _ => return None,
    };
    let out = std::mem::take(&mut f.out);
    Some((out, pre))
}
