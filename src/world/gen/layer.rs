//! Port of the `GenLayer` biome layer stack (`net.minecraft.world.gen.layer`).
//!
//! Layers are immutable after construction (the per-call "chunk seed" lives in a
//! local [`Rng`]), so a stack can be shared between threads.

use crate::world::biome::{self, Class, TempCategory};
use std::sync::Arc;

const M: i64 = 6364136223846793005;
const A: i64 = 1442695040888963407;

#[inline]
fn step(v: i64, add: i64) -> i64 {
    v.wrapping_mul(v.wrapping_mul(M).wrapping_add(A)).wrapping_add(add)
}

struct Rng {
    wgs: i64,
    cs: i64,
}

impl Rng {
    /// `initChunkSeed(x, z)`
    #[inline]
    fn init(&mut self, x: i32, z: i32) {
        let (x, z) = (x as i64, z as i64);
        let mut cs = self.wgs;
        cs = step(cs, x);
        cs = step(cs, z);
        cs = step(cs, x);
        cs = step(cs, z);
        self.cs = cs;
    }
    /// `nextInt(n)`
    #[inline]
    fn next(&mut self, n: i32) -> i32 {
        let mut r = ((self.cs >> 24) % n as i64) as i32;
        if r < 0 {
            r += n;
        }
        self.cs = step(self.cs, self.wgs);
        r
    }
    fn pick2(&mut self, a: i32, b: i32) -> i32 {
        if self.next(2) == 0 { a } else { b }
    }
    fn pick4(&mut self, a: i32, b: i32, c: i32, d: i32) -> i32 {
        [a, b, c, d][self.next(4) as usize]
    }
    /// `selectModeOrRandom`
    fn mode_or_random(&mut self, a: i32, b: i32, c: i32, d: i32) -> i32 {
        if b == c && c == d {
            b
        } else if a == b && a == c {
            a
        } else if a == b && a == d {
            a
        } else if a == c && a == d {
            a
        } else if a == b && c != d {
            a
        } else if a == c && b != d {
            a
        } else if a == d && b != c {
            a
        } else if b == c && a != d {
            b
        } else if b == d && a != c {
            b
        } else if c == d && a != b {
            c
        } else {
            self.pick4(a, b, c, d)
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Island,
    FuzzyZoom,
    Zoom,
    AddIsland,
    RemoveTooMuchOcean,
    AddSnow,
    EdgeCoolWarm,
    EdgeHeatIce,
    EdgeSpecial,
    AddMushroomIsland,
    DeepOcean,
    RiverInit,
    Biome,
    BiomeEdge,
    Hills,
    River,
    Smooth,
    RareBiome,
    Shore,
    RiverMix,
    VoronoiZoom,
}

pub struct GenLayer {
    kind: Kind,
    wgs: i64,
    parent: Option<Arc<GenLayer>>,
    parent2: Option<Arc<GenLayer>>,
}

fn is_oceanic(id: i32) -> bool {
    id == 0 || id == 24 || id == 10
}

/// `GenLayer.compareBiomesById`
fn same_biome(a: i32, b: i32) -> bool {
    if a == b {
        true
    } else if a != 38 && a != 39 {
        match (biome::get(a), biome::get(b)) {
            (Some(x), Some(y)) => x.class == y.class,
            _ => false,
        }
    } else {
        b == 38 || b == 39
    }
}

fn can_be_neighbors(a: i32, b: i32) -> bool {
    if same_biome(a, b) {
        return true;
    }
    match (biome::get(a), biome::get(b)) {
        (Some(x), Some(y)) => {
            let (ta, tb) = (x.temp_category, y.temp_category);
            ta == tb || ta == TempCategory::Medium || tb == TempCategory::Medium
        }
        _ => false,
    }
}

fn is_jungle(id: i32) -> bool {
    biome::get(id).map_or(false, |b| b.class == Class::Jungle)
}

fn is_jungle_compatible(id: i32) -> bool {
    is_jungle(id) || id == 23 || id == 21 || id == 22 || id == 4 || id == 5 || is_oceanic(id)
}

fn is_mesa(id: i32) -> bool {
    biome::get(id).map_or(false, |b| b.is_mesa())
}

impl GenLayer {
    fn make(
        kind: Kind,
        base_seed: i64,
        world_seed: i64,
        parent: Option<Arc<GenLayer>>,
        parent2: Option<Arc<GenLayer>>,
    ) -> Arc<GenLayer> {
        let mut base = base_seed;
        for _ in 0..3 {
            base = step(base, base_seed);
        }
        let mut wgs = world_seed;
        for _ in 0..3 {
            wgs = step(wgs, base);
        }
        Arc::new(GenLayer { kind, wgs, parent, parent2 })
    }

    /// `GenLayer.initializeAllBiomeGenerators(seed, WorldType.DEFAULT)`:
    /// returns `(genBiomes, biomeIndexLayer)`.
    pub fn build(seed: i64) -> (Arc<GenLayer>, Arc<GenLayer>) {
        let l = |kind, s: i64, p: Arc<GenLayer>| GenLayer::make(kind, s, seed, Some(p), None);
        let magnify = |s: i64, mut p: Arc<GenLayer>, n: i64| {
            for i in 0..n {
                p = GenLayer::make(Kind::Zoom, s + i, seed, Some(p), None);
            }
            p
        };

        let mut b = GenLayer::make(Kind::Island, 1, seed, None, None);
        b = l(Kind::FuzzyZoom, 2000, b);
        b = l(Kind::AddIsland, 1, b);
        b = l(Kind::Zoom, 2001, b);
        b = l(Kind::AddIsland, 2, b);
        b = l(Kind::AddIsland, 50, b);
        b = l(Kind::AddIsland, 70, b);
        b = l(Kind::RemoveTooMuchOcean, 2, b);
        b = l(Kind::AddSnow, 2, b);
        b = l(Kind::AddIsland, 3, b);
        b = l(Kind::EdgeCoolWarm, 2, b);
        b = l(Kind::EdgeHeatIce, 2, b);
        b = l(Kind::EdgeSpecial, 3, b);
        b = l(Kind::Zoom, 2002, b);
        b = l(Kind::Zoom, 2003, b);
        b = l(Kind::AddIsland, 4, b);
        b = l(Kind::AddMushroomIsland, 5, b);
        b = l(Kind::DeepOcean, 4, b);
        let base = magnify(1000, b, 0);

        let river_init = l(Kind::RiverInit, 100, base.clone());
        let mut biomes = l(Kind::Biome, 200, base);
        biomes = magnify(1000, biomes, 2);
        biomes = l(Kind::BiomeEdge, 1000, biomes);
        // Vanilla quirk: GenLayerHills does not override initWorldGenSeed, so the
        // two zoom layers that exist only on its river input never receive the
        // world seed and run with worldGenSeed == 0.
        let mut hills_river = river_init.clone();
        for _ in 0..2 {
            hills_river = Arc::new(GenLayer { kind: Kind::Zoom, wgs: 0, parent: Some(hills_river), parent2: None });
        }
        biomes = GenLayer::make(Kind::Hills, 1000, seed, Some(biomes), Some(hills_river));
        let mut river = magnify(1000, river_init, 2);
        river = magnify(1000, river, 4);
        river = l(Kind::River, 1, river);
        river = l(Kind::Smooth, 1000, river);
        biomes = l(Kind::RareBiome, 1001, biomes);
        for i in 0..4i64 {
            biomes = l(Kind::Zoom, 1000 + i, biomes);
            if i == 0 {
                biomes = l(Kind::AddIsland, 3, biomes);
            }
            if i == 1 {
                biomes = l(Kind::Shore, 1000, biomes);
            }
        }
        biomes = l(Kind::Smooth, 1000, biomes);
        let mix = GenLayer::make(Kind::RiverMix, 100, seed, Some(biomes), Some(river));
        let voronoi = l(Kind::VoronoiZoom, 10, mix.clone());
        (mix, voronoi)
    }

    fn p(&self) -> &GenLayer {
        self.parent.as_ref().unwrap()
    }

    /// `getInts(x, z, w, h)`
    pub fn get_ints(&self, x: i32, z: i32, w: i32, h: i32) -> Vec<i32> {
        let mut rng = Rng { wgs: self.wgs, cs: 0 };
        let (wu, hu) = (w as usize, h as usize);
        match self.kind {
            Kind::Island => {
                let mut out = vec![0i32; wu * hu];
                for j in 0..h {
                    for i in 0..w {
                        rng.init(x + i, z + j);
                        out[(i + j * w) as usize] = if rng.next(10) == 0 { 1 } else { 0 };
                    }
                }
                if x > -w && x <= 0 && z > -h && z <= 0 {
                    out[(-x + -z * w) as usize] = 1;
                }
                out
            }
            Kind::Zoom | Kind::FuzzyZoom => self.zoom(&mut rng, x, z, w, h),
            Kind::VoronoiZoom => self.voronoi(&mut rng, x, z, w, h),
            Kind::EdgeSpecial | Kind::RiverInit | Kind::Biome => {
                let p = self.p().get_ints(x, z, w, h);
                let mut out = vec![0i32; wu * hu];
                for j in 0..h {
                    for i in 0..w {
                        rng.init(i + x, j + z);
                        let idx = (i + j * w) as usize;
                        let mut v = p[idx];
                        out[idx] = match self.kind {
                            Kind::EdgeSpecial => {
                                if v != 0 && rng.next(13) == 0 {
                                    v |= ((1 + rng.next(15)) << 8) & 3840;
                                }
                                v
                            }
                            Kind::RiverInit => {
                                if v > 0 {
                                    rng.next(299999) + 2
                                } else {
                                    0
                                }
                            }
                            _ => {
                                let sp = (v & 3840) >> 8;
                                v &= -3841;
                                if is_oceanic(v) {
                                    v
                                } else if v == 14 {
                                    v
                                } else if v == 1 {
                                    if sp > 0 {
                                        if rng.next(3) == 0 { 39 } else { 38 }
                                    } else {
                                        [2, 2, 2, 35, 35, 1][rng.next(6) as usize]
                                    }
                                } else if v == 2 {
                                    if sp > 0 {
                                        21
                                    } else {
                                        [4, 29, 3, 1, 27, 6][rng.next(6) as usize]
                                    }
                                } else if v == 3 {
                                    if sp > 0 {
                                        32
                                    } else {
                                        [4, 3, 5, 1][rng.next(4) as usize]
                                    }
                                } else if v == 4 {
                                    [12, 12, 12, 30][rng.next(4) as usize]
                                } else {
                                    14
                                }
                            }
                        };
                    }
                }
                out
            }
            Kind::RiverMix => {
                let b = self.p().get_ints(x, z, w, h);
                let r = self.parent2.as_ref().unwrap().get_ints(x, z, w, h);
                let mut out = vec![0i32; wu * hu];
                for i in 0..wu * hu {
                    out[i] = if b[i] != 0 && b[i] != 24 {
                        if r[i] == 7 {
                            if b[i] == 12 {
                                11
                            } else if b[i] != 14 && b[i] != 15 {
                                r[i] & 255
                            } else {
                                15
                            }
                        } else {
                            b[i]
                        }
                    } else {
                        b[i]
                    };
                }
                out
            }
            _ => self.padded(&mut rng, x, z, w, h),
        }
    }

    fn padded(&self, rng: &mut Rng, x: i32, z: i32, w: i32, h: i32) -> Vec<i32> {
        let s = (w + 2) as usize;
        let p = self.p().get_ints(x - 1, z - 1, w + 2, h + 2);
        let r = if self.kind == Kind::Hills {
            self.parent2.as_ref().unwrap().get_ints(x - 1, z - 1, w + 2, h + 2)
        } else {
            Vec::new()
        };
        let mut out = vec![0i32; (w * h) as usize];
        for j in 0..h {
            for i in 0..w {
                let (iu, ju) = (i as usize, j as usize);
                let c = p[iu + 1 + (ju + 1) * s];
                let n = p[iu + 1 + ju * s];
                let e = p[iu + 2 + (ju + 1) * s];
                let wv = p[iu + (ju + 1) * s];
                let so = p[iu + 1 + (ju + 2) * s];
                let nw = p[iu + ju * s];
                let ne = p[iu + 2 + ju * s];
                let sw = p[iu + (ju + 2) * s];
                let se = p[iu + 2 + (ju + 2) * s];
                let nbrs = [n, e, wv, so];
                let any = |f: &dyn Fn(i32) -> bool| nbrs.iter().any(|&v| f(v));
                let all = |f: &dyn Fn(i32) -> bool| nbrs.iter().all(|&v| f(v));
                let v = match self.kind {
                    Kind::AddIsland => {
                        rng.init(i + x, j + z);
                        if c != 0 || (nw == 0 && ne == 0 && sw == 0 && se == 0) {
                            if c > 0 && (nw == 0 || ne == 0 || sw == 0 || se == 0) {
                                if rng.next(5) == 0 {
                                    if c == 4 { 4 } else { 0 }
                                } else {
                                    c
                                }
                            } else {
                                c
                            }
                        } else {
                            let mut cnt = 1;
                            let mut v = 1;
                            for d in [nw, ne, sw, se] {
                                if d != 0 {
                                    let k = cnt;
                                    cnt += 1;
                                    if rng.next(k) == 0 {
                                        v = d;
                                    }
                                }
                            }
                            if rng.next(3) == 0 {
                                v
                            } else if v == 4 {
                                4
                            } else {
                                0
                            }
                        }
                    }
                    Kind::AddMushroomIsland => {
                        rng.init(i + x, j + z);
                        if c == 0 && nw == 0 && ne == 0 && sw == 0 && se == 0 && rng.next(100) == 0 {
                            14
                        } else {
                            c
                        }
                    }
                    Kind::AddSnow => {
                        rng.init(i + x, j + z);
                        if c == 0 {
                            0
                        } else {
                            let r = rng.next(6);
                            if r == 0 {
                                4
                            } else if r <= 1 {
                                3
                            } else {
                                1
                            }
                        }
                    }
                    Kind::RemoveTooMuchOcean => {
                        rng.init(i + x, j + z);
                        if c == 0 && all(&|v| v == 0) && rng.next(2) == 0 { 1 } else { c }
                    }
                    Kind::DeepOcean => {
                        if c == 0 && all(&|v| v == 0) { 24 } else { c }
                    }
                    Kind::EdgeCoolWarm => {
                        if c == 1 && any(&|v| v == 3 || v == 4) { 2 } else { c }
                    }
                    Kind::EdgeHeatIce => {
                        if c == 4 && any(&|v| v == 1 || v == 2) { 3 } else { c }
                    }
                    Kind::RareBiome => {
                        rng.init(i + x, j + z);
                        if rng.next(57) == 0 {
                            if c == 1 { 129 } else { c }
                        } else {
                            c
                        }
                    }
                    Kind::River => {
                        let f = |v: i32| if v >= 2 { 2 + (v & 1) } else { v };
                        let fc = f(c);
                        if fc == f(wv) && fc == f(n) && fc == f(e) && fc == f(so) { -1 } else { 7 }
                    }
                    Kind::Smooth => {
                        let mut c = c;
                        if wv == e && n == so {
                            rng.init(i + x, j + z);
                            c = if rng.next(2) == 0 { wv } else { n };
                        } else {
                            if wv == e {
                                c = wv;
                            }
                            if n == so {
                                c = n;
                            }
                        }
                        c
                    }
                    Kind::BiomeEdge => {
                        if same_biome(c, 3) {
                            if all(&|v| can_be_neighbors(v, 3)) { c } else { 20 }
                        } else if c == 38 || c == 39 {
                            if all(&|v| same_biome(v, c)) { c } else { 37 }
                        } else if c == 32 {
                            if all(&|v| same_biome(v, 32)) { c } else { 5 }
                        } else if c == 2 {
                            if any(&|v| v == 12) { 34 } else { c }
                        } else if c == 6 {
                            if any(&|v| v == 2 || v == 30 || v == 12) {
                                1
                            } else if any(&|v| v == 21) {
                                23
                            } else {
                                c
                            }
                        } else {
                            c
                        }
                    }
                    Kind::Shore => {
                        let ocean_nbr = any(&is_oceanic);
                        let replace_if_ocean = |repl: i32| {
                            if is_oceanic(c) || !ocean_nbr { c } else { repl }
                        };
                        let b = biome::get(c);
                        if c == 14 {
                            if any(&|v| v == 0) { 15 } else { c }
                        } else if is_jungle(c) {
                            if any(&|v| !is_jungle_compatible(v)) {
                                23
                            } else if !ocean_nbr {
                                c
                            } else {
                                16
                            }
                        } else if c == 3 || c == 34 || c == 20 {
                            replace_if_ocean(25)
                        } else if b.map_or(false, |b| b.enable_snow) {
                            replace_if_ocean(26)
                        } else if c == 37 || c == 38 {
                            if ocean_nbr {
                                c
                            } else if all(&is_mesa) {
                                c
                            } else {
                                2
                            }
                        } else if c != 0 && c != 24 && c != 7 && c != 6 {
                            if !ocean_nbr { c } else { 16 }
                        } else {
                            c
                        }
                    }
                    Kind::Hills => {
                        rng.init(i + x, j + z);
                        let rv = r[iu + 1 + (ju + 1) * s];
                        let flag = (rv - 2) % 29 == 0;
                        if c != 0 && rv >= 2 && (rv - 2) % 29 == 1 && c < 128 {
                            if biome::get(c + 128).is_some() { c + 128 } else { c }
                        } else if rng.next(3) != 0 && !flag {
                            c
                        } else {
                            let mut v = c;
                            if c == 2 {
                                v = 17;
                            } else if c == 4 {
                                v = 18;
                            } else if c == 27 {
                                v = 28;
                            } else if c == 29 {
                                v = 1;
                            } else if c == 5 {
                                v = 19;
                            } else if c == 32 {
                                v = 33;
                            } else if c == 30 {
                                v = 31;
                            } else if c == 1 {
                                v = if rng.next(3) == 0 { 18 } else { 4 };
                            } else if c == 12 {
                                v = 13;
                            } else if c == 21 {
                                v = 22;
                            } else if c == 0 {
                                v = 24;
                            } else if c == 3 {
                                v = 34;
                            } else if c == 35 {
                                v = 36;
                            } else if same_biome(c, 38) {
                                v = 37;
                            } else if c == 24 && rng.next(3) == 0 {
                                v = if rng.next(2) == 0 { 1 } else { 4 };
                            }
                            if flag && v != c {
                                v = if biome::get(v + 128).is_some() { v + 128 } else { c };
                            }
                            if v == c {
                                c
                            } else {
                                let cnt = nbrs.iter().filter(|&&nb| same_biome(nb, c)).count();
                                if cnt >= 3 { v } else { c }
                            }
                        }
                    }
                    _ => unreachable!(),
                };
                out[(i + j * w) as usize] = v;
            }
        }
        out
    }

    fn zoom(&self, rng: &mut Rng, x: i32, z: i32, w: i32, h: i32) -> Vec<i32> {
        let fuzzy = self.kind == Kind::FuzzyZoom;
        let px = x >> 1;
        let pz = z >> 1;
        let pw = (w >> 1) + 2;
        let ph = (h >> 1) + 2;
        let p = self.p().get_ints(px, pz, pw, ph);
        let tw = ((pw - 1) << 1) as usize;
        let th = ((ph - 1) << 1) as usize;
        let mut tmp = vec![0i32; tw * th];
        let pwu = pw as usize;
        for j in 0..(ph - 1) {
            let ju = j as usize;
            let mut idx = (ju << 1) * tw;
            let mut a = p[ju * pwu];
            let mut b = p[(ju + 1) * pwu];
            for i in 0..(pw - 1) {
                let iu = i as usize;
                rng.init((i + px) << 1, (j + pz) << 1);
                let c = p[iu + 1 + ju * pwu];
                let d = p[iu + 1 + (ju + 1) * pwu];
                tmp[idx] = a;
                tmp[idx + tw] = rng.pick2(a, b);
                idx += 1;
                tmp[idx] = rng.pick2(a, c);
                tmp[idx + tw] = if fuzzy { rng.pick4(a, c, b, d) } else { rng.mode_or_random(a, c, b, d) };
                idx += 1;
                a = c;
                b = d;
            }
        }
        let mut out = vec![0i32; (w * h) as usize];
        let wu = w as usize;
        for j in 0..h as usize {
            let src = (j + (z & 1) as usize) * tw + (x & 1) as usize;
            out[j * wu..(j + 1) * wu].copy_from_slice(&tmp[src..src + wu]);
        }
        out
    }

    fn voronoi(&self, rng: &mut Rng, x: i32, z: i32, w: i32, h: i32) -> Vec<i32> {
        let x = x - 2;
        let z = z - 2;
        let px = x >> 2;
        let pz = z >> 2;
        let pw = (w >> 2) + 2;
        let ph = (h >> 2) + 2;
        let p = self.p().get_ints(px, pz, pw, ph);
        let tw = ((pw - 1) << 2) as usize;
        let th = ((ph - 1) << 2) as usize;
        let mut tmp = vec![0i32; tw * th];
        let pwu = pw as usize;
        let jit = |rng: &mut Rng| (rng.next(1024) as f64 / 1024.0 - 0.5) * 3.6;
        for j in 0..(ph - 1) {
            let ju = j as usize;
            let mut a = p[ju * pwu];
            let mut b = p[(ju + 1) * pwu];
            for i in 0..(pw - 1) {
                let iu = i as usize;
                rng.init((i + px) << 2, (j + pz) << 2);
                let ax = jit(rng);
                let az = jit(rng);
                rng.init((i + px + 1) << 2, (j + pz) << 2);
                let bx = jit(rng) + 4.0;
                let bz = jit(rng);
                rng.init((i + px) << 2, (j + pz + 1) << 2);
                let cx = jit(rng);
                let cz = jit(rng) + 4.0;
                rng.init((i + px + 1) << 2, (j + pz + 1) << 2);
                let dx = jit(rng) + 4.0;
                let dz = jit(rng) + 4.0;
                let e = p[iu + 1 + ju * pwu] & 255;
                let f = p[iu + 1 + (ju + 1) * pwu] & 255;
                for zz in 0..4usize {
                    let mut k = ((ju << 2) + zz) * tw + (iu << 2);
                    for xx in 0..4usize {
                        let (zf, xf) = (zz as f64, xx as f64);
                        let da = (zf - az) * (zf - az) + (xf - ax) * (xf - ax);
                        let db = (zf - bz) * (zf - bz) + (xf - bx) * (xf - bx);
                        let dc = (zf - cz) * (zf - cz) + (xf - cx) * (xf - cx);
                        let dd = (zf - dz) * (zf - dz) + (xf - dx) * (xf - dx);
                        tmp[k] = if da < db && da < dc && da < dd {
                            a
                        } else if db < da && db < dc && db < dd {
                            e
                        } else if dc < da && dc < db && dc < dd {
                            b
                        } else {
                            f
                        };
                        k += 1;
                    }
                }
                a = e;
                b = f;
            }
        }
        let mut out = vec![0i32; (w * h) as usize];
        let wu = w as usize;
        for j in 0..h as usize {
            let src = (j + (z & 3) as usize) * tw + (x & 3) as usize;
            out[j * wu..(j + 1) * wu].copy_from_slice(&tmp[src..src + wu]);
        }
        out
    }
}
