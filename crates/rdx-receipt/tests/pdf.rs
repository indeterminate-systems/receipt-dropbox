//! A PDF receipt: piece 4's reader gives its text layer, and the text extractor reads that.
//!
//! `UPDATE_GOLDEN=1 cargo test -p rdx-receipt --test pdf` rewrites
//! `fixtures/receipts/receipt-pdf.pdf`; review the diff before committing it.

use std::path::PathBuf;

use rdx_model::{Currency, Money, ReceiptKind};
use rdx_pdf::write::{Pdf, Style};
use rdx_receipt::{Document, Hints, MediaType, Status, extract};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/receipts/receipt-pdf.pdf")
}

/// A one-page receipt from a made-up print shop, laid out in two columns like a till receipt.
fn render() -> Vec<u8> {
    let mut pdf = Pdf::new();
    let page = pdf.page();
    let rows: &[(Style, &str, &str)] = &[
        (Style::Regular, "Receipt no: BP-7741", ""),
        (Style::Regular, "Date: 2026-03-14", ""),
        (Style::Regular, "Poster printing, 20 copies", "48.00"),
        (Style::Regular, "Sales tax", "3.84"),
        (Style::Bold, "Total paid", "USD 51.84"),
    ];
    page.text(Style::Bold, 16.0, 72.0, 720.0, "Brightline Print Shop");
    let mut y = 690.0;
    for (style, left, right) in rows {
        page.text(*style, 11.0, 72.0, y, left);
        if !right.is_empty() {
            page.text_right(*style, 11.0, 400.0, y, right);
        }
        y -= 18.0;
    }
    pdf.finish()
}

#[test]
fn fixture_is_current() {
    let actual = render();
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(fixture(), &actual).unwrap();
        return;
    }
    let expected = std::fs::read(fixture()).expect("run with UPDATE_GOLDEN=1 to create it");
    assert!(
        expected == actual,
        "receipt-pdf.pdf is stale: UPDATE_GOLDEN=1"
    );
}

#[test]
fn pdf_text_layer_extracts() {
    let text = rdx_pdf::text(&render()).unwrap().to_string();
    let x = extract(&Document::new(MediaType::Text, text), &Hints::default());
    assert_eq!(x.status, Status::Complete, "{x:#?}");
    assert_eq!(x.fields.merchant.as_deref(), Some("Brightline Print Shop"));
    assert_eq!(x.fields.total, Some(Money::new(5184, Currency::USD)));
    assert_eq!(x.fields.reference.as_deref(), Some("BP-7741"));
    assert_eq!(x.fields.kind, Some(ReceiptKind::Receipt));
    assert_eq!(
        x.fields.date.map(|d| d.to_string()).as_deref(),
        Some("2026-03-14")
    );
}
