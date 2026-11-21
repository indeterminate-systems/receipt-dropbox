//! A minimal PDF writer for synthetic fixtures: text in Helvetica, rules and images (for scans).
//!
//! The fonts are the standard ones, written without `/Widths` as many real producers do, so the
//! reader's fallback widths get exercised. Output is deterministic: no dates, no ids.

use crate::ImageData;
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
    images: Vec<(u32, u32, ImageData)>,
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

    /// Draws an image over the rectangle with its bottom left at (`x`, `y`), `width` by `height`
    /// points: a JPEG as it is, raw pixels compressed with Flate. Panics on
    /// [`ImageData::Unsupported`], which has nothing to draw.
    #[allow(clippy::too_many_arguments)]
    pub fn image(
        &mut self,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        pixels_wide: u32,
        pixels_high: u32,
        data: &ImageData,
    ) {
        assert!(
            !matches!(data, ImageData::Unsupported(_)),
            "nothing to draw for an unsupported image"
        );
        let name = self.images.len();
        self.ops.push_str(&format!(
            "q {width:.2} 0 0 {height:.2} {x:.2} {y:.2} cm /Im{name} Do Q\n"
        ));
        self.images.push((pixels_wide, pixels_high, data.clone()));
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
        serialize(self.objects(), "1.4", "")
    }

    /// The same document, protected with `password` as Mexican banks protect statements:
    /// AES-256 (PDF 2.0's revision 6), with `password` as both the user and the owner password.
    /// Deterministic like [`Pdf::finish`]: the salts and IVs derive from the content and password.
    pub fn finish_locked(self, password: &str) -> Vec<u8> {
        let mut objects = self.objects();
        let mut seed = password.as_bytes().to_vec();
        objects.iter().for_each(|o| seed.extend_from_slice(o));
        let lock = crate::crypt::lock::Lock::new(&seed);
        for (i, body) in objects.iter_mut().enumerate() {
            *body = encrypt_stream(body, |data| lock.encrypt(i + 1, data));
        }
        objects.push(lock.dictionary(password).into_bytes());
        let id = lock.id();
        let trailer = format!(" /Encrypt {} 0 R /ID [<{id}> <{id}>]", objects.len());
        serialize(objects, "1.7", &trailer)
    }

    fn objects(self) -> Vec<Vec<u8>> {
        // Objects: 1 catalog, 2 page tree, 3 and 4 fonts, then a page and its content per page,
        // then the images.
        let n_pages = self.pages.len();
        let mut next_image = 5 + 2 * n_pages;
        let mut image_objects: Vec<Vec<u8>> = Vec::new();
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
            // Text-only pages are written exactly as before images existed, so fixtures made
            // with this writer keep their digests.
            let mut xobjects = String::new();
            for (k, (w, h, data)) in page.images.iter().enumerate() {
                xobjects.push_str(&format!("/Im{k} {next_image} 0 R "));
                next_image += 1;
                image_objects.push(image_object(*w, *h, data));
            }
            if !xobjects.is_empty() {
                xobjects = format!(" /XObject << {xobjects}>>");
            }
            objects.push(
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_WIDTH} {PAGE_HEIGHT}] \
                     /Resources << /Font << /F1 3 0 R /F2 4 0 R >>{xobjects} >> /Contents {} 0 R >>",
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

        objects.extend(image_objects);
        objects
    }
}

/// The file around `objects` (numbered from 1, the first being the catalog), with `trailer`
/// added to the trailer dictionary.
fn serialize(objects: Vec<Vec<u8>>, version: &str, trailer: &str) -> Vec<u8> {
    let mut out = format!("%PDF-{version}\n").into_bytes();
    out.extend_from_slice(b"%\xe2\xe3\xcf\xd3\n");
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
            "trailer\n<< /Size {} /Root 1 0 R{trailer} >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn image_object(w: u32, h: u32, data: &ImageData) -> Vec<u8> {
    let (space, filter, body) = match data {
        ImageData::Jpeg(j) => ("DeviceGray", "DCTDecode", j.clone()),
        ImageData::Gray8(p) => ("DeviceGray", "FlateDecode", deflate(p)),
        ImageData::Rgb8(p) => ("DeviceRGB", "FlateDecode", deflate(p)),
        ImageData::Cmyk8(p) => ("DeviceCMYK", "FlateDecode", deflate(p)),
        ImageData::Unsupported(_) => unreachable!("refused when drawn"),
    };
    // A JPEG says its own colour space; readers take the JPEG's, so gray is only a label.
    let space = match data {
        ImageData::Jpeg(j) => jpeg_space(j).unwrap_or(space),
        _ => space,
    };
    let mut out = format!(
        "<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /{space} \
         /BitsPerComponent 8 /Filter /{filter} /Length {} >>\nstream\n",
        body.len()
    )
    .into_bytes();
    out.extend_from_slice(&body);
    out.extend_from_slice(b"\nendstream");
    out
}

/// Replaces the data of a stream object as this writer formats them (`<< … /Length n … >>`,
/// `stream`, data, `endstream`), fixing its `/Length`. Other objects are returned unchanged.
fn encrypt_stream(body: &[u8], encrypt: impl Fn(&[u8]) -> Vec<u8>) -> Vec<u8> {
    const START: &[u8] = b"\nstream\n";
    const END: &[u8] = b"\nendstream";
    let Some(at) = body.windows(START.len()).position(|w| w == START) else {
        return body.to_vec();
    };
    let data = &body[at + START.len()..body.len() - END.len()];
    let dict = String::from_utf8(body[..at].to_vec()).expect("the writer's dictionaries are ASCII");
    let sealed = encrypt(data);
    let dict = dict.replace(
        &format!("/Length {}", data.len()),
        &format!("/Length {}", sealed.len()),
    );
    let mut out = dict.into_bytes();
    out.extend_from_slice(START);
    out.extend_from_slice(&sealed);
    out.extend_from_slice(END);
    out
}

fn deflate(p: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(p, 6)
}

/// The colour space a JPEG's start-of-frame header implies, from its component count.
fn jpeg_space(j: &[u8]) -> Option<&'static str> {
    let mut i = 2;
    while i + 9 < j.len() {
        if j[i] != 0xff {
            return None;
        }
        let marker = j[i + 1];
        let len = usize::from(j[i + 2]) << 8 | usize::from(j[i + 3]);
        if matches!(marker, 0xc0..=0xc3) {
            return match j[i + 9] {
                1 => Some("DeviceGray"),
                3 => Some("DeviceRGB"),
                4 => Some("DeviceCMYK"),
                _ => None,
            };
        }
        i += 2 + len;
    }
    None
}
