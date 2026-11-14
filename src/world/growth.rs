//! The random ticks of the farming and plant blocks (`BlockCrops`,
//! `BlockStem`, `BlockSapling`, `BlockNetherWart`, `BlockCocoa`,
//! `BlockFarmland`, `BlockBush.checkAndDropBlock`), the other blocks whose
//! `updateTick` does something on a random tick (lit redstone ore, snow
//! blocks, still lava, the nether portal), and bone meal
//! (`ItemDye.func_150919_a` with each `IGrowable`).

use super::server::{GenEntity, ServerWorld};
use crate::block::{self, Material};
use crate::java::JavaRandom;

/// `World.getBlockLightValue` (with the neighbour rule for half blocks).
fn light(w: &mut ServerWorld, x: i32, y: i32, z: i32) -> i32 {
    w.block_light_value(x, y, z, true)
}

/// `MathHelper.getRandomIntegerInRange(rand, min, max)`
fn int_in_range(r: &mut JavaRandom, min: i32, max: i32) -> i32 {
    if min >= max {
        min
    } else {
        r.next_int_n(max - min + 1) + min
    }
}

impl ServerWorld {
    /// `Block.dropBlockAsItem(world, x, y, z, meta, 0)` drawing from `r`
    /// (the world's random, lent out while a random tick runs).
    pub fn drop_block_with(
        &mut self,
        r: &mut JavaRandom,
        id: u8,
        meta: i32,
        x: i32,
        y: i32,
        z: i32,
    ) {
        let v = crate::drops::drop_block_as_item_with_chance(id, meta, 1.0, 0, r);
        self.spawn_drops(v, x, y, z);
    }

    /// `BlockBush.canPlaceBlockOn` for the bushes added here.
    fn bush_soil(id: u8, below: u8) -> bool {
        match id {
            block::WHEAT
            | block::CARROTS
            | block::POTATOES
            | block::PUMPKIN_STEM
            | block::MELON_STEM => below == block::FARMLAND,
            block::NETHER_WART => below == block::SOUL_SAND,
            _ => below == block::GRASS || below == block::DIRT || below == block::FARMLAND,
        }
    }

    /// `BlockBush.canBlockStay` (crops, stems, saplings, nether wart and
    /// the bushes `can_block_stay` already knows).
    pub fn bush_can_stay(&mut self, id: u8, x: i32, y: i32, z: i32) -> bool {
        match id {
            block::WHEAT
            | block::CARROTS
            | block::POTATOES
            | block::PUMPKIN_STEM
            | block::MELON_STEM
            | block::NETHER_WART
            | block::SAPLING => {
                let below = self.block(x, y - 1, z);
                Self::bush_soil(id, below)
            }
            _ => self.can_block_stay(id, x, y, z),
        }
    }

    /// `BlockBush.checkAndDropBlock`
    pub fn check_and_drop(&mut self, r: &mut JavaRandom, id: u8, x: i32, y: i32, z: i32) -> bool {
        if self.bush_can_stay(id, x, y, z) {
            return false;
        }
        let meta = self.meta(x, y, z);
        self.drop_block_with(r, id, meta, x, y, z);
        self.set_block(x, y, z, block::AIR, 0, 2);
        true
    }

    /// `BlockCrops.getMoisture` / `BlockStem.getGrowthChance` (the same code).
    fn growth_chance(&mut self, id: u8, x: i32, y: i32, z: i32) -> f32 {
        let mut f = 1.0f32;
        let n = self.block(x, y, z - 1);
        let s = self.block(x, y, z + 1);
        let wst = self.block(x - 1, y, z);
        let e = self.block(x + 1, y, z);
        let nw = self.block(x - 1, y, z - 1);
        let ne = self.block(x + 1, y, z - 1);
        let se = self.block(x + 1, y, z + 1);
        let sw = self.block(x - 1, y, z + 1);
        let ew = wst == id || e == id;
        let ns = n == id || s == id;
        let diag = nw == id || ne == id || se == id || sw == id;
        for xx in x - 1..=x + 1 {
            for zz in z - 1..=z + 1 {
                let mut k = 0.0f32;
                if self.block(xx, y - 1, zz) == block::FARMLAND {
                    k = 1.0;
                    if self.meta(xx, y - 1, zz) > 0 {
                        k = 3.0;
                    }
                }
                if xx != x || zz != z {
                    k /= 4.0;
                }
                f += k;
            }
        }
        if diag || ew && ns {
            f /= 2.0;
        }
        f
    }

    /// `BlockStem.crop`
    fn stem_fruit(id: u8) -> u8 {
        if id == block::PUMPKIN_STEM {
            block::PUMPKIN
        } else {
            block::MELON_BLOCK
        }
    }

    /// `Block.updateTick` of the plants and the other randomly ticking
    /// blocks this module covers; false when `id` is none of them.
    pub fn growth_tick(&mut self, id: u8, x: i32, y: i32, z: i32, r: &mut JavaRandom) -> bool {
        match id {
            block::WHEAT | block::CARROTS | block::POTATOES => {
                // BlockCrops.updateTick
                if self.check_and_drop(r, id, x, y, z) {
                    return true;
                }
                if light(self, x, y + 1, z) >= 9 {
                    let m = self.meta(x, y, z);
                    if m < 7 {
                        let f = self.growth_chance(id, x, y, z);
                        if r.next_int_n((25.0f32 / f) as i32 + 1) == 0 {
                            self.set_meta(x, y, z, m + 1, 2);
                        }
                    }
                }
            }
            block::PUMPKIN_STEM | block::MELON_STEM => {
                // BlockStem.updateTick
                if self.check_and_drop(r, id, x, y, z) {
                    return true;
                }
                if light(self, x, y + 1, z) >= 9 {
                    let f = self.growth_chance(id, x, y, z);
                    if r.next_int_n((25.0f32 / f) as i32 + 1) == 0 {
                        let m = self.meta(x, y, z);
                        let fruit = Self::stem_fruit(id);
                        if m < 7 {
                            self.set_meta(x, y, z, m + 1, 2);
                        } else {
                            for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                                if self.block(x + dx, y, z + dz) == fruit {
                                    return true;
                                }
                            }
                            let (dx, dz) =
                                [(-1, 0), (1, 0), (0, -1), (0, 1)][r.next_int_n(4) as usize];
                            let (tx, tz) = (x + dx, z + dz);
                            let below = self.block(tx, y - 1, tz);
                            if self.material(tx, y, tz) == Material::Air
                                && (below == block::FARMLAND
                                    || below == block::DIRT
                                    || below == block::GRASS)
                            {
                                self.set_block(tx, y, tz, fruit, 0, 3);
                            }
                        }
                    }
                }
            }
            block::SAPLING => {
                // BlockSapling.updateTick
                if self.check_and_drop(r, id, x, y, z) {
                    return true;
                }
                if light(self, x, y + 1, z) >= 9 && r.next_int_n(7) == 0 {
                    self.mark_or_grow_sapling(x, y, z, r);
                }
            }
            block::NETHER_WART => {
                // BlockNetherWart.updateTick: grows first, then BlockBush's check
                let m = self.meta(x, y, z);
                if m < 3 && r.next_int_n(10) == 0 {
                    self.set_meta(x, y, z, m + 1, 2);
                }
                self.check_and_drop(r, id, x, y, z);
            }
            block::COCOA => {
                // BlockCocoa.updateTick
                let m = self.meta(x, y, z);
                if !self.cocoa_can_stay(x, y, z) {
                    self.drop_block_with(r, id, m, x, y, z);
                    self.set_block(x, y, z, block::AIR, 0, 2);
                } else if r.next_int_n(5) == 0 {
                    let stage = (m & 12) >> 2;
                    if stage < 2 {
                        self.set_meta(x, y, z, (stage + 1) << 2 | m & 3, 2);
                    }
                }
            }
            block::FARMLAND => {
                // BlockFarmland.updateTick
                if !self.irrigated(x, y, z) && !self.can_lightning_strike_at(x, y + 1, z) {
                    let m = self.meta(x, y, z);
                    if m > 0 {
                        self.set_meta(x, y, z, m - 1, 2);
                    } else if !matches!(
                        self.block(x, y + 1, z),
                        block::WHEAT
                            | block::MELON_STEM
                            | block::PUMPKIN_STEM
                            | block::POTATOES
                            | block::CARROTS
                    ) {
                        self.set_block(x, y, z, block::DIRT, 0, 3);
                    }
                } else {
                    self.set_meta(x, y, z, 7, 2);
                }
            }
            block::YELLOW_FLOWER
            | block::RED_FLOWER
            | block::TALLGRASS
            | block::DEADBUSH
            | block::WATERLILY => {
                // BlockBush.updateTick
                self.check_and_drop(r, id, x, y, z);
            }
            block::LIT_REDSTONE_ORE => {
                // BlockRedstoneOre.updateTick
                self.set_block(x, y, z, block::REDSTONE_ORE, 0, 3);
            }
            block::SNOW => {
                // BlockSnowBlock.updateTick
                if self.saved_light(false, x, y, z) > 11 {
                    let m = self.meta(x, y, z);
                    self.drop_block_with(r, id, m, x, y, z);
                    self.set_air(x, y, z);
                }
            }
            block::LAVA => self.still_lava_tick(x, y, z, r),
            block::PORTAL => {
                // BlockPortal.updateTick (BlockBreakable has no updateTick)
                if self.dim == 0 && r.next_int_n(2000) < self.difficulty {
                    let mut yy = y;
                    while !self.solid_top_surface(x, yy, z) && yy > 0 {
                        yy -= 1;
                    }
                    if yy > 0 && !block::info(self.block(x, yy + 1, z)).normal_cube() {
                        // ItemMonsterPlacer.spawnCreature(world, 57, ...)
                        self.gen_entities.push(GenEntity::Mob {
                            kind: crate::entity::mob::Kind::PigZombie,
                            pos: [x as f64 + 0.5, yy as f64 + 1.1, z as f64 + 0.5],
                            variant: 0,
                        });
                    }
                }
            }
            _ => return false,
        }
        true
    }

    /// `BlockCocoa.canBlockStay`: a jungle log on the side it faces.
    pub fn cocoa_can_stay(&mut self, x: i32, y: i32, z: i32) -> bool {
        let d = (self.meta(x, y, z) & 3) as usize;
        let (tx, tz) = (
            x + crate::world::features::trees::DIR_X[d],
            z + crate::world::features::trees::DIR_Z[d],
        );
        self.block(tx, y, tz) == block::LOG && self.meta(tx, y, tz) & 3 == 3
    }

    /// `BlockFarmland.isIrrigated` (`func_149779_m`)
    fn irrigated(&mut self, x: i32, y: i32, z: i32) -> bool {
        for xx in x - 4..=x + 4 {
            for yy in y..=y + 1 {
                for zz in z - 4..=z + 4 {
                    if self.material(xx, yy, zz) == Material::Water {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// `BlockStaticLiquid.updateTick` for still lava: fire nearby.
    fn still_lava_tick(&mut self, mut x: i32, mut y: i32, mut z: i32, r: &mut JavaRandom) {
        let n = r.next_int_n(3);
        let burn = |w: &mut ServerWorld, x: i32, y: i32, z: i32| w.material(x, y, z).can_burn();
        for _ in 0..n {
            x += r.next_int_n(3) - 1;
            y += 1;
            z += r.next_int_n(3) - 1;
            let m = self.material(x, y, z);
            if m == Material::Air {
                if burn(self, x - 1, y, z)
                    || burn(self, x + 1, y, z)
                    || burn(self, x, y, z - 1)
                    || burn(self, x, y, z + 1)
                    || burn(self, x, y - 1, z)
                    || burn(self, x, y + 1, z)
                {
                    self.set_block(x, y, z, block::FIRE, 0, 3);
                    return;
                }
            } else if m.blocks_movement() {
                return;
            }
        }
        if n == 0 {
            let (x0, z0) = (x, z);
            for _ in 0..3 {
                x = x0 + r.next_int_n(3) - 1;
                z = z0 + r.next_int_n(3) - 1;
                if self.is_air(x, y + 1, z) && burn(self, x, y, z) {
                    self.set_block(x, y + 1, z, block::FIRE, 0, 3);
                }
            }
        }
    }

    /// `BlockSapling.markOrGrowMarked`
    pub fn mark_or_grow_sapling(&mut self, x: i32, y: i32, z: i32, r: &mut JavaRandom) {
        let m = self.meta(x, y, z);
        if m & 8 == 0 {
            self.set_meta(x, y, z, m | 8, 4);
        } else {
            self.grow_sapling(x, y, z, r);
        }
    }

    fn is_sapling(&mut self, x: i32, y: i32, z: i32, kind: i32) -> bool {
        self.block(x, y, z) == block::SAPLING && self.meta(x, y, z) & 7 == kind
    }

    /// The 2x2 of saplings of `kind` that `(x, z)` is part of, as
    /// `growTree`'s labelled loops find it.
    fn sapling_square(&mut self, x: i32, y: i32, z: i32, kind: i32) -> Option<(i32, i32)> {
        for i in [0, -1] {
            for j in [0, -1] {
                if self.is_sapling(x + i, y, z + j, kind)
                    && self.is_sapling(x + i + 1, y, z + j, kind)
                    && self.is_sapling(x + i, y, z + j + 1, kind)
                    && self.is_sapling(x + i + 1, y, z + j + 1, kind)
                {
                    return Some((i, j));
                }
            }
        }
        None
    }

    /// `BlockSapling.growTree`
    pub fn grow_sapling(&mut self, x: i32, y: i32, z: i32, r: &mut JavaRandom) {
        use crate::world::features::{bigtrees, misc, trees};
        #[derive(Clone, Copy)]
        enum Gen {
            Big,
            Trees(i32, i32, i32),
            MegaPine(bool),
            Taiga2,
            Birch,
            MegaJungle,
            Savanna,
            Canopy,
        }
        let kind = self.meta(x, y, z) & 7;
        let mut g = if r.next_int_n(10) == 0 {
            Gen::Big
        } else {
            Gen::Trees(4, 0, 0)
        };
        let mut off = (0, 0);
        let mut square = false;
        match kind {
            1 => match self.sapling_square(x, y, z, 1) {
                Some(o) => {
                    off = o;
                    square = true;
                    g = Gen::MegaPine(r.next_boolean());
                }
                None => g = Gen::Taiga2,
            },
            2 => g = Gen::Birch,
            3 => match self.sapling_square(x, y, z, 3) {
                Some(o) => {
                    off = o;
                    square = true;
                    g = Gen::MegaJungle;
                }
                None => g = Gen::Trees(4 + r.next_int_n(7), 3, 3),
            },
            4 => g = Gen::Savanna,
            5 => match self.sapling_square(x, y, z, 5) {
                Some(o) => {
                    off = o;
                    square = true;
                    g = Gen::Canopy;
                }
                None => return,
            },
            _ => {}
        }
        let (i, j) = off;
        let cells: Vec<(i32, i32)> = if square {
            vec![(i, j), (i + 1, j), (i, j + 1), (i + 1, j + 1)]
        } else {
            vec![(0, 0)]
        };
        let meta = self.meta(x, y, z);
        for &(a, b) in &cells {
            self.set_block(x + a, y, z + b, block::AIR, 0, 4);
        }
        // WorldGenAbstractTree(true): setBlockAndNotifyAdequately uses flag 3,
        // except WorldGenMegaPineTree, which the sapling makes with false
        self.gen_notify = !matches!(g, Gen::MegaPine(_));
        let (gx, gz) = (x + i, z + j);
        let ok = match g {
            Gen::Big => bigtrees::big_tree(self, r, gx, y, gz, &mut 0),
            Gen::Trees(h, wood, leaf) => trees::trees(self, r, gx, y, gz, h, wood, leaf, false),
            Gen::MegaPine(b) => bigtrees::mega_pine(self, r, gx, y, gz, b),
            Gen::Taiga2 => trees::taiga2(self, r, gx, y, gz),
            Gen::Birch => trees::birch(self, r, gx, y, gz, false),
            Gen::MegaJungle => bigtrees::mega_jungle(self, r, gx, y, gz, 10, 20, 3, 3),
            Gen::Savanna => trees::savanna(self, r, gx, y, gz),
            Gen::Canopy => misc::canopy(self, r, gx, y, gz),
        };
        self.gen_notify = false;
        if !ok {
            for &(a, b) in &cells {
                self.set_block(x + a, y, z + b, block::SAPLING, meta & 7, 4);
            }
        }
    }

    /// `ItemDye.func_150919_a` (bone meal on a block): the `IGrowable`'s
    /// `canFertilize`, then on the server `shouldFertilize` and `fertilize`
    /// with the world's random. True when the bone meal was used.
    pub fn bone_meal(&mut self, x: i32, y: i32, z: i32) -> bool {
        let id = self.block(x, y, z);
        let m = self.meta(x, y, z);
        let can = match id {
            block::SAPLING | block::GRASS | block::BROWN_MUSHROOM | block::RED_MUSHROOM => true,
            block::WHEAT
            | block::CARROTS
            | block::POTATOES
            | block::PUMPKIN_STEM
            | block::MELON_STEM => m != 7,
            block::COCOA => (m & 12) >> 2 < 2,
            block::TALLGRASS => m != 0,
            block::DOUBLE_PLANT => {
                let v = self.double_plant_variant(x, y, z);
                v != 2 && v != 3
            }
            _ => return false,
        };
        if !can {
            return false;
        }
        let mut r = std::mem::replace(&mut self.rand, JavaRandom::new(0));
        let should = match id {
            block::SAPLING => r.next_float() < 0.45,
            block::BROWN_MUSHROOM | block::RED_MUSHROOM => r.next_float() < 0.4,
            _ => true,
        };
        if should {
            match id {
                block::SAPLING => self.mark_or_grow_sapling(x, y, z, &mut r),
                block::WHEAT
                | block::CARROTS
                | block::POTATOES
                | block::PUMPKIN_STEM
                | block::MELON_STEM => {
                    // BlockCrops.func_149863_m / BlockStem.func_149874_m
                    let v = (m + int_in_range(&mut r, 2, 5)).min(7);
                    self.set_meta(x, y, z, v, 2);
                }
                block::COCOA => {
                    let stage = (m & 12) >> 2;
                    self.set_meta(x, y, z, (stage + 1) << 2 | m & 3, 2);
                }
                block::TALLGRASS => {
                    // BlockTallGrass.func_149853_b: a double grass or fern
                    let v = if m == 2 { 3 } else { 2 };
                    if self.can_place_block_at(block::DOUBLE_PLANT, x, y, z) {
                        self.set_block(x, y, z, block::DOUBLE_PLANT, v, 2);
                        self.set_block(x, y + 1, z, block::DOUBLE_PLANT, 8, 2);
                    }
                }
                block::DOUBLE_PLANT => {
                    // BlockDoublePlant.func_149853_b: one more of itself
                    let v = self.double_plant_variant(x, y, z);
                    let stack = crate::item::Stack::new(block::DOUBLE_PLANT as u16, v, 1);
                    let v = crate::drops::drop_stack(&mut r, stack);
                    self.spawn_drops(v, x, y, z);
                }
                block::BROWN_MUSHROOM | block::RED_MUSHROOM => {
                    // BlockMushroom.func_149884_c
                    self.set_air(x, y, z);
                    self.gen_notify = true;
                    let kind = if id == block::BROWN_MUSHROOM { 0 } else { 1 };
                    let ok =
                        crate::world::features::misc::big_mushroom(self, &mut r, x, y, z, kind);
                    self.gen_notify = false;
                    if !ok {
                        self.set_block(x, y, z, id, m, 3);
                    }
                }
                block::GRASS => self.grass_bone_meal(x, y, z, &mut r),
                _ => {}
            }
        }
        self.rand = r;
        true
    }

    /// `BlockDoublePlant.func_149885_e`: the variant from the lower half.
    fn double_plant_variant(&mut self, x: i32, y: i32, z: i32) -> i32 {
        let m = self.meta(x, y, z);
        if m & 8 != 0 {
            self.meta(x, y - 1, z) & 7
        } else {
            m & 7
        }
    }

    /// `BlockGrass.func_149853_b`: tall grass and flowers around.
    fn grass_bone_meal(&mut self, x: i32, y: i32, z: i32, r: &mut JavaRandom) {
        'outer: for i in 0..128 {
            let (mut bx, mut by, mut bz) = (x, y + 1, z);
            for _ in 0..i / 16 {
                bx += r.next_int_n(3) - 1;
                by += (r.next_int_n(3) - 1) * r.next_int_n(3) / 2;
                bz += r.next_int_n(3) - 1;
                if self.block(bx, by - 1, bz) != block::GRASS
                    || block::info(self.block(bx, by, bz)).normal_cube()
                {
                    continue 'outer;
                }
            }
            if self.material(bx, by, bz) == Material::Air {
                if r.next_int_n(8) != 0 {
                    if self.can_block_stay(block::TALLGRASS, bx, by, bz) {
                        self.set_block(bx, by, bz, block::TALLGRASS, 1, 3);
                    }
                } else {
                    let biome = self.biome(bx, bz).id as u8;
                    let (fid, fm) = crate::world::populate::pick_flower(self, biome, r, bx, bz);
                    if self.can_block_stay(fid, bx, by, bz) {
                        self.set_block(bx, by, bz, fid, fm, 3);
                    }
                }
            }
        }
    }
}
