//! `net.minecraft.potion`: `Potion`, `PotionEffect` and `PotionHelper`
//! (the brewing bit formulas), ported line by line.

/// `Potion.potionTypes` ids.
pub const MOVE_SPEED: i32 = 1;
pub const MOVE_SLOWDOWN: i32 = 2;
pub const DIG_SPEED: i32 = 3;
pub const DIG_SLOWDOWN: i32 = 4;
pub const DAMAGE_BOOST: i32 = 5;
pub const HEAL: i32 = 6;
pub const HARM: i32 = 7;
pub const JUMP: i32 = 8;
pub const CONFUSION: i32 = 9;
pub const REGENERATION: i32 = 10;
pub const RESISTANCE: i32 = 11;
pub const FIRE_RESISTANCE: i32 = 12;
pub const WATER_BREATHING: i32 = 13;
pub const INVISIBILITY: i32 = 14;
pub const BLINDNESS: i32 = 15;
pub const NIGHT_VISION: i32 = 16;
pub const HUNGER: i32 = 17;
pub const WEAKNESS: i32 = 18;
pub const POISON: i32 = 19;
pub const WITHER: i32 = 20;
pub const HEALTH_BOOST: i32 = 21;
pub const ABSORPTION: i32 = 22;
pub const SATURATION: i32 = 23;

/// Which `IAttribute` a potion modifies.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Attr {
    MaxHealth,
    MovementSpeed,
    AttackDamage,
}

/// One registered `Potion`.
#[derive(Clone, Copy, Debug)]
pub struct Potion {
    pub id: i32,
    pub name: &'static str,
    pub bad: bool,
    pub liquid_color: i32,
    /// `statusIconIndex` (-1 none)
    pub icon: i32,
    pub effectiveness: f64,
    /// `PotionHealth`: heal, harm and saturation act at once.
    pub instant: bool,
    /// `registerPotionAttributeModifier`: attribute, UUID, amount, operation.
    pub modifier: Option<(Attr, u128, f64, i32)>,
}

const fn p(
    id: i32,
    bad: bool,
    color: i32,
    name: &'static str,
    icon: (i32, i32),
    eff: Option<f64>,
    instant: bool,
    modifier: Option<(Attr, u128, f64, i32)>,
) -> Potion {
    Potion {
        id,
        name,
        bad,
        liquid_color: color,
        icon: if icon.0 < 0 { -1 } else { icon.0 + icon.1 * 8 },
        effectiveness: match eff {
            Some(e) => e,
            None => {
                if bad {
                    0.5
                } else {
                    1.0
                }
            }
        },
        instant,
        modifier,
    }
}

const NO: (i32, i32) = (-1, 0);

/// `Potion.potionTypes`
pub static POTIONS: [Option<Potion>; 32] = {
    let mut t: [Option<Potion>; 32] = [None; 32];
    t[1] = Some(p(
        1,
        false,
        8171462,
        "potion.moveSpeed",
        (0, 0),
        None,
        false,
        Some((
            Attr::MovementSpeed,
            0x91AEAA56_376B_4498_935B_2F7F68070635,
            0.2f32 as f64,
            2,
        )),
    ));
    t[2] = Some(p(
        2,
        true,
        5926017,
        "potion.moveSlowdown",
        (1, 0),
        None,
        false,
        Some((
            Attr::MovementSpeed,
            0x7107DE5E_7CE8_4030_940E_514C1F160890,
            -0.15f32 as f64,
            2,
        )),
    ));
    t[3] = Some(p(
        3,
        false,
        14270531,
        "potion.digSpeed",
        (2, 0),
        Some(1.5),
        false,
        None,
    ));
    t[4] = Some(p(
        4,
        true,
        4866583,
        "potion.digSlowDown",
        (3, 0),
        None,
        false,
        None,
    ));
    t[5] = Some(p(
        5,
        false,
        9643043,
        "potion.damageBoost",
        (4, 0),
        None,
        false,
        Some((
            Attr::AttackDamage,
            0x648D7064_6A60_4F59_8ABE_C2C23A6DD7A9,
            3.0,
            2,
        )),
    ));
    t[6] = Some(p(6, false, 16262179, "potion.heal", NO, None, true, None));
    t[7] = Some(p(7, true, 4393481, "potion.harm", NO, None, true, None));
    t[8] = Some(p(
        8,
        false,
        7889559,
        "potion.jump",
        (2, 1),
        None,
        false,
        None,
    ));
    t[9] = Some(p(
        9,
        true,
        5578058,
        "potion.confusion",
        (3, 1),
        Some(0.25),
        false,
        None,
    ));
    t[10] = Some(p(
        10,
        false,
        13458603,
        "potion.regeneration",
        (7, 0),
        Some(0.25),
        false,
        None,
    ));
    t[11] = Some(p(
        11,
        false,
        10044730,
        "potion.resistance",
        (6, 1),
        None,
        false,
        None,
    ));
    t[12] = Some(p(
        12,
        false,
        14981690,
        "potion.fireResistance",
        (7, 1),
        None,
        false,
        None,
    ));
    t[13] = Some(p(
        13,
        false,
        3035801,
        "potion.waterBreathing",
        (0, 2),
        None,
        false,
        None,
    ));
    t[14] = Some(p(
        14,
        false,
        8356754,
        "potion.invisibility",
        (0, 1),
        None,
        false,
        None,
    ));
    t[15] = Some(p(
        15,
        true,
        2039587,
        "potion.blindness",
        (5, 1),
        Some(0.25),
        false,
        None,
    ));
    t[16] = Some(p(
        16,
        false,
        2039713,
        "potion.nightVision",
        (4, 1),
        None,
        false,
        None,
    ));
    t[17] = Some(p(
        17,
        true,
        5797459,
        "potion.hunger",
        (1, 1),
        None,
        false,
        None,
    ));
    t[18] = Some(p(
        18,
        true,
        4738376,
        "potion.weakness",
        (5, 0),
        None,
        false,
        Some((
            Attr::AttackDamage,
            0x22653B89_116E_49DC_9B6B_9971489B5BE5,
            2.0,
            0,
        )),
    ));
    t[19] = Some(p(
        19,
        true,
        5149489,
        "potion.poison",
        (6, 0),
        Some(0.25),
        false,
        None,
    ));
    t[20] = Some(p(
        20,
        true,
        3484199,
        "potion.wither",
        (1, 2),
        Some(0.25),
        false,
        None,
    ));
    t[21] = Some(p(
        21,
        false,
        16284963,
        "potion.healthBoost",
        (2, 2),
        None,
        false,
        Some((
            Attr::MaxHealth,
            0x5D6F0BA2_1186_46AC_B896_C61C5CEE99CC,
            4.0,
            0,
        )),
    ));
    t[22] = Some(p(
        22,
        false,
        2445989,
        "potion.absorption",
        (2, 2),
        None,
        false,
        None,
    ));
    t[23] = Some(p(
        23,
        false,
        16262179,
        "potion.saturation",
        NO,
        None,
        true,
        None,
    ));
    t
};

pub fn potion(id: i32) -> Option<&'static Potion> {
    if (0..32).contains(&id) {
        POTIONS[id as usize].as_ref()
    } else {
        None
    }
}

impl Potion {
    /// `Potion.isReady(duration, amplifier)`
    pub fn is_ready(&self, d: i32, a: i32) -> bool {
        if self.instant {
            // PotionHealth.isReady
            return d >= 1;
        }
        let period = |base: i32| {
            let k = base >> a;
            if k > 0 {
                d % k == 0
            } else {
                true
            }
        };
        match self.id {
            REGENERATION => period(50),
            POISON => period(25),
            WITHER => period(40),
            _ => self.id == HUNGER,
        }
    }

    /// `Potion.getAttributeModifierAmount` (`PotionAttackDamage` overrides it).
    pub fn modifier_amount(&self, amp: i32, base: f64) -> f64 {
        match self.id {
            WEAKNESS => (-0.5f32 * (amp + 1) as f32) as f64,
            DAMAGE_BOOST => 1.3 * (amp + 1) as f64,
            _ => base * (amp + 1) as f64,
        }
    }
}

/// `PotionEffect`
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Effect {
    pub id: i32,
    pub duration: i32,
    pub amplifier: i32,
    pub splash: bool,
    pub ambient: bool,
    /// `isPotionDurationMax`
    pub duration_max: bool,
}

impl Effect {
    pub fn new(id: i32, duration: i32, amplifier: i32) -> Effect {
        Effect {
            id,
            duration,
            amplifier,
            splash: false,
            ambient: false,
            duration_max: false,
        }
    }

    pub fn ambient(id: i32, duration: i32, amplifier: i32, ambient: bool) -> Effect {
        Effect {
            ambient,
            ..Effect::new(id, duration, amplifier)
        }
    }

    pub fn potion(&self) -> &'static Potion {
        potion(self.id).expect("registered potion")
    }

    /// `PotionEffect.combine`
    pub fn combine(&mut self, o: &Effect) {
        if o.amplifier > self.amplifier {
            self.amplifier = o.amplifier;
            self.duration = o.duration;
        } else if o.amplifier == self.amplifier && self.duration < o.duration {
            self.duration = o.duration;
        } else if !o.ambient && self.ambient {
            self.ambient = o.ambient;
        }
    }

    /// `PotionEffect.writeCustomPotionEffectToNBT`
    pub fn to_nbt(&self) -> crate::world::anvil::Nbt {
        use crate::world::anvil::{compound, Nbt};
        compound(vec![
            ("Id", Nbt::Byte(self.id as i8)),
            ("Amplifier", Nbt::Byte(self.amplifier as i8)),
            ("Duration", Nbt::Int(self.duration)),
            ("Ambient", Nbt::Byte(self.ambient as i8)),
        ])
    }

    /// `PotionEffect.readCustomPotionEffectFromNBT`
    pub fn from_nbt(t: &crate::world::anvil::Nbt) -> Option<Effect> {
        let id = t.int("Id")? as i8 as i32;
        potion(id)?;
        Some(Effect::ambient(
            id,
            t.int("Duration").unwrap_or(0) as i32,
            t.int("Amplifier").unwrap_or(0) as i8 as i32,
            t.int("Ambient").unwrap_or(0) != 0,
        ))
    }
}

// ---------------------------------------------------------------- PotionHelper

pub const SUGAR_EFFECT: &str = "-0+1-2-3&4-4+13";
pub const GHAST_TEAR_EFFECT: &str = "+0-1-2-3&4-4+13";
pub const SPIDER_EYE_EFFECT: &str = "-0-1+2-3&4-4+13";
pub const FERMENTED_SPIDER_EYE_EFFECT: &str = "-0+3-4+13";
pub const SPECKLED_MELON_EFFECT: &str = "+0-1+2-3&4-4+13";
pub const BLAZE_POWDER_EFFECT: &str = "+0-1-2+3&4-4+13";
pub const MAGMA_CREAM_EFFECT: &str = "+0+1-2-3&4-4+13";
pub const REDSTONE_EFFECT: &str = "-5+6-7";
pub const GLOWSTONE_EFFECT: &str = "+5-6-7";
pub const GUNPOWDER_EFFECT: &str = "+14&13-13";
pub const GOLDEN_CARROT_EFFECT: &str = "-0+1+2-3+13&4-4";
pub const PUFFERFISH_EFFECT: &str = "+0-1+2+3+13&4-4";

pub const PREFIXES: [&str; 32] = [
    "potion.prefix.mundane",
    "potion.prefix.uninteresting",
    "potion.prefix.bland",
    "potion.prefix.clear",
    "potion.prefix.milky",
    "potion.prefix.diffuse",
    "potion.prefix.artless",
    "potion.prefix.thin",
    "potion.prefix.awkward",
    "potion.prefix.flat",
    "potion.prefix.bulky",
    "potion.prefix.bungling",
    "potion.prefix.buttered",
    "potion.prefix.smooth",
    "potion.prefix.suave",
    "potion.prefix.debonair",
    "potion.prefix.thick",
    "potion.prefix.elegant",
    "potion.prefix.fancy",
    "potion.prefix.charming",
    "potion.prefix.dashing",
    "potion.prefix.refined",
    "potion.prefix.cordial",
    "potion.prefix.sparkling",
    "potion.prefix.potent",
    "potion.prefix.foul",
    "potion.prefix.odorless",
    "potion.prefix.rank",
    "potion.prefix.harsh",
    "potion.prefix.acrid",
    "potion.prefix.gross",
    "potion.prefix.stinky",
];

/// `potionRequirements`
fn requirement(id: i32) -> Option<&'static str> {
    Some(match id {
        REGENERATION => "0 & !1 & !2 & !3 & 0+6",
        MOVE_SPEED => "!0 & 1 & !2 & !3 & 1+6",
        FIRE_RESISTANCE => "0 & 1 & !2 & !3 & 0+6",
        HEAL => "0 & !1 & 2 & !3",
        POISON => "!0 & !1 & 2 & !3 & 2+6",
        WEAKNESS => "!0 & !1 & !2 & 3 & 3+6",
        HARM => "!0 & !1 & 2 & 3",
        MOVE_SLOWDOWN => "!0 & 1 & !2 & 3 & 3+6",
        DAMAGE_BOOST => "0 & !1 & !2 & 3 & 3+6",
        NIGHT_VISION => "!0 & 1 & 2 & !3 & 2+6",
        INVISIBILITY => "!0 & 1 & 2 & 3 & 2+6",
        WATER_BREATHING => "0 & !1 & 2 & 3 & 2+6",
        _ => return None,
    })
}

/// `potionAmplifiers`
fn amplifier_req(id: i32) -> Option<&'static str> {
    match id {
        MOVE_SPEED | DIG_SPEED | DAMAGE_BOOST | REGENERATION | HARM | HEAL | RESISTANCE
        | POISON => Some("5"),
        _ => None,
    }
}

pub fn check_flag(v: i32, bit: i32) -> bool {
    v & 1 << bit != 0
}

fn is_flag_set(v: i32, bit: i32) -> i32 {
    check_flag(v, bit) as i32
}

fn is_flag_unset(v: i32, bit: i32) -> i32 {
    !check_flag(v, bit) as i32
}

/// `getPotionPrefixIndex`
pub fn prefix_index(v: i32) -> usize {
    prefix_index_flags(v, 5, 4, 3, 2, 1) as usize
}

/// `getPotionPrefixIndexFlags`
pub fn prefix_index_flags(v: i32, a: i32, b: i32, c: i32, d: i32, e: i32) -> i32 {
    (if check_flag(v, a) { 16 } else { 0 })
        | (if check_flag(v, b) { 8 } else { 0 })
        | (if check_flag(v, c) { 4 } else { 0 })
        | (if check_flag(v, d) { 2 } else { 0 })
        | (if check_flag(v, e) { 1 } else { 0 })
}

/// `calcPotionLiquidColor`
pub fn liquid_color_of(effects: &[Effect]) -> i32 {
    if effects.is_empty() {
        return 3694022;
    }
    let (mut r, mut g, mut b, mut n) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for e in effects {
        let c = e.potion().liquid_color;
        for _ in 0..=e.amplifier {
            r += (c >> 16 & 255) as f32 / 255.0;
            g += (c >> 8 & 255) as f32 / 255.0;
            b += (c & 255) as f32 / 255.0;
            n += 1.0;
        }
    }
    let r = r / n * 255.0;
    let g = g / n * 255.0;
    let b = b / n * 255.0;
    (r as i32) << 16 | (g as i32) << 8 | b as i32
}

/// `getAreAmbient`
pub fn are_ambient(effects: &[Effect]) -> bool {
    effects.iter().all(|e| e.ambient)
}

/// `getLiquidColor(damage, false)`
pub fn liquid_color(damage: i32) -> i32 {
    liquid_color_of(&effects_of(damage, false))
}

fn get_potion_effect(
    not: bool,
    star: bool,
    neg: bool,
    cmp: i32,
    bit: i32,
    mul: i32,
    v: i32,
) -> i32 {
    let mut r = 0;
    if not {
        r = is_flag_unset(v, bit);
    } else if cmp != -1 {
        let n = (v as u32).count_ones() as i32;
        // countSetFlags only counts while var0 > 0
        let n = if v > 0 { n } else { 0 };
        if (cmp == 0 && n == bit) || (cmp == 1 && n > bit) || (cmp == 2 && n < bit) {
            r = 1;
        }
    } else {
        r = is_flag_set(v, bit);
    }
    if star {
        r *= mul;
    }
    if neg {
        r *= -1;
    }
    r
}

/// `parsePotionEffects(s, from, to, damage)`
fn parse(s: &[u8], from: i32, to: i32, v: i32) -> i32 {
    if !(from < s.len() as i32 && to >= 0 && from < to) {
        return 0;
    }
    let find = |c: u8| {
        s[from as usize..]
            .iter()
            .position(|&x| x == c)
            .map(|i| i as i32 + from)
    };
    if let Some(bar) = find(b'|').filter(|&i| i < to) {
        let a = parse(s, from, bar - 1, v);
        if a > 0 {
            return a;
        }
        let b = parse(s, bar + 1, to, v);
        return if b > 0 { b } else { 0 };
    }
    if let Some(amp) = find(b'&').filter(|&i| i < to) {
        let a = parse(s, from, amp - 1, v);
        if a <= 0 {
            return 0;
        }
        let b = parse(s, amp + 1, to, v);
        if b <= 0 {
            return 0;
        }
        return if a > b { a } else { b };
    }
    let (mut star, mut has_mul, mut has_num, mut not, mut neg) =
        (false, false, false, false, false);
    let mut cmp = -1;
    let (mut num, mut mul, mut total) = (0, 0, 0);
    macro_rules! flush {
        () => {
            if has_num {
                total += get_potion_effect(not, has_mul, neg, cmp, num, mul, v);
                not = false;
                neg = false;
                star = false;
                has_mul = false;
                has_num = false;
                mul = 0;
                num = 0;
                cmp = -1;
            }
        };
    }
    for i in from..to {
        let c = s[i as usize];
        if c.is_ascii_digit() {
            if star {
                mul = (c - b'0') as i32;
                has_mul = true;
            } else {
                num = num * 10 + (c - b'0') as i32;
                has_num = true;
            }
        } else if c == b'*' {
            star = true;
        } else if c == b'!' {
            flush!();
            not = true;
        } else if c == b'-' {
            flush!();
            neg = true;
        } else if c != b'=' && c != b'<' && c != b'>' {
            if c == b'+' && has_num {
                flush!();
            }
        } else {
            flush!();
            cmp = match c {
                b'=' => 0,
                b'<' => 2,
                _ => 1,
            };
        }
    }
    let _ = star;
    if has_num {
        total += get_potion_effect(not, has_mul, neg, cmp, num, mul, v);
    }
    total
}

/// `PotionHelper.getPotionEffects(damage, all)`
pub fn effects_of(v: i32, _all: bool) -> Vec<Effect> {
    let mut out = Vec::new();
    for pt in POTIONS.iter().flatten() {
        let Some(req) = requirement(pt.id) else {
            continue;
        };
        let mut n = parse(req.as_bytes(), 0, req.len() as i32, v);
        if n > 0 {
            let mut amp = 0;
            if let Some(a) = amplifier_req(pt.id) {
                amp = parse(a.as_bytes(), 0, a.len() as i32, v).max(0);
            }
            if pt.instant {
                n = 1;
            } else {
                n = 1200 * (n * 3 + (n - 1) * 2);
                n >>= amp;
                n = (n as f64 * pt.effectiveness).round() as i32;
                if v & 16384 != 0 {
                    n = (n as f64 * 0.75 + 0.5).round() as i32;
                }
            }
            let mut e = Effect::new(pt.id, n, amp);
            if v & 16384 != 0 {
                e.splash = true;
            }
            out.push(e);
        }
    }
    out
}

fn brew_bit(mut v: i32, bit: i32, clear: bool, toggle: bool, require: bool) -> i32 {
    if require {
        if !check_flag(v, bit) {
            return 0;
        }
    } else if clear {
        v &= !(1 << bit);
    } else if toggle {
        if v & 1 << bit == 0 {
            v |= 1 << bit;
        } else {
            v &= !(1 << bit);
        }
    } else {
        v |= 1 << bit;
    }
    v
}

/// `PotionHelper.applyIngredient`
pub fn apply_ingredient(mut v: i32, s: &str) -> i32 {
    let (mut has, mut toggle, mut clear, mut require) = (false, false, false, false);
    let mut n = 0;
    macro_rules! flush {
        () => {
            if has {
                v = brew_bit(v, n, clear, toggle, require);
                require = false;
                toggle = false;
                clear = false;
                has = false;
                n = 0;
            }
        };
    }
    for c in s.bytes() {
        if c.is_ascii_digit() {
            n = n * 10 + (c - b'0') as i32;
            has = true;
        } else if c == b'!' {
            flush!();
            toggle = true;
        } else if c == b'-' {
            flush!();
            clear = true;
        } else if c == b'+' {
            flush!();
        } else if c == b'&' {
            flush!();
            require = true;
        }
    }
    if has {
        v = brew_bit(v, n, clear, toggle, require);
    }
    v & 32767
}

/// `Item.getPotionEffect(stack)`: the brewing string of an ingredient.
pub fn ingredient_effect(id: u16, meta: i32) -> Option<&'static str> {
    use crate::item::*;
    Some(match id {
        353 => SUGAR_EFFECT,
        370 => GHAST_TEAR_EFFECT,
        375 => SPIDER_EYE_EFFECT,
        376 => FERMENTED_SPIDER_EYE_EFFECT,
        382 => SPECKLED_MELON_EFFECT,
        377 => BLAZE_POWDER_EFFECT,
        378 => MAGMA_CREAM_EFFECT,
        x if x == REDSTONE => REDSTONE_EFFECT,
        348 => GLOWSTONE_EFFECT,
        289 => GUNPOWDER_EFFECT,
        396 => GOLDEN_CARROT_EFFECT,
        // ItemFishFood: pufferfish (meta 3)
        349 if meta == 3 => PUFFERFISH_EFFECT,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brewing_formulas_match_vanilla_values() {
        // water bottle + nether wart = awkward (16)
        assert_eq!(apply_ingredient(0, "+4"), 16);
        // awkward + sugar = swiftness 8194
        let speed = apply_ingredient(16, SUGAR_EFFECT);
        assert_eq!(speed & 15, 2);
        let e = effects_of(8194, false);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].id, MOVE_SPEED);
        assert_eq!(e[0].duration, 3600);
        // Potion of Healing II (8229): instant, amplifier 1
        let h = effects_of(8229, false);
        assert_eq!((h[0].id, h[0].duration, h[0].amplifier), (HEAL, 1, 1));
        // Splash Poison (16388): 0.75 duration
        let s = effects_of(16388, false);
        assert_eq!((s[0].id, s[0].duration), (POISON, 676));
        assert!(s[0].splash);
        // the colour of a water bottle
        assert_eq!(liquid_color(0), 3694022);
    }
}
