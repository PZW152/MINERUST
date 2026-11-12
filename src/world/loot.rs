//! `WeightedRandomChestContent` and the chest tables of 1.7.10.

use crate::item::{self, ItemId, Stack};
use crate::java::JavaRandom;

#[derive(Clone, Copy, Debug)]
pub struct Loot {
    pub item: ItemId,
    pub meta: i32,
    pub min: i32,
    pub max: i32,
    pub weight: i32,
    /// Packed enchantments for enchanted books (`Stack::ench`).
    pub ench: u64,
}

pub const fn l(item: ItemId, meta: i32, min: i32, max: i32, weight: i32) -> Loot {
    Loot {
        item,
        meta,
        min,
        max,
        weight,
        ench: 0,
    }
}

/// `Items.enchanted_book.getRandom(rand, min, max, weight)`
pub fn enchanted_book(r: &mut JavaRandom, min: i32, max: i32, weight: i32) -> Loot {
    let list = super::features::dungeon::roll_enchanted_book(r);
    let s = Stack::new(403, 0, 1).with_enchantments(&list);
    Loot {
        item: 403,
        meta: 0,
        min,
        max,
        weight,
        ench: s.ench,
    }
}

/// `WeightedRandomChestContent.generateChestContents` (and the identical
/// dispenser variant): `count` weighted draws into random slots.
pub fn fill(r: &mut JavaRandom, table: &[Loot], slots: &mut [Option<Stack>], count: i32) {
    let total: i32 = table.iter().map(|e| e.weight).sum();
    if total <= 0 {
        return;
    }
    for _ in 0..count {
        let mut n = r.next_int_n(total);
        let mut e = table[0];
        for t in table {
            n -= t.weight;
            if n < 0 {
                e = *t;
                break;
            }
        }
        let k = e.min + r.next_int_n(e.max - e.min + 1);
        let mut st = Stack::new(e.item, e.meta, 1);
        st.ench = e.ench;
        if item::max_stack(e.item) as i32 >= k {
            st.count = k as u32;
            let slot = r.next_int_n(slots.len() as i32) as usize;
            slots[slot] = Some(st);
        } else {
            for _ in 0..k {
                let slot = r.next_int_n(slots.len() as i32) as usize;
                slots[slot] = Some(st);
            }
        }
    }
}

/// `WorldGenDungeons.field_111189_a`
pub const DUNGEON: &[Loot] = &[
    l(329, 0, 1, 1, 10),
    l(265, 0, 1, 4, 10),
    l(297, 0, 1, 1, 10),
    l(296, 0, 1, 4, 10),
    l(289, 0, 1, 4, 10),
    l(287, 0, 1, 4, 10),
    l(325, 0, 1, 1, 10),
    l(322, 0, 1, 1, 1),
    l(331, 0, 1, 4, 10),
    l(2256, 0, 1, 1, 10),
    l(2257, 0, 1, 1, 10),
    l(421, 0, 1, 1, 10),
    l(418, 0, 1, 1, 2),
    l(417, 0, 1, 1, 5),
    l(419, 0, 1, 1, 1),
];

/// `StructureMineshaftPieces.mineshaftChestContents`
pub const MINESHAFT: &[Loot] = &[
    l(265, 0, 1, 5, 10),
    l(266, 0, 1, 3, 5),
    l(331, 0, 4, 9, 5),
    l(351, 4, 4, 9, 5),
    l(264, 0, 1, 2, 3),
    l(263, 0, 3, 8, 10),
    l(297, 0, 1, 3, 15),
    l(257, 0, 1, 1, 1),
    l(66, 0, 4, 8, 1),
    l(362, 0, 2, 4, 10),
    l(361, 0, 2, 4, 10),
    l(329, 0, 1, 1, 3),
    l(417, 0, 1, 1, 1),
];

/// `StructureVillagePieces$House2.villageBlacksmithChestContents`
pub const BLACKSMITH: &[Loot] = &[
    l(264, 0, 1, 3, 3),
    l(265, 0, 1, 5, 10),
    l(266, 0, 1, 3, 5),
    l(297, 0, 1, 3, 15),
    l(260, 0, 1, 3, 15),
    l(257, 0, 1, 1, 5),
    l(267, 0, 1, 1, 5),
    l(307, 0, 1, 1, 5),
    l(306, 0, 1, 1, 5),
    l(308, 0, 1, 1, 5),
    l(309, 0, 1, 1, 5),
    l(49, 0, 3, 7, 5),
    l(6, 0, 3, 7, 5),
    l(329, 0, 1, 1, 3),
    l(417, 0, 1, 1, 1),
    l(418, 0, 1, 1, 1),
    l(419, 0, 1, 1, 1),
];

/// `ComponentScatteredFeaturePieces$DesertPyramid.itemsToGenerateInTemple`
/// (the jungle temple chest table is identical).
pub const TEMPLE: &[Loot] = &[
    l(264, 0, 1, 3, 3),
    l(265, 0, 1, 5, 10),
    l(266, 0, 2, 7, 15),
    l(388, 0, 1, 3, 2),
    l(352, 0, 4, 6, 20),
    l(367, 0, 3, 7, 16),
    l(329, 0, 1, 1, 3),
    l(417, 0, 1, 1, 1),
    l(418, 0, 1, 1, 1),
    l(419, 0, 1, 1, 1),
];

/// `ComponentScatteredFeaturePieces$JunglePyramid.junglePyramidsDispenserContents`
pub const JUNGLE_DISPENSER: &[Loot] = &[l(262, 0, 2, 7, 30)];

/// `StructureStrongholdPieces$ChestCorridor.strongholdChestContents`
pub const STRONGHOLD_CORRIDOR: &[Loot] = &[
    l(368, 0, 1, 1, 10),
    l(264, 0, 1, 3, 3),
    l(265, 0, 1, 5, 10),
    l(266, 0, 1, 3, 5),
    l(331, 0, 4, 9, 5),
    l(297, 0, 1, 3, 15),
    l(260, 0, 1, 3, 15),
    l(257, 0, 1, 1, 5),
    l(267, 0, 1, 1, 5),
    l(307, 0, 1, 1, 5),
    l(306, 0, 1, 1, 5),
    l(308, 0, 1, 1, 5),
    l(309, 0, 1, 1, 5),
    l(322, 0, 1, 1, 1),
    l(329, 0, 1, 1, 1),
    l(417, 0, 1, 1, 1),
    l(418, 0, 1, 1, 1),
    l(419, 0, 1, 1, 1),
];

/// `StructureStrongholdPieces$Library.strongholdLibraryChestContents`
pub const STRONGHOLD_LIBRARY: &[Loot] = &[
    l(340, 0, 1, 3, 20),
    l(339, 0, 2, 7, 20),
    l(395, 0, 1, 1, 1),
    l(345, 0, 1, 1, 1),
];

/// `StructureStrongholdPieces$RoomCrossing.strongholdRoomCrossingChestContents`
pub const STRONGHOLD_CROSSING: &[Loot] = &[
    l(265, 0, 1, 5, 10),
    l(266, 0, 1, 3, 5),
    l(331, 0, 4, 9, 5),
    l(263, 0, 3, 8, 10),
    l(297, 0, 1, 3, 15),
    l(260, 0, 1, 3, 15),
    l(257, 0, 1, 1, 1),
];

/// `table` followed by one extra entry (`WeightedRandomChestContent.addAll`).
pub fn with(table: &[Loot], extra: Loot) -> Vec<Loot> {
    let mut v = table.to_vec();
    v.push(extra);
    v
}
