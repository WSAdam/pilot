//! Screencast frames: decoded once, then scaled into the window's buffer.

use anyhow::{Context, Result};
use base64::Engine;

use crate::geometry::Rect;

/// A decoded frame, one `0x00RRGGBB` word per pixel (softbuffer's format).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u32>,
}

impl Frame {
    /// Decodes `Page.screencastFrame`'s base64 `data` (JPEG or PNG).
    pub fn decode(data: &str) -> Result<Self> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .context("screencast frame is not base64")?;
        let image = image::load_from_memory(&bytes)
            .context("screencast frame is not a JPEG or PNG image")?
            .into_rgb8();
        let (width, height) = image.dimensions();
        let pixels = image
            .pixels()
            .map(|pixel| {
                let [red, green, blue] = pixel.0;
                u32::from(red) << 16 | u32::from(green) << 8 | u32::from(blue)
            })
            .collect();
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    /// Scales the frame (nearest neighbour) into `target` of a
    /// `buffer_width` x `buffer_height` buffer, clipped to the buffer.
    pub fn blit(
        &self,
        buffer: &mut [u32],
        buffer_width: usize,
        buffer_height: usize,
        target: Rect,
    ) {
        if self.width == 0 || self.height == 0 || target.width <= 0.0 || target.height <= 0.0 {
            return;
        }
        let left = target.x.max(0.0).floor() as usize;
        let top = target.y.max(0.0).floor() as usize;
        let right = (target.right().ceil().max(0.0) as usize).min(buffer_width);
        let bottom = (target.bottom().ceil().max(0.0) as usize).min(buffer_height);
        let x_scale = f64::from(self.width) / target.width;
        let y_scale = f64::from(self.height) / target.height;
        for y in top..bottom {
            let source_y =
                (((y as f64 + 0.5 - target.y) * y_scale) as usize).min(self.height as usize - 1);
            let source_row = &self.pixels[source_y * self.width as usize..][..self.width as usize];
            let row = &mut buffer[y * buffer_width..][..buffer_width];
            for (x, pixel) in row.iter_mut().enumerate().take(right).skip(left) {
                let source_x =
                    (((x as f64 + 0.5 - target.x) * x_scale) as usize).min(self.width as usize - 1);
                *pixel = source_row[source_x];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    const RED: u32 = 0xff0000;
    const GREEN: u32 = 0x00ff00;
    const BLUE: u32 = 0x0000ff;
    const WHITE: u32 = 0xffffff;

    fn png_base64() -> String {
        // 2x2: red green / blue white.
        let image =
            image::RgbImage::from_raw(2, 2, vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255])
                .unwrap();
        let mut png = Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png).unwrap();
        base64::engine::general_purpose::STANDARD.encode(png.into_inner())
    }

    #[test]
    fn frames_decode_to_softbuffer_pixels() {
        let frame = Frame::decode(&png_base64()).unwrap();
        assert_eq!((frame.width, frame.height), (2, 2));
        assert_eq!(frame.pixels, [RED, GREEN, BLUE, WHITE]);
        assert!(Frame::decode("not base64!").is_err());
        assert!(Frame::decode("aGVsbG8=").is_err());
    }

    #[test]
    fn blitting_scales_into_the_target_and_leaves_the_rest() {
        let frame = Frame::decode(&png_base64()).unwrap();
        let mut buffer = vec![0; 6 * 4];
        frame.blit(&mut buffer, 6, 4, Rect::new(1.0, 0.0, 4.0, 4.0));
        #[rustfmt::skip]
        assert_eq!(buffer, [
            0, RED, RED, GREEN, GREEN, 0,
            0, RED, RED, GREEN, GREEN, 0,
            0, BLUE, BLUE, WHITE, WHITE, 0,
            0, BLUE, BLUE, WHITE, WHITE, 0,
        ]);
    }

    #[test]
    fn blitting_clips_to_the_buffer() {
        let frame = Frame::decode(&png_base64()).unwrap();
        let mut buffer = vec![0; 2 * 2];
        frame.blit(&mut buffer, 2, 2, Rect::new(-2.0, -2.0, 4.0, 4.0));
        assert_eq!(buffer, [WHITE, WHITE, WHITE, WHITE]);
        frame.blit(&mut buffer, 2, 2, Rect::new(5.0, 5.0, 4.0, 4.0));
        frame.blit(&mut buffer, 2, 2, Rect::new(0.0, 0.0, 0.0, 4.0));
        assert_eq!(buffer, [WHITE, WHITE, WHITE, WHITE]);
    }
}
