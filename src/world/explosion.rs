//! `Explosion`: `doExplosionA` (the rays, `getBlockDensity`, the entities'
//! damage and push) and the server's `doExplosionB(false)` (drops, air,
//! chained TNT, fire), with the affected blocks in the iteration order of
//! the `HashSet<ChunkPosition>` vanilla collects them in.

use super::ServerWorld;
use crate::block::{self, Material};
use crate::entity::player::Aabb;
use crate::java::JavaRandom;
use crate::util::math;
use crate::world::BlockView;

/// `Explosion.exploder`, for the hooks it overrides.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Exploder {
    None,
    /// A living exploder (a creeper) at its position.
    Living([f64; 3]),
    /// `EntityTNTPrimed`
    Tnt,
    /// `EntityMinecartTNT`: `isIgnited` keeps rails and the blocks under them.
    Cart {
        ignited: bool,
    },
}

/// `World.newExplosion` as asked for by an entity.
#[derive(Clone, Copy, Debug)]
pub struct Request {
    pub pos: [f64; 3],
    pub size: f32,
    pub flaming: bool,
    pub smoking: bool,
    pub exploder: Exploder,
    /// The exploding mob, left out of the targets.
    pub exclude: Option<u32>,
}

/// What the client is sent (`S27PacketExplosion`): it runs
/// `doExplosionB(true)` itself.
#[derive(Clone, Debug)]
pub struct View {
    pub pos: [f64; 3],
    pub size: f32,
    pub smoking: bool,
    pub blocks: Vec<[i32; 3]>,
}

/// Target tags: the kind in the high half, the id or index in the low.
pub const TAG_PLAYER: u64 = 1 << 32;
pub const TAG_MOB: u64 = 2 << 32;
pub const TAG_TNT: u64 = 3 << 32;
pub const TAG_ITEM: u64 = 4 << 32;
pub const TAG_VEHICLE: u64 = 5 << 32;
pub const TAG_ARROW: u64 = 6 << 32;
pub const TAG_THROWABLE: u64 = 7 << 32;
pub const TAG_FALLING: u64 = 8 << 32;

/// An entity in reach: what `doExplosionA` needs of it.
#[derive(Clone, Copy, Debug)]
pub struct Target {
    pub tag: u64,
    /// `posX/Y/Z`
    pub pos: [f64; 3],
    /// `getEyeHeight`
    pub eye: f64,
    pub bb: Aabb,
}

/// What an entity takes: `attackEntityFrom(explosion, damage)` and the
/// push added to its motion (`getBlastDamageReduction`, without
/// enchantments the same as the knockback the player is sent).
#[derive(Clone, Copy, Debug)]
pub struct Blast {
    pub tag: u64,
    pub damage: f32,
    pub push: [f64; 3],
}

/// `ChunkPosition.hashCode`
fn chunk_position_hash(p: (i32, i32, i32)) -> i32 {
    p.0.wrapping_mul(8976890)
        .wrapping_add(p.1.wrapping_mul(981131))
        .wrapping_add(p.2)
}

/// A Java 8 `HashSet`: keys in iteration order once all were added.
pub struct JavaHashSet<K: Copy + Eq + std::hash::Hash> {
    /// key, hash, insertion number
    entries: Vec<(K, i32, usize)>,
    seen: std::collections::HashSet<K>,
    hash: fn(K) -> i32,
}

impl<K: Copy + Eq + std::hash::Hash> JavaHashSet<K> {
    pub fn new(hash: fn(K) -> i32) -> Self {
        JavaHashSet {
            entries: Vec::new(),
            seen: std::collections::HashSet::new(),
            hash,
        }
    }
    pub fn insert(&mut self, k: K) {
        if !self.seen.insert(k) {
            return;
        }
        let h = (self.hash)(k);
        let n = self.entries.len();
        self.entries.push((k, h, n));
    }
    pub fn contains(&self, k: K) -> bool {
        self.seen.contains(&k)
    }
    /// `HashMap.keySet().iterator()`: by bucket of the final table, then
    /// by insertion within a bucket (resizes keep the relative order).
    pub fn into_order(self) -> Vec<K> {
        let n = self.entries.len();
        let mut cap = 16usize;
        // resize() doubles when ++size > threshold (0.75 * capacity)
        while n > cap * 3 / 4 {
            cap *= 2;
        }
        let mut v: Vec<(usize, usize, K)> = self
            .entries
            .into_iter()
            .map(|(k, h, i)| {
                let spread = (h ^ ((h as u32) >> 16) as i32) as u32 as usize;
                (spread & (cap - 1), i, k)
            })
            .collect();
        v.sort_by_key(|e| (e.0, e.1));
        v.into_iter().map(|e| e.2).collect()
    }
}

/// `Entity.getExplosionResistance` / `Block.getExplosionResistance`
fn resistance(w: &ServerWorld, exploder: &Exploder, x: i32, y: i32, z: i32, id: u8) -> f32 {
    if let Exploder::Cart { ignited: true } = exploder {
        let rail = crate::entity::minecart::is_rail;
        if rail(id) || rail(w.peek_block(x, y + 1, z)) {
            return 0.0;
        }
    }
    crate::entity::mob::explosion_resistance(id)
}

/// `Entity.canExplodeBlock`
fn can_explode(w: &ServerWorld, exploder: &Exploder, x: i32, y: i32, z: i32, id: u8) -> bool {
    if let Exploder::Cart { ignited: true } = exploder {
        let rail = crate::entity::minecart::is_rail;
        if rail(id) || rail(w.peek_block(x, y + 1, z)) {
            return false;
        }
    }
    true
}

/// `World.getBlockDensity(vec, bb)`
pub fn block_density(w: &dyn BlockView, c: [f64; 3], bb: &Aabb) -> f32 {
    let dx = 1.0 / ((bb.max[0] - bb.min[0]) * 2.0 + 1.0);
    let dy = 1.0 / ((bb.max[1] - bb.min[1]) * 2.0 + 1.0);
    let dz = 1.0 / ((bb.max[2] - bb.min[2]) * 2.0 + 1.0);
    if dx < 0.0 || dy < 0.0 || dz < 0.0 {
        return 0.0;
    }
    let mut free = 0;
    let mut all = 0;
    let mut a = 0.0f32;
    while a <= 1.0 {
        let mut b = 0.0f32;
        while b <= 1.0 {
            let mut k = 0.0f32;
            while k <= 1.0 {
                let p = [
                    bb.min[0] + (bb.max[0] - bb.min[0]) * a as f64,
                    bb.min[1] + (bb.max[1] - bb.min[1]) * b as f64,
                    bb.min[2] + (bb.max[2] - bb.min[2]) * k as f64,
                ];
                if crate::entity::raytrace::ray_trace_blocks(w, p, c, false, false, false).is_none()
                {
                    free += 1;
                }
                all += 1;
                k = (k as f64 + dz) as f32;
            }
            b = (b as f64 + dy) as f32;
        }
        a = (a as f64 + dx) as f32;
    }
    free as f32 / all as f32
}

/// `doExplosionA`: the affected blocks (in order) and what each target
/// in reach takes. `targets` exclude the exploder.
pub fn explosion_a(
    w: &mut ServerWorld,
    r: &Request,
    targets: &[Target],
) -> (Vec<[i32; 3]>, Vec<Blast>) {
    let range = 16;
    let mut set = JavaHashSet::new(chunk_position_hash);
    let c = r.pos;
    for i in 0..range {
        for j in 0..range {
            for k in 0..range {
                if i == 0 || i == range - 1 || j == 0 || j == range - 1 || k == 0 || k == range - 1
                {
                    let mut dx = (i as f32 / (range as f32 - 1.0) * 2.0 - 1.0) as f64;
                    let mut dy = (j as f32 / (range as f32 - 1.0) * 2.0 - 1.0) as f64;
                    let mut dz = (k as f32 / (range as f32 - 1.0) * 2.0 - 1.0) as f64;
                    let l = (dx * dx + dy * dy + dz * dz).sqrt();
                    dx /= l;
                    dy /= l;
                    dz /= l;
                    let mut f = r.size * (0.7 + w.rand.next_float() * 0.6);
                    let (mut x, mut y, mut z) = (c[0], c[1], c[2]);
                    let step = 0.3f32;
                    while f > 0.0 {
                        let bx = math::floor_double(x);
                        let by = math::floor_double(y);
                        let bz = math::floor_double(z);
                        let id = w.peek_block(bx, by, bz);
                        if block::material(id) != Material::Air {
                            let res = resistance(w, &r.exploder, bx, by, bz, id);
                            f -= (res + 0.3) * step;
                        }
                        if f > 0.0 && can_explode(w, &r.exploder, bx, by, bz, id) {
                            set.insert((bx, by, bz));
                        }
                        x += dx * step as f64;
                        y += dy * step as f64;
                        z += dz * step as f64;
                        f -= step * 0.75;
                    }
                }
            }
        }
    }
    let affected: Vec<[i32; 3]> = set
        .into_order()
        .into_iter()
        .map(|p| [p.0, p.1, p.2])
        .collect();
    // the entities
    let size = r.size * 2.0;
    let x0 = math::floor_double(c[0] - size as f64 - 1.0) as f64;
    let x1 = math::floor_double(c[0] + size as f64 + 1.0) as f64;
    let y0 = math::floor_double(c[1] - size as f64 - 1.0) as f64;
    let y1 = math::floor_double(c[1] + size as f64 + 1.0) as f64;
    let z0 = math::floor_double(c[2] - size as f64 - 1.0) as f64;
    let z1 = math::floor_double(c[2] + size as f64 + 1.0) as f64;
    let reach = Aabb {
        min: [x0, y0, z0],
        max: [x1, y1, z1],
    };
    let mut blasts = Vec::new();
    for t in targets {
        if !t.bb.intersects(&reach) {
            continue;
        }
        let d = ((t.pos[0] - c[0]).powi(2) + (t.pos[1] - c[1]).powi(2) + (t.pos[2] - c[2]).powi(2))
            .sqrt()
            / size as f64;
        if d > 1.0 {
            continue;
        }
        let mut ex = t.pos[0] - c[0];
        let mut ey = t.pos[1] + t.eye - c[1];
        let mut ez = t.pos[2] - c[2];
        let l = math::sqrt_double(ex * ex + ey * ey + ez * ez) as f64;
        if l == 0.0 {
            continue;
        }
        ex /= l;
        ey /= l;
        ez /= l;
        let density = block_density(&*w, c, &t.bb) as f64;
        let k = (1.0 - d) * density;
        let damage = ((k * k + k) / 2.0 * 8.0 * size as f64 + 1.0) as i32 as f32;
        // EnchantmentProtection.getBlastDamageReduction without Blast Protection
        blasts.push(Blast {
            tag: t.tag,
            damage,
            push: [ex * k, ey * k, ez * k],
        });
    }
    (affected, blasts)
}

/// What `doExplosionB(false)` leaves for the owner.
#[derive(Default)]
pub struct Results {
    /// `EntityTNTPrimed` from `BlockTNT.onBlockDestroyedByExplosion`: centre, fuse.
    pub tnt: Vec<([f64; 3], i32)>,
    /// The `random.explode` sound's pitch.
    pub pitch: f32,
}

/// `doExplosionB(false)` on the server. `rng` is `explosionRNG`.
pub fn explosion_b(
    w: &mut ServerWorld,
    r: &Request,
    affected: &[[i32; 3]],
    rng: &mut JavaRandom,
) -> Results {
    let mut out = Results {
        pitch: (1.0 + (w.rand.next_float() - w.rand.next_float()) * 0.2) * 0.7,
        ..Default::default()
    };
    if r.smoking {
        for &[x, y, z] in affected {
            let id = w.peek_block(x, y, z);
            if block::material(id) == Material::Air {
                continue;
            }
            let meta = w.peek_meta(x, y, z) as i32;
            // canDropFromExplosion: not TNT
            if id != block::TNT {
                drop_with_chance(w, id, meta, x, y, z, 1.0 / r.size);
            }
            w.set_block(x, y, z, block::AIR, 0, 3);
            // onBlockDestroyedByExplosion
            if id == block::TNT {
                let fuse = 80;
                let f = w.rand.next_int_n(fuse / 4) + fuse / 8;
                out.tnt
                    .push(([x as f64 + 0.5, y as f64 + 0.5, z as f64 + 0.5], f));
            }
        }
    }
    if r.flaming {
        for &[x, y, z] in affected {
            let here = w.peek_block(x, y, z);
            let below = w.peek_block(x, y - 1, z);
            if block::material(here) == Material::Air
                && block::info(below).full_block
                && rng.next_int_n(3) == 0
            {
                w.set_block(x, y, z, block::FIRE, 0, 3);
            }
        }
    }
    out
}

/// `Block.dropBlockAsItemWithChance(world, x, y, z, meta, chance, 0)`: each
/// item of the drop is kept with `chance`.
fn drop_with_chance(w: &mut ServerWorld, id: u8, meta: i32, x: i32, y: i32, z: i32, chance: f32) {
    let mut r = std::mem::replace(&mut w.rand, JavaRandom::new(0));
    let drops = crate::item::drops(id, meta as i32, &mut r);
    for st in drops {
        for _ in 0..st.count {
            if r.next_float() <= chance {
                // dropBlockAsItem_do
                let f = 0.7f32;
                let dx = (r.next_float() * f) as f64 + (1.0 - f) as f64 * 0.5;
                let dy = (r.next_float() * f) as f64 + (1.0 - f) as f64 * 0.5;
                let dz = (r.next_float() * f) as f64 + (1.0 - f) as f64 * 0.5;
                w.gen_entities
                    .push(super::server::GenEntity::ScatteredItem {
                        pos: [x as f64 + dx, y as f64 + dy, z as f64 + dz],
                        stack: crate::item::Stack::new(st.id, st.meta, 1),
                    });
            }
        }
    }
    w.rand = r;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The order of a Java 8 `HashSet<ChunkPosition>` with three keys:
    /// buckets of a 16 table by the spread hash.
    #[test]
    fn hash_set_order() {
        let mut s = JavaHashSet::new(chunk_position_hash);
        for p in [(0, 64, 0), (1, 64, 0), (0, 64, 1)] {
            s.insert(p);
        }
        let h = |p: (i32, i32, i32)| {
            let h = chunk_position_hash(p);
            ((h ^ ((h as u32) >> 16) as i32) & 15) as usize
        };
        let mut want = vec![(0, 64, 0), (1, 64, 0), (0, 64, 1)];
        want.sort_by_key(|&p| h(p));
        assert_eq!(s.into_order(), want);
    }
}
