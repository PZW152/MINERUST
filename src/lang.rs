//! `StringTranslate` / `LanguageMap`: `assets/minecraft/lang/en_US.lang`
//! from the game jar, and `String.format` of `%s` / `%1$s` arguments.

use std::collections::HashMap;
use std::io::Read;

#[derive(Default, Clone)]
pub struct Lang {
    map: HashMap<String, String>,
}

impl Lang {
    pub fn load(jar: &mut crate::render::atlas::Jar) -> Lang {
        let mut map = HashMap::new();
        if let Ok(mut f) = jar.by_name("assets/minecraft/lang/en_US.lang") {
            let mut s = String::new();
            if f.read_to_string(&mut s).is_ok() {
                for line in s.lines() {
                    if line.starts_with('#') {
                        continue;
                    }
                    if let Some((k, v)) = line.split_once('=') {
                        map.insert(k.to_string(), v.to_string());
                    }
                }
            }
        }
        Lang { map }
    }

    /// `StatCollector.translateToLocal`: the key itself when missing.
    pub fn get<'a>(&'a self, key: &'a str) -> &'a str {
        self.map.get(key).map_or(key, |v| v.as_str())
    }

    /// `ChatComponentTranslation`: arguments that are keys are translated.
    pub fn format(&self, key: &str, args: &[String]) -> String {
        let pattern = self.get(key);
        let args: Vec<&str> = args.iter().map(|a| self.get(a)).collect();
        let mut out = String::new();
        let mut next = 0usize;
        let b = pattern.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'%' && i + 1 < b.len() {
                if b[i + 1] == b's' {
                    out.push_str(args.get(next).copied().unwrap_or(""));
                    next += 1;
                    i += 2;
                    continue;
                }
                if b[i + 1] == b'%' {
                    out.push('%');
                    i += 2;
                    continue;
                }
                // %1$s
                let mut j = i + 1;
                while j < b.len() && b[j].is_ascii_digit() {
                    j += 1;
                }
                if j + 1 < b.len() && j > i + 1 && b[j] == b'$' && b[j + 1] == b's' {
                    let n: usize = pattern[i + 1..j].parse().unwrap_or(1);
                    out.push_str(args.get(n - 1).copied().unwrap_or(""));
                    i = j + 2;
                    continue;
                }
            }
            let ch = pattern[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
        out
    }
}
