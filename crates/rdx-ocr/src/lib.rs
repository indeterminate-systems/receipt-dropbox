//! OCR fallback: the text of a scanned PDF or a photo, in the same shape as a PDF's text layer.
//!
//! Most statements and receipts have a text layer, which [`rdx_pdf`] reads. A scan or a phone
//! photo has none: [`read`] then finds the words with [ocrs](https://github.com/robertknight/ocrs),
//! a pure-Rust OCR engine, and groups them into [`rdx_pdf::Line`]s and [`rdx_pdf::Span`]s with
//! positions in points, so the statement parsers and receipt extractors read a scan exactly as
//! they read a text PDF. Like the rest of the core it does no I/O: the two models (about 12 MB)
//! come from the caller, which in a plan means the host's blob store (see [`op`]).
//!
//! OCR misreads. Separators in amounts are repaired only where the value is certain (see
//! [`Reading::warnings`]); anything else is left for the parsers' checks to catch: a statement
//! whose lines don't add up is refused, and a receipt missing a field needs review.

mod layout;
pub mod op;
mod repair;

use std::fmt;

use rdx_pdf::{ImageData, Page, Text};
use serde::{Deserialize, Serialize};

pub use op::{OcrRead, register};

use layout::{Frame, Word};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OcrError {
    #[error(transparent)]
    Pdf(#[from] rdx_pdf::PdfError),
    #[error("OCR models: {0}")]
    Models(String),
    #[error("image: {0}")]
    Image(String),
    #[error("OCR engine: {0}")]
    Engine(String),
}

/// The two ocrs models, as `.rten` files.
pub struct Models {
    /// `text-detection.rten`: finds words.
    pub detection: Vec<u8>,
    /// `text-recognition.rten`: reads them.
    pub recognition: Vec<u8>,
}

/// Where a [`Reading`]'s text came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    /// The PDF's own text layer; no OCR was needed.
    TextLayer,
    /// OCR of the PDF's page images or of the image itself.
    Ocr,
    /// Not a PDF or an image (a CSV, OFX or email): nothing for OCR to do.
    None,
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Source::TextLayer => "text-layer",
            Source::Ocr => "ocr",
            Source::None => "none",
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub text: Text,
    pub source: Source,
    /// What a person checking the result should know: repaired amounts, images that couldn't
    /// be read, pages with no text.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    /// Resolution of a photo or image file, for positions in points: phones and scanners don't
    /// reliably record it. PDF pages state their own size.
    pub ppi: f64,
}

impl Default for Options {
    fn default() -> Self {
        Options { ppi: 200.0 }
    }
}

/// The kinds of document [`read`] tells apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Pdf,
    Png,
    Jpeg,
    Other,
}

pub fn kind(bytes: &[u8]) -> Kind {
    if rdx_pdf::is_pdf(bytes) {
        Kind::Pdf
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Kind::Png
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Kind::Jpeg
    } else {
        Kind::Other
    }
}

/// Reads a document's text: a PDF's text layer when it has one, OCR of its page images when it
/// doesn't, OCR of a PNG or JPEG. `models` is called only if OCR is needed, so a text PDF never
/// loads them.
pub fn read(
    bytes: &[u8],
    options: &Options,
    models: impl FnOnce() -> Result<Models, OcrError>,
) -> Result<Reading, OcrError> {
    let mut warnings = Vec::new();
    match kind(bytes) {
        Kind::Other => {
            warnings.push("not a PDF or an image, so there is nothing to read with OCR".into());
            Ok(Reading {
                text: Text { pages: vec![] },
                source: Source::None,
                warnings,
            })
        }
        Kind::Pdf => {
            let text = rdx_pdf::text(bytes)?;
            if !text.is_empty() {
                return Ok(Reading {
                    text,
                    source: Source::TextLayer,
                    warnings,
                });
            }
            let pages = rdx_pdf::page_images(bytes)?;
            let engine = Engine::new(models()?)?;
            let mut out = Vec::new();
            for (n, page) in pages.iter().enumerate() {
                let mut lines = Vec::new();
                for img in &page.images {
                    let gray = match gray(&img.data, img.pixels_wide, img.pixels_high) {
                        Ok(g) => g,
                        Err(why) => {
                            warnings.push(format!("page {}: skipped an image: {why}", n + 1));
                            continue;
                        }
                    };
                    let frame = Frame {
                        x: img.x,
                        top: img.y + img.height,
                        sx: img.width / f64::from(gray.width()),
                        sy: img.height / f64::from(gray.height()),
                    };
                    lines.extend(layout::lines(engine.words(&gray)?, frame));
                }
                // Images may overlap or stack; keep the page reading top to bottom.
                lines.sort_by(|a, b| b.y.total_cmp(&a.y));
                if lines.is_empty() {
                    warnings.push(format!("page {}: no text found", n + 1));
                }
                out.push(Page { lines });
            }
            Ok(finish(out, warnings))
        }
        Kind::Png | Kind::Jpeg => {
            let gray = image::load_from_memory(bytes)
                .map_err(|e| OcrError::Image(e.to_string()))?
                .into_luma8();
            let engine = Engine::new(models()?)?;
            let scale = 72.0 / options.ppi;
            let frame = Frame {
                x: 0.0,
                top: f64::from(gray.height()) * scale,
                sx: scale,
                sy: scale,
            };
            let lines = layout::lines(engine.words(&gray)?, frame);
            if lines.is_empty() {
                warnings.push("no text found in the image".into());
            }
            Ok(finish(vec![Page { lines }], warnings))
        }
    }
}

fn finish(mut pages: Vec<Page>, mut warnings: Vec<String>) -> Reading {
    let mut fix = |pages: &mut Vec<Page>, f: &dyn Fn(&str) -> (String, Vec<(String, String)>)| {
        for span in pages
            .iter_mut()
            .flat_map(|p| p.lines.iter_mut())
            .flat_map(|l| l.spans.iter_mut())
        {
            let (text, fixes) = f(&span.text);
            for (read, written) in fixes {
                warnings.push(format!("read `{read}` as `{written}`"));
            }
            span.text = text;
        }
    };
    fix(&mut pages, &repair::amounts);
    let convention = repair::decimal_convention(
        pages
            .iter()
            .flat_map(|p| p.lines.iter())
            .flat_map(|l| l.spans.iter())
            .map(|s| s.text.as_str()),
    );
    if let Some(decimal) = convention {
        fix(&mut pages, &|t| repair::to_convention(t, decimal));
    }
    Reading {
        text: Text { pages },
        source: Source::Ocr,
        warnings,
    }
}

fn gray(data: &ImageData, w: u32, h: u32) -> Result<image::GrayImage, String> {
    let sized = |px: Vec<u8>| {
        image::GrayImage::from_raw(w, h, px).ok_or_else(|| "pixel data doesn't fit its size".into())
    };
    match data {
        ImageData::Jpeg(j) => image::load_from_memory_with_format(j, image::ImageFormat::Jpeg)
            .map(|i| i.into_luma8())
            .map_err(|e| format!("JPEG: {e}")),
        ImageData::Gray8(px) => sized(px.clone()),
        ImageData::Rgb8(px) => sized(
            px.chunks_exact(3)
                .map(|c| luma(c[0], c[1], c[2]))
                .collect(),
        ),
        ImageData::Cmyk8(px) => sized(
            px.chunks_exact(4)
                .map(|c| {
                    let ink = |v: u8| 255 - v.saturating_add(c[3]);
                    luma(ink(c[0]), ink(c[1]), ink(c[2]))
                })
                .collect(),
        ),
        ImageData::Unsupported(why) => Err(why.clone()),
    }
}

fn luma(r: u8, g: u8, b: u8) -> u8 {
    ((u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114) / 1000) as u8
}

/// The loaded OCR engine.
pub struct Engine(ocrs::OcrEngine);

impl Engine {
    pub fn new(models: Models) -> Result<Engine, OcrError> {
        let load = |bytes: Vec<u8>, name: &str| {
            rten::Model::load(bytes)
                .map_err(|e| OcrError::Models(format!("{name} is not a usable `.rten` model: {e}")))
        };
        let engine = ocrs::OcrEngine::new(ocrs::OcrEngineParams {
            detection_model: Some(load(models.detection, "the detection model")?),
            recognition_model: Some(load(models.recognition, "the recognition model")?),
            ..Default::default()
        })
        .map_err(|e| OcrError::Engine(e.to_string()))?;
        Ok(Engine(engine))
    }

    fn words(&self, img: &image::GrayImage) -> Result<Vec<Word>, OcrError> {
        use ocrs::TextItem;
        let fail = |e: &dyn fmt::Display| OcrError::Engine(e.to_string());
        let source =
            ocrs::ImageSource::from_bytes(img.as_raw(), img.dimensions()).map_err(|e| fail(&e))?;
        let input = self.0.prepare_input(source).map_err(|e| fail(&e))?;
        let boxes = self.0.detect_words(&input).map_err(|e| fail(&e))?;
        let lines = self.0.find_text_lines(&input, &boxes);
        let texts = self.0.recognize_text(&input, &lines).map_err(|e| fail(&e))?;
        Ok(texts
            .iter()
            .flatten()
            .flat_map(|line| line.words().collect::<Vec<_>>())
            .map(|word| {
                let r = word.bounding_rect();
                Word {
                    left: r.left() as f32,
                    top: r.top() as f32,
                    right: r.right() as f32,
                    bottom: r.bottom() as f32,
                    text: word.to_string(),
                }
            })
            .collect())
    }
}
