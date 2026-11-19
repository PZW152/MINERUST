//! A full, correct relight of one chunk once nothing will be decorated into
//! it any more (behind `compat::fixed()`).
//!
//! 1.7.10 lights a chunk while it is generated and decorated, and every
//! light update gives up when a chunk within 17 blocks is not loaded
//! (`World.updateLightByType` / `checkChunksExist`). Trees and other
//! features that population writes near the edge of the loaded area
//! therefore leave sky light that never spread: dark patches under open
//! sky and black cave mouths, often along chunk borders, that stay until
//! something nearby changes. Here the chunk's sky and block light are
//! recomputed from scratch with the same rules as `computeLightValue`
//! (opacity at least 1, opaque blocks dark unless they glow, the heightmap
//! as the sky's source), taking the four neighbours' stored values as what
//! flows in. Light that then has to leave the chunk goes out through the
//! normal `updateLightByType`.

use super::{bidx, ServerWorld};
use crate::block;

const N: usize = 16 * 16 * 256;

impl ServerWorld {
    /// Whether `fix_light` should run for this chunk now: it and all eight
    /// neighbours are loaded and lit, and the chunk was not relit yet.
    pub fn wants_light_fix(&self, cx: i32, cz: i32) -> bool {
        match self.chunks.get(&(cx, cz)) {
            Some(c) if !c.light_fixed && c.terrain_populated => {}
            _ => return false,
        }
        for dx in -1..=1 {
            for dz in -1..=1 {
                if !self.chunks.contains_key(&(cx + dx, cz + dz)) {
                    return false;
                }
            }
        }
        true
    }

    /// A column whose top (heightmap) moved: its chunk and the neighbours
    /// within 15 blocks are relit once things settle, which cleans up what
    /// the step-by-step `updateLightByType` leaves behind (fire through a
    /// canopy, explosions, trees cut down).
    pub fn queue_relight(&mut self, x: i32, z: i32) {
        let due = self.total_time + 10;
        for (cx, cz) in [
            (x >> 4, z >> 4),
            ((x - 15) >> 4, z >> 4),
            ((x + 15) >> 4, z >> 4),
            (x >> 4, (z - 15) >> 4),
            (x >> 4, (z + 15) >> 4),
        ] {
            // a chunk not relit yet will be relit whole once it is ready
            // (population writes thousands of blocks; none of them count)
            if self.chunks.get(&(cx, cz)).is_some_and(|c| c.light_fixed) {
                self.relight_due.insert((cx, cz), due);
            }
        }
    }

    /// The queued relights that are due, at most four per tick.
    pub fn run_due_relights(&mut self) {
        if self.relight_due.is_empty() {
            return;
        }
        let now = self.total_time;
        let mut due: Vec<(i32, i32)> = self
            .relight_due
            .iter()
            .filter(|(_, &t)| t <= now)
            .map(|(&k, _)| k)
            .collect();
        due.sort();
        // about a millisecond each
        for key in due.into_iter().take(4) {
            self.relight_due.remove(&key);
            self.relight_now(key.0, key.1);
        }
    }

    /// `fix_light` for a chunk whose neighbours are loaded, whether or not
    /// it was relit before.
    pub fn relight_now(&mut self, cx: i32, cz: i32) -> bool {
        for dx in -1..=1 {
            for dz in -1..=1 {
                if !self.chunks.contains_key(&(cx + dx, cz + dz)) {
                    return false;
                }
            }
        }
        if !self.chunks[&(cx, cz)].terrain_populated {
            return false;
        }
        self.fix_light(cx, cz)
    }

    /// Recomputes the chunk's sky light (where there is a sky) and block
    /// light; true when anything changed.
    pub fn fix_light(&mut self, cx: i32, cz: i32) -> bool {
        let Some(c) = self.chunks.get_mut(&(cx, cz)) else {
            return false;
        };
        c.light_fixed = true;
        let no_sky = c.no_sky;
        let mut changed = false;
        if !no_sky {
            changed |= self.relight_chunk(cx, cz, true);
        }
        changed |= self.relight_chunk(cx, cz, false);
        changed
    }

    fn relight_chunk(&mut self, cx: i32, cz: i32, sky: bool) -> bool {
        let (wx0, wz0) = (cx * 16, cz * 16);
        // above every filled section of the chunk and its neighbours there
        // is only open sky and no block light: nothing to compute there
        let mut ymax = 0;
        for dx in -1..=1 {
            for dz in -1..=1 {
                if let Some(c) = self.chunks.get(&(cx + dx, cz + dz)) {
                    ymax = ymax.max(c.top_filled_segment() + 16);
                }
            }
        }
        let ymax = ymax.min(256);
        // effective opacity (0 = nothing gets in) and the source level
        let mut op = vec![0u8; N];
        let mut v = vec![0u8; N];
        {
            let c = &self.chunks[&(cx, cz)];
            for x in 0..16 {
                for z in 0..16 {
                    let h = c.height[(z << 4 | x) as usize];
                    for y in 0..ymax {
                        let i = bidx(x, y, z);
                        let info = block::info(c.block(x, y, z));
                        let mut o = info.light_opacity as i32;
                        if o >= 15 && info.light_value > 0 {
                            o = 1;
                        }
                        op[i] = if o >= 15 { 0 } else { o.max(1) as u8 };
                        v[i] = if sky {
                            if y >= h {
                                15
                            } else {
                                0
                            }
                        } else {
                            info.light_value
                        };
                    }
                }
            }
        }
        // buckets by level: every lit cell, then what flows in at the sides
        let mut buckets: Vec<Vec<u32>> = vec![Vec::new(); 16];
        for (i, &l) in v.iter().enumerate() {
            if l > 1 {
                buckets[l as usize].push(i as u32);
            }
        }
        let mut raise = |i: usize, l: i32, v: &mut [u8], b: &mut Vec<Vec<u32>>| {
            if op[i] == 0 {
                return;
            }
            let n = l - op[i] as i32;
            if n > v[i] as i32 {
                v[i] = n as u8;
                if n > 1 {
                    b[n as usize].push(i as u32);
                }
            }
        };
        for y in 0..ymax {
            for k in 0..16 {
                for (x, z, ox, oz) in [(0, k, -1, k), (15, k, 16, k), (k, 0, k, -1), (k, 15, k, 16)]
                {
                    let n = self.saved_light(sky, wx0 + ox, y, wz0 + oz);
                    raise(bidx(x, y, z), n, &mut v, &mut buckets);
                }
            }
        }
        for l in (2..=15).rev() {
            while let Some(i) = buckets[l].pop() {
                let i = i as usize;
                if v[i] as usize != l {
                    continue;
                }
                let (x, z, y) = ((i >> 12) as i32, (i >> 8 & 15) as i32, (i & 255) as i32);
                for (dx, dy, dz) in [
                    (1, 0, 0),
                    (-1, 0, 0),
                    (0, 1, 0),
                    (0, -1, 0),
                    (0, 0, 1),
                    (0, 0, -1),
                ] {
                    let (a, b, c) = (x + dx, y + dy, z + dz);
                    if !(0..16).contains(&a) || !(0..ymax).contains(&b) || !(0..16).contains(&c) {
                        continue;
                    }
                    raise(bidx(a, b, c), l as i32, &mut v, &mut buckets);
                }
            }
        }
        // write back, section by section; a missing section is only made
        // when it now holds something its default reading would not give
        let mut mask = 0u16;
        let mut edge = Vec::new();
        {
            let c = self.chunks.get_mut(&(cx, cz)).unwrap();
            for s in 0..ymax / 16 {
                let present = c.sections & (1 << s) != 0;
                let mut differs = false;
                for x in 0..16 {
                    for z in 0..16 {
                        for y in s * 16..s * 16 + 16 {
                            if c.saved_light(sky, x, y, z) != v[bidx(x, y, z)] as i32 {
                                differs = true;
                            }
                        }
                    }
                }
                if !differs {
                    continue;
                }
                if !present {
                    // Chunk.setLightValue makes the storage; its sky light
                    // is all written below
                    c.sections |= 1 << s;
                }
                let d = std::sync::Arc::make_mut(&mut c.data);
                let arr = if sky { &mut d.sky } else { &mut d.blk };
                for x in 0..16 {
                    for z in 0..16 {
                        for y in s * 16..s * 16 + 16 {
                            let i = bidx(x, y, z);
                            if arr[i] != v[i] {
                                arr[i] = v[i];
                                if x == 0 || x == 15 || z == 0 || z == 15 {
                                    edge.push((x, y, z));
                                }
                            }
                        }
                    }
                }
                mask |= 1 << s;
            }
        }
        if mask == 0 {
            return false;
        }
        // the meshes of this chunk, and of a neighbour only where a cell it
        // reads (the face it touches, the corner column) changed
        let grow = |m: u16| m | m << 1 | m >> 1;
        *self.dirty.entry((cx, cz)).or_insert(0) |= grow(mask);
        let mut side = std::collections::HashMap::<(i32, i32), u16>::new();
        for &(x, y, z) in &edge {
            let bit = 1u16 << (y >> 4);
            let ox = if x == 0 {
                -1
            } else if x == 15 {
                1
            } else {
                0
            };
            let oz = if z == 0 {
                -1
            } else if z == 15 {
                1
            } else {
                0
            };
            for (a, b) in [(ox, 0), (0, oz), (ox, oz)] {
                if (a, b) != (0, 0) {
                    *side.entry((cx + a, cz + b)).or_insert(0) |= bit;
                }
            }
        }
        for (k, m) in side {
            *self.dirty.entry(k).or_insert(0) |= grow(m);
        }
        // what changed at the border goes on into the neighbours the
        // ordinary way
        for (x, y, z) in edge {
            for (ox, oz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                let (a, b) = (x + ox, z + oz);
                if !(0..16).contains(&a) || !(0..16).contains(&b) {
                    self.update_light(sky, wx0 + a, y, wz0 + b);
                }
            }
        }
        true
    }
}
