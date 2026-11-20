//! The text layer of a PDF: its words, grouped into lines and spans with their positions.
//!
//! Statements and receipts arrive as PDFs. Most have a text layer, which this crate reads in pure
//! Rust with no I/O, so it runs in the PWA, on Workers and under wasmtime alike. A PDF without a
//! text layer (a scan) gives an empty [`Text`]; OCR (roadmap piece 11) handles those.
//!
//! Positions matter for statements: a withdrawal and a deposit differ only by the column the
//! amount sits in. So each [`Line`] keeps its [`Span`]s (runs of words separated by a gap wider
//! than about one character) with their left and right edges in points.
//!
//! ```
//! # #[cfg(feature = "write")] {
//! use rdx_pdf::write::{Pdf, Style};
//!
//! let mut pdf = Pdf::new();
//! let page = pdf.page();
//! page.text(Style::Regular, 10.0, 72.0, 700.0, "2026-03-03");
//! page.text(Style::Regular, 10.0, 140.0, 700.0, "Coffee beans");
//! page.text_right(Style::Regular, 10.0, 540.0, 700.0, "-4.50");
//! let text = rdx_pdf::text(&pdf.finish()).unwrap();
//! let spans: Vec<&str> = text.pages[0].lines[0].spans.iter().map(|s| s.text.as_str()).collect();
//! assert_eq!(spans, ["2026-03-03", "Coffee beans", "-4.50"]);
//! # }
//! ```

mod content;
mod file;
mod font;
mod layout;
mod lex;
mod op;
#[cfg(feature = "write")]
pub mod write;

use std::fmt;

use serde::{Deserialize, Serialize};

pub use op::{PdfText, read_document, register};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PdfError {
    #[error("not a PDF (no `%PDF-` header)")]
    NotPdf,
    #[error("the PDF is encrypted; save an unencrypted copy (print to PDF) and use that instead")]
    Encrypted,
    #[error("unsupported PDF feature: {0}")]
    Unsupported(String),
    #[error("malformed PDF: {0}")]
    Malformed(String),
}

/// A document's text, page by page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Text {
    pub pages: Vec<Page>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page {
    /// Top to bottom.
    pub lines: Vec<Line>,
}

/// Glyphs that share a baseline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Line {
    /// Baseline height in points from the bottom of the page.
    pub y: f64,
    /// Left to right.
    pub spans: Vec<Span>,
}

/// A run of words with no wide gap inside it: a table cell, a label or a value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Span {
    /// Left edge, in points from the left of the page.
    pub x: f64,
    /// Right edge. Amount columns are usually right-aligned, so this is what lines up.
    pub x_end: f64,
    pub text: String,
}

impl Line {
    /// The line's spans joined with two spaces, marking where the column gaps were.
    pub fn text(&self) -> String {
        self.spans
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join("  ")
    }
}

impl Text {
    /// No text on any page: a scan, or a PDF of images. OCR is the way in.
    pub fn is_empty(&self) -> bool {
        self.pages.iter().all(|p| p.lines.is_empty())
    }

    /// Every line, top to bottom and page by page.
    pub fn lines(&self) -> impl Iterator<Item = &Line> {
        self.pages.iter().flat_map(|p| p.lines.iter())
    }
}

/// Lines separated by newlines, pages by a form feed.
impl fmt::Display for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, page) in self.pages.iter().enumerate() {
            if i > 0 {
                f.write_str("\u{c}")?;
            }
            for line in &page.lines {
                writeln!(f, "{}", line.text())?;
            }
        }
        Ok(())
    }
}

/// Whether the bytes look like a PDF (a `%PDF-` header near the start).
pub fn is_pdf(bytes: &[u8]) -> bool {
    file::find(&bytes[..bytes.len().min(1024)], b"%PDF-", 0).is_some()
}

/// Reads the text layer of a PDF.
pub fn text(bytes: &[u8]) -> Result<Text, PdfError> {
    let doc = file::Doc::load(bytes)?;
    let pages = doc.pages();
    if pages.is_empty() {
        return Err(PdfError::Malformed("no pages found".into()));
    }
    let pages = pages
        .iter()
        .map(|p| {
            let mut interp = content::Interpreter::new(&doc);
            interp.run_page(&p.contents, &p.resources);
            Page {
                lines: layout::lines(interp.glyphs),
            }
        })
        .collect();
    Ok(Text { pages })
}
