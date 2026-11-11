//! World persistence.
//!
//! Every file is written to a temporary name, flushed to disk and then renamed
//! over the target, so a crash at any point leaves either the previous complete
//! file or the new complete file, never a torn one. Each file carries a magic
//! number, a format version and a CRC-32 of its payload; anything that fails
//! those checks is treated as absent instead of being trusted.
//!
//! The on-disk format is this project's own (not Anvil).

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const CHUNK_MAGIC: &[u8; 4] = b"MRC1";
const LEVEL_MAGIC: &str = "MINERUST-LEVEL";
pub const FORMAT_VERSION: u32 = 1;

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *t = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

/// Writes `data` to `path` atomically (temp file, fsync, rename).
pub fn write_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

pub fn chunk_path(dir: &Path, cx: i32, cz: i32) -> PathBuf {
    // Group by 32x32 region so directories stay small.
    dir.join("chunks").join(format!("r.{}.{}", cx >> 5, cz >> 5)).join(format!("c.{cx}.{cz}.mrc"))
}

pub fn write_chunk(dir: &Path, cx: i32, cz: i32, payload: &[u8]) -> io::Result<()> {
    let mut out = Vec::with_capacity(payload.len() + 16);
    out.extend_from_slice(CHUNK_MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&crc32(payload).to_le_bytes());
    out.extend_from_slice(payload);
    write_atomic(&chunk_path(dir, cx, cz), &out)
}

/// Returns the payload, or `None` if the file is missing, truncated, from an
/// unknown version or fails its checksum.
pub fn read_chunk(dir: &Path, cx: i32, cz: i32) -> Option<Vec<u8>> {
    let data = fs::read(chunk_path(dir, cx, cz)).ok()?;
    decode_chunk_file(&data)
}

pub fn decode_chunk_file(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 16 || &data[0..4] != CHUNK_MAGIC {
        return None;
    }
    let version = u32::from_le_bytes(data[4..8].try_into().ok()?);
    if version != FORMAT_VERSION {
        return None;
    }
    let len = u32::from_le_bytes(data[8..12].try_into().ok()?) as usize;
    let crc = u32::from_le_bytes(data[12..16].try_into().ok()?);
    let payload = data.get(16..16usize.checked_add(len)?)?;
    if payload.len() != len || crc32(payload) != crc {
        return None;
    }
    Some(payload.to_vec())
}

/// `level` file: world-wide state.
#[derive(Clone, Debug, PartialEq)]
pub struct Level {
    pub seed: i64,
    pub total_time: i64,
    pub world_time: i64,
    pub vanilla_parity: bool,
    pub player: Option<([f64; 3], [f32; 2], bool)>,
    /// Scheduled block updates: `(x, y, z, block, delay)`.
    pub ticks: Vec<(i32, i32, i32, u8, i64)>,
    /// Keys this version does not know; written back unchanged.
    pub unknown: Vec<(String, String)>,
}

pub fn level_path(dir: &Path) -> PathBuf {
    dir.join("level.txt")
}

pub fn write_level(dir: &Path, l: &Level) -> io::Result<()> {
    let mut body = String::new();
    body.push_str(&format!("seed={}\n", l.seed));
    body.push_str(&format!("total_time={}\n", l.total_time));
    body.push_str(&format!("world_time={}\n", l.world_time));
    body.push_str(&format!("vanilla_parity={}\n", l.vanilla_parity));
    if let Some((p, r, flying)) = &l.player {
        body.push_str(&format!("player={:?},{:?},{:?},{:?},{:?},{}\n", p[0], p[1], p[2], r[0], r[1], flying));
    }
    for t in &l.ticks {
        body.push_str(&format!("tick={},{},{},{},{}\n", t.0, t.1, t.2, t.3, t.4));
    }
    for (k, v) in &l.unknown {
        body.push_str(&format!("{k}={v}\n"));
    }
    let text = format!("{LEVEL_MAGIC}\nversion={FORMAT_VERSION}\ncrc={:08x}\n{body}", crc32(body.as_bytes()));
    write_atomic(&level_path(dir), text.as_bytes())
}

pub fn read_level(dir: &Path) -> Option<Level> {
    let text = fs::read_to_string(level_path(dir)).ok()?;
    parse_level(&text)
}

pub fn parse_level(text: &str) -> Option<Level> {
    let mut lines = text.splitn(4, '\n');
    if lines.next()? != LEVEL_MAGIC {
        return None;
    }
    let version: u32 = lines.next()?.strip_prefix("version=")?.parse().ok()?;
    if version != FORMAT_VERSION {
        return None;
    }
    let crc = u32::from_str_radix(lines.next()?.strip_prefix("crc=")?, 16).ok()?;
    let body = lines.next().unwrap_or("");
    if crc32(body.as_bytes()) != crc {
        return None;
    }
    let mut l = Level { seed: 0, total_time: 0, world_time: 0, vanilla_parity: false, player: None, ticks: Vec::new(), unknown: Vec::new() };
    let mut have_seed = false;
    for line in body.lines() {
        let (k, v) = line.split_once('=')?;
        match k {
            "seed" => {
                l.seed = v.parse().ok()?;
                have_seed = true;
            }
            "total_time" => l.total_time = v.parse().ok()?,
            "world_time" => l.world_time = v.parse().ok()?,
            "vanilla_parity" => l.vanilla_parity = v.parse().ok()?,
            "player" => {
                let f: Vec<&str> = v.split(',').collect();
                if f.len() != 6 {
                    return None;
                }
                let p = [f[0].parse().ok()?, f[1].parse().ok()?, f[2].parse().ok()?];
                let r = [f[3].parse().ok()?, f[4].parse().ok()?];
                l.player = Some((p, r, f[5].parse().ok()?));
            }
            "tick" => {
                let f: Vec<&str> = v.split(',').collect();
                if f.len() != 5 {
                    return None;
                }
                l.ticks.push((f[0].parse().ok()?, f[1].parse().ok()?, f[2].parse().ok()?, f[3].parse().ok()?, f[4].parse().ok()?));
            }
            _ => l.unknown.push((k.to_string(), v.to_string())),
        }
    }
    if !have_seed {
        return None;
    }
    Some(l)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("minerust-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn chunk_round_trip_and_corruption_is_rejected() {
        let dir = temp_dir("chunk");
        let payload: Vec<u8> = (0..5000u32).map(|i| (i * 7 % 251) as u8).collect();
        write_chunk(&dir, -3, 17, &payload).unwrap();
        assert_eq!(read_chunk(&dir, -3, 17).as_deref(), Some(&payload[..]));
        assert!(read_chunk(&dir, 0, 0).is_none());

        let path = chunk_path(&dir, -3, 17);
        let good = fs::read(&path).unwrap();
        // Truncation, bit flips and garbage must never be accepted or panic.
        for cut in [0usize, 3, 15, 16, good.len() / 2, good.len() - 1] {
            assert!(decode_chunk_file(&good[..cut]).is_none(), "truncated at {cut}");
        }
        for i in (0..good.len()).step_by(97) {
            let mut bad = good.clone();
            bad[i] ^= 0x40;
            assert!(decode_chunk_file(&bad).is_none(), "bit flip at {i}");
        }
        let mut huge = good.clone();
        huge[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode_chunk_file(&huge).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn interrupted_write_keeps_previous_file() {
        let dir = temp_dir("atomic");
        write_chunk(&dir, 1, 2, b"first version").unwrap();
        // A crash mid-save leaves only the temporary file behind.
        let path = chunk_path(&dir, 1, 2);
        fs::write(path.with_extension("tmp"), b"MRC1 half written").unwrap();
        assert_eq!(read_chunk(&dir, 1, 2).as_deref(), Some(&b"first version"[..]));
        // The next save replaces the stale temporary file.
        write_chunk(&dir, 1, 2, b"second version").unwrap();
        assert_eq!(read_chunk(&dir, 1, 2).as_deref(), Some(&b"second version"[..]));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn level_round_trip_keeps_unknown_keys() {
        let dir = temp_dir("level");
        let l = Level {
            seed: -4172144997902289642,
            total_time: 123456,
            world_time: 7890,
            vanilla_parity: true,
            player: Some(([1.5, 64.0, -2.25], [90.0, -12.5], true)),
            ticks: vec![(1, 2, 3, 8, 5), (-9, 60, 44, 12, 2)],
            unknown: vec![("future_key".into(), "some value".into())],
        };
        write_level(&dir, &l).unwrap();
        assert_eq!(read_level(&dir), Some(l));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn malformed_level_is_rejected_not_panicking() {
        for text in ["", "garbage", "MINERUST-LEVEL\nversion=1\ncrc=00000000\nseed=1\n", "MINERUST-LEVEL\nversion=99\ncrc=0\n", "MINERUST-LEVEL\nversion=x\n"] {
            assert!(parse_level(text).is_none(), "{text:?}");
        }
    }
}
