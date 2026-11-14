//! Beds on the server: `BlockBed.onBlockActivated`, `EntityPlayer`'s
//! `sleepInBedAt` / `wakeUpPlayer` / sleep timer, `WorldServer`'s
//! all-asleep check that skips the night, and the bed as the spawn point
//! (`verifyRespawnCoordinates`).

use super::*;
use crate::block;

/// `BlockBed.field_149981_a`: from the foot to the head, by direction.
pub const BED_DIRECTIONS: [[i32; 2]; 4] = [[0, 1], [-1, 0], [0, -1], [1, 0]];

/// `BlockBed.getSafeExitLocation(world, x, y, z, 0)`
pub fn safe_exit(
    w: &mut crate::world::server::ServerWorld,
    x: i32,
    y: i32,
    z: i32,
) -> Option<[i32; 3]> {
    let d = (w.meta(x, y, z) & 3) as usize;
    for k in 0..=1 {
        let x0 = x - BED_DIRECTIONS[d][0] * k - 1;
        let z0 = z - BED_DIRECTIONS[d][1] * k - 1;
        for bx in x0..=x0 + 2 {
            for bz in z0..=z0 + 2 {
                if w.solid_top_surface(bx, y - 1, bz)
                    && !w.material(bx, y, bz).is_opaque()
                    && !w.material(bx, y + 1, bz).is_opaque()
                {
                    return Some([bx, y, bz]);
                }
            }
        }
    }
    None
}

/// `BlockBed.setBedOccupied`
fn set_occupied(w: &mut crate::world::server::ServerWorld, p: [i32; 3], on: bool) {
    let m = w.meta(p[0], p[1], p[2]);
    let m = if on { m | 4 } else { m & !4 };
    w.set_meta(p[0], p[1], p[2], m, 4);
}

impl Sim {
    /// `BlockBed.onBlockActivated` on the server.
    pub(super) fn use_bed(&mut self, target: [i32; 3]) {
        let [mut x, y, mut z] = target;
        let mut meta = self.world.meta(x, y, z);
        if meta & 8 == 0 {
            let d = (meta & 3) as usize;
            x += BED_DIRECTIONS[d][0];
            z += BED_DIRECTIONS[d][1];
            if self.world.block(x, y, z) != block::BED {
                return;
            }
            meta = self.world.meta(x, y, z);
        }
        // WorldProvider.canRespawnHere: the surface only
        if self.dim != 0 {
            // the bed explodes where its head was found
            self.world.set_air(x, y, z);
            let d = (meta & 3) as usize;
            let (fx, fz) = (x + BED_DIRECTIONS[d][0], z + BED_DIRECTIONS[d][1]);
            if self.world.block(fx, y, fz) == block::BED {
                self.world.set_air(fx, y, fz);
            }
            self.mobs
                .queued_explosions
                .push(crate::world::explosion::Request {
                    pos: [fx as f64 + 0.5, y as f64 + 0.5, fz as f64 + 0.5],
                    size: 5.0,
                    flaming: true,
                    smoking: true,
                    exploder: crate::world::explosion::Exploder::None,
                    exclude: None,
                });
            return;
        }
        if meta & 4 != 0 {
            // isBedOccupied: only this player could be in it
            if self.sleeping == Some([x, y, z]) {
                self.chat_out
                    .push(crate::chat::ChatMsg::tr("tile.bed.occupied", vec![]));
                return;
            }
            set_occupied(&mut self.world, [x, y, z], false);
        }
        match self.sleep_in_bed_at([x, y, z]) {
            Ok(()) => set_occupied(&mut self.world, [x, y, z], true),
            Err(Some(key)) => self.chat_out.push(crate::chat::ChatMsg::tr(key, vec![])),
            Err(None) => {}
        }
    }

    /// `EntityPlayer.sleepInBedAt` (with `EntityPlayerMP`'s packets): the
    /// error is the chat message `BlockBed` shows, if any.
    fn sleep_in_bed_at(&mut self, p: [i32; 3]) -> Result<(), Option<&'static str>> {
        if self.sleeping.is_some() || self.splayer.dead || self.splayer.living.health <= 0.0 {
            return Err(None);
        }
        if self.dim != 0 {
            return Err(None);
        }
        // World.isDaytime
        if self.world.skylight_subtracted < 4 {
            return Err(Some("tile.bed.noSleep"));
        }
        let (feet, _) = self.play.player;
        if (feet[0] - p[0] as f64).abs() > 3.0
            || (feet[1] - p[1] as f64).abs() > 2.0
            || (feet[2] - p[2] as f64).abs() > 3.0
        {
            return Err(None);
        }
        // getEntitiesWithinAABB(EntityMob.class, +-8, +-5, +-8)
        use crate::entity::mob::Kind as K;
        let (x0, y0, z0) = (p[0] as f64 - 8.0, p[1] as f64 - 5.0, p[2] as f64 - 8.0);
        let (x1, y1, z1) = (p[0] as f64 + 8.0, p[1] as f64 + 5.0, p[2] as f64 + 8.0);
        // a dying mob is still in the world's entity list
        let near = self.mobs.list.iter().any(|m| {
            matches!(
                m.kind,
                K::Zombie
                    | K::PigZombie
                    | K::Skeleton
                    | K::Creeper
                    | K::Spider
                    | K::CaveSpider
                    | K::Enderman
                    | K::Silverfish
                    | K::Witch
                    | K::Blaze
            ) && m.bb.max[0] > x0
                && m.bb.min[0] < x1
                && m.bb.max[1] > y0
                && m.bb.min[1] < y1
                && m.bb.max[2] > z0
                && m.bb.min[2] < z1
        });
        if near {
            return Err(Some("tile.bed.notSafe"));
        }
        if self.riding.is_some() {
            self.dismount();
        }
        let d = self.world.meta(p[0], p[1], p[2]) & 3;
        let (mut fx, mut fz) = (0.5f32, 0.5f32);
        match d {
            0 => fz = 0.9,
            1 => fx = 0.1,
            2 => fz = 0.1,
            _ => fx = 0.9,
        }
        // setPosition(x + f1, y + 0.9375, z + f) with yOffset 0.2 on the
        // server's player (posY is the feet there before, here the
        // position is what the client's posY becomes)
        let pos = [
            p[0] as f64 + fx as f64,
            p[1] as f64 + 0.9375f32 as f64,
            p[2] as f64 + fz as f64,
        ];
        self.sleeping = Some(p);
        self.sleep_timer = 0;
        self.splayer.e.motion = [0.0; 3];
        self.sleep_event = Some(Some((p, pos)));
        Ok(())
    }

    /// `EntityPlayer.wakeUpPlayer(immediately, updateWorldFlag, setSpawn)`
    pub(super) fn wake_up(&mut self, immediately: bool, set_spawn: bool) {
        let Some(loc) = self.sleeping else { return };
        let mut exit = None;
        if self.world.block(loc[0], loc[1], loc[2]) == block::BED {
            set_occupied(&mut self.world, loc, false);
            let e = safe_exit(&mut self.world, loc[0], loc[1], loc[2]).unwrap_or([
                loc[0],
                loc[1] + 1,
                loc[2],
            ]);
            // setPosition(x + 0.5, y + yOffset + 0.1, z + 0.5): the feet
            exit = Some([
                e[0] as f64 + 0.5,
                e[1] as f64 + 0.1f32 as f64,
                e[2] as f64 + 0.5,
            ]);
        }
        self.sleeping = None;
        self.sleep_timer = if immediately { 0 } else { 100 };
        if set_spawn {
            // setSpawnChunk(playerLocation, false)
            self.bed_spawn = Some(loc);
        }
        if let Some(e) = exit {
            self.play.player.0 = e;
        }
        self.sleep_event = Some(None);
        self.wake_exit = exit;
    }

    /// `EntityPlayer.onUpdate`'s sleep part on the server, then
    /// `WorldServer.tick`'s `areAllPlayersAsleep`: with the only player
    /// fully asleep the night is skipped and the weather cleared.
    /// `EntityPlayer.attackEntityFrom`: a sleeping player that is attacked
    /// gets up (`wakeUpPlayer(true, true, false)`).
    pub(super) fn wake_if_hurt(&mut self) {
        if std::mem::take(&mut self.splayer.attacked) && self.sleeping.is_some() {
            self.wake_up(true, false);
        }
    }

    pub(super) fn tick_sleep(&mut self) {
        self.wake_if_hurt();
        if let Some(loc) = self.sleeping {
            self.sleep_timer = (self.sleep_timer + 1).min(100);
            if self.world.block(loc[0], loc[1], loc[2]) != block::BED {
                self.wake_up(true, false);
            } else if self.world.skylight_subtracted < 4 {
                self.wake_up(false, true);
            }
        } else if self.sleep_timer > 0 {
            self.sleep_timer += 1;
            if self.sleep_timer >= 110 {
                self.sleep_timer = 0;
            }
        }
        if self.sleeping.is_some() && self.sleep_timer >= 100 {
            // doDaylightCycle: the next morning
            let t = self.world_time + 24000;
            self.world_time = t - t % 24000;
            self.wake_up(false, true);
            // resetRainAndThunder
            self.weather.rain_time = 0;
            self.weather.raining = false;
            self.weather.thunder_time = 0;
            self.weather.thundering = false;
        }
    }

    /// `ServerConfigurationManager.respawnPlayer`: the bed, if it still
    /// stands (`verifyRespawnCoordinates`, not forced), else the world
    /// spawn and `S2BPacketChangeGameState(0)` ("tile.bed.notValid").
    pub(super) fn respawn_point(&mut self) -> [f64; 3] {
        if let Some(b) = self.bed_spawn.take() {
            if self.world.block(b[0], b[1], b[2]) == block::BED {
                if let Some(e) = safe_exit(&mut self.world, b[0], b[1], b[2]) {
                    self.bed_spawn = Some(b);
                    let mut p = [
                        e[0] as f64 + 0.5,
                        e[1] as f64 + 0.1f32 as f64,
                        e[2] as f64 + 0.5,
                    ];
                    // while (!getCollidingBoundingBoxes(player, bb).isEmpty())
                    //     setPosition(posX, posY + 1, posZ)
                    while self.box_blocked(p) {
                        p[1] += 1.0;
                    }
                    return p;
                }
            }
            self.chat_out
                .push(crate::chat::ChatMsg::tr("tile.bed.notValid", vec![]));
        }
        self.spawn
    }

    /// Whether the player's box (0.6 wide, 1.8 high, `Entity.setPosition`)
    /// with its feet at `feet` touches any collision box.
    fn box_blocked(&mut self, feet: [f64; 3]) -> bool {
        let w = (0.6f32 / 2.0) as f64;
        let bb = crate::entity::player::Aabb {
            min: [feet[0] - w, feet[1], feet[2] - w],
            max: [feet[0] + w, feet[1] + 1.8f32 as f64, feet[2] + w],
        };
        !crate::entity::player::colliding_boxes(&self.world, &bb).is_empty()
    }
}
