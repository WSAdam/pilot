//! Single-line labels for the window chrome (tab titles, the URL, the agent
//! badge), drawn from the system UI font's outlines. Glyphs are placed one
//! after another without shaping, which is enough for short labels.

use fontique::{
    Blob, Collection, CollectionOptions, GenericFamily, QueryFamily, QueryStatus, SourceCache,
};
use skrifa::{
    FontRef, GlyphId, MetadataProvider,
    instance::{LocationRef, Size},
    outline::{DrawSettings, OutlinePen},
};
use vello_cpu::{RenderContext, kurbo::BezPath};

const ELLIPSIS: char = '…';

pub struct TextRenderer {
    font: Blob<u8>,
    index: u32,
}

impl TextRenderer {
    /// The system UI font (or the system sans-serif), if the platform has one.
    pub fn system() -> Option<Self> {
        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: true,
        });
        let mut source_cache = SourceCache::default();
        let mut query = collection.query(&mut source_cache);
        query.set_families([
            QueryFamily::Generic(GenericFamily::SystemUi),
            QueryFamily::Generic(GenericFamily::SansSerif),
        ]);
        let mut found = None;
        query.matches_with(|font| {
            if FontRef::from_index(font.blob.as_ref(), font.index).is_ok() {
                found = Some(Self {
                    font: font.blob.clone(),
                    index: font.index,
                });
                QueryStatus::Stop
            } else {
                QueryStatus::Continue
            }
        });
        if found.is_none() {
            tracing::warn!("⚠️ no system UI font; the window chrome will have no labels");
        }
        found
    }

    fn font(&self) -> FontRef<'_> {
        FontRef::from_index(self.font.as_ref(), self.index).expect("checked when found")
    }

    /// The width of `text` at `size` pixels.
    pub fn measure(&self, text: &str, size: f32) -> f64 {
        let font = self.font();
        let charmap = font.charmap();
        let metrics = font.glyph_metrics(Size::new(size), LocationRef::default());
        text.chars()
            .map(|character| {
                let glyph = charmap.map(character).unwrap_or(GlyphId::NOTDEF);
                f64::from(metrics.advance_width(glyph).unwrap_or_default())
            })
            .sum()
    }

    /// Draws `text` with its baseline at `(x, baseline)`, cut short with an
    /// ellipsis if it is wider than `max_width`. Uses the context's paint.
    pub fn draw(
        &self,
        context: &mut RenderContext,
        text: &str,
        x: f64,
        baseline: f64,
        size: f32,
        max_width: f64,
    ) {
        let font = self.font();
        let charmap = font.charmap();
        let metrics = font.glyph_metrics(Size::new(size), LocationRef::default());
        let outlines = font.outline_glyphs();
        let ellipsis_width = self.measure(&ELLIPSIS.to_string(), size);
        let fits = self.measure(text, size) <= max_width;
        let mut pen = PathPen::default();
        let mut cursor = x;
        let mut characters: Vec<char> = text.chars().collect();
        if !fits {
            // Keep as many characters as fit before the ellipsis.
            let mut width = 0.0;
            let kept = characters
                .iter()
                .take_while(|&&character| {
                    let glyph = charmap.map(character).unwrap_or(GlyphId::NOTDEF);
                    width += f64::from(metrics.advance_width(glyph).unwrap_or_default());
                    width + ellipsis_width <= max_width
                })
                .count();
            characters.truncate(kept);
            characters.push(ELLIPSIS);
        }
        for character in characters {
            let glyph = charmap.map(character).unwrap_or(GlyphId::NOTDEF);
            if let Some(outline) = outlines.get(glyph) {
                pen.origin = (cursor, baseline);
                let settings = DrawSettings::unhinted(Size::new(size), LocationRef::default());
                if let Err(error) = outline.draw(settings, &mut pen) {
                    tracing::debug!(?error, "🔍 skipped a glyph that would not draw");
                }
            }
            cursor += f64::from(metrics.advance_width(glyph).unwrap_or_default());
        }
        context.fill_path(&pen.path);
    }

    /// Distance from the baseline to the top of capitals, for centering.
    pub fn ascent(&self, size: f32) -> f64 {
        f64::from(
            self.font()
                .metrics(Size::new(size), LocationRef::default())
                .ascent,
        )
    }
}

/// Collects glyph outlines into one path, flipping font units (y up) into
/// window pixels (y down) around `origin`.
#[derive(Default)]
struct PathPen {
    path: BezPath,
    origin: (f64, f64),
}

impl PathPen {
    fn point(&self, x: f32, y: f32) -> (f64, f64) {
        (self.origin.0 + f64::from(x), self.origin.1 - f64::from(y))
    }
}

impl OutlinePen for PathPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.path.move_to(self.point(x, y));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to(self.point(x, y));
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.path.quad_to(self.point(cx0, cy0), self.point(x, y));
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.path
            .curve_to(self.point(cx0, cy0), self.point(cx1, cy1), self.point(x, y));
    }

    fn close(&mut self) {
        self.path.close_path();
    }
}

#[cfg(test)]
mod tests {
    use vello_cpu::{Pixmap, color::palette::css};

    use super::*;

    fn inked_columns(text: &TextRenderer, label: &str, max_width: f64) -> Vec<bool> {
        let mut context = RenderContext::new(200, 24);
        context.set_paint(css::BLACK);
        text.draw(&mut context, label, 0.0, 18.0, 14.0, max_width);
        let mut pixmap = Pixmap::new(200, 24);
        context.flush();
        context.render(&mut pixmap, &mut vello_cpu::Resources::new());
        (0..200)
            .map(|x| (0..24).any(|y| pixmap.sample(x, y).a > 0))
            .collect()
    }

    #[test]
    fn labels_are_measured_drawn_and_cut_to_fit() {
        // Font discovery depends on the machine; macOS and desktop Linux
        // always have a UI font.
        let Some(text) = TextRenderer::system() else {
            eprintln!("⚠️ no system UI font on this machine; nothing to draw with");
            return;
        };
        assert_eq!(text.measure("", 14.0), 0.0);
        assert!(text.measure("pilot", 14.0) > text.measure("pi", 14.0));
        assert!(text.ascent(14.0) > 0.0);

        let full = inked_columns(&text, "pilot", 200.0);
        let last_inked = full
            .iter()
            .rposition(|&inked| inked)
            .expect("the label has ink");
        assert!(
            (last_inked as f64) < text.measure("pilot", 14.0) + 1.0,
            "ink stays inside the measured width"
        );

        let long = "https://a-very-long-address.example/with/a/long/path";
        let cut = inked_columns(&text, long, 80.0);
        let last_cut = cut
            .iter()
            .rposition(|&inked| inked)
            .expect("the cut label has ink");
        assert!(
            last_cut < 81,
            "a long label is cut to its width ({last_cut})"
        );
    }
}
