//! Rasterize one visible PDF viewport without retaining a Kindle storage handle.

use hayro::hayro_interpret::util::TransformExt;
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::{LoadPdfError, Pdf as Document};
use hayro::kurbo::Affine;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::vello_cpu::{Pixmap, RasterizerSettings, RenderContext, Resources, TargetInit};
use hayro::{render_into, RenderCache, RenderSettings};
use std::fmt;
use std::panic::{catch_unwind, AssertUnwindSafe};

pub const MAX_PDF_BYTES: usize = 32 * 1024 * 1024;
const MAX_PAGES: usize = 10_000;
const MAX_VIEWPORT_PIXELS: u32 = 4 * 1024 * 1024;
const MAX_VIEWPORT_SIDE: u32 = 2048;
const MAX_PROJECTED_SIDE: f32 = 32_768.0;

pub struct Pdf {
    document: Document,
}

#[derive(Clone, Copy, Debug)]
pub struct RenderRequest {
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    /// Screen position of the rotated page's top-left corner in pixels.
    pub x: f32,
    pub y: f32,
    /// Clockwise quarter turns in addition to the PDF page's recorded orientation.
    pub turns: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PdfError {
    TooLarge,
    TooManyPages,
    Damaged,
    PasswordProtected,
    MissingPage,
    InvalidPageSize,
    InvalidViewport,
    RenderFailed,
}

impl fmt::Display for PdfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::TooLarge => "This PDF exceeds Viewer's 32 MB limit.",
            Self::TooManyPages => "This PDF has more than 10,000 pages.",
            Self::Damaged => "This PDF is damaged or has no readable pages.",
            Self::PasswordProtected => {
                "This PDF requires a password or uses unsupported encryption."
            }
            Self::MissingPage => "That page is outside this PDF.",
            Self::InvalidPageSize => "This PDF page has unsupported dimensions.",
            Self::InvalidViewport => "This PDF view is too large to draw on the Kindle.",
            Self::RenderFailed => "Viewer could not draw this PDF page.",
        })
    }
}

impl std::error::Error for PdfError {}

impl Pdf {
    pub fn open(bytes: Vec<u8>) -> Result<Self, PdfError> {
        if bytes.len() > MAX_PDF_BYTES {
            return Err(PdfError::TooLarge);
        }
        let header = &bytes[..bytes.len().min(1024)];
        if !header.windows(5).any(|word| word == b"%PDF-") {
            return Err(PdfError::Damaged);
        }

        let document = catch_unwind(|| Document::new(bytes))
            .map_err(|_| PdfError::Damaged)?
            .map_err(|error| match error {
                LoadPdfError::Decryption(_) => PdfError::PasswordProtected,
                LoadPdfError::Invalid => PdfError::Damaged,
            })?;

        let count = document.pages().len();
        if count == 0 {
            return Err(PdfError::Damaged);
        }
        if count > MAX_PAGES {
            return Err(PdfError::TooManyPages);
        }
        Ok(Self { document })
    }

    pub fn page_count(&self) -> usize {
        self.document.pages().len()
    }

    pub fn page_size(&self, page: usize) -> Result<(f32, f32), PdfError> {
        let page = self
            .document
            .pages()
            .get(page)
            .ok_or(PdfError::MissingPage)?;
        let (width, height) = page.render_dimensions();
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            return Err(PdfError::InvalidPageSize);
        }
        Ok((width, height))
    }

    /// The returned frame is row-major grayscale, with exactly one byte per viewport pixel.
    pub fn render(&self, page: usize, request: RenderRequest) -> Result<Vec<u8>, PdfError> {
        let (page_width, page_height) = self.page_size(page)?;
        let count = request
            .width
            .checked_mul(request.height)
            .ok_or(PdfError::InvalidViewport)?;
        if request.width == 0
            || request.height == 0
            || count > MAX_VIEWPORT_PIXELS
            || request.width > MAX_VIEWPORT_SIDE
            || request.height > MAX_VIEWPORT_SIDE
            || !request.scale.is_finite()
            || request.scale <= 0.0
            || page_width.max(page_height) * request.scale > MAX_PROJECTED_SIDE
            || !request.x.is_finite()
            || !request.y.is_finite()
            || request.x.abs().max(request.y.abs()) > MAX_PROJECTED_SIDE
        {
            return Err(PdfError::InvalidViewport);
        }

        let page = &self.document.pages()[page];
        let page_width = f64::from(page_width);
        let page_height = f64::from(page_height);
        let turn = match request.turns % 4 {
            0 => Affine::IDENTITY,
            1 => Affine::new([0.0, 1.0, -1.0, 0.0, page_height, 0.0]),
            2 => Affine::new([-1.0, 0.0, 0.0, -1.0, page_width, page_height]),
            _ => Affine::new([0.0, -1.0, 1.0, 0.0, 0.0, page_width]),
        };
        let transform = Affine::translate((f64::from(request.x), f64::from(request.y)))
            * Affine::scale(f64::from(request.scale))
            * turn
            * page.initial_transform(true).to_kurbo();

        catch_unwind(AssertUnwindSafe(|| {
            let mut context = RenderContext::new(request.width as u16, request.height as u16);
            let mut pixels = Pixmap::new(context.width(), context.height());
            // Per-frame resources keep page-specific glyphs and render caches from accumulating.
            render_into(
                page,
                &RenderCache::new(),
                &InterpreterSettings::default(),
                &RenderSettings::default(),
                &mut context,
                transform,
            );
            context.flush();
            context.render_with(
                &mut pixels,
                &mut Resources::default(),
                RasterizerSettings {
                    target_init: TargetInit::Clear(WHITE),
                    ..Default::default()
                },
            );
            pixels
                .data_as_u8_slice()
                .chunks_exact(4)
                .map(|rgba| {
                    // The opaque white target has already composited the document's transparency.
                    ((54 * u16::from(rgba[0])
                        + 183 * u16::from(rgba[1])
                        + 19 * u16::from(rgba[2])
                        + 128)
                        >> 8) as u8
                })
                .collect()
        }))
        .map_err(|_| PdfError::RenderFailed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn document() -> Vec<u8> {
        let content = "0 g\n10 60 20 15 re f\nBT /F1 14 Tf 55 60 Td (PDF) Tj ET\n";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Count 2 /Kids [3 0 R 4 0 R] /Resources << /Font << /F1 6 0 R >> >> >>"
                .to_owned(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 80] /Contents 5 0 R >>".to_owned(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 80] /Rotate 90 /Contents 5 0 R >>"
                .to_owned(),
            format!(
                "<< /Length {} >>\nstream\n{}endstream",
                content.len(),
                content
            ),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        ];
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            writeln!(pdf, "{} 0 obj\n{object}\nendobj", i + 1).unwrap();
        }
        let xref = pdf.len();
        writeln!(pdf, "xref\n0 {}\n0000000000 65535 f ", offsets.len() + 1).unwrap();
        for offset in offsets {
            writeln!(pdf, "{offset:010} 00000 n ").unwrap();
        }
        writeln!(
            pdf,
            "trailer\n<< /Root 1 0 R /Size {} >>\nstartxref\n{xref}\n%%EOF",
            objects.len() + 1
        )
        .unwrap();
        pdf
    }

    #[test]
    fn renders_vector_content_with_document_rotation_and_viewport_panning() {
        let pdf = Pdf::open(document()).unwrap();
        assert_eq!(pdf.page_count(), 2);
        assert_eq!(pdf.page_size(0), Ok((120.0, 80.0)));
        assert_eq!(pdf.page_size(1), Ok((80.0, 120.0)));
        let frame = pdf
            .render(
                0,
                RenderRequest {
                    width: 120,
                    height: 80,
                    scale: 1.0,
                    x: 0.0,
                    y: 0.0,
                    turns: 0,
                },
            )
            .unwrap();
        assert_eq!(frame.len(), 120 * 80);
        assert!(frame[10 * 120 + 20] < 32);
        assert_eq!(frame[40 * 120 + 40], 255);
        assert!((8..20).any(|y| frame[y * 120 + 56..y * 120 + 85]
            .iter()
            .any(|&pixel| pixel < 96)));

        let frame = pdf
            .render(
                0,
                RenderRequest {
                    width: 80,
                    height: 120,
                    scale: 1.0,
                    x: 0.0,
                    y: 0.0,
                    turns: 1,
                },
            )
            .unwrap();
        assert!(frame[20 * 80 + 70] < 32);
        assert_eq!(frame[20 * 80 + 10], 255);
        let rotated_by_document = pdf
            .render(
                1,
                RenderRequest {
                    width: 80,
                    height: 120,
                    scale: 1.0,
                    x: 0.0,
                    y: 0.0,
                    turns: 0,
                },
            )
            .unwrap();
        assert_eq!(frame, rotated_by_document);

        let frame = pdf
            .render(
                0,
                RenderRequest {
                    width: 24,
                    height: 24,
                    scale: 1.0,
                    x: -10.0,
                    y: -5.0,
                    turns: 0,
                },
            )
            .unwrap();
        assert_eq!(frame.len(), 24 * 24);
        assert!(frame[6 * 24 + 4] < 32);
        assert_eq!(frame[18 * 24 + 4], 255);
    }

    #[test]
    fn rejects_invalid_documents_and_viewports() {
        assert!(matches!(
            Pdf::open(b"%PDF-1.4 damaged".to_vec()),
            Err(PdfError::Damaged)
        ));
        let pdf = Pdf::open(document()).unwrap();
        let request = RenderRequest {
            width: 640,
            height: 800,
            scale: 1.0,
            x: 0.0,
            y: 0.0,
            turns: 0,
        };
        assert_eq!(pdf.render(2, request), Err(PdfError::MissingPage));
        assert_eq!(
            pdf.render(
                0,
                RenderRequest {
                    width: u32::MAX,
                    ..request
                }
            ),
            Err(PdfError::InvalidViewport)
        );
        assert_eq!(
            pdf.render(
                0,
                RenderRequest {
                    scale: f32::NAN,
                    ..request
                }
            ),
            Err(PdfError::InvalidViewport)
        );
    }
}
