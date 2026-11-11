//! Switch between exact 1.7.10 behaviour and the corrected behaviour.
//!
//! The port is verified block-for-block against the original game, which means
//! it also reproduces the original's defects. Those that were later fixed by
//! Mojang (see `docs/BUG_AUDIT.md`) are corrected by default; `--vanilla-parity`
//! (or [`set_vanilla_parity`]) restores the 1.7.10 behaviour so the comparison
//! harness keeps passing.

use std::sync::atomic::{AtomicBool, Ordering};

static VANILLA_PARITY: AtomicBool = AtomicBool::new(false);

pub fn set_vanilla_parity(on: bool) {
    VANILLA_PARITY.store(on, Ordering::Relaxed);
}

/// True when the known 1.7.10 defects must be reproduced.
#[inline]
pub fn vanilla_parity() -> bool {
    VANILLA_PARITY.load(Ordering::Relaxed)
}

/// True when the corrected behaviour is active (the default).
#[inline]
pub fn fixed() -> bool {
    !vanilla_parity()
}
