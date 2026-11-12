//! `EntitySlime`, `EntityEnderman`, `EntitySquid`, `EntityBat`,
//! `EntityWolf`, `EntityOcelot`, `EntityHorse` and `EntityIronGolem`.

use super::*;

/// What a mob sees of the others when it picks a target.
#[derive(Clone, Copy)]
pub struct Other {
    pub id: u32,
    pub kind: Kind,
    pub pos: [f64; 3],
    pub tamed: bool,
}

/// A blow one mob deals another, applied once every mob has moved.
#[derive(Clone, Copy)]
pub struct Strike {
    pub target: u32,
    pub from: u32,
    pub amount: f32,
    pub at: [f64; 3],
    pub lift: f64,
}

/// A particle the client spawns: kind, position, motion.
pub type Puff = (&'static str, [f64; 3], [f64; 3]);

const CARRIABLE: [u8; 14] = [
    block::GRASS,
    block::DIRT,
    block::SAND,
    block::GRAVEL,
    block::YELLOW_FLOWER,
    block::RED_FLOWER,
    block::BROWN_MUSHROOM,
    block::RED_MUSHROOM,
    block::TNT,
    block::CACTUS,
    block::CLAY,
    block::PUMPKIN,
    block::MELON_BLOCK,
    block::MYCELIUM,
];

impl Mob {
    pub(super) fn wet(&self, w: &mut ServerWorld) -> bool {
        if self.in_water {
            return true;
        }
        let (x, y, z) = self.feet_block();
        w.raining && (w.can_see_sky(x, y, z) || w.can_see_sky(x, y + self.height() as i32, z))
    }

    /// `EntityEnderman.teleportTo`
    fn teleport_to(
        &mut self,
        w: &mut ServerWorld,
        x: f64,
        y: f64,
        z: f64,
        out: &mut Outcome,
    ) -> bool {
        let old = self.pos;
        let (bx, mut by, bz) = (
            math::floor_double(x),
            math::floor_double(y),
            math::floor_double(z),
        );
        let mut ny = y;
        if !w.loaded(bx, bz) {
            return false;
        }
        let mut found = false;
        while !found && by > 0 {
            if w.material(bx, by - 1, bz).blocks_movement() {
                found = true;
            } else {
                ny -= 1.0;
                by -= 1;
            }
        }
        if !found {
            return false;
        }
        self.set_position(x, ny, z);
        if !colliding_boxes(w, &self.bb).is_empty()
            || self.in_material(w, &self.bb, Material::Water)
            || self.in_material(w, &self.bb, Material::Lava)
        {
            self.set_position(old[0], old[1], old[2]);
            return false;
        }
        self.prev_pos = self.pos;
        self.path.clear();
        for i in 0..128 {
            let t = i as f64 / 127.0;
            let r = &mut self.rand;
            let m = [
                ((r.next_float() - 0.5) * 0.2) as f64,
                ((r.next_float() - 0.5) * 0.2) as f64,
                ((r.next_float() - 0.5) * 0.2) as f64,
            ];
            let p = [
                old[0] + (self.pos[0] - old[0]) * t + (r.next_double() - 0.5) * 0.6 * 2.0,
                old[1] + (self.pos[1] - old[1]) * t + r.next_double() * 2.9,
                old[2] + (self.pos[2] - old[2]) * t + (r.next_double() - 0.5) * 0.6 * 2.0,
            ];
            if i % 4 == 0 {
                out.particles.push(("portal", p, m));
            }
        }
        out.sounds.push(("mob.endermen.portal", old, 1.0, 1.0));
        out.sounds.push(("mob.endermen.portal", self.pos, 1.0, 1.0));
        true
    }

    fn teleport_randomly(&mut self, w: &mut ServerWorld, out: &mut Outcome) -> bool {
        let x = self.pos[0] + (self.rand.next_double() - 0.5) * 64.0;
        let y = self.pos[1] + (self.rand.next_int_n(64) - 32) as f64;
        let z = self.pos[2] + (self.rand.next_double() - 0.5) * 64.0;
        self.teleport_to(w, x, y, z, out)
    }

    /// `EntityEnderman.shouldAttackPlayer`: the player looks at its head.
    fn stared_at(&self, w: &ServerWorld, player: &PlayerInfo) -> bool {
        let (yaw, pitch) = (
            player.yaw.to_radians() as f64,
            player.pitch.to_radians() as f64,
        );
        let look = [
            -yaw.sin() * pitch.cos(),
            -pitch.sin(),
            yaw.cos() * pitch.cos(),
        ];
        let eye = player.eye();
        let v = [
            self.pos[0] - eye[0],
            self.bb.min[1] + self.height() as f64 / 2.0 - eye[1],
            self.pos[2] - eye[2],
        ];
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if len < 1e-6 {
            return false;
        }
        let d = (look[0] * v[0] + look[1] * v[1] + look[2] * v[2]) / len;
        d > 1.0 - 0.025 / len && self.can_see(w, eye)
    }
}

/// `EntitySlime.onUpdate` + `updateEntityActionState`.
pub fn slime(m: &mut Mob, player: &PlayerInfo, out: &mut Outcome) {
    let size = m.variant.max(1);
    m.prev_squish = m.squish;
    m.squish += (m.squish_amount - m.squish) * 0.5;
    // landing (tracked by `was_on_ground` in tick_mob)
    m.squish_amount *= 0.6;
    let near = player.attackable && m.dist2(player.feet) < 256.0;
    if near {
        // faceEntity(player, 10, 20)
        let (dx, dz) = (player.feet[0] - m.pos[0], player.feet[2] - m.pos[2]);
        let target = (dz.atan2(dx) * 180.0 / std::f64::consts::PI) as f32 - 90.0;
        m.yaw = approach(m.yaw, target, 10.0);
    }
    m.ai_speed = 0.1;
    if m.on_ground && {
        let d = m.jump_delay;
        m.jump_delay -= 1;
        d <= 0
    } {
        m.jump_delay = m.rand.next_int_n(20) + 10;
        if near {
            m.jump_delay /= 3;
        }
        m.jumping = true;
        let name = if size > 1 {
            "mob.slime.big"
        } else {
            "mob.slime.small"
        };
        let p = ((m.rand.next_float() - m.rand.next_float()) * 0.2 + 1.0) * 0.8;
        out.sounds.push((name, m.pos, 0.4 * size as f32, p));
        m.move_strafe = 1.0 - m.rand.next_float() * 2.0;
        m.move_forward = size as f32;
    } else {
        m.jumping = false;
        if m.on_ground {
            m.move_strafe = 0.0;
            m.move_forward = 0.0;
        }
    }
    // onCollideWithPlayer
    if size > 1 && player.attackable && m.attack_cooldown <= 0 {
        let r = 0.6 * size as f64;
        if m.dist2(player.feet) < r * r && m.bb.intersects(&player.bb().offset_copy(0.0, 0.0, 0.0))
        {
            m.attack_cooldown = 10;
            let (dx, dz) = (player.feet[0] - m.pos[0], player.feet[2] - m.pos[2]);
            let f = (dx * dx + dz * dz).sqrt().max(1e-4);
            let _ = f;
            out.hits.push(Hit::Melee {
                kind: Kind::Slime,
                amount: size as f32,
                knock: [0.0; 3],
                from: m.pos,
                effect: None,
            });
            out.attacker = Some(m.id);
            let p = (m.rand.next_float() - m.rand.next_float()) * 0.2 + 1.0;
            out.sounds.push(("mob.attack", m.pos, 1.0, p));
        }
    }
}

/// `EntitySlime.onUpdate` after the move: the squash on landing.
pub fn slime_landed(m: &mut Mob, was_on_ground: bool, out: &mut Outcome) {
    let size = m.variant.max(1);
    if m.on_ground && !was_on_ground {
        for _ in 0..size * 8 {
            let a = m.rand.next_float() * std::f32::consts::PI * 2.0;
            let f = m.rand.next_float() * 0.5 + 0.5;
            let dx = (math::sin(a) * size as f32 * 0.5 * f) as f64;
            let dz = (math::cos(a) * size as f32 * 0.5 * f) as f64;
            out.particles.push((
                "slime",
                [m.pos[0] + dx, m.bb.min[1], m.pos[2] + dz],
                [0.0; 3],
            ));
        }
        if size > 2 {
            let name = "mob.slime.big";
            let p = ((m.rand.next_float() - m.rand.next_float()) * 0.2 + 1.0) / 0.8;
            out.sounds.push((name, m.pos, 0.4 * size as f32, p));
        }
        m.squish_amount = -0.5;
    } else if !m.on_ground && was_on_ground {
        m.squish_amount = 1.0;
    }
}

/// `EntityEnderman.onLivingUpdate` and `findPlayerToAttack`.
pub fn enderman(
    m: &mut Mob,
    w: &mut ServerWorld,
    player: &PlayerInfo,
    day: bool,
    out: &mut Outcome,
) {
    let wet = m.wet(w);
    if wet {
        m.damage(1.0, None);
    }
    // mobGriefing: pick up or put down a block
    if m.carried == 0 {
        if m.rand.next_int_n(20) == 0 {
            let x = math::floor_double(m.pos[0] - 2.0 + m.rand.next_double() * 4.0);
            let y = math::floor_double(m.pos[1] + m.rand.next_double() * 3.0);
            let z = math::floor_double(m.pos[2] - 2.0 + m.rand.next_double() * 4.0);
            let id = w.block(x, y, z);
            if CARRIABLE.contains(&id) {
                m.carried = id;
                m.carried_meta = w.meta(x, y, z) as u8;
                w.set_block(x, y, z, block::AIR, 0, 3);
            }
        }
    } else if m.rand.next_int_n(2000) == 0 {
        let x = math::floor_double(m.pos[0] - 1.0 + m.rand.next_double() * 2.0);
        let y = math::floor_double(m.pos[1] + m.rand.next_double() * 2.0);
        let z = math::floor_double(m.pos[2] - 1.0 + m.rand.next_double() * 2.0);
        let below = w.block(x, y - 1, z);
        if w.material(x, y, z) == Material::Air
            && block::material(below) != Material::Air
            && block::info(below).normal
        {
            w.set_block(x, y, z, m.carried, m.carried_meta as i32, 3);
            m.carried = 0;
        }
    }
    if day {
        let (x, y, z) = m.feet_block();
        let f = brightness(w, x, y, z);
        if f > 0.5 && w.can_see_sky(x, y, z) && m.rand.next_float() * 30.0 < (f - 0.4) * 2.0 {
            m.target_player = false;
            m.angry = false;
            m.aggressive = false;
            m.teleport_randomly(w, out);
        }
    }
    if wet || m.fire > 0 {
        m.target_player = false;
        m.angry = false;
        m.aggressive = false;
        m.teleport_randomly(w, out);
    }
    if m.angry && !m.aggressive && m.rand.next_int_n(100) == 0 {
        m.angry = false;
    }
    // findPlayerToAttack (EntityCreature: every tick without a target)
    if !m.target_player && player.attackable && m.dist2(player.feet) < 64.0 * 64.0 {
        if m.stared_at(w, player) {
            m.aggressive = true;
            if m.stare == 0 {
                out.sounds
                    .push(("mob.endermen.stare", player.feet, 1.0, 1.0));
            }
            let s = m.stare;
            m.stare += 1;
            if s == 5 {
                m.stare = 0;
                m.angry = true;
                m.target_player = true;
            }
        } else {
            m.stare = 0;
        }
    }
    if !player.attackable {
        m.target_player = false;
    }
    if m.target_player {
        if m.stared_at(w, player) {
            if m.dist2(player.feet) < 16.0 {
                m.teleport_randomly(w, out);
            }
            m.teleport_delay = 0;
        } else if m.dist2(player.feet) > 256.0 {
            let d = m.teleport_delay;
            m.teleport_delay += 1;
            if d >= 30 {
                // teleportToEntity
                let e = player.eye();
                let mut v = [
                    m.pos[0] - player.feet[0],
                    m.bb.min[1] + m.height() as f64 / 2.0 - player.feet[1]
                        + (e[1] - player.feet[1]),
                    m.pos[2] - player.feet[2],
                ];
                let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
                for c in &mut v {
                    *c /= l;
                }
                let x = m.pos[0] + (m.rand.next_double() - 0.5) * 8.0 - v[0] * 16.0;
                let y = m.pos[1] + (m.rand.next_int_n(16) - 8) as f64 - v[1] * 16.0;
                let z = m.pos[2] + (m.rand.next_double() - 0.5) * 8.0 - v[2] * 16.0;
                if m.teleport_to(w, x, y, z, out) {
                    m.teleport_delay = 0;
                }
            }
        }
        chase(m, w, player, out);
        // attackingSpeedBoostModifier: 0.3 + 6.2
        m.path_speed = 6.5 / 0.3;
    } else {
        m.angry = false;
        m.teleport_delay = 0;
        wander(m, w, 1.0, 120);
    }
}

/// `EntitySquid.onLivingUpdate` / `updateEntityActionState` / move.
pub fn squid(m: &mut Mob, w: &mut ServerWorld) {
    m.prev_sq = m.sq;
    let tau = std::f32::consts::PI * 2.0;
    let pi = std::f32::consts::PI;
    m.sq[2] += m.sq_vel;
    if m.sq[2] > tau {
        m.sq[2] -= tau;
        if m.rand.next_int_n(10) == 0 {
            m.sq_vel = 1.0 / (m.rand.next_float() + 1.0) * 0.2;
        }
    }
    let water = {
        let bb = Aabb {
            min: [m.bb.min[0], m.bb.min[1] + 0.6, m.bb.min[2]],
            max: [m.bb.max[0], m.bb.max[1] - 0.6, m.bb.max[2]],
        };
        m.in_material(w, &bb, Material::Water)
    };
    m.in_water = water;
    // updateEntityActionState
    m.age += 0;
    if m.rand.next_int_n(50) == 0
        || !m.in_water
        || (m.sq_vec[0] == 0.0 && m.sq_vec[1] == 0.0 && m.sq_vec[2] == 0.0)
    {
        let a = m.rand.next_float() * tau;
        m.sq_vec = [
            math::cos(a) * 0.2,
            -0.1 + m.rand.next_float() * 0.2,
            math::sin(a) * 0.2,
        ];
    }
    if water {
        if m.sq[2] < pi {
            let f = m.sq[2] / pi;
            m.sq[3] = math::sin(f * f * pi) * pi * 0.25;
            if f > 0.75 {
                m.sq_speed = 1.0;
                m.sq_turn = 1.0;
            } else {
                m.sq_turn *= 0.8;
            }
        } else {
            m.sq[3] = 0.0;
            m.sq_speed *= 0.9;
            m.sq_turn *= 0.99;
        }
        m.motion = [
            (m.sq_vec[0] * m.sq_speed) as f64,
            (m.sq_vec[1] * m.sq_speed) as f64,
            (m.sq_vec[2] * m.sq_speed) as f64,
        ];
        let h = (m.motion[0] * m.motion[0] + m.motion[2] * m.motion[2]).sqrt() as f32;
        let target = -(m.motion[0].atan2(m.motion[2]) as f32) * 180.0 / pi;
        m.body_yaw += (target - m.body_yaw) * 0.1;
        m.yaw = m.body_yaw;
        m.head_yaw = m.body_yaw;
        m.sq[1] += pi * m.sq_turn * 1.5;
        let tp = -(h.atan2(m.motion[1] as f32)) * 180.0 / pi;
        m.sq[0] += (tp - m.sq[0]) * 0.1;
    } else {
        m.sq[3] = math::sin(m.sq[2]).abs() * pi * 0.25;
        m.motion[0] = 0.0;
        m.motion[1] -= 0.08;
        m.motion[1] *= 0.98f32 as f64;
        m.motion[2] = 0.0;
        m.sq[0] = (m.sq[0] as f64 + (-90.0 - m.sq[0] as f64) * 0.02) as f32;
        // EntityWaterMob: suffocates on land
        if m.ticks % 20 == 0 && m.rand.next_int_n(3) == 0 {
            m.damage(2.0, None);
        }
    }
    let mm = m.motion;
    m.move_entity(w, mm[0], mm[1], mm[2], false);
}

/// `EntityBat.onUpdate` / `updateAITasks`.
pub fn bat(m: &mut Mob, w: &mut ServerWorld, player: &PlayerInfo, out: &mut Outcome) {
    let (x, z) = (math::floor_double(m.pos[0]), math::floor_double(m.pos[2]));
    let up = m.pos[1] as i32 + 1;
    let ceiling = block::info(w.block(x, up, z)).normal_cube();
    if m.hanging {
        if !ceiling {
            m.hanging = false;
            out.sounds.push(("mob.bat.takeoff", m.pos, 0.05, 1.0));
        } else {
            if m.rand.next_int_n(200) == 0 {
                m.head_yaw = m.rand.next_int_n(360) as f32;
            }
            if m.dist2(player.feet) < 16.0 {
                m.hanging = false;
                out.sounds.push(("mob.bat.takeoff", m.pos, 0.05, 1.0));
            }
        }
    } else {
        if let Some(t) = m.bat_target {
            if !w.is_air(t[0], t[1], t[2]) || t[1] < 1 {
                m.bat_target = None;
            }
        }
        let (ix, iy, iz) = (m.pos[0] as i32, m.pos[1] as i32, m.pos[2] as i32);
        let near = m.bat_target.map_or(true, |t| {
            let d = [t[0] - ix, t[1] - iy, t[2] - iz];
            ((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]) as f32) < 4.0
        });
        if m.bat_target.is_none() || m.rand.next_int_n(30) == 0 || near {
            m.bat_target = Some([
                ix + m.rand.next_int_n(7) - m.rand.next_int_n(7),
                iy + m.rand.next_int_n(6) - 2,
                iz + m.rand.next_int_n(7) - m.rand.next_int_n(7),
            ]);
        }
        let t = m.bat_target.unwrap();
        let d = [
            t[0] as f64 + 0.5 - m.pos[0],
            t[1] as f64 + 0.1 - m.pos[1],
            t[2] as f64 + 0.5 - m.pos[2],
        ];
        m.motion[0] += (d[0].signum() * 0.5 - m.motion[0]) * 0.1f32 as f64;
        m.motion[1] += (d[1].signum() * 0.7f32 as f64 - m.motion[1]) * 0.1f32 as f64;
        m.motion[2] += (d[2].signum() * 0.5 - m.motion[2]) * 0.1f32 as f64;
        let target = (m.motion[2].atan2(m.motion[0]) * 180.0 / std::f64::consts::PI) as f32 - 90.0;
        m.yaw += wrap(target - m.yaw);
        m.move_forward = 0.5;
        if m.rand.next_int_n(100) == 0 && ceiling {
            m.hanging = true;
        }
    }
    if m.hanging {
        m.motion = [0.0; 3];
        let y = m.pos[1].floor() + 1.0 - m.height() as f64;
        let p = m.pos;
        m.set_position(p[0], y, p[2]);
        m.move_forward = 0.0;
    } else {
        // moveEntityWithHeading in the air (jumpMovementFactor 0.02)
        m.ai_speed = 0.7;
        let f = m.move_forward;
        m.move_with_heading(w, 0.0, f);
        m.motion[1] *= 0.6f32 as f64;
        m.fall_distance = 0.0;
    }
    m.body_yaw = m.yaw;
    if !m.hanging {
        m.head_yaw = m.yaw;
    }
}

/// Moves a tamed animal after its owner (`EntityAIFollowOwner`).
fn follow_owner(m: &mut Mob, w: &mut ServerWorld, player: &PlayerInfo) -> bool {
    let d2 = m.dist2(player.feet);
    if d2 < 4.0 {
        m.path.clear();
        return false;
    }
    if d2 < 100.0 && m.no_path() {
        return false;
    }
    if d2 >= 144.0 {
        // teleport next to the player
        let (px, py, pz) = (
            math::floor_double(player.feet[0]) - 2,
            math::floor_double(player.bb().min[1]),
            math::floor_double(player.feet[2]) - 2,
        );
        for l in 0..=4 {
            for k in 0..=4 {
                if (l < 1 || k < 1 || l > 3 || k > 3)
                    && w.solid_top_surface(px + l, py - 1, pz + k)
                    && !block::info(w.block(px + l, py, pz + k)).normal_cube()
                    && !block::info(w.block(px + l, py + 1, pz + k)).normal_cube()
                {
                    m.set_position((px + l) as f64 + 0.5, py as f64, (pz + k) as f64 + 0.5);
                    m.prev_pos = m.pos;
                    m.path.clear();
                    return true;
                }
            }
        }
    }
    m.look_at = Some(player.eye());
    if m.ticks % 10 == 0 {
        m.path_to(w, player.feet[0], player.feet[1], player.feet[2], 1.0);
    }
    true
}

/// Chases and bites another mob (`EntityAIAttackOnCollide` against a mob).
fn attack_other(
    m: &mut Mob,
    w: &mut ServerWorld,
    t: &Other,
    damage: f32,
    lift: f64,
    strikes: &mut Vec<Strike>,
    out: &mut Outcome,
) {
    m.look_at = Some([t.pos[0], t.pos[1] + 0.5, t.pos[2]]);
    if m.no_path() || m.ticks % 10 == 0 {
        if !m.path_to(w, t.pos[0], t.pos[1], t.pos[2], 1.0) {
            m.move_to = Some(t.pos);
            m.move_speed = 1.0;
        }
    }
    let (wd, _) = m.dims();
    let reach = (wd * 2.0 * wd * 2.0 + 0.6) as f64;
    if m.attack_cooldown <= 0 && m.dist2(t.pos) <= reach.max(2.0) + 1.0 {
        m.attack_cooldown = 20;
        m.is_swinging = true;
        m.swing_ticks = 0;
        strikes.push(Strike {
            target: t.id,
            from: m.id,
            amount: damage,
            at: m.pos,
            lift,
        });
        if m.kind == Kind::IronGolem {
            m.attack_timer = 10;
            out.sounds.push(("mob.irongolem.throw", m.pos, 1.0, 1.0));
        }
    }
}

/// Tamed wolves and ocelots, wolves hunting sheep, ocelots avoiding players.
#[allow(clippy::too_many_arguments)]
pub fn tameable(
    m: &mut Mob,
    w: &mut ServerWorld,
    player: &PlayerInfo,
    others: &[Other],
    strikes: &mut Vec<Strike>,
    out: &mut Outcome,
) {
    if m.sitting {
        m.path.clear();
        if m.dist2(player.feet) < 64.0 {
            m.look_at = Some(player.eye());
        }
        return;
    }
    let wolf = m.kind == Kind::Wolf;
    // target selection
    if m.tamed {
        // EntityAIOwnerHurtByTarget / EntityAIOwnerHurtTarget
        if let Some(id) = player.last_attacker.or(player.last_attacked) {
            if wolf && others.iter().any(|o| o.id == id && o.id != m.id) {
                m.target_mob = Some(id);
            }
        }
    } else if m.target_mob.is_none() {
        // EntityAITargetNonTamed: sheep for wolves, chickens for ocelots
        let (prey, chance) = if wolf {
            (Kind::Sheep, 200)
        } else {
            (Kind::Chicken, 750)
        };
        if m.rand.next_int_n(chance) == 0 {
            m.target_mob = others
                .iter()
                .filter(|o| o.kind == prey && m.dist2(o.pos) < 256.0)
                .min_by(|a, b| m.dist2(a.pos).partial_cmp(&m.dist2(b.pos)).unwrap())
                .map(|o| o.id);
        }
    }
    if let Some(id) = m.target_mob {
        match others.iter().find(|o| o.id == id) {
            Some(t) if m.dist2(t.pos) < 32.0 * 32.0 => {
                let t = *t;
                if wolf {
                    m.angry = !m.tamed;
                    // EntityAILeapAtTarget(0.4)
                    let d2 = m.dist2(t.pos);
                    if (4.0..=16.0).contains(&d2) && m.on_ground && m.rand.next_int_n(5) == 0 {
                        let (dx, dz) = (t.pos[0] - m.pos[0], t.pos[2] - m.pos[2]);
                        let f = (dx * dx + dz * dz).sqrt().max(1e-4);
                        m.motion[0] += dx / f * 0.5 * 0.8 + m.motion[0] * 0.2;
                        m.motion[2] += dz / f * 0.5 * 0.8 + m.motion[2] * 0.2;
                        m.motion[1] = 0.4f32 as f64;
                    }
                }
                let dmg = if wolf {
                    if m.tamed {
                        4.0
                    } else {
                        2.0
                    }
                } else {
                    3.0
                };
                attack_other(m, w, &t, dmg, 0.0, strikes, out);
                return;
            }
            _ => {
                m.target_mob = None;
                if wolf {
                    m.angry = false;
                }
            }
        }
    }
    if wolf && m.target_player && !m.tamed && player.attackable {
        m.angry = true;
        chase(m, w, player, out);
        return;
    }
    if m.tamed {
        if follow_owner(m, w, player) {
            return;
        }
    } else if !wolf {
        // EntityAIAvoidEntity(player, 16, 0.8, 1.33) unless the player holds fish
        let tempting = player.held == Some(349);
        if tempting && m.dist2(player.feet) < 100.0 {
            m.look_at = Some(player.eye());
            if m.dist2(player.feet) > 6.25 && (m.no_path() || m.ticks % 20 == 0) {
                m.path_to(w, player.feet[0], player.feet[1], player.feet[2], 0.6);
            }
            return;
        }
        if player.attackable && m.dist2(player.feet) < 256.0 {
            if m.task != Task::Flee || m.no_path() {
                if let Some(t) = m.random_target(w, 16, 7, Some(player.feet)) {
                    let speed = if m.dist2(player.feet) < 49.0 {
                        1.33
                    } else {
                        0.8
                    };
                    m.path_to(w, t[0], t[1], t[2], speed);
                    m.task = Task::Flee;
                }
            }
            return;
        }
        if m.task == Task::Flee {
            m.task = Task::Idle;
        }
    }
    wander(m, w, 1.0, 120);
}

/// `EntityHorse`: panic when hurt, wander, graze.
pub fn horse(m: &mut Mob, w: &mut ServerWorld, player: &PlayerInfo, out: &mut Outcome) {
    animal(m, w, player, out);
}

/// `EntityIronGolem`: defends villagers by attacking monsters.
pub fn golem(
    m: &mut Mob,
    w: &mut ServerWorld,
    player: &PlayerInfo,
    others: &[Other],
    strikes: &mut Vec<Strike>,
    out: &mut Outcome,
) {
    if m.attack_timer > 0 {
        m.attack_timer -= 1;
    }
    // EntityAINearestAttackableTarget(EntityLiving, IMob, not creepers)
    if m.target_mob.is_none() && m.rand.next_int_n(10) == 0 {
        m.target_mob = others
            .iter()
            .filter(|o| o.kind.hostile() && o.kind != Kind::Creeper && m.dist2(o.pos) < 256.0)
            .min_by(|a, b| m.dist2(a.pos).partial_cmp(&m.dist2(b.pos)).unwrap())
            .map(|o| o.id);
    }
    if let Some(id) = m.target_mob {
        match others.iter().find(|o| o.id == id) {
            Some(t) if m.dist2(t.pos) < 32.0 * 32.0 => {
                let t = *t;
                let dmg = 7.0 + m.rand.next_int_n(15) as f32;
                attack_other(m, w, &t, dmg, 0.4, strikes, out);
                return;
            }
            _ => m.target_mob = None,
        }
    }
    if m.target_player && !m.player_made && player.attackable {
        m.look_at = Some(player.eye());
        if m.no_path() || m.ticks % 10 == 0 {
            m.path_to(w, player.feet[0], player.feet[1], player.feet[2], 1.0);
        }
        let reach = (1.4f64 * 2.0).powi(2) + 0.6;
        if m.attack_cooldown <= 0 && m.dist2(player.feet) <= reach {
            m.attack_cooldown = 20;
            m.attack_timer = 10;
            let (dx, dz) = (player.feet[0] - m.pos[0], player.feet[2] - m.pos[2]);
            let f = (dx * dx + dz * dz).sqrt().max(1e-4);
            // EntityIronGolem.attackEntityAsMob: motionY += 0.4 after the hit
            let _ = f;
            out.hits.push(Hit::Melee {
                kind: Kind::IronGolem,
                amount: 7.0 + m.rand.next_int_n(15) as f32,
                knock: [0.0, 0.4f32 as f64, 0.0],
                from: m.pos,
                effect: None,
            });
            out.attacker = Some(m.id);
            out.sounds.push(("mob.irongolem.throw", m.pos, 1.0, 1.0));
        }
        return;
    }
    wander(m, w, 0.6, 120);
}

/// `Village.tick`: an iron golem now and then where enough villagers live.
pub fn village_golems(w: &mut ServerWorld, mobs: &mut Mobs) {
    let villagers: Vec<[f64; 3]> = mobs
        .list
        .iter()
        .filter(|m| m.kind == Kind::Villager && m.health > 0.0)
        .map(|m| m.pos)
        .collect();
    if villagers.len() < 10 {
        return;
    }
    let near = |a: [f64; 3], b: [f64; 3]| {
        (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2) < 32.0 * 32.0
    };
    let c = villagers[(w.total_time as usize / 20) % villagers.len()];
    let n = villagers.iter().filter(|v| near(c, **v)).count() as i32;
    let golems = mobs
        .list
        .iter()
        .filter(|m| m.kind == Kind::IronGolem && near(c, m.pos))
        .count() as i32;
    if n / 10 <= golems || w.rand.next_int_n(7000) != 0 {
        return;
    }
    for _ in 0..10 {
        let x = math::floor_double(c[0]) + w.rand.next_int_n(16) - 8;
        let y = math::floor_double(c[1]) + w.rand.next_int_n(6) - 3;
        let z = math::floor_double(c[2]) + w.rand.next_int_n(16) - 8;
        // Village.isValidIronGolemSpawningLocation
        let mut ok = w.solid_top_surface(x, y - 1, z);
        'check: for dx in -1..=1 {
            for dy in 0..3 {
                for dz in -1..=1 {
                    if block::info(w.block(x + dx, y + dy, z + dz)).normal_cube() {
                        ok = false;
                        break 'check;
                    }
                }
            }
        }
        if ok {
            let mut r = std::mem::replace(&mut w.rand, JavaRandom::new(0));
            mobs.spawn(
                Kind::IronGolem,
                [x as f64 + 0.5, y as f64, z as f64 + 0.5],
                0.0,
                &mut r,
            );
            w.rand = r;
            return;
        }
    }
}
