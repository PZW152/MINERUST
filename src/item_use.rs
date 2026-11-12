//! `Item.getItemUseAction`, `getMaxItemUseDuration` and the `ItemFood`
//! values of `Item.registerItems`.

use crate::item::ItemId;
use crate::potion;

/// `EnumAction`
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    None,
    Eat,
    Drink,
    Block,
    Bow,
}

/// One `ItemFood`.
#[derive(Clone, Copy, Debug)]
pub struct Food {
    pub heal: i32,
    pub saturation: f32,
    pub wolf_meat: bool,
    pub always_edible: bool,
    /// `setPotionEffect(id, seconds, amplifier, probability)`
    pub effect: Option<(i32, i32, i32, f32)>,
}

const fn f(heal: i32, saturation: f32, wolf_meat: bool) -> Food {
    Food {
        heal,
        saturation,
        wolf_meat,
        always_edible: false,
        effect: None,
    }
}

/// `ItemFood` (and `ItemFishFood`'s `FishType`) by item and damage.
pub fn food(id: ItemId, meta: i32) -> Option<Food> {
    Some(match id {
        260 => f(4, 0.3, false),
        282 => f(6, 0.6, false),
        297 => f(5, 0.6, false),
        319 => f(3, 0.3, true),
        320 => f(8, 0.8, true),
        322 => Food {
            always_edible: true,
            effect: Some((potion::REGENERATION, 5, 1, 1.0)),
            ..f(4, 1.2, false)
        },
        // FishType: cod, salmon, clownfish, pufferfish
        349 => match meta {
            1 => f(2, 0.1, false),
            2 => f(1, 0.1, false),
            3 => f(1, 0.1, false),
            _ => f(2, 0.1, false),
        },
        350 => match meta {
            1 => f(6, 0.8, false),
            // clownfish and pufferfish cannot be cooked: the raw values
            2 | 3 => f(1, 0.1, false),
            _ => f(5, 0.6, false),
        },
        357 => f(2, 0.1, false),
        360 => f(2, 0.3, false),
        363 => f(3, 0.3, true),
        364 => f(8, 0.8, true),
        365 => Food {
            effect: Some((potion::HUNGER, 30, 0, 0.3)),
            ..f(2, 0.3, true)
        },
        366 => f(6, 0.6, true),
        367 => Food {
            effect: Some((potion::HUNGER, 30, 0, 0.8)),
            ..f(4, 0.1, true)
        },
        375 => Food {
            effect: Some((potion::POISON, 5, 0, 1.0)),
            ..f(2, 0.8, false)
        },
        391 => f(4, 0.6, false),
        392 => f(1, 0.3, false),
        393 => f(6, 0.6, false),
        394 => Food {
            effect: Some((potion::POISON, 5, 0, 0.6)),
            ..f(2, 0.3, false)
        },
        396 => f(6, 1.2, false),
        400 => f(8, 0.3, false),
        _ => return None,
    })
}

/// `ItemPotion.isSplash`
pub fn is_splash(meta: i32) -> bool {
    meta & 16384 != 0
}

/// `Item.getItemUseAction`
pub fn action(id: ItemId, meta: i32) -> Action {
    if food(id, meta).is_some() {
        return Action::Eat;
    }
    match id {
        // ItemPotion, ItemBucketMilk
        373 | 335 => Action::Drink,
        // ItemSword
        267 | 268 | 272 | 276 | 283 => Action::Block,
        261 => Action::Bow,
        _ => Action::None,
    }
}

/// `Item.getMaxItemUseDuration`
pub fn max_duration(id: ItemId, meta: i32) -> i32 {
    match action(id, meta) {
        Action::Eat | Action::Drink => 32,
        Action::Block | Action::Bow => 72000,
        Action::None => 0,
    }
}
