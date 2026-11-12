//! `EntityDragon` and `EntityEnderCrystal`.

use super::*;

/// One part of the dragon's body: centre and half width / height.
#[derive(Clone, Copy, Debug)]
pub struct Part {
    pub pos: [f64; 3],
    pub w: f64,
    pub h: f64,
}

impl Part {
    fn bb(&self) -> Aabb {
        Aabb {
            min: [
                self.pos[0] - self.w / 2.0,
                self.pos[1],
                self.pos[2] - self.w / 2.0,
            ],
            max: [
                self.pos[0] + self.w / 2.0,
                self.pos[1] + self.h,
                self.pos[2] + self.w / 2.0,
            ],
        }
    }
}

impl Mob {
    /// `EntityDragon.getMovementOffsets`
    pub fn ring_offsets(&self, back: i32, partial: f32) -> [f64; 2] {
        let partial = if self.health <= 0.0 { 0.0 } else { partial };
        let p = 1.0 - partial as f64;
        let i = (self.ring_index - back) & 63;
        let j = (self.ring_index - back - 1) & 63;
        let a = self.ring[i as usize];
        let b = self.ring[j as usize];
        let mut dy = b[0] - a[0];
        while dy >= 180.0 {
            dy -= 360.0;
        }
        while dy < -180.0 {
            dy += 360.0;
        }
        [a[0] + dy * p, a[1] + (b[1] - a[1]) * p]
    }

    /// `setNewTarget`
    fn dragon_new_target(&mut self, player: &PlayerInfo) {
        self.hanging = false;
        if self.rand.next_int_n(2) == 0 && player.attackable {
            self.target_player = true;
        } else {
            loop {
                let tx = (self.rand.next_float() * 120.0 - 60.0) as f64;
                let ty = (70.0 + self.rand.next_float() * 50.0) as f64;
                let tz = (self.rand.next_float() * 120.0 - 60.0) as f64;
                self.waypoint = [tx, ty, tz];
                let d = [self.pos[0] - tx, self.pos[1] - ty, self.pos[2] - tz];
                if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] > 100.0 {
                    break;
                }
            }
            self.target_player = false;
        }
    }

    /// The seven parts after this tick's move (`dragonPartHead` and so on).
    pub fn dragon_parts(&self) -> [Part; 7] {
        let a = self.ring_offsets(5, 1.0);
        let b = self.ring_offsets(10, 1.0);
        let tilt = ((a[1] - b[1]) * 10.0 / 180.0 * std::f64::consts::PI) as f32;
        let (c, s) = (math::cos(tilt), -math::sin(tilt));
        let yaw = self.yaw.to_radians();
        let (sy, cy) = (math::sin(yaw), math::cos(yaw));
        let p = self.pos;
        let body = Part {
            pos: [p[0] + (sy * 0.5) as f64, p[1], p[2] - (cy * 0.5) as f64],
            w: 5.0,
            h: 3.0,
        };
        let wing1 = Part {
            pos: [
                p[0] + (cy * 4.5) as f64,
                p[1] + 2.0,
                p[2] + (sy * 4.5) as f64,
            ],
            w: 4.0,
            h: 2.0,
        };
        let wing2 = Part {
            pos: [
                p[0] - (cy * 4.5) as f64,
                p[1] + 2.0,
                p[2] - (sy * 4.5) as f64,
            ],
            w: 4.0,
            h: 3.0,
        };
        let o0 = self.ring_offsets(0, 1.0);
        let hs = math::sin(yaw - self.sq[1] * 0.01);
        let hc = math::cos(yaw - self.sq[1] * 0.01);
        let head = Part {
            pos: [
                p[0] + (hs * 5.5 * c) as f64,
                p[1] + (o0[1] - a[1]) + (s * 5.5) as f64,
                p[2] - (hc * 5.5 * c) as f64,
            ],
            w: 3.0,
            h: 3.0,
        };
        let mut tails = [Part {
            pos: p,
            w: 2.0,
            h: 2.0,
        }; 3];
        for (k, t) in tails.iter_mut().enumerate() {
            let o = self.ring_offsets(12 + k as i32 * 2, 1.0);
            let mut d = o[0] - a[0];
            while d >= 180.0 {
                d -= 360.0;
            }
            while d < -180.0 {
                d += 360.0;
            }
            let ang = yaw + (d as f32).to_radians();
            let (s2, c2) = (math::sin(ang), math::cos(ang));
            let (f, g) = (1.5f32, (k as f32 + 1.0) * 2.0);
            t.pos = [
                p[0] - ((sy * f + s2 * g) * c) as f64,
                p[1] + (o[1] - a[1]) - ((g + f) * s) as f64 + 1.5,
                p[2] + ((cy * f + c2 * g) * c) as f64,
            ];
        }
        [head, body, tails[0], tails[1], tails[2], wing1, wing2]
    }
}

/// `EntityDragon.onLivingUpdate` (server side).
pub fn dragon(
    m: &mut Mob,
    w: &mut ServerWorld,
    player: &PlayerInfo,
    crystals: &[(u32, [f64; 3])],
    out: &mut Outcome,
) {
    m.prev_wing = m.wing;
    // updateDragonEnderCrystal
    if let Some(id) = m.target_mob {
        if !crystals.iter().any(|c| c.0 == id) {
            // the crystal that was healing us blew up
            m.damage(10.0, None);
            m.target_mob = None;
        } else if m.ticks % 10 == 0 && m.health < 200.0 {
            m.health += 1.0;
        }
    }
    if m.rand.next_int_n(10) == 0 {
        let mut best = None;
        let mut bd = f64::MAX;
        for &(id, p) in crystals {
            let d = m.dist2(p);
            if (p[0] - m.pos[0]).abs() < 40.0
                && (p[2] - m.pos[2]).abs() < 40.0
                && (p[1] - m.pos[1]).abs() < 36.0
                && d < bd
            {
                bd = d;
                best = Some(id);
            }
        }
        m.target_mob = best;
    }
    let speed_xz = (m.motion[0] * m.motion[0] + m.motion[2] * m.motion[2]).sqrt();
    let mut a = 0.2f32 / ((speed_xz * 10.0) as f32 + 1.0);
    a *= 2f32.powf(m.motion[1] as f32);
    m.wing += if m.hanging { a * 0.5 } else { a };
    m.yaw = wrap(m.yaw);
    if m.ring_index < 0 {
        for e in m.ring.iter_mut() {
            *e = [m.yaw as f64, m.pos[1]];
        }
    }
    m.ring_index += 1;
    if m.ring_index == 64 {
        m.ring_index = 0;
    }
    m.ring[m.ring_index as usize] = [m.yaw as f64, m.pos[1]];

    let mut d = [
        m.waypoint[0] - m.pos[0],
        m.waypoint[1] - m.pos[1],
        m.waypoint[2] - m.pos[2],
    ];
    let dist = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
    if m.target_player && player.attackable {
        m.waypoint[0] = player.feet[0];
        m.waypoint[2] = player.feet[2];
        let (dx, dz) = (m.waypoint[0] - m.pos[0], m.waypoint[2] - m.pos[2]);
        let h = (dx * dx + dz * dz).sqrt();
        let lift = (0.4f32 as f64 + h / 80.0 - 1.0).min(10.0);
        m.waypoint[1] = player.feet[1] + lift;
    } else {
        m.target_player = false;
        m.waypoint[0] += m.rand.next_gaussian() * 2.0;
        m.waypoint[2] += m.rand.next_gaussian() * 2.0;
    }
    if m.anger != 0 || dist < 100.0 || dist > 22500.0 || m.collided_h {
        m.anger = 0;
        m.dragon_new_target(player);
    }
    d[1] /= (d[0] * d[0] + d[2] * d[2]).sqrt();
    d[1] = d[1].clamp(-0.6f32 as f64, 0.6f32 as f64);
    m.motion[1] += d[1] * 0.1f32 as f64;
    m.yaw = wrap(m.yaw);
    let want = 180.0 - d[0].atan2(d[2]) * 180.0 / std::f64::consts::PI;
    let mut turn = want - m.yaw as f64;
    while turn >= 180.0 {
        turn -= 360.0;
    }
    while turn < -180.0 {
        turn += 360.0;
    }
    turn = turn.clamp(-50.0, 50.0);
    let to = {
        let v = [
            m.waypoint[0] - m.pos[0],
            m.waypoint[1] - m.pos[1],
            m.waypoint[2] - m.pos[2],
        ];
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
        [v[0] / l, v[1] / l, v[2] / l]
    };
    let yr = m.yaw.to_radians();
    let face = {
        let v = [math::sin(yr) as f64, m.motion[1], -math::cos(yr) as f64];
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
        [v[0] / l, v[1] / l, v[2] / l]
    };
    let align =
        (((face[0] * to[0] + face[1] * to[1] + face[2] * to[2]) + 0.5) as f32 / 1.5).max(0.0);
    m.sq[1] *= 0.8;
    let f18 = speed_xz as f32 + 1.0;
    let d19 = (speed_xz + 1.0).min(40.0);
    m.sq[1] += (turn * (0.7 / d19 / f18 as f64)) as f32;
    m.yaw += m.sq[1] * 0.1;
    let f21 = (2.0 / (d19 + 1.0)) as f32;
    m.move_flying(0.0, -1.0, 0.06 * (align * f21 + (1.0 - f21)));
    // noClip movement
    let k = if m.hanging { 0.8 } else { 1.0 };
    let (mx, my, mz) = (m.motion[0] * k, m.motion[1] * k, m.motion[2] * k);
    let p = m.pos;
    m.set_position(p[0] + mx, p[1] + my, p[2] + mz);
    let mv = {
        let l = (m.motion[0] * m.motion[0] + m.motion[1] * m.motion[1] + m.motion[2] * m.motion[2])
            .sqrt()
            .max(1e-9);
        [m.motion[0] / l, m.motion[1] / l, m.motion[2] / l]
    };
    let f24 =
        0.8 + 0.15 * (((mv[0] * face[0] + mv[1] * face[1] + mv[2] * face[2]) + 1.0) / 2.0) as f32;
    m.motion[0] *= f24 as f64;
    m.motion[2] *= f24 as f64;
    m.motion[1] *= 0.91f32 as f64;
    m.body_yaw = m.yaw;
    m.head_yaw = m.yaw;

    let parts = m.dragon_parts();
    // wings knock things aside, the head bites
    if m.hurt_time == 0 && player.attackable {
        let pb = player.bb();
        for wing in [parts[5], parts[6]] {
            let mut bb = wing.bb();
            bb.min = [bb.min[0] - 4.0, bb.min[1] - 2.0 - 2.0, bb.min[2] - 4.0];
            bb.max = [bb.max[0] + 4.0, bb.max[1] + 2.0 - 2.0, bb.max[2] + 4.0];
            if bb.intersects(&pb) {
                let (cx, cz) = (parts[1].pos[0], parts[1].pos[2]);
                let (dx, dz) = (player.feet[0] - cx, player.feet[2] - cz);
                let d2 = (dx * dx + dz * dz).max(0.1);
                out.hits.push(Hit::Melee {
                    kind: Kind::EnderDragon,
                    amount: 0.0,
                    knock: [dx / d2 * 4.0, 0.2f32 as f64, dz / d2 * 4.0],
                    poison: 0,
                });
            }
        }
        let mut hb = parts[0].bb();
        hb.min = [hb.min[0] - 1.0, hb.min[1] - 1.0, hb.min[2] - 1.0];
        hb.max = [hb.max[0] + 1.0, hb.max[1] + 1.0, hb.max[2] + 1.0];
        if hb.intersects(&pb) {
            out.hits.push(Hit::Melee {
                kind: Kind::EnderDragon,
                amount: 10.0,
                knock: [0.0, 0.0, 0.0],
                poison: 0,
            });
            out.attacker = Some(m.id);
        }
    }
    // destroyBlocksInAABB for the head and the body
    let mut slowed = false;
    for part in [parts[0], parts[1]] {
        let bb = part.bb();
        let mut broke = false;
        for x in math::floor_double(bb.min[0])..=math::floor_double(bb.max[0]) {
            for y in math::floor_double(bb.min[1])..=math::floor_double(bb.max[1]) {
                for z in math::floor_double(bb.min[2])..=math::floor_double(bb.max[2]) {
                    let id = w.block(x, y, z);
                    if block::material(id) != Material::Air {
                        if id != block::OBSIDIAN && id != block::END_STONE && id != block::BEDROCK {
                            broke |= w.set_block(x, y, z, block::AIR, 0, 3);
                        } else {
                            slowed = true;
                        }
                    }
                }
            }
        }
        if broke {
            let p = [
                bb.min[0] + (bb.max[0] - bb.min[0]) * m.rand.next_float() as f64,
                bb.min[1] + (bb.max[1] - bb.min[1]) * m.rand.next_float() as f64,
                bb.min[2] + (bb.max[2] - bb.min[2]) * m.rand.next_float() as f64,
            ];
            out.explosions.push([p[0], p[1], p[2], 1.0]);
        }
    }
    m.hanging = slowed;
    if m.rand.next_int_n(80) == 0 {
        out.sounds.push((
            "mob.enderdragon.growl",
            m.pos,
            5.0,
            0.8 + m.rand.next_float() * 0.3,
        ));
    }
}

/// `EntityDragon.onDeathUpdate`: rises, explodes and leaves the exit portal.
pub fn dragon_dying(m: &mut Mob, w: &mut ServerWorld, out: &mut Outcome) -> bool {
    m.death_time += 1;
    let t = m.death_time;
    if (180..=200).contains(&t) {
        let r = &mut m.rand;
        let p = [
            m.pos[0] + ((r.next_float() - 0.5) * 8.0) as f64,
            m.pos[1] + 2.0 + ((r.next_float() - 0.5) * 4.0) as f64,
            m.pos[2] + ((r.next_float() - 0.5) * 8.0) as f64,
        ];
        out.explosions.push([p[0], p[1], p[2], 4.0]);
    }
    if t > 150 && t % 5 == 0 {
        out.hits.push(Hit::Xp(1000));
    }
    if t == 1 {
        out.sounds.push(("mob.enderdragon.end", m.pos, 5.0, 1.0));
    }
    let p = m.pos;
    m.set_position(p[0], p[1] + 0.1, p[2]);
    m.yaw += 20.0;
    m.body_yaw = m.yaw;
    if t == 200 {
        out.hits.push(Hit::Xp(2000));
        exit_portal(
            w,
            math::floor_double(m.pos[0]),
            math::floor_double(m.pos[2]),
        );
        return true;
    }
    false
}

/// `EntityDragon.createEnderPortal`
fn exit_portal(w: &mut ServerWorld, x: i32, z: i32) {
    let base = 64;
    let r = 4;
    for y in base - 1..=base + 32 {
        for px in x - r..=x + r {
            for pz in z - r..=z + r {
                let (dx, dz) = ((px - x) as f64, (pz - z) as f64);
                let d = dx * dx + dz * dz;
                if d <= (r as f64 - 0.5) * (r as f64 - 0.5) {
                    if y < base {
                        if d <= (r as f64 - 1.0 - 0.5) * (r as f64 - 1.0 - 0.5) {
                            w.set_block(px, y, pz, block::BEDROCK, 0, 3);
                        }
                    } else if y > base {
                        w.set_block(px, y, pz, block::AIR, 0, 3);
                    } else if d > (r as f64 - 1.0 - 0.5) * (r as f64 - 1.0 - 0.5) {
                        w.set_block(px, y, pz, block::BEDROCK, 0, 3);
                    } else {
                        w.set_block(px, y, pz, block::END_PORTAL, 0, 3);
                    }
                }
            }
        }
    }
    w.set_block(x, base, z, block::BEDROCK, 0, 3);
    w.set_block(x, base + 1, z, block::BEDROCK, 0, 3);
    w.set_block(x, base + 2, z, block::BEDROCK, 0, 3);
    w.set_block(x - 1, base + 2, z, block::TORCH, 0, 3);
    w.set_block(x + 1, base + 2, z, block::TORCH, 0, 3);
    w.set_block(x, base + 2, z - 1, block::TORCH, 0, 3);
    w.set_block(x, base + 2, z + 1, block::TORCH, 0, 3);
    w.set_block(x, base + 3, z, block::BEDROCK, 0, 3);
    w.set_block(x, base + 4, z, block::DRAGON_EGG, 0, 3);
}

/// `EntityEnderCrystal.onUpdate`: keeps its fire burning.
pub fn crystal(m: &mut Mob, w: &mut ServerWorld) {
    m.prev_pos = m.pos;
    m.sq[2] += 1.0;
    // posY is the centre of the 2x2 box (yOffset 1)
    let (x, y, z) = (
        math::floor_double(m.pos[0]),
        math::floor_double(m.pos[1] + 1.0),
        math::floor_double(m.pos[2]),
    );
    if w.provider.dim == 1 && w.block(x, y, z) != block::FIRE {
        w.set_block(x, y, z, block::FIRE, 0, 3);
    }
}
