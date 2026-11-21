//! The scanned statement fixture: the Typst rendering of the `example-bank` statement at 200 ppi
//! (`fixtures/scans/st-2026-03.s-0001.png`), saved as a grayscale JPEG on a PDF page with no
//! text layer, as a scanner would.
//!
//! `UPDATE_GOLDEN=1 cargo test -p rdx-ocr --test fixtures` rewrites
//! `fixtures/scans/st-2026-03.s-0001.scan.pdf`; review the diff before committing it.

mod common;

use common::fixture;
use rdx_pdf::ImageData;
use rdx_pdf::write::{PAGE_HEIGHT, PAGE_WIDTH, Pdf};

fn render() -> Vec<u8> {
    let png = std::fs::read(fixture("st-2026-03.s-0001.png")).unwrap();
    let gray = image::load_from_memory(&png).unwrap().into_luma8();
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 80)
        .encode_image(&gray)
        .unwrap();
    let mut pdf = Pdf::new();
    pdf.page().image(
        0.0,
        0.0,
        PAGE_WIDTH,
        PAGE_HEIGHT,
        gray.width(),
        gray.height(),
        &ImageData::Jpeg(jpeg),
    );
    pdf.finish()
}

#[test]
fn scanned_statement_is_current() {
    let path = fixture("st-2026-03.s-0001.scan.pdf");
    let actual = render();
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, &actual).unwrap();
        return;
    }
    let expected = std::fs::read(&path).expect("run with UPDATE_GOLDEN=1 to create it");
    assert!(expected == actual, "{} is stale: UPDATE_GOLDEN=1", path.display());
}
