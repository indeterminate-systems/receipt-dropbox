//! Image XObjects as OCR needs them: where each sits on the page and its pixels, or the JPEG it
//! carries. JPEG data is passed through undecoded; decoding images is the OCR crate's job.

use crate::content::{Interpreter, Matrix};
use crate::file::Doc;
use crate::lex::{Obj, Stream};
use crate::{ImageData, PageImage, PageImages, PdfError};

/// Images with more pixels than this are reported as unsupported rather than decoded: a page
/// scanned at 600 ppi is about 34 million.
const MAX_PIXELS: u64 = 80_000_000;

/// US Letter, for pages that state no media box.
const LETTER: [f64; 4] = [0.0, 0.0, 612.0, 792.0];

pub(crate) fn page_images(bytes: &[u8]) -> Result<Vec<PageImages>, PdfError> {
    let doc = Doc::load(bytes)?;
    let pages = doc.pages();
    if pages.is_empty() {
        return Err(PdfError::Malformed("no pages found".into()));
    }
    Ok(pages
        .iter()
        .map(|p| {
            let mut interp = Interpreter::new(&doc);
            interp.run_page(&p.contents, &p.resources);
            let [x0, y0, x1, y1] = p.media_box.unwrap_or(LETTER);
            PageImages {
                width: (x1 - x0).abs(),
                height: (y1 - y0).abs(),
                images: interp
                    .images
                    .iter()
                    .map(|(ctm, s)| placed(&doc, ctm, s, x0.min(x1), y0.min(y1)))
                    .collect(),
            }
        })
        .collect())
}

fn placed(doc: &Doc, ctm: &Matrix, s: &Stream, ox: f64, oy: f64) -> PageImage {
    // The image fills the unit square of its own space; map the corners onto the page.
    let corners = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)].map(|(u, v)| {
        (
            ctm[0] * u + ctm[2] * v + ctm[4],
            ctm[1] * u + ctm[3] * v + ctm[5],
        )
    });
    let min_x = corners.iter().map(|c| c.0).fold(f64::INFINITY, f64::min);
    let max_x = corners
        .iter()
        .map(|c| c.0)
        .fold(f64::NEG_INFINITY, f64::max);
    let min_y = corners.iter().map(|c| c.1).fold(f64::INFINITY, f64::min);
    let max_y = corners
        .iter()
        .map(|c| c.1)
        .fold(f64::NEG_INFINITY, f64::max);
    let int = |key: &[u8]| doc.dict_get(&s.dict, key).as_i64().unwrap_or(0).max(0) as u32;
    let (w, h) = (int(b"Width"), int(b"Height"));
    PageImage {
        x: min_x - ox,
        y: min_y - oy,
        width: max_x - min_x,
        height: max_y - min_y,
        pixels_wide: w,
        pixels_high: h,
        data: data(doc, s, w, h).unwrap_or_else(ImageData::Unsupported),
    }
}

fn data(doc: &Doc, s: &Stream, w: u32, h: u32) -> Result<ImageData, String> {
    if w == 0 || h == 0 {
        return Err("image without a width or height".into());
    }
    if u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(format!("image of {w}×{h} pixels is too large"));
    }
    let filters: Vec<Vec<u8>> = match doc.dict_get(&s.dict, b"Filter") {
        Obj::Name(n) => vec![n.clone()],
        Obj::Array(a) => a
            .iter()
            .filter_map(|o| doc.resolve(o).as_name().map(<[u8]>::to_vec))
            .collect(),
        _ => vec![],
    };
    let is_jpeg = |f: &[u8]| f == b"DCTDecode" || f == b"DCT";
    if let Some((last, before)) = filters.split_last()
        && is_jpeg(last)
    {
        let mut outer = s.clone();
        let names = before.iter().map(|n| Obj::Name(n.clone())).collect();
        outer.dict.insert(b"Filter".to_vec(), Obj::Array(names));
        outer.dict.remove(&b"DecodeParms"[..]);
        let jpeg = doc.decode(&outer).map_err(|e| e.to_string())?;
        return Ok(ImageData::Jpeg(jpeg));
    }
    if let Some(f) = filters.iter().find(|f| {
        !matches!(
            f.as_slice(),
            b"FlateDecode" | b"Fl" | b"ASCIIHexDecode" | b"AHx" | b"ASCII85Decode" | b"A85"
        )
    }) {
        return Err(format!(
            "images encoded with `{}` (only JPEG, Flate and uncompressed images are read)",
            String::from_utf8_lossy(f)
        ));
    }
    let raw = doc.decode(s).map_err(|e| e.to_string())?;
    let mask = matches!(doc.dict_get(&s.dict, b"ImageMask"), Obj::Bool(true));
    let bits = if mask {
        1
    } else {
        doc.dict_get(&s.dict, b"BitsPerComponent")
            .as_i64()
            .unwrap_or(8)
    };
    let components = if mask {
        1
    } else {
        components(doc, doc.dict_get(&s.dict, b"ColorSpace"))?
    };
    // `/Decode [1 0]` inverts; for an image mask, 0 marks painted (black) pixels.
    let inverted = match doc.dict_get(&s.dict, b"Decode").as_array() {
        Some([a, ..]) => doc.resolve(a).as_f64() == Some(1.0),
        _ => false,
    } ^ mask;
    let (w, h) = (w as usize, h as usize);
    let mut px = match bits {
        8 => {
            let need = w * h * components;
            if raw.len() < need {
                return Err("image data is shorter than its size".into());
            }
            raw[..need].to_vec()
        }
        1 if components == 1 => {
            let row = w.div_ceil(8);
            if raw.len() < row * h {
                return Err("image data is shorter than its size".into());
            }
            let mut out = Vec::with_capacity(w * h);
            for y in 0..h {
                for x in 0..w {
                    let bit = raw[y * row + x / 8] >> (7 - x % 8) & 1;
                    out.push(if bit == 1 { 255 } else { 0 });
                }
            }
            out
        }
        b => return Err(format!("images with {b} bits per component")),
    };
    if inverted {
        px.iter_mut().for_each(|p| *p = 255 - *p);
    }
    Ok(match components {
        1 => ImageData::Gray8(px),
        3 => ImageData::Rgb8(px),
        _ => ImageData::Cmyk8(px),
    })
}

fn components(doc: &Doc, cs: &Obj) -> Result<usize, String> {
    match cs {
        Obj::Name(n) => match n.as_slice() {
            b"DeviceGray" | b"G" | b"CalGray" => Ok(1),
            b"DeviceRGB" | b"RGB" | b"CalRGB" => Ok(3),
            b"DeviceCMYK" | b"CMYK" => Ok(4),
            other => Err(format!(
                "images in the `{}` colour space",
                String::from_utf8_lossy(other)
            )),
        },
        Obj::Array(a) => match (a.first().and_then(Obj::as_name), a.get(1)) {
            (Some(b"ICCBased"), Some(profile)) => match doc.resolve(profile) {
                Obj::Stream(p) => match doc.dict_get(&p.dict, b"N").as_i64() {
                    Some(n @ (1 | 3 | 4)) => Ok(n as usize),
                    _ => Err("an ICC colour space without a usable `/N`".into()),
                },
                _ => Err("an ICC colour space without its profile".into()),
            },
            (Some(b"CalGray"), _) => Ok(1),
            (Some(b"CalRGB"), _) => Ok(3),
            (Some(other), _) => Err(format!(
                "images in the `{}` colour space",
                String::from_utf8_lossy(other)
            )),
            _ => Err("an image with an unreadable colour space".into()),
        },
        Obj::Null => Err("an image without a colour space".into()),
        _ => Err("an image with an unreadable colour space".into()),
    }
}
