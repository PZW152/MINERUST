//! Riding a pig: `EntityPig.interact` mounting the player,
//! `Entity.updateRidden` for a player on a mob, `EntityPlayer.updateRidden`
//! (sneaking off), `ItemCarrotOnAStick` and the stick's wear from
//! `EntityAIControlledByPlayer`.

use super::*;
use crate::item::Stack;

/// `EntityPlayer.getYOffset()` on the server, where `yOffset` is 0.
const PLAYER_Y_OFFSET_MP: f64 = -0.5f32 as f64;

impl Sim {
    fn ridden_mob(&self) -> Option<usize> {
        let id = self.riding?;
        self.mobs.list.iter().position(|m| m.id == id)
    }

    /// `EntityPlayerMP.mountEntity(mob)`
    pub(super) fn mount_mob(&mut self, id: u32) {
        if self.riding.is_some() {
            return;
        }
        let Some(m) = self.mobs.list.iter_mut().find(|m| m.id == id) else {
            return;
        };
        m.ridden_by_player = true;
        self.riding = Some(id);
        self.ride = Default::default();
        self.vehicle_events.push(Event::Mount(Some(id)));
        let feet = self.play.player.0;
        self.teleport_to = Some(feet);
    }

    /// `Entity.updateRidden` for the player on a mob, right after the mobs'
    /// tick; sneaking gets off (`EntityPlayer.updateRidden`). Also applies
    /// the carrot on a stick's wear (`damageItem(1, player)`; a used-up
    /// stick turns back into a fishing rod with its tag).
    pub(super) fn tick_mob_rider(&mut self, play: &mut Play) {
        let wear = std::mem::take(&mut self.stick_wear);
        if wear > 0 {
            let sel = play.inv.selected;
            if let Some(s) = play.inv.main[sel].filter(|s| s.id == 398) {
                let (left, broke) =
                    crate::enchant::damage_item(s, wear, false, &mut self.splayer.e.rand);
                play.inv.main[sel] = if broke {
                    Some(Stack {
                        id: 346,
                        meta: 0,
                        count: 1,
                        tag: s.tag,
                    })
                } else {
                    left
                };
                play.inv_dirty = true;
            }
        }
        let Some(id) = self.riding else { return };
        let Some(k) = self.mobs.list.iter().position(|m| m.id == id) else {
            // a vehicle, or a mob that is gone
            if play.vehicles.iter().all(|v| v.v.id() != id) {
                self.riding = None;
                self.vehicle_events.push(Event::Mount(None));
            }
            return;
        };
        let m = &self.mobs.list[k];
        if m.health <= 0.0 || m.dead || self.splayer.dead {
            // ridingEntity.isDead
            self.mobs.list[k].ridden_by_player = false;
            self.riding = None;
            self.vehicle_events.push(Event::Mount(None));
            return;
        }
        // EntityAIRunAroundLikeCrazy threw the rider: mountEntity(null)
        if !m.ridden_by_player || self.ride.sneak {
            std::mem::swap(play, &mut self.play);
            self.dismount_mob();
            std::mem::swap(play, &mut self.play);
            self.ride.sneak = false;
            self.splayer.sneaking = false;
            return;
        }
        // setPosition(posX, posY + getMountedYOffset() + getYOffset(), posZ)
        let (w, h) = m.dims();
        let _ = w;
        let mut feet = [
            m.pos[0],
            m.pos[1] + h as f64 * 0.75 + PLAYER_Y_OFFSET_MP,
            m.pos[2],
        ];
        // EntityHorse.updateRiderPosition: rearing lifts the rider back
        if m.kind == crate::entity::mob::Kind::Horse && m.prev_rearing > 0.0 {
            let r = std::f32::consts::PI / 180.0;
            let (s, c) = (
                crate::util::math::sin(m.body_yaw * r),
                crate::util::math::cos(m.body_yaw * r),
            );
            let (a, b) = (0.7 * m.prev_rearing, 0.15 * m.prev_rearing);
            feet[0] += (a * s) as f64;
            feet[1] += b as f64;
            feet[2] -= (a * c) as f64;
        }
        play.player.0 = feet;
        self.splayer.place(feet);
        if let Some(p) = &mut self.player {
            p.0 = feet;
        }
    }

    /// `EntityLivingBase.dismountEntity` off a mob.
    pub(super) fn dismount_mob(&mut self) {
        let Some(k) = self.ridden_mob() else {
            return;
        };
        self.riding = None;
        let m = &mut self.mobs.list[k];
        m.ridden_by_player = false;
        let (_, h) = m.dims();
        let (mp, mb) = (m.pos, m.bb);
        let feet = self.play.player.0;
        let mut to = [mp[0], mb.min[1] + h as f64, mp[2]];
        let pb = Self::player_box(feet);
        'search: for dx in -1..=1 {
            for dz in -1..1 {
                if dx == 0 && dz == 0 {
                    continue;
                }
                let x = (feet[0] + dx as f64) as i32;
                let z = (feet[2] + dz as f64) as i32;
                let y = feet[1] as i32;
                let b = pb.offset_copy(dx as f64, 1.0, dz as f64);
                if crate::entity::player::blocks_colliding(&self.world, &b).is_empty() {
                    if self.world.solid_top_surface(x, y, z) {
                        to = [feet[0] + dx as f64, feet[1] + 1.0, feet[2] + dz as f64];
                        break 'search;
                    }
                    if self.world.solid_top_surface(x, y - 1, z)
                        || block::material(self.world.block(x, y - 1, z)) == block::Material::Water
                    {
                        to = [feet[0] + dx as f64, feet[1] + 1.0, feet[2] + dz as f64];
                    }
                }
            }
        }
        self.play.player.0 = to;
        self.splayer.place(to);
        if let Some(p) = &mut self.player {
            p.0 = to;
        }
        self.vehicle_events.push(Event::Mount(None));
        self.teleport_to = Some(to);
    }

    /// `ItemCarrotOnAStick.onItemRightClick`: a boost for the pig being
    /// steered, for 7 durability.
    pub(super) fn carrot_on_a_stick(&mut self, held: Stack) {
        let Some(k) = self.ridden_mob() else {
            return;
        };
        if self.mobs.list[k].kind != crate::entity::mob::Kind::Pig {
            return;
        }
        let max = crate::item::max_damage(held.id);
        if max - held.meta < 7 || !self.mobs.list[k].pig_boost() {
            return;
        }
        let creative = self.play.mode == GameMode::Creative;
        let (left, broke) =
            crate::enchant::damage_item(held, 7, creative, &mut self.splayer.e.rand);
        let sel = self.play.inv.selected;
        self.play.inv.main[sel] = if broke {
            Some(Stack {
                id: 346,
                meta: 0,
                count: 1,
                tag: held.tag,
            })
        } else {
            left
        };
        self.play.inv_dirty = true;
    }
}
