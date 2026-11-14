//! Living entities other than the player: `Entity` / `EntityLivingBase`
//! movement, the AI of the 1.7.10 animals, monsters and villagers, combat,
//! drops, natural spawning and despawning, arrows and creeper explosions.

use super::path::{self, Opts, Pt, Size};
use super::player::{colliding_boxes, Aabb};
use crate::block::{self, Material};
use crate::item::Stack;
use crate::java::JavaRandom;
use crate::util::math;
use crate::world::server::ServerWorld;

pub mod ai;
mod end;
mod extra;
mod nether;
pub use extra::{village_golems, Other, Puff, Strike};
pub use nether::Fireball;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Kind {
    Pig,
    Cow,
    Sheep,
    Chicken,
    Mooshroom,
    Zombie,
    Skeleton,
    Creeper,
    Spider,
    CaveSpider,
    Villager,
    Witch,
    Silverfish,
    Slime,
    Enderman,
    Squid,
    Bat,
    Wolf,
    Ocelot,
    Horse,
    IronGolem,
    Ghast,
    PigZombie,
    Blaze,
    MagmaCube,
    EnderDragon,
    EnderCrystal,
}

pub const ALL_KINDS: [Kind; 27] = [
    Kind::Pig,
    Kind::Cow,
    Kind::Sheep,
    Kind::Chicken,
    Kind::Mooshroom,
    Kind::Zombie,
    Kind::Skeleton,
    Kind::Creeper,
    Kind::Spider,
    Kind::CaveSpider,
    Kind::Villager,
    Kind::Witch,
    Kind::Silverfish,
    Kind::Slime,
    Kind::Enderman,
    Kind::Squid,
    Kind::Bat,
    Kind::Wolf,
    Kind::Ocelot,
    Kind::Horse,
    Kind::IronGolem,
    Kind::Ghast,
    Kind::PigZombie,
    Kind::Blaze,
    Kind::MagmaCube,
    Kind::EnderDragon,
    Kind::EnderCrystal,
];

impl Kind {
    /// `setSize(width, height)`
    pub fn size(self) -> (f32, f32) {
        match self {
            Kind::Pig => (0.9, 0.9),
            Kind::Cow | Kind::Sheep | Kind::Mooshroom => (0.9, 1.3),
            Kind::Chicken => (0.3, 0.7),
            Kind::Spider => (1.4, 0.9),
            Kind::CaveSpider => (0.7, 0.5),
            Kind::Silverfish => (0.3, 0.7),
            Kind::Slime => (0.6, 0.6),
            Kind::Enderman => (0.6, 2.9),
            Kind::Squid => (0.95, 0.95),
            Kind::Bat => (0.5, 0.9),
            Kind::Wolf | Kind::Ocelot => (0.6, 0.8),
            Kind::Horse => (1.4, 1.6),
            Kind::IronGolem => (1.4, 2.9),
            Kind::Ghast => (4.0, 4.0),
            Kind::MagmaCube => (0.6, 0.6),
            Kind::EnderDragon => (16.0, 8.0),
            Kind::EnderCrystal => (2.0, 2.0),
            _ => (0.6, 1.8),
        }
    }
    /// `SharedMonsterAttributes.maxHealth`
    pub fn max_health(self) -> f32 {
        match self {
            Kind::Pig | Kind::Cow | Kind::Mooshroom => 10.0,
            Kind::Sheep | Kind::Silverfish => 8.0,
            Kind::Chicken => 4.0,
            Kind::Spider => 16.0,
            Kind::CaveSpider => 12.0,
            Kind::Witch => 26.0,
            Kind::Slime => 1.0,
            Kind::Enderman => 40.0,
            Kind::Squid | Kind::Ocelot => 10.0,
            Kind::Bat => 6.0,
            Kind::Wolf => 8.0,
            Kind::Horse => 15.0,
            Kind::IronGolem => 100.0,
            Kind::Ghast => 10.0,
            Kind::MagmaCube => 1.0,
            Kind::EnderDragon => 200.0,
            Kind::EnderCrystal => 5.0,
            _ => 20.0,
        }
    }
    /// `SharedMonsterAttributes.movementSpeed`
    pub fn speed(self) -> f32 {
        match self {
            Kind::Pig | Kind::Chicken | Kind::Skeleton | Kind::Creeper | Kind::Witch => 0.25,
            Kind::Cow | Kind::Mooshroom => 0.2,
            Kind::Sheep | Kind::Zombie => 0.23,
            Kind::Spider | Kind::CaveSpider => 0.8,
            Kind::Villager => 0.5,
            Kind::Silverfish => 0.6,
            Kind::Slime => 0.7,
            Kind::Enderman | Kind::Wolf | Kind::Ocelot => 0.3,
            Kind::Squid | Kind::Bat => 0.7,
            Kind::Horse => 0.225,
            Kind::IronGolem => 0.25,
            Kind::PigZombie => 0.5,
            Kind::Blaze | Kind::Ghast | Kind::MagmaCube => 0.7,
            Kind::EnderDragon | Kind::EnderCrystal => 0.7,
        }
    }
    pub fn hostile(self) -> bool {
        matches!(
            self,
            Kind::Zombie
                | Kind::Skeleton
                | Kind::Creeper
                | Kind::Spider
                | Kind::CaveSpider
                | Kind::Witch
                | Kind::Silverfish
                | Kind::Slime
                | Kind::Enderman
                | Kind::Ghast
                | Kind::PigZombie
                | Kind::Blaze
                | Kind::MagmaCube
                | Kind::EnderDragon
        )
    }
    /// `getCreatureAttribute`
    pub fn creature(self) -> crate::enchant::Creature {
        use crate::enchant::Creature;
        match self {
            Kind::Zombie | Kind::PigZombie | Kind::Skeleton => Creature::Undead,
            Kind::Spider | Kind::CaveSpider | Kind::Silverfish => Creature::Arthropod,
            _ => Creature::Undefined,
        }
    }
    /// `getTotalArmorValue` without equipment: `EntityZombie` (and so the
    /// pigman) adds 2.
    pub fn natural_armor(self) -> i32 {
        match self {
            Kind::Zombie | Kind::PigZombie => 2,
            _ => 0,
        }
    }
    /// `isImmuneToFire`
    pub fn fireproof(self) -> bool {
        matches!(
            self,
            Kind::Ghast
                | Kind::PigZombie
                | Kind::Blaze
                | Kind::MagmaCube
                | Kind::EnderDragon
                | Kind::EnderCrystal
        )
    }
    /// `EnumCreatureType.creature` (counts towards the animal cap).
    pub fn animal(self) -> bool {
        matches!(
            self,
            Kind::Pig
                | Kind::Cow
                | Kind::Sheep
                | Kind::Chicken
                | Kind::Mooshroom
                | Kind::Wolf
                | Kind::Ocelot
                | Kind::Horse
        )
    }
    /// Uses the old `updateEntityActionState` AI instead of AI tasks.
    fn old_ai(self) -> bool {
        matches!(
            self,
            Kind::Spider
                | Kind::CaveSpider
                | Kind::Silverfish
                | Kind::Slime
                | Kind::Enderman
                | Kind::PigZombie
                | Kind::Blaze
                | Kind::MagmaCube
        )
    }
    pub fn name(self) -> &'static str {
        match self {
            Kind::Pig => "Pig",
            Kind::Cow => "Cow",
            Kind::Sheep => "Sheep",
            Kind::Chicken => "Chicken",
            Kind::Mooshroom => "Mooshroom",
            Kind::Zombie => "Zombie",
            Kind::Skeleton => "Skeleton",
            Kind::Creeper => "Creeper",
            Kind::Spider => "Spider",
            Kind::CaveSpider => "Cave Spider",
            Kind::Villager => "Villager",
            Kind::Witch => "Witch",
            Kind::Silverfish => "Silverfish",
            Kind::Slime => "Slime",
            Kind::Enderman => "Enderman",
            Kind::Squid => "Squid",
            Kind::Bat => "Bat",
            Kind::Wolf => "Wolf",
            Kind::Ocelot => "Ocelot",
            Kind::Horse => "Horse",
            Kind::IronGolem => "Iron Golem",
            Kind::Ghast => "Ghast",
            Kind::PigZombie => "Zombie Pigman",
            Kind::Blaze => "Blaze",
            Kind::MagmaCube => "Magma Cube",
            Kind::EnderDragon => "Ender Dragon",
            Kind::EnderCrystal => "Ender Crystal",
        }
    }
    /// `EntityList` string id.
    pub fn id(self) -> &'static str {
        match self {
            Kind::Pig => "Pig",
            Kind::Cow => "Cow",
            Kind::Sheep => "Sheep",
            Kind::Chicken => "Chicken",
            Kind::Mooshroom => "MushroomCow",
            Kind::Zombie => "Zombie",
            Kind::Skeleton => "Skeleton",
            Kind::Creeper => "Creeper",
            Kind::Spider => "Spider",
            Kind::CaveSpider => "CaveSpider",
            Kind::Villager => "Villager",
            Kind::Witch => "Witch",
            Kind::Silverfish => "Silverfish",
            Kind::Slime => "Slime",
            Kind::Enderman => "Enderman",
            Kind::Squid => "Squid",
            Kind::Bat => "Bat",
            Kind::Wolf => "Wolf",
            Kind::Ocelot => "Ozelot",
            Kind::Horse => "EntityHorse",
            Kind::IronGolem => "VillagerGolem",
            Kind::Ghast => "Ghast",
            Kind::PigZombie => "PigZombie",
            Kind::Blaze => "Blaze",
            Kind::MagmaCube => "LavaSlime",
            Kind::EnderDragon => "EnderDragon",
            Kind::EnderCrystal => "EnderCrystal",
        }
    }
    pub fn from_id(s: &str) -> Option<Kind> {
        ALL_KINDS.iter().copied().find(|k| k.id() == s)
    }
    /// `getTalkInterval`-style idle sound name (played by the client).
    pub fn eye_height(self) -> f32 {
        self.size().1 * 0.85
    }
    fn attack_damage(self) -> f32 {
        match self {
            Kind::Zombie => 3.0,
            Kind::Spider | Kind::CaveSpider => 2.0,
            Kind::Silverfish => 1.0,
            Kind::Enderman | Kind::IronGolem => 7.0,
            Kind::PigZombie => 5.0,
            Kind::Blaze => 6.0,
            _ => 2.0,
        }
    }
    fn follow_range(self) -> f32 {
        match self {
            Kind::Zombie => 40.0,
            Kind::Enderman => 64.0,
            Kind::Horse => 16.0,
            _ => 16.0,
        }
    }
}

/// What the AI is doing.
#[derive(Clone, Debug, PartialEq)]
enum Task {
    Idle,
    Wander,
    Panic,
    Tempt,
    Attack,
    Flee,
    EatGrass(i32),
}

#[derive(Clone, Debug)]
pub struct Mob {
    pub id: u32,
    pub kind: Kind,
    /// `EntityAgeable.getGrowingAge`: below 0 a child
    pub growing_age: i32,
    /// `posX`, `posY` (feet), `posZ`
    pub pos: [f64; 3],
    pub prev_pos: [f64; 3],
    pub motion: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub prev_pitch: f32,
    /// `renderYawOffset`
    pub body_yaw: f32,
    pub prev_body_yaw: f32,
    /// `rotationYawHead`
    pub head_yaw: f32,
    pub prev_head_yaw: f32,
    pub bb: Aabb,
    pub on_ground: bool,
    collided_h: bool,
    in_water: bool,
    fall_distance: f32,
    pub health: f32,
    pub hurt_time: i32,
    pub death_time: i32,
    hurt_resistant: i32,
    last_damage: f32,
    pub fire: i32,
    pub ticks: i32,
    pub limb_swing: f32,
    pub limb_amount: f32,
    pub prev_limb_amount: f32,
    /// `swingProgress` of the attacking arm.
    pub swing: f32,
    pub prev_swing: f32,
    swing_ticks: i32,
    is_swinging: bool,
    age: i32,
    jump_ticks: i32,
    jumping: bool,
    move_forward: f32,
    move_strafe: f32,
    ai_speed: f32,
    rand: JavaRandom,
    task: Task,
    task_ticks: i32,
    path: Vec<Pt>,
    path_index: usize,
    path_speed: f64,
    move_to: Option<[f64; 3]>,
    move_speed: f64,
    look_at: Option<[f64; 3]>,
    body_counter: i32,
    prev_head_for_body: f32,
    attack_cooldown: i32,
    pub target_player: bool,
    recently_hit: i32,
    /// `EnchantmentHelper.getLootingModifier` of the player whose damage
    /// was the last to land (0 for any other source), for `onDeath`.
    pub looting: i32,
    /// `EntityLiving.equipment` (held item, boots, leggings, chestplate,
    /// helmet) and `equipmentDropChances`.
    pub equipment: [Option<Stack>; 5],
    pub drop_chances: [f32; 5],
    /// `canPickUpLoot`
    pub can_pick_up_loot: bool,
    /// The zombie's "Random spawn bonus" to knockback resistance.
    pub kb_resist: f64,
    pub persistent: bool,
    // per-kind state
    /// Sheep fleece colour / villager profession / skeleton type.
    pub variant: i32,
    pub sheared: bool,
    /// Creeper `timeSinceIgnited` and `lastActiveTime`.
    pub fuse: i32,
    pub prev_fuse: i32,
    fuse_dir: i32,
    /// Creeper `hasIgnited` (flint and steel; DataWatcher 18).
    pub ignited: bool,
    /// Chicken wing flap (`field_70886_e` etc.) and egg timer.
    pub wing: f32,
    pub prev_wing: f32,
    wing_dest: f32,
    wing_speed: f32,
    egg_timer: i32,
    /// Sheep `sheepTimer` (head down while eating).
    pub eat_timer: i32,
    pub dead: bool,
    /// Slime `squishFactor`, `squishAmount` and `slimeJumpDelay`.
    pub squish: f32,
    pub prev_squish: f32,
    squish_amount: f32,
    jump_delay: i32,
    /// Enderman carried block and data, `isScreaming`, timers.
    pub carried: u8,
    pub carried_meta: u8,
    pub angry: bool,
    aggressive: bool,
    stare: i32,
    teleport_delay: i32,
    /// Squid pitch, yaw, rotation and tentacle angle, with last tick values.
    pub sq: [f32; 4],
    pub prev_sq: [f32; 4],
    sq_vel: f32,
    sq_speed: f32,
    sq_turn: f32,
    sq_vec: [f32; 3],
    /// Bat hanging from a ceiling, and where it flies to.
    pub hanging: bool,
    bat_target: Option<[i32; 3]>,
    /// Tamed by the player, and sitting (wolves and ocelots).
    pub tamed: bool,
    /// `ridingEntity`: the minecart it sits in.
    pub riding: Option<u32>,
    pub sitting: bool,
    /// Iron golem arm swing timer; built by the player.
    pub attack_timer: i32,
    pub player_made: bool,
    /// Another mob this one is after (wolves, golems).
    target_mob: Option<u32>,
    /// `EntityLiving.tasks` and `targetTasks`, for the mobs of `ai::uses_tasks`.
    tasks: ai::Tasks,
    target_tasks: ai::Tasks,
    /// `getAttackTarget()`
    pub attack_target: Option<ai::Target>,
    /// `getAITarget()` (the last attacker) and `getRevengeTimer()`.
    revenge: Option<ai::Target>,
    revenge_timer: i32,
    /// `EntityLookHelper.deltaLookYaw` / `deltaLookPitch` of `look_at`.
    look_delta: (f32, f32),
    /// `PathNavigate`: `totalTicks`, `ticksAtLastPos`, `lastPosCheck`,
    /// `noSunPathfind`, `avoidsWater`.
    nav_ticks: i32,
    nav_last_tick: i32,
    nav_last_pos: [f64; 3],
    avoid_sun: bool,
    avoids_water: bool,
    /// The old AI: `isJumping` as it stays between ticks, and
    /// `EntityLiving`'s `currentTarget`, `numTicksToChaseTarget` and
    /// `randomYawVelocity`.
    old_jump: bool,
    /// `EntityAISit.isSitting`: told to sit (`sitting` is whether it does).
    pub sit_order: bool,
    /// `EntityAnimal.inLove`: ticks of love left after being fed.
    pub in_love: i32,
    idle_target: bool,
    idle_ticks: i32,
    random_yaw: f32,
    /// Ghast `waypointX/Y/Z`.
    waypoint: [f64; 3],
    /// Zombie pigman `angerLevel` and `randomSoundDelay`.
    pub anger: i32,
    anger_sound: i32,
    /// `EntityDragon.ringBuffer` (yaw, y) and `ringBufferIndex`.
    pub ring: [[f64; 2]; 64],
    pub ring_index: i32,
    /// `EntityTrackerEntry` and what it told the client.
    pub net: Net,
}

/// A mob's `EntityTrackerEntry` and the client's view of it: the position
/// and angles of the last move packet (`setPositionAndRotation2`'s target)
/// and the `S19PacketEntityHeadLook` head yaw.
#[derive(Clone, Debug, Default)]
pub struct Net {
    tracker: Option<super::tracker::Tracker>,
    sp: super::tracker::ServerPos,
    pub at: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    /// Bumped by every move, look or teleport packet.
    pub seq: u32,
    head_byte: i32,
    pub head: f32,
}

impl super::tracker::Track for Mob {
    fn pos(&self) -> [f64; 3] {
        self.pos
    }
    fn yaw(&self) -> f32 {
        self.yaw
    }
    fn pitch(&self) -> f32 {
        self.pitch
    }
    fn motion(&self) -> [f64; 3] {
        self.motion
    }
    fn width(&self) -> f32 {
        self.dims().0
    }
    fn riding(&self) -> bool {
        self.riding.is_some()
    }
}

/// What the player looks like to the mobs.
#[derive(Clone, Copy)]
pub struct PlayerInfo {
    /// The server-side motion, health and active effects (a bit per
    /// potion id), for the witch's choice.
    pub motion: [f64; 3],
    pub health: f32,
    pub effects: u32,
    /// `EntityPlayer.attackEntityFrom` would take damage now: hurt
    /// resistance ticks, the last damage taken and the difficulty (0..3).
    pub hurt_resistant_time: i32,
    pub last_damage: f32,
    pub difficulty: i32,
    pub feet: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    /// The mob the player hit last, and the last one that hit the player
    /// (tamed wolves join in).
    pub last_attacked: Option<u32>,
    pub last_attacker: Option<u32>,
    pub attackable: bool,
    /// `isEntityAlive()`
    pub alive: bool,
    pub sneaking: bool,
    /// `getRevengeTimer()` and `getLastAttackerTime()` of the player.
    pub revenge_timer: i32,
    pub last_attack_time: i32,
    /// Worn armour pieces (`getArmorVisibility` is a quarter of it).
    pub armor_pieces: i32,
    /// Item ids the player holds (for tempting).
    pub held: Option<u16>,
    /// `getLootingModifier` of the held stack, for kills by the player's
    /// arrows (`onDeath` asks the damage source's entity).
    pub looting: i32,
}

impl PlayerInfo {
    /// Whether `attackEntityFrom(arrow from a mob, amount)` succeeds:
    /// difficulty scaling, then the hurt-resistance window.
    pub fn accepts(&self, amount: f32, scaled: bool) -> bool {
        if !self.attackable {
            return false;
        }
        let mut a = amount;
        if scaled {
            match self.difficulty {
                0 => a = 0.0,
                1 => a = a / 2.0 + 1.0,
                3 => a = a * 3.0 / 2.0,
                _ => {}
            }
        }
        if a == 0.0 {
            return false;
        }
        if self.hurt_resistant_time as f32 > 10.0 {
            a > self.last_damage
        } else {
            true
        }
    }

    fn eye(&self) -> [f64; 3] {
        [self.feet[0], self.feet[1] + 1.62, self.feet[2]]
    }
    fn bb(&self) -> Aabb {
        Aabb {
            min: [self.feet[0] - 0.3, self.feet[1], self.feet[2] - 0.3],
            max: [self.feet[0] + 0.3, self.feet[1] + 1.8, self.feet[2] + 0.3],
        }
    }
}

/// Things that happened to the player this tick.
#[derive(Clone, Debug)]
pub enum Hit {
    /// `attackEntityFrom(causeMobDamage(attacker), amount)`: the knockback
    /// comes from the attacker's position `from`; `knock` is extra
    /// velocity added afterwards (`addVelocity`), `effect` a potion
    /// effect given on a hit (duration -1: the cave spider's, by difficulty).
    Melee {
        kind: Kind,
        /// the attacker's id (Thorns strikes back at it)
        by: u32,
        /// Fire Aspect of the held weapon: `setFire(level * 4)` on a hit
        fire: i32,
        amount: f32,
        knock: [f64; 3],
        from: [f64; 3],
        effect: Option<(i32, i32)>,
    },
    /// `causeArrowDamage(arrow, shooter)`
    Arrow {
        amount: f32,
        knock: [f64; 3],
        shooter: Option<u32>,
    },
    /// `causeFireballDamage(fireball, shooter)`
    Fireball {
        amount: f32,
        shooter: u32,
        small: bool,
    },
    /// `Explosion.doExplosionA`: damage and the push (`knock`).
    Explosion {
        amount: f32,
        knock: [f64; 3],
        from: Option<[f64; 3]>,
    },
    /// `EntityEnderPearl`: the player lands here.
    Teleport([f64; 3]),
    /// A splash potion hit the player (`EntityPotion.onImpact`).
    Potion {
        effects: Vec<crate::potion::Effect>,
        scale: f64,
        from: [f64; 3],
    },
    Xp(i32),
    /// Set on fire for this many ticks (small fireballs).
    Burn(i32),
}

pub use super::arrow::EntityArrow as Arrow;

/// `wrapAngleTo180_float`
fn wrap(a: f32) -> f32 {
    let mut a = a % 360.0;
    if a >= 180.0 {
        a -= 360.0;
    }
    if a < -180.0 {
        a += 360.0;
    }
    a
}

fn approach(from: f32, to: f32, max: f32) -> f32 {
    let d = wrap(to - from).clamp(-max, max);
    from + d
}

impl Mob {
    pub fn new(id: u32, kind: Kind, pos: [f64; 3], yaw: f32, seed: i64) -> Mob {
        let (w, h) = kind.size();
        let hw = w as f64 / 2.0;
        let mut m = Mob {
            net: Net::default(),
            id,
            kind,
            pos,
            prev_pos: pos,
            motion: [0.0; 3],
            yaw,
            pitch: 0.0,
            prev_pitch: 0.0,
            body_yaw: yaw,
            prev_body_yaw: yaw,
            head_yaw: yaw,
            prev_head_yaw: yaw,
            bb: Aabb {
                min: [pos[0] - hw, pos[1], pos[2] - hw],
                max: [pos[0] + hw, pos[1] + h as f64, pos[2] + hw],
            },
            on_ground: false,
            collided_h: false,
            in_water: false,
            fall_distance: 0.0,
            health: kind.max_health(),
            hurt_time: 0,
            death_time: 0,
            hurt_resistant: 0,
            last_damage: 0.0,
            fire: 0,
            ticks: 0,
            limb_swing: 0.0,
            limb_amount: 0.0,
            prev_limb_amount: 0.0,
            swing: 0.0,
            prev_swing: 0.0,
            swing_ticks: 0,
            is_swinging: false,
            age: 0,
            jump_ticks: 0,
            jumping: false,
            move_forward: 0.0,
            move_strafe: 0.0,
            ai_speed: 0.0,
            rand: JavaRandom::new(seed),
            growing_age: 0,
            task: Task::Idle,
            task_ticks: 0,
            path: Vec::new(),
            path_index: 0,
            path_speed: 0.0,
            move_to: None,
            move_speed: 0.0,
            look_at: None,
            body_counter: 0,
            prev_head_for_body: yaw,
            attack_cooldown: 0,
            target_player: false,
            recently_hit: 0,
            looting: 0,
            equipment: [None; 5],
            drop_chances: [0.085; 5],
            can_pick_up_loot: false,
            kb_resist: 0.0,
            persistent: false,
            variant: 0,
            sheared: false,
            fuse: 0,
            prev_fuse: 0,
            fuse_dir: -1,
            ignited: false,
            tasks: ai::Tasks::default(),
            target_tasks: ai::Tasks::default(),
            attack_target: None,
            revenge: None,
            revenge_timer: 0,
            look_delta: (10.0, 40.0),
            nav_ticks: 0,
            nav_last_tick: 0,
            nav_last_pos: [0.0; 3],
            old_jump: false,
            sit_order: false,
            in_love: 0,
            idle_target: false,
            idle_ticks: 0,
            random_yaw: 0.0,
            avoid_sun: false,
            // getNavigator().setAvoidsWater(true) in the constructor
            avoids_water: matches!(
                kind,
                Kind::Pig
                    | Kind::Cow
                    | Kind::Mooshroom
                    | Kind::Sheep
                    | Kind::Villager
                    | Kind::Wolf
                    | Kind::Ocelot
                    | Kind::IronGolem
            ),
            wing: 0.0,
            prev_wing: 0.0,
            wing_dest: 0.0,
            wing_speed: 1.0,
            egg_timer: 0,
            eat_timer: 0,
            dead: false,
            squish: 0.0,
            prev_squish: 0.0,
            squish_amount: 0.0,
            jump_delay: 0,
            carried: 0,
            carried_meta: 0,
            angry: false,
            aggressive: false,
            stare: 0,
            teleport_delay: 0,
            sq: [0.0; 4],
            prev_sq: [0.0; 4],
            sq_vel: 0.0,
            sq_speed: 0.0,
            sq_turn: 0.0,
            sq_vec: [0.0; 3],
            hanging: kind == Kind::Bat,
            bat_target: None,
            tamed: false,
            riding: None,
            sitting: false,
            attack_timer: 0,
            player_made: false,
            target_mob: None,
            waypoint: pos,
            anger: 0,
            anger_sound: 0,
            ring: [[0.0; 2]; 64],
            ring_index: -1,
        };
        if kind == Kind::EnderDragon {
            m.waypoint = [pos[0], 100.0, pos[2]];
        }
        m.egg_timer = m.rand.next_int_n(6000) + 6000;
        match kind {
            Kind::Slime | Kind::MagmaCube => {
                // EntitySlime(World): size 1 << nextInt(3)
                let size = 1 << m.rand.next_int_n(3);
                m.jump_delay = m.rand.next_int_n(20) + 10;
                m.set_slime_size(size);
            }
            Kind::Squid => m.sq_vel = 1.0 / (m.rand.next_float() + 1.0) * 0.2,
            _ => {}
        }
        m
    }

    /// `EntitySlime.setSlimeSize`
    /// `setAttackTarget(null)`
    pub fn clear_ai_target(&mut self) {
        self.attack_target = None;
    }

    pub fn set_slime_size(&mut self, size: i32) {
        self.variant = size;
        self.health = (size * size) as f32;
        let p = self.pos;
        self.set_position(p[0], p[1], p[2]);
    }

    /// Width and height of the box (slimes scale with their size).
    pub fn dims(&self) -> (f32, f32) {
        match self.kind {
            Kind::Slime | Kind::MagmaCube => {
                let s = 0.6 * self.variant.max(1) as f32;
                (s, s)
            }
            // EntitySkeleton.setSkeletonType(1)
            Kind::Skeleton if self.variant == 1 => (0.72, 2.34),
            // EntityAgeable.setScaleForAge: the young are half the size
            k if self.growing_age < 0 && k.animal() => {
                let (w, h) = k.size();
                (w * 0.5, h * 0.5)
            }
            k => k.size(),
        }
    }

    pub fn max_health(&self) -> f32 {
        match self.kind {
            Kind::Slime | Kind::MagmaCube => (self.variant * self.variant) as f32,
            Kind::Wolf if self.tamed => 20.0,
            _ => self.kind.max_health(),
        }
    }

    fn width(&self) -> f32 {
        self.dims().0
    }
    fn height(&self) -> f32 {
        self.dims().1
    }

    pub fn set_position(&mut self, x: f64, y: f64, z: f64) {
        let (w, h) = self.dims();
        let hw = w as f64 / 2.0;
        self.pos = [x, y, z];
        self.bb = Aabb {
            min: [x - hw, y, z - hw],
            max: [x + hw, y + h as f64, z + hw],
        };
    }

    /// `onSpawnWithEgg`: `EntityLiving`'s follow-range bonus for all but
    /// the cave spider, then each class's own part.
    pub fn on_spawn(&mut self, world_rand: &mut JavaRandom, cx: &SpawnCtx) {
        if self.kind != Kind::CaveSpider {
            // followRange "Random spawn bonus" (no follow-range attribute
            // is read here yet; the draw keeps the mob's random in step)
            let _ = self.rand.next_gaussian() * 0.05;
        }
        match self.kind {
            Kind::Skeleton => {
                // EntitySkeleton.onSpawnWithEgg
                if cx.nether && self.rand.next_int_n(5) > 0 {
                    self.variant = 1;
                    let p = self.pos;
                    self.set_position(p[0], p[1], p[2]);
                    self.equipment[0] = Some(Stack::new(272, 0, 1));
                } else {
                    self.add_random_armor(cx);
                    self.equipment[0] = Some(Stack::new(261, 0, 1));
                    self.enchant_equipment(cx);
                }
                self.can_pick_up_loot = self.rand.next_float() < 0.55 * cx.tension;
                self.halloween(cx);
            }
            Kind::Zombie | Kind::PigZombie => {
                // EntityZombie.onSpawnWithEgg
                let t = cx.tension;
                self.can_pick_up_loot = self.rand.next_float() < 0.55 * t;
                // EntityZombie$GroupData: child and villager from the world's
                // random (not carried through a pack yet)
                let child = world_rand.next_float() < 0.05;
                let villager = world_rand.next_float() < 0.05;
                let _ = villager;
                if child {
                    // the chicken-jockey rolls (an existing chicken nearby,
                    // else a new one); baby zombies, zombie villagers and
                    // jockeys are not made yet (HANDOFF section 7)
                    if world_rand.next_float() >= 0.05 {
                        let _ = world_rand.next_float() < 0.05;
                    }
                }
                // setBreakDoorsAItask
                let _ = self.rand.next_float() < t * 0.1;
                if self.kind == Kind::PigZombie {
                    // EntityPigZombie.addRandomArmor
                    self.equipment[0] = Some(Stack::new(283, 0, 1));
                } else {
                    self.add_random_armor(cx);
                    let p = if cx.hard { 0.05 } else { 0.01 };
                    if self.rand.next_float() < p {
                        let k = self.rand.next_int_n(3);
                        self.equipment[0] = Some(Stack::new(if k == 0 { 267 } else { 256 }, 0, 1));
                    }
                }
                self.enchant_equipment(cx);
                self.halloween(cx);
                self.kb_resist = self.rand.next_double() * 0.05f32 as f64;
                let _follow = self.rand.next_double() * 1.5 * t as f64;
                if self.rand.next_float() < t * 0.05 {
                    // leader zombie: reinforcement chance and max health
                    let _ = self.rand.next_double();
                    let k = self.rand.next_double() * 3.0 + 1.0;
                    self.health = (self.kind.max_health() as f64 * (1.0 + k)) as f32;
                }
            }
            Kind::Sheep => {
                // EntitySheep.getRandomFleeceColor
                let i = world_rand.next_int_n(100);
                self.variant = if i < 5 {
                    15
                } else if i < 10 {
                    7
                } else if i < 15 {
                    8
                } else if i < 18 {
                    12
                } else if world_rand.next_int_n(500) == 0 {
                    6
                } else {
                    0
                };
            }
            Kind::Villager => self.variant = world_rand.next_int_n(5),
            Kind::Horse => {
                // EntityHorse.onSpawnWithEgg: a donkey one time in ten,
                // otherwise a horse of a random colour and markings.
                let r = &mut self.rand;
                if r.next_int_n(10) == 0 {
                    self.variant = 1 << 16;
                    self.health = 15.0;
                } else {
                    let colour = r.next_int_n(7);
                    let marks = r.next_int_n(5);
                    self.variant = colour | marks << 8;
                    self.health = 15.0 + r.next_int_n(8) as f32 + r.next_int_n(9) as f32;
                }
            }
            _ => {}
        }
    }

    /// `EntityLiving.addRandomArmor`
    fn add_random_armor(&mut self, cx: &SpawnCtx) {
        if self.rand.next_float() < 0.15 * cx.tension {
            let mut tier = self.rand.next_int_n(2);
            let stop = if cx.hard { 0.1 } else { 0.25 };
            for _ in 0..3 {
                if self.rand.next_float() < 0.095 {
                    tier += 1;
                }
            }
            for k in (0..4).rev() {
                if k < 3 && self.rand.next_float() < stop {
                    break;
                }
                if self.equipment[k + 1].is_none() {
                    if let Some(id) = armor_item_for_slot(k + 1, tier) {
                        self.equipment[k + 1] = Some(Stack::new(id, 0, 1));
                    }
                }
            }
        }
    }

    /// `EntityLiving.enchantEquipment`
    fn enchant_equipment(&mut self, cx: &SpawnCtx) {
        let t = cx.tension;
        if let Some(s) = self.equipment[0] {
            if self.rand.next_float() < 0.25 * t {
                let lvl = (5.0 + t * self.rand.next_int_n(18) as f32) as i32;
                self.equipment[0] = Some(add_random_enchantment(&mut self.rand, s, lvl));
            }
        }
        for k in 0..4 {
            if let Some(s) = self.equipment[k + 1] {
                if self.rand.next_float() < 0.5 * t {
                    let lvl = (5.0 + t * self.rand.next_int_n(18) as f32) as i32;
                    self.equipment[k + 1] = Some(add_random_enchantment(&mut self.rand, s, lvl));
                }
            }
        }
    }

    /// The pumpkin of October 31st (`World.getCurrentDate`).
    fn halloween(&mut self, cx: &SpawnCtx) {
        if self.equipment[4].is_none() && cx.halloween && self.rand.next_float() < 0.25 {
            let lit = self.rand.next_float() < 0.1;
            self.equipment[4] = Some(Stack::new(if lit { 91 } else { 86 }, 0, 1));
            self.drop_chances[4] = 0.0;
        }
    }

    /// The end of `EntitySkeleton.attackEntityWithRangedAttack`: the held
    /// bow's Power, Punch and Flame, and fire from a wither skeleton.
    pub fn bow_enchantments(&self, a: &mut Arrow) {
        use crate::enchant;
        let held = self.equipment[0];
        let power = enchant::level(enchant::POWER, held.as_ref());
        let punch = enchant::level(enchant::PUNCH, held.as_ref());
        if power > 0 {
            a.damage += power as f64 * 0.5 + 0.5;
        }
        if punch > 0 {
            a.knockback = punch;
        }
        if enchant::level(enchant::FLAME, held.as_ref()) > 0 || self.variant == 1 {
            a.e.set_fire(100);
        }
    }

    /// `getTotalArmorValue`: the worn pieces plus the zombie's 2.
    pub fn total_armor(&self) -> i32 {
        let worn: i32 = self.equipment[1..]
            .iter()
            .flatten()
            .filter_map(|s| crate::inventory::armor_points(s.id))
            .sum();
        let v = worn + self.kind.natural_armor();
        if matches!(self.kind, Kind::Zombie | Kind::PigZombie) {
            v.min(20)
        } else {
            v
        }
    }

    /// `EntityLiving.dropEquipment(recentlyHit, looting)`
    fn drop_equipment(&mut self, hit: bool, looting: i32, v: &mut Vec<Stack>) {
        for j in 0..5 {
            let Some(mut s) = self.equipment[j] else {
                continue;
            };
            let always = self.drop_chances[j] > 1.0;
            if (hit || always)
                && self.rand.next_float() - looting as f32 * 0.01 < self.drop_chances[j]
            {
                let max = crate::item::max_damage(s.id);
                if !always && max > 0 {
                    let k = (max - 25).max(1);
                    let inner = self.rand.next_int_n(k) + 1;
                    let mut l = max - self.rand.next_int_n(inner);
                    if l > k {
                        l = k;
                    }
                    if l < 1 {
                        l = 1;
                    }
                    s.meta = l;
                }
                v.push(s);
            }
        }
    }

    // ------------------------------------------------------------ movement

    fn in_material(&self, w: &ServerWorld, bb: &Aabb, m: Material) -> bool {
        let x0 = math::floor_double(bb.min[0]);
        let x1 = math::floor_double(bb.max[0] + 1.0);
        let y0 = math::floor_double(bb.min[1]);
        let y1 = math::floor_double(bb.max[1] + 1.0);
        let z0 = math::floor_double(bb.min[2]);
        let z1 = math::floor_double(bb.max[2] + 1.0);
        for x in x0..x1 {
            for y in y0..y1 {
                for z in z0..z1 {
                    let id = w.peek_block(x, y, z);
                    if block::material(id) == m {
                        let mm = w.peek_meta(x, y, z) as i32;
                        let level = if mm >= 8 { 0 } else { mm };
                        let top = (y + 1) as f64 - (level + 1) as f64 / 9.0;
                        if y1 as f64 >= top || m == Material::Lava {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// `Entity.moveEntity` (no sneaking).
    /// `Entity.moveEntity` from outside the mob's own update (pistons).
    pub fn push_move(&mut self, w: &ServerWorld, dx: f64, dy: f64, dz: f64) {
        self.move_entity(w, dx, dy, dz, false);
    }

    fn move_entity(&mut self, w: &ServerWorld, mut dx: f64, mut dy: f64, mut dz: f64, web: bool) {
        if web {
            dx *= 0.25;
            dy *= 0.05f32 as f64;
            dz *= 0.25;
            self.motion = [0.0; 3];
        }
        let (ox, oy, oz) = (dx, dy, dz);
        let orig = self.bb;
        let list = colliding_boxes(w, &self.bb.add_coord(dx, dy, dz));
        for b in &list {
            dy = b.calc_offset(&self.bb, 1, dy);
        }
        self.bb.offset(0.0, dy, 0.0);
        let stepping = self.on_ground || (oy != dy && oy < 0.0);
        for b in &list {
            dx = b.calc_offset(&self.bb, 0, dx);
        }
        self.bb.offset(dx, 0.0, 0.0);
        for b in &list {
            dz = b.calc_offset(&self.bb, 2, dz);
        }
        self.bb.offset(0.0, 0.0, dz);
        let step = if self.kind == Kind::Chicken {
            0.5
        } else {
            0.5f64
        };
        if stepping && (ox != dx || oz != dz) {
            let (sx, sy, sz) = (dx, dy, dz);
            let saved = self.bb;
            self.bb = orig;
            dx = ox;
            dy = step;
            dz = oz;
            let list = colliding_boxes(w, &self.bb.add_coord(dx, dy, dz));
            for b in &list {
                dy = b.calc_offset(&self.bb, 1, dy);
            }
            self.bb.offset(0.0, dy, 0.0);
            for b in &list {
                dx = b.calc_offset(&self.bb, 0, dx);
            }
            self.bb.offset(dx, 0.0, 0.0);
            for b in &list {
                dz = b.calc_offset(&self.bb, 2, dz);
            }
            self.bb.offset(0.0, 0.0, dz);
            dy = -step;
            for b in &list {
                dy = b.calc_offset(&self.bb, 1, dy);
            }
            self.bb.offset(0.0, dy, 0.0);
            if sx * sx + sz * sz >= dx * dx + dz * dz {
                dx = sx;
                dy = sy;
                dz = sz;
                self.bb = saved;
            }
        }
        self.pos = [
            (self.bb.min[0] + self.bb.max[0]) / 2.0,
            self.bb.min[1],
            (self.bb.min[2] + self.bb.max[2]) / 2.0,
        ];
        self.collided_h = ox != dx || oz != dz;
        self.on_ground = oy != dy && oy < 0.0;
        // Entity.updateFallState
        if self.on_ground {
            if self.fall_distance > 0.0 {
                let dmg = (self.fall_distance - 3.0).ceil();
                if dmg > 0.0 && self.kind != Kind::Chicken {
                    self.damage(dmg, None);
                }
                self.fall_distance = 0.0;
            }
        } else if dy < 0.0 {
            self.fall_distance -= dy as f32;
        }
        if ox != dx {
            self.motion[0] = 0.0;
        }
        if oy != dy {
            self.motion[1] = 0.0;
        }
        if oz != dz {
            self.motion[2] = 0.0;
        }
    }

    fn move_flying(&mut self, mut strafe: f32, mut forward: f32, friction: f32) {
        let mut f = strafe * strafe + forward * forward;
        if f >= 1.0e-4 {
            f = f.sqrt();
            if f < 1.0 {
                f = 1.0;
            }
            f = friction / f;
            strafe *= f;
            forward *= f;
            let s = math::sin(self.yaw * math::PI / 180.0);
            let c = math::cos(self.yaw * math::PI / 180.0);
            self.motion[0] += (strafe * c - forward * s) as f64;
            self.motion[2] += (forward * c + strafe * s) as f64;
        }
    }

    fn on_ladder(&self, w: &ServerWorld) -> bool {
        if matches!(self.kind, Kind::Spider | Kind::CaveSpider) && self.collided_h {
            return true;
        }
        let (x, y, z) = (
            math::floor_double(self.pos[0]),
            math::floor_double(self.bb.min[1]),
            math::floor_double(self.pos[2]),
        );
        let id = w.peek_block(x, y, z);
        id == block::LADDER || id == block::VINE
    }

    /// `EntityLivingBase.moveEntityWithHeading`
    fn move_with_heading(&mut self, w: &ServerWorld, strafe: f32, forward: f32) {
        let web = {
            let (x, y, z) = (
                math::floor_double(self.pos[0]),
                math::floor_double(self.pos[1] + 0.1),
                math::floor_double(self.pos[2]),
            );
            w.peek_block(x, y, z) == block::WEB
                && self.kind != Kind::Spider
                && self.kind != Kind::CaveSpider
        };
        if self.in_water {
            let y0 = self.pos[1];
            self.move_flying(strafe, forward, 0.02);
            let m = self.motion;
            self.move_entity(w, m[0], m[1], m[2], web);
            for v in &mut self.motion {
                *v *= 0.8f32 as f64;
            }
            self.motion[1] -= 0.02;
            if self.collided_h
                && self.offset_free(
                    w,
                    self.motion[0],
                    self.motion[1] + 0.6 - self.pos[1] + y0,
                    self.motion[2],
                )
            {
                self.motion[1] = 0.3;
            }
        } else if self.in_material(w, &self.bb, Material::Lava) {
            let y0 = self.pos[1];
            self.move_flying(strafe, forward, 0.02);
            let m = self.motion;
            self.move_entity(w, m[0], m[1], m[2], web);
            for v in &mut self.motion {
                *v *= 0.5;
            }
            self.motion[1] -= 0.02;
            if self.collided_h
                && self.offset_free(
                    w,
                    self.motion[0],
                    self.motion[1] + 0.6 - self.pos[1] + y0,
                    self.motion[2],
                )
            {
                self.motion[1] = 0.3;
            }
        } else {
            let slip = |s: &Mob| {
                let id = w.peek_block(
                    math::floor_double(s.pos[0]),
                    math::floor_double(s.bb.min[1]) - 1,
                    math::floor_double(s.pos[2]),
                );
                block::info(id).slipperiness * 0.91
            };
            let mut f2 = 0.91f32;
            if self.on_ground {
                f2 = slip(self);
            }
            let f3 = 0.16277136f32 / (f2 * f2 * f2);
            let f4 = if self.on_ground {
                self.ai_speed * f3
            } else {
                0.02
            };
            self.move_flying(strafe, forward, f4);
            f2 = 0.91;
            if self.on_ground {
                f2 = slip(self);
            }
            if self.on_ladder(w) {
                let f5 = 0.15f32 as f64;
                self.motion[0] = self.motion[0].clamp(-f5, f5);
                self.motion[2] = self.motion[2].clamp(-f5, f5);
                self.fall_distance = 0.0;
                if self.motion[1] < -0.15 {
                    self.motion[1] = -0.15;
                }
            }
            let m = self.motion;
            self.move_entity(w, m[0], m[1], m[2], web);
            if self.collided_h && self.on_ladder(w) {
                self.motion[1] = 0.2;
            }
            self.motion[1] -= 0.08;
            self.motion[1] *= 0.98f32 as f64;
            self.motion[0] *= f2 as f64;
            self.motion[2] *= f2 as f64;
        }
        // limb swing
        self.prev_limb_amount = self.limb_amount;
        let (dx, dz) = (
            self.pos[0] - self.prev_pos[0],
            self.pos[2] - self.prev_pos[2],
        );
        let f6 = ((dx * dx + dz * dz).sqrt() as f32 * 4.0).min(1.0);
        self.limb_amount += (f6 - self.limb_amount) * 0.4;
        self.limb_swing += self.limb_amount;
    }

    fn offset_free(&self, w: &ServerWorld, x: f64, y: f64, z: f64) -> bool {
        let bb = self.bb.offset_copy(x, y, z);
        colliding_boxes(w, &bb).is_empty()
            && !self.in_material(w, &bb, Material::Water)
            && !self.in_material(w, &bb, Material::Lava)
    }

    fn jump(&mut self) {
        self.motion[1] = 0.42f32 as f64;
    }

    // ------------------------------------------------------------ AI helpers

    fn feet_block(&self) -> (i32, i32, i32) {
        (
            math::floor_double(self.pos[0]),
            math::floor_double(self.pos[1]),
            math::floor_double(self.pos[2]),
        )
    }

    /// `PathNavigate.tryMoveToXYZ`
    fn path_to(&mut self, w: &mut ServerWorld, x: f64, y: f64, z: f64, speed: f64) -> bool {
        let (wd, h) = self.dims();
        let from = Pt {
            x: math::floor_double(self.bb.min[0]),
            y: math::floor_double(self.bb.min[1] + 0.5),
            z: math::floor_double(self.bb.min[2]),
        };
        let to = Pt {
            x: math::floor_double(x - wd as f64 / 2.0),
            y: math::floor_double(y),
            z: math::floor_double(z - wd as f64 / 2.0),
        };
        let size = Size {
            w: (wd + 1.0).floor() as i32,
            h: (h + 1.0).floor() as i32,
        };
        let opts = Opts {
            doors: self.kind == Kind::Villager,
            avoid_water: !self.kind.animal() && self.kind != Kind::Villager,
            can_drown: true,
        };
        let in_lava = self.in_material(w, &self.bb, Material::Lava);
        match path::find(
            w,
            from,
            to,
            size,
            self.kind.follow_range(),
            &opts,
            3,
            self.feet_block(),
            in_lava,
        ) {
            Some(p) if p.len() > 1 => {
                self.path = p;
                self.path_index = 1;
                self.path_speed = speed;
                true
            }
            _ => {
                self.path.clear();
                false
            }
        }
    }

    fn no_path(&self) -> bool {
        self.path_index >= self.path.len()
    }

    /// `PathNavigate.onUpdateNavigation`
    fn follow_path(&mut self) {
        if self.no_path() {
            return;
        }
        let half = ((self.width() + 1.0).floor() as f64) * 0.5;
        let pos = [self.pos[0], (self.bb.min[1] + 0.5).floor(), self.pos[2]];
        let reach = (self.width() * self.width()) as f64;
        while self.path_index < self.path.len() {
            let p = self.path[self.path_index];
            let v = [p.x as f64 + half, p.y as f64, p.z as f64 + half];
            let d = (v[0] - pos[0]).powi(2) + (v[1] - pos[1]).powi(2) + (v[2] - pos[2]).powi(2);
            if d < reach.max(0.36) {
                self.path_index += 1;
            } else {
                break;
            }
        }
        if let Some(p) = self.path.get(self.path_index) {
            self.move_to = Some([p.x as f64 + half, p.y as f64, p.z as f64 + half]);
            self.move_speed = self.path_speed;
        }
    }

    /// `EntityMoveHelper.onUpdateMoveHelper`
    fn move_helper(&mut self) {
        self.move_forward = 0.0;
        if let Some(t) = self.move_to.take() {
            let iy = (self.bb.min[1] + 0.5).floor();
            let (dx, dz, dy) = (t[0] - self.pos[0], t[2] - self.pos[2], t[1] - iy);
            if dx * dx + dy * dy + dz * dz >= 2.5000003e-7 {
                let target = (dz.atan2(dx) * 180.0 / std::f64::consts::PI) as f32 - 90.0;
                self.yaw = approach(self.yaw, target, 30.0);
                // EntityLivingBase.getAIMoveSpeed: 0.1 for the old AI, which
                // walks with moveForward = movementSpeed.
                let f = (self.move_speed * self.kind.speed() as f64) as f32;
                self.ai_speed = if self.kind.old_ai() { 0.1 } else { f };
                self.move_forward = f;
                if dy > 0.0 && dx * dx + dz * dz < 1.0 {
                    self.jumping = true;
                }
            }
        }
    }

    /// `EntityLookHelper.onUpdateLook`
    fn look_helper(&mut self) {
        self.pitch = 0.0;
        if let Some(t) = self.look_at.take() {
            let dx = t[0] - self.pos[0];
            let dy = t[1] - (self.pos[1] + self.height() as f64 * 0.85);
            let dz = t[2] - self.pos[2];
            let d = (dx * dx + dz * dz).sqrt();
            let yaw = (dz.atan2(dx) * 180.0 / std::f64::consts::PI) as f32 - 90.0;
            let pitch = -(dy.atan2(d) * 180.0 / std::f64::consts::PI) as f32;
            self.pitch = approach(self.pitch, pitch, 40.0);
            self.head_yaw = approach(self.head_yaw, yaw, 10.0);
        } else {
            self.head_yaw = approach(self.head_yaw, self.body_yaw, 10.0);
            let d = wrap(self.head_yaw - self.body_yaw);
            if !self.no_path() {
                if d < -75.0 {
                    self.head_yaw = self.body_yaw - 75.0;
                }
                if d > 75.0 {
                    self.head_yaw = self.body_yaw + 75.0;
                }
            }
        }
    }

    /// `EntityTracker.trackEntity` / `EntityTrackerEntry.sendLocationToAllClients`
    /// for this mob, seen from one client.
    pub fn track(&mut self) {
        use super::tracker::{byte_angle, Packet, Tracker};
        let mut t = match self.net.tracker.take() {
            Some(t) => t,
            None => {
                let (range, vel) = match self.kind {
                    Kind::Squid => (64, true),
                    Kind::Bat => (80, false),
                    Kind::EnderDragon => (160, true),
                    _ => (80, true),
                };
                Tracker::new(self, range, 3, vel, 0, 0)
            }
        };
        let mut out = Vec::new();
        let (mut air, mut vel) = (false, false);
        t.update(
            self, &mut air, false, &mut vel, self.pos, false, true, &mut out,
        );
        self.net.tracker = Some(t);
        for p in out {
            match p {
                Packet::Spawn { .. } => {
                    // S0FPacketSpawnMob: multiplyBy32AndRound, byte angles
                    let size = super::tracker::Size::of(self.dims().0);
                    self.net.sp.pos = [
                        size.mul32(self.pos[0]),
                        crate::util::math::floor_double(self.pos[1] * 32.0),
                        size.mul32(self.pos[2]),
                    ];
                    let b = |a: f32| ((a * 256.0 / 360.0) as i32 as i8) as f32 * 360.0 / 256.0;
                    self.net.at = [
                        self.net.sp.pos[0] as f64 / 32.0,
                        self.net.sp.pos[1] as f64 / 32.0,
                        self.net.sp.pos[2] as f64 / 32.0,
                    ];
                    self.net.yaw = b(self.yaw);
                    self.net.pitch = b(self.pitch);
                    self.net.head = b(self.head_yaw);
                    self.net.head_byte =
                        crate::util::math::floor_float(self.head_yaw * 256.0 / 360.0);
                    self.net.seq = self.net.seq.wrapping_add(1);
                }
                p => {
                    if let Some((at, y, pi)) = self.net.sp.apply(&p, self.net.yaw, self.net.pitch) {
                        self.net.at = at;
                        self.net.yaw = y;
                        self.net.pitch = pi;
                        self.net.seq = self.net.seq.wrapping_add(1);
                    }
                }
            }
        }
        // S19PacketEntityHeadLook
        let h = crate::util::math::floor_float(self.head_yaw * 256.0 / 360.0);
        if (h - self.net.head_byte).abs() >= 4 {
            self.net.head_byte = h;
            self.net.head = byte_angle(h);
        }
    }

    /// The end of `EntityLivingBase.onUpdate`: `renderYawOffset` turns
    /// towards the direction moved (`updateDistance`, once `func_110146_f`).
    pub fn update_distance(&mut self) {
        let (dx, dz) = (
            self.pos[0] - self.prev_pos[0],
            self.pos[2] - self.prev_pos[2],
        );
        let d2 = (dx * dx + dz * dz) as f32;
        let mut target = self.body_yaw;
        if d2 > 0.0025000002 {
            target = (dz.atan2(dx) as f32) * 180.0 / std::f32::consts::PI - 90.0;
        }
        if self.swing > 0.0 {
            target = self.yaw;
        }
        let a = wrap(target - self.body_yaw);
        self.body_yaw += a * 0.3;
        let mut b = wrap(self.yaw - self.body_yaw);
        b = b.clamp(-75.0, 75.0);
        self.body_yaw = self.yaw - b;
        if b * b > 2500.0 {
            self.body_yaw += b * 0.2;
        }
        // keep the previous angles within 180 degrees for interpolation
        while self.body_yaw - self.prev_body_yaw < -180.0 {
            self.prev_body_yaw -= 360.0;
        }
        while self.body_yaw - self.prev_body_yaw >= 180.0 {
            self.prev_body_yaw += 360.0;
        }
        while self.head_yaw - self.prev_head_yaw < -180.0 {
            self.prev_head_yaw -= 360.0;
        }
        while self.head_yaw - self.prev_head_yaw >= 180.0 {
            self.prev_head_yaw += 360.0;
        }
    }

    /// `EntityBodyHelper.updateRenderAngles`
    fn body_helper(&mut self) {
        let (dx, dz) = (
            self.pos[0] - self.prev_pos[0],
            self.pos[2] - self.prev_pos[2],
        );
        let bound = |a: f32, b: f32, m: f32| {
            let d = wrap(a - b).clamp(-m, m);
            a - d
        };
        if dx * dx + dz * dz > 2.5000003e-7 {
            self.body_yaw = self.yaw;
            self.head_yaw = bound(self.body_yaw, self.head_yaw, 75.0);
            self.prev_head_for_body = self.head_yaw;
            self.body_counter = 0;
        } else {
            let mut m = 75.0f32;
            if (self.head_yaw - self.prev_head_for_body).abs() > 15.0 {
                self.body_counter = 0;
                self.prev_head_for_body = self.head_yaw;
            } else {
                self.body_counter += 1;
                if self.body_counter > 10 {
                    m = (1.0 - (self.body_counter - 10) as f32 / 10.0).max(0.0) * 75.0;
                }
            }
            self.body_yaw = bound(self.head_yaw, self.body_yaw, m);
        }
    }

    /// `RandomPositionGenerator.findRandomTargetBlock` (block path weight
    /// favours grass for animals and darkness for monsters).
    fn random_target(
        &mut self,
        w: &mut ServerWorld,
        xz: i32,
        y: i32,
        away: Option<[f64; 3]>,
    ) -> Option<[f64; 3]> {
        let mut best = None;
        let mut best_w = -99999.0f32;
        for _ in 0..10 {
            let mut dx = self.rand.next_int_n(2 * xz) - xz;
            let dy = self.rand.next_int_n(2 * y) - y;
            let mut dz = self.rand.next_int_n(2 * xz) - xz;
            if let Some(a) = away {
                let (vx, vz) = (self.pos[0] - a[0], self.pos[2] - a[2]);
                if (dx as f64) * vx + (dz as f64) * vz < 0.0 {
                    continue;
                }
            }
            dx += math::floor_double(self.pos[0]);
            let ty = dy + math::floor_double(self.pos[1]);
            dz += math::floor_double(self.pos[2]);
            let wgt = self.path_weight(w, dx, ty, dz);
            if wgt > best_w {
                best_w = wgt;
                best = Some([dx as f64, ty as f64, dz as f64]);
            }
        }
        best
    }

    /// `getBlockPathWeight`
    fn path_weight(&self, w: &mut ServerWorld, x: i32, y: i32, z: i32) -> f32 {
        if self.kind.animal() {
            if w.block(x, y - 1, z) == block::GRASS {
                10.0
            } else {
                w.full_block_light(x, y, z) as f32 / 15.0 - 0.5
            }
        } else if self.kind.hostile() {
            0.5 - brightness(w, x, y, z)
        } else {
            0.0
        }
    }

    // ------------------------------------------------------------ combat

    /// `EntityLivingBase.attackEntityFrom` (without the attacker part).
    /// `EntityPotion.onImpact` on a mob: the instant potions act now
    /// (`Potion.affectEntity`, undead swapped).
    pub fn splash(&mut self, effects: &[crate::potion::Effect], k: f64, from: Option<[f64; 3]>) {
        use crate::potion::{HARM, HEAL};
        let undead = matches!(self.kind, Kind::Zombie | Kind::Skeleton | Kind::PigZombie);
        for e in effects {
            if !e.potion().instant {
                continue;
            }
            let hurts = (e.id == HARM && !undead) || (e.id == HEAL && undead);
            let heals = (e.id == HEAL && !undead) || (e.id == HARM && undead);
            if hurts {
                let n = (k * (6 << e.amplifier) as f64 + 0.5) as i32;
                self.damage(n as f32, from);
            } else if heals {
                let n = (k * (4 << e.amplifier) as f64 + 0.5) as i32;
                if self.health > 0.0 {
                    self.health = (self.health + n as f32).min(self.kind.max_health());
                }
            }
        }
    }

    /// `attackEntityFrom` with a source that bypasses armour.
    pub fn damage(&mut self, amount: f32, from: Option<[f64; 3]>) -> bool {
        self.hurt_by(amount, from, false, false)
    }

    /// `attackEntityFrom(DamageSource.drown, amount)`: the one source that
    /// skips `setBeenAttacked`.
    pub fn drown(&mut self, amount: f32) -> bool {
        self.hurt_by(amount, None, false, true)
    }

    /// `attackEntityFrom`; `blockable` is `!source.isUnblockable()`, when
    /// `damageEntity` takes the armour off (`applyArmorCalculations`).
    pub fn hurt(&mut self, amount: f32, from: Option<[f64; 3]>, blockable: bool) -> bool {
        self.hurt_by(amount, from, blockable, false)
    }

    fn hurt_by(
        &mut self,
        amount: f32,
        from: Option<[f64; 3]>,
        blockable: bool,
        drown: bool,
    ) -> bool {
        if self.health <= 0.0 {
            return false;
        }
        let applied;
        let fresh;
        if self.hurt_resistant as f32 > 10.0 && amount <= self.last_damage {
            return false;
        }
        self.looting = 0;
        if self.hurt_resistant as f32 > 10.0 {
            if amount <= self.last_damage {
                return false;
            }
            applied = self.armor_calc(amount - self.last_damage, blockable);
            self.last_damage = amount;
            fresh = false;
        } else {
            self.last_damage = amount;
            self.hurt_resistant = 20;
            applied = self.armor_calc(amount, blockable);
            self.hurt_time = 10;
            fresh = true;
        }
        if fresh {
            // setBeenAttacked: velocityChanged = rand.nextDouble() >= resistance
            if !drown {
                let _ = self.rand.next_double();
            }
            if let Some(a) = from {
                // the direction loop takes Math.random(), not the mob's rand
                let mut dx = a[0] - self.pos[0];
                let mut dz = a[2] - self.pos[2];
                while dx * dx + dz * dz < 1.0e-4 {
                    dx = (math::math_random() - math::math_random()) * 0.01;
                    dz = (math::math_random() - math::math_random()) * 0.01;
                }
                self.knock_back(dx, dz);
            } else {
                // attackedAtYaw = (int)(Math.random() * 2.0) * 180
                let _ = math::math_random();
            }
        }
        self.health -= applied;
        // EntityAnimal.attackEntityFrom
        self.in_love = 0;
        match self.kind {
            Kind::Bat => self.hanging = false,
            Kind::Enderman => self.angry = true,
            // aiSit.setSitting(false)
            Kind::Wolf | Kind::Ocelot => self.sit_order = false,
            _ => {}
        }
        if !ai::uses_tasks(self.kind)
            && (self.kind == Kind::Horse
                || (self.kind.animal() && !matches!(self.kind, Kind::Wolf | Kind::Ocelot))
                || self.kind == Kind::Villager)
        {
            self.task = Task::Panic;
            self.task_ticks = 0;
            self.path.clear();
        }
        true
    }

    /// `EntityLivingBase.knockBack`: no mob here has knockback resistance
    /// (the zombie's spawn bonus comes with `onSpawnWithEgg`, not ported),
    /// but the draw happens all the same.
    fn knock_back(&mut self, dx: f64, dz: f64) {
        if self.rand.next_double() < self.kb_resist {
            return;
        }
        let f = math::sqrt_double(dx * dx + dz * dz) as f64;
        let k = 0.4f32 as f64;
        self.motion[0] /= 2.0;
        self.motion[1] /= 2.0;
        self.motion[2] /= 2.0;
        self.motion[0] -= dx / f * k;
        self.motion[1] += k;
        self.motion[2] -= dz / f * k;
        if self.motion[1] > 0.4f32 as f64 {
            self.motion[1] = 0.4f32 as f64;
        }
    }

    /// `applyArmorCalculations` (mobs wear nothing, so `damageArmor` and
    /// the enchantment part of `applyPotionDamageCalculations` do nothing).
    fn armor_calc(&self, a: f32, blockable: bool) -> f32 {
        if !blockable {
            return a;
        }
        let k = 25 - self.total_armor();
        let f = a * k as f32;
        f / 25.0
    }

    /// `EntityLivingBase.onDeath`'s loot: `dropFewItems(recentlyHit > 0,
    /// looting)`, `dropEquipment` (mobs here wear nothing) and, after a
    /// player's hit, `rand.nextInt(200) - looting < 5` for `dropRareDrop`.
    /// Every `dropItem(item, 1)` is its own stack of one, as each makes its
    /// own `EntityItem`.
    fn drops(&mut self, by_player: bool, looting: i32) -> Vec<Stack> {
        let mut v = self.drop_few_items(by_player, looting);
        self.drop_equipment(by_player, looting, &mut v);
        if by_player {
            let j = self.rand.next_int_n(200) - looting;
            if j < 5 {
                self.drop_rare_drop(&mut v);
            }
        }
        v
    }

    /// `Entity.setFire` (mobs wear nothing, so no Fire Protection).
    pub fn set_fire(&mut self, seconds: i32) {
        let t = seconds * 20;
        if self.fire < t {
            self.fire = t;
        }
    }

    /// `Entity.isBurning` on the server
    pub fn burning(&self) -> bool {
        !self.kind.fireproof() && self.fire > 0
    }

    /// `dropFewItems(recentlyHit, looting)` of each class.
    fn drop_few_items(&mut self, hit: bool, l: i32) -> Vec<Stack> {
        let burning = self.burning();
        let mut v = Vec::new();
        let r = &mut self.rand;
        let mut each = |v: &mut Vec<Stack>, id: u16, meta: i32, n: i32| {
            for _ in 0..n {
                v.push(Stack::new(id, meta, 1));
            }
        };
        // EntityLiving.dropFewItems with getDropItem
        let living = |r: &mut JavaRandom, v: &mut Vec<Stack>, id: u16| {
            let mut n = r.next_int_n(3);
            if l > 0 {
                n += r.next_int_n(l + 1);
            }
            for _ in 0..n {
                v.push(Stack::new(id, 0, 1));
            }
        };
        match self.kind {
            Kind::Pig => {
                let n = r.next_int_n(3) + 1 + r.next_int_n(1 + l);
                each(&mut v, if burning { 320 } else { 319 }, 0, n);
            }
            Kind::Cow | Kind::Mooshroom => {
                let n = r.next_int_n(3) + r.next_int_n(1 + l);
                each(&mut v, 334, 0, n);
                let n = r.next_int_n(3) + 1 + r.next_int_n(1 + l);
                each(&mut v, if burning { 364 } else { 363 }, 0, n);
            }
            Kind::Sheep => {
                if !self.sheared {
                    v.push(Stack::new(block::WOOL as u16, self.variant, 1));
                }
            }
            Kind::Chicken => {
                let n = r.next_int_n(3) + r.next_int_n(1 + l);
                each(&mut v, 288, 0, n);
                each(&mut v, if burning { 366 } else { 365 }, 0, 1);
            }
            Kind::Zombie => living(r, &mut v, 367),
            Kind::Skeleton => {
                if self.variant == 1 {
                    let n = r.next_int_n(3 + l) - 1;
                    each(&mut v, 263, 0, n);
                } else {
                    let n = r.next_int_n(3 + l);
                    each(&mut v, 262, 0, n);
                }
                let n = r.next_int_n(3 + l);
                each(&mut v, 352, 0, n);
            }
            Kind::Creeper => living(r, &mut v, 289),
            Kind::Spider | Kind::CaveSpider => {
                living(r, &mut v, 287);
                if hit && (r.next_int_n(3) == 0 || r.next_int_n(1 + l) > 0) {
                    each(&mut v, 375, 0, 1);
                }
            }
            Kind::Witch => {
                let n = r.next_int_n(3) + 1;
                for _ in 0..n {
                    let mut k = r.next_int_n(3);
                    let item = [348, 353, 331, 375, 374, 289, 280, 280][r.next_int_n(8) as usize];
                    if l > 0 {
                        k += r.next_int_n(l + 1);
                    }
                    each(&mut v, item, 0, k);
                }
            }
            Kind::Slime => {
                // getDropItem: slime balls from the smallest only
                if self.variant == 1 {
                    living(r, &mut v, 341);
                }
            }
            Kind::MagmaCube => {
                if self.variant > 1 {
                    let mut n = r.next_int_n(4) - 2;
                    if l > 0 {
                        n += r.next_int_n(l + 1);
                    }
                    each(&mut v, 378, 0, n);
                }
            }
            Kind::Enderman => {
                let n = r.next_int_n(2 + l);
                each(&mut v, 368, 0, n);
            }
            Kind::Squid => {
                let n = r.next_int_n(3 + l) + 1;
                each(&mut v, 351, 0, n);
            }
            Kind::Horse => {
                // EntityHorse.getDropItem draws first: a zombie horse drops
                // nothing one time in four
                let none = r.next_int_n(4) == 0;
                let item = match self.variant >> 16 {
                    4 => Some(352),
                    3 => (!none).then_some(367),
                    _ => Some(334),
                };
                if let Some(id) = item {
                    living(r, &mut v, id);
                }
            }
            Kind::IronGolem => {
                let n = r.next_int_n(3);
                each(&mut v, block::RED_FLOWER as u16, 0, n);
                let n = 3 + r.next_int_n(3);
                each(&mut v, 265, 0, n);
            }
            Kind::Ghast => {
                let n = r.next_int_n(2) + r.next_int_n(1 + l);
                each(&mut v, 370, 0, n);
                let n = r.next_int_n(3) + r.next_int_n(1 + l);
                each(&mut v, 289, 0, n);
            }
            Kind::PigZombie => {
                let n = r.next_int_n(2 + l);
                each(&mut v, 367, 0, n);
                let n = r.next_int_n(2 + l);
                each(&mut v, 371, 0, n);
            }
            Kind::Blaze => {
                if hit {
                    let n = r.next_int_n(2 + l);
                    each(&mut v, 369, 0, n);
                }
            }
            // getDropItem is null (item 0 or -1), or dropFewItems is empty
            Kind::Villager
            | Kind::Silverfish
            | Kind::Bat
            | Kind::Wolf
            | Kind::Ocelot
            | Kind::EnderDragon
            | Kind::EnderCrystal => {}
        }
        v
    }

    /// `dropRareDrop(int)` (the argument is unused in 1.7.10).
    fn drop_rare_drop(&mut self, v: &mut Vec<Stack>) {
        match self.kind {
            Kind::Zombie => {
                let id = match self.rand.next_int_n(3) {
                    0 => 265,
                    1 => 391,
                    _ => 392,
                };
                v.push(Stack::new(id, 0, 1));
            }
            Kind::PigZombie => v.push(Stack::new(266, 0, 1)),
            Kind::Skeleton if self.variant == 1 => v.push(Stack::new(397, 1, 1)),
            _ => {}
        }
    }

    /// `getExperiencePoints`: animals and squid draw from the world's random.
    fn xp(&mut self, world_rand: &mut JavaRandom) -> i32 {
        match self.kind {
            Kind::Slime | Kind::MagmaCube => return self.variant,
            Kind::Blaze => return 10,
            Kind::EnderCrystal | Kind::EnderDragon => return 0,
            Kind::Squid => return 1 + world_rand.next_int_n(3),
            Kind::Bat | Kind::IronGolem => return 0,
            _ => {}
        }
        if self.kind.animal() {
            1 + world_rand.next_int_n(3)
        } else if self.kind.hostile() {
            5
        } else {
            0
        }
    }

    /// `moveStrafing`, `moveForward` (for the minecart it rides)
    pub fn move_input(&self) -> (f32, f32) {
        (self.move_strafe, self.move_forward)
    }

    pub fn eye(&self) -> [f64; 3] {
        [
            self.pos[0],
            self.pos[1] + self.height() as f64 * 0.85,
            self.pos[2],
        ]
    }

    fn dist2(&self, p: [f64; 3]) -> f64 {
        (self.pos[0] - p[0]).powi(2) + (self.pos[1] - p[1]).powi(2) + (self.pos[2] - p[2]).powi(2)
    }

    /// `EntityLivingBase.canEntityBeSeen`: no opaque block on the eye line.
    fn can_see(&self, w: &ServerWorld, to: [f64; 3]) -> bool {
        let from = self.eye();
        let steps = (((to[0] - from[0]).abs() + (to[1] - from[1]).abs() + (to[2] - from[2]).abs())
            * 4.0) as i32
            + 1;
        for i in 0..=steps {
            let t = i as f64 / steps as f64;
            let p = [
                from[0] + (to[0] - from[0]) * t,
                from[1] + (to[1] - from[1]) * t,
                from[2] + (to[2] - from[2]) * t,
            ];
            let id = w.peek_block(
                math::floor_double(p[0]),
                math::floor_double(p[1]),
                math::floor_double(p[2]),
            );
            if block::info(id).opaque {
                return false;
            }
        }
        true
    }
}

/// `World.getLightBrightness` (sky light reduced by the time of day).
pub fn brightness(w: &mut ServerWorld, x: i32, y: i32, z: i32) -> f32 {
    let l = w.block_light_value(x, y, z, true).clamp(0, 15);
    let f1 = 1.0 - l as f32 / 15.0;
    (1.0 - f1) / (f1 * 3.0 + 1.0)
}

/// Every mob, arrow and the spawning state of the world.
/// `EntityTNTPrimed`: `pos` is the centre of the 0.98 box.
#[derive(Clone, Debug)]
pub struct Tnt {
    pub pos: [f64; 3],
    pub prev_pos: [f64; 3],
    pub motion: [f64; 3],
    pub fuse: i32,
    on_ground: bool,
}

pub struct Mobs {
    pub list: Vec<Mob>,
    pub arrows: Vec<Arrow>,
    /// Snowballs, eggs, ender pearls and splash potions in flight.
    pub throwables: Vec<super::throwable::Throwable>,
    /// `playAuxSFX(2002)`: splash potions that broke (position, damage value).
    pub splashes: Vec<([f64; 3], i32)>,
    pub tnt: Vec<Tnt>,
    pub fireballs: Vec<Fireball>,
    /// Explosions of other entities (`EntityMinecartTNT.explodeCart`), done
    /// in the next tick.
    pub queued_explosions: Vec<crate::world::explosion::Request>,
    /// Entities outside `Mobs` an explosion can reach (items, vehicles,
    /// falling blocks), set by the owner before `tick`.
    pub blast_targets: Vec<crate::world::explosion::Target>,
    next_id: u32,
    pub rand: JavaRandom,
}

impl Default for Mobs {
    fn default() -> Self {
        Mobs {
            list: Vec::new(),
            arrows: Vec::new(),
            throwables: Vec::new(),
            splashes: Vec::new(),
            tnt: Vec::new(),
            fireballs: Vec::new(),
            queued_explosions: Vec::new(),
            blast_targets: Vec::new(),
            next_id: 1,
            rand: JavaRandom::new(0x6D6F62),
        }
    }
}

/// Output of a tick: damage to the player, dropped items, sounds.
#[derive(Default)]
pub struct Outcome {
    /// Throwables spawned this tick (the witch's potions).
    pub thrown: Vec<super::throwable::Throwable>,
    pub hits: Vec<Hit>,
    /// Experience to drop as `EntityXPOrb`s (position, amount).
    pub orbs: Vec<([f64; 3], i32)>,
    pub drops: Vec<([f64; 3], Stack)>,
    pub sounds: Vec<(&'static str, [f64; 3], f32, f32)>,
    /// Explosions (centre, size) for particles and sound.
    pub explosions: Vec<crate::world::explosion::View>,
    /// What the explosions did to `Mobs::blast_targets`.
    pub blasts: Vec<crate::world::explosion::Blast>,
    /// Particles for the client (portal, slime, heart, smoke).
    pub particles: Vec<Puff>,
    /// The mob that last hurt the player this tick.
    pub attacker: Option<u32>,
    /// `EntityAIHurtByTarget` calling the mobs around for help.
    pub calls: Vec<ai::Call>,
    /// `EntityAIMate.spawnBaby`
    pub births: Vec<ai::Birth>,
    /// Uses the held item lost to a right click on a mob
    /// (`damageItem(n, player)`, applied by the caller with the player's
    /// random).
    pub wear: i32,
}

/// `Entity.moveEntity` for a primed TNT box.
fn tnt_move(t: &mut Tnt, w: &ServerWorld) {
    use super::player::{colliding_boxes, Aabb};
    let h = 0.49;
    let mut bb = Aabb {
        min: [t.pos[0] - h, t.pos[1] - h, t.pos[2] - h],
        max: [t.pos[0] + h, t.pos[1] + h, t.pos[2] + h],
    };
    let (ox, oy, oz) = (t.motion[0], t.motion[1], t.motion[2]);
    let (mut dx, mut dy, mut dz) = (ox, oy, oz);
    let list = colliding_boxes(w, &bb.add_coord(dx, dy, dz));
    for b in &list {
        dy = b.calc_offset(&bb, 1, dy);
    }
    bb.offset(0.0, dy, 0.0);
    for b in &list {
        dx = b.calc_offset(&bb, 0, dx);
    }
    bb.offset(dx, 0.0, 0.0);
    for b in &list {
        dz = b.calc_offset(&bb, 2, dz);
    }
    bb.offset(0.0, 0.0, dz);
    t.pos = [
        (bb.min[0] + bb.max[0]) / 2.0,
        bb.min[1] + h,
        (bb.min[2] + bb.max[2]) / 2.0,
    ];
    t.on_ground = oy != dy && oy < 0.0;
    if ox != dx {
        t.motion[0] = 0.0;
    }
    if oy != dy {
        t.motion[1] = 0.0;
    }
    if oz != dz {
        t.motion[2] = 0.0;
    }
}

impl Mobs {
    /// `new EntityTNTPrimed(world, x, y, z, placer)` at the block centre.
    /// `World.createExplosion(entity, x, y, z, size, true)` for an entity
    /// that is not a mob.
    pub fn queue_explosion(&mut self, pos: [f64; 3], size: f32, ignited: bool) {
        use crate::world::explosion::{Exploder, Request};
        self.queued_explosions.push(Request {
            pos,
            size,
            flaming: false,
            smoking: true,
            exploder: Exploder::Cart { ignited },
            exclude: None,
        });
    }

    pub fn prime_tnt(&mut self, pos: [f64; 3], fuse: i32) {
        let f = (self.rand.next_double() * std::f64::consts::PI * 2.0) as f32;
        let motion = [
            (-(f.sin()) * 0.02) as f64,
            0.2f32 as f64,
            (-(f.cos()) * 0.02) as f64,
        ];
        self.tnt.push(Tnt {
            pos,
            prev_pos: pos,
            motion,
            fuse,
            on_ground: false,
        });
    }

    /// `ItemSnowball`, `ItemEgg`, `ItemEnderPearl`, `ItemPotion` (splash):
    /// `new EntityXxx(world, player)` from the server-side player.
    pub fn throw_from_player(
        &mut self,
        kind: super::throwable::Kind,
        feet: [f64; 3],
        yaw: f32,
        pitch: f32,
    ) {
        let id = self.rand.next_int() as u32;
        let t = super::throwable::Throwable::thrown(
            id,
            kind,
            super::arrow::Shooter::Player,
            feet,
            feet[1] + 0.12f32 as f64,
            yaw,
            pitch,
        );
        self.throwables.push(t);
    }

    /// `EntityThrowable.onUpdate` for everything thrown, and the impacts.
    fn tick_throwables(&mut self, w: &mut ServerWorld, player: &PlayerInfo, out: &mut Outcome) {
        use super::arrow::{Shooter, Target};
        use super::base::Fx;
        use super::throwable::{Impact, Kind as TK};
        let mut targets: Vec<(Target, Aabb)> = Vec::new();
        if player.attackable {
            targets.push((Target::Player, player.bb()));
        }
        for m in self.list.iter() {
            if m.health > 0.0 {
                targets.push((Target::Mob(m.id), m.bb));
            }
        }
        let mut list = std::mem::take(&mut self.throwables);
        for t in list.iter_mut() {
            let mut fx = Vec::new();
            let impacts = t.tick(&*w as &dyn crate::world::BlockView, &targets, &mut fx);
            let from = match t.thrower {
                Shooter::Player => Some(player.feet),
                Shooter::Mob(id) => self.list.iter().find(|m| m.id == id).map(|m| m.pos),
                Shooter::None => None,
            };
            for imp in impacts {
                match imp {
                    Impact::Hurt(Target::Mob(id), mut amount) => {
                        if let Some(m) = self.list.iter_mut().find(|m| m.id == id) {
                            // EntitySnowball: blazes take 3
                            if t.kind == TK::Snowball && m.kind == Kind::Blaze {
                                amount = 3.0;
                            }
                            m.hurt(amount, from, true);
                        }
                    }
                    // EntityPlayer.attackEntityFrom returns at once for 0
                    Impact::Hurt(Target::Player, _) => {}
                    Impact::Chickens(p, yaw, n) => {
                        for _ in 0..n {
                            let cid = self.spawn(
                                Kind::Chicken,
                                p,
                                yaw,
                                &mut JavaRandom::new(t.id as i64),
                            );
                            if let Some(c) = self.list.iter_mut().find(|m| m.id == cid) {
                                c.growing_age = -24000;
                                c.set_position(p[0], p[1], p[2]);
                            }
                        }
                    }
                    Impact::Teleport(p) => out.hits.push(Hit::Teleport(p)),
                    Impact::Xp(p, xp) => {
                        self.splashes.push((p, 0));
                        out.orbs.push((p, xp));
                    }
                    Impact::Splash(p, meta, hits) => {
                        let effects = crate::potion::effects_of(meta, false);
                        for (tg, k) in hits {
                            match tg {
                                Target::Player => out.hits.push(Hit::Potion {
                                    effects: effects.clone(),
                                    scale: k,
                                    from: from.unwrap_or(p),
                                }),
                                Target::Mob(id) => {
                                    if let Some(m) = self.list.iter_mut().find(|m| m.id == id) {
                                        m.splash(&effects, k, from);
                                    }
                                }
                            }
                        }
                        self.splashes.push((p, meta));
                    }
                }
            }
            for f in fx {
                match f {
                    Fx::Sound(n, p, v, pi) => out.sounds.push((n, p, v, pi)),
                    Fx::Particle(n, p, mo) => out.particles.push((n, p, mo)),
                    _ => {}
                }
            }
        }
        list.retain(|t| !t.e.dead);
        self.throwables.extend(list);
    }

    /// `ItemBow.onPlayerStoppedUsing`: `new EntityArrow(world, player, f * 2)`
    /// from the server-side player (whose `posY` is the feet, eye height 0.12).
    pub fn shoot_player_arrow(
        &mut self,
        feet: [f64; 3],
        yaw: f32,
        pitch: f32,
        v: f32,
        crit: bool,
        creative: bool,
    ) -> &mut Arrow {
        let id = self.rand.next_int() as u32;
        let mut a = Arrow::from_look(
            id,
            super::arrow::Shooter::Player,
            feet,
            feet[1] + 0.12f32 as f64,
            yaw,
            pitch,
            v,
        );
        a.crit = crit;
        if creative {
            a.pickup = 2;
        }
        self.arrows.push(a);
        self.arrows.last_mut().unwrap()
    }

    /// `BehaviorProjectileDispense` for arrows: `EntityArrow` launched along
    /// `dir` at 1.1 with inaccuracy 6.
    pub fn dispense_arrow(&mut self, pos: [f64; 3], dir: [f64; 3]) {
        let id = self.rand.next_int() as u32;
        let mut a = Arrow::at_pos(id, pos);
        a.pickup = 1;
        a.set_heading(dir[0], dir[1] + 0.1f32 as f64, dir[2], 1.1, 6.0);
        self.arrows.push(a);
    }

    /// A new mob with its own random, not yet in the world.
    pub fn make(&mut self, kind: Kind, pos: [f64; 3], yaw: f32) -> Mob {
        let id = self.next_id;
        self.next_id += 1;
        let seed = self.rand.next_long();
        Mob::new(id, kind, pos, yaw, seed)
    }

    pub fn spawn(
        &mut self,
        kind: Kind,
        pos: [f64; 3],
        yaw: f32,
        world_rand: &mut JavaRandom,
    ) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        let seed = self.rand.next_long();
        let mut m = Mob::new(id, kind, pos, yaw, seed);
        let cx = SpawnCtx::default();
        m.on_spawn(world_rand, &cx);
        self.list.push(m);
        id
    }

    /// `spawn` with the world's local difficulty and dimension.
    pub fn spawn_in(
        &mut self,
        w: &mut ServerWorld,
        kind: Kind,
        pos: [f64; 3],
        yaw: f32,
        world_rand: &mut JavaRandom,
    ) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        let seed = self.rand.next_long();
        let mut m = Mob::new(id, kind, pos, yaw, seed);
        let cx = SpawnCtx::of(w, pos);
        m.on_spawn(world_rand, &cx);
        self.list.push(m);
        id
    }

    /// One tick for every mob in a loaded chunk.
    pub fn tick(&mut self, w: &mut ServerWorld, player: &PlayerInfo, out: &mut Outcome) {
        let day = w.skylight_subtracted < 4;
        let villagers: Vec<[f64; 3]> = self
            .list
            .iter()
            .filter(|m| m.kind == Kind::Villager && !m.dead)
            .map(|m| m.pos)
            .collect();
        let zombies: Vec<[f64; 3]> = self
            .list
            .iter()
            .filter(|m| m.kind == Kind::Zombie && !m.dead)
            .map(|m| m.pos)
            .collect();
        let mut new_arrows = Vec::new();
        let mut explosions: Vec<crate::world::explosion::Request> = Vec::new();
        let mut balls: Vec<Fireball> = Vec::new();
        let others: Vec<Other> = self
            .list
            .iter()
            .filter(|m| m.health > 0.0)
            .map(|m| Other {
                id: m.id,
                kind: m.kind,
                pos: m.pos,
                tamed: m.tamed,
                bb: m.bb,
                width: m.dims().0,
                height: m.dims().1,
                growing_age: m.growing_age,
                in_love: m.in_love > 0,
                sitting: m.sitting,
                variant: m.variant,
            })
            .collect();
        let mut strikes: Vec<Strike> = Vec::new();
        for m in self.list.iter_mut() {
            if !w.loaded(math::floor_double(m.pos[0]), math::floor_double(m.pos[2])) {
                continue;
            }
            tick_mob(
                m,
                w,
                player,
                day,
                &villagers,
                &zombies,
                out,
                &mut new_arrows,
                &mut explosions,
                &others,
                &mut strikes,
                &mut balls,
            );
            if m.kind != Kind::EnderDragon {
                m.update_distance();
            }
            m.track();
        }
        ai::answer_calls(&mut self.list, &out.calls);
        out.calls.clear();
        // EntityAIMate.spawnBaby: the other parent, then the young one
        for b in std::mem::take(&mut out.births) {
            if let Some(o) = self.list.iter_mut().find(|m| m.id == b.parents[1]) {
                o.growing_age = 6000;
                o.in_love = 0;
            }
            let mut c = self.make(b.kind, b.pos, 0.0);
            c.growing_age = -24000;
            c.set_position(b.pos[0], b.pos[1], b.pos[2]);
            c.variant = b.variant;
            if b.tamed {
                c.tamed = true;
            }
            c.persistent = true;
            self.list.push(c);
        }
        self.fireballs.extend(balls);
        // EntityFireball.onUpdate
        let mut k = 0;
        while k < self.fireballs.len() {
            let hit = nether::tick_fireball(&mut self.fireballs[k], w, player, &self.list);
            let gone = self.fireballs[k].ticks_alive() > 600;
            match hit {
                Some((imp, at)) => {
                    let f = self.fireballs.remove(k);
                    match imp {
                        nether::Impact::Player => {
                            out.hits.push(Hit::Fireball {
                                amount: if f.small { 5.0 } else { 6.0 },
                                shooter: f.shooter_id(),
                                small: f.small,
                            });
                        }
                        nether::Impact::Mob(id) => {
                            if let Some(t) = self.list.iter_mut().find(|m| m.id == id) {
                                let dmg = if f.small { 5.0 } else { 6.0 };
                                if !(f.small && t.kind.fireproof())
                                    && t.hurt(dmg, Some(f.pos), true)
                                {
                                    if f.small && !t.kind.fireproof() {
                                        t.fire = 100;
                                    }
                                    out.sounds.push((mob_sound(t, false), t.pos, 1.0, 1.0));
                                }
                            }
                        }
                        nether::Impact::Block(c, side) => {
                            if f.small {
                                // EntitySmallFireball: fire on the side that was hit
                                let p = [c[0] + side[0], c[1] + side[1], c[2] + side[2]];
                                if w.is_air(p[0], p[1], p[2]) {
                                    w.set_block(p[0], p[1], p[2], block::FIRE, 0, 3);
                                }
                            }
                        }
                    }
                    if !f.small {
                        // EntityLargeFireball: newExplosion(null, ..., 1, true, mobGriefing)
                        explosions.push(crate::world::explosion::Request {
                            pos: at,
                            size: 1.0,
                            flaming: true,
                            smoking: true,
                            exploder: crate::world::explosion::Exploder::None,
                            exclude: None,
                        });
                    }
                }
                None if gone => {
                    self.fireballs.remove(k);
                }
                None => k += 1,
            }
        }
        self.arrows.extend(new_arrows);
        for st in strikes {
            // attackEntityFrom: a tamed wolf's kill counts as the player's
            // for loot (recentlyHit, attackingPlayer null so no looting)
            let tamed_wolf = self
                .list
                .iter()
                .any(|m| m.id == st.from && m.kind == Kind::Wolf && m.tamed);
            let Some(t) = self.list.iter_mut().find(|m| m.id == st.target) else {
                continue;
            };
            if t.hurt(st.amount, Some(st.at), true) {
                if tamed_wolf {
                    t.recently_hit = 100;
                }
                t.set_revenge(Some(ai::Target::Mob(st.from)));
                t.motion[1] += st.lift;
                out.sounds.push((mob_sound(t, false), t.pos, 1.0, 1.0));
                // EntityAIHurtByTarget
                if matches!(t.kind, Kind::Wolf | Kind::IronGolem | Kind::Ocelot) || t.kind.hostile()
                {
                    t.target_mob = Some(st.from);
                }
            }
        }
        // EntityTNTPrimed.onUpdate
        let mut k = 0;
        while k < self.tnt.len() {
            let t = &mut self.tnt[k];
            t.prev_pos = t.pos;
            t.motion[1] -= 0.04;
            tnt_move(t, w);
            for v in &mut t.motion {
                *v *= 0.98;
            }
            if t.on_ground {
                t.motion[0] *= 0.7;
                t.motion[2] *= 0.7;
                t.motion[1] *= -0.5;
            }
            let f = t.fuse;
            t.fuse -= 1;
            if f <= 0 {
                let c = t.pos;
                self.tnt.remove(k);
                // EntityTNTPrimed.explode
                explosions.push(crate::world::explosion::Request {
                    pos: c,
                    size: 4.0,
                    flaming: false,
                    smoking: true,
                    exploder: crate::world::explosion::Exploder::Tnt,
                    exclude: None,
                });
            } else {
                k += 1;
            }
        }
        explosions.extend(std::mem::take(&mut self.queued_explosions));
        for r in explosions {
            explode(w, self, player, r, out);
        }
        // deaths
        let mut splits: Vec<([f64; 3], i32, f32)> = Vec::new();
        let mut blasts: Vec<[f64; 3]> = Vec::new();
        let mut i = 0;
        while i < self.list.len() {
            let m = &mut self.list[i];
            if m.health <= 0.0 && m.death_time == 0 {
                let by_player = m.recently_hit > 0;
                let looting = m.looting;
                for s in m.drops(by_player, looting) {
                    out.drops.push(([m.pos[0], m.pos[1] + 0.0, m.pos[2]], s));
                }
                let s = mob_sound(m, true);
                if !s.is_empty() {
                    out.sounds.push((s, m.pos, 1.0, 1.0));
                }
                // EntitySlime.setDead: split into smaller slimes
                if m.kind == Kind::Slime && m.variant > 1 {
                    let size = m.variant;
                    let n = 2 + m.rand.next_int_n(3);
                    for k in 0..n {
                        let fx = ((k % 2) as f32 - 0.5) * size as f32 / 4.0;
                        let fz = ((k / 2) as f32 - 0.5) * size as f32 / 4.0;
                        let yaw = m.rand.next_float() * 360.0;
                        splits.push((
                            [m.pos[0] + fx as f64, m.pos[1] + 0.5, m.pos[2] + fz as f64],
                            size / 2,
                            yaw,
                        ));
                    }
                }
            }
            if m.health <= 0.0 {
                match m.kind {
                    Kind::EnderDragon => {
                        if end::dragon_dying(m, w, out) {
                            m.dead = true;
                        }
                    }
                    Kind::EnderCrystal => {
                        // EntityEnderCrystal.attackEntityFrom: a 6-strong fiery blast
                        if !m.dead {
                            blasts.push([m.pos[0], m.pos[1] + 1.0, m.pos[2]]);
                        }
                        m.dead = true;
                    }
                    _ => {
                        m.death_time += 1;
                        if m.death_time == 20 {
                            // EntityLivingBase.onDeathUpdate: the experience,
                            // unless a child (func_146066_aG)
                            if m.recently_hit > 0 && m.growing_age >= 0 {
                                let xp = m.xp(&mut w.rand);
                                if xp > 0 {
                                    out.orbs.push((m.pos, xp));
                                }
                            }
                        }
                        if m.death_time >= 20 {
                            m.dead = true;
                        }
                    }
                }
            }
            if m.dead {
                self.list.remove(i);
            } else {
                i += 1;
            }
        }
        for c in blasts {
            // EntityEnderCrystal: createExplosion(null, ..., 6.0F, true)
            let r = crate::world::explosion::Request {
                pos: c,
                size: 6.0,
                flaming: false,
                smoking: true,
                exploder: crate::world::explosion::Exploder::None,
                exclude: None,
            };
            explode(w, self, player, r, out);
        }
        for (pos, size, yaw) in splits {
            let id = self.next_id;
            self.next_id += 1;
            let seed = self.rand.next_long();
            let mut m = Mob::new(id, Kind::Slime, pos, yaw, seed);
            m.set_slime_size(size);
            self.list.push(m);
        }
        self.throwables.append(&mut out.thrown);
        tick_arrows(&mut self.arrows, w, player, out, &mut self.list);
        self.tick_throwables(w, player, out);
    }

    /// `attackEntityFrom(DamageSource.causePlayerDamage(player), damage)`
    /// on a mob (and the dragon's `attackEntityFromPart`); true when it
    /// took the hit.
    pub fn attack(
        &mut self,
        id: u32,
        damage: f32,
        attacker: [f64; 3],
        yaw: f32,
        out: &mut Outcome,
    ) -> bool {
        let _ = yaw;
        let Some(m) = self.list.iter_mut().find(|m| m.id == id) else {
            return false;
        };
        if m.health <= 0.0 {
            return false;
        }
        let mut dmg = damage;
        if m.kind == Kind::EnderDragon {
            // attackEntityFromPart: only the head takes full damage, and the
            // dragon breaks off to a point just ahead of itself
            let head = m.dragon_parts()[0].pos;
            let d = (head[0] - attacker[0]).powi(2) + (head[2] - attacker[2]).powi(2);
            if d > 25.0 {
                dmg = dmg / 4.0 + 1.0;
            }
            let yr = m.yaw.to_radians();
            m.waypoint = [
                m.pos[0]
                    + (math::sin(yr) * 5.0) as f64
                    + ((m.rand.next_float() - 0.5) * 2.0) as f64,
                m.pos[1] + (m.rand.next_float() * 3.0) as f64 + 1.0,
                m.pos[2] - (math::cos(yr) * 5.0) as f64
                    + ((m.rand.next_float() - 0.5) * 2.0) as f64,
            ];
            m.target_player = false;
        }
        if m.kind == Kind::EnderCrystal {
            dmg = 100.0;
        }
        if m.kind == Kind::PigZombie {
            // EntityPigZombie.attackEntityFrom: every pigman within 32 joins in
            let at = m.pos;
            for p in self.list.iter_mut() {
                if p.kind == Kind::PigZombie
                    && (p.pos[0] - at[0]).abs() <= 32.0
                    && (p.pos[1] - at[1]).abs() <= 32.0
                    && (p.pos[2] - at[2]).abs() <= 32.0
                {
                    p.anger = 400 + p.rand.next_int_n(400);
                    p.anger_sound = p.rand.next_int_n(40);
                }
            }
        }
        let Some(m) = self.list.iter_mut().find(|m| m.id == id) else {
            return false;
        };
        let hit = m.hurt(dmg, Some(attacker), true);
        if hit {
            m.recently_hit = 100;
            m.set_revenge(Some(ai::Target::Player));
            if ai::uses_old(m.kind) {
                m.attack_target = Some(ai::Target::Player);
            }
            if m.kind.hostile() {
                m.target_player = true;
            }
            let s = mob_sound(m, false);
            if !s.is_empty() {
                out.sounds.push((s, m.pos, 1.0, 1.0));
            }
            if matches!(m.kind, Kind::Wolf | Kind::IronGolem) && !m.tamed && !m.player_made {
                m.target_player = true;
            }
        }
        hit
    }

    /// `Entity.addVelocity` on a mob (the dragon's parts take it, not the
    /// dragon).
    pub fn add_velocity(&mut self, id: u32, v: [f64; 3]) {
        if let Some(m) = self.list.iter_mut().find(|m| m.id == id) {
            if m.kind != Kind::EnderDragon {
                for a in 0..3 {
                    m.motion[a] += v[a];
                }
            }
        }
    }

    /// `addPotionEffect` on a mob. Mobs do not keep potion effects yet
    /// (see HANDOFF); the call is where Bane of Arthropods' slowness lands.
    pub fn add_effect(&mut self, id: u32, potion: i32, duration: i32, amplifier: i32) {
        let _ = (id, potion, duration, amplifier);
    }

    /// Mob under the crosshair: nearest hit along the look ray within `reach`.
    pub fn ray(&self, eye: [f64; 3], dir: [f64; 3], reach: f64) -> Option<(u32, f64)> {
        let mut best: Option<(u32, f64)> = None;
        for m in &self.list {
            if m.health <= 0.0 {
                continue;
            }
            let b = Aabb {
                min: [m.bb.min[0] - 0.1, m.bb.min[1] - 0.1, m.bb.min[2] - 0.1],
                max: [m.bb.max[0] + 0.1, m.bb.max[1] + 0.1, m.bb.max[2] + 0.1],
            };
            if let Some(t) = ray_box(eye, dir, &b) {
                if t <= reach && best.map_or(true, |x| t < x.1) {
                    best = Some((m.id, t));
                }
            }
        }
        best
    }

    /// Right click on a mob (`EntityPlayer.interactWith` /
    /// `EntityLiving.interact`). Returns what the held stack turns into
    /// (`Some(None)` uses one up) and items that drop.
    pub fn interact(
        &mut self,
        id: u32,
        held: Option<Stack>,
        player: [f64; 3],
        out: &mut Outcome,
    ) -> Option<Option<Stack>> {
        let m = self
            .list
            .iter_mut()
            .find(|m| m.id == id && m.health > 0.0)?;
        let item = held.map(|s| s.id);
        let one_less = |s: Stack| {
            if s.count > 1 {
                Some(Stack {
                    count: s.count - 1,
                    ..s
                })
            } else {
                None
            }
        };
        let heart = |m: &mut Mob, out: &mut Outcome, tamed: bool| {
            let (wd, ht) = (m.width(), m.height());
            for _ in 0..7 {
                let r = &mut m.rand;
                let p = [
                    m.pos[0] + (r.next_float() * wd * 2.0 - wd) as f64,
                    m.pos[1] + 0.5 + (r.next_float() * ht) as f64,
                    m.pos[2] + (r.next_float() * wd * 2.0 - wd) as f64,
                ];
                let v = [
                    r.next_gaussian() * 0.02,
                    r.next_gaussian() * 0.02,
                    r.next_gaussian() * 0.02,
                ];
                out.particles
                    .push((if tamed { "heart" } else { "smoke" }, p, v));
            }
        };
        match (m.kind, item) {
            (Kind::Sheep, Some(359)) if !m.sheared && m.growing_age >= 0 => {
                // EntitySheep.onSheared
                m.sheared = true;
                let n = 1 + m.rand.next_int_n(3);
                for _ in 0..n {
                    out.drops.push((
                        [m.pos[0], m.pos[1] + 1.0, m.pos[2]],
                        Stack::new(block::WOOL as u16, m.variant, 1),
                    ));
                }
                out.sounds.push(("mob.sheep.shear", m.pos, 1.0, 1.0));
                out.wear = 1;
                Some(held)
            }
            (Kind::Creeper, Some(259)) => {
                // EntityCreeper.interact: flint and steel lights the fuse
                let pitch = m.rand.next_float() * 0.4 + 0.8;
                out.sounds.push((
                    "fire.ignite",
                    [m.pos[0] + 0.5, m.pos[1] + 0.5, m.pos[2] + 0.5],
                    1.0,
                    pitch,
                ));
                m.ignited = true;
                out.wear = 1;
                Some(held)
            }
            (Kind::Cow | Kind::Mooshroom, Some(325)) => {
                // EntityCow.interact: a bucket of milk
                let s = held.unwrap();
                if s.count > 1 {
                    out.drops.push((player, Stack::new(335, 0, 1)));
                    Some(one_less(s))
                } else {
                    Some(Some(Stack::new(335, 0, 1)))
                }
            }
            (Kind::Mooshroom, Some(281)) => {
                let s = held.unwrap();
                if s.count > 1 {
                    out.drops.push((player, Stack::new(282, 0, 1)));
                    Some(one_less(s))
                } else {
                    Some(Some(Stack::new(282, 0, 1)))
                }
            }
            (Kind::Wolf, Some(352)) if !m.tamed && !m.angry => {
                // EntityWolf.interact: a bone tames one time in three
                if m.rand.next_int_n(3) == 0 {
                    m.tamed = true;
                    m.sit_order = true;
                    m.clear_ai_target();
                    m.target_mob = None;
                    m.target_player = false;
                    m.health = 20.0;
                    m.variant = 14; // collar: red
                    heart(m, out, true);
                } else {
                    heart(m, out, false);
                }
                Some(one_less(held.unwrap()))
            }
            (Kind::Wolf, Some(351)) if m.tamed => {
                // dye recolours the collar
                let s = held.unwrap();
                m.variant = 15 - (s.meta & 15);
                Some(one_less(s))
            }
            (Kind::Wolf, Some(id))
                if m.tamed
                    && matches!(id, 319 | 320 | 363 | 364 | 365 | 366 | 367)
                    && m.health < 20.0 =>
            {
                let heal = match id {
                    320 | 364 => 8.0,
                    366 => 6.0,
                    363 | 319 => 3.0,
                    _ => 2.0,
                };
                m.health = (m.health + heal).min(20.0);
                Some(one_less(held.unwrap()))
            }
            (Kind::Ocelot, Some(349)) if !m.tamed => {
                let d = m.dist2(player);
                if d > 9.0 {
                    return None;
                }
                if m.rand.next_int_n(3) == 0 {
                    m.tamed = true;
                    m.sit_order = true;
                    m.variant = 1 + m.rand.next_int_n(3);
                    heart(m, out, true);
                } else {
                    heart(m, out, false);
                }
                Some(one_less(held.unwrap()))
            }
            // EntityAnimal.interact: its food puts a grown animal in love
            // for 600 ticks (func_146082_f)
            (_, Some(id)) if ai::breeding_item(m, id) && m.growing_age == 0 && m.in_love <= 0 => {
                m.in_love = 600;
                m.clear_ai_target();
                heart(m, out, true);
                Some(one_less(held.unwrap()))
            }
            (Kind::Wolf | Kind::Ocelot, _) if m.tamed => {
                // aiSit.setSitting(!isSitting()); the path and the target go
                m.sit_order = !m.sitting;
                m.path.clear();
                m.clear_ai_target();
                m.target_mob = None;
                None
            }
            _ => None,
        }
    }

    /// Removes mobs that may despawn when far away (`despawnEntity`).
    pub fn despawn(&mut self, player: [f64; 3]) {
        let rand = &mut self.rand;
        self.list.retain_mut(|m| {
            if m.persistent
                || matches!(m.kind, Kind::EnderDragon | Kind::EnderCrystal)
                || m.kind.animal()
                || m.kind == Kind::Villager
                || m.kind == Kind::IronGolem
                || m.tamed
            {
                return true;
            }
            let d = m.dist2(player);
            if d > 16384.0 {
                return false;
            }
            if m.age > 600 && rand.next_int_n(800) == 0 && d > 1024.0 {
                return false;
            }
            if d < 1024.0 {
                m.age = 0;
            }
            true
        });
    }

    pub fn count(&self, hostile: bool) -> usize {
        self.list
            .iter()
            .filter(|m| m.kind.hostile() == hostile && !m.kind.animal() == hostile)
            .count()
    }

    // ------------------------------------------------------------ save

    pub fn encode(&self) -> Vec<String> {
        self.list
            .iter()
            .filter(|m| m.health > 0.0)
            .map(|m| {
                // field 13: the equipment (`|` between slots, `-` empty),
                // field 14: canPickUpLoot and the drop chances
                let eq: Vec<String> = m
                    .equipment
                    .iter()
                    .map(|s| s.map_or_else(|| "-".to_string(), |s| s.to_code()))
                    .collect();
                let dc: Vec<String> = m.drop_chances.iter().map(|c| c.to_string()).collect();
                format!(
                    "{},{:?},{:?},{:?},{},{},{},{},{},{},{},{},{},{},{}|{}",
                    m.kind.id(),
                    m.pos[0],
                    m.pos[1],
                    m.pos[2],
                    m.yaw,
                    m.health,
                    m.variant,
                    m.sheared as i32,
                    m.persistent as i32,
                    m.tamed as i32 | (m.sitting as i32) << 1 | (m.player_made as i32) << 2,
                    m.carried,
                    m.carried_meta,
                    m.angry as i32,
                    eq.join("|"),
                    m.can_pick_up_loot as i32,
                    dc.join("|")
                )
            })
            .collect()
    }

    /// `EntityLiving.writeToNBT` for the region files.
    pub fn to_nbt(m: &Mob) -> crate::world::anvil::Nbt {
        use crate::world::anvil::{compound, list, Nbt};
        let mut v = vec![
            ("id", Nbt::String(m.kind.id().to_string())),
            (
                "Pos",
                list(vec![
                    Nbt::Double(m.pos[0]),
                    Nbt::Double(m.pos[1]),
                    Nbt::Double(m.pos[2]),
                ]),
            ),
            (
                "Motion",
                list(vec![
                    Nbt::Double(m.motion[0]),
                    Nbt::Double(m.motion[1]),
                    Nbt::Double(m.motion[2]),
                ]),
            ),
            (
                "Rotation",
                list(vec![Nbt::Float(m.yaw), Nbt::Float(m.pitch)]),
            ),
            ("HealF", Nbt::Float(m.health)),
            ("Health", Nbt::Short(m.health.ceil() as i16)),
            ("PersistenceRequired", Nbt::Byte(m.persistent as i8)),
        ];
        // EntityAgeable.writeEntityToNBT, EntityAnimal.writeEntityToNBT
        if m.kind.animal() || m.kind == Kind::Villager {
            v.push(("Age", Nbt::Int(m.growing_age)));
        }
        if m.kind.animal() {
            v.push(("InLove", Nbt::Int(m.in_love)));
        }
        match m.kind {
            Kind::Sheep => {
                v.push(("Color", Nbt::Byte(m.variant as i8)));
                v.push(("Sheared", Nbt::Byte(m.sheared as i8)));
            }
            Kind::Villager => v.push(("Profession", Nbt::Int(m.variant))),
            Kind::Skeleton => v.push(("SkeletonType", Nbt::Byte(m.variant as i8))),
            Kind::Slime | Kind::MagmaCube => v.push(("Size", Nbt::Int(m.variant - 1))),
            Kind::Enderman => {
                v.push(("carried", Nbt::Short(m.carried as i16)));
                v.push(("carriedData", Nbt::Short(m.carried_meta as i16)));
            }
            Kind::Wolf => {
                v.push(("Angry", Nbt::Byte(m.angry as i8)));
                v.push(("Sitting", Nbt::Byte(m.sitting as i8)));
                v.push(("CollarColor", Nbt::Byte(m.variant as i8)));
                v.push((
                    "Owner",
                    Nbt::String(if m.tamed {
                        "Player".into()
                    } else {
                        String::new()
                    }),
                ));
            }
            Kind::Ocelot => {
                v.push(("CatType", Nbt::Int(m.variant)));
                v.push(("Sitting", Nbt::Byte(m.sitting as i8)));
                v.push((
                    "Owner",
                    Nbt::String(if m.tamed {
                        "Player".into()
                    } else {
                        String::new()
                    }),
                ));
            }
            Kind::Horse => {
                v.push(("Type", Nbt::Int(m.variant >> 16)));
                v.push(("Variant", Nbt::Int(m.variant & 0xFFFF)));
            }
            Kind::IronGolem => v.push(("PlayerCreated", Nbt::Byte(m.player_made as i8))),
            Kind::PigZombie => v.push(("Anger", Nbt::Short(m.anger as i16))),
            _ => {}
        }
        // EntityLiving.writeEntityToNBT: CanPickUpLoot, Equipment, DropChances
        v.push(("CanPickUpLoot", Nbt::Byte(m.can_pick_up_loot as i8)));
        v.push((
            "Equipment",
            Nbt::List(
                10,
                m.equipment
                    .iter()
                    .map(|s| match s {
                        Some(s) => crate::world::anvil::item_nbt(None, s),
                        None => compound(vec![]),
                    })
                    .collect(),
            ),
        ));
        v.push((
            "DropChances",
            Nbt::List(5, m.drop_chances.iter().map(|&c| Nbt::Float(c)).collect()),
        ));
        compound(v)
    }

    /// Reads a living entity from a region file's `Entities` list.
    pub fn from_nbt(&mut self, t: &crate::world::anvil::Nbt) -> Option<u32> {
        use crate::world::anvil::Nbt;
        let kind = Kind::from_id(t.string("id")?)?;
        let pos = t.list("Pos");
        let p = |i: usize| match pos.get(i) {
            Some(Nbt::Double(v)) => Some(*v),
            _ => None,
        };
        let at = [p(0)?, p(1)?, p(2)?];
        let yaw = match t.list("Rotation").first() {
            Some(Nbt::Float(v)) => *v,
            _ => 0.0,
        };
        let mut m = self.make(kind, at, yaw);
        let id = m.id;
        m.growing_age = t.int("Age").unwrap_or(0) as i32;
        m.in_love = t.int("InLove").unwrap_or(0) as i32;
        if m.growing_age < 0 {
            m.set_position(at[0], at[1], at[2]);
        }
        match kind {
            Kind::Slime | Kind::MagmaCube => {
                m.set_slime_size(t.int("Size").unwrap_or(0) as i32 + 1)
            }
            Kind::Sheep => {
                m.variant = t.int("Color").unwrap_or(0) as i32;
                m.sheared = t.int("Sheared").unwrap_or(0) != 0;
            }
            Kind::Villager => m.variant = t.int("Profession").unwrap_or(0) as i32,
            Kind::Skeleton => {
                m.variant = t.int("SkeletonType").unwrap_or(0) as i32;
                m.set_position(at[0], at[1], at[2]);
            }
            Kind::Enderman => {
                m.carried = t.int("carried").unwrap_or(0) as u8;
                m.carried_meta = t.int("carriedData").unwrap_or(0) as u8;
            }
            Kind::Wolf => {
                m.angry = t.int("Angry").unwrap_or(0) != 0;
                m.sitting = t.int("Sitting").unwrap_or(0) != 0;
                m.sit_order = m.sitting;
                m.variant = t.int("CollarColor").unwrap_or(14) as i32;
                m.tamed = t.string("Owner").map_or(false, |o| !o.is_empty());
            }
            Kind::Ocelot => {
                m.variant = t.int("CatType").unwrap_or(0) as i32;
                m.sitting = t.int("Sitting").unwrap_or(0) != 0;
                m.sit_order = m.sitting;
                m.tamed = t.string("Owner").map_or(false, |o| !o.is_empty());
            }
            Kind::Horse => {
                m.variant = (t.int("Type").unwrap_or(0) as i32) << 16
                    | (t.int("Variant").unwrap_or(0) as i32 & 0xFFFF);
            }
            Kind::IronGolem => m.player_made = t.int("PlayerCreated").unwrap_or(0) != 0,
            Kind::PigZombie => m.anger = t.int("Anger").unwrap_or(0) as i32,
            _ => {}
        }
        if let Some(h) = t.float("HealF").or_else(|| t.float("Health")) {
            if h > 0.0 {
                m.health = h as f32;
            }
        }
        m.persistent = t.int("PersistenceRequired").unwrap_or(0) != 0;
        m.can_pick_up_loot = t.int("CanPickUpLoot").unwrap_or(0) != 0;
        for (k, e) in t.list("Equipment").iter().take(5).enumerate() {
            m.equipment[k] = crate::world::anvil::item_from_nbt(e);
        }
        for (k, c) in t.list("DropChances").iter().take(5).enumerate() {
            if let Nbt::Float(c) = c {
                m.drop_chances[k] = *c;
            }
        }
        self.list.push(m);
        Some(id)
    }

    pub fn decode(&mut self, line: &str) -> Option<()> {
        let f: Vec<&str> = line.split(',').collect();
        if f.len() < 9 {
            return None;
        }
        let kind = Kind::from_id(f[0])?;
        let pos = [f[1].parse().ok()?, f[2].parse().ok()?, f[3].parse().ok()?];
        let id = self.next_id;
        self.next_id += 1;
        let seed = self.rand.next_long();
        let mut m = Mob::new(id, kind, pos, f[4].parse().ok()?, seed);
        m.variant = f[6].parse().ok()?;
        if kind == Kind::Slime {
            m.set_slime_size(m.variant);
        }
        m.health = f[5].parse().ok()?;
        m.sheared = f[7] == "1";
        m.persistent = f[8] == "1";
        if f.len() >= 13 {
            let flags: i32 = f[9].parse().ok()?;
            m.tamed = flags & 1 != 0;
            m.sitting = flags & 2 != 0;
            m.sit_order = m.sitting;
            m.player_made = flags & 4 != 0;
            m.carried = f[10].parse().ok()?;
            m.carried_meta = f[11].parse().ok()?;
            m.angry = f[12] == "1";
        }
        if f.len() >= 15 {
            for (k, c) in f[13].split('|').take(5).enumerate() {
                m.equipment[k] = crate::item::Stack::from_code(c);
            }
            let mut g = f[14].split('|');
            m.can_pick_up_loot = g.next() == Some("1");
            for (k, c) in g.take(5).enumerate() {
                m.drop_chances[k] = c.parse().unwrap_or(0.085);
            }
        }
        self.list.push(m);
        Some(())
    }
}

/// Slab test of a ray against a box: entry distance.
pub fn ray_box(o: [f64; 3], d: [f64; 3], b: &Aabb) -> Option<f64> {
    let mut t0 = 0.0f64;
    let mut t1 = f64::INFINITY;
    for a in 0..3 {
        if d[a].abs() < 1e-12 {
            if o[a] < b.min[a] || o[a] > b.max[a] {
                return None;
            }
        } else {
            let (mut n, mut f) = ((b.min[a] - o[a]) / d[a], (b.max[a] - o[a]) / d[a]);
            if n > f {
                std::mem::swap(&mut n, &mut f);
            }
            t0 = t0.max(n);
            t1 = t1.min(f);
            if t0 > t1 {
                return None;
            }
        }
    }
    Some(t0)
}

pub fn hurt_sound(k: Kind) -> &'static str {
    match k {
        Kind::Pig => "mob.pig.say",
        Kind::Cow | Kind::Mooshroom => "mob.cow.hurt",
        Kind::Sheep => "mob.sheep.say",
        Kind::Chicken => "mob.chicken.hurt",
        Kind::Zombie => "mob.zombie.hurt",
        Kind::Skeleton => "mob.skeleton.hurt",
        Kind::Creeper => "mob.creeper.say",
        Kind::Spider | Kind::CaveSpider => "mob.spider.say",
        Kind::Villager => "mob.villager.hit",
        Kind::Witch => "mob.witch.hurt",
        Kind::Silverfish => "mob.silverfish.hit",
        Kind::Slime => "mob.slime.small",
        Kind::Enderman => "mob.endermen.hit",
        Kind::Squid => "",
        Kind::Bat => "mob.bat.hurt",
        Kind::Wolf => "mob.wolf.hurt",
        Kind::Ocelot => "mob.cat.hitt",
        Kind::Horse => "mob.horse.hit",
        Kind::IronGolem => "mob.irongolem.hit",
        Kind::Ghast => "mob.ghast.scream",
        Kind::PigZombie => "mob.zombiepig.zpighurt",
        Kind::Blaze => "mob.blaze.hit",
        Kind::MagmaCube => "mob.slime.small",
        Kind::EnderDragon => "mob.enderdragon.hit",
        Kind::EnderCrystal => "",
    }
}

pub fn death_sound(k: Kind) -> &'static str {
    match k {
        Kind::Pig => "mob.pig.death",
        Kind::Cow | Kind::Mooshroom => "mob.cow.hurt",
        Kind::Sheep => "mob.sheep.say",
        Kind::Chicken => "mob.chicken.hurt",
        Kind::Zombie => "mob.zombie.death",
        Kind::Skeleton => "mob.skeleton.death",
        Kind::Creeper => "mob.creeper.death",
        Kind::Spider | Kind::CaveSpider => "mob.spider.death",
        Kind::Villager => "mob.villager.death",
        Kind::Witch => "mob.witch.death",
        Kind::Silverfish => "mob.silverfish.kill",
        Kind::Slime => "mob.slime.small",
        Kind::Enderman => "mob.endermen.death",
        Kind::Squid => "",
        Kind::Bat => "mob.bat.death",
        Kind::Wolf => "mob.wolf.death",
        Kind::Ocelot => "mob.cat.hitt",
        Kind::Horse => "mob.horse.death",
        Kind::IronGolem => "mob.irongolem.death",
        Kind::Ghast => "mob.ghast.death",
        Kind::PigZombie => "mob.zombiepig.zpigdeath",
        Kind::Blaze => "mob.blaze.death",
        Kind::MagmaCube => "mob.slime.small",
        Kind::EnderDragon => "mob.enderdragon.end",
        Kind::EnderCrystal => "",
    }
}

/// Hurt or death sound of this particular mob (slime size, donkeys).
pub fn mob_sound(m: &Mob, death: bool) -> &'static str {
    match m.kind {
        Kind::Slime | Kind::MagmaCube if m.variant > 1 => "mob.slime.big",
        Kind::Horse if (m.variant >> 16) == 1 => {
            if death {
                "mob.horse.donkey.death"
            } else {
                "mob.horse.donkey.hit"
            }
        }
        k if death => death_sound(k),
        k => hurt_sound(k),
    }
}

pub fn idle_sound(k: Kind) -> Option<&'static str> {
    Some(match k {
        Kind::Pig => "mob.pig.say",
        Kind::Cow | Kind::Mooshroom => "mob.cow.say",
        Kind::Sheep => "mob.sheep.say",
        Kind::Chicken => "mob.chicken.say",
        Kind::Zombie => "mob.zombie.say",
        Kind::Skeleton => "mob.skeleton.say",
        Kind::Spider | Kind::CaveSpider => "mob.spider.say",
        Kind::Villager => "mob.villager.idle",
        Kind::Witch => "mob.witch.idle",
        Kind::Silverfish => "mob.silverfish.say",
        Kind::Enderman => "mob.endermen.idle",
        Kind::Bat => "mob.bat.idle",
        Kind::Wolf => "mob.wolf.bark",
        Kind::Horse => "mob.horse.idle",
        Kind::Ghast => "mob.ghast.moan",
        Kind::PigZombie => "mob.zombiepig.zpig",
        Kind::Blaze => "mob.blaze.breathe",
        Kind::EnderDragon => "mob.enderdragon.growl",
        Kind::Creeper
        | Kind::Slime
        | Kind::Squid
        | Kind::Ocelot
        | Kind::IronGolem
        | Kind::MagmaCube
        | Kind::EnderCrystal => return None,
    })
}

/// `getLivingSound` of this mob, with the state-dependent cases.
fn idle_name(m: &mut Mob) -> Option<&'static str> {
    Some(match m.kind {
        Kind::Enderman if m.angry => "mob.endermen.scream",
        Kind::Bat if m.hanging && m.rand.next_int_n(4) != 0 => return None,
        Kind::Wolf => {
            if m.angry {
                "mob.wolf.growl"
            } else if m.rand.next_int_n(3) == 0 {
                if m.tamed && m.health < 10.0 {
                    "mob.wolf.whine"
                } else {
                    "mob.wolf.panting"
                }
            } else {
                "mob.wolf.bark"
            }
        }
        Kind::Ocelot if m.tamed => {
            if m.rand.next_int_n(4) == 0 {
                "mob.cat.purreow"
            } else {
                "mob.cat.meow"
            }
        }
        Kind::Horse if (m.variant >> 16) == 1 => "mob.horse.donkey.idle",
        k => return idle_sound(k),
    })
}

#[allow(clippy::too_many_arguments)]
fn tick_mob(
    m: &mut Mob,
    w: &mut ServerWorld,
    player: &PlayerInfo,
    day: bool,
    villagers: &[[f64; 3]],
    zombies: &[[f64; 3]],
    out: &mut Outcome,
    arrows: &mut Vec<Arrow>,
    explosions: &mut Vec<crate::world::explosion::Request>,
    others: &[Other],
    strikes: &mut Vec<Strike>,
    balls: &mut Vec<Fireball>,
) {
    // EntityAgeable.onLivingUpdate
    if m.growing_age < 0 {
        m.growing_age += 1;
        if m.growing_age == 0 {
            // setScaleForAge: grown up
            let p = m.pos;
            m.set_position(p[0], p[1], p[2]);
        }
    } else if m.growing_age > 0 {
        m.growing_age -= 1;
    }
    // EntityAnimal.onLivingUpdate: love lasts while fully grown
    if m.growing_age != 0 {
        m.in_love = 0;
    }
    if m.in_love > 0 {
        m.in_love -= 1;
        if m.in_love % 10 == 0 {
            let v = [
                m.rand.next_gaussian() * 0.02,
                m.rand.next_gaussian() * 0.02,
                m.rand.next_gaussian() * 0.02,
            ];
            let (wd, ht) = m.dims();
            let p = [
                m.pos[0] + (m.rand.next_float() * wd * 2.0) as f64 - wd as f64,
                m.pos[1] + 0.5 + (m.rand.next_float() * ht) as f64,
                m.pos[2] + (m.rand.next_float() * wd * 2.0) as f64 - wd as f64,
            ];
            out.particles.push(("heart", p, v));
        }
    }
    m.prev_pos = m.pos;
    m.prev_body_yaw = m.body_yaw;
    m.prev_head_yaw = m.head_yaw;
    m.prev_pitch = m.pitch;
    m.prev_swing = m.swing;
    m.prev_fuse = m.fuse;
    m.prev_wing = m.wing;
    m.ticks += 1;
    m.age += 1;
    if m.hurt_time > 0 {
        m.hurt_time -= 1;
    }
    if m.hurt_resistant > 0 {
        m.hurt_resistant -= 1;
    }
    if m.recently_hit > 0 {
        m.recently_hit -= 1;
    }
    if m.attack_cooldown > 0 {
        m.attack_cooldown -= 1;
    }
    // swing progress (getArmSwingAnimationEnd = 6)
    if m.is_swinging {
        m.swing_ticks += 1;
        if m.swing_ticks >= 6 {
            m.swing_ticks = 0;
            m.is_swinging = false;
        }
    } else {
        m.swing_ticks = 0;
    }
    m.swing = m.swing_ticks as f32 / 6.0;
    if m.health <= 0.0 {
        m.move_forward = 0.0;
        m.move_strafe = 0.0;
        let mm = m.motion;
        m.move_with_heading(w, 0.0, 0.0);
        let _ = mm;
        return;
    }
    // water / lava / fire
    m.in_water = m.in_material(w, &m.bb.offset_copy(0.0, 0.0, 0.0), Material::Water);
    if m.in_water {
        m.fall_distance = 0.0;
        m.fire = 0;
    }
    if m.in_material(w, &m.bb, Material::Lava) && !m.kind.fireproof() {
        // DamageSource.lava is blockable
        m.hurt(4.0, None, true);
        m.fire = 300;
    }
    if m.kind.fireproof() {
        m.fire = 0;
    }
    if m.fire > 0 {
        if m.fire % 20 == 0 {
            m.damage(1.0, None);
        }
        m.fire -= 1;
    }
    // undead burn in daylight
    if day && (m.kind == Kind::Zombie || m.kind == Kind::Skeleton && m.variant != 1) {
        let (x, y, z) = m.feet_block();
        let f = brightness(w, x, y, z);
        if f > 0.5 && m.rand.next_float() * 30.0 < (f - 0.4) * 2.0 && w.can_see_sky(x, y, z) {
            m.fire = m.fire.max(160);
        }
    }
    // chickens
    if m.kind == Kind::Chicken {
        m.wing_dest += if m.on_ground { -1.0 } else { 4.0 } * 0.3;
        m.wing_dest = m.wing_dest.clamp(0.0, 1.0);
        if !m.on_ground && m.wing_speed < 1.0 {
            m.wing_speed = 1.0;
        }
        m.wing_speed *= 0.9;
        if !m.on_ground && m.motion[1] < 0.0 {
            m.motion[1] *= 0.6;
        }
        m.wing += m.wing_speed * 2.0;
        m.egg_timer -= 1;
        if m.egg_timer <= 0 {
            out.drops.push((m.pos, Stack::new(344, 0, 1)));
            out.sounds.push((
                "mob.chicken.plop",
                m.pos,
                1.0,
                (m.rand.next_float() - m.rand.next_float()) * 0.2 + 1.0,
            ));
            m.egg_timer = m.rand.next_int_n(6000) + 6000;
        }
    }
    // idle sounds (getTalkInterval 80, livingSoundTime)
    if m.rand.next_int_n(1000) < m.ticks % 80 && m.rand.next_int_n(80) == 0 {
        if let Some(s) = idle_name(m) {
            out.sounds.push((
                s,
                m.pos,
                1.0,
                (m.rand.next_float() - m.rand.next_float()) * 0.2 + 1.0,
            ));
        }
    }

    // Squid and bats move on their own.
    if m.kind == Kind::Squid {
        extra::squid(m, w);
        return;
    }
    if m.kind == Kind::Bat {
        extra::bat(m, w, player, out);
        return;
    }
    if m.kind == Kind::Ghast {
        nether::ghast(m, w, player, balls, out);
        return;
    }
    if m.kind == Kind::EnderCrystal {
        end::crystal(m, w);
        return;
    }
    if m.kind == Kind::EnderDragon {
        let crystals: Vec<(u32, [f64; 3])> = others
            .iter()
            .filter(|o| o.kind == Kind::EnderCrystal)
            .map(|o| (o.id, o.pos))
            .collect();
        end::dragon(m, w, player, &crystals, out);
        return;
    }

    // ---------------------------------------------------------------- AI
    let tasked = ai::uses_tasks(m.kind);
    if tasked {
        // EntityLiving.updateAITasks
        let mut cx = ai::Cx {
            w: &mut *w,
            player,
            others,
            out: &mut *out,
            arrows: &mut *arrows,
            strikes: &mut *strikes,
            day,
        };
        ai::update(m, &mut cx);
        if m.kind == Kind::Creeper {
            creeper_fuse(m, out, explosions);
        }
    }
    let old = ai::uses_old(m.kind);
    if old {
        // EntityCreature.updateEntityActionState
        let mut cx = ai::Cx {
            w: &mut *w,
            player,
            others,
            out: &mut *out,
            arrows: &mut *arrows,
            strikes: &mut *strikes,
            day,
        };
        ai::old_update(m, &mut cx);
    }
    if !tasked && !old {
        let pdist2 = m.dist2(player.feet);
        let sees_player = player.attackable
            && pdist2 < (m.kind.follow_range() as f64).powi(2)
            && m.can_see(w, player.eye());
        // targeting
        if m.kind == Kind::PigZombie {
            // EntityPigZombie: neutral until angered
            if m.anger_sound > 0 {
                m.anger_sound -= 1;
                if m.anger_sound == 0 {
                    let p = ((m.rand.next_float() - m.rand.next_float()) * 0.2 + 1.0) * 1.8;
                    out.sounds.push(("mob.zombiepig.zpigangry", m.pos, 2.0, p));
                }
            }
            if m.anger > 0 {
                m.anger -= 1;
            }
            m.target_player = m.anger > 0 && player.attackable;
        }
        if m.kind.hostile()
            && !matches!(
                m.kind,
                Kind::Enderman | Kind::Slime | Kind::PigZombie | Kind::EnderDragon
            )
        {
            let spider_calm = matches!(m.kind, Kind::Spider | Kind::CaveSpider) && {
                let (x, y, z) = m.feet_block();
                brightness(w, x, y, z) > 0.5
            };
            if !player.attackable {
                m.target_player = false;
            } else if !m.target_player && sees_player && !spider_calm && pdist2 < 16.0 * 16.0 {
                m.target_player = true;
            } else if m.target_player && pdist2 > (m.kind.follow_range() as f64).powi(2) {
                m.target_player = false;
            }
            if spider_calm && m.target_player && m.rand.next_int_n(100) == 0 {
                m.target_player = false;
            }
        }
        // swimming (EntityAISwimming)
        if (m.in_water || m.in_material(w, &m.bb, Material::Lava)) && m.rand.next_float() < 0.8 {
            m.jumping = true;
        }
        match m.kind {
            Kind::Zombie | Kind::Spider | Kind::CaveSpider | Kind::Silverfish => {
                if m.target_player {
                    chase(m, w, player, out);
                } else if m.kind == Kind::Zombie && !villagers.is_empty() {
                    // EntityAIAttackOnCollide against villagers
                    if let Some(v) = villagers
                        .iter()
                        .copied()
                        .filter(|v| m.dist2(*v) < 16.0 * 16.0)
                        .min_by(|a, b| m.dist2(*a).partial_cmp(&m.dist2(*b)).unwrap())
                    {
                        m.look_at = Some([v[0], v[1] + 1.62, v[2]]);
                        if m.no_path() || m.ticks % 20 == 0 {
                            m.path_to(w, v[0], v[1], v[2], 1.0);
                        }
                    } else {
                        wander(m, w, 1.0, 120);
                    }
                } else {
                    wander(m, w, 1.0, 120);
                }
            }
            Kind::Skeleton if m.variant == 1 => {
                if m.target_player {
                    chase(m, w, player, out);
                } else {
                    wander(m, w, 1.0, 120);
                }
            }
            Kind::Skeleton | Kind::Witch => {
                if m.target_player {
                    ranged(m, w, player, arrows, out);
                } else {
                    if m.kind == Kind::Skeleton && day && m.fire > 0 {
                        // EntityAIFleeSun
                        if m.no_path() {
                            if let Some(t) = m.random_target(w, 10, 3, None) {
                                if !w.can_see_sky(t[0] as i32, t[1] as i32, t[2] as i32) {
                                    m.path_to(w, t[0], t[1], t[2], 1.0);
                                }
                            }
                        }
                    }
                    wander(m, w, 1.0, 120);
                }
            }
            Kind::Villager => {
                // EntityAIAvoidEntity(zombie, 8)
                if let Some(z) = zombies.iter().copied().find(|z| m.dist2(*z) < 64.0) {
                    if m.task != Task::Flee || m.no_path() {
                        if let Some(t) = m.random_target(w, 16, 7, Some(z)) {
                            m.path_to(w, t[0], t[1], t[2], 0.6);
                            m.task = Task::Flee;
                        }
                    }
                } else {
                    if m.task == Task::Flee {
                        m.task = Task::Idle;
                    }
                    if pdist2 < 9.0 && m.rand.next_float() < 0.02 {
                        m.look_at = Some(player.eye());
                    }
                    wander(m, w, 0.6, 120);
                }
            }
            Kind::Slime | Kind::MagmaCube => extra::slime(m, player, out),
            Kind::PigZombie => {
                if m.target_player {
                    chase(m, w, player, out);
                } else {
                    wander(m, w, 1.0, 120);
                }
            }
            Kind::Blaze => {
                nether::blaze(m, w, player, balls, out);
                if m.target_player {
                    m.look_at = Some(player.eye());
                    if m.no_path() || m.ticks % 20 == 0 {
                        m.path_to(w, player.feet[0], player.feet[1], player.feet[2], 1.0);
                    }
                } else {
                    wander(m, w, 1.0, 120);
                }
            }
            Kind::Enderman => extra::enderman(m, w, player, day, out),
            Kind::Horse => extra::horse(m, w, player, out),
            _ => animal(m, w, player, out),
        }
        // look at the player now and then (EntityAIWatchClosest)
        if m.look_at.is_none() && pdist2 < 64.0 && m.task != Task::Panic {
            if m.rand.next_float() < 0.02 {
                m.task_ticks = 40 + m.rand.next_int_n(40);
            }
            if m.task_ticks > 0 && matches!(m.task, Task::Idle) {
                m.look_at = Some(player.eye());
                m.task_ticks -= 1;
            }
        }
        if !matches!(m.kind, Kind::Slime | Kind::MagmaCube) {
            m.follow_path();
            m.move_helper();
        }
        m.look_helper();
    }
    if !m.kind.old_ai() {
        m.body_helper();
    } else {
        // the old AI turns the head with the body (rotationYawHead)
        m.head_yaw = m.yaw;
    }
    // jumping (EntityJumpHelper / onLivingUpdate)
    if m.jumping {
        if m.in_water || m.in_material(w, &m.bb, Material::Lava) {
            m.motion[1] += 0.04f32 as f64;
        } else if m.on_ground && m.jump_ticks == 0 {
            m.jump();
            m.jump_ticks = 10;
        }
    } else {
        m.jump_ticks = 0;
    }
    if m.jump_ticks > 0 {
        m.jump_ticks -= 1;
    }
    m.jumping = false;
    m.move_strafe *= 0.98;
    m.move_forward *= 0.98;
    let (s, f) = (m.move_strafe, m.move_forward);
    let was_on_ground = m.on_ground;
    m.move_with_heading(w, s, f);
    if matches!(m.kind, Kind::Slime | Kind::MagmaCube) {
        extra::slime_landed(m, was_on_ground, out);
    }
    // push out of the player (Entity.applyEntityCollision, mobs side only)
    let pb = player.bb();
    if m.bb.intersects(&pb) {
        let (dx, dz) = (m.pos[0] - player.feet[0], m.pos[2] - player.feet[2]);
        let d = dx.abs().max(dz.abs());
        if d >= 0.01 {
            let k = 0.05 / d.sqrt().max(1.0);
            m.motion[0] += dx / d * k;
            m.motion[2] += dz / d * k;
        }
    }
}

/// `EntityAIWander` (one chance in `chance` per tick).
fn wander(m: &mut Mob, w: &mut ServerWorld, speed: f64, chance: i32) {
    if m.task == Task::Panic || m.task == Task::Flee {
        return;
    }
    if m.no_path() {
        if m.task == Task::Wander {
            m.task = Task::Idle;
        }
        if m.age < 100 && m.rand.next_int_n(chance) == 0 {
            if let Some(t) = m.random_target(w, 10, 7, None) {
                if m.path_to(w, t[0], t[1], t[2], speed) {
                    m.task = Task::Wander;
                }
            }
        } else if m.rand.next_int_n(chance) == 0 {
            if let Some(t) = m.random_target(w, 10, 7, None) {
                if m.path_to(w, t[0], t[1], t[2], speed) {
                    m.task = Task::Wander;
                }
            }
        }
        // EntityAILookIdle
        if m.look_at.is_none() && m.rand.next_float() < 0.02 {
            let a = std::f64::consts::PI * 2.0 * m.rand.next_double();
            m.look_at = Some([
                m.pos[0] + a.cos(),
                m.pos[1] + m.kind.eye_height() as f64,
                m.pos[2] + a.sin(),
            ]);
        }
    }
}

/// Passive animal AI: panic, tempt, eat grass, wander.
fn animal(m: &mut Mob, w: &mut ServerWorld, player: &PlayerInfo, out: &mut Outcome) {
    let (panic, tempt, item): (f64, f64, &[u16]) = match m.kind {
        Kind::Pig => (1.25, 1.2, &[391, 398]),
        Kind::Cow | Kind::Mooshroom => (2.0, 1.25, &[296]),
        Kind::Sheep => (1.25, 1.1, &[296]),
        Kind::Chicken => (1.4, 1.0, &[295]),
        _ => (1.25, 1.0, &[]),
    };
    if m.task == Task::Panic {
        m.task_ticks += 1;
        if m.no_path() {
            if m.task_ticks > 60 || m.hurt_time == 0 && m.task_ticks > 20 {
                m.task = Task::Idle;
            } else if let Some(t) = m.random_target(w, 5, 4, None) {
                m.path_to(w, t[0], t[1], t[2], panic);
            }
        }
        return;
    }
    // EntityAITempt
    let pdist2 = m.dist2(player.feet);
    if pdist2 < 100.0 && player.held.map_or(false, |h| item.contains(&h)) {
        m.task = Task::Tempt;
        m.look_at = Some(player.eye());
        if pdist2 < 6.25 {
            m.path.clear();
        } else if m.ticks % 10 == 0 || m.no_path() {
            m.path_to(w, player.feet[0], player.feet[1], player.feet[2], tempt);
        }
        return;
    }
    if m.task == Task::Tempt {
        m.task = Task::Idle;
        m.path.clear();
    }
    // EntityAIEatGrass
    if m.kind == Kind::Sheep {
        if let Task::EatGrass(t) = m.task {
            let t = t - 1;
            m.eat_timer = t;
            if t == 4 {
                let (x, y, z) = m.feet_block();
                if w.block(x, y, z) == block::TALLGRASS {
                    w.set_block(x, y, z, block::AIR, 0, 3);
                    m.sheared = false;
                } else if w.block(x, y - 1, z) == block::GRASS {
                    w.set_block(x, y - 1, z, block::DIRT, 0, 2);
                    m.sheared = false;
                }
            }
            m.task = if t <= 0 {
                Task::Idle
            } else {
                Task::EatGrass(t)
            };
            return;
        }
        if m.no_path() && m.rand.next_int_n(1000) == 0 {
            let (x, y, z) = m.feet_block();
            let tall = w.block(x, y, z) == block::TALLGRASS && w.meta(x, y, z) == 1;
            if tall || w.block(x, y - 1, z) == block::GRASS {
                m.task = Task::EatGrass(40);
                m.eat_timer = 40;
                return;
            }
        }
    }
    let _ = out;
    wander(m, w, 1.0, 120);
}

/// `EntityAIAttackOnCollide` against the player (also spiders' leap).
fn chase(m: &mut Mob, w: &mut ServerWorld, player: &PlayerInfo, out: &mut Outcome) {
    m.look_at = Some(player.eye());
    m.task = Task::Attack;
    let d2 = m.dist2(player.feet);
    if m.no_path() || m.ticks % (4 + m.rand.next_int_n(7)).max(1) == 0 {
        if !m.path_to(w, player.feet[0], player.feet[1], player.feet[2], 1.0) {
            // walk straight at the player when no path is found
            m.move_to = Some(player.feet);
            m.move_speed = 1.0;
        }
    }
    if matches!(m.kind, Kind::Spider | Kind::CaveSpider)
        && d2 > 4.0
        && d2 < 36.0
        && m.on_ground
        && m.rand.next_int_n(10) == 0
    {
        // EntitySpider.attackEntity: leap
        let (dx, dz) = (player.feet[0] - m.pos[0], player.feet[2] - m.pos[2]);
        let f = (dx * dx + dz * dz).sqrt();
        m.motion[0] = dx / f * 0.5 * 0.8 + m.motion[0] * 0.2;
        m.motion[2] = dz / f * 0.5 * 0.8 + m.motion[2] * 0.2;
        m.motion[1] = 0.4f32 as f64;
    }
    let (w2, _) = m.dims();
    let reach = (w2 * 2.0 * w2 * 2.0 + 0.6) as f64;
    let pb = player.bb();
    let in_y = m.bb.max[1] > pb.min[1] && m.bb.min[1] < pb.max[1];
    if m.attack_cooldown <= 0 && d2 <= reach.max(2.0) && in_y {
        m.attack_cooldown = 20;
        m.is_swinging = true;
        m.swing_ticks = 0;
        let (dx, dz) = (player.feet[0] - m.pos[0], player.feet[2] - m.pos[2]);
        let f = (dx * dx + dz * dz).sqrt().max(1e-4);
        let knock = [dx / f * 0.4, 0.4, dz / f * 0.4];
        let mut poison = if m.kind == Kind::CaveSpider {
            7 * 20
        } else {
            0
        };
        let mut amount = m.kind.attack_damage();
        if m.kind == Kind::Skeleton && m.variant == 1 {
            // stone sword, and the wither effect for 10 seconds
            amount = 2.0 + 5.0;
            poison = 200;
        }
        if m.kind == Kind::PigZombie {
            // golden sword
            amount += 4.0;
        }
        let effect = match poison {
            0 => None,
            200 => Some((crate::potion::WITHER, 200)),
            _ => Some((crate::potion::POISON, -1)),
        };
        let _ = knock;
        out.hits.push(Hit::Melee {
            kind: m.kind,
            by: m.id,
            fire: 0,
            amount,
            knock: [0.0; 3],
            from: m.pos,
            effect,
        });
        out.attacker = Some(m.id);
    }
}

/// `EntityAIArrowAttack`: skeleton arrows, witch potions (as damage).
fn ranged(
    m: &mut Mob,
    w: &mut ServerWorld,
    player: &PlayerInfo,
    arrows: &mut Vec<Arrow>,
    out: &mut Outcome,
) {
    m.look_at = Some(player.eye());
    m.task = Task::Attack;
    let d2 = m.dist2(player.feet);
    let max = if m.kind == Kind::Skeleton { 15.0 } else { 10.0 };
    let sees = m.can_see(w, player.eye());
    if d2 <= max * max && sees {
        m.path.clear();
    } else if m.no_path() || m.ticks % 20 == 0 {
        m.path_to(w, player.feet[0], player.feet[1], player.feet[2], 1.0);
    }
    if m.attack_cooldown <= 0 && sees && d2 <= max * max {
        let f = (d2.sqrt() / max) as f32;
        let (lo, hi) = if m.kind == Kind::Skeleton {
            (20.0, 60.0)
        } else {
            (60.0, 60.0)
        };
        m.attack_cooldown = (f * (hi - lo) + lo).floor() as i32;
        if m.kind == Kind::Skeleton {
            // EntitySkeleton.attackEntityWithRangedAttack
            let k = f.clamp(0.1, 1.0);
            let diff = player.difficulty;
            let id = m.rand.next_int() as u32;
            // posY + getEyeHeight (height * 0.85)
            let eye = m.pos[1] + (m.dims().1 * 0.85) as f64;
            let mut a = Arrow::at_target(
                id,
                super::arrow::Shooter::Mob(m.id),
                m.pos,
                eye,
                player.feet,
                player.feet[1],
                1.8,
                1.6,
                (14 - diff * 4) as f32,
            );
            a.damage =
                (k * 2.0) as f64 + (m.rand.next_gaussian() * 0.25 + (diff as f32 * 0.11f32) as f64);
            m.bow_enchantments(&mut a);
            arrows.push(a);
            out.sounds.push((
                "random.bow",
                m.pos,
                1.0,
                1.0 / (m.rand.next_float() * 0.4 + 0.8),
            ));
        } else {
            // EntityWitch.attackEntityWithRangedAttack
            let id = m.rand.next_int() as u32;
            let eye = m.pos[1] + (m.dims().1 * 0.85) as f64;
            let mut t = super::throwable::Throwable::thrown(
                id,
                super::throwable::Kind::Potion(32732),
                super::arrow::Shooter::Mob(m.id),
                m.pos,
                eye,
                m.yaw,
                0.0,
            );
            let dx = player.feet[0] + player.motion[0] - m.pos[0];
            let dy = player.feet[1] + 0.12f32 as f64 - 1.1f32 as f64 - m.pos[1];
            let dz = player.feet[2] + player.motion[2] - m.pos[2];
            let d = math::sqrt_double(dx * dx + dz * dz);
            let has = |id: i32| player.effects & 1 << id != 0;
            let mut meta = 32732;
            if d >= 8.0 && !has(crate::potion::MOVE_SLOWDOWN) {
                meta = 32698;
            } else if player.health >= 8.0 && !has(crate::potion::POISON) {
                meta = 32660;
            } else if d <= 3.0 && !has(crate::potion::WEAKNESS) && m.rand.next_float() < 0.25 {
                meta = 32696;
            }
            t.kind = super::throwable::Kind::Potion(meta);
            t.set_heading(dx, dy + (d * 0.2) as f64, dz, 0.75, 8.0);
            out.thrown.push(t);
        }
    }
}

/// `EntityCreeper.onUpdate`: the fuse follows `getCreeperState()`, which
/// `EntityAICreeperSwell` sets.
fn creeper_fuse(
    m: &mut Mob,
    out: &mut Outcome,
    explosions: &mut Vec<crate::world::explosion::Request>,
) {
    // EntityCreeper.onUpdate
    if m.ignited {
        m.fuse_dir = 1;
    }
    if m.fuse_dir > 0 && m.fuse == 0 {
        out.sounds.push(("creeper.primed", m.pos, 1.0, 0.5));
    }
    m.fuse += m.fuse_dir;
    if m.fuse < 0 {
        m.fuse = 0;
    }
    if m.fuse >= 30 {
        m.fuse = 30;
        // the creeper is the exploder (getExplosivePlacedBy)
        explosions.push(crate::world::explosion::Request {
            pos: m.pos,
            size: 3.0,
            flaming: false,
            smoking: true,
            exploder: crate::world::explosion::Exploder::Living(m.pos),
            exclude: Some(m.id),
        });
        m.health = 0.0;
        m.death_time = 19;
        m.dead = true;
    }
}

/// `EntityArrow.onUpdate` for every arrow: what it hits takes
/// `attackEntityFrom(causeArrowDamage(arrow, shooter), damage)`.
fn tick_arrows(
    arrows: &mut Vec<Arrow>,
    w: &mut ServerWorld,
    player: &PlayerInfo,
    out: &mut Outcome,
    mobs: &mut [Mob],
) {
    use super::arrow::{Shooter, Target};
    use super::base::Fx;
    let mut targets: Vec<(Target, Aabb)> = Vec::new();
    if player.attackable {
        targets.push((Target::Player, player.bb()));
    }
    for m in mobs.iter() {
        if m.health > 0.0 {
            targets.push((Target::Mob(m.id), m.bb));
        }
    }
    let mut ar = std::mem::take(arrows);
    for a in ar.iter_mut() {
        let mut fx = Vec::new();
        let pending = a.fly(&*w as &dyn crate::world::BlockView, &targets, &mut fx);
        let mut result = None;
        let mut enderman = false;
        if let Some(h) = pending {
            let from = match a.shooter {
                Shooter::Mob(id) => mobs.iter().find(|m| m.id == id).map(|m| m.pos),
                _ => None,
            };
            match h.target {
                Target::Player => {
                    let ok = player.accepts(h.damage as f32, matches!(a.shooter, Shooter::Mob(_)));
                    if h.burning {
                        out.hits.push(Hit::Burn(5 * 20));
                    }
                    if ok {
                        out.hits.push(Hit::Arrow {
                            amount: h.damage as f32,
                            knock: [0.0; 3],
                            shooter: match a.shooter {
                                Shooter::Mob(id) => Some(id),
                                _ => None,
                            },
                        });
                    }
                    result = Some(ok);
                }
                Target::Mob(id) => {
                    if let Some(m) = mobs.iter_mut().find(|m| m.id == id) {
                        enderman = m.kind == Kind::Enderman;
                        if h.burning && !enderman {
                            m.fire = m.fire.max(5 * 20);
                        }
                        let src = match a.shooter {
                            Shooter::Player => Some(player.feet),
                            _ => from,
                        };
                        let ok = m.hurt(h.damage as f32, src, true);
                        if ok {
                            match a.shooter {
                                Shooter::Player => {
                                    // attackEntityFrom: the source's entity is
                                    // a player
                                    m.recently_hit = 100;
                                    m.set_revenge(Some(ai::Target::Player));
                                    m.looting = player.looting;
                                }
                                Shooter::Mob(id) => m.set_revenge(Some(ai::Target::Mob(id))),
                                _ => {}
                            }
                        }
                        result = Some(ok);
                    }
                }
            }
        }
        let push = a.finish(
            &*w as &dyn crate::world::BlockView,
            result,
            enderman,
            &mut fx,
        );
        if let (Some(v), Some(Target::Mob(id))) = (push, pending.map(|p| p.target)) {
            if let Some(m) = mobs.iter_mut().find(|m| m.id == id) {
                for i in 0..3 {
                    m.motion[i] += v[i];
                }
            }
        }
        for f in fx {
            match f {
                Fx::Sound(n, p, v, pi) => out.sounds.push((n, p, v, pi)),
                Fx::Particle(n, p, mo) => out.particles.push((n, p, mo)),
                Fx::Collide(x, y, z, id) => {
                    // BlockTNT.onEntityCollidedWithBlock: a burning arrow lights it
                    if id == block::TNT && a.e.fire > 0 {
                        // ignite(world, x, y, z, 1, shooter): EntityTNTPrimed, fuse 80
                        w.prime_tnt(x, y, z, 80);
                    }
                }
                _ => {}
            }
        }
    }
    ar.retain(|a| !a.e.dead);
    *arrows = ar;
}

/// `Block.getExplosionResistance(null)`: `blockResistance / 5`.
pub fn explosion_resistance(id: u8) -> f32 {
    use crate::block::*;
    let explicit = match id {
        STONE | COBBLESTONE | STONE_SLAB | DOUBLE_STONE_SLAB | BRICK_BLOCK | MOSSY_COBBLESTONE
        | GOLD_BLOCK | IRON_BLOCK | DIAMOND_BLOCK | EMERALD_BLOCK | STONEBRICK | NETHER_BRICK
        | IRON_BARS | STONE_STAIRS | BRICK_STAIRS | STONE_BRICK_STAIRS | NETHER_BRICK_STAIRS
        | REDSTONE_BLOCK => 10.0,
        // wooden slabs: setResistance(5.0F)
        WOODEN_SLAB | DOUBLE_WOODEN_SLAB => 5.0,
        PLANKS | OAK_STAIRS | SPRUCE_STAIRS | BIRCH_STAIRS | JUNGLE_STAIRS | ACACIA_STAIRS
        | DARK_OAK_STAIRS | GOLD_ORE | IRON_ORE | COAL_ORE | LAPIS_ORE | LAPIS_BLOCK
        | DIAMOND_ORE | REDSTONE_ORE | LIT_REDSTONE_ORE | EMERALD_ORE | FENCE | FENCE_GATE => 5.0,
        // BlockWall: setResistance(cobblestone.blockResistance / 3)
        COBBLESTONE_WALL => 10.0,
        BEDROCK | END_PORTAL_FRAME => 6000000.0,
        FLOWING_WATER | WATER | FLOWING_LAVA | LAVA => 100.0,
        OBSIDIAN | ANVIL => 2000.0,
        END_STONE => 15.0,
        HARDENED_CLAY | STAINED_HARDENED_CLAY => 7.0,
        COAL_BLOCK => 10.0,
        _ => 0.0,
    };
    let h = crate::item::hardness(id).max(0.0);
    (h * 5.0).max(explicit * 3.0) / 5.0
}

/// `World.newExplosion`: `Explosion.doExplosionA` and the server's
/// `doExplosionB(false)`, then `S27PacketExplosion` for the client.
fn explode(
    w: &mut ServerWorld,
    mobs: &mut Mobs,
    player: &PlayerInfo,
    r: crate::world::explosion::Request,
    out: &mut Outcome,
) {
    use crate::world::explosion::{self as ex, Target};
    let mut targets: Vec<Target> = Vec::new();
    // EntityPlayerMP: posY is the feet, getEyeHeight 0.12
    targets.push(Target {
        tag: ex::TAG_PLAYER,
        pos: player.feet,
        eye: 0.12f32 as f64,
        bb: player.bb(),
    });
    for m in &mobs.list {
        if Some(m.id) != r.exclude && m.death_time == 0 {
            targets.push(Target {
                tag: ex::TAG_MOB | m.id as u64,
                pos: m.pos,
                eye: m.kind.eye_height() as f64,
                bb: m.bb,
            });
        }
    }
    for (i, t) in mobs.tnt.iter().enumerate() {
        let h = 0.49;
        targets.push(Target {
            tag: ex::TAG_TNT | i as u64,
            pos: t.pos,
            eye: 0.0,
            bb: Aabb {
                min: [t.pos[0] - h, t.pos[1] - h, t.pos[2] - h],
                max: [t.pos[0] + h, t.pos[1] + h, t.pos[2] + h],
            },
        });
    }
    for (i, a) in mobs.arrows.iter().enumerate() {
        targets.push(Target {
            tag: ex::TAG_ARROW | i as u64,
            pos: a.e.pos,
            eye: 0.0,
            bb: a.e.bb,
        });
    }
    for (i, t) in mobs.throwables.iter().enumerate() {
        targets.push(Target {
            tag: ex::TAG_THROWABLE | i as u64,
            pos: t.e.pos,
            eye: 0.0,
            bb: t.e.bb,
        });
    }
    targets.extend(mobs.blast_targets.iter().copied());
    let (affected, blasts) = ex::explosion_a(w, &r, &targets);
    let from = match r.exploder {
        ex::Exploder::Living(p) => Some(p),
        _ => None,
    };
    for b in blasts {
        let k = (b.tag & 0xFFFF_FFFF) as usize;
        match b.tag & !0xFFFF_FFFF {
            ex::TAG_PLAYER => out.hits.push(Hit::Explosion {
                amount: b.damage,
                knock: b.push,
                from,
            }),
            ex::TAG_MOB => {
                if let Some(m) = mobs.list.iter_mut().find(|m| m.id as usize == k) {
                    m.hurt(b.damage, None, true);
                    for a in 0..3 {
                        m.motion[a] += b.push[a];
                    }
                }
            }
            ex::TAG_TNT => {
                if let Some(t) = mobs.tnt.get_mut(k) {
                    for a in 0..3 {
                        t.motion[a] += b.push[a];
                    }
                }
            }
            ex::TAG_ARROW => {
                if let Some(t) = mobs.arrows.get_mut(k) {
                    for a in 0..3 {
                        t.e.motion[a] += b.push[a];
                    }
                }
            }
            ex::TAG_THROWABLE => {
                if let Some(t) = mobs.throwables.get_mut(k) {
                    for a in 0..3 {
                        t.e.motion[a] += b.push[a];
                    }
                }
            }
            _ => out.blasts.push(b),
        }
    }
    let mut rng = JavaRandom::new(mobs.rand.next_long());
    let res = ex::explosion_b(w, &r, &affected, &mut rng);
    out.sounds.push(("random.explode", r.pos, 4.0, res.pitch));
    out.explosions.push(ex::View {
        pos: r.pos,
        size: r.size,
        smoking: r.smoking,
        // WorldServer.newExplosion: cleared unless isSmoking
        blocks: if r.smoking { affected } else { Vec::new() },
    });
    for (p, fuse) in res.tnt {
        mobs.prime_tnt(p, fuse);
    }
}

/// `SpawnerAnimals.canCreatureTypeSpawnAtLocation` for land creatures.
pub fn can_spawn_at(w: &mut ServerWorld, x: i32, y: i32, z: i32) -> bool {
    if !w.solid_top_surface(x, y - 1, z) {
        return false;
    }
    let below = w.block(x, y - 1, z);
    below != block::BEDROCK
        && !block::info(w.block(x, y, z)).normal_cube()
        && !block::material(w.block(x, y, z)).is_liquid()
        && !block::info(w.block(x, y + 1, z)).normal_cube()
}

/// Spawn list entry: kind (None for creatures this port does not have), weight, group size.
type Entry = (Option<Kind>, i32, i32, i32);

/// `BiomeGenBase.spawnableCreatureList` by biome id.
pub fn creature_list(biome: i32) -> Vec<Entry> {
    let base = biome & 127;
    if base == 8 || base == 9 {
        return Vec::new();
    }
    let default = vec![
        (Some(Kind::Sheep), 12, 4, 4),
        (Some(Kind::Pig), 10, 4, 4),
        (Some(Kind::Chicken), 10, 4, 4),
        (Some(Kind::Cow), 8, 4, 4),
    ];
    match base {
        0 | 24 | 7 | 11 | 16 | 26 | 25 | 2 | 17 | 37 | 38 | 39 | 12 | 13 => Vec::new(),
        14 | 15 => vec![(Some(Kind::Mooshroom), 8, 4, 8)],
        1 => [default, vec![(Some(Kind::Horse), 5, 2, 6)]].concat(),
        35 | 36 => [default, vec![(Some(Kind::Horse), 1, 2, 6)]].concat(),
        4 | 18 => [default, vec![(Some(Kind::Wolf), 5, 4, 4)]].concat(),
        5 | 19 | 30 | 31 | 32 | 33 => [default, vec![(Some(Kind::Wolf), 8, 4, 4)]].concat(),
        21 | 22 | 23 => [default, vec![(Some(Kind::Chicken), 10, 4, 4)]].concat(),
        _ => default,
    }
}

/// `BiomeGenBase.spawnableMonsterList`
pub fn monster_list(biome: i32) -> Vec<Entry> {
    let base = biome & 127;
    match base {
        // BiomeGenHell
        8 => {
            return vec![
                (Some(Kind::Ghast), 50, 4, 4),
                (Some(Kind::PigZombie), 100, 4, 4),
                (Some(Kind::MagmaCube), 1, 4, 4),
            ]
        }
        // BiomeGenEnd
        9 => return vec![(Some(Kind::Enderman), 10, 4, 4)],
        _ => {}
    }
    if base == 14 || base == 15 {
        return Vec::new();
    }
    let mut v = vec![
        (Some(Kind::Spider), 100, 4, 4),
        (Some(Kind::Zombie), 100, 4, 4),
        (Some(Kind::Skeleton), 100, 4, 4),
        (Some(Kind::Creeper), 100, 4, 4),
        (Some(Kind::Slime), 100, 4, 4),
        (Some(Kind::Enderman), 10, 1, 4),
        (Some(Kind::Witch), 5, 1, 1),
    ];
    if base == 6 {
        v.push((Some(Kind::Slime), 1, 1, 1));
    }
    if (21..=23).contains(&base) {
        v.push((Some(Kind::Ocelot), 2, 1, 1));
    }
    v
}

/// `BiomeGenBase.spawnableWaterCreatureList`
pub fn water_list(biome: i32) -> Vec<Entry> {
    let base = biome & 127;
    if base == 14 || base == 15 || base == 8 || base == 9 {
        return Vec::new();
    }
    vec![(Some(Kind::Squid), 10, 4, 4)]
}

/// `BiomeGenBase.spawnableCaveCreatureList`
pub fn ambient_list(biome: i32) -> Vec<Entry> {
    if biome == 8 || biome == 9 {
        return Vec::new();
    }
    vec![(Some(Kind::Bat), 10, 8, 8)]
}

/// `MapGenNetherBridge.getSpawnList`
pub fn fortress_list() -> Vec<Entry> {
    vec![
        (Some(Kind::Blaze), 10, 2, 3),
        (Some(Kind::PigZombie), 5, 4, 4),
        (Some(Kind::Skeleton), 10, 4, 4),
        (Some(Kind::MagmaCube), 3, 4, 4),
    ]
}

/// `Chunk.getRandomWithSeed(987234911)`: one chunk in ten has slimes.
pub fn slime_chunk(seed: i64, cx: i32, cz: i32) -> bool {
    let s = seed
        .wrapping_add(cx.wrapping_mul(cx).wrapping_mul(4987142) as i64)
        .wrapping_add(cx.wrapping_mul(5947611) as i64)
        .wrapping_add((cz.wrapping_mul(cz) as i64).wrapping_mul(4392871))
        .wrapping_add(cz.wrapping_mul(389711) as i64)
        ^ 987234911;
    JavaRandom::new(s).next_int_n(10) == 0
}

/// `getCanSpawnHere` of each mob for natural spawning.
fn can_spawn_here(w: &mut ServerWorld, m: &mut Mob, r: &mut JavaRandom) -> bool {
    let (px, py, pz) = (
        math::floor_double(m.pos[0]),
        math::floor_double(m.bb.min[1]),
        math::floor_double(m.pos[2]),
    );
    // EntityLiving.getCanSpawnHere: room to stand and no liquid
    let free = |w: &mut ServerWorld, m: &Mob| {
        colliding_boxes(w, &m.bb).is_empty()
            && !m.in_material(w, &m.bb, Material::Water)
            && !m.in_material(w, &m.bb, Material::Lava)
    };
    let mob_light = |w: &mut ServerWorld, r: &mut JavaRandom| {
        // EntityMob.isValidLightLevel
        if w.saved_light(true, px, py, pz) > r.next_int_n(32) {
            false
        } else {
            w.block_light_value(px, py, pz, true) <= r.next_int_n(8)
        }
    };
    match m.kind {
        Kind::Slime => {
            let biome = w.biome(px, pz).id as i32;
            if biome == 6
                && m.pos[1] > 50.0
                && m.pos[1] < 70.0
                && r.next_float() < 0.5
                && r.next_float() < w.moon_factor
                && w.block_light_value(px, py, pz, true) <= r.next_int_n(8)
            {
                return free(w, m);
            }
            if r.next_int_n(10) == 0 && slime_chunk(w.seed(), px >> 4, pz >> 4) && m.pos[1] < 40.0 {
                return free(w, m);
            }
            false
        }
        Kind::Ocelot => {
            // EntityOcelot.getCanSpawnHere
            if r.next_int_n(3) == 0 {
                return false;
            }
            if !free(w, m) || py < 63 {
                return false;
            }
            matches!(
                w.block(px, py - 1, pz),
                block::GRASS | block::LEAVES | block::LEAVES2
            )
        }
        Kind::Squid => m.pos[1] > 45.0 && m.pos[1] < 63.0 && colliding_boxes(w, &m.bb).is_empty(),
        Kind::Ghast => r.next_int_n(20) == 0 && free(w, m),
        Kind::PigZombie | Kind::MagmaCube => free(w, m),
        Kind::Blaze => m.path_weight(w, px, py, pz) >= 0.0 && free(w, m),
        Kind::Bat => {
            if py >= 63 {
                return false;
            }
            let l = w.block_light_value(px, py, pz, true);
            if r.next_boolean() {
                return false;
            }
            if l > r.next_int_n(4) {
                return false;
            }
            free(w, m)
        }
        k if k.hostile() => mob_light(w, r) && m.path_weight(w, px, py, pz) >= 0.0 && free(w, m),
        _ => {
            w.block(px, py - 1, pz) == block::GRASS
                && w.full_block_light(px, py, pz) > 8
                && free(w, m)
        }
    }
}

fn pick<'a>(r: &mut JavaRandom, list: &'a [Entry]) -> &'a Entry {
    let total: i32 = list.iter().map(|e| e.1).sum();
    let mut n = r.next_int_n(total);
    for e in list {
        n -= e.1;
        if n < 0 {
            return e;
        }
    }
    &list[0]
}

/// `SpawnerAnimals.performWorldGenSpawning` with the populate random. The
/// animals are handed over through `gen_entities`.
pub fn world_gen_spawning(w: &mut ServerWorld, biome: i32, x0: i32, z0: i32, r: &mut JavaRandom) {
    let list = creature_list(biome);
    if list.is_empty() {
        return;
    }
    while r.next_float() < 0.1 {
        let mut wr = std::mem::replace(&mut w.rand, JavaRandom::new(0));
        let e = *pick(&mut wr, &list);
        w.rand = wr;
        let n = e.2 + r.next_int_n(1 + e.3 - e.2);
        let mut x = x0 + r.next_int_n(16);
        let mut z = z0 + r.next_int_n(16);
        let (sx, sz) = (x, z);
        for _ in 0..n {
            let mut done = false;
            let mut tries = 0;
            while !done && tries < 4 {
                let y = w.top_solid_or_liquid(x, z);
                if can_spawn_at(w, x, y, z) {
                    let yaw = r.next_float() * 360.0;
                    if let Some(kind) = e.0 {
                        w.gen_entities
                            .push(crate::world::server::GenEntity::Animal {
                                kind,
                                pos: [x as f64 + 0.5, y as f64, z as f64 + 0.5],
                                yaw,
                            });
                    }
                    done = true;
                }
                x += r.next_int_n(5) - r.next_int_n(5);
                z += r.next_int_n(5) - r.next_int_n(5);
                while x < x0 || x >= x0 + 16 || z < z0 || z >= z0 + 16 {
                    x = sx + r.next_int_n(5) - r.next_int_n(5);
                    z = sz + r.next_int_n(5) - r.next_int_n(5);
                }
                tries += 1;
            }
        }
    }
}

/// `SpawnerAnimals.findChunksForSpawning` around one player.
pub fn natural_spawning(
    w: &mut ServerWorld,
    mobs: &mut Mobs,
    player: [f64; 3],
    spawn_point: [f64; 3],
    animals: bool,
) {
    let (pcx, pcz) = (
        (player[0].floor() as i32) >> 4,
        (player[2].floor() as i32) >> 4,
    );
    let mut chunks = Vec::new();
    for dx in -8..=8 {
        for dz in -8..=8 {
            let edge = dx == -8 || dx == 8 || dz == -8 || dz == 8;
            if !edge && w.chunk_exists(pcx + dx, pcz + dz) {
                chunks.push((pcx + dx, pcz + dz));
            }
        }
    }
    let eligible = chunks.len() as i32 + 64; // edge chunks count towards the cap
                                             // EnumCreatureType: monster, creature, ambient, waterCreature
    for ty in 0..4 {
        let hostile = ty == 0;
        if ty == 1 && !animals {
            continue;
        }
        let cap = [70, 10, 15, 5][ty];
        let existing = mobs
            .list
            .iter()
            .filter(|m| match ty {
                0 => m.kind.hostile(),
                1 => m.kind.animal(),
                2 => m.kind == Kind::Bat,
                _ => m.kind == Kind::Squid,
            })
            .count() as i32;
        if existing > cap * eligible / 256 {
            continue;
        }
        let mut rand = std::mem::replace(&mut w.rand, JavaRandom::new(0));
        // shuffled order
        for i in (1..chunks.len()).rev() {
            let j = rand.next_int_n(i as i32 + 1) as usize;
            chunks.swap(i, j);
        }
        for &(cx, cz) in &chunks {
            let top = w.chunk(cx, cz).top_filled_segment();
            let x = cx * 16 + rand.next_int_n(16);
            let z = cz * 16 + rand.next_int_n(16);
            let y = rand.next_int_n((top + 16 - 1).max(1));
            let want = if ty == 3 {
                Material::Water
            } else {
                Material::Air
            };
            if block::info(w.block(x, y, z)).normal_cube() || w.material(x, y, z) != want {
                continue;
            }
            let mut spawned = 0;
            'packs: for _ in 0..3 {
                let (mut px, py, mut pz) = (x, y, z);
                let mut entry: Option<Entry> = None;
                for _ in 0..4 {
                    px += rand.next_int_n(6) - rand.next_int_n(6);
                    pz += rand.next_int_n(6) - rand.next_int_n(6);
                    let ok_here = if ty == 3 {
                        w.material(px, py, pz).is_liquid()
                            && w.material(px, py - 1, pz).is_liquid()
                            && !block::info(w.block(px, py + 1, pz)).normal_cube()
                    } else {
                        can_spawn_at(w, px, py, pz)
                    };
                    if !ok_here {
                        continue;
                    }
                    let (fx, fy, fz) = (px as f64 + 0.5, py as f64, pz as f64 + 0.5);
                    let dp = (fx - player[0]).powi(2)
                        + (fy - player[1]).powi(2)
                        + (fz - player[2]).powi(2);
                    let ds = (fx - spawn_point[0]).powi(2)
                        + (fy - spawn_point[1]).powi(2)
                        + (fz - spawn_point[2]).powi(2);
                    if dp < 576.0 || ds < 576.0 {
                        continue;
                    }
                    if entry.is_none() {
                        let biome = w.biome(px, pz).id as i32;
                        // ChunkProviderHell.getPossibleCreatures: fortresses
                        let in_fortress = ty == 0
                            && w.provider.dim == -1
                            && w.structures.as_ref().map_or(false, |st| {
                                st.fortress_piece_at(px, py, pz)
                                    || st.fortress_bounds_at(px, py, pz)
                                        && w.peek_block(px, py - 1, pz) == block::NETHER_BRICK
                            });
                        let list = match ty {
                            0 if in_fortress => fortress_list(),
                            0 => monster_list(biome),
                            1 => creature_list(biome),
                            2 => ambient_list(biome),
                            _ => water_list(biome),
                        };
                        if list.is_empty() {
                            break 'packs;
                        }
                        entry = Some(*pick(&mut rand, &list));
                    }
                    let Some(e) = entry else { break };
                    let Some(kind) = e.0 else { continue };
                    let _ = hostile;
                    // new Entity, setLocationAndAngles, getCanSpawnHere
                    let yaw = rand.next_float() * 360.0;
                    let mut m = mobs.make(kind, [fx, fy, fz], yaw);
                    if can_spawn_here(w, &mut m, &mut rand) {
                        let cx = SpawnCtx::of(w, m.pos);
                        m.on_spawn(&mut rand, &cx);
                        mobs.list.push(m);
                        spawned += 1;
                        if spawned >= 4 {
                            break 'packs;
                        }
                    }
                }
            }
        }
        w.rand = rand;
    }
}

#[cfg(test)]
mod loot_tests {
    use super::*;

    /// Against `tools/EnchantFxHarness.java`: `dropFewItems(hit, looting)`
    /// and the rare-drop roll of `onDeath` for every mob, seeded, with the
    /// next `rand.nextInt` to check the draws.
    #[test]
    fn death_loot_matches_the_game() {
        let mut n = 0;
        for l in include_str!("../../tools/enchant_fx_1710.txt").lines() {
            let Some(rest) = l.strip_prefix("d ") else {
                continue;
            };
            let (head, tail) = rest.split_once(" |").map_or((rest, ""), |x| x);
            let f: Vec<&str> = head.split(' ').collect();
            let kind = Kind::from_id(f[0]).unwrap();
            let int = |i: usize| f[i].parse::<i32>().unwrap();
            let (variant, burning, hit, looting, seed) = (int(1), int(2), int(3), int(4), int(5));
            let mut m = Mob::new(1, kind, [0.0, 64.0, 0.0], 0.0, seed as i64);
            match kind {
                Kind::Skeleton => m.variant = variant,
                Kind::Slime | Kind::MagmaCube => m.set_slime_size(variant),
                Kind::Sheep if variant < 0 => m.sheared = true,
                Kind::Sheep => m.variant = variant,
                Kind::Horse => m.variant = variant << 16,
                _ => {}
            }
            m.rand = JavaRandom::new(seed as i64);
            m.fire = burning * 100;
            let got: Vec<String> = m
                .drops(hit == 1, looting)
                .iter()
                .map(|s| format!("{}:{}:{}", s.id, s.meta, s.count))
                .collect();
            let want: Vec<&str> = f[6..].iter().copied().filter(|x| !x.is_empty()).collect();
            assert_eq!(got, want, "{l}");
            assert_eq!(m.rand.next_int().to_string(), tail, "{l}");
            n += 1;
        }
        assert_eq!(n, 33 * 96);
    }

    /// Against `tools/EnchantFxHarness.java`: `onSpawnWithEgg(null)` of
    /// skeletons, zombies and pigmen at a fixed local difficulty, normal
    /// and hard, seeded: the equipment with its enchantments,
    /// `canPickUpLoot`, and the next draw of the mob's and the world's random.
    #[test]
    fn spawn_equipment_matches_the_game() {
        let mut n = 0;
        for l in include_str!("../../tools/enchant_fx_1710.txt").lines() {
            let Some(rest) = l.strip_prefix("s ") else {
                continue;
            };
            let (head, tail) = rest.split_once(" |").unwrap();
            let f: Vec<&str> = head.split(' ').collect();
            let kind = Kind::from_id(f[0]).unwrap();
            let tension: f32 = f[1].parse().unwrap();
            let hard = f[2] == "1";
            let seed: i64 = f[3].parse().unwrap();
            let mut m = Mob::new(1, kind, [0.0, 64.0, 0.0], 0.0, seed);
            m.rand = JavaRandom::new(seed);
            let mut wr = JavaRandom::new(seed + 1000);
            let cx = SpawnCtx {
                tension,
                hard,
                nether: false,
                halloween: false,
            };
            m.on_spawn(&mut wr, &cx);
            let mut got: Vec<String> = m
                .equipment
                .iter()
                .map(|s| match s {
                    None => "-".to_string(),
                    Some(s) => {
                        let mut t = format!("{}:{}", s.id, s.meta);
                        for (id, lvl) in s.enchantments() {
                            t.push_str(&format!(".{id}/{lvl}"));
                        }
                        t
                    }
                })
                .collect();
            got.push(format!("loot={}", m.can_pick_up_loot as i32));
            assert_eq!(got.join(" "), f[4..].join(" "), "{l}");
            assert_eq!(
                format!("{} {}", m.rand.next_int(), wr.next_int()),
                tail,
                "{l}"
            );
            n += 1;
        }
        assert_eq!(n, 3 * 5 * 2 * 40);
    }
}

/// What `onSpawnWithEgg` reads from the world.
#[derive(Clone, Copy, Debug, Default)]
pub struct SpawnCtx {
    /// `getTensionFactorForBlock` at the mob
    pub tension: f32,
    pub hard: bool,
    /// `provider instanceof WorldProviderHell`
    pub nether: bool,
    /// `getCurrentDate()` is October 31st
    pub halloween: bool,
}

impl SpawnCtx {
    pub fn of(w: &mut ServerWorld, pos: [f64; 3]) -> SpawnCtx {
        SpawnCtx {
            tension: w.local_difficulty(pos[0], pos[1], pos[2]),
            hard: w.difficulty == 3,
            nether: w.provider.dim == -1,
            halloween: is_halloween(),
        }
    }
}

/// `Calendar.get(MONTH) + 1 == 10 && get(DAY_OF_MONTH) == 31` (UTC).
fn is_halloween() -> bool {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    // civil date from days since 1970-01-01 (Howard Hinnant)
    let z = secs.div_euclid(86400) + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    m == 10 && d == 31
}

/// `EntityLiving.getArmorItemForSlot(slot, tier)` (1 boots .. 4 helmet;
/// leather, gold, chain, iron, diamond)
fn armor_item_for_slot(slot: usize, tier: i32) -> Option<u16> {
    if !(0..=4).contains(&tier) {
        return None;
    }
    // item ids by material (leather 298.., chain 302.., iron 306..,
    // diamond 310.., gold 314..), helmet first
    let base = [298u16, 314, 302, 306, 310][tier as usize];
    match slot {
        4 => Some(base),
        3 => Some(base + 1),
        2 => Some(base + 2),
        1 => Some(base + 3),
        _ => None,
    }
}

/// `EnchantmentHelper.addRandomEnchantment` on a stack (not a book).
fn add_random_enchantment(r: &mut JavaRandom, s: Stack, level: i32) -> Stack {
    let list = crate::enchant::build_list(r, &s, level);
    let mut have = s.enchantments();
    have.extend(list);
    s.with_enchantments(&have)
}
