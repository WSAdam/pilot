// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Narrowly adapted from DioxusLabs/blitz commit
// d788124ab881f9bb537cb452ec1d837604a374a8:
// - examples/screenshot.rs
// - packages/blitz-paint/src/render.rs
// - crates/anyrender_svg (parsed SVG tree execution)
// The source project is licensed MIT OR Apache-2.0.

//! Vello CPU pixel rendering, RGBA readback and bounded software-filter surfaces.

mod filters;
#[cfg(test)]
mod tests;

use crate::geometry::{device_transform, to_backend_shape};
use crate::scene::{paint_fragment, sanitize_unit, to_backend_blend_mode, to_backend_color};
use crate::validation::{PaintStreamMetrics, validate_fragment_stream};
use crate::{PaintError, RasterImage};
use anyrender::ImageRenderer;
use anyrender::PaintScene;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use filters::{apply_software_color_filter, is_software_color_filter};
use moli_layout::{PaintCaptureSurface, PaintColor, PaintFragment, PaintSnapshot};
use peniko::kurbo::{Affine, Rect};
use peniko::{
    Blob, Extend, Fill, ImageAlphaType, ImageBrush, ImageData, ImageFormat, ImageQuality,
    ImageSampler,
};
use std::sync::Arc;

/// Maximum estimated RGBA bytes simultaneously owned by the output and nested
/// filter surfaces in one raster call.
pub const MAX_TRANSIENT_RASTER_BYTES: usize = 256 * 1024 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RasterDimensions {
    width: u32,
    height: u32,
    byte_len: usize,
}

/// Rasterizes an owned snapshot to row-major, straight-alpha RGBA8 pixels with Vello CPU.
pub fn raster_snapshot(snapshot: &PaintSnapshot) -> Result<RasterImage, PaintError> {
    let dimensions = validate_dimensions(snapshot.surface)?;
    let stream = validate_fragment_stream(snapshot)?;
    validate_resource_budget(dimensions, stream)?;
    let scale = f64::from(snapshot.surface.device_scale);

    let mut renderer = VelloCpuImageRenderer::new(dimensions.width, dimensions.height);
    let mut rgba = Vec::new();
    renderer.render_to_vec(
        |scene| {
            paint_canvas(scene, snapshot.canvas_color, dimensions);
            paint_fragment_stream(
                scene,
                snapshot,
                &snapshot.fragments,
                scale,
                dimensions,
                Affine::IDENTITY,
            );
        },
        &mut rgba,
    );

    if rgba.len() != dimensions.byte_len {
        return Err(PaintError::UnexpectedBufferLength {
            expected: dimensions.byte_len,
            actual: rgba.len(),
        });
    }
    unpremultiply_rgba(&mut rgba);

    Ok(RasterImage {
        width: dimensions.width,
        height: dimensions.height,
        rgba,
    })
}

/// pilot: the most rows one band of a banded raster holds.
pub const MAX_RASTER_BAND_ROWS: u32 = 8192;
/// pilot: the tallest raster drawn in bands, in device pixels (16 times one
/// surface's limit).
pub const MAX_BANDED_RASTER_HEIGHT: u32 = 1 << 20;

/// pilot: rows `top..top + image.height` of a raster drawn in bands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RasterBand {
    /// The band's first row in the whole raster.
    pub top: u32,
    /// The band's pixels: the whole raster's width, `image.height` rows.
    pub image: RasterImage,
}

/// pilot: whether [`raster_snapshot`] can draw this snapshot in one surface: at
/// most 65,535 device pixels a side, within the transient byte budget. A
/// snapshot that doesn't fit, such as a long page's full-page screenshot, can
/// still be drawn with [`raster_snapshot_in_bands`].
pub fn raster_fits_one_surface(snapshot: &PaintSnapshot) -> bool {
    let Ok(dimensions) = validate_dimensions(snapshot.surface) else {
        return false;
    };
    validate_fragment_stream(snapshot)
        .is_ok_and(|stream| validate_resource_budget(dimensions, stream).is_ok())
}

/// pilot: the device-pixel width and height a snapshot rasterizes to, checked
/// against the banded limits ([`MAX_BANDED_RASTER_HEIGHT`] rows).
pub fn raster_device_size(snapshot: &PaintSnapshot) -> Result<(u32, u32), PaintError> {
    validate_dimensions_up_to(snapshot.surface, MAX_BANDED_RASTER_HEIGHT)
        .map(|dimensions| (dimensions.width, dimensions.height))
}

/// pilot: draws a snapshot in horizontal bands, top to bottom, and hands each
/// band to `sink` as soon as it's drawn, so only one band's pixels are held at
/// a time. Each band is at most [`MAX_RASTER_BAND_ROWS`] rows and fits the
/// transient byte budget; the whole raster may be up to
/// [`MAX_BANDED_RASTER_HEIGHT`] rows. Returns the whole raster's width and
/// height. The bands' rows, stacked, are the pixels [`raster_snapshot`] would
/// draw for a snapshot that fits one surface.
pub fn raster_snapshot_in_bands<E: From<PaintError>>(
    snapshot: &PaintSnapshot,
    mut sink: impl FnMut(RasterBand) -> Result<(), E>,
) -> Result<(u32, u32), E> {
    let whole = validate_dimensions_up_to(snapshot.surface, MAX_BANDED_RASTER_HEIGHT)?;
    let stream = validate_fragment_stream(snapshot)?;
    let surface_count = stream.max_filter_depth.saturating_add(1);
    let row_bytes = usize::try_from(whole.width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .and_then(|bytes| bytes.checked_mul(surface_count))
        .ok_or(PaintError::BufferLengthOverflow {
            width: whole.width,
            height: whole.height,
        })?;
    let budget_rows =
        u32::try_from(MAX_TRANSIENT_RASTER_BYTES / row_bytes.max(1)).unwrap_or(u32::MAX);
    let band_rows = budget_rows.clamp(1, MAX_RASTER_BAND_ROWS);
    let scale = f64::from(snapshot.surface.device_scale);

    let mut top = 0u32;
    while top < whole.height {
        let rows = band_rows.min(whole.height - top);
        let band = RasterDimensions {
            width: whole.width,
            height: rows,
            byte_len: usize::try_from(u64::from(whole.width) * u64::from(rows) * 4).map_err(
                |_| PaintError::BufferLengthOverflow {
                    width: whole.width,
                    height: rows,
                },
            )?,
        };
        validate_resource_budget(band, stream)?;
        let origin = Affine::translate((0.0, -f64::from(top)));
        let mut renderer = VelloCpuImageRenderer::new(band.width, band.height);
        let mut rgba = Vec::new();
        renderer.render_to_vec(
            |scene| {
                paint_canvas(scene, snapshot.canvas_color, band);
                paint_fragment_stream(scene, snapshot, &snapshot.fragments, scale, band, origin);
            },
            &mut rgba,
        );
        if rgba.len() != band.byte_len {
            return Err(PaintError::UnexpectedBufferLength {
                expected: band.byte_len,
                actual: rgba.len(),
            }
            .into());
        }
        unpremultiply_rgba(&mut rgba);
        sink(RasterBand {
            top,
            image: RasterImage {
                width: band.width,
                height: band.height,
                rgba,
            },
        })?;
        top += rows;
    }
    Ok((whole.width, whole.height))
}

/// pilot: a scene that moves everything it draws by `origin`, so one band of
/// a tall raster can be painted from the whole page's coordinates. Brush and
/// glyph transforms are relative to the draw transform, so only that moves.
struct OffsetScene<'s, S: PaintScene> {
    inner: &'s mut S,
    origin: Affine,
}

impl<S: PaintScene> anyrender::RenderContext for OffsetScene<'_, S> {
    fn try_register_custom_resource(
        &mut self,
        resource: Box<dyn std::any::Any>,
    ) -> Result<anyrender::ResourceId, anyrender::RegisterResourceError> {
        self.inner.try_register_custom_resource(resource)
    }

    fn unregister_resource(&mut self, resource_id: anyrender::ResourceId) {
        self.inner.unregister_resource(resource_id);
    }

    fn renderer_specific_context(&self) -> Option<Box<dyn std::any::Any>> {
        self.inner.renderer_specific_context()
    }
}

impl<S: PaintScene> PaintScene for OffsetScene<'_, S> {
    fn reset(&mut self) {
        self.inner.reset();
    }

    fn push_layer(
        &mut self,
        blend: impl Into<peniko::BlendMode>,
        alpha: f32,
        transform: Affine,
        clip: &impl peniko::kurbo::Shape,
        filter: Option<Arc<anyrender::Filter>>,
        backdrop_filter: Option<Arc<anyrender::Filter>>,
    ) {
        self.inner.push_layer(
            blend,
            alpha,
            self.origin * transform,
            clip,
            filter,
            backdrop_filter,
        );
    }

    fn push_clip_layer(&mut self, transform: Affine, clip: &impl peniko::kurbo::Shape) {
        self.inner.push_clip_layer(self.origin * transform, clip);
    }

    fn pop_layer(&mut self) {
        self.inner.pop_layer();
    }

    fn stroke<'a>(
        &mut self,
        style: &peniko::kurbo::Stroke,
        transform: Affine,
        brush: impl Into<anyrender::PaintRef<'a>>,
        brush_transform: Option<Affine>,
        shape: &impl peniko::kurbo::Shape,
    ) {
        self.inner.stroke(
            style,
            self.origin * transform,
            brush,
            brush_transform,
            shape,
        );
    }

    fn fill<'a>(
        &mut self,
        style: Fill,
        transform: Affine,
        brush: impl Into<anyrender::PaintRef<'a>>,
        brush_transform: Option<Affine>,
        shape: &impl peniko::kurbo::Shape,
    ) {
        self.inner.fill(
            style,
            self.origin * transform,
            brush,
            brush_transform,
            shape,
        );
    }

    fn draw_glyphs<'a, 's: 'a>(
        &'s mut self,
        font: &'a peniko::FontData,
        font_size: f32,
        hint: bool,
        normalized_coords: &'a [anyrender::NormalizedCoord],
        embolden: peniko::kurbo::Vec2,
        style: impl Into<peniko::StyleRef<'a>>,
        brush: impl Into<anyrender::PaintRef<'a>>,
        brush_alpha: f32,
        transform: Affine,
        glyph_transform: Option<Affine>,
        glyphs: impl Iterator<Item = anyrender::Glyph> + Clone,
    ) {
        let transform = self.origin * transform;
        self.inner.draw_glyphs(
            font,
            font_size,
            hint,
            normalized_coords,
            embolden,
            style,
            brush,
            brush_alpha,
            transform,
            glyph_transform,
            glyphs,
        );
    }

    fn draw_box_shadow(
        &mut self,
        transform: Affine,
        rect: Rect,
        brush: peniko::Color,
        radius: f64,
        std_dev: f64,
    ) {
        self.inner
            .draw_box_shadow(self.origin * transform, rect, brush, radius, std_dev);
    }
}

// Vello's Pixmap stores premultiplied channels; RasterImage and our software
// color filters consume straight-alpha RGBA. Convert at each backend readback.
fn unpremultiply_rgba(rgba: &mut [u8]) {
    for pixel in rgba.as_chunks_mut::<4>().0 {
        let alpha = u16::from(pixel[3]);
        if alpha == 0 {
            pixel[..3].fill(0);
        } else if alpha != 255 {
            for channel in &mut pixel[..3] {
                *channel = ((u16::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
}

fn validate_resource_budget(
    dimensions: RasterDimensions,
    stream: PaintStreamMetrics,
) -> Result<(), PaintError> {
    // Backend filters may crop allocations to content bounds, whereas the
    // software color-filter fallback uses a complete surface. Charging every
    // nested filter as a complete surface is conservative and backend-neutral.
    let surface_count = stream.max_filter_depth.saturating_add(1);
    let required_bytes = dimensions.byte_len.saturating_mul(surface_count);
    if required_bytes > MAX_TRANSIENT_RASTER_BYTES {
        return Err(PaintError::TransientRasterBudgetExceeded {
            required_bytes,
            max_bytes: MAX_TRANSIENT_RASTER_BYTES,
        });
    }
    Ok(())
}

fn validate_dimensions(surface: PaintCaptureSurface) -> Result<RasterDimensions, PaintError> {
    validate_dimensions_up_to(surface, u16::MAX as u32)
}

/// pilot: checks a capture surface's device-pixel size, allowing heights up to
/// `max_height` (one surface's 65,535, or the banded limit).
fn validate_dimensions_up_to(
    surface: PaintCaptureSurface,
    max_height: u32,
) -> Result<RasterDimensions, PaintError> {
    if !surface.css_width.is_finite()
        || !surface.css_height.is_finite()
        || surface.css_width <= 0.0
        || surface.css_height <= 0.0
    {
        return Err(PaintError::InvalidCaptureSurfaceDimensions {
            width: surface.css_width,
            height: surface.css_height,
        });
    }

    let scale = surface.device_scale;
    if !scale.is_finite() || scale <= 0.0 {
        return Err(PaintError::InvalidCaptureDeviceScale {
            device_scale: scale,
        });
    }

    let width = (f64::from(surface.css_width) * f64::from(scale)).ceil();
    let height = (f64::from(surface.css_height) * f64::from(scale)).ceil();
    const MAX_DIMENSION: u32 = u16::MAX as u32;
    if width > f64::from(MAX_DIMENSION) || height > f64::from(max_height) {
        return Err(PaintError::BackendDimensionLimit {
            width,
            height,
            max_dimension: if width > f64::from(MAX_DIMENSION) {
                MAX_DIMENSION
            } else {
                max_height
            },
        });
    }

    let width = width as u32;
    let height = height as u32;
    let byte_len = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(PaintError::BufferLengthOverflow { width, height })?;

    Ok(RasterDimensions {
        width,
        height,
        byte_len,
    })
}

fn paint_canvas(
    scene: &mut impl PaintScene,
    canvas_color: PaintColor,
    dimensions: RasterDimensions,
) {
    let canvas = Rect::new(
        0.0,
        0.0,
        f64::from(dimensions.width),
        f64::from(dimensions.height),
    );
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        to_backend_color(canvas_color),
        None,
        &canvas,
    );
}

fn paint_fragment_stream(
    scene: &mut impl PaintScene,
    snapshot: &PaintSnapshot,
    fragments: &[PaintFragment],
    scale: f64,
    dimensions: RasterDimensions,
    // pilot: where this surface sits in the whole raster; IDENTITY except for
    // the bands of a banded raster.
    origin: Affine,
) {
    let mut index = 0usize;
    while index < fragments.len() {
        let fragment = &fragments[index];
        let isolated_filter = match fragment {
            PaintFragment::PushLayer {
                filter: Some(filter),
                ..
            } if is_software_color_filter(filter) => Some(filter),
            _ => None,
        };
        let Some(filter) = isolated_filter else {
            if origin == Affine::IDENTITY {
                paint_fragment(scene, snapshot, fragment, scale);
            } else {
                paint_fragment(
                    &mut OffsetScene {
                        inner: &mut *scene,
                        origin,
                    },
                    snapshot,
                    fragment,
                    scale,
                );
            }
            index += 1;
            continue;
        };

        let end = matching_layer_pop(fragments, index);
        let PaintFragment::PushLayer {
            opacity,
            blend_mode,
            composite,
            clip,
            transform,
            ..
        } = fragment
        else {
            unreachable!("isolated filters are represented by PushLayer")
        };

        // Resolve a color-filter subtree into a temporary transparent image,
        // transform its pixels once, then composite it with the layer's normal
        // clip/opacity/blend contract. This surface belongs only to this raster
        // call and is never cached by screenshot or screencast.
        let mut filtered_rgba = Vec::new();
        let mut renderer = VelloCpuImageRenderer::new(dimensions.width, dimensions.height);
        renderer.render_to_vec(
            |filtered_scene| {
                paint_fragment_stream(
                    filtered_scene,
                    snapshot,
                    &fragments[index + 1..end],
                    scale,
                    dimensions,
                    origin,
                );
            },
            &mut filtered_rgba,
        );
        unpremultiply_rgba(&mut filtered_rgba);
        apply_software_color_filter(&mut filtered_rgba, filter);

        // The filtered image is already in this surface's coordinates and is
        // drawn at IDENTITY below; only the layer's clip moves with the band.
        let clip = to_backend_shape(clip);
        scene.push_layer(
            to_backend_blend_mode(*blend_mode, *composite),
            sanitize_unit(*opacity),
            origin * device_transform(*transform, scale),
            &clip,
            None,
            None,
        );
        let image = ImageBrush {
            image: ImageData {
                data: Blob::new(Arc::new(filtered_rgba)),
                format: ImageFormat::Rgba8,
                alpha_type: ImageAlphaType::Alpha,
                width: dimensions.width,
                height: dimensions.height,
            },
            sampler: ImageSampler {
                x_extend: Extend::Pad,
                y_extend: Extend::Pad,
                quality: ImageQuality::Low,
                alpha: 1.0,
            },
        };
        scene.draw_image(image.as_ref(), Affine::IDENTITY);
        scene.pop_layer();
        index = end + 1;
    }
}

fn matching_layer_pop(fragments: &[PaintFragment], push_index: usize) -> usize {
    let mut depth = 0usize;
    for (index, fragment) in fragments.iter().enumerate().skip(push_index) {
        match fragment {
            PaintFragment::PushLayer { .. } | PaintFragment::PushClip { .. } => depth += 1,
            PaintFragment::PopLayer => {
                depth -= 1;
                if depth == 0 {
                    return index;
                }
            }
            _ => {}
        }
    }
    unreachable!("paint stream validation rejects unclosed layers")
}
