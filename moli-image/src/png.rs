use std::cell::RefCell;
use std::io::{Cursor, Write};
use std::rc::Rc;

use crate::{
    MAX_DECODED_RGBA_BYTES, MAX_ENCODED_IMAGE_BYTES, RgbaImage, RgbaImageError,
    bounded::BoundedBytes, rgba::checked_rgba_len,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PngEncodeOptions {
    /// Prefer lower CPU cost over smaller encoded output.
    pub optimize_for_speed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedPng {
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum PngEncodeError {
    #[error(transparent)]
    InvalidInput(#[from] RgbaImageError),
    #[error("PNG output contains {actual} bytes, exceeding the {max}-byte encoded image budget")]
    EncodedBufferBudgetExceeded { actual: usize, max: usize },
    #[error("PNG encoding failed: {0}")]
    Encoding(#[from] png::EncodingError),
}

#[derive(Debug, thiserror::Error)]
pub enum PngDecodeError {
    #[error("PNG decoding failed: {0}")]
    Decoding(#[from] png::DecodingError),
    #[error(transparent)]
    InvalidOutput(#[from] RgbaImageError),
    #[error("PNG decoder produced unsupported color type {color_type:?}")]
    UnsupportedColorType { color_type: png::ColorType },
}

pub fn encode_png(image: &RgbaImage) -> Result<EncodedPng, PngEncodeError> {
    encode_png_rgba8_with_options(
        image.width,
        image.height,
        &image.rgba,
        PngEncodeOptions::default(),
    )
}

pub fn encode_png_with_options(
    image: &RgbaImage,
    options: PngEncodeOptions,
) -> Result<EncodedPng, PngEncodeError> {
    encode_png_rgba8_with_options(image.width, image.height, &image.rgba, options)
}

/// Encodes a borrowed RGBA8 surface without first copying it into an owned
/// [`RgbaImage`].
pub fn encode_png_rgba8(
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<EncodedPng, PngEncodeError> {
    encode_png_rgba8_with_options(width, height, rgba, PngEncodeOptions::default())
}

pub fn encode_png_rgba8_with_options(
    width: u32,
    height: u32,
    rgba: &[u8],
    options: PngEncodeOptions,
) -> Result<EncodedPng, PngEncodeError> {
    let expected = checked_rgba_len(width, height)?;
    if rgba.len() != expected {
        return Err(PngEncodeError::InvalidInput(
            RgbaImageError::InvalidBufferLength {
                expected,
                actual: rgba.len(),
            },
        ));
    }
    let mut bytes = BoundedBytes::new(MAX_ENCODED_IMAGE_BYTES);
    let result = (|| {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        if options.optimize_for_speed {
            encoder.set_compression(png::Compression::Fast);
            encoder.set_filter(png::Filter::NoFilter);
        }
        let mut writer = encoder.write_header()?;
        writer.write_image_data(rgba)?;
        writer.finish()?;
        Ok::<_, png::EncodingError>(())
    })();
    if bytes.limit_exceeded() {
        return Err(PngEncodeError::EncodedBufferBudgetExceeded {
            actual: bytes.rejected_len().unwrap_or(usize::MAX),
            max: MAX_ENCODED_IMAGE_BYTES,
        });
    }
    result?;
    let bytes = bytes.into_inner();
    Ok(EncodedPng {
        width,
        height,
        bytes,
    })
}

/// pilot: a writer into a byte buffer the encoder's caller keeps a handle to,
/// so [`PngRowEncoder::finish`] can take the bytes back from the stream writer.
struct SharedBytes(Rc<RefCell<BoundedBytes>>);

impl Write for SharedBytes {
    fn write(&mut self, input: &[u8]) -> std::io::Result<usize> {
        self.0.borrow_mut().write(input)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// pilot: encodes an RGBA8 PNG from rows that arrive top to bottom, a band at a
/// time, so the caller never holds the whole image: a long page's full-page
/// screenshot can be taller than one raster surface and larger than the RGBA
/// budget. It decodes to the same pixels as [`encode_png_rgba8_with_options`];
/// only the encoded output is bounded, by [`MAX_ENCODED_IMAGE_BYTES`].
pub struct PngRowEncoder {
    width: u32,
    height: u32,
    rows_written: u32,
    output: Rc<RefCell<BoundedBytes>>,
    writer: png::StreamWriter<'static, SharedBytes>,
}

impl PngRowEncoder {
    pub fn new(width: u32, height: u32, options: PngEncodeOptions) -> Result<Self, PngEncodeError> {
        // One row must fit the RGBA budget; the whole image need not.
        checked_rgba_len(width, 1)?;
        let output = Rc::new(RefCell::new(BoundedBytes::new(MAX_ENCODED_IMAGE_BYTES)));
        let mut encoder = png::Encoder::new(SharedBytes(Rc::clone(&output)), width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        if options.optimize_for_speed {
            encoder.set_compression(png::Compression::Fast);
            encoder.set_filter(png::Filter::NoFilter);
        }
        let writer = encoder
            .write_header()
            .and_then(png::Writer::into_stream_writer);
        let writer = within_budget(&output, writer)?;
        Ok(Self {
            width,
            height,
            rows_written: 0,
            output,
            writer,
        })
    }

    /// Appends whole rows, `width * 4` bytes each, below the rows written so far.
    pub fn write_rows(&mut self, rgba: &[u8]) -> Result<(), PngEncodeError> {
        let row_len = self.width as usize * 4;
        let remaining_rows = (self.height - self.rows_written) as usize;
        if row_len == 0
            || !rgba.len().is_multiple_of(row_len)
            || rgba.len() / row_len > remaining_rows
        {
            return Err(PngEncodeError::InvalidInput(
                RgbaImageError::InvalidBufferLength {
                    expected: row_len.saturating_mul(remaining_rows),
                    actual: rgba.len(),
                },
            ));
        }
        let written = self
            .writer
            .write_all(rgba)
            .map_err(png::EncodingError::from);
        within_budget(&self.output, written)?;
        self.rows_written += (rgba.len() / row_len) as u32;
        Ok(())
    }

    /// Finishes the PNG once every row has been written.
    pub fn finish(self) -> Result<EncodedPng, PngEncodeError> {
        let Self {
            width,
            height,
            rows_written,
            output,
            writer,
        } = self;
        if rows_written != height {
            let row_len = width as usize * 4;
            return Err(PngEncodeError::InvalidInput(
                RgbaImageError::InvalidBufferLength {
                    expected: row_len.saturating_mul(height as usize),
                    actual: row_len.saturating_mul(rows_written as usize),
                },
            ));
        }
        within_budget(&output, writer.finish())?;
        let bytes = match Rc::try_unwrap(output) {
            Ok(cell) => cell.into_inner().into_inner(),
            Err(shared) => shared.replace(BoundedBytes::new(0)).into_inner(),
        };
        Ok(EncodedPng {
            width,
            height,
            bytes,
        })
    }
}

/// Reports a write past the encoded-bytes budget as that, not as the I/O error
/// the stream writer saw.
fn within_budget<T>(
    output: &Rc<RefCell<BoundedBytes>>,
    result: Result<T, png::EncodingError>,
) -> Result<T, PngEncodeError> {
    let bytes = output.borrow();
    if bytes.limit_exceeded() {
        return Err(PngEncodeError::EncodedBufferBudgetExceeded {
            actual: bytes.rejected_len().unwrap_or(usize::MAX),
            max: MAX_ENCODED_IMAGE_BYTES,
        });
    }
    result.map_err(PngEncodeError::from)
}

pub fn decode_png(bytes: &[u8]) -> Result<RgbaImage, PngDecodeError> {
    let mut decoder = png::Decoder::new_with_limits(
        Cursor::new(bytes),
        png::Limits {
            bytes: MAX_DECODED_RGBA_BYTES,
        },
    );
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    let (width, height) = (reader.info().width, reader.info().height);
    checked_rgba_len(width, height)?;
    let output_buffer_size = reader
        .output_buffer_size()
        .ok_or(RgbaImageError::BufferLengthOverflow { width, height })?;
    let mut decoded = vec![0; output_buffer_size];
    let output = reader.next_frame(&mut decoded)?;
    decoded.truncate(output.buffer_size());

    let rgba = match output.color_type {
        png::ColorType::Rgba => decoded,
        png::ColorType::Rgb => decoded
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255])
            .collect(),
        png::ColorType::Grayscale => decoded
            .into_iter()
            .flat_map(|value| [value, value, value, 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => decoded
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|pixel| [pixel[0], pixel[0], pixel[0], pixel[1]])
            .collect(),
        png::ColorType::Indexed => {
            return Err(PngDecodeError::UnsupportedColorType {
                color_type: output.color_type,
            });
        }
    };
    RgbaImage::try_new(output.width, output.height, rgba).map_err(Into::into)
}

pub(crate) fn png_dimensions(bytes: &[u8]) -> Result<(u32, u32), PngDecodeError> {
    let decoder = png::Decoder::new_with_limits(
        Cursor::new(bytes),
        png::Limits {
            bytes: MAX_DECODED_RGBA_BYTES,
        },
    );
    let reader = decoder.read_info()?;
    let dimensions = (reader.info().width, reader.info().height);
    checked_rgba_len(dimensions.0, dimensions.1)?;
    Ok(dimensions)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> RgbaImage {
        RgbaImage::try_new(
            2,
            2,
            vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 128, 255, 255, 255, 255,
            ],
        )
        .unwrap()
    }

    #[test]
    fn default_and_fast_png_round_trip_exact_rgba() {
        for options in [
            PngEncodeOptions::default(),
            PngEncodeOptions {
                optimize_for_speed: true,
            },
        ] {
            let encoded = encode_png_with_options(&sample(), options).unwrap();
            assert_eq!(&encoded.bytes[..8], b"\x89PNG\r\n\x1a\n");
            assert_eq!(decode_png(&encoded.bytes).unwrap(), sample());
        }
    }

    // 2026-11-12 (pilot): a full-page screenshot over the 128 MB RGBA budget
    // could not be encoded at all; rows now stream into the encoder in bands.
    #[test]
    fn rows_written_in_bands_decode_to_the_whole_image() {
        let width = 3u32;
        let height = 7u32;
        let rgba: Vec<u8> = (0..width * height * 4)
            .map(|byte| (byte * 37 % 251) as u8)
            .collect();
        for options in [
            PngEncodeOptions::default(),
            PngEncodeOptions {
                optimize_for_speed: true,
            },
        ] {
            let mut encoder = PngRowEncoder::new(width, height, options).unwrap();
            let row = width as usize * 4;
            encoder.write_rows(&rgba[..2 * row]).unwrap();
            encoder.write_rows(&rgba[2 * row..2 * row]).unwrap();
            encoder.write_rows(&rgba[2 * row..6 * row]).unwrap();
            encoder.write_rows(&rgba[6 * row..]).unwrap();
            let encoded = encoder.finish().unwrap();
            assert_eq!((encoded.width, encoded.height), (width, height));
            let whole = RgbaImage::try_new(width, height, rgba.clone()).unwrap();
            assert_eq!(decode_png(&encoded.bytes).unwrap(), whole);
        }
    }

    #[test]
    fn rows_can_make_an_image_taller_than_one_raster_surface() {
        let height = 70_000u32;
        let mut encoder = PngRowEncoder::new(1, height, PngEncodeOptions::default()).unwrap();
        for band in 0..7u8 {
            let rows = if band < 6 { 10_000 } else { height - 60_000 };
            encoder
                .write_rows(&[band, 0, 0, 255].repeat(rows as usize))
                .unwrap();
        }
        let decoded = decode_png(&encoder.finish().unwrap().bytes).unwrap();
        assert_eq!((decoded.width, decoded.height), (1, height));
        assert_eq!(&decoded.rgba[69_999 * 4..], &[6, 0, 0, 255]);
    }

    #[test]
    fn row_encoder_rejects_partial_rows_extra_rows_and_missing_rows() {
        let mut encoder = PngRowEncoder::new(2, 2, PngEncodeOptions::default()).unwrap();
        assert!(matches!(
            encoder.write_rows(&[0; 7]),
            Err(PngEncodeError::InvalidInput(
                RgbaImageError::InvalidBufferLength { .. }
            ))
        ));
        assert!(matches!(
            encoder.write_rows(&[0; 24]),
            Err(PngEncodeError::InvalidInput(
                RgbaImageError::InvalidBufferLength { .. }
            ))
        ));
        encoder.write_rows(&[0; 8]).unwrap();
        assert!(matches!(
            encoder.finish(),
            Err(PngEncodeError::InvalidInput(
                RgbaImageError::InvalidBufferLength { .. }
            ))
        ));
    }

    #[test]
    fn encode_rejects_inconsistent_rgba_metadata() {
        let image = RgbaImage {
            width: 2,
            height: 2,
            rgba: vec![0; 15],
        };
        assert!(matches!(
            encode_png(&image),
            Err(PngEncodeError::InvalidInput(
                RgbaImageError::InvalidBufferLength { .. }
            ))
        ));
    }
}
