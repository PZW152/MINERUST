//! `ServerCommandManager` and the vanilla commands a single player uses:
//! `C01PacketChatMessage` with a leading slash runs one, anything else is
//! `chat.type.text`.

use super::*;
use crate::chat::ChatMsg;
use crate::entity::living::{Difficulty, Source, SourceKind};
use crate::item::Stack;
use crate::item_data::{BLOCK_NAMES, ITEM_NAMES};

/// `EntityPlayer.getCommandSenderName` of the local player.
pub const PLAYER_NAME: &str = "Player";

/// A failed command: `CommandException` (red), with the usage key wrapped
/// in `commands.generic.usage` for `WrongUsageException`.
struct Fail(ChatMsg);

fn fail(key: &str, args: Vec<String>) -> Fail {
    Fail(ChatMsg::red(key, args))
}

fn usage(key: &str) -> Fail {
    Fail(ChatMsg::red("commands.generic.usage", vec![tr(key)]))
}

/// A `ChatComponentTranslation` argument (plain strings stay as they are).
fn tr(key: &str) -> String {
    format!("\u{1}{key}")
}

/// `CommandBase.parseInt` / `parseIntBounded` / `parseIntWithMin`
fn parse_int(s: &str, min: i32, max: i32) -> Result<i32, Fail> {
    let v: i32 = s
        .parse()
        .map_err(|_| fail("commands.generic.num.invalid", vec![s.into()]))?;
    if v < min {
        return Err(fail(
            "commands.generic.num.tooSmall",
            vec![v.to_string(), min.to_string()],
        ));
    }
    if v > max {
        return Err(fail(
            "commands.generic.num.tooBig",
            vec![v.to_string(), max.to_string()],
        ));
    }
    Ok(v)
}

fn parse_double(s: &str) -> Result<f64, Fail> {
    let v: f64 = s
        .parse()
        .map_err(|_| fail("commands.generic.num.invalid", vec![s.into()]))?;
    if !v.is_finite() {
        return Err(fail("commands.generic.num.invalid", vec![s.into()]));
    }
    Ok(v)
}

/// `CommandBase.func_110665_a`: `~` relative, and whole numbers to the
/// block centre.
fn coord(base: f64, s: &str, min: i32, max: i32) -> Result<f64, Fail> {
    let rel = s.starts_with('~');
    let mut v = if rel { base } else { 0.0 };
    if !rel || s.len() > 1 {
        let dot = s.contains('.');
        let t = if rel { &s[1..] } else { s };
        v += parse_double(t)?;
        if !dot && !rel {
            v += 0.5;
        }
    }
    if min != 0 || max != 0 {
        if v < min as f64 {
            return Err(fail(
                "commands.generic.double.tooSmall",
                vec![format!("{v:.2}"), min.to_string()],
            ));
        }
        if v > max as f64 {
            return Err(fail(
                "commands.generic.double.tooBig",
                vec![format!("{v:.2}"), max.to_string()],
            ));
        }
    }
    Ok(v)
}

/// `CommandBase.func_110666_a` for block coordinates (`~` from the
/// player's block, no centring).
fn block_coord(base: i32, s: &str) -> Result<i32, Fail> {
    let rel = s.starts_with('~');
    let mut v = if rel { base } else { 0 };
    if !rel || s.len() > 1 {
        let t = if rel { &s[1..] } else { s };
        v += parse_int(t, i32::MIN, i32::MAX)?;
    }
    Ok(v)
}

/// `CommandBase.getItemByText`
fn item_by_text(s: &str, notes: &mut Vec<ChatMsg>) -> Result<u16, Fail> {
    let name = if s.contains(':') {
        s.to_string()
    } else {
        format!("minecraft:{s}")
    };
    if let Some(e) = ITEM_NAMES.iter().find(|e| e.1 == name) {
        return Ok(e.0);
    }
    if let Ok(id) = s.parse::<u16>() {
        if let Some(e) = ITEM_NAMES.iter().find(|e| e.0 == id) {
            let mut m = ChatMsg::tr("commands.generic.deprecatedId", vec![e.1.to_string()]);
            m.color = Some('7');
            notes.push(m);
            return Ok(id);
        }
    }
    Err(fail("commands.give.notFound", vec![s.into()]))
}

/// `CommandBase.getBlockByText`
fn block_by_text(s: &str, notes: &mut Vec<ChatMsg>) -> Result<u16, Fail> {
    let name = if s.contains(':') {
        s.to_string()
    } else {
        format!("minecraft:{s}")
    };
    if let Some(e) = BLOCK_NAMES.iter().find(|e| e.1 == name) {
        return Ok(e.0);
    }
    if let Ok(id) = s.parse::<u16>() {
        if let Some(e) = BLOCK_NAMES.iter().find(|e| e.0 == id) {
            let mut m = ChatMsg::tr("commands.generic.deprecatedId", vec![e.1.to_string()]);
            m.color = Some('7');
            notes.push(m);
            return Ok(id);
        }
    }
    Err(fail("commands.give.notFound", vec![s.into()]))
}

/// The commands `help` lists, by name, with their usage key.
const COMMANDS: [(&str, &str); 18] = [
    ("clear", "commands.clear.usage"),
    ("difficulty", "commands.difficulty.usage"),
    ("effect", "commands.effect.usage"),
    ("gamemode", "commands.gamemode.usage"),
    ("give", "commands.give.usage"),
    ("help", "commands.help.usage"),
    ("kill", "commands.kill.usage"),
    ("me", "commands.me.usage"),
    ("say", "commands.say.usage"),
    ("seed", "commands.seed.usage"),
    ("setblock", "commands.setblock.usage"),
    ("setworldspawn", "commands.setworldspawn.usage"),
    ("spawnpoint", "commands.spawnpoint.usage"),
    ("time", "commands.time.usage"),
    ("toggledownfall", "commands.downfall.usage"),
    ("tp", "commands.tp.usage"),
    ("weather", "commands.weather.usage"),
    ("xp", "commands.xp.usage"),
];

/// `CommandGameMode.getGameModeFromCommand`
fn game_mode(s: &str) -> Result<GameMode, Fail> {
    match s.to_lowercase().as_str() {
        "survival" | "s" | "0" => Ok(GameMode::Survival),
        "creative" | "c" | "1" => Ok(GameMode::Creative),
        "adventure" | "a" | "2" => Ok(GameMode::Survival),
        o => {
            parse_int(o, 0, 3)?;
            Ok(GameMode::Survival)
        }
    }
}

impl Sim {
    /// `NetHandlerPlayServer.processChatMessage`
    pub(super) fn chat_message(&mut self, text: &str) {
        // StringUtils.normalizeSpace
        let t: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if let Some(cmd) = t.strip_prefix('/') {
            let mut out = Vec::new();
            match self.run_command(cmd, &mut out) {
                Ok(()) => {}
                Err(Fail(m)) => out.push(m),
            }
            self.chat_out.extend(out);
        } else {
            self.chat_out
                .push(ChatMsg::tr("chat.type.text", vec![PLAYER_NAME.into(), t]));
        }
    }

    /// `CommandHandler.executeCommand`
    fn run_command(&mut self, line: &str, out: &mut Vec<ChatMsg>) -> Result<(), Fail> {
        let args: Vec<&str> = line.split(' ').collect();
        let name = args[0].to_lowercase();
        let a = &args[1..];
        let feet = self.play.player.0;
        let bpos = [
            feet[0].floor() as i32,
            (feet[1] + 0.5).floor() as i32,
            feet[2].floor() as i32,
        ];
        match name.as_str() {
            "help" | "?" => {
                // CommandHelp: seven usages a page
                let page = match a.first() {
                    Some(s) => {
                        if let Some(c) = COMMANDS.iter().find(|c| c.0 == *s) {
                            return Err(usage(c.1));
                        }
                        parse_int(s, 1, (COMMANDS.len() as i32 - 1) / 7 + 1)? - 1
                    }
                    None => 0,
                };
                let pages = (COMMANDS.len() as i32 - 1) / 7 + 1;
                let mut h = ChatMsg::tr(
                    "commands.help.header",
                    vec![(page + 1).to_string(), pages.to_string()],
                );
                h.color = Some('2');
                out.push(h);
                for c in COMMANDS.iter().skip(page as usize * 7).take(7) {
                    out.push(ChatMsg::tr(c.1, vec![]));
                }
                if page == 0 {
                    let mut f = ChatMsg::tr("commands.help.footer", vec![]);
                    f.color = Some('a');
                    out.push(f);
                }
            }
            "give" => {
                if a.len() < 2 {
                    return Err(usage("commands.give.usage"));
                }
                if a[0] != PLAYER_NAME && a[0] != "@p" {
                    return Err(fail("commands.generic.player.notFound", vec![]));
                }
                let id = item_by_text(a[1], out)?;
                let n = match a.get(2) {
                    Some(s) => parse_int(s, 1, 64)?,
                    None => 1,
                };
                let meta = match a.get(3) {
                    Some(s) => parse_int(s, i32::MIN, i32::MAX)?,
                    None => 0,
                };
                let st = Stack::new(id, meta, n as u32);
                // EntityPlayer.dropPlayerItemWithRandomChoice at the feet,
                // picked up at once
                let mut rnd = std::mem::replace(&mut self.world.rand, JavaRandom::new(0));
                self.play.throw(st, &mut rnd);
                if let Some(e) = self.play.items.last_mut() {
                    e.pickup_delay = 0;
                }
                self.world.rand = rnd;
                // ItemStack.func_151000_E: "[name]" in the rarity colour
                let c = crate::item_text::rarity_color(crate::item_text::rarity(&st));
                let item = format!("\u{2}{c}{}:{}", st.id, st.meta);
                out.push(ChatMsg::tr(
                    "commands.give.success",
                    vec![item, n.to_string(), PLAYER_NAME.into()],
                ));
            }
            "time" => {
                if a.len() < 2 {
                    return Err(usage("commands.time.usage"));
                }
                match a[0] {
                    "set" => {
                        let v = match a[1] {
                            "day" => 1000,
                            "night" => 13000,
                            s => parse_int(s, 0, i32::MAX)?,
                        };
                        self.world_time = v as i64;
                        out.push(ChatMsg::tr("commands.time.set", vec![v.to_string()]));
                    }
                    "add" => {
                        let v = parse_int(a[1], 0, i32::MAX)?;
                        self.world_time += v as i64;
                        out.push(ChatMsg::tr("commands.time.added", vec![v.to_string()]));
                    }
                    _ => return Err(usage("commands.time.usage")),
                }
            }
            "gamemode" => {
                if a.is_empty() {
                    return Err(usage("commands.gamemode.usage"));
                }
                let m = game_mode(a[0])?;
                self.play.mode = m;
                self.play.inv_dirty = true;
                self.mode_changed = true;
                let mode = match m {
                    GameMode::Creative => "gameMode.creative",
                    _ => "gameMode.survival",
                };
                out.push(ChatMsg::tr(
                    "commands.gamemode.success.self",
                    vec![tr(mode)],
                ));
            }
            "tp" => {
                if a.is_empty() {
                    return Err(usage("commands.tp.usage"));
                }
                let a = if a.len() == 4 || a.len() == 2 {
                    &a[1..]
                } else {
                    a
                };
                if a.len() == 3 {
                    let x = coord(feet[0], a[0], -30_000_000, 30_000_000)?;
                    let y = coord(feet[1], a[1], 0, 0)?;
                    let z = coord(feet[2], a[2], -30_000_000, 30_000_000)?;
                    self.teleport_to = Some([x, y, z]);
                    self.splayer.e.fall_distance = 0.0;
                    out.push(ChatMsg::tr(
                        "commands.tp.success.coordinates",
                        vec![
                            PLAYER_NAME.into(),
                            format!("{x:.2}"),
                            format!("{y:.2}"),
                            format!("{z:.2}"),
                        ],
                    ));
                } else if a.len() == 1 && (a[0] == PLAYER_NAME || a[0] == "@p") {
                    out.push(ChatMsg::tr(
                        "commands.tp.success",
                        vec![PLAYER_NAME.into(), PLAYER_NAME.into()],
                    ));
                } else {
                    return Err(fail("commands.generic.player.notFound", vec![]));
                }
            }
            "weather" => {
                if a.is_empty() || a.len() > 2 {
                    return Err(usage("commands.weather.usage"));
                }
                let mut t = (300 + self.world.rand.next_int_n(600)) * 20;
                if let Some(s) = a.get(1) {
                    t = parse_int(s, 1, 1_000_000)? * 20;
                }
                let w = &mut self.weather;
                match a[0].to_lowercase().as_str() {
                    "clear" => {
                        w.rain_time = 0;
                        w.thunder_time = 0;
                        w.raining = false;
                        w.thundering = false;
                        out.push(ChatMsg::tr("commands.weather.clear", vec![]));
                    }
                    "rain" => {
                        w.rain_time = t;
                        w.raining = true;
                        w.thundering = false;
                        out.push(ChatMsg::tr("commands.weather.rain", vec![]));
                    }
                    "thunder" => {
                        w.thunder_time = t;
                        w.raining = true;
                        w.thundering = true;
                        out.push(ChatMsg::tr("commands.weather.thunder", vec![]));
                    }
                    _ => return Err(usage("commands.weather.usage")),
                }
            }
            "toggledownfall" => {
                // WorldServer.toggleRain
                self.weather.rain_time = 1;
                out.push(ChatMsg::tr("commands.downfall.success", vec![]));
            }
            "kill" => {
                // onKillCommand: outOfWorld for Float.MAX_VALUE
                let d = self.difficulty;
                self.splayer.attack(
                    &self.world,
                    Source::named(SourceKind::OutOfWorld),
                    f32::MAX,
                    d,
                );
                out.push(ChatMsg::tr(
                    "commands.kill.success",
                    vec![PLAYER_NAME.into()],
                ));
            }
            "effect" => {
                if a.len() < 2 {
                    return Err(usage("commands.effect.usage"));
                }
                if a[0] != PLAYER_NAME && a[0] != "@p" {
                    return Err(fail("commands.generic.player.notFound", vec![]));
                }
                if a[1] == "clear" {
                    let ids: Vec<i32> = self.splayer.living.effects.iter().map(|e| e.id).collect();
                    if ids.is_empty() {
                        return Err(fail(
                            "commands.effect.failure.notActive.all",
                            vec![PLAYER_NAME.into()],
                        ));
                    }
                    for id in ids {
                        self.splayer.living.remove_effect(id);
                    }
                    out.push(ChatMsg::tr(
                        "commands.effect.success.removed.all",
                        vec![PLAYER_NAME.into()],
                    ));
                    return Ok(());
                }
                let id = parse_int(a[1], 1, i32::MAX)?;
                let Some(p) = crate::potion::potion(id) else {
                    return Err(fail("commands.effect.notFound", vec![id.to_string()]));
                };
                let mut secs = 600;
                let mut ticks = 30;
                if let Some(s) = a.get(2) {
                    ticks = parse_int(s, 0, 1_000_000)?;
                    secs = if p.instant { ticks } else { ticks * 20 };
                } else if p.instant {
                    secs = 1;
                }
                let amp = match a.get(3) {
                    Some(s) => parse_int(s, 0, 255)?,
                    None => 0,
                };
                if ticks == 0 {
                    if !self.splayer.living.effects.iter().any(|e| e.id == id) {
                        return Err(fail(
                            "commands.effect.failure.notActive",
                            vec![tr(p.name), PLAYER_NAME.into()],
                        ));
                    }
                    self.splayer.living.remove_effect(id);
                    out.push(ChatMsg::tr(
                        "commands.effect.success.removed",
                        vec![tr(p.name), PLAYER_NAME.into()],
                    ));
                } else {
                    self.splayer
                        .living
                        .add_effect(crate::potion::Effect::new(id, secs, amp));
                    out.push(ChatMsg::tr(
                        "commands.effect.success",
                        vec![
                            tr(p.name),
                            id.to_string(),
                            amp.to_string(),
                            PLAYER_NAME.into(),
                            ticks.to_string(),
                        ],
                    ));
                }
            }
            "xp" => {
                if a.is_empty() {
                    return Err(usage("commands.xp.usage"));
                }
                let s = a[0];
                let levels = s.ends_with('l') || s.ends_with('L');
                let num = if levels { &s[..s.len() - 1] } else { s };
                let n = parse_int(num, i32::MIN, i32::MAX)?;
                if levels {
                    let p = &mut self.splayer;
                    p.xp_level += n;
                    if p.xp_level < 0 {
                        p.xp_level = 0;
                        p.xp = 0.0;
                        p.xp_total = 0;
                    }
                    let key = if n < 0 {
                        "commands.xp.success.negative.levels"
                    } else {
                        "commands.xp.success.levels"
                    };
                    out.push(ChatMsg::tr(
                        key,
                        vec![n.abs().to_string(), PLAYER_NAME.into()],
                    ));
                } else {
                    if n < 0 {
                        return Err(fail("commands.xp.failure.widthdrawXp", vec![]));
                    }
                    self.splayer.add_xp(n);
                    out.push(ChatMsg::tr(
                        "commands.xp.success",
                        vec![n.to_string(), PLAYER_NAME.into()],
                    ));
                }
            }
            "seed" => {
                out.push(ChatMsg::tr(
                    "commands.seed.success",
                    vec![self.world.provider.seed.to_string()],
                ));
            }
            "difficulty" => {
                if a.is_empty() {
                    return Err(usage("commands.difficulty.usage"));
                }
                let d = match a[0].to_lowercase().as_str() {
                    "peaceful" | "p" => 0,
                    "easy" | "e" => 1,
                    "normal" | "n" => 2,
                    "hard" | "h" => 3,
                    s => parse_int(s, 0, 3)?,
                };
                self.difficulty = match d {
                    0 => Difficulty::Peaceful,
                    1 => Difficulty::Easy,
                    3 => Difficulty::Hard,
                    _ => Difficulty::Normal,
                };
                let key = [
                    "options.difficulty.peaceful",
                    "options.difficulty.easy",
                    "options.difficulty.normal",
                    "options.difficulty.hard",
                ][d as usize];
                out.push(ChatMsg::tr("commands.difficulty.success", vec![tr(key)]));
            }
            "clear" => {
                let id = match a.get(1) {
                    Some(s) => Some(item_by_text(s, out)?),
                    None => None,
                };
                let meta = match a.get(2) {
                    Some(s) => parse_int(s, -1, i32::MAX)?,
                    None => -1,
                };
                // InventoryPlayer.clearInventory
                let mut n = 0;
                let inv = &mut self.play.inv;
                for slot in inv.main.iter_mut().chain(inv.armor.iter_mut()) {
                    if let Some(s) = slot {
                        if id.is_none_or(|i| i == s.id) && (meta <= -1 || s.meta == meta) {
                            n += s.count;
                            *slot = None;
                        }
                    }
                }
                self.play.inv_dirty = true;
                if n == 0 {
                    return Err(fail("commands.clear.failure", vec![PLAYER_NAME.into()]));
                }
                out.push(ChatMsg::tr(
                    "commands.clear.success",
                    vec![PLAYER_NAME.into(), n.to_string()],
                ));
            }
            "setblock" => {
                if a.len() < 4 {
                    return Err(usage("commands.setblock.usage"));
                }
                let x = block_coord(bpos[0], a[0])?;
                let y = block_coord(bpos[1], a[1])?;
                let z = block_coord(bpos[2], a[2])?;
                let id = block_by_text(a[3], out)?;
                let meta = match a.get(4) {
                    Some(s) => parse_int(s, 0, 15)?,
                    None => 0,
                };
                if !(0..256).contains(&y) {
                    return Err(fail("commands.setblock.outOfWorld", vec![]));
                }
                let mode = a.get(5).copied().unwrap_or("replace");
                let cur = self.world.block(x, y, z);
                if mode == "keep" && cur != block::AIR {
                    return Err(fail("commands.setblock.noChange", vec![]));
                }
                if mode == "destroy" {
                    self.world.set_block(x, y, z, block::AIR, 0, 3);
                }
                if cur == id as u8 && self.world.peek_meta(x, y, z) as i32 == meta {
                    return Err(fail("commands.setblock.noChange", vec![]));
                }
                self.world.set_block(x, y, z, id as u8, meta, 3);
                out.push(ChatMsg::tr("commands.setblock.success", vec![]));
            }
            "spawnpoint" => {
                let (x, y, z) = if a.len() >= 4 {
                    (
                        block_coord(bpos[0], a[1])?,
                        block_coord(bpos[1], a[2])?,
                        block_coord(bpos[2], a[3])?,
                    )
                } else {
                    (bpos[0], bpos[1], bpos[2])
                };
                self.spawn = [x as f64 + 0.5, y as f64, z as f64 + 0.5];
                out.push(ChatMsg::tr(
                    "commands.spawnpoint.success",
                    vec![
                        PLAYER_NAME.into(),
                        x.to_string(),
                        y.to_string(),
                        z.to_string(),
                    ],
                ));
            }
            "setworldspawn" => {
                let (x, y, z) = if a.len() == 3 {
                    (
                        block_coord(bpos[0], a[0])?,
                        block_coord(bpos[1], a[1])?,
                        block_coord(bpos[2], a[2])?,
                    )
                } else if a.is_empty() {
                    (bpos[0], bpos[1], bpos[2])
                } else {
                    return Err(usage("commands.setworldspawn.usage"));
                };
                self.spawn = [x as f64 + 0.5, y as f64, z as f64 + 0.5];
                out.push(ChatMsg::tr(
                    "commands.setworldspawn.success",
                    vec![x.to_string(), y.to_string(), z.to_string()],
                ));
            }
            "say" => {
                if a.is_empty() || a[0].is_empty() {
                    return Err(usage("commands.say.usage"));
                }
                let mut m = ChatMsg::tr(
                    "chat.type.announcement",
                    vec![PLAYER_NAME.into(), a.join(" ")],
                );
                m.color = Some('d');
                out.push(m);
            }
            "me" => {
                if a.is_empty() {
                    return Err(usage("commands.me.usage"));
                }
                out.push(ChatMsg::tr(
                    "chat.type.emote",
                    vec![PLAYER_NAME.into(), a.join(" ")],
                ));
            }
            _ => return Err(fail("commands.generic.notFound", vec![])),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(sim: &mut Sim, line: &str) -> Vec<ChatMsg> {
        let mut ev = Vec::new();
        sim.step(&[Command::Chat(line.into())], &mut ev);
        ev.into_iter()
            .filter_map(|e| match e {
                Event::Chat(v) => Some(v),
                _ => None,
            })
            .flatten()
            .collect()
    }

    #[test]
    fn commands_answer_like_vanilla() {
        let cfg = SimConfig {
            seed: 99,
            view: 2,
            save_dir: None,
            start_time: 6000,
            freeze_time: true,
            mode: GameMode::Survival,
        };
        let (mut sim, _) = Sim::new(&cfg);
        let m = run(&mut sim, "/time set night");
        assert_eq!(
            m,
            vec![ChatMsg::tr("commands.time.set", vec!["13000".into()])]
        );
        assert_eq!(sim.world_time, 13000);
        let m = run(&mut sim, "/give Player stone 5");
        assert_eq!(m[0].key, "commands.give.success");
        assert_eq!(m[0].args[1], "5");
        let m = run(&mut sim, "/give Player nothing_here");
        assert_eq!(
            m,
            vec![ChatMsg::red(
                "commands.give.notFound",
                vec!["nothing_here".into()]
            )]
        );
        let m = run(&mut sim, "/gamemode 1");
        assert_eq!(m[0].key, "commands.gamemode.success.self");
        assert_eq!(sim.play.mode, GameMode::Creative);
        let m = run(&mut sim, "/frobnicate");
        assert_eq!(m, vec![ChatMsg::red("commands.generic.notFound", vec![])]);
        let m = run(&mut sim, "/weather");
        assert_eq!(m[0].key, "commands.generic.usage");
        let m = run(&mut sim, "hello   there ");
        assert_eq!(
            m,
            vec![ChatMsg::tr(
                "chat.type.text",
                vec!["Player".into(), "hello there".into()]
            )]
        );
        let m = run(&mut sim, "/xp 3L");
        assert_eq!(m[0].key, "commands.xp.success.levels");
        assert_eq!(sim.splayer.xp_level, 3);
    }
}
