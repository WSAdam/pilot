//! What the window shows, apart from the OS window: the tab strip and
//! toolbar, and the latest frame letterboxed into the page area. Kept apart
//! so the whole window can be drawn into any buffer, with or without a
//! window to present it in.

use vello_cpu::{
    Pixmap, RenderContext, Resources,
    color::{AlphaColor, Srgb},
    kurbo::{BezPath, Rect as Box2, RoundedRect, Shape, Stroke},
};

use crate::{
    controller::Update,
    frame::Frame,
    geometry::{ChromeLayout, FrameMetadata, Rect, letterbox},
    tabs::Tab,
    text::TextRenderer,
};

/// Around a letterboxed frame, as a softbuffer `0x00RRGGBB` word.
pub const BACKDROP: u32 = 0x202124;
const LABEL_SIZE: f32 = 13.0;

fn rgb(red: u8, green: u8, blue: u8) -> AlphaColor<Srgb> {
    AlphaColor::from_rgb8(red, green, blue)
}

#[derive(Debug, Default)]
pub struct View {
    pub tabs: Vec<Tab>,
    pub active: Option<usize>,
    pub frame: Option<(Frame, FrameMetadata)>,
    pub agent_active: bool,
    /// The URL bar's text while it has focus.
    pub url_edit: Option<String>,
}

impl View {
    pub fn apply(&mut self, update: Update) {
        match update {
            Update::Tabs { tabs, active } => {
                self.tabs = tabs;
                self.active = active;
            }
            Update::Frame { frame, metadata } => self.frame = Some((frame, metadata)),
            Update::AgentActive(active) => self.agent_active = active,
        }
    }

    pub fn active_tab(&self) -> Option<&Tab> {
        self.active.and_then(|index| self.tabs.get(index))
    }

    pub fn active_url(&self) -> String {
        self.active_tab()
            .map(|tab| tab.url.clone())
            .unwrap_or_default()
    }

    /// Where the latest frame is drawn, and the page area it shows.
    pub fn shown_frame(&self, layout: &ChromeLayout) -> Option<(Rect, FrameMetadata)> {
        let (frame, metadata) = self.frame.as_ref()?;
        Some((
            letterbox(frame.width, frame.height, layout.content),
            *metadata,
        ))
    }

    /// Draws the window into `buffer`: `width` x `height` softbuffer words,
    /// the size `layout` was made for. Labels are drawn only with `text`.
    pub fn compose(
        &self,
        layout: &ChromeLayout,
        text: Option<&TextRenderer>,
        buffer: &mut [u32],
        width: usize,
        height: usize,
    ) {
        buffer.fill(BACKDROP);
        if let (Some((frame, _)), Some((shown, _))) = (&self.frame, self.shown_frame(layout)) {
            frame.blit(buffer, width, height, shown);
        }
        if let Some(chrome) = self.paint_chrome(layout, text) {
            let chrome_width = usize::from(chrome.width());
            for (index, pixel) in chrome.data().iter().enumerate() {
                let (x, y) = (index % chrome_width, index / chrome_width);
                if x < width && y < height {
                    buffer[y * width + x] =
                        u32::from(pixel.r) << 16 | u32::from(pixel.g) << 8 | u32::from(pixel.b);
                }
            }
        }
    }

    /// The tab strip and toolbar, as an opaque pixmap as wide as the window.
    fn paint_chrome(&self, layout: &ChromeLayout, text: Option<&TextRenderer>) -> Option<Pixmap> {
        let width = u16::try_from(layout.tab_strip.width.round() as u32).ok()?;
        let height = u16::try_from(layout.content.y.round() as u32).ok()?;
        if width == 0 || height == 0 {
            return None;
        }
        let scale = layout.scale();
        let label = LABEL_SIZE * scale as f32;
        let mut context = RenderContext::new(width, height);
        context.set_paint(rgb(222, 225, 230));
        context.fill_rect(&Box2::new(0.0, 0.0, f64::from(width), f64::from(height)));
        context.set_paint(rgb(255, 255, 255));
        context.fill_rect(&Box2::new(
            0.0,
            layout.tab_strip.bottom(),
            f64::from(width),
            f64::from(height),
        ));

        for (index, tab) in self.tabs.iter().enumerate() {
            let area = layout.tab(index, self.tabs.len());
            let active = Some(index) == self.active;
            if active {
                context.set_paint(rgb(255, 255, 255));
                context.fill_path(&rounded(area, 6.0 * scale, 2.0 * scale).to_path(0.1));
            }
            if let Some(text) = text {
                let title = if tab.title.is_empty() {
                    &tab.url
                } else {
                    &tab.title
                };
                let (title, title_color) = if title.is_empty() {
                    ("New tab", rgb(95, 99, 104))
                } else if active {
                    (title.as_str(), rgb(32, 33, 36))
                } else {
                    (title.as_str(), rgb(95, 99, 104))
                };
                context.set_paint(title_color);
                let baseline = area.y + (area.height + text.ascent(label) * 0.7) / 2.0;
                text.draw(
                    &mut context,
                    title,
                    area.x + 12.0 * scale,
                    baseline,
                    label,
                    area.width - 24.0 * scale,
                );
            }
        }

        context.set_stroke(Stroke::new(2.0 * scale));
        context.set_paint(rgb(95, 99, 104));
        context.stroke_path(&chevron(layout.back, -1.0));
        context.stroke_path(&chevron(layout.forward, 1.0));
        context.stroke_path(&reload_arrow(layout.reload));

        context.set_paint(rgb(241, 243, 244));
        context.fill_path(&rounded(layout.url_bar, layout.url_bar.height / 2.0, 0.0).to_path(0.1));
        // The badge's colour says whether the agent is driving, even
        // without a font for its label.
        let (badge, badge_text, badge_color) = if self.agent_active {
            (rgb(26, 115, 232), "● Agent active", rgb(255, 255, 255))
        } else {
            (rgb(241, 243, 244), "Agent idle", rgb(95, 99, 104))
        };
        context.set_paint(badge);
        context.fill_path(
            &rounded(layout.agent_badge, layout.agent_badge.height / 2.0, 0.0).to_path(0.1),
        );
        if let Some(text) = text {
            let (shown, color) = match &self.url_edit {
                Some(edit) => (format!("{edit}|"), rgb(32, 33, 36)),
                None => (self.active_url(), rgb(60, 64, 67)),
            };
            context.set_paint(color);
            let baseline =
                layout.url_bar.y + (layout.url_bar.height + text.ascent(label) * 0.7) / 2.0;
            text.draw(
                &mut context,
                &shown,
                layout.url_bar.x + 14.0 * scale,
                baseline,
                label,
                layout.url_bar.width - 28.0 * scale,
            );

            context.set_paint(badge_color);
            let badge_baseline =
                layout.agent_badge.y + (layout.agent_badge.height + text.ascent(label) * 0.7) / 2.0;
            text.draw(
                &mut context,
                badge_text,
                layout.agent_badge.x + 12.0 * scale,
                badge_baseline,
                label,
                layout.agent_badge.width - 24.0 * scale,
            );
        }

        let mut pixmap = Pixmap::new(width, height);
        context.flush();
        context.render(&mut pixmap, &mut Resources::new());
        Some(pixmap)
    }
}

fn rounded(area: Rect, radius: f64, inset: f64) -> RoundedRect {
    RoundedRect::new(
        area.x + inset,
        area.y + inset,
        area.right() - inset,
        area.bottom() + radius,
        radius,
    )
}

/// `<` (direction -1) or `>` (direction 1) centered in `area`.
fn chevron(area: Rect, direction: f64) -> BezPath {
    let (center_x, center_y) = (area.x + area.width / 2.0, area.y + area.height / 2.0);
    let size = area.width * 0.18;
    let mut path = BezPath::new();
    path.move_to((center_x - direction * size / 2.0, center_y - size));
    path.line_to((center_x + direction * size / 2.0, center_y));
    path.line_to((center_x - direction * size / 2.0, center_y + size));
    path
}

/// An open circle with an arrowhead, centered in `area`.
fn reload_arrow(area: Rect) -> BezPath {
    let (center_x, center_y) = (area.x + area.width / 2.0, area.y + area.height / 2.0);
    let radius = area.width * 0.22;
    let mut path = vello_cpu::kurbo::Arc::new(
        (center_x, center_y),
        (radius, radius),
        -std::f64::consts::FRAC_PI_4,
        std::f64::consts::PI * 1.6,
        0.0,
    )
    .into_path(0.1);
    let tip = (center_x + radius * 0.71, center_y - radius * 0.71);
    path.move_to((tip.0 - radius * 0.5, tip.1));
    path.line_to(tip);
    path.line_to((tip.0, tip.1 + radius * 0.5));
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: u32 = 0xff0000;
    const TAB_STRIP: u32 = 0xdee1e6;
    const TOOLBAR: u32 = 0xffffff;
    const BADGE_IDLE: u32 = 0xf1f3f4;
    const BADGE_ACTIVE: u32 = 0x1a73e8;

    fn view_with_red_frame() -> View {
        View {
            tabs: vec![Tab {
                target_id: "page".to_owned(),
                title: "Hello".to_owned(),
                url: "https://example.com/".to_owned(),
            }],
            active: Some(0),
            frame: Some((
                Frame {
                    width: 4,
                    height: 2,
                    pixels: vec![RED; 8],
                },
                FrameMetadata {
                    device_width: 4.0,
                    device_height: 2.0,
                },
            )),
            ..View::default()
        }
    }

    fn compose(view: &View, layout: &ChromeLayout, text: Option<&TextRenderer>) -> Vec<u32> {
        let (width, height) = (
            layout.tab_strip.width as usize,
            layout.content.bottom() as usize,
        );
        let mut buffer = vec![0; width * height];
        view.compose(layout, text, &mut buffer, width, height);
        buffer
    }

    fn pixel(buffer: &[u32], layout: &ChromeLayout, x: f64, y: f64) -> u32 {
        buffer[y as usize * layout.tab_strip.width as usize + x as usize]
    }

    /// Pixels in `area` dark enough to be label text.
    fn dark_pixels(buffer: &[u32], layout: &ChromeLayout, area: Rect) -> usize {
        let width = layout.tab_strip.width as usize;
        (area.y as usize..area.bottom() as usize)
            .flat_map(|y| (area.x as usize..area.right() as usize).map(move |x| y * width + x))
            .filter(|&index| {
                let word = buffer[index];
                [word >> 16, word >> 8, word]
                    .iter()
                    .all(|channel| channel & 0xff < 128)
            })
            .count()
    }

    #[test]
    fn the_chrome_band_sits_above_the_letterboxed_frame() {
        let layout = ChromeLayout::new(400, 300, 1.0);
        let buffer = compose(&view_with_red_frame(), &layout, None);
        // One tab, capped in width: the strip's background shows beyond it.
        assert_eq!(pixel(&buffer, &layout, 398.0, 2.0), TAB_STRIP);
        assert_eq!(
            pixel(&buffer, &layout, 1.0, layout.tab_strip.bottom() + 1.0),
            TOOLBAR
        );
        // The 2:1 frame fills the page area's width, centered vertically.
        let (shown, _) = view_with_red_frame().shown_frame(&layout).unwrap();
        assert_eq!(shown, Rect::new(0.0, 83.0, 400.0, 200.0));
        assert_eq!(pixel(&buffer, &layout, 200.0, 183.0), RED);
        assert_eq!(pixel(&buffer, &layout, 0.0, 83.0), RED);
        assert_eq!(pixel(&buffer, &layout, 399.0, 282.0), RED);
        assert_eq!(pixel(&buffer, &layout, 200.0, 70.0), BACKDROP);
        assert_eq!(pixel(&buffer, &layout, 200.0, 290.0), BACKDROP);
        assert!(
            (0..66).all(|y| {
                let word = pixel(&buffer, &layout, 200.0, f64::from(y));
                word != RED && word != BACKDROP
            }),
            "the chrome covers the band above the page area"
        );
    }

    #[test]
    fn without_a_frame_the_page_area_is_backdrop() {
        let layout = ChromeLayout::new(400, 300, 1.0);
        let view = View {
            frame: None,
            ..view_with_red_frame()
        };
        let buffer = compose(&view, &layout, None);
        assert_eq!(pixel(&buffer, &layout, 200.0, 183.0), BACKDROP);
    }

    // 2026-11-03: the badge was filled only when a system font loaded, so a
    // window without one never showed whether the agent was driving.
    #[test]
    fn the_agent_badge_turns_blue_while_the_agent_is_active_even_without_a_font() {
        let layout = ChromeLayout::new(800, 300, 1.0);
        let badge = |buffer: &[u32]| {
            pixel(
                buffer,
                &layout,
                layout.agent_badge.x + 4.0,
                layout.agent_badge.y + layout.agent_badge.height / 2.0,
            )
        };
        let mut view = view_with_red_frame();
        assert_eq!(badge(&compose(&view, &layout, None)), BADGE_IDLE);
        view.apply(Update::AgentActive(true));
        assert_eq!(badge(&compose(&view, &layout, None)), BADGE_ACTIVE);
        view.apply(Update::AgentActive(false));
        assert_eq!(badge(&compose(&view, &layout, None)), BADGE_IDLE);
    }

    #[test]
    fn the_tab_title_and_the_url_are_drawn_in_the_chrome() {
        let Some(text) = TextRenderer::system() else {
            if cfg!(target_os = "macos") {
                panic!("macOS always has a system UI font");
            }
            eprintln!("⚠️ no system UI font here; labels are not drawn");
            return;
        };
        let layout = ChromeLayout::new(800, 300, 1.0);
        let view = view_with_red_frame();
        let labelled = compose(&view, &layout, Some(&text));
        let bare = compose(&view, &layout, None);
        let tab = layout.tab(0, 1);
        assert!(
            dark_pixels(&labelled, &layout, tab) > 20,
            "the title is drawn"
        );
        assert_eq!(dark_pixels(&bare, &layout, tab), 0);
        assert!(
            dark_pixels(&labelled, &layout, layout.url_bar) > 20,
            "the URL is drawn"
        );
    }

    #[test]
    fn updates_replace_what_the_view_shows() {
        let mut view = View::default();
        assert_eq!(view.active_url(), "");
        let tabs = view_with_red_frame().tabs;
        view.apply(Update::Tabs {
            tabs: tabs.clone(),
            active: Some(0),
        });
        assert_eq!(view.active_tab(), tabs.first());
        assert_eq!(view.active_url(), "https://example.com/");
        let (frame, metadata) = view_with_red_frame().frame.unwrap();
        view.apply(Update::Frame {
            frame: frame.clone(),
            metadata,
        });
        assert_eq!(view.frame, Some((frame, metadata)));
    }
}
