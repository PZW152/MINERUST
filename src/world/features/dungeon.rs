//! `WorldGenDungeons`, and the enchanted-book roll shared by every loot chest.

use crate::block;
use crate::java::JavaRandom;
use crate::world::loot;
use crate::world::server::{GenTile, ServerWorld};

/// `EnchantmentHelper.buildEnchantmentList(rand, book, 30)`: the
/// `(id, level)` list a random enchanted book receives.
pub fn roll_enchanted_book(r: &mut JavaRandom) -> Vec<(i32, i32)> {
    crate::enchant::build_list(r, &crate::item::Stack::new(340, 0, 1), 30)
}

pub fn dungeon(w: &mut ServerWorld, r: &mut JavaRandom, x: i32, y: i32, z: i32) -> bool {
    let h = 3;
    let rx = r.next_int_n(2) + 2;
    let rz = r.next_int_n(2) + 2;
    let mut openings = 0;
    for bx in (x - rx - 1)..=(x + rx + 1) {
        for by in (y - 1)..=(y + h + 1) {
            for bz in (z - rz - 1)..=(z + rz + 1) {
                let solid = w.material(bx, by, bz).is_solid();
                if by == y - 1 && !solid {
                    return false;
                }
                if by == y + h + 1 && !solid {
                    return false;
                }
                if (bx == x - rx - 1 || bx == x + rx + 1 || bz == z - rz - 1 || bz == z + rz + 1)
                    && by == y
                    && w.is_air(bx, by, bz)
                    && w.is_air(bx, by + 1, bz)
                {
                    openings += 1;
                }
            }
        }
    }
    if !(1..=5).contains(&openings) {
        return false;
    }
    for bx in (x - rx - 1)..=(x + rx + 1) {
        let mut by = y + h;
        while by >= y - 1 {
            for bz in (z - rz - 1)..=(z + rz + 1) {
                if bx != x - rx - 1
                    && by != y - 1
                    && bz != z - rz - 1
                    && bx != x + rx + 1
                    && by != y + h + 1
                    && bz != z + rz + 1
                {
                    w.set_air(bx, by, bz);
                } else if by >= 0 && !w.material(bx, by - 1, bz).is_solid() {
                    w.set_air(bx, by, bz);
                } else if w.material(bx, by, bz).is_solid() {
                    if by == y - 1 && r.next_int_n(4) != 0 {
                        w.set_block(bx, by, bz, block::MOSSY_COBBLESTONE, 0, 2);
                    } else {
                        w.set_block(bx, by, bz, block::COBBLESTONE, 0, 2);
                    }
                }
            }
            by -= 1;
        }
    }
    for _ in 0..2 {
        for _ in 0..3 {
            let cx = x + r.next_int_n(rx * 2 + 1) - rx;
            let cz = z + r.next_int_n(rz * 2 + 1) - rz;
            if w.is_air(cx, y, cz) {
                let mut walls = 0;
                for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    if w.material(cx + dx, y, cz + dz).is_solid() {
                        walls += 1;
                    }
                }
                if walls == 1 {
                    w.set_block(cx, y, cz, block::CHEST, 0, 2);
                    let book = loot::enchanted_book(r, 1, 1, 1);
                    let mut slots = vec![None; 27];
                    loot::fill(r, &[loot::DUNGEON, &[book]].concat(), &mut slots, 8);
                    w.gen_tiles.push(GenTile::Chest {
                        pos: [cx, y, cz],
                        slots,
                    });
                    break;
                }
            }
        }
    }
    w.set_block(x, y, z, block::MOB_SPAWNER, 0, 2);
    // func_76543_b
    let mob = ["Skeleton", "Zombie", "Zombie", "Spider"][r.next_int_n(4) as usize];
    w.gen_tiles.push(GenTile::Spawner {
        pos: [x, y, z],
        mob,
    });
    true
}
