//! The "new AI" of `EntityLiving`: `EntityAITasks` with its two lists
//! (`tasks` and `targetTasks`), the `EntityAIBase` classes hung on them, and
//! `PathNavigate`. Each task keeps the fields of its class; the methods are
//! the class's `shouldExecute`, `continueExecuting`, `startExecuting`,
//! `resetTask` and `updateTask`, in that shape, so they read against the
//! original line by line. Random draws happen in the original order.

use super::*;

/// `EntityLivingBase` another mob's AI refers to: the player or a mob.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    Player,
    Mob(u32),
}

/// What a task reads from another entity.
#[derive(Clone, Copy)]
pub struct View {
    pub pos: [f64; 3],
    pub bb: Aabb,
    /// `getEyeHeight()`
    pub eye: f64,
    pub width: f32,
    pub alive: bool,
}

/// The world as one mob's tasks see it during its tick.
pub struct Cx<'a> {
    pub w: &'a mut ServerWorld,
    pub player: &'a PlayerInfo,
    pub others: &'a [Other],
    pub out: &'a mut Outcome,
    pub arrows: &'a mut Vec<Arrow>,
    pub strikes: &'a mut Vec<Strike>,
    /// `World.isDaytime()`
    pub day: bool,
}

impl Cx<'_> {
    pub fn view(&self, t: Target) -> Option<View> {
        match t {
            Target::Player => Some(View {
                pos: self.player.feet,
                bb: self.player.bb(),
                eye: 1.62f32 as f64,
                width: 0.6,
                alive: self.player.alive,
            }),
            Target::Mob(id) => self.others.iter().find(|o| o.id == id).map(|o| View {
                pos: o.pos,
                bb: o.bb,
                eye: (o.height * 0.85) as f64,
                width: o.width,
                alive: true,
            }),
        }
    }
}

/// The class a task was built for (`EntityPlayer.class`, ...).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Cls {
    Player,
    Kind(Kind),
    /// `EntityLiving.class`: any mob
    Living,
}

/// `EntityAITarget`'s own fields.
#[derive(Clone, Copy, Debug, Default)]
pub struct TargetState {
    unseen_ticks: i32,
}

/// One `EntityAIBase` with its fields.
#[derive(Clone, Debug)]
pub enum Ai {
    Swimming,
    Panic {
        speed: f64,
        pos: [f64; 3],
    },
    Tempt {
        speed: f64,
        item: u16,
        delay: i32,
        avoid_water: bool,
    },
    Wander {
        speed: f64,
        pos: [f64; 3],
    },
    /// `EntityAIWatchClosest`; `mutex` 3 makes it `EntityAIWatchClosest2`.
    WatchClosest {
        cls: Cls,
        dist: f32,
        chance: f32,
        mutex: i32,
        look_time: i32,
        closest: Option<Target>,
    },
    LookIdle {
        x: f64,
        z: f64,
        time: i32,
    },
    AttackOnCollide {
        cls: Option<Cls>,
        speed: f64,
        long_memory: bool,
        attack_tick: i32,
        delay: i32,
        target: [f64; 3],
        path: Option<Vec<Pt>>,
    },
    ArrowAttack {
        speed: f64,
        min: i32,
        max: i32,
        radius: f32,
        time: i32,
        see_time: i32,
        target: Option<Target>,
    },
    CreeperSwell {
        target: Option<Target>,
    },
    AvoidEntity {
        kind: Kind,
        dist: f32,
        far: f64,
        near: f64,
        closest: Option<Target>,
        path: Option<Vec<Pt>>,
    },
    FleeSun {
        speed: f64,
        pos: [f64; 3],
    },
    RestrictSun,
    EatGrass {
        timer: i32,
    },
    FollowParent {
        speed: f64,
        parent: Option<u32>,
        delay: i32,
    },
    /// The tasks of wolves, ocelots and iron golems.
    Pet(Pet),
    // --- targetTasks ---
    HurtByTarget {
        calls_for_help: bool,
        revenge_timer_old: i32,
        st: TargetState,
    },
    NearestAttackableTarget {
        cls: Cls,
        chance: i32,
        check_sight: bool,
        found: Option<Target>,
        st: TargetState,
    },
}

/// `EntityAITasks$EntityAITaskEntry`
#[derive(Clone, Debug)]
struct Entry {
    priority: i32,
    ai: Ai,
}

/// `EntityAITasks`
#[derive(Clone, Debug, Default)]
pub struct Tasks {
    entries: Vec<Entry>,
    /// `executingTaskEntries`, in the order they started.
    executing: Vec<usize>,
    tick_count: i32,
}

impl Tasks {
    pub fn add(&mut self, priority: i32, ai: Ai) {
        self.entries.push(Entry { priority, ai });
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `onUpdateTasks`
    fn update(&mut self, m: &mut Mob, cx: &mut Cx) {
        let mut started = Vec::new();
        let n = self.tick_count;
        self.tick_count += 1;
        if n % 3 == 0 {
            for i in 0..self.entries.len() {
                if self.executing.contains(&i) {
                    if self.can_use(i) && self.entries[i].ai.continue_executing(m, cx) {
                        continue;
                    }
                    self.entries[i].ai.reset_task(m, cx);
                    self.executing.retain(|&k| k != i);
                }
                if self.can_use(i) && self.entries[i].ai.should_execute(m, cx) {
                    started.push(i);
                    self.executing.push(i);
                }
            }
        } else {
            let mut k = 0;
            while k < self.executing.len() {
                let i = self.executing[k];
                if !self.entries[i].ai.continue_executing(m, cx) {
                    self.entries[i].ai.reset_task(m, cx);
                    self.executing.remove(k);
                } else {
                    k += 1;
                }
            }
        }
        for i in started {
            self.entries[i].ai.start_executing(m, cx);
        }
        for k in 0..self.executing.len() {
            let i = self.executing[k];
            self.entries[i].ai.update_task(m, cx);
        }
    }

    /// `canUse`: nothing running with a lower or equal priority number
    /// shares a mutex bit (every task here is interruptible).
    fn can_use(&self, i: usize) -> bool {
        let e = &self.entries[i];
        for (k, o) in self.entries.iter().enumerate() {
            if k != i
                && e.priority >= o.priority
                && self.executing.contains(&k)
                && e.ai.mutex() & o.ai.mutex() != 0
            {
                return false;
            }
        }
        true
    }

    /// `EntityAIEatGrass.getEatingGrassTimer` of the sheep's task.
    fn eat_grass_timer(&self) -> i32 {
        self.entries
            .iter()
            .find_map(|e| match e.ai {
                Ai::EatGrass { timer } => Some(timer),
                _ => None,
            })
            .unwrap_or(0)
    }
}

// ------------------------------------------------------------- the mob side

impl Mob {
    /// `getDistanceSqToEntity`
    fn dist2_to(&self, v: &View) -> f64 {
        self.dist2(v.pos)
    }

    /// `EntitySenses.canSee` (`canEntityBeSeen`)
    fn can_see_view(&self, w: &ServerWorld, v: &View) -> bool {
        self.can_see(w, [v.pos[0], v.pos[1] + v.eye, v.pos[2]])
    }

    /// `setRevengeTarget`
    pub(super) fn set_revenge(&mut self, t: Option<Target>) {
        self.revenge = t;
        self.revenge_timer = self.ticks;
    }

    /// `EntityLookHelper.setLookPosition`
    fn look(&mut self, p: [f64; 3], yaw: f32, pitch: f32) {
        self.look_at = Some(p);
        self.look_delta = (yaw, pitch);
    }

    /// `EntityLookHelper.setLookPositionWithEntity`
    fn look_at_entity(&mut self, v: &View, yaw: f32, pitch: f32) {
        self.look([v.pos[0], v.pos[1] + v.eye, v.pos[2]], yaw, pitch);
    }

    // ------------------------------------------------------- PathNavigate

    fn in_liquid(&self, w: &ServerWorld) -> bool {
        self.in_water || self.in_material(w, &self.bb, Material::Lava)
    }

    /// `canNavigate` (every mob here swims: `setCanSwim(true)`)
    fn can_navigate(&self, w: &ServerWorld) -> bool {
        // the iron golem has no EntityAISwimming, so no setCanSwim(true)
        self.on_ground || self.kind != Kind::IronGolem && self.in_liquid(w)
    }

    /// `PathFinder.createEntityPathTo(entity, x, y, z, range)`
    fn create_path(&self, w: &mut ServerWorld, x: f64, y: f64, z: f64) -> Option<Vec<Pt>> {
        self.create_path_with(
            w,
            x,
            y,
            z,
            self.kind.follow_range(),
            self.kind == Kind::Villager,
            self.avoids_water,
        )
    }

    /// `World.getPathEntityToEntity` / `getEntityPathToXYZ` with their
    /// range, `canPassOpenWoodenDoors` and `avoidsWater` arguments.
    #[allow(clippy::too_many_arguments)]
    fn create_path_with(
        &self,
        w: &mut ServerWorld,
        x: f64,
        y: f64,
        z: f64,
        range: f32,
        doors: bool,
        avoid_water: bool,
    ) -> Option<Vec<Pt>> {
        let (wd, h) = self.dims();
        let from = Pt {
            x: math::floor_double(self.bb.min[0]),
            y: math::floor_double(self.bb.min[1] + 0.5),
            z: math::floor_double(self.bb.min[2]),
        };
        let to = Pt {
            x: math::floor_double(x - (wd / 2.0) as f64),
            y: math::floor_double(y),
            z: math::floor_double(z - (wd / 2.0) as f64),
        };
        let size = Size {
            w: (wd + 1.0).floor() as i32,
            h: (h + 1.0).floor() as i32,
        };
        let opts = Opts {
            doors,
            avoid_water,
            can_drown: true,
        };
        let in_lava = self.in_material(w, &self.bb, Material::Lava);
        path::find(
            w,
            from,
            to,
            size,
            range,
            &opts,
            3,
            self.feet_block(),
            in_lava,
        )
    }

    /// `getPathToXYZ`
    fn path_to_xyz(&self, w: &mut ServerWorld, x: f64, y: f64, z: f64) -> Option<Vec<Pt>> {
        if !self.can_navigate(w) {
            return None;
        }
        // World.getEntityPathToXYZ: the middle of the block
        self.create_path(
            w,
            (math::floor_double(x) as f32 + 0.5) as f64,
            (y as i32 as f32 + 0.5) as f64,
            (math::floor_double(z) as f32 + 0.5) as f64,
        )
    }

    /// `getPathToEntityLiving`
    fn path_to_entity(&self, w: &mut ServerWorld, v: &View) -> Option<Vec<Pt>> {
        if !self.can_navigate(w) {
            return None;
        }
        self.create_path(w, v.pos[0], v.bb.min[1], v.pos[2])
    }

    /// `PathEntity.getVectorFromIndex`
    fn path_vec(&self, i: usize) -> [f64; 3] {
        let p = self.path[i];
        let half = ((self.width() + 1.0) as i32) as f64 * 0.5;
        [p.x as f64 + half, p.y as f64, p.z as f64 + half]
    }

    /// `setPath`
    fn set_path(&mut self, w: &mut ServerWorld, p: Option<Vec<Pt>>, speed: f64) -> bool {
        let Some(p) = p else {
            self.path.clear();
            self.path_index = 0;
            return false;
        };
        // isSamePath keeps the path in hand
        if p != self.path {
            self.path = p;
            self.path_index = 0;
        }
        if self.avoid_sun {
            self.remove_sunny_path(w);
        }
        if self.path.is_empty() {
            return false;
        }
        self.path_speed = speed;
        let v = self.nav_position(w);
        self.nav_last_tick = self.nav_ticks;
        self.nav_last_pos = v;
        true
    }

    /// `tryMoveToXYZ`
    fn try_move_to_xyz(&mut self, w: &mut ServerWorld, x: f64, y: f64, z: f64, speed: f64) -> bool {
        let p = self.path_to_xyz(
            w,
            math::floor_double(x) as f64,
            y as i32 as f64,
            math::floor_double(z) as f64,
        );
        self.set_path(w, p, speed)
    }

    /// `tryMoveToEntityLiving`
    fn try_move_to_entity(&mut self, w: &mut ServerWorld, v: &View, speed: f64) -> bool {
        match self.path_to_entity(w, v) {
            Some(p) => self.set_path(w, Some(p), speed),
            None => false,
        }
    }

    fn clear_path(&mut self) {
        self.path.clear();
        self.path_index = 0;
    }

    /// `removeSunnyPath`
    fn remove_sunny_path(&mut self, w: &mut ServerWorld) {
        let (x, z) = (
            math::floor_double(self.pos[0]),
            math::floor_double(self.pos[2]),
        );
        if !w.can_see_sky(x, (self.bb.min[1] + 0.5) as i32, z) {
            for i in 0..self.path.len() {
                let p = self.path[i];
                if w.can_see_sky(p.x, p.y, p.z) {
                    // setCurrentPathLength(i - 1)
                    self.path.truncate(i.saturating_sub(1));
                    return;
                }
            }
        }
    }

    /// `getEntityPosition` with `getPathableYPos`
    fn nav_position(&self, w: &mut ServerWorld) -> [f64; 3] {
        let y = if self.in_water {
            let (x, z) = (
                math::floor_double(self.pos[0]),
                math::floor_double(self.pos[2]),
            );
            let mut y = self.bb.min[1] as i32;
            let mut b = w.block(x, y, z);
            let mut n = 0;
            loop {
                if b != block::FLOWING_WATER && b != block::WATER {
                    break y;
                }
                y += 1;
                b = w.block(x, y, z);
                n += 1;
                if n > 16 {
                    break self.bb.min[1] as i32;
                }
            }
        } else {
            (self.bb.min[1] + 0.5) as i32
        };
        [self.pos[0], y as f64, self.pos[2]]
    }

    /// `onUpdateNavigation`
    fn nav_update(&mut self, w: &mut ServerWorld) {
        self.nav_ticks += 1;
        if !self.no_path() {
            if self.can_navigate(w) {
                self.path_follow(w);
            }
            if !self.no_path() {
                let v = self.path_vec(self.path_index);
                self.move_to = Some(v);
                self.move_speed = self.path_speed;
            }
        }
    }

    /// `pathFollow`
    fn path_follow(&mut self, w: &mut ServerWorld) {
        let pos = self.nav_position(w);
        let d2 = |a: [f64; 3], b: [f64; 3]| {
            (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
        };
        let mut end = self.path.len();
        for i in self.path_index..self.path.len() {
            if self.path[i].y != pos[1] as i32 {
                end = i;
                break;
            }
        }
        let reach = (self.width() * self.width()) as f64;
        for i in self.path_index..end {
            if d2(pos, self.path_vec(i)) < reach {
                self.path_index = i + 1;
            }
        }
        let sx = math::ceiling_float_int(self.width());
        let sy = self.height() as i32 + 1;
        let mut i = end as i32 - 1;
        while i >= self.path_index as i32 {
            if self.is_direct_path(w, pos, self.path_vec(i as usize), sx, sy, sx) {
                self.path_index = i as usize;
                break;
            }
            i -= 1;
        }
        if self.nav_ticks - self.nav_last_tick > 100 {
            if d2(pos, self.nav_last_pos) < 2.25 {
                self.clear_path();
            }
            self.nav_last_tick = self.nav_ticks;
            self.nav_last_pos = pos;
        }
    }

    /// `isDirectPathBetweenPoints`
    #[allow(clippy::too_many_arguments)]
    fn is_direct_path(
        &self,
        w: &mut ServerWorld,
        a: [f64; 3],
        b: [f64; 3],
        mut sx: i32,
        sy: i32,
        mut sz: i32,
    ) -> bool {
        let mut x = math::floor_double(a[0]);
        let mut z = math::floor_double(a[2]);
        let mut dx = b[0] - a[0];
        let mut dz = b[2] - a[2];
        let len2 = dx * dx + dz * dz;
        if len2 < 1.0e-8 {
            return false;
        }
        let inv = 1.0 / len2.sqrt();
        dx *= inv;
        dz *= inv;
        sx += 2;
        sz += 2;
        if !self.is_safe_to_stand_at(w, x, a[1] as i32, z, sx, sy, sz, a, dx, dz) {
            return false;
        }
        sx -= 2;
        sz -= 2;
        let step_x = 1.0 / dx.abs();
        let step_z = 1.0 / dz.abs();
        let mut tx = x as f64 - a[0];
        let mut tz = z as f64 - a[2];
        if dx >= 0.0 {
            tx += 1.0;
        }
        if dz >= 0.0 {
            tz += 1.0;
        }
        tx /= dx;
        tz /= dz;
        let ix = if dx < 0.0 { -1 } else { 1 };
        let iz = if dz < 0.0 { -1 } else { 1 };
        let ex = math::floor_double(b[0]);
        let ez = math::floor_double(b[2]);
        let mut rx = ex - x;
        let mut rz = ez - z;
        while rx * ix > 0 || rz * iz > 0 {
            if tx < tz {
                tx += step_x;
                x += ix;
                rx = ex - x;
            } else {
                tz += step_z;
                z += iz;
                rz = ez - z;
            }
            if !self.is_safe_to_stand_at(w, x, a[1] as i32, z, sx, sy, sz, a, dx, dz) {
                return false;
            }
        }
        true
    }

    /// `isSafeToStandAt`
    #[allow(clippy::too_many_arguments)]
    fn is_safe_to_stand_at(
        &self,
        w: &mut ServerWorld,
        x: i32,
        y: i32,
        z: i32,
        sx: i32,
        sy: i32,
        sz: i32,
        from: [f64; 3],
        dx: f64,
        dz: f64,
    ) -> bool {
        let x0 = x - sx / 2;
        let z0 = z - sz / 2;
        if !self.is_position_clear(w, x0, y, z0, sx, sy, sz, from, dx, dz) {
            return false;
        }
        for bx in x0..x0 + sx {
            for bz in z0..z0 + sz {
                let ox = bx as f64 + 0.5 - from[0];
                let oz = bz as f64 + 0.5 - from[2];
                if !(ox * dx + oz * dz < 0.0) {
                    let m = block::material(w.block(bx, y - 1, bz));
                    if m == Material::Air {
                        return false;
                    }
                    if m == Material::Water && !self.in_water {
                        return false;
                    }
                    if m == Material::Lava {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// `isPositionClear`
    #[allow(clippy::too_many_arguments)]
    fn is_position_clear(
        &self,
        w: &mut ServerWorld,
        x0: i32,
        y0: i32,
        z0: i32,
        sx: i32,
        sy: i32,
        sz: i32,
        from: [f64; 3],
        dx: f64,
        dz: f64,
    ) -> bool {
        for x in x0..x0 + sx {
            for y in y0..y0 + sy {
                for z in z0..z0 + sz {
                    let ox = x as f64 + 0.5 - from[0];
                    let oz = z as f64 + 0.5 - from[2];
                    if !(ox * dx + oz * dz < 0.0) && !blocks_movement_free(w, x, y, z) {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// `EntityLookHelper.onUpdateLook`
    fn look_update(&mut self) {
        self.pitch = 0.0;
        let turn = |from: f32, to: f32, max: f32| from + wrap(to - from).clamp(-max, max);
        if let Some(t) = self.look_at.take() {
            let (dy, dp) = self.look_delta;
            let dx = t[0] - self.pos[0];
            let dh = t[1] - (self.pos[1] + (self.height() * 0.85) as f64);
            let dz = t[2] - self.pos[2];
            let d = math::sqrt_double(dx * dx + dz * dz) as f64;
            let yaw = (dz.atan2(dx) * 180.0 / std::f32::consts::PI as f64) as f32 - 90.0;
            let pitch = (-(dh.atan2(d) * 180.0 / std::f32::consts::PI as f64)) as f32;
            self.pitch = turn(self.pitch, pitch, dp);
            self.head_yaw = turn(self.head_yaw, yaw, dy);
        } else {
            self.head_yaw = turn(self.head_yaw, self.body_yaw, 10.0);
        }
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

/// `Block.getBlocksMovement`: true where a mob may walk through.
fn blocks_movement_free(w: &mut ServerWorld, x: i32, y: i32, z: i32) -> bool {
    let id = w.block(x, y, z);
    match id {
        // BlockBasePressurePlate, BlockSign
        block::STONE_PRESSURE_PLATE
        | block::WOODEN_PRESSURE_PLATE
        | block::STANDING_SIGN
        | block::WALL_SIGN => true,
        // BlockDoor: open
        block::WOODEN_DOOR | block::IRON_DOOR => {
            let m = w.meta(x, y, z) as u8;
            let ww = &*w;
            let get = |x: i32, y: i32, z: i32| (ww.peek_block(x, y, z), ww.peek_meta(x, y, z));
            crate::shapes::door_full_meta(&get, x, y, z, m) & 4 != 0
        }
        // BlockFence
        block::FENCE | block::NETHER_BRICK_FENCE => false,
        // BlockLiquid
        block::FLOWING_WATER | block::WATER => true,
        block::FLOWING_LAVA | block::LAVA => false,
        // BlockTrapDoor: closed
        block::TRAPDOOR => w.meta(x, y, z) & 4 == 0,
        _ => !block::material(id).blocks_movement(),
    }
}

// ------------------------------------------------------------------ tasks

/// `World.getClosestPlayerToEntity(entity, distance)`
fn closest_player(m: &Mob, cx: &Cx, dist: f64) -> Option<Target> {
    let d = m.dist2(cx.player.feet);
    if dist < 0.0 || d < dist * dist {
        Some(Target::Player)
    } else {
        None
    }
}

/// The mobs of a class whose boxes touch `bb`, in list order.
fn mobs_within<'a>(m: &'a Mob, cx: &'a Cx, cls: Cls, bb: Aabb) -> impl Iterator<Item = &'a Other> {
    cx.others.iter().filter(move |o| {
        o.id != m.id
            && match cls {
                Cls::Kind(k) => o.kind == k,
                Cls::Living => true,
                Cls::Player => false,
            }
            && o.bb.intersects(&bb)
    })
}

fn expand(b: &Aabb, x: f64, y: f64, z: f64) -> Aabb {
    Aabb {
        min: [b.min[0] - x, b.min[1] - y, b.min[2] - z],
        max: [b.max[0] + x, b.max[1] + y, b.max[2] + z],
    }
}

fn is_class(t: Target, cls: Cls, cx: &Cx) -> bool {
    match (t, cls) {
        (Target::Player, Cls::Player) => true,
        (Target::Mob(_), Cls::Living) => true,
        (Target::Mob(id), Cls::Kind(k)) => cx.others.iter().any(|o| o.id == id && o.kind == k),
        _ => false,
    }
}

/// `EntityAITarget.isSuitableTarget` (none of the mobs here search
/// `nearbyOnly`, has a home or an owner).
fn is_suitable_target(m: &Mob, t: Option<Target>, check_sight: bool, cx: &Cx) -> bool {
    let Some(t) = t else { return false };
    if t == Target::Mob(m.id) {
        return false;
    }
    let Some(v) = cx.view(t) else { return false };
    if !v.alive {
        return false;
    }
    // EntityPlayer.capabilities.disableDamage
    if t == Target::Player && !cx.player.attackable {
        return false;
    }
    if check_sight && !m.can_see_view(cx.w, &v) {
        return false;
    }
    true
}

/// `EntityAITarget.continueExecuting`
fn target_continue(m: &mut Mob, check_sight: bool, st: &mut TargetState, cx: &Cx) -> bool {
    let Some(t) = m.attack_target else {
        return false;
    };
    let Some(v) = cx.view(t) else { return false };
    if !v.alive {
        return false;
    }
    let range = m.kind.follow_range() as f64;
    if m.dist2_to(&v) > range * range {
        return false;
    }
    if check_sight {
        if m.can_see_view(cx.w, &v) {
            st.unseen_ticks = 0;
        } else {
            st.unseen_ticks += 1;
            if st.unseen_ticks > 60 {
                return false;
            }
        }
    }
    // a player in creative mode
    !(t == Target::Player && !cx.player.attackable)
}

impl Ai {
    /// `getMutexBits`
    fn mutex(&self) -> i32 {
        match self {
            Ai::Pet(p) => p.mutex(),
            Ai::Swimming => 4,
            Ai::Panic { .. } | Ai::Wander { .. } | Ai::CreeperSwell { .. } => 1,
            Ai::AvoidEntity { .. } | Ai::FleeSun { .. } => 1,
            Ai::Tempt { .. } | Ai::LookIdle { .. } => 3,
            Ai::AttackOnCollide { .. } | Ai::ArrowAttack { .. } => 3,
            Ai::WatchClosest { mutex, .. } => *mutex,
            Ai::RestrictSun | Ai::FollowParent { .. } => 0,
            Ai::EatGrass { .. } => 7,
            Ai::HurtByTarget { .. } | Ai::NearestAttackableTarget { .. } => 1,
        }
    }

    fn should_execute(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        match self {
            Ai::Pet(p) => p.should_execute(m, cx),
            Ai::Swimming => m.in_liquid(cx.w),
            Ai::Panic { pos, .. } => {
                if m.revenge.is_none() && m.fire <= 0 {
                    return false;
                }
                match m.random_target(cx.w, 5, 4, None) {
                    Some(p) => {
                        *pos = p;
                        true
                    }
                    None => false,
                }
            }
            Ai::Tempt { item, delay, .. } => {
                if *delay > 0 {
                    *delay -= 1;
                    return false;
                }
                if closest_player(m, cx, 10.0).is_none() {
                    return false;
                }
                cx.player.held == Some(*item)
            }
            Ai::Wander { pos, .. } => {
                if m.age >= 100 {
                    return false;
                }
                if m.rand.next_int_n(120) != 0 {
                    return false;
                }
                match m.random_target(cx.w, 10, 7, None) {
                    Some(p) => {
                        *pos = p;
                        true
                    }
                    None => false,
                }
            }
            Ai::WatchClosest {
                cls,
                dist,
                chance,
                closest,
                ..
            } => {
                if m.rand.next_float() >= *chance {
                    return false;
                }
                *closest = if *cls == Cls::Player {
                    closest_player(m, cx, *dist as f64)
                } else {
                    // findNearestEntityWithinAABB
                    let bb = expand(&m.bb, *dist as f64, 3.0, *dist as f64);
                    let mut best: Option<(f64, u32)> = None;
                    for o in mobs_within(m, cx, *cls, bb) {
                        let d = m.dist2(o.pos);
                        if best.map_or(true, |b| d <= b.0) {
                            best = Some((d, o.id));
                        }
                    }
                    best.map(|b| Target::Mob(b.1))
                };
                closest.is_some()
            }
            Ai::LookIdle { .. } => m.rand.next_float() < 0.02,
            Ai::AttackOnCollide { cls, path, .. } => {
                let Some(t) = m.attack_target else {
                    return false;
                };
                let Some(v) = cx.view(t) else { return false };
                if !v.alive {
                    return false;
                }
                if let Some(c) = cls {
                    if !is_class(t, *c, cx) {
                        return false;
                    }
                }
                *path = m.path_to_entity(cx.w, &v);
                path.is_some()
            }
            Ai::ArrowAttack { target, .. } => match m.attack_target {
                Some(t) if cx.view(t).is_some() => {
                    *target = Some(t);
                    true
                }
                _ => false,
            },
            Ai::CreeperSwell { .. } => {
                m.fuse_dir > 0
                    || m.attack_target
                        .and_then(|t| cx.view(t))
                        .is_some_and(|v| m.dist2_to(&v) < 9.0)
            }
            Ai::AvoidEntity {
                kind,
                dist,
                closest,
                path,
                ..
            } => {
                let bb = expand(&m.bb, *dist as f64, 3.0, *dist as f64);
                // the entityFilter: alive and in sight; the first one found
                let mut first = None;
                for o in mobs_within(m, cx, Cls::Kind(*kind), bb) {
                    if m.can_see(
                        cx.w,
                        [o.pos[0], o.pos[1] + (o.height * 0.85) as f64, o.pos[2]],
                    ) {
                        first = Some((o.id, o.pos));
                        break;
                    }
                }
                let Some((id, at)) = first else { return false };
                *closest = Some(Target::Mob(id));
                let Some(v) = m.random_target(cx.w, 16, 7, Some(at)) else {
                    return false;
                };
                let d = |a: [f64; 3], b: [f64; 3]| {
                    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
                };
                if d(at, v) < d(at, m.pos) {
                    return false;
                }
                *path = m.path_to_xyz(cx.w, v[0], v[1], v[2]);
                // PathEntity.isDestinationSame
                path.as_ref()
                    .and_then(|p| p.last())
                    .is_some_and(|e| e.x == v[0] as i32 && e.y == v[1] as i32 && e.z == v[2] as i32)
            }
            Ai::FleeSun { pos, .. } => {
                if !cx.day || m.fire <= 0 {
                    return false;
                }
                let (x, z) = (math::floor_double(m.pos[0]), math::floor_double(m.pos[2]));
                if !cx.w.can_see_sky(x, m.bb.min[1] as i32, z) {
                    return false;
                }
                // findPossibleShelter
                for _ in 0..10 {
                    let x = math::floor_double(m.pos[0] + m.rand.next_int_n(20) as f64 - 10.0);
                    let y = math::floor_double(m.bb.min[1] + m.rand.next_int_n(6) as f64 - 3.0);
                    let z = math::floor_double(m.pos[2] + m.rand.next_int_n(20) as f64 - 10.0);
                    if !cx.w.can_see_sky(x, y, z) && m.path_weight(cx.w, x, y, z) < 0.0 {
                        *pos = [x as f64, y as f64, z as f64];
                        return true;
                    }
                }
                false
            }
            Ai::RestrictSun => cx.day,
            Ai::EatGrass { .. } => {
                let child = m.growing_age < 0;
                if m.rand.next_int_n(if child { 50 } else { 1000 }) != 0 {
                    return false;
                }
                let (x, y, z) = m.feet_block();
                if cx.w.block(x, y, z) == block::TALLGRASS && cx.w.meta(x, y, z) == 1 {
                    true
                } else {
                    cx.w.block(x, y - 1, z) == block::GRASS
                }
            }
            Ai::FollowParent { parent, .. } => {
                if m.growing_age >= 0 {
                    return false;
                }
                let bb = expand(&m.bb, 8.0, 4.0, 8.0);
                let mut best: Option<(f64, u32)> = None;
                for o in mobs_within(m, cx, Cls::Kind(m.kind), bb) {
                    if o.growing_age >= 0 {
                        let d = m.dist2(o.pos);
                        if best.map_or(true, |b| !(d > b.0)) {
                            best = Some((d, o.id));
                        }
                    }
                }
                let Some((d, id)) = best else { return false };
                if d < 9.0 {
                    return false;
                }
                *parent = Some(id);
                true
            }
            Ai::HurtByTarget {
                revenge_timer_old, ..
            } => {
                m.revenge_timer != *revenge_timer_old && is_suitable_target(m, m.revenge, false, cx)
            }
            Ai::NearestAttackableTarget {
                cls,
                chance,
                check_sight,
                found,
                ..
            } => {
                if *chance > 0 && m.rand.next_int_n(*chance) != 0 {
                    return false;
                }
                let range = m.kind.follow_range() as f64;
                let bb = expand(&m.bb, range, 4.0, range);
                // selectEntitiesWithinAABB, sorted by distance
                let mut best: Option<(f64, Target)> = None;
                if *cls == Cls::Player {
                    if cx.player.bb().intersects(&bb)
                        && is_suitable_target(m, Some(Target::Player), *check_sight, cx)
                    {
                        best = Some((m.dist2(cx.player.feet), Target::Player));
                    }
                } else {
                    let ids: Vec<(u32, [f64; 3])> = mobs_within(m, cx, *cls, bb)
                        .map(|o| (o.id, o.pos))
                        .collect();
                    for (id, pos) in ids {
                        if is_suitable_target(m, Some(Target::Mob(id)), *check_sight, cx) {
                            let d = m.dist2(pos);
                            if best.map_or(true, |b| d < b.0) {
                                best = Some((d, Target::Mob(id)));
                            }
                        }
                    }
                }
                *found = best.map(|b| b.1);
                found.is_some()
            }
        }
    }

    fn continue_executing(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        match self {
            Ai::Pet(p) => p.continue_executing(m, cx),
            Ai::Panic { .. } | Ai::Wander { .. } | Ai::AvoidEntity { .. } | Ai::FleeSun { .. } => {
                !m.no_path()
            }
            Ai::WatchClosest {
                dist,
                look_time,
                closest,
                ..
            } => {
                let Some(v) = closest.and_then(|t| cx.view(t)) else {
                    return false;
                };
                if !v.alive {
                    return false;
                }
                if m.dist2_to(&v) > (*dist * *dist) as f64 {
                    false
                } else {
                    *look_time > 0
                }
            }
            Ai::LookIdle { time, .. } => *time >= 0,
            Ai::AttackOnCollide { long_memory, .. } => {
                let Some(v) = m.attack_target.and_then(|t| cx.view(t)) else {
                    return false;
                };
                if !v.alive {
                    return false;
                }
                // isWithinHomeDistance: no home here
                *long_memory || !m.no_path()
            }
            Ai::ArrowAttack { .. } => self.should_execute(m, cx) || !m.no_path(),
            Ai::EatGrass { timer } => *timer > 0,
            Ai::FollowParent { parent, .. } => {
                let Some(v) = parent.and_then(|id| cx.view(Target::Mob(id))) else {
                    return false;
                };
                let d = m.dist2_to(&v);
                !(d < 9.0) && !(d > 256.0)
            }
            Ai::HurtByTarget { st, .. } => target_continue(m, false, st, cx),
            Ai::NearestAttackableTarget {
                check_sight, st, ..
            } => target_continue(m, *check_sight, st, cx),
            _ => self.should_execute(m, cx),
        }
    }

    fn start_executing(&mut self, m: &mut Mob, cx: &mut Cx) {
        match self {
            Ai::Panic { speed, pos } | Ai::Wander { speed, pos } | Ai::FleeSun { speed, pos } => {
                m.try_move_to_xyz(cx.w, pos[0], pos[1], pos[2], *speed);
            }
            Ai::Pet(p) => p.start_executing(m, cx),
            Ai::Tempt { avoid_water, .. } => {
                *avoid_water = m.avoids_water;
                m.avoids_water = false;
            }
            Ai::WatchClosest { look_time, .. } => {
                *look_time = 40 + m.rand.next_int_n(40);
            }
            Ai::LookIdle { x, z, time } => {
                let a = std::f64::consts::PI * 2.0 * m.rand.next_double();
                *x = a.cos();
                *z = a.sin();
                *time = 20 + m.rand.next_int_n(20);
            }
            Ai::AttackOnCollide {
                speed, delay, path, ..
            } => {
                let p = path.take();
                m.set_path(cx.w, p, *speed);
                *delay = 0;
            }
            Ai::CreeperSwell { target } => {
                m.clear_path();
                *target = m.attack_target;
            }
            Ai::AvoidEntity { far, path, .. } => {
                let p = path.take();
                m.set_path(cx.w, p, *far);
            }
            Ai::RestrictSun => m.avoid_sun = true,
            Ai::EatGrass { timer } => {
                *timer = 40;
                // setEntityState(10): the client lowers the head for 40 ticks
                m.eat_timer = 40;
                m.clear_path();
            }
            Ai::FollowParent { delay, .. } => *delay = 0,
            Ai::HurtByTarget {
                calls_for_help,
                revenge_timer_old,
                st,
            } => {
                m.attack_target = m.revenge;
                *revenge_timer_old = m.revenge_timer;
                if *calls_for_help {
                    if let Some(t) = m.revenge {
                        let range = m.kind.follow_range() as f64;
                        cx.out.calls.push(Call {
                            from: m.id,
                            kind: m.kind,
                            bb: Aabb {
                                min: [m.pos[0] - range, m.pos[1] - 10.0, m.pos[2] - range],
                                max: [
                                    m.pos[0] + 1.0 + range,
                                    m.pos[1] + 1.0 + 10.0,
                                    m.pos[2] + 1.0 + range,
                                ],
                            },
                            target: t,
                        });
                    }
                }
                *st = TargetState::default();
            }
            Ai::NearestAttackableTarget { found, st, .. } => {
                m.attack_target = *found;
                *st = TargetState::default();
            }
            _ => {}
        }
    }

    fn reset_task(&mut self, m: &mut Mob, _cx: &mut Cx) {
        match self {
            Ai::Tempt {
                delay, avoid_water, ..
            } => {
                m.clear_path();
                *delay = 100;
                m.avoids_water = *avoid_water;
            }
            Ai::WatchClosest { closest, .. } => *closest = None,
            Ai::AttackOnCollide { .. } => m.clear_path(),
            Ai::ArrowAttack {
                time,
                see_time,
                target,
                ..
            } => {
                *target = None;
                *see_time = 0;
                *time = -1;
            }
            Ai::CreeperSwell { target } => *target = None,
            Ai::Pet(p) => p.reset_task(m),
            Ai::AvoidEntity { closest, .. } => *closest = None,
            Ai::RestrictSun => m.avoid_sun = false,
            Ai::EatGrass { timer } => *timer = 0,
            Ai::FollowParent { parent, .. } => *parent = None,
            Ai::HurtByTarget { .. } | Ai::NearestAttackableTarget { .. } => {
                m.attack_target = None;
            }
            _ => {}
        }
    }

    fn update_task(&mut self, m: &mut Mob, cx: &mut Cx) {
        match self {
            Ai::Pet(p) => p.update_task(m, cx),
            Ai::Swimming => {
                if m.rand.next_float() < 0.8 {
                    m.jumping = true;
                }
            }
            Ai::Tempt { speed, .. } => {
                let v = cx.view(Target::Player).unwrap();
                m.look_at_entity(&v, 30.0, 40.0);
                if m.dist2_to(&v) < 6.25 {
                    m.clear_path();
                } else {
                    m.try_move_to_entity(cx.w, &v, *speed);
                }
            }
            Ai::WatchClosest {
                look_time, closest, ..
            } => {
                if let Some(v) = closest.and_then(|t| cx.view(t)) {
                    m.look([v.pos[0], v.pos[1] + v.eye, v.pos[2]], 10.0, 40.0);
                }
                *look_time -= 1;
            }
            Ai::LookIdle { x, z, time } => {
                *time -= 1;
                let eye = (m.height() * 0.85) as f64;
                m.look([m.pos[0] + *x, m.pos[1] + eye, m.pos[2] + *z], 10.0, 40.0);
            }
            Ai::AttackOnCollide {
                speed,
                long_memory,
                attack_tick,
                delay,
                target,
                ..
            } => {
                let Some(t) = m.attack_target else { return };
                let Some(v) = cx.view(t) else { return };
                m.look_at_entity(&v, 30.0, 30.0);
                let d2 = m.dist2([v.pos[0], v.bb.min[1], v.pos[2]]);
                let reach = (m.width() * 2.0 * (m.width() * 2.0) + v.width) as f64;
                *delay -= 1;
                if (*long_memory || m.can_see_view(cx.w, &v))
                    && *delay <= 0
                    && (*target == [0.0; 3]
                        || (v.pos[0] - target[0]).powi(2)
                            + (v.pos[1] - target[1]).powi(2)
                            + (v.pos[2] - target[2]).powi(2)
                            >= 1.0
                        || m.rand.next_float() < 0.05)
                {
                    *target = [v.pos[0], v.bb.min[1], v.pos[2]];
                    *delay = 4 + m.rand.next_int_n(7);
                    if d2 > 1024.0 {
                        *delay += 10;
                    } else if d2 > 256.0 {
                        *delay += 5;
                    }
                    if !m.try_move_to_entity(cx.w, &v, *speed) {
                        *delay += 15;
                    }
                }
                *attack_tick = (*attack_tick - 1).max(0);
                if d2 <= reach && *attack_tick <= 20 {
                    *attack_tick = 20;
                    attack_as_mob(m, t, &v, cx);
                }
            }
            Ai::ArrowAttack {
                speed,
                min,
                max,
                radius,
                time,
                see_time,
                target,
            } => {
                let Some(t) = *target else { return };
                let Some(v) = cx.view(t) else { return };
                let d2 = m.dist2([v.pos[0], v.bb.min[1], v.pos[2]]);
                let sees = m.can_see_view(cx.w, &v);
                if sees {
                    *see_time += 1;
                } else {
                    *see_time = 0;
                }
                if !(d2 > (*radius * *radius) as f64) && *see_time >= 20 {
                    m.clear_path();
                } else {
                    m.try_move_to_entity(cx.w, &v, *speed);
                }
                m.look_at_entity(&v, 30.0, 30.0);
                *time -= 1;
                if *time == 0 {
                    if d2 > (*radius * *radius) as f64 || !sees {
                        return;
                    }
                    let f = math::sqrt_double(d2) / *radius;
                    ranged_attack(m, t, &v, f.clamp(0.1, 1.0), cx);
                    *time = math::floor_float(f * (*max - *min) as f32 + *min as f32);
                } else if *time < 0 {
                    let f = math::sqrt_double(d2) / *radius;
                    *time = math::floor_float(f * (*max - *min) as f32 + *min as f32);
                }
            }
            Ai::CreeperSwell { target } => {
                // setCreeperState
                m.fuse_dir = match target.and_then(|t| cx.view(t)) {
                    None => -1,
                    Some(v) if m.dist2_to(&v) > 49.0 => -1,
                    Some(v) if !m.can_see_view(cx.w, &v) => -1,
                    Some(_) => 1,
                };
            }
            Ai::AvoidEntity {
                far, near, closest, ..
            } => {
                if let Some(v) = closest.and_then(|t| cx.view(t)) {
                    m.path_speed = if m.dist2_to(&v) < 49.0 { *near } else { *far };
                }
            }
            Ai::EatGrass { timer } => {
                *timer = (*timer - 1).max(0);
                if *timer == 4 {
                    let (x, y, z) = m.feet_block();
                    if cx.w.block(x, y, z) == block::TALLGRASS {
                        // World.breakBlock(x, y, z, false): gone, no drop
                        cx.w.set_block(x, y, z, block::AIR, 0, 3);
                        eat_grass_bonus(m);
                    } else if cx.w.block(x, y - 1, z) == block::GRASS {
                        cx.w.set_block(x, y - 1, z, block::DIRT, 0, 2);
                        eat_grass_bonus(m);
                    }
                }
            }
            Ai::FollowParent {
                speed,
                parent,
                delay,
            } => {
                *delay -= 1;
                if *delay <= 0 {
                    *delay = 10;
                    if let Some(v) = parent.and_then(|id| cx.view(Target::Mob(id))) {
                        m.try_move_to_entity(cx.w, &v, *speed);
                    }
                }
            }
            _ => {}
        }
    }
}

/// `EntitySheep.eatGrassBonus`: the wool grows back; a lamb grows up a
/// minute sooner (`addGrowth(60)`).
fn eat_grass_bonus(m: &mut Mob) {
    m.sheared = false;
    if m.growing_age < 0 {
        m.growing_age = (m.growing_age + 60 * 20).min(0);
    }
}

/// `attackEntityAsMob` of the mob's class.
fn attack_as_mob(m: &mut Mob, t: Target, v: &View, cx: &mut Cx) {
    // EntityCreeper.attackEntityAsMob does nothing
    if m.kind == Kind::Creeper {
        return;
    }
    // swingItem only with something in the hand
    if m.kind == Kind::Skeleton {
        m.is_swinging = true;
        m.swing_ticks = 0;
    }
    let mut amount = m.kind.attack_damage();
    let mut effect = None;
    if m.kind == Kind::Skeleton && m.variant == 1 {
        // a stone sword on the 4.0 base, and EntitySkeleton's wither
        amount = 4.0 + 5.0;
        effect = Some((crate::potion::WITHER, 200));
    }
    if m.kind == Kind::PigZombie {
        // the golden sword
        amount += 4.0;
    }
    let mut lift = 0.0;
    match m.kind {
        // EntityWolf.attackEntityAsMob
        Kind::Wolf => amount = if m.tamed { 4.0 } else { 2.0 },
        // EntityOcelot.attackEntityAsMob
        Kind::Ocelot => amount = 3.0,
        // EntityIronGolem.attackEntityAsMob: thrown upwards
        Kind::IronGolem => {
            m.attack_timer = 10;
            amount = (7 + m.rand.next_int_n(15)) as f32;
            lift = 0.4f32 as f64;
            cx.out.sounds.push(("mob.irongolem.throw", m.pos, 1.0, 1.0));
        }
        _ => {}
    }
    if m.kind == Kind::CaveSpider {
        // EntityCaveSpider.attackEntityAsMob: poison by difficulty
        effect = Some((crate::potion::POISON, -1));
    }
    match t {
        Target::Player => {
            cx.out.hits.push(Hit::Melee {
                kind: m.kind,
                amount,
                knock: [0.0, lift, 0.0],
                from: m.pos,
                effect,
            });
            cx.out.attacker = Some(m.id);
        }
        Target::Mob(id) => {
            let _ = v;
            cx.strikes.push(Strike {
                target: id,
                from: m.id,
                amount,
                at: m.pos,
                lift,
            });
        }
    }
}

/// `IRangedAttackMob.attackEntityWithRangedAttack(target, f)`
fn ranged_attack(m: &mut Mob, t: Target, v: &View, f: f32, cx: &mut Cx) {
    let player = cx.player;
    if m.kind == Kind::Skeleton {
        let diff = player.difficulty;
        let id = m.rand.next_int() as u32;
        // posY + getEyeHeight
        let eye = m.pos[1] + (m.dims().1 * 0.85) as f64;
        let height = (v.bb.max[1] - v.bb.min[1]) as f32;
        let mut a = Arrow::at_target(
            id,
            crate::entity::arrow::Shooter::Mob(m.id),
            m.pos,
            eye,
            v.pos,
            v.pos[1],
            height,
            1.6,
            (14 - diff * 4) as f32,
        );
        a.damage =
            (f * 2.0) as f64 + (m.rand.next_gaussian() * 0.25 + (diff as f32 * 0.11f32) as f64);
        if m.variant == 1 {
            a.e.set_fire(100);
        }
        cx.arrows.push(a);
        cx.out.sounds.push((
            "random.bow",
            m.pos,
            1.0,
            1.0 / (m.rand.next_float() * 0.4 + 0.8),
        ));
    } else if t == Target::Player {
        // EntityWitch
        let id = m.rand.next_int() as u32;
        let eye = m.pos[1] + (m.dims().1 * 0.85) as f64;
        let mut p = crate::entity::throwable::Throwable::thrown(
            id,
            crate::entity::throwable::Kind::Potion(32732),
            crate::entity::arrow::Shooter::Mob(m.id),
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
        p.kind = crate::entity::throwable::Kind::Potion(meta);
        p.set_heading(dx, dy + (d * 0.2) as f64, dz, 0.75, 8.0);
        cx.out.thrown.push(p);
    }
}

// ------------------------------------------------------------ the mob lists

/// The mobs whose `isAIEnabled()` is true and whose constructors are
/// ported here.
pub fn uses_tasks(k: Kind) -> bool {
    matches!(
        k,
        Kind::Zombie
            | Kind::Skeleton
            | Kind::Creeper
            | Kind::Witch
            | Kind::Pig
            | Kind::Cow
            | Kind::Mooshroom
            | Kind::Sheep
            | Kind::Chicken
            | Kind::Villager
            | Kind::Wolf
            | Kind::Ocelot
            | Kind::IronGolem
    )
}

fn watch(cls: Cls, dist: f32) -> Ai {
    Ai::WatchClosest {
        cls,
        dist,
        chance: 0.02,
        mutex: 2,
        look_time: 0,
        closest: None,
    }
}

fn watch2(cls: Cls, dist: f32, chance: f32) -> Ai {
    Ai::WatchClosest {
        cls,
        dist,
        chance,
        mutex: 3,
        look_time: 0,
        closest: None,
    }
}

fn wander(speed: f64) -> Ai {
    Ai::Wander {
        speed,
        pos: [0.0; 3],
    }
}

fn look_idle() -> Ai {
    Ai::LookIdle {
        x: 0.0,
        z: 0.0,
        time: 0,
    }
}

fn panic(speed: f64) -> Ai {
    Ai::Panic {
        speed,
        pos: [0.0; 3],
    }
}

fn tempt(speed: f64, item: u16) -> Ai {
    Ai::Tempt {
        speed,
        item,
        delay: 0,
        avoid_water: false,
    }
}

fn follow_parent(speed: f64) -> Ai {
    Ai::FollowParent {
        speed,
        parent: None,
        delay: 0,
    }
}

fn attack_on_collide(cls: Option<Cls>, speed: f64, long_memory: bool) -> Ai {
    Ai::AttackOnCollide {
        cls,
        speed,
        long_memory,
        attack_tick: 0,
        delay: 0,
        target: [0.0; 3],
        path: None,
    }
}

fn arrow_attack(speed: f64, min: i32, max: i32, radius: f32) -> Ai {
    Ai::ArrowAttack {
        speed,
        min,
        max,
        radius,
        time: -1,
        see_time: 0,
        target: None,
    }
}

fn hurt_by_target(calls_for_help: bool) -> Ai {
    Ai::HurtByTarget {
        calls_for_help,
        revenge_timer_old: 0,
        st: TargetState::default(),
    }
}

fn nearest_target(cls: Cls, check_sight: bool) -> Ai {
    Ai::NearestAttackableTarget {
        cls,
        chance: 0,
        check_sight,
        found: None,
        st: TargetState::default(),
    }
}

/// The constructor of the mob's class: its `tasks` and `targetTasks`.
/// Tasks that need systems not ported yet are left out and listed in
/// docs/HANDOFF.md (breeding, doors, villages, trading, riding).
fn build(m: &mut Mob) {
    let (t, tt) = (&mut m.tasks, &mut m.target_tasks);
    let player = Cls::Player;
    match m.kind {
        Kind::Zombie => {
            t.add(0, Ai::Swimming);
            t.add(2, attack_on_collide(Some(player), 1.0, false));
            t.add(
                4,
                attack_on_collide(Some(Cls::Kind(Kind::Villager)), 1.0, true),
            );
            t.add(7, wander(1.0));
            t.add(8, watch(player, 8.0));
            t.add(8, look_idle());
            tt.add(1, hurt_by_target(true));
            tt.add(2, nearest_target(player, true));
            tt.add(2, nearest_target(Cls::Kind(Kind::Villager), false));
        }
        Kind::Skeleton => {
            t.add(1, Ai::Swimming);
            t.add(2, Ai::RestrictSun);
            t.add(
                3,
                Ai::FleeSun {
                    speed: 1.0,
                    pos: [0.0; 3],
                },
            );
            t.add(5, wander(1.0));
            t.add(6, watch(player, 8.0));
            t.add(6, look_idle());
            // setCombatTask: a bow, or the wither skeleton's sword
            if m.variant == 1 {
                t.add(4, attack_on_collide(Some(player), 1.2, false));
            } else {
                t.add(4, arrow_attack(1.0, 20, 60, 15.0));
            }
            tt.add(1, hurt_by_target(false));
            tt.add(2, nearest_target(player, true));
        }
        Kind::Creeper => {
            t.add(1, Ai::Swimming);
            t.add(2, Ai::CreeperSwell { target: None });
            t.add(
                3,
                Ai::AvoidEntity {
                    kind: Kind::Ocelot,
                    dist: 6.0,
                    far: 1.0,
                    near: 1.2,
                    closest: None,
                    path: None,
                },
            );
            t.add(4, attack_on_collide(None, 1.0, false));
            t.add(5, wander(0.8));
            t.add(6, watch(player, 8.0));
            t.add(6, look_idle());
            tt.add(1, nearest_target(player, true));
            tt.add(2, hurt_by_target(false));
        }
        Kind::Witch => {
            t.add(1, Ai::Swimming);
            t.add(2, arrow_attack(1.0, 60, 60, 10.0));
            t.add(2, wander(1.0));
            t.add(3, watch(player, 8.0));
            t.add(3, look_idle());
            tt.add(1, hurt_by_target(false));
            tt.add(2, nearest_target(player, true));
        }
        Kind::Pig => {
            t.add(0, Ai::Swimming);
            t.add(1, panic(1.25));
            t.add(4, tempt(1.2, 398));
            t.add(4, tempt(1.2, 391));
            t.add(5, follow_parent(1.1));
            t.add(6, wander(1.0));
            t.add(7, watch(player, 6.0));
            t.add(8, look_idle());
        }
        Kind::Cow | Kind::Mooshroom => {
            t.add(0, Ai::Swimming);
            t.add(1, panic(2.0));
            t.add(3, tempt(1.25, 296));
            t.add(4, follow_parent(1.25));
            t.add(5, wander(1.0));
            t.add(6, watch(player, 6.0));
            t.add(7, look_idle());
        }
        Kind::Sheep => {
            t.add(0, Ai::Swimming);
            t.add(1, panic(1.25));
            t.add(3, tempt(1.1, 296));
            t.add(4, follow_parent(1.1));
            t.add(5, Ai::EatGrass { timer: 0 });
            t.add(6, wander(1.0));
            t.add(7, watch(player, 6.0));
            t.add(8, look_idle());
        }
        Kind::Chicken => {
            t.add(0, Ai::Swimming);
            t.add(1, panic(1.4));
            t.add(3, tempt(1.0, 295));
            t.add(4, follow_parent(1.1));
            t.add(5, wander(1.0));
            t.add(6, watch(player, 6.0));
            t.add(7, look_idle());
        }
        Kind::Villager => {
            t.add(0, Ai::Swimming);
            t.add(
                1,
                Ai::AvoidEntity {
                    kind: Kind::Zombie,
                    dist: 8.0,
                    far: 0.6,
                    near: 0.6,
                    closest: None,
                    path: None,
                },
            );
            t.add(9, watch2(player, 3.0, 1.0));
            t.add(9, watch2(Cls::Kind(Kind::Villager), 5.0, 0.02));
            t.add(9, wander(0.6));
            t.add(10, watch(Cls::Living, 8.0));
        }
        _ => {}
    }
}

/// `EntityLiving.updateAITasks`, after `++entityAge` and `despawnEntity`.
pub(super) fn update(m: &mut Mob, cx: &mut Cx) {
    if m.tasks.is_empty() {
        build(m);
        build_pet(m);
    }
    if m.kind == Kind::IronGolem && m.attack_timer > 0 {
        // EntityIronGolem.onLivingUpdate
        m.attack_timer -= 1;
    }
    // EntityLivingBase.onEntityUpdate: the attacker is forgotten after 100
    // ticks, or once it is dead
    if let Some(t) = m.revenge {
        if !cx.view(t).is_some_and(|v| v.alive) || m.ticks - m.revenge_timer > 100 {
            m.set_revenge(None);
        }
    }
    let mut tt = std::mem::take(&mut m.target_tasks);
    tt.update(m, cx);
    m.target_tasks = tt;
    let mut t = std::mem::take(&mut m.tasks);
    t.update(m, cx);
    // updateAITick: EntitySheep.sheepTimer
    if m.kind == Kind::Sheep {
        let timer = t.eat_grass_timer();
        if timer > 0 {
            m.eat_timer = timer;
        }
    }
    m.tasks = t;
    m.nav_update(cx.w);
    m.move_helper();
    m.look_update();
    m.target_player = m.attack_target == Some(Target::Player);
    if m.kind == Kind::Wolf {
        // EntityWolf.setAttackTarget: angry while it has one, unless tamed
        if m.attack_target.is_none() {
            m.angry = false;
        } else if !m.tamed {
            m.angry = true;
        }
    }
}

/// `EntityAIHurtByTarget.startExecuting` with `entityCallsForHelp`: the
/// mobs of one class around the victim that have no target take its.
#[derive(Clone, Copy, Debug)]
pub struct Call {
    pub from: u32,
    pub kind: Kind,
    pub bb: Aabb,
    pub target: Target,
}

pub(super) fn answer_calls(list: &mut [Mob], calls: &[Call]) {
    for c in calls {
        for m in list.iter_mut() {
            if m.id != c.from
                && m.kind == c.kind
                && m.health > 0.0
                && m.bb.intersects(&c.bb)
                && m.attack_target.is_none()
            {
                m.attack_target = Some(c.target);
            }
        }
    }
}

// ----------------------------------------- wolves, ocelots and iron golems

/// The tasks of `EntityTameable`s and of the iron golem.
#[derive(Clone, Debug)]
pub enum Pet {
    /// `EntityAISit`
    Sit,
    /// `EntityAIFollowOwner(pet, speed, minDist, maxDist)`
    FollowOwner {
        speed: f64,
        min: f32,
        max: f32,
        recalc: i32,
        old_water: bool,
    },
    /// `EntityAIBeg`
    Beg { dist: f32, timeout: i32 },
    /// `EntityAILeapAtTarget`
    Leap { y: f32, target: Option<Target> },
    /// `EntityAIOwnerHurtByTarget`
    OwnerHurtByTarget {
        timestamp: i32,
        found: Option<Target>,
        st: TargetState,
    },
    /// `EntityAIOwnerHurtTarget`
    OwnerHurtTarget {
        timestamp: i32,
        found: Option<Target>,
        st: TargetState,
    },
    /// `EntityAITargetNonTamed(pet, class, chance, false)`
    TargetNonTamed {
        kind: Kind,
        chance: i32,
        found: Option<Target>,
        st: TargetState,
    },
    /// `EntityAITempt` with `scaredByPlayerMovement` (the ocelot's fish)
    TemptScared {
        speed: f64,
        item: u16,
        delay: i32,
        at: [f64; 3],
        pitch: f64,
        yaw: f64,
        avoid_water: bool,
    },
    /// `EntityAIAvoidEntity(ocelot, EntityPlayer.class, ...)`
    AvoidPlayer {
        dist: f32,
        far: f64,
        near: f64,
        path: Option<Vec<Pt>>,
    },
    /// `EntityAIOcelotSit`
    OcelotSit {
        speed: f64,
        timer: i32,
        sitting_timer: i32,
        max_stay: i32,
        block: [i32; 3],
    },
    /// `EntityAIOcelotAttack`
    OcelotAttack {
        victim: Option<Target>,
        countdown: i32,
    },
    /// `EntityAIMoveTowardsTarget`
    MoveTowardsTarget {
        speed: f64,
        max: f32,
        pos: [f64; 3],
        target: Option<Target>,
    },
    /// `EntityAILookAtVillager`
    LookAtVillager {
        villager: Option<u32>,
        look_time: i32,
    },
    /// `EntityAINearestAttackableTarget(golem, EntityLiving.class, 0, false,
    /// true, IMob.mobSelector)`
    GolemTarget {
        found: Option<Target>,
        st: TargetState,
        search_status: i32,
        search_delay: i32,
    },
}

/// `ItemFood.isWolfsFavoriteMeat` (`EntityWolf.isBreedingItem`)
fn wolf_meat(id: u16) -> bool {
    matches!(id, 319 | 320 | 363 | 364 | 365 | 366 | 367)
}

/// `isSuitableTarget` for a tamed animal, with `shouldAttackEntity`: not
/// its owner, not another pet of the owner's, not a creeper or a ghast.
fn pet_suitable(m: &Mob, t: Option<Target>, cx: &Cx) -> bool {
    let Some(t) = t else { return false };
    match t {
        Target::Player => false,
        Target::Mob(id) => {
            let Some(o) = cx.others.iter().find(|o| o.id == id) else {
                return false;
            };
            if o.tamed || matches!(o.kind, Kind::Creeper | Kind::Ghast) {
                return false;
            }
            is_suitable_target(m, Some(t), false, cx)
        }
    }
}

/// `EntityAITarget.canEasilyReach`
fn can_easily_reach(m: &mut Mob, v: &View, delay: &mut i32, cx: &mut Cx) -> bool {
    *delay = 10 + m.rand.next_int_n(5);
    let Some(p) = m.path_to_entity(cx.w, v) else {
        return false;
    };
    let Some(e) = p.last() else { return false };
    let dx = e.x - math::floor_double(v.pos[0]);
    let dz = e.z - math::floor_double(v.pos[2]);
    (dx * dx + dz * dz) as f64 <= 2.25
}

/// `EntityAIOcelotSit.canSitOnBlock` (beds are not in the game yet; a
/// chest counts whether or not someone has it open)
fn can_sit_on(w: &mut ServerWorld, p: [i32; 3]) -> bool {
    matches!(w.block(p[0], p[1], p[2]), block::CHEST | block::LIT_FURNACE)
}

impl Pet {
    fn mutex(&self) -> i32 {
        match self {
            Pet::Sit | Pet::Leap { .. } | Pet::OcelotSit { .. } => 5,
            Pet::FollowOwner { .. } | Pet::TemptScared { .. } => 3,
            Pet::OcelotAttack { .. } | Pet::LookAtVillager { .. } => 3,
            Pet::Beg { .. } => 2,
            _ => 1,
        }
    }

    fn should_execute(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        match self {
            Pet::Sit => {
                if !m.tamed || m.in_water || !m.on_ground {
                    return false;
                }
                // the owner under attack nearby gets the pet up
                if m.dist2(cx.player.feet) < 144.0 && cx.player.last_attacker.is_some() {
                    false
                } else {
                    m.sit_order
                }
            }
            Pet::FollowOwner { min, .. } => {
                if !m.tamed || m.sitting {
                    return false;
                }
                !(m.dist2(cx.player.feet) < (*min * *min) as f64)
            }
            Pet::Beg { dist, .. } => {
                closest_player(m, cx, *dist as f64).is_some() && begs_for(m, cx)
            }
            Pet::Leap { target, .. } => {
                *target = m.attack_target;
                let Some(v) = target.and_then(|t| cx.view(t)) else {
                    return false;
                };
                let d = m.dist2_to(&v);
                if d < 4.0 || d > 16.0 {
                    return false;
                }
                if !m.on_ground {
                    return false;
                }
                m.rand.next_int_n(5) == 0
            }
            Pet::OwnerHurtByTarget {
                timestamp, found, ..
            } => {
                if !m.tamed {
                    return false;
                }
                *found = cx.player.last_attacker.map(Target::Mob);
                cx.player.revenge_timer != *timestamp && pet_suitable(m, *found, cx)
            }
            Pet::OwnerHurtTarget {
                timestamp, found, ..
            } => {
                if !m.tamed {
                    return false;
                }
                *found = cx.player.last_attacked.map(Target::Mob);
                cx.player.last_attack_time != *timestamp && pet_suitable(m, *found, cx)
            }
            Pet::TargetNonTamed {
                kind,
                chance,
                found,
                ..
            } => {
                if m.tamed {
                    return false;
                }
                if *chance > 0 && m.rand.next_int_n(*chance) != 0 {
                    return false;
                }
                let range = m.kind.follow_range() as f64;
                let bb = expand(&m.bb, range, 4.0, range);
                let ids: Vec<(u32, [f64; 3])> = mobs_within(m, cx, Cls::Kind(*kind), bb)
                    .map(|o| (o.id, o.pos))
                    .collect();
                let mut best: Option<(f64, u32)> = None;
                for (id, pos) in ids {
                    if is_suitable_target(m, Some(Target::Mob(id)), false, cx) {
                        let d = m.dist2(pos);
                        if best.map_or(true, |b| d < b.0) {
                            best = Some((d, id));
                        }
                    }
                }
                *found = best.map(|b| Target::Mob(b.1));
                found.is_some()
            }
            Pet::TemptScared { item, delay, .. } => {
                if *delay > 0 {
                    *delay -= 1;
                    return false;
                }
                closest_player(m, cx, 10.0).is_some() && cx.player.held == Some(*item)
            }
            Pet::AvoidPlayer { dist, path, .. } => {
                if m.tamed {
                    return false;
                }
                if closest_player(m, cx, *dist as f64).is_none() {
                    return false;
                }
                let at = cx.player.feet;
                let Some(v) = m.random_target(cx.w, 16, 7, Some(at)) else {
                    return false;
                };
                let d = |a: [f64; 3], b: [f64; 3]| {
                    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
                };
                if d(at, v) < d(at, m.pos) {
                    return false;
                }
                *path = m.path_to_xyz(cx.w, v[0], v[1], v[2]);
                path.as_ref()
                    .and_then(|p| p.last())
                    .is_some_and(|e| e.x == v[0] as i32 && e.y == v[1] as i32 && e.z == v[2] as i32)
            }
            Pet::OcelotSit { block, .. } => {
                if !(m.tamed && !m.sitting && m.rand.next_double() <= 0.0065f32 as f64) {
                    return false;
                }
                // isNearValidBlock
                let y = m.pos[1] as i32;
                let mut best = 2.147483647e9f64;
                let mut x = m.pos[0] as i32 - 8;
                while (x as f64) < m.pos[0] + 8.0 {
                    let mut z = m.pos[2] as i32 - 8;
                    while (z as f64) < m.pos[2] + 8.0 {
                        if can_sit_on(cx.w, [x, y, z]) && cx.w.is_air(x, y + 1, z) {
                            let d = m.dist2([x as f64, y as f64, z as f64]);
                            if d < best {
                                *block = [x, y, z];
                                best = d;
                            }
                        }
                        z += 1;
                    }
                    x += 1;
                }
                best < 2.147483647e9
            }
            Pet::OcelotAttack { victim, .. } => {
                *victim = m.attack_target.filter(|t| cx.view(*t).is_some());
                victim.is_some()
            }
            Pet::MoveTowardsTarget {
                max, pos, target, ..
            } => {
                *target = m.attack_target;
                let Some(v) = target.and_then(|t| cx.view(t)) else {
                    return false;
                };
                if m.dist2_to(&v) > (*max * *max) as f64 {
                    return false;
                }
                // findRandomTargetBlockTowards: the mirror of "away from"
                let mirror = [
                    2.0 * m.pos[0] - v.pos[0],
                    2.0 * m.pos[1] - v.pos[1],
                    2.0 * m.pos[2] - v.pos[2],
                ];
                match m.random_target(cx.w, 16, 7, Some(mirror)) {
                    Some(p) => {
                        *pos = p;
                        true
                    }
                    None => false,
                }
            }
            Pet::LookAtVillager { villager, .. } => {
                if !cx.day {
                    return false;
                }
                if m.rand.next_int_n(8000) != 0 {
                    return false;
                }
                let bb = expand(&m.bb, 6.0, 2.0, 6.0);
                let mut best: Option<(f64, u32)> = None;
                for o in mobs_within(m, cx, Cls::Kind(Kind::Villager), bb) {
                    let d = m.dist2(o.pos);
                    if best.map_or(true, |b| d <= b.0) {
                        best = Some((d, o.id));
                    }
                }
                *villager = best.map(|b| b.1);
                villager.is_some()
            }
            Pet::GolemTarget {
                found,
                search_status,
                search_delay,
                ..
            } => {
                let range = m.kind.follow_range() as f64;
                let bb = expand(&m.bb, range, 4.0, range);
                // IMob.mobSelector, and canAttackClass: no creepers
                let ids: Vec<(u32, [f64; 3])> = mobs_within(m, cx, Cls::Living, bb)
                    .filter(|o| o.kind.hostile() && !matches!(o.kind, Kind::Creeper | Kind::Ghast))
                    .map(|o| (o.id, o.pos))
                    .collect();
                let mut best: Option<(f64, u32)> = None;
                for (id, pos) in ids {
                    let t = Target::Mob(id);
                    if !is_suitable_target(m, Some(t), false, cx) {
                        continue;
                    }
                    // nearbyOnly
                    *search_delay -= 1;
                    if *search_delay <= 0 {
                        *search_status = 0;
                    }
                    if *search_status == 0 {
                        let v = cx.view(t).unwrap();
                        *search_status = if can_easily_reach(m, &v, search_delay, cx) {
                            1
                        } else {
                            2
                        };
                    }
                    if *search_status == 2 {
                        continue;
                    }
                    let d = m.dist2(pos);
                    if best.map_or(true, |b| d < b.0) {
                        best = Some((d, id));
                    }
                }
                *found = best.map(|b| Target::Mob(b.1));
                found.is_some()
            }
        }
    }

    fn continue_executing(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        match self {
            Pet::FollowOwner { max, .. } => {
                !m.no_path() && m.dist2(cx.player.feet) > (*max * *max) as f64 && !m.sitting
            }
            Pet::Beg { dist, timeout } => {
                if !cx.player.alive {
                    return false;
                }
                if m.dist2(cx.player.feet) > (*dist * *dist) as f64 {
                    false
                } else {
                    *timeout > 0 && begs_for(m, cx)
                }
            }
            Pet::Leap { .. } => !m.on_ground,
            Pet::OwnerHurtByTarget { st, .. }
            | Pet::OwnerHurtTarget { st, .. }
            | Pet::TargetNonTamed { st, .. }
            | Pet::GolemTarget { st, .. } => target_continue(m, false, st, cx),
            Pet::TemptScared { at, pitch, yaw, .. } => {
                let p = cx.player;
                if m.dist2(p.feet) < 36.0 {
                    let d = (p.feet[0] - at[0]).powi(2)
                        + (p.feet[1] - at[1]).powi(2)
                        + (p.feet[2] - at[2]).powi(2);
                    if d > 0.010000000000000002 {
                        return false;
                    }
                    if (p.pitch as f64 - *pitch).abs() > 5.0 || (p.yaw as f64 - *yaw).abs() > 5.0 {
                        return false;
                    }
                } else {
                    *at = p.feet;
                }
                *pitch = p.pitch as f64;
                *yaw = p.yaw as f64;
                self.should_execute(m, cx)
            }
            Pet::AvoidPlayer { .. } => !m.no_path(),
            Pet::OcelotSit {
                timer,
                sitting_timer,
                max_stay,
                block,
                ..
            } => *timer <= *max_stay && *sitting_timer <= 60 && can_sit_on(cx.w, *block),
            Pet::OcelotAttack { victim, .. } => {
                let Some(v) = victim.and_then(|t| cx.view(t)) else {
                    return false;
                };
                if m.dist2_to(&v) > 225.0 {
                    false
                } else {
                    !m.no_path() || self.should_execute(m, cx)
                }
            }
            Pet::MoveTowardsTarget { max, target, .. } => {
                let Some(v) = target.and_then(|t| cx.view(t)) else {
                    return false;
                };
                !m.no_path() && m.dist2_to(&v) < (*max * *max) as f64
            }
            Pet::LookAtVillager { look_time, .. } => *look_time > 0,
            Pet::Sit => self.should_execute(m, cx),
        }
    }

    fn start_executing(&mut self, m: &mut Mob, cx: &mut Cx) {
        match self {
            Pet::Sit => {
                m.clear_path();
                m.sitting = true;
            }
            Pet::FollowOwner {
                recalc, old_water, ..
            } => {
                *recalc = 0;
                *old_water = m.avoids_water;
                m.avoids_water = false;
            }
            Pet::Beg { timeout, .. } => {
                *timeout = 40 + m.rand.next_int_n(40);
            }
            Pet::Leap { y, target } => {
                if let Some(v) = target.and_then(|t| cx.view(t)) {
                    let dx = v.pos[0] - m.pos[0];
                    let dz = v.pos[2] - m.pos[2];
                    let h = math::sqrt_double(dx * dx + dz * dz) as f64;
                    m.motion[0] += dx / h * 0.5 * 0.8f32 as f64 + m.motion[0] * 0.2f32 as f64;
                    m.motion[2] += dz / h * 0.5 * 0.8f32 as f64 + m.motion[2] * 0.2f32 as f64;
                    m.motion[1] = *y as f64;
                }
            }
            Pet::OwnerHurtByTarget {
                timestamp,
                found,
                st,
            } => {
                m.attack_target = *found;
                *timestamp = cx.player.revenge_timer;
                *st = TargetState::default();
            }
            Pet::OwnerHurtTarget {
                timestamp,
                found,
                st,
            } => {
                m.attack_target = *found;
                *timestamp = cx.player.last_attack_time;
                *st = TargetState::default();
            }
            Pet::TargetNonTamed { found, st, .. } => {
                m.attack_target = *found;
                *st = TargetState::default();
            }
            Pet::GolemTarget {
                found,
                st,
                search_status,
                search_delay,
            } => {
                m.attack_target = *found;
                *st = TargetState::default();
                *search_status = 0;
                *search_delay = 0;
            }
            Pet::TemptScared {
                at, avoid_water, ..
            } => {
                *at = cx.player.feet;
                *avoid_water = m.avoids_water;
                m.avoids_water = false;
            }
            Pet::AvoidPlayer { far, path, .. } => {
                let p = path.take();
                m.set_path(cx.w, p, *far);
            }
            Pet::OcelotSit {
                speed,
                timer,
                sitting_timer,
                max_stay,
                block,
            } => {
                m.try_move_to_xyz(
                    cx.w,
                    block[0] as f64 + 0.5,
                    (block[1] + 1) as f64,
                    block[2] as f64 + 0.5,
                    *speed,
                );
                *timer = 0;
                *sitting_timer = 0;
                let n = m.rand.next_int_n(1200) + 1200;
                *max_stay = m.rand.next_int_n(n) + 1200;
                m.sit_order = false;
            }
            Pet::MoveTowardsTarget { speed, pos, .. } => {
                m.try_move_to_xyz(cx.w, pos[0], pos[1], pos[2], *speed);
            }
            Pet::LookAtVillager { look_time, .. } => *look_time = 400,
            Pet::OcelotAttack { .. } => {}
        }
    }

    fn reset_task(&mut self, m: &mut Mob) {
        match self {
            Pet::Sit | Pet::OcelotSit { .. } => m.sitting = false,
            Pet::FollowOwner { old_water, .. } => {
                m.clear_path();
                m.avoids_water = *old_water;
            }
            Pet::OwnerHurtByTarget { .. }
            | Pet::OwnerHurtTarget { .. }
            | Pet::TargetNonTamed { .. }
            | Pet::GolemTarget { .. } => m.attack_target = None,
            Pet::TemptScared {
                delay, avoid_water, ..
            } => {
                m.clear_path();
                *delay = 100;
                m.avoids_water = *avoid_water;
            }
            Pet::OcelotAttack { victim, .. } => {
                *victim = None;
                m.clear_path();
            }
            Pet::MoveTowardsTarget { target, .. } => *target = None,
            Pet::LookAtVillager { villager, .. } => *villager = None,
            _ => {}
        }
    }

    fn update_task(&mut self, m: &mut Mob, cx: &mut Cx) {
        match self {
            Pet::FollowOwner { speed, recalc, .. } => {
                let v = cx.view(Target::Player).unwrap();
                m.look_at_entity(&v, 10.0, 40.0);
                if m.sitting {
                    return;
                }
                *recalc -= 1;
                if *recalc > 0 {
                    return;
                }
                *recalc = 10;
                if m.try_move_to_entity(cx.w, &v, *speed) {
                    return;
                }
                if m.dist2_to(&v) < 144.0 {
                    return;
                }
                // out of reach: appear beside the owner
                let x0 = math::floor_double(v.pos[0]) - 2;
                let z0 = math::floor_double(v.pos[2]) - 2;
                let y = math::floor_double(v.bb.min[1]);
                for i in 0..=4 {
                    for k in 0..=4 {
                        if (i < 1 || k < 1 || i > 3 || k > 3)
                            && cx.w.solid_top_surface(x0 + i, y - 1, z0 + k)
                            && !block::info(cx.w.block(x0 + i, y, z0 + k)).normal_cube()
                            && !block::info(cx.w.block(x0 + i, y + 1, z0 + k)).normal_cube()
                        {
                            // setLocationAndAngles
                            m.set_position(
                                ((x0 + i) as f32 + 0.5) as f64,
                                y as f64,
                                ((z0 + k) as f32 + 0.5) as f64,
                            );
                            m.prev_pos = m.pos;
                            m.clear_path();
                            return;
                        }
                    }
                }
            }
            Pet::Beg { timeout, .. } => {
                let p = cx.player;
                m.look(
                    [p.feet[0], p.feet[1] + 1.62f32 as f64, p.feet[2]],
                    10.0,
                    40.0,
                );
                *timeout -= 1;
            }
            Pet::TemptScared { speed, .. } => {
                let v = cx.view(Target::Player).unwrap();
                m.look_at_entity(&v, 30.0, 40.0);
                if m.dist2_to(&v) < 6.25 {
                    m.clear_path();
                } else {
                    m.try_move_to_entity(cx.w, &v, *speed);
                }
            }
            Pet::AvoidPlayer { far, near, .. } => {
                m.path_speed = if m.dist2(cx.player.feet) < 49.0 {
                    *near
                } else {
                    *far
                };
            }
            Pet::OcelotSit {
                speed,
                timer,
                sitting_timer,
                block,
                ..
            } => {
                *timer += 1;
                m.sit_order = false;
                if m.dist2([block[0] as f64, (block[1] + 1) as f64, block[2] as f64]) > 1.0 {
                    m.sitting = false;
                    m.try_move_to_xyz(
                        cx.w,
                        block[0] as f64 + 0.5,
                        (block[1] + 1) as f64,
                        block[2] as f64 + 0.5,
                        *speed,
                    );
                    *sitting_timer += 1;
                } else if !m.sitting {
                    m.sitting = true;
                } else {
                    *sitting_timer -= 1;
                }
            }
            Pet::OcelotAttack { victim, countdown } => {
                let Some(t) = *victim else { return };
                let Some(v) = cx.view(t) else { return };
                m.look_at_entity(&v, 30.0, 30.0);
                let reach = (m.width() * 2.0 * (m.width() * 2.0)) as f64;
                let d2 = m.dist2([v.pos[0], v.bb.min[1], v.pos[2]]);
                let mut speed = 0.8;
                if d2 > reach && d2 < 16.0 {
                    speed = 1.33;
                } else if d2 < 225.0 {
                    speed = 0.6;
                }
                m.try_move_to_entity(cx.w, &v, speed);
                *countdown = (*countdown - 1).max(0);
                if !(d2 > reach) && *countdown <= 0 {
                    *countdown = 20;
                    attack_as_mob(m, t, &v, cx);
                }
            }
            Pet::LookAtVillager {
                villager,
                look_time,
            } => {
                if let Some(v) = villager.and_then(|id| cx.view(Target::Mob(id))) {
                    m.look_at_entity(&v, 30.0, 30.0);
                }
                *look_time -= 1;
            }
            _ => {}
        }
    }
}

/// `EntityAIBeg.hasPlayerGotBoneInHand`
fn begs_for(m: &Mob, cx: &Cx) -> bool {
    match cx.player.held {
        None => false,
        Some(id) => !m.tamed && id == 352 || wolf_meat(id),
    }
}

fn follow_owner(speed: f64, min: f32, max: f32) -> Ai {
    Ai::Pet(Pet::FollowOwner {
        speed,
        min,
        max,
        recalc: 0,
        old_water: false,
    })
}

fn target_non_tamed(kind: Kind, chance: i32) -> Ai {
    Ai::Pet(Pet::TargetNonTamed {
        kind,
        chance,
        found: None,
        st: TargetState::default(),
    })
}

/// The constructors of `EntityWolf`, `EntityOcelot` and `EntityIronGolem`
/// (without `EntityAIMate` and the village tasks).
fn build_pet(m: &mut Mob) {
    let (t, tt) = (&mut m.tasks, &mut m.target_tasks);
    let leap = |y: f32| Ai::Pet(Pet::Leap { y, target: None });
    match m.kind {
        Kind::Wolf => {
            t.add(1, Ai::Swimming);
            t.add(2, Ai::Pet(Pet::Sit));
            t.add(3, leap(0.4));
            t.add(4, attack_on_collide(None, 1.0, true));
            t.add(5, follow_owner(1.0, 10.0, 2.0));
            t.add(7, wander(1.0));
            t.add(
                8,
                Ai::Pet(Pet::Beg {
                    dist: 8.0,
                    timeout: 0,
                }),
            );
            t.add(9, watch(Cls::Player, 8.0));
            t.add(9, look_idle());
            tt.add(
                1,
                Ai::Pet(Pet::OwnerHurtByTarget {
                    timestamp: 0,
                    found: None,
                    st: TargetState::default(),
                }),
            );
            tt.add(
                2,
                Ai::Pet(Pet::OwnerHurtTarget {
                    timestamp: 0,
                    found: None,
                    st: TargetState::default(),
                }),
            );
            tt.add(3, hurt_by_target(true));
            tt.add(4, target_non_tamed(Kind::Sheep, 200));
        }
        Kind::Ocelot => {
            t.add(1, Ai::Swimming);
            t.add(2, Ai::Pet(Pet::Sit));
            t.add(
                3,
                Ai::Pet(Pet::TemptScared {
                    speed: 0.6,
                    item: 349,
                    delay: 0,
                    at: [0.0; 3],
                    pitch: 0.0,
                    yaw: 0.0,
                    avoid_water: false,
                }),
            );
            t.add(
                4,
                Ai::Pet(Pet::AvoidPlayer {
                    dist: 16.0,
                    far: 0.8,
                    near: 1.33,
                    path: None,
                }),
            );
            t.add(5, follow_owner(1.0, 10.0, 5.0));
            t.add(
                6,
                Ai::Pet(Pet::OcelotSit {
                    speed: 1.33,
                    timer: 0,
                    sitting_timer: 0,
                    max_stay: 0,
                    block: [0; 3],
                }),
            );
            t.add(7, leap(0.3));
            t.add(
                8,
                Ai::Pet(Pet::OcelotAttack {
                    victim: None,
                    countdown: 0,
                }),
            );
            t.add(10, wander(0.8));
            t.add(11, watch(Cls::Player, 10.0));
            tt.add(1, target_non_tamed(Kind::Chicken, 750));
        }
        Kind::IronGolem => {
            t.add(1, attack_on_collide(None, 1.0, true));
            t.add(
                2,
                Ai::Pet(Pet::MoveTowardsTarget {
                    speed: 0.9,
                    max: 32.0,
                    pos: [0.0; 3],
                    target: None,
                }),
            );
            t.add(
                5,
                Ai::Pet(Pet::LookAtVillager {
                    villager: None,
                    look_time: 0,
                }),
            );
            t.add(6, wander(0.6));
            t.add(7, watch(Cls::Player, 6.0));
            t.add(8, look_idle());
            tt.add(2, hurt_by_target(false));
            tt.add(
                3,
                Ai::Pet(Pet::GolemTarget {
                    found: None,
                    st: TargetState::default(),
                    search_status: 0,
                    search_delay: 0,
                }),
            );
        }
        _ => {}
    }
}

// ------------------------------------------------- the old AI (EntityCreature)

/// The mobs whose `isAIEnabled()` is false and that go through
/// `EntityCreature.updateEntityActionState` as ported here.
pub fn uses_old(k: Kind) -> bool {
    matches!(
        k,
        Kind::Spider | Kind::CaveSpider | Kind::Silverfish | Kind::PigZombie
    )
}

/// `World.getClosestVulnerablePlayerToEntity`
fn closest_vulnerable_player(m: &Mob, cx: &Cx, dist: f64) -> Option<Target> {
    let p = cx.player;
    // capabilities.disableDamage, isEntityAlive
    if !p.attackable || !p.alive {
        return None;
    }
    let d2 = m.dist2(p.feet);
    let mut reach = dist;
    if p.sneaking {
        reach *= 0.8f32 as f64;
    }
    if p.effects & 1 << crate::potion::INVISIBILITY != 0 {
        // getArmorVisibility
        let v = (p.armor_pieces as f32 / 4.0).max(0.1);
        reach *= (0.7f32 * v) as f64;
    }
    if dist < 0.0 || d2 < reach * reach {
        Some(Target::Player)
    } else {
        None
    }
}

/// `findPlayerToAttack` of the mob's class.
fn find_player_to_attack(m: &mut Mob, cx: &mut Cx) -> Option<Target> {
    match m.kind {
        // EntitySpider: only in the dark
        Kind::Spider | Kind::CaveSpider => {
            let (x, y, z) = m.feet_block();
            if brightness(cx.w, x, y, z) < 0.5 {
                closest_vulnerable_player(m, cx, 16.0)
            } else {
                None
            }
        }
        Kind::Silverfish => closest_vulnerable_player(m, cx, 8.0),
        // EntityPigZombie: only once angered; then EntityMob's
        Kind::PigZombie if m.anger == 0 => None,
        _ => {
            let t = closest_vulnerable_player(m, cx, 16.0)?;
            let v = cx.view(t)?;
            m.can_see_view(cx.w, &v).then_some(t)
        }
    }
}

/// `attackEntity(target, distance)` of the mob's class.
fn attack_entity(m: &mut Mob, t: Target, v: &View, dist: f32, cx: &mut Cx) {
    let overlap = v.bb.max[1] > m.bb.min[1] && v.bb.min[1] < m.bb.max[1];
    // EntityMob.attackEntity
    let mut melee = |m: &mut Mob, cx: &mut Cx, reach: f32| {
        if m.attack_cooldown <= 0 && dist < reach && overlap {
            m.attack_cooldown = 20;
            attack_as_mob(m, t, v, cx);
        }
    };
    match m.kind {
        Kind::Spider | Kind::CaveSpider => {
            let (x, y, z) = m.feet_block();
            let f = brightness(cx.w, x, y, z);
            if f > 0.5 && m.rand.next_int_n(100) == 0 {
                m.attack_target = None;
            } else if !(dist > 2.0) || !(dist < 6.0) || m.rand.next_int_n(10) != 0 {
                melee(m, cx, 2.0);
            } else if m.on_ground {
                // the leap
                let dx = v.pos[0] - m.pos[0];
                let dz = v.pos[2] - m.pos[2];
                let h = math::sqrt_double(dx * dx + dz * dz) as f64;
                m.motion[0] = dx / h * 0.5 * 0.8f32 as f64 + m.motion[0] * 0.2f32 as f64;
                m.motion[2] = dz / h * 0.5 * 0.8f32 as f64 + m.motion[2] * 0.2f32 as f64;
                m.motion[1] = 0.4f32 as f64;
            }
        }
        Kind::Silverfish => melee(m, cx, 1.2),
        _ => melee(m, cx, 2.0),
    }
}

/// `EntityLiving.faceEntity`
fn face_entity(m: &mut Mob, v: &View, yaw_max: f32, pitch_max: f32) {
    let dx = v.pos[0] - m.pos[0];
    let dz = v.pos[2] - m.pos[2];
    let dy = v.pos[1] + v.eye - (m.pos[1] + (m.height() * 0.85) as f64);
    let d = math::sqrt_double(dx * dx + dz * dz) as f64;
    let yaw = (dz.atan2(dx) * 180.0 / std::f64::consts::PI) as f32 - 90.0;
    let pitch = (-(dy.atan2(d) * 180.0 / std::f64::consts::PI)) as f32;
    let turn = |from: f32, to: f32, max: f32| from + wrap(to - from).clamp(-max, max);
    m.pitch = turn(m.pitch, pitch, pitch_max);
    m.yaw = turn(m.yaw, yaw, yaw_max);
}

/// `EntityCreature.updateEntityActionState`
pub(super) fn old_update(m: &mut Mob, cx: &mut Cx) {
    // tick_mob has counted this tick into entityAge already; the original
    // does that only in the idle branch below
    m.age -= 1;
    if m.kind == Kind::PigZombie {
        // EntityPigZombie.onUpdate
        if m.anger_sound > 0 {
            m.anger_sound -= 1;
            if m.anger_sound == 0 {
                let p = ((m.rand.next_float() - m.rand.next_float()) * 0.2 + 1.0) * 1.8;
                cx.out
                    .sounds
                    .push(("mob.zombiepig.zpigangry", m.pos, 2.0, p));
            }
        }
    }
    let has_path = |m: &Mob| !m.path.is_empty();
    // the paths of the old AI: 16 blocks, through open doors, swimming
    let path_to = |m: &Mob, cx: &mut Cx, v: &View| {
        m.create_path_with(cx.w, v.pos[0], v.bb.min[1], v.pos[2], 16.0, true, false)
    };
    let set = |m: &mut Mob, p: Option<Vec<Pt>>| {
        m.path = p.unwrap_or_default();
        m.path_index = 0;
    };
    match m.attack_target {
        None => {
            m.attack_target = find_player_to_attack(m, cx);
            if let Some(v) = m.attack_target.and_then(|t| cx.view(t)) {
                let p = path_to(m, cx, &v);
                set(m, p);
            }
        }
        Some(t) => match cx.view(t) {
            Some(v) if v.alive => {
                let dist = math::sqrt_double(m.dist2_to(&v));
                if m.can_see_view(cx.w, &v) {
                    attack_entity(m, t, &v, dist, cx);
                }
            }
            _ => m.attack_target = None,
        },
    }
    // a player in creative mode
    if m.attack_target == Some(Target::Player) && !cx.player.attackable {
        m.attack_target = None;
    }
    if m.attack_target.is_none() || has_path(m) && m.rand.next_int_n(20) != 0 {
        if (!has_path(m) && m.rand.next_int_n(180) == 0 || m.rand.next_int_n(120) == 0)
            && m.age < 100
        {
            // updateWanderPath
            let mut best: Option<(f32, [i32; 3])> = None;
            for _ in 0..10 {
                let x = math::floor_double(m.pos[0] + m.rand.next_int_n(13) as f64 - 6.0);
                let y = math::floor_double(m.pos[1] + m.rand.next_int_n(7) as f64 - 3.0);
                let z = math::floor_double(m.pos[2] + m.rand.next_int_n(13) as f64 - 6.0);
                let wgt = m.path_weight(cx.w, x, y, z);
                if best.map_or(wgt > -99999.0, |b| wgt > b.0) {
                    best = Some((wgt, [x, y, z]));
                }
            }
            if let Some((_, p)) = best {
                let found = m.create_path_with(
                    cx.w,
                    (p[0] as f32 + 0.5) as f64,
                    (p[1] as f32 + 0.5) as f64,
                    (p[2] as f32 + 0.5) as f64,
                    10.0,
                    true,
                    false,
                );
                set(m, found);
            }
        }
    } else if let Some(v) = m.attack_target.and_then(|t| cx.view(t)) {
        let p = path_to(m, cx, &v);
        set(m, p);
    }
    let iy = math::floor_double(m.bb.min[1] + 0.5);
    let in_water = m.in_water;
    let in_lava = m.in_material(cx.w, &m.bb, Material::Lava);
    m.pitch = 0.0;
    if has_path(m) && m.rand.next_int_n(100) != 0 {
        let reach = (m.width() * 2.0) as f64;
        let mut at = Some(m.path_vec(m.path_index));
        while let Some(p) = at {
            let d = (p[0] - m.pos[0]).powi(2) + (p[2] - m.pos[2]).powi(2);
            if !(d < reach * reach) {
                break;
            }
            m.path_index += 1;
            if m.path_index >= m.path.len() {
                at = None;
                m.clear_path();
            } else {
                at = Some(m.path_vec(m.path_index));
            }
        }
        m.old_jump = false;
        if let Some(p) = at {
            let dx = p[0] - m.pos[0];
            let dz = p[2] - m.pos[2];
            let dy = p[1] - iy as f64;
            let target = (dz.atan2(dx) * 180.0 / std::f64::consts::PI) as f32 - 90.0;
            let turn = wrap(target - m.yaw).clamp(-30.0, 30.0);
            // moveForward is the movementSpeed attribute itself (and the
            // pigman's attacking speed boost of 0.45 with a target)
            let mut speed = m.kind.speed();
            if m.kind == Kind::PigZombie && m.attack_target.is_some() {
                speed += 0.45;
            }
            m.move_forward = speed;
            m.ai_speed = 0.1;
            m.yaw += turn;
            if dy > 0.0 {
                m.old_jump = true;
            }
        }
        if let Some(v) = m.attack_target.and_then(|t| cx.view(t)) {
            face_entity(m, &v, 30.0, 30.0);
        }
        if m.collided_h && !has_path(m) {
            m.old_jump = true;
        }
        if m.rand.next_float() < 0.8 && (in_water || in_lava) {
            m.old_jump = true;
        }
    } else {
        // EntityLiving.updateEntityActionState
        m.age += 1;
        m.move_strafe = 0.0;
        m.move_forward = 0.0;
        if m.rand.next_float() < 0.02 {
            if closest_player(m, cx, 8.0).is_some() {
                m.idle_target = true;
                m.idle_ticks = 10 + m.rand.next_int_n(20);
            } else {
                m.random_yaw = (m.rand.next_float() - 0.5) * 20.0;
            }
        }
        if m.idle_target {
            let v = cx.view(Target::Player).unwrap();
            face_entity(m, &v, 10.0, 40.0);
            let t = m.idle_ticks;
            m.idle_ticks -= 1;
            if t <= 0 || !v.alive || m.dist2_to(&v) > 64.0 {
                m.idle_target = false;
            }
        } else {
            if m.rand.next_float() < 0.05 {
                m.random_yaw = (m.rand.next_float() - 0.5) * 20.0;
            }
            m.yaw += m.random_yaw;
            m.pitch = 0.0;
        }
        if in_water || in_lava {
            m.old_jump = m.rand.next_float() < 0.8;
        }
        m.clear_path();
    }
    m.jumping = m.old_jump;
    m.target_player = m.attack_target == Some(Target::Player);
}
