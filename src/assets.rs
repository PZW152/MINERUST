//! The launcher's asset store (`assets/indexes/1.7.10.json` and
//! `assets/objects/`), as `DefaultResourcePack` reads it, and a JSON reader
//! for the index and the pack metadata.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub enum Json {
    Obj(Vec<(String, Json)>),
    Arr(Vec<Json>),
    Str(String),
    Num(f64),
    Bool(bool),
    Null,
}

impl Json {
    pub fn get(&self, k: &str) -> Option<&Json> {
        match self {
            Json::Obj(v) => v.iter().find(|e| e.0 == k).map(|e| &e.1),
            _ => None,
        }
    }
    pub fn str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn parse(s: &[u8]) -> Option<Json> {
        parse(s, &mut 0)
    }
}

fn parse(s: &[u8], i: &mut usize) -> Option<Json> {
    let ws = |i: &mut usize| {
        while *i < s.len() && (s[*i] as char).is_whitespace() {
            *i += 1;
        }
    };
    ws(i);
    match *s.get(*i)? {
        b'{' => {
            *i += 1;
            let mut v = Vec::new();
            loop {
                ws(i);
                if *s.get(*i)? == b'}' {
                    *i += 1;
                    break;
                }
                let Json::Str(k) = parse(s, i)? else {
                    return None;
                };
                ws(i);
                *i += 1;
                let val = parse(s, i)?;
                v.push((k, val));
                ws(i);
                if *s.get(*i)? == b',' {
                    *i += 1;
                }
            }
            Some(Json::Obj(v))
        }
        b'[' => {
            *i += 1;
            let mut v = Vec::new();
            loop {
                ws(i);
                if *s.get(*i)? == b']' {
                    *i += 1;
                    break;
                }
                v.push(parse(s, i)?);
                ws(i);
                if *s.get(*i)? == b',' {
                    *i += 1;
                }
            }
            Some(Json::Arr(v))
        }
        b'"' => {
            *i += 1;
            let mut out: Vec<u8> = Vec::new();
            let mut pending_high: Option<u32> = None;
            while *s.get(*i)? != b'"' {
                if s[*i] == b'\\' {
                    *i += 1;
                    let c = *s.get(*i)?;
                    match c {
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'u' => {
                            let h = std::str::from_utf8(s.get(*i + 1..*i + 5)?).ok()?;
                            let mut cp = u32::from_str_radix(h, 16).ok()?;
                            *i += 4;
                            if (0xD800..0xDC00).contains(&cp) {
                                pending_high = Some(cp);
                                *i += 1;
                                continue;
                            }
                            if let (Some(hi), true) =
                                (pending_high.take(), (0xDC00..0xE000).contains(&cp))
                            {
                                cp = 0x10000 + ((hi - 0xD800) << 10) + (cp - 0xDC00);
                            }
                            let ch = char::from_u32(cp).unwrap_or('?');
                            let mut buf = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        other => out.push(other),
                    }
                    *i += 1;
                    continue;
                }
                out.push(s[*i]);
                *i += 1;
            }
            *i += 1;
            Some(Json::Str(String::from_utf8_lossy(&out).into_owned()))
        }
        b't' => {
            *i += 4;
            Some(Json::Bool(true))
        }
        b'f' => {
            *i += 5;
            Some(Json::Bool(false))
        }
        b'n' => {
            *i += 4;
            Some(Json::Null)
        }
        _ => {
            let st = *i;
            while *i < s.len() && matches!(s[*i], b'-' | b'.' | b'e' | b'E' | b'+' | b'0'..=b'9') {
                *i += 1;
            }
            std::str::from_utf8(&s[st..*i])
                .ok()?
                .parse()
                .ok()
                .map(Json::Num)
        }
    }
}

/// `DefaultResourcePack`'s asset map: resource path to object file.
#[derive(Clone, Default)]
pub struct Assets {
    index: HashMap<String, String>,
    objects: PathBuf,
}

impl Assets {
    /// `assets` holds `indexes/1.7.10.json` and `objects/`.
    pub fn open(assets: &Path) -> Option<Assets> {
        let idx = std::fs::read(assets.join("indexes").join("1.7.10.json")).ok()?;
        let j = Json::parse(&idx)?;
        let mut index = HashMap::new();
        if let Some(Json::Obj(objs)) = j.get("objects") {
            for (k, v) in objs {
                if let Some(h) = v.get("hash").and_then(Json::str) {
                    index.insert(k.clone(), h.to_string());
                }
            }
        }
        Some(Assets {
            index,
            objects: assets.join("objects"),
        })
    }

    /// The bytes of an indexed resource, e.g. `minecraft/lang/zh_TW.lang`.
    pub fn read(&self, name: &str) -> Option<Vec<u8>> {
        let h = self.index.get(name)?;
        std::fs::read(self.objects.join(&h[..2]).join(h)).ok()
    }
}
