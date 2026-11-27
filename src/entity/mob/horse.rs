//! `EntityHorse`: taming, feeding, the saddle, armour and chest, the
//! player's riding and jumping, and the animation counters.

use super::*;

/// Data watcher 16 bits.
pub const TAME: u32 = 2;
pub const SADDLE: u32 = 4;
pub const CHEST: u32 = 8;
pub const BRED: u32 = 16;
pub const EATING: u32 = 32;
pub const REARING: u32 = 64;
pub const MOUTH: u32 = 128;

/// `Items.iron_horse_armor`, `golden_horse_armor`, `diamond_horse_armor`
pub fn armor_index(id: Option<u16>) -> i32 {
    match id {
        Some(417) => 1,
        Some(418) => 2,
        Some(419) => 3,
        _ => 0,
    }
}

/// `EntityHorse.isArmorItem`
pub fn is_armor(id: u16) -> bool {
    (417..=419).contains(&id)
}

/// What `EntityHorse.interact` asks of the game.
#[derive(Debug, PartialEq)]
pub enum HorseAct {
    /// `openGUI`
    Gui,
    /// `mountTo`
    Mount,
    /// The held item is used up (one of it).
    Used,
    /// Handled, nothing else to do.
    Done,
    /// `super.interact` / not handled: the name tag may still apply.
    Pass,
}

impl Mob {
    /// `getHorseType`: 0 horse, 1 donkey, 2 mule, 3 zombie, 4 skeleton.
    pub fn horse_type(&self) -> i32 {
        self.variant >> 16
    }
    fn hflag(&self, f: u32) -> bool {
        self.horse_flags & f != 0
    }
    fn set_hflag(&mut self, f: u32, on: bool) {
        if on {
            self.horse_flags |= f;
        } else {
            self.horse_flags &= !f;
        }
    }
    pub fn horse_tame(&self) -> bool {
        self.hflag(TAME)
    }
    pub fn horse_saddled(&self) -> bool {
        self.hflag(SADDLE)
    }
    pub fn horse_chested(&self) -> bool {
        self.hflag(CHEST)
    }
    /// `canWearArmor`
    pub fn can_wear_armor(&self) -> bool {
        self.horse_type() == 0
    }
    /// `canCarryChest`
    pub fn can_carry_chest(&self) -> bool {
        matches!(self.horse_type(), 1 | 2)
    }
    /// `isUndead`
    pub fn horse_undead(&self) -> bool {
        matches!(self.horse_type(), 3 | 4)
    }
    /// `isMovementBlocked`
    pub fn horse_blocked(&self) -> bool {
        if self.ridden_by_player && self.horse_saddled() {
            true
        } else {
            self.hflag(EATING) || self.hflag(REARING)
        }
    }

    /// `getChestSize`
    fn horse_chest_size(&self) -> usize {
        if self.horse_chested() && matches!(self.horse_type(), 1 | 2) {
            17
        } else {
            2
        }
    }

    /// `initHorseChest`: a new chest keeping what fits.
    pub fn init_horse_chest(&mut self) {
        let n = self.horse_chest_size();
        self.horse_chest.resize(n, None);
        self.update_horse_slots();
    }

    /// `updateHorseSlots`
    pub fn update_horse_slots(&mut self) {
        let saddled = self.horse_chest.first().copied().flatten().is_some();
        self.set_hflag(SADDLE, saddled);
        if self.can_wear_armor() {
            let a = armor_index(self.horse_chest.get(1).copied().flatten().map(|s| s.id));
            self.horse_armor = a;
        }
    }

    /// `onInventoryChanged`: the armour and saddle sounds after the slots
    /// changed (`old` armour index and saddle).
    pub fn horse_inventory_changed(&mut self, old_armor: i32, old_saddle: bool, out: &mut Outcome) {
        self.update_horse_slots();
        if self.ticks > 20 {
            if old_armor != self.horse_armor {
                out.sounds.push(("mob.horse.armor", self.pos, 0.5, 1.0));
            }
            if !old_saddle && self.horse_saddled() {
                out.sounds.push(("mob.horse.leather", self.pos, 0.5, 1.0));
            }
        }
    }

    /// `getModifiedMaxHealth`, `getModifiedJumpStrength`,
    /// `getModifiedMovementSpeed`
    fn modified_health(&mut self) -> f64 {
        (15.0f32 + self.rand.next_int_n(8) as f32 + self.rand.next_int_n(9) as f32) as f64
    }
    fn modified_jump(&mut self) -> f64 {
        0.4f32 as f64
            + self.rand.next_double() * 0.2
            + self.rand.next_double() * 0.2
            + self.rand.next_double() * 0.2
    }
    fn modified_speed(&mut self) -> f64 {
        (0.45f32 as f64
            + self.rand.next_double() * 0.3
            + self.rand.next_double() * 0.3
            + self.rand.next_double() * 0.3)
            * 0.25
    }

    /// `EntityHorse.onSpawnWithEgg` (no group data: each horse its own).
    pub fn spawn_horse(&mut self) {
        let (htype, variant) = if self.rand.next_int_n(10) == 0 {
            (1, 0)
        } else {
            let colour = self.rand.next_int_n(7);
            let marks = self.rand.next_int_n(5);
            (0, colour | marks << 8)
        };
        self.variant = htype << 16 | variant;
        if self.rand.next_int_n(5) == 0 {
            self.growing_age = -24000;
        }
        if htype != 4 && htype != 3 {
            let h = self.modified_health();
            self.potions.attrs.max_health.base = h;
            if htype == 0 {
                let s = self.modified_speed();
                self.potions.attrs.movement_speed.base = s;
            } else {
                self.potions.attrs.movement_speed.base = 0.175f32 as f64;
            }
        } else {
            self.potions.attrs.max_health.base = 15.0;
            self.potions.attrs.movement_speed.base = 0.2f32 as f64;
        }
        self.jump_strength = if htype != 2 && htype != 1 {
            self.modified_jump()
        } else {
            0.5
        };
        self.health = self.max_health();
        self.init_horse_chest();
    }

    /// `createChild` for two horses `a` (this) and `b`: type, colours and
    /// the averaged attributes.
    pub fn horse_child_of(&mut self, a: &Mob, b: &Mob, r: &mut JavaRandom) {
        let (ta, tb) = (a.horse_type(), b.horse_type());
        let t = if ta == tb {
            ta
        } else if ta == 0 && tb == 1 || ta == 1 && tb == 0 {
            2
        } else {
            0
        };
        let mut v = 0;
        if t == 0 {
            let k = r.next_int_n(9);
            v = if k < 4 {
                a.variant & 0xFF
            } else if k < 8 {
                b.variant & 0xFF
            } else {
                r.next_int_n(7)
            };
            let m = r.next_int_n(5);
            v |= if m < 2 {
                a.variant & 0xFF00
            } else if m < 4 {
                b.variant & 0xFF00
            } else {
                r.next_int_n(5) << 8 & 0xFF00
            };
        }
        self.variant = t << 16 | v;
        let h = (15.0f32 + r.next_int_n(8) as f32 + r.next_int_n(9) as f32) as f64;
        self.potions.attrs.max_health.base =
            (a.potions.attrs.max_health.base + b.potions.attrs.max_health.base + h) / 3.0;
        let j = 0.4f32 as f64 + r.next_double() * 0.2 + r.next_double() * 0.2 + r.next_double() * 0.2;
        self.jump_strength = (a.jump_strength + b.jump_strength + j) / 3.0;
        let s = (0.45f32 as f64 + r.next_double() * 0.3 + r.next_double() * 0.3 + r.next_double() * 0.3)
            * 0.25;
        self.potions.attrs.movement_speed.base =
            (a.potions.attrs.movement_speed.base + b.potions.attrs.movement_speed.base + s) / 3.0;
        self.health = self.max_health();
        self.init_horse_chest();
    }

    /// `openHorseMouth`
    fn open_horse_mouth(&mut self) {
        self.horse_counters[1] = 1;
        self.set_hflag(MOUTH, true);
    }

    /// `makeHorseRear`
    pub fn make_horse_rear(&mut self) {
        self.horse_counters[2] = 1;
        self.set_hflag(EATING, false);
        self.set_hflag(REARING, true);
    }

    /// `makeHorseRearWithSound` (`getAngrySoundName`)
    pub fn rear_with_sound(&mut self, out: &mut Outcome) {
        self.make_horse_rear();
        self.open_horse_mouth();
        self.make_horse_rear();
        let s = match self.horse_type() {
            3 | 4 => None,
            1 | 2 => Some("mob.horse.donkey.angry"),
            _ => Some("mob.horse.angry"),
        };
        if let Some(s) = s {
            let (v, p) = self.sound_volume_pitch();
            out.sounds.push((s, self.pos, v, p));
        }
    }

    /// `eatingHorse`
    fn eating_horse(&mut self, out: &mut Outcome) {
        self.open_horse_mouth();
        let p = 1.0 + (self.rand.next_float() - self.rand.next_float()) * 0.2;
        out.sounds.push(("eating", self.pos, 1.0, p));
    }

    /// `heal`
    fn horse_heal(&mut self, n: f32) {
        if self.health > 0.0 {
            self.health = (self.health + n).min(self.max_health());
        }
    }

    /// `EntityHorse.interact` with what the player holds, sneaking or not.
    pub fn horse_interact(&mut self, held: Option<Stack>, sneaking: bool, out: &mut Outcome) -> HorseAct {
        let adult = self.growing_age >= 0;
        if held.is_some_and(|s| s.id == 383) {
            return HorseAct::Pass;
        }
        if !self.horse_tame() && self.horse_undead() {
            return HorseAct::Pass;
        }
        if self.horse_tame() && adult && sneaking {
            return HorseAct::Gui;
        }
        if adult && self.ridden_by_player {
            return HorseAct::Pass;
        }
        if let Some(s) = held {
            let mut used = false;
            if self.can_wear_armor() && is_armor(s.id) {
                if !self.horse_tame() {
                    self.rear_with_sound(out);
                    return HorseAct::Done;
                }
                return HorseAct::Gui;
            }
            if !self.horse_undead() {
                let (mut heal, mut grow, mut temper) = (0.0f32, 0, 0);
                match s.id {
                    296 => (heal, grow, temper) = (2.0, 60, 3),
                    353 => (heal, grow, temper) = (1.0, 30, 3),
                    297 => (heal, grow, temper) = (7.0, 180, 3),
                    170 => (heal, grow) = (20.0, 180),
                    260 => (heal, grow, temper) = (3.0, 60, 3),
                    396 => {
                        (heal, grow, temper) = (4.0, 60, 5);
                        if self.horse_tame() && self.growing_age == 0 {
                            used = true;
                            self.in_love = 600;
                        }
                    }
                    322 => {
                        (heal, grow, temper) = (10.0, 240, 10);
                        if self.horse_tame() && self.growing_age == 0 {
                            used = true;
                            self.in_love = 600;
                        }
                    }
                    _ => {}
                }
                if self.health < self.max_health() && heal > 0.0 {
                    self.horse_heal(heal);
                    used = true;
                }
                if !adult && grow > 0 {
                    // EntityAgeable.addGrowth
                    self.growing_age = (self.growing_age + grow).min(0);
                    used = true;
                }
                if temper > 0 && (used || !self.horse_tame()) && temper < 100 {
                    used = true;
                    self.temper = (self.temper + temper).clamp(0, 100);
                }
                if used {
                    self.eating_horse(out);
                }
            }
            if !self.horse_tame() && !used {
                if s.id == NAME_TAG && s.custom_name().is_some() {
                    return HorseAct::Pass;
                }
                self.rear_with_sound(out);
                return HorseAct::Done;
            }
            if !used && self.can_carry_chest() && !self.horse_chested() && s.id == block::CHEST as u16 {
                self.set_hflag(CHEST, true);
                let p = (self.rand.next_float() - self.rand.next_float()) * 0.2 + 1.0;
                out.sounds.push(("mob.chickenplop", self.pos, 1.0, p));
                used = true;
                self.init_horse_chest();
            }
            if !used && adult && !self.horse_saddled() && s.id == 329 {
                return HorseAct::Gui;
            }
            if used {
                return HorseAct::Used;
            }
        }
        if !adult || self.ridden_by_player {
            return HorseAct::Pass;
        }
        if held.is_some_and(|s| s.id == NAME_TAG && s.custom_name().is_some()) {
            return HorseAct::Pass;
        }
        // mountTo
        self.set_hflag(EATING, false);
        self.set_hflag(REARING, false);
        HorseAct::Mount
    }

    /// `C0BPacketEntityAction` 6: `setJumpPower` from the rider's bar.
    pub fn set_jump_power(&mut self, mut p: i32) {
        if !self.horse_saddled() {
            return;
        }
        if p < 0 {
            p = 0;
        } else {
            self.stand_sliding = true;
            self.make_horse_rear();
        }
        self.jump_power = if p >= 90 {
            1.0
        } else {
            0.4 + 0.4 * p as f32 / 90.0
        };
    }

    /// The server part of `onLivingUpdate` and `onUpdate`'s counters.
    pub fn horse_update(&mut self, w: &ServerWorld) {
        if self.rand.next_int_n(900) == 0 && self.death_time == 0 {
            self.horse_heal(1.0);
        }
        if !self.hflag(EATING)
            && !self.ridden_by_player
            && self.rand.next_int_n(300) == 0
            && w.peek_block(
                math::floor_double(self.pos[0]),
                math::floor_double(self.pos[1]) - 1,
                math::floor_double(self.pos[2]),
            ) == block::GRASS
        {
            self.set_hflag(EATING, true);
        }
        if self.hflag(EATING) {
            self.horse_counters[0] += 1;
            if self.horse_counters[0] > 50 {
                self.horse_counters[0] = 0;
                self.set_hflag(EATING, false);
            }
        }
        // onUpdate
        if self.horse_counters[1] > 0 {
            self.horse_counters[1] += 1;
            if self.horse_counters[1] > 30 {
                self.horse_counters[1] = 0;
                self.set_hflag(MOUTH, false);
            }
        }
        if self.horse_counters[2] > 0 {
            self.horse_counters[2] += 1;
            if self.horse_counters[2] > 20 {
                self.horse_counters[2] = 0;
                self.set_hflag(REARING, false);
            }
        }
        // the rearing amount, for the rider's seat
        self.prev_rearing = self.rearing;
        if self.hflag(REARING) {
            self.rearing = (self.rearing + ((1.0 - self.rearing) * 0.4 + 0.05)).min(1.0);
        } else {
            self.stand_sliding = false;
            self.rearing = (self.rearing
                + ((0.8 * self.rearing * self.rearing * self.rearing - self.rearing) * 0.6 - 0.05))
                .max(0.0);
        }
    }

    /// `moveEntityWithHeading` with a player on a saddled horse: the
    /// rider's input, the charged jump, step height 1.
    pub fn horse_ridden_move(&mut self, w: &ServerWorld, p: &PlayerInfo) {
        self.yaw = p.yaw;
        self.prev_yaw = p.yaw;
        self.pitch = p.pitch * 0.5;
        self.head_yaw = self.yaw;
        self.body_yaw = self.yaw;
        let mut strafe = p.ride_strafe * 0.5;
        let mut forward = p.ride_forward;
        if forward <= 0.0 {
            forward *= 0.25;
            self.horse_counters[3] = 0;
        }
        if self.on_ground && self.jump_power == 0.0 && self.hflag(REARING) && !self.stand_sliding {
            strafe = 0.0;
            forward = 0.0;
        }
        if self.jump_power > 0.0 && !self.horse_jumping && self.on_ground {
            self.motion[1] = self.jump_strength * self.jump_power as f64;
            if let Some(a) = self.potions.amplifier(crate::potion::JUMP) {
                self.motion[1] += ((a + 1) as f32 * 0.1f32) as f64;
            }
            self.horse_jumping = true;
            if forward > 0.0 {
                let r = std::f32::consts::PI / 180.0;
                let (s, c) = (math::sin(self.yaw * r), math::cos(self.yaw * r));
                self.motion[0] += (-0.4f32 * s * self.jump_power) as f64;
                self.motion[2] += (0.4f32 * c * self.jump_power) as f64;
                self.move_sounds.push(("mob.horse.jump", 0.4, 1.0));
            }
            self.jump_power = 0.0;
        }
        self.horse_step = 1.0;
        let speed = self.speed_attr();
        self.ai_speed = speed;
        self.jump_factor = speed * 0.1;
        self.move_with_heading(w, strafe, forward);
        if self.on_ground {
            self.jump_power = 0.0;
            self.horse_jumping = false;
        }
        // the limbs follow the distance moved
        self.prev_limb_amount = self.limb_amount;
        let dx = self.pos[0] - self.prev_pos[0];
        let dz = self.pos[2] - self.prev_pos[2];
        let f = (math::sqrt_double(dx * dx + dz * dz) * 4.0).min(1.0);
        self.limb_amount += (f - self.limb_amount) * 0.4;
        self.limb_swing += self.limb_amount;
    }

    /// `dropChestItems`: everything in the horse's inventory, and the chest.
    pub fn horse_chest_drops(&mut self) -> Vec<Stack> {
        let mut v: Vec<Stack> = self.horse_chest.iter().flatten().copied().collect();
        if self.horse_chested() {
            v.push(Stack::new(block::CHEST as u16, 0, 1));
            self.set_hflag(CHEST, false);
        }
        v
    }

    /// `writeEntityToNBT`
    pub fn horse_nbt(&self, v: &mut Vec<(&'static str, crate::world::anvil::Nbt)>) {
        use crate::world::anvil::{item_nbt, Nbt};
        v.push(("EatingHaystack", Nbt::Byte(self.hflag(EATING) as i8)));
        v.push(("ChestedHorse", Nbt::Byte(self.horse_chested() as i8)));
        v.push(("HasReproduced", Nbt::Byte(0)));
        v.push(("Bred", Nbt::Byte(self.hflag(BRED) as i8)));
        v.push(("Temper", Nbt::Int(self.temper)));
        v.push(("Tame", Nbt::Byte(self.horse_tame() as i8)));
        v.push(("OwnerUUID", Nbt::String(String::new())));
        if self.horse_chested() {
            let items = self
                .horse_chest
                .iter()
                .enumerate()
                .skip(2)
                .filter_map(|(k, s)| s.map(|s| item_nbt(Some(k as i8), &s)))
                .collect();
            v.push(("Items", Nbt::List(10, items)));
        }
        if let Some(s) = self.horse_chest.get(1).copied().flatten() {
            v.push(("ArmorItem", item_nbt(None, &s)));
        }
        if let Some(s) = self.horse_chest.first().copied().flatten() {
            v.push(("SaddleItem", item_nbt(None, &s)));
        }
        // SharedMonsterAttributes.writeBaseAttributeMapToNBT (the horse's own)
        let attr = |n: &str, b: f64| {
            Nbt::Compound(vec![
                ("Name".to_string(), Nbt::String(n.to_string())),
                ("Base".to_string(), Nbt::Double(b)),
            ])
        };
        v.push((
            "Attributes",
            Nbt::List(
                10,
                vec![
                    attr("generic.maxHealth", self.potions.attrs.max_health.base),
                    attr("generic.movementSpeed", self.potions.attrs.movement_speed.base),
                    attr("horse.jumpStrength", self.jump_strength),
                ],
            ),
        ));
    }

    /// `readEntityFromNBT`
    pub fn horse_from_nbt(&mut self, t: &crate::world::anvil::Nbt) {
        let b = |k: &str| t.int(k).unwrap_or(0) != 0;
        self.set_hflag(EATING, b("EatingHaystack"));
        self.set_hflag(BRED, b("Bred"));
        self.set_hflag(CHEST, b("ChestedHorse"));
        self.temper = t.int("Temper").unwrap_or(0) as i32;
        self.set_hflag(TAME, b("Tame"));
        for a in t.list("Attributes") {
            let base = match a.get("Base") {
                Some(crate::world::anvil::Nbt::Double(d)) => *d,
                _ => continue,
            };
            match a.string("Name") {
                Some("generic.maxHealth") => self.potions.attrs.max_health.base = base,
                Some("generic.movementSpeed") => self.potions.attrs.movement_speed.base = base,
                Some("horse.jumpStrength") => self.jump_strength = base,
                _ => {}
            }
        }
        self.init_horse_chest();
        if self.horse_chested() {
            for it in t.list("Items") {
                let k = (it.int("Slot").unwrap_or(0) & 255) as usize;
                if k >= 2 && k < self.horse_chest.len() {
                    self.horse_chest[k] = crate::world::anvil::item_from_nbt(it);
                }
            }
        }
        if let Some(s) = t.get("ArmorItem").and_then(crate::world::anvil::item_from_nbt) {
            if is_armor(s.id) {
                self.horse_chest[1] = Some(s);
            }
        }
        if let Some(s) = t.get("SaddleItem").and_then(crate::world::anvil::item_from_nbt) {
            if s.id == 329 {
                self.horse_chest[0] = Some(s);
            }
        } else if b("Saddle") {
            self.horse_chest[0] = Some(Stack::new(329, 0, 1));
        }
        self.update_horse_slots();
    }
}
