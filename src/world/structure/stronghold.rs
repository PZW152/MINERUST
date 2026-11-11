//! `MapGenStronghold` and `StructureStrongholdPieces`.

use super::{find_biome_position, find_intersecting, Kind, Piece, Start, BB};
use crate::block::{self, *};
use crate::java::JavaRandom;
use crate::world::gen::chunk_provider::ChunkProvider;
use crate::world::loot;
use crate::world::server::ServerWorld;

/// `Stronghold$Door`
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Door {
    Opening,
    Wood,
    Grates,
    Iron,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HKind {
    Stairs {
        source: bool,
    },
    Straight {
        left: bool,
        right: bool,
    },
    ChestCorridor {
        chest: bool,
    },
    Corridor {
        steps: i32,
    },
    Crossing {
        left_low: bool,
        left_high: bool,
        right_low: bool,
        right_high: bool,
    },
    LeftTurn,
    Library {
        tall: bool,
    },
    PortalRoom {
        spawner: bool,
    },
    Prison,
    RoomCrossing {
        room: i32,
    },
    StairsStraight,
}

#[derive(Clone, Copy, Debug)]
pub struct Hold {
    pub door: Door,
    pub k: HKind,
}

impl Hold {
    pub fn save(&self, out: &mut Vec<i32>) {
        out.push(self.door as i32);
        match self.k {
            HKind::Stairs { source } => out.extend_from_slice(&[0, source as i32]),
            HKind::Straight { left, right } => {
                out.extend_from_slice(&[1, left as i32, right as i32])
            }
            HKind::ChestCorridor { chest } => out.extend_from_slice(&[2, chest as i32]),
            HKind::Corridor { steps } => out.extend_from_slice(&[3, steps]),
            HKind::Crossing {
                left_low,
                left_high,
                right_low,
                right_high,
            } => out.extend_from_slice(&[
                4,
                left_low as i32,
                left_high as i32,
                right_low as i32,
                right_high as i32,
            ]),
            HKind::LeftTurn => out.push(5),
            HKind::Library { tall } => out.extend_from_slice(&[6, tall as i32]),
            HKind::PortalRoom { spawner } => out.extend_from_slice(&[7, spawner as i32]),
            HKind::Prison => out.push(8),
            HKind::RoomCrossing { room } => out.extend_from_slice(&[9, room]),
            HKind::StairsStraight => out.push(10),
        }
    }
    pub fn load(v: &mut std::slice::Iter<i32>) -> Option<Hold> {
        let mut t = || v.next().copied();
        let door = match t()? {
            1 => Door::Wood,
            2 => Door::Grates,
            3 => Door::Iron,
            _ => Door::Opening,
        };
        let k = match t()? {
            0 => HKind::Stairs { source: t()? != 0 },
            1 => HKind::Straight {
                left: t()? != 0,
                right: t()? != 0,
            },
            2 => HKind::ChestCorridor { chest: t()? != 0 },
            3 => HKind::Corridor { steps: t()? },
            4 => HKind::Crossing {
                left_low: t()? != 0,
                left_high: t()? != 0,
                right_low: t()? != 0,
                right_high: t()? != 0,
            },
            5 => HKind::LeftTurn,
            6 => HKind::Library { tall: t()? != 0 },
            7 => HKind::PortalRoom { spawner: t()? != 0 },
            8 => HKind::Prison,
            9 => HKind::RoomCrossing { room: t()? },
            _ => HKind::StairsStraight,
        };
        Some(Hold { door, k })
    }
}

/// `MapGenStronghold.canSpawnStructureAtCoords`'s one-time coordinate search.
pub fn find_coords(provider: &ChunkProvider) -> Vec<(i32, i32)> {
    let allowed: Vec<i32> = (0..256)
        .filter(|&i| crate::world::biome::get(i).map_or(false, |b| b.root_height > 0.0))
        .map(|i| crate::world::biome::get(i).unwrap().id as i32)
        .collect();
    let mut r = JavaRandom::new(provider.seed);
    let mut angle = r.next_double() * std::f64::consts::PI * 2.0;
    let mut ring = 1;
    let mut spread = 3;
    let mut out = Vec::new();
    for i in 0..3 {
        let dist = (1.25 * ring as f64 + r.next_double()) * (32.0 * ring as f64);
        let mut x = (angle.cos() * dist + 0.5).floor() as i64 as i32;
        let mut z = (angle.sin() * dist + 0.5).floor() as i64 as i32;
        if let Some((px, pz)) =
            find_biome_position(provider, (x << 4) + 8, (z << 4) + 8, 112, &allowed, &mut r)
        {
            x = px >> 4;
            z = pz >> 4;
        }
        out.push((x, z));
        angle += std::f64::consts::PI * 2.0 * ring as f64 / spread as f64;
        if i == spread {
            ring += 2 + r.next_int_n(5);
            spread = spread + 1 + r.next_int_n(2);
        }
    }
    out
}

/// Piece classes of `pieceWeightArray`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class {
    Straight,
    Prison,
    LeftTurn,
    RightTurn,
    RoomCrossing,
    StairsStraight,
    Stairs,
    Crossing,
    ChestCorridor,
    Library,
    PortalRoom,
}

#[derive(Clone, Copy, Debug)]
struct Weight {
    class: Class,
    weight: i32,
    spawned: i32,
    limit: i32,
}

impl Weight {
    fn can_spawn_type(&self, ty: i32) -> bool {
        let base = self.limit == 0 || self.spawned < self.limit;
        match self.class {
            Class::Library => base && ty > 4,
            Class::PortalRoom => base && ty > 5,
            _ => base,
        }
    }
    fn can_spawn_more(&self) -> bool {
        self.limit == 0 || self.spawned < self.limit
    }
}

struct Build {
    weights: Vec<Weight>,
    forced: Option<Class>,
    last: Option<Class>,
    pending: Vec<usize>,
    start_bb: BB,
    portal: bool,
}

fn hp(door: Door, k: HKind) -> Kind {
    Kind::Hold(Hold { door, k })
}

/// `getRandomDoor`
fn random_door(r: &mut JavaRandom) -> Door {
    match r.next_int_n(5) {
        2 => Door::Wood,
        3 => Door::Grates,
        4 => Door::Iron,
        _ => Door::Opening,
    }
}

/// `MapGenStronghold.getStructureStart`: rebuilt until it has a portal room.
pub fn start(r: &mut JavaRandom, cx: i32, cz: i32) -> Start {
    loop {
        let (s, portal) = try_start(r, cx, cz);
        if !s.pieces.is_empty() && portal {
            return s;
        }
    }
}

/// `MapGenStronghold$Start`
fn try_start(r: &mut JavaRandom, cx: i32, cz: i32) -> (Start, bool) {
    let weights = [
        (Class::Straight, 40, 0),
        (Class::Prison, 5, 5),
        (Class::LeftTurn, 20, 0),
        (Class::RightTurn, 20, 0),
        (Class::RoomCrossing, 10, 6),
        (Class::StairsStraight, 5, 5),
        (Class::Stairs, 5, 5),
        (Class::Crossing, 5, 4),
        (Class::ChestCorridor, 5, 4),
        (Class::Library, 10, 2),
        (Class::PortalRoom, 20, 1),
    ]
    .iter()
    .map(|&(class, weight, limit)| Weight {
        class,
        weight,
        spawned: 0,
        limit,
    })
    .collect();
    let (x, z) = ((cx << 4) + 2, (cz << 4) + 2);
    let mode = r.next_int_n(4);
    let bb = BB::new(x, 64, z, x + 5 - 1, 74, z + 5 - 1);
    let first = Piece::new(
        0,
        mode,
        bb,
        hp(Door::Opening, HKind::Stairs { source: true }),
    );
    let mut list = vec![first];
    let mut b = Build {
        weights,
        forced: None,
        last: None,
        pending: Vec::new(),
        start_bb: bb,
        portal: false,
    };
    build(&mut list, 0, &mut b, r);
    while !b.pending.is_empty() {
        let i = r.next_int_n(b.pending.len() as i32) as usize;
        let p = b.pending.remove(i);
        build(&mut list, p, &mut b, r);
    }
    let mut s = Start {
        cx,
        cz,
        bb: BB::default(),
        pieces: list,
        sizeable: true,
    };
    s.update_bb();
    s.mark_available_height(r, 10);
    (s, b.portal)
}

/// `getStrongholdComponentFromWeightedPiece`
#[allow(clippy::too_many_arguments)]
fn from_class(
    class: Class,
    list: &[Piece],
    r: &mut JavaRandom,
    x: i32,
    y: i32,
    z: i32,
    mode: i32,
    ty: i32,
) -> Option<Piece> {
    let ok = |bb: &BB| bb.y0 > 10 && find_intersecting(list, bb).is_none();
    let c =
        |ox: i32, oy: i32, w: i32, h: i32, d: i32| BB::component(x, y, z, ox, oy, 0, w, h, d, mode);
    Some(match class {
        Class::Straight => {
            let bb = c(-1, -1, 5, 5, 7);
            if !ok(&bb) {
                return None;
            }
            let door = random_door(r);
            let left = r.next_int_n(2) == 0;
            let right = r.next_int_n(2) == 0;
            Piece::new(ty, mode, bb, hp(door, HKind::Straight { left, right }))
        }
        Class::Prison => {
            let bb = c(-1, -1, 9, 5, 11);
            if !ok(&bb) {
                return None;
            }
            Piece::new(ty, mode, bb, hp(random_door(r), HKind::Prison))
        }
        // RightTurn has no findValidPlacement of its own: the inherited static
        // LeftTurn.findValidPlacement builds a LeftTurn.
        Class::LeftTurn | Class::RightTurn => {
            let bb = c(-1, -1, 5, 5, 5);
            if !ok(&bb) {
                return None;
            }
            Piece::new(ty, mode, bb, hp(random_door(r), HKind::LeftTurn))
        }
        Class::RoomCrossing => {
            let bb = c(-4, -1, 11, 7, 11);
            if !ok(&bb) {
                return None;
            }
            let door = random_door(r);
            Piece::new(
                ty,
                mode,
                bb,
                hp(
                    door,
                    HKind::RoomCrossing {
                        room: r.next_int_n(5),
                    },
                ),
            )
        }
        Class::StairsStraight => {
            let bb = c(-1, -7, 5, 11, 8);
            if !ok(&bb) {
                return None;
            }
            Piece::new(ty, mode, bb, hp(random_door(r), HKind::StairsStraight))
        }
        Class::Stairs => {
            let bb = c(-1, -7, 5, 11, 5);
            if !ok(&bb) {
                return None;
            }
            Piece::new(
                ty,
                mode,
                bb,
                hp(random_door(r), HKind::Stairs { source: false }),
            )
        }
        Class::Crossing => {
            let bb = c(-4, -3, 10, 9, 11);
            if !ok(&bb) {
                return None;
            }
            let door = random_door(r);
            let left_low = r.next_boolean();
            let left_high = r.next_boolean();
            let right_low = r.next_boolean();
            let right_high = r.next_int_n(3) > 0;
            Piece::new(
                ty,
                mode,
                bb,
                hp(
                    door,
                    HKind::Crossing {
                        left_low,
                        left_high,
                        right_low,
                        right_high,
                    },
                ),
            )
        }
        Class::ChestCorridor => {
            let bb = c(-1, -1, 5, 5, 7);
            if !ok(&bb) {
                return None;
            }
            Piece::new(
                ty,
                mode,
                bb,
                hp(random_door(r), HKind::ChestCorridor { chest: false }),
            )
        }
        Class::Library => {
            let mut bb = c(-4, -1, 14, 11, 15);
            if !ok(&bb) {
                bb = c(-4, -1, 14, 6, 15);
                if !ok(&bb) {
                    return None;
                }
            }
            let door = random_door(r);
            Piece::new(ty, mode, bb, hp(door, HKind::Library { tall: bb.ys() > 6 }))
        }
        Class::PortalRoom => {
            let bb = c(-4, -1, 11, 8, 16);
            if !ok(&bb) {
                return None;
            }
            Piece::new(
                ty,
                mode,
                bb,
                hp(Door::Opening, HKind::PortalRoom { spawner: false }),
            )
        }
    })
}

/// `getNextComponent`
#[allow(clippy::too_many_arguments)]
fn next_component(
    list: &[Piece],
    b: &mut Build,
    r: &mut JavaRandom,
    x: i32,
    y: i32,
    z: i32,
    mode: i32,
    ty: i32,
) -> Option<Piece> {
    let any = b.weights.iter().any(|w| w.limit > 0 && w.spawned < w.limit);
    let total: i32 = b.weights.iter().map(|w| w.weight).sum();
    if !any {
        return None;
    }
    if let Some(class) = b.forced.take() {
        if let Some(p) = from_class(class, list, r, x, y, z, mode, ty) {
            return Some(p);
        }
    }
    for _ in 0..5 {
        let mut k = r.next_int_n(total);
        let mut i = 0;
        while i < b.weights.len() {
            let pw = b.weights[i];
            k -= pw.weight;
            if k < 0 {
                if !pw.can_spawn_type(ty) || Some(pw.class) == b.last {
                    break;
                }
                if let Some(p) = from_class(pw.class, list, r, x, y, z, mode, ty) {
                    b.weights[i].spawned += 1;
                    b.last = Some(pw.class);
                    if !b.weights[i].can_spawn_more() {
                        b.weights.remove(i);
                    }
                    return Some(p);
                }
            }
            i += 1;
        }
    }
    // Corridor.findSize
    let bb = BB::component(x, y, z, -1, -1, 0, 5, 5, 4, mode);
    let hit = find_intersecting(list, &bb)?;
    let hb = hit.bb;
    if hb.y0 == bb.y0 {
        for n in (1..=3).rev() {
            let shorter = BB::component(x, y, z, -1, -1, 0, 5, 5, n - 1, mode);
            if !hb.intersects(&shorter) {
                let cb = BB::component(x, y, z, -1, -1, 0, 5, 5, n, mode);
                if cb.y0 > 1 {
                    let steps = if mode != 2 && mode != 0 {
                        cb.xs()
                    } else {
                        cb.zs()
                    };
                    return Some(Piece::new(
                        ty,
                        mode,
                        cb,
                        hp(Door::Opening, HKind::Corridor { steps }),
                    ));
                }
                return None;
            }
        }
    }
    None
}

/// `getNextValidComponent`
#[allow(clippy::too_many_arguments)]
fn next(
    list: &mut Vec<Piece>,
    b: &mut Build,
    r: &mut JavaRandom,
    x: i32,
    y: i32,
    z: i32,
    mode: i32,
    ty: i32,
) {
    if ty > 50 {
        return;
    }
    if (x - b.start_bb.x0).abs() > 112 || (z - b.start_bb.z0).abs() > 112 {
        return;
    }
    if let Some(p) = next_component(list, b, r, x, y, z, mode, ty + 1) {
        list.push(p);
        b.pending.push(list.len() - 1);
    }
}

/// `getNextComponentNormal`
fn normal(list: &mut Vec<Piece>, i: usize, b: &mut Build, r: &mut JavaRandom, a: i32, dy: i32) {
    let (bb, m, ty) = (list[i].bb, list[i].mode, list[i].ty);
    match m {
        0 => next(list, b, r, bb.x0 + a, bb.y0 + dy, bb.z1 + 1, m, ty),
        1 => next(list, b, r, bb.x0 - 1, bb.y0 + dy, bb.z0 + a, m, ty),
        2 => next(list, b, r, bb.x0 + a, bb.y0 + dy, bb.z0 - 1, m, ty),
        3 => next(list, b, r, bb.x1 + 1, bb.y0 + dy, bb.z0 + a, m, ty),
        _ => {}
    }
}

/// `getNextComponentX`
fn side_x(list: &mut Vec<Piece>, i: usize, b: &mut Build, r: &mut JavaRandom, dy: i32, a: i32) {
    let (bb, m, ty) = (list[i].bb, list[i].mode, list[i].ty);
    match m {
        0 | 2 => next(list, b, r, bb.x0 - 1, bb.y0 + dy, bb.z0 + a, 1, ty),
        1 | 3 => next(list, b, r, bb.x0 + a, bb.y0 + dy, bb.z0 - 1, 2, ty),
        _ => {}
    }
}

/// `getNextComponentZ`
fn side_z(list: &mut Vec<Piece>, i: usize, b: &mut Build, r: &mut JavaRandom, dy: i32, a: i32) {
    let (bb, m, ty) = (list[i].bb, list[i].mode, list[i].ty);
    match m {
        0 | 2 => next(list, b, r, bb.x1 + 1, bb.y0 + dy, bb.z0 + a, 3, ty),
        1 | 3 => next(list, b, r, bb.x0 + a, bb.y0 + dy, bb.z1 + 1, 0, ty),
        _ => {}
    }
}

/// `buildComponent`
fn build(list: &mut Vec<Piece>, i: usize, b: &mut Build, r: &mut JavaRandom) {
    let Kind::Hold(h) = list[i].kind else { return };
    let mode = list[i].mode;
    match h.k {
        HKind::Stairs { source } => {
            if source {
                b.forced = Some(Class::Crossing);
            }
            normal(list, i, b, r, 1, 1);
        }
        HKind::Straight { left, right } => {
            normal(list, i, b, r, 1, 1);
            if left {
                side_x(list, i, b, r, 1, 2);
            }
            if right {
                side_z(list, i, b, r, 1, 2);
            }
        }
        HKind::ChestCorridor { .. } | HKind::Prison | HKind::StairsStraight => {
            normal(list, i, b, r, 1, 1)
        }
        HKind::Crossing {
            left_low,
            left_high,
            right_low,
            right_high,
        } => {
            let (mut a, mut c) = (3, 5);
            if mode == 1 || mode == 2 {
                a = 8 - a;
                c = 8 - c;
            }
            normal(list, i, b, r, 5, 1);
            if left_low {
                side_x(list, i, b, r, a, 1);
            }
            if left_high {
                side_x(list, i, b, r, c, 7);
            }
            if right_low {
                side_z(list, i, b, r, a, 1);
            }
            if right_high {
                side_z(list, i, b, r, c, 7);
            }
        }
        HKind::LeftTurn => {
            if mode != 2 && mode != 3 {
                side_z(list, i, b, r, 1, 1);
            } else {
                side_x(list, i, b, r, 1, 1);
            }
        }
        HKind::RoomCrossing { .. } => {
            normal(list, i, b, r, 4, 1);
            side_x(list, i, b, r, 1, 4);
            side_z(list, i, b, r, 1, 4);
        }
        HKind::PortalRoom { .. } => b.portal = true,
        HKind::Library { .. } | HKind::Corridor { .. } => {}
    }
}

// ------------------------------------------------------------------ placement

/// `StructureStrongholdPieces$Stones`
fn stones(r: &mut JavaRandom, edge: bool) -> (u8, i32) {
    if edge {
        let f = r.next_float();
        if f < 0.2 {
            (STONEBRICK, 2)
        } else if f < 0.5 {
            (STONEBRICK, 1)
        } else if f < 0.55 {
            (MONSTER_EGG, 2)
        } else {
            (STONEBRICK, 0)
        }
    } else {
        (AIR, 0)
    }
}

/// `Stronghold.placeDoor`
fn place_door(q: &Piece, w: &mut ServerWorld, clip: &BB, door: Door, x: i32, y: i32, z: i32) {
    match door {
        Door::Opening => q.fill(w, clip, x, y, z, x + 3 - 1, y + 3 - 1, z, AIR, AIR, false),
        Door::Wood | Door::Iron => {
            for (dx, dy) in [(0, 0), (0, 1), (0, 2), (1, 2), (2, 2), (2, 1), (2, 0)] {
                q.pb(w, STONEBRICK, 0, x + dx, y + dy, z, clip);
            }
            let id = if door == Door::Wood {
                WOODEN_DOOR
            } else {
                IRON_DOOR
            };
            q.pb(w, id, 0, x + 1, y, z, clip);
            q.pb(w, id, 8, x + 1, y + 1, z, clip);
            if door == Door::Iron {
                q.pb(
                    w,
                    STONE_BUTTON,
                    q.mo(STONE_BUTTON, 4),
                    x + 2,
                    y + 1,
                    z + 1,
                    clip,
                );
                q.pb(
                    w,
                    STONE_BUTTON,
                    q.mo(STONE_BUTTON, 3),
                    x + 2,
                    y + 1,
                    z - 1,
                    clip,
                );
            }
        }
        Door::Grates => {
            q.pb(w, AIR, 0, x + 1, y, z, clip);
            q.pb(w, AIR, 0, x + 1, y + 1, z, clip);
            for (dx, dy) in [(0, 0), (0, 1), (0, 2), (1, 2), (2, 2), (2, 1), (2, 0)] {
                q.pb(w, IRON_BARS, 0, x + dx, y + dy, z, clip);
            }
        }
    }
}

pub fn add_parts(p: &mut Piece, w: &mut ServerWorld, r: &mut JavaRandom, clip: &BB) -> bool {
    let Kind::Hold(h) = p.kind else { return true };
    let q = p.clone();
    let rs =
        |w: &mut ServerWorld,
         r: &mut JavaRandom,
         x0: i32,
         y0: i32,
         z0: i32,
         x1: i32,
         y1: i32,
         z1: i32,
         keep: bool| q.fill_random(w, clip, x0, y0, z0, x1, y1, z1, keep, r, &stones);
    if !matches!(h.k, HKind::PortalRoom { .. }) && q.liquid_in(w, clip) {
        return false;
    }
    match h.k {
        HKind::Stairs { .. } => {
            rs(w, r, 0, 0, 0, 4, 10, 4, true);
            place_door(&q, w, clip, h.door, 1, 7, 0);
            place_door(&q, w, clip, Door::Opening, 1, 1, 4);
            for (id, x, y, z) in [
                (STONEBRICK, 2, 6, 1),
                (STONEBRICK, 1, 5, 1),
                (STONE_SLAB, 1, 6, 1),
                (STONEBRICK, 1, 5, 2),
                (STONEBRICK, 1, 4, 3),
                (STONE_SLAB, 1, 5, 3),
                (STONEBRICK, 2, 4, 3),
                (STONEBRICK, 3, 3, 3),
                (STONE_SLAB, 3, 4, 3),
                (STONEBRICK, 3, 3, 2),
                (STONEBRICK, 3, 2, 1),
                (STONE_SLAB, 3, 3, 1),
                (STONEBRICK, 2, 2, 1),
                (STONEBRICK, 1, 1, 1),
                (STONE_SLAB, 1, 2, 1),
                (STONEBRICK, 1, 1, 2),
                (STONE_SLAB, 1, 1, 3),
            ] {
                q.pb(w, id, 0, x, y, z, clip);
            }
        }
        HKind::Straight { left, right } => {
            rs(w, r, 0, 0, 0, 4, 4, 6, true);
            place_door(&q, w, clip, h.door, 1, 1, 0);
            place_door(&q, w, clip, Door::Opening, 1, 1, 6);
            q.maybe(w, clip, r, 0.1, 1, 2, 1, TORCH, 0);
            q.maybe(w, clip, r, 0.1, 3, 2, 1, TORCH, 0);
            q.maybe(w, clip, r, 0.1, 1, 2, 5, TORCH, 0);
            q.maybe(w, clip, r, 0.1, 3, 2, 5, TORCH, 0);
            if left {
                q.fill(w, clip, 0, 1, 2, 0, 3, 4, AIR, AIR, false);
            }
            if right {
                q.fill(w, clip, 4, 1, 2, 4, 3, 4, AIR, AIR, false);
            }
        }
        HKind::ChestCorridor { chest } => {
            rs(w, r, 0, 0, 0, 4, 4, 6, true);
            place_door(&q, w, clip, h.door, 1, 1, 0);
            place_door(&q, w, clip, Door::Opening, 1, 1, 6);
            q.fill(w, clip, 3, 1, 2, 3, 1, 4, STONEBRICK, STONEBRICK, false);
            q.pb(w, STONE_SLAB, 5, 3, 1, 1, clip);
            q.pb(w, STONE_SLAB, 5, 3, 1, 5, clip);
            q.pb(w, STONE_SLAB, 5, 3, 2, 2, clip);
            q.pb(w, STONE_SLAB, 5, 3, 2, 4, clip);
            for z in 2..=4 {
                q.pb(w, STONE_SLAB, 5, 2, 1, z, clip);
            }
            let mut made = chest;
            if !made {
                let (y, x, z) = (q.y_off(2), q.x_off(3, 3), q.z_off(3, 3));
                if clip.inside(x, y, z) {
                    made = true;
                    let book = loot::enchanted_book(r, 1, 1, 1);
                    let table = loot::with(loot::STRONGHOLD_CORRIDOR, book);
                    let n = 2 + r.next_int_n(2);
                    q.chest(w, clip, r, 3, 2, 3, &table, n);
                }
            }
            p.kind = hp(h.door, HKind::ChestCorridor { chest: made });
        }
        HKind::Corridor { steps } => {
            for z in 0..steps {
                for x in 0..=4 {
                    q.pb(w, STONEBRICK, 0, x, 0, z, clip);
                }
                for y in 1..=3 {
                    q.pb(w, STONEBRICK, 0, 0, y, z, clip);
                    q.pb(w, AIR, 0, 1, y, z, clip);
                    q.pb(w, AIR, 0, 2, y, z, clip);
                    q.pb(w, AIR, 0, 3, y, z, clip);
                    q.pb(w, STONEBRICK, 0, 4, y, z, clip);
                }
                for x in 0..=4 {
                    q.pb(w, STONEBRICK, 0, x, 4, z, clip);
                }
            }
        }
        HKind::Crossing {
            left_low,
            left_high,
            right_low,
            right_high,
        } => {
            rs(w, r, 0, 0, 0, 9, 8, 10, true);
            place_door(&q, w, clip, h.door, 4, 3, 0);
            if left_low {
                q.fill(w, clip, 0, 3, 1, 0, 5, 3, AIR, AIR, false);
            }
            if right_low {
                q.fill(w, clip, 9, 3, 1, 9, 5, 3, AIR, AIR, false);
            }
            if left_high {
                q.fill(w, clip, 0, 5, 7, 0, 7, 9, AIR, AIR, false);
            }
            if right_high {
                q.fill(w, clip, 9, 5, 7, 9, 7, 9, AIR, AIR, false);
            }
            q.fill(w, clip, 5, 1, 10, 7, 3, 10, AIR, AIR, false);
            rs(w, r, 1, 2, 1, 8, 2, 6, false);
            rs(w, r, 4, 1, 5, 4, 4, 9, false);
            rs(w, r, 8, 1, 5, 8, 4, 9, false);
            rs(w, r, 1, 4, 7, 3, 4, 9, false);
            rs(w, r, 1, 3, 5, 3, 3, 6, false);
            q.fill(w, clip, 1, 3, 4, 3, 3, 4, STONE_SLAB, STONE_SLAB, false);
            q.fill(w, clip, 1, 4, 6, 3, 4, 6, STONE_SLAB, STONE_SLAB, false);
            rs(w, r, 5, 1, 7, 7, 1, 8, false);
            q.fill(w, clip, 5, 1, 9, 7, 1, 9, STONE_SLAB, STONE_SLAB, false);
            q.fill(w, clip, 5, 2, 7, 7, 2, 7, STONE_SLAB, STONE_SLAB, false);
            q.fill(w, clip, 4, 5, 7, 4, 5, 9, STONE_SLAB, STONE_SLAB, false);
            q.fill(w, clip, 8, 5, 7, 8, 5, 9, STONE_SLAB, STONE_SLAB, false);
            q.fill(
                w,
                clip,
                5,
                5,
                7,
                7,
                5,
                9,
                DOUBLE_STONE_SLAB,
                DOUBLE_STONE_SLAB,
                false,
            );
            q.pb(w, TORCH, 0, 6, 5, 6, clip);
        }
        HKind::LeftTurn => {
            rs(w, r, 0, 0, 0, 4, 4, 4, true);
            place_door(&q, w, clip, h.door, 1, 1, 0);
            if q.mode != 2 && q.mode != 3 {
                q.fill(w, clip, 4, 1, 1, 4, 3, 3, AIR, AIR, false);
            } else {
                q.fill(w, clip, 0, 1, 1, 0, 3, 3, AIR, AIR, false);
            }
        }
        HKind::Library { tall } => {
            let top = if tall { 11 } else { 6 };
            rs(w, r, 0, 0, 0, 13, top - 1, 14, true);
            place_door(&q, w, clip, h.door, 4, 1, 0);
            q.fill_chance(w, clip, r, 0.07, 2, 1, 1, 11, 4, 13, WEB, WEB, false);
            for z in 1..=13 {
                if (z - 1) % 4 == 0 {
                    q.fill(w, clip, 1, 1, z, 1, 4, z, PLANKS, PLANKS, false);
                    q.fill(w, clip, 12, 1, z, 12, 4, z, PLANKS, PLANKS, false);
                    q.pb(w, TORCH, 0, 2, 3, z, clip);
                    q.pb(w, TORCH, 0, 11, 3, z, clip);
                    if tall {
                        q.fill(w, clip, 1, 6, z, 1, 9, z, PLANKS, PLANKS, false);
                        q.fill(w, clip, 12, 6, z, 12, 9, z, PLANKS, PLANKS, false);
                    }
                } else {
                    q.fill(w, clip, 1, 1, z, 1, 4, z, BOOKSHELF, BOOKSHELF, false);
                    q.fill(w, clip, 12, 1, z, 12, 4, z, BOOKSHELF, BOOKSHELF, false);
                    if tall {
                        q.fill(w, clip, 1, 6, z, 1, 9, z, BOOKSHELF, BOOKSHELF, false);
                        q.fill(w, clip, 12, 6, z, 12, 9, z, BOOKSHELF, BOOKSHELF, false);
                    }
                }
            }
            let mut z = 3;
            while z < 12 {
                q.fill(w, clip, 3, 1, z, 4, 3, z, BOOKSHELF, BOOKSHELF, false);
                q.fill(w, clip, 6, 1, z, 7, 3, z, BOOKSHELF, BOOKSHELF, false);
                q.fill(w, clip, 9, 1, z, 10, 3, z, BOOKSHELF, BOOKSHELF, false);
                z += 2;
            }
            if tall {
                q.fill(w, clip, 1, 5, 1, 3, 5, 13, PLANKS, PLANKS, false);
                q.fill(w, clip, 10, 5, 1, 12, 5, 13, PLANKS, PLANKS, false);
                q.fill(w, clip, 4, 5, 1, 9, 5, 2, PLANKS, PLANKS, false);
                q.fill(w, clip, 4, 5, 12, 9, 5, 13, PLANKS, PLANKS, false);
                q.pb(w, PLANKS, 0, 9, 5, 11, clip);
                q.pb(w, PLANKS, 0, 8, 5, 11, clip);
                q.pb(w, PLANKS, 0, 9, 5, 10, clip);
                q.fill(w, clip, 3, 6, 2, 3, 6, 12, FENCE, FENCE, false);
                q.fill(w, clip, 10, 6, 2, 10, 6, 10, FENCE, FENCE, false);
                q.fill(w, clip, 4, 6, 2, 9, 6, 2, FENCE, FENCE, false);
                q.fill(w, clip, 4, 6, 12, 8, 6, 12, FENCE, FENCE, false);
                q.pb(w, FENCE, 0, 9, 6, 11, clip);
                q.pb(w, FENCE, 0, 8, 6, 11, clip);
                q.pb(w, FENCE, 0, 9, 6, 10, clip);
                let l = q.mo(LADDER, 3);
                for y in 1..=7 {
                    q.pb(w, LADDER, l, 10, y, 13, clip);
                }
                let (cx, cz) = (7, 7);
                for (x, y, z) in [
                    (cx - 1, 9, cz),
                    (cx, 9, cz),
                    (cx - 1, 8, cz),
                    (cx, 8, cz),
                    (cx - 1, 7, cz),
                    (cx, 7, cz),
                    (cx - 2, 7, cz),
                    (cx + 1, 7, cz),
                    (cx - 1, 7, cz - 1),
                    (cx - 1, 7, cz + 1),
                    (cx, 7, cz - 1),
                    (cx, 7, cz + 1),
                ] {
                    q.pb(w, FENCE, 0, x, y, z, clip);
                }
                for (x, y, z) in [
                    (cx - 2, 8, cz),
                    (cx + 1, 8, cz),
                    (cx - 1, 8, cz - 1),
                    (cx - 1, 8, cz + 1),
                    (cx, 8, cz - 1),
                    (cx, 8, cz + 1),
                ] {
                    q.pb(w, TORCH, 0, x, y, z, clip);
                }
            }
            let book = loot::enchanted_book(r, 1, 5, 2);
            let table = loot::with(loot::STRONGHOLD_LIBRARY, book);
            let n = 1 + r.next_int_n(4);
            q.chest(w, clip, r, 3, 3, 5, &table, n);
            if tall {
                q.pb(w, AIR, 0, 12, 9, 1, clip);
                let book = loot::enchanted_book(r, 1, 5, 2);
                let table = loot::with(loot::STRONGHOLD_LIBRARY, book);
                let n = 1 + r.next_int_n(4);
                q.chest(w, clip, r, 12, 8, 1, &table, n);
            }
        }
        HKind::PortalRoom { spawner } => {
            rs(w, r, 0, 0, 0, 10, 7, 15, false);
            place_door(&q, w, clip, Door::Grates, 4, 1, 0);
            let y = 6;
            rs(w, r, 1, y, 1, 1, y, 14, false);
            rs(w, r, 9, y, 1, 9, y, 14, false);
            rs(w, r, 2, y, 1, 8, y, 2, false);
            rs(w, r, 2, y, 14, 8, y, 14, false);
            rs(w, r, 1, 1, 1, 2, 1, 4, false);
            rs(w, r, 8, 1, 1, 9, 1, 4, false);
            q.fill(w, clip, 1, 1, 1, 1, 1, 3, FLOWING_LAVA, FLOWING_LAVA, false);
            q.fill(w, clip, 9, 1, 1, 9, 1, 3, FLOWING_LAVA, FLOWING_LAVA, false);
            rs(w, r, 3, 1, 8, 7, 1, 12, false);
            q.fill(
                w,
                clip,
                4,
                1,
                9,
                6,
                1,
                11,
                FLOWING_LAVA,
                FLOWING_LAVA,
                false,
            );
            let mut z = 3;
            while z < 14 {
                q.fill(w, clip, 0, 3, z, 0, 4, z, IRON_BARS, IRON_BARS, false);
                q.fill(w, clip, 10, 3, z, 10, 4, z, IRON_BARS, IRON_BARS, false);
                z += 2;
            }
            let mut x = 2;
            while x < 9 {
                q.fill(w, clip, x, 3, 15, x, 4, 15, IRON_BARS, IRON_BARS, false);
                x += 2;
            }
            let s = q.mo(STONE_BRICK_STAIRS, 3);
            rs(w, r, 4, 1, 5, 6, 1, 7, false);
            rs(w, r, 4, 2, 6, 6, 2, 7, false);
            rs(w, r, 4, 3, 7, 6, 3, 7, false);
            for x in 4..=6 {
                q.pb(w, STONE_BRICK_STAIRS, s, x, 1, 4, clip);
                q.pb(w, STONE_BRICK_STAIRS, s, x, 2, 5, clip);
                q.pb(w, STONE_BRICK_STAIRS, s, x, 3, 6, clip);
            }
            let (mut a, mut b2, mut c, mut d) = (2, 0, 3, 1);
            match q.mode {
                0 => {
                    a = 0;
                    b2 = 2;
                }
                1 => {
                    a = 1;
                    b2 = 3;
                    c = 0;
                    d = 2;
                }
                3 => {
                    a = 3;
                    b2 = 1;
                    c = 0;
                    d = 2;
                }
                _ => {}
            }
            let eye = |r: &mut JavaRandom| if r.next_float() > 0.9 { 4 } else { 0 };
            for (base, x, z) in [
                (a, 4, 8),
                (a, 5, 8),
                (a, 6, 8),
                (b2, 4, 12),
                (b2, 5, 12),
                (b2, 6, 12),
                (c, 3, 9),
                (c, 3, 10),
                (c, 3, 11),
                (d, 7, 9),
                (d, 7, 10),
                (d, 7, 11),
            ] {
                let m = base + eye(r);
                q.pb(w, END_PORTAL_FRAME, m, x, 3, z, clip);
            }
            let mut has = spawner;
            if !has {
                let (y, x, z) = (q.y_off(3), q.x_off(5, 6), q.z_off(5, 6));
                if clip.inside(x, y, z) {
                    has = true;
                    q.spawner(w, x, y, z, "Silverfish");
                }
            }
            p.kind = hp(h.door, HKind::PortalRoom { spawner: has });
        }
        HKind::Prison => {
            rs(w, r, 0, 0, 0, 8, 4, 10, true);
            place_door(&q, w, clip, h.door, 1, 1, 0);
            q.fill(w, clip, 1, 1, 10, 3, 3, 10, AIR, AIR, false);
            rs(w, r, 4, 1, 1, 4, 3, 1, false);
            rs(w, r, 4, 1, 3, 4, 3, 3, false);
            rs(w, r, 4, 1, 7, 4, 3, 7, false);
            rs(w, r, 4, 1, 9, 4, 3, 9, false);
            q.fill(w, clip, 4, 1, 4, 4, 3, 6, IRON_BARS, IRON_BARS, false);
            q.fill(w, clip, 5, 1, 5, 7, 3, 5, IRON_BARS, IRON_BARS, false);
            q.pb(w, IRON_BARS, 0, 4, 3, 2, clip);
            q.pb(w, IRON_BARS, 0, 4, 3, 8, clip);
            let m = q.mo(IRON_DOOR, 3);
            q.pb(w, IRON_DOOR, m, 4, 1, 2, clip);
            q.pb(w, IRON_DOOR, m + 8, 4, 2, 2, clip);
            q.pb(w, IRON_DOOR, m, 4, 1, 8, clip);
            q.pb(w, IRON_DOOR, m + 8, 4, 2, 8, clip);
        }
        HKind::RoomCrossing { room } => {
            rs(w, r, 0, 0, 0, 10, 6, 10, true);
            place_door(&q, w, clip, h.door, 4, 1, 0);
            q.fill(w, clip, 4, 1, 10, 6, 3, 10, AIR, AIR, false);
            q.fill(w, clip, 0, 1, 4, 0, 3, 6, AIR, AIR, false);
            q.fill(w, clip, 10, 1, 4, 10, 3, 6, AIR, AIR, false);
            match room {
                0 => {
                    q.pb(w, STONEBRICK, 0, 5, 1, 5, clip);
                    q.pb(w, STONEBRICK, 0, 5, 2, 5, clip);
                    q.pb(w, STONEBRICK, 0, 5, 3, 5, clip);
                    for (x, z) in [(4, 5), (6, 5), (5, 4), (5, 6)] {
                        q.pb(w, TORCH, 0, x, 3, z, clip);
                    }
                    for (x, z) in [
                        (4, 4),
                        (4, 5),
                        (4, 6),
                        (6, 4),
                        (6, 5),
                        (6, 6),
                        (5, 4),
                        (5, 6),
                    ] {
                        q.pb(w, STONE_SLAB, 0, x, 1, z, clip);
                    }
                }
                1 => {
                    for i in 0..5 {
                        q.pb(w, STONEBRICK, 0, 3, 1, 3 + i, clip);
                        q.pb(w, STONEBRICK, 0, 7, 1, 3 + i, clip);
                        q.pb(w, STONEBRICK, 0, 3 + i, 1, 3, clip);
                        q.pb(w, STONEBRICK, 0, 3 + i, 1, 7, clip);
                    }
                    q.pb(w, STONEBRICK, 0, 5, 1, 5, clip);
                    q.pb(w, STONEBRICK, 0, 5, 2, 5, clip);
                    q.pb(w, STONEBRICK, 0, 5, 3, 5, clip);
                    q.pb(w, FLOWING_WATER, 0, 5, 4, 5, clip);
                }
                2 => {
                    for z in 1..=9 {
                        q.pb(w, COBBLESTONE, 0, 1, 3, z, clip);
                        q.pb(w, COBBLESTONE, 0, 9, 3, z, clip);
                    }
                    for x in 1..=9 {
                        q.pb(w, COBBLESTONE, 0, x, 3, 1, clip);
                        q.pb(w, COBBLESTONE, 0, x, 3, 9, clip);
                    }
                    for (x, y, z) in [
                        (5, 1, 4),
                        (5, 1, 6),
                        (5, 3, 4),
                        (5, 3, 6),
                        (4, 1, 5),
                        (6, 1, 5),
                        (4, 3, 5),
                        (6, 3, 5),
                    ] {
                        q.pb(w, COBBLESTONE, 0, x, y, z, clip);
                    }
                    for y in 1..=3 {
                        q.pb(w, COBBLESTONE, 0, 4, y, 4, clip);
                        q.pb(w, COBBLESTONE, 0, 6, y, 4, clip);
                        q.pb(w, COBBLESTONE, 0, 4, y, 6, clip);
                        q.pb(w, COBBLESTONE, 0, 6, y, 6, clip);
                    }
                    q.pb(w, TORCH, 0, 5, 3, 5, clip);
                    for z in 2..=8 {
                        q.pb(w, PLANKS, 0, 2, 3, z, clip);
                        q.pb(w, PLANKS, 0, 3, 3, z, clip);
                        if z <= 3 || z >= 7 {
                            q.pb(w, PLANKS, 0, 4, 3, z, clip);
                            q.pb(w, PLANKS, 0, 5, 3, z, clip);
                            q.pb(w, PLANKS, 0, 6, 3, z, clip);
                        }
                        q.pb(w, PLANKS, 0, 7, 3, z, clip);
                        q.pb(w, PLANKS, 0, 8, 3, z, clip);
                    }
                    let l = q.mo(LADDER, 4);
                    q.pb(w, LADDER, l, 9, 1, 3, clip);
                    q.pb(w, LADDER, l, 9, 2, 3, clip);
                    q.pb(w, LADDER, l, 9, 3, 3, clip);
                    let book = loot::enchanted_book(r, 1, 1, 1);
                    let table = loot::with(loot::STRONGHOLD_CROSSING, book);
                    let n = 1 + r.next_int_n(4);
                    q.chest(w, clip, r, 3, 4, 8, &table, n);
                }
                _ => {}
            }
        }
        HKind::StairsStraight => {
            rs(w, r, 0, 0, 0, 4, 10, 7, true);
            place_door(&q, w, clip, h.door, 1, 7, 0);
            place_door(&q, w, clip, Door::Opening, 1, 1, 7);
            let m = q.mo(STONE_STAIRS, 2);
            for i in 0..6 {
                q.pb(w, STONE_STAIRS, m, 1, 6 - i, 1 + i, clip);
                q.pb(w, STONE_STAIRS, m, 2, 6 - i, 1 + i, clip);
                q.pb(w, STONE_STAIRS, m, 3, 6 - i, 1 + i, clip);
                if i < 5 {
                    q.pb(w, STONEBRICK, 0, 1, 5 - i, 1 + i, clip);
                    q.pb(w, STONEBRICK, 0, 2, 5 - i, 1 + i, clip);
                    q.pb(w, STONEBRICK, 0, 3, 5 - i, 1 + i, clip);
                }
            }
        }
    }
    let _ = block::AIR;
    true
}

/// The portal room of a stronghold start (`Stairs2.strongholdPortalRoom`).
pub fn portal_room(s: &Start) -> Option<&Piece> {
    s.pieces
        .iter()
        .find(|p| matches!(&p.kind, Kind::Hold(h) if matches!(h.k, HKind::PortalRoom { .. })))
}
