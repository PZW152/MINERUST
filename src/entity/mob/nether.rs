//! `EntityGhast`, `EntityPigZombie`, `EntityBlaze`, `EntityMagmaCube`, the
//! wither skeleton, and the fireballs (`EntityLargeFireball`,
//! `EntitySmallFireball`).

use super::*;

/// `EntityFireball`: a large one explodes, a small one sets fires.
#[derive(Clone, Debug)]
pub struct Fireball {
    pub pos: [f64; 3],
    pub prev_pos: [f64; 3],
    motion: [f64; 3],
    acc: [f64; 3],
    pub small: bool,
    shooter: u32,
    ticks: i32,
}

impl Fireball {
    /// `shootingEntity`
    pub fn shooter_id(&self) -> u32 {
        self.shooter
    }

    pub fn ticks_alive(&self) -> i32 {
        self.ticks
    }

    /// `new EntityFireball(world, shooter, ax, ay, az)` placed at `pos`.
    pub fn new(
        r: &mut JavaRandom,
        shooter: u32,
        pos: [f64; 3],
        dir: [f64; 3],
        small: bool,
    ) -> Fireball {
        let mut a = dir;
        for v in &mut a {
            *v += r.next_gaussian() * 0.4;
        }
        let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt().max(1e-9);
        Fireball {
            pos,
            prev_pos: pos,
            motion: [0.0; 3],
            acc: [a[0] / l * 0.1, a[1] / l * 0.1, a[2] / l * 0.1],
            small,
            shooter,
            ticks: 0,
        }
    }
}

/// What a fireball ran into this tick.
pub enum Impact {
    Block([i32; 3], [i32; 3]),
    Player,
    Mob(u32),
}

/// `EntityFireball.onUpdate`; returns the impact if there was one.
pub fn tick_fireball(
    f: &mut Fireball,
    w: &ServerWorld,
    player: &PlayerInfo,
    mobs: &[Mob],
) -> Option<(Impact, [f64; 3])> {
    f.prev_pos = f.pos;
    f.ticks += 1;
    let from = f.pos;
    let to = [
        f.pos[0] + f.motion[0],
        f.pos[1] + f.motion[1],
        f.pos[2] + f.motion[2],
    ];
    // rayTraceBlocks against collision boxes, in small steps
    let steps = ((f.motion[0].abs() + f.motion[1].abs() + f.motion[2].abs()) * 8.0)
        .ceil()
        .max(1.0) as i32;
    let mut block_hit = None;
    let mut prev_cell = [
        math::floor_double(from[0]),
        math::floor_double(from[1]),
        math::floor_double(from[2]),
    ];
    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        let p = [
            from[0] + (to[0] - from[0]) * t,
            from[1] + (to[1] - from[1]) * t,
            from[2] + (to[2] - from[2]) * t,
        ];
        let c = [
            math::floor_double(p[0]),
            math::floor_double(p[1]),
            math::floor_double(p[2]),
        ];
        let id = w.peek_block(c[0], c[1], c[2]);
        if id != block::AIR && block::collision_box(id, w.peek_meta(c[0], c[1], c[2])).is_some() {
            let side = [
                prev_cell[0] - c[0],
                prev_cell[1] - c[1],
                prev_cell[2] - c[2],
            ];
            block_hit = Some((c, side, t));
            break;
        }
        prev_cell = c;
    }
    let reach_t = block_hit.map_or(1.0, |b| b.2);
    let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let mut best: Option<(Impact, f64)> = None;
    let grow = |b: &Aabb| Aabb {
        min: [b.min[0] - 0.3, b.min[1] - 0.3, b.min[2] - 0.3],
        max: [b.max[0] + 0.3, b.max[1] + 0.3, b.max[2] + 0.3],
    };
    if player.attackable {
        if let Some(t) = ray_box(from, d, &grow(&player.bb())) {
            if t <= reach_t {
                best = Some((Impact::Player, t));
            }
        }
    }
    for m in mobs {
        if m.health <= 0.0
            || (m.id == f.shooter && f.ticks < 25)
            || m.kind == Kind::EnderCrystal && false
        {
            continue;
        }
        if let Some(t) = ray_box(from, d, &grow(&m.bb)) {
            if t <= reach_t && best.as_ref().map_or(true, |b| t < b.1) {
                best = Some((Impact::Mob(m.id), t));
            }
        }
    }
    let impact = match best {
        Some((i, t)) => Some((i, t)),
        None => block_hit.map(|(c, s, t)| (Impact::Block(c, s), t)),
    };
    let at = f.pos;
    f.pos = to;
    for k in 0..3 {
        f.motion[k] += f.acc[k];
        f.motion[k] *= 0.95;
    }
    impact.map(|(i, t)| (i, [at[0] + d[0] * t, at[1] + d[1] * t, at[2] + d[2] * t]))
}

/// `EntityGhast.updateEntityActionState`
pub fn ghast(
    m: &mut Mob,
    w: &mut ServerWorld,
    player: &PlayerInfo,
    balls: &mut Vec<Fireball>,
    out: &mut Outcome,
) {
    m.prev_fuse = m.fuse;
    let d = [
        m.waypoint[0] - m.pos[0],
        m.waypoint[1] - m.pos[1],
        m.waypoint[2] - m.pos[2],
    ];
    let mut dist = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
    if !(1.0..=3600.0).contains(&dist) {
        m.waypoint = [
            m.pos[0] + ((m.rand.next_float() * 2.0 - 1.0) * 16.0) as f64,
            m.pos[1] + ((m.rand.next_float() * 2.0 - 1.0) * 16.0) as f64,
            m.pos[2] + ((m.rand.next_float() * 2.0 - 1.0) * 16.0) as f64,
        ];
    }
    let c = m.jump_delay;
    m.jump_delay -= 1;
    if c <= 0 {
        m.jump_delay += m.rand.next_int_n(5) + 2;
        dist = dist.sqrt();
        let d = [
            m.waypoint[0] - m.pos[0],
            m.waypoint[1] - m.pos[1],
            m.waypoint[2] - m.pos[2],
        ];
        // isCourseTraversable
        let mut ok = true;
        let step = [d[0] / dist, d[1] / dist, d[2] / dist];
        let mut bb = m.bb;
        let mut i = 1.0;
        while i < dist {
            bb.offset(step[0], step[1], step[2]);
            if !colliding_boxes(w, &bb).is_empty() {
                ok = false;
                break;
            }
            i += 1.0;
        }
        if ok {
            for k in 0..3 {
                m.motion[k] += d[k] / dist * 0.1;
            }
        } else {
            m.waypoint = m.pos;
        }
    }
    let near = player.attackable && m.dist2(player.feet) < 64.0 * 64.0;
    if near {
        let dx = player.feet[0] - m.pos[0];
        let dy = player.feet[1] + 0.9 - (m.pos[1] + 2.0);
        let dz = player.feet[2] - m.pos[2];
        m.yaw = -(dx.atan2(dz) as f32) * 180.0 / std::f32::consts::PI;
        m.body_yaw = m.yaw;
        if m.can_see(w, player.eye()) {
            if m.fuse == 10 {
                out.sounds.push((
                    "mob.ghast.charge",
                    m.pos,
                    10.0,
                    (m.rand.next_float() - m.rand.next_float()) * 0.2 + 1.0,
                ));
            }
            m.fuse += 1;
            if m.fuse == 20 {
                out.sounds.push((
                    "mob.ghast.fireball",
                    m.pos,
                    10.0,
                    (m.rand.next_float() - m.rand.next_float()) * 0.2 + 1.0,
                ));
                let yaw = m.yaw.to_radians();
                let look = [-(yaw.sin()) as f64, 0.0, yaw.cos() as f64];
                let at = [
                    m.pos[0] + look[0] * 4.0,
                    m.pos[1] + 2.0 + 0.5,
                    m.pos[2] + look[2] * 4.0,
                ];
                balls.push(Fireball::new(&mut m.rand, m.id, at, [dx, dy, dz], false));
                m.fuse = -40;
            }
        } else if m.fuse > 0 {
            m.fuse -= 1;
        }
    } else {
        m.yaw = -(m.motion[0].atan2(m.motion[2]) as f32) * 180.0 / std::f32::consts::PI;
        m.body_yaw = m.yaw;
        if m.fuse > 0 {
            m.fuse -= 1;
        }
    }
    m.head_yaw = m.yaw;
    m.angry = m.fuse > 10;
    // EntityFlying.moveEntityWithHeading
    let mm = m.motion;
    m.move_entity(w, mm[0], mm[1], mm[2], false);
    for v in &mut m.motion {
        *v *= 0.91;
    }
}

/// `EntityBlaze.onLivingUpdate` / `attackEntity` (old AI: walks at the
/// player and lobs three fireballs per volley).
pub fn blaze(
    m: &mut Mob,
    w: &mut ServerWorld,
    player: &PlayerInfo,
    balls: &mut Vec<Fireball>,
    out: &mut Outcome,
) {
    if m.wet(w) {
        m.drown(1.0);
    }
    m.teleport_delay -= 1;
    if m.teleport_delay <= 0 {
        m.teleport_delay = 100;
        m.sq[0] = 0.5 + m.rand.next_gaussian() as f32 * 3.0;
    }
    if m.target_player && player.feet[1] + 1.62 > m.pos[1] + 1.53 + m.sq[0] as f64 {
        m.motion[1] += (0.3 - m.motion[1]) * 0.3;
    }
    if m.rand.next_int_n(24) == 0 {
        let v = 1.0 + m.rand.next_float();
        let p = m.rand.next_float() * 0.7 + 0.3;
        out.sounds.push((
            "fire.fire",
            [m.pos[0] + 0.5, m.pos[1] + 0.5, m.pos[2] + 0.5],
            v,
            p,
        ));
    }
    if !m.on_ground && m.motion[1] < 0.0 {
        m.motion[1] *= 0.6;
    }
    if m.target_player {
        let d2 = m.dist2(player.feet);
        let pb = player.bb();
        if m.attack_cooldown <= 0 && d2 < 4.0 && pb.max[1] > m.bb.min[1] && pb.min[1] < m.bb.max[1]
        {
            m.attack_cooldown = 20;
            let (dx, dz) = (player.feet[0] - m.pos[0], player.feet[2] - m.pos[2]);
            let f = (dx * dx + dz * dz).sqrt().max(1e-4);
            let _ = f;
            out.hits.push(Hit::Melee {
                kind: Kind::Blaze,
                by: m.id,
                amount: 6.0,
                knock: [0.0; 3],
                from: m.pos,
                effect: None,
            });
            out.attacker = Some(m.id);
        } else if d2 < 30.0 * 30.0 {
            let dx = player.feet[0] - m.pos[0];
            let dy = player.feet[1] + 0.9 - (m.pos[1] + 0.9);
            let dz = player.feet[2] - m.pos[2];
            if m.attack_cooldown == 0 {
                m.stare += 1;
                if m.stare == 1 {
                    m.attack_cooldown = 60;
                    m.angry = true;
                } else if m.stare <= 4 {
                    m.attack_cooldown = 6;
                } else {
                    m.attack_cooldown = 100;
                    m.stare = 0;
                    m.angry = false;
                }
                if m.stare > 1 {
                    let spread = (d2.sqrt() as f32).sqrt() * 0.5;
                    out.sounds.push((
                        "mob.ghast.fireball",
                        m.pos,
                        10.0,
                        (m.rand.next_float() - m.rand.next_float()) * 0.2 + 1.0,
                    ));
                    let g1 = m.rand.next_gaussian() * spread as f64;
                    let g2 = m.rand.next_gaussian() * spread as f64;
                    let at = [m.pos[0], m.pos[1] + 0.9 + 0.5, m.pos[2]];
                    balls.push(Fireball::new(
                        &mut m.rand,
                        m.id,
                        at,
                        [dx + g1, dy, dz + g2],
                        true,
                    ));
                }
            }
            m.yaw = (dz.atan2(dx) * 180.0 / std::f64::consts::PI) as f32 - 90.0;
        }
    }
}
