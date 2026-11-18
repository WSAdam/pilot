//! The rate animation frames run at, for every document in the process.
//!
//! Chrome's animation frames follow the display's refresh rate: its compositor
//! drives them from the display's vsync, and from a 60 Hz timer when there is
//! no display to follow (`BeginFrameArgs::DefaultInterval()`). The embedder
//! reads the display once, at startup, and sets the rate here before any page
//! runs; until it does, frames run at 60 Hz.

use std::sync::atomic::{AtomicU64, Ordering};

/// The rate when the embedder sets none, or one no display has: Chrome's
/// rate without a display.
pub const DEFAULT_ANIMATION_FRAME_RATE_HZ: f64 = 60.0;

/// The slowest and fastest rates taken as a display's.
const MIN_ANIMATION_FRAME_RATE_HZ: f64 = 1.0;
const MAX_ANIMATION_FRAME_RATE_HZ: f64 = 1000.0;

/// The frame interval in milliseconds as `f64` bits, or zero while unset.
static ANIMATION_FRAME_INTERVAL_MS_BITS: AtomicU64 = AtomicU64::new(0);

/// Whether `rate_hz` can be a display's refresh rate, in frames per second.
pub fn is_usable_animation_frame_rate(rate_hz: f64) -> bool {
    rate_hz.is_finite()
        && (MIN_ANIMATION_FRAME_RATE_HZ..=MAX_ANIMATION_FRAME_RATE_HZ).contains(&rate_hz)
}

/// Sets the animation frame rate of every document in this process and
/// returns the rate in effect: `rate_hz`, or 60 Hz when no display has it.
/// The embedder calls this once, before any page runs.
pub fn set_animation_frame_rate(rate_hz: f64) -> f64 {
    let rate_hz = usable_rate_or_default(rate_hz);
    ANIMATION_FRAME_INTERVAL_MS_BITS.store(interval_ms(rate_hz).to_bits(), Ordering::Relaxed);
    rate_hz
}

/// The time between two animation frames, in milliseconds.
pub(crate) fn animation_frame_interval_ms() -> f64 {
    interval_ms_from_bits(ANIMATION_FRAME_INTERVAL_MS_BITS.load(Ordering::Relaxed))
}

fn usable_rate_or_default(rate_hz: f64) -> f64 {
    if is_usable_animation_frame_rate(rate_hz) {
        rate_hz
    } else {
        DEFAULT_ANIMATION_FRAME_RATE_HZ
    }
}

fn interval_ms(rate_hz: f64) -> f64 {
    1000.0 / rate_hz
}

fn interval_ms_from_bits(bits: u64) -> f64 {
    if bits == 0 {
        interval_ms(DEFAULT_ANIMATION_FRAME_RATE_HZ)
    } else {
        f64::from_bits(bits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // These tests leave the process-wide rate alone: the rAF tests in this
    // crate run at the 60 Hz default.

    // 2026-11-18 (pilot): frames ran on a fixed 60 Hz grid, half the rate of
    // Chrome's on a 120 Hz ProMotion Mac (Chromium 1223: 8.33 ms apart).
    #[test]
    fn frames_run_at_60_hz_until_the_embedder_sets_a_rate() {
        assert_eq!(interval_ms_from_bits(0), 1000.0 / 60.0);
    }

    #[test]
    fn a_set_rate_is_kept_as_its_interval() {
        let interval = interval_ms(usable_rate_or_default(120.0));
        assert_eq!(interval_ms_from_bits(interval.to_bits()), 1000.0 / 120.0);
        // CoreVideo's nominal period on a ProMotion panel: 199999 / 24 MHz.
        let promotion = 24_000_000.0 / 199_999.0;
        assert_eq!(
            interval_ms_from_bits(interval_ms(usable_rate_or_default(promotion)).to_bits()),
            1000.0 / promotion
        );
    }

    #[test]
    fn a_rate_no_display_has_falls_back_to_60_hz() {
        for rate in [
            0.0,
            -120.0,
            0.5,
            1000.5,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            assert!(!is_usable_animation_frame_rate(rate), "{rate}");
            assert_eq!(usable_rate_or_default(rate), 60.0, "{rate}");
        }
        for rate in [1.0, 24.0, 59.94, 60.0, 120.0, 144.0, 240.0, 1000.0] {
            assert!(is_usable_animation_frame_rate(rate), "{rate}");
            assert_eq!(usable_rate_or_default(rate), rate);
        }
    }
}
