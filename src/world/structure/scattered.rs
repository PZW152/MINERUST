//! `MapGenScatteredFeature` and `ComponentScatteredFeaturePieces`: desert and
//! jungle temples, witch huts.

use super::{Kind, Piece, Start, BB};
use crate::block::*;
use crate::java::JavaRandom;
use crate::world::gen::chunk_provider::ChunkProvider;
use crate::world::loot;
use crate::world::server::{GenEntity, ServerWorld};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SKind {
    Desert {
        chests: [bool; 4],
    },
    Jungle {
        main: bool,
        hidden: bool,
        trap1: bool,
        trap2: bool,
    },
    Swamp {
        witch: bool,
    },
}

/// `ComponentScatteredFeaturePieces$Feature`
#[derive(Clone, Copy, Debug)]
pub struct Scatter {
    pub w: i32,
    pub h: i32,
    pub d: i32,
    /// `horizontalPos`
    pub hpos: i32,
    pub k: SKind,
}

impl Scatter {
    pub fn save(&self, out: &mut Vec<i32>) {
        out.extend_from_slice(&[self.w, self.h, self.d, self.hpos]);
        match self.k {
            SKind::Desert { chests } => out.extend_from_slice(&[
                0,
                chests[0] as i32,
                chests[1] as i32,
                chests[2] as i32,
                chests[3] as i32,
            ]),
            SKind::Jungle {
                main,
                hidden,
                trap1,
                trap2,
            } => {
                out.extend_from_slice(&[1, main as i32, hidden as i32, trap1 as i32, trap2 as i32])
            }
            SKind::Swamp { witch } => out.extend_from_slice(&[2, witch as i32]),
        }
    }
    pub fn load(v: &mut std::slice::Iter<i32>) -> Option<Scatter> {
        let mut t = || v.next().copied();
        let (w, h, d, hpos) = (t()?, t()?, t()?, t()?);
        let k = match t()? {
            0 => SKind::Desert {
                chests: [t()? != 0, t()? != 0, t()? != 0, t()? != 0],
            },
            1 => SKind::Jungle {
                main: t()? != 0,
                hidden: t()? != 0,
                trap1: t()? != 0,
                trap2: t()? != 0,
            },
            _ => SKind::Swamp { witch: t()? != 0 },
        };
        Some(Scatter { w, h, d, hpos, k })
    }
}

/// `MapGenScatteredFeature.canSpawnStructureAtCoords`
pub fn can_spawn(provider: &ChunkProvider, seed: i64, x: i32, z: i32) -> bool {
    let (dist, sep) = (32, 8);
    let (mut gx, mut gz) = (x, z);
    if gx < 0 {
        gx -= dist - 1;
    }
    if gz < 0 {
        gz -= dist - 1;
    }
    let (mut px, mut pz) = (gx / dist, gz / dist);
    let mut r = JavaRandom::new(0);
    r.set_seed(
        (px as i64)
            .wrapping_mul(341873128712)
            .wrapping_add((pz as i64).wrapping_mul(132897987541))
            .wrapping_add(seed)
            .wrapping_add(14357617),
    );
    px *= dist;
    pz *= dist;
    px += r.next_int_n(dist - sep);
    pz += r.next_int_n(dist - sep);
    if x == px && z == pz {
        let b = provider.biome_at(x * 16 + 8, z * 16 + 8);
        return matches!(b, 2 | 17 | 21 | 22 | 6);
    }
    false
}

/// `MapGenScatteredFeature$Start`
pub fn start(provider: &ChunkProvider, r: &mut JavaRandom, cx: i32, cz: i32) -> Start {
    let biome = provider.biome_at(cx * 16 + 8, cz * 16 + 8);
    let (w, h, d, k) = match biome {
        21 | 22 => (
            12,
            10,
            15,
            SKind::Jungle {
                main: false,
                hidden: false,
                trap1: false,
                trap2: false,
            },
        ),
        6 => (7, 5, 9, SKind::Swamp { witch: false }),
        _ => (21, 15, 21, SKind::Desert { chests: [false; 4] }),
    };
    let (x, y, z) = (cx * 16, 64, cz * 16);
    let mode = r.next_int_n(4);
    let bb = match mode {
        0 | 2 => BB::new(x, y, z, x + w - 1, y + h - 1, z + d - 1),
        _ => BB::new(x, y, z, x + d - 1, y + h - 1, z + w - 1),
    };
    let p = Piece::new(
        0,
        mode,
        bb,
        Kind::Scatter(Scatter {
            w,
            h,
            d,
            hpos: -1,
            k,
        }),
    );
    let mut s = Start {
        cx,
        cz,
        bb: BB::default(),
        pieces: vec![p],
        sizeable: true,
    };
    s.update_bb();
    s
}

/// `offsetToAverageGroundLevel`
fn to_ground(p: &mut Piece, w: &mut ServerWorld, clip: &BB, dy: i32) -> bool {
    let Kind::Scatter(s) = &p.kind else {
        return false;
    };
    if s.hpos >= 0 {
        return true;
    }
    let (mut sum, mut n) = (0, 0);
    for z in p.bb.z0..=p.bb.z1 {
        for x in p.bb.x0..=p.bb.x1 {
            if clip.inside(x, 64, z) {
                sum += w.top_solid_or_liquid(x, z).max(64);
                n += 1;
            }
        }
    }
    if n == 0 {
        return false;
    }
    let hpos = sum / n;
    let off = hpos - p.bb.y0 + dy;
    p.bb.offset(0, off, 0);
    if let Kind::Scatter(s) = &mut p.kind {
        s.hpos = hpos;
    }
    true
}

/// `JunglePyramid$Stones`
fn stones(r: &mut JavaRandom, _edge: bool) -> (u8, i32) {
    if r.next_float() < 0.4 {
        (COBBLESTONE, 0)
    } else {
        (MOSSY_COBBLESTONE, 0)
    }
}

/// `BlockLever.invertMetadata`
fn lever_meta(facing: i32) -> i32 {
    match facing {
        0 => 0,
        1 => 5,
        2 => 4,
        3 => 3,
        4 => 2,
        5 => 1,
        _ => -1,
    }
}

pub fn add_parts(p: &mut Piece, w: &mut ServerWorld, r: &mut JavaRandom, clip: &BB) -> bool {
    let Kind::Scatter(s) = p.kind else {
        return true;
    };
    match s.k {
        SKind::Desert { mut chests } => {
            let q = p.clone();
            desert(&q, w, clip);
            for (i, done) in chests.iter_mut().enumerate() {
                if !*done {
                    let dx = [0, -1, 0, 1][i] * 2;
                    let dz = [1, 0, -1, 0][i] * 2;
                    let book = loot::enchanted_book(r, 1, 1, 1);
                    let table = loot::with(loot::TEMPLE, book);
                    let n = 2 + r.next_int_n(5);
                    *done = q.chest(w, clip, r, 10 + dx, -11, 10 + dz, &table, n);
                }
            }
            if let Kind::Scatter(s) = &mut p.kind {
                s.k = SKind::Desert { chests };
            }
            true
        }
        SKind::Jungle { .. } => {
            if !to_ground(p, w, clip, 0) {
                return false;
            }
            jungle(p, w, r, clip);
            true
        }
        SKind::Swamp { witch } => {
            if !to_ground(p, w, clip, 0) {
                return false;
            }
            let q = p.clone();
            q.fill_meta(w, clip, 1, 1, 1, 5, 1, 7, PLANKS, 1, PLANKS, 1, false);
            q.fill_meta(w, clip, 1, 4, 2, 5, 4, 7, PLANKS, 1, PLANKS, 1, false);
            q.fill_meta(w, clip, 2, 1, 0, 4, 1, 0, PLANKS, 1, PLANKS, 1, false);
            q.fill_meta(w, clip, 2, 2, 2, 3, 3, 2, PLANKS, 1, PLANKS, 1, false);
            q.fill_meta(w, clip, 1, 2, 3, 1, 3, 6, PLANKS, 1, PLANKS, 1, false);
            q.fill_meta(w, clip, 5, 2, 3, 5, 3, 6, PLANKS, 1, PLANKS, 1, false);
            q.fill_meta(w, clip, 2, 2, 7, 4, 3, 7, PLANKS, 1, PLANKS, 1, false);
            q.fill(w, clip, 1, 0, 2, 1, 3, 2, LOG, LOG, false);
            q.fill(w, clip, 5, 0, 2, 5, 3, 2, LOG, LOG, false);
            q.fill(w, clip, 1, 0, 7, 1, 3, 7, LOG, LOG, false);
            q.fill(w, clip, 5, 0, 7, 5, 3, 7, LOG, LOG, false);
            q.pb(w, FENCE, 0, 2, 3, 2, clip);
            q.pb(w, FENCE, 0, 3, 3, 7, clip);
            q.pb(w, AIR, 0, 1, 3, 4, clip);
            q.pb(w, AIR, 0, 5, 3, 4, clip);
            q.pb(w, AIR, 0, 5, 3, 5, clip);
            q.pb(w, FLOWER_POT, 7, 1, 3, 5, clip);
            q.pb(w, CRAFTING_TABLE, 0, 3, 2, 6, clip);
            q.pb(w, CAULDRON, 0, 4, 2, 6, clip);
            q.pb(w, FENCE, 0, 1, 2, 1, clip);
            q.pb(w, FENCE, 0, 5, 2, 1, clip);
            let (a, b, c, d) = (
                q.mo(OAK_STAIRS, 3),
                q.mo(OAK_STAIRS, 1),
                q.mo(OAK_STAIRS, 0),
                q.mo(OAK_STAIRS, 2),
            );
            q.fill_meta(
                w,
                clip,
                0,
                4,
                1,
                6,
                4,
                1,
                SPRUCE_STAIRS,
                a,
                SPRUCE_STAIRS,
                a,
                false,
            );
            q.fill_meta(
                w,
                clip,
                0,
                4,
                2,
                0,
                4,
                7,
                SPRUCE_STAIRS,
                c,
                SPRUCE_STAIRS,
                c,
                false,
            );
            q.fill_meta(
                w,
                clip,
                6,
                4,
                2,
                6,
                4,
                7,
                SPRUCE_STAIRS,
                b,
                SPRUCE_STAIRS,
                b,
                false,
            );
            q.fill_meta(
                w,
                clip,
                0,
                4,
                8,
                6,
                4,
                8,
                SPRUCE_STAIRS,
                d,
                SPRUCE_STAIRS,
                d,
                false,
            );
            for z in [2, 7] {
                for x in [1, 5] {
                    q.fill_down(w, LOG, 0, x, -1, z, clip);
                }
            }
            let mut witch = witch;
            if !witch {
                let (x, y, z) = (q.x_off(2, 5), q.y_off(2), q.z_off(2, 5));
                if clip.inside(x, y, z) {
                    witch = true;
                    super::spawn(
                        w,
                        GenEntity::Witch {
                            pos: [x as f64 + 0.5, y as f64, z as f64 + 0.5],
                        },
                    );
                }
            }
            if let Kind::Scatter(s) = &mut p.kind {
                s.k = SKind::Swamp { witch };
            }
            true
        }
    }
}

fn desert(q: &Piece, w: &mut ServerWorld, clip: &BB) {
    let Kind::Scatter(s) = q.kind else { return };
    let (sx, sz) = (s.w, s.d);
    q.fill(
        w,
        clip,
        0,
        -4,
        0,
        sx - 1,
        0,
        sz - 1,
        SANDSTONE,
        SANDSTONE,
        false,
    );
    for i in 1..=9 {
        q.fill(
            w,
            clip,
            i,
            i,
            i,
            sx - 1 - i,
            i,
            sz - 1 - i,
            SANDSTONE,
            SANDSTONE,
            false,
        );
        q.fill(
            w,
            clip,
            i + 1,
            i,
            i + 1,
            sx - 2 - i,
            i,
            sz - 2 - i,
            AIR,
            AIR,
            false,
        );
    }
    for x in 0..sx {
        for z in 0..sz {
            q.fill_down(w, SANDSTONE, 0, x, -5, z, clip);
        }
    }
    let s3 = q.mo(SANDSTONE_STAIRS, 3);
    let s2 = q.mo(SANDSTONE_STAIRS, 2);
    let s0 = q.mo(SANDSTONE_STAIRS, 0);
    let s1 = q.mo(SANDSTONE_STAIRS, 1);
    let (orange, blue) = (1, 11);
    q.fill(w, clip, 0, 0, 0, 4, 9, 4, SANDSTONE, AIR, false);
    q.fill(w, clip, 1, 10, 1, 3, 10, 3, SANDSTONE, SANDSTONE, false);
    q.pb(w, SANDSTONE_STAIRS, s3, 2, 10, 0, clip);
    q.pb(w, SANDSTONE_STAIRS, s2, 2, 10, 4, clip);
    q.pb(w, SANDSTONE_STAIRS, s0, 0, 10, 2, clip);
    q.pb(w, SANDSTONE_STAIRS, s1, 4, 10, 2, clip);
    q.fill(w, clip, sx - 5, 0, 0, sx - 1, 9, 4, SANDSTONE, AIR, false);
    q.fill(
        w,
        clip,
        sx - 4,
        10,
        1,
        sx - 2,
        10,
        3,
        SANDSTONE,
        SANDSTONE,
        false,
    );
    q.pb(w, SANDSTONE_STAIRS, s3, sx - 3, 10, 0, clip);
    q.pb(w, SANDSTONE_STAIRS, s2, sx - 3, 10, 4, clip);
    q.pb(w, SANDSTONE_STAIRS, s0, sx - 5, 10, 2, clip);
    q.pb(w, SANDSTONE_STAIRS, s1, sx - 1, 10, 2, clip);
    q.fill(w, clip, 8, 0, 0, 12, 4, 4, SANDSTONE, AIR, false);
    q.fill(w, clip, 9, 1, 0, 11, 3, 4, AIR, AIR, false);
    for (x, y) in [(9, 1), (9, 2), (9, 3), (10, 3), (11, 3), (11, 2), (11, 1)] {
        q.pb(w, SANDSTONE, 2, x, y, 1, clip);
    }
    q.fill(w, clip, 4, 1, 1, 8, 3, 3, SANDSTONE, AIR, false);
    q.fill(w, clip, 4, 1, 2, 8, 2, 2, AIR, AIR, false);
    q.fill(w, clip, 12, 1, 1, 16, 3, 3, SANDSTONE, AIR, false);
    q.fill(w, clip, 12, 1, 2, 16, 2, 2, AIR, AIR, false);
    q.fill(
        w,
        clip,
        5,
        4,
        5,
        sx - 6,
        4,
        sz - 6,
        SANDSTONE,
        SANDSTONE,
        false,
    );
    q.fill(w, clip, 9, 4, 9, 11, 4, 11, AIR, AIR, false);
    q.fill_meta(w, clip, 8, 1, 8, 8, 3, 8, SANDSTONE, 2, SANDSTONE, 2, false);
    q.fill_meta(
        w, clip, 12, 1, 8, 12, 3, 8, SANDSTONE, 2, SANDSTONE, 2, false,
    );
    q.fill_meta(
        w, clip, 8, 1, 12, 8, 3, 12, SANDSTONE, 2, SANDSTONE, 2, false,
    );
    q.fill_meta(
        w, clip, 12, 1, 12, 12, 3, 12, SANDSTONE, 2, SANDSTONE, 2, false,
    );
    q.fill(w, clip, 1, 1, 5, 4, 4, 11, SANDSTONE, SANDSTONE, false);
    q.fill(
        w,
        clip,
        sx - 5,
        1,
        5,
        sx - 2,
        4,
        11,
        SANDSTONE,
        SANDSTONE,
        false,
    );
    q.fill(w, clip, 6, 7, 9, 6, 7, 11, SANDSTONE, SANDSTONE, false);
    q.fill(
        w,
        clip,
        sx - 7,
        7,
        9,
        sx - 7,
        7,
        11,
        SANDSTONE,
        SANDSTONE,
        false,
    );
    q.fill_meta(
        w, clip, 5, 5, 9, 5, 7, 11, SANDSTONE, 2, SANDSTONE, 2, false,
    );
    q.fill_meta(
        w,
        clip,
        sx - 6,
        5,
        9,
        sx - 6,
        7,
        11,
        SANDSTONE,
        2,
        SANDSTONE,
        2,
        false,
    );
    q.pb(w, AIR, 0, 5, 5, 10, clip);
    q.pb(w, AIR, 0, 5, 6, 10, clip);
    q.pb(w, AIR, 0, 6, 6, 10, clip);
    q.pb(w, AIR, 0, sx - 6, 5, 10, clip);
    q.pb(w, AIR, 0, sx - 6, 6, 10, clip);
    q.pb(w, AIR, 0, sx - 7, 6, 10, clip);
    q.fill(w, clip, 2, 4, 4, 2, 6, 4, AIR, AIR, false);
    q.fill(w, clip, sx - 3, 4, 4, sx - 3, 6, 4, AIR, AIR, false);
    q.pb(w, SANDSTONE_STAIRS, s3, 2, 4, 5, clip);
    q.pb(w, SANDSTONE_STAIRS, s3, 2, 3, 4, clip);
    q.pb(w, SANDSTONE_STAIRS, s3, sx - 3, 4, 5, clip);
    q.pb(w, SANDSTONE_STAIRS, s3, sx - 3, 3, 4, clip);
    q.fill(w, clip, 1, 1, 3, 2, 2, 3, SANDSTONE, SANDSTONE, false);
    q.fill(
        w,
        clip,
        sx - 3,
        1,
        3,
        sx - 2,
        2,
        3,
        SANDSTONE,
        SANDSTONE,
        false,
    );
    q.pb(w, SANDSTONE_STAIRS, 0, 1, 1, 2, clip);
    q.pb(w, SANDSTONE_STAIRS, 0, sx - 2, 1, 2, clip);
    q.pb(w, STONE_SLAB, 1, 1, 2, 2, clip);
    q.pb(w, STONE_SLAB, 1, sx - 2, 2, 2, clip);
    q.pb(w, SANDSTONE_STAIRS, s1, 2, 1, 2, clip);
    q.pb(w, SANDSTONE_STAIRS, s0, sx - 3, 1, 2, clip);
    q.fill(w, clip, 4, 3, 5, 4, 3, 18, SANDSTONE, SANDSTONE, false);
    q.fill(
        w,
        clip,
        sx - 5,
        3,
        5,
        sx - 5,
        3,
        17,
        SANDSTONE,
        SANDSTONE,
        false,
    );
    q.fill(w, clip, 3, 1, 5, 4, 2, 16, AIR, AIR, false);
    q.fill(w, clip, sx - 6, 1, 5, sx - 5, 2, 16, AIR, AIR, false);
    let mut z = 5;
    while z <= 17 {
        q.pb(w, SANDSTONE, 2, 4, 1, z, clip);
        q.pb(w, SANDSTONE, 1, 4, 2, z, clip);
        q.pb(w, SANDSTONE, 2, sx - 5, 1, z, clip);
        q.pb(w, SANDSTONE, 1, sx - 5, 2, z, clip);
        z += 2;
    }
    for (x, z) in [
        (10, 7),
        (10, 8),
        (9, 9),
        (11, 9),
        (8, 10),
        (12, 10),
        (7, 10),
        (13, 10),
        (9, 11),
        (11, 11),
        (10, 12),
        (10, 13),
    ] {
        q.pb(w, WOOL, orange, x, 0, z, clip);
    }
    q.pb(w, WOOL, blue, 10, 0, 10, clip);
    // The two corner towers' decorated faces.
    let tower: [(u8, i32, i32, i32); 21] = [
        (SANDSTONE, 2, 2, 1),
        (WOOL, -1, 2, 2),
        (SANDSTONE, 2, 2, 3),
        (SANDSTONE, 2, 3, 1),
        (WOOL, -1, 3, 2),
        (SANDSTONE, 2, 3, 3),
        (WOOL, -1, 4, 1),
        (SANDSTONE, 1, 4, 2),
        (WOOL, -1, 4, 3),
        (SANDSTONE, 2, 5, 1),
        (WOOL, -1, 5, 2),
        (SANDSTONE, 2, 5, 3),
        (WOOL, -1, 6, 1),
        (SANDSTONE, 1, 6, 2),
        (WOOL, -1, 6, 3),
        (WOOL, -1, 7, 1),
        (WOOL, -1, 7, 2),
        (WOOL, -1, 7, 3),
        (SANDSTONE, 2, 8, 1),
        (SANDSTONE, 2, 8, 2),
        (SANDSTONE, 2, 8, 3),
    ];
    let mut x = 0;
    while x <= sx - 1 {
        for &(id, m, y, z) in &tower {
            q.pb(w, id, if m < 0 { orange } else { m }, x, y, z, clip);
        }
        x += sx - 1;
    }
    let mut x = 2;
    while x <= sx - 3 {
        for &(id, m, y, dz) in &tower {
            // Same pattern laid along X on the front face (z = 0).
            q.pb(
                w,
                id,
                if m < 0 { orange } else { m },
                x + dz - 2,
                y,
                0,
                clip,
            );
        }
        x += sx - 3 - 2;
    }
    q.fill_meta(
        w, clip, 8, 4, 0, 12, 6, 0, SANDSTONE, 2, SANDSTONE, 2, false,
    );
    q.pb(w, AIR, 0, 8, 6, 0, clip);
    q.pb(w, AIR, 0, 12, 6, 0, clip);
    q.pb(w, WOOL, orange, 9, 5, 0, clip);
    q.pb(w, SANDSTONE, 1, 10, 5, 0, clip);
    q.pb(w, WOOL, orange, 11, 5, 0, clip);
    q.fill_meta(
        w, clip, 8, -14, 8, 12, -11, 12, SANDSTONE, 2, SANDSTONE, 2, false,
    );
    q.fill_meta(
        w, clip, 8, -10, 8, 12, -10, 12, SANDSTONE, 1, SANDSTONE, 1, false,
    );
    q.fill_meta(
        w, clip, 8, -9, 8, 12, -9, 12, SANDSTONE, 2, SANDSTONE, 2, false,
    );
    q.fill(w, clip, 8, -8, 8, 12, -1, 12, SANDSTONE, SANDSTONE, false);
    q.fill(w, clip, 9, -11, 9, 11, -1, 11, AIR, AIR, false);
    q.pb(w, STONE_PRESSURE_PLATE, 0, 10, -11, 10, clip);
    q.fill(w, clip, 9, -13, 9, 11, -13, 11, TNT, AIR, false);
    q.pb(w, AIR, 0, 8, -11, 10, clip);
    q.pb(w, AIR, 0, 8, -10, 10, clip);
    q.pb(w, SANDSTONE, 1, 7, -10, 10, clip);
    q.pb(w, SANDSTONE, 2, 7, -11, 10, clip);
    q.pb(w, AIR, 0, 12, -11, 10, clip);
    q.pb(w, AIR, 0, 12, -10, 10, clip);
    q.pb(w, SANDSTONE, 1, 13, -10, 10, clip);
    q.pb(w, SANDSTONE, 2, 13, -11, 10, clip);
    q.pb(w, AIR, 0, 10, -11, 8, clip);
    q.pb(w, AIR, 0, 10, -10, 8, clip);
    q.pb(w, SANDSTONE, 1, 10, -10, 7, clip);
    q.pb(w, SANDSTONE, 2, 10, -11, 7, clip);
    q.pb(w, AIR, 0, 10, -11, 12, clip);
    q.pb(w, AIR, 0, 10, -10, 12, clip);
    q.pb(w, SANDSTONE, 1, 10, -10, 13, clip);
    q.pb(w, SANDSTONE, 2, 10, -11, 13, clip);
}

fn jungle(p: &mut Piece, w: &mut ServerWorld, r: &mut JavaRandom, clip: &BB) {
    let q = p.clone();
    let Kind::Scatter(s) = q.kind else { return };
    let SKind::Jungle {
        mut main,
        mut hidden,
        mut trap1,
        mut trap2,
    } = s.k
    else {
        return;
    };
    let (sx, sz) = (s.w, s.d);
    let a = q.mo(STONE_STAIRS, 3);
    let b = q.mo(STONE_STAIRS, 2);
    let c = q.mo(STONE_STAIRS, 0);
    let d = q.mo(STONE_STAIRS, 1);
    let rs =
        |w: &mut ServerWorld,
         r: &mut JavaRandom,
         x0: i32,
         y0: i32,
         z0: i32,
         x1: i32,
         y1: i32,
         z1: i32| q.fill_random(w, clip, x0, y0, z0, x1, y1, z1, false, r, &stones);
    rs(w, r, 0, -4, 0, sx - 1, 0, sz - 1);
    rs(w, r, 2, 1, 2, 9, 2, 2);
    rs(w, r, 2, 1, 12, 9, 2, 12);
    rs(w, r, 2, 1, 3, 2, 2, 11);
    rs(w, r, 9, 1, 3, 9, 2, 11);
    rs(w, r, 1, 3, 1, 10, 6, 1);
    rs(w, r, 1, 3, 13, 10, 6, 13);
    rs(w, r, 1, 3, 2, 1, 6, 12);
    rs(w, r, 10, 3, 2, 10, 6, 12);
    rs(w, r, 2, 3, 2, 9, 3, 12);
    rs(w, r, 2, 6, 2, 9, 6, 12);
    rs(w, r, 3, 7, 3, 8, 7, 11);
    rs(w, r, 4, 8, 4, 7, 8, 10);
    q.fill_air(w, clip, 3, 1, 3, 8, 2, 11);
    q.fill_air(w, clip, 4, 3, 6, 7, 3, 9);
    q.fill_air(w, clip, 2, 4, 2, 9, 5, 12);
    q.fill_air(w, clip, 4, 6, 5, 7, 6, 9);
    q.fill_air(w, clip, 5, 7, 6, 6, 7, 8);
    q.fill_air(w, clip, 5, 1, 2, 6, 2, 2);
    q.fill_air(w, clip, 5, 2, 12, 6, 2, 12);
    q.fill_air(w, clip, 5, 5, 1, 6, 5, 1);
    q.fill_air(w, clip, 5, 5, 13, 6, 5, 13);
    q.pb(w, AIR, 0, 1, 5, 5, clip);
    q.pb(w, AIR, 0, 10, 5, 5, clip);
    q.pb(w, AIR, 0, 1, 5, 9, clip);
    q.pb(w, AIR, 0, 10, 5, 9, clip);
    for z in [0, 14] {
        rs(w, r, 2, 4, z, 2, 5, z);
        rs(w, r, 4, 4, z, 4, 5, z);
        rs(w, r, 7, 4, z, 7, 5, z);
        rs(w, r, 9, 4, z, 9, 5, z);
    }
    rs(w, r, 5, 6, 0, 6, 6, 0);
    for x in [0, 11] {
        let mut z = 2;
        while z <= 12 {
            rs(w, r, x, 4, z, x, 5, z);
            z += 2;
        }
        rs(w, r, x, 6, 5, x, 6, 5);
        rs(w, r, x, 6, 9, x, 6, 9);
    }
    rs(w, r, 2, 7, 2, 2, 9, 2);
    rs(w, r, 9, 7, 2, 9, 9, 2);
    rs(w, r, 2, 7, 12, 2, 9, 12);
    rs(w, r, 9, 7, 12, 9, 9, 12);
    rs(w, r, 4, 9, 4, 4, 9, 4);
    rs(w, r, 7, 9, 4, 7, 9, 4);
    rs(w, r, 4, 9, 10, 4, 9, 10);
    rs(w, r, 7, 9, 10, 7, 9, 10);
    rs(w, r, 5, 9, 7, 6, 9, 7);
    for (m, x, y, z) in [
        (a, 5, 9, 6),
        (a, 6, 9, 6),
        (b, 5, 9, 8),
        (b, 6, 9, 8),
        (a, 4, 0, 0),
        (a, 5, 0, 0),
        (a, 6, 0, 0),
        (a, 7, 0, 0),
        (a, 4, 1, 8),
        (a, 4, 2, 9),
        (a, 4, 3, 10),
        (a, 7, 1, 8),
        (a, 7, 2, 9),
        (a, 7, 3, 10),
    ] {
        q.pb(w, STONE_STAIRS, m, x, y, z, clip);
    }
    rs(w, r, 4, 1, 9, 4, 1, 9);
    rs(w, r, 7, 1, 9, 7, 1, 9);
    rs(w, r, 4, 1, 10, 7, 2, 10);
    rs(w, r, 5, 4, 5, 6, 4, 5);
    q.pb(w, STONE_STAIRS, c, 4, 4, 5, clip);
    q.pb(w, STONE_STAIRS, d, 7, 4, 5, clip);
    for i in 0..4 {
        q.pb(w, STONE_STAIRS, b, 5, -i, 6 + i, clip);
        q.pb(w, STONE_STAIRS, b, 6, -i, 6 + i, clip);
        q.fill_air(w, clip, 5, -i, 7 + i, 6, -i, 9 + i);
    }
    q.fill_air(w, clip, 1, -3, 12, 10, -1, 13);
    q.fill_air(w, clip, 1, -3, 1, 3, -1, 13);
    q.fill_air(w, clip, 1, -3, 1, 9, -1, 5);
    let mut z = 1;
    while z <= 13 {
        rs(w, r, 1, -3, z, 1, -2, z);
        z += 2;
    }
    let mut z = 2;
    while z <= 12 {
        rs(w, r, 1, -1, z, 3, -1, z);
        z += 2;
    }
    rs(w, r, 2, -2, 1, 5, -2, 1);
    rs(w, r, 7, -2, 1, 9, -2, 1);
    rs(w, r, 6, -3, 1, 6, -3, 1);
    rs(w, r, 6, -1, 1, 6, -1, 1);
    q.pb(w, TRIPWIRE_HOOK, q.mo(TRIPWIRE_HOOK, 3) | 4, 1, -3, 8, clip);
    q.pb(w, TRIPWIRE_HOOK, q.mo(TRIPWIRE_HOOK, 1) | 4, 4, -3, 8, clip);
    q.pb(w, TRIPWIRE, 4, 2, -3, 8, clip);
    q.pb(w, TRIPWIRE, 4, 3, -3, 8, clip);
    for z in (1..=7).rev() {
        q.pb(w, REDSTONE_WIRE, 0, 5, -3, z, clip);
    }
    q.pb(w, REDSTONE_WIRE, 0, 4, -3, 1, clip);
    q.pb(w, MOSSY_COBBLESTONE, 0, 3, -3, 1, clip);
    if !trap1 {
        trap1 = q.dispenser(w, clip, r, 3, -2, 1, 2, loot::JUNGLE_DISPENSER, 2);
    }
    q.pb(w, VINE, 15, 3, -2, 2, clip);
    q.pb(w, TRIPWIRE_HOOK, q.mo(TRIPWIRE_HOOK, 2) | 4, 7, -3, 1, clip);
    q.pb(w, TRIPWIRE_HOOK, q.mo(TRIPWIRE_HOOK, 0) | 4, 7, -3, 5, clip);
    q.pb(w, TRIPWIRE, 4, 7, -3, 2, clip);
    q.pb(w, TRIPWIRE, 4, 7, -3, 3, clip);
    q.pb(w, TRIPWIRE, 4, 7, -3, 4, clip);
    q.pb(w, REDSTONE_WIRE, 0, 8, -3, 6, clip);
    q.pb(w, REDSTONE_WIRE, 0, 9, -3, 6, clip);
    q.pb(w, REDSTONE_WIRE, 0, 9, -3, 5, clip);
    q.pb(w, MOSSY_COBBLESTONE, 0, 9, -3, 4, clip);
    q.pb(w, REDSTONE_WIRE, 0, 9, -2, 4, clip);
    if !trap2 {
        trap2 = q.dispenser(w, clip, r, 9, -2, 3, 4, loot::JUNGLE_DISPENSER, 2);
    }
    q.pb(w, VINE, 15, 8, -1, 3, clip);
    q.pb(w, VINE, 15, 8, -2, 3, clip);
    if !main {
        let book = loot::enchanted_book(r, 1, 1, 1);
        let table = loot::with(loot::TEMPLE, book);
        let n = 2 + r.next_int_n(5);
        main = q.chest(w, clip, r, 8, -3, 3, &table, n);
    }
    for (x, y, z) in [
        (9, -3, 2),
        (8, -3, 1),
        (4, -3, 5),
        (5, -2, 5),
        (5, -1, 5),
        (6, -3, 5),
        (7, -2, 5),
        (7, -1, 5),
        (8, -3, 5),
    ] {
        q.pb(w, MOSSY_COBBLESTONE, 0, x, y, z, clip);
    }
    rs(w, r, 9, -1, 1, 9, -1, 5);
    q.fill_air(w, clip, 8, -3, 8, 10, -1, 10);
    q.pb(w, STONEBRICK, 3, 8, -2, 11, clip);
    q.pb(w, STONEBRICK, 3, 9, -2, 11, clip);
    q.pb(w, STONEBRICK, 3, 10, -2, 11, clip);
    let lm = lever_meta(q.mo(LEVER, 2));
    q.pb(w, LEVER, lm, 8, -2, 12, clip);
    q.pb(w, LEVER, lm, 9, -2, 12, clip);
    q.pb(w, LEVER, lm, 10, -2, 12, clip);
    rs(w, r, 8, -3, 8, 8, -3, 10);
    rs(w, r, 10, -3, 8, 10, -3, 10);
    q.pb(w, MOSSY_COBBLESTONE, 0, 10, -2, 9, clip);
    q.pb(w, REDSTONE_WIRE, 0, 8, -2, 9, clip);
    q.pb(w, REDSTONE_WIRE, 0, 8, -2, 10, clip);
    q.pb(w, REDSTONE_WIRE, 0, 10, -1, 9, clip);
    q.pb(w, STICKY_PISTON, 1, 9, -2, 8, clip);
    q.pb(w, STICKY_PISTON, q.mo(STICKY_PISTON, 4), 10, -2, 8, clip);
    q.pb(w, STICKY_PISTON, q.mo(STICKY_PISTON, 4), 10, -1, 8, clip);
    q.pb(
        w,
        UNPOWERED_REPEATER,
        q.mo(UNPOWERED_REPEATER, 2),
        10,
        -2,
        10,
        clip,
    );
    if !hidden {
        let book = loot::enchanted_book(r, 1, 1, 1);
        let table = loot::with(loot::TEMPLE, book);
        let n = 2 + r.next_int_n(5);
        hidden = q.chest(w, clip, r, 9, -3, 10, &table, n);
    }
    if let Kind::Scatter(s) = &mut p.kind {
        s.k = SKind::Jungle {
            main,
            hidden,
            trap1,
            trap2,
        };
    }
}
