//! Every receipt in the synthetic scenario, rendered as a plain-text receipt, an HTML receipt and
//! an emailed bill, extracts back to the same merchant, date, total, reference and kind.

use std::path::PathBuf;

use rdx_model::{Receipt, ReceiptKind};
use rdx_receipt::{Document, Hints, MediaType, Status, extract};

fn receipts() -> Vec<Receipt> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/small/scenario.json");
    let scenario: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    serde_json::from_value(scenario["receipts"].clone()).unwrap()
}

fn heading(r: &Receipt) -> &'static str {
    match r.kind {
        ReceiptKind::Receipt => "Receipt",
        ReceiptKind::Bill => "Invoice",
        ReceiptKind::Refund => "Refund receipt",
    }
}

fn total_label(r: &Receipt) -> &'static str {
    match r.kind {
        ReceiptKind::Refund => "Amount refunded",
        ReceiptKind::Bill => "Amount due",
        ReceiptKind::Receipt => "Total",
    }
}

fn text(r: &Receipt) -> String {
    let mut s = format!("{}\n{}\nDate: {}\n", r.merchant, heading(r), r.date);
    if let Some(reference) = &r.reference {
        s += &format!("Invoice number: {reference}\n");
    }
    for item in &r.items {
        s += &format!("{:<30}{:>12}\n", item.description, item.amount.decimal());
    }
    s += &format!(
        "{:<30}{:>12} {}\n",
        total_label(r),
        r.total.decimal(),
        r.total.currency().code()
    );
    s
}

fn html(r: &Receipt) -> String {
    let mut rows = format!("<tr><td>Date</td><td>{}</td></tr>", r.date);
    if let Some(reference) = &r.reference {
        rows += &format!("<tr><td>Invoice #</td><td>{reference}</td></tr>");
    }
    for item in &r.items {
        rows += &format!(
            "<tr><td>{}</td><td>{}</td></tr>",
            item.description,
            item.amount.decimal()
        );
    }
    rows += &format!(
        "<tr><td><b>{}</b></td><td><b>{} {}</b></td></tr>",
        total_label(r),
        r.total.currency().code(),
        r.total.decimal()
    );
    format!(
        "<html><body><h2>{}</h2><p>{}</p><table>{rows}</table></body></html>",
        r.merchant.replace('&', "&amp;"),
        heading(r)
    )
}

fn email(r: &Receipt) -> String {
    format!(
        "From: \"{}\" <billing@merchant.example>\nTo: supporter@example.org\nSubject: {}\n\
         MIME-Version: 1.0\nContent-Type: text/html; charset=utf-8\n\n{}",
        r.merchant,
        heading(r),
        html(r)
    )
}

#[test]
fn synthetic_receipts_round_trip_through_every_shape() {
    let receipts = receipts();
    assert!(!receipts.is_empty());
    for r in &receipts {
        for (shape, doc) in [
            ("text", Document::new(MediaType::Text, text(r))),
            ("html", Document::new(MediaType::Html, html(r))),
            ("email", Document::new(MediaType::Email, email(r))),
        ] {
            let x = extract(&doc, &Hints::default());
            let ctx = format!("{} as {shape}: {x:#?}", r.id);
            assert_eq!(x.status, Status::Complete, "{ctx}");
            assert_eq!(
                x.fields.merchant.as_deref(),
                Some(r.merchant.as_str()),
                "{ctx}"
            );
            assert_eq!(x.fields.date, Some(r.date), "{ctx}");
            assert_eq!(x.fields.total, Some(r.total), "{ctx}");
            assert_eq!(x.fields.reference, r.reference, "{ctx}");
            assert_eq!(x.fields.kind, Some(r.kind), "{ctx}");
        }
    }
}
