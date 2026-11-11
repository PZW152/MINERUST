//! `MapGenMineshaft`, `StructureMineshaftStart`, `StructureMineshaftPieces`.

use super::{find_intersecting, Kind, Piece, Start, BB};
use crate::block;
use crate::item::Stack;
use crate::java::JavaRandom;
use crate::world::loot;
use crate::world::server::{GenEntity, ServerWorld};

#[derive(Clone, Debug)]
pub enum Mine {
    Room {
        links: Vec<BB>,
    },
    Corridor {
        rails: bool,
        spiders: bool,
        spawner: bool,
        sections: i32,
    },
    Cross {
        dir: i32,
        multi: bool,
    },
    Stairs,
}

impl Mine {
    pub fn save(&self, out: &mut Vec<i32>) {
        match self {
            Mine::Room { links } => {
                out.push(0);
                out.push(links.len() as i32);
                for b in links {
                    out.extend_from_slice(&[b.x0, b.y0, b.z0, b.x1, b.y1, b.z1]);
                }
            }
            Mine::Corridor {
                rails,
                spiders,
                spawner,
                sections,
            } => out.extend_from_slice(&[
                1,
                *rails as i32,
                *spiders as i32,
                *spawner as i32,
                *sections,
            ]),
            Mine::Cross { dir, multi } => out.extend_from_slice(&[2, *dir, *multi as i32]),
            Mine::Stairs => out.push(3),
        }
    }
    pub fn load(v: &mut std::slice::Iter<i32>) -> Option<Mine> {
        let mut t = || v.next().copied();
        Some(match t()? {
            0 => {
                let n = t()? as usize;
                let mut links = Vec::new();
                for _ in 0..n {
                    links.push(BB::new(t()?, t()?, t()?, t()?, t()?, t()?));
                }
                Mine::Room { links }
            }
            1 => Mine::Corridor {
                rails: t()? != 0,
                spiders: t()? != 0,
                spawner: t()? != 0,
                sections: t()?,
            },
            2 => Mine::Cross {
                dir: t()?,
                multi: t()? != 0,
            },
            _ => Mine::Stairs,
        })
    }
}

/// `MapGenMineshaft.canSpawnStructureAtCoords`
pub fn can_spawn(r: &mut JavaRandom, x: i32, z: i32) -> bool {
    r.next_double() < 0.004 && r.next_int_n(80) < x.abs().max(z.abs())
}

/// `StructureMineshaftStart`
pub fn start(r: &mut JavaRandom, cx: i32, cz: i32) -> Start {
    let (x, z) = ((cx << 4) + 2, (cz << 4) + 2);
    let bb = BB::new(
        x,
        50,
        z,
        x + 7 + r.next_int_n(6),
        54 + r.next_int_n(6),
        z + 7 + r.next_int_n(6),
    );
    let room = Piece::new(0, -1, bb, Kind::Mine(Mine::Room { links: Vec::new() }));
    let mut list = vec![room];
    build(&mut list, 0, 0, r);
    let mut s = Start {
        cx,
        cz,
        bb: BB::default(),
        pieces: list,
        sizeable: true,
    };
    s.update_bb();
    s.mark_available_height(r, 10);
    s
}

/// `StructureMineshaftPieces.getRandomComponent`
fn random_component(
    list: &[Piece],
    r: &mut JavaRandom,
    x: i32,
    y: i32,
    z: i32,
    mode: i32,
    ty: i32,
) -> Option<Piece> {
    let k = r.next_int_n(100);
    if k >= 80 {
        let bb = cross_placement(list, r, x, y, z, mode)?;
        Some(Piece::new(
            ty,
            -1,
            bb,
            Kind::Mine(Mine::Cross {
                dir: mode,
                multi: bb.ys() > 3,
            }),
        ))
    } else if k >= 70 {
        let bb = stairs_placement(list, x, y, z, mode)?;
        Some(Piece::new(ty, mode, bb, Kind::Mine(Mine::Stairs)))
    } else {
        let bb = corridor_placement(list, r, x, y, z, mode)?;
        let rails = r.next_int_n(3) == 0;
        let spiders = !rails && r.next_int_n(23) == 0;
        let sections = if mode != 2 && mode != 0 {
            bb.xs() / 5
        } else {
            bb.zs() / 5
        };
        Some(Piece::new(
            ty,
            mode,
            bb,
            Kind::Mine(Mine::Corridor {
                rails,
                spiders,
                spawner: false,
                sections,
            }),
        ))
    }
}

/// `getNextMineShaftComponent`; `root` is the room the shaft starts from.
#[allow(clippy::too_many_arguments)]
fn next(
    list: &mut Vec<Piece>,
    root: usize,
    r: &mut JavaRandom,
    x: i32,
    y: i32,
    z: i32,
    mode: i32,
    ty: i32,
) -> Option<BB> {
    if ty > 8 {
        return None;
    }
    let rb = list[root].bb;
    if (x - rb.x0).abs() > 80 || (z - rb.z0).abs() > 80 {
        return None;
    }
    let p = random_component(list, r, x, y, z, mode, ty + 1)?;
    let bb = p.bb;
    list.push(p);
    let i = list.len() - 1;
    build(list, i, root, r);
    Some(bb)
}

/// `buildComponent` of piece `i`.
fn build(list: &mut Vec<Piece>, i: usize, root: usize, r: &mut JavaRandom) {
    let p = list[i].clone();
    let b = p.bb;
    let ty = p.ty;
    match &p.kind {
        Kind::Mine(Mine::Room { .. }) => {
            let mut links = Vec::new();
            let hy = (b.ys() - 3 - 1).max(1);
            let mut k = 0;
            while k < b.xs() {
                k += r.next_int_n(b.xs());
                if k + 3 > b.xs() {
                    break;
                }
                let yy = b.y0 + r.next_int_n(hy) + 1;
                if let Some(c) = next(list, root, r, b.x0 + k, yy, b.z0 - 1, 2, ty) {
                    links.push(BB::new(c.x0, c.y0, b.z0, c.x1, c.y1, b.z0 + 1));
                }
                k += 4;
            }
            k = 0;
            while k < b.xs() {
                k += r.next_int_n(b.xs());
                if k + 3 > b.xs() {
                    break;
                }
                let yy = b.y0 + r.next_int_n(hy) + 1;
                if let Some(c) = next(list, root, r, b.x0 + k, yy, b.z1 + 1, 0, ty) {
                    links.push(BB::new(c.x0, c.y0, b.z1 - 1, c.x1, c.y1, b.z1));
                }
                k += 4;
            }
            k = 0;
            while k < b.zs() {
                k += r.next_int_n(b.zs());
                if k + 3 > b.zs() {
                    break;
                }
                let yy = b.y0 + r.next_int_n(hy) + 1;
                if let Some(c) = next(list, root, r, b.x0 - 1, yy, b.z0 + k, 1, ty) {
                    links.push(BB::new(b.x0, c.y0, c.z0, b.x0 + 1, c.y1, c.z1));
                }
                k += 4;
            }
            k = 0;
            while k < b.zs() {
                k += r.next_int_n(b.zs());
                if k + 3 > b.zs() {
                    break;
                }
                let yy = b.y0 + r.next_int_n(hy) + 1;
                if let Some(c) = next(list, root, r, b.x1 + 1, yy, b.z0 + k, 3, ty) {
                    links.push(BB::new(b.x1 - 1, c.y0, c.z0, b.x1, c.y1, c.z1));
                }
                k += 4;
            }
            if let Kind::Mine(Mine::Room { links: l }) = &mut list[i].kind {
                *l = links;
            }
        }
        Kind::Mine(Mine::Corridor { .. }) => {
            let k = r.next_int_n(4);
            let m = p.mode;
            match m {
                0 => {
                    if k <= 1 {
                        let yy = b.y0 - 1 + r.next_int_n(3);
                        next(list, root, r, b.x0, yy, b.z1 + 1, m, ty);
                    } else if k == 2 {
                        let yy = b.y0 - 1 + r.next_int_n(3);
                        next(list, root, r, b.x0 - 1, yy, b.z1 - 3, 1, ty);
                    } else {
                        let yy = b.y0 - 1 + r.next_int_n(3);
                        next(list, root, r, b.x1 + 1, yy, b.z1 - 3, 3, ty);
                    }
                }
                1 => {
                    if k <= 1 {
                        let yy = b.y0 - 1 + r.next_int_n(3);
                        next(list, root, r, b.x0 - 1, yy, b.z0, m, ty);
                    } else if k == 2 {
                        let yy = b.y0 - 1 + r.next_int_n(3);
                        next(list, root, r, b.x0, yy, b.z0 - 1, 2, ty);
                    } else {
                        let yy = b.y0 - 1 + r.next_int_n(3);
                        next(list, root, r, b.x0, yy, b.z1 + 1, 0, ty);
                    }
                }
                2 => {
                    if k <= 1 {
                        let yy = b.y0 - 1 + r.next_int_n(3);
                        next(list, root, r, b.x0, yy, b.z0 - 1, m, ty);
                    } else if k == 2 {
                        let yy = b.y0 - 1 + r.next_int_n(3);
                        next(list, root, r, b.x0 - 1, yy, b.z0, 1, ty);
                    } else {
                        let yy = b.y0 - 1 + r.next_int_n(3);
                        next(list, root, r, b.x1 + 1, yy, b.z0, 3, ty);
                    }
                }
                3 => {
                    if k <= 1 {
                        let yy = b.y0 - 1 + r.next_int_n(3);
                        next(list, root, r, b.x1 + 1, yy, b.z0, m, ty);
                    } else if k == 2 {
                        let yy = b.y0 - 1 + r.next_int_n(3);
                        next(list, root, r, b.x1 - 3, yy, b.z0 - 1, 2, ty);
                    } else {
                        let yy = b.y0 - 1 + r.next_int_n(3);
                        next(list, root, r, b.x1 - 3, yy, b.z1 + 1, 0, ty);
                    }
                }
                _ => {}
            }
            if ty < 8 {
                if m != 2 && m != 0 {
                    let mut x = b.x0 + 3;
                    while x + 3 <= b.x1 {
                        let j = r.next_int_n(5);
                        if j == 0 {
                            next(list, root, r, x, b.y0, b.z0 - 1, 2, ty + 1);
                        } else if j == 1 {
                            next(list, root, r, x, b.y0, b.z1 + 1, 0, ty + 1);
                        }
                        x += 5;
                    }
                } else {
                    let mut z = b.z0 + 3;
                    while z + 3 <= b.z1 {
                        let j = r.next_int_n(5);
                        if j == 0 {
                            next(list, root, r, b.x0 - 1, b.y0, z, 1, ty + 1);
                        } else if j == 1 {
                            next(list, root, r, b.x1 + 1, b.y0, z, 3, ty + 1);
                        }
                        z += 5;
                    }
                }
            }
        }
        Kind::Mine(Mine::Cross { dir, multi }) => {
            let (dir, multi) = (*dir, *multi);
            match dir {
                0 => {
                    next(list, root, r, b.x0 + 1, b.y0, b.z1 + 1, 0, ty);
                    next(list, root, r, b.x0 - 1, b.y0, b.z0 + 1, 1, ty);
                    next(list, root, r, b.x1 + 1, b.y0, b.z0 + 1, 3, ty);
                }
                1 => {
                    next(list, root, r, b.x0 + 1, b.y0, b.z0 - 1, 2, ty);
                    next(list, root, r, b.x0 + 1, b.y0, b.z1 + 1, 0, ty);
                    next(list, root, r, b.x0 - 1, b.y0, b.z0 + 1, 1, ty);
                }
                2 => {
                    next(list, root, r, b.x0 + 1, b.y0, b.z0 - 1, 2, ty);
                    next(list, root, r, b.x0 - 1, b.y0, b.z0 + 1, 1, ty);
                    next(list, root, r, b.x1 + 1, b.y0, b.z0 + 1, 3, ty);
                }
                3 => {
                    next(list, root, r, b.x0 + 1, b.y0, b.z0 - 1, 2, ty);
                    next(list, root, r, b.x0 + 1, b.y0, b.z1 + 1, 0, ty);
                    next(list, root, r, b.x1 + 1, b.y0, b.z0 + 1, 3, ty);
                }
                _ => {}
            }
            if multi {
                if r.next_boolean() {
                    next(list, root, r, b.x0 + 1, b.y0 + 3 + 1, b.z0 - 1, 2, ty);
                }
                if r.next_boolean() {
                    next(list, root, r, b.x0 - 1, b.y0 + 3 + 1, b.z0 + 1, 1, ty);
                }
                if r.next_boolean() {
                    next(list, root, r, b.x1 + 1, b.y0 + 3 + 1, b.z0 + 1, 3, ty);
                }
                if r.next_boolean() {
                    next(list, root, r, b.x0 + 1, b.y0 + 3 + 1, b.z1 + 1, 0, ty);
                }
            }
        }
        Kind::Mine(Mine::Stairs) => match p.mode {
            0 => {
                next(list, root, r, b.x0, b.y0, b.z1 + 1, 0, ty);
            }
            1 => {
                next(list, root, r, b.x0 - 1, b.y0, b.z0, 1, ty);
            }
            2 => {
                next(list, root, r, b.x0, b.y0, b.z0 - 1, 2, ty);
            }
            3 => {
                next(list, root, r, b.x1 + 1, b.y0, b.z0, 3, ty);
            }
            _ => {}
        },
        _ => {}
    }
}

/// `Corridor.findValidPlacement`
fn corridor_placement(
    list: &[Piece],
    r: &mut JavaRandom,
    x: i32,
    y: i32,
    z: i32,
    mode: i32,
) -> Option<BB> {
    let mut bb = BB::new(x, y, z, x, y + 2, z);
    let mut n = r.next_int_n(3) + 2;
    while n > 0 {
        let len = n * 5;
        match mode {
            0 => {
                bb.x1 = x + 2;
                bb.z1 = z + (len - 1);
            }
            1 => {
                bb.x0 = x - (len - 1);
                bb.z1 = z + 2;
            }
            2 => {
                bb.x1 = x + 2;
                bb.z0 = z - (len - 1);
            }
            3 => {
                bb.x1 = x + (len - 1);
                bb.z1 = z + 2;
            }
            _ => {}
        }
        if find_intersecting(list, &bb).is_none() {
            break;
        }
        n -= 1;
    }
    if n > 0 {
        Some(bb)
    } else {
        None
    }
}

/// `Cross.findValidPlacement`
fn cross_placement(
    list: &[Piece],
    r: &mut JavaRandom,
    x: i32,
    y: i32,
    z: i32,
    mode: i32,
) -> Option<BB> {
    let mut bb = BB::new(x, y, z, x, y + 2, z);
    if r.next_int_n(4) == 0 {
        bb.y1 += 4;
    }
    match mode {
        0 => {
            bb.x0 = x - 1;
            bb.x1 = x + 3;
            bb.z1 = z + 4;
        }
        1 => {
            bb.x0 = x - 4;
            bb.z0 = z - 1;
            bb.z1 = z + 3;
        }
        2 => {
            bb.x0 = x - 1;
            bb.x1 = x + 3;
            bb.z0 = z - 4;
        }
        3 => {
            bb.x1 = x + 4;
            bb.z0 = z - 1;
            bb.z1 = z + 3;
        }
        _ => {}
    }
    if find_intersecting(list, &bb).is_some() {
        None
    } else {
        Some(bb)
    }
}

/// `Stairs.findValidPlacement`
fn stairs_placement(list: &[Piece], x: i32, y: i32, z: i32, mode: i32) -> Option<BB> {
    let mut bb = BB::new(x, y - 5, z, x, y + 2, z);
    match mode {
        0 => {
            bb.x1 = x + 2;
            bb.z1 = z + 8;
        }
        1 => {
            bb.x0 = x - 8;
            bb.z1 = z + 2;
        }
        2 => {
            bb.x1 = x + 2;
            bb.z0 = z - 8;
        }
        3 => {
            bb.x1 = x + 8;
            bb.z1 = z + 2;
        }
        _ => {}
    }
    if find_intersecting(list, &bb).is_some() {
        None
    } else {
        Some(bb)
    }
}

/// `Corridor.generateStructureChestContents`: a rail with a chest minecart.
#[allow(clippy::too_many_arguments)]
fn minecart_chest(
    p: &Piece,
    w: &mut ServerWorld,
    clip: &BB,
    r: &mut JavaRandom,
    x: i32,
    y: i32,
    z: i32,
    table: &[loot::Loot],
    count: i32,
) -> bool {
    let (wx, wy, wz) = (p.x_off(x, z), p.y_off(y), p.z_off(x, z));
    if clip.inside(wx, wy, wz) && w.is_air(wx, wy, wz) {
        let m = r.next_boolean() as i32;
        w.set_block(wx, wy, wz, block::RAIL, p.mo(block::RAIL, m), 2);
        let mut slots: Vec<Option<Stack>> = vec![None; 27];
        loot::fill(r, table, &mut slots, count);
        super::spawn(
            w,
            GenEntity::MinecartChest {
                pos: [wx as f64 + 0.5, wy as f64 + 0.5, wz as f64 + 0.5],
                slots,
            },
        );
        true
    } else {
        false
    }
}

/// `addComponentParts`
pub fn add_parts(p: &mut Piece, w: &mut ServerWorld, r: &mut JavaRandom, clip: &BB) -> bool {
    use block::*;
    let b = p.bb;
    match p.kind.clone() {
        Kind::Mine(Mine::Room { links }) => {
            if p.liquid_in(w, clip) {
                return false;
            }
            p.fill(w, clip, b.x0, b.y0, b.z0, b.x1, b.y0, b.z1, DIRT, AIR, true);
            p.fill(
                w,
                clip,
                b.x0,
                b.y0 + 1,
                b.z0,
                b.x1,
                (b.y0 + 3).min(b.y1),
                b.z1,
                AIR,
                AIR,
                false,
            );
            for l in &links {
                p.fill(
                    w,
                    clip,
                    l.x0,
                    l.y1 - 2,
                    l.z0,
                    l.x1,
                    l.y1,
                    l.z1,
                    AIR,
                    AIR,
                    false,
                );
            }
            p.hemisphere(w, clip, b.x0, b.y0 + 4, b.z0, b.x1, b.y1, b.z1, AIR, false);
            true
        }
        Kind::Mine(Mine::Corridor {
            rails,
            spiders,
            mut spawner,
            sections,
        }) => {
            if p.liquid_in(w, clip) {
                return false;
            }
            let len = sections * 5 - 1;
            p.fill(w, clip, 0, 0, 0, 2, 1, len, AIR, AIR, false);
            p.fill_chance(w, clip, r, 0.8, 0, 2, 0, 2, 2, len, AIR, AIR, false);
            if spiders {
                p.fill_chance(w, clip, r, 0.6, 0, 0, 0, 2, 1, len, WEB, AIR, false);
            }
            for s in 0..sections {
                let z = 2 + s * 5;
                p.fill(w, clip, 0, 0, z, 0, 1, z, FENCE, AIR, false);
                p.fill(w, clip, 2, 0, z, 2, 1, z, FENCE, AIR, false);
                if r.next_int_n(4) == 0 {
                    p.fill(w, clip, 0, 2, z, 0, 2, z, PLANKS, AIR, false);
                    p.fill(w, clip, 2, 2, z, 2, 2, z, PLANKS, AIR, false);
                } else {
                    p.fill(w, clip, 0, 2, z, 2, 2, z, PLANKS, AIR, false);
                }
                p.maybe(w, clip, r, 0.1, 0, 2, z - 1, WEB, 0);
                p.maybe(w, clip, r, 0.1, 2, 2, z - 1, WEB, 0);
                p.maybe(w, clip, r, 0.1, 0, 2, z + 1, WEB, 0);
                p.maybe(w, clip, r, 0.1, 2, 2, z + 1, WEB, 0);
                p.maybe(w, clip, r, 0.05, 0, 2, z - 2, WEB, 0);
                p.maybe(w, clip, r, 0.05, 2, 2, z - 2, WEB, 0);
                p.maybe(w, clip, r, 0.05, 0, 2, z + 2, WEB, 0);
                p.maybe(w, clip, r, 0.05, 2, 2, z + 2, WEB, 0);
                p.maybe(w, clip, r, 0.05, 1, 2, z - 1, TORCH, 0);
                p.maybe(w, clip, r, 0.05, 1, 2, z + 1, TORCH, 0);
                if r.next_int_n(100) == 0 {
                    let book = loot::enchanted_book(r, 1, 1, 1);
                    let table = loot::with(loot::MINESHAFT, book);
                    let n = 3 + r.next_int_n(4);
                    minecart_chest(p, w, clip, r, 2, 0, z - 1, &table, n);
                }
                if r.next_int_n(100) == 0 {
                    let book = loot::enchanted_book(r, 1, 1, 1);
                    let table = loot::with(loot::MINESHAFT, book);
                    let n = 3 + r.next_int_n(4);
                    minecart_chest(p, w, clip, r, 0, 0, z + 1, &table, n);
                }
                if spiders && !spawner {
                    let y = p.y_off(0);
                    let zz = z - 1 + r.next_int_n(3);
                    let x = p.x_off(1, zz);
                    let zz2 = p.z_off(1, zz);
                    if clip.inside(x, y, zz2) {
                        spawner = true;
                        p.spawner(w, x, y, zz2, "CaveSpider");
                    }
                }
            }
            for x in 0..=2 {
                for z in 0..=len {
                    if block::material(p.get(w, x, -1, z, clip)) == block::Material::Air {
                        p.pb(w, PLANKS, 0, x, -1, z, clip);
                    }
                }
            }
            if rails {
                for z in 0..=len {
                    let id = p.get(w, 1, -1, z, clip);
                    if block::material(id) != block::Material::Air && block::info(id).full_block {
                        let m = p.mo(RAIL, 0);
                        p.maybe(w, clip, r, 0.7, 1, 0, z, RAIL, m);
                    }
                }
            }
            p.kind = Kind::Mine(Mine::Corridor {
                rails,
                spiders,
                spawner,
                sections,
            });
            true
        }
        Kind::Mine(Mine::Cross { multi, .. }) => {
            if p.liquid_in(w, clip) {
                return false;
            }
            if multi {
                p.fill(
                    w,
                    clip,
                    b.x0 + 1,
                    b.y0,
                    b.z0,
                    b.x1 - 1,
                    b.y0 + 3 - 1,
                    b.z1,
                    AIR,
                    AIR,
                    false,
                );
                p.fill(
                    w,
                    clip,
                    b.x0,
                    b.y0,
                    b.z0 + 1,
                    b.x1,
                    b.y0 + 3 - 1,
                    b.z1 - 1,
                    AIR,
                    AIR,
                    false,
                );
                p.fill(
                    w,
                    clip,
                    b.x0 + 1,
                    b.y1 - 2,
                    b.z0,
                    b.x1 - 1,
                    b.y1,
                    b.z1,
                    AIR,
                    AIR,
                    false,
                );
                p.fill(
                    w,
                    clip,
                    b.x0,
                    b.y1 - 2,
                    b.z0 + 1,
                    b.x1,
                    b.y1,
                    b.z1 - 1,
                    AIR,
                    AIR,
                    false,
                );
                p.fill(
                    w,
                    clip,
                    b.x0 + 1,
                    b.y0 + 3,
                    b.z0 + 1,
                    b.x1 - 1,
                    b.y0 + 3,
                    b.z1 - 1,
                    AIR,
                    AIR,
                    false,
                );
            } else {
                p.fill(
                    w,
                    clip,
                    b.x0 + 1,
                    b.y0,
                    b.z0,
                    b.x1 - 1,
                    b.y1,
                    b.z1,
                    AIR,
                    AIR,
                    false,
                );
                p.fill(
                    w,
                    clip,
                    b.x0,
                    b.y0,
                    b.z0 + 1,
                    b.x1,
                    b.y1,
                    b.z1 - 1,
                    AIR,
                    AIR,
                    false,
                );
            }
            p.fill(
                w,
                clip,
                b.x0 + 1,
                b.y0,
                b.z0 + 1,
                b.x0 + 1,
                b.y1,
                b.z0 + 1,
                PLANKS,
                AIR,
                false,
            );
            p.fill(
                w,
                clip,
                b.x0 + 1,
                b.y0,
                b.z1 - 1,
                b.x0 + 1,
                b.y1,
                b.z1 - 1,
                PLANKS,
                AIR,
                false,
            );
            p.fill(
                w,
                clip,
                b.x1 - 1,
                b.y0,
                b.z0 + 1,
                b.x1 - 1,
                b.y1,
                b.z0 + 1,
                PLANKS,
                AIR,
                false,
            );
            p.fill(
                w,
                clip,
                b.x1 - 1,
                b.y0,
                b.z1 - 1,
                b.x1 - 1,
                b.y1,
                b.z1 - 1,
                PLANKS,
                AIR,
                false,
            );
            for x in b.x0..=b.x1 {
                for z in b.z0..=b.z1 {
                    if block::material(p.get(w, x, b.y0 - 1, z, clip)) == block::Material::Air {
                        p.pb(w, PLANKS, 0, x, b.y0 - 1, z, clip);
                    }
                }
            }
            true
        }
        Kind::Mine(Mine::Stairs) => {
            if p.liquid_in(w, clip) {
                return false;
            }
            p.fill(w, clip, 0, 5, 0, 2, 7, 1, AIR, AIR, false);
            p.fill(w, clip, 0, 0, 7, 2, 2, 8, AIR, AIR, false);
            for i in 0..5 {
                p.fill(
                    w,
                    clip,
                    0,
                    5 - i - if i < 4 { 1 } else { 0 },
                    2 + i,
                    2,
                    7 - i,
                    2 + i,
                    AIR,
                    AIR,
                    false,
                );
            }
            true
        }
        _ => true,
    }
}
