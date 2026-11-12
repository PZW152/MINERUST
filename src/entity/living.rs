//! The parts of `EntityLivingBase` shared by the player and the mobs:
//! attributes (`BaseAttributeMap`, `ModifiableAttributeInstance`), active
//! potion effects and the damage pipeline (`attackEntityFrom`,
//! `damageEntity`, armour, resistance, absorption).

use crate::potion::{self, Attr, Effect};

/// `AttributeModifier`
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Modifier {
    pub uuid: u128,
    pub amount: f64,
    /// 0 add, 1 multiply base, 2 multiply total
    pub op: i32,
}

/// `ModifiableAttributeInstance` of a `RangedAttribute`.
#[derive(Clone, Debug)]
pub struct Attribute {
    pub base: f64,
    pub min: f64,
    pub max: f64,
    /// in insertion order
    pub modifiers: Vec<Modifier>,
}

impl Attribute {
    pub fn new(base: f64, min: f64, max: f64) -> Attribute {
        Attribute {
            base,
            min,
            max,
            modifiers: Vec::new(),
        }
    }

    /// `computeValue`
    pub fn value(&self) -> f64 {
        let mut b = self.base;
        for m in self.modifiers.iter().filter(|m| m.op == 0) {
            b += m.amount;
        }
        let mut t = b;
        for m in self.modifiers.iter().filter(|m| m.op == 1) {
            t += b * m.amount;
        }
        for m in self.modifiers.iter().filter(|m| m.op == 2) {
            t *= 1.0 + m.amount;
        }
        // RangedAttribute.clampValue
        t.clamp(self.min, self.max)
    }

    pub fn remove(&mut self, uuid: u128) {
        self.modifiers.retain(|m| m.uuid != uuid);
    }

    pub fn apply(&mut self, m: Modifier) {
        self.remove(m.uuid);
        self.modifiers.push(m);
    }
}

/// `SharedMonsterAttributes` an entity registers.
#[derive(Clone, Debug)]
pub struct Attributes {
    pub max_health: Attribute,
    pub follow_range: Attribute,
    pub knockback_resistance: Attribute,
    pub movement_speed: Attribute,
    pub attack_damage: Attribute,
}

impl Attributes {
    /// `EntityLivingBase.applyEntityAttributes` defaults (`EntityPlayer`
    /// sets the speed to 0.1 and registers attack damage 1.0).
    pub fn new(max_health: f64, speed: f64, attack: f64) -> Attributes {
        Attributes {
            max_health: Attribute::new(max_health, 0.0, f64::MAX),
            follow_range: Attribute::new(32.0, 0.0, 2048.0),
            knockback_resistance: Attribute::new(0.0, 0.0, 1.0),
            movement_speed: Attribute::new(speed, 0.0, f64::MAX),
            attack_damage: Attribute::new(attack, 0.0, f64::MAX),
        }
    }

    pub fn get_mut(&mut self, a: Attr) -> &mut Attribute {
        match a {
            Attr::MaxHealth => &mut self.max_health,
            Attr::MovementSpeed => &mut self.movement_speed,
            Attr::AttackDamage => &mut self.attack_damage,
        }
    }
}

/// `HashMap<Integer, PotionEffect> activePotionsMap` with Java 8 iteration
/// order: bucket `id & (capacity - 1)`, insertion order inside a bucket.
#[derive(Clone, Debug, Default)]
pub struct Effects {
    list: Vec<Effect>,
    cap: usize,
}

impl Effects {
    fn grow(&mut self) {
        if self.cap == 0 {
            self.cap = 16;
        }
        while self.list.len() > self.cap * 3 / 4 {
            self.cap *= 2;
        }
    }

    /// Indices in iteration order.
    fn order(&self) -> Vec<usize> {
        let cap = self.cap.max(16);
        let mut v: Vec<(usize, usize)> = self
            .list
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let h = e.id as u32;
                (((h ^ (h >> 16)) as usize) & (cap - 1), i)
            })
            .collect();
        v.sort();
        v.into_iter().map(|x| x.1).collect()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Effect> {
        let o = self.order();
        o.into_iter().map(move |i| &self.list[i])
    }

    pub fn values(&self) -> Vec<Effect> {
        self.iter().copied().collect()
    }

    pub fn get(&self, id: i32) -> Option<&Effect> {
        self.list.iter().find(|e| e.id == id)
    }

    pub fn get_mut(&mut self, id: i32) -> Option<&mut Effect> {
        self.list.iter_mut().find(|e| e.id == id)
    }

    pub fn contains(&self, id: i32) -> bool {
        self.get(id).is_some()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    fn insert(&mut self, e: Effect) {
        self.list.push(e);
        self.grow();
    }

    fn remove(&mut self, id: i32) -> Option<Effect> {
        let i = self.list.iter().position(|e| e.id == id)?;
        Some(self.list.remove(i))
    }

    pub fn clear(&mut self) {
        self.list.clear();
    }
}

/// `DamageSource`
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Source {
    pub kind: SourceKind,
    /// `isUnblockable`: armour does not apply
    pub unblockable: bool,
    /// `isDamageAbsolute`: neither resistance nor enchantments apply
    pub absolute: bool,
    pub fire: bool,
    pub explosion: bool,
    pub projectile: bool,
    pub magic: bool,
    /// `isDifficultyScaled` (damage from an entity against players)
    pub difficulty_scaled: bool,
    /// The attacker (`getEntity`): its id (`u32::MAX` for the player) and
    /// position, for the knockback direction.
    pub attacker: Option<(u32, [f64; 3])>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    InFire,
    OnFire,
    Lava,
    InWall,
    Drown,
    Starve,
    Cactus,
    Fall,
    OutOfWorld,
    Generic,
    Magic,
    Wither,
    Anvil,
    FallingBlock,
    Mob(crate::entity::mob::Kind),
    Player,
    Arrow,
    Fireball,
    Thrown,
    IndirectMagic,
    Thorns,
    Explosion,
}

impl Source {
    pub const fn of(kind: SourceKind) -> Source {
        Source {
            kind,
            unblockable: false,
            absolute: false,
            fire: false,
            explosion: false,
            projectile: false,
            magic: false,
            difficulty_scaled: false,
            attacker: None,
        }
    }

    /// The static `DamageSource` fields.
    pub fn named(kind: SourceKind) -> Source {
        let mut s = Source::of(kind);
        match kind {
            SourceKind::InFire => s.fire = true,
            SourceKind::OnFire => {
                s.unblockable = true;
                s.fire = true
            }
            SourceKind::Lava => s.fire = true,
            SourceKind::InWall | SourceKind::Drown | SourceKind::Fall | SourceKind::Generic => {
                s.unblockable = true
            }
            SourceKind::Starve => {
                s.unblockable = true;
                s.absolute = true;
            }
            SourceKind::OutOfWorld => {
                // setDamageBypassesArmor().setDamageAllowedInCreativeMode()
                s.unblockable = true;
            }
            SourceKind::Magic => {
                s.unblockable = true;
                s.magic = true
            }
            SourceKind::Wither => s.unblockable = true,
            _ => {}
        }
        s
    }

    /// `getHungerDamage`: `setDamageBypassesArmor` also zeroes it.
    pub fn hunger(&self) -> f32 {
        if self.unblockable {
            0.0
        } else {
            0.3
        }
    }

    /// `DamageSource.causeMobDamage`
    pub fn mob(kind: crate::entity::mob::Kind, id: u32, pos: [f64; 3]) -> Source {
        let mut s = Source::of(SourceKind::Mob(kind));
        s.attacker = Some((id, pos));
        s.difficulty_scaled = true;
        s
    }

    /// `DamageSource.setExplosionSource(explosion)`
    pub fn explosion(attacker: Option<(u32, [f64; 3])>) -> Source {
        let mut s = Source::of(SourceKind::Explosion);
        s.explosion = true;
        s.difficulty_scaled = true;
        s.attacker = attacker;
        s
    }

    /// `causeArrowDamage`
    pub fn arrow(attacker: Option<(u32, [f64; 3])>) -> Source {
        let mut s = Source::of(SourceKind::Arrow);
        s.projectile = true;
        s.attacker = attacker;
        s.difficulty_scaled = attacker.is_some();
        s
    }

    /// `causeFireballDamage`
    pub fn fireball(attacker: Option<(u32, [f64; 3])>) -> Source {
        let mut s = Source::of(SourceKind::Fireball);
        s.projectile = true;
        s.fire = true;
        s.attacker = attacker;
        s.difficulty_scaled = attacker.is_some();
        s
    }

    /// `causeIndirectMagicDamage`
    pub fn indirect_magic(attacker: Option<(u32, [f64; 3])>) -> Source {
        let mut s = Source::of(SourceKind::IndirectMagic);
        s.unblockable = true;
        s.magic = true;
        s.attacker = attacker;
        s.difficulty_scaled = attacker.is_some();
        s
    }
}

/// `EnumDifficulty`
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Difficulty {
    Peaceful = 0,
    Easy = 1,
    Normal = 2,
    Hard = 3,
}

/// The living-entity state the player and mobs share.
#[derive(Clone, Debug)]
pub struct Living {
    pub health: f32,
    pub prev_health: f32,
    pub absorption: f32,
    pub attrs: Attributes,
    pub effects: Effects,
    pub potions_need_update: bool,
    /// dataWatcher 7 and 8: the particle colour and ambience
    pub potion_color: i32,
    pub potion_ambient: bool,
    pub invisible: bool,
    pub hurt_resistant_time: i32,
    pub max_hurt_resistant_time: i32,
    pub last_damage: f32,
    pub hurt_time: i32,
    pub max_hurt_time: i32,
    pub death_time: i32,
    /// `EnumCreatureAttribute.UNDEAD`
    pub undead: bool,
    /// `getTotalArmorValue`
    pub armor: i32,
}

impl Living {
    pub fn new(attrs: Attributes) -> Living {
        let h = attrs.max_health.value() as f32;
        Living {
            health: h,
            prev_health: h,
            absorption: 0.0,
            attrs,
            effects: Effects::default(),
            potions_need_update: true,
            potion_color: 0,
            potion_ambient: false,
            invisible: false,
            hurt_resistant_time: 0,
            max_hurt_resistant_time: 20,
            last_damage: 0.0,
            hurt_time: 0,
            max_hurt_time: 0,
            death_time: 0,
            undead: false,
            armor: 0,
        }
    }

    pub fn max_health(&self) -> f32 {
        self.attrs.max_health.value() as f32
    }

    /// `setHealth`: clamped to `[0, getMaxHealth()]`.
    pub fn set_health(&mut self, h: f32) {
        self.health = h.clamp(0.0, self.max_health());
    }

    /// `heal`
    pub fn heal(&mut self, a: f32) {
        if self.health > 0.0 {
            self.set_health(self.health + a);
        }
    }

    pub fn active(&self, id: i32) -> bool {
        self.effects.contains(id)
    }

    pub fn amplifier(&self, id: i32) -> Option<i32> {
        self.effects.get(id).map(|e| e.amplifier)
    }

    /// `isPotionApplicable`
    pub fn applicable(&self, e: &Effect) -> bool {
        !(self.undead && (e.id == potion::REGENERATION || e.id == potion::POISON))
    }

    fn apply_modifiers(&mut self, e: &Effect) {
        if e.id == potion::ABSORPTION {
            self.absorption += (4 * (e.amplifier + 1)) as f32;
        }
        let pt = e.potion();
        if let Some((a, uuid, amount, op)) = pt.modifier {
            let at = self.attrs.get_mut(a);
            at.remove(uuid);
            at.apply(Modifier {
                uuid,
                amount: pt.modifier_amount(e.amplifier, amount),
                op,
            });
        }
    }

    fn remove_modifiers(&mut self, e: &Effect) {
        if e.id == potion::ABSORPTION {
            self.absorption -= (4 * (e.amplifier + 1)) as f32;
        }
        let pt = e.potion();
        if let Some((a, uuid, _, _)) = pt.modifier {
            self.attrs.get_mut(a).remove(uuid);
        }
        if e.id == potion::HEALTH_BOOST && self.health > self.max_health() {
            self.health = self.max_health();
        }
    }

    /// `addPotionEffect`
    pub fn add_effect(&mut self, e: Effect) {
        if !self.applicable(&e) {
            return;
        }
        if self.effects.contains(e.id) {
            let cur = self.effects.get_mut(e.id).unwrap();
            cur.combine(&e);
            let cur = *cur;
            // onChangedPotionEffect(effect, true)
            self.potions_need_update = true;
            self.remove_modifiers(&cur);
            self.apply_modifiers(&cur);
        } else {
            self.effects.insert(e);
            self.potions_need_update = true;
            self.apply_modifiers(&e);
        }
    }

    /// `removePotionEffect`
    pub fn remove_effect(&mut self, id: i32) {
        if let Some(e) = self.effects.remove(id) {
            self.potions_need_update = true;
            self.remove_modifiers(&e);
        }
    }

    /// `clearActivePotions` (milk)
    pub fn clear_effects(&mut self) {
        for e in self.effects.values() {
            self.effects.remove(e.id);
            self.potions_need_update = true;
            self.remove_modifiers(&e);
        }
    }

    /// `updatePotionEffects` up to the particles. `perform` runs
    /// `Potion.performEffect(entity, amplifier)` for the effects that are
    /// ready this tick, in iteration order.
    pub fn update_effects(&mut self, mut perform: impl FnMut(&mut Living, i32, i32)) {
        for e in self.effects.values() {
            let Some(cur) = self.effects.get(e.id).copied() else {
                continue;
            };
            // PotionEffect.onUpdate
            let mut c = cur;
            let alive = if c.duration > 0 {
                if c.potion().is_ready(c.duration, c.amplifier) {
                    perform(self, c.id, c.amplifier);
                }
                c.duration -= 1;
                c.duration > 0
            } else {
                false
            };
            if let Some(x) = self.effects.get_mut(c.id) {
                x.duration = c.duration;
            }
            if !alive {
                self.effects.remove(c.id);
                self.potions_need_update = true;
                self.remove_modifiers(&c);
            } else if c.duration % 600 == 0 {
                self.potions_need_update = true;
            }
        }
        if self.potions_need_update {
            if self.effects.is_empty() {
                self.potion_ambient = false;
                self.potion_color = 0;
                self.invisible = false;
            } else {
                let v = self.effects.values();
                self.potion_color = potion::liquid_color_of(&v);
                self.potion_ambient = potion::are_ambient(&v);
                self.invisible = self.active(potion::INVISIBILITY);
            }
            self.potions_need_update = false;
        }
    }

    /// `applyArmorCalculations` (the armour wear is the caller's).
    pub fn armor_calc(&self, s: &Source, a: f32) -> f32 {
        if s.unblockable {
            return a;
        }
        let k = 25 - self.armor;
        let f = a * k as f32;
        f / 25.0
    }

    /// `applyPotionDamageCalculations`; `ench` is
    /// `EnchantmentHelper.getEnchantmentModifierDamage` of the armour.
    pub fn potion_calc(&self, s: &Source, mut a: f32, ench: i32) -> f32 {
        if s.absolute {
            return a;
        }
        if let Some(amp) = self.amplifier(potion::RESISTANCE) {
            if s.kind != SourceKind::OutOfWorld {
                let k = (amp + 1) * 5;
                let f = a * (25 - k) as f32;
                a = f / 25.0;
            }
        }
        if a <= 0.0 {
            return 0.0;
        }
        let e = ench.min(20);
        if e > 0 {
            let f = a * (25 - e) as f32;
            a = f / 25.0;
        }
        a
    }

    /// `damageEntity` after the armour and potion reductions: absorption
    /// first, then health. Returns the health taken.
    pub fn absorb_and_take(&mut self, a: f32) -> f32 {
        let before = a;
        let a = (a - self.absorption).max(0.0);
        self.absorption -= before - a;
        if a != 0.0 {
            let h = self.health;
            self.set_health(h - a);
            self.absorption -= a;
        }
        a
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_and_strength_modifiers() {
        let mut l = Living::new(Attributes::new(20.0, 0.1f32 as f64, 1.0));
        l.add_effect(Effect::new(potion::MOVE_SPEED, 100, 1));
        // 0.1 * (1 + 0.2 * 2)
        assert!((l.attrs.movement_speed.value() - 0.1f32 as f64 * 1.4).abs() < 1e-9);
        l.add_effect(Effect::new(potion::DAMAGE_BOOST, 100, 0));
        assert!((l.attrs.attack_damage.value() - 1.0 * 2.3).abs() < 1e-9);
        l.add_effect(Effect::new(potion::HEALTH_BOOST, 3, 0));
        assert_eq!(l.max_health(), 24.0);
        l.heal(10.0);
        assert_eq!(l.health, 24.0);
        for _ in 0..3 {
            l.update_effects(|_, _, _| {});
        }
        assert!(!l.active(potion::HEALTH_BOOST));
        assert_eq!(l.max_health(), 20.0);
        assert_eq!(l.health, 20.0);
    }

    #[test]
    fn poison_ticks_every_25() {
        let mut l = Living::new(Attributes::new(20.0, 0.1, 1.0));
        l.add_effect(Effect::new(potion::POISON, 100, 0));
        let mut hits = 0;
        for _ in 0..100 {
            l.update_effects(|_, id, _| {
                if id == potion::POISON {
                    hits += 1;
                }
            });
        }
        assert_eq!(hits, 4);
    }
}
