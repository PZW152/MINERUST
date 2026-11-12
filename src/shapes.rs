//! Block bounds that depend on metadata and neighbours: the
//! `setBlockBoundsBasedOnState` / `addCollisionBoxesToList` logic of the
//! non-cube blocks, shared by collision, picking and tessellation.

use crate::block::{self, Material};

/// `[minX, minY, minZ, maxX, maxY, maxZ]` in block space.
pub type Bx = [f32; 6];

/// Neighbour lookup: `(id, meta)` at world coordinates.
pub type Get<'a> = &'a dyn Fn(i32, i32, i32) -> (u8, u8);

const FULL: Bx = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];

fn same_stair(get: Get, x: i32, y: i32, z: i32, meta: u8) -> bool {
    let (id, m) = get(x, y, z);
    block::is_stairs(id) && m == meta
}

/// `BlockStairs.setBaseBounds` + `isInnerStair` + `setStepShapeForCollisions`:
/// the boxes a stair block is made of (rendering and collision use the same).
pub fn stairs(get: Get, x: i32, y: i32, z: i32, meta: u8) -> Vec<Bx> {
    let mut out = Vec::with_capacity(3);
    let top = meta & 4 != 0;
    out.push(if top {
        [0.0, 0.5, 0.0, 1.0, 1.0, 1.0]
    } else {
        [0.0, 0.0, 0.0, 1.0, 0.5, 1.0]
    });
    let (y0, y1) = if top { (0.0, 0.5) } else { (0.5, 1.0) };
    let half = meta & 4;
    let dir = meta & 3;
    let nb = |dx: i32, dz: i32| -> Option<u8> {
        let (id, m) = get(x + dx, y, z + dz);
        if block::is_stairs(id) && m & 4 == half {
            Some(m & 3)
        } else {
            None
        }
    };
    // isInnerStair
    let (mut a, mut b, mut c, mut d) = (0.0f32, 1.0f32, 0.0f32, 0.5f32);
    let mut full_step = true;
    match dir {
        0 => {
            a = 0.5;
            d = 1.0;
            if let Some(o) = nb(1, 0) {
                if o == 3 && !same_stair(get, x, y, z + 1, meta) {
                    d = 0.5;
                    full_step = false;
                } else if o == 2 && !same_stair(get, x, y, z - 1, meta) {
                    c = 0.5;
                    full_step = false;
                }
            }
        }
        1 => {
            b = 0.5;
            d = 1.0;
            if let Some(o) = nb(-1, 0) {
                if o == 3 && !same_stair(get, x, y, z + 1, meta) {
                    d = 0.5;
                    full_step = false;
                } else if o == 2 && !same_stair(get, x, y, z - 1, meta) {
                    c = 0.5;
                    full_step = false;
                }
            }
        }
        2 => {
            c = 0.5;
            d = 1.0;
            if let Some(o) = nb(0, 1) {
                if o == 1 && !same_stair(get, x + 1, y, z, meta) {
                    b = 0.5;
                    full_step = false;
                } else if o == 0 && !same_stair(get, x - 1, y, z, meta) {
                    a = 0.5;
                    full_step = false;
                }
            }
        }
        _ => {
            if let Some(o) = nb(0, -1) {
                if o == 1 && !same_stair(get, x + 1, y, z, meta) {
                    b = 0.5;
                    full_step = false;
                } else if o == 0 && !same_stair(get, x - 1, y, z, meta) {
                    a = 0.5;
                    full_step = false;
                }
            }
        }
    }
    out.push([a, y0, c, b, y1, d]);
    if !full_step {
        return out;
    }
    // setStepShapeForCollisions
    let (mut a, mut b, mut c, mut d) = (0.0f32, 0.5f32, 0.5f32, 1.0f32);
    let mut extra = false;
    match dir {
        0 => {
            if let Some(o) = nb(-1, 0) {
                if o == 3 && !same_stair(get, x, y, z - 1, meta) {
                    c = 0.0;
                    d = 0.5;
                    extra = true;
                } else if o == 2 && !same_stair(get, x, y, z + 1, meta) {
                    c = 0.5;
                    d = 1.0;
                    extra = true;
                }
            }
        }
        1 => {
            if let Some(o) = nb(1, 0) {
                a = 0.5;
                b = 1.0;
                if o == 3 && !same_stair(get, x, y, z - 1, meta) {
                    c = 0.0;
                    d = 0.5;
                    extra = true;
                } else if o == 2 && !same_stair(get, x, y, z + 1, meta) {
                    c = 0.5;
                    d = 1.0;
                    extra = true;
                }
            }
        }
        2 => {
            if let Some(o) = nb(0, -1) {
                c = 0.0;
                d = 0.5;
                if o == 1 && !same_stair(get, x - 1, y, z, meta) {
                    extra = true;
                } else if o == 0 && !same_stair(get, x + 1, y, z, meta) {
                    a = 0.5;
                    b = 1.0;
                    extra = true;
                }
            }
        }
        _ => {
            if let Some(o) = nb(0, 1) {
                if o == 1 && !same_stair(get, x - 1, y, z, meta) {
                    extra = true;
                } else if o == 0 && !same_stair(get, x + 1, y, z, meta) {
                    a = 0.5;
                    b = 1.0;
                    extra = true;
                }
            }
        }
    }
    if extra {
        out.push([a, y0, c, b, y1, d]);
    }
    out
}

/// `BlockFence.canConnectFenceTo`
pub fn fence_connects(id: u8) -> bool {
    let b = block::info(id);
    id == block::FENCE
        || id == block::NETHER_BRICK_FENCE
        || (b.material.is_opaque() && b.normal && b.material != Material::Gourd)
}

fn fence_links(get: Get, x: i32, y: i32, z: i32) -> [bool; 4] {
    [
        fence_connects(get(x, y, z - 1).0),
        fence_connects(get(x, y, z + 1).0),
        fence_connects(get(x - 1, y, z).0),
        fence_connects(get(x + 1, y, z).0),
    ]
}

/// `BlockFence.addCollisionBoxesToList` (1.5 blocks high).
pub fn fence_collision(get: Get, x: i32, y: i32, z: i32) -> Vec<Bx> {
    let [n, s, w, e] = fence_links(get, x, y, z);
    let mut out = Vec::new();
    let (mut f, mut f1, mut f2, mut f3) = (0.375f32, 0.625f32, 0.375f32, 0.625f32);
    if n {
        f2 = 0.0;
    }
    if s {
        f3 = 1.0;
    }
    if n || s {
        out.push([f, 0.0, f2, f1, 1.5, f3]);
    }
    f2 = 0.375;
    f3 = 0.625;
    if w {
        f = 0.0;
    }
    if e {
        f1 = 1.0;
    }
    if w || e || (!n && !s) {
        out.push([f, 0.0, f2, f1, 1.5, f3]);
    }
    out
}

/// `RenderBlocks.renderBlockFence`
pub fn fence_render(get: Get, x: i32, y: i32, z: i32) -> Vec<Bx> {
    let [n, s, w, e] = fence_links(get, x, y, z);
    let mut out = vec![[0.375, 0.0, 0.375, 0.625, 1.0, 0.625]];
    let ew = w || e;
    let ns = n || s;
    let (f, f1) = (0.4375f32, 0.5625f32);
    let x0 = if w { 0.0 } else { f };
    let x1 = if e { 1.0 } else { f1 };
    let z0 = if n { 0.0 } else { f };
    let z1 = if s { 1.0 } else { f1 };
    for (y0, y1) in [(0.75f32, 0.9375f32), (0.375, 0.5625)] {
        if ew {
            out.push([x0, y0, f, x1, y1, f1]);
        }
        if ns {
            out.push([f, y0, z0, f1, y1, z1]);
        }
    }
    out
}

/// `BlockPane.canPaneConnectToBlock`
fn pane_connects(own: u8, id: u8) -> bool {
    block::info(id).full_block || id == own || id == block::GLASS
}

pub fn pane_links(get: Get, own: u8, x: i32, y: i32, z: i32) -> [bool; 4] {
    [
        pane_connects(own, get(x, y, z - 1).0),
        pane_connects(own, get(x, y, z + 1).0),
        pane_connects(own, get(x - 1, y, z).0),
        pane_connects(own, get(x + 1, y, z).0),
    ]
}

/// `BlockPane.addCollisionBoxesToList`
pub fn pane(get: Get, own: u8, x: i32, y: i32, z: i32) -> Vec<Bx> {
    let [n, s, w, e] = pane_links(get, own, x, y, z);
    let any = w || e || n || s;
    let mut out = Vec::new();
    if (!w || !e) && any {
        if w && !e {
            out.push([0.0, 0.0, 0.4375, 0.5, 1.0, 0.5625]);
        } else if !w && e {
            out.push([0.5, 0.0, 0.4375, 1.0, 1.0, 0.5625]);
        }
    } else {
        out.push([0.0, 0.0, 0.4375, 1.0, 1.0, 0.5625]);
    }
    if (!n || !s) && any {
        if n && !s {
            out.push([0.4375, 0.0, 0.0, 0.5625, 1.0, 0.5]);
        } else if !n && s {
            out.push([0.4375, 0.0, 0.5, 0.5625, 1.0, 1.0]);
        }
    } else {
        out.push([0.4375, 0.0, 0.0, 0.5625, 1.0, 1.0]);
    }
    out
}

/// `BlockDoor.getFullMetadata`
pub fn door_full_meta(get: Get, x: i32, y: i32, z: i32, meta: u8) -> u8 {
    let top = meta & 8 != 0;
    let (lower, upper) = if top {
        (get(x, y - 1, z).1, meta)
    } else {
        (meta, get(x, y + 1, z).1)
    };
    let flipped = upper & 1 != 0;
    (lower & 7) | if top { 8 } else { 0 } | if flipped { 16 } else { 0 }
}

/// `BlockDoor.setBoundBasedOnMeta`
pub fn door(full: u8) -> Bx {
    let f = 0.1875f32;
    let open = full & 4 != 0;
    let flip = full & 16 != 0;
    let a = [0.0, 0.0, 0.0, 1.0, 1.0, f];
    let b = [0.0, 0.0, 1.0 - f, 1.0, 1.0, 1.0];
    let c = [0.0, 0.0, 0.0, f, 1.0, 1.0];
    let d = [1.0 - f, 0.0, 0.0, 1.0, 1.0, 1.0];
    match full & 3 {
        0 => {
            if open {
                if !flip {
                    a
                } else {
                    b
                }
            } else {
                c
            }
        }
        1 => {
            if open {
                if !flip {
                    d
                } else {
                    c
                }
            } else {
                a
            }
        }
        2 => {
            if open {
                if !flip {
                    b
                } else {
                    a
                }
            } else {
                d
            }
        }
        _ => {
            if open {
                if !flip {
                    c
                } else {
                    d
                }
            } else {
                b
            }
        }
    }
}

/// `BlockLadder.updateLadderBounds`
pub fn ladder(meta: u8) -> Bx {
    let f = 0.125f32;
    match meta {
        2 => [0.0, 0.0, 1.0 - f, 1.0, 1.0, 1.0],
        3 => [0.0, 0.0, 0.0, 1.0, 1.0, f],
        4 => [1.0 - f, 0.0, 0.0, 1.0, 1.0, 1.0],
        5 => [0.0, 0.0, 0.0, f, 1.0, 1.0],
        _ => FULL,
    }
}

/// `BlockButton.func_150043_b`
fn button(meta: u8) -> Bx {
    let pressed = meta & 8 != 0;
    let (f, f1, f2) = (0.375f32, 0.625f32, 0.1875f32);
    let f3 = if pressed { 0.0625 } else { 0.125 };
    match meta & 7 {
        1 => [0.0, f, 0.5 - f2, f3, f1, 0.5 + f2],
        2 => [1.0 - f3, f, 0.5 - f2, 1.0, f1, 0.5 + f2],
        3 => [0.5 - f2, f, 0.0, 0.5 + f2, f1, f3],
        _ => [0.5 - f2, f, 1.0 - f3, 0.5 + f2, f1, 1.0],
    }
}

/// `BlockLever.setBlockBoundsBasedOnState`
fn lever(meta: u8) -> Bx {
    let f = 0.1875f32;
    match meta & 7 {
        1 => [0.0, 0.2, 0.5 - f, f * 2.0, 0.8, 0.5 + f],
        2 => [1.0 - f * 2.0, 0.2, 0.5 - f, 1.0, 0.8, 0.5 + f],
        3 => [0.5 - f, 0.2, 0.0, 0.5 + f, 0.8, f * 2.0],
        4 => [0.5 - f, 0.2, 1.0 - f * 2.0, 0.5 + f, 0.8, 1.0],
        5 | 6 => [0.25, 0.0, 0.25, 0.75, 0.6, 0.75],
        _ => [0.25, 0.4, 0.25, 0.75, 1.0, 0.75],
    }
}

/// `BlockTripWireHook.setBlockBoundsBasedOnState`
fn tripwire_hook(meta: u8) -> Bx {
    let f = 0.1875f32;
    match meta & 3 {
        3 => [0.5 - f, 0.2, 0.0, 0.5 + f, 0.8, f * 2.0],
        1 => [1.0 - f * 2.0, 0.2, 0.5 - f, 1.0, 0.8, 0.5 + f],
        0 => [0.5 - f, 0.2, 1.0 - f * 2.0, 0.5 + f, 0.8, 1.0],
        _ => [0.0, 0.2, 0.5 - f, f * 2.0, 0.8, 0.5 + f],
    }
}

/// `BlockTrapDoor.func_150117_b`
pub fn trapdoor(meta: u8) -> Bx {
    let f = 0.1875f32;
    if meta & 4 != 0 {
        match meta & 3 {
            0 => [0.0, 0.0, 1.0 - f, 1.0, 1.0, 1.0],
            1 => [0.0, 0.0, 0.0, 1.0, 1.0, f],
            2 => [1.0 - f, 0.0, 0.0, 1.0, 1.0, 1.0],
            _ => [0.0, 0.0, 0.0, f, 1.0, 1.0],
        }
    } else if meta & 8 != 0 {
        [0.0, 1.0 - f, 0.0, 1.0, 1.0, 1.0]
    } else {
        [0.0, 0.0, 0.0, 1.0, f, 1.0]
    }
}

/// `BlockPistonExtension.addCollisionBoxesToList`: the plate and the arm.
pub fn piston_head(meta: u8) -> Vec<Bx> {
    let (a, b) = (0.375f32, 0.625f32);
    match meta & 7 {
        0 => vec![[0.0, 0.0, 0.0, 1.0, 0.25, 1.0], [a, 0.25, a, b, 1.0, b]],
        1 => vec![[0.0, 0.75, 0.0, 1.0, 1.0, 1.0], [a, 0.0, a, b, 0.75, b]],
        2 => vec![
            [0.0, 0.0, 0.0, 1.0, 1.0, 0.25],
            [0.25, a, 0.25, 0.75, b, 1.0],
        ],
        3 => vec![
            [0.0, 0.0, 0.75, 1.0, 1.0, 1.0],
            [0.25, a, 0.0, 0.75, b, 0.75],
        ],
        4 => vec![
            [0.0, 0.0, 0.0, 0.25, 1.0, 1.0],
            [0.25, a, 0.25, 1.0, b, 0.75],
        ],
        _ => vec![
            [0.75, 0.0, 0.0, 1.0, 1.0, 1.0],
            [0.0, a, 0.25, 0.75, b, 0.75],
        ],
    }
}

/// `BlockPortal.setBlockBoundsBasedOnState`: metadata 0 takes the axis
/// from the neighbouring portal blocks.
pub fn portal_axis(get: Get, x: i32, y: i32, z: i32, meta: u8) -> i32 {
    let a = (meta & 3) as i32;
    if a != 0 {
        return a;
    }
    if get(x - 1, y, z).0 != block::PORTAL && get(x + 1, y, z).0 != block::PORTAL {
        2
    } else {
        1
    }
}

/// `BlockPortal` bounds for an axis.
pub fn portal_bounds(axis: i32) -> Bx {
    let (mut a, mut b) = (0.125f32, 0.125f32);
    if axis == 1 {
        a = 0.5;
    }
    if axis == 2 {
        b = 0.5;
    }
    [0.5 - a, 0.0, 0.5 - b, 0.5 + a, 1.0, 0.5 + b]
}

/// Boxes of the blocks whose shape only depends on their own metadata.
pub fn static_boxes(id: u8, meta: u8) -> Vec<Bx> {
    match id {
        block::FARMLAND => vec![[0.0, 0.0, 0.0, 1.0, 0.9375, 1.0]],
        block::STONE_PRESSURE_PLATE | block::WOODEN_PRESSURE_PLATE => {
            let h = if meta > 0 { 0.03125 } else { 0.0625 };
            vec![[0.0625, 0.0, 0.0625, 0.9375, h, 0.9375]]
        }
        block::STONE_BUTTON | block::WOODEN_BUTTON => vec![button(meta)],
        block::LEVER => vec![lever(meta)],
        block::TRIPWIRE_HOOK => vec![tripwire_hook(meta)],
        block::UNPOWERED_REPEATER | block::POWERED_REPEATER => {
            vec![[0.0, 0.0, 0.0, 1.0, 0.125, 1.0]]
        }
        block::END_PORTAL_FRAME => {
            let mut v = vec![[0.0, 0.0, 0.0, 1.0, 0.8125, 1.0]];
            if meta & 4 != 0 {
                v.push([0.3125, 0.8125, 0.3125, 0.6875, 1.0, 0.6875]);
            }
            v
        }
        block::FLOWER_POT => vec![[0.3125, 0.0, 0.3125, 0.6875, 0.375, 0.6875]],
        block::CARPET => vec![[0.0, 0.0, 0.0, 1.0, 0.0625, 1.0]],
        block::CAULDRON => vec![FULL],
        block::TRAPDOOR => vec![trapdoor(meta)],
        // BlockPortal.setBlockBoundsBasedOnState: axis 1 spans x, 2 spans z
        block::PORTAL => vec![portal_bounds(if meta & 3 == 0 {
            2
        } else {
            (meta & 3) as i32
        })],
        block::END_PORTAL => vec![[0.0, 0.0, 0.0, 1.0, 0.75, 1.0]],
        block::DRAGON_EGG => vec![[0.0625, 0.0, 0.0625, 0.9375, 1.0, 0.9375]],
        block::PISTON_HEAD => piston_head(meta),
        _ => vec![FULL],
    }
}

/// `Block.addCollisionBoxesToList` for the block at `(x, y, z)`.
pub fn collision(get: Get, id: u8, meta: u8, x: i32, y: i32, z: i32, out: &mut Vec<Bx>) {
    let b = block::info(id);
    if b.solid {
        out.push(FULL);
        return;
    }
    match b.shape {
        block::Shape::Stairs => out.extend(stairs(get, x, y, z, meta)),
        block::Shape::Fence => out.extend(fence_collision(get, x, y, z)),
        block::Shape::Pane => out.extend(pane(get, id, x, y, z)),
        block::Shape::Door => out.push(door(door_full_meta(get, x, y, z, meta))),
        block::Shape::Ladder => out.push(ladder(meta)),
        _ => match id {
            block::FARMLAND => out.push(FULL),
            block::UNPOWERED_REPEATER | block::POWERED_REPEATER => {
                out.push([0.0, 0.0, 0.0, 1.0, 0.125, 1.0])
            }
            block::PORTAL | block::END_PORTAL => {}
            block::END_PORTAL_FRAME | block::TRAPDOOR | block::PISTON_HEAD | block::DRAGON_EGG => {
                out.extend(static_boxes(id, meta))
            }
            block::FLOWER_POT => out.push([0.3125, 0.0, 0.3125, 0.6875, 0.375, 0.6875]),
            block::CAULDRON => {
                // BlockCauldron.addCollisionBoxesToList
                let f = 0.125f32;
                out.push([0.0, 0.0, 0.0, 1.0, 0.3125, 1.0]);
                out.push([0.0, 0.0, 0.0, f, 1.0, 1.0]);
                out.push([0.0, 0.0, 0.0, 1.0, 1.0, f]);
                out.push([1.0 - f, 0.0, 0.0, 1.0, 1.0, 1.0]);
                out.push([0.0, 0.0, 1.0 - f, 1.0, 1.0, 1.0]);
            }
            _ => {
                if let Some(c) = block::collision_box(id, meta) {
                    out.push(c);
                }
            }
        },
    }
}

fn union(v: &[Bx]) -> Bx {
    let mut u = [f32::MAX, f32::MAX, f32::MAX, f32::MIN, f32::MIN, f32::MIN];
    for b in v {
        for a in 0..3 {
            u[a] = u[a].min(b[a]);
            u[a + 3] = u[a + 3].max(b[a + 3]);
        }
    }
    u
}

/// Selection bounds (`getSelectedBoundingBoxFromPool`), block space.
pub fn selection(get: Get, id: u8, meta: u8, x: i32, y: i32, z: i32) -> Bx {
    if id == block::PORTAL {
        return portal_bounds(portal_axis(get, x, y, z, meta));
    }
    let b = block::info(id);
    match b.shape {
        block::Shape::Stairs => FULL,
        block::Shape::Fence => {
            let [n, s, w, e] = fence_links(get, x, y, z);
            [
                if w { 0.0 } else { 0.375 },
                0.0,
                if n { 0.0 } else { 0.375 },
                if e { 1.0 } else { 0.625 },
                1.0,
                if s { 1.0 } else { 0.625 },
            ]
        }
        block::Shape::Pane => union(&pane(get, id, x, y, z)),
        block::Shape::Door => door(door_full_meta(get, x, y, z, meta)),
        block::Shape::Ladder => ladder(meta),
        block::Shape::Rail => {
            let shape = if id == block::RAIL { meta } else { meta & 7 };
            if (2..=5).contains(&shape) {
                [0.0, 0.0, 0.0, 1.0, 0.625, 1.0]
            } else {
                [0.0, 0.0, 0.0, 1.0, 0.125, 1.0]
            }
        }
        block::Shape::Crops => [0.0, 0.0, 0.0, 1.0, 0.25, 1.0],
        block::Shape::Wire => [0.0, 0.0, 0.0, 1.0, 0.0625, 1.0],
        block::Shape::Tripwire => [0.0, 0.0, 0.0, 1.0, 0.15625, 1.0],
        block::Shape::Boxes => union(&static_boxes(id, meta)),
        block::Shape::Torch => {
            let f = 0.15f32;
            match meta {
                1 => [0.0, 0.2, 0.5 - f, f * 2.0, 0.8, 0.5 + f],
                2 => [1.0 - f * 2.0, 0.2, 0.5 - f, 1.0, 0.8, 0.5 + f],
                3 => [0.5 - f, 0.2, 0.0, 0.5 + f, 0.8, f * 2.0],
                4 => [0.5 - f, 0.2, 1.0 - f * 2.0, 0.5 + f, 0.8, 1.0],
                _ => [0.5 - 0.1, 0.0, 0.5 - 0.1, 0.5 + 0.1, 0.6, 0.5 + 0.1],
            }
        }
        _ => block::collision_box(id, meta).unwrap_or(FULL),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn straight_stairs_have_two_boxes() {
        let get = |x: i32, _y: i32, _z: i32| {
            if x == 0 {
                (block::OAK_STAIRS, 0u8)
            } else {
                (0u8, 0u8)
            }
        };
        let b = stairs(&get, 0, 64, 0, 0);
        assert_eq!(b.len(), 2);
        assert_eq!(b[1], [0.5, 0.5, 0.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn lone_fence_is_a_post() {
        let get = |_x: i32, _y: i32, _z: i32| (0u8, 0u8);
        assert_eq!(
            fence_collision(&get, 0, 64, 0),
            vec![[0.375, 0.0, 0.375, 0.625, 1.5, 0.625]]
        );
    }
}
