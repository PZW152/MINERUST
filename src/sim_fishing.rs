//! The server side of fishing: `ItemFishingRod.onItemRightClick`, the
//! `EntityFishHook` tick with its angler checks and `caughtEntity`, and
//! `handleHookRetraction`. The hook itself is `entity::fish_hook`.

use super::*;
use crate::entity::base::Fx;
use crate::entity::fish_hook::{Burst, Caught, FishHook};
use crate::entity::player::Aabb;
use crate::entity::vehicle::{Tracked, Vehicle, VEHICLE_ID};
use crate::util::math;

/// `Items.fishing_rod`
pub const FISHING_ROD: crate::item::ItemId = 346;

impl Sim {
    /// `ItemFishingRod.onItemRightClick` on the server.
    pub(super) fn use_fishing_rod(&mut self, held: crate::item::Stack) {
        if self.fish_hook.is_some() {
            let n = self.hook_retraction();
            // ItemStack.damageItem(n, player): nothing for creative players
            if n > 0 {
                let creative = self.play.mode == GameMode::Creative;
                let (left, broke) =
                    crate::enchant::damage_item(held, n, creative, &mut self.splayer.e.rand);
                if broke {
                    self.splayer.fx.push(Fx::BrokenItem(held));
                }
                let i = self.play.inv.selected;
                self.play.inv.main[i] = left;
                self.play.inv_dirty = true;
            }
        } else {
            let (feet, yaw) = self.play.player;
            // playSoundAtEntity(player, "random.bow", 0.5, 0.4 / (itemRand.nextFloat() * 0.4 + 0.8))
            let p = 0.4 / math::item_rand(|r| r.next_float() * 0.4 + 0.8);
            self.pending_out.push(crate::entity::mob::Outcome {
                sounds: vec![("random.bow", feet, 0.5, p)],
                ..Default::default()
            });
            // new Random() for the hook
            let seed = (math::math_random() * (1u64 << 48) as f64) as i64;
            let pitch = self.play.pitch;
            self.fish_hook = Some(FishHook::cast(seed, feet, yaw, pitch));
        }
    }

    /// `EntityFishHook.onUpdate` on the server, in the entity pass.
    pub(super) fn tick_fish_hook(&mut self, play: &mut Play) {
        let Some(mut h) = self.fish_hook.take() else {
            return;
        };
        let mut fx = Vec::new();
        h.base_tick(&self.world, &mut fx);
        let held = play.inv.held();
        let feet = play.player.0;
        // the server player's posY is its feet; getDistanceSqToEntity
        let d = [
            h.e.pos[0] - feet[0],
            h.e.pos[1] - feet[1],
            h.e.pos[2] - feet[2],
        ];
        let far = d[0] * d[0] + d[1] * d[1] + d[2] * d[2] > 1024.0;
        if self.splayer.dead || held.map_or(true, |s| s.id != FISHING_ROD) || far {
            // setDead; angler.fishEntity = null
            self.hook_fx(fx, Vec::new(), play);
            return;
        }
        if let Some(c) = h.caught {
            if let Some((pos, min_y, height)) = Self::caught_entity(&self.mobs, play, c) {
                h.follow(pos, min_y, height);
                self.hook_fx(fx, Vec::new(), play);
                self.fish_hook = Some(h);
                return;
            }
            h.caught = None;
        }
        // every entity with canBeCollidedWith: living mobs, boats, minecarts
        let mut targets: Vec<(Caught, Aabb)> = Vec::new();
        for m in &self.mobs.list {
            if m.health > 0.0 {
                targets.push((Caught::Mob(m.id), m.bb));
            }
        }
        for v in &play.vehicles {
            if matches!(v.v, Vehicle::Boat(_) | Vehicle::Cart(_)) && !v.v.e().dead {
                targets.push((Caught::Vehicle(v.v.id()), v.v.e().bb));
            }
        }
        let lure = crate::enchant::level(crate::enchant::LURE, held.as_ref());
        let mut bursts = Vec::new();
        let struck = h.tick(&mut self.world, &targets, lure, &mut fx, &mut bursts);
        if let Some(c) = struck {
            // attackEntityFrom(causeThrownDamage(this, angler), 0)
            let hurt = match c {
                Caught::Mob(id) => self
                    .mobs
                    .list
                    .iter_mut()
                    .find(|m| m.id == id)
                    .is_some_and(|m| m.hurt(0.0, Some(feet), true)),
                Caught::Vehicle(id) => self.attack_vehicle_in(play, id, 0.0, 0),
            };
            if hurt {
                h.caught = Some(c);
            }
        }
        self.hook_fx(fx, bursts, play);
        if !h.e.dead {
            self.fish_hook = Some(h);
        }
    }

    /// Position, `boundingBox.minY` and height of a caught entity that is
    /// still alive.
    fn caught_entity(
        mobs: &crate::entity::mob::Mobs,
        play: &Play,
        c: Caught,
    ) -> Option<([f64; 3], f64, f32)> {
        match c {
            Caught::Mob(id) => mobs
                .list
                .iter()
                .find(|m| m.id == id && m.health > 0.0)
                .map(|m| (m.pos, m.bb.min[1], m.dims().1)),
            Caught::Vehicle(id) => play
                .vehicles
                .iter()
                .find(|v| v.v.id() == id && !v.v.e().dead)
                .map(|v| (v.v.e().pos, v.v.e().bb.min[1], v.v.e().height)),
        }
    }

    /// The hook's sounds and particles; server particle packets are
    /// expanded as `NetHandlerPlayClient.handleParticles` does.
    fn hook_fx(&mut self, fx: Vec<Fx>, bursts: Vec<Burst>, play: &mut Play) {
        let mut all = fx;
        for b in bursts {
            b.expand(&mut self.av_rand, &mut all);
        }
        for f in all {
            if matches!(f, Fx::Sound(..) | Fx::Particle(..)) {
                play.fx.push(f);
            }
        }
    }

    /// `EntityFishHook.handleHookRetraction`: what reeling in costs the rod.
    fn hook_retraction(&mut self) -> i32 {
        let Some(mut h) = self.fish_hook.take() else {
            return 0;
        };
        let feet = self.play.player.0;
        let mut var1 = 0;
        if let Some(c) = h.caught {
            let d = [
                feet[0] - h.e.pos[0],
                feet[1] - h.e.pos[1],
                feet[2] - h.e.pos[2],
            ];
            let var8 = math::sqrt_double(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]) as f64;
            let var10 = 0.1;
            let pull = |m: &mut [f64; 3]| {
                m[0] += d[0] * var10;
                m[1] += d[1] * var10 + math::sqrt_double(var8) as f64 * 0.08;
                m[2] += d[2] * var10;
            };
            match c {
                Caught::Mob(id) => {
                    if let Some(m) = self.mobs.list.iter_mut().find(|m| m.id == id) {
                        pull(&mut m.motion);
                    }
                }
                Caught::Vehicle(id) => {
                    if let Some(v) = self.play.vehicles.iter_mut().find(|v| v.v.id() == id) {
                        pull(&mut v.v.e_mut().motion);
                    }
                }
            }
            var1 = 3;
        } else if h.catchable() {
            let held = self.play.inv.held();
            let luck = crate::enchant::level(crate::enchant::LUCK_OF_THE_SEA, held.as_ref());
            let lure = crate::enchant::level(crate::enchant::LURE, held.as_ref());
            let mut wr = std::mem::replace(&mut self.world.rand, JavaRandom::new(0));
            let (stack, kind) = h.fishing_result(&mut wr, luck, lure);
            self.world.rand = wr;
            // StatList.junkFishedStat / treasureFishedStat / fishCaughtStat
            let stat = ["stat.junkFished", "stat.treasureFished", "stat.fishCaught"][kind as usize];
            self.stats_file.increase_stat(stat, 1);
            let p = h.e.pos;
            let id = self.play.next_item;
            self.play.next_item += 1;
            let mut item = crate::entity::item::ItemEntity::new(id, stack, p);
            let d = [feet[0] - p[0], feet[1] - p[1], feet[2] - p[2]];
            let var9 = math::sqrt_double(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]) as f64;
            let var11 = 0.1;
            item.e.motion = [
                d[0] * var11,
                d[1] * var11 + math::sqrt_double(var9) as f64 * 0.08,
                d[2] * var11,
            ];
            self.play.spawn(item);
            // new EntityXPOrb(world, posX, posY + 0.5, posZ + 0.5, rand.nextInt(6) + 1)
            let value = h.e.rand.next_int_n(6) + 1;
            let oid = VEHICLE_ID | self.play.next_item;
            self.play.next_item += 1;
            let seed = (math::math_random() * (1u64 << 48) as f64) as i64;
            let mut r = std::mem::replace(&mut self.play.prand, JavaRandom::new(0));
            let o = crate::entity::xp_orb::XpOrb::new(
                oid,
                feet[0],
                feet[1] + 0.5,
                feet[2] + 0.5,
                value,
                &mut r,
                seed,
            );
            self.play.prand = r;
            self.play.vehicles.push(Tracked::new(Vehicle::Orb(o)));
            var1 = 1;
        }
        if h.in_ground {
            var1 = 2;
        }
        var1
    }
}
