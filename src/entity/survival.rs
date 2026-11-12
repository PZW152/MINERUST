//! The client's copy of the player's health, hunger, air, fire and
//! experience (`EntityPlayerSP` as `S06PacketUpdateHealth` and friends
//! leave it). The server's `server_player` is authoritative.

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
    pub prev_food: i32,
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
    /// From the server (`EntityPlayerMP`): absorption hearts, the maximum
    /// health, the active effects and the death message.
    pub absorption: f32,
    pub max_health: f32,
    pub effects: Vec<crate::potion::Effect>,
    pub death_text: String,
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
            absorption: 0.0,
            max_health: MAX_HEALTH,
            effects: Vec::new(),
            death_text: String::new(),
        }
    }
}

impl Stats {
    /// `S06PacketUpdateHealth`, `S1FPacketSetExperience`, the effect packets.
    pub fn apply_view(&mut self, v: &crate::entity::server_player::PlayerStatsView) {
        // EntityPlayerSP.setPlayerSPHealth
        let d = self.health - v.health;
        if d < 0.0 {
            self.hurt_resistance = 10;
        } else if d > 0.0 {
            self.hurt_resistance = 20;
        }
        self.prev_food = self.food;
        self.health = v.health;
        self.max_health = v.max_health;
        self.absorption = v.absorption;
        self.food = v.food;
        self.saturation = v.saturation;
        self.air = v.air;
        self.fire = v.fire;
        self.xp_level = v.xp_level;
        self.xp_total = v.xp_total;
        self.xp = v.xp;
        self.effects = v.effects.clone();
    }

    /// The client's own part of a tick: the fall distance it uses for
    /// critical hits, the hurt and death animations.
    pub fn client_tick(&mut self, p: &Player, moved: [f64; 3]) {
        if self.dead {
            self.death_time += 1;
            return;
        }
        if self.hurt_time > 0 {
            self.hurt_time -= 1;
        }
        if self.hurt_resistance > 0 {
            self.hurt_resistance -= 1;
        }
        if p.in_water {
            self.fall_distance = 0.0;
        }
        if p.on_ground {
            self.fall_distance = 0.0;
        } else if moved[1] < 0.0 && !p.flying {
            self.fall_distance -= moved[1] as f32;
        }
    }

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
