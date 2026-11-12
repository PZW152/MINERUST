//! `MapGenNetherBridge` and `StructureNetherBridgePieces` (the Nether fortress).

use super::{find_intersecting, Kind, Piece, Start, BB};
use crate::block::*;
use crate::java::JavaRandom;
use crate::world::loot::{self, Loot};
use crate::world::server::ServerWorld;

/// Piece classes of the fortress.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FKind {
    Crossing3,
    End { seed: i32 },
    Straight,
    Corridor3,
    Corridor4,
    Entrance,
    Crossing2,
    Corridor { chest: bool },
    Corridor5,
    Corridor2 { chest: bool },
    StalkRoom,
    Throne { spawner: bool },
    Crossing,
    Stairs,
}

impl FKind {
    pub fn save(&self, out: &mut Vec<i32>) {
        match *self {
            FKind::Crossing3 => out.push(0),
            FKind::End { seed } => out.extend_from_slice(&[1, seed]),
            FKind::Straight => out.push(2),
            FKind::Corridor3 => out.push(3),
            FKind::Corridor4 => out.push(4),
            FKind::Entrance => out.push(5),
            FKind::Crossing2 => out.push(6),
            FKind::Corridor { chest } => out.extend_from_slice(&[7, chest as i32]),
            FKind::Corridor5 => out.push(8),
            FKind::Corridor2 { chest } => out.extend_from_slice(&[9, chest as i32]),
            FKind::StalkRoom => out.push(10),
            FKind::Throne { spawner } => out.extend_from_slice(&[11, spawner as i32]),
            FKind::Crossing => out.push(12),
            FKind::Stairs => out.push(13),
        }
    }
    pub fn load(v: &mut std::slice::Iter<i32>) -> Option<FKind> {
        let mut t = || v.next().copied();
        Some(match t()? {
            0 => FKind::Crossing3,
            1 => FKind::End { seed: t()? },
            2 => FKind::Straight,
            3 => FKind::Corridor3,
            4 => FKind::Corridor4,
            5 => FKind::Entrance,
            6 => FKind::Crossing2,
            7 => FKind::Corridor { chest: t()? != 0 },
            8 => FKind::Corridor5,
            9 => FKind::Corridor2 { chest: t()? != 0 },
            10 => FKind::StalkRoom,
            11 => FKind::Throne { spawner: t()? != 0 },
            12 => FKind::Crossing,
            _ => FKind::Stairs,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class {
    Straight,
    Crossing3,
    Crossing,
    Stairs,
    Throne,
    Entrance,
    Corridor5,
    Crossing2,
    Corridor2,
    Corridor,
    Corridor3,
    Corridor4,
    StalkRoom,
}

/// `PieceWeight`, identified by its index in the static arrays (the same
/// objects are shared by both lists of a start).
#[derive(Clone, Copy, Debug)]
struct Weight {
    id: usize,
    class: Class,
    weight: i32,
    max: i32,
    in_row: bool,
}

const PRIMARY: [(Class, i32, i32, bool); 6] = [
    (Class::Straight, 30, 0, true),
    (Class::Crossing3, 10, 4, false),
    (Class::Crossing, 10, 4, false),
    (Class::Stairs, 10, 3, false),
    (Class::Throne, 5, 2, false),
    (Class::Entrance, 5, 1, false),
];
const SECONDARY: [(Class, i32, i32, bool); 7] = [
    (Class::Corridor5, 25, 0, true),
    (Class::Crossing2, 15, 5, false),
    (Class::Corridor2, 5, 10, false),
    (Class::Corridor, 5, 10, false),
    (Class::Corridor3, 10, 3, true),
    (Class::Corridor4, 7, 2, false),
    (Class::StalkRoom, 5, 2, false),
];

struct Build {
    primary: Vec<Weight>,
    secondary: Vec<Weight>,
    /// `placeCount` of every weight (13 entries).
    count: [i32; 13],
    last: Option<usize>,
    pending: Vec<usize>,
    start_bb: BB,
}

pub const LOOT: &[Loot] = &[
    loot::l(264, 0, 1, 3, 5),
    loot::l(265, 0, 1, 5, 5),
    loot::l(266, 0, 1, 3, 15),
    loot::l(283, 0, 1, 1, 5),
    loot::l(315, 0, 1, 1, 5),
    loot::l(259, 0, 1, 1, 5),
    loot::l(372, 0, 3, 7, 5),
    loot::l(329, 0, 1, 1, 10),
    loot::l(418, 0, 1, 1, 8),
    loot::l(417, 0, 1, 1, 5),
    loot::l(419, 0, 1, 1, 3),
];

fn fp(k: FKind) -> Kind {
    Kind::Fortress(k)
}

/// `MapGenNetherBridge.canSpawnStructureAtCoords` (reseeds `r`).
pub fn can_spawn(r: &mut JavaRandom, seed: i64, x: i32, z: i32) -> bool {
    let (k, l) = (x >> 4, z >> 4);
    r.set_seed((k ^ (l << 4)) as i64 ^ seed);
    r.next_int();
    if r.next_int_n(3) != 0 {
        return false;
    }
    if x != (k << 4) + 4 + r.next_int_n(8) {
        return false;
    }
    z == (l << 4) + 4 + r.next_int_n(8)
}

/// `MapGenNetherBridge$Start`
pub fn start(r: &mut JavaRandom, cx: i32, cz: i32) -> Start {
    let mk = |list: &[(Class, i32, i32, bool)], base: usize| {
        list.iter()
            .enumerate()
            .map(|(i, &(class, weight, max, in_row))| Weight {
                id: base + i,
                class,
                weight,
                max,
                in_row,
            })
            .collect::<Vec<_>>()
    };
    let (x, z) = ((cx << 4) + 2, (cz << 4) + 2);
    let mode = r.next_int_n(4);
    let bb = BB::new(x, 64, z, x + 19 - 1, 73, z + 19 - 1);
    let first = Piece::new(0, mode, bb, fp(FKind::Crossing3));
    let mut list = vec![first];
    let mut b = Build {
        primary: mk(&PRIMARY, 0),
        secondary: mk(&SECONDARY, 6),
        count: [0; 13],
        last: None,
        pending: Vec::new(),
        start_bb: bb,
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
    s.set_random_height(r, 48, 70);
    s
}

/// `createValidComponent` of each class.
#[allow(clippy::too_many_arguments)]
fn create(
    class: Class,
    list: &[Piece],
    r: &mut JavaRandom,
    x: i32,
    y: i32,
    z: i32,
    mode: i32,
    ty: i32,
) -> Option<Piece> {
    let (ox, oy, w, h, d, k) = match class {
        Class::Straight => (-1, -3, 5, 10, 19, FKind::Straight),
        Class::Crossing3 => (-8, -3, 19, 10, 19, FKind::Crossing3),
        Class::Crossing => (-2, 0, 7, 9, 7, FKind::Crossing),
        Class::Stairs => (-2, 0, 7, 11, 7, FKind::Stairs),
        Class::Throne => (-2, 0, 7, 8, 9, FKind::Throne { spawner: false }),
        Class::Entrance => (-5, -3, 13, 14, 13, FKind::Entrance),
        Class::Corridor5 => (-1, 0, 5, 7, 5, FKind::Corridor5),
        Class::Crossing2 => (-1, 0, 5, 7, 5, FKind::Crossing2),
        Class::Corridor2 => (-1, 0, 5, 7, 5, FKind::Corridor2 { chest: false }),
        Class::Corridor => (-1, 0, 5, 7, 5, FKind::Corridor { chest: false }),
        Class::Corridor3 => (-1, -7, 5, 14, 10, FKind::Corridor3),
        Class::Corridor4 => (-3, 0, 9, 7, 9, FKind::Corridor4),
        Class::StalkRoom => (-5, -3, 13, 14, 13, FKind::StalkRoom),
    };
    let bb = BB::component(x, y, z, ox, oy, 0, w, h, d, mode);
    if bb.y0 <= 10 || find_intersecting(list, &bb).is_some() {
        return None;
    }
    let k = match k {
        FKind::Corridor { .. } => FKind::Corridor {
            chest: r.next_int_n(3) == 0,
        },
        FKind::Corridor2 { .. } => FKind::Corridor2 {
            chest: r.next_int_n(3) == 0,
        },
        other => other,
    };
    Some(Piece::new(ty, mode, bb, fp(k)))
}

/// `End.of`
fn end_of(
    list: &[Piece],
    r: &mut JavaRandom,
    x: i32,
    y: i32,
    z: i32,
    mode: i32,
    ty: i32,
) -> Option<Piece> {
    let bb = BB::component(x, y, z, -1, -3, 0, 5, 10, 8, mode);
    if bb.y0 <= 10 || find_intersecting(list, &bb).is_some() {
        return None;
    }
    let seed = r.next_int();
    Some(Piece::new(ty, mode, bb, fp(FKind::End { seed })))
}

/// `Piece.getNextComponent` (weighted pick from one list).
#[allow(clippy::too_many_arguments)]
fn next_weighted(
    secondary: bool,
    list: &[Piece],
    b: &mut Build,
    r: &mut JavaRandom,
    x: i32,
    y: i32,
    z: i32,
    mode: i32,
    ty: i32,
) -> Option<Piece> {
    let weights = if secondary { &b.secondary } else { &b.primary };
    let mut any = false;
    let mut total = 0;
    for w in weights {
        if w.max > 0 && b.count[w.id] < w.max {
            any = true;
        }
        total += w.weight;
    }
    let total = if any { total } else { -1 };
    let ok = total > 0 && ty <= 30;
    let mut tries = 0;
    while tries < 5 && ok {
        tries += 1;
        let mut n = r.next_int_n(total);
        let snapshot: Vec<Weight> = if secondary {
            b.secondary.clone()
        } else {
            b.primary.clone()
        };
        for w in snapshot {
            n -= w.weight;
            if n < 0 {
                let can = w.max == 0 || b.count[w.id] < w.max;
                if !can || (Some(w.id) == b.last && !w.in_row) {
                    break;
                }
                if let Some(p) = create(w.class, list, r, x, y, z, mode, ty) {
                    b.count[w.id] += 1;
                    b.last = Some(w.id);
                    if !(w.max == 0 || b.count[w.id] < w.max) {
                        let l = if secondary {
                            &mut b.secondary
                        } else {
                            &mut b.primary
                        };
                        l.retain(|q| q.id != w.id);
                    }
                    return Some(p);
                }
            }
        }
    }
    end_of(list, r, x, y, z, mode, ty)
}

/// `Piece.getNextComponent` (distance check, adds to the list).
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
    secondary: bool,
) {
    if (x - b.start_bb.x0).abs() <= 112 && (z - b.start_bb.z0).abs() <= 112 {
        if let Some(p) = next_weighted(secondary, list, b, r, x, y, z, mode, ty + 1) {
            list.push(p);
            b.pending.push(list.len() - 1);
        }
    } else {
        // End.of without adding it to the list
        let _ = end_of(list, r, x, y, z, mode, ty);
    }
}

fn normal(
    list: &mut Vec<Piece>,
    i: usize,
    b: &mut Build,
    r: &mut JavaRandom,
    a: i32,
    c: i32,
    sec: bool,
) {
    let p = &list[i];
    let (bb, ty, mode) = (p.bb, p.ty, p.mode);
    match mode {
        0 => next(list, b, r, bb.x0 + a, bb.y0 + c, bb.z1 + 1, mode, ty, sec),
        1 => next(list, b, r, bb.x0 - 1, bb.y0 + c, bb.z0 + a, mode, ty, sec),
        2 => next(list, b, r, bb.x0 + a, bb.y0 + c, bb.z0 - 1, mode, ty, sec),
        3 => next(list, b, r, bb.x1 + 1, bb.y0 + c, bb.z0 + a, mode, ty, sec),
        _ => {}
    }
}

fn side_x(
    list: &mut Vec<Piece>,
    i: usize,
    b: &mut Build,
    r: &mut JavaRandom,
    a: i32,
    c: i32,
    sec: bool,
) {
    let p = &list[i];
    let (bb, ty) = (p.bb, p.ty);
    match p.mode {
        0 | 2 => next(list, b, r, bb.x0 - 1, bb.y0 + a, bb.z0 + c, 1, ty, sec),
        1 | 3 => next(list, b, r, bb.x0 + c, bb.y0 + a, bb.z0 - 1, 2, ty, sec),
        _ => {}
    }
}

fn side_z(
    list: &mut Vec<Piece>,
    i: usize,
    b: &mut Build,
    r: &mut JavaRandom,
    a: i32,
    c: i32,
    sec: bool,
) {
    let p = &list[i];
    let (bb, ty) = (p.bb, p.ty);
    match p.mode {
        0 | 2 => next(list, b, r, bb.x1 + 1, bb.y0 + a, bb.z0 + c, 3, ty, sec),
        1 | 3 => next(list, b, r, bb.x0 + c, bb.y0 + a, bb.z1 + 1, 0, ty, sec),
        _ => {}
    }
}

/// `buildComponent`
fn build(list: &mut Vec<Piece>, i: usize, b: &mut Build, r: &mut JavaRandom) {
    let Kind::Fortress(k) = list[i].kind else {
        return;
    };
    let mode = list[i].mode;
    match k {
        FKind::Crossing3 => {
            normal(list, i, b, r, 8, 3, false);
            side_x(list, i, b, r, 3, 8, false);
            side_z(list, i, b, r, 3, 8, false);
        }
        FKind::Straight => normal(list, i, b, r, 1, 3, false),
        FKind::Corridor3 => normal(list, i, b, r, 1, 0, true),
        FKind::Corridor4 => {
            let c = if mode == 1 || mode == 2 { 5 } else { 1 };
            let a = r.next_int_n(8) > 0;
            side_x(list, i, b, r, 0, c, a);
            let a = r.next_int_n(8) > 0;
            side_z(list, i, b, r, 0, c, a);
        }
        FKind::Entrance => normal(list, i, b, r, 5, 3, true),
        FKind::Crossing2 => {
            normal(list, i, b, r, 1, 0, true);
            side_x(list, i, b, r, 0, 1, true);
            side_z(list, i, b, r, 0, 1, true);
        }
        FKind::Corridor { .. } => side_x(list, i, b, r, 0, 1, true),
        FKind::Corridor5 => normal(list, i, b, r, 1, 0, true),
        FKind::Corridor2 { .. } => side_z(list, i, b, r, 0, 1, true),
        FKind::StalkRoom => {
            normal(list, i, b, r, 5, 3, true);
            normal(list, i, b, r, 5, 11, true);
        }
        FKind::Crossing => {
            normal(list, i, b, r, 2, 0, false);
            side_x(list, i, b, r, 0, 2, false);
            side_z(list, i, b, r, 0, 2, false);
        }
        FKind::Stairs => side_z(list, i, b, r, 6, 2, false),
        FKind::End { .. } | FKind::Throne { .. } => {}
    }
}

// ---------------------------------------------------------------- building

struct Ctx<'a> {
    p: &'a Piece,
    clip: &'a BB,
}

impl Ctx<'_> {
    #[allow(clippy::too_many_arguments)]
    fn f(
        &self,
        w: &mut ServerWorld,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        edge: u8,
        inner: u8,
    ) {
        self.p
            .fill(w, self.clip, x0, y0, z0, x1, y1, z1, edge, inner, false);
    }
    fn nb(&self, w: &mut ServerWorld, x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32) {
        self.f(w, x0, y0, z0, x1, y1, z1, NETHER_BRICK, NETHER_BRICK);
    }
    fn fence(&self, w: &mut ServerWorld, x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32) {
        self.f(
            w,
            x0,
            y0,
            z0,
            x1,
            y1,
            z1,
            NETHER_BRICK_FENCE,
            NETHER_BRICK_FENCE,
        );
    }
    fn air(&self, w: &mut ServerWorld, x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32) {
        self.f(w, x0, y0, z0, x1, y1, z1, AIR, AIR);
    }
    fn pb(&self, w: &mut ServerWorld, id: u8, m: i32, x: i32, y: i32, z: i32) {
        self.p.pb(w, id, m, x, y, z, self.clip);
    }
    fn down(&self, w: &mut ServerWorld, x: i32, z: i32) {
        self.p.fill_down(w, NETHER_BRICK, 0, x, -1, z, self.clip);
    }
}

/// The towers of the entrance and the nether wart room share this shell.
fn room_shell(c: &Ctx, w: &mut ServerWorld) {
    c.nb(w, 0, 3, 0, 12, 4, 12);
    c.air(w, 0, 5, 0, 12, 13, 12);
    c.nb(w, 0, 5, 0, 1, 12, 12);
    c.nb(w, 11, 5, 0, 12, 12, 12);
    c.nb(w, 2, 5, 11, 4, 12, 12);
    c.nb(w, 8, 5, 11, 10, 12, 12);
    c.nb(w, 5, 9, 11, 7, 12, 12);
    c.nb(w, 2, 5, 0, 4, 12, 1);
    c.nb(w, 8, 5, 0, 10, 12, 1);
    c.nb(w, 5, 9, 0, 7, 12, 1);
    c.nb(w, 2, 11, 2, 10, 12, 10);
}

fn room_battlements(c: &Ctx, w: &mut ServerWorld) {
    let mut i = 1;
    while i <= 11 {
        c.fence(w, i, 10, 0, i, 11, 0);
        c.fence(w, i, 10, 12, i, 11, 12);
        c.fence(w, 0, 10, i, 0, 11, i);
        c.fence(w, 12, 10, i, 12, 11, i);
        c.pb(w, NETHER_BRICK, 0, i, 13, 0);
        c.pb(w, NETHER_BRICK, 0, i, 13, 12);
        c.pb(w, NETHER_BRICK, 0, 0, 13, i);
        c.pb(w, NETHER_BRICK, 0, 12, 13, i);
        c.pb(w, NETHER_BRICK_FENCE, 0, i + 1, 13, 0);
        c.pb(w, NETHER_BRICK_FENCE, 0, i + 1, 13, 12);
        c.pb(w, NETHER_BRICK_FENCE, 0, 0, 13, i + 1);
        c.pb(w, NETHER_BRICK_FENCE, 0, 12, 13, i + 1);
        i += 2;
    }
    c.pb(w, NETHER_BRICK_FENCE, 0, 0, 13, 0);
    c.pb(w, NETHER_BRICK_FENCE, 0, 0, 13, 12);
    c.pb(w, NETHER_BRICK_FENCE, 0, 0, 13, 0);
    c.pb(w, NETHER_BRICK_FENCE, 0, 12, 13, 0);
}

fn room_base(c: &Ctx, w: &mut ServerWorld) {
    c.nb(w, 4, 2, 0, 8, 2, 12);
    c.nb(w, 0, 2, 4, 12, 2, 8);
    c.nb(w, 4, 0, 0, 8, 1, 3);
    c.nb(w, 4, 0, 9, 8, 1, 12);
    c.nb(w, 0, 0, 4, 3, 1, 8);
    c.nb(w, 9, 0, 4, 12, 1, 8);
    for x in 4..=8 {
        for z in 0..=2 {
            c.down(w, x, z);
            c.down(w, x, 12 - z);
        }
    }
    for x in 0..=2 {
        for z in 4..=8 {
            c.down(w, x, z);
            c.down(w, 12 - x, z);
        }
    }
}

/// The 5x5 corridor floor and the columns below it.
fn small_base(c: &Ctx, w: &mut ServerWorld, n: i32) {
    for x in 0..=n {
        for z in 0..=n {
            c.down(w, x, z);
        }
    }
}

/// `addComponentParts`
pub fn add_parts(p: &mut Piece, w: &mut ServerWorld, r: &mut JavaRandom, clip: &BB) -> bool {
    let Kind::Fortress(k) = p.kind else {
        return true;
    };
    let snapshot = p.clone();
    let c = Ctx { p: &snapshot, clip };
    match k {
        FKind::Crossing3 => {
            c.nb(w, 7, 3, 0, 11, 4, 18);
            c.nb(w, 0, 3, 7, 18, 4, 11);
            c.air(w, 8, 5, 0, 10, 7, 18);
            c.air(w, 0, 5, 8, 18, 7, 10);
            c.nb(w, 7, 5, 0, 7, 5, 7);
            c.nb(w, 7, 5, 11, 7, 5, 18);
            c.nb(w, 11, 5, 0, 11, 5, 7);
            c.nb(w, 11, 5, 11, 11, 5, 18);
            c.nb(w, 0, 5, 7, 7, 5, 7);
            c.nb(w, 11, 5, 7, 18, 5, 7);
            c.nb(w, 0, 5, 11, 7, 5, 11);
            c.nb(w, 11, 5, 11, 18, 5, 11);
            c.nb(w, 7, 2, 0, 11, 2, 5);
            c.nb(w, 7, 2, 13, 11, 2, 18);
            c.nb(w, 7, 0, 0, 11, 1, 3);
            c.nb(w, 7, 0, 15, 11, 1, 18);
            for x in 7..=11 {
                for z in 0..=2 {
                    c.down(w, x, z);
                    c.down(w, x, 18 - z);
                }
            }
            c.nb(w, 0, 2, 7, 5, 2, 11);
            c.nb(w, 13, 2, 7, 18, 2, 11);
            c.nb(w, 0, 0, 7, 3, 1, 11);
            c.nb(w, 15, 0, 7, 18, 1, 11);
            for x in 0..=2 {
                for z in 7..=11 {
                    c.down(w, x, z);
                    c.down(w, 18 - x, z);
                }
            }
        }
        FKind::End { seed } => {
            let mut q = JavaRandom::new(seed as i64);
            for x in 0..=4 {
                for y in 3..=4 {
                    let n = q.next_int_n(8);
                    c.nb(w, x, y, 0, x, y, n);
                }
            }
            let n = q.next_int_n(8);
            c.nb(w, 0, 5, 0, 0, 5, n);
            let n = q.next_int_n(8);
            c.nb(w, 4, 5, 0, 4, 5, n);
            for x in 0..=4 {
                let n = q.next_int_n(5);
                c.nb(w, x, 2, 0, x, 2, n);
            }
            for x in 0..=4 {
                for y in 0..=1 {
                    let n = q.next_int_n(3);
                    c.nb(w, x, y, 0, x, y, n);
                }
            }
        }
        FKind::Straight => {
            c.nb(w, 0, 3, 0, 4, 4, 18);
            c.air(w, 1, 5, 0, 3, 7, 18);
            c.nb(w, 0, 5, 0, 0, 5, 18);
            c.nb(w, 4, 5, 0, 4, 5, 18);
            c.nb(w, 0, 2, 0, 4, 2, 5);
            c.nb(w, 0, 2, 13, 4, 2, 18);
            c.nb(w, 0, 0, 0, 4, 1, 3);
            c.nb(w, 0, 0, 15, 4, 1, 18);
            for x in 0..=4 {
                for z in 0..=2 {
                    c.down(w, x, z);
                    c.down(w, x, 18 - z);
                }
            }
            c.fence(w, 0, 1, 1, 0, 4, 1);
            c.fence(w, 0, 3, 4, 0, 4, 4);
            c.fence(w, 0, 3, 14, 0, 4, 14);
            c.fence(w, 0, 1, 17, 0, 4, 17);
            c.fence(w, 4, 1, 1, 4, 4, 1);
            c.fence(w, 4, 3, 4, 4, 4, 4);
            c.fence(w, 4, 3, 14, 4, 4, 14);
            c.fence(w, 4, 1, 17, 4, 4, 17);
        }
        FKind::Corridor3 => {
            let sm = snapshot.mo(NETHER_BRICK_STAIRS, 2);
            for i in 0..=9 {
                let lo = (7 - i).max(1);
                let hi = (lo + 5).max(14 - i).min(13);
                let z = i;
                c.nb(w, 0, 0, z, 4, lo, z);
                c.air(w, 1, lo + 1, z, 3, hi - 1, z);
                if i <= 6 {
                    c.pb(w, NETHER_BRICK_STAIRS, sm, 1, lo + 1, z);
                    c.pb(w, NETHER_BRICK_STAIRS, sm, 2, lo + 1, z);
                    c.pb(w, NETHER_BRICK_STAIRS, sm, 3, lo + 1, z);
                }
                c.nb(w, 0, hi, z, 4, hi, z);
                c.nb(w, 0, lo + 1, z, 0, hi - 1, z);
                c.nb(w, 4, lo + 1, z, 4, hi - 1, z);
                if i & 1 == 0 {
                    c.fence(w, 0, lo + 2, z, 0, lo + 3, z);
                    c.fence(w, 4, lo + 2, z, 4, lo + 3, z);
                }
                for x in 0..=4 {
                    c.down(w, x, z);
                }
            }
        }
        FKind::Corridor4 => {
            c.nb(w, 0, 0, 0, 8, 1, 8);
            c.air(w, 0, 2, 0, 8, 5, 8);
            c.nb(w, 0, 6, 0, 8, 6, 5);
            c.nb(w, 0, 2, 0, 2, 5, 0);
            c.nb(w, 6, 2, 0, 8, 5, 0);
            c.fence(w, 1, 3, 0, 1, 4, 0);
            c.fence(w, 7, 3, 0, 7, 4, 0);
            c.nb(w, 0, 2, 4, 8, 2, 8);
            c.air(w, 1, 1, 4, 2, 2, 4);
            c.air(w, 6, 1, 4, 7, 2, 4);
            c.fence(w, 0, 3, 8, 8, 3, 8);
            c.fence(w, 0, 3, 6, 0, 3, 7);
            c.fence(w, 8, 3, 6, 8, 3, 7);
            c.nb(w, 0, 3, 4, 0, 5, 5);
            c.nb(w, 8, 3, 4, 8, 5, 5);
            c.nb(w, 1, 3, 5, 2, 5, 5);
            c.nb(w, 6, 3, 5, 7, 5, 5);
            c.fence(w, 1, 4, 5, 1, 5, 5);
            c.fence(w, 7, 4, 5, 7, 5, 5);
            for z in 0..=5 {
                for x in 0..=8 {
                    c.down(w, x, z);
                }
            }
        }
        FKind::Entrance => {
            room_shell(&c, w);
            c.fence(w, 5, 8, 0, 7, 8, 0);
            room_battlements(&c, w);
            let mut z = 3;
            while z <= 9 {
                c.fence(w, 1, 7, z, 1, 8, z);
                c.fence(w, 11, 7, z, 11, 8, z);
                z += 2;
            }
            room_base(&c, w);
            c.nb(w, 5, 5, 5, 7, 5, 7);
            c.air(w, 6, 1, 6, 6, 4, 6);
            c.pb(w, NETHER_BRICK, 0, 6, 0, 6);
            c.pb(w, FLOWING_LAVA, 0, 6, 5, 6);
            let (x, y, z) = (
                snapshot.x_off(6, 6),
                snapshot.y_off(5),
                snapshot.z_off(6, 6),
            );
            if clip.inside(x, y, z) {
                w.scheduled_immediate = true;
                w.update_tick(FLOWING_LAVA, x, y, z, r);
                w.scheduled_immediate = false;
            }
        }
        FKind::Crossing2 => {
            c.nb(w, 0, 0, 0, 4, 1, 4);
            c.air(w, 0, 2, 0, 4, 5, 4);
            c.nb(w, 0, 2, 0, 0, 5, 0);
            c.nb(w, 4, 2, 0, 4, 5, 0);
            c.nb(w, 0, 2, 4, 0, 5, 4);
            c.nb(w, 4, 2, 4, 4, 5, 4);
            c.nb(w, 0, 6, 0, 4, 6, 4);
            small_base(&c, w, 4);
        }
        FKind::Corridor { chest } => {
            c.nb(w, 0, 0, 0, 4, 1, 4);
            c.air(w, 0, 2, 0, 4, 5, 4);
            c.nb(w, 4, 2, 0, 4, 5, 4);
            c.fence(w, 4, 3, 1, 4, 4, 1);
            c.fence(w, 4, 3, 3, 4, 4, 3);
            c.nb(w, 0, 2, 0, 0, 5, 0);
            c.nb(w, 0, 2, 4, 3, 5, 4);
            c.f(w, 1, 3, 4, 1, 4, 4, NETHER_BRICK_FENCE, NETHER_BRICK);
            c.f(w, 3, 3, 4, 3, 4, 4, NETHER_BRICK_FENCE, NETHER_BRICK);
            if chest {
                let (x, y, z) = (
                    snapshot.x_off(3, 3),
                    snapshot.y_off(2),
                    snapshot.z_off(3, 3),
                );
                if clip.inside(x, y, z) {
                    p.kind = fp(FKind::Corridor { chest: false });
                    let n = 2 + r.next_int_n(4);
                    snapshot.chest(w, clip, r, 3, 2, 3, LOOT, n);
                }
            }
            c.nb(w, 0, 6, 0, 4, 6, 4);
            small_base(&c, w, 4);
        }
        FKind::Corridor5 => {
            c.nb(w, 0, 0, 0, 4, 1, 4);
            c.air(w, 0, 2, 0, 4, 5, 4);
            c.nb(w, 0, 2, 0, 0, 5, 4);
            c.nb(w, 4, 2, 0, 4, 5, 4);
            c.fence(w, 0, 3, 1, 0, 4, 1);
            c.fence(w, 0, 3, 3, 0, 4, 3);
            c.fence(w, 4, 3, 1, 4, 4, 1);
            c.fence(w, 4, 3, 3, 4, 4, 3);
            c.nb(w, 0, 6, 0, 4, 6, 4);
            small_base(&c, w, 4);
        }
        FKind::Corridor2 { chest } => {
            c.nb(w, 0, 0, 0, 4, 1, 4);
            c.air(w, 0, 2, 0, 4, 5, 4);
            c.nb(w, 0, 2, 0, 0, 5, 4);
            c.fence(w, 0, 3, 1, 0, 4, 1);
            c.fence(w, 0, 3, 3, 0, 4, 3);
            c.nb(w, 4, 2, 0, 4, 5, 0);
            c.nb(w, 1, 2, 4, 4, 5, 4);
            c.f(w, 1, 3, 4, 1, 4, 4, NETHER_BRICK_FENCE, NETHER_BRICK);
            c.f(w, 3, 3, 4, 3, 4, 4, NETHER_BRICK_FENCE, NETHER_BRICK);
            if chest {
                let (x, y, z) = (
                    snapshot.x_off(1, 3),
                    snapshot.y_off(2),
                    snapshot.z_off(1, 3),
                );
                if clip.inside(x, y, z) {
                    p.kind = fp(FKind::Corridor2 { chest: false });
                    let n = 2 + r.next_int_n(4);
                    snapshot.chest(w, clip, r, 1, 2, 3, LOOT, n);
                }
            }
            c.nb(w, 0, 6, 0, 4, 6, 4);
            small_base(&c, w, 4);
        }
        FKind::StalkRoom => {
            room_shell(&c, w);
            room_battlements(&c, w);
            let mut z = 3;
            while z <= 9 {
                c.fence(w, 1, 7, z, 1, 8, z);
                c.fence(w, 11, 7, z, 11, 8, z);
                z += 2;
            }
            let s3 = snapshot.mo(NETHER_BRICK_STAIRS, 3);
            for i in 0..=6 {
                let z = i + 4;
                for x in 5..=7 {
                    c.pb(w, NETHER_BRICK_STAIRS, s3, x, 5 + i, z);
                }
                if (5..=8).contains(&z) {
                    c.nb(w, 5, 5, z, 7, i + 4, z);
                } else if (9..=10).contains(&z) {
                    c.nb(w, 5, 8, z, 7, i + 4, z);
                }
                if i >= 1 {
                    c.air(w, 5, 6 + i, z, 7, 9 + i, z);
                }
            }
            for x in 5..=7 {
                c.pb(w, NETHER_BRICK_STAIRS, s3, x, 12, 11);
            }
            c.fence(w, 5, 6, 7, 5, 7, 7);
            c.fence(w, 7, 6, 7, 7, 7, 7);
            c.air(w, 5, 13, 12, 7, 13, 12);
            c.nb(w, 2, 5, 2, 3, 5, 3);
            c.nb(w, 2, 5, 9, 3, 5, 10);
            c.nb(w, 2, 5, 4, 2, 5, 8);
            c.nb(w, 9, 5, 2, 10, 5, 3);
            c.nb(w, 9, 5, 9, 10, 5, 10);
            c.nb(w, 10, 5, 4, 10, 5, 8);
            let s0 = snapshot.mo(NETHER_BRICK_STAIRS, 0);
            let s1 = snapshot.mo(NETHER_BRICK_STAIRS, 1);
            for z in [2, 3, 9, 10] {
                c.pb(w, NETHER_BRICK_STAIRS, s1, 4, 5, z);
            }
            for z in [2, 3, 9, 10] {
                c.pb(w, NETHER_BRICK_STAIRS, s0, 8, 5, z);
            }
            c.f(w, 3, 4, 4, 4, 4, 8, SOUL_SAND, SOUL_SAND);
            c.f(w, 8, 4, 4, 9, 4, 8, SOUL_SAND, SOUL_SAND);
            c.f(w, 3, 5, 4, 4, 5, 8, NETHER_WART, NETHER_WART);
            c.f(w, 8, 5, 4, 9, 5, 8, NETHER_WART, NETHER_WART);
            room_base(&c, w);
        }
        FKind::Throne { spawner } => {
            c.air(w, 0, 2, 0, 6, 7, 7);
            c.nb(w, 1, 0, 0, 5, 1, 7);
            c.nb(w, 1, 2, 1, 5, 2, 7);
            c.nb(w, 1, 3, 2, 5, 3, 7);
            c.nb(w, 1, 4, 3, 5, 4, 7);
            c.nb(w, 1, 2, 0, 1, 4, 2);
            c.nb(w, 5, 2, 0, 5, 4, 2);
            c.nb(w, 1, 5, 2, 1, 5, 3);
            c.nb(w, 5, 5, 2, 5, 5, 3);
            c.nb(w, 0, 5, 3, 0, 5, 8);
            c.nb(w, 6, 5, 3, 6, 5, 8);
            c.nb(w, 1, 5, 8, 5, 5, 8);
            c.pb(w, NETHER_BRICK_FENCE, 0, 1, 6, 3);
            c.pb(w, NETHER_BRICK_FENCE, 0, 5, 6, 3);
            c.fence(w, 0, 6, 3, 0, 6, 8);
            c.fence(w, 6, 6, 3, 6, 6, 8);
            c.fence(w, 1, 6, 8, 5, 7, 8);
            c.fence(w, 2, 8, 8, 4, 8, 8);
            if !spawner {
                let (x, y, z) = (
                    snapshot.x_off(3, 5),
                    snapshot.y_off(5),
                    snapshot.z_off(3, 5),
                );
                if clip.inside(x, y, z) {
                    p.kind = fp(FKind::Throne { spawner: true });
                    w.set_block(x, y, z, MOB_SPAWNER, 0, 2);
                    snapshot.spawner(w, x, y, z, "Blaze");
                }
            }
            small_base(&c, w, 6);
        }
        FKind::Crossing => {
            c.nb(w, 0, 0, 0, 6, 1, 6);
            c.air(w, 0, 2, 0, 6, 7, 6);
            c.nb(w, 0, 2, 0, 1, 6, 0);
            c.nb(w, 0, 2, 6, 1, 6, 6);
            c.nb(w, 5, 2, 0, 6, 6, 0);
            c.nb(w, 5, 2, 6, 6, 6, 6);
            c.nb(w, 0, 2, 0, 0, 6, 1);
            c.nb(w, 0, 2, 5, 0, 6, 6);
            c.nb(w, 6, 2, 0, 6, 6, 1);
            c.nb(w, 6, 2, 5, 6, 6, 6);
            c.nb(w, 2, 6, 0, 4, 6, 0);
            c.fence(w, 2, 5, 0, 4, 5, 0);
            c.nb(w, 2, 6, 6, 4, 6, 6);
            c.fence(w, 2, 5, 6, 4, 5, 6);
            c.nb(w, 0, 6, 2, 0, 6, 4);
            c.fence(w, 0, 5, 2, 0, 5, 4);
            c.nb(w, 6, 6, 2, 6, 6, 4);
            c.fence(w, 6, 5, 2, 6, 5, 4);
            small_base(&c, w, 6);
        }
        FKind::Stairs => {
            c.nb(w, 0, 0, 0, 6, 1, 6);
            c.air(w, 0, 2, 0, 6, 10, 6);
            c.nb(w, 0, 2, 0, 1, 8, 0);
            c.nb(w, 5, 2, 0, 6, 8, 0);
            c.nb(w, 0, 2, 1, 0, 8, 6);
            c.nb(w, 6, 2, 1, 6, 8, 6);
            c.nb(w, 1, 2, 6, 5, 8, 6);
            c.fence(w, 0, 3, 2, 0, 5, 4);
            c.fence(w, 6, 3, 2, 6, 5, 2);
            c.fence(w, 6, 3, 4, 6, 5, 4);
            c.pb(w, NETHER_BRICK, 0, 5, 2, 5);
            c.nb(w, 4, 2, 5, 4, 3, 5);
            c.nb(w, 3, 2, 5, 3, 4, 5);
            c.nb(w, 2, 2, 5, 2, 5, 5);
            c.nb(w, 1, 2, 5, 1, 6, 5);
            c.nb(w, 1, 7, 1, 5, 7, 4);
            c.air(w, 6, 8, 2, 6, 8, 4);
            c.nb(w, 2, 6, 0, 4, 8, 0);
            c.fence(w, 2, 5, 0, 4, 5, 0);
            small_base(&c, w, 6);
        }
    }
    true
}
