//! `Block.onEntityCollidedWithBlock` and `Block.onEntityWalking` for the
//! blocks that react to any entity.

use super::base::{Ent, Source};
use crate::block::{self, *};
use crate::world::ServerWorld;

/// `onEntityCollidedWithBlock`. Returns damage for the entity's own
/// `attackEntityFrom`. Pressure plates, tripwire and detector rails are
/// handled by `ServerWorld::redstone_entity_tick`.
pub fn collided(
    w: &mut ServerWorld,
    e: &mut Ent,
    x: i32,
    y: i32,
    z: i32,
    id: u8,
) -> Option<(Source, f32)> {
    match id {
        // BlockWeb: setInWeb
        WEB => {
            e.in_web = true;
            e.fall_distance = 0.0;
            None
        }
        // BlockSoulSand
        SOUL_SAND => {
            e.motion[0] *= 0.4;
            e.motion[2] *= 0.4;
            None
        }
        // BlockCactus
        CACTUS => Some((Source::Cactus, 1.0)),
        // BlockPortal: setInPortal
        PORTAL => {
            if e.riding.is_none() && e.ridden_by.is_none() {
                e.set_in_portal();
            }
            None
        }
        // BlockEndPortal: travelToDimension(1), done by the owner.
        END_PORTAL => {
            if e.riding.is_none() && e.ridden_by.is_none() {
                e.in_end_portal = true;
            }
            None
        }
        // BlockCauldron: a burning entity in the water is put out.
        CAULDRON => {
            let level = w.peek_meta(x, y, z) as i32;
            let top = y as f32 + (6.0 + (3 * level) as f32) / 16.0;
            if e.fire > 0 && level > 0 && e.bb.min[1] <= top as f64 {
                e.fire = 0;
                w.set_meta(x, y, z, (level - 1).clamp(0, 3), 2);
                w.update_neighbors_about_output(x, y, z, CAULDRON);
            }
            None
        }
        _ => None,
    }
}

/// `onEntityWalking`
pub fn walked(w: &mut ServerWorld, x: i32, y: i32, z: i32, id: u8) {
    match id {
        REDSTONE_ORE | LIT_REDSTONE_ORE => w.redstone_ore_light_up(x, y, z),
        // BlockStairs: the model block's
        _ if block::is_stairs(id) => {}
        _ => {}
    }
}
