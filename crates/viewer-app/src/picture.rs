use std::io::Cursor;

use image::{DynamicImage, GrayImage, ImageDecoder, ImageError, ImageFormat, ImageReader, Limits};

use crate::pdf::RenderRequest;

const MAX_FILE_BYTES: usize = 24 * 1024 * 1024;
const MAX_DIMENSION: u32 = 8192;
const MAX_IMAGE_PIXELS: u64 = 16 * 1024 * 1024;
const MAX_DECODED_BYTES: u64 = 64 * 1024 * 1024;
const MAX_VIEWPORT_PIXELS: u64 = 8 * 1024 * 1024;
const IMAGE_TOO_LARGE: &str = "This image is too large to open on this Kindle.";

pub struct Picture {
    gray: GrayImage,
}

impl Picture {
    pub fn open(bytes: Vec<u8>) -> Result<Self, String> {
        if bytes.len() > MAX_FILE_BYTES {
            return Err(IMAGE_TOO_LARGE.into());
        }
        let format = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            ImageFormat::Png
        } else if bytes.starts_with(b"\xff\xd8\xff") {
            ImageFormat::Jpeg
        } else {
            return Err("Choose a JPEG or PNG image.".into());
        };

        let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
        let mut limits = Limits::default();
        limits.max_image_width = Some(MAX_DIMENSION);
        limits.max_image_height = Some(MAX_DIMENSION);
        limits.max_alloc = Some(MAX_DECODED_BYTES);
        reader.limits(limits);
        let mut decoder = reader.into_decoder().map_err(decode_error)?;
        let (width, height) = decoder.dimensions();
        if width == 0 || height == 0 {
            return Err("This image is damaged or incomplete.".into());
        }
        if u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS
            || decoder.total_bytes() > MAX_DECODED_BYTES
        {
            return Err(IMAGE_TOO_LARGE.into());
        }

        let orientation = if format == ImageFormat::Jpeg {
            decoder.orientation().map_err(decode_error)?
        } else {
            image::metadata::Orientation::NoTransforms
        };
        let decoded = DynamicImage::from_decoder(decoder).map_err(decode_error)?;
        let gray = if decoded.color().has_alpha() {
            let with_alpha = decoded.into_luma_alpha8();
            GrayImage::from_fn(width, height, |x, y| {
                let [luma, alpha] = with_alpha[(x, y)].0.map(u16::from);
                let paper = (luma * alpha + 255 * (255 - alpha) + 127) / 255;
                image::Luma([paper as u8])
            })
        } else {
            decoded.into_luma8()
        };
        let mut oriented = DynamicImage::ImageLuma8(gray);
        oriented.apply_orientation(orientation);
        Ok(Self {
            gray: oriented.into_luma8(),
        })
    }

    pub fn width(&self) -> u32 {
        self.gray.width()
    }

    pub fn height(&self) -> u32 {
        self.gray.height()
    }

    pub fn render(&self, request: RenderRequest) -> Result<Vec<u8>, String> {
        let count = u64::from(request.width) * u64::from(request.height);
        let inverse_scale = request.scale.recip();
        if count == 0
            || count > MAX_VIEWPORT_PIXELS
            || !request.scale.is_finite()
            || request.scale <= 0.0
            || !inverse_scale.is_finite()
            || !request.x.is_finite()
            || !request.y.is_finite()
        {
            return Err("This view is too large to show on this Kindle.".into());
        }
        let turns = request.turns % 4;
        let (source_width, source_height) = (self.width() as f32, self.height() as f32);
        let (picture_width, picture_height) = if turns % 2 == 0 {
            (source_width, source_height)
        } else {
            (source_height, source_width)
        };

        let mut viewport = vec![255; count as usize];
        let left = request.x.floor().clamp(0.0, request.width as f32) as u32;
        let top = request.y.floor().clamp(0.0, request.height as f32) as u32;
        let right = (request.x + picture_width * request.scale)
            .ceil()
            .clamp(0.0, request.width as f32) as u32;
        let bottom = (request.y + picture_height * request.scale)
            .ceil()
            .clamp(0.0, request.height as f32) as u32;
        for y in top..bottom {
            let v = (y as f32 + 0.5 - request.y) * inverse_scale;
            if !(0.0..picture_height).contains(&v) {
                continue;
            }
            for x in left..right {
                let u = (x as f32 + 0.5 - request.x) * inverse_scale;
                if !(0.0..picture_width).contains(&u) {
                    continue;
                }
                let (source_x, source_y) = match turns {
                    0 => (u, v),
                    1 => (v, source_height - u),
                    2 => (source_width - u, source_height - v),
                    _ => (source_width - v, u),
                };
                let index = (y * request.width + x) as usize;
                viewport[index] = self.sample(source_x, source_y);
            }
        }
        Ok(viewport)
    }

    fn sample(&self, x: f32, y: f32) -> u8 {
        let x = (x - 0.5).clamp(0.0, (self.width() - 1) as f32);
        let y = (y - 0.5).clamp(0.0, (self.height() - 1) as f32);
        let left = x as u32;
        let top = y as u32;
        let right = (left + 1).min(self.width() - 1);
        let bottom = (top + 1).min(self.height() - 1);
        let across = x - left as f32;
        let down = y - top as f32;
        let pixel = |x, y| f32::from(self.gray[(x, y)].0[0]);
        let upper = pixel(left, top) * (1.0 - across) + pixel(right, top) * across;
        let lower = pixel(left, bottom) * (1.0 - across) + pixel(right, bottom) * across;
        (upper * (1.0 - down) + lower * down).round() as u8
    }
}

fn decode_error(error: ImageError) -> String {
    match error {
        ImageError::Limits(_) => IMAGE_TOO_LARGE.into(),
        ImageError::Unsupported(_) => "This image uses an unsupported format.".into(),
        _ => "This image is damaged or incomplete.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{
        codecs::jpeg::JpegEncoder, codecs::png::PngEncoder, ExtendedColorType, ImageEncoder,
    };

    fn request(width: u32, height: u32) -> RenderRequest {
        RenderRequest {
            width,
            height,
            scale: 1.0,
            x: 0.0,
            y: 0.0,
            turns: 0,
        }
    }

    #[test]
    fn png_alpha_is_composited_over_white() {
        let mut bytes = Vec::new();
        PngEncoder::new(&mut bytes)
            .write_image(
                &[0, 0, 0, 0, 0, 0, 0, 128, 0, 0, 0, 255],
                3,
                1,
                ExtendedColorType::Rgba8,
            )
            .unwrap();
        let picture = Picture::open(bytes).unwrap();
        assert_eq!(picture.render(request(3, 1)).unwrap(), [255, 127, 0]);
    }

    #[test]
    fn png_dimensions_are_bounded() {
        let mut bytes = Vec::new();
        let width = MAX_DIMENSION + 1;
        PngEncoder::new(&mut bytes)
            .write_image(&vec![0; width as usize], width, 1, ExtendedColorType::L8)
            .unwrap();
        assert_eq!(Picture::open(bytes).err().as_deref(), Some(IMAGE_TOO_LARGE));
    }

    #[test]
    fn jpeg_honors_exif_orientation() {
        fn jpeg(exif: Option<Vec<u8>>) -> Picture {
            let mut bytes = Vec::new();
            let mut encoder = JpegEncoder::new_with_quality(&mut bytes, 100);
            if let Some(exif) = exif {
                encoder.set_exif_metadata(exif).unwrap();
            }
            encoder
                .encode(&[0, 40, 80, 120, 160, 200], 2, 3, ExtendedColorType::L8)
                .unwrap();
            Picture::open(bytes).unwrap()
        }

        // A little-endian TIFF header followed by Exif orientation 6, a 90-degree turn.
        let exif =
            b"II\x2a\x00\x08\x00\x00\x00\x01\x00\x12\x01\x03\x00\x01\x00\x00\x00\x06\x00\x00\x00\x00\x00\x00\x00";
        let regular = jpeg(None);
        let rotated = jpeg(Some(exif.to_vec()));
        assert_eq!((rotated.width(), rotated.height()), (3, 2));
        let mut turn = request(3, 2);
        turn.turns = 1;
        assert_eq!(
            rotated.render(request(3, 2)).unwrap(),
            regular.render(turn).unwrap()
        );
    }

    #[test]
    fn rotated_picture_is_panned_and_clipped_to_the_viewport() {
        let picture = Picture {
            gray: GrayImage::from_raw(2, 3, vec![0, 1, 2, 3, 4, 5]).unwrap(),
        };
        let mut view = request(3, 3);
        view.turns = 1;
        view.x = -1.0;
        view.y = 1.0;
        assert_eq!(
            picture.render(view).unwrap(),
            [255, 255, 255, 2, 0, 255, 3, 1, 255]
        );
        assert!(picture.render(request(u32::MAX, u32::MAX)).is_err());
    }
}
