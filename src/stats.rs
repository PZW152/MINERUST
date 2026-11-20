//! The per-player statistics file of a world (`StatisticsFile`,
//! `stats/<uuid>.json`) and the achievement tree (`AchievementList`).
//!
//! Only the achievement part is used so far: "Taking Inventory"
//! (`openInventory`) is unlocked by opening the inventory, and the client
//! shows the inventory hint while the player has no achievement. The other
//! statistics are kept as they are read, so a vanilla world's file survives.

use crate::assets::Json;
use std::path::{Path, PathBuf};

/// `AchievementList`: stat id and parent, in registration order.
pub const ACHIEVEMENTS: &[(&str, Option<&str>)] = &[
    ("achievement.openInventory", None),
    ("achievement.mineWood", Some("achievement.openInventory")),
    ("achievement.buildWorkBench", Some("achievement.mineWood")),
    (
        "achievement.buildPickaxe",
        Some("achievement.buildWorkBench"),
    ),
    ("achievement.buildFurnace", Some("achievement.buildPickaxe")),
    ("achievement.acquireIron", Some("achievement.buildFurnace")),
    ("achievement.buildHoe", Some("achievement.buildWorkBench")),
    ("achievement.makeBread", Some("achievement.buildHoe")),
    ("achievement.bakeCake", Some("achievement.buildHoe")),
    (
        "achievement.buildBetterPickaxe",
        Some("achievement.buildPickaxe"),
    ),
    ("achievement.cookFish", Some("achievement.buildFurnace")),
    ("achievement.onARail", Some("achievement.acquireIron")),
    ("achievement.buildSword", Some("achievement.buildWorkBench")),
    ("achievement.killEnemy", Some("achievement.buildSword")),
    ("achievement.killCow", Some("achievement.buildSword")),
    ("achievement.flyPig", Some("achievement.killCow")),
    ("achievement.snipeSkeleton", Some("achievement.killEnemy")),
    ("achievement.diamonds", Some("achievement.acquireIron")),
    ("achievement.diamondsToYou", Some("achievement.diamonds")),
    ("achievement.portal", Some("achievement.diamonds")),
    ("achievement.ghast", Some("achievement.portal")),
    ("achievement.blazeRod", Some("achievement.portal")),
    ("achievement.potion", Some("achievement.blazeRod")),
    ("achievement.theEnd", Some("achievement.blazeRod")),
    ("achievement.theEnd2", Some("achievement.theEnd")),
    ("achievement.enchantments", Some("achievement.diamonds")),
    ("achievement.overkill", Some("achievement.enchantments")),
    ("achievement.bookcase", Some("achievement.enchantments")),
    ("achievement.breedCow", Some("achievement.killCow")),
    ("achievement.spawnWither", Some("achievement.theEnd2")),
    ("achievement.killWither", Some("achievement.spawnWither")),
    ("achievement.fullBeacon", Some("achievement.killWither")),
    ("achievement.exploreAllBiomes", Some("achievement.theEnd")),
];

pub const OPEN_INVENTORY: &str = "achievement.openInventory";

/// `Achievement.theItemStack`: item id and damage of the icon.
pub fn icon(id: &str) -> (u16, i32) {
    match id {
        "achievement.openInventory" => (340, 0),      // book
        "achievement.mineWood" => (17, 0),            // log
        "achievement.buildWorkBench" => (58, 0),      // crafting_table
        "achievement.buildPickaxe" => (270, 0),       // wooden_pickaxe
        "achievement.buildFurnace" => (61, 0),        // furnace
        "achievement.acquireIron" => (265, 0),        // iron_ingot
        "achievement.buildHoe" => (290, 0),           // wooden_hoe
        "achievement.makeBread" => (297, 0),          // bread
        "achievement.bakeCake" => (354, 0),           // cake
        "achievement.buildBetterPickaxe" => (274, 0), // stone_pickaxe
        "achievement.cookFish" => (350, 0),           // cooked_fished
        "achievement.onARail" => (66, 0),             // rail
        "achievement.buildSword" => (268, 0),         // wooden_sword
        "achievement.killEnemy" => (352, 0),          // bone
        "achievement.killCow" => (334, 0),            // leather
        "achievement.flyPig" => (329, 0),             // saddle
        "achievement.snipeSkeleton" => (261, 0),      // bow
        "achievement.diamonds" => (56, 0),            // diamond_ore
        "achievement.diamondsToYou" => (264, 0),      // diamond
        "achievement.portal" => (49, 0),              // obsidian
        "achievement.ghast" => (370, 0),              // ghast_tear
        "achievement.blazeRod" => (369, 0),           // blaze_rod
        "achievement.potion" => (373, 0),             // potionitem
        "achievement.theEnd" => (381, 0),             // ender_eye
        "achievement.theEnd2" => (122, 0),            // dragon_egg
        "achievement.enchantments" => (116, 0),       // enchanting_table
        "achievement.overkill" => (276, 0),           // diamond_sword
        "achievement.bookcase" => (47, 0),            // bookshelf
        "achievement.breedCow" => (296, 0),           // wheat
        "achievement.spawnWither" => (397, 1),        // skull (wither skeleton)
        "achievement.killWither" => (399, 0),         // nether_star
        "achievement.fullBeacon" => (138, 0),         // beacon
        "achievement.exploreAllBiomes" => (313, 0),   // diamond_boots
        _ => (0, 0),
    }
}

pub fn is_achievement(id: &str) -> bool {
    ACHIEVEMENTS.iter().any(|a| a.0 == id)
}

fn parent(id: &str) -> Option<&'static str> {
    ACHIEVEMENTS.iter().find(|a| a.0 == id).and_then(|a| a.1)
}

/// `StatisticsFile`: the server's copy, read from and written to the world.
pub struct StatisticsFile {
    path: Option<PathBuf>,
    /// stat id and its JSON value, in file order
    data: Vec<(String, Json)>,
    dirty: Vec<String>,
}

impl StatisticsFile {
    /// `ServerConfigurationManager.getPlayerStatsFile`: `stats/<uuid>.json`
    /// in the world folder; an old `stats/<name>.json` is renamed to it.
    /// Without a world folder (`--no-save`) nothing is read or written.
    pub fn open(world_dir: Option<&Path>, name: &str) -> StatisticsFile {
        let path = world_dir.map(|d| {
            let dir = d.join("stats");
            let file = dir.join(format!("{}.json", offline_uuid(name)));
            if !file.exists() {
                let old = dir.join(format!("{name}.json"));
                if old.is_file() {
                    let _ = std::fs::rename(&old, &file);
                }
            }
            file
        });
        let mut s = StatisticsFile {
            path,
            data: Vec::new(),
            dirty: Vec::new(),
        };
        s.read_stat_file();
        s
    }

    /// `readStatFile`
    fn read_stat_file(&mut self) {
        let Some(p) = &self.path else { return };
        let Ok(bytes) = std::fs::read(p) else { return };
        match Json::parse(&bytes) {
            Some(Json::Obj(v)) => self.data = v,
            Some(_) => self.data.clear(),
            None => eprintln!("Couldn't parse statistics file {}", p.display()),
        }
    }

    /// `saveStatFile`
    pub fn save_stat_file(&self) -> std::io::Result<()> {
        let Some(p) = &self.path else { return Ok(()) };
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(p, dump(&Json::Obj(self.data.clone())))
    }

    /// `StatFileWriter.writeStat`: the integer value (or `value` of an object).
    pub fn write_stat(&self, id: &str) -> i32 {
        match self.data.iter().find(|e| e.0 == id).map(|e| &e.1) {
            Some(Json::Num(n)) => *n as i32,
            Some(o @ Json::Obj(_)) => match o.get("value") {
                Some(Json::Num(n)) => *n as i32,
                _ => 0,
            },
            _ => 0,
        }
    }

    /// `hasAchievementUnlocked`
    pub fn has_achievement_unlocked(&self, id: &str) -> bool {
        self.write_stat(id) > 0
    }

    /// `canUnlockAchievement`: the parent is unlocked, or there is none.
    pub fn can_unlock_achievement(&self, id: &str) -> bool {
        parent(id).map_or(true, |p| self.has_achievement_unlocked(p))
    }

    /// `EntityPlayer.triggerAchievement` → `addStat(stat, 1)` →
    /// `StatisticsFile.increaseStat` → `unlockAchievement`. An achievement
    /// whose parent is missing is not counted (`StatFileWriter.increaseStat`).
    /// Returns true when the achievement is new, which makes the server send
    /// the dirty statistics (`hasUnsentAchievement`).
    pub fn trigger_achievement(&mut self, id: &str) -> bool {
        if !self.can_unlock_achievement(id) {
            return false;
        }
        let old = self.write_stat(id);
        let new = old.saturating_add(1);
        self.set(id, new);
        if !self.dirty.iter().any(|d| d == id) {
            self.dirty.push(id.to_string());
        }
        old == 0
    }

    /// `EntityPlayer.addStat(stat, n)` for a general statistic
    /// (`StatisticsFile.increaseStat`).
    pub fn increase_stat(&mut self, id: &str, n: i32) {
        let old = self.write_stat(id);
        self.set(id, old.saturating_add(n));
        if !self.dirty.iter().any(|d| d == id) {
            self.dirty.push(id.to_string());
        }
    }

    fn set(&mut self, id: &str, v: i32) {
        match self.data.iter_mut().find(|e| e.0 == id).map(|e| &mut e.1) {
            // an entry with progress keeps it (TupleIntJsonSerializable)
            Some(Json::Obj(fields)) => match fields.iter_mut().find(|f| f.0 == "value") {
                Some(f) => f.1 = Json::Num(v as f64),
                None => fields.insert(0, ("value".to_string(), Json::Num(v as f64))),
            },
            Some(e) => *e = Json::Num(v as f64),
            None => self.data.push((id.to_string(), Json::Num(v as f64))),
        }
    }

    /// `sendAchievements` (on joining): every unlocked achievement.
    pub fn achievements(&mut self) -> Vec<(String, i32)> {
        let mut out = Vec::new();
        for (id, _) in ACHIEVEMENTS {
            if self.has_achievement_unlocked(id) {
                out.push((id.to_string(), self.write_stat(id)));
                self.dirty.retain(|d| d != id);
            }
        }
        out
    }

    /// `sendStats` after an achievement: the dirty statistics.
    pub fn take_dirty(&mut self) -> Vec<(String, i32)> {
        std::mem::take(&mut self.dirty)
            .into_iter()
            .map(|id| {
                let v = self.write_stat(&id);
                (id, v)
            })
            .collect()
    }
}

/// Gson's compact `JsonObject.toString`.
fn dump(j: &Json) -> String {
    match j {
        Json::Obj(v) => {
            let fields: Vec<String> = v
                .iter()
                .map(|(k, v)| format!("{}:{}", quote(k), dump(v)))
                .collect();
            format!("{{{}}}", fields.join(","))
        }
        Json::Arr(v) => format!("[{}]", v.iter().map(dump).collect::<Vec<_>>().join(",")),
        Json::Str(s) => quote(s),
        Json::Num(n) if n.fract() == 0.0 && n.abs() < 9.0e15 => format!("{}", *n as i64),
        Json::Num(n) => format!("{n}"),
        Json::Bool(b) => b.to_string(),
        Json::Null => "null".into(),
    }
}

fn quote(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            '<' | '>' | '&' | '=' | '\'' => o.push_str(&format!("\\u{:04x}", c as u32)), // Gson's HTML-safe escaping
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// The UUID of an offline player: `UUID.nameUUIDFromBytes("OfflinePlayer:" + name)`
/// (MD5, version 3), as launchers without an account pass it.
pub fn offline_uuid(name: &str) -> String {
    let mut h = md5(format!("OfflinePlayer:{name}").as_bytes());
    h[6] = (h[6] & 0x0f) | 0x30;
    h[8] = (h[8] & 0x3f) | 0x80;
    let hex: String = h.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// RFC 1321.
fn md5(input: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    let k: Vec<u32> = (0..64)
        .map(|i| ((i as f64 + 1.0).sin().abs() * 4294967296.0) as u32)
        .collect();
    let (mut a0, mut b0, mut c0, mut d0) =
        (0x67452301u32, 0xefcdab89u32, 0x98badcfeu32, 0x10325476u32);
    let mut msg = input.to_vec();
    let bits = (input.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_le_bytes());
    for chunk in msg.chunks(64) {
        let m: Vec<u32> = chunk
            .chunks(4)
            .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
            .collect();
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f = f.wrapping_add(a).wrapping_add(k[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(S[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }
    let mut out = [0u8; 16];
    for (i, v) in [a0, b0, c0, d0].iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_and_offline_uuid() {
        let hex = |b: [u8; 16]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
        assert_eq!(hex(md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(
            hex(md5(b"The quick brown fox jumps over the lazy dog")),
            "9e107d9d372bb6826bd81d3542a419d6"
        );
        // UUID.nameUUIDFromBytes("OfflinePlayer:Notch".getBytes(UTF_8))
        assert_eq!(
            offline_uuid("Notch"),
            "b50ad385-829d-3141-a216-7e7d7539ba7f"
        );
    }

    #[test]
    fn open_inventory_unlocks_once_and_children_need_it() {
        let mut s = StatisticsFile {
            path: None,
            data: Vec::new(),
            dirty: Vec::new(),
        };
        assert!(!s.trigger_achievement("achievement.mineWood"));
        assert!(s.trigger_achievement(OPEN_INVENTORY));
        assert!(!s.trigger_achievement(OPEN_INVENTORY));
        assert_eq!(s.write_stat(OPEN_INVENTORY), 2);
        assert_eq!(s.take_dirty(), vec![(OPEN_INVENTORY.to_string(), 2)]);
        assert!(s.can_unlock_achievement("achievement.mineWood"));
        assert_eq!(
            dump(&Json::Obj(s.data.clone())),
            "{\"achievement.openInventory\":2}"
        );
    }
}
