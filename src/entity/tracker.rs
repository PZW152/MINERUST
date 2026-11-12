//! `EntityTrackerEntry` for object entities (boats, minecarts): when the
//! client is told an entity moved (`S14`/`S18`), its velocity (`S12`) and
//! its data watcher (`S1C`), and the packets' rounding.

use super::base::Ent;
use crate::util::math;

/// `Entity$EnumEntitySize`, from `setSize`'s width.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    S1,
    S2,
    S3,
    S4,
    S5,
    S6,
}

impl Size {
    /// `Entity.setSize`: `width % 2.0F`.
    pub fn of(width: f32) -> Size {
        let f = width % 2.0;
        if f < 0.375 {
            Size::S1
        } else if f < 0.75 {
            Size::S2
        } else if f < 1.0 {
            Size::S3
        } else if f < 1.375 {
            Size::S4
        } else if f < 1.75 {
            Size::S5
        } else {
            Size::S6
        }
    }

    /// `multiplyBy32AndRound`
    pub fn mul32(self, v: f64) -> i32 {
        let d = v - (math::floor_double(v) as f64 + 0.5);
        let fl = math::floor_double(v * 32.0);
        let ce = (v * 32.0).ceil() as i32;
        match self {
            Size::S1 => {
                if if d < 0.0 { d >= -0.3125 } else { d >= 0.3125 } {
                    fl
                } else {
                    ce
                }
            }
            Size::S2 => {
                if if d < 0.0 { d >= -0.3125 } else { d >= 0.3125 } {
                    ce
                } else {
                    fl
                }
            }
            Size::S3 => {
                if d > 0.0 {
                    fl
                } else {
                    ce
                }
            }
            Size::S4 => {
                if if d < 0.0 { d >= -0.1875 } else { d >= 0.1875 } {
                    fl
                } else {
                    ce
                }
            }
            Size::S5 => {
                if if d < 0.0 { d >= -0.1875 } else { d >= 0.1875 } {
                    ce
                } else {
                    fl
                }
            }
            Size::S6 => {
                if d > 0.0 {
                    ce
                } else {
                    fl
                }
            }
        }
    }
}

/// `MathHelper.floor_float(angle * 256.0F / 360.0F)`
pub fn angle_byte(a: f32) -> i32 {
    math::floor_float(a * 256.0 / 360.0)
}

/// The client's reading of a packet angle: `(byte) * 360 / 256.0F`.
pub fn byte_angle(a: i32) -> f32 {
    (a as i8) as i32 as f32 * 360.0 / 256.0
}

/// `S12PacketEntityVelocity`'s rounding: clamped to 3.9, times 8000.
pub fn velocity_ints(m: [f64; 3]) -> [i32; 3] {
    let c = |v: f64| (v.clamp(-3.9, 3.9) * 8000.0) as i32;
    [c(m[0]), c(m[1]), c(m[2])]
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Packet {
    /// `S0EPacketSpawnObject`: `floor(pos * 32)`, angles, and the velocity
    /// sent right after (`sendVelocityUpdates`).
    Spawn {
        pos: [i32; 3],
        yaw: i32,
        pitch: i32,
        velocity: Option<[i32; 3]>,
    },
    /// `S15`/`S17`: a move by 1/32 steps, with the angles when they changed.
    RelMove {
        d: [i32; 3],
        look: Option<(i32, i32)>,
    },
    /// `S16PacketEntityLook`
    Look(i32, i32),
    /// `S18PacketEntityTeleport`
    Teleport { pos: [i32; 3], yaw: i32, pitch: i32 },
    /// `S12PacketEntityVelocity`
    Velocity([i32; 3]),
    /// `S13PacketDestroyEntities`
    Destroy,
}

#[derive(Clone, Debug)]
pub struct Tracker {
    /// `blocksDistanceThreshold`
    pub range: i32,
    freq: i32,
    send_velocity: bool,
    size: Size,
    last: [i32; 3],
    last_yaw: i32,
    last_pitch: i32,
    motion: [f64; 3],
    ticks: i32,
    since_teleport: i32,
    /// `ridingEntity` (the flag): the last update was while riding.
    riding: bool,
    /// `posX/Y/Z` and `isDataInitialized`: where the tracking was last
    /// checked.
    checked: Option<[f64; 3]>,
    /// Whether the player sees it.
    pub watching: bool,
}

impl Tracker {
    /// `EntityTracker.addEntityToTracker(entity, range, frequency, velocity)`
    pub fn new(e: &Ent, range: i32, freq: i32, send_velocity: bool) -> Tracker {
        Tracker {
            range,
            freq,
            send_velocity,
            size: Size::of(e.width),
            last: [
                math::floor_double(e.pos[0] * 32.0),
                math::floor_double(e.pos[1] * 32.0),
                math::floor_double(e.pos[2] * 32.0),
            ],
            last_yaw: angle_byte(e.yaw),
            last_pitch: angle_byte(e.pitch),
            motion: [0.0; 3],
            ticks: 0,
            since_teleport: 0,
            riding: false,
            checked: None,
            watching: false,
        }
    }

    /// `updatePlayerEntity` / `tryStartWachingThis`: whether the player at
    /// `player` (feet), whose view holds the entity's chunk when `in_view`,
    /// should see it. Returns the spawn or destroy packet on a change.
    pub fn update_watcher(&mut self, e: &Ent, player: [f64; 3], in_view: bool) -> Option<Packet> {
        let dx = player[0] - self.last[0] as f64 / 32.0;
        let dz = player[2] - self.last[2] as f64 / 32.0;
        let r = self.range as f64;
        if dx >= -r && dx <= r && dz >= -r && dz <= r {
            if !self.watching && in_view {
                self.watching = true;
                // S0EPacketSpawnObject, then S12 (sendVelocityUpdates)
                return Some(Packet::Spawn {
                    pos: [
                        math::floor_double(e.pos[0] * 32.0),
                        math::floor_double(e.pos[1] * 32.0),
                        math::floor_double(e.pos[2] * 32.0),
                    ],
                    yaw: angle_byte(e.yaw),
                    pitch: angle_byte(e.pitch),
                    velocity: self.send_velocity.then(|| velocity_ints(e.motion)),
                });
            }
        } else if self.watching {
            self.watching = false;
            return Some(Packet::Destroy);
        }
        None
    }

    /// `sendLocationToAllClients`, up to the watchers: `airborne` is
    /// `isAirBorne`, `changed` the data watcher's `hasChanges`,
    /// `velocity_changed` `velocityChanged`. `player_moved` stands for the
    /// player's own entry having moved four blocks (the watchers are then
    /// re-checked as `EntityTracker.updateTrackedEntities` does).
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        e: &Ent,
        airborne: &mut bool,
        changed: bool,
        velocity_changed: &mut bool,
        player: [f64; 3],
        player_moved: bool,
        in_view: bool,
        out: &mut Vec<Packet>,
    ) {
        let far = match self.checked {
            None => true,
            Some(c) => {
                (e.pos[0] - c[0]).powi(2) + (e.pos[1] - c[1]).powi(2) + (e.pos[2] - c[2]).powi(2)
                    > 16.0
            }
        };
        if far {
            self.checked = Some(e.pos);
        }
        if far || player_moved {
            if let Some(p) = self.update_watcher(e, player, in_view) {
                out.push(p);
            }
        }
        if self.ticks % self.freq == 0 || *airborne || changed {
            if e.riding.is_none() {
                self.since_teleport += 1;
                let x = self.size.mul32(e.pos[0]);
                let y = math::floor_double(e.pos[1] * 32.0);
                let z = self.size.mul32(e.pos[2]);
                let yaw = angle_byte(e.yaw);
                let pitch = angle_byte(e.pitch);
                let d = [x - self.last[0], y - self.last[1], z - self.last[2]];
                let moved = d.iter().any(|v| v.abs() >= 4) || self.ticks % 60 == 0;
                let rotated =
                    (yaw - self.last_yaw).abs() >= 4 || (pitch - self.last_pitch).abs() >= 4;
                let mut packet = None;
                if self.ticks > 0 {
                    if d.iter().any(|&v| !(-128..128).contains(&v))
                        || self.since_teleport > 400
                        || self.riding
                    {
                        self.since_teleport = 0;
                        packet = Some(Packet::Teleport {
                            pos: [x, y, z],
                            yaw,
                            pitch,
                        });
                    } else if moved && rotated {
                        packet = Some(Packet::RelMove {
                            d,
                            look: Some((yaw, pitch)),
                        });
                    } else if moved {
                        packet = Some(Packet::RelMove { d, look: None });
                    } else if rotated {
                        packet = Some(Packet::Look(yaw, pitch));
                    }
                }
                if self.send_velocity {
                    let dm = [
                        e.motion[0] - self.motion[0],
                        e.motion[1] - self.motion[1],
                        e.motion[2] - self.motion[2],
                    ];
                    let k = 0.02;
                    let d2 = dm[0] * dm[0] + dm[1] * dm[1] + dm[2] * dm[2];
                    if d2 > k * k || d2 > 0.0 && e.motion == [0.0; 3] {
                        self.motion = e.motion;
                        if self.watching {
                            out.push(Packet::Velocity(velocity_ints(e.motion)));
                        }
                    }
                }
                if let Some(p) = packet {
                    if self.watching {
                        out.push(p);
                    }
                }
                if moved {
                    self.last = [x, y, z];
                }
                if rotated {
                    self.last_yaw = yaw;
                    self.last_pitch = pitch;
                }
                self.riding = false;
            } else {
                let yaw = angle_byte(e.yaw);
                let pitch = angle_byte(e.pitch);
                if (yaw - self.last_yaw).abs() >= 4 || (pitch - self.last_pitch).abs() >= 4 {
                    if self.watching {
                        out.push(Packet::Look(yaw, pitch));
                    }
                    self.last_yaw = yaw;
                    self.last_pitch = pitch;
                }
                self.last = [
                    self.size.mul32(e.pos[0]),
                    math::floor_double(e.pos[1] * 32.0),
                    self.size.mul32(e.pos[2]),
                ];
                self.riding = true;
            }
            *airborne = false;
        }
        self.ticks += 1;
        if *velocity_changed {
            if self.watching {
                out.push(Packet::Velocity(velocity_ints(e.motion)));
            }
            *velocity_changed = false;
        }
    }
}

/// The client's side of the packets: `serverPosX/Y/Z` and the position and
/// angles `setPositionAndRotation2` is called with.
#[derive(Clone, Copy, Debug, Default)]
pub struct ServerPos {
    pub pos: [i32; 3],
}

impl ServerPos {
    /// `handleEntityMovement` / `handleEntityTeleport`: the target position
    /// and angles, or `None` for packets that do not move the entity.
    pub fn apply(&mut self, p: &Packet, yaw: f32, pitch: f32) -> Option<([f64; 3], f32, f32)> {
        let at = |s: [i32; 3], dy: f64| {
            [
                s[0] as f64 / 32.0,
                s[1] as f64 / 32.0 + dy,
                s[2] as f64 / 32.0,
            ]
        };
        match *p {
            Packet::RelMove { d, look } => {
                for i in 0..3 {
                    self.pos[i] += d[i];
                }
                let (y, pi) = look.map_or((yaw, pitch), |(a, b)| (byte_angle(a), byte_angle(b)));
                Some((at(self.pos, 0.0), y, pi))
            }
            Packet::Look(a, b) => Some((at(self.pos, 0.0), byte_angle(a), byte_angle(b))),
            Packet::Teleport { pos, yaw, pitch } => {
                self.pos = pos;
                Some((at(pos, 0.015625), byte_angle(yaw), byte_angle(pitch)))
            }
            _ => None,
        }
    }
}
