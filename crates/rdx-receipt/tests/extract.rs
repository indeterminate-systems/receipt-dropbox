//! Extraction from the synthetic documents in `fixtures/receipts/`: one per input shape the
//! extractor has to handle (emailed bill, HTML receipt, Spanish till ticket, CFDI, forwarded
//! refund, a document with no total).

use std::path::PathBuf;

use rdx_model::{Currency, Date, Money, ReceiptKind};
use rdx_receipt::{DateOrder, Document, Hints, MediaType, Status, extract};

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/receipts")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn usd(minor: i64) -> Money {
    Money::new(minor, Currency::USD)
}

fn date(s: &str) -> Date {
    s.parse().unwrap()
}

fn usd_hints() -> Hints {
    Hints {
        currency: Some(Currency::USD),
        ..Hints::default()
    }
}

#[test]
fn emailed_bill_with_quoted_printable_body() {
    let doc = Document::new(MediaType::Email, fixture("bill-email.eml"));
    let x = extract(&doc, &usd_hints());
    assert_eq!(x.status, Status::Complete, "{x:#?}");
    assert_eq!(x.fields.merchant.as_deref(), Some("Paperleaf Supply"));
    assert_eq!(x.fields.date, Some(date("2026-03-10")));
    assert_eq!(x.fields.total, Some(usd(27989)));
    assert_eq!(x.fields.reference.as_deref(), Some("INV-20417"));
    assert_eq!(x.fields.kind, Some(ReceiptKind::Bill));
    assert_eq!(x.extractor, "text");
}

#[test]
fn html_receipt() {
    let doc = Document::new(MediaType::Html, fixture("receipt-html.html"));
    let x = extract(&doc, &usd_hints());
    assert_eq!(x.status, Status::Complete, "{x:#?}");
    assert_eq!(x.fields.merchant.as_deref(), Some("Metro Rideshare"));
    assert_eq!(x.fields.date, Some(date("2026-03-21")));
    assert_eq!(x.fields.total, Some(usd(23553)));
    assert_eq!(x.fields.reference.as_deref(), Some("MR-88213"));
    assert_eq!(x.fields.kind, Some(ReceiptKind::Receipt));
}

#[test]
fn spanish_ticket_needs_the_date_order_and_currency_hints() {
    let doc = Document::new(MediaType::Text, fixture("ticket-es.txt"));
    let mx = Hints {
        currency: Some(Currency::new("MXN").unwrap()),
        date_order: Some(DateOrder::Dmy),
        ..Hints::default()
    };
    let x = extract(&doc, &mx);
    assert_eq!(x.status, Status::Complete, "{x:#?}");
    assert_eq!(x.fields.merchant.as_deref(), Some("CAFETERÍA LA LINTERNA"));
    assert_eq!(x.fields.date, Some(date("2026-03-03")));
    assert_eq!(
        x.fields.total,
        Some(Money::new(14500, Currency::new("MXN").unwrap()))
    );
    assert_eq!(x.fields.reference.as_deref(), Some("004512"));

    // Without hints, `$` could be pesos or dollars and 03/03 is unambiguous but 03/04 wouldn't
    // be: the currency is missing and the extraction asks for review rather than guessing.
    let x = extract(&doc, &Hints::default());
    assert_eq!(x.status, Status::NeedsReview);
    assert!(x.missing.contains(&"currency"), "{x:#?}");
}

#[test]
fn ambiguous_dates_are_not_guessed() {
    let doc = Document::new(
        MediaType::Text,
        "Corner Store\nDate: 03/04/2026\nTotal USD 12.00\n".to_owned(),
    );
    let x = extract(&doc, &Hints::default());
    assert_eq!(x.fields.date, None);
    assert!(x.missing.contains(&"date"));
    assert!(x.warnings.iter().any(|w| w.contains("ambiguous")), "{x:#?}");

    let x = extract(
        &doc,
        &Hints {
            date_order: Some(DateOrder::Mdy),
            ..Hints::default()
        },
    );
    assert_eq!(x.fields.date, Some(date("2026-03-04")));
    assert_eq!(x.status, Status::Complete, "{x:#?}");
}

#[test]
fn cfdi_xml() {
    let doc = Document::new(MediaType::Xml, fixture("cfdi-ingreso.xml"));
    let x = extract(&doc, &Hints::default());
    assert_eq!(x.status, Status::Complete, "{x:#?}");
    assert_eq!(x.extractor, "cfdi");
    assert_eq!(x.fields.merchant.as_deref(), Some("PAPELERÍA EJEMPLO"));
    assert_eq!(x.fields.date, Some(date("2026-03-12")));
    assert_eq!(
        x.fields.total,
        Some(Money::new(116000, Currency::new("MXN").unwrap()))
    );
    assert_eq!(x.fields.reference.as_deref(), Some("A-1187"));
    assert_eq!(x.fields.kind, Some(ReceiptKind::Bill));
    let fiscal = x.fiscal.expect("CFDI fiscal data");
    assert_eq!(fiscal.uuid, "6F1E2D3C-4B5A-4978-8899-AABBCCDDEEFF");
    assert_eq!(fiscal.issuer_rfc, "EKU9003173C9");
    assert_eq!(fiscal.receiver_rfc, "XIA190128J61");
    assert_eq!(fiscal.payment_form.as_deref(), Some("04"));
}

#[test]
fn forwarded_refund_uses_the_original_sender_and_lists_the_pdf() {
    let doc = Document::new(MediaType::Email, fixture("refund-forwarded.eml"));
    let x = extract(&doc, &Hints::default());
    assert_eq!(x.status, Status::Complete, "{x:#?}");
    // The outer From is the supporter who forwarded it, not the merchant.
    assert_eq!(x.fields.merchant.as_deref(), Some("Lantern Cafe Catering"));
    assert_eq!(x.fields.kind, Some(ReceiptKind::Refund));
    assert_eq!(x.fields.date, Some(date("2026-03-18")));
    assert_eq!(
        x.fields.total,
        Some(Money::new(4250, Currency::new("EUR").unwrap()))
    );
    assert_eq!(x.fields.reference.as_deref(), Some("LC-5521"));
    assert_eq!(x.attachments.len(), 1);
    assert_eq!(x.attachments[0].media_type, "application/pdf");
    assert_eq!(
        x.attachments[0].file_name.as_deref(),
        Some("refund-LC-5521.pdf")
    );
    assert!(x.attachments[0].digest.as_str().starts_with("sha256:"));
}

#[test]
fn a_document_without_a_total_needs_review() {
    let doc = Document::new(MediaType::Text, fixture("no-total.txt"));
    let x = extract(&doc, &usd_hints());
    assert_eq!(x.status, Status::NeedsReview);
    assert_eq!(x.missing, vec!["total"]);
    assert_eq!(x.fields.merchant.as_deref(), Some("Northwind Hardware"));
    assert_eq!(x.fields.reference.as_deref(), Some("NW-3301"));
}

#[test]
fn source_digest_is_over_the_document_as_received() {
    let text = fixture("ticket-es.txt");
    let doc = Document::new(MediaType::Text, text.clone());
    assert_eq!(
        doc.digest(),
        rdx_model::Digest::of_bytes(text.as_bytes()),
        "the digest identifies the original, before any decoding"
    );
}
