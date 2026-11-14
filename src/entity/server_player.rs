//! The server side of the player (`EntityPlayerMP`): health, absorption,
//! potion effects, `FoodStats`, air, fire, experience, the damage pipeline
//! and the `CombatTracker` death message. The client moves the player and
//! reports where it is, as `C03PacketPlayer` does; everything here is
//! authoritative and mirrored back to the client.

use super::base::{Ent, Fx};
use super::living::{Attributes, Difficulty, Living, Source, SourceKind};
use crate::block::{self, Material};
use crate::potion::{self, Effect};
use crate::util::math;
use crate::world::{BlockView, ServerWorld};

/// `FoodStats`
#[derive(Clone, Debug)]
pub struct FoodStats {
    pub level: i32,
    pub saturation: f32,
    pub exhaustion: f32,
    pub timer: i32,
    pub prev_level: i32,
}

impl Default for FoodStats {
    fn default() -> Self {
        FoodStats {
            level: 20,
            saturation: 5.0,
            exhaustion: 0.0,
            timer: 0,
            prev_level: 20,
        }
    }
}

impl FoodStats {
    /// `addStats(heal, saturationModifier)`
    pub fn add_stats(&mut self, n: i32, sat: f32) {
        self.level = (n + self.level).min(20);
        self.saturation = (self.saturation + n as f32 * sat * 2.0).min(self.level as f32);
    }

    /// `addExhaustion`
    pub fn add_exhaustion(&mut self, f: f32) {
        self.exhaustion = (self.exhaustion + f).min(40.0);
    }

    pub fn need_food(&self) -> bool {
        self.level < 20
    }
}

/// `CombatEntry`
#[derive(Clone, Debug)]
struct CombatEntry {
    src: Source,
    ticks: i32,
    damage: f32,
    fall_suffix: Option<&'static str>,
    fall_distance: f32,
}

/// What the client needs to draw the HUD and adjust its own movement.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerStatsView {
    pub health: f32,
    pub max_health: f32,
    pub absorption: f32,
    pub food: i32,
    pub saturation: f32,
    pub air: i32,
    pub fire: i32,
    pub xp_level: i32,
    pub xp_total: i32,
    pub xp: f32,
    pub effects: Vec<Effect>,
    /// `movementSpeed` attribute value (without sprinting).
    pub speed: f64,
    pub dead: bool,
}

/// A translatable death message: key and arguments (translated names).
#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub key: String,
    pub args: Vec<String>,
}

pub struct ServerPlayer {
    pub living: Living,
    pub food: FoodStats,
    pub e: Ent,
    pub air: i32,
    pub xp_level: i32,
    pub xp_total: i32,
    pub xp: f32,
    pub score: i32,
    pub creative: bool,
    pub flying: bool,
    pub sneaking: bool,
    pub sprinting: bool,
    pub dead: bool,
    /// Set once the death has been handled (items dropped, client told).
    pub death_reported: bool,
    combat: Vec<CombatEntry>,
    last_damage_time: i32,
    taking_damage: bool,
    in_combat: bool,
    /// Sounds and particles this tick.
    pub fx: Vec<Fx>,
    /// `S12PacketEntityVelocity` for the client (knockback).
    pub velocity: Option<[f64; 3]>,
    /// The client should play the hurt animation (`setEntityState(2)`).
    pub hurt_anim: bool,
    /// The position the client last reported (feet).
    pub feet: [f64; 3],
    have_pos: bool,
    on_ground: bool,
    pub last_sent: Option<PlayerStatsView>,
    /// `xpCooldown`: ticks before the next orb can be picked up.
    pub xp_cooldown: i32,
    /// Ticks since the last attack from an entity (`entityAge`).
    pub entity_age: i32,
    /// `itemInUse` / `itemInUseCount`: the hotbar slot, the stack as it was
    /// when use began, and the ticks left.
    pub using: Option<(usize, crate::item::Stack, i32)>,
    /// `inventory.armorInventory` (`getLastActiveItems`): a copy of the
    /// worn pieces for the duration of a tick, taken from the inventory
    /// before damage can happen and written back when `armor_changed`.
    pub armor_items: [Option<crate::item::Stack>; 4],
    pub armor_changed: bool,
}

impl ServerPlayer {
    pub fn new() -> ServerPlayer {
        // EntityPlayer: maxHealth 20, movementSpeed 0.1, attackDamage 1
        let mut l = Living::new(Attributes::new(20.0, 0.10000000149011612, 1.0));
        l.max_hurt_resistant_time = 20;
        let mut e = Ent::new(0.6, 1.8, 0x706C61796572);
        e.y_offset = 1.62;
        e.portal_cooldown = 10;
        ServerPlayer {
            living: l,
            food: FoodStats::default(),
            e,
            air: 300,
            xp_level: 0,
            xp_total: 0,
            xp: 0.0,
            score: 0,
            creative: false,
            flying: false,
            sneaking: false,
            sprinting: false,
            dead: false,
            death_reported: false,
            combat: Vec::new(),
            last_damage_time: 0,
            taking_damage: false,
            in_combat: false,
            fx: Vec::new(),
            velocity: None,
            hurt_anim: false,
            feet: [0.0; 3],
            xp_cooldown: 0,
            have_pos: false,
            on_ground: false,
            last_sent: None,
            entity_age: 0,
            using: None,
            armor_items: [None; 4],
            armor_changed: false,
        }
    }

    /// `EntityPlayer.canEat`
    pub fn can_eat(&self, always: bool) -> bool {
        (always || self.food.need_food()) && !self.creative
    }

    pub fn view(&self) -> PlayerStatsView {
        PlayerStatsView {
            health: self.living.health,
            max_health: self.living.max_health(),
            absorption: self.living.absorption,
            food: self.food.level,
            saturation: self.food.saturation,
            air: self.air,
            fire: self.e.fire,
            xp_level: self.xp_level,
            xp_total: self.xp_total,
            xp: self.xp,
            effects: self.living.effects.values(),
            speed: self.living.attrs.movement_speed.value(),
            dead: self.dead,
        }
    }

    /// `EntityPlayer.xpBarCap`
    pub fn xp_bar_cap(&self) -> i32 {
        if self.xp_level >= 30 {
            62 + (self.xp_level - 30) * 7
        } else if self.xp_level >= 15 {
            17 + (self.xp_level - 15) * 3
        } else {
            17
        }
    }

    /// `EntityPlayer.addExperience`
    pub fn add_xp(&mut self, n: i32) {
        self.score += n;
        let n = n.min(i32::MAX - self.xp_total);
        self.xp += n as f32 / self.xp_bar_cap() as f32;
        self.xp_total += n;
        while self.xp >= 1.0 {
            self.xp = (self.xp - 1.0) * self.xp_bar_cap() as f32;
            // addExperienceLevel(1)
            self.xp_level += 1;
            if self.xp_level < 0 {
                self.xp_level = 0;
                self.xp = 0.0;
                self.xp_total = 0;
            }
            self.xp /= self.xp_bar_cap() as f32;
        }
    }

    /// `C03PacketPlayer`: the client moved. Runs `addMovementStat`, the
    /// jump exhaustion and `handleFalling` as `processPlayer` does.
    pub fn moved(
        &mut self,
        w: &ServerWorld,
        feet: [f64; 3],
        on_ground: bool,
        flying: bool,
        sneaking: bool,
        sprinting: bool,
    ) {
        self.flying = flying;
        self.sneaking = sneaking;
        self.sprinting = sprinting;
        if !self.have_pos {
            self.have_pos = true;
            self.feet = feet;
            self.on_ground = on_ground;
            self.e.set_position(feet[0], feet[1] + 1.62, feet[2]);
            return;
        }
        let d = [
            feet[0] - self.feet[0],
            feet[1] - self.feet[1],
            feet[2] - self.feet[2],
        ];
        // NetHandlerPlayServer.processPlayer: the jump
        if self.on_ground && !on_ground && d[1] > 0.0 {
            // EntityPlayer.jump: addStat(jump), exhaustion
            self.food.add_exhaustion(if sprinting { 0.8 } else { 0.2 });
        }
        self.feet = feet;
        self.e.set_position(feet[0], feet[1] + 1.62, feet[2]);
        self.e.on_ground = on_ground;
        self.on_ground = on_ground;
        if !self.creative {
            self.add_movement_stat(w, d);
        }
        // EntityPlayerMP.handleFalling -> updateFallState
        if !self.dead {
            if on_ground {
                if self.e.fall_distance > 0.0 {
                    let f = self.e.fall_distance;
                    self.fall(w, f);
                    self.e.fall_distance = 0.0;
                }
            } else if d[1] < 0.0 {
                self.e.fall_distance = (self.e.fall_distance as f64 - d[1]) as f32;
            }
        }
    }

    /// The server placed the player (riding, dismounting): feet.
    pub fn place(&mut self, feet: [f64; 3]) {
        self.feet = feet;
        self.e.set_position(feet[0], feet[1] + 1.62, feet[2]);
    }

    /// `Entity.fall` passed on by a vehicle (`riddenByEntity.fall`).
    pub fn fall_from(&mut self, w: &ServerWorld, distance: f32) {
        if !self.dead {
            self.fall(w, distance);
        }
    }

    /// `EntityPlayer.addMovementStat` (the exhaustion part)
    fn add_movement_stat(&mut self, w: &ServerWorld, d: [f64; 3]) {
        let wv = &*w as &dyn BlockView;
        if self.e.is_inside_of_material(wv, Material::Water, 0.12) {
            let i =
                (math::sqrt_double(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]) * 100.0).round() as i32;
            if i > 0 {
                self.food.add_exhaustion(0.015 * i as f32 * 0.01);
            }
        } else if self.e.in_water {
            let i = (math::sqrt_double(d[0] * d[0] + d[2] * d[2]) * 100.0).round() as i32;
            if i > 0 {
                self.food.add_exhaustion(0.015 * i as f32 * 0.01);
            }
        } else if self.on_ladder(w) {
        } else if self.on_ground {
            let i = (math::sqrt_double(d[0] * d[0] + d[2] * d[2]) * 100.0).round() as i32;
            if i > 0 {
                if self.sprinting {
                    self.food.add_exhaustion(0.099999994 * i as f32 * 0.01);
                } else {
                    self.food.add_exhaustion(0.01 * i as f32 * 0.01);
                }
            }
        }
    }

    /// `EntityLivingBase.isOnLadder`
    fn on_ladder(&self, w: &ServerWorld) -> bool {
        let b = w.peek_block(
            math::floor_double(self.e.pos[0]),
            math::floor_double(self.e.bb.min[1]),
            math::floor_double(self.e.pos[2]),
        );
        b == block::LADDER || b == block::VINE
    }

    /// `EntityPlayer.fall` -> `EntityLivingBase.fall`
    fn fall(&mut self, w: &ServerWorld, distance: f32) {
        // capabilities.allowFlying
        if self.creative {
            return;
        }
        let jump = self
            .living
            .amplifier(potion::JUMP)
            .map_or(0.0, |a| (a + 1) as f32);
        let i = math::ceiling_float_int(distance - 3.0 - jump);
        if i > 0 {
            let s = if i > 4 {
                "game.player.hurt.fall.big"
            } else {
                "game.player.hurt.fall.small"
            };
            self.fx.push(Fx::Sound(s, self.e.pos, 1.0, 1.0));
            self.attack(
                w,
                Source::named(SourceKind::Fall),
                i as f32,
                Difficulty::Normal,
            );
            let x = math::floor_double(self.e.pos[0]);
            let y = math::floor_double(self.e.pos[1] - 0.2f32 as f64 - self.e.y_offset as f64);
            let z = math::floor_double(self.e.pos[2]);
            let b = w.peek_block(x, y, z);
            if block::material(b) != Material::Air {
                let st = block::step_sound(b);
                self.fx.push(Fx::Sound(
                    st.step,
                    self.e.pos,
                    st.volume * 0.5,
                    st.pitch * 0.75,
                ));
                // Block.onFallenUpon (farmland trampling)
                self.fx.push(Fx::Walk(x, y, z, b));
            }
        }
    }

    /// `EntityPlayer.attackEntityFrom` + `EntityLivingBase.attackEntityFrom`
    /// with the knockback, for this player. Returns whether it hurt.
    pub fn attack(
        &mut self,
        w: &ServerWorld,
        src: Source,
        mut amount: f32,
        diff: Difficulty,
    ) -> bool {
        let entries = self.combat.len();
        let r = self.attack_inner(w, src, amount, diff);
        // CombatTracker.calculateFallSuffix at the time of the damage
        if self.combat.len() > entries
            || (r
                && self
                    .combat
                    .last()
                    .is_some_and(|c| c.ticks == self.e.ticks_existed))
        {
            let s = self.fall_suffix(w);
            if let Some(c) = self.combat.last_mut() {
                c.fall_suffix = s;
            }
        }
        let _ = &mut amount;
        r
    }

    fn attack_inner(
        &mut self,
        w: &ServerWorld,
        src: Source,
        mut amount: f32,
        diff: Difficulty,
    ) -> bool {
        let _ = w;
        // capabilities.disableDamage && !canHarmInCreative
        if self.creative && src.kind != SourceKind::OutOfWorld {
            return false;
        }
        self.entity_age = 0;
        if self.living.health <= 0.0 {
            return false;
        }
        if src.difficulty_scaled {
            match diff {
                Difficulty::Peaceful => amount = 0.0,
                Difficulty::Easy => amount = amount / 2.0 + 1.0,
                Difficulty::Hard => amount = amount * 3.0 / 2.0,
                Difficulty::Normal => {}
            }
        }
        if amount == 0.0 {
            return false;
        }
        // EntityLivingBase.attackEntityFrom
        if src.fire && self.living.active(potion::FIRE_RESISTANCE) {
            return false;
        }
        let mut fresh = true;
        if self.living.hurt_resistant_time as f32 > self.living.max_hurt_resistant_time as f32 / 2.0
        {
            if amount <= self.living.last_damage {
                return false;
            }
            let a = amount - self.living.last_damage;
            self.damage_entity(&src, a);
            self.living.last_damage = amount;
            fresh = false;
        } else {
            self.living.last_damage = amount;
            self.living.prev_health = self.living.health;
            self.living.hurt_resistant_time = self.living.max_hurt_resistant_time;
            self.damage_entity(&src, amount);
            self.living.hurt_time = 10;
            self.living.max_hurt_time = 10;
        }
        if fresh {
            self.hurt_anim = true;
            if let Some((_, from)) = src.attacker {
                let mut dx = from[0] - self.e.pos[0];
                let mut dz = from[2] - self.e.pos[2];
                while dx * dx + dz * dz < 1.0e-4 {
                    dx = (self.e.rand.next_double() - self.e.rand.next_double()) * 0.01;
                    dz = (self.e.rand.next_double() - self.e.rand.next_double()) * 0.01;
                }
                self.knock_back(dx, dz);
            }
        }
        if self.living.health <= 0.0 {
            // getDeathSound
            let pitch = self.sound_pitch();
            self.fx
                .push(Fx::Sound("game.player.die", self.e.pos, 1.0, pitch));
            self.on_death();
        } else if fresh {
            // getHurtSound
            let pitch = self.sound_pitch();
            self.fx
                .push(Fx::Sound("game.player.hurt", self.e.pos, 1.0, pitch));
        }
        true
    }

    /// `EntityLivingBase.getSoundPitch` (adults)
    fn sound_pitch(&mut self) -> f32 {
        (self.e.rand.next_float() - self.e.rand.next_float()) * 0.2 + 1.0
    }

    /// `EntityLivingBase.knockBack` against the server-side motion; the
    /// result is sent to the client as an absolute velocity.
    fn knock_back(&mut self, dx: f64, dz: f64) {
        if self.e.rand.next_double() < self.living.attrs.knockback_resistance.value() {
            return;
        }
        let f = math::sqrt_double(dx * dx + dz * dz) as f64;
        let k = 0.4f32 as f64;
        let m = &mut self.e.motion;
        m[0] /= 2.0;
        m[1] /= 2.0;
        m[2] /= 2.0;
        m[0] -= dx / f * k;
        m[1] += k;
        m[2] -= dz / f * k;
        if m[1] > 0.4f32 as f64 {
            m[1] = 0.4f32 as f64;
        }
        self.velocity = Some(*m);
    }

    /// After the worn pieces changed: the armour value and the Fire
    /// Protection that `setFire` reads.
    pub fn armor_updated(&mut self) {
        self.living.armor = self.total_armor();
        self.e.fire_protection =
            crate::enchant::max_level_in(crate::enchant::FIRE_PROTECTION, &self.armor_items);
    }

    /// `getTotalArmorValue`: the `damageReduceAmount` of the worn pieces.
    pub fn total_armor(&self) -> i32 {
        self.armor_items
            .iter()
            .flatten()
            .filter_map(|s| crate::inventory::armor_points(s.id))
            .sum()
    }

    /// `InventoryPlayer.damageArmor`: every worn `ItemArmor` takes
    /// `damage / 4` (at least 1) through `ItemStack.damageItem` with the
    /// player's random, so Unbreaking can spare it.
    fn damage_armor(&mut self, damage: f32) {
        let mut n = damage / 4.0;
        if n < 1.0 {
            n = 1.0;
        }
        for i in 0..4 {
            let Some(s) = self.armor_items[i] else {
                continue;
            };
            if crate::inventory::armor_points(s.id).is_none() {
                continue;
            }
            let (left, broke) =
                crate::enchant::damage_item(s, n as i32, self.creative, &mut self.e.rand);
            if broke {
                self.fx.push(Fx::BrokenItem(s));
            }
            self.armor_items[i] = left;
            self.armor_changed = true;
        }
        self.e.fire_protection =
            crate::enchant::max_level_in(crate::enchant::FIRE_PROTECTION, &self.armor_items);
    }

    /// `EntityPlayer.damageEntity`
    fn damage_entity(&mut self, src: &Source, mut a: f32) {
        // isBlocking: a sword held up halves what gets through
        let blocking = self.using.is_some_and(|(_, st, _)| {
            crate::item::tool(st.id).is_some_and(|t| t.kind == crate::item::ToolKind::Sword)
        });
        if !src.unblockable && blocking && a > 0.0 {
            a = (1.0 + a) * 0.5;
        }
        // applyArmorCalculations: getTotalArmorValue, then damageArmor,
        // then the reduction
        let a = if src.unblockable {
            a
        } else {
            self.living.armor = self.total_armor();
            let k = 25 - self.living.armor;
            let f = a * k as f32;
            self.damage_armor(a);
            f / 25.0
        };
        let items = self.armor_items;
        let a = self
            .living
            .potion_calc(src, a, || crate::enchant::modifier_damage(&items, src));
        let before = a;
        let a = (a - self.living.absorption).max(0.0);
        self.living.absorption -= before - a;
        if a != 0.0 {
            self.food.add_exhaustion(src.hunger());
            let h = self.living.health;
            self.living.set_health(h - a);
            self.track_damage(*src, a);
        }
    }

    fn fall_suffix(&self, w: &ServerWorld) -> Option<&'static str> {
        if self.on_ladder(w) {
            let b = w.peek_block(
                math::floor_double(self.e.pos[0]),
                math::floor_double(self.e.bb.min[1]),
                math::floor_double(self.e.pos[2]),
            );
            if b == block::LADDER {
                return Some("ladder");
            } else if b == block::VINE {
                return Some("vines");
            }
            None
        } else if self.e.in_water {
            Some("water")
        } else {
            None
        }
    }

    /// `CombatTracker.trackDamage` (the fall suffix is worked out by the
    /// caller that has the world: `tick` records it on the last entry).
    fn track_damage(&mut self, src: Source, a: f32) {
        self.reset_combat();
        self.combat.push(CombatEntry {
            src,
            ticks: self.e.ticks_existed,
            damage: a,
            fall_suffix: None,
            fall_distance: self.e.fall_distance,
        });
        self.last_damage_time = self.e.ticks_existed;
        self.taking_damage = true;
        if src.attacker.is_some() && !self.in_combat && self.living.health > 0.0 {
            self.in_combat = true;
        }
    }

    /// `CombatTracker.reset`
    fn reset_combat(&mut self) {
        let k = if self.in_combat { 300 } else { 100 };
        if self.taking_damage
            && (self.living.health <= 0.0 || self.e.ticks_existed - self.last_damage_time > k)
        {
            self.taking_damage = false;
            self.in_combat = false;
            self.combat.clear();
        }
    }

    /// `EntityPlayerMP.onDeath` (the rest is the game state's).
    fn on_death(&mut self) {
        self.dead = true;
    }

    /// `CombatTracker.getDeathMessage`
    pub fn death_message(&self, names: &dyn Fn(&Source) -> Option<String>) -> Message {
        let me = "Player".to_string();
        let Some(last) = self.combat.last() else {
            return Message {
                key: "death.attack.generic".into(),
                args: vec![me],
            };
        };
        let is_fall = |s: &Source| s.kind == SourceKind::Fall;
        let is_void = |s: &Source| s.kind == SourceKind::OutOfWorld;
        // getBestCombatEntry
        // (the suffix fallback compares against a byte that stays 0, so it
        // never applies)
        let mut best: Option<&CombatEntry> = None;
        let mut most = 0.0f32;
        for (i, c) in self.combat.iter().enumerate() {
            let prev = if i > 0 {
                Some(&self.combat[i - 1])
            } else {
                None
            };
            // CombatEntry.getDamageAmount
            let amount = if is_void(&c.src) {
                f32::MAX
            } else {
                c.fall_distance
            };
            if (is_fall(&c.src) || is_void(&c.src))
                && amount > 0.0
                && (best.is_none() || amount > most)
            {
                best = if i > 0 { prev } else { Some(c) };
                most = amount;
            }
        }
        let best = if most > 5.0 && best.is_some() {
            best
        } else {
            None
        };
        let last_name = names(&last.src);
        if let (Some(b), true) = (best, is_fall(&last.src)) {
            let bname = names(&b.src);
            if is_fall(&b.src) || is_void(&b.src) {
                let suffix = b.fall_suffix.unwrap_or("generic");
                return Message {
                    key: format!("death.fell.accident.{suffix}"),
                    args: vec![me],
                };
            } else if bname.is_some() && (last_name.is_none() || bname != last_name) {
                return Message {
                    key: "death.fell.assist".into(),
                    args: vec![me, bname.unwrap()],
                };
            } else if let Some(n) = last_name {
                return Message {
                    key: "death.fell.finish".into(),
                    args: vec![me, n],
                };
            } else {
                return Message {
                    key: "death.fell.killer".into(),
                    args: vec![me],
                };
            }
        }
        // DamageSource.getDeathMessage
        let ty = damage_type(&last.src);
        let key = format!("death.attack.{ty}");
        let mut args = vec![me];
        if let Some(n) = last_name {
            args.push(n);
        }
        Message { key, args }
    }

    /// One server tick of `EntityPlayerMP.onUpdateEntity` for the survival
    /// state: `Entity.onEntityUpdate`, `EntityLivingBase.onEntityUpdate`,
    /// potion effects, `FoodStats.onUpdate` and the peaceful regeneration.
    pub fn tick(&mut self, w: &ServerWorld, diff: Difficulty, natural_regen: bool) {
        if !self.have_pos || self.dead {
            if self.dead {
                self.living.death_time += 1;
            }
            return;
        }
        self.e.ticks_existed += 1;
        self.entity_age += 1;
        // EntityPlayer.onUpdate
        if self.xp_cooldown > 0 {
            self.xp_cooldown -= 1;
        }
        let wv = &*w as &dyn BlockView;
        let mut fx = Vec::new();
        // Entity.onEntityUpdate
        self.e.handle_water_movement(wv, None, &mut fx);
        if self.e.fire > 0 {
            if self.e.fire % 20 == 0 {
                self.attack(w, Source::named(SourceKind::OnFire), 1.0, diff);
            }
            self.e.fire -= 1;
        }
        if self.e.handle_lava_movement(wv) {
            self.attack(w, Source::named(SourceKind::Lava), 4.0, diff);
            self.e.set_fire(15);
            self.e.fall_distance *= 0.5;
        }
        if self.e.pos[1] < -64.0 {
            // EntityLivingBase.kill
            self.attack(w, Source::named(SourceKind::OutOfWorld), 4.0, diff);
        }
        // EntityLivingBase.onEntityUpdate
        if self.living.health > 0.0 && self.inside_opaque_block(w) {
            self.attack(w, Source::named(SourceKind::InWall), 1.0, diff);
        }
        if self.living.health > 0.0 && self.e.is_inside_of_material(wv, Material::Water, 0.12) {
            if !self.living.active(potion::WATER_BREATHING) && !self.creative {
                // decreaseAirSupply: Respiration may keep the breath
                let r =
                    crate::enchant::max_level_in(crate::enchant::RESPIRATION, &self.armor_items);
                if !(r > 0 && self.e.rand.next_int_n(r + 1) > 0) {
                    self.air -= 1;
                }
                if self.air == -20 {
                    self.air = 0;
                    for _ in 0..8 {
                        let r = &mut self.e.rand;
                        let a = r.next_float() - r.next_float();
                        let b = r.next_float() - r.next_float();
                        let c = r.next_float() - r.next_float();
                        let p = [
                            self.e.pos[0] + a as f64,
                            self.e.pos[1] + b as f64,
                            self.e.pos[2] + c as f64,
                        ];
                        fx.push(Fx::Particle("bubble", p, self.e.motion));
                    }
                    self.attack(w, Source::named(SourceKind::Drown), 2.0, diff);
                }
            }
            self.e.fire = 0;
        } else {
            self.air = 300;
        }
        if self.living.health > 0.0 && self.e.is_wet(wv) {
            self.e.fire = 0;
        }
        if self.living.hurt_time > 0 {
            self.living.hurt_time -= 1;
        }
        if self.living.hurt_resistant_time > 0 {
            self.living.hurt_resistant_time -= 1;
        }
        // updatePotionEffects
        let mut pending: Vec<(i32, i32)> = Vec::new();
        self.living
            .update_effects(|_, id, amp| pending.push((id, amp)));
        for (id, amp) in pending {
            self.perform_effect(w, id, amp, diff);
        }
        self.potion_particles(&mut fx);
        // EntityPlayer.onUpdate: FoodStats.onUpdate
        self.food_update(w, diff, natural_regen);
        // EntityPlayer.onLivingUpdate: peaceful regeneration
        if diff == Difficulty::Peaceful
            && self.living.health < self.living.max_health()
            && natural_regen
            && self.e.ticks_existed % 240 == 0
        {
            self.living.heal(1.0);
        }
        // the zero-input moveEntityWithHeading of the server-side player
        // keeps the motion that knockback starts from
        if self.e.in_water || self.e.handle_lava_movement(wv) {
            let f = if self.e.in_water { 0.8 } else { 0.5 };
            self.e.motion[0] *= f;
            self.e.motion[1] = self.e.motion[1] * f - 0.02;
            self.e.motion[2] *= f;
        } else {
            let mut f = 0.91f32;
            if self.on_ground {
                let b = w.peek_block(
                    math::floor_double(self.e.pos[0]),
                    math::floor_double(self.e.bb.min[1]) - 1,
                    math::floor_double(self.e.pos[2]),
                );
                f = block::info(b).slipperiness * 0.91;
            }
            if self.on_ground {
                self.e.motion[1] = 0.0;
            } else {
                self.e.motion[1] -= 0.08;
                self.e.motion[1] *= 0.98f32 as f64;
            }
            self.e.motion[0] *= f as f64;
            self.e.motion[2] *= f as f64;
        }
        self.reset_combat();
        self.fx.extend(fx);
    }

    /// `Entity.isEntityInsideOpaqueBlock` (eye-level corners)
    fn inside_opaque_block(&self, w: &ServerWorld) -> bool {
        for i in 0..8 {
            let fx = (((i >> 0) % 2) as f32 - 0.5) * self.e.width * 0.8;
            let fy = (((i >> 1) % 2) as f32 - 0.5) * 0.1;
            let fz = (((i >> 2) % 2) as f32 - 0.5) * self.e.width * 0.8;
            let x = math::floor_double(self.e.pos[0] + fx as f64);
            // getEyeHeight is 0.12 for the player
            let y = math::floor_double(self.e.pos[1] + 0.12f32 as f64 + fy as f64);
            let z = math::floor_double(self.e.pos[2] + fz as f64);
            if block::info(w.peek_block(x, y, z)).normal_cube() {
                return true;
            }
        }
        false
    }

    /// `Potion.performEffect` for the player.
    fn perform_effect(&mut self, w: &ServerWorld, id: i32, amp: i32, diff: Difficulty) {
        match id {
            potion::REGENERATION => {
                if self.living.health < self.living.max_health() {
                    self.living.heal(1.0);
                }
            }
            potion::POISON => {
                if self.living.health > 1.0 {
                    self.attack(w, Source::named(SourceKind::Magic), 1.0, diff);
                }
            }
            potion::WITHER => {
                self.attack(w, Source::named(SourceKind::Wither), 1.0, diff);
            }
            potion::HUNGER => self.food.add_exhaustion(0.025 * (amp + 1) as f32),
            potion::SATURATION => self.food.add_stats(amp + 1, 1.0),
            potion::HEAL => self.living.heal((4 << amp).max(0) as f32),
            potion::HARM => {
                self.attack(w, Source::named(SourceKind::Magic), (6 << amp) as f32, diff);
            }
            _ => {}
        }
    }

    /// The particle part of `updatePotionEffects`.
    fn potion_particles(&mut self, fx: &mut Vec<Fx>) {
        let c = self.living.potion_color;
        if c <= 0 {
            return;
        }
        let r = &mut self.e.rand;
        let mut show = if !self.living.invisible {
            r.next_boolean()
        } else {
            r.next_int_n(15) == 0
        };
        if self.living.potion_ambient {
            show &= r.next_int_n(5) == 0;
        }
        if show {
            let rgb = [
                (c >> 16 & 255) as f64 / 255.0,
                (c >> 8 & 255) as f64 / 255.0,
                (c & 255) as f64 / 255.0,
            ];
            let p = [
                self.e.pos[0] + (r.next_double() - 0.5) * self.e.width as f64,
                self.e.pos[1] + r.next_double() * self.e.height as f64 - self.e.y_offset as f64,
                self.e.pos[2] + (r.next_double() - 0.5) * self.e.width as f64,
            ];
            let name = if self.living.potion_ambient {
                "mobSpellAmbient"
            } else {
                "mobSpell"
            };
            fx.push(Fx::Particle(name, p, rgb));
        }
    }

    /// `FoodStats.onUpdate`
    fn food_update(&mut self, w: &ServerWorld, diff: Difficulty, natural_regen: bool) {
        let f = &mut self.food;
        f.prev_level = f.level;
        if f.exhaustion > 4.0 {
            f.exhaustion -= 4.0;
            if f.saturation > 0.0 {
                f.saturation = (f.saturation - 1.0).max(0.0);
            } else if diff != Difficulty::Peaceful {
                f.level = (f.level - 1).max(0);
            }
        }
        // EntityPlayer.shouldHeal
        let should_heal = self.living.health > 0.0 && self.living.health < self.living.max_health();
        if natural_regen && self.food.level >= 18 && should_heal {
            self.food.timer += 1;
            if self.food.timer >= 80 {
                self.living.heal(1.0);
                self.food.add_exhaustion(3.0);
                self.food.timer = 0;
            }
        } else if self.food.level <= 0 {
            self.food.timer += 1;
            if self.food.timer >= 80 {
                let h = self.living.health;
                if h > 10.0 || diff == Difficulty::Hard || (h > 1.0 && diff == Difficulty::Normal) {
                    self.attack(w, Source::named(SourceKind::Starve), 1.0, diff);
                }
                self.food.timer = 0;
            }
        } else {
            self.food.timer = 0;
        }
    }

    /// `EntityPlayerMP.clonePlayer` after death (no keepInventory).
    pub fn respawn(&mut self) {
        let creative = self.creative;
        let score = self.score;
        *self = ServerPlayer::new();
        self.creative = creative;
        let _ = score;
    }

    /// The save string: health, food, saturation, exhaustion, air, fire,
    /// level, total, progress, absorption, food timer, then the effects.
    pub fn encode(&self) -> String {
        let mut s = format!(
            "{},{},{},{},{},{},{},{},{},{},{}",
            self.living.health,
            self.food.level,
            self.food.saturation,
            self.food.exhaustion,
            self.air,
            self.e.fire,
            self.xp_level,
            self.xp_total,
            self.xp,
            self.living.absorption,
            self.food.timer
        );
        for e in self.living.effects.iter() {
            s.push_str(&format!(
                ",{}:{}:{}:{}",
                e.id, e.amplifier, e.duration, e.ambient as i32
            ));
        }
        s
    }

    pub fn decode(&mut self, t: &str) {
        let f: Vec<&str> = t.split(',').collect();
        if f.len() < 6 {
            return;
        }
        let num = |i: usize| f.get(i).and_then(|v| v.parse::<f64>().ok());
        if let Some(h) = num(0) {
            self.living.health = (h as f32).clamp(0.0, 20.0);
        }
        self.food.level = num(1).unwrap_or(20.0).clamp(0.0, 20.0) as i32;
        self.food.saturation = num(2).unwrap_or(5.0) as f32;
        self.food.exhaustion = num(3).unwrap_or(0.0).clamp(0.0, 40.0) as f32;
        self.air = num(4).unwrap_or(300.0).clamp(-20.0, 300.0) as i32;
        self.e.fire = num(5).unwrap_or(0.0) as i32;
        if f.len() >= 9 {
            self.xp_level = num(6).unwrap_or(0.0).max(0.0) as i32;
            self.xp_total = num(7).unwrap_or(0.0).max(0.0) as i32;
            self.xp = num(8).unwrap_or(0.0).clamp(0.0, 1.0) as f32;
        }
        if f.len() >= 11 {
            self.living.absorption = num(9).unwrap_or(0.0) as f32;
            self.food.timer = num(10).unwrap_or(0.0) as i32;
            for e in &f[11..] {
                let p: Vec<i32> = e.split(':').filter_map(|v| v.parse().ok()).collect();
                if p.len() == 4 && potion::potion(p[0]).is_some() {
                    self.living
                        .add_effect(Effect::ambient(p[0], p[2], p[1], p[3] != 0));
                }
            }
            // add_effect gave absorption again
            self.living.absorption = num(9).unwrap_or(0.0) as f32;
        }
        if self.living.health <= 0.0 {
            // died and quit: come back alive
            let c = self.creative;
            *self = ServerPlayer::new();
            self.creative = c;
        }
    }
}

impl Default for ServerPlayer {
    fn default() -> Self {
        ServerPlayer::new()
    }
}

/// `DamageSource.damageType`
fn damage_type(s: &Source) -> &'static str {
    match s.kind {
        SourceKind::InFire => "inFire",
        SourceKind::OnFire => "onFire",
        SourceKind::Lava => "lava",
        SourceKind::InWall => "inWall",
        SourceKind::Drown => "drown",
        SourceKind::Starve => "starve",
        SourceKind::Cactus => "cactus",
        SourceKind::Fall => "fall",
        SourceKind::OutOfWorld => "outOfWorld",
        SourceKind::Generic => "generic",
        SourceKind::Magic => "magic",
        SourceKind::Wither => "wither",
        SourceKind::Anvil => "anvil",
        SourceKind::FallingBlock => "fallingBlock",
        SourceKind::Mob(_) => "mob",
        SourceKind::Player => "player",
        SourceKind::Arrow => "arrow",
        SourceKind::Fireball => "fireball",
        SourceKind::Thrown => "thrown",
        SourceKind::IndirectMagic => "indirectMagic",
        SourceKind::Thorns => "thorns",
        SourceKind::Explosion => {
            if s.attacker.is_some() {
                "explosion.player"
            } else {
                "explosion"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::gen::chunk_provider::ChunkProvider;

    fn world() -> ServerWorld {
        let mut w = ServerWorld::new(std::sync::Arc::new(ChunkProvider::new(1)));
        for cx in -2..=2 {
            for cz in -2..=2 {
                w.chunk(cx, cz);
            }
        }
        for x in -8..8 {
            for z in -8..8 {
                for y in 1..200 {
                    w.set_block(x, y, z, block::AIR, 0, 2);
                }
                w.set_block(x, 0, z, block::STONE, 0, 2);
            }
        }
        w
    }

    fn at(p: &mut ServerPlayer, w: &ServerWorld, y: f64, ground: bool) {
        p.moved(w, [0.5, y, 0.5], ground, false, false, false);
    }

    #[test]
    fn fall_damage_is_distance_minus_three() {
        let w = world();
        let mut p = ServerPlayer::new();
        at(&mut p, &w, 11.0, false);
        for i in 0..10 {
            at(&mut p, &w, 10.0 - i as f64, false);
        }
        at(&mut p, &w, 1.0, true);
        assert_eq!(p.living.health, 13.0);
        assert_eq!(p.e.fall_distance, 0.0);
    }

    #[test]
    fn creative_takes_no_fall_damage() {
        let w = world();
        let mut p = ServerPlayer::new();
        p.creative = true;
        at(&mut p, &w, 40.0, false);
        at(&mut p, &w, 1.0, true);
        assert_eq!(p.living.health, 20.0);
    }

    #[test]
    fn drowning_starts_after_fifteen_seconds() {
        let mut w = world();
        for y in 1..5 {
            w.set_block(0, y, 0, block::WATER, 0, 2);
        }
        let mut p = ServerPlayer::new();
        at(&mut p, &w, 1.0, true);
        for _ in 0..319 {
            p.tick(&w, Difficulty::Normal, true);
        }
        assert_eq!(p.living.health, 20.0);
        p.tick(&w, Difficulty::Normal, true);
        assert_eq!(p.living.health, 18.0);
    }

    #[test]
    fn hurt_resistance_blocks_repeated_damage() {
        let w = world();
        let mut p = ServerPlayer::new();
        at(&mut p, &w, 1.0, true);
        let lava = Source::named(SourceKind::Lava);
        assert!(p.attack(&w, lava, 4.0, Difficulty::Normal));
        assert!(!p.attack(&w, lava, 4.0, Difficulty::Normal));
        assert_eq!(p.living.health, 16.0);
    }

    #[test]
    fn regenerates_when_fed_and_scales_mob_damage() {
        let w = world();
        let mut p = ServerPlayer::new();
        at(&mut p, &w, 1.0, true);
        p.living.health = 10.0;
        for _ in 0..80 {
            p.tick(&w, Difficulty::Normal, true);
        }
        assert_eq!(p.living.health, 11.0);
        // EntityPlayer.attackEntityFrom: Easy halves mob damage plus one
        let z = Source::mob(crate::entity::mob::Kind::Zombie, 1, [3.5, 1.0, 0.5]);
        p.attack(&w, z, 4.0, Difficulty::Easy);
        assert_eq!(p.living.health, 8.0);
        // knockback away from the zombie
        assert!(p.velocity.unwrap()[0] < 0.0);
    }

    #[test]
    fn resistance_and_poison() {
        let w = world();
        let mut p = ServerPlayer::new();
        at(&mut p, &w, 1.0, true);
        p.living.add_effect(Effect::new(potion::RESISTANCE, 100, 1));
        // (1 + 1) * 5 = 10 of 25 taken off
        p.attack(
            &w,
            Source::named(SourceKind::Cactus),
            5.0,
            Difficulty::Normal,
        );
        assert_eq!(p.living.health, 17.0);
        p.living.add_effect(Effect::new(potion::POISON, 50, 0));
        p.living.hurt_resistant_time = 0;
        for _ in 0..50 {
            p.tick(&w, Difficulty::Normal, true);
            p.living.hurt_resistant_time = 0;
        }
        // magic bypasses armour but not Resistance: two hits of 0.6
        assert!((p.living.health - (17.0 - 2.0 * 0.6)).abs() < 1e-5);
        let m = p.death_message(&|_| None);
        assert_eq!(m.key, "death.attack.magic");
    }
}
