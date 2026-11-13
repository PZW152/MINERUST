//! `SoundManager` / `SoundHandler`: resolves 1.7.10 sound events through
//! `sounds.json` and the asset index, decodes the Ogg Vorbis files and mixes
//! them with distance attenuation and stereo panning. Pure Rust: `cpal` for
//! the output device, `lewton` for Vorbis.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct Clip {
    rate: u32,
    channels: usize,
    samples: Arc<Vec<f32>>,
}

struct Voice {
    clip: Clip,
    pos: f64,
    step: f64,
    gain: [f32; 2],
    stream: bool,
}

#[derive(Default)]
struct Mixer {
    voices: Vec<Voice>,
    master: f32,
}

/// One sound file choice of an event.
#[derive(Clone)]
struct Entry {
    name: String,
    stream: bool,
}

pub struct Audio {
    objects: PathBuf,
    index: HashMap<String, String>,
    events: HashMap<String, Vec<Entry>>,
    cache: HashMap<String, Option<Clip>>,
    mixer: Arc<Mutex<Mixer>>,
    out_rate: u32,
    _stream: Option<cpal::Stream>,
    rand: crate::java::JavaRandom,
    listener: [f64; 3],
    yaw: f32,
    music_timer: i32,
}

/// Tiny JSON reader for the asset index and `sounds.json`.
#[derive(Debug, Clone)]
enum J {
    Obj(Vec<(String, J)>),
    Arr(Vec<J>),
    Str(String),
    Num(f64),
    Bool(bool),
    Null,
}

fn parse(s: &[u8], i: &mut usize) -> Option<J> {
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
                if s[*i] == b'}' {
                    *i += 1;
                    break;
                }
                let J::Str(k) = parse(s, i)? else { return None };
                ws(i);
                *i += 1; // ':'
                let val = parse(s, i)?;
                v.push((k, val));
                ws(i);
                if s[*i] == b',' {
                    *i += 1;
                }
            }
            Some(J::Obj(v))
        }
        b'[' => {
            *i += 1;
            let mut v = Vec::new();
            loop {
                ws(i);
                if s[*i] == b']' {
                    *i += 1;
                    break;
                }
                v.push(parse(s, i)?);
                ws(i);
                if s[*i] == b',' {
                    *i += 1;
                }
            }
            Some(J::Arr(v))
        }
        b'"' => {
            *i += 1;
            let mut out = Vec::new();
            while s[*i] != b'"' {
                if s[*i] == b'\\' {
                    *i += 1;
                }
                out.push(s[*i]);
                *i += 1;
            }
            *i += 1;
            Some(J::Str(String::from_utf8_lossy(&out).into_owned()))
        }
        b't' => {
            *i += 4;
            Some(J::Bool(true))
        }
        b'f' => {
            *i += 5;
            Some(J::Bool(false))
        }
        b'n' => {
            *i += 4;
            Some(J::Null)
        }
        _ => {
            let st = *i;
            while *i < s.len()
                && (s[*i] == b'-'
                    || s[*i] == b'.'
                    || s[*i] == b'e'
                    || s[*i] == b'E'
                    || s[*i] == b'+'
                    || s[*i].is_ascii_digit())
            {
                *i += 1;
            }
            std::str::from_utf8(&s[st..*i])
                .ok()?
                .parse()
                .ok()
                .map(J::Num)
        }
    }
}

fn get<'a>(o: &'a J, k: &str) -> Option<&'a J> {
    match o {
        J::Obj(v) => v.iter().find(|e| e.0 == k).map(|e| &e.1),
        _ => None,
    }
}

impl Audio {
    /// `assets` is the launcher's asset directory (`indexes/`, `objects/`).
    pub fn new(assets: &Path) -> Option<Audio> {
        let idx = std::fs::read(assets.join("indexes").join("1.7.10.json")).ok()?;
        let j = parse(&idx, &mut 0)?;
        let mut index = HashMap::new();
        if let Some(J::Obj(objs)) = get(&j, "objects") {
            for (k, v) in objs {
                if let Some(J::Str(h)) = get(v, "hash") {
                    index.insert(k.clone(), h.clone());
                }
            }
        }
        let objects = assets.join("objects");
        let sj = {
            let h = index.get("minecraft/sounds.json")?;
            std::fs::read(objects.join(&h[..2]).join(h)).ok()?
        };
        let sj = parse(&sj, &mut 0)?;
        let mut events = HashMap::new();
        if let J::Obj(v) = &sj {
            for (name, e) in v {
                let mut list = Vec::new();
                if let Some(J::Arr(a)) = get(e, "sounds") {
                    for s in a {
                        match s {
                            J::Str(n) => list.push(Entry {
                                name: n.clone(),
                                stream: false,
                            }),
                            o => {
                                if let Some(J::Str(n)) = get(o, "name") {
                                    let stream = matches!(get(o, "stream"), Some(J::Bool(true)));
                                    list.push(Entry {
                                        name: n.clone(),
                                        stream,
                                    });
                                }
                            }
                        }
                    }
                }
                events.insert(name.clone(), list);
            }
        }
        let mixer = Arc::new(Mutex::new(Mixer {
            voices: Vec::new(),
            master: 1.0,
        }));
        let (stream, out_rate) = match open_output(mixer.clone()) {
            Some(x) => (Some(x.0), x.1),
            None => (None, 44100),
        };
        Some(Audio {
            objects,
            index,
            events,
            cache: HashMap::new(),
            mixer,
            out_rate,
            _stream: stream,
            rand: crate::java::JavaRandom::new(0x534f554e44),
            listener: [0.0; 3],
            yaw: 0.0,
            music_timer: 12000,
        })
    }

    pub fn set_listener(&mut self, pos: [f64; 3], yaw: f32) {
        self.listener = pos;
        self.yaw = yaw;
    }

    fn clip(&mut self, name: &str) -> Option<Clip> {
        if let Some(c) = self.cache.get(name) {
            return c.clone();
        }
        let key = format!("minecraft/sounds/{name}.ogg");
        let clip = self.index.get(&key).and_then(|h| {
            let f = std::fs::File::open(self.objects.join(&h[..2]).join(h)).ok()?;
            let mut r =
                lewton::inside_ogg::OggStreamReader::new(std::io::BufReader::new(f)).ok()?;
            let channels = r.ident_hdr.audio_channels as usize;
            let rate = r.ident_hdr.audio_sample_rate;
            let mut samples = Vec::new();
            while let Ok(Some(p)) = r.read_dec_packet_itl() {
                samples.extend(p.iter().map(|&s| s as f32 / 32768.0));
            }
            Some(Clip {
                rate,
                channels,
                samples: Arc::new(samples),
            })
        });
        self.cache.insert(name.to_string(), clip.clone());
        clip
    }

    /// `SoundManager.playSound` for a positioned event (`null` = no position).
    pub fn play(&mut self, event: &str, pos: Option<[f64; 3]>, volume: f32, pitch: f32) {
        let Some(list) = self.events.get(event) else {
            return;
        };
        if list.is_empty() {
            return;
        }
        let e = list[self.rand.next_int_n(list.len() as i32) as usize].clone();
        let Some(clip) = self.clip(&e.name) else {
            return;
        };
        // SoundManager: linear attenuation over 16 blocks scaled by volume.
        let (mut l, mut r) = (1.0f32, 1.0f32);
        let mut gain = volume.clamp(0.0, 1.0);
        if let Some(p) = pos {
            let d = [
                p[0] - self.listener[0],
                p[1] - self.listener[1],
                p[2] - self.listener[2],
            ];
            let dist = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() as f32;
            let range = 16.0 * volume.max(1.0);
            if dist > range {
                return;
            }
            gain = volume.min(1.0) * (1.0 - dist / range);
            // pan from the listener's right vector
            let yr = self.yaw.to_radians();
            let right = [-(yr.cos()) as f64, 0.0, -(yr.sin()) as f64];
            let pan = if dist > 0.01 {
                ((d[0] * right[0] + d[2] * right[2]) / dist as f64) as f32
            } else {
                0.0
            };
            l = (1.0 - pan.max(0.0) * 0.7).max(0.0);
            r = (1.0 + pan.min(0.0) * 0.7).max(0.0);
        }
        let step = clip.rate as f64 / self.out_rate as f64 * pitch.clamp(0.5, 2.0) as f64;
        if let Ok(mut m) = self.mixer.lock() {
            if m.voices.len() < 64 {
                m.voices.push(Voice {
                    clip,
                    pos: 0.0,
                    step,
                    gain: [gain * l, gain * r],
                    stream: e.stream,
                });
            }
        }
    }

    /// `MusicTicker`: a game track every 12000..24000 ticks.
    pub fn tick_music(&mut self) {
        let playing = self
            .mixer
            .lock()
            .map_or(true, |m| m.voices.iter().any(|v| v.stream));
        if playing {
            return;
        }
        self.music_timer -= 1;
        if self.music_timer <= 0 {
            self.play("music.game", None, 1.0, 1.0);
            self.music_timer = 12000 + self.rand.next_int_n(12000);
        }
    }
}

fn open_output(mixer: Arc<Mutex<Mixer>>) -> Option<(cpal::Stream, u32)> {
    let host = cpal::default_host();
    let dev = host.default_output_device()?;
    let cfg = dev.default_output_config().ok()?;
    let rate = cfg.sample_rate().0;
    let ch = cfg.channels() as usize;
    let config: cpal::StreamConfig = cfg.clone().into();
    let stream = match cfg.sample_format() {
        cpal::SampleFormat::F32 => dev.build_output_stream(
            &config,
            move |out: &mut [f32], _| fill(&mixer, out, ch),
            |e| eprintln!("audio: {e}"),
            None,
        ),
        cpal::SampleFormat::I16 => dev.build_output_stream(
            &config,
            move |out: &mut [i16], _| {
                let mut buf = vec![0f32; out.len()];
                fill(&mixer, &mut buf, ch);
                for (o, s) in out.iter_mut().zip(buf) {
                    *o = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
                }
            },
            |e| eprintln!("audio: {e}"),
            None,
        ),
        _ => return None,
    }
    .ok()?;
    stream.play().ok()?;
    Some((stream, rate))
}

fn fill(mixer: &Mutex<Mixer>, out: &mut [f32], ch: usize) {
    for s in out.iter_mut() {
        *s = 0.0;
    }
    let Ok(mut m) = mixer.lock() else { return };
    let master = m.master;
    let frames = out.len() / ch.max(1);
    m.voices.retain_mut(|v| {
        let n = v.clip.samples.len() / v.clip.channels.max(1);
        for f in 0..frames {
            let i = v.pos as usize;
            if i >= n {
                return false;
            }
            let (a, b) = if v.clip.channels >= 2 {
                (
                    v.clip.samples[i * v.clip.channels],
                    v.clip.samples[i * v.clip.channels + 1],
                )
            } else {
                (v.clip.samples[i], v.clip.samples[i])
            };
            if ch >= 2 {
                out[f * ch] += a * v.gain[0] * master;
                out[f * ch + 1] += b * v.gain[1] * master;
            } else {
                out[f * ch] += (a + b) * 0.5 * v.gain[0] * master;
            }
            v.pos += v.step;
        }
        true
    });
    // MC-1538: many overlapping sounds used to clip and crackle. Compress
    // only the part above 0.8 so normal levels are untouched.
    for s in out.iter_mut() {
        let a = s.abs();
        if a > 0.8 {
            *s = s.signum() * (0.8 + 0.2 * ((a - 0.8) / 0.2).tanh());
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn decodes_vanilla_sounds_when_assets_exist() {
        let Some(root) = std::env::var_os("APPDATA")
            .map(|a| std::path::PathBuf::from(a).join("Sapling").join("assets"))
            .filter(|p| p.exists())
        else {
            return;
        };
        let root = root.as_path();
        let mut a = super::Audio::new(root).expect("asset index");
        assert!(a.events.contains_key("mob.zombie.say"));
        let c = a.clip("mob/zombie/say1").expect("decoded");
        assert!(c.samples.len() > 1000 && c.rate > 0);
    }
}
