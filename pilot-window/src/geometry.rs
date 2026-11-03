//! Where the window draws its chrome and the page, and how a point in the
//! window maps to a CSS pixel in the page.

use serde_json::Value;

/// A rectangle in physical window pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
}

// Chrome sizes in logical pixels; the window's scale factor multiplies them.
const TAB_STRIP_HEIGHT: f64 = 30.0;
const TOOLBAR_HEIGHT: f64 = 36.0;
const BUTTON_SIZE: f64 = 28.0;
const MARGIN: f64 = 4.0;
const AGENT_BADGE_WIDTH: f64 = 116.0;
const MAX_TAB_WIDTH: f64 = 220.0;

/// The window's regions for one size and scale factor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChromeLayout {
    pub tab_strip: Rect,
    pub back: Rect,
    pub forward: Rect,
    pub reload: Rect,
    pub url_bar: Rect,
    pub agent_badge: Rect,
    /// The page area; screencast frames are letterboxed into it.
    pub content: Rect,
    scale: f64,
}

impl ChromeLayout {
    pub fn new(width: u32, height: u32, scale: f64) -> Self {
        let (width, height) = (f64::from(width), f64::from(height));
        let tab_strip = Rect::new(0.0, 0.0, width, TAB_STRIP_HEIGHT * scale);
        let toolbar_height = TOOLBAR_HEIGHT * scale;
        let (button, margin) = (BUTTON_SIZE * scale, MARGIN * scale);
        let button_y = tab_strip.bottom() + (toolbar_height - button) / 2.0;
        let back = Rect::new(margin, button_y, button, button);
        let forward = Rect::new(back.right() + margin, button_y, button, button);
        let reload = Rect::new(forward.right() + margin, button_y, button, button);
        let badge_width = AGENT_BADGE_WIDTH * scale;
        let agent_badge = Rect::new(
            (width - margin - badge_width).max(reload.right() + margin),
            button_y,
            badge_width,
            button,
        );
        let url_x = reload.right() + margin;
        let url_bar = Rect::new(
            url_x,
            button_y,
            (agent_badge.x - margin - url_x).max(0.0),
            button,
        );
        let content_top = tab_strip.bottom() + toolbar_height;
        let content = Rect::new(0.0, content_top, width, (height - content_top).max(0.0));
        Self {
            tab_strip,
            back,
            forward,
            reload,
            url_bar,
            agent_badge,
            content,
            scale,
        }
    }

    /// The `index`th of `count` tabs: equal widths, capped, left-aligned.
    pub fn tab(&self, index: usize, count: usize) -> Rect {
        let count = count.max(1) as f64;
        let width = (self.tab_strip.width / count).min(MAX_TAB_WIDTH * self.scale);
        Rect::new(
            index as f64 * width,
            self.tab_strip.y,
            width,
            self.tab_strip.height,
        )
    }

    pub fn scale(&self) -> f64 {
        self.scale
    }
}

/// Fits a `frame_width` x `frame_height` frame into `area`, keeping its
/// aspect ratio and centering it (letterboxing). The agent owns the page's
/// viewport size; the window only scales what it is sent.
pub fn letterbox(frame_width: u32, frame_height: u32, area: Rect) -> Rect {
    if frame_width == 0 || frame_height == 0 || area.width <= 0.0 || area.height <= 0.0 {
        return Rect::new(area.x, area.y, 0.0, 0.0);
    }
    let (frame_width, frame_height) = (f64::from(frame_width), f64::from(frame_height));
    let scale = (area.width / frame_width).min(area.height / frame_height);
    let (width, height) = (frame_width * scale, frame_height * scale);
    Rect::new(
        area.x + (area.width - width) / 2.0,
        area.y + (area.height - height) / 2.0,
        width,
        height,
    )
}

/// The page area a screencast frame shows, in CSS pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameMetadata {
    pub device_width: f64,
    pub device_height: f64,
}

impl FrameMetadata {
    /// Reads `Page.screencastFrame`'s `metadata`.
    pub fn from_event(metadata: &Value) -> Option<Self> {
        let device_width = metadata["deviceWidth"].as_f64()?;
        let device_height = metadata["deviceHeight"].as_f64()?;
        (device_width > 0.0 && device_height > 0.0).then_some(Self {
            device_width,
            device_height,
        })
    }
}

/// Maps a window point to the page CSS pixel under it, as
/// `Input.dispatchMouseEvent` expects, when the point is on the shown frame.
pub fn window_to_page(x: f64, y: f64, shown: Rect, frame: FrameMetadata) -> Option<(f64, f64)> {
    if !shown.contains(x, y) {
        return None;
    }
    Some((
        (x - shown.x) / shown.width * frame.device_width,
        (y - shown.y) / shown.height * frame.device_height,
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_chrome_sits_above_the_page_and_scales_with_the_window() {
        let layout = ChromeLayout::new(1280, 800, 1.0);
        assert_eq!(layout.content, Rect::new(0.0, 66.0, 1280.0, 734.0));
        assert!(layout.back.right() < layout.forward.x);
        assert!(layout.reload.right() < layout.url_bar.x);
        assert!(layout.url_bar.right() < layout.agent_badge.x);
        assert!(layout.agent_badge.right() <= 1280.0);

        let doubled = ChromeLayout::new(2560, 1600, 2.0);
        assert_eq!(doubled.content, Rect::new(0.0, 132.0, 2560.0, 1468.0));
        assert_eq!(doubled.url_bar.height, layout.url_bar.height * 2.0);
    }

    #[test]
    fn a_tiny_window_keeps_every_region_non_negative() {
        let layout = ChromeLayout::new(100, 40, 1.0);
        assert_eq!(layout.content.height, 0.0);
        assert!(layout.url_bar.width >= 0.0);
    }

    #[test]
    fn tabs_share_the_strip_up_to_a_maximum_width() {
        let layout = ChromeLayout::new(1000, 600, 1.0);
        assert_eq!(layout.tab(0, 2).width, 220.0);
        assert_eq!(layout.tab(1, 2).x, 220.0);
        assert_eq!(layout.tab(9, 10).width, 100.0);
        assert_eq!(layout.tab(0, 0).width, 220.0);
    }

    #[test]
    fn frames_are_letterboxed_and_centered() {
        let area = Rect::new(0.0, 100.0, 1000.0, 1000.0);
        assert_eq!(
            letterbox(1280, 720, area),
            Rect::new(0.0, 318.75, 1000.0, 562.5)
        );
        assert_eq!(
            letterbox(500, 1000, area),
            Rect::new(250.0, 100.0, 500.0, 1000.0)
        );
        assert_eq!(letterbox(0, 720, area).width, 0.0);
    }

    #[test]
    fn window_points_map_to_page_css_pixels() {
        let frame =
            FrameMetadata::from_event(&json!({"deviceWidth": 1280, "deviceHeight": 720})).unwrap();
        // A 1280x720 page shown at half size, 100px down the window.
        let shown = Rect::new(0.0, 100.0, 640.0, 360.0);
        assert_eq!(window_to_page(0.0, 100.0, shown, frame), Some((0.0, 0.0)));
        assert_eq!(
            window_to_page(320.0, 280.0, shown, frame),
            Some((640.0, 360.0))
        );
        assert_eq!(window_to_page(320.0, 99.0, shown, frame), None);
        assert_eq!(window_to_page(640.0, 200.0, shown, frame), None);
    }

    #[test]
    fn frame_metadata_needs_a_page_size() {
        assert_eq!(FrameMetadata::from_event(&json!({})), None);
        assert_eq!(
            FrameMetadata::from_event(&json!({"deviceWidth": 0, "deviceHeight": 720})),
            None
        );
    }
}
