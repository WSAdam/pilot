//! The rate pilot's animation frames run at: the display's refresh rate, read
//! the way Chrome's compositor reads it.
//!
//! Chrome's compositor (viz) drives animation frames from the display's vsync.
//! On macOS that is a CVDisplayLink for the display the compositor names, at
//! the link's nominal refresh period
//! (components/viz/service/frame_sinks/external_begin_frame_source_mac.cc,
//! ui/display/mac/cv_display_link_mac.mm). Headless Chrome, the shell and new
//! headless alike, has a screen of its own whose first display has id 1
//! (ui/display/headless/headless_screen_manager.cc), and macOS gives the
//! built-in panel CGDirectDisplayID 1, so headless Chrome follows that panel:
//! 120 Hz on a ProMotion Mac. A headful window follows its display, the main
//! one when it opens. Without a display link, and on Linux, where headless
//! Chrome has no vsync provider, viz runs a 60 Hz timer
//! (`BeginFrameArgs::DefaultInterval`).

use std::ffi::OsStr;

/// Set to a rate in frames per second to use instead of the display's.
pub const FRAME_RATE_ENV: &str = "PILOT_FRAME_RATE";

/// Where pilot's animation frame rate came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameRateSource {
    /// [`FRAME_RATE_ENV`].
    Override,
    /// The display's refresh rate.
    Display,
    /// No display rate: Chrome's 60 Hz timer.
    Default,
}

/// An animation frame rate, in frames per second, and where it came from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameRate {
    pub hz: f64,
    pub source: FrameRateSource,
}

/// The animation frame rate: [`FRAME_RATE_ENV`]'s value when it is a usable
/// rate, else `display_rate()` when it is one, else 60 Hz.
pub fn choose_frame_rate(
    override_value: Option<&OsStr>,
    display_rate: impl FnOnce() -> Option<f64>,
) -> FrameRate {
    if let Some(value) = override_value {
        match value
            .to_str()
            .and_then(|text| text.trim().parse::<f64>().ok())
        {
            Some(hz) if moli_core::is_usable_animation_frame_rate(hz) => {
                return FrameRate {
                    hz,
                    source: FrameRateSource::Override,
                };
            }
            _ => tracing::warn!(
                ?value,
                "⚠️ {FRAME_RATE_ENV} is not a frame rate between 1 and 1000; pilot reads the display's"
            ),
        }
    }
    match display_rate() {
        Some(hz) if moli_core::is_usable_animation_frame_rate(hz) => FrameRate {
            hz,
            source: FrameRateSource::Display,
        },
        _ => FrameRate {
            hz: moli_core::DEFAULT_ANIMATION_FRAME_RATE_HZ,
            source: FrameRateSource::Default,
        },
    }
}

/// Reads the animation frame rate once, for this process, and sets it for
/// every document before any page runs.
pub fn install_frame_rate(headless: bool) -> FrameRate {
    let chosen = choose_frame_rate(std::env::var_os(FRAME_RATE_ENV).as_deref(), || {
        display_frame_rate(headless)
    });
    let hz = moli_core::set_animation_frame_rate(chosen.hz);
    tracing::info!(hz, source = ?chosen.source, "🔍 animation frames run at {hz:.3} Hz");
    FrameRate { hz, ..chosen }
}

/// The refresh rate of the display Chrome's animation frames would follow:
/// display 1 for a headless launch, the main display for a headful one. None
/// when that display has no display link.
#[cfg(target_os = "macos")]
pub fn display_frame_rate(headless: bool) -> Option<f64> {
    let display_id = if headless {
        mac::HEADLESS_DISPLAY_ID
    } else {
        mac::main_display_id()
    };
    mac::nominal_refresh_rate(display_id)
}

/// Headless Chrome on Linux has no vsync provider: its frames follow a 60 Hz
/// timer, not a display.
#[cfg(not(target_os = "macos"))]
pub fn display_frame_rate(_headless: bool) -> Option<f64> {
    None
}

/// The rate of a CoreVideo refresh period of `time_value / time_scale`
/// seconds, as CVDisplayLinkMac::GetRefreshInterval reads it; None for an
/// indefinite or empty period.
#[cfg(any(target_os = "macos", test))]
fn rate_from_refresh_period(time_value: i64, time_scale: i32, indefinite: bool) -> Option<f64> {
    if indefinite || time_value <= 0 || time_scale <= 0 {
        return None;
    }
    Some(f64::from(time_scale) / time_value as f64)
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::c_void;

    /// The display headless Chrome's screen names first:
    /// `HeadlessScreenManager::GetNewDisplayId` counts from 1.
    pub(super) const HEADLESS_DISPLAY_ID: u32 = 1;

    /// CoreVideo's `CVTime` (CVBase.h).
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CvTime {
        time_value: i64,
        time_scale: i32,
        flags: i32,
    }

    /// `kCVReturnSuccess` (CVReturn.h).
    const CV_RETURN_SUCCESS: i32 = 0;
    /// `kCVTimeIsIndefinite` (CVBase.h).
    const CV_TIME_IS_INDEFINITE: i32 = 1 << 0;

    #[link(name = "CoreVideo", kind = "framework")]
    unsafe extern "C" {
        fn CVDisplayLinkCreateWithCGDisplay(
            display_id: u32,
            display_link_out: *mut *mut c_void,
        ) -> i32;
        fn CVDisplayLinkGetCurrentCGDisplay(display_link: *mut c_void) -> u32;
        fn CVDisplayLinkGetNominalOutputVideoRefreshPeriod(display_link: *mut c_void) -> CvTime;
        fn CVDisplayLinkRelease(display_link: *mut c_void);
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGMainDisplayID() -> u32;
    }

    /// The main display: the one with the menu bar, where a window opens.
    pub(super) fn main_display_id() -> u32 {
        // SAFETY: CGMainDisplayID takes no arguments and reads the display
        // configuration; it returns an id even with no display attached.
        unsafe { CGMainDisplayID() }
    }

    /// `display_id`'s nominal refresh rate, read as Chrome's CVDisplayLinkMac
    /// reads it: None when CoreVideo cannot create a display link for it, or
    /// creates one with no current display (Chrome's crbug.com/1218720
    /// workaround), or the link's period is indefinite.
    pub(super) fn nominal_refresh_rate(display_id: u32) -> Option<f64> {
        let mut display_link: *mut c_void = std::ptr::null_mut();
        // SAFETY: `display_link` is a valid place for CoreVideo to write the
        // created link to.
        let created = unsafe { CVDisplayLinkCreateWithCGDisplay(display_id, &mut display_link) };
        if created != CV_RETURN_SUCCESS || display_link.is_null() {
            return None;
        }
        // SAFETY: `display_link` is the link created above, which this
        // function owns and releases once. Its period is read only when it
        // has a current display: without one its internal state is unset.
        let period = unsafe {
            let period = if CVDisplayLinkGetCurrentCGDisplay(display_link) == 0 {
                None
            } else {
                Some(CVDisplayLinkGetNominalOutputVideoRefreshPeriod(
                    display_link,
                ))
            };
            CVDisplayLinkRelease(display_link);
            period
        }?;
        super::rate_from_refresh_period(
            period.time_value,
            period.time_scale,
            (period.flags & CV_TIME_IS_INDEFINITE) != 0,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    const PROMOTION_HZ: f64 = 24_000_000.0 / 199_999.0;

    // 2026-11-18: pilot's animation frames ran at 60 Hz on a 120 Hz Mac, where
    // headless Chromium 1223's step at the panel's rate (8.33 ms apart).
    #[test]
    fn frames_follow_the_display_rate() {
        assert_eq!(
            choose_frame_rate(None, || Some(PROMOTION_HZ)),
            FrameRate {
                hz: PROMOTION_HZ,
                source: FrameRateSource::Display
            }
        );
    }

    #[test]
    fn frames_run_at_60_hz_without_a_display_rate() {
        for display in [None, Some(0.0), Some(-60.0), Some(f64::NAN), Some(5000.0)] {
            assert_eq!(
                choose_frame_rate(None, || display),
                FrameRate {
                    hz: 60.0,
                    source: FrameRateSource::Default
                },
                "{display:?}"
            );
        }
    }

    #[test]
    fn the_frame_rate_override_wins_over_the_display() {
        let display_read = Cell::new(false);
        let chosen = choose_frame_rate(Some(OsStr::new(" 60 ")), || {
            display_read.set(true);
            Some(PROMOTION_HZ)
        });
        assert_eq!(
            chosen,
            FrameRate {
                hz: 60.0,
                source: FrameRateSource::Override
            }
        );
        assert!(!display_read.get(), "the override needs no display");
    }

    #[test]
    fn an_unusable_frame_rate_override_is_ignored() {
        for value in ["", "fast", "0", "-120", "NaN", "inf", "1e9"] {
            assert_eq!(
                choose_frame_rate(Some(OsStr::new(value)), || Some(PROMOTION_HZ)),
                FrameRate {
                    hz: PROMOTION_HZ,
                    source: FrameRateSource::Display
                },
                "{value:?}"
            );
            assert_eq!(
                choose_frame_rate(Some(OsStr::new(value)), || None).source,
                FrameRateSource::Default,
                "{value:?}"
            );
        }
    }

    // CoreVideo's nominal period on a 120 Hz ProMotion panel is 199999 ticks
    // of a 24 MHz clock, on a 60 Hz display 400000.
    #[test]
    fn a_refresh_period_reads_as_its_rate() {
        assert_eq!(
            rate_from_refresh_period(199_999, 24_000_000, false),
            Some(PROMOTION_HZ)
        );
        assert_eq!(
            rate_from_refresh_period(400_000, 24_000_000, false),
            Some(60.0)
        );
        assert_eq!(rate_from_refresh_period(400_000, 24_000_000, true), None);
        assert_eq!(rate_from_refresh_period(0, 24_000_000, false), None);
        assert_eq!(rate_from_refresh_period(400_000, 0, false), None);
    }

    #[test]
    fn display_frame_rate_reads_no_display_off_macos() {
        if cfg!(not(target_os = "macos")) {
            assert_eq!(display_frame_rate(true), None);
            assert_eq!(display_frame_rate(false), None);
        }
    }
}
