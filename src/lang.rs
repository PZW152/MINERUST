//! `StringTranslate` / `LanguageMap`: `assets/minecraft/lang/en_US.lang`
//! from the game jar, and `String.format` of `%s` / `%1$s` arguments.

use std::collections::HashMap;
use std::io::Read;

static SHARED: std::sync::RwLock<Option<std::sync::Arc<Lang>>> = std::sync::RwLock::new(None);

/// `StringTranslate.getInstance()`: the language last loaded, shared by
/// the client and the integrated server (empty before any is loaded).
pub fn shared() -> std::sync::Arc<Lang> {
    SHARED.read().unwrap().clone().unwrap_or_default()
}

#[derive(Default, Clone)]
pub struct Lang {
    map: HashMap<String, String>,
    unicode: bool,
    pub code: String,
}

/// `Locale.loadLocaleData`: `key=value` lines, `%d`/`%f` patterns as `%s`.
fn read_lang(text: &str, map: &mut HashMap<String, String>) {
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        map.insert(k.to_string(), format_pattern(v));
    }
}

/// `Locale.pattern` (`%(\d+\$)?[\d\.]*[df]`) replaced by `%$1s`.
fn format_pattern(v: &str) -> String {
    let b = v.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let mut j = i + 1;
            let ds = j;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            let mut arg = "";
            let mut k = j;
            if k < b.len() && b[k] == b'$' && k > ds {
                arg = &v[ds..k + 1];
                k += 1;
            } else {
                k = ds;
            }
            let mut m = k;
            while m < b.len() && (b[m].is_ascii_digit() || b[m] == b'.') {
                m += 1;
            }
            if m < b.len() && (b[m] == b'd' || b[m] == b'f') {
                out.push('%');
                out.push_str(arg);
                out.push('s');
                i = m + 1;
                continue;
            }
        }
        let ch = v[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// A `Language` of the pack metadata: code, name, region.
#[derive(Clone, Debug)]
pub struct Language {
    pub code: String,
    pub name: String,
    pub region: String,
}

impl Language {
    /// `Language.toString`
    pub fn label(&self) -> String {
        format!("{} ({})", self.name, self.region)
    }
}

/// `LanguageManager.parseLanguageMetadata`: en_US, then the asset pack's list.
pub fn languages(assets: Option<&crate::assets::Assets>) -> Vec<Language> {
    let mut v = vec![Language {
        code: "en_US".into(),
        name: "English".into(),
        region: "US".into(),
    }];
    let meta = assets
        .and_then(|a| a.read("pack.mcmeta"))
        .and_then(|b| crate::assets::Json::parse(&b));
    if let Some(crate::assets::Json::Obj(list)) = meta.as_ref().and_then(|m| m.get("language")) {
        for (code, l) in list {
            let name = l
                .get("name")
                .and_then(|x| x.str())
                .unwrap_or(code)
                .to_string();
            let region = l
                .get("region")
                .and_then(|x| x.str())
                .unwrap_or("")
                .to_string();
            if let Some(e) = v.iter_mut().find(|x| x.code == *code) {
                e.name = name;
                e.region = region;
            } else {
                v.push(Language {
                    code: code.clone(),
                    name,
                    region,
                });
            }
        }
    }
    // LanguageManager.getLanguages: a TreeSet by code
    v.sort_by(|a, b| a.code.encode_utf16().cmp(b.code.encode_utf16()));
    v
}

impl Lang {
    pub fn load(jar: &mut crate::render::atlas::Jar) -> Lang {
        Self::load_language(jar, None, "en_US")
    }

    /// `Locale.loadLocaleDataFiles(["en_US", code])`: English first, the
    /// chosen language over it; then `checkUnicode`.
    pub fn load_language(
        jar: &mut crate::render::atlas::Jar,
        assets: Option<&crate::assets::Assets>,
        code: &str,
    ) -> Lang {
        let mut map = HashMap::new();
        let mut codes = vec!["en_US"];
        if code != "en_US" {
            codes.push(code);
        }
        for c in codes {
            let mut text = String::new();
            let found = match jar.by_name(&format!("assets/minecraft/lang/{c}.lang")) {
                Ok(mut f) => f.read_to_string(&mut text).is_ok(),
                Err(_) => false,
            };
            if !found {
                if let Some(b) = assets.and_then(|a| a.read(&format!("minecraft/lang/{c}.lang"))) {
                    text = String::from_utf8_lossy(&b).into_owned();
                }
            }
            read_lang(&text, &mut map);
        }
        // Locale.checkUnicode
        let (mut wide, mut all) = (0usize, 0usize);
        for v in map.values() {
            all += v.chars().count();
            wide += v.chars().filter(|&c| c as u32 >= 256).count();
        }
        let unicode = all > 0 && (wide as f32 / all as f32) > 0.1;
        let l = Lang {
            map,
            unicode,
            code: code.to_string(),
        };
        // StringTranslate.replaceWith: the singleton the integrated server
        // reads too
        *SHARED.write().unwrap() = Some(std::sync::Arc::new(l.clone()));
        l
    }

    /// `LanguageManager.isCurrentLocaleUnicode`
    pub fn is_unicode(&self) -> bool {
        self.unicode
    }

    /// `StatCollector.translateToLocal`: the key itself when missing.
    pub fn get<'a>(&'a self, key: &'a str) -> &'a str {
        self.map.get(key).map_or(key, |v| v.as_str())
    }

    /// `ChatComponentTranslation`: arguments that are keys are translated.
    pub fn format(&self, key: &str, args: &[String]) -> String {
        let args: Vec<String> = args.iter().map(|a| self.get(a).to_string()).collect();
        self.format_raw(key, &args)
    }

    /// `ChatComponentTranslation` with its arguments as given.
    pub fn format_raw(&self, key: &str, args: &[String]) -> String {
        let pattern = self.get(key);
        let args: Vec<&str> = args.iter().map(|a| a.as_str()).collect();
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
