//! Random block ticks (`WorldServer.func_147456_g`) and the `updateTick`
//! behaviour of the blocks that use them.

use crate::block::{self, Material};
use crate::java::JavaRandom;
use crate::world::server::ServerWorld;

/// `Direction.offsetX` / `offsetZ` / `facingToDirection`
const DIR_X: [i32; 4] = [0, -1, 0, 1];
const DIR_Z: [i32; 4] = [1, 0, -1, 0];
const FACING_TO_DIR: [i32; 6] = [-1, -1, 2, 0, 1, 3];

/// `Block.getTickRandomly()`
#[inline]
pub fn ticks_randomly(id: u8) -> bool {
    matches!(
        id,
        block::GRASS
            | block::MYCELIUM
            | block::LEAVES
            | block::LEAVES2
            | block::ICE
            | block::SNOW_LAYER
            | block::CACTUS
            | block::REEDS
            | block::BROWN_MUSHROOM
            | block::RED_MUSHROOM
            | block::VINE
            | block::FIRE
            | block::SAPLING
            | block::YELLOW_FLOWER
            | block::RED_FLOWER
            | block::TALLGRASS
            | block::DEADBUSH
            | block::WATERLILY
            | block::WHEAT
            | block::CARROTS
            | block::POTATOES
            | block::PUMPKIN_STEM
            | block::MELON_STEM
            | block::NETHER_WART
            | block::COCOA
            | block::FARMLAND
            | block::LIT_REDSTONE_ORE
            | block::SNOW
            | block::LAVA
            | block::PORTAL
            // BlockLiquid: every liquid; still water's updateTick is empty
            | block::FLOWING_WATER
            | block::FLOWING_LAVA
            | block::DOUBLE_PLANT
            | block::TORCH
            | block::REDSTONE_TORCH
            | block::UNLIT_REDSTONE_TORCH
            | block::STONE_BUTTON
            | block::WOODEN_BUTTON
            | block::STONE_PRESSURE_PLATE
            | block::WOODEN_PRESSURE_PLATE
            | block::DETECTOR_RAIL
            | block::TRIPWIRE
            | block::TRIPWIRE_HOOK
    )
}

impl ServerWorld {
    /// The per-chunk part of `WorldServer.func_147456_g`: natural freezing and
    /// three random block ticks per 16-high section.
    pub fn update_blocks(&mut self, cx: i32, cz: i32) {
        if !self.chunk_exists(cx, cz) {
            return;
        }
        let (bx, bz) = (cx * 16, cz * 16);
        if self.rand.next_int_n(16) == 0 {
            self.update_lcg = self.update_lcg.wrapping_mul(3).wrapping_add(1013904223);
            let k = self.update_lcg >> 2;
            let (x, z) = (bx + (k & 15), bz + (k >> 8 & 15));
            let y = self.precipitation_height(x, z);
            if self.can_block_freeze(x, y - 1, z, true) {
                self.set_block(x, y - 1, z, block::ICE, 0, 3);
            }
            if self.raining && self.can_snow_at(x, y, z, true) {
                self.set_block(x, y, z, block::SNOW_LAYER, 0, 3);
            }
        }
        let sections = self.chunks[&(cx, cz)].sections;
        for sec in 0..16i32 {
            if sections & (1 << sec) == 0 {
                continue;
            }
            for _ in 0..3 {
                self.update_lcg = self.update_lcg.wrapping_mul(3).wrapping_add(1013904223);
                let k = self.update_lcg >> 2;
                let (lx, lz, ly) = (k & 15, k >> 8 & 15, k >> 16 & 15);
                let (x, y, z) = (bx + lx, (sec << 4) + ly, bz + lz);
                let id = match self.chunks.get(&(cx, cz)) {
                    Some(c) => c.block(lx, y, lz),
                    None => return,
                };
                if ticks_randomly(id) {
                    let mut rnd = std::mem::replace(&mut self.rand, JavaRandom::new(0));
                    self.random_tick(id, x, y, z, &mut rnd);
                    self.rand = rnd;
                }
            }
        }
    }

    /// `Block.updateTick` for randomly ticking blocks.
    pub fn random_tick(&mut self, id: u8, x: i32, y: i32, z: i32, r: &mut JavaRandom) {
        match id {
            block::GRASS | block::MYCELIUM => {
                let above_opacity = block::info(self.block(x, y + 1, z)).light_opacity;
                if self.block_light_value(x, y + 1, z, true) < 4 && above_opacity > 2 {
                    self.set_block(x, y, z, block::DIRT, 0, 3);
                } else if self.block_light_value(x, y + 1, z, true) >= 9 {
                    for _ in 0..4 {
                        let tx = x + r.next_int_n(3) - 1;
                        let ty = y + r.next_int_n(5) - 3;
                        let tz = z + r.next_int_n(3) - 1;
                        let above = self.block(tx, ty + 1, tz);
                        if self.block(tx, ty, tz) == block::DIRT
                            && self.meta(tx, ty, tz) == 0
                            && self.block_light_value(tx, ty + 1, tz, true) >= 4
                            && block::info(above).light_opacity <= 2
                        {
                            self.set_block(tx, ty, tz, id, 0, 3);
                        }
                    }
                }
            }
            block::LEAVES | block::LEAVES2 => self.leaf_decay(id, x, y, z, r),
            block::FIRE => self.fire_tick(x, y, z, r),
            block::ICE => {
                if self.saved_light(false, x, y, z) > 11 - 3 {
                    if self.provider.dim == -1 {
                        // provider.isHellWorld
                        self.set_air(x, y, z);
                        return;
                    }
                    self.set_block(x, y, z, block::WATER, 0, 3);
                }
            }
            block::SNOW_LAYER => {
                if self.saved_light(false, x, y, z) > 11 {
                    self.set_air(x, y, z);
                }
            }
            block::CACTUS | block::REEDS => {
                // BlockReed.updateTick: func_150170_e drops the reed
                if id == block::REEDS
                    && self.block(x, y - 1, z) != block::REEDS
                    && !self.can_block_stay(id, x, y, z)
                {
                    let m = self.meta(x, y, z);
                    self.drop_block_with(r, id, m, x, y, z);
                    self.set_air(x, y, z);
                    return;
                }
                if self.is_air(x, y + 1, z) {
                    let mut h = 1;
                    while self.block(x, y - h, z) == id {
                        h += 1;
                    }
                    if h < 3 {
                        let m = self.meta(x, y, z);
                        if m == 15 {
                            self.set_block(x, y + 1, z, id, 0, 3);
                            self.set_meta(x, y, z, 0, 4);
                            // BlockCactus: onNeighborBlockChange(x, y + 1, z)
                            if id == block::CACTUS && !self.can_block_stay(id, x, y + 1, z) {
                                self.destroy_block_with(r, x, y + 1, z);
                            }
                        } else {
                            self.set_meta(x, y, z, m + 1, 4);
                        }
                    }
                }
            }
            block::BROWN_MUSHROOM | block::RED_MUSHROOM => {
                if r.next_int_n(25) != 0 {
                    return;
                }
                let mut budget = 5;
                for bx in (x - 4)..=(x + 4) {
                    for bz in (z - 4)..=(z + 4) {
                        for by in (y - 1)..=(y + 1) {
                            if self.block(bx, by, bz) == id {
                                budget -= 1;
                                if budget <= 0 {
                                    return;
                                }
                            }
                        }
                    }
                }
                let (mut px, mut py, mut pz) = (x, y, z);
                let mut tx = px + r.next_int_n(3) - 1;
                let mut ty = py + r.next_int_n(2) - r.next_int_n(2);
                let mut tz = pz + r.next_int_n(3) - 1;
                for _ in 0..4 {
                    if self.is_air(tx, ty, tz) && self.can_block_stay(id, tx, ty, tz) {
                        px = tx;
                        py = ty;
                        pz = tz;
                    }
                    tx = px + r.next_int_n(3) - 1;
                    ty = py + r.next_int_n(2) - r.next_int_n(2);
                    tz = pz + r.next_int_n(3) - 1;
                }
                if self.is_air(tx, ty, tz) && self.can_block_stay(id, tx, ty, tz) {
                    self.set_block(tx, ty, tz, id, 0, 2);
                }
            }
            block::VINE => self.vine_tick(x, y, z, r),
            // BlockDoublePlant.checkAndDropBlock (BlockBush.updateTick)
            block::DOUBLE_PLANT => self.double_plant_check(r, x, y, z),
            // BlockTorch.updateTick: a torch without a side yet picks one
            block::TORCH => {
                if self.meta(x, y, z) == 0 {
                    self.torch_added(id, x, y, z);
                }
            }
            // The blocks whose updateTick is the same for a random tick as
            // for a scheduled one: flowing liquids, redstone torches,
            // buttons, pressure plates, detector rails, tripwire and hooks
            block::FLOWING_WATER
            | block::FLOWING_LAVA
            | block::REDSTONE_TORCH
            | block::UNLIT_REDSTONE_TORCH
            | block::STONE_BUTTON
            | block::WOODEN_BUTTON
            | block::STONE_PRESSURE_PLATE
            | block::WOODEN_PRESSURE_PLATE
            | block::DETECTOR_RAIL
            | block::TRIPWIRE
            | block::TRIPWIRE_HOOK => self.update_tick(id, x, y, z, r),
            _ => {
                self.growth_tick(id, x, y, z, r);
            }
        }
    }

    /// `BlockLeaves.updateTick`: leaves flagged for a check decay unless a log
    /// is reachable through at most four leaf blocks.
    fn leaf_decay(&mut self, id: u8, x: i32, y: i32, z: i32, r: &mut JavaRandom) {
        let meta = self.meta(x, y, z);
        if meta & 8 == 0 || meta & 4 != 0 {
            return;
        }
        const R: i32 = 4;
        const N: usize = 32;
        const H: i32 = 16;
        let at = |dx: i32, dy: i32, dz: i32| {
            ((dx + H) as usize) * N * N + ((dy + H) as usize) * N + (dz + H) as usize
        };
        // Vanilla keeps this array between calls; when the neighbourhood is
        // not loaded the stale centre value decides. A fresh array (centre 0)
        // means "keep", which is also what a stale log-reachable result gives.
        let mut s = vec![0i32; N * N * N];
        if self.check_chunks_exist(
            x - R - 1,
            y - R - 1,
            z - R - 1,
            x + R + 1,
            y + R + 1,
            z + R + 1,
        ) {
            for dx in -R..=R {
                for dy in -R..=R {
                    for dz in -R..=R {
                        let b = self.block(x + dx, y + dy, z + dz);
                        s[at(dx, dy, dz)] = if b == block::LOG || b == block::LOG2 {
                            0
                        } else if block::material(b) == Material::Leaves {
                            -2
                        } else {
                            -1
                        };
                    }
                }
            }
            for step in 1..=4 {
                for dx in -R..=R {
                    for dy in -R..=R {
                        for dz in -R..=R {
                            if s[at(dx, dy, dz)] == step - 1 {
                                for (ox, oy, oz) in [
                                    (-1, 0, 0),
                                    (1, 0, 0),
                                    (0, -1, 0),
                                    (0, 1, 0),
                                    (0, 0, -1),
                                    (0, 0, 1),
                                ] {
                                    let i = at(dx + ox, dy + oy, dz + oz);
                                    if s[i] == -2 {
                                        s[i] = step;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if s[at(0, 0, 0)] >= 0 {
            self.set_meta(x, y, z, meta & !8, 4);
        } else {
            // removeLeaves: the sapling and apple chances, then air
            self.drop_block_with(r, id, meta, x, y, z);
            self.set_air(x, y, z);
        }
    }

    /// `BlockVine.updateTick`
    fn vine_tick(&mut self, x: i32, y: i32, z: i32, r: &mut JavaRandom) {
        if r.next_int_n(4) != 0 {
            return;
        }
        let mut budget = 5;
        let mut crowded = false;
        'count: for bx in (x - 4)..=(x + 4) {
            for bz in (z - 4)..=(z + 4) {
                for by in (y - 1)..=(y + 1) {
                    if self.block(bx, by, bz) == block::VINE {
                        budget -= 1;
                        if budget <= 0 {
                            crowded = true;
                            break 'count;
                        }
                    }
                }
            }
        }
        let meta = self.meta(x, y, z);
        let side = r.next_int_n(6);
        let dir = FACING_TO_DIR[side as usize];
        if side == 1 && y < 255 && self.is_air(x, y + 1, z) {
            if crowded {
                return;
            }
            let mut m = r.next_int_n(16) & meta;
            if m > 0 {
                for d in 0..4usize {
                    if !self.vine_can_attach(x + DIR_X[d], y + 1, z + DIR_Z[d]) {
                        m &= !(1 << d);
                    }
                }
                if m > 0 {
                    self.set_block(x, y + 1, z, block::VINE, m, 2);
                }
            }
        } else if (2..=5).contains(&side) && meta & (1 << dir) == 0 {
            if crowded {
                return;
            }
            let d = dir as usize;
            let (nx, nz) = (x + DIR_X[d], z + DIR_Z[d]);
            let nb = block::info(self.block(nx, y, nz));
            if nb.material == Material::Air {
                let l = (d + 1) & 3;
                let rr = (d + 3) & 3;
                if meta & (1 << l) != 0 && self.vine_can_attach(nx + DIR_X[l], y, nz + DIR_Z[l]) {
                    self.set_block(nx, y, nz, block::VINE, 1 << l, 2);
                } else if meta & (1 << rr) != 0
                    && self.vine_can_attach(nx + DIR_X[rr], y, nz + DIR_Z[rr])
                {
                    self.set_block(nx, y, nz, block::VINE, 1 << rr, 2);
                } else if meta & (1 << l) != 0
                    && self.is_air(nx + DIR_X[l], y, nz + DIR_Z[l])
                    && self.vine_can_attach(x + DIR_X[l], y, z + DIR_Z[l])
                {
                    self.set_block(
                        nx + DIR_X[l],
                        y,
                        nz + DIR_Z[l],
                        block::VINE,
                        1 << ((d + 2) & 3),
                        2,
                    );
                } else if meta & (1 << rr) != 0
                    && self.is_air(nx + DIR_X[rr], y, nz + DIR_Z[rr])
                    && self.vine_can_attach(x + DIR_X[rr], y, z + DIR_Z[rr])
                {
                    self.set_block(
                        nx + DIR_X[rr],
                        y,
                        nz + DIR_Z[rr],
                        block::VINE,
                        1 << ((d + 2) & 3),
                        2,
                    );
                } else if self.vine_can_attach(nx, y + 1, nz) {
                    self.set_block(nx, y, nz, block::VINE, 0, 2);
                }
            } else if nb.normal_cube() {
                self.set_meta(x, y, z, meta | 1 << dir, 2);
            }
        } else if y > 1 {
            let below = self.block(x, y - 1, z);
            if block::material(below) == Material::Air {
                let m = r.next_int_n(16) & meta;
                if m > 0 {
                    self.set_block(x, y - 1, z, block::VINE, m, 2);
                }
            } else if below == block::VINE {
                let m = r.next_int_n(16) & meta;
                let bm = self.meta(x, y - 1, z);
                if bm != (bm | m) {
                    self.set_meta(x, y - 1, z, bm | m, 2);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::gen::chunk_provider::boxed;
    use crate::world::{ChunkData, ChunkProvider};
    use std::sync::Arc;

    fn flat() -> ServerWorld {
        let mut w = ServerWorld::new(Arc::new(ChunkProvider::new(0)));
        for cx in -2..=2 {
            for cz in -2..=2 {
                let mut blocks = boxed();
                for c in 0..256 {
                    for y in 0..64 {
                        blocks[(c << 8) | y] = if y == 63 { block::DIRT } else { block::STONE };
                    }
                }
                w.insert_raw(ChunkData {
                    cx,
                    cz,
                    blocks,
                    meta: boxed(),
                    biomes: [1; 256],
                    sky: boxed(),
                    blk: boxed(),
                });
            }
        }
        w
    }

    #[test]
    fn grass_spreads_to_lit_dirt_and_dies_under_cover() {
        let mut w = flat();
        w.set_block(0, 63, 0, block::GRASS, 0, 3);
        let mut r = JavaRandom::new(1);
        for _ in 0..400 {
            for x in -2..=2 {
                for z in -2..=2 {
                    if w.block(x, 63, z) == block::GRASS {
                        w.random_tick(block::GRASS, x, 63, z, &mut r);
                    }
                }
            }
        }
        let grown = (-2..=2)
            .flat_map(|x| (-2..=2).map(move |z| (x, z)))
            .filter(|&(x, z)| w.block(x, 63, z) == block::GRASS)
            .count();
        assert!(grown > 10, "grass did not spread: {grown}");
        // Covered by an opaque block it turns back into dirt.
        w.set_block(0, 64, 0, block::STONE, 0, 3);
        w.random_tick(block::GRASS, 0, 63, 0, &mut r);
        assert_eq!(w.block(0, 63, 0), block::DIRT);
    }

    #[test]
    fn leaves_decay_only_without_a_log() {
        let mut w = flat();
        let mut r = JavaRandom::new(1);
        w.set_block(0, 64, 0, block::LOG, 0, 3);
        w.set_block(1, 64, 0, block::LEAVES, 8, 3);
        w.set_block(8, 64, 8, block::LEAVES, 8, 3);
        w.random_tick(block::LEAVES, 1, 64, 0, &mut r);
        w.random_tick(block::LEAVES, 8, 64, 8, &mut r);
        assert_eq!(w.block(1, 64, 0), block::LEAVES);
        assert_eq!(w.meta(1, 64, 0), 0, "check flag must be cleared");
        assert_eq!(w.block(8, 64, 8), block::AIR);
    }

    #[test]
    fn cactus_grows_to_three_blocks() {
        let mut w = flat();
        let mut r = JavaRandom::new(1);
        w.set_block(0, 63, 0, block::SAND, 0, 2);
        w.set_block(0, 64, 0, block::CACTUS, 0, 2);
        for _ in 0..200 {
            for y in 64..70 {
                if w.block(0, y, 0) == block::CACTUS {
                    w.random_tick(block::CACTUS, 0, y, 0, &mut r);
                }
            }
        }
        assert_eq!(w.block(0, 66, 0), block::CACTUS);
        assert_eq!(w.block(0, 67, 0), block::AIR);
    }

    fn items(w: &ServerWorld) -> Vec<u16> {
        use crate::world::server::GenEntity;
        w.gen_entities
            .iter()
            .filter_map(|e| match e {
                GenEntity::ScatteredItem { stack, .. } | GenEntity::Item { stack, .. } => {
                    Some(stack.id)
                }
                _ => None,
            })
            .collect()
    }

    /// `BlockCactus.onNeighborBlockChange` breaks it with its drop and the
    /// break effect; `BlockReed` and `BlockDoublePlant` drop themselves.
    #[test]
    fn plants_that_lose_their_place_drop_themselves() {
        let mut w = flat();
        w.set_block(0, 63, 0, block::SAND, 0, 2);
        w.set_block(0, 64, 0, block::CACTUS, 0, 2);
        w.gen_entities.clear();
        w.set_block(1, 64, 0, block::STONE, 0, 3);
        assert_eq!(w.block(0, 64, 0), block::AIR);
        assert_eq!(items(&w), vec![block::CACTUS as u16]);
        assert_eq!(w.destroyed.len(), 1, "playAuxSFX(2001)");

        // reeds: the water beside their sand goes
        w.set_block(5, 63, 4, block::WATER, 0, 2);
        w.set_block(4, 64, 4, block::REEDS, 0, 2);
        assert_eq!(w.block(4, 64, 4), block::REEDS);
        w.gen_entities.clear();
        w.set_block(5, 63, 4, block::STONE, 0, 3);
        // not a neighbour of the changed block: it goes on its next tick
        assert_eq!(w.block(4, 64, 4), block::REEDS);
        let mut r = JavaRandom::new(1);
        w.random_tick(block::REEDS, 4, 64, 4, &mut r);
        assert_eq!(w.block(4, 64, 4), block::AIR);
        assert_eq!(items(&w), vec![338]);

        // a double plant whose soil goes: one drop from the lower half
        w.set_block(8, 64, 8, block::DOUBLE_PLANT, 1, 2);
        w.set_block(8, 65, 8, block::DOUBLE_PLANT, 8, 2);
        w.gen_entities.clear();
        w.set_block(8, 63, 8, block::STONE, 0, 3);
        assert_eq!(w.block(8, 64, 8), block::AIR);
        assert_eq!(w.block(8, 65, 8), block::AIR);
        assert_eq!(items(&w), vec![block::DOUBLE_PLANT as u16]);
    }

    /// `BlockCrops.updateTick` goes on after `BlockBush`'s check removed
    /// the crop: the growth roll is still drawn (on what is air by then).
    #[test]
    fn a_dropped_crop_still_draws_its_growth_roll() {
        let mut w = flat();
        w.set_block(0, 64, 0, block::WHEAT, 7, 2);
        let mut r = JavaRandom::new(7);
        let mut expect = JavaRandom::new(7);
        w.gen_entities.clear();
        w.random_tick(block::WHEAT, 0, 64, 0, &mut r);
        assert_eq!(w.block(0, 64, 0), block::AIR);
        let n = items(&w).len();
        assert!(n >= 2, "ripe wheat drops wheat and seeds: {n}");
        // the drops' draws, then getMoisture on dirt (1.0): nextInt(26)
        let _ = crate::drops::drop_block_as_item_with_chance(block::WHEAT, 7, 1.0, 0, &mut expect);
        assert!(w.block_light_value(0, 65, 0, true) >= 9);
        expect.next_int_n(26);
        assert_eq!(r.next_int(), expect.next_int());
    }

    /// A pressed stone button is released by a random tick as by its
    /// scheduled one (`BlockButton.updateTick`).
    #[test]
    fn random_ticks_release_buttons() {
        let mut w = flat();
        let mut r = JavaRandom::new(1);
        w.set_block(0, 64, 0, block::STONE, 0, 2);
        w.set_block(1, 64, 0, block::STONE_BUTTON, 1 | 8, 2);
        assert!(ticks_randomly(block::STONE_BUTTON));
        w.random_tick(block::STONE_BUTTON, 1, 64, 0, &mut r);
        assert_eq!(w.block(1, 64, 0), block::STONE_BUTTON);
        assert_eq!(w.meta(1, 64, 0), 1);
    }

    /// `BlockDynamicLiquid.func_149813_h`: water drops what it washes
    /// away, lava hisses at what it flows into and where it meets water.
    #[test]
    fn water_washes_torches_out_and_lava_hisses() {
        use crate::world::redstone::Effect;
        let mut w = flat();
        w.set_block(2, 64, 0, block::TORCH, 5, 2);
        w.set_block(0, 64, 0, block::FLOWING_WATER, 0, 3);
        w.gen_entities.clear();
        for _ in 0..40 {
            w.total_time += 1;
            w.tick_updates();
        }
        assert_eq!(block::material(w.block(2, 64, 0)), Material::Water);
        assert_eq!(items(&w), vec![block::TORCH as u16]);

        let fizzes = |w: &ServerWorld| {
            w.redstone_effects
                .iter()
                .filter(|e| matches!(e, Effect::Sound("random.fizz", ..)))
                .count()
        };
        let mut w = flat();
        w.set_block(0, 64, 0, block::FLOWING_LAVA, 0, 3);
        for _ in 0..40 {
            w.total_time += 1;
            w.tick_updates();
        }
        assert_eq!(w.block(1, 64, 0), block::FLOWING_LAVA);
        let spread = fizzes(&w);
        assert!(spread >= 4, "lava hisses as it spreads: {spread}");
        // water beside the source: obsidian, and one more hiss
        w.set_block(0, 65, 0, block::WATER, 0, 3);
        assert_eq!(w.block(0, 64, 0), block::OBSIDIAN);
        assert!(fizzes(&w) > spread);
    }

    /// `BlockLeaves.removeLeaves` drops through `dropBlockAsItem`.
    #[test]
    fn decaying_leaves_roll_their_drops() {
        let mut w = flat();
        let mut saplings = 0;
        let mut r = JavaRandom::new(3);
        for i in 0..400 {
            w.set_block(8, 64, 8, block::LEAVES, 8, 2);
            w.gen_entities.clear();
            w.random_tick(block::LEAVES, 8, 64, 8, &mut r);
            assert_eq!(w.block(8, 64, 8), block::AIR, "round {i}");
            saplings += items(&w)
                .iter()
                .filter(|&&i| i == block::SAPLING as u16)
                .count();
        }
        // one in twenty
        assert!((5..60).contains(&saplings), "saplings: {saplings}");
    }
}
