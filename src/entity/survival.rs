//! Player health, hunger, air and fire (`EntityLivingBase`, `EntityPlayer`,
//! `FoodStats`), on Normal difficulty.

use super::player::Player;
use crate::block::Material;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GameMode {
    Survival,
    Creative,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Damage {
    Fall,
    Drown,
    Lava,
    Fire,
    Starve,
    Void,
    /// `DamageSource.causeMobDamage`
    Mob(crate::entity::mob::Kind),
    Arrow,
    Explosion,
    Poison,
}

#[derive(Clone, Debug)]
pub struct Stats {
    pub health: f32,
    pub food: i32,
    pub saturation: f32,
    pub exhaustion: f32,
    food_timer: i32,
    prev_food: i32,
    pub air: i32,
    pub fall_distance: f32,
    pub fire: i32,
    /// `hurtResistantTime`
    pub hurt_resistance: i32,
    last_damage: f32,
    /// `hurtTime` / `maxHurtTime`, for the camera shake and red tint.
    pub hurt_time: i32,
    pub dead: bool,
    pub death_time: i32,
    /// Last cause of death, for the death screen.
    pub last_cause: Option<Damage>,
    /// `experienceLevel`, `experienceTotal`, `experience` (bar progress).
    pub xp_level: i32,
    pub xp_total: i32,
    pub xp: f32,
    /// Remaining ticks of the poison effect.
    pub poison: i32,
}

pub const MAX_HEALTH: f32 = 20.0;
pub const MAX_HURT_TIME: i32 = 10;

impl Default for Stats {
    fn default() -> Self {
        Stats {
            health: MAX_HEALTH,
            food: 20,
            saturation: 5.0,
            exhaustion: 0.0,
            food_timer: 0,
            prev_food: 20,
            air: 300,
            fall_distance: 0.0,
            fire: 0,
            hurt_resistance: 0,
            last_damage: 0.0,
            hurt_time: 0,
            dead: false,
            death_time: 0,
            last_cause: None,
            xp_level: 0,
            xp_total: 0,
            xp: 0.0,
            poison: 0,
        }
    }
}

impl Stats {
    /// `FoodStats.addExhaustion`
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
        let n = n.min(i32::MAX - self.xp_total);
        self.xp += n as f32 / self.xp_bar_cap() as f32;
        self.xp_total += n;
        while self.xp >= 1.0 {
            self.xp = (self.xp - 1.0) * self.xp_bar_cap() as f32;
            self.xp_level += 1;
            self.xp /= self.xp_bar_cap() as f32;
        }
    }

    pub fn add_exhaustion(&mut self, f: f32) {
        self.exhaustion = (self.exhaustion + f).min(40.0);
    }

    /// `EntityLivingBase.attackEntityFrom` without armour or potions.
    pub fn damage(&mut self, cause: Damage, amount: f32, mode: GameMode) -> bool {
        if self.dead || amount <= 0.0 {
            return false;
        }
        if mode == GameMode::Creative && cause != Damage::Void {
            return false;
        }
        let applied = if self.hurt_resistance as f32 > 20.0 / 2.0 {
            if amount <= self.last_damage {
                return false;
            }
            let d = amount - self.last_damage;
            self.last_damage = amount;
            d
        } else {
            self.last_damage = amount;
            self.hurt_resistance = 20;
            self.hurt_time = MAX_HURT_TIME;
            amount
        };
        // EntityPlayer.damageEntity: hunger cost of taking damage.
        if cause != Damage::Starve && cause != Damage::Void && cause != Damage::Drown {
            self.add_exhaustion(0.3);
        }
        self.health = (self.health - applied).max(0.0);
        if self.health <= 0.0 {
            self.dead = true;
            self.last_cause = Some(cause);
        }
        true
    }

    /// `EntityLivingBase.fall` / `EntityPlayer.fall`
    fn fall(&mut self, distance: f32, mode: GameMode, flying_allowed: bool) {
        if flying_allowed {
            return;
        }
        let i = (distance - 3.0f32).ceil() as i32;
        if i > 0 {
            self.damage(Damage::Fall, i as f32, mode);
        }
    }

    /// One tick, after the player's movement.
    ///
    /// `moved_y` is the vertical displacement of this tick, `eye` the
    /// material at eye level, `in_lava` whether the body touches lava.
    pub fn tick(
        &mut self,
        p: &Player,
        moved: [f64; 3],
        jumped: bool,
        eye: Material,
        in_lava: bool,
        mode: GameMode,
    ) {
        if self.dead {
            self.death_time += 1;
            return;
        }
        // Potion.poison: one damage every 25 ticks while above one health
        if self.poison > 0 {
            if self.poison % 25 == 0 && self.health > 1.0 {
                self.damage(Damage::Poison, 1.0, mode);
            }
            self.poison -= 1;
        }
        if self.hurt_time > 0 {
            self.hurt_time -= 1;
        }
        if self.hurt_resistance > 0 {
            self.hurt_resistance -= 1;
        }
        let flying_allowed = mode == GameMode::Creative;

        // Entity.updateFallState
        if p.in_water {
            self.fall_distance = 0.0;
        }
        if p.on_ground {
            if self.fall_distance > 0.0 {
                let d = self.fall_distance;
                self.fall(d, mode, flying_allowed);
                self.fall_distance = 0.0;
            }
        } else if moved[1] < 0.0 && !p.flying {
            self.fall_distance -= moved[1] as f32;
        }

        // Fire and lava (Entity.onEntityUpdate).
        if p.in_water {
            self.fire = 0;
        }
        if in_lava {
            self.damage(Damage::Lava, 4.0, mode);
            self.fire = self.fire.max(300);
            self.fall_distance *= 0.5;
        }
        if self.fire > 0 {
            if self.fire % 20 == 0 {
                self.damage(Damage::Fire, 1.0, mode);
            }
            self.fire -= 1;
        }

        // Air (EntityLivingBase.onEntityUpdate).
        if eye == Material::Water && mode == GameMode::Survival {
            self.air -= 1;
            if self.air == -20 {
                self.air = 0;
                self.damage(Damage::Drown, 2.0, mode);
            }
        } else {
            self.air = 300;
        }

        // The void.
        if p.bb.min[1] < -64.0 {
            self.damage(Damage::Void, 4.0, mode);
        }

        if mode == GameMode::Creative {
            return;
        }

        // Exhaustion from movement (EntityPlayer.addMovementStat) and jumping.
        let horiz = ((moved[0] * moved[0] + moved[2] * moved[2]).sqrt() * 100.0).round() as i32;
        if p.in_water {
            let total = ((moved[0] * moved[0] + moved[1] * moved[1] + moved[2] * moved[2]).sqrt()
                * 100.0)
                .round() as i32;
            if total > 0 {
                self.add_exhaustion(0.015f32 * total as f32 * 0.01f32);
            }
        } else if p.on_ground && horiz > 0 {
            if p.sprinting {
                self.add_exhaustion(0.099999994f32 * horiz as f32 * 0.01f32);
            } else {
                self.add_exhaustion(0.01f32 * horiz as f32 * 0.01f32);
            }
        }
        if jumped {
            self.add_exhaustion(if p.sprinting { 0.8 } else { 0.2 });
        }

        // FoodStats.onUpdate (Normal difficulty, natural regeneration on).
        self.prev_food = self.food;
        if self.exhaustion > 4.0 {
            self.exhaustion -= 4.0;
            if self.saturation > 0.0 {
                self.saturation = (self.saturation - 1.0).max(0.0);
            } else {
                self.food = (self.food - 1).max(0);
            }
        }
        if self.food >= 18 && self.health > 0.0 && self.health < MAX_HEALTH {
            self.food_timer += 1;
            if self.food_timer >= 80 {
                self.health = (self.health + 1.0).min(MAX_HEALTH);
                self.add_exhaustion(3.0);
                self.food_timer = 0;
            }
        } else if self.food <= 0 {
            self.food_timer += 1;
            if self.food_timer >= 80 {
                if self.health > 1.0 {
                    self.damage(Damage::Starve, 1.0, mode);
                }
                self.food_timer = 0;
            }
        } else {
            self.food_timer = 0;
        }
    }

    /// Text of the death screen subtitle (`DamageSource.func_151519_b`).
    pub fn death_message(&self) -> &'static str {
        match self.last_cause {
            Some(Damage::Fall) => "Player hit the ground too hard",
            Some(Damage::Drown) => "Player drowned",
            Some(Damage::Lava) => "Player tried to swim in lava",
            Some(Damage::Fire) => "Player burned to death",
            Some(Damage::Starve) => "Player starved to death",
            Some(Damage::Void) => "Player fell out of the world",
            Some(Damage::Mob(k)) => match k {
                crate::entity::mob::Kind::Zombie => "Player was slain by Zombie",
                crate::entity::mob::Kind::Spider => "Player was slain by Spider",
                crate::entity::mob::Kind::CaveSpider => "Player was slain by Cave Spider",
                crate::entity::mob::Kind::Silverfish => "Player was slain by Silverfish",
                crate::entity::mob::Kind::Witch => "Player was killed by Witch using magic",
                _ => "Player was slain by a mob",
            },
            Some(Damage::Arrow) => "Player was shot by Skeleton",
            Some(Damage::Explosion) => "Player was blown up by Creeper",
            Some(Damage::Poison) => "Player died",
            None => "Player died",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player() -> Player {
        let mut p = Player::new(0.5, 64.0, 0.5);
        p.on_ground = false;
        p
    }

    #[test]
    fn fall_damage_is_distance_minus_three() {
        let mut s = Stats::default();
        let mut p = player();
        for _ in 0..10 {
            s.tick(
                &p,
                [0.0, -1.0, 0.0],
                false,
                Material::Air,
                false,
                GameMode::Survival,
            );
        }
        p.on_ground = true;
        s.tick(
            &p,
            [0.0, 0.0, 0.0],
            false,
            Material::Air,
            false,
            GameMode::Survival,
        );
        assert_eq!(s.health, 13.0);
        assert_eq!(s.fall_distance, 0.0);
    }

    #[test]
    fn creative_takes_no_fall_damage() {
        let mut s = Stats::default();
        let mut p = player();
        for _ in 0..30 {
            s.tick(
                &p,
                [0.0, -1.0, 0.0],
                false,
                Material::Air,
                false,
                GameMode::Creative,
            );
        }
        p.on_ground = true;
        s.tick(
            &p,
            [0.0, 0.0, 0.0],
            false,
            Material::Air,
            false,
            GameMode::Creative,
        );
        assert_eq!(s.health, 20.0);
    }

    #[test]
    fn drowning_starts_after_fifteen_seconds() {
        let mut s = Stats::default();
        let mut p = player();
        p.in_water = true;
        for _ in 0..319 {
            s.tick(
                &p,
                [0.0; 3],
                false,
                Material::Water,
                false,
                GameMode::Survival,
            );
        }
        assert_eq!(s.health, 20.0);
        s.tick(
            &p,
            [0.0; 3],
            false,
            Material::Water,
            false,
            GameMode::Survival,
        );
        assert_eq!(s.health, 18.0);
    }

    #[test]
    fn hurt_resistance_blocks_repeated_damage() {
        let mut s = Stats::default();
        assert!(s.damage(Damage::Lava, 4.0, GameMode::Survival));
        assert!(!s.damage(Damage::Lava, 4.0, GameMode::Survival));
        assert_eq!(s.health, 16.0);
    }

    #[test]
    fn regenerates_when_fed() {
        let mut s = Stats::default();
        s.health = 10.0;
        let p = Player::new(0.5, 64.0, 0.5);
        for _ in 0..80 {
            s.tick(
                &p,
                [0.0; 3],
                false,
                Material::Air,
                false,
                GameMode::Survival,
            );
        }
        assert_eq!(s.health, 11.0);
    }
}
