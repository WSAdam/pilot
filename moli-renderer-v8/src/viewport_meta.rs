//! pilot: `<meta name=viewport>` under phone emulation.
//!
//! While `Emulation.setDeviceMetricsOverride` emulates a phone
//! (`mobile: true`, Playwright's `isMobile`), Chrome lays the main Document
//! out at the width its viewport meta tag asks for: 980px without one, the
//! screen width for `width=device-width`, N for `width=N`. It then shows that
//! layout scaled to fit the screen. Desktop emulation ignores the tag.
//!
//! The parser ports Blink's `HTMLMetaElement::ParseContentAttribute`, the
//! resolution `ViewportDescription::Resolve`, and the page-scale limits that
//! `PageScaleConstraintsSet` applies on a phone. Content wider than the layout
//! viewport, which lowers Chrome's minimum page scale further, is not modeled.

use crate::{
    document_runtime::DomHandle,
    dom::native::{DomHost, Node},
};

/// The layout width of a phone page without a viewport meta tag, and of a tag
/// that sets no width (Blink's mobile `ViewportDefaultMinWidth`).
const MOBILE_DEFAULT_LAYOUT_WIDTH: f64 = 980.0;
/// Blink clamps viewport-meta lengths and scales to these ranges.
const MIN_LENGTH: f64 = 1.0;
const MAX_LENGTH: f64 = 10_000.0;
const MIN_ZOOM: f64 = 0.1;
const MAX_ZOOM: f64 = 10.0;
/// A phone's page-scale limits, which a viewport meta tag also starts from.
const DEFAULT_MINIMUM_PAGE_SCALE: f64 = 0.25;
const DEFAULT_MAXIMUM_PAGE_SCALE: f64 = 5.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum ViewportLength {
    Auto,
    ExtendToZoom,
    DeviceWidth,
    DeviceHeight,
    Fixed(f64),
}

/// A length once the device keywords are replaced by the screen size.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ResolvedLength {
    Auto,
    ExtendToZoom,
    Px(f64),
}

impl ResolvedLength {
    fn new(length: ViewportLength, device_width: f64, device_height: f64) -> Self {
        match length {
            ViewportLength::Auto => Self::Auto,
            ViewportLength::ExtendToZoom => Self::ExtendToZoom,
            ViewportLength::DeviceWidth => Self::Px(device_width),
            ViewportLength::DeviceHeight => Self::Px(device_height),
            ViewportLength::Fixed(px) => Self::Px(px),
        }
    }

    /// The pixel length, `extend-to-zoom` replaced by `extended`.
    fn px_or_extended(self, extended: Option<f64>) -> Option<f64> {
        match self {
            Self::Auto => None,
            Self::ExtendToZoom => extended,
            Self::Px(px) => Some(px),
        }
    }
}

/// Blink's `ViewportDescription`: the main Document's last
/// `<meta name=viewport content>`, or the phone's default without one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ViewportDescription {
    /// Whether a viewport meta tag supplied it (Blink's legacy viewport type).
    from_meta_tag: bool,
    min_width: ViewportLength,
    max_width: ViewportLength,
    min_height: ViewportLength,
    max_height: ViewportLength,
    zoom: Option<f64>,
    min_zoom: Option<f64>,
    max_zoom: Option<f64>,
    user_zoom: bool,
}

/// How a phone shows the main Document, in CSS pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MobileViewport {
    /// The layout viewport (initial containing block): what the page is laid
    /// out in, what media queries see, `documentElement.clientWidth`.
    pub(crate) layout_width: u32,
    pub(crate) layout_height: u32,
    /// `window.innerWidth`/`innerHeight`: the screen at the minimum page scale.
    pub(crate) frame_width: u32,
    pub(crate) frame_height: u32,
    /// `visualViewport.scale`: the page scale the screen shows the layout at.
    pub(crate) page_scale: f64,
    /// `visualViewport.width`/`height`: the screen at that page scale.
    pub(crate) visual_width: f64,
    pub(crate) visual_height: f64,
}

impl ViewportDescription {
    /// A phone page without a viewport meta tag: at least 980px wide.
    const fn user_agent_default() -> Self {
        Self {
            from_meta_tag: false,
            min_width: ViewportLength::Fixed(MOBILE_DEFAULT_LAYOUT_WIDTH),
            max_width: ViewportLength::Auto,
            min_height: ViewportLength::Auto,
            max_height: ViewportLength::Auto,
            zoom: None,
            min_zoom: None,
            max_zoom: None,
            user_zoom: true,
        }
    }

    /// Blink's `GetViewportDescriptionFromContentAttribute`.
    fn from_meta_content(content: &str) -> Self {
        let mut description = Self {
            from_meta_tag: true,
            min_width: ViewportLength::Auto,
            max_width: ViewportLength::Auto,
            min_height: ViewportLength::Auto,
            max_height: ViewportLength::Auto,
            zoom: None,
            min_zoom: None,
            max_zoom: None,
            user_zoom: true,
        };
        let content = content.to_ascii_lowercase();
        for_each_key_value_pair(&content, |key, value| description.apply(key, value));
        if description.min_zoom.is_none() {
            description.min_zoom = Some(DEFAULT_MINIMUM_PAGE_SCALE);
        }
        if description.max_zoom.is_none() {
            description.max_zoom = Some(DEFAULT_MAXIMUM_PAGE_SCALE);
            description.min_zoom = description
                .min_zoom
                .map(|min_zoom| min_zoom.min(DEFAULT_MAXIMUM_PAGE_SCALE));
        }
        description
    }

    /// Blink's `HTMLMetaElement::ProcessViewportKeyValuePair`, for the keys
    /// that decide the layout viewport and the page scale.
    fn apply(&mut self, key: &str, value: &str) {
        match key {
            "width" => {
                let width = parse_length(value);
                if width != ViewportLength::Auto {
                    self.min_width = ViewportLength::ExtendToZoom;
                    self.max_width = width;
                }
            }
            "height" => {
                let height = parse_length(value);
                if height != ViewportLength::Auto {
                    self.min_height = ViewportLength::ExtendToZoom;
                    self.max_height = height;
                }
            }
            "initial-scale" => self.zoom = parse_zoom(value),
            "minimum-scale" => self.min_zoom = parse_zoom(value),
            "maximum-scale" => self.max_zoom = parse_zoom(value),
            "user-scalable" => self.user_zoom = parse_user_zoom(value),
            _ => {}
        }
    }

    /// Lays this description out on a `device_width` x `device_height` phone
    /// screen: Blink's `ViewportDescription::Resolve`, then the final page
    /// scale limits of `PageScaleConstraintsSet`.
    pub(crate) fn resolve(self, device_width: u32, device_height: u32) -> MobileViewport {
        let device_width = f64::from(device_width.max(1));
        let device_height = f64::from(device_height.max(1));

        // A tag's width is `min: extend-to-zoom; max: <width>`. Without one,
        // a tag falls back to 980px, or to the zoomed screen width when it
        // sets an initial scale and no height.
        let (min_width, max_width) = if self.from_meta_tag && self.max_width == ViewportLength::Auto
        {
            if self.zoom.is_none() {
                (
                    ViewportLength::ExtendToZoom,
                    ViewportLength::Fixed(MOBILE_DEFAULT_LAYOUT_WIDTH),
                )
            } else if self.max_height == ViewportLength::Auto {
                (ViewportLength::ExtendToZoom, ViewportLength::ExtendToZoom)
            } else {
                (self.min_width, self.max_width)
            }
        } else {
            (self.min_width, self.max_width)
        };
        let resolve = |length| ResolvedLength::new(length, device_width, device_height);
        let (min_width, max_width) = (resolve(min_width), resolve(max_width));
        let (min_height, max_height) = (resolve(self.min_height), resolve(self.max_height));

        let min_zoom = self.min_zoom;
        let mut max_zoom = self.max_zoom;
        if let (Some(min), Some(max)) = (min_zoom, max_zoom) {
            max_zoom = Some(min.max(max));
        }
        let constrain = |zoom: Option<f64>| {
            compare_ignoring_auto(
                min_zoom,
                compare_ignoring_auto(max_zoom, zoom, f64::min),
                f64::max,
            )
        };
        let zoom = if self.zoom.is_some() {
            constrain(self.zoom)
        } else {
            None
        };

        let extend_zoom = compare_ignoring_auto(zoom, max_zoom, f64::min);
        let (extend_width, extend_height) = match extend_zoom {
            Some(extend_zoom) if extend_zoom > 0.0 => (
                Some(device_width / extend_zoom),
                Some(device_height / extend_zoom),
            ),
            _ => (None, None),
        };
        let max_width = max_width.px_or_extended(extend_width);
        let max_height = max_height.px_or_extended(extend_height);
        let min_width =
            min_width.px_or_extended(compare_ignoring_auto(extend_width, max_width, f64::max));
        let min_height =
            min_height.px_or_extended(compare_ignoring_auto(extend_height, max_height, f64::max));

        let width = if min_width.is_some() || max_width.is_some() {
            compare_ignoring_auto(
                min_width,
                compare_ignoring_auto(max_width, Some(device_width), f64::min),
                f64::max,
            )
        } else {
            None
        };
        let height = if min_height.is_some() || max_height.is_some() {
            compare_ignoring_auto(
                min_height,
                compare_ignoring_auto(max_height, Some(device_height), f64::min),
                f64::max,
            )
        } else {
            None
        };
        let width = width.unwrap_or_else(|| {
            height.map_or(device_width, |height| height * device_width / device_height)
        });
        let height = height.unwrap_or(width * device_height / device_width);

        // The scale that fits the layout to the screen, which a page that
        // sets no initial scale starts at; `user-scalable=no` pins to it.
        let resolved_zoom = if zoom.is_some() {
            zoom
        } else {
            let fit_width = (width > 0.0).then_some(device_width / width);
            let fit = if height > 0.0 {
                Some(fit_width.map_or(device_height / height, |fit_width| {
                    fit_width.max(device_height / height)
                }))
            } else {
                fit_width
            };
            constrain(fit)
        };
        let (min_zoom, max_zoom) = if self.user_zoom {
            (min_zoom, max_zoom)
        } else {
            (resolved_zoom, resolved_zoom)
        };
        let initial_zoom = self.zoom.and(resolved_zoom);

        let layout_width = floor_px(width);
        let layout_height = floor_px(height);
        // The phone's limits, overridden by the page; the page may not zoom
        // out past its own width.
        let minimum_scale = min_zoom
            .unwrap_or(DEFAULT_MINIMUM_PAGE_SCALE)
            .max(device_width / f64::from(layout_width));
        let maximum_scale = max_zoom
            .unwrap_or(DEFAULT_MAXIMUM_PAGE_SCALE)
            .max(minimum_scale);
        let page_scale = initial_zoom.map_or(minimum_scale, |zoom| {
            zoom.clamp(minimum_scale, maximum_scale)
        });
        MobileViewport {
            layout_width,
            layout_height,
            frame_width: ceil_px(device_width / minimum_scale),
            frame_height: ceil_px(device_height / minimum_scale),
            page_scale,
            visual_width: snap_px(device_width / page_scale),
            visual_height: snap_px(device_height / page_scale),
        }
    }
}

/// The description of the last document-tree `<meta name=viewport>` with a
/// `content` attribute, as Blink applies tags in order. The tag can change
/// after load (`document.write`, `page.setContent`); callers read it again
/// whenever the DOM changes.
pub(crate) fn document_viewport_description(
    dom_host: &DomHost,
    document: DomHandle,
) -> ViewportDescription {
    dom_host
        .html_elements_by_local_name_in_document_tree_order(document, "meta")
        .into_iter()
        .rev()
        .find_map(|handle| {
            let element = dom_host.node(handle).and_then(Node::as_element)?;
            if !element
                .attribute("name")
                .is_some_and(|name| name.eq_ignore_ascii_case("viewport"))
            {
                return None;
            }
            element
                .attribute("content")
                .map(ViewportDescription::from_meta_content)
        })
        .unwrap_or_else(ViewportDescription::user_agent_default)
}

/// Blink's `ParseContentAttribute` key/value tokenizer over lowercased
/// content. Reading past the end yields NUL, which is a separator.
fn for_each_key_value_pair(content: &str, mut apply: impl FnMut(&str, &str)) {
    let bytes = content.as_bytes();
    let length = bytes.len();
    let at = |index: usize| bytes.get(index).copied().unwrap_or(0);
    let mut index = 0;
    while index < length {
        while index < length && is_separator(at(index)) {
            index += 1;
        }
        let key_begin = index;
        while !is_separator(at(index)) {
            index += 1;
        }
        let key_end = index;
        // Up to the `=`, without passing a `,`.
        while index < length && at(index) != b'=' && at(index) != b',' {
            index += 1;
        }
        while index < length && is_separator(at(index)) && at(index) != b',' {
            index += 1;
        }
        let value_begin = index;
        while !is_separator(at(index)) {
            index += 1;
        }
        let value_end = index;
        apply(
            content.get(key_begin..key_end).unwrap_or_default(),
            content.get(value_begin..value_end).unwrap_or_default(),
        );
    }
}

fn is_separator(byte: u8) -> bool {
    matches!(
        byte,
        b' ' | b'\t' | b'\n' | b'\r' | b'=' | b',' | b';' | b'\0'
    )
}

/// Blink's `ParseViewportValueAsLength`: unknown and negative values are auto.
fn parse_length(value: &str) -> ViewportLength {
    match value {
        "device-width" => ViewportLength::DeviceWidth,
        "device-height" => ViewportLength::DeviceHeight,
        _ => match parse_number(value) {
            Some(number) if number >= 0.0 => {
                ViewportLength::Fixed(number.clamp(MIN_LENGTH, MAX_LENGTH))
            }
            _ => ViewportLength::Auto,
        },
    }
}

/// Blink's `ParseViewportValueAsZoom`: unknown values read as 0, then clamp.
fn parse_zoom(value: &str) -> Option<f64> {
    let number = match value {
        "yes" => 1.0,
        "no" => 0.0,
        "device-width" | "device-height" => MAX_ZOOM,
        _ => parse_number(value).unwrap_or(0.0),
    };
    (number >= 0.0).then_some(number.clamp(MIN_ZOOM, MAX_ZOOM))
}

/// Blink's `ParseViewportValueAsUserZoom`.
fn parse_user_zoom(value: &str) -> bool {
    match value {
        "yes" | "device-width" | "device-height" => true,
        "no" => false,
        _ => parse_number(value).is_some_and(|number| number.abs() >= 1.0),
    }
}

/// The leading decimal number of a value, as Blink's `CharactersToFloat`
/// reads it (`600px` is 600). `None` when the value does not start with one.
fn parse_number(value: &str) -> Option<f64> {
    let bytes = value.as_bytes();
    let mut end = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let integer_begin = end;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    let mut digits = end - integer_begin;
    if bytes.get(end) == Some(&b'.') {
        let fraction_begin = end + 1;
        let mut fraction_end = fraction_begin;
        while bytes.get(fraction_end).is_some_and(u8::is_ascii_digit) {
            fraction_end += 1;
        }
        if digits > 0 || fraction_end > fraction_begin {
            digits += fraction_end - fraction_begin;
            end = fraction_end;
        }
    }
    if digits == 0 {
        return None;
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let mut exponent_end = end + 1;
        if matches!(bytes.get(exponent_end), Some(b'+' | b'-')) {
            exponent_end += 1;
        }
        let exponent_digits_begin = exponent_end;
        while bytes.get(exponent_end).is_some_and(u8::is_ascii_digit) {
            exponent_end += 1;
        }
        if exponent_end > exponent_digits_begin {
            end = exponent_end;
        }
    }
    value
        .get(..end)?
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite())
}

/// Rounds away float noise from a value that is meant to be whole.
fn snap_px(value: f64) -> f64 {
    let rounded = value.round();
    if (value - rounded).abs() < 1e-6 {
        rounded
    } else {
        value
    }
}

fn floor_px(value: f64) -> u32 {
    snap_px(value).floor().clamp(1.0, f64::from(u32::MAX)) as u32
}

fn ceil_px(value: f64) -> u32 {
    snap_px(value).ceil().clamp(1.0, f64::from(u32::MAX)) as u32
}

/// Blink's `CompareIgnoringAuto`: the other value when either is auto.
fn compare_ignoring_auto(
    first: Option<f64>,
    second: Option<f64>,
    compare: fn(f64, f64) -> f64,
) -> Option<f64> {
    match (first, second) {
        (Some(first), Some(second)) => Some(compare(first, second)),
        (Some(first), None) => Some(first),
        (None, second) => second,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(content: Option<&str>, width: u32, height: u32) -> MobileViewport {
        content
            .map_or_else(
                ViewportDescription::user_agent_default,
                ViewportDescription::from_meta_content,
            )
            .resolve(width, height)
    }

    // 2026-11-17: pilot ignored <meta name=viewport> under phone emulation; these sizes are Chromium 1223's (headless shell) at 390x844 and 1024x1366.
    #[test]
    fn viewport_meta_tags_resolve_to_chromes_layout_and_window_sizes() {
        let cases: [(Option<&str>, (u32, u32), [u32; 4]); 16] = [
            (None, (390, 844), [980, 2120, 980, 2121]),
            (
                Some("width=device-width, initial-scale=1"),
                (390, 844),
                [390, 844, 390, 844],
            ),
            (Some("initial-scale=1"), (390, 844), [390, 844, 390, 844]),
            (Some("width=600"), (390, 844), [600, 1298, 600, 1299]),
            (Some("width=200"), (390, 844), [200, 432, 200, 433]),
            (
                Some("width=600, initial-scale=1"),
                (390, 844),
                [600, 1298, 600, 1299],
            ),
            (
                Some("width=600; initial-scale=1"),
                (390, 844),
                [600, 1298, 600, 1299],
            ),
            (Some("WIDTH = 600px"), (390, 844), [600, 1298, 600, 1299]),
            (Some("user-scalable=no"), (390, 844), [980, 2120, 980, 2121]),
            (Some("width=foo"), (390, 844), [980, 2120, 980, 2121]),
            (Some("width=20000"), (390, 844), [10000, 21641, 1560, 3376]),
            (Some("width=50"), (390, 844), [78, 168, 78, 169]),
            (
                Some("initial-scale=no"),
                (390, 844),
                [1560, 3376, 1560, 3376],
            ),
            (
                Some("width=600, initial-scale=1, user-scalable=no"),
                (390, 844),
                [600, 1298, 390, 844],
            ),
            (
                Some("height=device-height"),
                (390, 844),
                [980, 844, 980, 2121],
            ),
            (None, (1024, 1366), [1024, 1366, 1024, 1366]),
        ];
        for (content, (width, height), expected) in cases {
            let viewport = layout(content, width, height);
            assert_eq!(
                [
                    viewport.layout_width,
                    viewport.layout_height,
                    viewport.frame_width,
                    viewport.frame_height,
                ],
                expected,
                "{content:?} at {width}x{height}"
            );
        }
    }

    // 2026-11-17: visualViewport reported scale 1 under phone emulation; Chrome shows a page without a viewport meta tag at scale 390/980.
    #[test]
    fn the_visual_viewport_shows_the_layout_at_the_initial_page_scale() {
        let unset = layout(None, 390, 844);
        assert!((unset.page_scale - 390.0 / 980.0).abs() < 1e-9);
        assert_eq!(unset.visual_width, 980.0);
        assert!((unset.visual_height - 844.0 * 980.0 / 390.0).abs() < 1e-9);

        let zoomed = layout(Some("width=device-width, initial-scale=2"), 390, 844);
        assert_eq!(zoomed.page_scale, 2.0);
        assert_eq!((zoomed.visual_width, zoomed.visual_height), (195.0, 422.0));
        assert_eq!((zoomed.frame_width, zoomed.layout_width), (390, 390));

        let pinned = layout(Some("width=600, initial-scale=1"), 390, 844);
        assert_eq!(pinned.page_scale, 1.0);
        assert_eq!((pinned.visual_width, pinned.frame_width), (390.0, 600));
    }
}
