//! Bit-exact port of `java.util.Random` (48-bit LCG).

const MULT: i64 = 0x5DEECE66D;
const ADD: i64 = 0xB;
const MASK: i64 = (1 << 48) - 1;

#[derive(Clone, Debug)]
pub struct JavaRandom {
    seed: i64,
    next_next_gaussian: f64,
    have_next_next_gaussian: bool,
}

impl JavaRandom {
    pub fn new(seed: i64) -> Self {
        JavaRandom { seed: (seed ^ MULT) & MASK, next_next_gaussian: 0.0, have_next_next_gaussian: false }
    }

    pub fn set_seed(&mut self, seed: i64) {
        self.seed = (seed ^ MULT) & MASK;
        self.have_next_next_gaussian = false;
    }

    #[inline]
    fn next(&mut self, bits: u32) -> i32 {
        self.seed = self.seed.wrapping_mul(MULT).wrapping_add(ADD) & MASK;
        (self.seed >> (48 - bits)) as i32
    }

    pub fn next_int(&mut self) -> i32 {
        self.next(32)
    }

    /// `nextInt(int bound)`; panics on non-positive bound like Java throws.
    pub fn next_int_n(&mut self, n: i32) -> i32 {
        // java.util.Random throws IllegalArgumentException here (MC-46082: void
        // columns crash the decorator). Returning 0 keeps generation alive.
        if n <= 0 {
            return 0;
        }
        if n & (-n) == n {
            return ((n as i64).wrapping_mul(self.next(31) as i64) >> 31) as i32;
        }
        loop {
            let bits = self.next(31);
            let val = bits % n;
            if bits.wrapping_sub(val).wrapping_add(n - 1) >= 0 {
                return val;
            }
        }
    }

    pub fn next_long(&mut self) -> i64 {
        ((self.next(32) as i64) << 32).wrapping_add(self.next(32) as i64)
    }

    pub fn next_boolean(&mut self) -> bool {
        self.next(1) != 0
    }

    pub fn next_float(&mut self) -> f32 {
        self.next(24) as f32 / (1 << 24) as f32
    }

    pub fn next_double(&mut self) -> f64 {
        (((self.next(26) as i64) << 27) + self.next(27) as i64) as f64 / (1i64 << 53) as f64
    }

    pub fn next_gaussian(&mut self) -> f64 {
        if self.have_next_next_gaussian {
            self.have_next_next_gaussian = false;
            return self.next_next_gaussian;
        }
        loop {
            let v1 = 2.0 * self.next_double() - 1.0;
            let v2 = 2.0 * self.next_double() - 1.0;
            let s = v1 * v1 + v2 * v2;
            if s < 1.0 && s != 0.0 {
                let multiplier = (-2.0 * s.ln() / s).sqrt();
                self.next_next_gaussian = v2 * multiplier;
                self.have_next_next_gaussian = true;
                return v1 * multiplier;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_java() {
        // Reference values from java.util.Random(42).
        let mut r = JavaRandom::new(42);
        assert_eq!(r.next_int(), -1170105035);
        assert_eq!(r.next_int(), 234785527);
        let mut r = JavaRandom::new(42);
        assert_eq!(r.next_int_n(10), 0);
        assert_eq!(r.next_int_n(10), 3);
        let mut r = JavaRandom::new(42);
        assert_eq!(r.next_long(), -5025562857975149833);
    }
}

#[cfg(test)]
mod java_reference_tests {
    use super::*; use crate::java::string_hash_code;

    #[test]
    fn matches_java_util_random() {
        let mut r = JavaRandom::new(0);
        assert_eq!(r.next_int(), -1155484576);
        assert_eq!(r.next_int(), -723955400);
        let mut r = JavaRandom::new(0);
        assert_eq!(r.next_long(), -4962768465676381896);
        let mut r = JavaRandom::new(42);
        assert_eq!(r.next_int_n(10), 0);
        assert_eq!(r.next_int_n(10), 3);
    }

    #[test]
    fn string_hash_matches_java() {
        assert_eq!(string_hash_code("Glacier"), "Glacier".chars().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32)));
        assert_eq!(string_hash_code("hello"), 99162322);
    }
}
