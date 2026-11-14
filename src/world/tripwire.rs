//! `BlockTripWire` and `BlockTripWireHook` state updates (attachment bits).

use super::server::ServerWorld;
use crate::block::{self, TRIPWIRE, TRIPWIRE_HOOK};

const DX: [i32; 4] = [0, -1, 0, 1];
const DZ: [i32; 4] = [1, 0, -1, 0];
const OPPOSITE: [i32; 4] = [2, 3, 0, 1];

/// `dropBlockAsItem(x, y, z, meta, 0)` then `setBlockToAir`
fn drop(w: &mut ServerWorld, x: i32, y: i32, z: i32, _item: u16) {
    let (b, m) = (w.block(x, y, z), w.meta(x, y, z));
    w.drop_block_with_chance(b, m, x, y, z, 1.0, 0);
    w.set_block(x, y, z, block::AIR, 0, 3);
}

/// `BlockTripWire.onBlockAdded`
pub fn wire_added(w: &mut ServerWorld, x: i32, y: i32, z: i32) {
    let m = if w.solid_top_surface(x, y - 1, z) {
        0
    } else {
        2
    };
    w.set_meta(x, y, z, m, 3);
    update_hooks(w, x, y, z, m);
}

/// `BlockTripWire.updateTripwireHooks`
pub fn update_hooks(w: &mut ServerWorld, x: i32, y: i32, z: i32, meta: i32) {
    for d in 0..2 {
        for k in 1..42 {
            let (hx, hz) = (x + DX[d] * k, z + DZ[d] * k);
            let id = w.block(hx, y, hz);
            if id == TRIPWIRE_HOOK {
                let hm = w.meta(hx, y, hz);
                if hm & 3 == OPPOSITE[d] {
                    hook_state(w, hx, y, hz, false, hm, true, k, meta);
                }
                break;
            }
            if id != TRIPWIRE {
                break;
            }
        }
    }
}

/// `BlockTripWire.onNeighborBlockChange`
pub fn wire_neighbour(w: &mut ServerWorld, x: i32, y: i32, z: i32) {
    let m = w.meta(x, y, z);
    let raised = m & 2 == 2;
    let unsupported = !w.solid_top_surface(x, y - 1, z);
    if raised != unsupported {
        drop(w, x, y, z, crate::item::STRING);
    }
}

/// `BlockTripWireHook.onNeighborBlockChange`
pub fn hook_neighbour(w: &mut ServerWorld, x: i32, y: i32, z: i32, src: u8) {
    if src == TRIPWIRE_HOOK {
        return;
    }
    let nc =
        |w: &mut ServerWorld, x: i32, y: i32, z: i32| block::info(w.block(x, y, z)).normal_cube();
    let any = nc(w, x - 1, y, z) || nc(w, x + 1, y, z) || nc(w, x, y, z - 1) || nc(w, x, y, z + 1);
    if !any {
        drop(w, x, y, z, TRIPWIRE_HOOK as u16);
        return;
    }
    let f = w.meta(x, y, z) & 3;
    let off = (!nc(w, x - 1, y, z) && f == 3)
        || (!nc(w, x + 1, y, z) && f == 1)
        || (!nc(w, x, y, z - 1) && f == 0)
        || (!nc(w, x, y, z + 1) && f == 2);
    if off {
        drop(w, x, y, z, TRIPWIRE_HOOK as u16);
    }
}

/// `BlockTripWireHook.updateState`
#[allow(clippy::too_many_arguments)]
pub fn hook_state(
    w: &mut ServerWorld,
    x: i32,
    y: i32,
    z: i32,
    removed: bool,
    meta: i32,
    notify: bool,
    at: i32,
    at_meta: i32,
) {
    let f = (meta & 3) as usize;
    let was_attached = meta & 4 == 4;
    let was_powered = meta & 8 == 8;
    let mut attached = !removed;
    let mut powered = false;
    let raised = !w.solid_top_surface(x, y - 1, z);
    let (dx, dz) = (DX[f], DZ[f]);
    let mut other = 0;
    let mut wires = [0i32; 42];
    for k in 1..42 {
        let (px, pz) = (x + dx * k, z + dz * k);
        let id = w.block(px, y, pz);
        if id == TRIPWIRE_HOOK {
            let om = w.meta(px, y, pz);
            if om & 3 == OPPOSITE[f] {
                other = k;
            }
            break;
        }
        if id != TRIPWIRE && k != at {
            wires[k as usize] = -1;
            attached = false;
        } else {
            let m = if k == at { at_meta } else { w.meta(px, y, pz) };
            let not_disarmed = m & 8 != 8;
            let triggered = m & 1 == 1;
            let wire_raised = m & 2 == 2;
            attached &= wire_raised == raised;
            powered |= not_disarmed && triggered;
            wires[k as usize] = m;
            if k == at {
                w.schedule_update(x, y, z, TRIPWIRE_HOOK, 10);
                attached &= not_disarmed;
            }
        }
    }
    attached &= other > 1;
    powered &= attached;
    let bits = (if attached { 4 } else { 0 }) | (if powered { 8 } else { 0 });
    let new_meta = f as i32 | bits;
    if other > 0 {
        let (ox, oz) = (x + dx * other, z + dz * other);
        let of = OPPOSITE[f];
        w.set_meta(ox, y, oz, of | bits, 3);
        hook_neighbours(w, ox, y, oz, of);
        click(w, attached, powered, was_attached, was_powered);
    }
    click(w, attached, powered, was_attached, was_powered);
    if !removed {
        w.set_meta(x, y, z, new_meta, 3);
        if notify {
            hook_neighbours(w, x, y, z, f as i32);
        }
    }
    if was_attached != attached {
        for k in 1..other {
            let (px, pz) = (x + dx * k, z + dz * k);
            let mut m = wires[k as usize];
            if m >= 0 {
                if attached {
                    m |= 4;
                } else {
                    m &= !4;
                }
                w.set_meta(px, y, pz, m, 3);
            }
        }
    }
}

/// `playClickSound`: only its `world.rand` use matters here.
fn click(
    w: &mut ServerWorld,
    attached: bool,
    powered: bool,
    was_attached: bool,
    was_powered: bool,
) {
    if !(powered && !was_powered)
        && !(!powered && was_powered)
        && !(attached && !was_attached)
        && (!attached && was_attached)
    {
        w.rand.next_float();
    }
}

/// `BlockTripWireHook.updateNeighbors`
fn hook_neighbours(w: &mut ServerWorld, x: i32, y: i32, z: i32, f: i32) {
    w.notify_neighbours(x, y, z, TRIPWIRE_HOOK);
    match f {
        3 => w.notify_neighbours(x - 1, y, z, TRIPWIRE_HOOK),
        1 => w.notify_neighbours(x + 1, y, z, TRIPWIRE_HOOK),
        0 => w.notify_neighbours(x, y, z - 1, TRIPWIRE_HOOK),
        2 => w.notify_neighbours(x, y, z + 1, TRIPWIRE_HOOK),
        _ => {}
    }
}

/// `BlockTripWireHook.updateTick`
pub fn hook_tick(w: &mut ServerWorld, x: i32, y: i32, z: i32) {
    let m = w.meta(x, y, z);
    hook_state(w, x, y, z, false, m, true, -1, 0);
}
