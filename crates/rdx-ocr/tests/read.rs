//! Reading scans and photos with the real models: the synthetic fixtures in `fixtures/scans`.

mod common;

use common::{fixture, models};
use rdx_ocr::{OcrError, Options, Source, read};

fn no_models() -> Result<rdx_ocr::Models, OcrError> {
    panic!("a document with a text layer must not load the OCR models")
}

#[test]
fn a_text_pdf_is_read_from_its_text_layer_without_the_models() {
    let pdf = std::fs::read(
        common::repo().join("fixtures/synthetic/small/statements/st-2026-03.s-0001.pdf"),
    )
    .unwrap();
    let r = read(&pdf, &Options::default(), no_models).unwrap();
    assert_eq!(r.source, Source::TextLayer);
    assert_eq!(r.text, rdx_pdf::text(&pdf).unwrap());
    assert!(r.warnings.is_empty());
}

#[test]
fn other_documents_are_left_alone() {
    let r = read(b"date,description,amount\n", &Options::default(), no_models).unwrap();
    assert_eq!(r.source, Source::None);
    assert!(r.text.pages.is_empty());
    assert_eq!(r.warnings.len(), 1);
}

#[test]
fn a_receipt_photo_is_read_line_by_line() {
    let png = std::fs::read(fixture("receipt-photo.png")).unwrap();
    let r = read(&png, &Options::default(), || Ok(models())).unwrap();
    assert_eq!(r.source, Source::Ocr);
    let text = r.text.to_string();
    for expected in ["HARBOR HARDWARE", "Date: 2026-03-19", "Cable ties"] {
        assert!(text.contains(expected), "{expected:?} not in:\n{text}");
    }
    // The total and its label end up on one line, in separate columns.
    let total = r
        .text
        .lines()
        .find(|l| l.text().contains("TOTAL"))
        .unwrap_or_else(|| panic!("no TOTAL line in:\n{text}"));
    let spans: Vec<&str> = total.spans.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(spans, ["TOTAL MXN", "342.78"], "{text}");
}

#[test]
fn a_scanned_statement_pdf_is_read_from_its_page_image_in_points() {
    let pdf = std::fs::read(fixture("st-2026-03.s-0001.scan.pdf")).unwrap();
    assert!(rdx_pdf::text(&pdf).unwrap().is_empty(), "the scan has no text layer");
    let r = read(&pdf, &Options::default(), || Ok(models())).unwrap();
    assert_eq!(r.source, Source::Ocr);
    let text = r.text.to_string();
    assert!(text.contains("Example Bank"), "{text}");
    let header = r
        .text
        .lines()
        .find(|l| l.spans.iter().any(|s| s.text == "Withdrawals") && l.spans.len() == 5)
        .unwrap_or_else(|| panic!("no table header in:\n{text}"));
    let spans: Vec<&str> = header.spans.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(spans, ["Date", "Description", "Withdrawals", "Deposits", "Balance"]);
    // Positions are in points on the page: the Balance column ends at the right margin, 54 pt
    // in from the edge of a 612 pt page.
    let balance = &header.spans[4];
    assert!((balance.x_end - 558.0).abs() < 4.0, "{balance:?}");
    // Each amount sits under its column, as the example-bank parser needs.
    let ends: Vec<f64> = header.spans[2..].iter().map(|s| s.x_end).collect();
    for line in r.text.lines().filter(|l| l.spans[0].text.starts_with("03/")) {
        for amount in &line.spans[2..] {
            assert!(
                ends.iter().any(|e| (e - amount.x_end).abs() < 6.0),
                "{amount:?} is under no column ({ends:?}):\n{text}"
            );
        }
    }
}

#[test]
fn bad_models_say_so() {
    let png = std::fs::read(fixture("receipt-photo.png")).unwrap();
    let err = read(&png, &Options::default(), || {
        Ok(rdx_ocr::Models {
            detection: b"not a model".to_vec(),
            recognition: b"nor this".to_vec(),
        })
    })
    .unwrap_err();
    assert!(err.to_string().contains("not a usable `.rten` model"), "{err}");
}
