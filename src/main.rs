//! Minecraft 1.7.10 rewritten in Rust on Vulkan.

#![allow(dead_code)]

mod assets;
mod audio;
mod block;
mod chat;
mod compat;
mod container;
mod crafting;
mod creative;
mod dimension;
mod drops;
mod enchant;
mod entity;
mod front;
mod game;
mod gui;
mod gui_container;
mod gui_repair;
mod inventory;
mod item;
mod item_data;
mod item_table;
mod item_tag;
mod item_text;
mod item_use;
mod java;
mod lang;
mod menu;
mod panorama;
mod particles;
mod play;
mod potion;
mod render;
mod repair;
mod screens;
mod settings;
mod shapes;
mod sim;
mod util;
mod world;

use game::{Config, Game, Request};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

/// `--dump <seed> <count> <file>`: writes raw generated chunks for comparison
/// against the reference implementation (see `tools/Harness.java`).
fn dump(args: &[String]) {
    use std::io::Write;
    let seed: i64 = args[0].parse().unwrap();
    let n: i32 = args[1].parse().unwrap();
    let p = world::ChunkProvider::new(seed);
    let mut out = std::io::BufWriter::new(std::fs::File::create(&args[2]).unwrap());
    for i in 0..n {
        let cx = ((i * 37 % 101) - 50) * 3;
        let cz = ((i * 53 % 103) - 51) * 3;
        let c = p.provide_chunk(cx, cz);
        out.write_all(&c.blocks[..]).unwrap();
        out.write_all(&c.meta[..]).unwrap();
        out.write_all(&c.biomes).unwrap();
    }
}

/// `--compare <a.bin> <b.bin>`: per-chunk difference report for two dumps.
fn compare(args: &[String]) {
    let a = std::fs::read(&args[0]).unwrap();
    let b = std::fs::read(&args[1]).unwrap();
    const N: usize = 65536 * 2 + 256;
    println!("sizes: {} / {} ({} chunks)", a.len(), b.len(), a.len() / N);
    let (mut bad, mut total) = (0, 0);
    for (i, (ca, cb)) in a.chunks(N).zip(b.chunks(N)).enumerate() {
        let i = i as i32;
        let (cx, cz) = (((i * 37 % 101) - 50) * 3, ((i * 53 % 103) - 51) * 3);
        let cnt = |r: std::ops::Range<usize>| {
            ca[r.clone()]
                .iter()
                .zip(&cb[r])
                .filter(|(x, y)| x != y)
                .count()
        };
        let (db, dm, dbi) = (cnt(0..65536), cnt(65536..131072), cnt(131072..N));
        total += 1;
        if db + dm + dbi > 0 {
            bad += 1;
            if bad <= 12 {
                println!("chunk {i} ({cx},{cz}): blocks {db}, meta {dm}, biomes {dbi}");
                let mut pairs = std::collections::BTreeMap::new();
                for j in 131072..N {
                    if ca[j] != cb[j] {
                        *pairs.entry((ca[j], cb[j])).or_insert(0) += 1;
                    }
                }
                if !pairs.is_empty() {
                    println!("   biome pairs (rust, java): {pairs:?}");
                }
                let mut shown = 0;
                for j in 0..131072 {
                    if ca[j] != cb[j] && shown < 6 {
                        let k = j % 65536;
                        println!(
                            "   {} x={} z={} y={}: {} vs {}",
                            if j < 65536 { "block" } else { "meta" },
                            k >> 12,
                            (k >> 8) & 15,
                            k & 255,
                            ca[j],
                            cb[j]
                        );
                        shown += 1;
                        if shown == 1 {
                            let base = k & !255;
                            let y = k & 255;
                            let r = y.saturating_sub(4)..(y + 4).min(256);
                            let col = |d: &[u8], o: usize| {
                                r.clone().map(|yy| d[o + base + yy]).collect::<Vec<u8>>()
                            };
                            println!(
                                "     y {:?}: rust blocks {:?} meta {:?}",
                                r,
                                col(ca, 0),
                                col(ca, 65536)
                            );
                            println!(
                                "     java blocks {:?} meta {:?}; biome r/j {} {}",
                                col(cb, 0),
                                col(cb, 65536),
                                ca[131072 + ((k >> 8) & 15) * 16 + (k >> 12)],
                                cb[131072 + ((k >> 8) & 15) * 16 + (k >> 12)]
                            );
                        }
                    }
                }
            }
        }
    }
    println!("{bad} of {total} chunks differ");
}

/// `--dump-pop <seed> <cx> <cz> <radius> <file>`: loads a square of chunks in
/// the order `tools/PopHarness.java` uses and writes the populated ones
/// (blocks, metadata, sky light, block light).
fn dump_pop(args: &[String]) {
    use std::io::Write;
    let seed: i64 = args[0].parse().unwrap();
    let (ox, oz, r): (i32, i32, i32) = (
        args[1].parse().unwrap(),
        args[2].parse().unwrap(),
        args[3].parse().unwrap(),
    );
    let dim: i32 = std::env::var("DIM")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut w = world::server::ServerWorld::new(Arc::new(world::ChunkProvider::new_dim(seed, dim)));
    w.map_features = std::env::var_os("NO_FEATURES").is_none();
    for cx in -r..=r {
        for cz in -r..=r {
            if std::env::var_os("TRACE_PRECHECK").is_some() && cx == r && cz == r {
                let tc = std::env::var("TRACE_CHUNK").unwrap();
                let q: Vec<i32> = tc.split(',').map(|s| s.parse().unwrap()).collect();
                w.debug_check_light(q[0], q[1]);
                let c = &w.chunks[&(q[0], q[1])];
                let mut sum = 0i64;
                for x in 0..16 {
                    for z in 0..16 {
                        for y in 80..96 {
                            sum += c.saved_light(true, x, y, z) as i64;
                        }
                    }
                }
                println!("step precheck sum5={} lightpop={}", sum, c.light_populated);
            }
            w.load_chunk(ox + cx, oz + cz);
            if let Ok(tc) = std::env::var("TRACE_CHUNK") {
                let q: Vec<i32> = tc.split(',').map(|s| s.parse().unwrap()).collect();
                if let Some(c) = w.chunks.get(&(q[0], q[1])) {
                    let mut sum = 0i64;
                    for x in 0..16 {
                        for z in 0..16 {
                            for y in 80..96 {
                                sum += c.saved_light(true, x, y, z) as i64;
                            }
                        }
                    }
                    println!(
                        "step {},{} mask={} sum5={} pop={}",
                        ox + cx,
                        oz + cz,
                        c.sections,
                        sum,
                        c.terrain_populated
                    );
                }
            }
        }
    }
    write_dump(&w, ox, oz, r, &args[4]);
    println!("chunks loaded: {}", w.chunks.len());
}

/// Writes chunks `-r..r` around `(ox, oz)` in the PopHarness dump format.
fn write_dump(w: &world::server::ServerWorld, ox: i32, oz: i32, r: i32, path: &str) {
    use std::io::Write;
    let mut out = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    let mut sky = vec![0u8; 65536];
    let mut blk = vec![0u8; 65536];
    for cx in -r..r {
        for cz in -r..r {
            let c = &w.chunks[&(ox + cx, oz + cz)];
            assert!(c.terrain_populated, "not populated: {cx},{cz}");
            for x in 0..16 {
                for z in 0..16 {
                    for y in 0..256 {
                        let i = ((x * 16 + z) * 256 + y) as usize;
                        sky[i] = c.saved_light(true, x, y, z) as u8;
                        blk[i] = c.saved_light(false, x, y, z) as u8;
                    }
                }
            }
            out.write_all(&c.data.blocks[..]).unwrap();
            out.write_all(&c.data.meta[..]).unwrap();
            out.write_all(&sky).unwrap();
            out.write_all(&blk).unwrap();
            out.write_all(&c.sections.to_be_bytes()).unwrap();
            for h in c.height {
                out.write_all(&(h as i16).to_be_bytes()).unwrap();
            }
        }
    }
}

/// `--write-region <seed> <cx> <cz> <radius> <dir>`: generates like
/// `--dump-pop` and stores the chunks as vanilla region files.
fn write_region(args: &[String]) {
    let seed: i64 = args[0].parse().unwrap();
    let (ox, oz, r): (i32, i32, i32) = (
        args[1].parse().unwrap(),
        args[2].parse().unwrap(),
        args[3].parse().unwrap(),
    );
    let mut w = world::server::ServerWorld::new(Arc::new(world::ChunkProvider::new(seed)));
    w.map_features = std::env::var_os("NO_FEATURES").is_none();
    for cx in -r..=r {
        for cz in -r..=r {
            w.load_chunk(ox + cx, oz + cz);
        }
    }
    let dir = std::path::PathBuf::from(&args[4]);
    for cx in -r..r {
        for cz in -r..r {
            w.write_anvil_chunk(&dir, ox + cx, oz + cz, Some((Vec::new(), Vec::new())))
                .unwrap();
        }
    }
    println!("written");
}

/// `--dump-region <seed> <cx> <cz> <radius> <dir> <out>`: reads region files
/// (from either side) and dumps them like `--dump-pop`.
fn dump_region(args: &[String]) {
    let seed: i64 = args[0].parse().unwrap();
    let (ox, oz, r): (i32, i32, i32) = (
        args[1].parse().unwrap(),
        args[2].parse().unwrap(),
        args[3].parse().unwrap(),
    );
    let mut w = world::server::ServerWorld::new(Arc::new(world::ChunkProvider::new(seed)));
    w.anvil = true;
    w.save_dir = Some(std::path::PathBuf::from(&args[4]));
    for cx in -r..r {
        for cz in -r..r {
            w.load_chunk(ox + cx, oz + cz);
        }
    }
    write_dump(&w, ox, oz, r, &args[5]);
    println!("read");
}

/// `--compare-pop <a> <b> <cx> <cz> <radius>`
fn compare_pop(args: &[String]) {
    let a = std::fs::read(&args[0]).unwrap();
    let b = std::fs::read(&args[1]).unwrap();
    let (ox, oz, r): (i32, i32, i32) = (
        args[2].parse().unwrap(),
        args[3].parse().unwrap(),
        args[4].parse().unwrap(),
    );
    const N: usize = 65536 * 4 + 2 + 512;
    println!("sizes: {} / {} ({} chunks)", a.len(), b.len(), a.len() / N);
    let names = ["block", "meta", "sky", "blocklight"];
    let (mut bad, mut total) = (0, 0);
    let mut pairs = std::collections::BTreeMap::new();
    for (i, (ca, cb)) in a.chunks(N).zip(b.chunks(N)).enumerate() {
        let (cx, cz) = (ox - r + (i as i32) / (2 * r), oz - r + (i as i32) % (2 * r));
        let cnt: Vec<usize> = (0..4)
            .map(|k| {
                ca[k * 65536..(k + 1) * 65536]
                    .iter()
                    .zip(&cb[k * 65536..(k + 1) * 65536])
                    .filter(|(x, y)| x != y)
                    .count()
            })
            .collect();
        total += 1;
        for j in 0..65536 {
            if ca[j] != cb[j] {
                *pairs.entry((ca[j], cb[j])).or_insert(0usize) += 1;
            }
        }
        let extra = ca[262144..] != cb[262144..];
        if extra {
            println!(
                "chunk ({cx},{cz}): sections rust {:#06x} java {:#06x}, height diffs {}",
                u16::from_be_bytes([ca[262144], ca[262145]]),
                u16::from_be_bytes([cb[262144], cb[262145]]),
                ca[262146..]
                    .chunks(2)
                    .zip(cb[262146..].chunks(2))
                    .filter(|(a, b)| a != b)
                    .count()
            );
        }
        if cnt.iter().sum::<usize>() > 0 || extra {
            bad += 1;
            if bad <= 10 {
                println!(
                    "chunk ({cx},{cz}): blocks {}, meta {}, sky {}, blocklight {}",
                    cnt[0], cnt[1], cnt[2], cnt[3]
                );
                let mut shown = 0;
                for j in 0..262144 {
                    if ca[j] != cb[j] && shown < 5 {
                        let k = j % 65536;
                        println!(
                            "   {} x={} y={} z={}: {} vs {} (block {})",
                            names[j / 65536],
                            cx * 16 + (k >> 12) as i32,
                            k & 255,
                            cz * 16 + ((k >> 8) & 15) as i32,
                            ca[j],
                            cb[j],
                            ca[k]
                        );
                        shown += 1;
                    }
                }
            }
        }
    }
    if !pairs.is_empty() {
        println!("block pairs (rust, java): {pairs:?}");
    }
    println!("{bad} of {total} chunks differ");
}

/// `--find-biomes <seed>`: one chunk position per biome id found near the origin.
/// `--locate <seed> <radius>`: structure starts within `radius` chunks of the
/// origin, as `type chunkX chunkZ blockX blockY blockZ pieces`.
fn locate(args: &[String]) {
    let seed: i64 = args[0].parse().unwrap();
    let r: i32 = args[1].parse().unwrap();
    let dim: i32 = std::env::var("DIM")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let provider = world::ChunkProvider::new_dim(seed, dim);
    let mut s = world::structure::Structures::new(&provider);
    let mut rand = java::JavaRandom::new(0);
    let mut x = -r;
    while x <= r {
        let mut z = -r;
        while z <= r {
            s.register(&provider, x, z, &mut rand);
            z += 17;
        }
        x += 17;
    }
    for g in &s.gens {
        for st in g.map.starts() {
            if !st.sizeable {
                continue;
            }
            let p = &st.pieces[0];
            println!(
                "{:?} {} {} {} {} {} {}",
                g.ty,
                st.cx,
                st.cz,
                p.bb.cx(),
                p.bb.y0,
                p.bb.cz(),
                st.pieces.len()
            );
        }
    }
    println!("strongholds: {:?}", s.stronghold_coords);
}

fn find_biomes(args: &[String]) {
    let seed: i64 = args[0].parse().unwrap();
    let p = world::ChunkProvider::new(seed);
    let mut found = std::collections::BTreeMap::new();
    let mut step = 0;
    'outer: for ring in 0..80i32 {
        for a in -ring..=ring {
            for b in -ring..=ring {
                if a.abs() != ring && b.abs() != ring {
                    continue;
                }
                let (x, z) = (a * 96, b * 96);
                step += 1;
                let id = p.biome_at(x, z);
                // Require a patch large enough that the populated area is inside it.
                if !found.contains_key(&id)
                    && [0, 2, 6, 8].iter().all(|&i| {
                        p.biome_index.get_ints(
                            x + (i as i32 % 3 - 1) * 40,
                            z + (i as i32 / 3 - 1) * 40,
                            1,
                            1,
                        )[0] == id
                    })
                {
                    found.insert(id, (x >> 4, z >> 4));
                }
                if step > 40000 {
                    break 'outer;
                }
            }
        }
    }
    for (id, (cx, cz)) in found {
        println!("{id} {cx} {cz}");
    }
}

struct Options {
    cfg: Config,
    jar: Option<PathBuf>,
    /// Window size in physical pixels.
    size: (u32, u32),
    no_save: bool,
    save_dir: Option<PathBuf>,
    seed_given: bool,
    /// Automated capture: file, seconds to wait.
    auto_shot: Option<(PathBuf, f64)>,
    /// Straight into a world (a seed, world, position or capture was given)
    /// instead of the title screen.
    direct: bool,
    /// `--title`: the title screen even with the options above.
    title: bool,
}

/// The file next to the executable that remembers a jar picked by hand.
fn jar_memo() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.join("minerust_jar.txt")))
}

/// `%APPDATA%` as Windows knows it (`FOLDERID_RoamingAppData`), which does
/// not depend on the environment the process was started with.
#[cfg(windows)]
fn roaming_appdata() -> Option<PathBuf> {
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{FOLDERID_RoamingAppData, SHGetKnownFolderPath};
    let mut p: *mut u16 = std::ptr::null_mut();
    let hr =
        unsafe { SHGetKnownFolderPath(&FOLDERID_RoamingAppData, 0, std::ptr::null_mut(), &mut p) };
    if hr != 0 || p.is_null() {
        return None;
    }
    let s = unsafe {
        let mut n = 0;
        while *p.add(n) != 0 {
            n += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(p, n))
    };
    unsafe { CoTaskMemFree(p as *const _) };
    Some(PathBuf::from(s))
}

#[cfg(not(windows))]
fn roaming_appdata() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".minecraft").join(".."))
}

/// A file dialog for the jar when none was found.
#[cfg(windows)]
fn pick_jar() -> Option<PathBuf> {
    use windows_sys::Win32::UI::Controls::Dialogs::*;
    let w = |s: &str| {
        s.encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>()
    };
    let filter: Vec<u16> = "Minecraft 1.7.10 jar (1.7.10.jar)\0*.jar\0\0"
        .encode_utf16()
        .collect();
    let title = w("找不到 1.7.10.jar：請選擇原版 Minecraft 1.7.10 的客戶端 jar");
    let mut buf = vec![0u16; 4096];
    let mut ofn: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    ofn.lpstrFilter = filter.as_ptr();
    ofn.lpstrFile = buf.as_mut_ptr();
    ofn.nMaxFile = buf.len() as u32;
    ofn.lpstrTitle = title.as_ptr();
    ofn.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
    if unsafe { GetOpenFileNameW(&mut ofn) } == 0 {
        return None;
    }
    let n = buf.iter().position(|&c| c == 0).unwrap_or(0);
    Some(PathBuf::from(String::from_utf16_lossy(&buf[..n])))
}

#[cfg(not(windows))]
fn pick_jar() -> Option<PathBuf> {
    None
}

fn find_jar(explicit: &Option<PathBuf>) -> Result<PathBuf, String> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(p) = explicit {
        candidates.push(p.clone());
    }
    if let Ok(p) = std::env::var("MC1710_JAR") {
        candidates.push(p.into());
    }
    // a jar picked by hand earlier
    if let Some(t) = jar_memo().and_then(|m| std::fs::read_to_string(m).ok()) {
        candidates.push(PathBuf::from(t.trim()));
    }
    // next to the executable, then the working directory
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.to_path_buf()))
    {
        candidates.push(dir.join("1.7.10.jar"));
    }
    candidates.push("1.7.10.jar".into());
    // a game directory beside the executable or one of its parents
    // (<repo>/game when run from target/release)
    if let Ok(exe) = std::env::current_exe() {
        for dir in exe.ancestors().skip(1).take(4) {
            candidates.push(
                dir.join("game")
                    .join("versions")
                    .join("1.7.10")
                    .join("1.7.10.jar"),
            );
        }
    }
    // the launchers' game directories: <root>/versions/1.7.10/1.7.10.jar,
    // with the sounds and languages in <root>/assets
    let version = |root: PathBuf| root.join("versions").join("1.7.10").join("1.7.10.jar");
    let mut roots: Vec<PathBuf> = Vec::new();
    roots.extend(roaming_appdata());
    roots.extend(std::env::var_os("APPDATA").map(PathBuf::from));
    if let Some(home) = std::env::var_os("USERPROFILE") {
        roots.push(PathBuf::from(home).join("AppData").join("Roaming"));
    }
    if let Some(user) = std::env::var_os("USERNAME") {
        roots.push(
            PathBuf::from(r"C:\Users")
                .join(user)
                .join("AppData")
                .join("Roaming"),
        );
    }
    // MSIX-packaged apps (a launcher started from one) write AppData into
    // their own LocalCache, which only they see as %APPDATA%
    if let Some(local) = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| roaming_appdata().and_then(|r| r.parent().map(|p| p.join("Local"))))
    {
        if let Ok(rd) = std::fs::read_dir(local.join("Packages")) {
            for e in rd.flatten() {
                let r = e.path().join("LocalCache").join("Roaming");
                if r.is_dir() {
                    roots.push(r);
                }
            }
        }
    }
    roots.dedup();
    for r in &roots {
        for launcher in [".minecraft", "Sapling", ".hmcl", "PCL", ".tlauncher"] {
            candidates.push(version(r.join(launcher)));
        }
    }
    if let Some(found) = candidates.iter().find(|p| p.is_file()) {
        return Ok(found.clone());
    }
    if let Some(p) = pick_jar().filter(|p| p.is_file()) {
        if let Some(m) = jar_memo() {
            let _ = std::fs::write(m, p.to_string_lossy().as_bytes());
        }
        return Ok(p);
    }
    let tried: Vec<String> = candidates
        .iter()
        .map(|p| format!("  {}", p.display()))
        .collect();
    Err(format!(
        "找不到 1.7.10.jar（用 --jar <路徑> 或環境變數 MC1710_JAR 指定原版客戶端 jar，材質由其中讀取）\n\n找過的位置：\n{}",
        tried.join("\n")
    ))
}

fn parse_args() -> Options {
    let args: Vec<String> = std::env::args().collect();
    let mut o = Options {
        cfg: Config {
            mode: entity::survival::GameMode::Survival,
            save_dir: None,
            // A new vanilla world without a typed seed uses `new Random().nextLong()`.
            seed: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as i64)
                .unwrap_or(0),
            render_distance: 12,
            start_time: 0,
            pos: None,
            look: None,
            freeze_time: false,
        },
        jar: None,
        size: (854, 480),
        no_save: false,
        save_dir: None,
        seed_given: false,
        auto_shot: None,
        direct: false,
        title: false,
    };
    let mut i = 1;
    let num = |s: &String| s.parse::<f64>().unwrap_or(0.0);
    let _ = &o.seed_given;
    while i < args.len() {
        match args[i].as_str() {
            "--seed" if i + 1 < args.len() => {
                // GuiCreateWorld: numeric seeds are used directly, text is hashed.
                let s = &args[i + 1];
                o.cfg.seed = match s.parse::<i64>() {
                    Ok(v) if v != 0 => v,
                    _ => java::string_hash_code(s) as i64,
                };
                o.seed_given = true;
                i += 1;
            }
            "--jar" if i + 1 < args.len() => {
                o.jar = Some(args[i + 1].clone().into());
                i += 1;
            }
            "--distance" if i + 1 < args.len() => {
                o.cfg.render_distance = (num(&args[i + 1]) as i32).clamp(2, 16);
                i += 1;
            }
            "--time" if i + 1 < args.len() => {
                o.cfg.start_time = num(&args[i + 1]) as i64;
                i += 1;
            }
            "--freeze-time" => o.cfg.freeze_time = true,
            "--weather" if i + 1 < args.len() => {
                std::env::set_var("MINERUST_WEATHER", &args[i + 1]);
                i += 1;
            }
            "--debug" => std::env::set_var("MINERUST_F3", "1"),
            "--dim" if i + 1 < args.len() => {
                std::env::set_var("MINERUST_DIM", &args[i + 1]);
                i += 1;
            }
            "--setblock" if i + 1 < args.len() => {
                // `/setblock x y z id meta`, several separated by ';'
                std::env::set_var("MINERUST_SETBLOCK", &args[i + 1]);
                i += 1;
            }
            "--summon" if i + 1 < args.len() => {
                std::env::set_var("MINERUST_SUMMON", &args[i + 1]);
                i += 1;
            }
            "--vanilla-parity" => compat::set_vanilla_parity(true),
            "--no-save" => o.no_save = true,
            "--give" if i + 1 < args.len() => {
                std::env::set_var("MINERUST_GIVE", &args[i + 1]);
                i += 1;
            }
            "--effect" if i + 1 < args.len() => {
                // CommandEffect: "id:seconds:amplifier", several separated by ','
                std::env::set_var("MINERUST_EFFECT", &args[i + 1]);
                i += 1;
            }
            "--open" if i + 1 < args.len() => {
                std::env::set_var("MINERUST_OPEN", &args[i + 1]);
                i += 1;
            }
            "--creative" => o.cfg.mode = entity::survival::GameMode::Creative,
            "--validate" => std::env::set_var("MINERUST_VALIDATE", "1"),
            "--world" if i + 1 < args.len() => {
                o.save_dir = Some(args[i + 1].clone().into());
                i += 1;
            }
            "--size" if i + 2 < args.len() => {
                o.size = (
                    (num(&args[i + 1]) as u32).max(64),
                    (num(&args[i + 2]) as u32).max(64),
                );
                i += 2;
            }
            "--pos" if i + 3 < args.len() => {
                o.cfg.pos = Some([num(&args[i + 1]), num(&args[i + 2]), num(&args[i + 3])]);
                i += 3;
            }
            "--look" if i + 2 < args.len() => {
                o.cfg.look = Some([num(&args[i + 1]) as f32, num(&args[i + 2]) as f32]);
                i += 2;
            }
            "--screenshot" if i + 2 < args.len() => {
                o.auto_shot = Some((args[i + 1].clone().into(), num(&args[i + 2])));
                i += 2;
            }
            "--title" => o.title = true,
            "--menu" if i + 1 < args.len() => {
                std::env::set_var("MINERUST_MENU", &args[i + 1]);
                o.title = true;
                i += 1;
            }
            other => eprintln!("unknown argument: {other}"),
        }
        i += 1;
    }
    o.direct = !o.title
        && (o.seed_given
            || o.save_dir.is_some()
            || o.auto_shot.is_some()
            || o.cfg.pos.is_some()
            || o.no_save);
    o
}

/// `org.lwjgl.opengl.Sync`: holds the loop to a frame rate
/// (`Display.sync(getLimitFramerate())` in `Minecraft.runGameLoop`).
struct FrameSync {
    next: Option<Instant>,
}

impl FrameSync {
    fn sync(&mut self, fps: i32) {
        if fps <= 0 {
            return;
        }
        let step = std::time::Duration::from_nanos(1_000_000_000 / fps as u64);
        let now = Instant::now();
        let Some(next) = self.next else {
            self.next = Some(now + step);
            return;
        };
        // sleep while more than a scheduler slice remains, then yield
        loop {
            let t = Instant::now();
            if t >= next {
                break;
            }
            if next - t > std::time::Duration::from_micros(1500) {
                std::thread::sleep(std::time::Duration::from_millis(1));
            } else {
                std::thread::yield_now();
            }
        }
        // nextFrame = max(nextFrame + NANOS_IN_SECOND / fps, getTime())
        self.next = Some((next + step).max(Instant::now()));
    }
}

/// `MINERUST_BENCH=<seconds>`: frame times once the world has loaded.
struct Bench {
    secs: f64,
    began: Option<Instant>,
    last: Instant,
    frames: Vec<f32>,
    prof0: [f64; 4],
    /// Server ticks and their nanoseconds when the measurement began.
    ticks0: (u64, u64),
}

struct App {
    bench: Option<Bench>,
    sync: FrameSync,
    opts: Options,
    game: Option<Game>,
    /// The title screen and its menus while no world is loaded.
    front: Option<front::Front>,
    window: Option<Arc<Window>>,
    grabbed: bool,
    /// Whether the game wanted the mouse on the last frame.
    wanted_grab: bool,
    focused: bool,
    started: Instant,
    shift: bool,
    ctrl: bool,
    shot_taken: Option<Instant>,
    error: Option<String>,
    atlas: Option<Arc<render::atlas::Atlas>>,
}

impl App {
    fn set_grab(&mut self, grab: bool) {
        let Some(w) = &self.window else { return };
        if grab {
            if w.set_cursor_grab(CursorGrabMode::Locked).is_err() {
                let _ = w.set_cursor_grab(CursorGrabMode::Confined);
            }
            w.set_cursor_visible(false);
        } else {
            let _ = w.set_cursor_grab(CursorGrabMode::None);
            w.set_cursor_visible(true);
            // MouseHelper.ungrabMouseCursor: the pointer goes to the middle
            let size = w.inner_size();
            let mid = (size.width as f32 / 2.0, size.height as f32 / 2.0);
            let _ = w.set_cursor_position(winit::dpi::PhysicalPosition::new(mid.0, mid.1));
            if let Some(g) = &mut self.game {
                g.input = Default::default();
                g.mouse_px = mid;
            }
        }
        if std::env::var_os("MINERUST_DEBUG_GRAB").is_some() {
            println!("Mouse: {}", if grab { "grabbed" } else { "released" });
        }
        self.grabbed = grab;
    }

    fn handle(&mut self, req: Request, el: &ActiveEventLoop) {
        match req {
            Request::None => {}
            Request::Release => self.set_grab(false),
            Request::Grab => self.set_grab(true),
            Request::Quit => {
                if let Some(g) = &mut self.game {
                    g.shutdown();
                }
                self.game = None;
                el.exit();
            }
            Request::QuitToTitle => {
                // Minecraft.loadWorld(null): save, then GuiMainMenu
                let Some(mut g) = self.game.take() else {
                    return;
                };
                g.shutdown();
                let (renderer, shell, lang, audio) = g.into_parts();
                let atlas = self.atlas.clone().unwrap();
                if let Some(shell) = shell {
                    self.front = Some(front::Front::new(renderer, atlas, shell, lang, audio));
                }
                self.set_grab(false);
            }
        }
    }

    /// `Minecraft.launchIntegratedServer` from the menus.
    fn launch(&mut self, folder: PathBuf, new: Option<menu::NewWorld>) {
        let Some(f) = self.front.take() else { return };
        let front::Front {
            renderer,
            atlas,
            shell,
            lang,
            audio,
            ..
        } = f;
        let mut cfg = self.opts.cfg.clone();
        cfg.save_dir = Some(folder.clone());
        cfg.render_distance = shell.settings.render_distance.clamp(2, 16);
        cfg.pos = None;
        cfg.look = None;
        if let Some(n) = &new {
            let _ = front::new_level_dat(&folder, n);
            cfg.seed = n.seed;
            cfg.mode = if n.mode == "creative" {
                entity::survival::GameMode::Creative
            } else {
                entity::survival::GameMode::Survival
            };
        }
        let mut game = Game::new(renderer, atlas, &cfg);
        game.lang = lang;
        game.audio = audio;
        game.shell = Some(shell);
        game.apply_settings();
        game.apply_start_screen();
        println!("World: {}", folder.display());
        self.game = Some(game);
        self.set_grab(true);
    }

    fn front_action(&mut self, a: menu::Action, el: &ActiveEventLoop) {
        match a {
            menu::Action::Quit => {
                self.front = None;
                el.exit();
            }
            menu::Action::Play { folder, new } => self.launch(folder, new),
            _ => {}
        }
    }

    /// The bound key or button of a `KeyBinding` (LWJGL code).
    fn bound(&self, d: &str) -> i32 {
        self.game
            .as_ref()
            .and_then(|g| g.shell.as_ref())
            .map_or_else(
                || settings::GameSettings::default().key(d),
                |s| s.settings.key(d),
            )
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("Minecraft 1.7.10")
            .with_inner_size(winit::dpi::PhysicalSize::new(
                self.opts.size.0,
                self.opts.size.1,
            ));
        let window = match el.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                self.error = Some(format!("create_window: {e}"));
                el.exit();
                return;
            }
        };
        let result = (|| -> Result<(), String> {
            let jar_path = find_jar(&self.opts.jar)?;
            println!("Jar: {}", jar_path.display());
            let file = std::fs::File::open(&jar_path)
                .map_err(|e| format!("{}: {e}", jar_path.display()))?;
            let mut jar =
                zip::ZipArchive::new(file).map_err(|e| format!("{}: {e}", jar_path.display()))?;
            let atlas = Arc::new(render::atlas::Atlas::load(&mut jar)?);
            atlas.glyphs.lock().unwrap().set_jar(jar_path.clone());
            self.atlas = Some(atlas.clone());
            let renderer = render::vk::Renderer::new(&window, &atlas)?;
            println!("Vulkan device: {}", renderer.device_name);
            // The launcher keeps assets next to versions/: <root>/assets.
            let root = jar_path
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.parent())
                .map(|p| p.to_path_buf());
            let assets = root
                .as_ref()
                .and_then(|r| assets::Assets::open(&r.join("assets")));
            let game_dir = std::env::current_dir().unwrap_or_default();
            let settings = settings::GameSettings::load(&game_dir);
            let mut splashes = Vec::new();
            if let Ok(mut f) = jar.by_name("assets/minecraft/texts/splashes.txt") {
                use std::io::Read;
                let mut s = String::new();
                let _ = f.read_to_string(&mut s);
                splashes = s
                    .lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect();
            }
            let shell = front::Shell {
                languages: lang::languages(assets.as_ref()),
                settings,
                assets,
                jar_path: jar_path.clone(),
                saves: game_dir.join("saves"),
                clipboard: String::new(),
                splashes,
                ctrl: false,
                shift: false,
            };
            let lang = shell.load_lang(&atlas);
            let audio = if self.opts.auto_shot.is_none() {
                root.as_ref()
                    .and_then(|r| audio::Audio::new(&r.join("assets")))
            } else {
                None
            };
            println!(
                "Sound: {}",
                if audio.is_some() {
                    "on"
                } else if self.opts.auto_shot.is_some() {
                    "off (screenshot run)"
                } else {
                    "off (assets not found)"
                }
            );
            if !self.opts.direct {
                let mut f = front::Front::new(renderer, atlas, shell, lang, audio);
                if let Ok(m) = std::env::var("MINERUST_MENU") {
                    for name in m.split(',') {
                        f.menu.open_named(name, &f.lang);
                    }
                }
                self.front = Some(f);
                return Ok(());
            }
            // Worlds live in `saves/<name>`; automated captures never save.
            if !self.opts.no_save && self.opts.auto_shot.is_none() {
                let dir = self.opts.save_dir.clone().unwrap_or_else(|| {
                    PathBuf::from("saves").join(if self.opts.seed_given {
                        format!("seed_{}", self.opts.cfg.seed)
                    } else {
                        "world".to_string()
                    })
                });
                self.opts.cfg.save_dir = Some(dir);
            }
            let mut game = Game::new(renderer, atlas, &self.opts.cfg);
            game.lang = lang;
            game.audio = audio;
            game.shell = Some(shell);
            game.apply_video_settings();
            game.apply_start_screen();
            println!(
                "Seed: {}{}",
                game.seed,
                if game.resumed { " (resumed)" } else { "" }
            );
            if let Some(d) = &self.opts.cfg.save_dir {
                println!("World: {}", d.display());
            }
            println!("Spawn: {}", game.spawn_info());
            self.game = Some(game);
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.window = Some(window);
                self.started = Instant::now();
            }
            Err(e) => {
                self.error = Some(e);
                el.exit();
            }
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                if let Some(g) = &mut self.game {
                    g.shutdown();
                }
                self.game = None;
                self.front = None;
                el.exit();
            }
            WindowEvent::Resized(_) => {
                if let Some(g) = &mut self.game {
                    g.renderer.resize();
                }
                if let Some(f) = &mut self.front {
                    f.renderer.resize();
                }
            }
            WindowEvent::Focused(f) => {
                self.focused = f;
                if !f {
                    self.set_grab(false);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(g) = &mut self.game {
                    g.mouse_px = (position.x as f32, position.y as f32);
                    g.mouse_moved();
                }
                if let Some(f) = &mut self.front {
                    f.mouse_px = (position.x as f32, position.y as f32);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let pressed = state == ElementState::Pressed;
                let b = match button {
                    MouseButton::Left => 0,
                    MouseButton::Right => 1,
                    MouseButton::Middle => 2,
                    _ => return,
                };
                if let Some(f) = &mut self.front {
                    let a = f.mouse_button(b, pressed);
                    self.front_action(a, el);
                    return;
                }
                let Some(g) = &mut self.game else { return };
                if pressed && g.wants_grab() && !self.grabbed {
                    // The first click only captures the mouse.
                    self.set_grab(true);
                    return;
                }
                // KeyBinding: the mouse buttons bound to attack, use, pick
                let mut b = b;
                if g.wants_grab() {
                    let code = -100 + b as i32;
                    let (attack, use_, pick) = (
                        self.bound("key.attack"),
                        self.bound("key.use"),
                        self.bound("key.pickItem"),
                    );
                    b = if code == attack {
                        0
                    } else if code == use_ {
                        1
                    } else if code == pick {
                        2
                    } else {
                        return;
                    };
                }
                let Some(g) = &mut self.game else { return };
                let shift = self.shift;
                let req = g.mouse_button(b, pressed, shift);
                self.handle(req, el);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                // InventoryPlayer.changeCurrentItem
                let d = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => y,
                    winit::event::MouseScrollDelta::PixelDelta(p) => p.y as f32,
                };
                if d != 0.0 {
                    if let Some(f) = &mut self.front {
                        f.wheel(d > 0.0);
                    }
                    if let Some(g) = &mut self.game {
                        if g.menu_open() {
                            g.menu_wheel(d > 0.0);
                        } else {
                            g.scroll(d > 0.0);
                        }
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                match code {
                    KeyCode::ShiftLeft | KeyCode::ShiftRight => self.shift = pressed,
                    KeyCode::ControlLeft | KeyCode::ControlRight => self.ctrl = pressed,
                    _ => {}
                }
                let (ctrl, shift) = (self.ctrl, self.shift);
                // Minecraft.dispatchKeypresses: keyBindFullscreen works on
                // every screen (toggleFullscreen, then saveOptions)
                if pressed && !event.repeat {
                    let lw = settings::lwjgl_code(code);
                    let st = match (&mut self.front, &mut self.game) {
                        (Some(f), _) => Some(&mut f.shell.settings),
                        (None, Some(g)) => g.shell.as_mut().map(|s| &mut s.settings),
                        _ => None,
                    };
                    if let Some(st) = st {
                        if lw == st.key("key.fullscreen") {
                            st.full_screen = !st.full_screen;
                            st.save();
                            return;
                        }
                    }
                }
                // GuiScreen.handleKeyboardInput: the character and the key
                let ch = |e: &winit::event::KeyEvent| -> char {
                    if ctrl {
                        // Ctrl+A/C/V/X as control characters
                        match code {
                            KeyCode::KeyA => return '\u{1}',
                            KeyCode::KeyC => return '\u{3}',
                            KeyCode::KeyV => return '\u{16}',
                            KeyCode::KeyX => return '\u{18}',
                            _ => {}
                        }
                    }
                    e.text
                        .as_ref()
                        .and_then(|t| t.chars().next())
                        .filter(|c| !c.is_control())
                        .unwrap_or('\0')
                };
                if let Some(f) = &mut self.front {
                    f.shell.ctrl = ctrl;
                    f.shell.shift = shift;
                    if pressed {
                        let c = ch(&event);
                        let a = f.key(c, settings::lwjgl_code(code));
                        self.front_action(a, el);
                    }
                    return;
                }
                let Some(g) = &mut self.game else { return };
                if let Some(s) = g.shell.as_mut() {
                    s.ctrl = ctrl;
                    s.shift = shift;
                }
                if g.menu_open() {
                    if pressed {
                        let c = ch(&event);
                        let req = g.menu_key(c, settings::lwjgl_code(code));
                        self.handle(req, el);
                    }
                    return;
                }
                let lw = settings::lwjgl_code(code);
                if g.text_input() {
                    if pressed {
                        let c = ch(&event);
                        let req = g.text_key(c, lw, ctrl, shift);
                        self.handle(req, el);
                    }
                    return;
                }
                if lw == self.bound("key.screenshot") && pressed && !event.repeat {
                    let Some(g) = &mut self.game else { return };
                    let _ = std::fs::create_dir_all("screenshots");
                    let t = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    g.screenshot = Some(PathBuf::from(format!("screenshots/{t}.png")));
                    return;
                }
                let Some(g) = &mut self.game else { return };
                if code == KeyCode::F3 && pressed && !event.repeat {
                    g.show_debug = !g.show_debug;
                    return;
                }
                if pressed && !event.repeat {
                    let req = g.key(code, true, ctrl);
                    if req != Request::None {
                        self.handle(req, el);
                        return;
                    }
                }
                let bind = |d: &str| self.bound(d);
                let (fw, lf, bk, rt, jm, sn, sp) = (
                    bind("key.forward"),
                    bind("key.left"),
                    bind("key.back"),
                    bind("key.right"),
                    bind("key.jump"),
                    bind("key.sneak"),
                    bind("key.sprint"),
                );
                let Some(g) = &mut self.game else { return };
                if !g.wants_grab() {
                    return;
                }
                if lw == fw {
                    g.input.forward = pressed;
                }
                if lw == bk {
                    g.input.back = pressed;
                }
                if lw == lf {
                    g.input.left = pressed;
                }
                if lw == rt {
                    g.input.right = pressed;
                }
                if lw == jm {
                    g.input.jump = pressed;
                }
                if lw == sn {
                    g.input.sneak = pressed;
                }
                if lw == sp {
                    g.input.sprint = pressed;
                }
            }
            WindowEvent::RedrawRequested => {
                let Some(w) = self.window.clone() else { return };
                // gameSettings.fullScreen, from F11 or the video settings
                let full = match (&self.front, &self.game) {
                    (Some(f), _) => Some(f.shell.settings.full_screen),
                    (None, Some(g)) => g.shell.as_ref().map(|s| s.settings.full_screen),
                    _ => None,
                };
                if let Some(full) = full {
                    if self.opts.auto_shot.is_none() && w.fullscreen().is_some() != full {
                        w.set_fullscreen(full.then(|| winit::window::Fullscreen::Borderless(None)));
                    }
                }
                let size = w.inner_size();
                if let Some(f) = &mut self.front {
                    if let Some((path, wait)) = &self.opts.auto_shot {
                        let elapsed = self.started.elapsed().as_secs_f64();
                        if self.shot_taken.is_none() && elapsed >= *wait {
                            f.screenshot = Some(path.clone());
                            self.shot_taken = Some(Instant::now());
                        } else if let Some(t) = self.shot_taken {
                            if t.elapsed().as_secs_f64() > 0.5 {
                                self.front = None;
                                el.exit();
                                return;
                            }
                        }
                    }
                    // Display.setVSyncEnabled; getLimitFramerate is 30 with
                    // a screen open and no world
                    f.renderer.set_vsync(f.shell.settings.enable_vsync);
                    let a = f.frame((size.width, size.height));
                    self.front_action(a, el);
                    if self.bench.is_none() {
                        self.sync.sync(30);
                    }
                    return;
                }
                if let Some(g) = &mut self.game {
                    if let Some(b) = &mut self.bench {
                        let now = Instant::now();
                        let elapsed = self.started.elapsed().as_secs_f64();
                        match b.began {
                            None => {
                                if g.idle() || elapsed > 180.0 {
                                    println!(
                                        "Bench: world ready after {elapsed:.2} s, {} meshes",
                                        g.loaded_meshes()
                                    );
                                    g.renderer.set_vsync(
                                        std::env::var_os("MINERUST_BENCH_VSYNC").is_some(),
                                    );
                                    b.began = Some(now);
                                    b.prof0 = g.prof;
                                    {
                                        use std::sync::atomic::Ordering::Relaxed;
                                        b.ticks0 = (
                                            sim::TICK_COUNT.load(Relaxed),
                                            sim::TICK_NANOS.load(Relaxed),
                                        );
                                        sim::TICK_WORST.store(0, Relaxed);
                                    }
                                    println!(
                                        "Bench: at start: {} chunk snapshots, {} mesh uploads, {} events",
                                        g.counts[0], g.counts[1], g.counts[2]
                                    );
                                }
                            }
                            Some(t0) => {
                                b.frames
                                    .push(now.duration_since(b.last).as_secs_f32() * 1000.0);
                                // a slow turn, so the visible set keeps changing
                                if std::env::var_os("MINERUST_BENCH_STILL").is_none() {
                                    g.player.turn(1.5, 0.0);
                                }
                                if now.duration_since(t0).as_secs_f64() >= b.secs {
                                    let mut f = b.frames.split_off(2.min(b.frames.len()));
                                    let n = f.len().max(1);
                                    let avg = f.iter().sum::<f32>() / n as f32;
                                    f.sort_by(|a, b| a.partial_cmp(b).unwrap());
                                    let pct = |p: f64| f[((n as f64 * p) as usize).min(n - 1)];
                                    println!(
                                        "Bench: {n} frames, avg {avg:.3} ms ({:.0} fps), median {:.3}, 99% {:.3}, 99.9% {:.3}, max {:.3} ms",
                                        1000.0 / avg,
                                        pct(0.5),
                                        pct(0.99),
                                        pct(0.999),
                                        f[n - 1]
                                    );
                                    let p = g.prof;
                                    let k = 1000.0 / n as f64;
                                    println!(
                                        "Bench: per frame pump {:.3} ms, ticks {:.3}, build {:.3}, render {:.3}",
                                        (p[0] - b.prof0[0]) * k,
                                        (p[1] - b.prof0[1]) * k,
                                        (p[2] - b.prof0[2]) * k,
                                        (p[3] - b.prof0[3]) * k
                                    );
                                    {
                                        use std::sync::atomic::Ordering::Relaxed;
                                        let n = sim::TICK_COUNT.load(Relaxed) - b.ticks0.0;
                                        let ns = sim::TICK_NANOS.load(Relaxed) - b.ticks0.1;
                                        println!(
                                            "Bench: server {n} ticks, avg {:.3} ms, worst {:.3} ms",
                                            ns as f64 / n.max(1) as f64 / 1e6,
                                            sim::TICK_WORST.load(Relaxed) as f64 / 1e6
                                        );
                                    }
                                    println!(
                                        "Bench: totals: {} chunk snapshots, {} mesh uploads, {} events",
                                        g.counts[0], g.counts[1], g.counts[2]
                                    );
                                    g.shutdown();
                                    self.game = None;
                                    el.exit();
                                    return;
                                }
                            }
                        }
                        b.last = now;
                    }
                    if let Some((path, wait)) = &self.opts.auto_shot {
                        let elapsed = self.started.elapsed().as_secs_f64();
                        if self.shot_taken.is_none()
                            && elapsed >= *wait
                            && (g.idle()
                                || elapsed >= *wait + 60.0
                                || std::env::var_os("MINERUST_NOWAIT").is_some())
                        {
                            println!("Meshes: {}", g.loaded_meshes());
                            g.screenshot = Some(path.clone());
                            self.shot_taken = Some(Instant::now());
                        } else if let Some(t) = self.shot_taken {
                            if t.elapsed().as_secs_f64() > 0.5 {
                                g.shutdown();
                                self.game = None;
                                el.exit();
                                return;
                            }
                        }
                    }
                    g.frame((size.width, size.height));
                    // Minecraft.displayGuiScreen: a screen the server opened
                    // (a container window, the death screen) takes the mouse
                    // out of the game (setIngameNotInFocus) and closing it
                    // gives it back (setIngameFocus). Screens opened by a key
                    // or a click ask for this themselves.
                    let want = g.wants_grab();
                    {
                        // MINERUST_DEBUG_GRAB: start captured, as a real
                        // session does, so captures can check the release
                        static ONCE: std::sync::Once = std::sync::Once::new();
                        let mut first = false;
                        ONCE.call_once(|| first = true);
                        if first && want && std::env::var_os("MINERUST_DEBUG_GRAB").is_some() {
                            self.set_grab(true);
                        }
                    }
                    if want != self.wanted_grab {
                        self.wanted_grab = want;
                        if self.focused && want != self.grabbed {
                            self.set_grab(want);
                        }
                    }
                    let Some(g) = &mut self.game else { return };
                    // isFramerateLimitBelowMax: 260 on the slider is "Unlimited"
                    let limit = g.limit_framerate();
                    if self.bench.is_none() && limit < 260 {
                        self.sync.sync(limit);
                    }
                }
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _el: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta } = event {
            if self.grabbed && self.focused {
                if let Some(g) = &mut self.game {
                    // EntityRenderer.updateCameraAndRender: mouseSensitivity
                    // * 0.6 + 0.2, cubed, times 8; invertMouse flips y
                    let (sens, inv) = g.shell.as_ref().map_or((0.5, false), |s| {
                        (s.settings.mouse_sensitivity, s.settings.invert_mouse)
                    });
                    let f = sens * 0.6 + 0.2;
                    let k = f * f * f * 8.0;
                    let dy = -(delta.1 as f32) * k * if inv { -1.0 } else { 1.0 };
                    g.player.turn(delta.0 as f32 * k, -dy);
                }
            }
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 2 && args[1].starts_with("--dump") {
        // Comparison dumps must reproduce 1.7.10 exactly.
        compat::set_vanilla_parity(true);
    }
    if args.len() >= 5 && args[1] == "--dump" {
        dump(&args[2..]);
        return;
    }
    if args.len() >= 4 && args[1] == "--locate" {
        locate(&args[2..]);
        return;
    }
    if args.len() >= 3 && args[1] == "--find-biomes" {
        find_biomes(&args[2..]);
        return;
    }
    if args.len() >= 4 && args[1] == "--export-anvil" {
        // --export-anvil <world dir> <new vanilla world dir>
        let cfg = sim::SimConfig {
            seed: 0,
            view: 2,
            save_dir: Some(std::path::PathBuf::from(&args[2])),
            start_time: 0,
            freeze_time: true,
            mode: entity::survival::GameMode::Survival,
        };
        let (mut s, _) = sim::Sim::new(&cfg);
        match s.export_anvil(std::path::Path::new(&args[3])) {
            Ok(n) => println!("exported {n} chunks to {}", args[3]),
            Err(e) => eprintln!("export failed: {e}"),
        }
        return;
    }
    if args.len() >= 7 && args[1] == "--write-region" {
        write_region(&args[2..]);
        return;
    }
    if args.len() >= 8 && args[1] == "--dump-region" {
        dump_region(&args[2..]);
        return;
    }
    if args.len() >= 7 && args[1] == "--dump-pop" {
        dump_pop(&args[2..]);
        return;
    }
    if args.len() >= 7 && args[1] == "--compare-pop" {
        compare_pop(&args[2..]);
        return;
    }
    if args.len() >= 4 && args[1] == "--compare" {
        compare(&args[2..]);
        return;
    }
    let opts = parse_args();
    let el = EventLoop::new().expect("event loop");
    el.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        opts,
        game: None,
        front: None,
        atlas: None,
        window: None,
        grabbed: false,
        wanted_grab: true,
        focused: true,
        started: Instant::now(),
        sync: FrameSync { next: None },
        bench: std::env::var("MINERUST_BENCH")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .map(|secs| Bench {
                secs,
                began: None,
                last: Instant::now(),
                frames: Vec::new(),
                prof0: [0.0; 4],
                ticks0: (0, 0),
            }),
        shift: false,
        ctrl: false,
        shot_taken: None,
        error: None,
    };
    let _ = el.run_app(&mut app);
    // The renderer must go before the window it presents to.
    app.game = None;
    app.front = None;
    app.window = None;
    if std::env::var_os("MINERUST_VALIDATE").is_some() {
        println!(
            "Vulkan validation messages: {}",
            render::vk::VALIDATION_MESSAGES.load(std::sync::atomic::Ordering::Relaxed)
        );
    }
    if let Some(e) = app.error {
        eprintln!("錯誤: {e}");
        error_dialog(&e);
        std::process::exit(1);
    }
}

/// A started-by-double-click run has its console closed before anyone can
/// read the error: show it in a dialog too.
#[cfg(windows)]
fn error_dialog(e: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    let w = |s: &str| {
        s.encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>()
    };
    let (text, title) = (w(e), w("MINERUST"));
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(not(windows))]
fn error_dialog(_e: &str) {}
