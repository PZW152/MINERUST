//! `EntityWither`: the spawning charge, flying after its target, the three
//! heads and their skulls (`EntityWitherSkull`), breaking blocks when hurt,
//! and the soul-sand T that makes it (`BlockSkull.makeWither`).

use super::*;

/// The wither's own fields and data watchers 17-20.
#[derive(Clone, Debug, Default)]
pub struct WitherState {
    /// `getInvulTime` (data watcher 20)
    pub invul: i32,
    /// The side heads' pitch and yaw (`xRotationHeads`, `yRotationHeads`)
    /// and the previous tick's.
    pub heads: [[f32; 2]; 2],
    pub prev_heads: [[f32; 2]; 2],
    /// Watched targets of the three heads: 0 none, `u32::MAX` the player,
    /// else a mob.
    pub targets: [u32; 3],
    next_head: [i32; 2],
    idle: [i32; 2],
    pub block_break: i32,
    /// `EntityAIArrowAttack.rangedAttackTime`
    ranged: i32,
    seen: i32,
}

/// `MathHelper.wrapAngleTo180_float`
fn wrap(a: f32) -> f32 {
    let mut a = a % 360.0;
    if a >= 180.0 {
        a -= 360.0;
    }
    if a < -180.0 {
        a += 360.0;
    }
    a
}

fn rotlerp(from: f32, to: f32, max: f32) -> f32 {
    from + wrap(to - from).clamp(-max, max)
}

impl Mob {
    /// `EntityWither.ignite`: the charge before it wakes.
    pub fn wither_ignite(&mut self) {
        self.wither.invul = 220;
        self.health = 300.0 / 3.0;
    }

    /// `isArmored`: at half health or below.
    pub fn wither_armored(&self) -> bool {
        self.health <= 150.0
    }

    fn head_x(&self, i: usize) -> f64 {
        if i == 0 {
            return self.pos[0];
        }
        let a = (self.body_yaw + 180.0 * (i as f32 - 1.0)) / 180.0 * std::f32::consts::PI;
        self.pos[0] + math::cos(a) as f64 * 1.3
    }

    fn head_y(&self, i: usize) -> f64 {
        if i == 0 {
            self.pos[1] + 3.0
        } else {
            self.pos[1] + 2.2
        }
    }

    fn head_z(&self, i: usize) -> f64 {
        if i == 0 {
            return self.pos[2];
        }
        let a = (self.body_yaw + 180.0 * (i as f32 - 1.0)) / 180.0 * std::f32::consts::PI;
        self.pos[2] + math::sin(a) as f64 * 1.3
    }

    /// `launchWitherSkullToCoords`
    fn launch_skull(&mut self, head: usize, to: [f64; 3], blue: bool, balls: &mut Vec<Fireball>, out: &mut Outcome) {
        // playAuxSFX 1014: mob.wither.shoot
        let p = 1.0 + (self.rand.next_float() - self.rand.next_float()) * 0.2;
        out.sounds.push(("mob.wither.shoot", self.pos, 2.0, p));
        let from = [self.head_x(head), self.head_y(head), self.head_z(head)];
        let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
        let mut f = Fireball::new(&mut self.rand, self.id, from, d, false);
        f.wither = if blue { 2 } else { 1 };
        balls.push(f);
    }
}

/// Where a target is: its position and eye height.
fn target_at(t: u32, player: &PlayerInfo, others: &[Other]) -> Option<([f64; 3], f64)> {
    if t == u32::MAX {
        return (player.alive && player.attackable).then_some((player.feet, 1.62));
    }
    others
        .iter()
        .find(|o| o.id == t)
        .map(|o| (o.pos, (o.height * 0.85) as f64))
}

/// `IEntitySelector` of `EntityWither`: living, not undead.
fn undead(k: Kind) -> bool {
    matches!(k, Kind::Zombie | Kind::PigZombie | Kind::Skeleton | Kind::Wither)
}

/// `EntityWither.onLivingUpdate` and `updateAITasks`.
pub fn tick(
    m: &mut Mob,
    w: &mut ServerWorld,
    player: &PlayerInfo,
    others: &[Other],
    balls: &mut Vec<Fireball>,
    out: &mut Outcome,
    explosions: &mut Vec<crate::world::explosion::Request>,
) {
    let st = &mut m.wither;
    if st.invul > 0 {
        // the charge: healing, then the blast
        let left = st.invul - 1;
        if left <= 0 {
            explosions.push(crate::world::explosion::Request {
                pos: [m.pos[0], m.pos[1] + (4.0 * 0.85) as f64, m.pos[2]],
                size: 7.0,
                flaming: false,
                // newExplosion(this, ..., 7, false, mobGriefing)
                smoking: crate::gamerules::bool("mobGriefing"),
                exploder: crate::world::explosion::Exploder::Living(m.pos),
                exclude: Some(m.id),
            });
            // playBroadcastSound 1013: mob.wither.spawn
            out.sounds.push(("mob.wither.spawn", m.pos, 1.0, 1.0));
        }
        m.wither.invul = left;
        if m.ticks % 10 == 0 {
            m.health = (m.health + 10.0).min(300.0);
        }
    } else {
        // EntityAINearestAttackableTarget / HurtByTarget for head 0: the
        // player within 40 when it can be hurt
        let me = m.pos;
        let d2 = move |p: [f64; 3]| (p[0] - me[0]).powi(2) + (p[1] - me[1]).powi(2) + (p[2] - me[2]).powi(2);
        if player.alive && player.attackable && d2(player.feet) < 40.0 * 40.0 {
            m.wither.targets[0] = u32::MAX;
        } else {
            m.wither.targets[0] = 0;
        }
        // EntityAIArrowAttack(1.0, 40, 20): head 0 fires every 40 ticks
        if let Some((p, eye)) = target_at(m.wither.targets[0], player, others) {
            let sees = m.can_see(w, [p[0], p[1] + eye, p[2]]);
            m.wither.seen = if sees { m.wither.seen + 1 } else { 0 };
            let d = d2(p);
            m.wither.ranged -= 1;
            if m.wither.ranged == 0 {
                if d <= 400.0 && sees {
                    let blue = m.rand.next_float() < 0.001;
                    m.launch_skull(0, [p[0], p[1] + eye * 0.5, p[2]], blue, balls, out);
                }
                let f = (d.sqrt() / 20.0) as f32;
                m.wither.ranged = (f * 20.0 + 20.0).floor() as i32;
            } else if m.wither.ranged < 0 {
                let f = (d.sqrt() / 20.0) as f32;
                m.wither.ranged = (f * 20.0 + 20.0).floor() as i32;
            }
            // getLookHelper: head 0 looks at it
            let (dx, dz) = (p[0] - m.pos[0], p[2] - m.pos[2]);
            let dy = p[1] + eye - (m.pos[1] + 3.4);
            m.head_yaw = (dz.atan2(dx).to_degrees() as f32) - 90.0;
            m.pitch = -(dy.atan2((dx * dx + dz * dz).sqrt()).to_degrees() as f32);
        }
        // the side heads
        for h in 1..3usize {
            if m.ticks < m.wither.next_head[h - 1] {
                continue;
            }
            m.wither.next_head[h - 1] = m.ticks + 10 + m.rand.next_int_n(10);
            let hard = player.difficulty >= 2;
            let idle = m.wither.idle[h - 1];
            m.wither.idle[h - 1] += 1;
            if hard && idle > 15 {
                let (r, ry) = (10.0, 5.0);
                let p = [
                    m.pos[0] - r + m.rand.next_double() * 2.0 * r,
                    m.pos[1] - ry + m.rand.next_double() * 2.0 * ry,
                    m.pos[2] - r + m.rand.next_double() * 2.0 * r,
                ];
                m.launch_skull(h + 1 - 1, p, true, balls, out);
                m.wither.idle[h - 1] = 0;
            }
            let t = m.wither.targets[h];
            if t != 0 {
                match target_at(t, player, others) {
                    Some((p, eye)) if d2(p) <= 900.0 && m.can_see(w, [p[0], p[1] + eye, p[2]]) => {
                        m.launch_skull(h, [p[0], p[1] + eye * 0.5, p[2]], false, balls, out);
                        m.wither.next_head[h - 1] = m.ticks + 40 + m.rand.next_int_n(20);
                        m.wither.idle[h - 1] = 0;
                    }
                    _ => m.wither.targets[h] = 0,
                }
            } else {
                // a random living thing that is not undead within 20 x 8 x 20
                let mut cands: Vec<u32> = Vec::new();
                let near = move |p: [f64; 3]| {
                    (p[0] - me[0]).abs() < 20.5 && (p[1] - me[1]).abs() < 12.0 && (p[2] - me[2]).abs() < 20.5
                };
                if player.alive && player.attackable && near(player.feet) {
                    cands.push(u32::MAX);
                }
                for o in others {
                    if o.id != m.id && !undead(o.kind) && near(o.pos) {
                        cands.push(o.id);
                    }
                }
                for _ in 0..10 {
                    if cands.is_empty() {
                        break;
                    }
                    let k = m.rand.next_int_n(cands.len() as i32) as usize;
                    let c = cands[k];
                    if let Some((p, eye)) = target_at(c, player, others) {
                        if m.can_see(w, [p[0], p[1] + eye, p[2]]) {
                            m.wither.targets[h] = c;
                            break;
                        }
                    }
                    cands.remove(k);
                }
            }
        }
        // breaking out after being hurt
        if m.wither.block_break > 0 {
            m.wither.block_break -= 1;
            if m.wither.block_break == 0 && crate::gamerules::bool("mobGriefing") {
                let (x0, y0, z0) = (
                    math::floor_double(m.pos[0]),
                    math::floor_double(m.pos[1]),
                    math::floor_double(m.pos[2]),
                );
                let mut broke = false;
                for dx in -1..=1 {
                    for dz in -1..=1 {
                        for dy in 0..=3 {
                            let (x, y, z) = (x0 + dx, y0 + dy, z0 + dz);
                            let id = w.peek_block(x, y, z);
                            if id != block::AIR
                                && !matches!(id, block::BEDROCK | block::END_PORTAL | block::END_PORTAL_FRAME | 137)
                            {
                                // World.func_147480_a(x, y, z, true): drops and the
                                // breaking effect
                                let meta = w.peek_meta(x, y, z) as i32;
                                let v = crate::drops::drop_block_as_item_with_chance(id, meta, 1.0, 0, &mut m.rand);
                                w.set_block(x, y, z, block::AIR, 0, 3);
                                w.spawn_drops(v, x, y, z);
                                w.destroyed.push(([x, y, z], id, meta));
                                broke = true;
                            }
                        }
                    }
                }
                if broke {
                    // playAuxSFX 1012: mob.zombie.woodbreak
                    out.sounds.push(("mob.zombie.woodbreak", m.pos, 2.0, 1.0));
                }
            }
        }
        if m.ticks % 20 == 0 {
            m.health = (m.health + 1.0).min(300.0);
        }
    }
    // onLivingUpdate: float up to the target and close in
    m.motion[1] *= 0.6f32 as f64;
    if m.wither.invul <= 0 {
        if let Some((p, _)) = target_at(m.wither.targets[0], player, others) {
            if m.pos[1] < p[1] || !m.wither_armored() && m.pos[1] < p[1] + 5.0 {
                if m.motion[1] < 0.0 {
                    m.motion[1] = 0.0;
                }
                m.motion[1] += (0.5 - m.motion[1]) * 0.6f32 as f64;
            }
            let (dx, dz) = (p[0] - m.pos[0], p[2] - m.pos[2]);
            let d = dx * dx + dz * dz;
            if d > 9.0 {
                let l = d.sqrt();
                m.motion[0] += (dx / l * 0.5 - m.motion[0]) * 0.6f32 as f64;
                m.motion[2] += (dz / l * 0.5 - m.motion[2]) * 0.6f32 as f64;
            }
        }
    }
    if m.motion[0] * m.motion[0] + m.motion[2] * m.motion[2] > 0.05f32 as f64 {
        m.yaw = (m.motion[2].atan2(m.motion[0]) as f32).to_degrees() - 90.0;
    }
    m.move_with_heading(w, 0.0, 0.0);
    m.body_yaw = m.yaw;
    if m.wither.invul > 0 {
        m.head_yaw = m.yaw;
    }
    // the side heads turn to their targets
    m.wither.prev_heads = m.wither.heads;
    for h in 0..2usize {
        match target_at(m.wither.targets[h + 1], player, others) {
            Some((p, eye)) => {
                let (hx, hy, hz) = (m.head_x(h + 1), m.head_y(h + 1), m.head_z(h + 1));
                let (dx, dy, dz) = (p[0] - hx, p[1] + eye - hy, p[2] - hz);
                let l = (dx * dx + dz * dz).sqrt();
                let yaw = (dz.atan2(dx).to_degrees() as f32) - 90.0;
                let pitch = -(dy.atan2(l).to_degrees() as f32);
                m.wither.heads[h][0] = rotlerp(m.wither.heads[h][0], pitch, 40.0);
                m.wither.heads[h][1] = rotlerp(m.wither.heads[h][1], yaw, 10.0);
            }
            None => m.wither.heads[h][1] = rotlerp(m.wither.heads[h][1], m.body_yaw, 10.0),
        }
    }
    // smoke at the heads, spell puffs while armoured or charging
    let armored = m.wither_armored();
    for h in 0..3usize {
        let p = [
            m.head_x(h) + m.rand.next_gaussian() * 0.3f32 as f64,
            m.head_y(h) + m.rand.next_gaussian() * 0.3f32 as f64,
            m.head_z(h) + m.rand.next_gaussian() * 0.3f32 as f64,
        ];
        out.particles.push(("smoke", p, [0.0; 3]));
        if armored && w.rand.next_int_n(4) == 0 {
            out.particles.push(("mobSpell", p, [0.7, 0.7, 0.5]));
        }
    }
    if m.wither.invul > 0 {
        for _ in 0..3 {
            let p = [
                m.pos[0] + m.rand.next_gaussian(),
                m.pos[1] + (m.rand.next_float() * 3.3) as f64,
                m.pos[2] + m.rand.next_gaussian(),
            ];
            out.particles.push(("mobSpell", p, [0.7, 0.7, 0.9]));
        }
    }
}

/// `BlockSkull.makeWither`: three wither skulls on a T of soul sand,
/// along z or x through the head just placed at `p`. The blocks to clear
/// and where the wither stands (and its yaw).
pub fn summon_shape(
    block_at: &dyn Fn(i32, i32, i32) -> u8,
    wither_skull: &dyn Fn(i32, i32, i32) -> bool,
    p: [i32; 3],
) -> Option<(Vec<[i32; 3]>, [f64; 3], f32)> {
    let [x, y, z] = p;
    if y < 2 {
        return None;
    }
    let ss = block::SOUL_SAND;
    for k in -2..=0 {
        let z0 = z + k;
        if block_at(x, y - 1, z0) == ss
            && block_at(x, y - 1, z0 + 1) == ss
            && block_at(x, y - 2, z0 + 1) == ss
            && block_at(x, y - 1, z0 + 2) == ss
            && wither_skull(x, y, z0)
            && wither_skull(x, y, z0 + 1)
            && wither_skull(x, y, z0 + 2)
        {
            let clear = vec![
                [x, y, z0],
                [x, y, z0 + 1],
                [x, y, z0 + 2],
                [x, y - 1, z0],
                [x, y - 1, z0 + 1],
                [x, y - 1, z0 + 2],
                [x, y - 2, z0 + 1],
            ];
            return Some((clear, [x as f64 + 0.5, y as f64 - 1.45, z0 as f64 + 1.5], 90.0));
        }
    }
    for k in -2..=0 {
        let x0 = x + k;
        if block_at(x0, y - 1, z) == ss
            && block_at(x0 + 1, y - 1, z) == ss
            && block_at(x0 + 1, y - 2, z) == ss
            && block_at(x0 + 2, y - 1, z) == ss
            && wither_skull(x0, y, z)
            && wither_skull(x0 + 1, y, z)
            && wither_skull(x0 + 2, y, z)
        {
            let clear = vec![
                [x0, y, z],
                [x0 + 1, y, z],
                [x0 + 2, y, z],
                [x0, y - 1, z],
                [x0 + 1, y - 1, z],
                [x0 + 2, y - 1, z],
                [x0 + 1, y - 2, z],
            ];
            return Some((clear, [x0 as f64 + 1.5, y as f64 - 1.45, z as f64 + 0.5], 0.0));
        }
    }
    None
}
