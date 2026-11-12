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

mod extra;
pub use extra::{village_golems, Other, Puff, Strike};

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
}

pub const ALL_KINDS: [Kind; 21] = [
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
            Kind::Spider | Kind::CaveSpider | Kind::Silverfish | Kind::Slime | Kind::Enderman
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
    pub persistent: bool,
    // per-kind state
    /// Sheep fleece colour / villager profession / skeleton type.
    pub variant: i32,
    pub sheared: bool,
    /// Creeper `timeSinceIgnited` and `lastActiveTime`.
    pub fuse: i32,
    pub prev_fuse: i32,
    fuse_dir: i32,
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
    pub sitting: bool,
    /// Iron golem arm swing timer; built by the player.
    pub attack_timer: i32,
    pub player_made: bool,
    /// Another mob this one is after (wolves, golems).
    target_mob: Option<u32>,
}

/// What the player looks like to the mobs.
#[derive(Clone, Copy)]
pub struct PlayerInfo {
    pub feet: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    /// The mob the player hit last, and the last one that hit the player
    /// (tamed wolves join in).
    pub last_attacked: Option<u32>,
    pub last_attacker: Option<u32>,
    pub attackable: bool,
    /// Item ids the player holds (for tempting).
    pub held: Option<u16>,
}

impl PlayerInfo {
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
    /// Damage with the attacker kind and the knockback motion to add.
    Melee {
        kind: Kind,
        amount: f32,
        knock: [f64; 3],
        poison: i32,
    },
    Arrow {
        amount: f32,
        knock: [f64; 3],
    },
    Explosion {
        amount: f32,
        knock: [f64; 3],
    },
    Xp(i32),
}

#[derive(Clone, Debug)]
pub struct Arrow {
    pub pos: [f64; 3],
    pub prev_pos: [f64; 3],
    pub motion: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub ticks: i32,
    pub stuck: bool,
    damage: f64,
}

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
            persistent: false,
            variant: 0,
            sheared: false,
            fuse: 0,
            prev_fuse: 0,
            fuse_dir: -1,
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
            sitting: false,
            attack_timer: 0,
            player_made: false,
            target_mob: None,
        };
        m.egg_timer = m.rand.next_int_n(6000) + 6000;
        match kind {
            Kind::Slime => {
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
    pub fn set_slime_size(&mut self, size: i32) {
        self.variant = size;
        self.health = (size * size) as f32;
        let p = self.pos;
        self.set_position(p[0], p[1], p[2]);
    }

    /// Width and height of the box (slimes scale with their size).
    pub fn dims(&self) -> (f32, f32) {
        if self.kind == Kind::Slime {
            let s = 0.6 * self.variant.max(1) as f32;
            (s, s)
        } else {
            self.kind.size()
        }
    }

    pub fn max_health(&self) -> f32 {
        match self.kind {
            Kind::Slime => (self.variant * self.variant) as f32,
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

    /// `EntitySheep.onSpawnWithEgg` / villager profession and the like.
    pub fn on_spawn(&mut self, world_rand: &mut JavaRandom) {
        match self.kind {
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
    pub fn damage(&mut self, amount: f32, from: Option<[f64; 3]>) -> bool {
        if self.health <= 0.0 {
            return false;
        }
        let applied;
        if self.hurt_resistant as f32 > 10.0 {
            if amount <= self.last_damage {
                return false;
            }
            applied = amount - self.last_damage;
            self.last_damage = amount;
        } else {
            self.last_damage = amount;
            self.hurt_resistant = 20;
            applied = amount;
            self.hurt_time = 10;
            if let Some(a) = from {
                // knockBack
                let (mut dx, mut dz) = (a[0] - self.pos[0], a[2] - self.pos[2]);
                while dx * dx + dz * dz < 1.0e-4 {
                    dx = (self.rand.next_double() - self.rand.next_double()) * 0.01;
                    dz = (self.rand.next_double() - self.rand.next_double()) * 0.01;
                }
                let f = (dx * dx + dz * dz).sqrt();
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
        }
        self.health -= applied;
        match self.kind {
            Kind::Bat => self.hanging = false,
            Kind::Enderman => self.angry = true,
            Kind::Wolf | Kind::Ocelot => self.sitting = false,
            _ => {}
        }
        if self.kind == Kind::Horse
            || (self.kind.animal() && !matches!(self.kind, Kind::Wolf | Kind::Ocelot))
            || self.kind == Kind::Villager
        {
            self.task = Task::Panic;
            self.task_ticks = 0;
            self.path.clear();
        }
        true
    }

    /// `dropFewItems` + `dropEquipment`: what the mob leaves when killed.
    fn drops(&mut self, by_player: bool) -> Vec<Stack> {
        let burning = self.fire > 0;
        let mut v = Vec::new();
        let mut add = |id: u16, meta: i32, n: i32| {
            if n > 0 {
                v.push(Stack::new(id, meta, n as u32));
            }
        };
        let r = &mut self.rand;
        match self.kind {
            Kind::Pig => {
                let n = r.next_int_n(3) + 1;
                add(if burning { 320 } else { 319 }, 0, n);
            }
            Kind::Cow | Kind::Mooshroom => {
                let n = r.next_int_n(3);
                add(334, 0, n);
                let n = r.next_int_n(3) + 1;
                add(if burning { 364 } else { 363 }, 0, n);
            }
            Kind::Sheep => {
                if !self.sheared {
                    add(block::WOOL as u16, self.variant, 1);
                }
            }
            Kind::Chicken => {
                let n = r.next_int_n(3);
                add(288, 0, n);
                add(if burning { 366 } else { 365 }, 0, 1);
            }
            Kind::Zombie => {
                let n = r.next_int_n(3);
                add(367, 0, n);
                if by_player && r.next_int_n(200) < 5 {
                    match r.next_int_n(3) {
                        0 => add(265, 0, 1),
                        1 => add(391, 0, 1),
                        _ => add(392, 0, 1),
                    }
                }
            }
            Kind::Skeleton => {
                let n = r.next_int_n(3);
                add(262, 0, n);
                let n = r.next_int_n(3);
                add(352, 0, n);
            }
            Kind::Creeper => {
                let n = r.next_int_n(3);
                add(289, 0, n);
            }
            Kind::Spider | Kind::CaveSpider => {
                let n = r.next_int_n(3);
                add(287, 0, n);
                if by_player && r.next_int_n(3) == 0 {
                    add(375, 0, 1);
                }
            }
            Kind::Witch => {
                let n = r.next_int_n(3) + 1;
                for _ in 0..n {
                    let k = r.next_int_n(3);
                    let item = [348, 353, 331, 375, 374, 289, 280, 280][r.next_int_n(8) as usize];
                    add(item, 0, k);
                }
            }
            Kind::Slime => {
                if self.variant == 1 {
                    let n = r.next_int_n(3);
                    add(341, 0, n);
                }
            }
            Kind::Enderman => {
                let n = r.next_int_n(2);
                add(368, 0, n);
            }
            Kind::Squid => {
                let n = r.next_int_n(3) + 1;
                add(351, 0, n);
            }
            Kind::Horse => {
                let n = r.next_int_n(3);
                add(334, 0, n);
            }
            Kind::IronGolem => {
                let n = r.next_int_n(3);
                add(block::RED_FLOWER as u16, 0, n);
                let n = 3 + r.next_int_n(3);
                add(265, 0, n);
            }
            Kind::Villager | Kind::Silverfish | Kind::Bat | Kind::Wolf | Kind::Ocelot => {}
        }
        if self.kind == Kind::Enderman && self.carried != 0 {
            v.push(Stack::new(self.carried as u16, self.carried_meta as i32, 1));
        }
        v
    }

    fn xp(&mut self) -> i32 {
        match self.kind {
            Kind::Slime => return self.variant,
            Kind::Squid => return 1 + self.rand.next_int_n(3),
            Kind::Bat | Kind::IronGolem => return 0,
            _ => {}
        }
        if self.kind.animal() {
            1 + self.rand.next_int_n(3)
        } else if self.kind.hostile() {
            5
        } else {
            0
        }
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
    pub tnt: Vec<Tnt>,
    next_id: u32,
    pub rand: JavaRandom,
}

impl Default for Mobs {
    fn default() -> Self {
        Mobs {
            list: Vec::new(),
            arrows: Vec::new(),
            tnt: Vec::new(),
            next_id: 1,
            rand: JavaRandom::new(0x6D6F62),
        }
    }
}

/// Output of a tick: damage to the player, dropped items, sounds.
#[derive(Default)]
pub struct Outcome {
    pub hits: Vec<Hit>,
    pub drops: Vec<([f64; 3], Stack)>,
    pub sounds: Vec<(&'static str, [f64; 3], f32, f32)>,
    /// Explosions (centre, size) for particles and sound.
    pub explosions: Vec<[f64; 4]>,
    /// Particles for the client (portal, slime, heart, smoke).
    pub particles: Vec<Puff>,
    /// The mob that last hurt the player this tick.
    pub attacker: Option<u32>,
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

    /// `BehaviorProjectileDispense` for arrows: `EntityArrow` launched along
    /// `dir` at 1.1 with inaccuracy 6.
    pub fn dispense_arrow(&mut self, pos: [f64; 3], dir: [f64; 3]) {
        let mut v = [dir[0], dir[1] + 0.1, dir[2]];
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        for c in v.iter_mut() {
            *c /= len;
            *c += self.rand.next_gaussian()
                * (if self.rand.next_boolean() { -1.0 } else { 1.0 })
                * 0.007499999832361937
                * 6.0;
            *c *= 1.1;
        }
        let h = (v[0] * v[0] + v[2] * v[2]).sqrt();
        let yaw = (v[0].atan2(v[2]) * 180.0 / std::f64::consts::PI) as f32;
        let pitch = (v[1].atan2(h) * 180.0 / std::f64::consts::PI) as f32;
        self.arrows.push(Arrow {
            pos,
            prev_pos: pos,
            motion: v,
            yaw,
            pitch,
            ticks: 0,
            stuck: false,
            damage: 2.0,
        });
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
        m.on_spawn(world_rand);
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
        let mut explosions: Vec<([f64; 3], f32)> = Vec::new();
        let others: Vec<Other> = self
            .list
            .iter()
            .filter(|m| m.health > 0.0)
            .map(|m| Other {
                id: m.id,
                kind: m.kind,
                pos: m.pos,
                tamed: m.tamed,
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
            );
        }
        self.arrows.extend(new_arrows);
        for st in strikes {
            let Some(t) = self.list.iter_mut().find(|m| m.id == st.target) else {
                continue;
            };
            if t.damage(st.amount, Some(st.at)) {
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
                explosions.push((c, 4.0));
            } else {
                k += 1;
            }
        }
        for (c, size) in explosions {
            explode(w, self, player, c, size, out);
        }
        // deaths
        let mut splits: Vec<([f64; 3], i32, f32)> = Vec::new();
        let mut i = 0;
        while i < self.list.len() {
            let m = &mut self.list[i];
            if m.health <= 0.0 && m.death_time == 0 {
                let by_player = m.recently_hit > 0;
                for s in m.drops(by_player) {
                    out.drops.push(([m.pos[0], m.pos[1] + 0.0, m.pos[2]], s));
                }
                if by_player {
                    let xp = m.xp();
                    if xp > 0 {
                        out.hits.push(Hit::Xp(xp));
                    }
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
                m.death_time += 1;
                if m.death_time >= 20 {
                    m.dead = true;
                }
            }
            if m.dead {
                self.list.remove(i);
            } else {
                i += 1;
            }
        }
        for (pos, size, yaw) in splits {
            let id = self.next_id;
            self.next_id += 1;
            let seed = self.rand.next_long();
            let mut m = Mob::new(id, Kind::Slime, pos, yaw, seed);
            m.set_slime_size(size);
            self.list.push(m);
        }
        tick_arrows(&mut self.arrows, w, player, out, &mut self.list);
    }

    /// `EntityPlayer.attackTargetEntityWithCurrentItem`
    pub fn attack(
        &mut self,
        id: u32,
        damage: f32,
        crit: bool,
        attacker: [f64; 3],
        yaw: f32,
        knockback: i32,
        out: &mut Outcome,
    ) {
        let Some(m) = self.list.iter_mut().find(|m| m.id == id) else {
            return;
        };
        if m.health <= 0.0 {
            return;
        }
        let mut dmg = damage;
        if crit {
            dmg += m.rand.next_int_n((dmg / 2.0) as i32 + 2) as f32;
        }
        if m.damage(dmg, Some(attacker)) {
            m.recently_hit = 100;
            if m.kind.hostile() {
                m.target_player = true;
            }
            if knockback > 0 {
                let r = yaw * std::f32::consts::PI / 180.0;
                m.motion[0] += (-r.sin() * knockback as f32 * 0.5) as f64;
                m.motion[1] += 0.1;
                m.motion[2] += (r.cos() * knockback as f32 * 0.5) as f64;
            }
            let s = mob_sound(m, false);
            if !s.is_empty() {
                out.sounds.push((s, m.pos, 1.0, 1.0));
            }
            if matches!(m.kind, Kind::Wolf | Kind::IronGolem) && !m.tamed && !m.player_made {
                m.target_player = true;
            }
        }
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
            (Kind::Sheep, Some(359)) if !m.sheared => {
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
                let s = held.unwrap();
                let d = s.meta + 1;
                Some(if d > crate::item::max_damage(s.id) {
                    None
                } else {
                    Some(Stack { meta: d, ..s })
                })
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
                    m.sitting = true;
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
                    m.sitting = true;
                    m.variant = 1 + m.rand.next_int_n(3);
                    heart(m, out, true);
                } else {
                    heart(m, out, false);
                }
                Some(one_less(held.unwrap()))
            }
            (Kind::Wolf | Kind::Ocelot, _) if m.tamed => {
                m.sitting = !m.sitting;
                m.path.clear();
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
                format!(
                    "{},{:?},{:?},{:?},{},{},{},{},{},{},{},{},{}",
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
                    m.angry as i32
                )
            })
            .collect()
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
            m.player_made = flags & 4 != 0;
            m.carried = f[10].parse().ok()?;
            m.carried_meta = f[11].parse().ok()?;
            m.angry = f[12] == "1";
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
    }
}

/// Hurt or death sound of this particular mob (slime size, donkeys).
pub fn mob_sound(m: &Mob, death: bool) -> &'static str {
    match m.kind {
        Kind::Slime if m.variant > 1 => "mob.slime.big",
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
        Kind::Creeper | Kind::Slime | Kind::Squid | Kind::Ocelot | Kind::IronGolem => return None,
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
    explosions: &mut Vec<([f64; 3], f32)>,
    others: &[Other],
    strikes: &mut Vec<Strike>,
) {
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
    if m.in_material(w, &m.bb, Material::Lava) {
        m.damage(4.0, None);
        m.fire = 300;
    }
    if m.fire > 0 {
        if m.fire % 20 == 0 {
            m.damage(1.0, None);
        }
        m.fire -= 1;
    }
    // undead burn in daylight
    if day && matches!(m.kind, Kind::Zombie | Kind::Skeleton) {
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

    // ---------------------------------------------------------------- AI
    let pdist2 = m.dist2(player.feet);
    let sees_player = player.attackable
        && pdist2 < (m.kind.follow_range() as f64).powi(2)
        && m.can_see(w, player.eye());
    // targeting
    if m.kind.hostile() && !matches!(m.kind, Kind::Enderman | Kind::Slime) {
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
        Kind::Creeper => {
            creeper(m, w, player, out, explosions);
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
        Kind::Slime => extra::slime(m, player, out),
        Kind::Enderman => extra::enderman(m, w, player, day, out),
        Kind::Wolf | Kind::Ocelot => extra::tameable(m, w, player, others, strikes, out),
        Kind::Horse => extra::horse(m, w, player, out),
        Kind::IronGolem => extra::golem(m, w, player, others, strikes, out),
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
    if m.kind != Kind::Slime {
        m.follow_path();
        m.move_helper();
    }
    m.look_helper();
    if !m.kind.old_ai() {
        m.body_helper();
    } else {
        // EntityLivingBase.func_110146_f: body follows movement direction
        m.body_yaw = m.yaw;
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
    if m.kind == Kind::Slime {
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
        let poison = if m.kind == Kind::CaveSpider {
            7 * 20
        } else {
            0
        };
        out.hits.push(Hit::Melee {
            kind: m.kind,
            amount: m.kind.attack_damage(),
            knock,
            poison,
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
            // EntitySkeleton.attackEntityWithRangedAttack: EntityArrow(world, this, target, 1.6, 14 - difficulty*4)
            let start = [m.pos[0], m.eye()[1] - 0.1, m.pos[2]];
            let dy = player.feet[1] + 1.62 * 0.333 - start[1];
            let (dx, dz) = (player.feet[0] - start[0], player.feet[2] - start[2]);
            let h = (dx * dx + dz * dz).sqrt();
            if h >= 1.0e-7 {
                let yaw = (dz.atan2(dx) * 180.0 / std::f64::consts::PI) as f32 - 90.0;
                let pitch = -(dy.atan2(h) * 180.0 / std::f64::consts::PI) as f32;
                let off = [dx / h, dz / h];
                let pos = [start[0] + off[0], start[1], start[2] + off[1]];
                let mut v = [dx, dy + h * 0.2f32 as f64, dz];
                let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                let inacc = 10.0f64; // 14 - normal difficulty * 4
                for (i, c) in v.iter_mut().enumerate() {
                    *c /= len;
                    *c += m.rand.next_gaussian()
                        * (if m.rand.next_boolean() { -1.0 } else { 1.0 })
                        * 0.007499999832361937
                        * inacc;
                    *c *= 1.6;
                    let _ = i;
                }
                arrows.push(Arrow {
                    pos,
                    prev_pos: pos,
                    motion: v,
                    yaw,
                    pitch,
                    ticks: 0,
                    stuck: false,
                    damage: 2.0 + m.rand.next_gaussian() * 0.25 + 2.0 * 0.11,
                });
                out.sounds.push((
                    "random.bow",
                    m.pos,
                    1.0,
                    1.0 / (m.rand.next_float() * 0.4 + 0.8),
                ));
            }
        } else {
            // EntityWitch throws a splash potion of harming (6 damage) at range.
            out.hits.push(Hit::Melee {
                kind: Kind::Witch,
                amount: 6.0,
                knock: [0.0, 0.0, 0.0],
                poison: 0,
            });
            out.sounds.push((
                "random.bow",
                m.pos,
                0.5,
                0.4 / (m.rand.next_float() * 0.4 + 0.8),
            ));
        }
    }
}

/// `EntityAICreeperSwell` + `EntityCreeper.onUpdate`
fn creeper(
    m: &mut Mob,
    w: &mut ServerWorld,
    player: &PlayerInfo,
    out: &mut Outcome,
    explosions: &mut Vec<([f64; 3], f32)>,
) {
    let d2 = m.dist2(player.feet);
    let swelling = m.target_player && d2 < 9.0 && m.can_see(w, player.eye());
    let stop = !m.target_player || d2 > 49.0;
    if swelling {
        m.fuse_dir = 1;
        m.path.clear();
        m.look_at = Some(player.eye());
    } else if stop {
        m.fuse_dir = -1;
    }
    if m.target_player && !swelling {
        m.look_at = Some(player.eye());
        if m.no_path() || m.ticks % 10 == 0 {
            m.path_to(w, player.feet[0], player.feet[1], player.feet[2], 1.0);
        }
    } else if !m.target_player {
        wander(m, w, 0.8, 120);
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
        explosions.push((m.pos, 3.0));
        m.health = 0.0;
        m.death_time = 19;
        m.dead = true;
    }
}

/// `EntityArrow.onUpdate` (simplified hit test against the player and mobs).
fn tick_arrows(
    arrows: &mut Vec<Arrow>,
    w: &mut ServerWorld,
    player: &PlayerInfo,
    out: &mut Outcome,
    mobs: &mut [Mob],
) {
    arrows.retain_mut(|a| {
        a.ticks += 1;
        a.prev_pos = a.pos;
        if a.stuck {
            return a.ticks < 1200;
        }
        let next = [
            a.pos[0] + a.motion[0],
            a.pos[1] + a.motion[1],
            a.pos[2] + a.motion[2],
        ];
        // block hit
        let steps = 8;
        for i in 1..=steps {
            let t = i as f64 / steps as f64;
            let p = [
                a.pos[0] + a.motion[0] * t,
                a.pos[1] + a.motion[1] * t,
                a.pos[2] + a.motion[2] * t,
            ];
            let id = w.peek_block(
                math::floor_double(p[0]),
                math::floor_double(p[1]),
                math::floor_double(p[2]),
            );
            if block::info(id).solid {
                a.pos = p;
                a.stuck = true;
                a.ticks = 0;
                out.sounds.push(("random.bowhit", p, 1.0, 1.2));
                return true;
            }
        }
        // player hit
        let pb = player.bb();
        let len = (a.motion[0].powi(2) + a.motion[1].powi(2) + a.motion[2].powi(2)).sqrt();
        if player.attackable {
            let grown = Aabb {
                min: [pb.min[0] - 0.3, pb.min[1] - 0.3, pb.min[2] - 0.3],
                max: [pb.max[0] + 0.3, pb.max[1] + 0.3, pb.max[2] + 0.3],
            };
            if ray_box(a.pos, a.motion, &grown).map_or(false, |t| t <= 1.0) {
                let dmg = (len * a.damage).ceil() as f32;
                let h = (a.motion[0].powi(2) + a.motion[2].powi(2)).sqrt().max(1e-4);
                out.hits.push(Hit::Arrow {
                    amount: dmg,
                    knock: [
                        a.motion[0] * 0.6 / h * 0.0,
                        0.1,
                        a.motion[2] * 0.6 / h * 0.0,
                    ],
                });
                out.sounds.push(("random.bowhit", a.pos, 1.0, 1.2));
                return false;
            }
        }
        for m in mobs.iter_mut() {
            if m.health > 0.0
                && a.ticks > 2
                && ray_box(a.pos, a.motion, &m.bb).map_or(false, |t| t <= 1.0)
            {
                m.damage((len * a.damage).ceil() as f32, Some(a.pos));
                return false;
            }
        }
        a.pos = next;
        let h = (a.motion[0].powi(2) + a.motion[2].powi(2)).sqrt();
        a.yaw = (a.motion[0].atan2(a.motion[2]) * 180.0 / std::f64::consts::PI) as f32;
        a.pitch = (a.motion[1].atan2(h) * 180.0 / std::f64::consts::PI) as f32;
        let drag = if block::material(w.peek_block(
            math::floor_double(a.pos[0]),
            math::floor_double(a.pos[1]),
            math::floor_double(a.pos[2]),
        )) == Material::Water
        {
            0.8
        } else {
            0.99
        };
        for v in &mut a.motion {
            *v *= drag;
        }
        a.motion[1] -= 0.05f32 as f64;
        a.ticks < 1200 && a.pos[1] > -64.0
    });
}

/// `Block.getExplosionResistance(null)`: `blockResistance / 5`.
pub fn explosion_resistance(id: u8) -> f32 {
    use crate::block::*;
    let explicit = match id {
        STONE | COBBLESTONE | STONE_SLAB | DOUBLE_STONE_SLAB | BRICK_BLOCK | MOSSY_COBBLESTONE
        | GOLD_BLOCK | IRON_BLOCK | DIAMOND_BLOCK | EMERALD_BLOCK | STONEBRICK | NETHER_BRICK
        | IRON_BARS | STONE_STAIRS | BRICK_STAIRS | STONE_BRICK_STAIRS | NETHER_BRICK_STAIRS
        | REDSTONE_BLOCK => 10.0,
        PLANKS | OAK_STAIRS | SPRUCE_STAIRS | BIRCH_STAIRS | JUNGLE_STAIRS | ACACIA_STAIRS
        | DARK_OAK_STAIRS | GOLD_ORE | IRON_ORE | COAL_ORE | LAPIS_ORE | LAPIS_BLOCK
        | DIAMOND_ORE | REDSTONE_ORE | LIT_REDSTONE_ORE | EMERALD_ORE | FENCE => 5.0,
        BEDROCK | END_PORTAL_FRAME => 6000000.0,
        FLOWING_WATER | WATER | FLOWING_LAVA | LAVA => 100.0,
        OBSIDIAN => 2000.0,
        END_STONE => 15.0,
        HARDENED_CLAY | STAINED_HARDENED_CLAY => 7.0,
        COAL_BLOCK => 10.0,
        _ => 0.0,
    };
    let h = crate::item::hardness(id).max(0.0);
    (h * 5.0).max(explicit * 3.0) / 5.0
}

/// `Explosion.doExplosionA` + `doExplosionB(true)`
fn explode(
    w: &mut ServerWorld,
    mobs: &mut Mobs,
    player: &PlayerInfo,
    c: [f64; 3],
    size: f32,
    out: &mut Outcome,
) {
    let r = &mut mobs.rand;
    let mut hit: std::collections::BTreeSet<(i32, i32, i32)> = std::collections::BTreeSet::new();
    for i in 0..16 {
        for j in 0..16 {
            for k in 0..16 {
                if i == 0 || i == 15 || j == 0 || j == 15 || k == 0 || k == 15 {
                    let mut d = [
                        i as f64 / 15.0 * 2.0 - 1.0,
                        j as f64 / 15.0 * 2.0 - 1.0,
                        k as f64 / 15.0 * 2.0 - 1.0,
                    ];
                    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                    for v in &mut d {
                        *v /= l;
                    }
                    let mut f1 = size * (0.7 + r.next_float() * 0.6);
                    let mut p = [c[0], c[1], c[2]];
                    let step = 0.3f32;
                    while f1 > 0.0 {
                        let (bx, by, bz) = (
                            math::floor_double(p[0]),
                            math::floor_double(p[1]),
                            math::floor_double(p[2]),
                        );
                        let id = w.block(bx, by, bz);
                        if id != block::AIR {
                            f1 -= (explosion_resistance(id) + 0.3) * step;
                        }
                        if f1 > 0.0 && (0..256).contains(&by) {
                            hit.insert((bx, by, bz));
                        }
                        for a in 0..3 {
                            p[a] += d[a] * step as f64;
                        }
                        f1 -= step * 0.75;
                    }
                }
            }
        }
    }
    // entity damage
    let f = size * 2.0;
    let apply = |pos: [f64; 3], eye: f64| -> Option<(f32, [f64; 3])> {
        let dist = ((pos[0] - c[0]).powi(2) + (pos[1] - c[1]).powi(2) + (pos[2] - c[2]).powi(2))
            .sqrt()
            / f as f64;
        if dist > 1.0 {
            return None;
        }
        let (mut dx, mut dy, mut dz) = (pos[0] - c[0], pos[1] + eye - c[1], pos[2] - c[2]);
        let l = (dx * dx + dy * dy + dz * dz).sqrt();
        if l == 0.0 {
            return None;
        }
        dx /= l;
        dy /= l;
        dz /= l;
        let density = 1.0; // world.getBlockDensity approximated as unobstructed
        let d10 = (1.0 - dist) * density;
        let dmg = ((d10 * d10 + d10) / 2.0 * 8.0 * f as f64 + 1.0) as i32 as f32;
        Some((dmg, [dx * d10, dy * d10, dz * d10]))
    };
    if player.attackable {
        if let Some((dmg, k)) = apply(player.feet, 1.62) {
            out.hits.push(Hit::Explosion {
                amount: dmg,
                knock: k,
            });
        }
    }
    for t in mobs.tnt.iter_mut() {
        if let Some((_, k)) = apply(t.pos, 0.0) {
            for a in 0..3 {
                t.motion[a] += k[a];
            }
        }
    }
    for m in mobs.list.iter_mut() {
        if let Some((dmg, k)) = apply(m.pos, m.kind.eye_height() as f64) {
            m.damage(dmg, None);
            for a in 0..3 {
                m.motion[a] += k[a];
            }
        }
    }
    // blocks
    let mut chain = Vec::new();
    let r = &mut mobs.rand;
    for &(x, y, z) in hit.iter() {
        let id = w.block(x, y, z);
        if id == block::AIR {
            continue;
        }
        if block::material(id).is_liquid() {
            continue;
        }
        let meta = w.meta(x, y, z);
        if id == block::TNT {
            // BlockTNT.onBlockDestroyedByExplosion: a short fuse, no drop.
            w.set_block(x, y, z, block::AIR, 0, 3);
            let fuse = w.rand.next_int_n(20) + 10;
            chain.push(([x as f64 + 0.5, y as f64 + 0.5, z as f64 + 0.5], fuse));
            continue;
        }
        if r.next_float() <= 1.0 / size {
            for s in crate::item::drops(id, meta, r) {
                out.drops
                    .push(([x as f64 + 0.5, y as f64 + 0.5, z as f64 + 0.5], s));
            }
        }
        w.set_block(x, y, z, block::AIR, 0, 3);
    }
    out.explosions.push([c[0], c[1], c[2], size as f64]);
    out.sounds.push((
        "random.explode",
        c,
        4.0,
        (1.0 + (r.next_float() - r.next_float()) * 0.2) * 0.7,
    ));
    for (p, fuse) in chain {
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
    if base == 14 || base == 15 {
        return Vec::new();
    }
    vec![(Some(Kind::Squid), 10, 4, 4)]
}

/// `BiomeGenBase.spawnableCaveCreatureList`
pub fn ambient_list(_biome: i32) -> Vec<Entry> {
    vec![(Some(Kind::Bat), 10, 8, 8)]
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
                        let list = match ty {
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
                        m.on_spawn(&mut rand);
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
