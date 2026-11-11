//! `MapGenVillage` and `StructureVillagePieces`.

use super::{biomes_viable, find_intersecting, rand_range, Kind, Piece, Start, BB};
use crate::block::{self, *};
use crate::java::JavaRandom;
use crate::world::gen::chunk_provider::ChunkProvider;
use crate::world::loot;
use crate::world::server::{GenEntity, ServerWorld};

/// `MapGenVillage.villageSpawnBiomes`: plains, desert, savanna.
const BIOMES: [i32; 3] = [1, 2, 35];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VKind {
    Well,
    Path { length: i32 },
    Torch,
    House1,
    House2 { chest: bool },
    House3,
    House4 { terrace: bool },
    Church,
    Hall,
    WoodHut { tall: bool, table: i32 },
    Field1 { crops: [u8; 4] },
    Field2 { crops: [u8; 2] },
}

/// `StructureVillagePieces$Village`
#[derive(Clone, Debug)]
pub struct VPiece {
    pub avg: i32,
    pub villagers: i32,
    pub desert: bool,
    pub v: VKind,
}

impl VPiece {
    pub fn save(&self, out: &mut Vec<i32>) {
        out.extend_from_slice(&[self.avg, self.villagers, self.desert as i32]);
        match self.v {
            VKind::Well => out.push(0),
            VKind::Path { length } => out.extend_from_slice(&[1, length]),
            VKind::Torch => out.push(2),
            VKind::House1 => out.push(3),
            VKind::House2 { chest } => out.extend_from_slice(&[4, chest as i32]),
            VKind::House3 => out.push(5),
            VKind::House4 { terrace } => out.extend_from_slice(&[6, terrace as i32]),
            VKind::Church => out.push(7),
            VKind::Hall => out.push(8),
            VKind::WoodHut { tall, table } => out.extend_from_slice(&[9, tall as i32, table]),
            VKind::Field1 { crops } => out.extend_from_slice(&[
                10,
                crops[0] as i32,
                crops[1] as i32,
                crops[2] as i32,
                crops[3] as i32,
            ]),
            VKind::Field2 { crops } => {
                out.extend_from_slice(&[11, crops[0] as i32, crops[1] as i32])
            }
        }
    }
    pub fn load(v: &mut std::slice::Iter<i32>) -> Option<VPiece> {
        let mut t = || v.next().copied();
        let avg = t()?;
        let villagers = t()?;
        let desert = t()? != 0;
        let k = match t()? {
            0 => VKind::Well,
            1 => VKind::Path { length: t()? },
            2 => VKind::Torch,
            3 => VKind::House1,
            4 => VKind::House2 { chest: t()? != 0 },
            5 => VKind::House3,
            6 => VKind::House4 { terrace: t()? != 0 },
            7 => VKind::Church,
            8 => VKind::Hall,
            9 => VKind::WoodHut {
                tall: t()? != 0,
                table: t()?,
            },
            10 => VKind::Field1 {
                crops: [t()? as u8, t()? as u8, t()? as u8, t()? as u8],
            },
            _ => VKind::Field2 {
                crops: [t()? as u8, t()? as u8],
            },
        };
        Some(VPiece {
            avg,
            villagers,
            desert,
            v: k,
        })
    }
}

/// Piece classes in `getStructureVillageWeightedPieceList` order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class {
    House4,
    Church,
    House1,
    WoodHut,
    Hall,
    Field1,
    Field2,
    House2,
    House3,
}

/// `StructureVillagePieces$PieceWeight`
#[derive(Clone, Copy, Debug)]
struct Weight {
    class: Class,
    weight: i32,
    spawned: i32,
    limit: i32,
}

impl Weight {
    fn can_spawn_more(&self) -> bool {
        self.limit == 0 || self.spawned < self.limit
    }
}

/// Build-time state kept by `StructureVillagePieces$Start`.
struct Build<'a> {
    provider: &'a ChunkProvider,
    weights: Vec<Weight>,
    last: Option<Class>,
    houses: Vec<usize>,
    roads: Vec<usize>,
    terrain: i32,
    desert: bool,
    start_bb: BB,
}

/// `MapGenVillage.canSpawnStructureAtCoords`
pub fn can_spawn(provider: &ChunkProvider, seed: i64, x: i32, z: i32) -> bool {
    let (dist, sep) = (32, 8);
    let (mut gx, mut gz) = (x, z);
    if gx < 0 {
        gx -= dist - 1;
    }
    if gz < 0 {
        gz -= dist - 1;
    }
    let (mut px, mut pz) = (gx / dist, gz / dist);
    let mut r = JavaRandom::new(0);
    r.set_seed(
        (px as i64)
            .wrapping_mul(341873128712)
            .wrapping_add((pz as i64).wrapping_mul(132897987541))
            .wrapping_add(seed)
            .wrapping_add(10387312),
    );
    px *= dist;
    pz *= dist;
    px += r.next_int_n(dist - sep);
    pz += r.next_int_n(dist - sep);
    x == px && z == pz && biomes_viable(provider, x * 16 + 8, z * 16 + 8, 0, &BIOMES)
}

/// `MapGenVillage$Start`
pub fn start(provider: &ChunkProvider, r: &mut JavaRandom, cx: i32, cz: i32) -> Start {
    let terrain = 0;
    let mut weights = Vec::new();
    for (class, weight, lo, hi) in [
        (Class::House4, 4, 2 + terrain, 4 + terrain * 2),
        (Class::Church, 20, terrain, 1 + terrain),
        (Class::House1, 20, terrain, 2 + terrain),
        (Class::WoodHut, 3, 2 + terrain, 5 + terrain * 3),
        (Class::Hall, 15, terrain, 2 + terrain),
        (Class::Field1, 3, 1 + terrain, 4 + terrain),
        (Class::Field2, 3, 2 + terrain, 4 + terrain * 2),
        (Class::House2, 15, 0, 1 + terrain),
        (Class::House3, 8, terrain, 3 + terrain * 2),
    ] {
        let limit = rand_range(r, lo, hi);
        weights.push(Weight {
            class,
            weight,
            spawned: 0,
            limit,
        });
    }
    weights.retain(|w| w.limit != 0);
    let (x, z) = ((cx << 4) + 2, (cz << 4) + 2);
    // StructureVillagePieces$Start -> Well(null, 0, rand, x, z)
    let mode = r.next_int_n(4);
    let bb = BB::new(x, 64, z, x + 6 - 1, 78, z + 6 - 1);
    let biome = provider.biome_at(x, z);
    let desert = biome == 2 || biome == 17;
    // The start piece is built with a null start, so it is never a desert piece.
    let well = Piece::new(
        0,
        mode,
        bb,
        Kind::Village(VPiece {
            avg: -1,
            villagers: 0,
            desert: false,
            v: VKind::Well,
        }),
    );
    let mut list = vec![well];
    let mut b = Build {
        provider,
        weights,
        last: None,
        houses: Vec::new(),
        roads: Vec::new(),
        terrain,
        desert,
        start_bb: bb,
    };
    build(&mut list, 0, &mut b, r);
    while !b.roads.is_empty() || !b.houses.is_empty() {
        if b.roads.is_empty() {
            let i = r.next_int_n(b.houses.len() as i32) as usize;
            let p = b.houses.remove(i);
            build(&mut list, p, &mut b, r);
        } else {
            let i = r.next_int_n(b.roads.len() as i32) as usize;
            let p = b.roads.remove(i);
            build(&mut list, p, &mut b, r);
        }
    }
    let non_roads = list
        .iter()
        .filter(|p| !matches!(&p.kind, Kind::Village(v) if matches!(v.v, VKind::Path { .. })))
        .count();
    let mut s = Start {
        cx,
        cz,
        bb: BB::default(),
        pieces: list,
        sizeable: non_roads > 2,
    };
    s.update_bb();
    s
}

fn vp(b: &Build, v: VKind) -> Kind {
    Kind::Village(VPiece {
        avg: -1,
        villagers: 0,
        desert: b.desert,
        v,
    })
}

/// `createPiece` (the static `of` / `create` factories).
#[allow(clippy::too_many_arguments)]
fn create(
    b: &Build,
    class: Class,
    list: &[Piece],
    r: &mut JavaRandom,
    x: i32,
    y: i32,
    z: i32,
    mode: i32,
    ty: i32,
) -> Option<Piece> {
    let (w, h, d) = match class {
        Class::House4 => (5, 6, 5),
        Class::Church => (5, 12, 9),
        Class::House1 => (9, 9, 6),
        Class::WoodHut => (4, 6, 5),
        Class::Hall => (9, 7, 11),
        Class::Field1 => (13, 4, 9),
        Class::Field2 => (7, 4, 9),
        Class::House2 => (10, 6, 7),
        Class::House3 => (9, 7, 12),
    };
    let bb = BB::component(x, y, z, 0, 0, 0, w, h, d, mode);
    let deep = bb.y0 > 10;
    if (class != Class::House4 && !deep) || find_intersecting(list, &bb).is_some() {
        return None;
    }
    let crop = |r: &mut JavaRandom| match r.next_int_n(5) {
        0 => CARROTS,
        1 => POTATOES,
        _ => WHEAT,
    };
    let v = match class {
        Class::House4 => VKind::House4 {
            terrace: r.next_boolean(),
        },
        Class::Church => VKind::Church,
        Class::House1 => VKind::House1,
        Class::WoodHut => {
            let tall = r.next_boolean();
            VKind::WoodHut {
                tall,
                table: r.next_int_n(3),
            }
        }
        Class::Hall => VKind::Hall,
        Class::Field1 => VKind::Field1 {
            crops: [crop(r), crop(r), crop(r), crop(r)],
        },
        Class::Field2 => VKind::Field2 {
            crops: [crop(r), crop(r)],
        },
        Class::House2 => VKind::House2 { chest: false },
        Class::House3 => VKind::House3,
    };
    Some(Piece::new(ty, mode, bb, vp(b, v)))
}

/// `getNextVillageComponent`
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
    if !any || total <= 0 {
        return None;
    }
    for _ in 0..5 {
        let mut k = r.next_int_n(total);
        let mut i = 0;
        while i < b.weights.len() {
            let pw = b.weights[i];
            k -= pw.weight;
            if k < 0 {
                if !pw.can_spawn_more() || (Some(pw.class) == b.last && b.weights.len() > 1) {
                    break;
                }
                if let Some(p) = create(b, pw.class, list, r, x, y, z, mode, ty) {
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
    // StructureVillagePieces$Torch.findSize
    let bb = BB::component(x, y, z, 0, 0, 0, 3, 4, 2, mode);
    if find_intersecting(list, &bb).is_some() {
        None
    } else {
        Some(Piece::new(ty, mode, bb, vp(b, VKind::Torch)))
    }
}

fn biome_ok(b: &Build, bb: &BB) -> bool {
    let cx = (bb.x0 + bb.x1) / 2;
    let cz = (bb.z0 + bb.z1) / 2;
    let sx = bb.x1 - bb.x0;
    let sz = bb.z1 - bb.z0;
    let s = if sx > sz { sx } else { sz };
    biomes_viable(b.provider, cx, cz, s / 2 + 4, &BIOMES)
}

/// `getNextVillageStructureComponent`
#[allow(clippy::too_many_arguments)]
fn next_house(
    list: &mut Vec<Piece>,
    b: &mut Build,
    r: &mut JavaRandom,
    x: i32,
    y: i32,
    z: i32,
    mode: i32,
    ty: i32,
) -> Option<BB> {
    if ty > 50 {
        return None;
    }
    if (x - b.start_bb.x0).abs() > 112 || (z - b.start_bb.z0).abs() > 112 {
        return None;
    }
    let p = next_component(list, b, r, x, y, z, mode, ty + 1)?;
    if biome_ok(b, &p.bb) {
        let bb = p.bb;
        list.push(p);
        b.houses.push(list.len() - 1);
        return Some(bb);
    }
    None
}

/// `getNextComponentVillagePath`
#[allow(clippy::too_many_arguments)]
fn next_path(
    list: &mut Vec<Piece>,
    b: &mut Build,
    r: &mut JavaRandom,
    x: i32,
    y: i32,
    z: i32,
    mode: i32,
    ty: i32,
) -> Option<BB> {
    if ty > 3 + b.terrain {
        return None;
    }
    if (x - b.start_bb.x0).abs() > 112 || (z - b.start_bb.z0).abs() > 112 {
        return None;
    }
    // Path.findSize
    let mut len = 7 * rand_range(r, 3, 5);
    let mut found = None;
    while len >= 7 {
        let bb = BB::component(x, y, z, 0, 0, 0, 3, 3, len, mode);
        if find_intersecting(list, &bb).is_none() {
            found = Some(bb);
            break;
        }
        len -= 7;
    }
    let bb = found?;
    if bb.y0 <= 10 {
        return None;
    }
    let length = bb.xs().max(bb.zs());
    let p = Piece::new(ty, mode, bb, vp(b, VKind::Path { length }));
    if biome_ok(b, &bb) {
        list.push(p);
        b.roads.push(list.len() - 1);
        return Some(bb);
    }
    None
}

/// `getNextComponentNN`
fn nn(
    list: &mut Vec<Piece>,
    i: usize,
    b: &mut Build,
    r: &mut JavaRandom,
    dy: i32,
    off: i32,
) -> Option<BB> {
    let p = &list[i];
    let (bb, ty) = (p.bb, p.ty);
    match p.mode {
        0 | 2 => next_house(list, b, r, bb.x0 - 1, bb.y0 + dy, bb.z0 + off, 1, ty),
        1 | 3 => next_house(list, b, r, bb.x0 + off, bb.y0 + dy, bb.z0 - 1, 2, ty),
        _ => None,
    }
}

/// `getNextComponentPP`
fn pp(
    list: &mut Vec<Piece>,
    i: usize,
    b: &mut Build,
    r: &mut JavaRandom,
    dy: i32,
    off: i32,
) -> Option<BB> {
    let p = &list[i];
    let (bb, ty) = (p.bb, p.ty);
    match p.mode {
        0 | 2 => next_house(list, b, r, bb.x1 + 1, bb.y0 + dy, bb.z0 + off, 3, ty),
        1 | 3 => next_house(list, b, r, bb.x0 + off, bb.y0 + dy, bb.z1 + 1, 0, ty),
        _ => None,
    }
}

/// `buildComponent`
fn build(list: &mut Vec<Piece>, i: usize, b: &mut Build, r: &mut JavaRandom) {
    let (bb, ty, mode) = (list[i].bb, list[i].ty, list[i].mode);
    let v = match &list[i].kind {
        Kind::Village(v) => v.v,
        _ => return,
    };
    match v {
        VKind::Well => {
            next_path(list, b, r, bb.x0 - 1, bb.y1 - 4, bb.z0 + 1, 1, ty);
            next_path(list, b, r, bb.x1 + 1, bb.y1 - 4, bb.z0 + 1, 3, ty);
            next_path(list, b, r, bb.x0 + 1, bb.y1 - 4, bb.z0 - 1, 2, ty);
            next_path(list, b, r, bb.x0 + 1, bb.y1 - 4, bb.z1 + 1, 0, ty);
        }
        VKind::Path { length } => {
            let mut any = false;
            let mut k = r.next_int_n(5);
            while k < length - 8 {
                if let Some(c) = nn(list, i, b, r, 0, k) {
                    k += c.xs().max(c.zs());
                    any = true;
                }
                k += 2 + r.next_int_n(5);
            }
            let mut k = r.next_int_n(5);
            while k < length - 8 {
                if let Some(c) = pp(list, i, b, r, 0, k) {
                    k += c.xs().max(c.zs());
                    any = true;
                }
                k += 2 + r.next_int_n(5);
            }
            if any && r.next_int_n(3) > 0 {
                match mode {
                    0 => next_path(list, b, r, bb.x0 - 1, bb.y0, bb.z1 - 2, 1, ty),
                    1 => next_path(list, b, r, bb.x0, bb.y0, bb.z0 - 1, 2, ty),
                    2 => next_path(list, b, r, bb.x0 - 1, bb.y0, bb.z0, 1, ty),
                    3 => next_path(list, b, r, bb.x1 - 2, bb.y0, bb.z0 - 1, 2, ty),
                    _ => None,
                };
            }
            if any && r.next_int_n(3) > 0 {
                match mode {
                    0 => next_path(list, b, r, bb.x1 + 1, bb.y0, bb.z1 - 2, 3, ty),
                    1 => next_path(list, b, r, bb.x0, bb.y0, bb.z1 + 1, 0, ty),
                    2 => next_path(list, b, r, bb.x1 + 1, bb.y0, bb.z0, 3, ty),
                    3 => next_path(list, b, r, bb.x1 - 2, bb.y0, bb.z1 + 1, 0, ty),
                    _ => None,
                };
            }
        }
        _ => {}
    }
}

// ------------------------------------------------------------------ placement

struct V<'a> {
    p: &'a Piece,
    desert: bool,
}

impl V<'_> {
    /// `updateBlock` / `updateBlockMetadata`
    fn sub(&self, id: u8, meta: i32) -> (u8, i32) {
        if !self.desert {
            return (id, meta);
        }
        match id {
            LOG | LOG2 | COBBLESTONE | GRAVEL => (SANDSTONE, if id == GRAVEL { meta } else { 0 }),
            PLANKS => (SANDSTONE, 2),
            OAK_STAIRS | STONE_STAIRS => (SANDSTONE_STAIRS, meta),
            _ => (id, meta),
        }
    }
    fn pb(&self, w: &mut ServerWorld, id: u8, meta: i32, x: i32, y: i32, z: i32, clip: &BB) {
        let (id, meta) = self.sub(id, meta);
        self.p.pb(w, id, meta, x, y, z, clip);
    }
    #[allow(clippy::too_many_arguments)]
    fn fill(
        &self,
        w: &mut ServerWorld,
        clip: &BB,
        x0: i32,
        y0: i32,
        z0: i32,
        x1: i32,
        y1: i32,
        z1: i32,
        edge: u8,
        inner: u8,
    ) {
        let (e, em) = self.sub(edge, 0);
        let (i, im) = self.sub(inner, 0);
        self.p
            .fill_meta(w, clip, x0, y0, z0, x1, y1, z1, e, em, i, im, false);
    }
    fn down(&self, w: &mut ServerWorld, id: u8, x: i32, y: i32, z: i32, clip: &BB) {
        let (id, m) = self.sub(id, 0);
        self.p.fill_down(w, id, m, x, y, z, clip);
    }
    /// The stone step in front of a door.
    fn step(&self, w: &mut ServerWorld, x: i32, clip: &BB) {
        if block::material(self.p.get(w, x, 0, -1, clip)) == Material::Air
            && block::material(self.p.get(w, x, -1, -1, clip)) != Material::Air
        {
            self.pb(w, STONE_STAIRS, self.p.mo(STONE_STAIRS, 3), x, 0, -1, clip);
        }
    }
    fn foundation(
        &self,
        w: &mut ServerWorld,
        clip: &BB,
        x0: i32,
        x1: i32,
        z0: i32,
        z1: i32,
        top: i32,
    ) {
        for z in z0..z1 {
            for x in x0..x1 {
                self.p.clear_up(w, x, top, z, clip);
                self.down(w, COBBLESTONE, x, -1, z, clip);
            }
        }
    }
}

/// `getAverageGroundLevel`
fn average_ground(p: &Piece, w: &mut ServerWorld, clip: &BB) -> i32 {
    let (mut sum, mut n) = (0, 0);
    for z in p.bb.z0..=p.bb.z1 {
        for x in p.bb.x0..=p.bb.x1 {
            if clip.inside(x, 64, z) {
                sum += w.top_solid_or_liquid(x, z).max(64);
                n += 1;
            }
        }
    }
    if n == 0 {
        -1
    } else {
        sum / n
    }
}

/// `getVillagerType`
fn profession(v: VKind, i: i32) -> i32 {
    match v {
        VKind::House1 => 1,
        VKind::House2 { .. } => 3,
        VKind::Church => 2,
        VKind::Hall => {
            if i == 0 {
                4
            } else {
                0
            }
        }
        _ => 0,
    }
}

/// `spawnVillagers`
fn spawn_villagers(
    p: &mut Piece,
    w: &mut ServerWorld,
    clip: &BB,
    x: i32,
    y: i32,
    z: i32,
    count: i32,
) {
    let Kind::Village(vp) = &p.kind else { return };
    let (mut spawned, v) = (vp.villagers, vp.v);
    let start = spawned;
    for i in start..count {
        let (wx, wy, wz) = (p.x_off(x + i, z), p.y_off(y), p.z_off(x + i, z));
        if !clip.inside(wx, wy, wz) {
            break;
        }
        spawned += 1;
        super::spawn(
            w,
            GenEntity::Villager {
                pos: [wx as f64 + 0.5, wy as f64, wz as f64 + 0.5],
                profession: profession(v, i),
            },
        );
    }
    if let Kind::Village(vp) = &mut p.kind {
        vp.villagers = spawned;
    }
}

/// `addComponentParts`
pub fn add_parts(p: &mut Piece, w: &mut ServerWorld, r: &mut JavaRandom, clip: &BB) -> bool {
    let Kind::Village(vpc) = p.kind.clone() else {
        return true;
    };
    // Height adjustment shared by every piece except roads.
    let height = match vpc.v {
        VKind::Well => Some(3),
        VKind::Torch => Some(4 - 1),
        VKind::House1 => Some(9 - 1),
        VKind::House2 { .. } => Some(6 - 1),
        VKind::House3 => Some(7 - 1),
        VKind::House4 { .. } => Some(6 - 1),
        VKind::Church => Some(12 - 1),
        VKind::Hall => Some(7 - 1),
        VKind::WoodHut { .. } => Some(6 - 1),
        VKind::Field1 { .. } | VKind::Field2 { .. } => Some(4 - 1),
        VKind::Path { .. } => None,
    };
    if let Some(h) = height {
        if vpc.avg < 0 {
            let avg = average_ground(p, w, clip);
            if let Kind::Village(v) = &mut p.kind {
                v.avg = avg;
            }
            if avg < 0 {
                return true;
            }
            let dy = avg - p.bb.y1 + h;
            p.bb.offset(0, dy, 0);
        }
    }
    let piece = p.clone();
    let v = V {
        p: &piece,
        desert: vpc.desert,
    };
    let q = &piece;
    match vpc.v {
        VKind::Path { .. } => {
            let (id, _) = v.sub(GRAVEL, 0);
            for x in q.bb.x0..=q.bb.x1 {
                for z in q.bb.z0..=q.bb.z1 {
                    if clip.inside(x, 64, z) {
                        let y = w.top_solid_or_liquid(x, z) - 1;
                        w.set_block(x, y, z, id, 0, 2);
                    }
                }
            }
        }
        VKind::Well => {
            v.fill(w, clip, 1, 0, 1, 4, 12, 4, COBBLESTONE, FLOWING_WATER);
            for (x, z) in [(2, 2), (3, 2), (2, 3), (3, 3)] {
                v.pb(w, AIR, 0, x, 12, z, clip);
            }
            for (x, z) in [(1, 1), (4, 1), (1, 4), (4, 4)] {
                v.pb(w, FENCE, 0, x, 13, z, clip);
                v.pb(w, FENCE, 0, x, 14, z, clip);
            }
            v.fill(w, clip, 1, 15, 1, 4, 15, 4, COBBLESTONE, COBBLESTONE);
            for z in 0..=5 {
                for x in 0..=5 {
                    if x == 0 || x == 5 || z == 0 || z == 5 {
                        v.pb(w, GRAVEL, 0, x, 11, z, clip);
                        q.clear_up(w, x, 12, z, clip);
                    }
                }
            }
        }
        VKind::Torch => {
            v.fill(w, clip, 0, 0, 0, 2, 3, 1, AIR, AIR);
            v.pb(w, FENCE, 0, 1, 0, 0, clip);
            v.pb(w, FENCE, 0, 1, 1, 0, clip);
            v.pb(w, FENCE, 0, 1, 2, 0, clip);
            v.pb(w, WOOL, 15, 1, 3, 0, clip);
            v.pb(w, TORCH, 0, 0, 3, 0, clip);
            v.pb(w, TORCH, 0, 1, 3, 1, clip);
            v.pb(w, TORCH, 0, 2, 3, 0, clip);
            v.pb(w, TORCH, 0, 1, 3, -1, clip);
        }
        VKind::House1 => {
            v.fill(w, clip, 1, 1, 1, 7, 5, 4, AIR, AIR);
            v.fill(w, clip, 0, 0, 0, 8, 0, 5, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 0, 5, 0, 8, 5, 5, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 0, 6, 1, 8, 6, 4, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 0, 7, 2, 8, 7, 3, COBBLESTONE, COBBLESTONE);
            let (a, b2) = (q.mo(OAK_STAIRS, 3), q.mo(OAK_STAIRS, 2));
            for d in -1..=2 {
                for x in 0..=8 {
                    v.pb(w, OAK_STAIRS, a, x, 6 + d, d, clip);
                    v.pb(w, OAK_STAIRS, b2, x, 6 + d, 5 - d, clip);
                }
            }
            v.fill(w, clip, 0, 1, 0, 0, 1, 5, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 1, 1, 5, 8, 1, 5, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 8, 1, 0, 8, 1, 4, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 2, 1, 0, 7, 1, 0, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 0, 2, 0, 0, 4, 0, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 0, 2, 5, 0, 4, 5, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 8, 2, 5, 8, 4, 5, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 8, 2, 0, 8, 4, 0, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 0, 2, 1, 0, 4, 4, PLANKS, PLANKS);
            v.fill(w, clip, 1, 2, 5, 7, 4, 5, PLANKS, PLANKS);
            v.fill(w, clip, 8, 2, 1, 8, 4, 4, PLANKS, PLANKS);
            v.fill(w, clip, 1, 2, 0, 7, 4, 0, PLANKS, PLANKS);
            for (x, y, z) in [
                (4, 2, 0),
                (5, 2, 0),
                (6, 2, 0),
                (4, 3, 0),
                (5, 3, 0),
                (6, 3, 0),
                (0, 2, 2),
                (0, 2, 3),
                (0, 3, 2),
                (0, 3, 3),
                (8, 2, 2),
                (8, 2, 3),
                (8, 3, 2),
                (8, 3, 3),
                (2, 2, 5),
                (3, 2, 5),
                (5, 2, 5),
                (6, 2, 5),
            ] {
                v.pb(w, GLASS_PANE, 0, x, y, z, clip);
            }
            v.fill(w, clip, 1, 4, 1, 7, 4, 1, PLANKS, PLANKS);
            v.fill(w, clip, 1, 4, 4, 7, 4, 4, PLANKS, PLANKS);
            v.fill(w, clip, 1, 3, 4, 7, 3, 4, BOOKSHELF, BOOKSHELF);
            v.pb(w, PLANKS, 0, 7, 1, 4, clip);
            v.pb(w, OAK_STAIRS, q.mo(OAK_STAIRS, 0), 7, 1, 3, clip);
            let s = q.mo(OAK_STAIRS, 3);
            for x in [6, 5, 4, 3] {
                v.pb(w, OAK_STAIRS, s, x, 1, 4, clip);
            }
            v.pb(w, FENCE, 0, 6, 1, 3, clip);
            v.pb(w, WOODEN_PRESSURE_PLATE, 0, 6, 2, 3, clip);
            v.pb(w, FENCE, 0, 4, 1, 3, clip);
            v.pb(w, WOODEN_PRESSURE_PLATE, 0, 4, 2, 3, clip);
            v.pb(w, CRAFTING_TABLE, 0, 7, 1, 1, clip);
            v.pb(w, AIR, 0, 1, 1, 0, clip);
            v.pb(w, AIR, 0, 1, 2, 0, clip);
            q.door(w, clip, 1, 1, 0, q.mo(WOODEN_DOOR, 1));
            v.step(w, 1, clip);
            v.foundation(w, clip, 0, 9, 0, 6, 9);
            spawn_villagers(p, w, clip, 2, 1, 2, 1);
        }
        VKind::House2 { chest } => {
            v.fill(w, clip, 0, 1, 0, 9, 4, 6, AIR, AIR);
            v.fill(w, clip, 0, 0, 0, 9, 0, 6, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 0, 4, 0, 9, 4, 6, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 0, 5, 0, 9, 5, 6, STONE_SLAB, STONE_SLAB);
            v.fill(w, clip, 1, 5, 1, 8, 5, 5, AIR, AIR);
            v.fill(w, clip, 1, 1, 0, 2, 3, 0, PLANKS, PLANKS);
            v.fill(w, clip, 0, 1, 0, 0, 4, 0, LOG, LOG);
            v.fill(w, clip, 3, 1, 0, 3, 4, 0, LOG, LOG);
            v.fill(w, clip, 0, 1, 6, 0, 4, 6, LOG, LOG);
            v.pb(w, PLANKS, 0, 3, 3, 1, clip);
            v.fill(w, clip, 3, 1, 2, 3, 3, 2, PLANKS, PLANKS);
            v.fill(w, clip, 4, 1, 3, 5, 3, 3, PLANKS, PLANKS);
            v.fill(w, clip, 0, 1, 1, 0, 3, 5, PLANKS, PLANKS);
            v.fill(w, clip, 1, 1, 6, 5, 3, 6, PLANKS, PLANKS);
            v.fill(w, clip, 5, 1, 0, 5, 3, 0, FENCE, FENCE);
            v.fill(w, clip, 9, 1, 0, 9, 3, 0, FENCE, FENCE);
            v.fill(w, clip, 6, 1, 4, 9, 4, 6, COBBLESTONE, COBBLESTONE);
            v.pb(w, FLOWING_LAVA, 0, 7, 1, 5, clip);
            v.pb(w, FLOWING_LAVA, 0, 8, 1, 5, clip);
            v.pb(w, IRON_BARS, 0, 9, 2, 5, clip);
            v.pb(w, IRON_BARS, 0, 9, 2, 4, clip);
            v.fill(w, clip, 7, 2, 4, 8, 2, 5, AIR, AIR);
            v.pb(w, COBBLESTONE, 0, 6, 1, 3, clip);
            v.pb(w, FURNACE, 0, 6, 2, 3, clip);
            v.pb(w, FURNACE, 0, 6, 3, 3, clip);
            v.pb(w, DOUBLE_STONE_SLAB, 0, 8, 1, 1, clip);
            for (x, z) in [(0, 2), (0, 4), (2, 6), (4, 6)] {
                v.pb(w, GLASS_PANE, 0, x, 2, z, clip);
            }
            v.pb(w, FENCE, 0, 2, 1, 4, clip);
            v.pb(w, WOODEN_PRESSURE_PLATE, 0, 2, 2, 4, clip);
            v.pb(w, PLANKS, 0, 1, 1, 5, clip);
            v.pb(w, OAK_STAIRS, q.mo(OAK_STAIRS, 3), 2, 1, 5, clip);
            v.pb(w, OAK_STAIRS, q.mo(OAK_STAIRS, 1), 1, 1, 4, clip);
            let mut made = chest;
            if !made {
                let (cy, cx, cz) = (q.y_off(1), q.x_off(5, 5), q.z_off(5, 5));
                if clip.inside(cx, cy, cz) {
                    made = true;
                    let n = 3 + r.next_int_n(6);
                    q.chest(w, clip, r, 5, 1, 5, loot::BLACKSMITH, n);
                }
            }
            for x in 6..=8 {
                v.step(w, x, clip);
            }
            v.foundation(w, clip, 0, 10, 0, 7, 6);
            if let Kind::Village(vv) = &mut p.kind {
                vv.v = VKind::House2 { chest: made };
            }
            spawn_villagers(p, w, clip, 7, 1, 1, 1);
        }
        VKind::House3 => {
            v.fill(w, clip, 1, 1, 1, 7, 4, 4, AIR, AIR);
            v.fill(w, clip, 2, 1, 6, 8, 4, 10, AIR, AIR);
            v.fill(w, clip, 2, 0, 5, 8, 0, 10, PLANKS, PLANKS);
            v.fill(w, clip, 1, 0, 1, 7, 0, 4, PLANKS, PLANKS);
            v.fill(w, clip, 0, 0, 0, 0, 3, 5, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 8, 0, 0, 8, 3, 10, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 1, 0, 0, 7, 2, 0, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 1, 0, 5, 2, 1, 5, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 2, 0, 6, 2, 3, 10, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 3, 0, 10, 7, 3, 10, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 1, 2, 0, 7, 3, 0, PLANKS, PLANKS);
            v.fill(w, clip, 1, 2, 5, 2, 3, 5, PLANKS, PLANKS);
            v.fill(w, clip, 0, 4, 1, 8, 4, 1, PLANKS, PLANKS);
            v.fill(w, clip, 0, 4, 4, 3, 4, 4, PLANKS, PLANKS);
            v.fill(w, clip, 0, 5, 2, 8, 5, 3, PLANKS, PLANKS);
            for (x, y, z) in [(0, 4, 2), (0, 4, 3), (8, 4, 2), (8, 4, 3), (8, 4, 4)] {
                v.pb(w, PLANKS, 0, x, y, z, clip);
            }
            let (a, b2) = (q.mo(OAK_STAIRS, 3), q.mo(OAK_STAIRS, 2));
            for d in -1..=2 {
                for x in 0..=8 {
                    v.pb(w, OAK_STAIRS, a, x, 4 + d, d, clip);
                    if (d > -1 || x <= 1) && (d > 0 || x <= 3) && (d > 1 || x <= 4 || x >= 6) {
                        v.pb(w, OAK_STAIRS, b2, x, 4 + d, 5 - d, clip);
                    }
                }
            }
            v.fill(w, clip, 3, 4, 5, 3, 4, 10, PLANKS, PLANKS);
            v.fill(w, clip, 7, 4, 2, 7, 4, 10, PLANKS, PLANKS);
            v.fill(w, clip, 4, 5, 4, 4, 5, 10, PLANKS, PLANKS);
            v.fill(w, clip, 6, 5, 4, 6, 5, 10, PLANKS, PLANKS);
            v.fill(w, clip, 5, 6, 3, 5, 6, 10, PLANKS, PLANKS);
            let s0 = q.mo(OAK_STAIRS, 0);
            for k in (1..=4).rev() {
                v.pb(w, PLANKS, 0, k, 2 + k, 7 - k, clip);
                for z in (8 - k)..=10 {
                    v.pb(w, OAK_STAIRS, s0, k, 2 + k, z, clip);
                }
            }
            let s1 = q.mo(OAK_STAIRS, 1);
            v.pb(w, PLANKS, 0, 6, 6, 3, clip);
            v.pb(w, PLANKS, 0, 7, 5, 4, clip);
            v.pb(w, OAK_STAIRS, s1, 6, 6, 4, clip);
            for x in 6..=8 {
                for z in 5..=10 {
                    v.pb(w, OAK_STAIRS, s1, x, 12 - x, z, clip);
                }
            }
            let blocks: [(u8, i32, i32, i32); 22] = [
                (LOG, 0, 2, 1),
                (LOG, 0, 2, 4),
                (GLASS_PANE, 0, 2, 2),
                (GLASS_PANE, 0, 2, 3),
                (LOG, 4, 2, 0),
                (GLASS_PANE, 5, 2, 0),
                (LOG, 6, 2, 0),
                (LOG, 8, 2, 1),
                (GLASS_PANE, 8, 2, 2),
                (GLASS_PANE, 8, 2, 3),
                (LOG, 8, 2, 4),
                (PLANKS, 8, 2, 5),
                (LOG, 8, 2, 6),
                (GLASS_PANE, 8, 2, 7),
                (GLASS_PANE, 8, 2, 8),
                (LOG, 8, 2, 9),
                (LOG, 2, 2, 6),
                (GLASS_PANE, 2, 2, 7),
                (GLASS_PANE, 2, 2, 8),
                (LOG, 2, 2, 9),
                (LOG, 4, 4, 10),
                (GLASS_PANE, 5, 4, 10),
            ];
            for (id, x, y, z) in blocks {
                v.pb(w, id, 0, x, y, z, clip);
            }
            v.pb(w, LOG, 0, 6, 4, 10, clip);
            v.pb(w, PLANKS, 0, 5, 5, 10, clip);
            v.pb(w, AIR, 0, 2, 1, 0, clip);
            v.pb(w, AIR, 0, 2, 2, 0, clip);
            v.pb(w, TORCH, 0, 2, 3, 1, clip);
            q.door(w, clip, 2, 1, 0, q.mo(WOODEN_DOOR, 1));
            v.fill(w, clip, 1, 0, -1, 3, 2, -1, AIR, AIR);
            v.step(w, 2, clip);
            v.foundation(w, clip, 0, 9, 0, 5, 7);
            v.foundation(w, clip, 2, 9, 5, 11, 7);
            spawn_villagers(p, w, clip, 4, 1, 2, 2);
        }
        VKind::House4 { terrace } => {
            v.fill(w, clip, 0, 0, 0, 4, 0, 4, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 0, 4, 0, 4, 4, 4, LOG, LOG);
            v.fill(w, clip, 1, 4, 1, 3, 4, 3, PLANKS, PLANKS);
            for (x, z) in [(0, 0), (4, 0), (0, 4), (4, 4)] {
                for y in 1..=3 {
                    v.pb(w, COBBLESTONE, 0, x, y, z, clip);
                }
            }
            v.fill(w, clip, 0, 1, 1, 0, 3, 3, PLANKS, PLANKS);
            v.fill(w, clip, 4, 1, 1, 4, 3, 3, PLANKS, PLANKS);
            v.fill(w, clip, 1, 1, 4, 3, 3, 4, PLANKS, PLANKS);
            v.pb(w, GLASS_PANE, 0, 0, 2, 2, clip);
            v.pb(w, GLASS_PANE, 0, 2, 2, 4, clip);
            v.pb(w, GLASS_PANE, 0, 4, 2, 2, clip);
            for (x, y) in [(1, 1), (1, 2), (1, 3), (2, 3), (3, 3), (3, 2), (3, 1)] {
                v.pb(w, PLANKS, 0, x, y, 0, clip);
            }
            v.step(w, 2, clip);
            v.fill(w, clip, 1, 1, 1, 3, 3, 3, AIR, AIR);
            if terrace {
                for (x, z) in [
                    (0, 0),
                    (1, 0),
                    (2, 0),
                    (3, 0),
                    (4, 0),
                    (0, 4),
                    (1, 4),
                    (2, 4),
                    (3, 4),
                    (4, 4),
                    (4, 1),
                    (4, 2),
                    (4, 3),
                    (0, 1),
                    (0, 2),
                    (0, 3),
                ] {
                    v.pb(w, FENCE, 0, x, 5, z, clip);
                }
            }
            if terrace {
                let m = q.mo(LADDER, 3);
                for y in 1..=4 {
                    v.pb(w, LADDER, m, 3, y, 3, clip);
                }
            }
            v.pb(w, TORCH, 0, 2, 3, 1, clip);
            v.foundation(w, clip, 0, 5, 0, 5, 6);
            spawn_villagers(p, w, clip, 1, 1, 2, 1);
        }
        VKind::Church => {
            v.fill(w, clip, 1, 1, 1, 3, 3, 7, AIR, AIR);
            v.fill(w, clip, 1, 5, 1, 3, 9, 3, AIR, AIR);
            v.fill(w, clip, 1, 0, 0, 3, 0, 8, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 1, 1, 0, 3, 10, 0, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 0, 1, 1, 0, 10, 3, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 4, 1, 1, 4, 10, 3, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 0, 0, 4, 0, 4, 7, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 4, 0, 4, 4, 4, 7, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 1, 1, 8, 3, 4, 8, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 1, 5, 4, 3, 10, 4, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 1, 5, 5, 3, 5, 7, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 0, 9, 0, 4, 9, 4, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 0, 4, 0, 4, 4, 4, COBBLESTONE, COBBLESTONE);
            for (x, y, z) in [
                (0, 11, 2),
                (4, 11, 2),
                (2, 11, 0),
                (2, 11, 4),
                (1, 1, 6),
                (1, 1, 7),
                (2, 1, 7),
                (3, 1, 6),
                (3, 1, 7),
            ] {
                v.pb(w, COBBLESTONE, 0, x, y, z, clip);
            }
            v.pb(w, STONE_STAIRS, q.mo(STONE_STAIRS, 3), 1, 1, 5, clip);
            v.pb(w, STONE_STAIRS, q.mo(STONE_STAIRS, 3), 2, 1, 6, clip);
            v.pb(w, STONE_STAIRS, q.mo(STONE_STAIRS, 3), 3, 1, 5, clip);
            v.pb(w, STONE_STAIRS, q.mo(STONE_STAIRS, 1), 1, 2, 7, clip);
            v.pb(w, STONE_STAIRS, q.mo(STONE_STAIRS, 0), 3, 2, 7, clip);
            for (x, y, z) in [
                (0, 2, 2),
                (0, 3, 2),
                (4, 2, 2),
                (4, 3, 2),
                (0, 6, 2),
                (0, 7, 2),
                (4, 6, 2),
                (4, 7, 2),
                (2, 6, 0),
                (2, 7, 0),
                (2, 6, 4),
                (2, 7, 4),
                (0, 3, 6),
                (4, 3, 6),
                (2, 3, 8),
            ] {
                v.pb(w, GLASS_PANE, 0, x, y, z, clip);
            }
            for (x, y, z) in [(2, 4, 7), (1, 4, 6), (3, 4, 6), (2, 4, 5)] {
                v.pb(w, TORCH, 0, x, y, z, clip);
            }
            let m = q.mo(LADDER, 4);
            for y in 1..=9 {
                v.pb(w, LADDER, m, 3, y, 3, clip);
            }
            v.pb(w, AIR, 0, 2, 1, 0, clip);
            v.pb(w, AIR, 0, 2, 2, 0, clip);
            q.door(w, clip, 2, 1, 0, q.mo(WOODEN_DOOR, 1));
            v.step(w, 2, clip);
            v.foundation(w, clip, 0, 5, 0, 9, 12);
            spawn_villagers(p, w, clip, 2, 1, 2, 1);
        }
        VKind::Hall => {
            v.fill(w, clip, 1, 1, 1, 7, 4, 4, AIR, AIR);
            v.fill(w, clip, 2, 1, 6, 8, 4, 10, AIR, AIR);
            v.fill(w, clip, 2, 0, 6, 8, 0, 10, DIRT, DIRT);
            v.pb(w, COBBLESTONE, 0, 6, 0, 6, clip);
            v.fill(w, clip, 2, 1, 6, 2, 1, 10, FENCE, FENCE);
            v.fill(w, clip, 8, 1, 6, 8, 1, 10, FENCE, FENCE);
            v.fill(w, clip, 3, 1, 10, 7, 1, 10, FENCE, FENCE);
            v.fill(w, clip, 1, 0, 1, 7, 0, 4, PLANKS, PLANKS);
            v.fill(w, clip, 0, 0, 0, 0, 3, 5, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 8, 0, 0, 8, 3, 5, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 1, 0, 0, 7, 1, 0, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 1, 0, 5, 7, 1, 5, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 1, 2, 0, 7, 3, 0, PLANKS, PLANKS);
            v.fill(w, clip, 1, 2, 5, 7, 3, 5, PLANKS, PLANKS);
            v.fill(w, clip, 0, 4, 1, 8, 4, 1, PLANKS, PLANKS);
            v.fill(w, clip, 0, 4, 4, 8, 4, 4, PLANKS, PLANKS);
            v.fill(w, clip, 0, 5, 2, 8, 5, 3, PLANKS, PLANKS);
            for (x, z) in [(0, 2), (0, 3), (8, 2), (8, 3)] {
                v.pb(w, PLANKS, 0, x, 4, z, clip);
            }
            let (a, b2) = (q.mo(OAK_STAIRS, 3), q.mo(OAK_STAIRS, 2));
            for d in -1..=2 {
                for x in 0..=8 {
                    v.pb(w, OAK_STAIRS, a, x, 4 + d, d, clip);
                    v.pb(w, OAK_STAIRS, b2, x, 4 + d, 5 - d, clip);
                }
            }
            for (x, z) in [(0, 1), (0, 4), (8, 1), (8, 4)] {
                v.pb(w, LOG, 0, x, 2, z, clip);
            }
            for (x, z) in [
                (0, 2),
                (0, 3),
                (8, 2),
                (8, 3),
                (2, 5),
                (3, 5),
                (5, 0),
                (6, 5),
            ] {
                v.pb(w, GLASS_PANE, 0, x, 2, z, clip);
            }
            v.pb(w, FENCE, 0, 2, 1, 3, clip);
            v.pb(w, WOODEN_PRESSURE_PLATE, 0, 2, 2, 3, clip);
            v.pb(w, PLANKS, 0, 1, 1, 4, clip);
            v.pb(w, OAK_STAIRS, q.mo(OAK_STAIRS, 3), 2, 1, 4, clip);
            v.pb(w, OAK_STAIRS, q.mo(OAK_STAIRS, 1), 1, 1, 3, clip);
            v.fill(
                w,
                clip,
                5,
                0,
                1,
                7,
                0,
                3,
                DOUBLE_STONE_SLAB,
                DOUBLE_STONE_SLAB,
            );
            v.pb(w, DOUBLE_STONE_SLAB, 0, 6, 1, 1, clip);
            v.pb(w, DOUBLE_STONE_SLAB, 0, 6, 1, 2, clip);
            v.pb(w, AIR, 0, 2, 1, 0, clip);
            v.pb(w, AIR, 0, 2, 2, 0, clip);
            v.pb(w, TORCH, 0, 2, 3, 1, clip);
            q.door(w, clip, 2, 1, 0, q.mo(WOODEN_DOOR, 1));
            v.step(w, 2, clip);
            v.pb(w, AIR, 0, 6, 1, 5, clip);
            v.pb(w, AIR, 0, 6, 2, 5, clip);
            v.pb(w, TORCH, 0, 6, 3, 4, clip);
            q.door(w, clip, 6, 1, 5, q.mo(WOODEN_DOOR, 1));
            v.foundation(w, clip, 0, 9, 0, 5, 7);
            spawn_villagers(p, w, clip, 4, 1, 2, 2);
        }
        VKind::WoodHut { tall, table } => {
            v.fill(w, clip, 1, 1, 1, 3, 5, 4, AIR, AIR);
            v.fill(w, clip, 0, 0, 0, 3, 0, 4, COBBLESTONE, COBBLESTONE);
            v.fill(w, clip, 1, 0, 1, 2, 0, 3, DIRT, DIRT);
            if tall {
                v.fill(w, clip, 1, 4, 1, 2, 4, 3, LOG, LOG);
            } else {
                v.fill(w, clip, 1, 5, 1, 2, 5, 3, LOG, LOG);
            }
            for (x, z) in [
                (1, 0),
                (2, 0),
                (1, 4),
                (2, 4),
                (0, 1),
                (0, 2),
                (0, 3),
                (3, 1),
                (3, 2),
                (3, 3),
            ] {
                v.pb(w, LOG, 0, x, 4, z, clip);
            }
            v.fill(w, clip, 0, 1, 0, 0, 3, 0, LOG, LOG);
            v.fill(w, clip, 3, 1, 0, 3, 3, 0, LOG, LOG);
            v.fill(w, clip, 0, 1, 4, 0, 3, 4, LOG, LOG);
            v.fill(w, clip, 3, 1, 4, 3, 3, 4, LOG, LOG);
            v.fill(w, clip, 0, 1, 1, 0, 3, 3, PLANKS, PLANKS);
            v.fill(w, clip, 3, 1, 1, 3, 3, 3, PLANKS, PLANKS);
            v.fill(w, clip, 1, 1, 0, 2, 3, 0, PLANKS, PLANKS);
            v.fill(w, clip, 1, 1, 4, 2, 3, 4, PLANKS, PLANKS);
            v.pb(w, GLASS_PANE, 0, 0, 2, 2, clip);
            v.pb(w, GLASS_PANE, 0, 3, 2, 2, clip);
            if table > 0 {
                v.pb(w, FENCE, 0, table, 1, 3, clip);
                v.pb(w, WOODEN_PRESSURE_PLATE, 0, table, 2, 3, clip);
            }
            v.pb(w, AIR, 0, 1, 1, 0, clip);
            v.pb(w, AIR, 0, 1, 2, 0, clip);
            q.door(w, clip, 1, 1, 0, q.mo(WOODEN_DOOR, 1));
            v.step(w, 1, clip);
            v.foundation(w, clip, 0, 4, 0, 5, 6);
            spawn_villagers(p, w, clip, 1, 1, 2, 1);
        }
        VKind::Field1 { crops } => {
            v.fill(w, clip, 0, 1, 0, 12, 4, 8, AIR, AIR);
            for x in [1, 4, 7, 10] {
                v.fill(w, clip, x, 0, 1, x + 1, 0, 7, FARMLAND, FARMLAND);
            }
            v.fill(w, clip, 0, 0, 0, 0, 0, 8, LOG, LOG);
            v.fill(w, clip, 6, 0, 0, 6, 0, 8, LOG, LOG);
            v.fill(w, clip, 12, 0, 0, 12, 0, 8, LOG, LOG);
            v.fill(w, clip, 1, 0, 0, 11, 0, 0, LOG, LOG);
            v.fill(w, clip, 1, 0, 8, 11, 0, 8, LOG, LOG);
            v.fill(w, clip, 3, 0, 1, 3, 0, 7, WATER, WATER);
            v.fill(w, clip, 9, 0, 1, 9, 0, 7, WATER, WATER);
            for z in 1..=7 {
                for (k, x) in [
                    (0, 1),
                    (0, 2),
                    (1, 4),
                    (1, 5),
                    (2, 7),
                    (2, 8),
                    (3, 10),
                    (3, 11),
                ] {
                    let m = rand_range(r, 2, 7);
                    v.pb(w, crops[k], m, x, 1, z, clip);
                }
            }
            for z in 0..9 {
                for x in 0..13 {
                    q.clear_up(w, x, 4, z, clip);
                    v.down(w, DIRT, x, -1, z, clip);
                }
            }
        }
        VKind::Field2 { crops } => {
            v.fill(w, clip, 0, 1, 0, 6, 4, 8, AIR, AIR);
            v.fill(w, clip, 1, 0, 1, 2, 0, 7, FARMLAND, FARMLAND);
            v.fill(w, clip, 4, 0, 1, 5, 0, 7, FARMLAND, FARMLAND);
            v.fill(w, clip, 0, 0, 0, 0, 0, 8, LOG, LOG);
            v.fill(w, clip, 6, 0, 0, 6, 0, 8, LOG, LOG);
            v.fill(w, clip, 1, 0, 0, 5, 0, 0, LOG, LOG);
            v.fill(w, clip, 1, 0, 8, 5, 0, 8, LOG, LOG);
            v.fill(w, clip, 3, 0, 1, 3, 0, 7, WATER, WATER);
            for z in 1..=7 {
                for (k, x) in [(0, 1), (0, 2), (1, 4), (1, 5)] {
                    let m = rand_range(r, 2, 7);
                    v.pb(w, crops[k], m, x, 1, z, clip);
                }
            }
            for z in 0..9 {
                for x in 0..7 {
                    q.clear_up(w, x, 4, z, clip);
                    v.down(w, DIRT, x, -1, z, clip);
                }
            }
        }
    }
    true
}
