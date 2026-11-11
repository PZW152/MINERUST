pub mod random;
pub use random::JavaRandom;

/// Java `String.hashCode()` (used for text world seeds).
pub fn string_hash_code(s: &str) -> i32 {
    let mut h: i32 = 0;
    for c in s.encode_utf16() {
        h = h.wrapping_mul(31).wrapping_add(c as i32);
    }
    h
}
