//! A minimal PDF writer for synthetic fixtures: text in Helvetica, nothing else.
//!
//! The fonts are the standard ones, written without `/Widths` as many real producers do, so the
//! reader's fallback widths get exercised. Output is deterministic: no dates, no ids.

use crate::font::helvetica_width;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Regular,
    Bold,
}

/// US Letter, in points.
pub const PAGE_WIDTH: f64 = 612.0;
pub const PAGE_HEIGHT: f64 = 792.0;

#[derive(Default)]
pub struct Pdf {
    pages: Vec<PageContent>,
}

#[derive(Default)]
pub struct PageContent {
    ops: String,
}

/// Width of `text` in Helvetica at `size` points (bold is treated as regular; close enough to
/// right-align columns).
pub fn text_width(text: &str, size: f64) -> f64 {
    text.chars().map(helvetica_width).sum::<f64>() * size
}

impl PageContent {
    /// Draws `text` with its baseline starting at (`x`, `y`), in points from the bottom left.
    pub fn text(&mut self, style: Style, size: f64, x: f64, y: f64, text: &str) {
        let font = match style {
            Style::Regular => "F1",
            Style::Bold => "F2",
        };
        self.ops.push_str(&format!(
            "BT /{font} {size} Tf 1 0 0 1 {x:.2} {y:.2} Tm ({}) Tj ET\n",
            escape(text)
        ));
    }

    /// Draws `text` so that it ends at `x_end`.
    pub fn text_right(&mut self, style: Style, size: f64, x_end: f64, y: f64, text: &str) {
        self.text(style, size, x_end - text_width(text, size), y, text);
    }

    /// A horizontal rule, as statements draw under table headers.
    pub fn rule(&mut self, x0: f64, x1: f64, y: f64) {
        self.ops
            .push_str(&format!("0.5 w {x0:.2} {y:.2} m {x1:.2} {y:.2} l S\n"));
    }
}

/// WinAnsi bytes as a literal string body.
fn escape(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '(' | ')' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            ' '..='~' => out.push(c),
            // Latin-1 characters are the same code in WinAnsi.
            '\u{a0}'..='\u{ff}' => out.push_str(&format!("\\{:03o}", c as u32)),
            _ => out.push('?'),
        }
    }
    out
}

impl Pdf {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn page(&mut self) -> &mut PageContent {
        self.pages.push(PageContent::default());
        self.pages.last_mut().expect("just pushed")
    }

    pub fn finish(self) -> Vec<u8> {
        // Objects: 1 catalog, 2 page tree, 3 and 4 fonts, then a page and its content per page.
        let n_pages = self.pages.len();
        let mut objects: Vec<Vec<u8>> = Vec::new();
        objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
        let kids: Vec<String> = (0..n_pages).map(|i| format!("{} 0 R", 5 + 2 * i)).collect();
        objects.push(
            format!(
                "<< /Type /Pages /Kids [{}] /Count {n_pages} >>",
                kids.join(" ")
            )
            .into_bytes(),
        );
        for base in ["Helvetica", "Helvetica-Bold"] {
            objects.push(
                format!(
                    "<< /Type /Font /Subtype /Type1 /BaseFont /{base} /Encoding /WinAnsiEncoding >>"
                )
                .into_bytes(),
            );
        }
        for (i, page) in self.pages.iter().enumerate() {
            objects.push(
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_WIDTH} {PAGE_HEIGHT}] \
                     /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents {} 0 R >>",
                    6 + 2 * i
                )
                .into_bytes(),
            );
            let data = miniz_oxide::deflate::compress_to_vec_zlib(page.ops.as_bytes(), 6);
            let mut stream = format!(
                "<< /Length {} /Filter /FlateDecode >>\nstream\n",
                data.len()
            )
            .into_bytes();
            stream.extend_from_slice(&data);
            stream.extend_from_slice(b"\nendstream");
            objects.push(stream);
        }

        let mut out = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
        let mut offsets = Vec::new();
        for (i, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for off in offsets {
            out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        out
    }
}
