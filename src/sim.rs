//! The integrated server: owns the world and advances it at a fixed 20 ticks
//! per second on its own thread. The client only ever sees chunk snapshots and
//! talks to the simulation through commands, so a slow frame can never change
//! the outcome of a tick and a slow tick can never stall a frame.
//!
//! One tick is a pure function of the previous state and the commands applied
//! at its start: chunk loading order, population order and block update order
//! never depend on wall-clock time, hash-map iteration or thread scheduling.

use crate::block::{self, Material};
use crate::entity::survival::GameMode;
use crate::inventory::Inventory;
use crate::java::JavaRandom;
use crate::play::{InvView, ItemView, Open, Play};
use crate::render::mesher::MeshChunk;
use crate::world::save::{self, Level};
use crate::world::{BlockView, ChunkData, ChunkProvider, ServerWorld};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// New chunks brought in per tick (generated in parallel, inserted in order).
const LOADS_PER_TICK: usize = 8;
/// `MinecraftServer` autosave interval.
const AUTOSAVE_TICKS: i64 = 900;

#[path = "sim_bed.rs"]
mod bed;
#[path = "commands.rs"]
mod commands;
pub use commands::PLAYER_NAME;
#[path = "sim_dispense.rs"]
mod dispense;
#[path = "sim_fishing.rs"]
mod fishing;
#[path = "sim_items.rs"]
mod items;
#[path = "sim_riding.rs"]
mod riding;
#[path = "sim_vehicles.rs"]
mod vehicles;

#[derive(Clone, Debug)]
pub enum Command {
    /// `C16PacketClientStatus(OPEN_INVENTORY_ACHIEVEMENT)`: sent when the
    /// inventory key is pressed.
    OpenInventoryAchievement,
    /// The player's chunk; loading is centred on it.
    Center(i32, i32),
    /// `MINERUST_BENCH_TNT=n`: an n-cube of TNT eight blocks east of the
    /// player, one of them primed.
    BenchTnt(i32),
    Break([i32; 3]),
    Place {
        pos: [i32; 3],
        id: u8,
        meta: i32,
    },
    /// Position, rotation and flying state (`C03PacketPlayer`).
    Player([f64; 3], [f32; 2], bool),
    /// `onGround`, sneaking and sprinting, sent before `Player`
    /// (`C03PacketPlayer.onGround`, `C0BPacketEntityAction`).
    Motion {
        on_ground: bool,
        sneaking: bool,
        sprinting: bool,
    },
    /// Survival digging finished (or a creative click) at the block.
    Dig([i32; 3]),
    /// Right click on a block face.
    Use {
        target: [i32; 3],
        face: [i32; 3],
        sneaking: bool,
        /// Where on the block the click landed (`hitX`, `hitY`, `hitZ`).
        hit: [f32; 3],
    },
    /// `Use` itself, after its statistics are noted.
    UseOnInner {
        target: [i32; 3],
        face: [i32; 3],
        sneaking: bool,
        hit: [f32; 3],
    },
    Select(usize),
    /// `C0EPacketClickWindow`: container slot number (-999 outside),
    /// mouse button or hotbar key, click mode.
    WindowClick {
        slot: i32,
        button: i32,
        mode: i32,
    },
    OpenInventory,
    /// `C11PacketEnchantItem`
    EnchantItem(usize),
    /// `C17PacketCustomPayload` `MC|ItemName` (the anvil's text field)
    RenameItem(String),
    /// `MC|TrSel`: the offer chosen in the merchant window
    MerchantSelect(usize),
    /// `MC|Beacon`: the chosen primary and secondary powers.
    BeaconSet(i32, i32),
    /// `C16PacketClientStatus(REQUEST_STATS)`
    RequestStats,
    /// `MC|BEdit` / `MC|BSign`: the book and quill's pages, and its title
    /// when signed.
    BookEdit(Vec<String>, Option<String>),
    /// `C0BPacketEntityAction` 6: the horse jump bar's power (0..100)
    HorseJump(i32),
    /// `C12PacketUpdateSign`
    UpdateSign([i32; 3], [String; 4]),
    /// `C01PacketChatMessage`: chat text or a command.
    Chat(String),
    /// `C10PacketCreativeInventoryAction`: a `ContainerPlayer` slot set, or
    /// (slot below 0) a stack thrown.
    CreativeSlot {
        slot: i32,
        stack: Option<crate::item::Stack>,
    },
    CloseWindow,
    DropHeld {
        all: bool,
    },
    /// Middle click on a block (its position).
    PickBlock([i32; 3]),
    /// Middle click on an entity in creative: the item, its damage and
    /// `getHasSubtypes`.
    PickEntity(crate::item::ItemId, i32, bool),
    SetMode(GameMode),
    /// Health, food and the like, encoded by the client for the save file.
    Stats(String),
    /// The player died: the inventory spills out.
    Died,
    Respawn,
    /// `/give`: adds the stack to the inventory.
    Give(crate::item::Stack),
    /// "Leave Bed" (`GuiSleepMP`).
    LeaveBed,
    /// Survival digging started on a block (`C07PacketPlayerDigging` 0:
    /// `ItemInWorldManager.onBlockClicked`), with the side hit.
    ClickBlock([i32; 3], i32),
    /// Left click on a mob or vehicle (`C02PacketUseEntity` ATTACK; the
    /// server runs `attackTargetEntityWithCurrentItem`). `crit` is the
    /// client's falling state (`fallDistance > 0`, airborne, not on a
    /// ladder, in water or riding), `sprint` its `isSprinting`.
    Attack {
        id: u32,
        crit: bool,
        sprint: bool,
    },
    /// Whether mobs may target and hurt the player.
    PlayerState {
        attackable: bool,
    },
    /// Right click on a mob.
    Interact {
        id: u32,
    },
    /// The client has switched to the dimension it was sent.
    DimensionAck,
    /// Right click with nothing targeted (`Item.onItemRightClick`).
    UseAir,
    /// `C0CPacketInput`: `moveStrafing`, `moveForward`, jump and sneak
    /// while riding.
    Input {
        strafe: f32,
        forward: f32,
        jump: bool,
        sneak: bool,
    },
    /// `IntegratedServer`: the view distance follows the client's setting.
    ViewDistance(i32),
    /// `gameSettings.difficulty` (`IntegratedServer.tick`), by id.
    Difficulty(i32),
    /// The use key was let go while using an item
    /// (`C07PacketPlayerDigging` 5, `onPlayerStoppedUsing`).
    StopUsing,
    Shutdown,
}

pub enum Event {
    /// `S2CPacketSpawnGlobalEntity`: a lightning bolt.
    Lightning([f64; 3]),
    /// `S35PacketUpdateTileEntity` 4: a head's kind and rotation.
    Skull([i32; 3], i32, i32),
    /// The beacons whose beam shows (`isComplete`).
    Beacons(Vec<[i32; 3]>),
    /// `S34PacketMaps`: a map's colours and markers.
    Map(crate::map::MapView),
    /// `S37PacketStatistics`: statistic id and value.
    Statistics(Vec<(String, i32)>),
    /// `S36PacketSignEditorOpen`
    OpenSign([i32; 3]),
    /// `S33PacketUpdateSign`
    SignText([i32; 3], [String; 4]),
    /// `S02PacketChat`
    Chat(Vec<crate::chat::ChatMsg>),
    /// `S2BPacketChangeGameState` (reason 3): the game mode changed.
    GameMode(GameMode),
    /// A chunk appeared or changed. `ready` means nothing unpopulated can
    /// still write into it, so it may be meshed.
    Chunk {
        key: (i32, i32),
        chunk: MeshChunk,
        ready: bool,
        /// The 16-high sections that changed (all of them for a new chunk).
        changed: u16,
    },
    /// The end of one server tick's events, with the time it finished: the
    /// client applies the tick as a whole and interpolates what the server
    /// simulates (`prev` to `pos`) from this moment over the next 50 ms.
    /// The number is how many block digs and uses the server has handled
    /// (`ACTIONS_DONE`), which settles the client's predictions of them.
    Tick(Instant, u32),
    /// The chunk left the client's range.
    Unload((i32, i32)),
    Inventory(Box<InvView>),
    /// Every dropped item near the player, sent each tick.
    Items(Vec<ItemView>),
    /// After `Respawn`: where to put the player (feet).
    Respawn([f64; 3]),
    /// `S03PacketTimeUpdate`: total time, world time and `doDaylightCycle`
    /// (the packet's negative time).
    TimeUpdate(i64, i64, bool),
    /// `S05PacketSpawnPosition`: the world spawn (block coordinates).
    SpawnPosition([i32; 3]),
    /// Every mob and arrow near the player, sent each tick.
    Mobs(Vec<MobView>, Vec<ArrowView>),
    /// Damage and experience for the player.
    Hits(Vec<crate::entity::mob::Hit>),
    /// Sounds: name, position, volume, pitch.
    Sounds(Vec<(&'static str, [f64; 3], f32, f32)>),
    /// `S0DPacketCollectItem` for an experience orb the player took.
    CollectOrb(u32),
    /// `S0DPacketCollectItem` for items a mob picked up: the item and the
    /// mob it flies to.
    MobCollect(Vec<(crate::play::ItemView, u32)>),
    /// `S27PacketExplosion`s
    Explosions(Vec<crate::world::explosion::View>),
    Weather(Weather),
    /// Primed TNT near the player: position, previous position, fuse.
    Tnt(Vec<([f64; 3], [f64; 3], i32)>),
    /// Note blocks played this tick: position, instrument, pitch 0..24.
    Notes(Vec<([i32; 3], &'static str, i32)>),
    /// `playAuxSFX(2005)` (`ItemDye.func_150918_a` on the client).
    BoneMeal([i32; 3]),
    /// Particles from mobs: kind, position, motion.
    Particles(Vec<crate::entity::mob::Puff>),
    /// `playAuxSFX(2006)`: the dust of hard landings (block, `ceil(fall - 3)`).
    FallDust(Vec<([i32; 3], i32)>),
    /// The player moved to another dimension: drop every chunk and stand here.
    Dimension {
        dim: i32,
        pos: [f64; 3],
        yaw: f32,
    },
    /// Ticks spent in a Nether portal (`Entity.portalCounter`).
    Portal(i32),
    /// Moving piston blocks (`TileEntityPiston`), sent while any exist and
    /// once more when the last one stops.
    Moving(Vec<([i32; 3], crate::world::redstone::MovingView)>),
    /// The player's health, food, air, experience and effects
    /// (`S06PacketUpdateHealth`, `S1F`, `S1D`), when they change.
    PlayerStats(crate::entity::server_player::PlayerStatsView),
    /// `S12PacketEntityVelocity` (`set`) and `S27PacketExplosion`'s push (`add`).
    Velocity {
        set: Option<[f64; 3]>,
        add: [f64; 3],
    },
    /// `S19PacketEntityStatus` 2: the hurt animation and sound.
    Hurt,
    /// `S0APacketUseBed` (the head of the bed and where the player lies) or,
    /// with `None`, `S0BPacketAnimation` 2 (waking up).
    Sleep(Option<([i32; 3], [f64; 3])>),
    /// `S0BPacketAnimation` 4 / 5: an `EntityCrit2FX` of "crit" (false) or
    /// "magicCrit" (true) on the mob.
    CritFx(u32, bool),
    /// The server's `setSprinting(false)` (entity metadata flag 3).
    StopSprinting,
    /// `setItemInUse` started (stack, ticks) or `clearItemInUse` (None).
    Using(Option<(crate::item::Stack, i32)>),
    /// `S19PacketEntityStatus` 9: the item use finished.
    UseFinished(crate::item::Stack),
    /// The player died: `CombatTracker.getDeathMessage` (key, arguments).
    Died(crate::entity::server_player::Message),
    /// `S08PacketPlayerPosLook`: the server moved the player (feet).
    Teleport([f64; 3]),
    /// `playAuxSFX(2002)`: splash potions broke (position, damage value).
    Splashes(Vec<([f64; 3], i32)>),
    /// Snowballs, eggs, pearls and potions in flight: position, previous
    /// position, item and damage.
    Throwables(Vec<([f64; 3], [f64; 3], u16, i32)>),
    /// `playAuxSFX(1005)`: a jukebox starts this record (item id) or stops (0).
    Record([i32; 3], i32),
    /// The player's `EntityFishHook` (position, previous position).
    FishHook(Option<([f64; 3], [f64; 3])>),
    /// `EntityTrackerEntry` packets about vehicles: id and packet.
    Vehicles(Vec<(u32, crate::entity::tracker::Packet)>),
    /// `S1CPacketEntityMetadata` of vehicles.
    VehicleMeta(Vec<(u32, crate::entity::vehicle::VMeta)>),
    /// `setEntityState(17)` of a firework rocket: where, its motion and the
    /// firework item (`WorldClient.makeFireworks`).
    Firework([f64; 3], [f64; 3], Option<crate::item::Stack>),
    /// `S19PacketEntityStatus` of vehicles (10: a TNT minecart ignites).
    VehicleStatus(Vec<(u32, i32)>),
    /// `S1BPacketEntityAttach` for the player: the vehicle, or none.
    Mount(Option<u32>),
    /// `playAuxSFX(2001)` of blocks the world broke: position, block, meta.
    Destroyed(Vec<([i32; 3], u8, i32)>),
    /// `playAuxSFX` sounds whose pitch the client picks (1010 to 1012).
    AuxSounds(Vec<(i32, [i32; 3])>),
    /// `S25PacketBlockBreakAnim`: breaker, block, progress (outside 0..10
    /// ends it).
    BreakAnims(Vec<(u32, [i32; 3], i32)>),
    /// Falling blocks near the player, sent each tick.
    Falling(Vec<crate::entity::falling::FallingView>),
    /// The player picked these items up (`S0DPacketCollectItem`).
    Collect(Vec<crate::play::ItemView>),
    /// `TileEntityPiston.launchWithSlimeBlock` boxes and displacements, for
    /// the player's own `moveEntity`.
    Push(Vec<([f64; 3], [f64; 3], [f64; 3])>),
    /// Fireballs and thrown eyes: position, previous position, kind
    /// (0 large fireball, 1 small fireball, 2 eye of ender).
    Fireballs(Vec<([f64; 3], [f64; 3], u8)>),
}

/// `EntityEnderEye`: floats towards the stronghold for four seconds.
#[derive(Clone, Debug)]
pub struct EnderEye {
    pos: [f64; 3],
    prev: [f64; 3],
    motion: [f64; 3],
    target: [f64; 3],
    timer: i32,
    drop: bool,
}

/// A dimension the player is not in, kept loaded and saved with the rest.
struct Parked {
    dim: i32,
    world: ServerWorld,
    mobs: crate::entity::mob::Mobs,
    unsaved: std::collections::BTreeSet<(i32, i32)>,
    tiles: std::collections::BTreeMap<[i32; 3], crate::play::Tile>,
    items: Vec<crate::entity::item::ItemEntity>,
    falling: Vec<crate::entity::falling::FallingBlock>,
    vehicles: Vec<crate::entity::vehicle::Tracked>,
    teleport_rand: JavaRandom,
}

/// `C0CPacketInput` while riding.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RideInput {
    pub strafe: f32,
    pub forward: f32,
    pub jump: bool,
    pub sneak: bool,
}

/// What the client draws of a mob.
#[derive(Clone, Debug)]
pub struct MobView {
    pub id: u32,
    pub kind: crate::entity::mob::Kind,
    pub pos: [f64; 3],
    pub prev: [f64; 3],
    pub body_yaw: f32,
    pub prev_body_yaw: f32,
    pub head_yaw: f32,
    pub prev_head_yaw: f32,
    /// The client's `EntityBodyHelper` (`prevRenderYawHead`,
    /// `rotationTickCounter`) for the classes with task AI.
    pub body_head: f32,
    pub body_ticks: i32,
    pub pitch: f32,
    pub prev_pitch: f32,
    pub limb_swing: f32,
    pub limb_amount: f32,
    pub prev_limb_amount: f32,
    pub swing: f32,
    pub prev_swing: f32,
    pub hurt_time: i32,
    pub death_time: i32,
    pub ticks: i32,
    pub variant: i32,
    pub sheared: bool,
    pub fuse: i32,
    pub prev_fuse: i32,
    pub wing: f32,
    pub prev_wing: f32,
    pub eat_timer: i32,
    pub fire: bool,
    /// dataWatcher 7 and 8: potion particle colour and ambience
    pub potion: (i32, bool),
    pub bb: [f64; 6],
    /// Slime squish (previous, current).
    pub squish: [f32; 2],
    /// Squid pitch, yaw, rotation, tentacle angle (previous, current).
    pub sq: [[f32; 4]; 2],
    /// 1 tamed, 2 sitting, 4 angry or screaming, 8 hanging bat, 16 child.
    pub flags: u32,
    /// Enderman carried block and data.
    pub carried: [u8; 2],
    /// Iron golem `attackTimer`.
    pub attack_timer: i32,
    /// `EntityLiving.equipment` as the client sees it
    /// (`S04PacketEntityEquipment`).
    pub equipment: [Option<crate::item::Stack>; 5],
    pub health: f32,
    /// Ender dragon movement history (yaw, y), newest at `ring_index`.
    pub ring: Vec<[f64; 2]>,
    pub ring_index: i32,
    /// The tracker's packets as the client got them: target position and
    /// angles of `setPositionAndRotation2`, a counter that moves with every
    /// packet, and the `S19` head yaw.
    pub net_at: [f64; 3],
    pub net_yaw: f32,
    pub net_pitch: f32,
    pub net_seq: u32,
    pub net_head: f32,
    /// Client side: `rotationYaw` and `newPosRotationIncrements`.
    pub yaw: f32,
    pub incs: i32,
    /// `getCustomNameTag`, and whether the player rides it (no label then).
    pub custom_name: String,
    pub ridden: bool,
    /// `EntityHorse`: data watcher 16 and 22, `getHorseSize`, the client's
    /// head lean, rearing and mouth amounts (previous, current each) and
    /// `tailCounter`.
    pub horse_flags: u32,
    pub horse_armor: i32,
    pub horse_size: f32,
    pub hanim: [f32; 6],
    pub tail_counter: i32,
    /// `getMaxHealth`, for the mount's hearts.
    pub max_health: f32,
    /// `EntityWither.getInvulTime` and the side heads' pitch and yaw
    /// (previous, current).
    pub wither_invul: i32,
    pub wither_heads: [[[f32; 2]; 2]; 2],
    /// `EntityWolf`: wet (`getWolfShaking`), the shake (previous, current),
    /// begging, and the client's `headRotationCourse` (old, new).
    pub wolf_wet: bool,
    pub shake: [f32; 2],
    pub begging: bool,
    pub head_course: [f32; 2],
    /// `S1BPacketEntityAttach(1, ...)`: whom the lead goes to.
    pub leash: Option<LeashView>,
}

/// The holder of a lead as the client draws it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LeashView {
    Player,
    /// A leash knot's position.
    Knot([f64; 3]),
}

#[derive(Clone, Debug)]
pub struct ArrowView {
    /// `isBurning`
    pub burning: bool,
    pub pos: [f64; 3],
    pub prev: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
}

pub struct SimConfig {
    pub seed: i64,
    pub view: i32,
    pub save_dir: Option<PathBuf>,
    pub start_time: i64,
    pub freeze_time: bool,
    pub mode: GameMode,
}

pub struct Start {
    pub pos: [f64; 3],
    pub rot: [f32; 2],
    pub flying: bool,
    pub world_time: i64,
    pub seed: i64,
    pub resumed: bool,
    pub mode: GameMode,
    /// Saved player stats, if any.
    pub stats: Option<String>,
}

pub struct Sim {
    pub world: ServerWorld,
    centre: (i32, i32),
    view: i32,
    offsets: Vec<(i32, i32)>,
    /// The last `Event::Moving` listed some blocks.
    sent_moving: bool,
    /// An ender pearl landed: the player is moved there.
    teleport_to: Option<[f64; 3]>,
    /// Chat lines for the client (`S02PacketChat`).
    chat_out: Vec<crate::chat::ChatMsg>,
    /// `/gamemode` ran: the client is told the new mode.
    mode_changed: bool,
    /// Events from commands, sent with the tick's.
    vehicle_events: Vec<Event>,
    /// Sounds from vehicles outside their tick.
    vehicle_sounds: Vec<(&'static str, [f64; 3], f32, f32)>,
    /// `ridingEntity` of the player: a boat's id.
    riding: Option<u32>,
    /// `EntityPlayer.playerLocation` while sleeping, `sleepTimer`, and the
    /// bed that is the spawn point (`spawnChunk`).
    sleeping: Option<[i32; 3]>,
    sleep_timer: i32,
    bed_spawn: Option<[i32; 3]>,
    /// `S0APacketUseBed` (bed, position) / the wake animation, for the client.
    sleep_event: Option<Option<([i32; 3], [f64; 3])>>,
    wake_exit: Option<[f64; 3]>,
    /// The last `C0CPacketInput`.
    ride: RideInput,
    /// Where the player's tracker entry last re-checked the watchers.
    track_check: Option<[f64; 3]>,
    /// The item in use started or stopped this tick.
    using_changed: bool,
    /// `setSprinting(false)` after a knockback hit, for the client.
    stop_sprinting: bool,
    bone_meal_fx: Vec<[i32; 3]>,
    /// `S0BPacketAnimation` 4 (critical) / 5 (magic critical) on a mob.
    crit_fx: Vec<(u32, bool)>,
    /// `S27PacketExplosion` pushes for the client this tick.
    push_add: [f64; 3],
    /// `EntityPlayerMP`: the server side of the player.
    pub splayer: crate::entity::server_player::ServerPlayer,
    /// `World.difficultySetting`
    pub difficulty: crate::entity::living::Difficulty,
    /// The last `Command::Motion`.
    motion_flags: (bool, bool, bool),
    /// Chunks the client currently holds, with the `ready` flag last sent.
    sent: std::collections::BTreeMap<(i32, i32), bool>,
    /// Chunks changed since they were last written to disk.
    unsaved: std::collections::BTreeSet<(i32, i32)>,
    pub world_time: i64,
    freeze_time: bool,
    player: Option<([f64; 3], [f32; 2], bool)>,
    /// The mob the player hit last and the last one that hit the player.
    last_attacked: Option<u32>,
    last_attacker: Option<u32>,
    /// `EntityLivingBase.lastAttackerTime` and `revengeTimer` of the player
    /// (in ticks of this world).
    last_attacked_time: i32,
    last_attacker_time: i32,
    unknown_level_keys: Vec<(String, String)>,
    pub play: Play,
    spawn: [f64; 3],
    /// The world spawn last sent to the client (`S05PacketSpawnPosition`
    /// on login and after `/setworldspawn`).
    sent_spawn: Option<[i32; 3]>,
    /// The world's game rules until the first `step` installs them on the
    /// server thread (`gamerules`).
    rules_pending: Option<crate::gamerules::GameRules>,
    /// `MinecraftServer.tickCounter`, for the time packet every 20 ticks.
    server_ticks: u64,
    stats: Option<String>,
    respawn_pending: bool,
    pub mobs: crate::entity::mob::Mobs,
    player_attackable: bool,
    pending_out: Vec<crate::entity::mob::Outcome>,
    /// `EntityPlayer.fishEntity`
    fish_hook: Option<crate::entity::fish_hook::FishHook>,
    /// `NetHandlerPlayClient.avRandomizer` for server particle packets
    av_rand: JavaRandom,
    /// `EntityAIControlledByPlayer`: carrot-on-a-stick wear to apply.
    stick_wear: i32,
    /// Stands in for each `TileEntityDispenser`'s own `new Random()`.
    tile_rand: JavaRandom,
    /// `Bootstrap$12.succeeded` / `Bootstrap$13.succeeded`: once false,
    /// false for good (fixed outside `--vanilla-parity`).
    flint_succeeded: bool,
    dye_succeeded: bool,
    pub weather: Weather,
    /// `EntityPlayer.dimension`
    pub dim: i32,
    parked: Vec<Parked>,
    /// `Teleporter.random` of the current dimension.
    teleport_rand: JavaRandom,
    /// `Entity.portalCounter` and `timeUntilPortal`.
    portal_ticks: i32,
    portal_cooldown: i32,
    /// Player positions are stale until the client confirms a dimension change.
    await_ack: bool,
    /// The world directory (the overworld's).
    base_dir: Option<PathBuf>,
    /// `MapStorage`'s maps, and the ids whose data the client has.
    pub maps: crate::map::Maps,
    maps_sent: std::collections::HashSet<i32>,
    /// `EntityLightningBolt`s: position, `lightningState`,
    /// `boltLivingTime` and the entity's random.
    bolts: Vec<([f64; 3], i32, i32, JavaRandom)>,
    beams_sent: Vec<[i32; 3]>,
    /// A dimension change asked for by a command (respawning out of the Nether).
    pending_travel: Option<i32>,
    /// Tell the client on the first tick which dimension the save is in.
    announce_dim: bool,
    /// `EntityPlayerMP.statsFile`
    stats_file: crate::stats::StatisticsFile,
    /// `ServerConfigurationManager.initializeConnectionToPlayer` sends the
    /// unlocked achievements on the first tick.
    send_achievements: bool,
    /// statistics to send with this tick's events
    stats_out: Vec<(String, i32)>,
    stats_sent_all: bool,
    was_dead: bool,
    /// `startMinecartRidingCoordinate`
    rail_start: Option<[i32; 3]>,
    eyes: Vec<EnderEye>,
    /// `level.dat` of a world opened from vanilla (Anvil mode).
    anvil_level: Option<crate::world::anvil::Nbt>,
    /// Chunks whose tile entities and entities were taken over already.
    anvil_imported: std::collections::BTreeSet<(i32, i32, i32)>,
}

/// Level keys owned by the gameplay state (kept out of `unknown`).
const PLAY_KEYS: [&str; 8] = [
    "gamerules",
    "inventory",
    "mode",
    "tile",
    "spawn",
    "stats",
    "weather",
    "dim",
];

/// `WorldInfo` rain and thunder state and `World.updateWeather`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Weather {
    pub raining: bool,
    pub rain_time: i32,
    pub thundering: bool,
    pub thunder_time: i32,
    pub rain: f32,
    pub prev_rain: f32,
    pub thunder: f32,
    pub prev_thunder: f32,
}

impl Weather {
    pub fn update(&mut self, r: &mut JavaRandom) {
        if self.thunder_time <= 0 {
            self.thunder_time = if self.thundering {
                r.next_int_n(12000) + 3600
            } else {
                r.next_int_n(168000) + 12000
            };
        } else {
            self.thunder_time -= 1;
            if self.thunder_time <= 0 {
                self.thundering = !self.thundering;
            }
        }
        self.prev_thunder = self.thunder;
        self.thunder = (self.thunder + if self.thundering { 0.01 } else { -0.01 }).clamp(0.0, 1.0);
        if self.rain_time <= 0 {
            self.rain_time = if self.raining {
                r.next_int_n(12000) + 12000
            } else {
                r.next_int_n(168000) + 12000
            };
        } else {
            self.rain_time -= 1;
            if self.rain_time <= 0 {
                self.raining = !self.raining;
            }
        }
        self.prev_rain = self.rain;
        self.rain = (self.rain + if self.raining { 0.01 } else { -0.01 }).clamp(0.0, 1.0);
    }
    fn encode(&self) -> String {
        format!(
            "{},{},{},{},{},{}",
            self.raining as i32,
            self.rain_time,
            self.thundering as i32,
            self.thunder_time,
            self.rain,
            self.thunder
        )
    }
    fn decode(s: &str) -> Option<Weather> {
        let f: Vec<&str> = s.split(',').collect();
        if f.len() != 6 {
            return None;
        }
        let rain: f32 = f[4].parse().ok()?;
        let thunder: f32 = f[5].parse().ok()?;
        Some(Weather {
            raining: f[0] == "1",
            rain_time: f[1].parse().ok()?,
            thundering: f[2] == "1",
            thunder_time: f[3].parse().ok()?,
            rain,
            prev_rain: rain,
            thunder,
            prev_thunder: thunder,
        })
    }
}

fn spiral(radius: i32) -> Vec<(i32, i32)> {
    let mut v = Vec::new();
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            v.push((dx, dz));
        }
    }
    // Total order: distance first, then coordinates, so ties never depend on
    // the sort implementation.
    v.sort_by_key(|&(x, z)| (x * x + z * z, x, z));
    v
}

impl Sim {
    pub fn new(cfg: &SimConfig) -> (Sim, Start) {
        let level = cfg.save_dir.as_ref().and_then(|d| save::read_level(d));
        // A vanilla world: level.dat and region files, no level.txt.
        let anvil = if level.is_none() {
            cfg.save_dir
                .as_ref()
                .and_then(|d| crate::world::anvil::read_level_dat(d))
        } else {
            None
        };
        let anvil_seed = anvil
            .as_ref()
            .and_then(|r| r.get("Data"))
            .and_then(|d| d.int("RandomSeed"));
        let seed = level
            .as_ref()
            .map_or(anvil_seed.unwrap_or(cfg.seed), |l| l.seed);
        if let Some(l) = &level {
            crate::compat::set_vanilla_parity(l.vanilla_parity);
        }
        let provider = Arc::new(ChunkProvider::new(seed));
        let mut world = ServerWorld::new(provider);
        world.save_dir = cfg.save_dir.clone();
        world.anvil = anvil.is_some();
        if let Some(d) = &cfg.save_dir {
            // data/structures.txt: the MapGenStructureData of vanilla.
            if let Ok(text) = std::fs::read_to_string(d.join("data").join("structures.txt")) {
                if let Some(st) = world.structures.as_mut() {
                    if st.load(text.trim()).is_none() {
                        eprintln!("structure data is damaged; structures will be re-derived");
                    }
                }
            }
        }
        let mut sim = Sim {
            world,
            centre: (0, 0),
            view: cfg.view,
            offsets: spiral(cfg.view + 2),
            sent_moving: false,
            splayer: crate::entity::server_player::ServerPlayer::new(),
            difficulty: crate::entity::living::Difficulty::Normal,
            motion_flags: (false, false, false),
            push_add: [0.0; 3],
            using_changed: false,
            stop_sprinting: false,
            bone_meal_fx: Vec::new(),
            crit_fx: Vec::new(),
            teleport_to: None,
            chat_out: Vec::new(),
            mode_changed: false,
            vehicle_events: Vec::new(),
            vehicle_sounds: Vec::new(),
            riding: None,
            sleeping: None,
            sleep_timer: 0,
            bed_spawn: None,
            sleep_event: None,
            wake_exit: None,
            ride: RideInput::default(),
            track_check: None,
            sent: Default::default(),
            unsaved: Default::default(),
            world_time: cfg.start_time,
            freeze_time: cfg.freeze_time,
            player: None,
            last_attacked: None,
            last_attacker: None,
            last_attacked_time: 0,
            last_attacker_time: 0,
            unknown_level_keys: Vec::new(),
            play: Play::new(cfg.mode),
            spawn: [0.0; 3],
            sent_spawn: None,
            rules_pending: Some(crate::gamerules::GameRules::default()),
            server_ticks: 0,
            stats: None,
            respawn_pending: false,
            mobs: crate::entity::mob::Mobs::default(),
            player_attackable: true,
            pending_out: Vec::new(),
            fish_hook: None,
            av_rand: JavaRandom::new(
                (crate::util::math::math_random() * (1u64 << 48) as f64) as i64,
            ),
            tile_rand: JavaRandom::new(
                (crate::util::math::math_random() * (1u64 << 48) as f64) as i64,
            ),
            flint_succeeded: true,
            stick_wear: 0,
            dye_succeeded: true,
            weather: Weather::default(),
            dim: 0,
            parked: Vec::new(),
            teleport_rand: JavaRandom::new(seed),
            portal_ticks: 0,
            portal_cooldown: 0,
            await_ack: false,
            base_dir: cfg.save_dir.clone(),
            maps: crate::map::Maps::new(cfg.save_dir.clone()),
            maps_sent: Default::default(),
            bolts: Vec::new(),
            beams_sent: Vec::new(),
            pending_travel: None,
            announce_dim: false,
            stats_file: crate::stats::StatisticsFile::open(
                cfg.save_dir.as_deref(),
                commands::PLAYER_NAME,
            ),
            send_achievements: true,
            stats_out: Vec::new(),
            stats_sent_all: false,
            was_dead: false,
            rail_start: None,
            eyes: Vec::new(),
            anvil_level: None,
            anvil_imported: Default::default(),
        };
        if let Some(d) = &cfg.save_dir {
            if let Ok(text) = std::fs::read_to_string(d.join("data").join("entities.txt")) {
                for line in text.lines() {
                    let _ = sim.mobs.decode(line);
                }
            }
            if let Ok(b) = std::fs::read(d.join("data").join("drops.dat")) {
                let mut r = JavaRandom::new(cfg.seed);
                sim.play.load_drops(&b, &mut r);
            }
        }
        let mut saved_dim = 0;
        sim.anvil_level = anvil;
        let anvil_start = sim
            .anvil_level
            .clone()
            .map(|root| sim.start_from_level_dat(&root, &mut saved_dim));
        let start = match level {
            _ if anvil_start.is_some() => anvil_start.unwrap(),
            Some(l) => {
                sim.world.total_time = l.total_time;
                sim.world_time = l.world_time;
                for t in &l.ticks {
                    sim.world.restore_tick(t.0, t.1, t.2, t.3, t.4);
                }
                sim.unknown_level_keys = l
                    .unknown
                    .iter()
                    .filter(|(k, _)| !PLAY_KEYS.contains(&k.as_str()))
                    .cloned()
                    .collect();
                let mut spawn = None;
                for (k, v) in &l.unknown {
                    match k.as_str() {
                        "dim" => saved_dim = v.parse().unwrap_or(0),
                        "inventory" => {
                            if let Some(inv) = Inventory::decode(v) {
                                sim.play.inv = inv;
                            }
                        }
                        "mode" => {
                            sim.play.mode = if v == "creative" {
                                GameMode::Creative
                            } else {
                                GameMode::Survival
                            }
                        }
                        "tile" => {
                            let _ = sim.play.decode_tile(v);
                        }
                        "spawn" => {
                            let f: Vec<f64> = v.split(',').filter_map(|x| x.parse().ok()).collect();
                            if f.len() == 3 {
                                spawn = Some([f[0], f[1], f[2]]);
                            }
                        }
                        "stats" => {
                            sim.stats = Some(v.clone());
                            sim.splayer.decode(v);
                        }
                        "weather" => {
                            if let Some(w) = Weather::decode(v) {
                                sim.weather = w;
                            }
                        }
                        // name=value pairs, ';' apart
                        "gamerules" => {
                            let mut g = crate::gamerules::GameRules::default();
                            for kv in v.split(';') {
                                if let Some((k, val)) = kv.split_once('=') {
                                    g.0.insert(k.to_string(), val.to_string());
                                }
                            }
                            sim.rules_pending = Some(g);
                        }
                        _ => {}
                    }
                }
                sim.player = l.player;
                sim.spawn = match spawn {
                    Some(s) => s,
                    None => sim.find_spawn(),
                };
                let (pos, rot, flying) = l.player.unwrap_or((sim.spawn, [0.0, 0.0], false));
                Start {
                    pos,
                    rot,
                    flying,
                    world_time: l.world_time,
                    seed,
                    resumed: true,
                    mode: sim.play.mode,
                    stats: sim.stats.clone(),
                }
            }
            None => {
                let pos = sim.find_spawn();
                sim.spawn = pos;
                Start {
                    pos,
                    rot: [0.0, 0.0],
                    flying: false,
                    world_time: cfg.start_time,
                    seed,
                    resumed: false,
                    mode: sim.play.mode,
                    stats: None,
                }
            }
        };
        // Unit tests build small worlds of their own and would only get
        // slower and noisier (gravel falling far away); one test covers it.
        if !cfg!(test) {
            sim.initial_world_chunk_load();
        }
        // `/weather`: forced from the command line.
        if let Ok(w) = std::env::var("MINERUST_WEATHER") {
            let on = w == "rain" || w == "thunder";
            sim.weather.raining = on;
            sim.weather.thundering = w == "thunder";
            sim.weather.rain_time = if on { 24000 } else { 168000 };
            sim.weather.thunder_time = if w == "thunder" { 24000 } else { 168000 };
            sim.weather.rain = if on { 1.0 } else { 0.0 };
            sim.weather.prev_rain = sim.weather.rain;
            sim.weather.thunder = if w == "thunder" { 1.0 } else { 0.0 };
            sim.weather.prev_thunder = sim.weather.thunder;
        }
        sim.play.player = (start.pos, start.rot[0]);
        if let Some(d) = std::env::var("MINERUST_DIM")
            .ok()
            .and_then(|v| v.parse::<i32>().ok())
        {
            if d != saved_dim {
                sim.pending_travel = Some(d);
            }
        }
        if saved_dim != 0 {
            // the player was saved in the Nether or the End
            let next = sim.take_world(saved_dim);
            let old = Parked {
                dim: 0,
                world: std::mem::replace(&mut sim.world, next.world),
                mobs: std::mem::replace(&mut sim.mobs, next.mobs),
                unsaved: std::mem::replace(&mut sim.unsaved, next.unsaved),
                tiles: std::mem::replace(&mut sim.play.tiles, next.tiles),
                items: std::mem::replace(&mut sim.play.items, next.items),
                falling: std::mem::replace(&mut sim.play.falling, next.falling),
                vehicles: std::mem::replace(&mut sim.play.vehicles, next.vehicles),
                teleport_rand: std::mem::replace(&mut sim.teleport_rand, next.teleport_rand),
            };
            sim.world.total_time = old.world.total_time;
            sim.parked.push(old);
            sim.dim = saved_dim;
            sim.announce_dim = true;
        }
        sim.centre = (
            (start.pos[0].floor() as i32) >> 4,
            (start.pos[2].floor() as i32) >> 4,
        );
        (sim, start)
    }

    /// `MinecraftServer.initialWorldChunkLoad` ("Preparing spawn area"): every
    /// server start loads the 25x25 chunks around the overworld spawn point,
    /// x outer and z inner, before any player is placed. Population of a
    /// chunk runs as soon as its neighbours exist, and decorations reach into
    /// the neighbouring chunks, so where trees overlap the result depends on
    /// this order; loading outward from the player instead gave other trees
    /// near spawn than vanilla.
    ///
    /// Terrain only depends on the seed and the chunk position, so it is
    /// generated in parallel; chunks are inserted (and populated) strictly in
    /// vanilla's order.
    fn initial_world_chunk_load(&mut self) {
        if self.world.provider.dim != 0 {
            return;
        }
        // WorldInfo spawn: the block the spawn position stands on
        let (sx, sz) = (self.spawn[0].floor() as i32, self.spawn[2].floor() as i32);
        let order: Vec<(i32, i32)> = (-192..=192)
            .step_by(16)
            .flat_map(|x| {
                (-192..=192)
                    .step_by(16)
                    .map(move |z| ((sx + x) >> 4, (sz + z) >> 4))
            })
            .collect();
        let fresh: Vec<(i32, i32)> = order
            .iter()
            .copied()
            .filter(|k| !self.world.chunk_exists(k.0, k.1) && !self.world.is_stored(k.0, k.1))
            .collect();
        let provider = self.world.provider.clone();
        let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
        let per = fresh.len().div_ceil(workers.max(1)).max(1);
        let mut generated: std::collections::HashMap<(i32, i32), ChunkData> =
            std::thread::scope(|s| {
                let handles: Vec<_> = fresh
                    .chunks(per)
                    .map(|part| {
                        let p = provider.clone();
                        s.spawn(move || {
                            part.iter()
                                .map(|&(cx, cz)| ((cx, cz), p.provide_chunk(cx, cz)))
                                .collect::<Vec<_>>()
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .flat_map(|h| h.join().expect("chunk generation panicked"))
                    .collect()
            });
        for (cx, cz) in order {
            if self.world.chunk_exists(cx, cz) {
                // pulled in by a neighbour's population in the meantime
                continue;
            }
            match generated.remove(&(cx, cz)) {
                Some(data) => {
                    self.world.insert_generated(data);
                }
                None => self.world.load_chunk(cx, cz),
            }
        }
    }

    /// `WorldServer.createSpawnPosition` followed by the player placement of
    /// `EntityPlayerMP` (without its random offset). Returns the feet position.
    fn find_spawn(&mut self) -> [f64; 3] {
        let provider = self.world.provider.clone();
        let world = &mut self.world;
        let (mut sx, mut sz) = {
            let mut top_is_grass = |x: i32, z: i32| {
                let mut k = 63;
                while !world.is_air(x, k + 1, z) {
                    k += 1;
                }
                world.block(x, k, z) == block::GRASS
            };
            provider.find_spawn(&mut top_is_grass)
        };
        let dry = |w: &mut ServerWorld, x: i32, z: i32| {
            let y = w.top_solid_or_liquid(x, z);
            y > 0 && !w.material(x, y, z).is_liquid() && !w.material(x, y - 1, z).is_liquid()
        };
        if crate::compat::fixed() && !dry(world, sx, sz) {
            // MC-7908: when the search above gives up, vanilla leaves the
            // player wherever it stopped, possibly in the middle of an ocean.
            'search: for ring in 1..128i32 {
                for a in -ring..=ring {
                    for (dx, dz) in [(a, -ring), (a, ring), (-ring, a), (ring, a)] {
                        let (x, z) = (sx + dx * 4, sz + dz * 4);
                        if dry(world, x, z) {
                            sx = x;
                            sz = z;
                            break 'search;
                        }
                    }
                }
            }
        }
        let mut y = world.top_solid_or_liquid(sx, sz).max(1);
        let (px, pz) = (sx as f64 + 0.5, sz as f64 + 0.5);
        // Move up while the bounding box is obstructed.
        loop {
            let bb = crate::entity::player::Aabb {
                min: [px - 0.3, y as f64, pz - 0.3],
                max: [px + 0.3, y as f64 + 1.8, pz + 0.3],
            };
            if crate::entity::player::colliding_boxes(&*world as &dyn BlockView, &bb).is_empty()
                || y >= 255
            {
                break;
            }
            y += 1;
        }
        [px, y as f64, pz]
    }

    fn ready(&self, cx: i32, cz: i32) -> bool {
        for dx in -1..=0 {
            for dz in -1..=0 {
                match self.world.chunks.get(&(cx + dx, cz + dz)) {
                    Some(c) if c.terrain_populated => {}
                    _ => return false,
                }
            }
        }
        true
    }

    fn snapshot(&self, key: (i32, i32)) -> Option<MeshChunk> {
        let c = self.world.chunks.get(&key)?;
        Some(MeshChunk {
            data: c.data.clone(),
            sections: c.sections,
            height: Box::new(c.height),
            pots: self.play.pots_in(key.0, key.1),
        })
    }

    /// Applies one player command with server-side validation.
    fn apply(&mut self, cmd: &Command) {
        match *cmd {
            Command::Center(x, z) => self.centre = (x, z),
            Command::BenchTnt(n) => {
                let f = self.play.player.0;
                let (x, y, z) = (
                    f[0].floor() as i32 + 8,
                    f[1].floor() as i32,
                    f[2].floor() as i32,
                );
                for dx in 0..n {
                    for dy in 0..n {
                        for dz in 0..n {
                            self.world
                                .set_block(x + dx, y + dy, z + dz, crate::block::TNT, 0, 3);
                        }
                    }
                }
                self.world.prime_tnt(x, y + n - 1, z, 20);
            }
            Command::ViewDistance(v) => {
                self.view = v;
                self.offsets = spiral(v + 2);
            }
            Command::Difficulty(d) => {
                use crate::entity::living::Difficulty as D;
                self.difficulty = match d {
                    0 => D::Peaceful,
                    1 => D::Easy,
                    3 => D::Hard,
                    _ => D::Normal,
                };
            }
            Command::Motion {
                on_ground,
                sneaking,
                sprinting,
            } => self.motion_flags = (on_ground, sneaking, sprinting),
            Command::Player(p, r, f) => {
                if self.await_ack {
                    return;
                }
                // NetHandlerPlayServer.processPlayer: a sleeping player stays put
                if self.sleeping.is_some() {
                    return;
                }
                if self.riding.is_some() {
                    // NetHandlerPlayServer.processPlayer, riding: only the look
                    // and onGround; the boat places the player.
                    let feet = self.play.player.0;
                    self.player = Some((feet, r, f));
                    self.play.player.1 = r[0];
                    self.play.pitch = r[1];
                    self.splayer.e.on_ground = self.motion_flags.0;
                    let _ = p;
                    return;
                }
                let (g, sn, sp) = self.motion_flags;
                self.splayer.moved(&self.world, p, g, f, sn, sp);
                let d = self.difficulty;
                self.splayer.block_contacts(&mut self.world, d);
                self.player = Some((p, r, f));
                self.play.player = (p, r[0]);
                self.play.pitch = r[1];
            }
            Command::DimensionAck => self.await_ack = false,
            Command::UseAir => self.use_air(),
            Command::StopUsing => {
                // EntityPlayer.stopUsingItem
                if let Some((_, st, left)) = self.splayer.using.take() {
                    self.stopped_using(st, left);
                }
            }
            Command::Break(p) => {
                let id = self.world.block(p[0], p[1], p[2]);
                if id != block::AIR && id != block::BEDROCK && !block::material(id).is_liquid() {
                    self.world.set_block(p[0], p[1], p[2], block::AIR, 0, 3);
                }
            }
            Command::Place { pos: p, id, meta } => {
                if !(0..256).contains(&p[1]) || !block::info(id).exists || id == block::AIR {
                    return;
                }
                let w = &mut self.world;
                if !w.material(p[0], p[1], p[2]).replaceable() {
                    return;
                }
                // `ItemBlock.onItemUse`: the block decides whether it can
                // exist there (MC-1168: a lily pad must sit on still water and
                // never replaces it).
                if !w.can_place_block_at(id, p[0], p[1], p[2]) {
                    return;
                }
                let needs_support = matches!(
                    block::info(id).material,
                    Material::Plants | Material::Vine | Material::Cactus
                );
                if needs_support
                    && id != block::DOUBLE_PLANT
                    && id != block::VINE
                    && !w.can_block_stay(id, p[0], p[1], p[2])
                {
                    return;
                }
                if id == block::DOUBLE_PLANT {
                    w.set_block(p[0], p[1], p[2], id, meta & 7, 3);
                    w.set_block(p[0], p[1] + 1, p[2], id, 8, 3);
                } else {
                    w.set_block(p[0], p[1], p[2], id, meta, 3);
                }
            }
            Command::ClickBlock(p, side) => {
                let mut play = std::mem::replace(&mut self.play, Play::new(GameMode::Survival));
                if play.mode == GameMode::Survival {
                    play.can_eat = self.splayer.can_eat(false);
                    play.click_block(&mut self.world, p, side);
                    for (n, sat) in play.ate.drain(..) {
                        self.splayer.food.add_stats(n, sat);
                    }
                }
                self.play = play;
            }
            Command::Dig(p) => {
                ACTIONS_DONE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let mut play = std::mem::replace(&mut self.play, Play::new(GameMode::Survival));
                let id = self.world.block(p[0], p[1], p[2]);
                std::mem::swap(&mut play.wear_rand, &mut self.splayer.e.rand);
                play.dig(&mut self.world, p);
                std::mem::swap(&mut play.wear_rand, &mut self.splayer.e.rand);
                for s in play.broke.drain(..) {
                    self.splayer.fx.push(crate::entity::base::Fx::BrokenItem(s));
                }
                // Block.harvestBlock: addStat(mineBlockStatArray[id]),
                // player.addExhaustion(0.025)
                if play.mode == GameMode::Survival
                    && id != crate::block::AIR
                    && self.world.block(p[0], p[1], p[2]) != id
                {
                    self.stats_file.increase_stat(&format!("stat.mineBlock.{id}"), 1);
                    self.splayer.food.add_exhaustion(0.025);
                }
                self.play = play;
            }
            Command::Use {
                target,
                face,
                sneaking,
                hit,
            } => {
                ACTIONS_DONE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                // ItemStack.tryPlaceItemIntoWorld: addStat(objectUseStats) when
                // the held stack was used (seen as it shrinking)
                let before = self.play.inv.held();
                let sel = self.play.inv.selected;
                self.apply(&Command::UseOnInner {
                    target,
                    face,
                    sneaking,
                    hit,
                });
                let after = self.play.inv.main[sel];
                if let Some(b) = before {
                    let used = match after {
                        None => true,
                        Some(a) => a.id != b.id || a.count < b.count,
                    };
                    if used {
                        self.stats_file.increase_stat(&format!("stat.useItem.{}", b.id), 1);
                    }
                }
            }
            Command::UseOnInner {
                target,
                face,
                sneaking,
                hit,
            } => {
                // ItemMinecart.onItemUse (rails have no onBlockActivated)
                if self.place_cart(target) {
                    return;
                }
                // BlockBed.onBlockActivated needs the player and the mobs
                if self.world.block(target[0], target[1], target[2]) == crate::block::BED
                    && (!sneaking || self.play.inv.held().is_none())
                {
                    self.use_bed(target);
                    return;
                }
                let mut play = std::mem::replace(&mut self.play, Play::new(GameMode::Survival));
                std::mem::swap(&mut play.wear_rand, &mut self.splayer.e.rand);
                play.can_eat = self.splayer.can_eat(false);
                let used = play.use_block(&mut self.world, target, face, sneaking, hit);
                std::mem::swap(&mut play.wear_rand, &mut self.splayer.e.rand);
                for (n, sat) in play.ate.drain(..) {
                    self.splayer.food.add_stats(n, sat);
                }
                for s in play.broke.drain(..) {
                    self.splayer.fx.push(crate::entity::base::Fx::BrokenItem(s));
                }
                self.play = play;
                // ItemMonsterPlacer.onItemUse always returns true: no sendUseItem
                let egg = self.play.inv.held().filter(|s| s.id == items::SPAWN_EGG);
                let hang = self.play.inv.held().filter(|s| {
                    s.id == crate::entity::hanging::PAINTING
                        || s.id == crate::entity::hanging::ITEM_FRAME
                });
                let rocket = self
                    .play
                    .inv
                    .held()
                    .filter(|s| s.id == crate::entity::rocket::FIREWORKS);
                if let (false, Some(s)) = (used, rocket) {
                    // ItemFirework.onItemUse: a rocket at the clicked point
                    let at = [
                        target[0] as f64 + hit[0] as f64,
                        target[1] as f64 + hit[1] as f64,
                        target[2] as f64 + hit[2] as f64,
                    ];
                    let id = self.alloc_vehicle_id();
                    let seed = self.mobs.rand.next_long();
                    let r = crate::entity::rocket::Rocket::new(id, at, Some(s), seed);
                    self.play
                        .vehicles
                        .push(crate::entity::vehicle::Tracked::new(
                            crate::entity::vehicle::Vehicle::Rocket(r),
                        ));
                    if self.play.mode != GameMode::Creative {
                        self.play.inv.consume_held();
                        self.play.inv_dirty = true;
                    }
                } else if !used
                    && self.play.inv.held().is_some_and(|s| s.id == crate::entity::hanging::LEAD)
                    && crate::block::info(self.world.peek_block(target[0], target[1], target[2])).shape
                        == crate::block::Shape::Fence
                {
                    // ItemLead.onItemUse: true on a fence, mobs or not
                    self.attach_lead(target);
                } else if let (false, Some(h)) = (used, hang) {
                    // ItemHangingEntity.onItemUse: true on the four sides
                    let side = match face {
                        [0, -1, 0] => 0,
                        [0, 1, 0] => 1,
                        [0, 0, -1] => 2,
                        [0, 0, 1] => 3,
                        [-1, 0, 0] => 4,
                        _ => 5,
                    };
                    if self.place_hanging(h.id, target, side)
                        && self.play.mode != GameMode::Creative
                    {
                        self.play.inv.consume_held();
                        self.play.inv_dirty = true;
                    }
                    if side < 2 {
                        self.use_air();
                    }
                } else if !used && egg.is_some() {
                    self.use_egg_on(egg.unwrap(), target, face);
                } else if !used {
                    // Minecraft.func_147121_ag: sendUseItem
                    self.use_air();
                }
            }
            Command::Select(i) => {
                if i < 9 && self.play.inv.selected != i {
                    self.play.inv.selected = i;
                    self.play.inv_dirty = true;
                }
            }
            Command::Chat(ref t) => {
                let t = t.clone();
                self.chat_message(&t);
            }
            Command::EnchantItem(k) => {
                let mut play = std::mem::replace(&mut self.play, Play::new(GameMode::Survival));
                let lvl = self.splayer.xp_level;
                if let Some(cost) = play.enchant_item(&mut self.world, k, lvl) {
                    // EntityPlayer.addExperienceLevel(-cost)
                    let p = &mut self.splayer;
                    p.xp_level -= cost;
                    if p.xp_level < 0 {
                        p.xp_level = 0;
                        p.xp = 0.0;
                        p.xp_total = 0;
                    }
                }
                self.play = play;
            }
            Command::UpdateSign(p, ref lines) => {
                let l = lines.clone();
                self.play.update_sign(p, l);
            }
            Command::CreativeSlot { slot, stack } => {
                let mut play = std::mem::replace(&mut self.play, Play::new(GameMode::Survival));
                play.creative_slot(&mut self.world, slot, stack);
                self.play = play;
            }
            Command::WindowClick { slot, button, mode } => {
                let mut play = std::mem::replace(&mut self.play, Play::new(GameMode::Survival));
                play.xp_level = self.splayer.xp_level;
                let spent = play.window_click(&mut self.world, slot, button, mode);
                self.play = play;
                self.merchant_effects();
                self.horse_slots_back();
                if spent != 0 {
                    // EntityPlayer.addExperienceLevel(-cost)
                    let p = &mut self.splayer;
                    p.xp_level -= spent;
                    if p.xp_level < 0 {
                        p.xp_level = 0;
                        p.xp = 0.0;
                        p.xp_total = 0;
                    }
                    self.play.xp_level = p.xp_level;
                }
            }
            Command::RenameItem(ref n) => {
                let n = n.clone();
                self.play.xp_level = self.splayer.xp_level;
                self.play.rename(&n);
            }
            Command::OpenInventoryAchievement => {
                // NetHandlerPlayServer.processClientStatus →
                // triggerAchievement(openInventory); a new achievement is
                // sent at once (hasUnsentAchievement → sendStats)
                if self
                    .stats_file
                    .trigger_achievement(crate::stats::OPEN_INVENTORY)
                {
                    let dirty = self.stats_file.take_dirty();
                    self.stats_out.extend(dirty);
                }
            }
            Command::OpenInventory => {
                self.play.open = Open::Inventory;
                self.play.inv.grid_size = 2;
                self.play.inv_dirty = true;
            }
            Command::CloseWindow => {
                // ContainerMerchant.onContainerClosed: setCustomer(null)
                if let crate::play::Open::Merchant(id) = self.play.open {
                    self.mobs.end_trade(id);
                }
                let mut play = std::mem::replace(&mut self.play, Play::new(GameMode::Survival));
                play.close(&mut self.world);
                self.play = play;
            }
            Command::BookEdit(ref pages, ref title) => {
                // ItemWritableBook.validBookPageTagContents: at most 50
                // pages of under 256 characters
                let valid = pages.len() <= 50 && pages.iter().all(|p| p.chars().count() < 256);
                if let (true, Some(h)) = (valid, self.play.inv.held()) {
                    if let Some(s) =
                        crate::book::apply(h, pages, title.as_deref(), crate::sim::commands::PLAYER_NAME)
                    {
                        let sel = self.play.inv.selected;
                        self.play.inv.main[sel] = Some(s);
                        self.play.inv_dirty = true;
                    }
                }
            }
            Command::RequestStats => {
                // func_150876_a: everything the first time, then what changed
                let v = if self.stats_sent_all {
                    self.stats_file.take_dirty()
                } else {
                    self.stats_sent_all = true;
                    let _ = self.stats_file.take_dirty();
                    self.stats_file.all()
                };
                self.stats_out.extend(v);
            }
            Command::BeaconSet(a, b) => {
                // NetHandlerPlayServer: with a payment, it is used up and the
                // powers are set
                if let crate::play::Open::Beacon(p) = self.play.open.clone() {
                    if let Some(crate::play::Tile::Beacon {
                        levels,
                        primary,
                        secondary,
                        payment,
                        ..
                    }) = self.play.tiles.get_mut(&p)
                    {
                        if let Some(s) = payment.take() {
                            if s.count > 1 {
                                *payment = Some(crate::item::Stack { count: s.count - 1, ..s });
                            }
                            *primary = crate::play::beacon_primary(*levels, a);
                            *secondary = crate::play::beacon_secondary(*levels, b);
                            self.play.inv_dirty = true;
                        }
                    }
                }
            }
            Command::MerchantSelect(i) => {
                self.play.select_offer(i);
                self.merchant_effects();
            }
            Command::HorseJump(p) => {
                if let Some(id) = self.riding {
                    if let Some(m) = self
                        .mobs
                        .list
                        .iter_mut()
                        .find(|m| m.id == id && m.kind == crate::entity::mob::Kind::Horse)
                    {
                        m.set_jump_power(p);
                    }
                }
            }
            Command::DropHeld { all } => {
                let mut play = std::mem::replace(&mut self.play, Play::new(GameMode::Survival));
                play.drop_held(&mut self.world, all);
                self.play = play;
            }
            Command::PickBlock(p) => self.play.pick_block(&self.world, p),
            Command::PickEntity(item, dmg, sub) => {
                if self.play.mode == crate::entity::survival::GameMode::Creative {
                    self.play.set_current_item(item, dmg, sub);
                }
            }
            Command::SetMode(m) => {
                self.play.mode = m;
                self.play.inv_dirty = true;
            }
            Command::Stats(_) => {}
            Command::Died => {}
            Command::LeaveBed => {
                // C0BPacketEntityAction 3: wakeUpPlayer(false, true, true)
                self.wake_up(false, true);
            }
            Command::Respawn => {
                self.wake_up(true, false);
                self.splayer.respawn(crate::gamerules::bool("keepInventory"));
                self.respawn_pending = true;
                if self.dim != 0 {
                    self.pending_travel = Some(0);
                }
            }
            Command::Shutdown => {}
            _ => self.apply_more(cmd),
        }
    }

    /// `EntityPlayer.onDeath`: the inventory spills out
    /// (`InventoryPlayer.dropAllItems`: the main slots, then the armour),
    /// unless keepInventory.
    fn drop_inventory(&mut self) {
        if crate::gamerules::bool("keepInventory") {
            return;
        }
        {
            {
                let mut play = std::mem::replace(&mut self.play, Play::new(GameMode::Survival));
                if play.mode == GameMode::Survival {
                    let mut rnd =
                        std::mem::replace(&mut self.world.rand, crate::java::JavaRandom::new(0));
                    play.close(&mut self.world);
                    for i in 0..crate::inventory::MAIN {
                        if let Some(st) = play.inv.main[i].take() {
                            play.throw_with(st, true, &mut rnd);
                        }
                    }
                    for i in 0..4 {
                        if let Some(st) = play.inv.armor[i].take() {
                            play.throw_with(st, true, &mut rnd);
                        }
                    }
                    self.world.rand = rnd;
                    play.inv_dirty = true;
                }
                self.play = play;
            }
        }
    }

    fn apply_more(&mut self, cmd: &Command) {
        match *cmd {
            Command::Attack { id, sprint, .. } if id & crate::entity::vehicle::VEHICLE_ID != 0 => {
                // attackTargetEntityWithCurrentItem against a non-living
                // entity: no enchantment part, no critical hit
                self.splayer.food.add_exhaustion(0.3);
                let damage = self.player_attack_damage();
                self.attack_vehicle(id, damage, sprint as i32);
            }
            Command::Interact { id } if id & crate::entity::vehicle::VEHICLE_ID != 0 => {
                self.interact_vehicle(id);
            }
            Command::Input {
                strafe,
                forward,
                jump,
                sneak,
            } => self.ride_input(strafe, forward, jump, sneak),
            Command::Attack { id, crit, sprint } => self.attack_mob(id, crit, sprint),
            Command::PlayerState { attackable } => self.player_attackable = attackable,
            Command::Interact { id } if self.mobs.villager_interact(id, self.play.inv.held()) => {
                // EntityVillager.interact: setCustomer, displayGUIMerchant
                let name = self
                    .mobs
                    .list
                    .iter()
                    .find(|m| m.id == id)
                    .map(|m| m.custom_name.clone())
                    .unwrap_or_default();
                self.play.open = crate::play::Open::Merchant(id);
                self.play.offers = self.mobs.offers_of(id);
                self.play.merchant_name = name;
                self.play.merchant_slots = [None; 3];
                self.play.merchant_index = 0;
                self.play.inv_dirty = true;
            }
            Command::Interact { id } if self.leash_interact(id) => {}
            Command::Interact { id } => {
                let mut out = crate::entity::mob::Outcome::default();
                let held = self.play.inv.held();
                // EntityHorse.interact
                use crate::entity::mob::horse::HorseAct;
                let sneaking = self.motion_flags.1;
                let act = self
                    .mobs
                    .list
                    .iter_mut()
                    .find(|m| m.id == id && m.kind == crate::entity::mob::Kind::Horse && m.health > 0.0)
                    .map(|m| m.horse_interact(held, sneaking, &mut out));
                if let Some(act) = act.filter(|a| *a != HorseAct::Pass) {
                    match act {
                        HorseAct::Gui => self.open_horse(id),
                        HorseAct::Mount => self.mount_mob(id),
                        HorseAct::Used => {
                            if self.play.mode != GameMode::Creative {
                                self.play.inv.consume_held();
                                self.play.inv_dirty = true;
                            }
                        }
                        _ => {}
                    }
                    self.pending_out.push(out);
                    return;
                }
                let (feet, _) = self.play.player;
                if let Some(mut next) = self.mobs.interact(id, held, feet, &mut out) {
                    let i = self.play.inv.selected;
                    // shears on a sheep, flint and steel on a creeper:
                    // ItemStack.damageItem(1, player)
                    if out.wear > 0 {
                        if let Some(s) = held {
                            let creative = self.play.mode == GameMode::Creative;
                            let (left, broke) = crate::enchant::damage_item(
                                s,
                                out.wear,
                                creative,
                                &mut self.splayer.e.rand,
                            );
                            if broke {
                                self.splayer.fx.push(crate::entity::base::Fx::BrokenItem(s));
                            }
                            next = left;
                        }
                    }
                    if self.play.mode == GameMode::Survival || held.map_or(false, |s| s.id == 325) {
                        self.play.inv.main[i] = next;
                    }
                    self.play.inv_dirty = true;
                }
                // EntityPig.interact -> EntityPlayer.mountEntity
                if let Some(mid) = out.mount.take() {
                    self.mount_mob(mid);
                }
                self.pending_out.push(out);
            }
            Command::Give(s) => {
                if s.count > 0 && s.count <= 64 {
                    let _ = self.play.inv.add(s);
                    self.play.inv_dirty = true;
                }
            }
            Command::Shutdown => {}
            _ => {}
        }
    }

    /// `ItemMap.onUpdate` for every map in the inventory (the held one
    /// also draws), and `S34PacketMaps` for what changed and for the maps
    /// in item frames.
    fn tick_maps(&mut self, events: &mut Vec<Event>) {
        let (feet, yaw) = self.play.player;
        let spawn = [self.spawn[0].floor() as i32, self.spawn[2].floor() as i32];
        let time = self.world_time;
        let mut shown = Vec::new();
        for i in 0..self.play.inv.main.len() {
            let Some(mut s) = self.play.inv.main[i] else { continue };
            if s.id != 358 {
                continue;
            }
            // ItemMap.onCreated: an extended copy becomes a new map one
            // scale up around the same centre
            if s.nbt().is_some_and(|t| t.int("map_is_scaling").unwrap_or(0) != 0) {
                let old = self.maps.get_or_make(s.meta, spawn, self.dim).1.clone();
                let nid = self.maps.unique_id();
                let mut m = old;
                m.scale = (m.scale + 1).min(4);
                m.colours = vec![0; 16384];
                m.marks.clear();
                m.step = 0;
                m.sent = false;
                m.saved = false;
                self.maps.list.insert(nid, m);
                s = crate::item::Stack { meta: nid, ..s }.edit_tag(|t| {
                    if let crate::world::anvil::Nbt::Compound(v) = t {
                        v.retain(|(k, _)| k != "map_is_scaling");
                    }
                });
                if s.nbt().is_some_and(|t| matches!(&*t, crate::world::anvil::Nbt::Compound(v) if v.is_empty())) {
                    s.tag = crate::item_tag::Tag::NONE;
                }
                self.play.inv.main[i] = Some(s);
                self.play.inv_dirty = true;
            }
            let (id, m) = self.maps.get_or_make(s.meta, spawn, self.dim);
            if id != s.meta {
                self.play.inv.main[i] = Some(crate::item::Stack { meta: id, ..s });
                self.play.inv_dirty = true;
            }
            if m.dim as i32 == self.dim {
                m.update_player_mark(feet[0], feet[2], yaw, time);
            } else if !m.marks.is_empty() {
                m.marks.clear();
                m.sent = false;
            }
            if i == self.play.inv.selected {
                m.update(&self.world, feet[0], feet[2]);
            }
            shown.push(id);
        }
        // a map no longer carried loses the player's marker
        for (id, m) in self.maps.list.iter_mut() {
            if !shown.contains(id) && !m.marks.is_empty() {
                m.marks.clear();
                m.sent = false;
            }
        }
        for v in &self.play.vehicles {
            if let crate::entity::vehicle::Vehicle::Hang(h) = &v.v {
                if let Some(s) = h.item.filter(|s| s.id == 358) {
                    shown.push(s.meta);
                }
            }
        }
        for id in shown {
            let known = self.maps_sent.contains(&id);
            let Some(m) = self.maps.get(id) else { continue };
            if !m.sent || !known {
                m.sent = true;
                events.push(Event::Map(crate::map::MapView {
                    id,
                    colours: m.colours.clone(),
                    marks: m.marks.clone(),
                }));
                self.maps_sent.insert(id);
            }
        }
    }

    /// `TileEntityBeacon.updateEntity` every 80 ticks: the pyramid under
    /// it (`updateSegmentColors`), then the powers for the player in range
    /// (`addEffectsToPlayers`); the client gets the beams.
    fn tick_beacons(&mut self, events: &mut Vec<Event>) {
        if self.world.total_time % 80 != 0 {
            return;
        }
        let feet = self.play.player.0;
        let keys: Vec<[i32; 3]> = self
            .play
            .tiles
            .iter()
            .filter(|(_, t)| matches!(t, crate::play::Tile::Beacon { .. }))
            .map(|(p, _)| *p)
            .collect();
        let mut beams = Vec::new();
        for p in keys {
            let [x, y, z] = p;
            if self.world.peek_block(x, y, z) != crate::block::BEACON {
                continue;
            }
            let sees = self.world.can_see_sky(x, y + 1, z);
            let mut lv = 0;
            if sees {
                for d in 1..=4 {
                    let yy = y - d;
                    if yy < 0 {
                        break;
                    }
                    let mut ok = true;
                    'row: for xx in x - d..=x + d {
                        for zz in z - d..=z + d {
                            let b = self.world.peek_block(xx, yy, zz);
                            if !matches!(
                                b,
                                crate::block::EMERALD_BLOCK
                                    | crate::block::GOLD_BLOCK
                                    | crate::block::DIAMOND_BLOCK
                                    | crate::block::IRON_BLOCK
                            ) {
                                ok = false;
                                break 'row;
                            }
                        }
                    }
                    if !ok {
                        break;
                    }
                    lv = d;
                }
            }
            let Some(crate::play::Tile::Beacon {
                levels,
                primary,
                secondary,
                complete,
                ..
            }) = self.play.tiles.get_mut(&p)
            else {
                continue;
            };
            let before = *levels;
            *levels = lv;
            *complete = sees && lv > 0;
            // the full pyramid's achievement for a player near it
            if lv == 4 && before < 4 {
                let near = (feet[0] - x as f64).abs() <= 10.0
                    && feet[1] >= (y - 4) as f64 - 5.0
                    && feet[1] <= y as f64 + 5.0
                    && (feet[2] - z as f64).abs() <= 10.0;
                if near && self.stats_file.trigger_achievement("achievement.fullBeacon") {
                    let dirty = self.stats_file.take_dirty();
                    self.stats_out.extend(dirty);
                }
            }
            if *complete {
                beams.push(p);
            }
            if *complete && lv > 0 && *primary > 0 {
                let r = (lv * 10 + 10) as f64;
                let inside = feet[0] >= x as f64 - r
                    && feet[0] <= x as f64 + 1.0 + r
                    && feet[1] >= y as f64 - r
                    && feet[2] >= z as f64 - r
                    && feet[2] <= z as f64 + 1.0 + r;
                if inside {
                    let amp = if lv >= 4 && *primary == *secondary { 1 } else { 0 };
                    let (a, b) = (*primary, *secondary);
                    self.splayer
                        .living
                        .add_effect(crate::potion::Effect::ambient(a, 180, amp, true));
                    if lv >= 4 && a != b && b > 0 {
                        self.splayer
                            .living
                            .add_effect(crate::potion::Effect::ambient(b, 180, 0, true));
                    }
                }
            }
        }
        if self.open_window_is_beacon() {
            self.play.inv_dirty = true;
        }
        if beams != self.beams_sent {
            self.beams_sent = beams.clone();
            events.push(Event::Beacons(beams));
        }
    }

    fn open_window_is_beacon(&self) -> bool {
        matches!(self.play.open, crate::play::Open::Beacon(_))
    }

    /// `EntityPlayerMP.triggerAchievement`: unlocked once its parent is,
    /// then sent with the other changed statistics.
    fn achieve(&mut self, id: &str) {
        if self.stats_file.trigger_achievement(id) {
            let dirty = self.stats_file.take_dirty();
            self.stats_out.extend(dirty);
        }
    }

    /// `EntityHorse.openGUI` (`displayGUIHorse`): tamed, and not ridden by
    /// someone else.
    fn open_horse(&mut self, id: u32) {
        let Some(m) = self.mobs.list.iter().find(|m| m.id == id) else {
            return;
        };
        if !m.horse_tame() {
            return;
        }
        self.play.open = crate::play::Open::Horse(id, m.horse_chested(), m.can_wear_armor());
        self.play.horse_slots = m.horse_chest.clone();
        // AnimalChest's name: getCommandSenderName ('#' marks a lang key)
        self.play.merchant_name = if m.custom_name.is_empty() {
            match m.horse_type() {
                1 => "#entity.donkey.name",
                2 => "#entity.mule.name",
                3 => "#entity.zombiehorse.name",
                4 => "#entity.skeletonhorse.name",
                _ => "#entity.horse.name",
            }
            .to_string()
        } else {
            m.custom_name.clone()
        };
        self.play.inv_dirty = true;
    }

    /// A click in the horse's window lands in its `AnimalChest`:
    /// `onInventoryChanged` (saddle and armour, with their sounds).
    fn horse_slots_back(&mut self) {
        let crate::play::Open::Horse(id, ..) = self.play.open else {
            return;
        };
        let Some(m) = self.mobs.list.iter_mut().find(|m| m.id == id) else {
            return;
        };
        if m.horse_chest == self.play.horse_slots {
            return;
        }
        let (a, s) = (m.horse_armor, m.horse_saddled());
        m.horse_chest = self.play.horse_slots.clone();
        let mut out = crate::entity::mob::Outcome::default();
        m.horse_inventory_changed(a, s, &mut out);
        self.pending_out.push(out);
    }

    /// The villager's side of a merchant window click: `useRecipe` for each
    /// trade, `verifySellingItem` for each new goods slot, then the window
    /// reads the offers again.
    fn merchant_effects(&mut self) {
        let (traded, verified) = std::mem::take(&mut self.play.merchant_fx);
        let crate::play::Open::Merchant(id) = self.play.open else {
            return;
        };
        if traded.is_empty() && verified.is_empty() {
            return;
        }
        let mut out = crate::entity::mob::Outcome::default();
        for has in verified {
            self.mobs.villager_verify(id, has, &mut out);
        }
        for k in traded {
            self.mobs.villager_traded(id, k, &mut out);
        }
        self.play.offers = self.mobs.offers_of(id);
        self.play.inv_dirty = true;
        self.pending_out.push(out);
    }

    /// Brings in missing chunks around the centre, nearest first.
    fn load_chunks(&mut self) {
        let mut wanted: Vec<(i32, i32)> = Vec::with_capacity(LOADS_PER_TICK);
        for &(dx, dz) in &self.offsets {
            let key = (self.centre.0 + dx, self.centre.1 + dz);
            if !self.world.chunk_exists(key.0, key.1) {
                wanted.push(key);
                if wanted.len() == LOADS_PER_TICK {
                    break;
                }
            }
        }
        if wanted.is_empty() {
            return;
        }
        if self.world.provider.dim == -1 {
            // ChunkProviderHell shares one generator between provideChunk and
            // populate, so the order of generation matters: one at a time.
            for key in wanted {
                self.world.load_chunk(key.0, key.1);
            }
            return;
        }
        // Generate the new ones in parallel; the terrain of a chunk depends
        // only on the seed and its coordinates.
        let provider = self.world.provider.clone();
        let fresh: Vec<(i32, i32)> = wanted
            .iter()
            .copied()
            .filter(|k| !self.world.is_stored(k.0, k.1))
            .collect();
        let generated: Vec<ChunkData> = std::thread::scope(|s| {
            let handles: Vec<_> = fresh
                .iter()
                .map(|&(cx, cz)| {
                    let p = provider.clone();
                    s.spawn(move || p.provide_chunk(cx, cz))
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().expect("chunk generation panicked"))
                .collect()
        });
        let mut generated = generated.into_iter();
        // Insert strictly in the order they were requested.
        for key in wanted {
            if self.world.chunk_exists(key.0, key.1) {
                // Pulled in by a neighbour's population in the meantime.
                if fresh.contains(&key) {
                    generated.next();
                }
                continue;
            }
            if fresh.contains(&key) {
                let data = generated.next().expect("generated chunk missing");
                self.world.insert_generated(data);
            } else {
                self.world.load_chunk(key.0, key.1);
            }
        }
    }

    fn unload_far(&mut self) {
        let r = self.view + 4;
        let mut far: Vec<(i32, i32)> = self
            .world
            .chunks
            .keys()
            .copied()
            .filter(|&(x, z)| (x - self.centre.0).abs() > r || (z - self.centre.1).abs() > r)
            .collect();
        far.sort();
        for key in far {
            match self.world.unload_chunk(key.0, key.1) {
                Ok(()) => {
                    self.unsaved.remove(&key);
                }
                Err(e) => {
                    self.unsaved.insert(key);
                    eprintln!("failed to save chunk {},{}: {e}", key.0, key.1);
                }
            }
        }
    }

    /// The world's game rules (installed or still waiting to be).
    pub fn rules(&self) -> crate::gamerules::GameRules {
        self.rules_pending
            .clone()
            .unwrap_or_else(crate::gamerules::current)
    }

    /// Advances the world by one tick and reports what the client must know.
    pub fn step(&mut self, commands: &[Command], events: &mut Vec<Event>) {
        if let Some(g) = self.rules_pending.take() {
            crate::gamerules::install(g);
        }
        // --sleep "x y z": use that bed once, 40 ticks in (screenshots)
        if self.server_ticks == 40 {
            if let Ok(v) = std::env::var("MINERUST_SLEEP") {
                let p: Vec<i32> = v.split_whitespace().filter_map(|t| t.parse().ok()).collect();
                if p.len() == 3 {
                    self.use_bed([p[0], p[1], p[2]]);
                }
            }
        }
        // MinecraftServer.updateTimeLightAndEntities: the time, every 20
        // ticks and at login, before the worlds tick
        if self.server_ticks % 20 == 0 {
            events.push(Event::TimeUpdate(
                self.world.total_time,
                self.world_time,
                crate::gamerules::bool("doDaylightCycle"),
            ));
        }
        self.server_ticks += 1;
        let sp = self.spawn.map(|v| v.floor() as i32);
        if self.sent_spawn != Some(sp) {
            self.sent_spawn = Some(sp);
            events.push(Event::SpawnPosition(sp));
        }
        self.play.difficulty_peaceful = self.difficulty == crate::entity::living::Difficulty::Peaceful;
        for c in commands {
            self.apply(c);
        }
        self.armor_in();
        // NetHandlerPlayServer.onNetworkTick
        if self.play.creative_drops > 0 {
            self.play.creative_drops -= 1;
        }
        self.splayer.creative = self.play.mode == GameMode::Creative;
        self.world.dim = self.dim;
        self.world.difficulty = self.difficulty as i32;
        // PlayerManager: the chunks in view count towards inhabitedTime
        self.world.update_inhabited(self.centre, self.view);
        if !self.respawn_pending {
            let d = self.difficulty;
            let regen = crate::gamerules::bool("naturalRegeneration");
            self.splayer.tick(&self.world, d, regen);
            // EntityLivingBase.onDeathUpdate at deathTime 20: a player always
            // drops its experience (isPlayer), unless doMobLoot is off;
            // EntityPlayer.getExperiencePoints is 0 under keepInventory
            if self.splayer.dead
                && self.splayer.living.death_time == 20
                && crate::gamerules::bool("doMobLoot")
            {
                let mut xp = if crate::gamerules::bool("keepInventory") {
                    0
                } else {
                    (self.splayer.xp_level * 7).min(100)
                };
                let at = self.play.player.0;
                while xp > 0 {
                    let k = crate::entity::xp_orb::xp_split(xp);
                    xp -= k;
                    self.play.orbs.push((at, k));
                }
            }
            self.tick_sleep();
        }
        let mut use_events = Vec::new();
        self.tick_item_use(&mut use_events);
        events.extend(use_events);
        self.load_chunks();
        if self.world.anvil {
            self.import_anvil();
        }
        self.summon_once();
        // WorldServer.tick
        if !self.freeze_time && crate::gamerules::bool("doDaylightCycle") {
            self.world_time += 1;
        }
        {
            let mut r = std::mem::replace(&mut self.world.rand, JavaRandom::new(0));
            self.weather.update(&mut r);
            self.world.rand = r;
            self.world.raining = self.dim == 0 && self.weather.rain > 0.2;
            // getWeightedThunderStrength(1) > 0.9
            self.world.thundering = self.dim == 0 && self.weather.thunder * self.weather.rain > 0.9;
        }
        self.world.skylight_subtracted = match self.dim {
            // WorldProviderHell.calculateCelestialAngle is always 0.5 (midnight),
            // WorldProviderEnd's always 0.0 (noon).
            -1 => 11,
            1 => 0,
            _ => skylight_subtracted_weather(
                self.world_time,
                self.weather.rain,
                self.weather.thunder,
            ),
        };
        let mut shown = self.weather;
        if self.dim != 0 {
            shown.rain = 0.0;
            shown.prev_rain = 0.0;
            shown.thunder = 0.0;
            shown.prev_thunder = 0.0;
        }
        events.push(Event::Weather(shown));
        self.tick_portal(events);
        self.world.tick();
        let radius = self.view.min(7);
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                self.world
                    .tick_chunk(self.centre.0 + dx, self.centre.1 + dz);
                self.world
                    .update_blocks(self.centre.0 + dx, self.centre.1 + dz);
            }
        }
        // WorldServer.func_147488_Z
        self.world.run_block_events();
        if self.world.total_time % 40 == 0 {
            self.unload_far();
        }
        let mut play = std::mem::replace(&mut self.play, Play::new(GameMode::Survival));
        play.tick(&mut self.world);
        // ContainerHorseInventory.canInteractWith: alive and within 8
        if let crate::play::Open::Horse(id, ..) = play.open {
            let feet = play.player.0;
            let ok = self.mobs.list.iter().any(|m| {
                let d = (m.pos[0] - feet[0]).powi(2)
                    + (m.pos[1] - feet[1]).powi(2)
                    + (m.pos[2] - feet[2]).powi(2);
                m.id == id && m.health > 0.0 && !m.dead && d < 64.0
            });
            if !ok {
                play.close(&mut self.world);
            }
        }
        // Container.canInteractWith for ContainerMerchant: the villager must
        // still have the player as its customer
        if let crate::play::Open::Merchant(id) = play.open {
            let ok = self
                .mobs
                .list
                .iter()
                .any(|m| m.id == id && m.customer && m.health > 0.0 && !m.dead);
            if !ok {
                self.mobs.end_trade(id);
                play.close(&mut self.world);
            }
        }
        self.tick_daylight_detectors();
        // EntityFallingBlock.fall: attackEntityFrom(anvil / fallingBlock) on
        // every entity in the block's box
        for (bb, amount, anvil) in std::mem::take(&mut play.falling_hits) {
            use crate::entity::living::{Source, SourceKind};
            let kind = if anvil {
                SourceKind::Anvil
            } else {
                SourceKind::FallingBlock
            };
            if !self.respawn_pending && Self::player_box(play.player.0).intersects(&bb) {
                let d = self.difficulty;
                self.splayer
                    .attack(&self.world, Source::of(kind), amount, d);
            }
            for m in self.mobs.list.iter_mut() {
                if m.bb.intersects(&bb) {
                    m.hurt_by_falling(amount);
                }
            }
        }
        // Entity contact with pressure plates and tripwire.
        {
            use crate::world::redstone::{Aabb6, Occupant};
            let mut occ = Vec::new();
            let (f, _) = play.player;
            if !self.respawn_pending {
                occ.push(Occupant {
                    bb: Aabb6 {
                        min: [f[0] - 0.3, f[1], f[2] - 0.3],
                        max: [f[0] + 0.3, f[1] + 1.8, f[2] + 0.3],
                    },
                    living: true,
                    minecart: false,
                    arrow: false,
                    collides: true,
                    inventory: None,
                });
            }
            for m in &self.mobs.list {
                if !m.dead {
                    occ.push(Occupant {
                        bb: Aabb6 {
                            min: m.bb.min,
                            max: m.bb.max,
                        },
                        living: true,
                        minecart: false,
                        arrow: false,
                        collides: true,
                        inventory: None,
                    });
                }
            }
            for it in &play.items {
                let b = it.bb();
                occ.push(Occupant {
                    bb: Aabb6 {
                        min: b.min,
                        max: b.max,
                    },
                    living: false,
                    minecart: false,
                    arrow: false,
                    collides: true,
                    inventory: None,
                });
            }
            for a in &self.mobs.arrows {
                occ.push(Occupant {
                    bb: Aabb6 {
                        min: a.e.bb.min,
                        max: a.e.bb.max,
                    },
                    living: false,
                    minecart: false,
                    arrow: true,
                    collides: true,
                    inventory: None,
                });
            }
            for t in &self.mobs.tnt {
                occ.push(Occupant {
                    bb: Aabb6 {
                        min: [t.pos[0] - 0.49, t.pos[1] - 0.49, t.pos[2] - 0.49],
                        max: [t.pos[0] + 0.49, t.pos[1] + 0.49, t.pos[2] + 0.49],
                    },
                    living: false,
                    minecart: false,
                    arrow: false,
                    collides: true,
                    inventory: None,
                });
            }
            for b in &play.vehicles {
                let e = b.v.e();
                occ.push(Occupant {
                    bb: Aabb6 {
                        min: e.bb.min,
                        max: e.bb.max,
                    },
                    living: false,
                    minecart: matches!(b.v, crate::entity::vehicle::Vehicle::Cart(_)),
                    arrow: false,
                    collides: true,
                    inventory: match &b.v {
                        crate::entity::vehicle::Vehicle::Cart(c)
                            if matches!(
                                c.kind,
                                crate::entity::minecart::CartKind::Chest
                                    | crate::entity::minecart::CartKind::Hopper
                            ) =>
                        {
                            Some(crate::world::redstone::calc_redstone_from_inventory(
                                &c.slots,
                            ))
                        }
                        _ => None,
                    },
                });
            }
            // EntityThrowable moves without doBlockCollisions
            for t in &self.mobs.throwables {
                occ.push(Occupant {
                    bb: Aabb6 {
                        min: t.e.bb.min,
                        max: t.e.bb.max,
                    },
                    living: false,
                    minecart: false,
                    arrow: false,
                    collides: false,
                    inventory: None,
                });
            }
            for f in &play.falling {
                let b = f.e.bb;
                occ.push(Occupant {
                    bb: Aabb6 {
                        min: b.min,
                        max: b.max,
                    },
                    living: false,
                    minecart: false,
                    arrow: false,
                    collides: true,
                    inventory: None,
                });
            }
            if let Some(h) = &self.fish_hook {
                occ.push(Occupant {
                    bb: Aabb6 {
                        min: h.e.bb.min,
                        max: h.e.bb.max,
                    },
                    living: false,
                    minecart: false,
                    arrow: false,
                    collides: true,
                    inventory: None,
                });
            }
            self.world.occupants = occ;
            self.world.redstone_entity_tick();
        }
        for t in std::mem::take(&mut self.world.gen_tiles) {
            play.add_gen_tile(t);
        }
        let mut rnd = std::mem::replace(&mut self.world.rand, JavaRandom::new(0));
        for e in std::mem::take(&mut self.world.gen_entities) {
            use crate::world::server::GenEntity;
            match e {
                GenEntity::Villager { pos, profession } => {
                    let id =
                        self.mobs
                            .spawn(crate::entity::mob::Kind::Villager, pos, 0.0, &mut rnd);
                    if let Some(m) = self.mobs.list.iter_mut().find(|m| m.id == id) {
                        m.variant = profession;
                        m.persistent = true;
                    }
                }
                GenEntity::Witch { pos } => {
                    let id = self
                        .mobs
                        .spawn(crate::entity::mob::Kind::Witch, pos, 0.0, &mut rnd);
                    if let Some(m) = self.mobs.list.iter_mut().find(|m| m.id == id) {
                        m.persistent = true;
                    }
                }
                GenEntity::Dragon { pos, yaw } => {
                    let id =
                        self.mobs
                            .spawn(crate::entity::mob::Kind::EnderDragon, pos, yaw, &mut rnd);
                    if let Some(m) = self.mobs.list.iter_mut().find(|m| m.id == id) {
                        m.persistent = true;
                    }
                }
                GenEntity::Crystal { pos, yaw } => {
                    let id =
                        self.mobs
                            .spawn(crate::entity::mob::Kind::EnderCrystal, pos, yaw, &mut rnd);
                    if let Some(m) = self.mobs.list.iter_mut().find(|m| m.id == id) {
                        m.persistent = true;
                    }
                }
                GenEntity::Mob { kind, pos, variant } => {
                    // ItemMonsterPlacer.spawnCreature: onSpawnWithEgg
                    let id = self
                        .mobs
                        .spawn_in(&mut self.world, kind, pos, 0.0, &mut rnd);
                    if let Some(m) = self.mobs.list.iter_mut().find(|m| m.id == id) {
                        m.variant = variant;
                        m.persistent = true;
                    }
                }
                GenEntity::Animal { kind, pos, yaw } => {
                    let id = self.mobs.spawn(kind, pos, yaw, &mut rnd);
                    if let Some(m) = self.mobs.list.iter_mut().find(|m| m.id == id) {
                        m.persistent = true;
                    }
                }
                other => play.add_gen_entity(other, &mut rnd),
            }
        }
        self.world.rand = rnd;
        self.tick_mobs(&mut play, events);
        self.tick_mob_rider(&mut play);
        self.tick_vehicles(&mut play, events);
        self.tick_fish_hook(&mut play);
        // World.updateEntities: tile entities after the entities.
        self.world.tick_moving();
        self.apply_pushes(&mut play, events);
        {
            use crate::entity::base::Fx;
            let mut sounds = Vec::new();
            let mut parts = Vec::new();
            for f in play.fx.drain(..) {
                match f {
                    Fx::Sound(n, p, v, pi) => sounds.push((n, p, v, pi)),
                    Fx::Particle(n, p, m) => parts.push((n, p, m)),
                    _ => {}
                }
            }
            parts.extend(self.world.particles.drain(..));
            if !sounds.is_empty() {
                events.push(Event::Sounds(sounds));
            }
            if !parts.is_empty() {
                events.push(Event::Particles(parts));
            }
            if !play.collected.is_empty() {
                events.push(Event::Collect(std::mem::take(&mut play.collected)));
            }
            if !play.mob_collected.is_empty() {
                events.push(Event::MobCollect(std::mem::take(&mut play.mob_collected)));
            }
        }
        let moving: Vec<([i32; 3], crate::world::redstone::MovingView)> = self
            .world
            .moving
            .iter()
            .map(|m| (m.pos, m.view()))
            .collect();
        if !moving.is_empty() || self.sent_moving {
            self.sent_moving = !moving.is_empty();
            events.push(Event::Moving(moving));
        }
        self.play = play;
        self.tick_maps(events);
        self.tick_lightning(events);
        // EntityPlayer.onUpdate: playOneMinute; addStat calls; onDeath
        self.stats_file.increase_stat("stat.playOneMinute", 1);
        for a in std::mem::take(&mut self.play.achievements) {
            self.achieve(a);
        }
        for (k, n) in std::mem::take(&mut self.play.stat_adds) {
            self.stats_file.increase_stat(&k, n);
        }
        // addMountedMovementStat: a thousand blocks from where the
        // minecart ride began
        let on_cart = self.riding.is_some_and(|id| {
            self.play
                .vehicles
                .iter()
                .any(|v| v.v.id() == id && matches!(v.v, crate::entity::vehicle::Vehicle::Cart(_)))
        });
        if on_cart {
            let f = self.play.player.0;
            let here = [f[0].floor() as i32, f[1].floor() as i32, f[2].floor() as i32];
            match self.rail_start {
                None => self.rail_start = Some(here),
                Some(s) => {
                    let d = [(s[0] - here[0]) as f64, (s[1] - here[1]) as f64, (s[2] - here[2]) as f64];
                    if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] >= 1_000_000.0 {
                        self.achieve("achievement.onARail");
                    }
                }
            }
        } else {
            self.rail_start = None;
        }
        let drops = std::mem::take(&mut self.play.drops_made);
        if drops > 0 {
            self.stats_file.increase_stat("stat.drop", drops);
        }
        for (k, n) in std::mem::take(&mut self.splayer.stat_adds) {
            self.stats_file.increase_stat(k, n);
        }
        if self.splayer.dead && !self.was_dead {
            self.stats_file.increase_stat("stat.deaths", 1);
            // EntityList.entityEggs' entityKilledByStat
            if let Some(id) = self.splayer.last_attacker_id() {
                if let Some(m) = self.mobs.list.iter().find(|m| m.id == id) {
                    let k = format!("stat.entityKilledBy.{}", m.kind.id());
                    self.stats_file.increase_stat(&k, 1);
                }
            }
        }
        self.was_dead = self.splayer.dead;
        self.tick_leashes();
        self.tick_beacons(events);
        self.pick_up_arrows(events);
        self.player_events(events);
        if self.play.inv_dirty {
            self.play.inv_dirty = false;
            events.push(Event::Inventory(Box::new(self.play.view())));
        }
        events.push(Event::Items(self.play.item_views()));
        events.push(Event::Throwables(
            self.mobs
                .throwables
                .iter()
                .map(|t| {
                    let (id, m) = t.kind.item();
                    (t.e.pos, t.e.prev, id, m)
                })
                .collect(),
        ));
        events.push(Event::FishHook(
            self.fish_hook.as_ref().map(|h| (h.e.pos, h.e.prev)),
        ));
        events.push(Event::Falling(
            self.play.falling.iter().map(|f| f.view()).collect(),
        ));
        if self.respawn_pending {
            self.respawn_pending = false;
            let at = self.respawn_point();
            self.play.player.0 = at;
            self.centre = ((at[0].floor() as i32) >> 4, (at[2].floor() as i32) >> 4);
            events.push(Event::Respawn(at));
        }

        // relight.rs: once a chunk can no longer be decorated into and its
        // neighbours are there, its light is recomputed (vanilla leaves the
        // dark patches its generation-time updates could not spread)
        if crate::compat::fixed() {
            self.world.run_due_relights();
            for i in 0..self.offsets.len() {
                let (dx, dz) = self.offsets[i];
                let key = (self.centre.0 + dx, self.centre.1 + dz);
                if self.ready(key.0, key.1) && self.world.wants_light_fix(key.0, key.1) {
                    self.world.fix_light(key.0, key.1);
                }
            }
        }

        // Changes, in a fixed order.
        let mut dirty: Vec<((i32, i32), u16)> = self.world.dirty.drain().collect();
        dirty.sort();
        for (key, _) in &dirty {
            self.unsaved.insert(*key);
        }
        let range = self.view + 2;
        let in_range =
            |c: (i32, i32), k: (i32, i32)| (k.0 - c.0).abs() <= range && (k.1 - c.1).abs() <= range;
        let gone: Vec<(i32, i32)> = self
            .sent
            .keys()
            .copied()
            .filter(|&k| !in_range(self.centre, k) || !self.world.chunk_exists(k.0, k.1))
            .collect();
        for key in gone {
            self.sent.remove(&key);
            events.push(Event::Unload(key));
        }
        for (key, changed) in dirty {
            if in_range(self.centre, key) && self.world.chunk_exists(key.0, key.1) {
                let ready = self.ready(key.0, key.1);
                // a chunk the client has not seen yet is new as a whole
                let changed = if self.sent.insert(key, ready).is_some() {
                    changed
                } else {
                    0xFFFF
                };
                events.push(Event::Chunk {
                    key,
                    chunk: self.snapshot(key).unwrap(),
                    ready,
                    changed,
                });
                for (p, t) in self.play.signs_in(key.0, key.1) {
                    events.push(Event::SignText(p, t));
                }
                for (p, k, r) in self.play.skulls_in(key.0, key.1) {
                    events.push(Event::Skull(p, k, r));
                }
            }
        }
        // Chunks that entered the range or became ready without changing.
        for &(dx, dz) in &self.offsets {
            let key = (self.centre.0 + dx, self.centre.1 + dz);
            if !self.world.chunk_exists(key.0, key.1) {
                continue;
            }
            let ready = self.ready(key.0, key.1);
            if self.sent.get(&key) != Some(&ready) {
                let known = self.sent.insert(key, ready).is_some();
                events.push(Event::Chunk {
                    key,
                    chunk: self.snapshot(key).unwrap(),
                    ready,
                    changed: if known { 0 } else { 0xFFFF },
                });
                for (p, t) in self.play.signs_in(key.0, key.1) {
                    events.push(Event::SignText(p, t));
                }
                for (p, k, r) in self.play.skulls_in(key.0, key.1) {
                    events.push(Event::Skull(p, k, r));
                }
            }
        }
        if self.world.save_dir.is_some() && self.world.total_time % AUTOSAVE_TICKS == 0 {
            if let Err(e) = self.save() {
                eprintln!("autosave failed: {e}");
            }
        }
    }

    /// Reads `level.dat` (`WorldInfo`, the player of a single-player world).
    fn start_from_level_dat(&mut self, root: &crate::world::anvil::Nbt, dim: &mut i32) -> Start {
        use crate::world::anvil::Nbt;
        let d = root
            .get("Data")
            .cloned()
            .unwrap_or(Nbt::Compound(Vec::new()));
        self.world.total_time = d.int("Time").unwrap_or(0);
        self.world_time = d.int("DayTime").unwrap_or(self.world.total_time);
        self.rules_pending = Some(crate::gamerules::GameRules::from_nbt(d.get("GameRules")));
        self.weather.raining = d.int("raining").unwrap_or(0) != 0;
        self.weather.rain_time = d.int("rainTime").unwrap_or(0) as i32;
        self.weather.thundering = d.int("thundering").unwrap_or(0) != 0;
        self.weather.thunder_time = d.int("thunderTime").unwrap_or(0) as i32;
        self.weather.rain = if self.weather.raining { 1.0 } else { 0.0 };
        self.weather.prev_rain = self.weather.rain;
        self.play.mode = if d.int("GameType").unwrap_or(0) == 1 {
            GameMode::Creative
        } else {
            GameMode::Survival
        };
        let sx = d.int("SpawnX").unwrap_or(0) as f64 + 0.5;
        let sy = d.int("SpawnY").unwrap_or(64) as f64;
        let sz = d.int("SpawnZ").unwrap_or(0) as f64 + 0.5;
        self.spawn = [sx, sy, sz];
        // WorldServer.initialize: a new world (WorldInfo.isInitialized false)
        // gets createSpawnPosition
        let initialized = match d.get("initialized") {
            Some(_) => d.int("initialized").unwrap_or(1) != 0,
            None => d.get("SpawnX").is_some(),
        };
        if !initialized {
            self.spawn = self.find_spawn();
        }
        let mut pos = self.spawn;
        let mut rot = [0.0f32; 2];
        let mut flying = false;
        if let Some(p) = d.get("Player") {
            let ps = p.list("Pos");
            if let (Some(Nbt::Double(x)), Some(Nbt::Double(y)), Some(Nbt::Double(z))) =
                (ps.first(), ps.get(1), ps.get(2))
            {
                // EntityPlayer.posY sits yOffset (1.62) above the feet
                pos = [*x, *y - 1.62, *z];
            }
            if let (Some(Nbt::Float(a)), Some(Nbt::Float(b))) =
                (p.list("Rotation").first(), p.list("Rotation").get(1))
            {
                rot = [*a, *b];
            }
            *dim = p.int("Dimension").unwrap_or(0) as i32;
            if let (Some(x), Some(y), Some(z)) = (p.int("SpawnX"), p.int("SpawnY"), p.int("SpawnZ"))
            {
                self.bed_spawn = Some([x as i32, y as i32, z as i32]);
            }
            // EntityPlayer.readEntityFromNBT: InventoryEnderChest.loadInventoryFromNBT
            for (k, st) in crate::world::anvil::slots(p.list("EnderItems")) {
                if (0..27).contains(&k) {
                    self.play.ender[k as usize] = Some(st);
                }
            }
            for (k, st) in crate::world::anvil::slots(p.list("Inventory")) {
                if (0..36).contains(&k) {
                    self.play.inv.main[k as usize] = Some(st);
                }
                if (100..104).contains(&k) {
                    self.play.inv.armor[k as usize - 100] = Some(st);
                }
            }
            if let Some(ab) = p.get("abilities") {
                flying = ab.int("flying").unwrap_or(0) != 0;
            }
        }
        self.player = Some((pos, rot, flying));
        Start {
            pos,
            rot,
            flying,
            world_time: self.world_time,
            seed: self.world.seed(),
            resumed: true,
            mode: self.play.mode,
            stats: None,
        }
    }

    /// Tile entities, entities and dropped items of one chunk, in NBT.
    fn chunk_lists(
        play: &Play,
        mobs: &crate::entity::mob::Mobs,
        cx: i32,
        cz: i32,
        dim: i32,
    ) -> (Vec<crate::world::anvil::Nbt>, Vec<crate::world::anvil::Nbt>) {
        let tiles = play.tiles_nbt(cx, cz);
        let mut entities: Vec<crate::world::anvil::Nbt> = mobs
            .list
            .iter()
            .filter(|m| {
                m.health > 0.0
                    && (m.pos[0].floor() as i32) >> 4 == cx
                    && (m.pos[2].floor() as i32) >> 4 == cz
            })
            .map(crate::entity::mob::Mobs::to_nbt)
            .collect();
        entities.extend(play.items_nbt(cx, cz, dim));
        entities.extend(play.falling_nbt(cx, cz, dim));
        entities.extend(play.vehicles_nbt(cx, cz, dim));
        (tiles, entities)
    }

    /// Saves one dimension in Anvil form: changed chunks and every chunk that
    /// holds (or held) tile entities or entities.
    fn save_anvil_world(
        world: &mut ServerWorld,
        play: &Play,
        mobs: &crate::entity::mob::Mobs,
        unsaved: &mut std::collections::BTreeSet<(i32, i32)>,
        touched: &std::collections::BTreeSet<(i32, i32)>,
    ) -> std::io::Result<()> {
        use crate::world::anvil;
        let Some(dir) = world.save_dir.clone() else {
            return Ok(());
        };
        let mut keys = unsaved.clone();
        keys.extend(touched.iter().copied());
        for p in play.tiles.keys() {
            keys.insert((p[0] >> 4, p[2] >> 4));
        }
        for m in &mobs.list {
            keys.insert((
                (m.pos[0].floor() as i32) >> 4,
                (m.pos[2].floor() as i32) >> 4,
            ));
        }
        for e in &play.items {
            keys.insert((
                (e.e.pos[0].floor() as i32) >> 4,
                (e.e.pos[2].floor() as i32) >> 4,
            ));
        }
        let dim = world.provider.dim;
        let began = std::time::Instant::now();
        // the chunks' NBT first (it reads the game state), then the
        // compression on every core, then each region file opened once
        let mut roots: Vec<(i32, i32, anvil::Nbt)> = Vec::new();
        for (cx, cz) in keys {
            let lists = Self::chunk_lists(play, mobs, cx, cz, dim);
            if world.chunks.contains_key(&(cx, cz)) {
                if let Some(root) = world.anvil_chunk_root(&dir, cx, cz, Some(lists)) {
                    roots.push((cx, cz, root));
                }
            } else if let Some(mut root) = anvil::read_region_chunk(&dir, cx, cz) {
                // an unloaded chunk: its lists as the game holds them now
                // (empty when everything that was in it has left)
                if let Some(level) = root.get_mut("Level") {
                    level.set("TileEntities", anvil::Nbt::List(10, lists.0));
                    level.set("Entities", anvil::Nbt::List(10, lists.1));
                }
                roots.push((cx, cz, root));
            }
        }
        let n = roots.len();
        let threads = std::thread::available_parallelism()
            .map_or(4, |v| v.get())
            .clamp(1, 16);
        let per = n.div_ceil(threads).max(1);
        let packed: Vec<std::io::Result<Vec<(i32, i32, Vec<u8>)>>> = std::thread::scope(|sc| {
            let hs: Vec<_> = roots
                .chunks(per)
                .map(|part| {
                    sc.spawn(move || {
                        part.iter()
                            .map(|(cx, cz, root)| {
                                anvil::compress_chunk(root).map(|d| (*cx, *cz, d))
                            })
                            .collect::<std::io::Result<Vec<_>>>()
                    })
                })
                .collect();
            hs.into_iter()
                .map(|h| h.join().expect("chunk compression panicked"))
                .collect()
        });
        let mut all = Vec::with_capacity(n);
        for part in packed {
            all.extend(part?);
        }
        anvil::write_region_batch(&dir, &all, (world.total_time / 20) as u32, true)?;
        unsaved.clear();
        if n > 64 {
            println!(
                "Saved {n} chunks of dimension {dim} in {:.2} s",
                began.elapsed().as_secs_f64()
            );
        }
        Ok(())
    }

    /// Writes an opened vanilla world back: region files and `level.dat`.
    fn save_anvil(&mut self) -> std::io::Result<()> {
        use crate::world::anvil::{compound, item_nbt, list, Nbt};
        let Some(base) = self.base_dir.clone() else {
            return Ok(());
        };
        let touched: std::collections::BTreeSet<(i32, i32)> = self
            .anvil_imported
            .iter()
            .filter(|k| k.0 == self.dim)
            .map(|k| (k.1, k.2))
            .collect();
        Self::save_anvil_world(
            &mut self.world,
            &self.play,
            &self.mobs,
            &mut self.unsaved,
            &touched,
        )?;
        for p in &mut self.parked {
            let touched: std::collections::BTreeSet<(i32, i32)> = self
                .anvil_imported
                .iter()
                .filter(|k| k.0 == p.dim)
                .map(|k| (k.1, k.2))
                .collect();
            let mut tmp = Play::new(GameMode::Survival);
            tmp.tiles = p.tiles.clone();
            tmp.items = p.items.clone();
            tmp.falling = p.falling.clone();
            tmp.vehicles = p.vehicles.clone();
            Self::save_anvil_world(&mut p.world, &tmp, &p.mobs, &mut p.unsaved, &touched)?;
        }
        let mut root = self
            .anvil_level
            .clone()
            .unwrap_or_else(|| compound(vec![("Data", compound(vec![]))]));
        if root.get("Data").is_none() {
            root.set("Data", compound(vec![]));
        }
        let (feet, rot, flying) = self.player.unwrap_or((self.spawn, [0.0, 0.0], false));
        let d = root.get_mut("Data").unwrap();
        d.set("RandomSeed", Nbt::Long(self.world.seed()));
        d.set("Time", Nbt::Long(self.world.total_time));
        d.set("DayTime", Nbt::Long(self.world_time));
        d.set("GameRules", self.rules().to_nbt());
        d.set("raining", Nbt::Byte(self.weather.raining as i8));
        d.set("rainTime", Nbt::Int(self.weather.rain_time));
        d.set("thundering", Nbt::Byte(self.weather.thundering as i8));
        d.set("thunderTime", Nbt::Int(self.weather.thunder_time));
        d.set(
            "GameType",
            Nbt::Int((self.play.mode == GameMode::Creative) as i32),
        );
        d.set("version", Nbt::Int(19133));
        d.set("SpawnX", Nbt::Int(self.spawn[0].floor() as i32));
        d.set("SpawnY", Nbt::Int(self.spawn[1].floor() as i32));
        d.set("SpawnZ", Nbt::Int(self.spawn[2].floor() as i32));
        if d.get("LevelName").is_none() {
            d.set("LevelName", Nbt::String("MINERUST".into()));
        }
        if d.get("generatorName").is_none() {
            d.set("generatorName", Nbt::String("default".into()));
            d.set("generatorVersion", Nbt::Int(1));
            d.set("MapFeatures", Nbt::Byte(1));
        }
        // InventoryPlayer.writeToNBT: main slots, then armour as 100..104
        let mut inv: Vec<Nbt> = self
            .play
            .inv
            .main
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().map(|s| item_nbt(Some(i as i8), s)))
            .collect();
        for (i, s) in self.play.inv.armor.iter().enumerate() {
            if let Some(s) = s {
                inv.push(item_nbt(Some(100 + i as i8), s));
            }
        }
        let mut player = d.get("Player").cloned().unwrap_or_else(|| compound(vec![]));
        player.set(
            "Pos",
            list(vec![
                Nbt::Double(feet[0]),
                Nbt::Double(feet[1] + 1.62),
                Nbt::Double(feet[2]),
            ]),
        );
        player.set(
            "Rotation",
            list(vec![Nbt::Float(rot[0]), Nbt::Float(rot[1])]),
        );
        player.set("Dimension", Nbt::Int(self.dim));
        // EntityPlayer.writeEntityToNBT: the bed spawn
        match self.bed_spawn {
            Some(b) => {
                player.set("SpawnX", Nbt::Int(b[0]));
                player.set("SpawnY", Nbt::Int(b[1]));
                player.set("SpawnZ", Nbt::Int(b[2]));
                player.set("SpawnForced", Nbt::Byte(0));
            }
            None => {
                if let Nbt::Compound(v) = &mut player {
                    v.retain(|(k, _)| {
                        !matches!(k.as_str(), "SpawnX" | "SpawnY" | "SpawnZ" | "SpawnForced")
                    });
                }
            }
        }
        player.set("Inventory", Nbt::List(10, inv));
        let ender: Vec<Nbt> = self
            .play
            .ender
            .iter()
            .enumerate()
            .filter_map(|(k, s)| s.map(|s| crate::world::anvil::item_nbt(Some(k as i8), &s)))
            .collect();
        player.set("EnderItems", Nbt::List(10, ender));
        player.set("SelectedItemSlot", Nbt::Int(self.play.inv.selected as i32));
        let mut ab = player
            .get("abilities")
            .cloned()
            .unwrap_or_else(|| compound(vec![]));
        ab.set("flying", Nbt::Byte(flying as i8));
        ab.set(
            "mayfly",
            Nbt::Byte((self.play.mode == GameMode::Creative) as i8),
        );
        ab.set(
            "instabuild",
            Nbt::Byte((self.play.mode == GameMode::Creative) as i8),
        );
        player.set("abilities", ab);
        d.set("Player", player);
        d.set("initialized", Nbt::Byte(1));
        d.set(
            "LastPlayed",
            Nbt::Long(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |t| t.as_millis() as i64),
            ),
        );
        self.anvil_level = Some(root.clone());
        crate::world::anvil::write_level_dat(&base, &root)
    }

    /// `--export-anvil`: writes this world (in this project's format) as a
    /// vanilla 1.7.10 world in `dst`: every stored chunk, tile entity and
    /// entity, and `level.dat`.
    pub fn export_anvil(&mut self, dst: &std::path::Path) -> std::io::Result<usize> {
        let src = self.world.save_dir.clone();
        let mut keys = Vec::new();
        if let Some(d) = &src {
            let chunks = d.join("chunks");
            if let Ok(regions) = std::fs::read_dir(&chunks) {
                for r in regions.flatten() {
                    if let Ok(files) = std::fs::read_dir(r.path()) {
                        for f in files.flatten() {
                            let name = f.file_name().to_string_lossy().to_string();
                            let parts: Vec<&str> =
                                name.trim_end_matches(".mrc").split('.').collect();
                            if parts.len() == 3 && parts[0] == "c" {
                                if let (Ok(x), Ok(z)) = (parts[1].parse(), parts[2].parse()) {
                                    keys.push((x, z));
                                }
                            }
                        }
                    }
                }
            }
        }
        keys.sort();
        for &(x, z) in &keys {
            self.world.load_chunk(x, z);
        }
        std::fs::create_dir_all(dst)?;
        self.world.save_dir = Some(dst.to_path_buf());
        self.world.anvil = true;
        self.base_dir = Some(dst.to_path_buf());
        self.unsaved.extend(keys.iter().copied());
        self.save_anvil()?;
        Ok(keys.len())
    }

    /// Takes over the tile entities and entities of chunks read from region files.
    fn import_anvil(&mut self) {
        let loaded = std::mem::take(&mut self.world.anvil_loaded);
        let mut rnd = std::mem::replace(&mut self.world.rand, JavaRandom::new(0));
        for (cx, cz, tiles, entities) in loaded {
            if !self.anvil_imported.insert((self.dim, cx, cz)) {
                continue;
            }
            for t in &tiles {
                self.play.tile_from_nbt(t);
            }
            for e in &entities {
                if e.string("id") == Some("Item") {
                    self.play.item_from_nbt(e, &mut rnd);
                } else if e.string("id") == Some("FallingSand") {
                    self.play.falling_from_nbt(e);
                } else if !self.play.vehicle_from_nbt(e) {
                    let _ = self.mobs.from_nbt(e);
                }
            }
        }
        self.world.rand = rnd;
    }

    /// `ItemEnderEye.onItemRightClick`: thrown towards the nearest stronghold.
    fn use_air(&mut self) {
        let Some(held) = self.play.inv.held() else {
            return;
        };
        // ItemBoat
        if held.id == 333 {
            self.place_boat();
            return;
        }
        // ItemBucket (milk is drunk below)
        if matches!(
            held.id,
            items::BUCKET | items::WATER_BUCKET | items::LAVA_BUCKET
        ) {
            self.use_bucket(held);
            return;
        }
        // ItemMonsterPlacer
        if held.id == items::SPAWN_EGG {
            self.use_egg_air(held);
            return;
        }
        // ItemCarrotOnAStick
        if held.id == 398 {
            self.carrot_on_a_stick(held);
            return;
        }
        // ItemFishingRod
        if held.id == fishing::FISHING_ROD {
            self.use_fishing_rod(held);
            return;
        }
        // ItemArmor.onItemRightClick: into the empty armour slot
        if let Some(t) = crate::container::armor_type(held.id) {
            let k = 3 - t;
            if self.play.inv.armor[k].is_none() {
                self.play.inv.armor[k] = Some(crate::item::Stack { count: 1, ..held });
                // ItemInWorldManager.tryUseItem keeps a creative player's stack
                if self.play.mode != GameMode::Creative {
                    let sel = self.play.inv.selected;
                    self.play.inv.main[sel] = None;
                }
                self.play.inv_dirty = true;
            }
            return;
        }
        // ItemEmptyMap.onItemRightClick: a new map around the player
        if held.id == 395 {
            let id = self.maps.unique_id();
            let (feet, _) = self.play.player;
            let mut m = crate::map::MapData::new(0, feet[0], feet[2], self.dim);
            m.saved = false;
            self.maps.list.insert(id, m);
            let map = crate::item::Stack::new(358, id, 1);
            let sel = self.play.inv.selected;
            if held.count <= 1 {
                self.play.inv.main[sel] = Some(map);
            } else {
                self.play.inv.main[sel] = Some(crate::item::Stack {
                    count: held.count - 1,
                    ..held
                });
                if let Some(map) = self.play.inv.add(map) {
                    let mut rnd = std::mem::replace(&mut self.world.rand, JavaRandom::new(0));
                    self.play.throw(map, &mut rnd);
                    self.world.rand = rnd;
                }
            }
            self.play.inv_dirty = true;
            return;
        }
        // ItemSnowball, ItemEgg, ItemEnderPearl, ItemPotion (splash)
        {
            use crate::entity::throwable::Kind as TK;
            let creative = self.play.mode == GameMode::Creative;
            let kind = match held.id {
                332 => Some(TK::Snowball),
                344 => Some(TK::Egg),
                // ItemEnderPearl: not in creative
                368 if !creative => Some(TK::EnderPearl),
                373 if crate::item_use::is_splash(held.meta) => Some(TK::Potion(held.meta)),
                // ItemExpBottle
                384 => Some(TK::ExpBottle),
                _ => None,
            };
            if let Some(k) = kind {
                if !creative {
                    self.play.inv.consume_held();
                    self.play.inv_dirty = true;
                }
                let (feet, yaw) = self.play.player;
                let p = 0.4 / (self.world.rand.next_float() * 0.4 + 0.8);
                let pitch_snd = if held.id == 373 { 0.5 } else { 0.5 };
                self.pending_out.push(crate::entity::mob::Outcome {
                    sounds: vec![("random.bow", [feet[0], feet[1], feet[2]], pitch_snd, p)],
                    ..Default::default()
                });
                let pitch = self.play.pitch;
                self.mobs.throw_from_player(k, feet, yaw, pitch);
                return;
            }
        }
        // ItemFood, ItemPotion, ItemBucketMilk, ItemBow, ItemSword.onItemRightClick
        if self.splayer.using.is_none() && !self.splayer.dead {
            use crate::item_use::{action, food, is_splash, max_duration, Action};
            let start = match action(held.id, held.meta) {
                Action::Eat => {
                    let f = food(held.id, held.meta).unwrap();
                    self.splayer.can_eat(f.always_edible)
                }
                Action::Drink => !(held.id == 373 && is_splash(held.meta)),
                Action::Bow => {
                    self.play.mode == GameMode::Creative
                        || self.play.inv.main.iter().flatten().any(|s| s.id == 262)
                }
                Action::Block => true,
                Action::None => false,
            };
            if start {
                let n = max_duration(held.id, held.meta);
                self.splayer.using = Some((self.play.inv.selected, held, n));
                self.using_changed = true;
                return;
            }
        }
        if held.id != 381 || self.dim != 0 {
            return;
        }
        let (feet, _) = self.play.player;
        let Some(target) = self
            .world
            .structures
            .as_ref()
            .and_then(|s| s.nearest_stronghold(feet[0] as i32, feet[1] as i32, feet[2] as i32))
        else {
            return;
        };
        let pos = [feet[0], feet[1] + 1.62 - 0.0, feet[2]];
        // EntityEnderEye.moveTowards
        let (dx, dz) = (target[0] as f64 - pos[0], target[2] as f64 - pos[2]);
        let d = (dx * dx + dz * dz).sqrt();
        let t = if d > 12.0 {
            [pos[0] + dx / d * 12.0, pos[1] + 8.0, pos[2] + dz / d * 12.0]
        } else {
            [target[0] as f64, target[1] as f64, target[2] as f64]
        };
        let drop = self.world.rand.next_int_n(5) > 0;
        self.eyes.push(EnderEye {
            pos,
            prev: pos,
            motion: [0.0; 3],
            target: t,
            timer: 0,
            drop,
        });
        let p = 0.4 / (self.world.rand.next_float() * 0.4 + 0.8);
        self.pending_out.push(crate::entity::mob::Outcome {
            sounds: vec![("random.bow", feet, 0.5, p)],
            ..Default::default()
        });
        if self.play.mode == GameMode::Survival {
            self.play.inv.consume_held();
            self.play.inv_dirty = true;
        }
    }

    /// `EntityPlayer.onUpdate`: the item in use counts down; at 0
    /// `onItemUseFinish` eats, drinks or empties it.
    fn tick_item_use(&mut self, events: &mut Vec<Event>) {
        let Some((slot, st, left)) = self.splayer.using else {
            return;
        };
        // getCurrentItem() == itemInUse
        if self.play.inv.selected != slot || self.play.inv.main[slot] != Some(st) {
            self.splayer.using = None;
            self.using_changed = true;
            return;
        }
        let left = left - 1;
        if left == 0 {
            self.splayer.using = None;
            self.using_changed = true;
            events.push(Event::UseFinished(st));
            self.use_finished(slot, st);
        } else {
            self.splayer.using = Some((slot, st, left));
        }
    }

    /// `onItemUseFinish` -> `ItemStack.onFoodEaten`
    fn use_finished(&mut self, slot: usize, st: crate::item::Stack) {
        use crate::item::Stack;
        use crate::potion::{self, Effect};
        let creative = self.play.mode == GameMode::Creative;
        let pos = self.splayer.e.pos;
        let mut result = Some(st);
        let dec = |s: Stack| {
            if s.count > 1 {
                Some(Stack {
                    count: s.count - 1,
                    ..s
                })
            } else {
                None
            }
        };
        if let Some(f) = crate::item_use::food(st.id, st.meta) {
            // ItemFood.onEaten
            result = dec(st);
            self.splayer.food.add_stats(f.heal, f.saturation);
            let p = self.world.rand.next_float() * 0.1 + 0.9;
            self.pending_out.push(crate::entity::mob::Outcome {
                sounds: vec![("random.burp", pos, 0.5, p)],
                ..Default::default()
            });
            let l = &mut self.splayer.living;
            if st.id == 322 {
                // ItemAppleGold.onFoodEaten
                l.add_effect(Effect::new(potion::ABSORPTION, 2400, 0));
                if st.meta > 0 {
                    l.add_effect(Effect::new(potion::REGENERATION, 600, 4));
                    l.add_effect(Effect::new(potion::RESISTANCE, 6000, 0));
                    l.add_effect(Effect::new(potion::FIRE_RESISTANCE, 6000, 0));
                } else if self.world.rand.next_float() < 1.0 {
                    l.add_effect(Effect::new(potion::REGENERATION, 5 * 20, 1));
                }
            } else {
                if st.id == 349 && st.meta == 3 {
                    // ItemFishFood: pufferfish
                    l.add_effect(Effect::new(potion::POISON, 1200, 3));
                    l.add_effect(Effect::new(potion::HUNGER, 300, 2));
                    l.add_effect(Effect::new(potion::CONFUSION, 300, 1));
                }
                if let Some((id, secs, amp, prob)) = f.effect {
                    if self.world.rand.next_float() < prob {
                        l.add_effect(Effect::new(id, secs * 20, amp));
                    }
                }
            }
            if st.id == 282 {
                // ItemSoup: a bowl comes back
                result = Some(Stack::new(281, 0, 1));
            }
        } else if st.id == 373 {
            // ItemPotion.onEaten
            let mut left = st;
            if !creative {
                left.count -= 1;
            }
            for e in potion::effects_of(st.meta, false) {
                if e.potion().instant {
                    let ad = e.amplifier;
                    if e.id == potion::HEAL {
                        self.splayer.living.heal((4 << ad).max(0) as f32);
                    } else if e.id == potion::HARM {
                        let d = self.difficulty;
                        let src = crate::entity::living::Source::named(
                            crate::entity::living::SourceKind::Magic,
                        );
                        self.splayer.attack(&self.world, src, (6 << ad) as f32, d);
                    }
                } else {
                    self.splayer.living.add_effect(e);
                }
            }
            result = if creative {
                Some(left)
            } else if left.count == 0 {
                Some(Stack::new(374, 0, 1))
            } else {
                if let Some(rest) = self.play.inv.add(Stack::new(374, 0, 1)) {
                    let mut r = std::mem::replace(&mut self.world.rand, JavaRandom::new(0));
                    self.play.throw(rest, &mut r);
                    self.world.rand = r;
                }
                Some(left)
            };
        } else if st.id == 335 {
            // ItemBucketMilk.onEaten
            let mut left = st;
            if !creative {
                left.count -= 1;
            }
            self.splayer.living.clear_effects();
            result = if left.count == 0 {
                Some(Stack::new(325, 0, 1))
            } else {
                Some(left)
            };
        }
        self.play.inv.main[slot] = result.filter(|s| s.count > 0);
        self.play.inv_dirty = true;
    }

    /// `ItemStack.onPlayerStoppedUsing` (the bow fires; see `fire_bow`).
    fn stopped_using(&mut self, st: crate::item::Stack, left: i32) {
        self.using_changed = true;
        if st.id == 261 {
            self.fire_bow(st, left);
        }
    }

    /// `ItemBow.onPlayerStoppedUsing`
    fn fire_bow(&mut self, st: crate::item::Stack, left: i32) {
        use crate::enchant;
        let creative = self.play.mode == GameMode::Creative;
        let infinite = creative || enchant::level(enchant::INFINITY, Some(&st)) > 0;
        let has_arrow = self.play.inv.main.iter().flatten().any(|s| s.id == 262);
        if !infinite && !has_arrow {
            return;
        }
        let j = 72000 - left;
        let mut f = j as f32 / 20.0;
        f = (f * f + f * 2.0) / 3.0;
        if (f as f64) < 0.1 {
            return;
        }
        if f > 1.0 {
            f = 1.0;
        }
        let (feet, yaw) = self.play.player;
        let pitch = self.play.pitch;
        let crit = f == 1.0;
        let power = enchant::level(enchant::POWER, Some(&st));
        let punch = enchant::level(enchant::PUNCH, Some(&st));
        let flame = enchant::level(enchant::FLAME, Some(&st)) > 0;
        // var1.damageItem(1, player) on the stack being used
        let sel = self.play.inv.selected;
        if let Some(b) = self.play.inv.main[sel].filter(|b| b.id == 261) {
            let (left, broke) = enchant::damage_item(b, 1, creative, &mut self.splayer.e.rand);
            if broke {
                self.splayer.fx.push(crate::entity::base::Fx::BrokenItem(b));
            }
            self.play.inv.main[sel] = left;
        }
        // playSoundAtEntity(player, "random.bow", 1, 1 / (itemRand.nextFloat() * 0.4 + 1.2) + f * 0.5)
        let p = 1.0 / crate::util::math::item_rand(|r| r.next_float() * 0.4 + 1.2) + f * 0.5;
        self.pending_out.push(crate::entity::mob::Outcome {
            sounds: vec![("random.bow", [feet[0], feet[1] + 1.62, feet[2]], 1.0, p)],
            ..Default::default()
        });
        let a = self
            .mobs
            .shoot_player_arrow(feet, yaw, pitch, f * 2.0, crit, infinite);
        if power > 0 {
            a.damage += power as f64 * 0.5 + 0.5;
        }
        if punch > 0 {
            a.knockback = punch;
        }
        if flame {
            a.e.set_fire(100);
        }
        if !infinite {
            // InventoryPlayer.consumeInventoryItem(Items.arrow)
            if let Some(i) = self
                .play
                .inv
                .main
                .iter()
                .position(|s| s.map_or(false, |s| s.id == 262))
            {
                let s = self.play.inv.main[i].unwrap();
                self.play.inv.main[i] = if s.count > 1 {
                    Some(crate::item::Stack {
                        count: s.count - 1,
                        ..s
                    })
                } else {
                    None
                };
            }
        }
        self.play.inv_dirty = true;
    }

    /// The directory of a dimension's chunks and data (`DIM-1`, `DIM1`).
    fn dim_dir(&self, dim: i32) -> Option<PathBuf> {
        self.base_dir.as_ref().map(|b| {
            if dim == 0 {
                b.clone()
            } else {
                b.join(format!("DIM{dim}"))
            }
        })
    }

    /// `Entity.onEntityUpdate` portal handling for the player.
    fn tick_portal(&mut self, events: &mut Vec<Event>) {
        if self.send_achievements {
            // markAllDirty, then sendAchievements
            self.send_achievements = false;
            events.push(Event::Statistics(self.stats_file.achievements()));
        }
        if self.announce_dim {
            self.announce_dim = false;
            let (pos, yaw) = self.play.player;
            for key in std::mem::take(&mut self.sent).into_keys() {
                events.push(Event::Unload(key));
            }
            self.await_ack = true;
            events.push(Event::Dimension {
                dim: self.dim,
                pos,
                yaw,
            });
            return;
        }
        if let Some(to) = self.pending_travel.take() {
            self.travel(to, events);
            return;
        }
        if self.await_ack {
            return;
        }
        let (feet, _) = self.play.player;
        let contact = crate::dimension::portal_contact(&mut self.world, feet);
        match contact {
            Some(block::END_PORTAL) => {
                // BlockEndPortal.onEntityCollidedWithBlock
                let to = if self.dim == 1 { 0 } else { 1 };
                self.travel(to, events);
            }
            Some(_) => {
                // setInPortal / the portal counter
                if self.portal_cooldown > 0 {
                    self.portal_cooldown = 10;
                } else {
                    let max = if self.play.mode == GameMode::Creative {
                        0
                    } else {
                        80
                    };
                    let n = self.portal_ticks;
                    self.portal_ticks += 1;
                    if n >= max && self.dim != 1 {
                        self.portal_ticks = max;
                        self.portal_cooldown = 10;
                        let to = if self.dim == -1 { 0 } else { -1 };
                        self.travel(to, events);
                    }
                }
            }
            None => {
                if self.portal_ticks > 0 {
                    self.portal_ticks = (self.portal_ticks - 4).max(0);
                }
            }
        }
        if self.portal_cooldown > 0 && contact.is_none() {
            self.portal_cooldown -= 1;
        }
        events.push(Event::Portal(self.portal_ticks));
    }

    /// Creates the world of a dimension (or fetches it from the parked ones).
    fn take_world(&mut self, dim: i32) -> Parked {
        if let Some(i) = self.parked.iter().position(|p| p.dim == dim) {
            return self.parked.remove(i);
        }
        let seed = self.world.seed();
        let provider = Arc::new(ChunkProvider::new_dim(seed, dim));
        let mut world = ServerWorld::new(provider);
        world.save_dir = self.dim_dir(dim);
        world.total_time = self.world.total_time;
        world.anvil = self.world.anvil;
        let mut mobs = crate::entity::mob::Mobs::default();
        let mut tiles = std::collections::BTreeMap::new();
        let mut drops = Play::new(GameMode::Survival);
        if let Some(d) = &world.save_dir {
            if let Ok(text) = std::fs::read_to_string(d.join("data").join("structures.txt")) {
                if let Some(st) = world.structures.as_mut() {
                    let _ = st.load(text.trim());
                }
            }
            if let Ok(text) = std::fs::read_to_string(d.join("data").join("entities.txt")) {
                for line in text.lines() {
                    let _ = mobs.decode(line);
                }
            }
            if let Ok(text) = std::fs::read_to_string(d.join("data").join("tiles.txt")) {
                let mut tmp = Play::new(GameMode::Survival);
                for line in text.lines() {
                    let _ = tmp.decode_tile(line);
                }
                tiles = tmp.tiles;
            }
            if let Ok(b) = std::fs::read(d.join("data").join("drops.dat")) {
                let mut r = JavaRandom::new(seed);
                drops.load_drops(&b, &mut r);
            }
        }
        Parked {
            dim,
            world,
            mobs,
            unsaved: Default::default(),
            tiles,
            items: drops.items,
            falling: drops.falling,
            vehicles: drops.vehicles,
            teleport_rand: JavaRandom::new(seed),
        }
    }

    /// `ServerConfigurationManager.transferPlayerToDimension`
    fn travel(&mut self, to: i32, events: &mut Vec<Event>) {
        if to == self.dim {
            return;
        }
        // EntityPlayerMP.travelToDimension: portal, theEnd, theEnd2
        match (self.dim, to) {
            (1, 0) => self.play.achievements.push("achievement.theEnd2"),
            (_, 1) => self.play.achievements.push("achievement.theEnd"),
            (_, -1) => self.play.achievements.push("achievement.portal"),
            _ => {}
        }
        let (feet, yaw) = self.play.player;
        let from = self.dim;
        // swap the worlds
        let next = self.take_world(to);
        let mut old = Parked {
            dim: from,
            world: std::mem::replace(&mut self.world, next.world),
            mobs: std::mem::replace(&mut self.mobs, next.mobs),
            unsaved: std::mem::replace(&mut self.unsaved, next.unsaved),
            tiles: std::mem::replace(&mut self.play.tiles, next.tiles),
            items: std::mem::replace(&mut self.play.items, next.items),
            falling: std::mem::replace(&mut self.play.falling, next.falling),
            vehicles: std::mem::replace(&mut self.play.vehicles, next.vehicles),
            teleport_rand: std::mem::replace(&mut self.teleport_rand, next.teleport_rand),
        };
        // Flush native-format chunks now. The Anvil save handles parked
        // dimensions together with their entity and tile-entity lists.
        if !old.world.anvil {
            if let Err(e) = Self::save_legacy_chunks(&old.world, &mut old.unsaved) {
                eprintln!("failed to save dimension {from}: {e}");
            }
        }
        self.parked.push(old);
        self.dim = to;
        // the hook stays behind in the old world
        self.fish_hook = None;
        self.world.total_time = self.parked.last().unwrap().world.total_time;
        self.last_attacked = None;
        self.last_attacker = None;
        // where the player arrives
        let mut pos = feet;
        let mut new_yaw = yaw;
        if to == 1 {
            // WorldProviderEnd.getEntrancePortalLocation
            pos = [100.0, 50.0, 0.0];
            self.load_around(pos);
            pos = crate::dimension::end_platform(&mut self.world, pos);
        } else if from == 1 {
            // out of the End: back to the spawn point
            pos = self.spawn;
            self.load_around(pos);
        } else {
            let k = if to == -1 { 1.0 / 8.0 } else { 8.0 };
            pos[0] = (pos[0] * k).clamp(-29999872.0, 29999872.0);
            pos[2] = (pos[2] * k).clamp(-29999872.0, 29999872.0);
            self.load_around(pos);
            match crate::dimension::find_portal(&mut self.world, pos, yaw) {
                Some((p, y)) => {
                    pos = p;
                    new_yaw = y;
                }
                None => {
                    let mut r = std::mem::replace(&mut self.teleport_rand, JavaRandom::new(0));
                    crate::dimension::make_portal(&mut self.world, &mut r, pos);
                    self.teleport_rand = r;
                    if let Some((p, y)) = crate::dimension::find_portal(&mut self.world, pos, yaw) {
                        pos = p;
                        new_yaw = y;
                    }
                }
            }
        }
        self.play.player = (pos, new_yaw);
        if let Some(p) = &mut self.player {
            p.0 = pos;
            p.1[0] = new_yaw;
        }
        self.centre = ((pos[0].floor() as i32) >> 4, (pos[2].floor() as i32) >> 4);
        for key in std::mem::take(&mut self.sent).into_keys() {
            events.push(Event::Unload(key));
        }
        self.await_ack = true;
        self.portal_cooldown = 10;
        events.push(Event::Dimension {
            dim: to,
            pos,
            yaw: new_yaw,
        });
        let sound = if to == 1 || from == 1 {
            ""
        } else {
            "portal.travel"
        };
        if !sound.is_empty() {
            events.push(Event::Sounds(vec![(sound, pos, 1.0, 1.0)]));
        }
    }

    /// Loads the 3x3 chunks around a position (portal search and building).
    fn load_around(&mut self, pos: [f64; 3]) {
        let (cx, cz) = ((pos[0].floor() as i32) >> 4, (pos[2].floor() as i32) >> 4);
        for dx in -2..=2 {
            for dz in -2..=2 {
                self.world.load_chunk(cx + dx, cz + dz);
            }
        }
    }

    /// `--summon Kind[:tamed|:sit|:baby|:big|:n],...`: a row of mobs four blocks in
    /// front of the player once the world is up (for checking models).
    fn summon_once(&mut self) {
        static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if self.world.total_time < 20 || DONE.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        {
            let (f, _) = self.play.player;
            let (cx, cz) = ((f[0].floor() as i32) >> 4, (f[2].floor() as i32) >> 4);
            for dx in -1..=1 {
                for dz in -1..=1 {
                    if !self.world.chunk_exists(cx + dx, cz + dz) {
                        return;
                    }
                }
            }
        }
        DONE.store(true, std::sync::atomic::Ordering::Relaxed);
        // CommandEffect: id, seconds (30), amplifier (0)
        if let Ok(list) = std::env::var("MINERUST_EFFECT") {
            for e in list.split(',') {
                let v: Vec<i32> = e.split(':').filter_map(|t| t.parse().ok()).collect();
                if let Some(&id) = v.first() {
                    if crate::potion::potion(id).is_some() {
                        let secs = v.get(1).copied().unwrap_or(30);
                        let amp = v.get(2).copied().unwrap_or(0);
                        self.splayer.living.add_effect(crate::potion::Effect::new(
                            id,
                            secs * 20,
                            amp,
                        ));
                    }
                }
            }
        }
        // CommandSetBlock: x y z id [meta]
        if let Ok(list) = std::env::var("MINERUST_SETBLOCK") {
            for cmd in list.split(';') {
                let v: Vec<i32> = cmd
                    .split_whitespace()
                    .filter_map(|t| t.parse().ok())
                    .collect();
                if v.len() >= 4 && (0..256).contains(&v[1]) {
                    let meta = v.get(4).copied().unwrap_or(0);
                    self.world.set_block(v[0], v[1], v[2], v[3] as u8, meta, 3);
                    if v[3] as u8 == crate::block::BEACON {
                        self.play.tiles.entry([v[0], v[1], v[2]]).or_insert_with(crate::play::Tile::beacon);
                    }
                    // a sign's lines after `text=`, '|' apart
                    if let Some(t) = cmd.split_once("text=").map(|t| t.1) {
                        let mut text: [String; 4] = Default::default();
                        for (l, s) in text.iter_mut().zip(t.split('|')) {
                            *l = s.chars().take(15).collect();
                        }
                        self.play.tiles.insert(
                            [v[0], v[1], v[2]],
                            crate::play::Tile::Sign { text, editable: false },
                        );
                    }
                }
            }
        }
        // --hang "x y z dir P Title" or "x y z dir F id[:meta] rotation", ';' apart
        if let Ok(list) = std::env::var("MINERUST_HANG") {
            use crate::entity::hanging::{art, Hanging, ART_COUNT};
            for cmd in list.split(';') {
                let t: Vec<&str> = cmd.split_whitespace().collect();
                if t.len() < 5 {
                    continue;
                }
                let p: Vec<i32> = t[..4].iter().filter_map(|v| v.parse().ok()).collect();
                if p.len() < 4 {
                    continue;
                }
                let id = self.alloc_vehicle_id();
                let seed = self.mobs.rand.next_long();
                let tile = [p[0], p[1], p[2]];
                let h = if t[4] == "P" {
                    let name = t.get(5).copied().unwrap_or("");
                    let a = (0..ART_COUNT).find(|&a| art(a).0 == name).unwrap_or(0);
                    let mut h = Hanging::new(id, tile, Some(a), seed);
                    h.set_direction(p[3] & 3);
                    h
                } else {
                    let mut h = Hanging::frame(id, tile, p[3] & 3, seed);
                    if let Some(it) = t.get(5) {
                        let mut f = it.split(':').filter_map(|v| v.parse::<i32>().ok());
                        let item = f.next().unwrap_or(0);
                        let meta = f.next().unwrap_or(0);
                        if item > 0 {
                            h.item = Some(crate::item::Stack::new(item as u16, meta, 1));
                        }
                    }
                    h.rotation = t.get(6).and_then(|v| v.parse().ok()).unwrap_or(0) & 3;
                    h
                };
                self.play
                    .vehicles
                    .push(crate::entity::vehicle::Tracked::new(
                        crate::entity::vehicle::Vehicle::Hang(h),
                    ));
            }
        }
        let Ok(list) = std::env::var("MINERUST_SUMMON") else {
            return;
        };
        let (feet, yaw) = self.play.player;
        let (s, c) = (yaw.to_radians().sin() as f64, yaw.to_radians().cos() as f64);
        let fwd = [-s, c];
        let side = [c, s];
        let items: Vec<&str> = list.split(',').filter(|x| !x.is_empty()).collect();
        let n = items.len() as f64;
        let mut rnd = std::mem::replace(&mut self.world.rand, JavaRandom::new(0));
        for (i, item) in items.iter().enumerate() {
            let mut parts = item.split(':');
            let name = parts.next().unwrap_or("");
            let off = (i as f64 - (n - 1.0) / 2.0) * 2.2;
            if name == "LightningBolt" {
                // a bolt five blocks ahead, on the ground
                let (x, z) = (feet[0] + fwd[0] * 5.0 + side[0] * off, feet[2] + fwd[1] * 5.0 + side[1] * off);
                let (bx, bz) = (x.floor() as i32, z.floor() as i32);
                let y = self.world.precipitation_height(bx, bz);
                self.world.bolts.push([bx, y, bz]);
                continue;
            }
            if name == "Item" {
                // Item:<id>:<count>: a dropped stack three blocks ahead
                let id = parts.next().and_then(|v| v.parse().ok()).unwrap_or(1u16);
                let count = parts.next().and_then(|v| v.parse().ok()).unwrap_or(1u32);
                let p = [
                    feet[0] + fwd[0] * 3.0 + side[0] * off,
                    feet[1] + 0.5,
                    feet[2] + fwd[1] * 3.0 + side[1] * off,
                ];
                let mut st = crate::item::Stack::new(id, 0, 1);
                st.count = count;
                let eid = self.play.next_item;
                self.play.next_item += 1;
                self.play
                    .spawn(crate::entity::item::ItemEntity::new(eid, st, p));
                continue;
            }
            if let Some(kind) = crate::entity::minecart::CartKind::from_entity_id(name) {
                // on the rail under the summoning spot
                let dist = 3.0;
                let x = feet[0] + fwd[0] * dist + side[0] * off;
                let z = feet[2] + fwd[1] * dist + side[1] * off;
                let (bx, bz) = (x.floor() as i32, z.floor() as i32);
                let mut y = feet[1].floor() as i32 + 3;
                while y > 1
                    && !crate::entity::minecart::is_rail(self.world.block(bx, y, bz))
                    && !self.world.material(bx, y - 1, bz).blocks_movement()
                {
                    y -= 1;
                }
                let id = self.alloc_vehicle_id();
                let seed = self.mobs.rand.next_long();
                let mut c = crate::entity::minecart::Minecart::new(id, kind, seed);
                c.e.set_location_and_angles(
                    bx as f64 + 0.5,
                    y as f64,
                    bz as f64 + 0.5,
                    yaw + 90.0,
                    0.0,
                );
                let parts: Vec<&str> = parts.collect();
                if parts.contains(&"lit") {
                    c.fuel = 3600;
                    c.push = [
                        -(yaw.to_radians().sin() as f64),
                        yaw.to_radians().cos() as f64,
                    ];
                }
                self.play
                    .vehicles
                    .push(crate::entity::vehicle::Tracked::new(
                        crate::entity::vehicle::Vehicle::Cart(c),
                    ));
                if parts.contains(&"ride") {
                    self.interact_vehicle(id);
                }
                continue;
            }
            // Fireworks:shape:colour[:fade]:life -- a rocket with one star
            if name == "Fireworks" {
                use crate::world::anvil::Nbt;
                let v: Vec<i32> = parts.filter_map(|t| t.parse().ok()).collect();
                let shape = v.first().copied().unwrap_or(0);
                let col = v.get(1).copied().unwrap_or(11743532);
                let mut ex = vec![
                    ("Type".to_string(), Nbt::Byte(shape as i8)),
                    ("Colors".to_string(), Nbt::IntArray(vec![col])),
                ];
                if let Some(&f) = v.get(2).filter(|&&f| f > 0) {
                    ex.push(("FadeColors".to_string(), Nbt::IntArray(vec![f])));
                }
                let fw = Nbt::Compound(vec![
                    ("Explosions".into(), Nbt::List(10, vec![Nbt::Compound(ex)])),
                    ("Flight".into(), Nbt::Byte(1)),
                ]);
                let st = crate::item::Stack::new(401, 0, 1).edit_tag(|t| t.set("Fireworks", fw));
                let dist: f64 = std::env::var("MINERUST_SUMMON_DIST")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(10.0);
                let x = feet[0] + fwd[0] * dist + side[0] * off;
                let z = feet[2] + fwd[1] * dist + side[1] * off;
                let id = self.alloc_vehicle_id();
                let seed = self.mobs.rand.next_long();
                let mut r = crate::entity::rocket::Rocket::new(id, [x, feet[1], z], Some(st), seed);
                if let Some(&l) = v.get(3) {
                    r.lifetime = l;
                }
                self.play
                    .vehicles
                    .push(crate::entity::vehicle::Tracked::new(
                        crate::entity::vehicle::Vehicle::Rocket(r),
                    ));
                continue;
            }
            if name == "XPOrb" {
                let v: i32 = parts.next().and_then(|t| t.parse().ok()).unwrap_or(7);
                let dist: f64 = std::env::var("MINERUST_SUMMON_DIST")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(3.0);
                let x = feet[0] + fwd[0] * dist + side[0] * off;
                let z = feet[2] + fwd[1] * dist + side[1] * off;
                let mut play = std::mem::replace(&mut self.play, Play::new(GameMode::Survival));
                self.spawn_orbs(&mut play, [x, feet[1] + 1.0, z], v);
                self.play = play;
                continue;
            }
            if name == "Boat" {
                // CommandSummon: EntityList.createEntityByName + setLocationAndAngles
                let dist = 4.0;
                let x = feet[0] + fwd[0] * dist + side[0] * off;
                let z = feet[2] + fwd[1] * dist + side[1] * off;
                let (bx, bz) = (x.floor() as i32, z.floor() as i32);
                let mut y = feet[1].floor() as i32 + 3;
                while y > 1 && {
                    let m = self.world.material(bx, y - 1, bz);
                    !m.blocks_movement() && !m.is_liquid()
                } {
                    y -= 1;
                }
                let id = self.alloc_vehicle_id();
                let seed = self.mobs.rand.next_long();
                let mut b = crate::entity::boat::Boat::new(id, seed);
                b.e.set_location_and_angles(x, y as f64, z, yaw + 90.0, 0.0);
                self.world.solid_entities.push((id, b.bounding_box()));
                self.play
                    .vehicles
                    .push(crate::entity::vehicle::Tracked::new(
                        crate::entity::vehicle::Vehicle::Boat(b),
                    ));
                if parts.any(|p| p == "ride") {
                    self.interact_vehicle(id);
                }
                continue;
            }
            let Some(kind) = crate::entity::mob::Kind::from_id(name) else {
                continue;
            };
            let dist: f64 = std::env::var("MINERUST_SUMMON_DIST")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5.0);
            let x = feet[0] + fwd[0] * dist + side[0] * off;
            let z = feet[2] + fwd[1] * dist + side[1] * off;
            let (bx, bz) = (x.floor() as i32, z.floor() as i32);
            let mut y = feet[1].floor() as i32 + 3;
            while y > 1 && !self.world.material(bx, y - 1, bz).blocks_movement() {
                y -= 1;
            }
            let id = self.mobs.spawn_in(
                &mut self.world,
                kind,
                [x, y as f64, z],
                yaw + 180.0,
                &mut rnd,
            );
            if std::env::var("MINERUST_DBG").is_ok() {
                eprintln!("summon {:?} at {:.1} {} {:.1}", kind, x, y, z);
            }
            if let Some(m) = self.mobs.list.iter_mut().find(|m| m.id == id) {
                m.persistent = true;
                for p in parts {
                    if let Some(n) = p.strip_prefix("name=") {
                        m.custom_name = n.to_string();
                        continue;
                    }
                    match p {
                        "tamed" => m.tamed = true,
                        "sit" => {
                            m.sitting = true;
                            m.sit_order = true;
                        }
                        "baby"
                            if matches!(
                                m.kind,
                                crate::entity::mob::Kind::Zombie
                                    | crate::entity::mob::Kind::PigZombie
                            ) =>
                        {
                            m.set_baby(true)
                        }
                        "baby" => {
                            m.growing_age = -24000;
                            let p = m.pos;
                            m.set_position(p[0], p[1], p[2]);
                        }
                        "villager" => m.zombie_villager = true,
                        "saddle" => {
                            m.saddled = true;
                            if m.kind == crate::entity::mob::Kind::Horse {
                                m.horse_flags |= 6;
                                m.horse_chest[0] = Some(crate::item::Stack::new(329, 0, 1));
                            }
                        }
                        "chest" => m.horse_flags |= 8,
                        "leash" => m.leash = Some(crate::entity::mob::Leash::Player),
                        "jockey" => {
                            m.jockey = Some(if m.kind == crate::entity::mob::Kind::Spider {
                                (crate::entity::mob::Kind::Skeleton, false)
                            } else {
                                (crate::entity::mob::Kind::Chicken, false)
                            })
                        }
                        "iron" | "gold" | "diamond" => {
                            m.horse_armor = match p {
                                "iron" => 1,
                                "gold" => 2,
                                _ => 3,
                            }
                        }
                        "big" => m.set_slime_size(4),
                        "hang" => m.hanging = true,
                        v => {
                            if let Ok(k) = v.parse::<i32>() {
                                m.variant = k;
                            }
                        }
                    }
                }
            }
        }
        let pending: Vec<u32> = self.mobs.list.iter().filter(|m| m.jockey.is_some()).map(|m| m.id).collect();
        for id in pending {
            self.mobs.finish_jockey(id, &mut rnd, &crate::entity::mob::SpawnCtx::default());
        }
        self.world.rand = rnd;
    }

    fn save_legacy_chunks(
        world: &ServerWorld,
        unsaved: &mut std::collections::BTreeSet<(i32, i32)>,
    ) -> std::io::Result<()> {
        let Some(dir) = &world.save_dir else {
            return Ok(());
        };
        for &(cx, cz) in unsaved.iter() {
            if let Some(chunk) = world.chunks.get(&(cx, cz)) {
                save::write_chunk(dir, cx, cz, &chunk.to_bytes())?;
            }
        }
        unsaved.clear();
        Ok(())
    }

    /// Writes the level file and every chunk changed since the last save.
    pub fn save(&mut self) -> std::io::Result<()> {
        let Some(base) = self.base_dir.clone() else {
            return Ok(());
        };
        self.maps.save()?;
        // ServerConfigurationManager.writePlayerData → saveStatFile
        self.stats_file.save_stat_file()?;
        // Shutdown can apply commands and save before a tick reports changes.
        self.unsaved.extend(self.world.dirty.keys().copied());
        for p in &mut self.parked {
            p.unsaved.extend(p.world.dirty.keys().copied());
        }
        if self.world.anvil {
            return self.save_anvil();
        }
        // every dimension but the overworld keeps its own data folder
        let wdir = self.world.save_dir.clone().unwrap_or(base.clone());
        Self::save_legacy_chunks(&self.world, &mut self.unsaved)?;
        for p in &mut self.parked {
            let Some(d) = p.world.save_dir.clone() else {
                continue;
            };
            Self::save_legacy_chunks(&p.world, &mut p.unsaved)?;
            let data = d.join("data");
            std::fs::create_dir_all(&data)?;
            if let Some(st) = &p.world.structures {
                save::write_atomic(&data.join("structures.txt"), st.save().as_bytes())?;
            }
            save::write_atomic(
                &data.join("entities.txt"),
                p.mobs.encode().join("\n").as_bytes(),
            )?;
            save::write_atomic(
                &data.join("drops.dat"),
                &Play::drops_nbt(&p.items, &p.falling, &p.vehicles, p.dim),
            )?;
            if p.dim != 0 {
                let mut tmp = Play::new(GameMode::Survival);
                tmp.tiles = p.tiles.clone();
                save::write_atomic(
                    &data.join("tiles.txt"),
                    tmp.encode_tiles().join("\n").as_bytes(),
                )?;
            }
        }
        if self.dim != 0 {
            let data = wdir.join("data");
            std::fs::create_dir_all(&data)?;
            save::write_atomic(
                &data.join("tiles.txt"),
                self.play.encode_tiles().join("\n").as_bytes(),
            )?;
        }
        let overworld_tiles = if self.dim == 0 {
            self.play.encode_tiles()
        } else {
            let mut tmp = Play::new(GameMode::Survival);
            if let Some(p) = self.parked.iter().find(|p| p.dim == 0) {
                tmp.tiles = p.tiles.clone();
            }
            tmp.encode_tiles()
        };
        let ticks = if self.dim == 0 {
            self.world.pending_tick_list()
        } else {
            self.parked
                .iter()
                .find(|p| p.dim == 0)
                .map(|p| p.world.pending_tick_list())
                .unwrap_or_default()
        };
        let level = Level {
            seed: self.world.seed(),
            total_time: self.world.total_time,
            world_time: self.world_time,
            vanilla_parity: crate::compat::vanilla_parity(),
            player: self.player,
            ticks,
            unknown: {
                let mut u = self.unknown_level_keys.clone();
                u.push(("dim".into(), self.dim.to_string()));
                u.push(("inventory".into(), self.play.inv.encode()));
                u.push(("weather".into(), self.weather.encode()));
                u.push((
                    "gamerules".into(),
                    self.rules()
                        .0
                        .iter()
                        .map(|(k, v)| format!("{k}={v}"))
                        .collect::<Vec<_>>()
                        .join(";"),
                ));
                u.push((
                    "mode".into(),
                    if self.play.mode == GameMode::Creative {
                        "creative"
                    } else {
                        "survival"
                    }
                    .into(),
                ));
                u.push((
                    "spawn".into(),
                    format!(
                        "{:?},{:?},{:?}",
                        self.spawn[0], self.spawn[1], self.spawn[2]
                    ),
                ));
                u.push(("stats".into(), self.splayer.encode()));
                for t in overworld_tiles {
                    u.push(("tile".into(), t));
                }
                u
            },
        };
        if let Some(st) = &self.world.structures {
            let data = wdir.join("data");
            std::fs::create_dir_all(&data)?;
            save::write_atomic(&data.join("structures.txt"), st.save().as_bytes())?;
        }
        {
            let data = wdir.join("data");
            std::fs::create_dir_all(&data)?;
            save::write_atomic(
                &data.join("entities.txt"),
                self.mobs.encode().join("\n").as_bytes(),
            )?;
            save::write_atomic(
                &data.join("drops.dat"),
                &Play::drops_nbt(
                    &self.play.items,
                    &self.play.falling,
                    &self.play.vehicles,
                    self.dim,
                ),
            )?;
        }
        save::write_level(&base, &level)
    }

    /// Mob AI, spawning and despawning, and what the client must hear of it.
    /// `attackEntityFrom(explosion)` and the push for the entities outside
    /// `Mobs` an explosion reached.
    fn apply_blasts(&mut self, play: &mut Play, blasts: Vec<crate::world::explosion::Blast>) {
        use crate::entity::vehicle::Vehicle;
        use crate::world::explosion::{TAG_FALLING, TAG_ITEM, TAG_VEHICLE};
        for b in blasts {
            let id = (b.tag & 0xFFFF_FFFF) as u32;
            match b.tag & !0xFFFF_FFFF {
                TAG_ITEM => {
                    if let Some(it) = play.items.iter_mut().find(|i| i.id == id) {
                        it.attack(None, true, b.damage);
                        for a in 0..3 {
                            it.e.motion[a] += b.push[a];
                        }
                    }
                }
                TAG_FALLING => {
                    if let Some(f) = play.falling.iter_mut().find(|f| f.id == id) {
                        for a in 0..3 {
                            f.e.motion[a] += b.push[a];
                        }
                    }
                }
                TAG_VEHICLE => {
                    let Some(i) = play.vehicles.iter().position(|v| v.v.id() == id) else {
                        continue;
                    };
                    let mut outs = Vec::new();
                    let mut broke = None;
                    match &mut play.vehicles[i].v {
                        Vehicle::Boat(boat) => broke = boat.attack(b.damage, false),
                        Vehicle::Cart(c) => c.attack(b.damage, false, true, false, &mut outs),
                        Vehicle::Orb(o) => o.attack(b.damage),
                        Vehicle::Hang(_) => {
                            self.break_hanging(play, i, false, false);
                            continue;
                        }
                        // Entity.attackEntityFrom: no effect, the push still applies
                        Vehicle::Rocket(_) => {}
                    }
                    {
                        let e = play.vehicles[i].v.e_mut();
                        for a in 0..3 {
                            e.motion[a] += b.push[a];
                        }
                    }
                    if let Some(drop) = broke {
                        if drop {
                            let p = play.vehicles[i].v.e().pos;
                            let nid = play.next_item;
                            play.next_item += 1;
                            let mut r = std::mem::replace(&mut play.prand, JavaRandom::new(0));
                            let mut it = crate::entity::item::ItemEntity::new(
                                nid,
                                crate::item::Stack::new(333, 0, 1),
                                p,
                            );
                            play.prand = r;
                            it.pickup_delay = 10;
                            play.items.push(it);
                        }
                    }
                    self.vehicle_outs(play, i, outs);
                }
                _ => {}
            }
        }
    }

    fn tick_mobs(&mut self, play: &mut Play, events: &mut Vec<Event>) {
        use crate::entity::mob;
        let (feet, yaw) = play.player;
        let held = play.inv.held().map(|s| s.id);
        let mut effect_bits = 0u32;
        for e in self.splayer.living.effects.iter() {
            effect_bits |= 1 << e.id;
        }
        let info = mob::PlayerInfo {
            motion: self.splayer.e.motion,
            health: self.splayer.living.health,
            effects: effect_bits,
            hurt_resistant_time: self.splayer.living.hurt_resistant_time,
            last_damage: self.splayer.living.last_damage,
            difficulty: self.difficulty as i32,
            feet,
            yaw,
            pitch: self.player.map_or(0.0, |p| p.1[1]),
            attackable: self.player_attackable
                && play.mode == GameMode::Survival
                && !self.splayer.dead,
            alive: !self.splayer.dead,
            sneaking: self.motion_flags.1,
            revenge_timer: self.last_attacker_time,
            last_attack_time: self.last_attacked_time,
            armor_pieces: play.inv.armor.iter().flatten().count() as i32,
            held,
            looting: crate::enchant::level(crate::enchant::LOOTING, play.inv.held().as_ref()),
            creative: play.mode == GameMode::Creative,
            ride_strafe: self.ride.strafe,
            ride_forward: self.ride.forward,
            // both are forgotten after 100 ticks (onEntityUpdate)
            last_attacked: self
                .last_attacked
                .filter(|_| self.world.total_time as i32 - self.last_attacked_time <= 100),
            last_attacker: self
                .last_attacker
                .filter(|_| self.world.total_time as i32 - self.last_attacker_time <= 100),
        };
        // World.getCurrentMoonPhaseFactor
        self.world.moon_factor = [1.0, 0.75, 0.5, 0.25, 0.0, 0.25, 0.5, 0.75]
            [((self.world_time / 24000 % 8 + 8) % 8) as usize];
        let mut out = mob::Outcome::default();
        {
            // entities an explosion in the mobs' tick can reach
            use crate::world::explosion::{Target, TAG_FALLING, TAG_ITEM, TAG_VEHICLE};
            let mut t = Vec::new();
            for it in &play.items {
                t.push(Target {
                    tag: TAG_ITEM | it.id as u64,
                    pos: it.e.pos,
                    eye: 0.0,
                    bb: it.bb(),
                });
            }
            for f in &play.falling {
                t.push(Target {
                    tag: TAG_FALLING | f.id as u64,
                    pos: f.e.pos,
                    eye: 0.0,
                    bb: f.e.bb,
                });
            }
            for v in &play.vehicles {
                t.push(Target {
                    tag: TAG_VEHICLE | v.v.id() as u64,
                    pos: v.v.e().pos,
                    eye: 0.0,
                    bb: v.v.e().bb,
                });
            }
            self.mobs.blast_targets = t;
        }
        self.mobs.tick(&mut self.world, &info, &mut out);
        self.stick_wear += std::mem::take(&mut out.stick_wear);
        self.mobs_pick_up_loot(play);
        self.mobs.blast_targets.clear();
        self.apply_blasts(play, std::mem::take(&mut out.blasts));
        // the explosions' drops spawn now, as spawnEntityInWorld does
        {
            use crate::world::server::GenEntity;
            let mut rest = Vec::new();
            let mut rnd = std::mem::replace(&mut self.world.rand, JavaRandom::new(0));
            for e in std::mem::take(&mut self.world.gen_entities) {
                match e {
                    GenEntity::ScatteredItem { .. } => play.add_gen_entity(e, &mut rnd),
                    other => rest.push(other),
                }
            }
            self.world.rand = rnd;
            self.world.gen_entities = rest;
        }
        let animals = self.world.total_time % 400 == 0;
        // WorldServer.tick: findChunksForSpawning only under doMobSpawning
        if crate::gamerules::bool("doMobSpawning") {
            mob::natural_spawning(&mut self.world, &mut self.mobs, feet, self.spawn, animals);
        }
        mob::village_golems(&mut self.world, &mut self.mobs);
        // forget targets that are gone
        if let Some(id) = self.last_attacked {
            if !self.mobs.list.iter().any(|m| m.id == id && m.health > 0.0) {
                self.last_attacked = None;
            }
        }
        if let Some(id) = self.last_attacker {
            if !self.mobs.list.iter().any(|m| m.id == id && m.health > 0.0) {
                self.last_attacker = None;
            }
        }
        if self.world.total_time % 40 == 0 {
            self.mobs.despawn(feet);
        }
        let mut outs = std::mem::take(&mut self.pending_out);
        outs.push(out);
        let mut hits = Vec::new();
        let mut sounds = Vec::new();
        let mut notes = Vec::new();
        {
            use crate::world::redstone::Effect;
            let mut rnd = std::mem::replace(&mut self.world.rand, JavaRandom::new(0));
            for e in std::mem::take(&mut self.world.redstone_effects) {
                match e {
                    Effect::Sound(n, p, v, pi) => sounds.push((n, p, v, pi)),
                    Effect::Note(p, inst, pitch) => notes.push((p, inst, pitch)),
                    Effect::Tnt(p, fuse) => self.mobs.prime_tnt(p, fuse),
                    Effect::BoneMeal(p) => self.bone_meal_fx.push(p),
                    Effect::Record(p, id) => events.push(Event::Record(p, id)),
                    Effect::Dispense(p, facing) => {
                        self.dispense_block(play, p, facing, &mut rnd, &mut sounds);
                    }
                }
            }
            self.world.rand = rnd;
        }
        if !notes.is_empty() {
            events.push(Event::Notes(notes));
        }
        let mut explosions = Vec::new();
        let mut particles = Vec::new();
        let mut dust = Vec::new();
        let mut rnd = std::mem::replace(&mut self.world.rand, JavaRandom::new(0));
        for o in outs {
            dust.extend(o.dust);
            // the mobs' applyEntityCollision on the player (addVelocity)
            if o.player_push != [0.0; 3] {
                for a in 0..3 {
                    self.splayer.e.motion[a] += o.player_push[a];
                    self.push_add[a] += o.player_push[a];
                }
            }
            if o.attacker.is_some() {
                self.last_attacker = o.attacker;
                self.last_attacker_time = self.world.total_time as i32;
            }
            particles.extend(o.particles);
            for (p, st) in o.drops {
                play.add_gen_entity(
                    crate::world::server::GenEntity::Item { pos: p, stack: st },
                    &mut rnd,
                );
            }
            hits.extend(o.hits);
            sounds.extend(o.sounds);
            explosions.extend(o.explosions);
            for (p, xp) in o.orbs {
                self.spawn_orbs(play, p, xp);
            }
            for a in o.achievements {
                self.achieve(a);
            }
            for k in o.bred {
                self.stats_file.increase_stat("stat.animalsBred", 1);
                if k == crate::entity::mob::Kind::Cow && self.stats_file.trigger_achievement("achievement.breedCow") {
                    let dirty = self.stats_file.take_dirty();
                    self.stats_out.extend(dirty);
                }
            }
        }
        for (p, xp) in std::mem::take(&mut play.orbs) {
            self.spawn_orbs(play, p, xp);
        }
        self.world.rand = rnd;
        self.apply_player_hits(hits);
        self.wake_if_hurt();
        if !sounds.is_empty() {
            events.push(Event::Sounds(sounds));
        }
        if !explosions.is_empty() {
            events.push(Event::Explosions(explosions));
        }
        if !particles.is_empty() {
            events.push(Event::Particles(particles));
        }
        if !dust.is_empty() {
            events.push(Event::FallDust(dust));
        }
        let r2 = ((self.view + 1) * 16) as f64;
        let views: Vec<MobView> = self
            .mobs
            .list
            .iter()
            .filter(|m| (m.pos[0] - feet[0]).abs() < r2 && (m.pos[2] - feet[2]).abs() < r2)
            .map(|m| MobView {
                id: m.id,
                kind: m.kind,
                pos: m.pos,
                prev: m.prev_pos,
                body_yaw: m.body_yaw,
                prev_body_yaw: m.prev_body_yaw,
                head_yaw: m.head_yaw,
                prev_head_yaw: m.prev_head_yaw,
                body_head: m.head_yaw,
                body_ticks: 0,
                pitch: m.pitch,
                prev_pitch: m.prev_pitch,
                limb_swing: m.limb_swing,
                limb_amount: m.limb_amount,
                prev_limb_amount: m.prev_limb_amount,
                swing: m.swing,
                prev_swing: m.prev_swing,
                hurt_time: m.hurt_time,
                death_time: m.death_time,
                ticks: m.ticks,
                variant: m.variant,
                sheared: m.sheared,
                fuse: m.fuse,
                prev_fuse: m.prev_fuse,
                wing: m.wing,
                prev_wing: m.prev_wing,
                eat_timer: m.eat_timer,
                fire: m.fire > 0,
                potion: (m.potions.potion_color, m.potions.potion_ambient),
                squish: [m.prev_squish, m.squish],
                sq: [m.prev_sq, m.sq],
                flags: m.tamed as u32
                    | (m.sitting as u32) << 1
                    | (m.angry as u32) << 2
                    | (m.hanging as u32) << 3
                    | (m.is_child() as u32) << 4
                    | (m.zombie_villager as u32) << 5
                    | ((m.conversion >= 0) as u32) << 6
                    | (m.saddled as u32) << 7
                    | (m.powered as u32) << 8,
                carried: [m.carried, m.carried_meta],
                attack_timer: m.attack_timer,
                equipment: m.equipment,
                health: m.health,
                ring: if m.kind == crate::entity::mob::Kind::EnderDragon {
                    m.ring.to_vec()
                } else {
                    Vec::new()
                },
                ring_index: m.ring_index,
                net_at: m.net.at,
                net_yaw: m.net.yaw,
                net_pitch: m.net.pitch,
                net_seq: m.net.seq,
                net_head: m.net.head,
                yaw: m.yaw,
                incs: 0,
                custom_name: m.custom_name.clone(),
                ridden: m.ridden_by_player,
                horse_flags: m.horse_flags,
                horse_armor: m.horse_armor,
                // EntityHorse.getHorseSize
                horse_size: if m.growing_age >= 0 {
                    1.0
                } else {
                    0.5 + (-24000 - m.growing_age) as f32 / -24000.0 * 0.5
                },
                hanim: [0.0; 6],
                tail_counter: 0,
                max_health: m.max_health(),
                wither_invul: m.wither.invul,
                wolf_wet: m.wolf_wet,
                shake: m.shake,
                begging: m.begging,
                head_course: [0.0; 2],
                wither_heads: [m.wither.prev_heads, m.wither.heads],
                leash: m.leash.map(|l| match l {
                    crate::entity::mob::Leash::Player => LeashView::Player,
                    crate::entity::mob::Leash::Knot(b) => LeashView::Knot([
                        b[0] as f64 + 0.5,
                        b[1] as f64 + 0.5,
                        b[2] as f64 + 0.5,
                    ]),
                }),
                bb: [
                    m.bb.min[0],
                    m.bb.min[1],
                    m.bb.min[2],
                    m.bb.max[0],
                    m.bb.max[1],
                    m.bb.max[2],
                ],
            })
            .collect();
        let arrows: Vec<ArrowView> = self
            .mobs
            .arrows
            .iter()
            .map(|a| ArrowView {
                burning: a.e.fire > 0,
                pos: a.e.pos,
                prev: a.e.prev,
                yaw: a.e.yaw,
                pitch: a.e.pitch,
            })
            .collect();
        events.push(Event::Mobs(views, arrows));
        events.push(Event::Tnt(
            self.mobs
                .tnt
                .iter()
                .map(|t| (t.pos, t.prev_pos, t.fuse))
                .collect(),
        ));
        // EntityEnderEye.onUpdate
        let mut drops = Vec::new();
        let mut eye_sounds = Vec::new();
        self.eyes.retain_mut(|e| {
            e.prev = e.pos;
            for k in 0..3 {
                e.pos[k] += e.motion[k];
            }
            let (dx, dz) = (e.target[0] - e.pos[0], e.target[2] - e.pos[2]);
            let dist = (dx * dx + dz * dz).sqrt() as f32;
            let ang = dz.atan2(dx) as f32;
            let speed = (e.motion[0] * e.motion[0] + e.motion[2] * e.motion[2]).sqrt();
            let mut v = speed + (dist as f64 - speed) * 0.0025;
            if dist < 1.0 {
                v *= 0.8;
                e.motion[1] *= 0.8;
            }
            e.motion[0] = (ang.cos() as f64) * v;
            e.motion[2] = (ang.sin() as f64) * v;
            if e.pos[1] < e.target[1] {
                e.motion[1] += (1.0 - e.motion[1]) * 0.015f32 as f64;
            } else {
                e.motion[1] += (-1.0 - e.motion[1]) * 0.015f32 as f64;
            }
            e.timer += 1;
            if e.timer > 80 {
                if e.drop {
                    drops.push((e.pos, crate::item::Stack::new(381, 0, 1)));
                } else {
                    eye_sounds.push(("random.glass", e.pos, 1.0, 1.0));
                }
                return false;
            }
            true
        });
        if !eye_sounds.is_empty() {
            events.push(Event::Sounds(eye_sounds));
        }
        let mut rnd = std::mem::replace(&mut self.world.rand, JavaRandom::new(0));
        for (p, st) in drops {
            play.add_gen_entity(
                crate::world::server::GenEntity::Item { pos: p, stack: st },
                &mut rnd,
            );
        }
        self.world.rand = rnd;
        let mut shots: Vec<([f64; 3], [f64; 3], u8)> = self
            .mobs
            .fireballs
            .iter()
            .map(|f| {
                let k = match f.wither {
                    1 => 3,
                    2 => 4,
                    _ => f.small as u8,
                };
                (f.pos, f.prev_pos, k)
            })
            .collect();
        shots.extend(self.eyes.iter().map(|e| (e.pos, e.prev, 2u8)));
        events.push(Event::Fireballs(shots));
    }

    /// Damage from mobs, projectiles, explosions and potions, as their
    /// `attackEntityFrom(source, amount)` calls on the player.
    fn apply_player_hits(&mut self, hits: Vec<crate::entity::mob::Hit>) {
        use crate::entity::living::{Difficulty, Source};
        use crate::entity::mob::Hit;
        use crate::potion;
        let d = self.difficulty;
        let pos_of = |mobs: &crate::entity::mob::Mobs, id: Option<u32>| {
            id.and_then(|i| mobs.list.iter().find(|m| m.id == i).map(|m| (i, m.pos)))
        };
        for h in hits {
            match h {
                Hit::Melee {
                    kind,
                    by,
                    fire,
                    amount,
                    knock,
                    from,
                    effect,
                } => {
                    if amount <= 0.0 {
                        // Entity.addVelocity: the server-side motion only
                        for a in 0..3 {
                            self.splayer.e.motion[a] += knock[a];
                        }
                        continue;
                    }
                    let src = Source::mob(kind, 0, from);
                    if self.splayer.attack(&self.world, src, amount, d) {
                        // EntityMob.attackEntityAsMob, on a hit: the push,
                        // Fire Aspect, then applyThornEnchantments(player, mob)
                        if knock != [0.0; 3] {
                            for a in 0..3 {
                                self.splayer.e.motion[a] += knock[a];
                            }
                            self.splayer.velocity = Some(self.splayer.e.motion);
                        }
                        if fire > 0 {
                            self.splayer.e.set_fire(fire * 4);
                        }
                        use crate::entity::mob::Kind as K;
                        let entity_mob = matches!(
                            kind,
                            K::Zombie
                                | K::PigZombie
                                | K::Skeleton
                                | K::Spider
                                | K::CaveSpider
                                | K::Silverfish
                                | K::Enderman
                                | K::Blaze
                        );
                        if entity_mob && (knock[0] != 0.0 || knock[2] != 0.0) {
                            // the Knockback branch slows the attacker too
                            if let Some(m) = self.mobs.list.iter_mut().find(|m| m.id == by) {
                                m.motion[0] *= 0.6;
                                m.motion[2] *= 0.6;
                            }
                        }
                        if entity_mob {
                            self.thorns(by);
                        }
                        if matches!(kind, K::Zombie | K::PigZombie) {
                            // EntityZombie.attackEntityAsMob, after super's
                            // hit: a burning zombie with empty hands sets
                            // its victim alight
                            let id = d as i32;
                            if let Some(m) = self.mobs.list.iter_mut().find(|m| m.id == by) {
                                if m.zombie_ignites(id) {
                                    self.splayer.e.set_fire(2 * id);
                                }
                            }
                        }
                        match effect {
                            // EntityCaveSpider.attackEntityAsMob
                            Some((potion::POISON, -1)) => {
                                let s = match d {
                                    Difficulty::Normal => 7,
                                    Difficulty::Hard => 15,
                                    _ => 0,
                                };
                                if s > 0 {
                                    self.splayer.living.add_effect(potion::Effect::new(
                                        potion::POISON,
                                        s * 20,
                                        0,
                                    ));
                                }
                            }
                            Some((id, dur)) => self
                                .splayer
                                .living
                                .add_effect(potion::Effect::new(id, dur, 0)),
                            None => {}
                        }
                    }
                }
                Hit::Arrow {
                    amount, shooter, ..
                } => {
                    let src = Source::arrow(pos_of(&self.mobs, shooter));
                    // EntityArrow.onUpdate: on a hit from a living shooter,
                    // applyThornEnchantments(player, shooter)
                    if self.splayer.attack(&self.world, src, amount, d) {
                        if let Some(id) =
                            shooter.filter(|&i| self.mobs.list.iter().any(|m| m.id == i))
                        {
                            self.thorns(id);
                        }
                    }
                }
                Hit::Fireball {
                    amount,
                    shooter,
                    small,
                } => {
                    let src = Source::fireball(pos_of(&self.mobs, Some(shooter)));
                    // EntitySmallFireball.onImpact: the target is set on fire
                    if self.splayer.attack(&self.world, src, amount, d) && small {
                        self.splayer.e.set_fire(5);
                    }
                }
                Hit::Explosion {
                    amount,
                    knock,
                    from,
                } => {
                    let src = Source::explosion(from.map(|p| (0, p)));
                    self.splayer.attack(&self.world, src, amount, d);
                    for a in 0..3 {
                        self.splayer.e.motion[a] += knock[a];
                    }
                    if self.splayer.velocity.is_some() {
                        self.splayer.velocity = Some(self.splayer.e.motion);
                    }
                    self.push_add = [
                        self.push_add[0] + knock[0],
                        self.push_add[1] + knock[1],
                        self.push_add[2] + knock[2],
                    ];
                }
                Hit::Potion {
                    effects,
                    scale,
                    from,
                } => {
                    // EntityPotion.onImpact
                    for e in effects {
                        let pt = e.potion();
                        if pt.instant {
                            // Potion.affectEntity
                            if e.id == potion::HARM {
                                let n = (scale * (6 << e.amplifier) as f64 + 0.5) as i32;
                                self.splayer.attack(
                                    &self.world,
                                    Source::indirect_magic(Some((0, from))),
                                    n as f32,
                                    d,
                                );
                            } else if e.id == potion::HEAL {
                                let n = (scale * (4 << e.amplifier) as f64 + 0.5) as i32;
                                self.splayer.living.heal(n as f32);
                            }
                        } else {
                            let dur = (scale * e.duration as f64 + 0.5) as i32;
                            if dur > 20 {
                                self.splayer.living.add_effect(potion::Effect::new(
                                    e.id,
                                    dur,
                                    e.amplifier,
                                ));
                            }
                        }
                    }
                }
                Hit::Xp(n) => self.splayer.add_xp(n),
                Hit::Teleport(p) => {
                    // EntityEnderPearl.onImpact: setPositionAndUpdate, then
                    // fallDistance = 0 and 5 fall damage
                    self.teleport_to = Some(p);
                    self.splayer.e.fall_distance = 0.0;
                    let src = crate::entity::living::Source::named(
                        crate::entity::living::SourceKind::Fall,
                    );
                    self.splayer.attack(&self.world, src, 5.0, d);
                }
                Hit::Burn(t) => self.splayer.e.set_fire(t / 20),
            }
        }
    }

    /// `EnchantmentHelper.applyThornEnchantments(player, attacker)`: each
    /// Thorns enchantment on the worn pieces, in order, runs
    /// `EnchantmentThorns.onUserHurt` with the player's random; the piece
    /// that pays is always the first one with Thorns
    /// (`getEnchantedItem`).
    fn thorns(&mut self, attacker: u32) {
        use crate::enchant;
        let items = self.splayer.armor_items;
        for s in items.iter().flatten() {
            let lv = s
                .enchantments()
                .into_iter()
                .filter(|e| e.0 == enchant::THORNS)
                .map(|e| e.1)
                .collect::<Vec<_>>();
            for l in lv {
                let first = self
                    .splayer
                    .armor_items
                    .iter()
                    .position(|a| enchant::level(enchant::THORNS, a.as_ref()) > 0);
                let r = &mut self.splayer.e.rand;
                let cost = if enchant::thorns_should_hit(l, r) {
                    let dmg = enchant::thorns_damage(l, r) as f32;
                    let mut out = crate::entity::mob::Outcome::default();
                    let feet = self.play.player.0;
                    let yaw = self.play.player.1;
                    // causeThornsDamage(player): blockable, from the player
                    self.mobs.attack(attacker, dmg, feet, yaw, &mut out);
                    if let Some(m) = self.mobs.list.iter_mut().find(|m| m.id == attacker) {
                        m.looting = enchant::level(enchant::LOOTING, self.play.inv.held().as_ref());
                        out.sounds.push(("damage.thorns", m.pos, 0.5, 1.0));
                    }
                    self.pending_out.push(out);
                    3
                } else {
                    1
                };
                if let Some(i) = first {
                    let p = self.splayer.armor_items[i].unwrap();
                    let creative = self.play.mode == GameMode::Creative;
                    let (left, broke) =
                        enchant::damage_item(p, cost, creative, &mut self.splayer.e.rand);
                    if broke {
                        self.splayer.fx.push(crate::entity::base::Fx::BrokenItem(p));
                    }
                    self.splayer.armor_items[i] = left;
                    self.splayer.armor_changed = true;
                    self.splayer.armor_updated();
                }
            }
        }
    }

    /// `EntityArrow.onCollideWithPlayer` for the arrows in reach.
    fn pick_up_arrows(&mut self, events: &mut Vec<Event>) {
        if self.splayer.dead || self.respawn_pending {
            return;
        }
        let (f, _) = self.play.player;
        let reach = crate::entity::player::Aabb {
            min: [f[0] - 0.3 - 1.0, f[1] - 0.5, f[2] - 0.3 - 1.0],
            max: [f[0] + 0.3 + 1.0, f[1] + 1.8 + 0.5, f[2] + 0.3 + 1.0],
        };
        let creative = self.play.mode == GameMode::Creative;
        let mut sounds = Vec::new();
        for a in self.mobs.arrows.iter_mut() {
            if a.e.dead || !a.in_ground || a.arrow_shake > 0 || !a.e.bb.intersects(&reach) {
                continue;
            }
            let mut ok = a.pickup == 1 || (a.pickup == 2 && creative);
            if a.pickup == 1
                && self
                    .play
                    .inv
                    .add(crate::item::Stack::new(262, 0, 1))
                    .is_some()
            {
                ok = false;
            }
            if ok {
                let r = &mut a.e.rand;
                let p = ((r.next_float() - r.next_float()) * 0.7 + 1.0) * 2.0;
                sounds.push(("random.pop", a.e.pos, 0.2, p));
                a.e.dead = true;
                self.play.inv_dirty = true;
            }
        }
        self.mobs.arrows.retain(|a| !a.e.dead);
        if !sounds.is_empty() {
            events.push(Event::Sounds(sounds));
        }
    }

    /// `getEntityAttribute(attackDamage).getAttributeValue()` with the held
    /// item's weapon modifier applied (`EntityLivingBase.onUpdate` swaps it
    /// whenever the held stack changes).
    fn player_attack_damage(&self) -> f32 {
        let mut a = self.splayer.living.attrs.attack_damage.clone();
        if let Some(d) = self
            .play
            .inv
            .held()
            .and_then(|s| crate::item::weapon_damage(s.id))
        {
            // Item.field_111210_e, the one UUID every weapon shares
            a.apply(crate::entity::living::Modifier {
                uuid: 0xCB3F55D3_645C_4F38_A497_9C13A33DB5CF,
                amount: d as f64,
                op: 0,
            });
        }
        a.value() as f32
    }

    /// The looting part of `EntityLiving.onLivingUpdate`: a mob that can
    /// pick up loot takes an item within a block sideways of its box into
    /// the slot `getArmorPosition` gives, when the slot is empty or the item
    /// is better (a sword over anything but a sword, a sharper sword,
    /// armour over non-armour, more protection; on a tie the more worn or
    /// the enchanted one), dropping what it had by its drop chance - 0.1.
    fn mobs_pick_up_loot(&mut self, play: &mut Play) {
        use crate::entity::player::Aabb;
        use crate::item::{tool, ToolKind};
        // getArmorPosition
        let slot_of = |id: u16| -> usize {
            if id == crate::block::PUMPKIN as u16 || id == 397 {
                return 4;
            }
            match crate::container::armor_type(id) {
                Some(t) => 4 - t,
                None => 0,
            }
        };
        let sword = |id: u16| tool(id).is_some_and(|t| t.kind == ToolKind::Sword);
        let mut drops: Vec<([f64; 3], crate::item::Stack)> = Vec::new();
        for mi in 0..self.mobs.list.len() {
            let (bb, ok) = {
                let m = &self.mobs.list[mi];
                (
                    m.bb,
                    // EntityLiving.onLivingUpdate: canPickUpLoot, alive,
                    // mobGriefing
                    m.can_pick_up_loot
                        && m.health > 0.0
                        && m.death_time == 0
                        && crate::gamerules::bool("mobGriefing"),
                )
            };
            if !ok {
                continue;
            }
            let reach = Aabb {
                min: [bb.min[0] - 1.0, bb.min[1], bb.min[2] - 1.0],
                max: [bb.max[0] + 1.0, bb.max[1], bb.max[2] + 1.0],
            };
            for ii in 0..play.items.len() {
                let e = &play.items[ii];
                if e.e.dead || !e.bb().intersects(&reach) {
                    continue;
                }
                let st = e.stack;
                let slot = slot_of(st.id);
                let m = &mut self.mobs.list[mi];
                let better = match m.equipment[slot] {
                    None => true,
                    Some(c) => {
                        let tie = || {
                            st.meta > c.meta
                                || st.tag != crate::item_tag::Tag::NONE
                                    && c.tag == crate::item_tag::Tag::NONE
                        };
                        if slot == 0 {
                            if sword(st.id) && !sword(c.id) {
                                true
                            } else if sword(st.id) && sword(c.id) {
                                let (a, b) = (
                                    crate::item::weapon_damage(st.id).unwrap_or(0.0),
                                    crate::item::weapon_damage(c.id).unwrap_or(0.0),
                                );
                                if a == b {
                                    tie()
                                } else {
                                    a > b
                                }
                            } else {
                                false
                            }
                        } else {
                            let pa = crate::inventory::armor_points(st.id);
                            let pb = crate::inventory::armor_points(c.id);
                            match (pa, pb) {
                                (Some(_), None) => true,
                                (Some(a), Some(b)) => {
                                    if a == b {
                                        tie()
                                    } else {
                                        a > b
                                    }
                                }
                                _ => false,
                            }
                        }
                    }
                };
                if !better {
                    continue;
                }
                if let Some(c) = m.equipment[slot] {
                    if m.replaced_drops(slot) {
                        drops.push((m.pos, c));
                    }
                }
                m.equipment[slot] = Some(st);
                m.drop_chances[slot] = 2.0;
                m.persistent = true;
                let id = m.id;
                let e = &mut play.items[ii];
                play.mob_collected.push((
                    crate::play::ItemView {
                        id: e.id,
                        stack: st,
                        pos: e.e.pos,
                        prev: e.e.prev,
                        age: e.age,
                        hover: e.hover,
                    },
                    id,
                ));
                e.e.dead = true;
            }
        }
        // Entity.entityDropItem(stack, 0): at the feet, 10 ticks before it
        // can be picked up
        for (p, st) in drops {
            let id = play.next_item;
            play.next_item += 1;
            let mut e = crate::entity::item::ItemEntity::new(id, st, p);
            e.pickup_delay = 10;
            play.spawn(e);
        }
    }

    /// `EntityPlayer.attackTargetEntityWithCurrentItem` against a mob.
    fn attack_mob(&mut self, id: u32, crit: bool, sprint: bool) {
        use crate::enchant;
        use crate::entity::mob::Kind;
        let held = self.play.inv.held();
        let Some(m) = self.mobs.list.iter().find(|m| m.id == id) else {
            return;
        };
        let kind = m.kind;
        // The dragon is hit through an EntityDragonPart and the crystal is
        // not an EntityLivingBase: neither gets the living-only parts.
        let living = !matches!(kind, Kind::EnderDragon | Kind::EnderCrystal);
        let mut f = self.player_attack_damage();
        let mut kb = 0;
        let mut ench = 0.0f32;
        if living {
            ench = enchant::modifier_living(held.as_ref(), kind.creature());
            kb += enchant::level(enchant::KNOCKBACK, held.as_ref());
        }
        if sprint {
            kb += 1;
        }
        if !(f > 0.0 || ench > 0.0) {
            return;
        }
        let crit = crit && living && !self.splayer.living.active(crate::potion::BLINDNESS);
        if crit && f > 0.0 {
            f *= 1.5;
        }
        f += ench;
        let fire = enchant::level(enchant::FIRE_ASPECT, held.as_ref());
        let mut lit = false;
        if living && fire > 0 {
            if let Some(m) = self.mobs.list.iter_mut().find(|m| m.id == id) {
                if !m.burning() {
                    lit = true;
                    m.set_fire(1);
                }
            }
        }
        let mut out = crate::entity::mob::Outcome::default();
        let (feet, yaw) = self.play.player;
        let hit = self.mobs.attack(id, f, feet, yaw, &mut out);
        if hit && f >= 18.0 {
            // attackTargetEntityWithCurrentItem: nine hearts in one blow
            self.play.achievements.push("achievement.overkill");
        }
        if hit {
            self.stats_file.increase_stat("stat.damageDealt", (f * 10.0).round() as i32);
            // EntityPlayer.onKillEntity / addStat(mobKills), killEntity.<name>
            if let Some(m) = self.mobs.list.iter().find(|m| m.id == id) {
                if m.health <= 0.0 && living {
                    // EntityPlayer.onKillEntity: a monster is killEnemy
                    let monster = m.kind.hostile()
                        || matches!(m.kind, Kind::Slime | Kind::MagmaCube | Kind::Ghast | Kind::Blaze | Kind::Enderman | Kind::Silverfish | Kind::PigZombie | Kind::Wither);
                    if monster {
                        self.play.achievements.push("achievement.killEnemy");
                    }
                    self.stats_file.increase_stat("stat.mobKills", 1);
                    let k = format!("stat.killEntity.{}", m.kind.id());
                    self.stats_file.increase_stat(&k, 1);
                }
            }
            if kb > 0 {
                self.mobs.add_velocity(
                    id,
                    [
                        (-crate::util::math::sin(yaw * std::f32::consts::PI / 180.0)
                            * kb as f32
                            * 0.5) as f64,
                        0.1,
                        (crate::util::math::cos(yaw * std::f32::consts::PI / 180.0)
                            * kb as f32
                            * 0.5) as f64,
                    ],
                );
                // motionX/Z *= 0.6 on the server's copy; setSprinting(false)
                // reaches the client through the entity metadata
                self.splayer.e.motion[0] *= 0.6;
                self.splayer.e.motion[2] *= 0.6;
                self.stop_sprinting = true;
            }
            // onCriticalHit / onEnchantmentCritical: S0BPacketAnimation 4 / 5
            if crit {
                self.crit_fx.push((id, false));
            }
            if ench > 0.0 {
                self.crit_fx.push((id, true));
            }
            self.last_attacked = Some(id);
            self.last_attacked_time = self.world.total_time as i32;
            if living {
                // applyThornEnchantments(target, player): the target's
                // armour may carry Thorns (enchantEquipment)
                let wearer = self
                    .mobs
                    .list
                    .iter_mut()
                    .find(|m| m.id == id)
                    .map(|m| (m.thorns_hits(), m.pos));
                if let Some((hits, pos)) = wearer {
                    let d = self.difficulty;
                    for dmg in hits {
                        let src = crate::entity::living::Source::thorns(id, pos);
                        self.splayer.attack(&self.world, src, dmg, d);
                    }
                }
                // applyArthropodEnchantments(player, target): Bane of
                // Arthropods slows the target, drawing from the player's rand
                for d in enchant::arthropod_slowness(
                    held.as_ref(),
                    kind.creature(),
                    &mut self.splayer.e.rand,
                ) {
                    self.mobs.add_effect(id, crate::potion::MOVE_SLOWDOWN, d, 3);
                }
            }
            // ItemStack.hitEntity on the mob (the dragon itself for a part)
            if kind != Kind::EnderCrystal {
                if let Some(s) = held {
                    let cost = match crate::item::tool(s.id).map(|t| t.kind) {
                        Some(crate::item::ToolKind::Sword) => 1,
                        Some(crate::item::ToolKind::Hoe) | None => 0,
                        Some(_) => 2,
                    };
                    if cost > 0 {
                        let creative = self.play.mode == GameMode::Creative;
                        let (left, broke) =
                            enchant::damage_item(s, cost, creative, &mut self.splayer.e.rand);
                        if broke {
                            self.splayer.fx.push(crate::entity::base::Fx::BrokenItem(s));
                        }
                        let i = self.play.inv.selected;
                        self.play.inv.main[i] = left;
                        self.play.inv_dirty = true;
                    }
                }
            }
            if living {
                if fire > 0 {
                    if let Some(m) = self.mobs.list.iter_mut().find(|m| m.id == id) {
                        m.set_fire(fire * 4);
                    }
                }
                // the killer's looting for onDeath
                if let Some(m) = self.mobs.list.iter_mut().find(|m| m.id == id) {
                    m.looting = enchant::level(enchant::LOOTING, held.as_ref());
                }
            }
            self.splayer.food.add_exhaustion(0.3);
        } else if lit {
            if let Some(m) = self.mobs.list.iter_mut().find(|m| m.id == id) {
                m.fire = 0;
            }
        }
        self.pending_out.push(out);
    }

    /// Copies the worn pieces into the server player before anything can
    /// hurt it (`getLastActiveItems`, `getTotalArmorValue`).
    fn armor_in(&mut self) {
        self.armor_out();
        self.splayer.armor_items = self.play.inv.armor;
        self.splayer.armor_updated();
    }

    /// Writes back what hits did to the worn pieces.
    fn armor_out(&mut self) {
        if std::mem::take(&mut self.splayer.armor_changed) {
            self.play.inv.armor = self.splayer.armor_items;
            self.play.inv_dirty = true;
        }
        self.splayer.armor_items = self.play.inv.armor;
        self.splayer.armor_updated();
    }

    /// The player's state for the client, and the consequences of dying.
    fn player_events(&mut self, events: &mut Vec<Event>) {
        // the worn pieces as damage left them
        self.armor_out();
        let fx = std::mem::take(&mut self.splayer.fx);
        let mut sounds = Vec::new();
        let mut parts = Vec::new();
        let mut dust = Vec::new();
        for f in fx {
            match f {
                crate::entity::base::Fx::Sound(n, p, v, pi) => sounds.push((n, p, v, pi)),
                crate::entity::base::Fx::Particle(n, p, m) => parts.push((n, p, m)),
                crate::entity::base::Fx::BrokenItem(b) => {
                    // ItemStack.damageItem: addStat(objectBreakStats)
                    self.stats_file.increase_stat(&format!("stat.breakItem.{}", b.id), 1);
                    // renderBrokenItemStack: playSound("random.break", 0.8,
                    // 0.8 + worldObj.rand.nextFloat() * 0.4); its particles
                    // are client-side and the server's do nothing
                    let p = 0.8 + self.world.rand.next_float() * 0.4;
                    sounds.push(("random.break", self.splayer.e.pos, 0.8, p));
                }
                // BlockFarmland.onFallenUpon: trampled by a player
                crate::entity::base::Fx::Landed(x, y, z, id, fall) => {
                    if id == crate::block::FARMLAND && self.world.rand.next_float() < fall - 0.5 {
                        self.world.set_block(x, y, z, crate::block::DIRT, 0, 3);
                    }
                }
                crate::entity::base::Fx::Dust(x, y, z, n) => dust.push(([x, y, z], n)),
                _ => {}
            }
        }
        if !sounds.is_empty() {
            events.push(Event::Sounds(sounds));
        }
        if !parts.is_empty() {
            events.push(Event::Particles(parts));
        }
        if !dust.is_empty() {
            events.push(Event::FallDust(dust));
        }
        if std::mem::take(&mut self.splayer.hurt_anim) {
            events.push(Event::Hurt);
        }
        if let Some(p) = self.teleport_to.take() {
            self.play.player.0 = p;
            events.push(Event::Teleport(p));
        }
        if let Some(p) = self.play.sign_open.take() {
            events.push(Event::OpenSign(p));
        }
        for (p, t) in std::mem::take(&mut self.play.sign_updates) {
            events.push(Event::SignText(p, t));
        }
        for (p, k, r) in std::mem::take(&mut self.play.skull_updates) {
            events.push(Event::Skull(p, k, r));
        }
        // BlockSkull.makeWither: the wither, charging, and the snow puffs
        for (at, yaw) in std::mem::take(&mut self.play.wither_spawns) {
            let mut r = JavaRandom::new(self.world.rand.next_long());
            let id = self.mobs.spawn(crate::entity::mob::Kind::Wither, at, yaw, &mut r);
            if let Some(m) = self.mobs.list.iter_mut().find(|m| m.id == id) {
                m.yaw = yaw;
                m.body_yaw = yaw;
                m.head_yaw = yaw;
                m.persistent = true;
                m.wither_ignite();
            }
            if self.stats_file.trigger_achievement("achievement.spawnWither") {
                let dirty = self.stats_file.take_dirty();
                self.stats_out.extend(dirty);
            }
            let mut puffs = Vec::new();
            for _ in 0..120 {
                puffs.push((
                    "snowballpoof",
                    [
                        at[0] - 0.5 + self.world.rand.next_double(),
                        at[1] - 0.55 + self.world.rand.next_double() * 3.9,
                        at[2] - 0.5 + self.world.rand.next_double(),
                    ],
                    [0.0; 3],
                ));
            }
            self.pending_out.push(crate::entity::mob::Outcome {
                particles: puffs,
                ..Default::default()
            });
        }
        if std::mem::take(&mut self.mode_changed) {
            events.push(Event::GameMode(self.play.mode));
        }
        if !self.chat_out.is_empty() {
            events.push(Event::Chat(std::mem::take(&mut self.chat_out)));
        }
        if !self.stats_out.is_empty() {
            events.push(Event::Statistics(std::mem::take(&mut self.stats_out)));
        }
        let splashes = std::mem::take(&mut self.mobs.splashes);
        if !splashes.is_empty() {
            events.push(Event::Splashes(splashes));
        }
        for (id, magic) in std::mem::take(&mut self.crit_fx) {
            events.push(Event::CritFx(id, magic));
        }
        for p in std::mem::take(&mut self.bone_meal_fx) {
            events.push(Event::BoneMeal(p));
        }
        if let Some(s) = self.sleep_event.take() {
            if let Some(e) = self.wake_exit.take() {
                events.push(Event::Teleport(e));
            }
            events.push(Event::Sleep(s));
        }
        if std::mem::take(&mut self.stop_sprinting) {
            events.push(Event::StopSprinting);
        }
        if std::mem::take(&mut self.using_changed) {
            events.push(Event::Using(self.splayer.using.map(|(_, st, n)| (st, n))));
        }
        let set = self.splayer.velocity.take();
        let add = std::mem::take(&mut self.push_add);
        if set.is_some() || add != [0.0; 3] {
            events.push(Event::Velocity { set, add });
        }
        if self.splayer.dead && !self.splayer.death_reported {
            self.splayer.death_reported = true;
            self.drop_inventory();
            let names = |s: &crate::entity::living::Source| -> Option<String> {
                use crate::entity::living::SourceKind as K;
                match s.kind {
                    K::Mob(k) => Some(format!("entity.{}.name", k.id())),
                    K::Arrow if s.attacker.is_some() => Some("entity.Skeleton.name".into()),
                    K::Fireball if s.attacker.is_some() => Some("entity.Ghast.name".into()),
                    _ => None,
                }
            };
            let msg = self.splayer.death_message(&names);
            events.push(Event::Died(msg));
        }
        let v = self.splayer.view();
        if self.splayer.last_sent.as_ref() != Some(&v) {
            self.splayer.last_sent = Some(v.clone());
            events.push(Event::PlayerStats(v));
        }
    }

    /// `TileEntityPiston.launchWithSlimeBlock`: every entity in a moving
    /// block's box is moved with it (`Entity.moveEntity`).
    fn apply_pushes(&mut self, play: &mut Play, events: &mut Vec<Event>) {
        let pushes = std::mem::take(&mut self.world.pushes);
        if pushes.is_empty() {
            return;
        }
        let mut to_player = Vec::new();
        for (bb, d) in pushes {
            let b = crate::entity::player::Aabb {
                min: bb.min,
                max: bb.max,
            };
            let (f, _) = play.player;
            let pb = crate::entity::player::Aabb {
                min: [f[0] - 0.3, f[1], f[2] - 0.3],
                max: [f[0] + 0.3, f[1] + 1.8, f[2] + 0.3],
            };
            if !self.respawn_pending && pb.intersects(&b) {
                to_player.push((bb.min, bb.max, d));
            }
            for m in self.mobs.list.iter_mut() {
                if !m.dead && m.bb.intersects(&b) {
                    m.push_move(&self.world, d[0], d[1], d[2]);
                }
            }
            let mut fx = Vec::new();
            for it in play.items.iter_mut() {
                if !it.e.dead && it.e.bb.intersects(&b) {
                    it.e.move_entity(
                        &self.world as &dyn crate::world::BlockView,
                        d[0],
                        d[1],
                        d[2],
                        false,
                        &[],
                        &mut fx,
                    );
                }
            }
            for f in fx {
                if let crate::entity::base::Fx::Sound(..) = f {
                    play.fx.push(f);
                }
            }
        }
        if !to_player.is_empty() {
            events.push(Event::Push(to_player));
        }
    }

    /// Order-independent digest of the whole simulation state.
    pub fn state_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |bytes: &[u8]| {
            for &b in bytes {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        let mut keys: Vec<(i32, i32)> = self.world.chunks.keys().copied().collect();
        keys.sort();
        for key in keys {
            let c = &self.world.chunks[&key];
            mix(&key.0.to_le_bytes());
            mix(&key.1.to_le_bytes());
            mix(&c.data.blocks[..]);
            mix(&c.data.meta[..]);
            mix(&c.data.sky[..]);
            mix(&c.data.blk[..]);
            mix(&c.sections.to_le_bytes());
            mix(&[c.terrain_populated as u8, c.light_populated as u8]);
        }
        for t in self.world.pending_tick_list() {
            mix(&t.0.to_le_bytes());
            mix(&t.1.to_le_bytes());
            mix(&t.2.to_le_bytes());
            mix(&[t.3]);
            mix(&t.4.to_le_bytes());
        }
        mix(&self.world.total_time.to_le_bytes());
        mix(&self.world_time.to_le_bytes());
        h
    }
}

/// `World.calculateSkylightSubtracted(1.0)` for a world time.
/// `World.calculateSkylightSubtracted` with rain and thunder.
pub fn skylight_subtracted_weather(world_time: i64, rain: f32, thunder: f32) -> i32 {
    let f = celestial_angle(world_time, 1.0);
    let mut f1 =
        1.0f32 - (crate::util::math::cos(f * crate::util::math::PI * 2.0f32) * 2.0f32 + 0.5f32);
    f1 = f1.clamp(0.0, 1.0);
    f1 = 1.0 - f1;
    f1 = (f1 as f64 * (1.0 - (rain * 5.0f32) as f64 / 16.0)) as f32;
    f1 = (f1 as f64 * (1.0 - (thunder * 5.0f32) as f64 / 16.0)) as f32;
    f1 = 1.0 - f1;
    (f1 * 11.0f32) as i32
}

pub fn skylight_subtracted(world_time: i64) -> i32 {
    let f = celestial_angle(world_time, 1.0);
    let mut f1 =
        1.0f32 - (crate::util::math::cos(f * crate::util::math::PI * 2.0f32) * 2.0f32 + 0.5f32);
    f1 = f1.clamp(0.0, 1.0);
    f1 = 1.0 - f1;
    f1 = 1.0 - f1;
    (f1 * 11.0f32) as i32
}

/// `WorldProvider.calculateCelestialAngle`
pub fn celestial_angle(world_time: i64, partial: f32) -> f32 {
    let j = (world_time % 24000) as i32;
    let mut f1 = (j as f32 + partial) / 24000.0f32 - 0.25f32;
    if f1 < 0.0 {
        f1 += 1.0;
    }
    if f1 > 1.0 {
        f1 -= 1.0;
    }
    let f2 = f1;
    f1 = 1.0f32 - (((f1 as f64 * std::f64::consts::PI).cos() + 1.0) / 2.0) as f32;
    f2 + (f1 - f2) / 3.0f32
}

pub struct SimHandle {
    pub commands: Sender<Command>,
    pub events: Receiver<Event>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl SimHandle {
    /// Stops the server thread; it saves the world before exiting.
    pub fn shutdown(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for SimHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// `Command::Dig` and `Command::Use` handled so far, by any world of this
/// process: the client numbers the ones it sends the same way.
pub static ACTIONS_DONE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Server ticks run and the nanoseconds they took (the F3 graph's
/// `tickTimeArray`; read by the benchmark).
pub static TICK_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static TICK_NANOS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static TICK_WORST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Runs the simulation on its own thread at 20 ticks per second.
pub fn spawn(mut sim: Sim) -> SimHandle {
    let (cmd_tx, cmd_rx) = channel::<Command>();
    let (ev_tx, ev_rx) = channel::<Event>();
    let thread = std::thread::Builder::new()
        .name("server".into())
        .stack_size(16 << 20)
        .spawn(move || {
            let tick = Duration::from_millis(50);
            let mut next = Instant::now();
            let mut events = Vec::new();
            loop {
                let mut commands = Vec::new();
                let mut stop = false;
                loop {
                    match cmd_rx.try_recv() {
                        Ok(Command::Shutdown) => stop = true,
                        Ok(c) => commands.push(c),
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            stop = true;
                            break;
                        }
                    }
                }
                if stop {
                    for c in &commands {
                        sim.apply(c);
                    }
                    if let Err(e) = sim.save() {
                        eprintln!("failed to save the world: {e}");
                    }
                    return;
                }
                let began = Instant::now();
                sim.step(&commands, &mut events);
                {
                    use std::sync::atomic::Ordering::Relaxed;
                    let ns = began.elapsed().as_nanos() as u64;
                    TICK_COUNT.fetch_add(1, Relaxed);
                    TICK_NANOS.fetch_add(ns, Relaxed);
                    TICK_WORST.fetch_max(ns, Relaxed);
                }
                events.push(Event::Tick(
                    Instant::now(),
                    ACTIONS_DONE.load(std::sync::atomic::Ordering::Relaxed),
                ));
                for e in events.drain(..) {
                    if ev_tx.send(e).is_err() {
                        let _ = sim.save();
                        return;
                    }
                }
                next += tick;
                let now = Instant::now();
                if next > now {
                    std::thread::sleep(next - now);
                } else if now - next > Duration::from_secs(1) {
                    // Hopelessly behind (initial load): do not try to catch up.
                    next = now;
                }
            }
        })
        .expect("failed to start the server thread");
    SimHandle {
        commands: cmd_tx,
        events: ev_rx,
        thread: Some(thread),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(seed: i64, ticks: usize) -> (u64, usize) {
        let cfg = SimConfig {
            seed,
            view: 2,
            save_dir: None,
            start_time: 0,
            freeze_time: false,
            mode: GameMode::Creative,
        };
        let (mut sim, start) = Sim::new(&cfg);
        let (cx, cz) = (
            (start.pos[0].floor() as i32) >> 4,
            (start.pos[2].floor() as i32) >> 4,
        );
        let (bx, by, bz) = (
            start.pos[0].floor() as i32,
            start.pos[1] as i32,
            start.pos[2].floor() as i32,
        );
        let mut events = Vec::new();
        let mut count = 0;
        for t in 0..ticks {
            // A scripted session: wander, dig a shaft, pour water, drop sand.
            let mut cmds = vec![Command::Center(cx + (t as i32 / 40), cz - (t as i32 / 60))];
            if t >= 10 && t < 20 {
                cmds.push(Command::Break([bx, by - 1 - (t as i32 - 10), bz]));
            }
            if t == 30 {
                cmds.push(Command::Place {
                    pos: [bx + 1, by + 3, bz],
                    id: block::STONE,
                    meta: 0,
                });
                cmds.push(Command::Place {
                    pos: [bx + 1, by + 4, bz],
                    id: block::FLOWING_WATER,
                    meta: 0,
                });
            }
            if t == 35 {
                cmds.push(Command::Place {
                    pos: [bx - 2, by + 6, bz],
                    id: block::SAND,
                    meta: 0,
                });
                cmds.push(Command::Break([bx + 1, by + 3, bz]));
            }
            sim.step(&cmds, &mut events);
            count += events.len();
            events.clear();
        }
        (sim.state_hash(), count)
    }

    /// Gate G1: the same seed and the same input script give the same world,
    /// run after run, regardless of how the generation threads are scheduled.
    #[test]
    fn replay_is_deterministic() {
        let a = run(2026, 120);
        let b = run(2026, 120);
        let c = run(2026, 120);
        assert_eq!(a, b);
        assert_eq!(a, c);
        assert_ne!(a.0, run(2027, 120).0, "different seeds must differ");
    }

    /// Saving and reloading mid-session must not change the outcome.
    #[test]
    fn save_and_reload_round_trips() {
        let dir = std::env::temp_dir().join(format!("minerust-sim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cfg = SimConfig {
            seed: 77,
            view: 2,
            save_dir: Some(dir.clone()),
            start_time: 0,
            freeze_time: false,
            mode: GameMode::Creative,
        };
        let mut events = Vec::new();
        let (hash_before, pos) = {
            let (mut sim, start) = Sim::new(&cfg);
            let (bx, by, bz) = (
                start.pos[0].floor() as i32,
                start.pos[1] as i32,
                start.pos[2].floor() as i32,
            );
            for t in 0..60 {
                let cmds = if t == 20 {
                    vec![
                        Command::Break([bx, by - 1, bz]),
                        Command::Place {
                            pos: [bx, by + 2, bz],
                            id: block::COBBLESTONE,
                            meta: 0,
                        },
                    ]
                } else {
                    vec![]
                };
                sim.step(&cmds, &mut events);
                events.clear();
            }
            sim.save().unwrap();
            (sim.state_hash(), [bx, by, bz])
        };
        let (mut sim, start) = Sim::new(&cfg);
        assert!(start.resumed);
        assert_eq!(sim.world.total_time, 60);
        // Bring the same chunks back and compare block for block.
        sim.step(&[], &mut events);
        assert_eq!(sim.world.block(pos[0], pos[1] - 1, pos[2]), block::AIR);
        assert_eq!(
            sim.world.block(pos[0], pos[1] + 2, pos[2]),
            block::COBBLESTONE
        );
        let _ = hash_before;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn failed_world_save_recovery() {
        use std::fs;
        for (anvil, subdir) in [(false, "chunks"), (true, "region")] {
            let dir = std::env::temp_dir().join(format!(
                "minerust-save-retry-{}-{subdir}",
                std::process::id()
            ));
            fs::create_dir_all(&dir).unwrap();
            let cfg = SimConfig {
                seed: 77,
                view: 2,
                save_dir: Some(dir.clone()),
                start_time: 0,
                freeze_time: true,
                mode: GameMode::Creative,
            };
            let (mut sim, _) = Sim::new(&cfg);
            sim.world.anvil = anvil;
            for x in [0, 16] {
                sim.world.load_chunk(x >> 4, 0);
                sim.world.set_block(x, 200, 0, block::STONE, 0, 2);
                sim.unsaved.insert((x >> 4, 0));
            }
            let blocked = dir.join(subdir);
            fs::write(&blocked, b"blocks chunk writes").unwrap();
            assert!(sim.save().is_err());
            assert!(sim.unsaved.contains(&(0, 0)));
            assert!(sim.unsaved.contains(&(1, 0)));

            sim.centre = (100, 100);
            sim.unload_far();
            assert!(sim.world.chunk_exists(0, 0));
            assert!(sim.world.chunk_exists(1, 0));
            assert!(sim.unsaved.contains(&(0, 0)));

            fs::remove_file(blocked).unwrap();
            // This change happened after the last tick had reported changes.
            sim.world.load_chunk(2, 0);
            sim.world.set_block(32, 200, 0, block::STONE, 0, 2);
            assert!(!sim.unsaved.contains(&(2, 0)));
            sim.save().unwrap();
            assert!(sim.unsaved.is_empty());
            let (mut restored, _) = Sim::new(&cfg);
            for x in [0, 16, 32] {
                restored.world.load_chunk(x >> 4, 0);
                assert_eq!(restored.world.block(x, 200, 0), block::STONE);
            }

            sim.world.set_block(0, 200, 0, block::COBBLESTONE, 0, 2);
            sim.unsaved.insert((0, 0));
            if anvil {
                sim.play
                    .falling
                    .push(crate::entity::falling::FallingBlock::new(
                        1,
                        [0.5, 201.0, 0.5],
                        block::SAND,
                        0,
                    ));
            }
            sim.travel(1, &mut Vec::new());
            sim.save().unwrap();
            let (mut restored, _) = Sim::new(&cfg);
            let mut overworld = restored.take_world(0).world;
            overworld.load_chunk(0, 0);
            assert_eq!(overworld.block(0, 200, 0), block::COBBLESTONE);
            if anvil {
                assert!(!save::chunk_path(&dir, 0, 0).exists());
                let (_, _, _, entities) = overworld.anvil_loaded.pop().unwrap();
                assert!(entities
                    .iter()
                    .any(|tag| tag.string("id") == Some("FallingSand")));
            }
            fs::remove_dir_all(dir).unwrap();
        }
    }

    /// Survival loop: dig with a pickaxe, the drop is picked up, placing
    /// consumes it, and the crafting grid works through the server.
    #[test]
    fn spawn_area_is_prepared_like_initial_world_chunk_load() {
        let cfg = SimConfig {
            seed: 12345,
            view: 2,
            save_dir: None,
            start_time: 0,
            freeze_time: true,
            mode: GameMode::Survival,
        };
        let (mut sim, _) = Sim::new(&cfg);
        sim.initial_world_chunk_load();
        let (sx, sz) = (sim.spawn[0].floor() as i32, sim.spawn[2].floor() as i32);
        for x in (-192..=192).step_by(16) {
            for z in (-192..=192).step_by(16) {
                assert!(sim.world.chunk_exists((sx + x) >> 4, (sz + z) >> 4));
            }
        }
    }

    #[test]
    fn survival_dig_pickup_place_and_craft() {
        let cfg = SimConfig {
            seed: 2026,
            view: 2,
            save_dir: None,
            start_time: 0,
            freeze_time: true,
            mode: GameMode::Survival,
        };
        let (mut sim, start) = Sim::new(&cfg);
        let mut ev = Vec::new();
        for _ in 0..3 {
            sim.step(&[], &mut ev);
        }
        let (bx, by, bz) = (
            start.pos[0].floor() as i32,
            start.pos[1] as i32,
            start.pos[2].floor() as i32,
        );
        let below = [bx, by - 1, bz];
        // Make sure there is stone to dig right under the player.
        sim.world
            .set_block(below[0], below[1], below[2], block::STONE, 0, 3);
        let feet = [start.pos[0], start.pos[1], start.pos[2]];
        let pick = crate::item::Stack::new(270, 0, 1);
        sim.step(
            &[
                Command::Player(feet, [0.0, 0.0], false),
                Command::Give(pick),
                Command::Select(0),
                Command::Dig(below),
            ],
            &mut ev,
        );
        assert_eq!(sim.world.block(below[0], below[1], below[2]), block::AIR);
        assert_eq!(
            sim.play.inv.main[0].map(|s| s.meta),
            Some(1),
            "pickaxe loses durability"
        );
        let mut picked = false;
        // The player drops into the hole it dug.
        let in_hole = [feet[0], feet[1] - 1.0, feet[2]];
        for _ in 0..60 {
            sim.step(&[Command::Player(in_hole, [0.0, 0.0], false)], &mut ev);
            if sim
                .play
                .inv
                .main
                .iter()
                .flatten()
                .any(|s| s.id == block::COBBLESTONE as u16)
            {
                picked = true;
                break;
            }
        }
        assert!(picked, "cobblestone was not picked up");
        assert!(sim.play.items.is_empty());
        // Digging stone by hand yields nothing.
        let side = [bx + 2, by - 1, bz];
        sim.world
            .set_block(side[0], side[1], side[2], block::STONE, 0, 3);
        sim.step(&[Command::Select(8), Command::Dig(side)], &mut ev);
        assert_eq!(sim.world.block(side[0], side[1], side[2]), block::AIR);
        assert!(sim.play.items.is_empty(), "stone dug by hand must not drop");
        // Place the cobblestone back: the stack shrinks.
        let slot = sim
            .play
            .inv
            .main
            .iter()
            .position(|s| matches!(s, Some(s) if s.id == block::COBBLESTONE as u16))
            .unwrap();
        sim.world.set_block(bx + 3, by - 1, bz, block::STONE, 0, 3);
        sim.world.set_block(bx + 3, by, bz, block::AIR, 0, 3);
        sim.step(
            &[
                Command::Select(slot),
                Command::Use {
                    target: [bx + 3, by - 1, bz],
                    face: [0, 1, 0],
                    sneaking: false,
                    hit: [0.5, 1.0, 0.5],
                },
            ],
            &mut ev,
        );
        assert_eq!(sim.world.block(bx + 3, by, bz), block::COBBLESTONE);
        assert!(sim.play.inv.main[slot].is_none());
        // Craft planks from a log in the 2x2 grid.
        sim.step(
            &[
                Command::Give(crate::item::Stack::new(block::LOG as u16, 0, 1)),
                Command::OpenInventory,
            ],
            &mut ev,
        );
        let log_slot = sim
            .play
            .inv
            .main
            .iter()
            .position(|s| matches!(s, Some(s) if s.id == block::LOG as u16))
            .unwrap();
        sim.step(
            &[
                // ContainerPlayer: hotbar slots are 36..45, the grid 1..5
                Command::WindowClick {
                    slot: if log_slot < 9 {
                        36 + log_slot
                    } else {
                        log_slot
                    } as i32,
                    button: 0,
                    mode: 0,
                },
                Command::WindowClick {
                    slot: 1,
                    button: 0,
                    mode: 0,
                },
                Command::WindowClick {
                    slot: 0,
                    button: 0,
                    mode: 1,
                },
                Command::CloseWindow,
            ],
            &mut ev,
        );
        assert_eq!(
            sim.play
                .inv
                .main
                .iter()
                .flatten()
                .filter(|s| s.id == block::PLANKS as u16)
                .map(|s| s.count)
                .sum::<u32>(),
            4
        );
    }

    #[test]
    #[ignore]
    fn mob_body_follows_walking() {
        let cfg = SimConfig {
            seed: 12345,
            view: 3,
            save_dir: None,
            start_time: 6000,
            freeze_time: true,
            mode: GameMode::Creative,
        };
        let (mut sim, start) = Sim::new(&cfg);
        let mut ev = Vec::new();
        for _ in 0..40 {
            sim.step(&[], &mut ev);
        }
        let p = start.pos;
        let id = 900000;
        sim.mobs.list.push(crate::entity::mob::Mob::new(
            id,
            crate::entity::mob::Kind::Cow,
            [p[0] + 3.0, p[1] + 1.0, p[2] + 3.0],
            0.0,
            7,
        ));
        for t in 0..600 {
            sim.step(&[], &mut ev);
            if let Some(m) = sim.mobs.list.iter().find(|m| m.id == id) {
                let (dx, dz) = (m.pos[0] - m.prev_pos[0], m.pos[2] - m.prev_pos[2]);
                if dx * dx + dz * dz > 1e-6 {
                    let dir = (dz.atan2(dx) * 180.0 / std::f64::consts::PI) as f32 - 90.0;
                    eprintln!(
                        "{t} pos {:.3} {:.3} move {:.1} yaw {:.1} body {:.1} head {:.1}",
                        m.pos[0], m.pos[2], dir, m.yaw, m.body_yaw, m.head_yaw
                    );
                }
            }
        }
    }

    fn redstone_sim() -> (Sim, i32, i32, i32) {
        let cfg = SimConfig {
            seed: 4242,
            view: 2,
            save_dir: None,
            start_time: 6000,
            freeze_time: true,
            mode: GameMode::Creative,
        };
        let (mut sim, start) = Sim::new(&cfg);
        let mut ev = Vec::new();
        // Let the spawn area finish generating and drain the gravel ticks
        // ore population schedules, as the spawn preparation does.
        for _ in 0..400 {
            sim.step(&[], &mut ev);
            if sim.world.pending_ticks() < 200 {
                break;
            }
        }
        let (x, z) = (
            start.pos[0].floor() as i32 + 2,
            start.pos[2].floor() as i32 - 6,
        );
        let y = 200;
        for dx in -2..12 {
            for dz in -2..14 {
                sim.world.set_block(x + dx, y, z + dz, block::STONE, 0, 3);
            }
        }
        (sim, x, y + 1, z)
    }

    fn steps(sim: &mut Sim, n: usize) {
        let mut ev = Vec::new();
        for _ in 0..n {
            sim.step(&[], &mut ev);
        }
    }

    #[test]
    fn lever_wire_lamp_and_torch_inverter() {
        let (mut sim, x, y, z) = redstone_sim();
        let w = &mut sim.world;
        w.set_block(x, y, z, block::LEVER, 5, 3);
        for i in 1..4 {
            w.set_block(x + i, y, z, block::REDSTONE_WIRE, 0, 3);
        }
        w.set_block(x + 4, y, z, block::REDSTONE_LAMP, 0, 3);
        w.toggle_lever(x, y, z);
        assert_eq!(w.meta(x + 1, y, z), 15);
        assert_eq!(w.meta(x + 3, y, z), 13);
        assert_eq!(w.block(x + 4, y, z), block::LIT_REDSTONE_LAMP);
        w.toggle_lever(x, y, z);
        assert_eq!(w.meta(x + 1, y, z), 0);
        assert_eq!(
            w.block(x + 4, y, z),
            block::LIT_REDSTONE_LAMP,
            "lamps go out after 4 ticks"
        );
        steps(&mut sim, 5);
        assert_eq!(sim.world.block(x + 4, y, z), block::REDSTONE_LAMP);

        // A torch on the side of a block, inverted by a lever on top.
        let w = &mut sim.world;
        let (sx, sz) = (x, z + 3);
        w.set_block(sx, y, sz, block::STONE, 0, 3);
        w.set_block(sx + 1, y, sz, block::REDSTONE_TORCH, 1, 3);
        w.set_block(sx, y + 1, sz, block::LEVER, 5, 3);
        assert_eq!(w.block(sx + 1, y, sz), block::REDSTONE_TORCH);
        w.toggle_lever(sx, y + 1, sz);
        steps(&mut sim, 3);
        assert_eq!(sim.world.block(sx + 1, y, sz), block::UNLIT_REDSTONE_TORCH);
        sim.world.toggle_lever(sx, y + 1, sz);
        steps(&mut sim, 3);
        assert_eq!(sim.world.block(sx + 1, y, sz), block::REDSTONE_TORCH);
    }

    #[test]
    fn repeater_delays_and_piston_pushes() {
        let (mut sim, x, y, z) = redstone_sim();
        let w = &mut sim.world;
        // Lever, repeater with its input at -x (direction 1), lamp.
        w.set_block(x, y, z, block::LEVER, 5, 3);
        w.set_block(x + 1, y, z, block::UNPOWERED_REPEATER, 1, 3);
        w.set_block(x + 2, y, z, block::REDSTONE_LAMP, 0, 3);
        w.toggle_lever(x, y, z);
        assert_eq!(w.block(x + 2, y, z), block::REDSTONE_LAMP);
        steps(&mut sim, 1);
        assert_eq!(sim.world.block(x + 2, y, z), block::REDSTONE_LAMP);
        steps(&mut sim, 2);
        assert_eq!(sim.world.block(x + 1, y, z), block::POWERED_REPEATER);
        assert_eq!(sim.world.block(x + 2, y, z), block::LIT_REDSTONE_LAMP);

        // Piston facing east with cobblestone in front; a lever on the
        // stone behind it powers it. The block event runs at the end of the
        // tick, then TileEntityPiston moves the blocks over two ticks and
        // places them on the third.
        let w = &mut sim.world;
        let pz = z + 6;
        w.set_block(x, y, pz, block::PISTON, 5, 3);
        w.set_block(x + 1, y, pz, block::COBBLESTONE, 0, 3);
        w.set_block(x - 1, y, pz, block::STONE, 0, 3);
        w.set_block(x - 1, y + 1, pz, block::LEVER, 5, 3);
        w.toggle_lever(x - 1, y + 1, pz);
        assert_eq!(
            w.meta(x, y, pz),
            5,
            "the event waits for the end of the tick"
        );
        steps(&mut sim, 1);
        let w = &mut sim.world;
        assert_eq!(w.meta(x, y, pz), 13);
        assert_eq!(w.block(x + 1, y, pz), block::PISTON_EXTENSION);
        assert_eq!(w.block(x + 2, y, pz), block::PISTON_EXTENSION);
        assert_eq!(w.moving.len(), 2);
        assert_eq!(
            w.moving_at([x + 2, y, pz]).unwrap().block,
            block::COBBLESTONE
        );
        assert_eq!(w.moving_at([x + 2, y, pz]).unwrap().progress, 0.5);
        steps(&mut sim, 2);
        let w = &mut sim.world;
        assert_eq!(w.block(x + 1, y, pz), block::PISTON_HEAD);
        assert_eq!(w.block(x + 2, y, pz), block::COBBLESTONE);
        assert!(w.moving.is_empty());
        w.toggle_lever(x - 1, y + 1, pz);
        assert_eq!(w.meta(x, y, pz), 5);
        steps(&mut sim, 1);
        let w = &mut sim.world;
        // Retracting: the base becomes a moving block that turns back into
        // the piston; the head is removed at once.
        assert_eq!(w.block(x, y, pz), block::PISTON_EXTENSION);
        assert_eq!(w.block(x + 1, y, pz), block::AIR);
        steps(&mut sim, 2);
        let w = &mut sim.world;
        assert_eq!(w.block(x, y, pz), block::PISTON);
        assert_eq!(w.meta(x, y, pz), 5);
        assert_eq!(w.block(x + 2, y, pz), block::COBBLESTONE);

        // A sticky piston pulls the block back.
        let sz = z + 9;
        w.set_block(x, y, sz, block::STICKY_PISTON, 5, 3);
        w.set_block(x + 1, y, sz, block::COBBLESTONE, 0, 3);
        w.set_block(x - 1, y, sz, block::STONE, 0, 3);
        w.set_block(x - 1, y + 1, sz, block::LEVER, 5, 3);
        w.toggle_lever(x - 1, y + 1, sz);
        steps(&mut sim, 3);
        let w = &mut sim.world;
        assert_eq!(w.block(x + 2, y, sz), block::COBBLESTONE);
        assert_eq!(w.block(x + 1, y, sz), block::PISTON_HEAD);
        w.toggle_lever(x - 1, y + 1, sz);
        steps(&mut sim, 1);
        let w = &mut sim.world;
        assert_eq!(w.block(x + 1, y, sz), block::PISTON_EXTENSION);
        assert_eq!(w.block(x + 2, y, sz), block::AIR);
        steps(&mut sim, 2);
        let w = &mut sim.world;
        assert_eq!(w.block(x + 1, y, sz), block::COBBLESTONE);
        assert_eq!(w.block(x, y, sz), block::STICKY_PISTON);
    }

    /// BlockFalling.tryFall spawns an EntityFallingBlock that lands two
    /// blocks lower; drops.dat keeps items over a save.
    #[test]
    fn sand_falls_as_an_entity() {
        let (mut sim, x, y, z) = redstone_sim();
        sim.world.set_block(x, y + 3, z, block::SAND, 1, 3);
        assert_eq!(sim.world.block(x, y + 3, z), block::SAND);
        assert_eq!(sim.world.meta(x, y + 3, z), 1);
        let mut seen = false;
        for _ in 0..40 {
            steps(&mut sim, 1);
            // Exposed cave gravel falls too (WorldGenMinable scheduled it).
            if let Some(f) = sim.play.falling.iter().find(|f| f.block == block::SAND) {
                seen = true;
                assert_eq!(f.meta, 1);
            }
        }
        assert!(seen, "an EntityFallingBlock appeared");
        assert!(!sim.play.falling.iter().any(|f| f.block == block::SAND));
        assert_eq!(sim.world.block(x, y + 3, z), block::AIR);
        assert_eq!(sim.world.block(x, y, z), block::SAND);
        assert_eq!(sim.world.meta(x, y, z), 1, "red sand stays red");

        let mut r = JavaRandom::new(3);
        let it = crate::entity::item::ItemEntity::new(
            7,
            crate::item::Stack::new(crate::item::STICK, 0, 5),
            [x as f64 + 0.5, y as f64 + 1.0, z as f64 + 0.5],
        );
        let bytes = Play::drops_nbt(&[it], &[], &[], 0);
        let mut back = Play::new(GameMode::Survival);
        back.load_drops(&bytes, &mut r);
        assert_eq!(back.items.len(), 1);
        assert_eq!(back.items[0].stack.count, 5);
    }

    /// `EntityPlayerMP.field_147101_bU`, `EntityFallingBlock.fall` with the
    /// helmet rule of `EntityLivingBase.attackEntityFrom`, and Thorns on a
    /// mob's armour (`EnchantmentHelper.func_151384_a`).
    #[test]
    fn spawn_protection_anvils_and_mob_thorns() {
        use crate::entity::living::{Difficulty, Source, SourceKind};
        use crate::entity::mob::Kind;
        use crate::item::Stack;
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        sim.play.mode = GameMode::Survival;
        let feet = [x as f64 + 0.5, (y + 1) as f64, z as f64 + 0.5];
        let here = Command::Player(feet, [-90.0, 0.0], false);
        sim.step(&[here.clone()], &mut Vec::new());

        // nothing but the void hurts for the first 60 ticks
        let cactus = Source::named(SourceKind::Cactus);
        sim.splayer.respawn_invulnerable = 60;
        assert!(!sim
            .splayer
            .attack(&sim.world, cactus, 1.0, Difficulty::Normal));
        for _ in 0..61 {
            sim.step(&[here.clone()], &mut Vec::new());
        }
        assert!(sim.splayer.respawn_invulnerable <= 0);
        assert_eq!(sim.splayer.living.health, 20.0);

        // an anvil from seven blocks up, onto an iron helmet
        sim.play.inv.armor[3] = Some(Stack::new(306, 0, 1));
        for dy in 1..12 {
            sim.world.set_block(x, y + dy, z, block::AIR, 0, 3);
        }
        sim.world.set_block(x, y + 9, z, block::ANVIL, 0, 3);
        for _ in 0..60 {
            sim.step(&[here.clone()], &mut Vec::new());
        }
        let after_anvil = sim.splayer.living.health;
        assert!(after_anvil < 20.0, "the anvil hurt: {after_anvil}");
        let helmet = sim.play.inv.armor[3].expect("the helmet survives one anvil");
        assert!(helmet.meta > 4, "the helmet took it: {}", helmet.meta);

        // a cow in a Thorns III chestplate, punched until it bites back
        for dy in 1..12 {
            sim.world.set_block(x, y + dy, z, block::AIR, 0, 3);
        }
        sim.splayer.living.health = 20.0;
        let mut r = JavaRandom::new(5);
        let at = [x as f64 + 2.5, (y + 1) as f64, z as f64 + 0.5];
        let cow = sim.mobs.spawn(Kind::Cow, at, 0.0, &mut r);
        let plate = Stack::new(307, 0, 1).with_enchantments(&[(crate::enchant::THORNS, 3)]);
        if let Some(m) = sim.mobs.list.iter_mut().find(|m| m.id == cow) {
            m.equipment[3] = Some(plate);
            m.health = 10.0;
        }
        let mut lowest = 20.0f32;
        for _ in 0..8 {
            if let Some(m) = sim.mobs.list.iter_mut().find(|m| m.id == cow) {
                m.health = 10.0;
            }
            sim.step(
                &[
                    here.clone(),
                    Command::Attack {
                        id: cow,
                        crit: false,
                        sprint: false,
                    },
                ],
                &mut Vec::new(),
            );
            lowest = lowest.min(sim.splayer.living.health);
            for _ in 0..12 {
                sim.step(&[here.clone()], &mut Vec::new());
            }
        }
        assert!(lowest < 20.0, "Thorns III never hit in eight punches");
        let worn = sim
            .mobs
            .list
            .iter()
            .find(|m| m.id == cow)
            .and_then(|m| m.equipment[3])
            .map_or(i32::MAX, |s| s.meta);
        assert!(worn >= 8, "the chestplate pays 1 or 3 a punch: {worn}");
    }

    /// Timing of a 6x6x6 block of TNT going off (run with `--ignored
    /// --nocapture`): the worst and total server tick.
    #[test]
    #[ignore]
    fn tnt_cube_timing() {
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        for dx in 0..6 {
            for dy in 1..7 {
                for dz in 0..6 {
                    sim.world
                        .set_block(x + dx, y + dy, z + dz, block::TNT, 0, 3);
                }
            }
        }
        sim.world.prime_tnt(x, y + 6, z, 20);
        let mut worst = 0.0f64;
        let mut total = 0.0f64;
        let mut ev = Vec::new();
        let mut peak = 0;
        for i in 0..200 {
            let t = std::time::Instant::now();
            sim.step(&[], &mut ev);
            let d = t.elapsed().as_secs_f64() * 1000.0;
            total += d;
            if d > 50.0 {
                println!(
                    "tick {i}: {d:.1} ms, {} tnt, {} events",
                    sim.mobs.tnt.len(),
                    ev.len()
                );
            }
            worst = worst.max(d);
            peak = peak.max(sim.mobs.tnt.len());
            ev.clear();
        }
        println!(
            "worst {worst:.1} ms, total {total:.1} ms, peak tnt {peak}, items {}",
            sim.play.items.len()
        );
    }

    /// Sky light a from-scratch propagation gives inside a box, with the
    /// world's own values as the boundary; returns the cells that differ
    /// (position, stored, expected).
    fn sky_light_errors(
        w: &mut crate::world::server::ServerWorld,
        x0: i32,
        x1: i32,
        y0: i32,
        y1: i32,
        z0: i32,
        z1: i32,
    ) -> Vec<([i32; 3], i32, i32)> {
        let (nx, ny, nz) = (
            (x1 - x0 + 1) as usize,
            (y1 - y0 + 1) as usize,
            (z1 - z0 + 1) as usize,
        );
        let at = |x: i32, y: i32, z: i32| {
            ((x - x0) as usize * ny + (y - y0) as usize) * nz + (z - z0) as usize
        };
        let mut op = vec![0i32; nx * ny * nz];
        let mut sky = vec![false; nx * ny * nz];
        let mut v = vec![0i32; nx * ny * nz];
        for x in x0..=x1 {
            for z in z0..=z1 {
                let h = w.height(x, z);
                for y in y0..=y1 {
                    let i = at(x, y, z);
                    op[i] = (block::info(w.peek_block(x, y, z)).light_opacity as i32).max(1);
                    sky[i] = y >= h;
                    v[i] = if sky[i] { 15 } else { 0 };
                }
            }
        }
        loop {
            let mut changed = false;
            for x in x0..=x1 {
                for y in y0..=y1 {
                    for z in z0..=z1 {
                        let i = at(x, y, z);
                        if sky[i] || op[i] >= 15 {
                            continue;
                        }
                        let mut m = 0;
                        for (dx, dy, dz) in [
                            (1, 0, 0),
                            (-1, 0, 0),
                            (0, 1, 0),
                            (0, -1, 0),
                            (0, 0, 1),
                            (0, 0, -1),
                        ] {
                            let (a, b, c) = (x + dx, y + dy, z + dz);
                            let n = if a < x0 || a > x1 || b < y0 || b > y1 || c < z0 || c > z1 {
                                w.saved_light(true, a, b, c)
                            } else {
                                v[at(a, b, c)]
                            };
                            m = m.max(n);
                        }
                        let e = (m - op[i]).max(0);
                        if e != v[i] {
                            v[i] = e;
                            changed = true;
                        }
                    }
                }
            }
            if !changed {
                break;
            }
        }
        let mut out = Vec::new();
        for x in x0 + 1..x1 {
            for y in y0 + 1..y1 {
                for z in z0 + 1..z1 {
                    let got = w.saved_light(true, x, y, z);
                    let want = v[at(x, y, z)];
                    if got != want {
                        out.push(([x, y, z], got, want));
                    }
                }
            }
        }
        out
    }

    /// The world from the user's screenshot (seed -2050906875735508110 at
    /// -198, 71, 261): the canopy burns, then sky light is checked.
    #[test]
    fn burnt_canopy_light() {
        let cfg = SimConfig {
            seed: -2050906875735508110,
            view: 4,
            save_dir: None,
            start_time: 3510,
            freeze_time: true,
            mode: GameMode::Creative,
        };
        let (mut sim, _) = Sim::new(&cfg);
        let feet = [-197.7, 71.4, 261.0];
        let mut ev = Vec::new();
        sim.step(&[Command::Center(-13, 16)], &mut ev);
        for _ in 0..600 {
            sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut ev);
            ev.clear();
        }
        let (cx, cz) = (-198, 261);
        let before = sky_light_errors(&mut sim.world, cx - 24, cx + 24, 55, 110, cz - 24, cz + 24);
        println!("before: {} wrong", before.len());
        let summary = |w: &mut crate::world::server::ServerWorld, v: &[([i32; 3], i32, i32)]| {
            let (mut dark, mut bright, mut surface) = (0, 0, 0);
            let mut cols = std::collections::BTreeMap::new();
            for (p, got, want) in v {
                if got < want {
                    dark += 1
                } else {
                    bright += 1
                }
                let h = w.height(p[0], p[2]);
                if p[1] >= h - 4 {
                    surface += 1;
                }
                let edge = (
                    p[0] & 15 == 0 || p[0] & 15 == 15,
                    p[2] & 15 == 0 || p[2] & 15 == 15,
                );
                *cols.entry(edge).or_insert(0) += 1;
            }
            println!("  too dark {dark}, too bright {bright}, within 4 of the top {surface}, by chunk edge (x, z) {cols:?}");
        };
        summary(&mut sim.world, &before);
        assert!(before.is_empty(), "generation left wrong sky light");
        // light the leaves on fire, as a lightning strike or lava would
        let mut lit = 0;
        for x in cx - 16..=cx + 16 {
            for z in cz - 16..=cz + 16 {
                for y in 60..100 {
                    if block::material(sim.world.peek_block(x, y, z)) == block::Material::Leaves
                        && sim.world.peek_block(x, y + 1, z) == block::AIR
                    {
                        sim.world.set_block(x, y + 1, z, block::FIRE, 0, 3);
                        lit += 1;
                    }
                }
            }
        }
        println!("lit {lit}");
        for _ in 0..1200 {
            sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut ev);
            ev.clear();
        }
        let unfixed = (-15..=-11)
            .flat_map(|x| (14..=18).map(move |z| (x, z)))
            .filter(|&(x, z)| sim.world.wants_light_fix(x, z) || !sim.world.chunk_exists(x, z))
            .count();
        println!("chunks in the box not relit: {unfixed}");
        let after = sky_light_errors(&mut sim.world, cx - 24, cx + 24, 55, 110, cz - 24, cz + 24);
        println!("after: {} wrong", after.len());
        summary(&mut sim.world, &after);
        assert!(after.is_empty(), "the fire left wrong sky light");
        let t = std::time::Instant::now();
        for x in -15..=-11 {
            for z in 14..=18 {
                sim.world.relight_now(x, z);
            }
        }
        println!("25 relights: {:.2} ms", t.elapsed().as_secs_f64() * 1000.0);
        for e in after.iter().take(15) {
            println!(
                "  {:?} stored {} expected {} block {}",
                e.0,
                e.1,
                e.2,
                sim.world.peek_block(e.0[0], e.0[1], e.0[2])
            );
        }
    }

    /// ItemFood, ItemPotion and ItemBow through setItemInUse.
    #[test]
    fn player_projectiles_launch_at_head_height() {
        use crate::item::Stack;
        let cfg = SimConfig {
            seed: 12345,
            view: 2,
            save_dir: None,
            start_time: 6000,
            freeze_time: true,
            mode: GameMode::Survival,
        };
        let (mut sim, _) = Sim::new(&cfg);
        let feet = [8.5, 120.0, 8.5];
        sim.apply(&Command::Player(feet, [0.0, 0.0], false));
        let expected_y = feet[1] + 1.52;
        for (id, meta) in [(373, 16421), (332, 0), (344, 0), (368, 0), (384, 0)] {
            sim.play.inv.main[0] = Some(Stack::new(id, meta, 1));
            sim.apply(&Command::UseAir);
            let projectile = sim.mobs.throwables.pop().expect("item was thrown");
            assert_eq!(projectile.kind.item(), (id, meta));
            let y = projectile.e.pos[1];
            assert!((y - expected_y).abs() < 1e-6, "item {id} launched at {y}");
        }

        sim.play.inv.main[0] = Some(Stack::new(261, 0, 1));
        sim.play.inv.main[1] = Some(Stack::new(262, 0, 1));
        sim.apply(&Command::UseAir);
        sim.splayer.using.as_mut().expect("bow is drawn").2 -= 20;
        sim.apply(&Command::StopUsing);
        let y = sim.mobs.arrows.last().expect("arrow was fired").e.pos[1];
        assert!((y - expected_y).abs() < 1e-6, "arrow launched at {y}");
    }

    #[test]
    fn eat_drink_and_shoot() {
        let (mut sim, x, y, z) = redstone_sim();
        // EntityPlayer.canEat: not in creative
        sim.play.mode = GameMode::Survival;
        let feet = [x as f64 + 0.5, y as f64, z as f64 + 0.5];
        sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut Vec::new());
        sim.splayer.food.level = 10;
        sim.play.inv.selected = 0;
        sim.play.inv.main[0] = Some(crate::item::Stack::new(364, 0, 2));
        steps(&mut sim, 1);
        sim.step(&[Command::UseAir], &mut Vec::new());
        assert!(sim.splayer.using.is_some(), "steak is being eaten");
        steps(&mut sim, 32);
        assert!(sim.splayer.using.is_none());
        assert_eq!(sim.splayer.food.level, 18);
        assert_eq!(sim.play.inv.main[0].unwrap().count, 1);

        // Potion of Swiftness (8194)
        sim.play.inv.main[0] = Some(crate::item::Stack::new(373, 8194, 1));
        sim.step(&[Command::UseAir], &mut Vec::new());
        steps(&mut sim, 32);
        assert!(sim.splayer.living.active(crate::potion::MOVE_SPEED));
        assert_eq!(
            sim.play.inv.main[0].unwrap().id,
            374,
            "a glass bottle is left"
        );

        // the bow, fully drawn, against the stone floor
        sim.play.inv.main[0] = Some(crate::item::Stack::new(261, 0, 1));
        sim.play.inv.main[1] = Some(crate::item::Stack::new(262, 0, 5));
        sim.play.pitch = 60.0;
        sim.step(&[Command::UseAir], &mut Vec::new());
        steps(&mut sim, 25);
        sim.step(&[Command::StopUsing], &mut Vec::new());
        assert_eq!(sim.mobs.arrows.len(), 1);
        // (crit is cleared once it sticks, on its first tick here)
        assert_eq!(sim.play.inv.main[1].unwrap().count, 4);
        assert_eq!(sim.play.inv.main[0].unwrap().meta, 1, "the bow wears");
        for _ in 0..40 {
            steps(&mut sim, 1);
            if sim.mobs.arrows.first().is_some_and(|a| a.in_ground) {
                break;
            }
        }
        assert!(
            sim.mobs.arrows.first().is_some_and(|a| a.in_ground),
            "the arrow sticks in the ground"
        );
    }

    /// TileEntityBrewingStand: water bottles and nether wart make awkward
    /// potions in 400 ticks; sugar then makes swiftness.
    #[test]
    fn brewing_stand_brews() {
        use crate::item::Stack;
        use crate::play::Tile;
        let (mut sim, x, y, z) = redstone_sim();
        sim.world.set_block(x, y, z, block::BREWING_STAND, 0, 3);
        let p = [x, y, z];
        sim.play.tiles.insert(
            p,
            Tile::Brewing {
                slots: [
                    Some(Stack::new(373, 0, 1)),
                    Some(Stack::new(373, 0, 1)),
                    None,
                    Some(Stack::new(372, 0, 1)),
                ],
                brew_time: 0,
                filled: 0,
                ingredient: 0,
            },
        );
        steps(&mut sim, 1);
        assert_eq!(sim.world.meta(x, y, z), 3, "two bottles show on the block");
        steps(&mut sim, 400);
        let Some(Tile::Brewing { slots, .. }) = sim.play.tiles.get(&p).cloned() else {
            panic!()
        };
        assert_eq!(slots[0].unwrap().meta, 16, "awkward potion");
        assert!(slots[3].is_none(), "the wart is used up");
        if let Some(Tile::Brewing { slots, .. }) = sim.play.tiles.get_mut(&p) {
            slots[3] = Some(Stack::new(353, 0, 1));
        }
        steps(&mut sim, 402);
        let Some(Tile::Brewing { slots, .. }) = sim.play.tiles.get(&p).cloned() else {
            panic!()
        };
        let e = crate::potion::effects_of(slots[0].unwrap().meta, false);
        assert_eq!(e[0].id, crate::potion::MOVE_SPEED);
    }

    /// TileEntityHopper moves one item every 8 ticks from the chest above
    /// into the chest it faces, picks up items lying on top, and stops
    /// while powered. A dropper pushes into the chest in front; a dispenser
    /// fires snowballs and pours a water bucket, keeping the empty bucket.
    #[test]
    fn hoppers_droppers_and_dispensers() {
        use crate::item::Stack;
        use crate::play::Tile;
        let (mut sim, x, y, z) = redstone_sim();
        let feet = [x as f64 + 9.5, y as f64, z as f64 + 9.5];
        let step = |sim: &mut Sim, n: usize| {
            for _ in 0..n {
                sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut Vec::new());
            }
        };
        let chest = |slots: &[(usize, Stack)]| {
            let mut a = [None; 27];
            for (k, s) in slots {
                a[*k] = Some(*s);
            }
            Tile::Chest { slots: a }
        };
        let count = |sim: &Sim, p: [i32; 3]| -> u32 {
            match sim.play.tiles.get(&p) {
                Some(Tile::Chest { slots }) => slots.iter().flatten().map(|s| s.count).sum(),
                Some(Tile::Hopper { slots, .. }) => slots.iter().flatten().map(|s| s.count).sum(),
                _ => 0,
            }
        };
        // chest, hopper (facing down), chest
        let (top, hop, bottom) = ([x, y + 2, z], [x, y + 1, z], [x, y, z]);
        sim.world
            .set_block(bottom[0], bottom[1], bottom[2], block::CHEST, 2, 3);
        sim.world
            .set_block(hop[0], hop[1], hop[2], block::HOPPER, 0, 3);
        sim.world
            .set_block(top[0], top[1], top[2], block::CHEST, 2, 3);
        sim.play
            .tiles
            .insert(top, chest(&[(0, Stack::new(4, 0, 3))]));
        sim.play.tiles.insert(bottom, chest(&[]));
        sim.play.tiles.insert(
            hop,
            Tile::Hopper {
                slots: [None; 5],
                cooldown: -1,
            },
        );
        step(&mut sim, 60);
        assert_eq!(count(&sim, top), 0, "the top chest is emptied");
        assert_eq!(count(&sim, bottom), 3, "into the bottom chest");
        // an item on the top chest's place: take the chest away
        sim.world
            .set_block(top[0], top[1], top[2], block::AIR, 0, 3);
        sim.play.tiles.remove(&top);
        let id = sim.play.next_item;
        sim.play.next_item += 1;
        let mut it = crate::entity::item::ItemEntity::new(
            id,
            Stack::new(12, 0, 2),
            [x as f64 + 0.5, (y + 2) as f64 + 0.1, z as f64 + 0.5],
        );
        it.e.motion = [0.0; 3];
        sim.play.spawn(it);
        step(&mut sim, 40);
        assert_eq!(count(&sim, bottom), 5, "the dropped sand went through");
        // powered: nothing moves
        sim.world
            .set_block(x + 1, y + 1, z, block::REDSTONE_BLOCK, 0, 3);
        step(&mut sim, 2);
        assert_eq!(sim.world.meta(hop[0], hop[1], hop[2]) & 8, 8, "disabled");
        if let Some(Tile::Hopper { slots, .. }) = sim.play.tiles.get_mut(&hop) {
            slots[0] = Some(Stack::new(1, 0, 1));
        }
        step(&mut sim, 40);
        assert_eq!(count(&sim, bottom), 5, "a powered hopper holds on");
        assert_eq!(count(&sim, hop), 1);

        // a dropper facing +x into a chest, triggered by a redstone block
        let (dr, ch) = ([x + 4, y, z + 4], [x + 5, y, z + 4]);
        sim.world.set_block(ch[0], ch[1], ch[2], block::CHEST, 4, 3);
        sim.world
            .set_block(dr[0], dr[1], dr[2], block::DROPPER, 5, 3);
        sim.world.set_meta(dr[0], dr[1], dr[2], 5, 2);
        sim.play.tiles.insert(ch, chest(&[]));
        let mut ds = [None; 9];
        ds[4] = Some(Stack::new(264, 0, 2));
        sim.play.tiles.insert(
            dr,
            Tile::Dispenser {
                slots: ds,
                dropper: true,
            },
        );
        sim.world
            .set_block(dr[0] - 1, dr[1], dr[2], block::REDSTONE_BLOCK, 0, 3);
        step(&mut sim, 10);
        assert_eq!(count(&sim, ch), 1, "one diamond into the chest");

        // a dispenser facing +x: snowballs, then a water bucket
        let dp = [x + 4, y, z + 8];
        sim.world
            .set_block(dp[0], dp[1], dp[2], block::DISPENSER, 5, 3);
        // BlockDispenser.onBlockAdded turns it away from solid neighbours
        sim.world.set_meta(dp[0], dp[1], dp[2], 5, 2);
        let mut ds = [None; 9];
        ds[0] = Some(Stack::new(332, 0, 4));
        sim.play.tiles.insert(
            dp,
            Tile::Dispenser {
                slots: ds,
                dropper: false,
            },
        );
        let thrown = sim.mobs.throwables.len();
        sim.world
            .set_block(dp[0], dp[1] + 1, dp[2], block::REDSTONE_BLOCK, 0, 3);
        step(&mut sim, 6);
        assert_eq!(sim.mobs.throwables.len(), thrown + 1, "a snowball flies");
        sim.world
            .set_block(dp[0], dp[1] + 1, dp[2], block::AIR, 0, 3);
        step(&mut sim, 6);
        let mut ds = [None; 9];
        ds[0] = Some(Stack::new(326, 0, 1));
        sim.play.tiles.insert(
            dp,
            Tile::Dispenser {
                slots: ds,
                dropper: false,
            },
        );
        sim.world
            .set_block(dp[0], dp[1] + 1, dp[2], block::REDSTONE_BLOCK, 0, 3);
        step(&mut sim, 6);
        assert_eq!(
            block::material(sim.world.block(dp[0] + 1, dp[1], dp[2])),
            block::Material::Water,
            "water poured in front"
        );
        match sim.play.tiles.get(&dp) {
            Some(Tile::Dispenser { slots, .. }) => {
                assert_eq!(slots[0].map(|s| s.id), Some(325), "the empty bucket stays")
            }
            _ => panic!(),
        }
    }

    /// ItemSaddle on a pig, EntityPig.interact mounts the player, the
    /// carrot on a stick steers it (EntityAIControlledByPlayer) and boosts
    /// it for 7 durability, and sneaking gets off.
    #[test]
    fn saddled_pigs_can_be_ridden() {
        use crate::entity::mob::Kind;
        use crate::item::Stack;
        let (mut sim, x, y, z) = redstone_sim();
        sim.play.mode = GameMode::Survival;
        let feet = [x as f64 + 2.5, y as f64, z as f64 + 2.5];
        let mut r = JavaRandom::new(3);
        let pig = sim.mobs.spawn(
            Kind::Pig,
            [x as f64 + 3.5, y as f64, z as f64 + 2.5],
            0.0,
            &mut r,
        );
        // getAIMoveSpeed is whatever the last AI move set: let it walk once
        // (EntityAITempt with a carrot), as a pig in the world has walked before
        let sel = sim.play.inv.selected;
        sim.play.inv.main[sel] = Some(Stack::new(391, 0, 1));
        let far = [x as f64 + 8.5, y as f64, z as f64 + 2.5];
        for _ in 0..60 {
            sim.step(
                &[Command::Player(far, [90.0, 20.0], false)],
                &mut Vec::new(),
            );
        }
        sim.play.inv.main[sel] = Some(Stack::new(329, 0, 1));
        sim.step(
            &[Command::Player(feet, [-90.0, 20.0], false)],
            &mut Vec::new(),
        );
        sim.step(&[Command::Interact { id: pig }], &mut Vec::new());
        let m = sim.mobs.list.iter().find(|m| m.id == pig).unwrap();
        assert!(m.saddled, "saddled");
        assert!(sim.play.inv.main[sel].is_none(), "the saddle is used");
        let mut ev = Vec::new();
        sim.step(&[Command::Interact { id: pig }], &mut ev);
        assert!(
            ev.iter()
                .any(|e| matches!(e, Event::Mount(Some(i)) if *i == pig)),
            "mounted"
        );
        // steer east (yaw -90) with a carrot on a stick
        sim.play.inv.main[sel] = Some(Stack::new(398, 0, 1));
        let start = sim.mobs.list.iter().find(|m| m.id == pig).unwrap().pos;
        // until it has gone two blocks east (the platform ends at x + 12)
        for _ in 0..300 {
            sim.step(
                &[Command::Player(feet, [-90.0, 20.0], false)],
                &mut Vec::new(),
            );
            let m = sim.mobs.list.iter().find(|m| m.id == pig).unwrap();
            if m.pos[0] - start[0] > 2.0 {
                break;
            }
        }
        let m = sim.mobs.list.iter().find(|m| m.id == pig).unwrap();
        assert!(
            m.pos[0] - start[0] > 2.0,
            "it walked east: {:?} -> {:?}",
            start,
            m.pos
        );
        assert!((m.pos[2] - start[2]).abs() < 0.5, "straight ahead");
        let pf = sim.play.player.0;
        assert!(
            (pf[0] - m.pos[0]).abs() < 1e-6 && (pf[1] - (m.pos[1] + 0.9 * 0.75 - 0.5)).abs() < 1e-3,
            "the player sits on it"
        );
        // the boost costs 7
        sim.step(&[Command::UseAir], &mut Vec::new());
        assert_eq!(sim.play.inv.main[sel].unwrap().meta, 7, "boost wear");
        // sneak off
        let mut ev = Vec::new();
        sim.step(
            &[Command::Input {
                strafe: 0.0,
                forward: 0.0,
                jump: false,
                sneak: true,
            }],
            &mut ev,
        );
        sim.step(&[Command::Player(feet, [-90.0, 20.0], false)], &mut ev);
        assert!(
            ev.iter().any(|e| matches!(e, Event::Mount(None))),
            "got off"
        );
        assert!(
            !sim.mobs
                .list
                .iter()
                .find(|m| m.id == pig)
                .unwrap()
                .ridden_by_player
        );
    }

    /// ItemRecord.onItemUse puts the record in (metadata 1, aux event 1005
    /// with the item, comparator signal id - 2256 + 1); BlockJukebox's
    /// activation ejects it (event 1005 with 0, the record as an item).
    #[test]
    fn jukebox_plays_and_ejects_records() {
        use crate::item::Stack;
        let (mut sim, x, y, z) = redstone_sim();
        sim.play.mode = GameMode::Survival;
        let feet = [x as f64 + 9.5, y as f64, z as f64 + 9.5];
        let jb = [x + 2, y, z + 2];
        sim.world
            .set_block(jb[0], jb[1], jb[2], block::JUKEBOX, 0, 3);
        let cp = [x + 3, y, z + 2];
        sim.world
            .set_block(cp[0], cp[1], cp[2], block::UNPOWERED_COMPARATOR, 1, 3);
        let sel = sim.play.inv.selected;
        sim.play.inv.main[sel] = Some(Stack::new(2257, 0, 1));
        let use_it = |sim: &mut Sim| {
            let mut ev = Vec::new();
            sim.step(
                &[Command::Use {
                    target: jb,
                    face: [0, 1, 0],
                    sneaking: false,
                    hit: [0.5, 1.0, 0.5],
                }],
                &mut ev,
            );
            ev
        };
        let ev = use_it(&mut sim);
        assert_eq!(sim.world.meta(jb[0], jb[1], jb[2]), 1, "a record inside");
        assert!(sim.play.inv.main[sel].is_none(), "the record left the hand");
        assert!(ev
            .iter()
            .any(|e| matches!(e, Event::Record(p, 2257) if *p == jb)));
        for _ in 0..8 {
            sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut Vec::new());
        }
        assert_eq!(
            sim.world.comparator_output(cp[0], cp[1], cp[2]),
            2,
            "cat is record 2"
        );
        let items = sim.play.items.len();
        let ev = use_it(&mut sim);
        assert_eq!(sim.world.meta(jb[0], jb[1], jb[2]), 0, "empty again");
        assert!(ev
            .iter()
            .any(|e| matches!(e, Event::Record(p, 0) if *p == jb)));
        assert_eq!(sim.play.items.len(), items + 1, "the record pops out");
        assert_eq!(sim.play.items.last().unwrap().stack.id, 2257);
    }

    /// TileEntityDaylightDetector: every 20 ticks the sky light scaled by
    /// the sun's angle; 15 at noon under open sky, 0 at midnight.
    #[test]
    fn daylight_detector_follows_the_sun() {
        let (mut sim, x, y, z) = redstone_sim();
        let feet = [x as f64 + 9.5, y as f64, z as f64 + 9.5];
        let step = |sim: &mut Sim, n: usize| {
            for _ in 0..n {
                sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut Vec::new());
            }
        };
        let p = [x + 3, y, z + 3];
        sim.world
            .set_block(p[0], p[1], p[2], block::DAYLIGHT_DETECTOR, 0, 3);
        step(&mut sim, 45);
        assert_eq!(sim.world.meta(p[0], p[1], p[2]), 15, "noon, open sky");
        assert_eq!(sim.world.weak_power(p[0], p[1], p[2], 2), 15);
        sim.world_time = 18000;
        step(&mut sim, 45);
        assert_eq!(sim.world.meta(p[0], p[1], p[2]), 0, "midnight");
    }

    /// BlockRedstoneComparator reads Container.calcRedstoneFromInventory
    /// of the chest behind it, follows its changes, subtracts a side input
    /// in subtract mode, and reads a cake's bites.
    #[test]
    fn comparators_read_containers() {
        use crate::item::Stack;
        use crate::play::Tile;
        let (mut sim, x, y, z) = redstone_sim();
        let feet = [x as f64 + 9.5, y as f64, z as f64 + 9.5];
        let step = |sim: &mut Sim, n: usize| {
            for _ in 0..n {
                sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut Vec::new());
            }
        };
        // chest at x, comparator at x + 1 facing +x (direction 1 takes input from x - 1)
        let (ch, cp) = ([x, y, z], [x + 1, y, z]);
        sim.world.set_block(ch[0], ch[1], ch[2], block::CHEST, 2, 3);
        sim.play.tiles.insert(ch, Tile::Chest { slots: [None; 27] });
        sim.world
            .set_block(cp[0], cp[1], cp[2], block::UNPOWERED_COMPARATOR, 1, 3);
        step(&mut sim, 6);
        assert_eq!(
            sim.world.comparator_output(cp[0], cp[1], cp[2]),
            0,
            "empty chest"
        );
        if let Some(Tile::Chest { slots }) = sim.play.tiles.get_mut(&ch) {
            slots[0] = Some(Stack::new(4, 0, 1));
        }
        step(&mut sim, 6);
        assert_eq!(
            sim.world.comparator_output(cp[0], cp[1], cp[2]),
            1,
            "one item"
        );
        assert_eq!(
            sim.world.weak_power(cp[0], cp[1], cp[2], 4),
            1,
            "powers forward"
        );
        if let Some(Tile::Chest { slots }) = sim.play.tiles.get_mut(&ch) {
            for s in slots.iter_mut() {
                *s = Some(Stack::new(4, 0, 64));
            }
        }
        step(&mut sim, 6);
        assert_eq!(sim.world.comparator_output(cp[0], cp[1], cp[2]), 15, "full");
        // subtract mode with a redstone block (15) at the side
        sim.world.comparator_toggle(cp[0], cp[1], cp[2]);
        assert_eq!(sim.world.meta(cp[0], cp[1], cp[2]) & 4, 4, "subtract mode");
        if let Some(Tile::Chest { slots }) = sim.play.tiles.get_mut(&ch) {
            *slots = [None; 27];
            slots[0] = Some(Stack::new(4, 0, 64));
        }
        // a redstone block right beside it gives no strong power: nothing is
        // taken away (getInputSignalFromSide uses isBlockProvidingPowerTo)
        sim.world
            .set_block(cp[0], cp[1], cp[2] - 1, block::REDSTONE_BLOCK, 0, 3);
        step(&mut sim, 8);
        assert_eq!(
            sim.world.comparator_output(cp[0], cp[1], cp[2]),
            1,
            "a block does not subtract"
        );
        // a powered wire does: 64/64 of one of 27 slots gives
        // floor(1/27 * 14) + 1 = 1, minus the wire's 15 at the side
        sim.world
            .set_block(cp[0], cp[1], cp[2] - 1, block::REDSTONE_WIRE, 0, 3);
        sim.world
            .set_block(cp[0], cp[1], cp[2] - 2, block::REDSTONE_BLOCK, 0, 3);
        step(&mut sim, 8);
        assert_eq!(
            sim.world.comparator_output(cp[0], cp[1], cp[2]),
            0,
            "subtracted"
        );
        // a cake behind another comparator: 7 bites left -> 14, one eaten -> 12
        let (ck, c2) = ([x + 4, y, z + 4], [x + 5, y, z + 4]);
        sim.world.set_block(ck[0], ck[1], ck[2], block::CAKE, 0, 3);
        sim.world
            .set_block(c2[0], c2[1], c2[2], block::UNPOWERED_COMPARATOR, 1, 3);
        // as a player places it: onBlockPlacedBy schedules the first update
        sim.world.comparator_placed(c2[0], c2[1], c2[2]);
        step(&mut sim, 6);
        assert_eq!(
            sim.world.comparator_output(c2[0], c2[1], c2[2]),
            14,
            "whole cake"
        );
        sim.world.set_meta(ck[0], ck[1], ck[2], 1, 3);
        step(&mut sim, 6);
        assert_eq!(
            sim.world.comparator_output(c2[0], c2[1], c2[2]),
            12,
            "one slice gone"
        );
    }

    /// ItemInWorldManager: a creative player breaks bedrock at once; a
    /// survival player never does.
    #[test]
    fn creative_breaks_bedrock() {
        let (mut sim, x, y, z) = redstone_sim();
        let p = [x + 3, y - 1, z + 3];
        sim.world.set_block(p[0], p[1], p[2], block::BEDROCK, 0, 3);
        sim.play.mode = GameMode::Survival;
        sim.play.dig(&mut sim.world, p);
        assert_eq!(
            sim.world.block(p[0], p[1], p[2]),
            block::BEDROCK,
            "survival: no"
        );
        sim.play.mode = GameMode::Creative;
        sim.play.dig(&mut sim.world, p);
        assert_eq!(
            sim.world.block(p[0], p[1], p[2]),
            block::AIR,
            "creative: gone"
        );
    }

    /// ItemBucket picks up a source block and pours it out on the face
    /// in view; ItemMonsterPlacer spawns its mob on the clicked face.
    #[test]
    fn buckets_and_spawn_eggs() {
        let (mut sim, x, y, z) = redstone_sim();
        sim.play.mode = GameMode::Survival;
        // a water source two blocks ahead, at floor level, in a stone basin
        sim.world.set_block(x + 2, y - 1, z, block::WATER, 0, 3);
        let feet = [x as f64 + 0.5, y as f64, z as f64 + 0.5];
        // looking down at it: yaw -90 faces +x
        let look = [-90.0f32, 40.0];
        let sel = sim.play.inv.selected;
        sim.play.inv.main[sel] = Some(crate::item::Stack::new(325, 0, 1));
        for _ in 0..2 {
            sim.step(&[Command::Player(feet, look, false)], &mut Vec::new());
        }
        sim.step(&[Command::UseAir], &mut Vec::new());
        assert_eq!(sim.play.inv.main[sel].unwrap().id, 326, "filled with water");
        assert_eq!(
            sim.world.block(x + 2, y - 1, z),
            block::AIR,
            "the source is gone"
        );
        // pour it back: the ray now hits the stone under the hole
        sim.step(&[Command::UseAir], &mut Vec::new());
        assert_eq!(sim.play.inv.main[sel].unwrap().id, 325, "empty again");
        assert_eq!(
            sim.world.block(x + 2, y - 1, z),
            block::FLOWING_WATER,
            "poured"
        );
        // a pig from an egg on the floor
        let pigs = |s: &Sim| {
            s.mobs
                .list
                .iter()
                .filter(|m| m.kind == crate::entity::mob::Kind::Pig)
                .count()
        };
        let before = pigs(&sim);
        sim.play.inv.main[sel] = Some(crate::item::Stack::new(383, 90, 2));
        sim.step(
            &[Command::Use {
                target: [x + 1, y - 1, z],
                face: [0, 1, 0],
                sneaking: false,
                hit: [0.5, 1.0, 0.5],
            }],
            &mut Vec::new(),
        );
        assert_eq!(pigs(&sim), before + 1, "a pig");
        assert_eq!(sim.play.inv.main[sel].unwrap().count, 1, "one egg used");
    }

    /// BlockPressurePlateWeighted: the power is the number of entities on
    /// the plate scaled to its maximum (15 for gold, 150 for iron), rounded
    /// up, and it lets go 10 ticks after they leave.
    #[test]
    fn weighted_plates_count_entities() {
        let (mut sim, x, y, z) = redstone_sim();
        sim.world
            .set_block(x, y, z, block::LIGHT_WEIGHTED_PRESSURE_PLATE, 0, 3);
        sim.world
            .set_block(x + 4, y, z, block::HEAVY_WEIGHTED_PRESSURE_PLATE, 0, 3);
        // keep the player away from both
        let feet = [x as f64 + 8.5, y as f64, z as f64 + 8.5];
        for px in [x, x + 4] {
            // five different items, so none merge
            for (i, id) in [1u16, 3, 4, 5, 12].iter().enumerate() {
                let n = sim.play.next_item;
                sim.play.next_item += 1;
                let mut e = crate::entity::item::ItemEntity::new(
                    n,
                    crate::item::Stack::new(*id, 0, 1),
                    [
                        px as f64 + 0.3 + i as f64 * 0.1,
                        y as f64 + 0.2,
                        z as f64 + 0.5,
                    ],
                );
                e.e.motion = [0.0; 3];
                e.pickup_delay = 32767;
                sim.play.spawn(e);
            }
        }
        for _ in 0..20 {
            sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut Vec::new());
        }
        assert_eq!(sim.world.meta(x, y, z), 5, "gold: 5 of 15");
        assert_eq!(sim.world.meta(x + 4, y, z), 1, "iron: ceil(5 / 150 * 15)");
        assert_eq!(
            crate::world::redstone::plate_power(block::LIGHT_WEIGHTED_PRESSURE_PLATE, 5),
            5
        );
        sim.play.items.clear();
        for _ in 0..25 {
            sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut Vec::new());
        }
        assert_eq!(sim.world.meta(x, y, z), 0, "released");
        assert_eq!(sim.world.meta(x + 4, y, z), 0, "released");
    }

    /// ItemFishingRod casts an EntityFishHook into a pool; once a fish
    /// bites (ticksCatchable), reeling in throws the catch at the player,
    /// drops experience and wears the rod by 1. Reeling an empty line in
    /// costs nothing.
    #[test]
    fn fishing_catches_something() {
        use crate::entity::vehicle::Vehicle;
        let (mut sim, x, y, z) = redstone_sim();
        sim.play.mode = GameMode::Survival;
        for dx in 4..10 {
            for dz in 2..10 {
                sim.world
                    .set_block(x + dx, y - 2, z + dz, block::STONE, 0, 3);
                sim.world
                    .set_block(x + dx, y - 1, z + dz, block::WATER, 0, 3);
            }
        }
        let feet = [x as f64 + 2.5, y as f64, z as f64 + 5.5];
        let look = [-90.0f32, 20.0];
        let sel = sim.play.inv.selected;
        sim.play.inv.main[sel] = Some(crate::item::Stack::new(346, 0, 1));
        for _ in 0..2 {
            sim.step(&[Command::Player(feet, look, false)], &mut Vec::new());
        }
        sim.step(&[Command::UseAir], &mut Vec::new());
        assert!(sim.fish_hook.is_some(), "the hook is cast");
        // an empty reel-in: no catch, no wear
        sim.step(&[Command::UseAir], &mut Vec::new());
        assert!(sim.fish_hook.is_none());
        assert_eq!(sim.play.inv.main[sel].unwrap().meta, 0);
        sim.step(&[Command::UseAir], &mut Vec::new());
        let mut bit = false;
        let mut in_water = false;
        let mut saw_wake = false;
        for _ in 0..3000 {
            let mut ev = Vec::new();
            sim.step(&[Command::Player(feet, look, false)], &mut ev);
            saw_wake |= ev
                .iter()
                .any(|e| matches!(e, Event::Particles(l) if l.iter().any(|p| p.0 == "wake")));
            let h = sim.fish_hook.as_ref().expect("the hook stays out");
            in_water |= block::material(sim.world.peek_block(
                h.e.pos[0].floor() as i32,
                h.e.pos[1].floor() as i32,
                h.e.pos[2].floor() as i32,
            )) == block::Material::Water;
            if h.catchable() {
                bit = true;
                break;
            }
        }
        assert!(in_water, "the bobber floats in the pool");
        assert!(saw_wake, "the fish swims up with a wake");
        assert!(bit, "a fish bites within 3000 ticks");
        let items_before = sim.play.items.len();
        let orbs_before = sim
            .play
            .vehicles
            .iter()
            .filter(|v| matches!(v.v, Vehicle::Orb(_)))
            .count();
        let xp_before = sim.splayer.xp_total;
        sim.step(&[Command::UseAir], &mut Vec::new());
        assert!(sim.fish_hook.is_none());
        assert_eq!(
            sim.play.items.len(),
            items_before + 1,
            "the catch flies out"
        );
        let orbs = sim
            .play
            .vehicles
            .iter()
            .filter(|v| matches!(v.v, Vehicle::Orb(_)))
            .count();
        // the orb lands at the player's feet and is usually collected at once
        let gained = sim.splayer.xp_total - xp_before;
        assert!(
            orbs == orbs_before + 1 || (1..=6).contains(&gained),
            "one orb worth 1..6 ({orbs} orbs, {gained} xp)"
        );
        assert_eq!(
            sim.play.inv.main[sel].unwrap().meta,
            1,
            "the rod wears by 1"
        );
        let caught = sim.stats_file.write_stat("stat.fishCaught")
            + sim.stats_file.write_stat("stat.junkFished")
            + sim.stats_file.write_stat("stat.treasureFished");
        assert_eq!(caught, 1);
    }

    /// EntityExpBottle breaks into 3..11 experience; the EntityXPOrbs fly
    /// to the player and onCollideWithPlayer adds them, one per xpCooldown.
    #[test]
    fn experience_orbs_are_collected() {
        use crate::entity::vehicle::Vehicle;
        let (mut sim, x, y, z) = redstone_sim();
        sim.play.mode = GameMode::Survival;
        let feet = [x as f64 + 3.5, y as f64, z as f64 + 3.5];
        for _ in 0..2 {
            sim.step(
                &[Command::Player(feet, [0.0, 90.0], false)],
                &mut Vec::new(),
            );
        }
        let before = sim.splayer.xp_total;
        let sel = sim.play.inv.selected;
        sim.play.inv.main[sel] = Some(crate::item::Stack::new(384, 0, 1));
        let mut ev0 = Vec::new();
        sim.step(&[Command::UseAir], &mut ev0);
        let mut seen = 0;
        let mut collected = ev0
            .iter()
            .filter(|e| matches!(e, Event::CollectOrb(_)))
            .count();
        for _ in 0..100 {
            let mut ev = Vec::new();
            sim.step(&[Command::Player(feet, [0.0, 90.0], false)], &mut ev);
            let n = sim
                .play
                .vehicles
                .iter()
                .filter(|v| matches!(v.v, Vehicle::Orb(_)))
                .count();
            seen = seen.max(n);
            collected += ev
                .iter()
                .filter(|e| matches!(e, Event::CollectOrb(_)))
                .count();
        }
        let _ = seen;
        assert!(collected >= 1, "orbs were picked up");
        let gained = sim.splayer.xp_total - before;
        assert!((3..=11).contains(&gained), "gained {gained}");
        assert!(!sim
            .play
            .vehicles
            .iter()
            .any(|v| matches!(v.v, Vehicle::Orb(_))));
    }

    /// Explosion.doExplosionA: World.getBlockDensity shields a player
    /// behind obsidian; TNT breaks the platform and primes the TNT next to it.
    #[test]
    fn explosions_follow_block_density() {
        let run = |wall: bool| -> (f32, bool, usize) {
            let (mut sim, x, y, z) = redstone_sim();
            sim.play.mode = GameMode::Survival;
            let feet = [x as f64 + 4.5, y as f64, z as f64 + 0.5];
            if wall {
                for dy in 0..4 {
                    for dx in 0..9 {
                        sim.world
                            .set_block(x + dx, y + dy, z + 2, block::OBSIDIAN, 0, 3);
                    }
                }
            }
            sim.world.set_block(x + 4, y, z + 6, block::TNT, 0, 3);
            for _ in 0..2 {
                sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut Vec::new());
            }
            sim.splayer.living.health = 20.0;
            sim.mobs
                .prime_tnt([x as f64 + 4.5, y as f64 + 0.5, z as f64 + 5.5], 0);
            let mut ev = Vec::new();
            for _ in 0..3 {
                sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut ev);
            }
            let broke = sim.world.block(x + 4, y - 1, z + 5) == block::AIR;
            let chained = sim.mobs.tnt.len();
            (20.0 - sim.splayer.living.health, broke, chained)
        };
        let (open, broke, chained) = run(false);
        let (shielded, _, _) = run(true);
        assert!(broke, "the platform under the TNT is gone");
        assert_eq!(chained, 1, "the TNT beside it was primed");
        assert!(open > shielded, "open {open} vs behind obsidian {shielded}");
        assert!(
            shielded <= 1.5,
            "only the +1 of (int)(...+1.0) gets through: {shielded}"
        );
    }

    /// ItemMinecart.onItemUse puts a cart on a rail; moveOnRail keeps it on
    /// the line, a detector rail powers under it, a rider's forward push
    /// starts it from rest, and a broken chest minecart drops the cart, the
    /// chest and its contents.
    #[test]
    fn minecarts_on_rails() {
        use crate::entity::vehicle::Vehicle;
        let (mut sim, x, y, z) = redstone_sim();
        let rx = x + 3;
        for dz in 0..14 {
            let id = if dz == 5 {
                block::DETECTOR_RAIL
            } else {
                block::RAIL
            };
            sim.world.set_block(rx, y, z + dz, id, 0, 3);
        }
        let feet = [rx as f64 + 0.5, y as f64, z as f64 - 1.5];
        for _ in 0..2 {
            sim.step(
                &[Command::Player(feet, [0.0, 30.0], false)],
                &mut Vec::new(),
            );
        }
        let sel = sim.play.inv.selected;
        sim.play.inv.main[sel] = Some(crate::item::Stack::new(328, 0, 1));
        sim.step(
            &[Command::Use {
                target: [rx, y, z + 1],
                face: [0, 1, 0],
                sneaking: false,
                hit: [0.5, 1.0, 0.5],
            }],
            &mut Vec::new(),
        );
        assert_eq!(sim.play.vehicles.len(), 1, "placed");
        let id = sim.play.vehicles[0].v.id();
        sim.play.vehicles[0].v.e_mut().motion = [0.0, 0.0, 0.3];
        let mut powered = false;
        for _ in 0..40 {
            sim.step(
                &[Command::Player(feet, [0.0, 30.0], false)],
                &mut Vec::new(),
            );
            if sim.world.meta(rx, y, z + 5) & 8 != 0 {
                powered = true;
            }
            let p = sim.play.vehicles[0].v.e().pos;
            assert!(
                (p[0] - (rx as f64 + 0.5)).abs() < 1.0e-9,
                "on the line: {:?}",
                p
            );
        }
        assert!(powered, "the detector rail saw it");
        let p = sim.play.vehicles[0].v.e().pos;
        assert!(p[2] > z as f64 + 5.0, "rolled: {:?}", p);
        // the cart stopped against the end; ride it back with forward
        for _ in 0..80 {
            sim.step(
                &[Command::Player(feet, [0.0, 30.0], false)],
                &mut Vec::new(),
            );
        }
        let feet2 = [p[0], y as f64, p[2] - 1.5];
        sim.step(
            &[Command::Player(feet2, [180.0, 0.0], false)],
            &mut Vec::new(),
        );
        sim.step(&[Command::Interact { id }], &mut Vec::new());
        assert_eq!(sim.riding, Some(id));
        let before = sim.play.vehicles[0].v.e().pos;
        for _ in 0..20 {
            sim.step(
                &[
                    Command::Player(feet2, [180.0, 0.0], false),
                    Command::Input {
                        strafe: 0.0,
                        forward: 1.0,
                        jump: false,
                        sneak: false,
                    },
                ],
                &mut Vec::new(),
            );
        }
        let after = sim.play.vehicles[0].v.e().pos;
        assert!(
            after[2] < before[2] - 0.5,
            "pushed north: {:?} -> {:?}",
            before,
            after
        );
        sim.step(
            &[Command::Input {
                strafe: 0.0,
                forward: 0.0,
                jump: false,
                sneak: true,
            }],
            &mut Vec::new(),
        );
        assert_eq!(sim.riding, None);
        // a chest minecart with a stack in it, broken in survival
        sim.play.mode = GameMode::Survival;
        sim.play.inv.main[sel] = Some(crate::item::Stack::new(342, 0, 1));
        let feet3 = [rx as f64 + 0.5, y as f64, z as f64 - 1.5];
        sim.step(
            &[Command::Player(feet3, [0.0, 30.0], false)],
            &mut Vec::new(),
        );
        sim.step(
            &[Command::Use {
                target: [rx, y, z + 1],
                face: [0, 1, 0],
                sneaking: false,
                hit: [0.5, 1.0, 0.5],
            }],
            &mut Vec::new(),
        );
        let chest = sim
            .play
            .vehicles
            .iter_mut()
            .find(|b| matches!(&b.v, Vehicle::Cart(c) if c.kind == crate::entity::minecart::CartKind::Chest))
            .expect("chest minecart");
        let cid = chest.v.id();
        if let Vehicle::Cart(c) = &mut chest.v {
            c.slots[0] = Some(crate::item::Stack::new(block::STONE as u16, 0, 40));
        }
        sim.play.items.clear();
        for _ in 0..5 {
            sim.step(
                &[Command::Attack {
                    id: cid,
                    crit: false,
                    sprint: false,
                }],
                &mut Vec::new(),
            );
        }
        assert!(!sim.play.vehicles.iter().any(|b| b.v.id() == cid), "broken");
        let n = |id: u16| -> u32 {
            sim.play
                .items
                .iter()
                .filter(|i| i.stack.id == id)
                .map(|i| i.stack.count as u32)
                .sum()
        };
        assert_eq!(n(328), 1);
        assert_eq!(n(block::CHEST as u16), 1);
        assert_eq!(n(block::STONE as u16), 40);
    }

    /// EntityBoat.onUpdate: running into a wall faster than 0.2 breaks the
    /// boat into three planks and two sticks; the rider is let go.
    #[test]
    fn boats_wreck_on_walls() {
        let (mut sim, x, y, z) = redstone_sim();
        for dx in 0..13 {
            for dz in 0..15 {
                let edge = dx == 0 || dx == 12 || dz == 0 || dz == 14;
                let id = if edge { block::STONE } else { block::WATER };
                sim.world.set_block(x + dx, y, z + dz, id, 0, 3);
            }
        }
        let feet = [x as f64 + 6.5, (y + 1) as f64, z as f64 + 0.5];
        for _ in 0..2 {
            sim.step(
                &[Command::Player(feet, [0.0, 45.0], false)],
                &mut Vec::new(),
            );
        }
        sim.play.inv.main[sim.play.inv.selected] = Some(crate::item::Stack::new(333, 0, 1));
        sim.step(&[Command::UseAir], &mut Vec::new());
        let id = sim.play.vehicles[0].v.id();
        sim.step(&[Command::Interact { id }], &mut Vec::new());
        assert_eq!(sim.riding, Some(id));
        for _ in 0..80 {
            sim.step(
                &[
                    Command::Player(feet, [0.0, 0.0], false),
                    Command::Input {
                        strafe: 0.0,
                        forward: 1.0,
                        jump: false,
                        sneak: false,
                    },
                ],
                &mut Vec::new(),
            );
        }
        assert!(sim.play.vehicles.is_empty(), "wrecked");
        assert_eq!(sim.riding, None);
        let n = |id: u16| -> u32 {
            sim.play
                .items
                .iter()
                .filter(|i| i.stack.id == id)
                .map(|i| i.stack.count as u32)
                .sum()
        };
        assert_eq!(n(block::PLANKS as u16), 3);
        assert_eq!(n(280), 2);
    }

    /// ItemBoat places a boat on water; interactFirst mounts it, the
    /// rider paddles it with C0CPacketInput, sneaking dismounts, and five
    /// fist hits break it into a boat item.
    #[test]
    fn boats_place_ride_and_break() {
        let (mut sim, x, y, z) = redstone_sim();
        // a pool: stone walls on the platform, water inside
        for dx in 0..13 {
            for dz in 0..15 {
                let edge = dx == 0 || dx == 12 || dz == 0 || dz == 14;
                let id = if edge { block::STONE } else { block::WATER };
                sim.world.set_block(x + dx, y, z + dz, id, 0, 3);
            }
        }
        let feet = [x as f64 + 6.5, (y + 1) as f64, z as f64 + 0.5];
        let mut ev = Vec::new();
        for _ in 0..2 {
            sim.step(&[Command::Player(feet, [0.0, 45.0], false)], &mut ev);
        }
        sim.play.inv.selected = 0;
        sim.play.inv.main[0] = Some(crate::item::Stack::new(333, 0, 1));
        sim.step(&[Command::UseAir], &mut ev);
        assert_eq!(sim.play.vehicles.len(), 1, "placed on the water");
        let b0 = sim.play.vehicles[0].v.e().pos;
        assert_eq!(b0[0], x as f64 + 6.5);
        assert!(b0[2] > feet[2] + 1.0);
        let id = sim.play.vehicles[0].v.id();
        // it settles in the water
        for _ in 0..40 {
            sim.step(&[Command::Player(feet, [0.0, 45.0], false)], &mut ev);
        }
        let mut ev = Vec::new();
        sim.step(&[Command::Interact { id }], &mut ev);
        assert_eq!(sim.riding, Some(id));
        assert!(ev
            .iter()
            .any(|e| matches!(e, Event::Mount(Some(i)) if *i == id)));
        let start = sim.play.vehicles[0].v.e().pos;
        // paddle south
        for _ in 0..25 {
            sim.step(
                &[
                    Command::Player(feet, [0.0, 0.0], false),
                    Command::Input {
                        strafe: 0.0,
                        forward: 1.0,
                        jump: false,
                        sneak: false,
                    },
                ],
                &mut Vec::new(),
            );
        }
        let now = sim.play.vehicles[0].v.e().pos;
        assert!(
            now[2] > start[2] + 1.0,
            "moved south: {:?} -> {:?}",
            start,
            now
        );
        // the rider sits on the boat (updateRiderPosition)
        let r = sim.play.vehicles[0].v.rider_position(-0.5f32 as f64);
        assert!((sim.play.player.0[2] - r[2]).abs() < 1.0);
        // sneak: off
        let mut ev = Vec::new();
        sim.step(
            &[Command::Input {
                strafe: 0.0,
                forward: 0.0,
                jump: false,
                sneak: true,
            }],
            &mut ev,
        );
        assert_eq!(sim.riding, None);
        assert!(ev.iter().any(|e| matches!(e, Event::Mount(None))));
        assert!(ev.iter().any(|e| matches!(e, Event::Teleport(_))));
        // five fist hits in survival
        sim.play.mode = GameMode::Survival;
        for _ in 0..5 {
            sim.step(
                &[Command::Attack {
                    id,
                    crit: false,
                    sprint: false,
                }],
                &mut Vec::new(),
            );
        }
        assert!(sim.play.vehicles.is_empty(), "broken");
        assert!(sim.play.items.iter().any(|i| i.stack.id == 333));
    }

    /// EntityEnderPearl moves the thrower and costs 5 health; a splash
    /// potion of harming thrown straight down hurts the thrower.
    #[test]
    fn pearls_and_splash_potions() {
        let (mut sim, x, y, z) = redstone_sim();
        sim.play.mode = GameMode::Survival;
        let feet = [x as f64 + 0.5, y as f64, z as f64 + 0.5];
        for _ in 0..2 {
            sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut Vec::new());
        }
        sim.play.inv.selected = 0;
        sim.play.inv.main[0] = Some(crate::item::Stack::new(368, 0, 1));
        // A level throw towards a wall five blocks ahead.
        for dx in -3..4 {
            for dy in 0..8 {
                sim.world
                    .set_block(x + dx, y + dy, z + 5, block::STONE, 0, 3);
            }
        }
        let mut moved = None;
        let mut ev = Vec::new();
        sim.step(&[Command::UseAir], &mut ev);
        for e in ev {
            if let Event::Teleport(p) = e {
                moved = Some(p);
            }
        }
        for _ in 0..60 {
            if moved.is_some() {
                break;
            }
            let mut ev = Vec::new();
            sim.step(&[], &mut ev);
            for e in ev {
                if let Event::Teleport(p) = e {
                    moved = Some(p);
                }
            }
            if moved.is_some() {
                break;
            }
        }
        let p = moved.expect("the pearl landed");
        assert!((p[2] - feet[2]).abs() > 1.0, "moved along the throw");
        assert_eq!(sim.splayer.living.health, 15.0);

        // Start the potion check back on the platform. The pearl can leave
        // the player in mid-air, outside the splash's vertical reach.
        sim.splayer.living.hurt_resistant_time = 0;
        let before = sim.splayer.living.health;
        sim.play.inv.main[0] = Some(crate::item::Stack::new(373, 16396, 1));
        sim.step(
            &[Command::Player(feet, [0.0, 90.0], false), Command::UseAir],
            &mut Vec::new(),
        );
        for _ in 0..10 {
            sim.step(
                &[Command::Player(feet, [0.0, 90.0], false)],
                &mut Vec::new(),
            );
        }
        assert!(sim.splayer.living.health < before, "harmed by the splash");
    }

    /// TileEntityPiston.launchWithSlimeBlock moves an item lying in front.
    #[test]
    fn piston_pushes_items() {
        let (mut sim, x, y, z) = redstone_sim();
        let w = &mut sim.world;
        w.set_block(x, y, z, block::PISTON, 5, 3);
        w.set_block(x - 1, y, z, block::STONE, 0, 3);
        w.set_block(x - 1, y + 1, z, block::LEVER, 5, 3);
        let mut r = JavaRandom::new(1);
        let mut it = crate::entity::item::ItemEntity::new(
            999,
            crate::item::Stack::new(crate::item::STICK, 0, 1),
            [x as f64 + 1.5, y as f64 + 0.125, z as f64 + 0.5],
        );
        it.e.motion = [0.0; 3];
        it.pickup_delay = 100;
        sim.play.items.push(it);
        // (other items may lie around: a chicken's egg, say)
        let at = |sim: &Sim| sim.play.items.iter().find(|i| i.id == 999).unwrap().e.pos[0];
        let before = at(&sim);
        sim.world.toggle_lever(x - 1, y + 1, z);
        steps(&mut sim, 3);
        let after = at(&sim);
        assert!(after > before + 0.4, "pushed from {before} to {after}");
    }

    #[test]
    fn powered_tnt_primes_and_explodes() {
        let (mut sim, x, y, z) = redstone_sim();
        sim.world.set_block(x + 5, y, z + 5, block::TNT, 0, 3);
        sim.world
            .set_block(x + 6, y, z + 5, block::REDSTONE_BLOCK, 0, 3);
        assert_eq!(sim.world.block(x + 5, y, z + 5), block::AIR);
        steps(&mut sim, 1);
        assert_eq!(sim.mobs.tnt.len(), 1);
        steps(&mut sim, 85);
        assert!(sim.mobs.tnt.is_empty());
        assert_eq!(
            sim.world.block(x + 5, y - 1, z + 5),
            block::AIR,
            "the floor is blown away"
        );
    }

    #[test]
    fn pressure_plate_opens_door() {
        let (mut sim, x, y, z) = redstone_sim();
        let w = &mut sim.world;
        w.set_block(x, y, z, block::WOODEN_DOOR, 0, 2);
        w.set_block(x, y + 1, z, block::WOODEN_DOOR, 8, 2);
        w.set_block(x + 1, y, z, block::STONE_PRESSURE_PLATE, 0, 3);
        let feet = [x as f64 + 1.5, y as f64, z as f64 + 0.5];
        let mut ev = Vec::new();
        sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut ev);
        sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut ev);
        assert_eq!(sim.world.meta(x + 1, y, z), 1);
        assert_eq!(sim.world.meta(x, y, z) & 4, 4, "door opened");
    }

    /// BlockDoor.onBlockHarvested: a creative player breaking the top
    /// half takes the lower one too, and nothing drops.
    #[test]
    fn creative_door_break_drops_nothing() {
        let (mut sim, x, y, z) = redstone_sim();
        let w = &mut sim.world;
        w.set_block(x, y, z, block::WOODEN_DOOR, 0, 2);
        w.set_block(x, y + 1, z, block::WOODEN_DOOR, 8, 2);
        let feet = [x as f64 + 2.5, y as f64, z as f64 + 0.5];
        let mut ev = Vec::new();
        sim.step(&[Command::SetMode(GameMode::Creative)], &mut ev);
        sim.step(&[Command::Player(feet, [90.0, 0.0], false), Command::Dig([x, y + 1, z])], &mut ev);
        for _ in 0..3 {
            sim.step(&[Command::Player(feet, [90.0, 0.0], false)], &mut ev);
        }
        assert_eq!(sim.world.block(x, y, z), block::AIR);
        assert_eq!(sim.world.block(x, y + 1, z), block::AIR);
        assert!(sim.play.items.is_empty(), "{:?}", sim.play.items.len());
    }

    /// BlockStairs.onBlockPlaced / onBlockPlacedBy, BlockSlab.onBlockPlaced
    /// and ItemSlab.onItemUse: the hit position and the player's yaw decide.
    #[test]
    fn stairs_face_the_player_and_slabs_join() {
        let (mut sim, x, y, z) = redstone_sim();
        for dx in 0..8 {
            for dy in 1..4 {
                for dz in 0..3 {
                    sim.world
                        .set_block(x + dx, y + dy, z + dz, block::AIR, 0, 3);
                }
            }
            sim.world.set_block(x + dx, y, z, block::STONE, 0, 3);
        }
        sim.world.set_block(x + 2, y + 1, z, block::STONE, 0, 3);
        sim.world.set_block(x + 6, y + 1, z, block::STONE, 0, 3);
        let feet = [x as f64 + 0.5, (y + 1) as f64, z as f64 + 8.5];
        let sel = sim.play.inv.selected;
        let mut put =
            |sim: &mut Sim, item: u16, yaw: f32, target: [i32; 3], face: [i32; 3], hy: f32| {
                sim.play.inv.main[sel] = Some(crate::item::Stack::new(item, 0, 8));
                sim.step(
                    &[
                        Command::Player(feet, [yaw, 0.0], false),
                        Command::Use {
                            target,
                            face,
                            sneaking: false,
                            hit: [0.5, hy, 0.5],
                        },
                    ],
                    &mut Vec::new(),
                );
            };
        // on the floor, looking south: ascending to the north
        put(&mut sim, 53, 0.0, [x, y, z], [0, 1, 0], 1.0);
        assert_eq!(sim.world.block(x, y + 1, z), block::OAK_STAIRS);
        assert_eq!(sim.world.meta(x, y + 1, z), 2);
        // on the upper half of a side, looking west: upside down
        put(&mut sim, 53, 90.0, [x + 2, y + 1, z], [0, 0, 1], 0.8);
        assert_eq!(sim.world.block(x + 2, y + 1, z + 1), block::OAK_STAIRS);
        assert_eq!(sim.world.meta(x + 2, y + 1, z + 1), 1 | 4);
        // a slab, then a second one on top of it: the double slab
        put(&mut sim, 44, 0.0, [x + 4, y, z], [0, 1, 0], 1.0);
        assert_eq!(sim.world.block(x + 4, y + 1, z), block::STONE_SLAB);
        assert_eq!(sim.world.meta(x + 4, y + 1, z), 0);
        put(&mut sim, 44, 0.0, [x + 4, y + 1, z], [0, 1, 0], 0.5);
        assert_eq!(sim.world.block(x + 4, y + 1, z), block::DOUBLE_STONE_SLAB);
        // the upper half of a side: the top slab
        put(&mut sim, 126, 0.0, [x + 6, y + 1, z], [0, 0, 1], 0.9);
        assert_eq!(sim.world.block(x + 6, y + 1, z + 1), block::WOODEN_SLAB);
        assert_eq!(sim.world.meta(x + 6, y + 1, z + 1), 8);
    }

    /// BlockTorch (any wall will hold it, even clicking the underside of a
    /// block; fences take one on top), BlockTrapDoor.isValidSupportBlock
    /// (slabs hold one) and BlockEndPortalFrame.onBlockPlacedBy.
    #[test]
    fn torches_trapdoors_and_frames_place_as_the_game_does() {
        let (mut sim, x, y, z) = redstone_sim();
        for dx in -1..10 {
            for dy in 1..5 {
                for dz in -1..3 {
                    sim.world.set_block(x + dx, y + dy, z + dz, block::AIR, 0, 3);
                }
            }
            sim.world.set_block(x + dx, y, z, block::STONE, 0, 3);
        }
        sim.world.set_block(x, y + 3, z, block::STONE, 0, 3);
        sim.world.set_block(x + 1, y + 2, z, block::STONE, 0, 3);
        sim.world.set_block(x + 3, y + 1, z, block::FENCE, 0, 3);
        sim.world.set_block(x + 5, y + 1, z, block::STONE_SLAB, 0, 3);
        let feet = [x as f64 + 4.5, (y + 1) as f64, z as f64 + 6.5];
        let sel = sim.play.inv.selected;
        let mut put = |sim: &mut Sim, item: u16, yaw: f32, target: [i32; 3], face: [i32; 3]| {
            sim.play.inv.main[sel] = Some(crate::item::Stack::new(item, 0, 8));
            sim.step(
                &[
                    Command::Player(feet, [yaw, 0.0], false),
                    Command::Use { target, face, sneaking: false, hit: [0.5, 0.3, 0.5] },
                ],
                &mut Vec::new(),
            );
        };
        put(&mut sim, 50, 0.0, [x, y + 3, z], [0, -1, 0]);
        assert_eq!(sim.world.block(x, y + 2, z), block::TORCH, "under a block");
        assert_eq!(sim.world.meta(x, y + 2, z), 2, "on the wall to the east");
        put(&mut sim, 50, 0.0, [x + 3, y + 1, z], [0, 1, 0]);
        assert_eq!(sim.world.block(x + 3, y + 2, z), block::TORCH, "on a fence");
        assert_eq!(sim.world.meta(x + 3, y + 2, z), 5);
        put(&mut sim, 96, 0.0, [x + 5, y + 1, z], [0, 0, 1]);
        assert_eq!(sim.world.block(x + 5, y + 1, z + 1), block::TRAPDOOR, "on a slab");
        assert_eq!(sim.world.meta(x + 5, y + 1, z + 1), 1);
        put(&mut sim, 120, 90.0, [x + 7, y, z], [0, 1, 0]);
        assert_eq!(sim.world.block(x + 7, y + 1, z), block::END_PORTAL_FRAME);
        assert_eq!(sim.world.meta(x + 7, y + 1, z), 3);
    }

    /// BlockDragonEgg.tryTeleport and BlockRedstoneOre.onBlockActivated.
    #[test]
    fn dragon_egg_hops_and_redstone_ore_glows() {
        let (mut sim, x, y, z) = redstone_sim();
        sim.world.set_block(x, y + 1, z, block::DRAGON_EGG, 0, 3);
        sim.world.set_block(x + 2, y + 1, z, block::REDSTONE_ORE, 0, 3);
        let feet = [x as f64 + 1.5, (y + 1) as f64, z as f64 + 3.5];
        let use_on = |t: [i32; 3]| Command::Use { target: t, face: [0, 0, 1], sneaking: false, hit: [0.5, 0.5, 1.0] };
        sim.step(&[Command::Player(feet, [180.0, 0.0], false), use_on([x, y + 1, z])], &mut Vec::new());
        assert_ne!(sim.world.block(x, y + 1, z), block::DRAGON_EGG, "the egg moved");
        let mut found = false;
        for dx in -16..16 {
            for dy in -8..8 {
                for dz in -16..16 {
                    found |= sim.world.block(x + dx, y + 1 + dy, z + dz) == block::DRAGON_EGG;
                }
            }
        }
        assert!(found, "the egg is somewhere near");
        sim.step(&[use_on([x + 2, y + 1, z])], &mut Vec::new());
        assert_eq!(sim.world.block(x + 2, y + 1, z), block::LIT_REDSTONE_ORE);
    }

    /// BlockCauldron.onBlockActivated: a bucket fills it, a bottle takes a
    /// level as a water bottle, dyed leather is washed.
    #[test]
    fn cauldron_fills_and_empties() {
        let (mut sim, x, y, z) = redstone_sim();
        sim.world.set_block(x, y + 1, z, block::CAULDRON, 0, 3);
        sim.step(&[Command::SetMode(GameMode::Survival)], &mut Vec::new());
        let feet = [x as f64 + 0.5, (y + 1) as f64, z as f64 + 3.5];
        let sel = sim.play.inv.selected;
        let mut use_with = |sim: &mut Sim, s: crate::item::Stack| {
            sim.play.inv.main[sel] = Some(s);
            sim.step(
                &[
                    Command::Player(feet, [180.0, 0.0], false),
                    Command::Use { target: [x, y + 1, z], face: [0, 1, 0], sneaking: false, hit: [0.5, 1.0, 0.5] },
                ],
                &mut Vec::new(),
            );
        };
        use_with(&mut sim, crate::item::Stack::new(326, 0, 1));
        assert_eq!(sim.world.meta(x, y + 1, z), 3);
        assert_eq!(sim.play.inv.main[sel].map(|s| s.id), Some(325), "the bucket is empty");
        use_with(&mut sim, crate::item::Stack::new(374, 0, 2));
        assert_eq!(sim.world.meta(x, y + 1, z), 2);
        assert!(sim.play.inv.main.iter().flatten().any(|s| s.id == 373 && s.meta == 0), "a water bottle");
        let dyed = crate::item::Stack::new(299, 0, 1).edit_tag(|t| {
            use crate::world::anvil::Nbt;
            t.set("display", Nbt::Compound(vec![("color".to_string(), Nbt::Int(0xff0000))]));
        });
        use_with(&mut sim, dyed);
        assert_eq!(sim.world.meta(x, y + 1, z), 1);
        assert_eq!(sim.play.inv.main[sel].and_then(|s| s.dye_color()), None, "washed");
    }

    /// BlockFlowerPot: a sapling goes in an empty pot, a second plant does
    /// not, middle click picks the plant, breaking drops pot and plant.
    #[test]
    fn flower_pots_take_plants() {
        let (mut sim, x, y, z) = redstone_sim();
        sim.world.set_block(x, y + 1, z, block::FLOWER_POT, 0, 3);
        sim.step(&[Command::SetMode(GameMode::Survival)], &mut Vec::new());
        let feet = [x as f64 + 0.5, (y + 1) as f64, z as f64 + 3.5];
        let sel = sim.play.inv.selected;
        let at = [x, y + 1, z];
        let mut use_with = |sim: &mut Sim, s: crate::item::Stack| {
            sim.play.inv.main[sel] = Some(s);
            sim.step(
                &[
                    Command::Player(feet, [180.0, 0.0], false),
                    Command::Use { target: at, face: [0, 1, 0], sneaking: false, hit: [0.5, 0.375, 0.5] },
                ],
                &mut Vec::new(),
            );
        };
        use_with(&mut sim, crate::item::Stack::new(6, 2, 3));
        assert_eq!(sim.play.tiles.get(&at), Some(&crate::play::Tile::FlowerPot { item: 6, data: 2 }));
        assert_eq!(sim.world.meta(x, y + 1, z), 2);
        assert_eq!(sim.play.inv.main[sel].map(|s| s.count), Some(2));
        use_with(&mut sim, crate::item::Stack::new(38, 0, 1));
        assert_eq!(sim.play.tiles.get(&at), Some(&crate::play::Tile::FlowerPot { item: 6, data: 2 }), "full");
        assert_eq!(sim.play.pots_in(x >> 4, z >> 4), vec![([(x & 15) as u8, (y + 1) as u8, (z & 15) as u8], 6, 2)]);
        sim.play.inv.main[sel] = None;
        sim.step(&[Command::Dig(at)], &mut Vec::new());
        let ids: Vec<(u16, i32)> = sim.play.items.iter().map(|e| (e.stack.id, e.stack.meta)).collect();
        assert!(ids.contains(&(6, 2)) && ids.contains(&(390, 0)), "{ids:?}");
        assert!(sim.play.tiles.get(&at).is_none());
        assert_eq!(crate::play::pick_item(140, 0, 0, None), Some((390, 0, false)));
    }

    /// A stone floor with nothing on it around the test site.
    fn arena(sim: &mut Sim, x: i32, y: i32, z: i32) {
        for dx in -2..14 {
            for dz in -2..14 {
                sim.world.set_block(x + dx, y, z + dz, block::STONE, 0, 3);
                for dy in 1..5 {
                    sim.world
                        .set_block(x + dx, y + dy, z + dz, block::AIR, 0, 3);
                }
            }
        }
    }

    /// The task AI (`EntityAITasks`): a zombie takes the nearest player it
    /// can see as its target (`EntityAINearestAttackableTarget`), walks up
    /// (`EntityAIAttackOnCollide`) and hits.
    #[test]
    fn zombie_hunts_the_player() {
        use crate::entity::mob::{ai::Target, Kind};
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        sim.play.mode = GameMode::Survival;
        let feet = [x as f64 + 0.5, (y + 1) as f64, z as f64 + 0.5];
        let mut r = JavaRandom::new(5);
        let id = sim.mobs.spawn(
            Kind::Zombie,
            [x as f64 + 9.5, (y + 1) as f64, z as f64 + 0.5],
            0.0,
            &mut r,
        );
        let before = sim.splayer.living.health;
        let mut targeted = false;
        for _ in 0..200 {
            sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut Vec::new());
            let m = sim.mobs.list.iter().find(|m| m.id == id).unwrap();
            targeted |= m.attack_target == Some(Target::Player);
            if sim.splayer.living.health < before {
                break;
            }
        }
        assert!(targeted, "the zombie chose the player");
        assert!(sim.splayer.living.health < before, "and hit");
    }

    /// `EntityAIBreakDoor`: a door-breaking zombie after a villager shut
    /// behind a wooden door paths through it (`canPassClosedWoodenDoors`),
    /// hits it with the crack growing for 240 ticks, and on Hard breaks it.
    #[test]
    fn zombie_breaks_a_door_on_hard() {
        use crate::entity::mob::Kind;
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        sim.difficulty = crate::entity::living::Difficulty::Hard;
        sim.play.mode = GameMode::Creative;
        for dz in -2..14 {
            for dy in 1..4 {
                sim.world.set_block(x + 4, y + dy, z + dz, block::STONE, 0, 3);
            }
        }
        for dy in 1..4 {
            for (cx, cz) in [(1, 5), (2, 4), (2, 6), (3, 4), (3, 6)] {
                sim.world.set_block(x + cx, y + dy, z + cz, block::STONE, 0, 3);
            }
        }
        sim.world.set_block(x + 4, y + 1, z + 5, block::WOODEN_DOOR, 0, 2);
        sim.world.set_block(x + 4, y + 2, z + 5, block::WOODEN_DOOR, 8, 2);
        let feet = [x as f64 + 12.5, (y + 1) as f64, z as f64 + 12.5];
        let mut r = JavaRandom::new(5);
        sim.mobs.spawn(Kind::Villager, [x as f64 + 2.5, (y + 1) as f64, z as f64 + 5.5], 0.0, &mut r);
        let id = sim.mobs.spawn(Kind::Zombie, [x as f64 + 9.5, (y + 1) as f64, z as f64 + 5.5], 0.0, &mut r);
        sim.mobs.list.iter_mut().find(|m| m.id == id).unwrap().set_break_doors_task(true);
        let (mut cracked, mut knocks) = (0, 0);
        for _ in 0..800 {
            let mut ev = Vec::new();
            sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut ev);
            for e in ev {
                match e {
                    Event::BreakAnims(v) => cracked = cracked.max(v.iter().map(|a| a.2).max().unwrap_or(0)),
                    Event::AuxSounds(v) => knocks += v.iter().filter(|a| a.0 == 1010).count(),
                    _ => {}
                }
            }
            if sim.world.block(x + 4, y + 1, z + 5) != block::WOODEN_DOOR {
                break;
            }
        }
        assert!(cracked >= 9, "the crack grew to {cracked}");
        assert!(knocks > 0, "the zombie knocked");
        assert_ne!(sim.world.block(x + 4, y + 2, z + 5), block::WOODEN_DOOR, "the door broke");
        assert_ne!(sim.world.block(x + 4, y + 1, z + 5), block::WOODEN_DOOR, "both halves");
    }

    /// `EntityZombie.attackEntityFrom` on Hard: a zombie whose
    /// reinforcement chance comes up calls a new zombie to a dark spot 7 to
    /// 40 blocks off, after the attacker; each pays 0.05 of its chance.
    #[test]
    fn hurt_zombie_calls_reinforcements_on_hard() {
        use crate::entity::mob::{ai::Target, Kind};
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        sim.difficulty = crate::entity::living::Difficulty::Hard;
        sim.world_time = 18000;
        sim.play.mode = GameMode::Survival;
        let feet = [x as f64 + 0.5, (y + 1) as f64, z as f64 + 0.5];
        sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut Vec::new());
        let mut r = JavaRandom::new(5);
        let id = sim.mobs.spawn(Kind::Zombie, [x as f64 + 6.5, (y + 1) as f64, z as f64 + 6.5], 0.0, &mut r);
        let callee = |m: &&crate::entity::mob::Mob| m.reinforce_mods.iter().any(|x| x.0.ends_with("callee charge"));
        let mut called = false;
        for _ in 0..40 {
            let m = sim.mobs.list.iter_mut().find(|m| m.id == id).unwrap();
            m.reinforce_base = 1.0;
            m.reinforce_mods.clear();
            m.health = 20.0;
            m.hurt_src = Some(Target::Player);
            assert!(m.hurt(1.0, None, true));
            // past the hurt-resistance window
            for _ in 0..11 {
                sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut Vec::new());
            }
            if sim.mobs.list.iter().any(|m| callee(&m)) {
                called = true;
                break;
            }
        }
        assert!(called, "a reinforcement came");
        let new = sim.mobs.list.iter().find(callee).unwrap();
        assert_eq!(new.kind, Kind::Zombie);
        assert_eq!(new.attack_target, Some(Target::Player));
        let caller = sim.mobs.list.iter().find(|m| m.id == id).unwrap();
        assert!(caller.reinforce_mods.iter().any(|x| x.0.ends_with("caller charge")));
    }

    /// EntitySilverfish: an idle one crawls into the stone beside it, and a
    /// hurt one wakes the ones hiding near by (breaking their blocks under
    /// mobGriefing).
    #[test]
    fn silverfish_hide_and_call_for_help() {
        use crate::entity::mob::{ai::Target, Kind};
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        sim.play.mode = GameMode::Creative;
        let feet = [x as f64 + 12.5, (y + 1) as f64, z as f64 + 12.5];
        // a stone pit, so every side is stone
        for dx in -1..=1 {
            for dz in -1..=1 {
                for dy in 0..3 {
                    if (dx, dz) != (0, 0) || dy == 0 {
                        sim.world.set_block(x + 5 + dx, y + 1 + dy, z + 5 + dz, block::STONE, 0, 3);
                    }
                }
            }
        }
        let mut r = JavaRandom::new(5);
        let id = sim.mobs.spawn(Kind::Silverfish, [x as f64 + 5.5, (y + 2) as f64, z as f64 + 5.5], 0.0, &mut r);
        for _ in 0..200 {
            sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut Vec::new());
            if !sim.mobs.list.iter().any(|m| m.id == id && !m.dead) {
                break;
            }
        }
        let mut eggs = Vec::new();
        for dx in -1..=1 {
            for dz in -1..=1 {
                for dy in 0..3 {
                    let p = [x + 5 + dx, y + 1 + dy, z + 5 + dz];
                    if sim.world.block(p[0], p[1], p[2]) == block::MONSTER_EGG {
                        eggs.push(p);
                    }
                }
            }
        }
        assert_eq!(eggs.len(), 1, "it hid in the stone");
        let egg = eggs[0];
        // a second one, hurt by the player, calls the first out; it stands
        // on planks so it has no stone to hide in
        for dx in -3..=3 {
            for dz in -3..=3 {
                sim.world.set_block(x + 9 + dx, y, z + 9 + dz, block::PLANKS, 0, 3);
            }
        }
        let id2 = sim.mobs.spawn(Kind::Silverfish, [x as f64 + 9.5, (y + 1) as f64, z as f64 + 9.5], 0.0, &mut r);
        let m = sim.mobs.list.iter_mut().find(|m| m.id == id2).unwrap();
        m.hurt_src = Some(Target::Player);
        m.hurt(1.0, None, true);
        for _ in 0..30 {
            sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut Vec::new());
        }
        // it came out, breaking its block (and, walled in by stone, may
        // hide again beside it)
        assert_eq!(sim.world.block(egg[0], egg[1], egg[2]), block::AIR, "the hidden one came out");
    }

    /// A hoe tills grass, seeds go on farmland, bone meal grows the crop,
    /// random ticks grow it further (`BlockCrops.updateTick`), farmland
    /// without water dries, and the crop drops when its soil goes.
    #[test]
    fn farming_from_hoe_to_harvest() {
        let (mut sim, x, y, z) = redstone_sim();
        sim.play.mode = GameMode::Survival;
        let sel = sim.play.inv.selected;
        let feet = [x as f64 + 0.5, y as f64, z as f64 - 1.5];
        let mut ev = Vec::new();
        sim.step(&[Command::Player(feet, [0.0, 30.0], false)], &mut ev);
        let use_on = |sim: &mut Sim, p: [i32; 3]| {
            sim.step(
                &[Command::Use {
                    target: p,
                    face: [0, 1, 0],
                    sneaking: false,
                    hit: [0.5, 1.0, 0.5],
                }],
                &mut Vec::new(),
            );
        };
        let soil = [x, y - 1, z];
        sim.world
            .set_block(soil[0], soil[1], soil[2], block::GRASS, 0, 3);
        sim.play.inv.main[sel] = Some(crate::item::Stack::new(291, 0, 1));
        use_on(&mut sim, soil);
        assert_eq!(sim.world.block(soil[0], soil[1], soil[2]), block::FARMLAND);
        assert_eq!(sim.play.inv.main[sel].unwrap().meta, 1, "the hoe wears");
        sim.play.inv.main[sel] = Some(crate::item::Stack::new(295, 0, 3));
        use_on(&mut sim, soil);
        assert_eq!(sim.world.block(x, y, z), block::WHEAT);
        assert_eq!(sim.play.inv.main[sel].unwrap().count, 2);
        // bone meal: 2..5 stages at a time
        sim.play.inv.main[sel] = Some(crate::item::Stack::new(351, 15, 1));
        use_on(&mut sim, [x, y, z]);
        let m = sim.world.meta(x, y, z);
        assert!((2..=5).contains(&m), "{m}");
        assert!(sim.play.inv.main[sel].is_none());
        // random ticks in full light finish it
        let mut r = JavaRandom::new(3);
        for _ in 0..2000 {
            sim.world.growth_tick(block::WHEAT, x, y, z, &mut r);
        }
        assert_eq!(sim.world.meta(x, y, z), 7);
        // dry farmland: the crop keeps it from turning to dirt
        sim.world.set_meta(soil[0], soil[1], soil[2], 0, 2);
        sim.world
            .growth_tick(block::FARMLAND, soil[0], soil[1], soil[2], &mut r);
        assert_eq!(sim.world.block(soil[0], soil[1], soil[2]), block::FARMLAND);
        // water nearby wets it
        sim.world.set_block(x + 3, y - 1, z, block::WATER, 0, 3);
        sim.world
            .growth_tick(block::FARMLAND, soil[0], soil[1], soil[2], &mut r);
        assert_eq!(sim.world.meta(soil[0], soil[1], soil[2]), 7);
        // the soil turned to stone: the wheat drops
        sim.world.gen_entities.clear();
        sim.world
            .set_block(soil[0], soil[1], soil[2], block::STONE, 0, 3);
        assert_eq!(sim.world.block(x, y, z), block::AIR);
        assert!(sim.world.gen_entities.iter().any(|e| matches!(
            e,
            crate::world::server::GenEntity::ScatteredItem { stack, .. } if stack.id == 296
        )));
    }

    /// A pumpkin stem grows to full and puts a pumpkin beside itself; an
    /// oak sapling grows into a tree.
    #[test]
    fn stems_bear_fruit_and_saplings_grow() {
        let (mut sim, x, y, z) = redstone_sim();
        let mut r = JavaRandom::new(9);
        for dx in -1..=1 {
            for dz in -1..=1 {
                sim.world
                    .set_block(x + 4 + dx, y - 1, z + 4 + dz, block::DIRT, 0, 3);
            }
        }
        sim.world
            .set_block(x + 4, y - 1, z + 4, block::FARMLAND, 7, 3);
        sim.world
            .set_block(x + 4, y, z + 4, block::PUMPKIN_STEM, 0, 3);
        for _ in 0..5000 {
            sim.world
                .growth_tick(block::PUMPKIN_STEM, x + 4, y, z + 4, &mut r);
        }
        assert_eq!(sim.world.meta(x + 4, y, z + 4), 7);
        let fruit = [(-1, 0), (1, 0), (0, -1), (0, 1)]
            .iter()
            .filter(|(dx, dz)| sim.world.block(x + 4 + dx, y, z + 4 + dz) == block::PUMPKIN)
            .count();
        assert_eq!(fruit, 1);
        // a sapling on dirt: marked, then grown
        sim.world.set_block(x + 8, y - 1, z + 8, block::DIRT, 0, 3);
        sim.world.set_block(x + 8, y, z + 8, block::SAPLING, 0, 3);
        let mut grew = false;
        for _ in 0..40 {
            sim.world.mark_or_grow_sapling(x + 8, y, z + 8, &mut r);
            if sim.world.block(x + 8, y, z + 8) == block::LOG {
                grew = true;
                break;
            }
        }
        assert!(grew, "the sapling became a trunk");
    }

    /// Cake slices feed a hungry player; a fence gate swings open and shut.
    #[test]
    fn cake_and_fence_gate() {
        let (mut sim, x, y, z) = redstone_sim();
        sim.play.mode = GameMode::Survival;
        let feet = [x as f64 + 0.5, y as f64, z as f64 - 1.5];
        sim.step(
            &[Command::Player(feet, [0.0, 30.0], false)],
            &mut Vec::new(),
        );
        sim.world.set_block(x, y, z, block::CAKE, 0, 3);
        sim.splayer.food.level = 10;
        let sel = sim.play.inv.selected;
        sim.play.inv.main[sel] = None;
        let use_on = |sim: &mut Sim, p: [i32; 3]| {
            sim.step(
                &[Command::Use {
                    target: p,
                    face: [0, 0, -1],
                    sneaking: false,
                    hit: [0.5, 0.5, 0.0],
                }],
                &mut Vec::new(),
            );
        };
        use_on(&mut sim, [x, y, z]);
        assert_eq!(sim.world.meta(x, y, z), 1);
        assert_eq!(sim.splayer.food.level, 12);
        // a left click eats too (onBlockClicked)
        sim.step(&[Command::ClickBlock([x, y, z], 2)], &mut Vec::new());
        assert_eq!(sim.world.meta(x, y, z), 2);
        sim.world.set_block(x + 2, y, z, block::FENCE_GATE, 0, 3);
        use_on(&mut sim, [x + 2, y, z]);
        assert_eq!(sim.world.meta(x + 2, y, z) & 4, 4, "open");
        let m = sim.world.meta(x + 2, y, z) as u8;
        let mut out = Vec::new();
        {
            let w = &sim.world;
            let get = |a: i32, b: i32, c: i32| (w.peek_block(a, b, c), w.peek_meta(a, b, c));
            crate::shapes::collision(&get, block::FENCE_GATE, m, x + 2, y, z, &mut out);
        }
        assert!(out.is_empty(), "an open gate lets you through");
        use_on(&mut sim, [x + 2, y, z]);
        assert_eq!(sim.world.meta(x + 2, y, z) & 4, 0, "shut");
    }

    /// A bed at night: the player lies down, the night is skipped once
    /// fully asleep, and the bed becomes the spawn point.
    #[test]
    fn sleeping_skips_the_night() {
        let (mut sim, x, y, z) = redstone_sim();
        sim.play.mode = GameMode::Survival;
        sim.world_time = 18000;
        let feet = [x as f64 + 0.5, y as f64, z as f64 - 1.0];
        sim.step(
            &[Command::Player(feet, [0.0, 30.0], false)],
            &mut Vec::new(),
        );
        sim.mobs.list.clear();
        let sel = sim.play.inv.selected;
        sim.play.inv.main[sel] = Some(crate::item::Stack::new(355, 0, 1));
        sim.step(
            &[Command::Use {
                target: [x, y - 1, z],
                face: [0, 1, 0],
                sneaking: false,
                hit: [0.5, 1.0, 0.5],
            }],
            &mut Vec::new(),
        );
        // yaw 0 looks to +z: the foot here, the head beyond
        assert_eq!(sim.world.block(x, y, z), block::BED);
        assert_eq!(sim.world.meta(x, y, z + 1), 8);
        sim.mobs.list.clear();
        let mut ev = Vec::new();
        sim.step(
            &[Command::Use {
                target: [x, y, z],
                face: [0, 1, 0],
                sneaking: false,
                hit: [0.5, 0.5, 0.5],
            }],
            &mut ev,
        );
        assert_eq!(sim.sleeping, Some([x, y, z + 1]));
        assert!(ev.iter().any(|e| matches!(e, Event::Sleep(Some(_)))));
        assert_eq!(sim.world.meta(x, y, z + 1) & 4, 4, "occupied");
        steps(&mut sim, 101);
        assert!(sim.sleeping.is_none(), "awake");
        assert_eq!(sim.world_time, 24000);
        assert_eq!(sim.bed_spawn, Some([x, y, z + 1]));
        assert_eq!(sim.world.meta(x, y, z + 1) & 4, 0);
        // in daylight the bed says no
        sim.step(
            &[Command::Use {
                target: [x, y, z],
                face: [0, 1, 0],
                sneaking: false,
                hit: [0.5, 0.5, 0.5],
            }],
            &mut Vec::new(),
        );
        assert!(sim.sleeping.is_none());
    }

    /// `attackTargetEntityWithCurrentItem` with an enchanted sword
    /// (Sharpness, Smite, Fire Aspect, Knockback, the zombie's 2 armour),
    /// Feather Falling on the player's boots and an enchanted bow.
    #[test]
    fn enchanted_weapons_and_armour() {
        use crate::enchant::*;
        use crate::entity::living::{Difficulty, Source, SourceKind};
        use crate::entity::mob::Kind;
        use crate::item::Stack;
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        sim.play.mode = GameMode::Survival;
        let feet = [x as f64 + 0.5, (y + 1) as f64, z as f64 + 0.5];
        let mut r = JavaRandom::new(5);
        let at = [x as f64 + 2.5, (y + 1) as f64, z as f64 + 0.5];
        let a = sim.mobs.spawn(Kind::Zombie, at, 0.0, &mut r);
        let b = sim
            .mobs
            .spawn(Kind::Zombie, [at[0], at[1], at[2] + 6.0], 0.0, &mut r);
        let sel = sim.play.inv.selected;
        let sword = Stack::new(276, 0, 1).with_enchantments(&[
            (SHARPNESS, 5),
            (FIRE_ASPECT, 2),
            (KNOCKBACK, 1),
        ]);
        sim.play.inv.main[sel] = Some(sword);
        sim.step(
            &[
                Command::Player(feet, [-90.0, 0.0], false),
                Command::Attack {
                    id: a,
                    crit: false,
                    sprint: false,
                },
            ],
            &mut Vec::new(),
        );
        let m = sim.mobs.list.iter().find(|m| m.id == a).unwrap();
        // (1 + 7 + 5 * 1.25) through 2 points of armour
        let want = 20.0 - (14.25f32 * 23.0) / 25.0;
        assert_eq!(m.health, want);
        assert!(m.fire > 140, "fire aspect: {}", m.fire);
        assert!(m.motion[0].abs() > 0.4, "knockback: {:?}", m.motion);
        assert_eq!(sim.play.inv.main[sel].unwrap().meta, 1, "the sword wears");
        // Smite V against the undead: 1 + 7 + 12.5
        sim.play.inv.main[sel] = Some(Stack::new(276, 0, 1).with_enchantments(&[(SMITE, 5)]));
        sim.step(
            &[
                Command::Player(feet, [0.0, 0.0], false),
                Command::Attack {
                    id: b,
                    crit: false,
                    sprint: false,
                },
            ],
            &mut Vec::new(),
        );
        let m = sim.mobs.list.iter().find(|m| m.id == b).unwrap();
        assert_eq!(m.health, 20.0 - (20.5f32 * 23.0) / 25.0);
        // a 10-point fall: Feather Falling IV adds 18, halved to 9..18
        sim.play.inv.armor[3] =
            Some(Stack::new(313, 0, 1).with_enchantments(&[(FEATHER_FALLING, 4)]));
        sim.armor_in();
        sim.splayer.living.health = 20.0;
        sim.splayer.living.hurt_resistant_time = 0;
        sim.splayer.attack(
            &sim.world,
            Source::named(SourceKind::Fall),
            10.0,
            Difficulty::Normal,
        );
        let h = sim.splayer.living.health;
        assert!(
            (20.0 - 10.0 * 16.0 / 25.0..=20.0 - 10.0 * 7.0 / 25.0).contains(&h),
            "{h}"
        );
        // Power V, Punch II, Flame, Infinity: no arrow needed
        let bow = Stack::new(261, 0, 1).with_enchantments(&[
            (POWER, 5),
            (PUNCH, 2),
            (FLAME, 1),
            (INFINITY, 1),
        ]);
        sim.play.inv.main[sel] = Some(bow);
        let before = sim.mobs.arrows.len();
        sim.fire_bow(bow, 72000 - 20);
        assert_eq!(sim.mobs.arrows.len(), before + 1);
        let arrow = sim.mobs.arrows.last().unwrap();
        assert_eq!(arrow.damage, 2.0 + 5.0 * 0.5 + 0.5);
        assert_eq!(arrow.knockback, 2);
        assert_eq!(arrow.pickup, 2);
        assert_eq!(arrow.e.fire, 2000);
        assert_eq!(sim.play.inv.main[sel].unwrap().meta, 1, "the bow wears");
    }

    /// The old AI (`EntityCreature.updateEntityActionState`): a zombie
    /// pigman leaves the player alone until it is hit, then comes for it.
    #[test]
    fn pigman_is_neutral_until_hit() {
        use crate::entity::mob::Kind;
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        sim.play.mode = GameMode::Survival;
        let feet = [x as f64 + 0.5, (y + 1) as f64, z as f64 + 0.5];
        let mut r = JavaRandom::new(5);
        let id = sim.mobs.spawn(
            Kind::PigZombie,
            [x as f64 + 3.5, (y + 1) as f64, z as f64 + 0.5],
            0.0,
            &mut r,
        );
        let before = sim.splayer.living.health;
        for _ in 0..100 {
            sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut Vec::new());
        }
        assert_eq!(sim.splayer.living.health, before, "left alone");
        sim.step(
            &[
                Command::Player(feet, [0.0, 0.0], false),
                Command::Attack {
                    id,
                    crit: false,
                    sprint: false,
                },
            ],
            &mut Vec::new(),
        );
        for _ in 0..200 {
            sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut Vec::new());
            if sim.splayer.living.health < before {
                break;
            }
        }
        assert!(sim.splayer.living.health < before, "struck back");
    }

    /// `EntityAIOwnerHurtTarget`: a tamed wolf goes for what its owner
    /// hit. `EntityAINearestAttackableTarget` with `IMob.mobSelector`: an
    /// iron golem goes for a zombie and hurts it.
    #[test]
    fn wolves_and_golems_pick_their_targets() {
        use crate::entity::mob::{ai::Target, Kind};
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        // a roof, so the zombie does not burn up, and walls, so it is not
        // knocked off the edge into the world below
        for dx in -2..14 {
            for dz in -2..14 {
                sim.world
                    .set_block(x + dx, y + 5, z + dz, block::STONE, 0, 3);
                if dx == -2 || dx == 13 || dz == -2 || dz == 13 {
                    for dy in 1..5 {
                        sim.world
                            .set_block(x + dx, y + dy, z + dz, block::STONE, 0, 3);
                    }
                }
            }
        }
        let feet = [x as f64 + 0.5, (y + 1) as f64, z as f64 + 0.5];
        let mut r = JavaRandom::new(5);
        let at = |dx: f64, dz: f64| [x as f64 + dx, (y + 1) as f64, z as f64 + dz];
        let wolf = sim.mobs.spawn(Kind::Wolf, at(2.5, 2.5), 0.0, &mut r);
        sim.mobs
            .list
            .iter_mut()
            .find(|m| m.id == wolf)
            .unwrap()
            .tamed = true;
        let zombie = sim.mobs.spawn(Kind::Zombie, at(8.5, 2.5), 0.0, &mut r);
        for _ in 0..5 {
            sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut Vec::new());
        }
        sim.step(
            &[
                Command::Player(feet, [0.0, 0.0], false),
                Command::Attack {
                    id: zombie,
                    crit: false,
                    sprint: false,
                },
            ],
            &mut Vec::new(),
        );
        let mut went = false;
        for _ in 0..40 {
            sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut Vec::new());
            let w = sim.mobs.list.iter().find(|m| m.id == wolf).unwrap();
            went |= w.attack_target == Some(Target::Mob(zombie));
        }
        assert!(went, "the wolf went for the zombie its owner hit");

        let golem = sim.mobs.spawn(Kind::IronGolem, at(6.5, 8.5), 0.0, &mut r);
        let mut hunted = false;
        for _ in 0..200 {
            sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut Vec::new());
            if let Some(g) = sim.mobs.list.iter().find(|m| m.id == golem) {
                hunted |= matches!(g.attack_target, Some(Target::Mob(_)));
            }
            if !sim
                .mobs
                .list
                .iter()
                .any(|m| m.id == zombie && m.health > 0.0)
            {
                break;
            }
        }
        assert!(hunted, "the golem chose a monster");
        assert!(
            !sim.mobs
                .list
                .iter()
                .any(|m| m.id == zombie && m.health > 0.0),
            "and the zombie is dead"
        );
    }

    /// `EntityAnimal.interact` and `EntityAIMate`: two fed cows come
    /// together and have one calf; the parents cannot breed for a while.
    /// `EntityLiving.onLivingUpdate`'s looting: a zombie that can pick up
    /// loot takes a sword, then a sharper one (dropping the first, whose
    /// drop chance became 2), and a helmet into slot 4; one that cannot
    /// leaves them.
    #[test]
    fn zombies_pick_up_loot() {
        use crate::entity::mob::Kind;
        use crate::item::Stack;
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        let feet = [x as f64 + 0.5, (y + 1) as f64, z as f64 + 0.5];
        let mut r = JavaRandom::new(5);
        let at = [x as f64 + 8.5, (y + 1) as f64, z as f64 + 8.5];
        let zid = sim.mobs.spawn(Kind::Zombie, at, 0.0, &mut r);
        if let Some(m) = sim.mobs.list.iter_mut().find(|m| m.id == zid) {
            m.can_pick_up_loot = true;
            m.equipment[4] = Some(Stack::new(310, 0, 1));
            m.potions.add_effect(crate::potion::Effect::new(
                crate::potion::MOVE_SLOWDOWN,
                9999,
                9,
            ));
        }
        let drop = |sim: &mut Sim, id: u16| {
            let eid = sim.play.next_item;
            sim.play.next_item += 1;
            let mut e = crate::entity::item::ItemEntity::new(eid, Stack::new(id, 0, 1), at);
            e.e.motion = [0.0; 3];
            sim.play.spawn(e);
        };
        let run = |sim: &mut Sim| {
            for _ in 0..10 {
                sim.step(&[Command::Player(feet, [0.0, 0.0], true)], &mut Vec::new());
            }
        };
        let held = |sim: &Sim| {
            sim.mobs
                .list
                .iter()
                .find(|m| m.id == zid)
                .and_then(|m| m.equipment[0])
                .map(|s| s.id)
        };
        drop(&mut sim, 272); // stone sword
        run(&mut sim);
        assert_eq!(held(&sim), Some(272));
        drop(&mut sim, 268); // wooden: worse, stays on the ground
        run(&mut sim);
        assert_eq!(held(&sim), Some(272));
        drop(&mut sim, 276); // diamond: better
        run(&mut sim);
        assert_eq!(held(&sim), Some(276));
        let ground: Vec<u16> = sim
            .play
            .items
            .iter()
            .filter(|e| !e.e.dead)
            .map(|e| e.stack.id)
            .collect();
        assert!(
            ground.contains(&268) && ground.contains(&272),
            "left and dropped: {ground:?}"
        );
        let m = sim.mobs.list.iter().find(|m| m.id == zid).unwrap();
        assert!(m.persistent);
        assert_eq!(m.drop_chances[0], 2.0);
    }

    /// `EntityZombie.setChild`: half the size, the 50% speed boost, no
    /// burning in the sun, higher voices, 12 experience, saved as IsBaby.
    #[test]
    fn baby_zombies() {
        use crate::entity::mob::{Kind, Mob, Mobs};
        let mut m = Mob::new(1, Kind::Zombie, [0.5, 70.0, 0.5], 0.0, 9);
        let speed = m.speed_attr();
        m.set_baby(true);
        assert_eq!(m.dims(), (0.3, 0.9));
        assert!((m.speed_attr() - speed * 1.5).abs() < 1e-6);
        let (_, pitch) = m.sound_volume_pitch();
        assert!((1.3..=1.7).contains(&pitch), "pitch {pitch}");
        let tag = Mobs::to_nbt(&m);
        let mut back = Mobs::default();
        back.from_nbt(&tag).unwrap();
        assert!(back.list[0].baby);
        assert_eq!(back.list[0].dims(), (0.3, 0.9));
    }

    /// `Entity.moveEntity`'s end on the server: a player or a mob standing
    /// in fire takes `inFire` and, after `fireResistance` ticks (20 for the
    /// player, 1 for mobs), burns for 8 seconds, taking `onFire` with a hurt
    /// sound; a cactus pricks a mob; water puts the burning out.
    #[test]
    fn fire_and_cactus_hurt() {
        use crate::entity::mob::Kind;
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        sim.play.mode = GameMode::Survival;
        sim.splayer.creative = false;
        let feet = [x as f64 + 0.5, (y + 1) as f64, z as f64 + 0.5];
        let here = Command::Player(feet, [0.0, 0.0], true);
        sim.step(&[here.clone()], &mut Vec::new());
        sim.splayer.respawn_invulnerable = 0;
        let mut r = JavaRandom::new(5);
        let zat = [x as f64 + 4.5, (y + 1) as f64, z as f64 + 4.5];
        let zom = sim.mobs.spawn(Kind::Zombie, zat, 0.0, &mut r);
        let cow = sim.mobs.spawn(
            Kind::Cow,
            [x as f64 + 8.5, (y + 1) as f64, z as f64 + 8.5],
            0.0,
            &mut r,
        );
        for m in sim.mobs.list.iter_mut() {
            m.equipment[4] = Some(crate::item::Stack::new(310, 0, 1));
            m.persistent = true;
        }
        let calm = Command::PlayerState { attackable: false };
        let mut heard = false;
        for i in 0..30 {
            // keep the fire lit (from the second tick: one move outside
            // it first sets fire to -fireResistance) and the mobs in place
            if i > 0 {
                sim.world.set_block(x, y + 1, z, block::FIRE, 0, 3);
                sim.world.set_block(x + 4, y + 1, z + 4, block::FIRE, 0, 3);
            }
            sim.world
                .set_block(x + 9, y + 1, z + 8, block::CACTUS, 0, 3);
            for m in sim.mobs.list.iter_mut() {
                if m.id == zom {
                    m.set_position(zat[0], zat[1], zat[2]);
                } else if m.id == cow {
                    let p = [x as f64 + 8.75, (y + 1) as f64, z as f64 + 8.5];
                    m.set_position(p[0], p[1], p[2]);
                    m.motion = [0.2, 0.0, 0.0];
                }
            }
            let mut ev = Vec::new();
            sim.step(&[calm.clone(), here.clone()], &mut ev);
            heard |= ev.iter().any(
                |e| matches!(e, Event::Sounds(v) if v.iter().any(|s| s.0 == "mob.zombie.hurt")),
            );
        }
        assert!(sim.splayer.living.health < 20.0, "fire hurt the player");
        assert!(
            sim.splayer.e.fire > 100,
            "the player caught fire: {}",
            sim.splayer.e.fire
        );
        let get = |id: u32| sim.mobs.list.iter().find(|m| m.id == id).unwrap();
        assert!(get(zom).health < 20.0);
        assert!(get(zom).fire > 100, "the zombie burns: {}", get(zom).fire);
        assert!(heard, "the zombie cried out");
        assert!(get(cow).health < 10.0, "the cactus pricked the cow");
    }

    /// `collideWithNearbyEntities`: two cows put in the same place push
    /// each other apart, and a cow standing in the player pushes the player
    /// (`addVelocity`, sent on as a velocity to add).
    #[test]
    fn mobs_push_each_other_and_the_player() {
        use crate::entity::mob::Kind;
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        let feet = [x as f64 + 0.5, (y + 1) as f64, z as f64 + 0.5];
        let mut r = JavaRandom::new(5);
        let a = sim.mobs.spawn(
            Kind::Cow,
            [x as f64 + 6.5, (y + 1) as f64, z as f64 + 6.5],
            0.0,
            &mut r,
        );
        let b = sim.mobs.spawn(
            Kind::Cow,
            [x as f64 + 6.6, (y + 1) as f64, z as f64 + 6.5],
            0.0,
            &mut r,
        );
        let c = sim
            .mobs
            .spawn(Kind::Cow, [feet[0] + 0.3, feet[1], feet[2]], 0.0, &mut r);
        let _ = c;
        let mut pushed = false;
        for _ in 0..10 {
            let mut ev = Vec::new();
            sim.step(&[Command::Player(feet, [0.0, 0.0], true)], &mut ev);
            for e in &ev {
                if let Event::Velocity { add, .. } = e {
                    pushed |= add[0] < 0.0;
                }
            }
        }
        let pos = |id: u32| sim.mobs.list.iter().find(|m| m.id == id).unwrap().pos;
        let gap = (pos(b)[0] - pos(a)[0]).abs();
        assert!(gap > 0.3, "the cows moved apart: {gap}");
        assert!(pushed, "the cow pushed the player away (-x)");
    }

    /// The client turns a task-AI mob's body after its head
    /// (`EntityBodyHelper`): a standing cow whose head the server turned
    /// 150 degrees from its facing ends up with its body within 75 degrees
    /// of the head, then lined up with it, instead of the head twisting
    /// round on a body that stays put. A spider (old AI) keeps
    /// `updateDistance`'s body near its facing.
    #[test]
    fn client_body_follows_the_head() {
        use crate::entity::mob::Kind;
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        let feet = [x as f64 + 0.5, (y + 1) as f64, z as f64 + 0.5];
        let mut r = JavaRandom::new(5);
        let cow = sim.mobs.spawn(
            Kind::Cow,
            [x as f64 + 4.5, (y + 1) as f64, z as f64 + 4.5],
            0.0,
            &mut r,
        );
        let mut view = None;
        for _ in 0..3 {
            let mut ev = Vec::new();
            sim.step(&[Command::Player(feet, [0.0, 0.0], true)], &mut ev);
            for e in ev {
                if let Event::Mobs(v, _) = e {
                    view = v.into_iter().find(|m| m.id == cow).or(view);
                }
            }
        }
        let mut m = view.expect("the cow is seen");
        m.yaw = 0.0;
        m.body_yaw = 0.0;
        m.head_yaw = 150.0;
        m.body_head = 0.0;
        m.body_ticks = 0;
        let mut worst: f32 = 0.0;
        for _ in 0..40 {
            crate::game::update_body(&mut m, 0.0, 0.0);
            worst = worst.max((m.head_yaw - m.body_yaw).abs());
        }
        assert!(
            worst <= 75.0 + 1e-3,
            "head never more than 75 from the body: {worst}"
        );
        assert!(
            (m.head_yaw - m.body_yaw).abs() < 1e-3,
            "lined up after standing still"
        );
        let mut s = m.clone();
        s.kind = Kind::Spider;
        s.body_yaw = 0.0;
        for _ in 0..40 {
            crate::game::update_body(&mut s, 0.0, 0.0);
        }
        assert!(
            s.body_yaw.abs() <= 75.0,
            "the old AI keeps its body near its facing"
        );
    }

    /// Mobs move through `Entity.moveEntity`: a zombie falling ten blocks
    /// onto farmland plays the hostile fall sound, takes `ceil(fall - 3)`,
    /// tramples the farmland, raises landing dust (`playAuxSFX(2006)`) and
    /// steps with its own step sound; a cow held under water runs out of
    /// air after 300 ticks and drowns two points at a time.
    #[test]
    fn mobs_fall_step_and_drown() {
        use crate::entity::mob::Kind;
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        for dx in -2..14 {
            for dz in -2..14 {
                for dy in 5..16 {
                    sim.world
                        .set_block(x + dx, y + dy, z + dz, block::AIR, 0, 3);
                }
            }
        }
        sim.world.set_block(x + 4, y, z + 4, block::FARMLAND, 0, 3);
        // a flooded cell with a lid for the cow
        for dx in 7..12 {
            for dz in 7..12 {
                for dy in 1..4 {
                    let edge = dx == 7 || dx == 11 || dz == 7 || dz == 11 || dy == 3;
                    let b = if edge { block::STONE } else { block::WATER };
                    sim.world.set_block(x + dx, y + dy, z + dz, b, 0, 3);
                }
            }
        }
        let feet = [x as f64 + 0.5, (y + 1) as f64, z as f64 + 0.5];
        let mut r = JavaRandom::new(5);
        let zom = sim.mobs.spawn(
            Kind::Zombie,
            [x as f64 + 4.5, (y + 11) as f64, z as f64 + 4.5],
            0.0,
            &mut r,
        );
        let cow = sim.mobs.spawn(
            Kind::Cow,
            [x as f64 + 9.5, (y + 1) as f64, z as f64 + 9.5],
            0.0,
            &mut r,
        );
        for m in sim.mobs.list.iter_mut() {
            m.equipment[4] = Some(crate::item::Stack::new(310, 0, 1));
            m.persistent = true;
        }
        let calm = Command::PlayerState { attackable: false };
        let (mut fall_sound, mut step, mut dust) = (false, false, false);
        let mut landed_hp = None;
        for i in 0..420 {
            let mut ev = Vec::new();
            sim.step(
                &[calm.clone(), Command::Player(feet, [0.0, 0.0], true)],
                &mut ev,
            );
            for e in &ev {
                match e {
                    Event::Sounds(v) => {
                        fall_sound |= v.iter().any(|s| s.0 == "game.hostile.hurt.fall.big");
                        step |= v.iter().any(|s| s.0 == "mob.zombie.step");
                    }
                    Event::FallDust(v) => {
                        dust |= v.iter().any(|d| d.0 == [x + 4, y, z + 4] && d.1 >= 6)
                    }
                    _ => {}
                }
            }
            if i == 40 {
                landed_hp = sim.mobs.list.iter().find(|m| m.id == zom).map(|m| m.health);
            }
            // walk it across the floor (its own AI may stand still): the
            // steps come from moveEntity's distance, not from the AI
            if (60..90).contains(&i) {
                if let Some(m) = sim.mobs.list.iter_mut().find(|m| m.id == zom) {
                    m.motion[0] = if i < 75 { 0.2 } else { -0.2 };
                }
            }
            if i == 250 {
                let hp = sim.mobs.list.iter().find(|m| m.id == cow).map(|m| m.health);
                assert_eq!(hp, Some(10.0), "air lasts 300 ticks");
            }
        }
        let hp = landed_hp.expect("the zombie is there");
        assert!(hp <= 20.0 - 6.0 && hp > 0.0, "fall damage: {hp}");
        assert!(fall_sound, "the hostile fall sound");
        assert!(dust, "landing dust on the farmland");
        assert_eq!(
            sim.world.block(x + 4, y, z + 4),
            block::DIRT,
            "the farmland was trampled"
        );
        assert!(step, "the zombie's own footsteps");
        // 15 seconds of air, then 2 a second: gone by now, or nearly
        let hp = sim
            .mobs
            .list
            .iter()
            .find(|m| m.id == cow)
            .map_or(0.0, |m| m.health);
        assert!(hp < 10.0, "the cow drowned: {hp}");
    }

    /// Zombie villagers: on hard a zombie's villager victim rises as one
    /// (`onKillEntity`), a golden apple cures a weakened one
    /// (`interact` / `startConversion`), the cure survives a save, and when
    /// it runs out a nauseous villager takes its place.
    #[test]
    fn zombie_villagers() {
        use crate::entity::living::Difficulty;
        use crate::entity::mob::{Kind, Mobs};
        use crate::potion;
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        sim.difficulty = Difficulty::Hard;
        let feet = [x as f64 + 0.5, (y + 1) as f64, z as f64 + 0.5];
        let mut r = JavaRandom::new(5);
        let at = |dx: f64, dz: f64| [x as f64 + dx, (y + 1) as f64, z as f64 + dz];
        let vil = sim.mobs.spawn(Kind::Villager, at(6.5, 6.5), 0.0, &mut r);
        let zom = sim.mobs.spawn(Kind::Zombie, at(7.5, 6.5), 0.0, &mut r);
        for m in sim.mobs.list.iter_mut() {
            m.equipment[4] = Some(crate::item::Stack::new(310, 0, 1));
            if m.id == vil {
                m.health = 1.0;
            }
            // its own 5% roll at spawn
            m.zombie_villager = false;
        }
        let calm = Command::PlayerState { attackable: false };
        let mut risen = None;
        for _ in 0..200 {
            sim.step(
                &[calm.clone(), Command::Player(feet, [0.0, 0.0], true)],
                &mut Vec::new(),
            );
            if let Some(m) = sim.mobs.list.iter().find(|m| m.zombie_villager) {
                risen = Some(m.id);
                break;
            }
        }
        let risen = risen.expect("the villager rose as a zombie villager");
        assert!(
            !sim.mobs.list.iter().any(|m| m.id == vil),
            "the villager is gone at once"
        );
        assert!(sim.mobs.list.iter().any(|m| m.id == zom));
        // the cure needs weakness
        let mut out = crate::entity::mob::Outcome::default();
        let apple = Some(crate::item::Stack::new(322, 0, 2));
        assert!(sim.mobs.interact(risen, apple, feet, &mut out).is_none());
        sim.mobs.add_effect(risen, potion::WEAKNESS, 600, 0);
        let left = sim.mobs.interact(risen, apple, feet, &mut out);
        assert_eq!(left.unwrap().unwrap().count, 1, "one apple used");
        assert!(out.sounds.iter().any(|s| s.0 == "mob.zombie.remedy"));
        let m = sim.mobs.list.iter().find(|m| m.id == risen).unwrap();
        assert!((3600..=6000).contains(&m.conversion));
        assert!(!m.potions.active(potion::WEAKNESS));
        assert_eq!(m.potions.amplifier(potion::DAMAGE_BOOST), Some(0));
        // the save keeps it
        let tag = Mobs::to_nbt(m);
        let mut back = Mobs::default();
        back.from_nbt(&tag).unwrap();
        assert!(back.list[0].zombie_villager);
        assert_eq!(back.list[0].conversion, m.conversion);
        // near the end
        for m in sim.mobs.list.iter_mut() {
            if m.id == risen {
                m.conversion = 2;
            }
        }
        for _ in 0..3 {
            sim.step(
                &[calm.clone(), Command::Player(feet, [0.0, 0.0], true)],
                &mut Vec::new(),
            );
        }
        assert!(!sim.mobs.list.iter().any(|m| m.id == risen));
        let v = sim
            .mobs
            .list
            .iter()
            .filter(|m| m.kind == Kind::Villager)
            .max_by_key(|m| m.id)
            .expect("a villager in its place");
        assert!(v.potions.active(potion::CONFUSION));
    }

    /// Mobs keep potion effects (`activePotionsMap`): poison ticks them
    /// down to 1 health, slowness lowers the movementSpeed attribute,
    /// regeneration does not work on the undead, Bane of Arthropods slows
    /// a spider, and the effects survive a save.
    #[test]
    fn mobs_keep_potion_effects() {
        use crate::entity::mob::Kind;
        use crate::potion;
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        let feet = [x as f64 + 0.5, (y + 1) as f64, z as f64 + 0.5];
        let mut r = JavaRandom::new(5);
        let at = |dx: f64, dz: f64| [x as f64 + dx, (y + 1) as f64, z as f64 + dz];
        let cow = sim.mobs.spawn(Kind::Cow, at(4.5, 4.5), 0.0, &mut r);
        let zombie = sim.mobs.spawn(Kind::Zombie, at(8.5, 8.5), 0.0, &mut r);
        sim.mobs.add_effect(cow, potion::POISON, 400, 0);
        sim.mobs.add_effect(cow, potion::MOVE_SLOWDOWN, 400, 1);
        sim.mobs.add_effect(zombie, potion::REGENERATION, 400, 0);
        let get = |sim: &Sim, id: u32| sim.mobs.list.iter().find(|m| m.id == id).unwrap().clone();
        let base = Kind::Cow.speed();
        let slow = get(&sim, cow).speed_attr();
        assert!(
            (slow - base * 0.7).abs() < 1e-4,
            "slowness II: {slow} of {base}"
        );
        if let Some(m) = sim.mobs.list.iter_mut().find(|m| m.id == zombie) {
            m.health = 10.0;
            // a helmet keeps it from burning in the sun
            m.equipment[4] = Some(crate::item::Stack::new(310, 0, 1));
        }
        for i in 0..300 {
            sim.step(&[Command::Player(feet, [0.0, 0.0], true)], &mut Vec::new());
            for id in [cow, zombie] {
                if !sim.mobs.list.iter().any(|m| m.id == id) {
                    panic!("mob {id} (cow {cow}) gone at tick {i}");
                }
            }
        }
        let c = get(&sim, cow);
        assert_eq!(c.health, 1.0, "poison stops at one health");
        assert!(c.potions.active(potion::POISON));
        assert_eq!(
            get(&sim, zombie).health,
            10.0,
            "no regeneration for the undead"
        );
        // the save keeps them
        let tag = crate::entity::mob::Mobs::to_nbt(&c);
        let mut back = crate::entity::mob::Mobs::default();
        back.from_nbt(&tag).unwrap();
        assert!(back.list[0].potions.active(potion::MOVE_SLOWDOWN));
        // Bane of Arthropods slows a spider (applyArthropodEnchantments)
        let spider = sim.mobs.spawn(Kind::Spider, at(2.5, 0.5), 0.0, &mut r);
        let sel = sim.play.inv.selected;
        sim.play.inv.main[sel] = Some(
            crate::item::Stack::new(276, 0, 1)
                .with_enchantments(&[(crate::enchant::BANE_OF_ARTHROPODS, 5)]),
        );
        sim.play.mode = GameMode::Survival;
        sim.step(
            &[
                Command::Player(feet, [-90.0, 0.0], false),
                Command::Attack {
                    id: spider,
                    crit: false,
                    sprint: false,
                },
            ],
            &mut Vec::new(),
        );
        let s = sim.mobs.list.iter().find(|m| m.id == spider);
        assert!(
            s.map_or(true, |m| m.potions.active(potion::MOVE_SLOWDOWN)),
            "Bane of Arthropods V slows"
        );
    }

    /// `EntityVillager.interact`, `ContainerMerchant`, `useRecipe`.
    #[test]
    fn withers_rise_from_soul_sand_and_skulls() {
        use crate::entity::mob::Kind;
        use crate::item::Stack;
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        sim.play.mode = GameMode::Survival;
        sim.difficulty = crate::entity::living::Difficulty::Normal;
        let (bx, by, bz) = (x + 4, y + 1, z);
        // the T of soul sand along x
        for dx in 0..3 {
            sim.world.set_block(bx + dx, by + 1, bz, crate::block::SOUL_SAND, 0, 3);
        }
        sim.world.set_block(bx + 1, by, bz, crate::block::SOUL_SAND, 0, 3);
        let feet = [x as f64 + 0.5, (y + 1) as f64, z as f64 + 0.5];
        let place = |sim: &mut Sim, sx: i32| {
            let sel = sim.play.inv.selected;
            sim.play.inv.main[sel] = Some(Stack::new(397, 1, 1));
            sim.step(
                &[
                    Command::Player(feet, [0.0, 0.0], false),
                    Command::Use {
                        target: [sx, by + 1, bz],
                        face: [0, 1, 0],
                        sneaking: false,
                        hit: [0.5, 1.0, 0.5],
                    },
                ],
                &mut Vec::new(),
            );
        };
        place(&mut sim, bx);
        place(&mut sim, bx + 1);
        assert!(!sim.mobs.list.iter().any(|m| m.kind == Kind::Wither));
        place(&mut sim, bx + 2);
        let w = sim.mobs.list.iter().find(|m| m.kind == Kind::Wither).expect("a wither");
        assert_eq!(w.wither.invul, 220);
        assert_eq!(sim.world.peek_block(bx + 1, by, bz), 0, "the soul sand is gone");
        assert_eq!(sim.world.peek_block(bx, by + 2, bz), 0, "the skulls are gone");
        // it charges, heals and then blasts
        for _ in 0..230 {
            sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut Vec::new());
        }
        let w = sim.mobs.list.iter().find(|m| m.kind == Kind::Wither).expect("still there");
        assert_eq!(w.wither.invul, 0);
        assert!(w.health > 250.0, "healed to {}", w.health);
    }

    #[test]
    fn lightning_charges_creepers_and_turns_pigs() {
        use crate::entity::mob::Kind;
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        sim.difficulty = crate::entity::living::Difficulty::Normal;
        let mut r = JavaRandom::new(4);
        let at = [x as f64 + 4.5, (y + 1) as f64, z as f64 + 0.5];
        let creeper = sim.mobs.spawn(Kind::Creeper, at, 0.0, &mut r);
        let pig = sim.mobs.spawn(Kind::Pig, [at[0] + 1.0, at[1], at[2]], 0.0, &mut r);
        let feet = [x as f64 - 10.5, (y + 1) as f64, z as f64 + 0.5];
        sim.world.bolts.push([x + 4, y + 1, z]);
        let mut ev = Vec::new();
        sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut ev);
        assert!(ev.iter().any(|e| matches!(e, Event::Lightning(_))));
        for _ in 0..3 {
            sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut ev);
        }
        assert!(sim.mobs.list.iter().find(|m| m.id == creeper).unwrap().powered, "charged");
        assert!(sim.mobs.list.iter().all(|m| m.id != pig || m.dead), "the pig is gone");
        assert!(sim.mobs.list.iter().any(|m| m.kind == Kind::PigZombie), "a zombie pigman");
    }

    #[test]
    fn picking_up_wood_and_crafting_unlock_achievements() {
        use crate::item::Stack;
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        sim.play.mode = GameMode::Survival;
        let feet = [x as f64 + 0.5, (y + 1) as f64, z as f64 + 0.5];
        sim.step(&[Command::Player(feet, [0.0, 0.0], false), Command::OpenInventoryAchievement], &mut Vec::new());
        let id = sim.play.next_item;
        sim.play.next_item += 1;
        let mut it = crate::entity::item::ItemEntity::new(id, Stack::new(17, 0, 1), feet);
        it.pickup_delay = 0;
        sim.play.items.push(it);
        let mut got = Vec::new();
        for _ in 0..3 {
            let mut ev = Vec::new();
            sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut ev);
            for e in ev {
                if let Event::Statistics(v) = e {
                    got.extend(v.into_iter().map(|x| x.0));
                }
            }
        }
        assert!(got.iter().any(|g| g == "achievement.mineWood"), "{got:?}");
        assert!(sim.stats_file.has_achievement_unlocked("achievement.mineWood"));
    }

    #[test]
    fn wet_wolves_shake_dry() {
        use crate::entity::mob::Kind;
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        let mut r = JavaRandom::new(2);
        let id = sim.mobs.spawn(Kind::Wolf, [x as f64 + 3.5, (y + 1) as f64, z as f64 + 0.5], 0.0, &mut r);
        let feet = [x as f64 - 6.5, (y + 1) as f64, z as f64 + 0.5];
        for _ in 0..5 {
            sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut Vec::new());
        }
        sim.mobs.list.iter_mut().find(|m| m.id == id).unwrap().wolf_wet = true;
        let mut shook = false;
        for _ in 0..80 {
            sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut Vec::new());
            let m = sim.mobs.list.iter().find(|m| m.id == id).unwrap();
            shook |= m.wolf_shaking && m.shake[1] > 0.5;
        }
        let m = sim.mobs.list.iter().find(|m| m.id == id).unwrap();
        assert!(shook, "it shook");
        assert!(!m.wolf_wet, "and is dry");
    }

    #[test]
    fn fire_charges_light_fires() {
        use crate::item::Stack;
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        sim.play.mode = GameMode::Survival;
        let floor = [x + 2, y, z];
        sim.world.set_block(floor[0], floor[1], floor[2], crate::block::STONE, 0, 3);
        sim.world.set_block(floor[0], floor[1] + 1, floor[2], 0, 0, 3);
        let sel = sim.play.inv.selected;
        sim.play.inv.main[sel] = Some(Stack::new(385, 0, 2));
        sim.step(
            &[Command::Use {
                target: floor,
                face: [0, 1, 0],
                sneaking: false,
                hit: [0.5, 1.0, 0.5],
            }],
            &mut Vec::new(),
        );
        assert_eq!(sim.world.peek_block(floor[0], floor[1] + 1, floor[2]), crate::block::FIRE);
        assert_eq!(sim.play.inv.main[sel].map(|s| s.count), Some(1));
    }

    #[test]
    fn heads_place_turn_and_drop() {
        use crate::item::Stack;
        use crate::play::Tile;
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        sim.play.mode = GameMode::Survival;
        let floor = [x + 2, y, z];
        sim.world.set_block(floor[0], floor[1], floor[2], crate::block::STONE, 0, 3);
        sim.world.set_block(floor[0], floor[1] + 1, floor[2], 0, 0, 3);
        let sel = sim.play.inv.selected;
        sim.play.inv.main[sel] = Some(Stack::new(397, 4, 1));
        let mut ev = Vec::new();
        sim.step(
            &[
                Command::Player([x as f64 + 0.5, (y + 1) as f64, z as f64 + 0.5], [90.0, 30.0], false),
                Command::Use {
                    target: floor,
                    face: [0, 1, 0],
                    sneaking: false,
                    hit: [0.5, 1.0, 0.5],
                },
            ],
            &mut ev,
        );
        let at = [floor[0], floor[1] + 1, floor[2]];
        assert_eq!(sim.world.peek_block(at[0], at[1], at[2]), crate::block::SKULL);
        assert_eq!(sim.world.peek_meta(at[0], at[1], at[2]), 1, "on the floor");
        // yaw 90 is four sixteenths round
        assert_eq!(sim.play.tiles.get(&at), Some(&Tile::Skull { kind: 4, rot: 4 }));
        assert!(ev.iter().any(|e| matches!(e, Event::Skull(p, 4, 4) if *p == at)));
        assert!(sim.play.inv.main[sel].is_none());
    }

    #[test]
    fn beacons_build_pay_and_empower() {
        use crate::item::Stack;
        use crate::play::{Open, Tile};
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        sim.play.mode = GameMode::Survival;
        // a one-level iron pyramid with the beacon on top, open sky
        let (bx, by, bz) = (x + 3, y + 2, z);
        for dx in -1..=1 {
            for dz in -1..=1 {
                sim.world.set_block(bx + dx, by - 1, bz + dz, crate::block::IRON_BLOCK, 0, 3);
            }
        }
        sim.world.set_block(bx, by, bz, crate::block::BEACON, 0, 3);
        sim.play.tiles.insert([bx, by, bz], Tile::beacon());
        for yy in by + 1..256 {
            sim.world.set_block(bx, yy, bz, 0, 0, 3);
        }
        let feet = [x as f64 + 0.5, (y + 1) as f64, z as f64 + 0.5];
        let step = |sim: &mut Sim, c: &[Command]| {
            let mut ev = Vec::new();
            let mut v = vec![Command::Player(feet, [0.0, 0.0], false)];
            v.extend_from_slice(c);
            sim.step(&v, &mut ev);
            ev
        };
        let mut beams = None;
        for _ in 0..81 {
            for e in step(&mut sim, &[]) {
                if let Event::Beacons(v) = e {
                    beams = Some(v);
                }
            }
        }
        assert_eq!(beams, Some(vec![[bx, by, bz]]), "the beam shows");
        let levels = |sim: &Sim| match sim.play.tiles.get(&[bx, by, bz]) {
            Some(Tile::Beacon { levels, primary, .. }) => (*levels, *primary),
            _ => (-9, -9),
        };
        assert_eq!(levels(&sim).0, 1);
        // pay with a diamond and pick Speed; Strength is beyond level 1
        sim.play.open = Open::Beacon([bx, by, bz]);
        if let Some(Tile::Beacon { payment, .. }) = sim.play.tiles.get_mut(&[bx, by, bz]) {
            *payment = Some(Stack::new(264, 0, 1));
        }
        step(&mut sim, &[Command::BeaconSet(crate::potion::DAMAGE_BOOST, 0)]);
        assert_eq!(levels(&sim).1, 0, "not at level 1");
        if let Some(Tile::Beacon { payment, .. }) = sim.play.tiles.get_mut(&[bx, by, bz]) {
            *payment = Some(Stack::new(264, 0, 1));
        }
        step(&mut sim, &[Command::BeaconSet(crate::potion::MOVE_SPEED, 0)]);
        assert_eq!(levels(&sim).1, crate::potion::MOVE_SPEED);
        assert!(matches!(sim.play.tiles.get(&[bx, by, bz]), Some(Tile::Beacon { payment: None, .. })));
        for _ in 0..81 {
            step(&mut sim, &[]);
        }
        assert!(
            sim.splayer.living.active(crate::potion::MOVE_SPEED),
            "Speed from the beacon"
        );
    }

    #[test]
    fn leads_tie_pull_and_break() {
        use crate::entity::mob::{Kind, Leash};
        use crate::item::Stack;
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        sim.play.mode = GameMode::Survival;
        let mut r = JavaRandom::new(5);
        let cow = sim.mobs.spawn(Kind::Cow, [x as f64 + 2.5, (y + 1) as f64, z as f64 + 0.5], 0.0, &mut r);
        let zombie = sim.mobs.spawn(Kind::Zombie, [x as f64 - 2.5, (y + 1) as f64, z as f64 + 0.5], 0.0, &mut r);
        let step = |sim: &mut Sim, feet: [f64; 3], c: &[Command]| {
            let mut v = vec![Command::Player(feet, [0.0, 0.0], false)];
            v.extend_from_slice(c);
            sim.step(&v, &mut Vec::new());
        };
        let feet = [x as f64 + 0.5, (y + 1) as f64, z as f64 + 0.5];
        sim.play.inv.main[0] = Some(Stack::new(420, 0, 2));
        step(&mut sim, feet, &[Command::Interact { id: zombie }]);
        assert_eq!(sim.mobs.list.iter().find(|m| m.id == zombie).unwrap().leash, None, "no leading monsters");
        step(&mut sim, feet, &[Command::Interact { id: cow }]);
        let m = sim.mobs.list.iter().find(|m| m.id == cow).unwrap();
        assert_eq!(m.leash, Some(Leash::Player));
        assert_eq!(sim.play.inv.main[0].map(|s| s.count), Some(1));
        // the saved mob keeps its lead
        let t = crate::entity::mob::Mobs::to_nbt(m);
        assert_eq!(t.int("Leashed"), Some(1));
        // walk 8 blocks away: the cow is pulled after
        let far = [feet[0], feet[1], feet[2] + 8.0];
        let d0 = 8.0;
        for _ in 0..40 {
            step(&mut sim, far, &[]);
        }
        let m = sim.mobs.list.iter().find(|m| m.id == cow).unwrap();
        let d = ((m.pos[0] - far[0]).powi(2) + (m.pos[2] - far[2]).powi(2)).sqrt();
        assert!(d < d0 - 2.0, "pulled to {d}");
        assert_eq!(m.leash, Some(Leash::Player));
        // too far: the lead breaks and drops
        let gone = [feet[0], feet[1], feet[2] + 40.0];
        step(&mut sim, gone, &[]);
        let m = sim.mobs.list.iter().find(|m| m.id == cow).unwrap();
        assert_eq!(m.leash, None);
        assert!(sim.play.items.iter().any(|i| i.stack.id == 420), "the lead dropped");
    }

    #[test]
    fn maps_draw_and_extend() {
        use crate::item::Stack;
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        sim.play.mode = GameMode::Survival;
        let feet = [x as f64 + 0.5, (y + 1) as f64, z as f64 + 0.5];
        let step = |sim: &mut Sim, c: &[Command]| {
            let mut ev = Vec::new();
            let mut v = vec![Command::Player(feet, [0.0, 0.0], false)];
            v.extend_from_slice(c);
            sim.step(&v, &mut ev);
            ev
        };
        sim.play.inv.main[0] = Some(Stack::new(395, 0, 2));
        step(&mut sim, &[Command::UseAir]);
        assert_eq!(sim.play.inv.main[0].map(|s| (s.id, s.count)), Some((395, 1)));
        let map = sim.play.inv.main.iter().flatten().find(|s| s.id == 358).copied();
        let map = map.expect("a filled map in the inventory");
        // hold it: a sixteenth of the columns each tick, then all drawn
        let at = sim.play.inv.main.iter().position(|s| s.is_some_and(|s| s.id == 358)).unwrap();
        sim.play.inv.selected = at;
        let mut seen = None;
        for _ in 0..20 {
            for e in step(&mut sim, &[]) {
                if let Event::Map(v) = e {
                    seen = Some(v);
                }
            }
        }
        let v = seen.expect("S34PacketMaps");
        assert_eq!(v.id, map.meta);
        let drawn = v.colours.iter().filter(|&&c| c != 0).count();
        assert!(drawn > 8000, "only {drawn} pixels drawn");
        assert_eq!(v.marks.len(), 1);
        assert_eq!(v.marks[0].icon, 0, "the player's marker");
        let m = sim.maps.list[&map.meta].clone();
        assert_eq!(m.scale, 0);
        // eight paper around it: a new map one scale up, same centre
        let cells: Vec<Option<Stack>> = (0..9)
            .map(|i| Some(if i == 4 { map } else { Stack::new(339, 0, 1) }))
            .collect();
        let out = crate::crafting::find_matching(&crate::crafting::Grid {
            cells: &cells,
            width: 3,
        })
        .expect("RecipesMapExtending");
        sim.play.inv.main[8] = Some(out);
        step(&mut sim, &[]);
        let big = sim.play.inv.main[8].unwrap();
        assert_ne!(big.meta, map.meta);
        assert!(big.nbt().is_none(), "map_is_scaling cleared");
        let b = &sim.maps.list[&big.meta];
        assert_eq!(b.scale, 1);
        assert_eq!((b.x_center, b.z_center), (m.x_center, m.z_center));
    }

    #[test]
    fn horses_saddle_ride_and_jump() {
        use crate::entity::mob::Kind;
        use crate::item::Stack;
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        sim.play.mode = GameMode::Survival;
        let feet = [x as f64 + 0.5, (y + 1) as f64, z as f64 + 0.5];
        let mut r = JavaRandom::new(3);
        let hid = sim.mobs.spawn(
            Kind::Horse,
            [x as f64 + 2.5, (y + 1) as f64, z as f64 + 0.5],
            0.0,
            &mut r,
        );
        if let Some(m) = sim.mobs.list.iter_mut().find(|m| m.id == hid) {
            m.variant = 0;
            m.growing_age = 0;
        }
        let step = |sim: &mut Sim, c: &[Command]| {
            let mut v = vec![Command::Player(feet, [-90.0, 0.0], false)];
            v.extend_from_slice(c);
            sim.step(&v, &mut Vec::new());
        };
        let horse = |sim: &Sim| sim.mobs.list.iter().find(|m| m.id == hid).unwrap().clone();
        // an untamed horse rears at a saddle
        sim.play.inv.main[0] = Some(Stack::new(329, 0, 1));
        step(&mut sim, &[Command::Interact { id: hid }]);
        assert!(!matches!(sim.play.open, crate::play::Open::Horse(..)));
        // tamed, the saddle opens its inventory; shift-click puts it on
        if let Some(m) = sim.mobs.list.iter_mut().find(|m| m.id == hid) {
            m.horse_flags |= crate::entity::mob::horse::TAME;
            m.tamed = true;
        }
        step(&mut sim, &[Command::Interact { id: hid }]);
        assert!(matches!(sim.play.open, crate::play::Open::Horse(i, false, true) if i == hid));
        step(
            &mut sim,
            &[Command::WindowClick {
                slot: 29,
                button: 0,
                mode: 1,
            }],
        );
        assert!(sim.play.inv.main[0].is_none(), "the saddle left the hotbar");
        step(&mut sim, &[Command::CloseWindow]);
        assert!(horse(&sim).horse_saddled(), "saddled");
        // an empty hand mounts it
        step(&mut sim, &[Command::Interact { id: hid }]);
        assert_eq!(sim.riding, Some(hid));
        for _ in 0..20 {
            step(&mut sim, &[]);
        }
        let y0 = horse(&sim).pos[1];
        // a full jump: the horse leaves the ground and rears
        step(&mut sim, &[Command::HorseJump(100)]);
        let mut top = y0;
        for _ in 0..15 {
            step(&mut sim, &[]);
            top = top.max(horse(&sim).pos[1]);
        }
        assert!(top > y0 + 1.0, "jumped from {y0} to {top}");
        // the client sees the saddle and the rider
        let mut ev = Vec::new();
        sim.step(&[Command::Player(feet, [-90.0, 0.0], false)], &mut ev);
        let seen = ev.iter().any(|e| match e {
            Event::Mobs(v, _) => v.iter().any(|v| v.id == hid && v.horse_flags & 4 != 0 && v.ridden),
            _ => false,
        });
        assert!(seen);
    }

    #[test]
    fn villagers_trade() {
        use crate::entity::mob::Kind;
        use crate::item::Stack;
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        sim.play.mode = GameMode::Survival;
        let feet = [x as f64 + 0.5, (y + 1) as f64, z as f64 + 0.5];
        let mut r = JavaRandom::new(9);
        let vid = sim.mobs.spawn(
            Kind::Villager,
            [x as f64 + 2.5, (y + 1) as f64, z as f64 + 0.5],
            0.0,
            &mut r,
        );
        if let Some(m) = sim.mobs.list.iter_mut().find(|m| m.id == vid) {
            m.variant = 0;
            m.growing_age = 0;
        }
        let step = |sim: &mut Sim, c: &[Command]| {
            let mut v = vec![Command::Player(feet, [-90.0, 0.0], false)];
            v.extend_from_slice(c);
            sim.step(&v, &mut Vec::new());
        };
        step(&mut sim, &[Command::Interact { id: vid }]);
        assert!(matches!(sim.play.open, crate::play::Open::Merchant(i) if i == vid));
        let offer = sim.play.offers[0];
        assert!(sim.mobs.list.iter().find(|m| m.id == vid).unwrap().customer);
        // enough of the price in the hotbar's first slot
        sim.play.inv.main[0] = Some(Stack {
            count: 64,
            ..offer.buy
        });
        if let Some(b) = offer.buy_b {
            sim.play.inv.main[1] = Some(Stack { count: 64, ..b });
        }
        let click = |sim: &mut Sim, slot: i32, mode: i32| {
            step(
                sim,
                &[Command::WindowClick {
                    slot,
                    button: 0,
                    mode,
                }],
            );
        };
        // hotbar slot 0 is container slot 30; put it into price slot 0
        click(&mut sim, 30, 0);
        click(&mut sim, 0, 0);
        if offer.buy_b.is_some() {
            click(&mut sim, 31, 0);
            click(&mut sim, 1, 0);
        }
        assert_eq!(
            sim.play.merchant_slots[2].map(|s| s.id),
            Some(offer.sell.id),
            "the goods show"
        );
        // shift-click the goods into the inventory
        click(&mut sim, 2, 1);
        let have: u32 = sim
            .play
            .inv
            .main
            .iter()
            .flatten()
            .filter(|s| s.id == offer.sell.id && s.id != offer.buy.id)
            .map(|s| s.count)
            .sum();
        if offer.sell.id != offer.buy.id {
            assert!(have >= offer.sell.count, "the goods arrived");
        }
        // Container.retrySlotClick: shift-clicking trades again while the
        // goods come back, until the offer is used up (7) or the price runs out
        let uses = sim
            .mobs
            .list
            .iter()
            .find(|m| m.id == vid)
            .unwrap()
            .offers
            .as_ref()
            .unwrap()[0]
            .uses;
        let most = (64 / offer.buy.count) as i32;
        assert_eq!(uses, most.min(7));
        assert_eq!(
            sim.play.merchant_slots[0].map_or(0, |s| s.count),
            64 - offer.buy.count * uses as u32,
            "the price was paid"
        );
        // closing gives the prices back and ends the trading
        step(&mut sim, &[Command::CloseWindow]);
        assert!(!sim.mobs.list.iter().find(|m| m.id == vid).unwrap().customer);
        assert!(sim.play.merchant_slots.iter().all(|s| s.is_none()));
        // the offers survive a save
        let t =
            crate::entity::mob::Mobs::to_nbt(sim.mobs.list.iter().find(|m| m.id == vid).unwrap());
        let mut other = crate::entity::mob::Mobs::default();
        let id = other.from_nbt(&t).unwrap();
        let back = other.list.iter().find(|m| m.id == id).unwrap();
        assert_eq!(back.offers.as_ref().unwrap()[0].uses, uses);
    }

    /// `RecipeFireworks`, `ItemFirework`, `EntityFireworkRocket`.
    #[test]
    fn fireworks_fly_and_burst() {
        use crate::crafting::{find_matching, Grid};
        use crate::item::Stack;
        let craft = |cells: Vec<Option<Stack>>| {
            find_matching(&Grid {
                cells: &cells,
                width: 3,
            })
        };
        // a large-ball star: gunpowder, red and blue dye, a fire charge, a diamond
        let mut c = vec![None; 9];
        c[0] = Some(Stack::new(289, 0, 1));
        c[1] = Some(Stack::new(351, 1, 1));
        c[2] = Some(Stack::new(351, 4, 1));
        c[3] = Some(Stack::new(385, 0, 1));
        c[4] = Some(Stack::new(264, 0, 1));
        let star = craft(c).expect("a firework star");
        assert_eq!(star.id, 402);
        let ex = crate::particles::Explosion::from_nbt(
            star.tag.get().unwrap().get("Explosion").unwrap(),
        );
        assert_eq!(ex.shape, 1);
        assert!(ex.trail && !ex.flicker);
        assert_eq!(ex.colors, vec![11743532, 2437522]);
        // a fade colour on top
        let mut c = vec![None; 9];
        c[0] = Some(star);
        c[1] = Some(Stack::new(351, 11, 1));
        let star = craft(c).expect("faded star");
        let ex = crate::particles::Explosion::from_nbt(
            star.tag.get().unwrap().get("Explosion").unwrap(),
        );
        assert_eq!(ex.fades, vec![14602026]);
        // two gunpowder, paper and the star: Flight 2
        let mut c = vec![None; 9];
        c[0] = Some(Stack::new(289, 0, 1));
        c[1] = Some(Stack::new(289, 0, 1));
        c[2] = Some(Stack::new(339, 0, 1));
        c[3] = Some(star);
        let rocket = craft(c).expect("a rocket");
        assert_eq!(rocket.id, 401);
        let fw = rocket.tag.get().unwrap().get("Fireworks").cloned().unwrap();
        assert_eq!(fw.int("Flight"), Some(2));
        assert_eq!(
            crate::particles::Explosion::of_rocket(Some(rocket)).len(),
            1
        );
        // two paper: nothing
        let mut c = vec![None; 9];
        c[0] = Some(Stack::new(289, 0, 1));
        c[1] = Some(Stack::new(339, 0, 1));
        c[2] = Some(Stack::new(339, 0, 1));
        assert!(craft(c).is_none());
        // launched from the ground: it climbs, then goes off within 30..41 ticks
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        sim.play.mode = GameMode::Survival;
        let feet = [x as f64 + 0.5, (y + 1) as f64, z as f64 + 0.5];
        let sel = sim.play.inv.selected;
        sim.play.inv.main[sel] = Some(Stack { count: 2, ..rocket });
        let mut ev = Vec::new();
        sim.step(
            &[
                Command::Player(feet, [0.0, 0.0], false),
                Command::Use {
                    target: [x + 3, y, z + 3],
                    face: [0, 1, 0],
                    sneaking: false,
                    hit: [0.5, 1.0, 0.5],
                },
            ],
            &mut ev,
        );
        assert_eq!(sim.play.inv.main[sel].map(|s| s.count), Some(1));
        let rid = sim
            .play
            .vehicles
            .iter()
            .find(|v| matches!(v.v, crate::entity::vehicle::Vehicle::Rocket(_)))
            .map(|v| v.v.id())
            .expect("a rocket in the air");
        let mut burst = None;
        let mut top = 0.0f64;
        for t in 0..60 {
            let mut ev = Vec::new();
            sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut ev);
            if let Some(v) = sim.play.vehicles.iter().find(|v| v.v.id() == rid) {
                top = top.max(v.v.e().pos[1]);
            }
            for e in ev {
                if let Event::Firework(p, _, item) = e {
                    burst = Some((t, p, item));
                }
            }
            if burst.is_some() {
                break;
            }
        }
        let (t, p, item) = burst.expect("it went off");
        assert!((29..=42).contains(&t), "after {t} ticks");
        assert!(p[1] > (y + 10) as f64, "high up ({})", p[1]);
        assert_eq!(item.map(|s| s.id), Some(401));
        assert!(top > (y + 5) as f64);
    }

    /// `BlockPumpkin.onBlockAdded` and `EntitySnowman`.
    #[test]
    fn golems_from_pumpkins() {
        use crate::entity::mob::Kind;
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        let feet = [x as f64 + 0.5, (y + 1) as f64, z as f64 + 0.5];
        let step =
            |sim: &mut Sim| sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut Vec::new());
        // snow, snow, pumpkin
        let s = [x + 6, y + 1, z + 6];
        sim.world.set_block(s[0], s[1], s[2], block::SNOW, 0, 3);
        sim.world.set_block(s[0], s[1] + 1, s[2], block::SNOW, 0, 3);
        sim.world
            .set_block(s[0], s[1] + 2, s[2], block::PUMPKIN, 0, 3);
        step(&mut sim);
        let snowman = sim
            .mobs
            .list
            .iter()
            .find(|m| m.kind == Kind::SnowGolem)
            .map(|m| (m.id, m.pos));
        let (sid, sp) = snowman.expect("a snow golem");
        assert!(
            (sp[0] - (s[0] as f64 + 0.5)).abs() < 0.3 && (sp[2] - (s[2] as f64 + 0.5)).abs() < 0.3
        );
        for dy in 0..3 {
            let b = sim.world.block(s[0], s[1] + dy, s[2]);
            // (the golem may already have left snow at its feet)
            assert!(
                b == block::AIR || b == block::SNOW_LAYER,
                "the blocks are used up"
            );
        }
        // an iron T: pumpkin on top makes an iron golem; a lone column does not
        let g = [x + 2, y + 1, z + 10];
        sim.world
            .set_block(g[0], g[1], g[2], block::IRON_BLOCK, 0, 3);
        sim.world
            .set_block(g[0], g[1] + 1, g[2], block::IRON_BLOCK, 0, 3);
        sim.world
            .set_block(g[0] - 1, g[1] + 1, g[2], block::IRON_BLOCK, 0, 3);
        sim.world
            .set_block(g[0] + 1, g[1] + 1, g[2], block::IRON_BLOCK, 0, 3);
        sim.world
            .set_block(g[0], g[1] + 2, g[2], block::LIT_PUMPKIN, 0, 3);
        step(&mut sim);
        let golem = sim
            .mobs
            .list
            .iter()
            .find(|m| m.kind == Kind::IronGolem)
            .expect("an iron golem");
        assert!(golem.player_made);
        assert_eq!(sim.world.block(g[0] - 1, g[1] + 1, g[2]), block::AIR);
        // the snow golem throws snowballs at a zombie in range
        let mut r = JavaRandom::new(3);
        let zp = [sp[0] + 6.0, (y + 1) as f64, sp[2]];
        let zid = sim.mobs.spawn(Kind::Zombie, zp, 0.0, &mut r);
        let mut thrown = false;
        for _ in 0..200 {
            step(&mut sim);
            if sim
                .mobs
                .throwables
                .iter()
                .any(|t| matches!(t.thrower, crate::entity::arrow::Shooter::Mob(i) if i == sid))
            {
                thrown = true;
                break;
            }
        }
        assert!(thrown, "snowballs at the zombie");
        let _ = zid;
        // snow under its feet in a cold enough biome, or none in a warm one
        let t = {
            let b = sim.world.biome(s[0], s[2]);
            b.float_temperature(&sim.world.provider.terrain.noise, s[0], s[1], s[2])
        };
        let snow = (-12..12)
            .flat_map(|dx| (-12..12).map(move |dz| (dx, dz)))
            .any(|(dx, dz)| sim.world.block(s[0] + dx, y + 1, s[2] + dz) == block::SNOW_LAYER);
        if t < 0.8 {
            assert!(snow, "a trail of snow");
        } else {
            assert!(!snow, "no snow where it is warm");
        }
    }

    /// `BlockChest` (trapped) and `BlockEnderChest`.
    #[test]
    fn trapped_and_ender_chests() {
        use crate::item::Stack;
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        let feet = [x as f64 + 0.5, (y + 1) as f64, z as f64 + 0.5];
        let trapped = [x + 2, y + 1, z + 2];
        let lamp = [x + 3, y + 1, z + 2];
        sim.world.set_block(
            trapped[0],
            trapped[1],
            trapped[2],
            block::TRAPPED_CHEST,
            2,
            3,
        );
        sim.world
            .set_block(lamp[0], lamp[1], lamp[2], block::REDSTONE_LAMP, 0, 3);
        let sel = sim.play.inv.selected;
        sim.play.inv.main[sel] = None;
        let open = |sim: &mut Sim, p: [i32; 3]| {
            sim.step(
                &[
                    Command::Player(feet, [0.0, 0.0], false),
                    Command::Use {
                        target: p,
                        face: [0, 1, 0],
                        sneaking: false,
                        hit: [0.5, 1.0, 0.5],
                    },
                ],
                &mut Vec::new(),
            );
        };
        let step = |sim: &mut Sim, c: &[Command]| {
            sim.step(c, &mut Vec::new());
        };
        open(&mut sim, trapped);
        step(&mut sim, &[]);
        assert!(matches!(sim.play.open, crate::play::Open::Chest(_)));
        assert_eq!(
            sim.world.weak_power(trapped[0], trapped[1], trapped[2], 4),
            1
        );
        assert_eq!(
            sim.world.block(lamp[0], lamp[1], lamp[2]),
            block::LIT_REDSTONE_LAMP,
            "the open trapped chest lights the lamp"
        );
        step(&mut sim, &[Command::CloseWindow]);
        for _ in 0..6 {
            step(&mut sim, &[]);
        }
        assert_eq!(
            sim.world.weak_power(trapped[0], trapped[1], trapped[2], 4),
            0
        );
        assert_eq!(
            sim.world.block(lamp[0], lamp[1], lamp[2]),
            block::REDSTONE_LAMP
        );
        // no third chest beside a pair, of the same kind
        let pair = [x + 2, y + 1, z + 3];
        assert!(sim
            .world
            .can_place_block_at(block::TRAPPED_CHEST, pair[0], pair[1], pair[2]));
        sim.world
            .set_block(pair[0], pair[1], pair[2], block::TRAPPED_CHEST, 2, 3);
        assert!(!sim
            .world
            .can_place_block_at(block::TRAPPED_CHEST, x + 2, y + 1, z + 4));
        assert!(!sim
            .world
            .can_place_block_at(block::TRAPPED_CHEST, x + 1, y + 1, z + 2));
        assert!(sim
            .world
            .can_place_block_at(block::CHEST, x + 2, y + 1, z + 4));
        // two ender chests share one inventory
        let e1 = [x + 6, y + 1, z + 2];
        let e2 = [x + 8, y + 1, z + 2];
        sim.world
            .set_block(e1[0], e1[1], e1[2], block::ENDER_CHEST, 2, 3);
        sim.world
            .set_block(e2[0], e2[1], e2[2], block::ENDER_CHEST, 2, 3);
        open(&mut sim, e1);
        assert!(matches!(sim.play.open, crate::play::Open::Ender(_)));
        sim.play.ender[4] = Some(Stack::new(264, 0, 5));
        step(&mut sim, &[Command::CloseWindow]);
        open(&mut sim, e2);
        assert_eq!(sim.play.tile_vec()[4].map(|s| s.count), Some(5));
        step(&mut sim, &[Command::CloseWindow]);
        // under a solid block it does not open
        sim.world
            .set_block(e2[0], e2[1] + 1, e2[2], block::STONE, 0, 3);
        open(&mut sim, e2);
        assert!(!matches!(sim.play.open, crate::play::Open::Ender(_)));
        // eight obsidian when mined
        let mut r = JavaRandom::new(1);
        let d = crate::drops::drop_block_as_item_with_chance(block::ENDER_CHEST, 0, 1.0, 0, &mut r);
        let n: i32 = d
            .iter()
            .map(|s| match s {
                crate::drops::Spawn::Item(st, _) if st.id == 49 => st.count as i32,
                _ => 0,
            })
            .sum();
        assert_eq!(n, 8);
    }

    /// `ItemHangingEntity`, `EntityPainting`, `EntityItemFrame`.
    #[test]
    fn paintings_and_item_frames() {
        use crate::entity::hanging::Hanging;
        use crate::entity::vehicle::Vehicle;
        use crate::item::Stack;
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        sim.play.mode = GameMode::Survival;
        let feet = [x as f64 + 0.5, (y + 1) as f64, z as f64 + 0.5];
        for dx in -2..4 {
            for dy in 1..5 {
                sim.world
                    .set_block(x + dx, y + dy, z + 6, block::STONE, 0, 3);
            }
        }
        for dy in 1..4 {
            sim.world
                .set_block(x + 9, y + dy, z + 6, block::STONE, 0, 3);
        }
        let sel = sim.play.inv.selected;
        let use_on = |sim: &mut Sim, target: [i32; 3]| {
            sim.step(
                &[Command::Use {
                    target,
                    face: [0, 0, -1],
                    sneaking: false,
                    hit: [0.5, 0.5, 0.0],
                }],
                &mut Vec::new(),
            );
        };
        let hangs = |sim: &Sim| -> Vec<Hanging> {
            sim.play
                .vehicles
                .iter()
                .filter_map(|v| match &v.v {
                    Vehicle::Hang(h) if !h.e.dead => Some(h.clone()),
                    _ => None,
                })
                .collect()
        };
        // a painting on the north face: a random art that fits
        sim.play.inv.main[sel] = Some(Stack::new(321, 0, 2));
        use_on(&mut sim, [x, y + 2, z + 6]);
        let h = hangs(&sim);
        assert_eq!(h.len(), 1, "a painting hangs");
        let painting = h[0].clone();
        assert!(painting.art.is_some());
        assert_eq!(painting.dir, 2);
        assert_eq!(painting.tile, [x, y + 2, z + 6]);
        assert!(painting.e.pos[2] < (z + 6) as f64, "in front of the wall");
        assert_eq!(sim.play.inv.main[sel].map(|s| s.count), Some(1));
        // an item frame on the lone column; on the top face nothing
        sim.play.inv.main[sel] = Some(Stack::new(389, 0, 1));
        sim.step(
            &[Command::Use {
                target: [x + 9, y + 3, z + 6],
                face: [0, 1, 0],
                sneaking: false,
                hit: [0.5, 1.0, 0.5],
            }],
            &mut Vec::new(),
        );
        assert_eq!(hangs(&sim).len(), 1, "not on a top face");
        use_on(&mut sim, [x + 9, y + 2, z + 6]);
        let frame = hangs(&sim)
            .into_iter()
            .find(|h| h.art.is_none())
            .expect("a frame");
        assert!(sim.play.inv.main[sel].is_none());
        // a diamond goes in, then turns
        sim.play.inv.main[sel] = Some(Stack::new(264, 0, 3));
        let poke = |sim: &mut Sim| {
            sim.step(&[Command::Interact { id: frame.id }], &mut Vec::new());
        };
        poke(&mut sim);
        poke(&mut sim);
        let f = hangs(&sim).into_iter().find(|h| h.id == frame.id).unwrap();
        assert_eq!(f.item.map(|s| (s.id, s.count)), Some((264, 1)));
        assert_eq!(f.rotation, 1);
        assert_eq!(sim.play.inv.main[sel].map(|s| s.count), Some(2));
        // the save keeps it
        let t = sim
            .play
            .vehicles
            .iter()
            .find(|v| v.v.id() == frame.id)
            .unwrap()
            .to_nbt(0);
        let back = crate::entity::vehicle::Tracked::from_nbt(&t, 7, 1).unwrap();
        match back.v {
            Vehicle::Hang(b) => {
                assert_eq!(b.item.map(|s| s.id), Some(264));
                assert_eq!(b.rotation, 1);
                assert_eq!(b.tile, f.tile);
                assert_eq!(b.e.pos, f.e.pos);
            }
            _ => panic!("not a hanging entity"),
        }
        // the first hit takes the item out, the second breaks the frame
        let items = |sim: &Sim| -> Vec<u16> { sim.play.items.iter().map(|i| i.stack.id).collect() };
        let hit = |sim: &mut Sim, id: u32| {
            sim.step(
                &[
                    Command::Player(feet, [0.0, 0.0], false),
                    Command::Attack {
                        id,
                        crit: false,
                        sprint: false,
                    },
                ],
                &mut Vec::new(),
            );
        };
        hit(&mut sim, frame.id);
        assert!(items(&sim).contains(&264), "the diamond drops");
        let f = hangs(&sim).into_iter().find(|h| h.id == frame.id).unwrap();
        assert!(f.item.is_none(), "the frame stays");
        hit(&mut sim, frame.id);
        assert!(items(&sim).contains(&389), "the frame drops");
        assert!(hangs(&sim).iter().all(|h| h.id != frame.id));
        // the wall goes: within 100 ticks the painting falls
        let p = painting.clone();
        let w = p.width_pixels() / 16;
        let h0 = p.height_pixels() / 16;
        let x0 = crate::util::math::floor_double(p.e.pos[0] - p.width_pixels() as f64 / 32.0);
        let y0 = crate::util::math::floor_double(p.e.pos[1] - p.height_pixels() as f64 / 32.0);
        for i in 0..w.max(1) {
            for j in 0..h0.max(1) {
                sim.world.set_block(x0 + i, y0 + j, z + 6, block::AIR, 0, 3);
            }
        }
        for _ in 0..102 {
            sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut Vec::new());
        }
        assert!(hangs(&sim).is_empty(), "the painting fell");
        assert!(items(&sim).contains(&321), "and dropped");
    }

    #[test]
    fn name_tags_name_mobs() {
        use crate::entity::mob::{Kind, Mobs, Outcome};
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        let feet = [x as f64 + 0.5, (y + 1) as f64, z as f64 + 0.5];
        let mut r = JavaRandom::new(5);
        let cow = sim.mobs.spawn(
            Kind::Cow,
            [x as f64 + 3.5, (y + 1) as f64, z as f64 + 0.5],
            0.0,
            &mut r,
        );
        // a tag never renamed at the anvil does nothing
        let plain = Some(crate::item::Stack::new(421, 0, 2));
        let mut out = Outcome::default();
        assert!(sim.mobs.interact(cow, plain, feet, &mut out).is_none());
        let tag = crate::item::Stack::new(421, 0, 2).with_name(Some("Bessie"));
        let left = sim.mobs.interact(cow, Some(tag), feet, &mut out);
        assert_eq!(left.flatten().map(|s| s.count), Some(1), "one tag used");
        let m = sim.mobs.list.iter().find(|m| m.id == cow).unwrap();
        assert_eq!(m.custom_name, "Bessie");
        assert!(m.persistent, "a named mob stays");
        // the name survives a save
        let t = Mobs::to_nbt(m);
        let mut other = Mobs::default();
        let id = other.from_nbt(&t).unwrap();
        let back = other.list.iter().find(|m| m.id == id).unwrap();
        assert_eq!(back.custom_name, "Bessie");
        // and reaches the client
        let mut ev = Vec::new();
        sim.step(&[Command::Player(feet, [-90.0, 0.0], false)], &mut ev);
        let named = ev.iter().any(|e| match e {
            Event::Mobs(v, _) => v.iter().any(|v| v.id == cow && v.custom_name == "Bessie"),
            _ => false,
        });
        assert!(named);
    }

    #[test]
    fn fed_cows_have_a_calf() {
        use crate::entity::mob::Kind;
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        let feet = [x as f64 + 0.5, (y + 1) as f64, z as f64 + 0.5];
        let mut r = JavaRandom::new(5);
        let at = |dx: f64, dz: f64| [x as f64 + dx, (y + 1) as f64, z as f64 + dz];
        let a = sim.mobs.spawn(Kind::Cow, at(4.5, 4.5), 0.0, &mut r);
        let b = sim.mobs.spawn(Kind::Cow, at(8.5, 4.5), 0.0, &mut r);
        let wheat = Some(crate::item::Stack::new(296, 0, 8));
        for id in [a, b] {
            let mut out = crate::entity::mob::Outcome::default();
            let left = sim.mobs.interact(id, wheat, feet, &mut out);
            assert_eq!(left.flatten().map(|s| s.count), Some(7), "one wheat eaten");
        }
        // (grown cows may spawn elsewhere meanwhile: count the young)
        let calves = |sim: &Sim| {
            sim.mobs
                .list
                .iter()
                .filter(|m| m.kind == Kind::Cow && m.growing_age < 0)
                .count()
        };
        assert_eq!(calves(&sim), 0);
        for _ in 0..300 {
            sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut Vec::new());
        }
        assert_eq!(calves(&sim), 1, "one calf");
        for id in [a, b] {
            let m = sim.mobs.list.iter().find(|m| m.id == id).unwrap();
            assert!(m.growing_age > 0 && m.in_love == 0, "the parents rest");
        }
    }

    /// `EntityAIPanic` after a blow, `EntityAITempt` with wheat in hand.
    #[test]
    fn cows_panic_and_follow_wheat() {
        use crate::entity::mob::Kind;
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        let feet = [x as f64 + 0.5, (y + 1) as f64, z as f64 + 5.5];
        let mut r = JavaRandom::new(5);
        let start = [x as f64 + 8.5, (y + 1) as f64, z as f64 + 5.5];
        let id = sim.mobs.spawn(Kind::Cow, start, 0.0, &mut r);
        let pos = |sim: &Sim| sim.mobs.list.iter().find(|m| m.id == id).unwrap().pos;
        // wheat: the cow comes within 2.5 blocks
        sim.play.inv.main[sim.play.inv.selected] = Some(crate::item::Stack::new(296, 0, 1));
        for _ in 0..200 {
            sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut Vec::new());
        }
        let p = pos(&sim);
        let d = ((p[0] - feet[0]).powi(2) + (p[2] - feet[2]).powi(2)).sqrt();
        assert!(d < 3.5, "tempted to {d} blocks away");
        // a blow: it runs
        sim.play.inv.main[sim.play.inv.selected] = None;
        let from = pos(&sim);
        let mut moved = 0.0f64;
        sim.step(
            &[
                Command::Player(feet, [0.0, 0.0], false),
                Command::Attack {
                    id,
                    crit: false,
                    sprint: false,
                },
            ],
            &mut Vec::new(),
        );
        for _ in 0..60 {
            sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut Vec::new());
            let p = pos(&sim);
            moved = moved.max(((p[0] - from[0]).powi(2) + (p[2] - from[2]).powi(2)).sqrt());
        }
        assert!(moved > 2.0, "panicked {moved} blocks");
    }

    /// `EntityAIArrowAttack`: a skeleton that has seen the player for a
    /// second shoots.
    #[test]
    fn skeleton_shoots_the_player() {
        use crate::entity::mob::Kind;
        let (mut sim, x, y, z) = redstone_sim();
        arena(&mut sim, x, y, z);
        // a roof: in the sun it would burn and run for shade (EntityAIFleeSun)
        for dx in -2..14 {
            for dz in -2..14 {
                sim.world
                    .set_block(x + dx, y + 5, z + dz, block::STONE, 0, 3);
            }
        }
        sim.play.mode = GameMode::Survival;
        let feet = [x as f64 + 0.5, (y + 1) as f64, z as f64 + 0.5];
        let mut r = JavaRandom::new(5);
        sim.mobs.spawn(
            Kind::Skeleton,
            [x as f64 + 8.5, (y + 1) as f64, z as f64 + 0.5],
            0.0,
            &mut r,
        );
        let mut shot = false;
        for _ in 0..200 {
            sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut Vec::new());
            shot |= !sim.mobs.arrows.is_empty();
        }
        assert!(shot, "an arrow was shot");
    }

    /// EntityCreeper.interact: flint and steel lights the fuse, which then
    /// runs for `fuseTime` (30) ticks whatever the player does.
    #[test]
    fn flint_and_steel_ignites_a_creeper() {
        use crate::entity::mob::Kind;
        let (mut sim, x, y, z) = redstone_sim();
        // the player on the platform (mobs over 128 blocks away despawn)
        let feet = [x as f64 + 0.5, y as f64, z as f64 + 0.5];
        sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut Vec::new());
        let mut r = JavaRandom::new(5);
        let id = sim.mobs.spawn(
            Kind::Creeper,
            [x as f64 + 3.5, y as f64, z as f64 + 3.5],
            0.0,
            &mut r,
        );
        sim.play.inv.main[sim.play.inv.selected] = Some(crate::item::Stack::new(259, 0, 1));
        let mut ev = Vec::new();
        sim.step(&[Command::Interact { id }], &mut ev);
        let lit = ev.iter().any(|e| match e {
            Event::Sounds(l) => l.iter().any(|s| s.0 == "fire.ignite"),
            _ => false,
        });
        assert!(lit, "fire.ignite played");
        assert!(sim.mobs.list.iter().any(|m| m.id == id && m.ignited));
        for _ in 0..28 {
            sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut ev);
        }
        assert!(
            sim.mobs.list.iter().any(|m| m.id == id && !m.dead),
            "still swelling"
        );
        let mut boom = false;
        for _ in 0..4 {
            ev.clear();
            sim.step(&[], &mut ev);
            boom |= ev.iter().any(|e| matches!(e, Event::Explosions(_)));
        }
        assert!(boom, "exploded after the fuse");
    }

    #[test]
    fn big_slime_splits_and_bone_tames_wolf() {
        use crate::entity::mob::Kind;
        let (mut sim, x, y, z) = redstone_sim();
        let mut r = JavaRandom::new(5);
        let id = sim.mobs.spawn(
            Kind::Slime,
            [x as f64 + 3.5, y as f64, z as f64 + 3.5],
            0.0,
            &mut r,
        );
        let m = sim.mobs.list.iter_mut().find(|m| m.id == id).unwrap();
        m.set_slime_size(4);
        m.health = 0.5;
        let mut out = crate::entity::mob::Outcome::default();
        sim.mobs
            .attack(id, 5.0, [x as f64, y as f64, z as f64], 0.0, &mut out);
        steps(&mut sim, 1);
        let small: Vec<i32> = sim
            .mobs
            .list
            .iter()
            .filter(|m| m.kind == Kind::Slime && m.health > 0.0)
            .map(|m| m.variant)
            .collect();
        assert!((2..=4).contains(&small.len()), "{small:?}");
        assert!(small.iter().all(|&v| v == 2));

        let wid = sim.mobs.spawn(
            Kind::Wolf,
            [x as f64 + 6.5, y as f64, z as f64 + 6.5],
            0.0,
            &mut r,
        );
        let mut tamed = false;
        for _ in 0..30 {
            let mut out = crate::entity::mob::Outcome::default();
            let bone = Some(crate::item::Stack::new(352, 0, 64));
            sim.mobs
                .interact(wid, bone, [x as f64, y as f64, z as f64], &mut out);
            let w = sim.mobs.list.iter().find(|m| m.id == wid).unwrap();
            if w.tamed {
                tamed = true;
                // told to sit (aiSit.setSitting); EntityAISit acts on it
                assert!(w.sit_order);
                assert_eq!(w.health, 20.0);
                break;
            }
        }
        assert!(tamed, "a bone tames one time in three");
        steps(&mut sim, 20);
        let w = sim.mobs.list.iter().find(|m| m.id == wid).unwrap();
        assert!(w.sitting, "sat down");
    }

    #[test]
    fn nether_portal_round_trip() {
        let (mut sim, x, y, z) = redstone_sim();
        // a 4 x 5 obsidian frame along x
        let w = &mut sim.world;
        for i in 0..4 {
            w.set_block(x + i, y, z, block::OBSIDIAN, 0, 3);
            w.set_block(x + i, y + 4, z, block::OBSIDIAN, 0, 3);
        }
        for j in 1..4 {
            w.set_block(x, y + j, z, block::OBSIDIAN, 0, 3);
            w.set_block(x + 3, y + j, z, block::OBSIDIAN, 0, 3);
        }
        w.set_block(x + 1, y + 1, z, block::FIRE, 0, 3);
        for i in 1..3 {
            for j in 1..4 {
                assert_eq!(w.block(x + i, y + j, z), block::PORTAL, "lit at {i},{j}");
            }
        }
        let inside = [x as f64 + 1.5, (y + 1) as f64, z as f64 + 0.5];
        let mut ev = Vec::new();
        sim.step(&[Command::Player(inside, [0.0, 0.0], false)], &mut ev);
        sim.step(&[Command::Player(inside, [0.0, 0.0], false)], &mut ev);
        assert_eq!(sim.dim, -1, "creative players travel at once");
        let arrived = ev.iter().find_map(|e| match e {
            Event::Dimension { dim: -1, pos, .. } => Some(*pos),
            _ => None,
        });
        let pos = arrived.expect("dimension event");
        assert!((pos[0] - inside[0] / 8.0).abs() < 20.0 && (pos[2] - inside[2] / 8.0).abs() < 20.0);
        assert_eq!(
            crate::dimension::portal_contact(&mut sim.world, pos),
            Some(block::PORTAL),
            "standing in the new portal"
        );
        // stay put: the cooldown keeps us here
        ev.clear();
        sim.step(&[Command::DimensionAck], &mut ev);
        for _ in 0..5 {
            sim.step(&[Command::Player(pos, [0.0, 0.0], false)], &mut ev);
        }
        assert_eq!(sim.dim, -1);
        // step out, wait, step back in
        let out = [pos[0] + 3.0, pos[1], pos[2] + 3.0];
        for _ in 0..15 {
            sim.step(&[Command::Player(out, [0.0, 0.0], false)], &mut ev);
        }
        sim.step(&[Command::Player(pos, [0.0, 0.0], false)], &mut ev);
        sim.step(&[Command::Player(pos, [0.0, 0.0], false)], &mut ev);
        assert_eq!(sim.dim, 0, "back to the overworld");
        let (ppos, _) = sim.play.player;
        assert!(
            (ppos[0] - inside[0]).abs() < 3.0 && (ppos[2] - inside[2]).abs() < 3.0,
            "{ppos:?}"
        );
    }

    #[test]
    fn end_portal_and_dragon_exit() {
        let (mut sim, x, y, z) = redstone_sim();
        // a ring of twelve frames around a 3x3 hole (frames face inwards)
        let (cx, cz) = (x + 3, z + 3);
        let w = &mut sim.world;
        for i in -1..=1 {
            w.set_block(cx + i, y, cz - 2, block::END_PORTAL_FRAME, 0 | 4, 3); // facing +z
            w.set_block(cx + i, y, cz + 2, block::END_PORTAL_FRAME, 2 | 4, 3); // facing -z
            w.set_block(cx - 2, y, cz + i, block::END_PORTAL_FRAME, 3 | 4, 3); // facing +x
            w.set_block(cx + 2, y, cz + i, block::END_PORTAL_FRAME, 1, 3); // facing -x
        }
        w.set_meta(cx + 2, y, cz - 1, 1 | 4, 3);
        w.set_meta(cx + 2, y, cz + 1, 1 | 4, 3);
        let mut ev = Vec::new();
        let eye = crate::item::Stack::new(381, 0, 1);
        sim.step(
            &[
                Command::Give(eye),
                Command::Select(0),
                Command::Use {
                    target: [cx + 2, y, cz],
                    face: [0, 1, 0],
                    sneaking: false,
                    hit: [0.5, 1.0, 0.5],
                },
            ],
            &mut ev,
        );
        for i in -1..=1 {
            for j in -1..=1 {
                assert_eq!(
                    sim.world.block(cx + i, y, cz + j),
                    block::END_PORTAL,
                    "{i},{j}"
                );
            }
        }
        // jump in
        let feet = [cx as f64 + 0.5, y as f64, cz as f64 + 0.5];
        sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut ev);
        sim.step(&[Command::Player(feet, [0.0, 0.0], false)], &mut ev);
        assert_eq!(sim.dim, 1);
        assert_eq!(
            sim.world.block(100, 48, 0),
            block::OBSIDIAN,
            "the landing platform"
        );
        // a dead dragon leaves the way home
        let mut r = JavaRandom::new(1);
        let id = sim.mobs.spawn(
            crate::entity::mob::Kind::EnderDragon,
            [0.5, 80.0, 0.5],
            0.0,
            &mut r,
        );
        sim.mobs
            .list
            .iter_mut()
            .find(|m| m.id == id)
            .unwrap()
            .health = 0.0;
        ev.clear();
        sim.step(&[Command::DimensionAck], &mut ev);
        for _ in 0..210 {
            sim.step(&[], &mut ev);
        }
        assert!(!sim.mobs.list.iter().any(|m| m.id == id));
        let mut found = false;
        for px in -6..=6 {
            for pz in -6..=6 {
                if sim.world.block(px, 64, pz) == block::END_PORTAL {
                    found = true;
                }
            }
        }
        assert!(found, "exit portal at y 64");
        assert!((0..256).any(|py| sim.world.block(0, py, 0) == block::DRAGON_EGG));
    }

    #[test]
    fn anvil_world_round_trip() {
        use crate::world::anvil::{compound, Nbt};
        let dir = std::env::temp_dir().join(format!("minerust-anvilsim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // a fresh vanilla level.dat
        let root = compound(vec![(
            "Data",
            compound(vec![
                ("RandomSeed", Nbt::Long(77)),
                ("Time", Nbt::Long(1000)),
                ("DayTime", Nbt::Long(1000)),
                ("SpawnX", Nbt::Int(0)),
                ("SpawnY", Nbt::Int(80)),
                ("SpawnZ", Nbt::Int(0)),
                ("GameType", Nbt::Int(1)),
            ]),
        )]);
        crate::world::anvil::write_level_dat(&dir, &root).unwrap();
        let cfg = SimConfig {
            seed: 0,
            view: 2,
            save_dir: Some(dir.clone()),
            start_time: 0,
            freeze_time: true,
            mode: GameMode::Creative,
        };
        let (chest, mob_pos);
        {
            let (mut sim, start) = Sim::new(&cfg);
            assert_eq!(sim.world.seed(), 77);
            assert!(sim.world.anvil);
            let mut ev = Vec::new();
            for _ in 0..30 {
                sim.step(&[], &mut ev);
            }
            let (x, z) = (start.pos[0].floor() as i32, start.pos[2].floor() as i32);
            chest = [x + 1, 100, z + 1];
            sim.world
                .set_block(chest[0], chest[1], chest[2], block::CHEST, 2, 3);
            let mut slots = [None; 27];
            slots[4] = Some(crate::item::Stack::new(264, 0, 3));
            sim.play
                .tiles
                .insert(chest, crate::play::Tile::Chest { slots });
            let mut r = JavaRandom::new(3);
            mob_pos = [x as f64 + 0.5, 101.0, z as f64 + 3.5];
            let id = sim
                .mobs
                .spawn(crate::entity::mob::Kind::Cow, mob_pos, 0.0, &mut r);
            sim.mobs
                .list
                .iter_mut()
                .find(|m| m.id == id)
                .unwrap()
                .persistent = true;
            sim.save().unwrap();
        }
        assert!(dir.join("region").read_dir().unwrap().count() > 0);
        assert!(
            !dir.join("level.txt").exists(),
            "an Anvil world stays Anvil"
        );
        let (mut sim, _) = Sim::new(&cfg);
        let mut ev = Vec::new();
        for _ in 0..30 {
            sim.step(&[], &mut ev);
        }
        assert_eq!(sim.world.block(chest[0], chest[1], chest[2]), block::CHEST);
        match sim.play.tiles.get(&chest) {
            Some(crate::play::Tile::Chest { slots }) => {
                assert_eq!(slots[4].map(|s| (s.id, s.count)), Some((264, 3)))
            }
            other => panic!("chest lost: {other:?}"),
        }
        assert!(sim.mobs.list.iter().any(
            |m| m.kind == crate::entity::mob::Kind::Cow && (m.pos[0] - mob_pos[0]).abs() < 2.0
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_to_anvil() {
        let base = std::env::temp_dir().join(format!("minerust-export-{}", std::process::id()));
        let (src, dst) = (base.join("own"), base.join("vanilla"));
        let _ = std::fs::remove_dir_all(&base);
        let cfg = SimConfig {
            seed: 99,
            view: 2,
            save_dir: Some(src.clone()),
            start_time: 0,
            freeze_time: true,
            mode: GameMode::Creative,
        };
        let marker;
        {
            let (mut sim, start) = Sim::new(&cfg);
            let mut ev = Vec::new();
            for _ in 0..20 {
                sim.step(&[], &mut ev);
            }
            marker = [
                start.pos[0].floor() as i32,
                120,
                start.pos[2].floor() as i32,
            ];
            sim.world
                .set_block(marker[0], marker[1], marker[2], block::GOLD_BLOCK, 0, 3);
            sim.step(&[], &mut ev);
            sim.save().unwrap();
            // unloading writes the rest to disk
            let keys: Vec<(i32, i32)> = sim.world.chunks.keys().copied().collect();
            for k in keys {
                sim.world.unload_chunk(k.0, k.1).unwrap();
            }
        }
        let (mut sim, _) = Sim::new(&cfg);
        let n = sim.export_anvil(&dst).unwrap();
        assert!(n > 0);
        assert!(dst.join("level.dat").exists());
        let cfg2 = SimConfig {
            save_dir: Some(dst.clone()),
            ..cfg
        };
        let (mut sim, _) = Sim::new(&cfg2);
        assert!(sim.world.anvil);
        assert_eq!(sim.world.seed(), 99);
        sim.world.load_chunk(marker[0] >> 4, marker[2] >> 4);
        assert_eq!(
            sim.world.block(marker[0], marker[1], marker[2]),
            block::GOLD_BLOCK
        );
        let _ = std::fs::remove_dir_all(&base);
    }
}
