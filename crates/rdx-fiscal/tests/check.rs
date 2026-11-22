//! Proof checks under each policy kind, on the synthetic CFDIs in fixtures/fiscal.

use std::path::{Path, PathBuf};

use rdx_fiscal::{Expense, Policy, Proof, Report, check};
use rdx_model::{Currency, Digest, Money, Receipt, ReceiptId, ReceiptKind, SupporterId};
use serde_json::json;

fn fixture(name: &str) -> String {
    let base: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    std::fs::read_to_string(base.join(name)).unwrap()
}

/// The policy as `rdx/fiscal.ncl` exports it with the 2026 defaults.
fn policy(kind: &str) -> Policy {
    let mut p = json!({
        "kind": kind,
        "sat_registered": true,
        "fiscal_year": 2026,
        "rules": {
            "confirmed": false,
            "cash": { "forms": ["01"], "threshold": { "minor": 200000, "currency": "MXN" } },
            "fuel_product_codes": ["151015"],
            "cfdi_window": { "days_before": 3, "days_after": 31 },
            "foreign_invoice_fields": ["vendor", "tax_id", "address", "description", "buyer"],
        },
    });
    if kind != "informal-group" {
        p["profile"] = json!({
            "rfc": "XIA190128J61",
            "name": "ORGANIZACIÓN DE PRUEBA",
            "postcode": "06000",
            "regime": "603",
            "uso_cfdi": ["G03"],
        });
    }
    serde_json::from_value(p).unwrap()
}

fn receipt(id: &str, date: &str, minor: i64) -> Receipt {
    Receipt {
        id: ReceiptId::new(id).unwrap(),
        supporter: SupporterId::new("s-0001").unwrap(),
        kind: ReceiptKind::Bill,
        merchant: "Papelería Ejemplo".into(),
        date: date.parse().unwrap(),
        total: Money::new(minor, Currency::new("MXN").unwrap()),
        reference: None,
        items: Vec::new(),
        source: Digest::of_bytes(id.as_bytes()),
    }
}

fn expense(r: Receipt, cfdi: Option<&str>) -> Expense {
    Expense {
        cfdi: cfdi.map(fixture),
        sat: Some(
            serde_json::from_value(
                json!({ "status": "vigente", "efos": "none", "checked": "2026-03-20T10:00:00Z" }),
            )
            .unwrap(),
        ),
        purpose: Some("Office supplies for the March workshop".into()),
        ..Expense::new(r)
    }
}

fn codes(report: &Report, i: usize) -> Vec<&str> {
    report.assessments[i]
        .findings
        .iter()
        .map(|f| f.code)
        .collect()
}

#[test]
fn a_cfdi_to_the_organization_is_proof() {
    let r = check(
        &policy("nonprofit"),
        &[expense(
            receipt("r-1", "2026-03-12", 116000),
            Some("receipts/cfdi-ingreso.xml"),
        )],
    )
    .unwrap();
    let a = &r.assessments[0];
    assert_eq!(a.proof, Proof::Supported, "{:?}", a.findings);
    assert_eq!(a.meta["cfdi"], "6F1E2D3C-4B5A-4978-8899-AABBCCDDEEFF");
    assert_eq!(a.meta["rfc-emisor"], "EKU9003173C9");
    assert_eq!(a.meta["forma-pago"], "04");
    assert_eq!(a.meta["sat-status"], "vigente");
    assert_eq!(a.meta["proof"], "supported");
    assert_eq!(r.summary.supported, 1);
    // The rules aren't confirmed by a contador yet, and every run says so.
    assert!(
        r.warnings.iter().any(|w| w.contains("not confirmed")),
        "{:?}",
        r.warnings
    );
}

#[test]
fn without_a_sat_check_it_needs_review() {
    let mut e = expense(
        receipt("r-1", "2026-03-12", 116000),
        Some("receipts/cfdi-ingreso.xml"),
    );
    e.sat = None;
    let r = check(&policy("company"), &[e]).unwrap();
    assert_eq!(r.assessments[0].proof, Proof::NeedsReview);
    assert_eq!(codes(&r, 0), ["sat-unchecked"]);
    assert_eq!(r.assessments[0].meta["sat-status"], "unchecked");
}

#[test]
fn a_cfdi_to_the_supporter_says_how_to_fix_it() {
    let r = check(
        &policy("company"),
        &[expense(
            receipt("r-2", "2026-03-14", 58000),
            Some("fiscal/cfdi-to-supporter.xml"),
        )],
    )
    .unwrap();
    let a = &r.assessments[0];
    assert_eq!(a.proof, Proof::NotSupported);
    let f = a
        .findings
        .iter()
        .find(|f| f.code == "cfdi-to-other-rfc")
        .unwrap();
    assert!(f.message.contains("CACX7605101P8"), "{}", f.message);
    let fix = f.fix.as_deref().unwrap();
    assert!(
        fix.contains("XIA190128J61") && fix.contains("G03") && fix.contains("06000"),
        "{fix}"
    );
}

#[test]
fn a_cfdi_to_the_general_public_is_not_the_organizations() {
    let r = check(
        &policy("company"),
        &[expense(
            receipt("r-3", "2026-03-09", 35000),
            Some("fiscal/cfdi-publico-general.xml"),
        )],
    )
    .unwrap();
    assert_eq!(r.assessments[0].proof, Proof::NotSupported);
    assert!(
        codes(&r, 0).contains(&"cfdi-to-general-public"),
        "{:?}",
        codes(&r, 0)
    );
}

#[test]
fn fuel_and_large_payments_in_cash_are_flagged() {
    let r = check(
        &policy("company"),
        &[expense(
            receipt("r-4", "2026-03-18", 250000),
            Some("fiscal/cfdi-fuel-cash.xml"),
        )],
    )
    .unwrap();
    let c = codes(&r, 0);
    assert!(
        c.contains(&"cash-over-threshold") && c.contains(&"fuel-paid-cash"),
        "{c:?}"
    );
    assert_eq!(r.assessments[0].proof, Proof::NotSupported);
}

#[test]
fn a_ppd_invoice_needs_its_payment_complement() {
    let r = check(
        &policy("company"),
        &[expense(
            receipt("r-5", "2026-03-02", 580000),
            Some("fiscal/cfdi-ppd.xml"),
        )],
    )
    .unwrap();
    assert_eq!(codes(&r, 0), ["ppd-unpaid"]);
    assert_eq!(r.assessments[0].proof, Proof::NeedsReview);

    let mut e = expense(
        receipt("r-5", "2026-03-02", 580000),
        Some("fiscal/cfdi-ppd.xml"),
    );
    e.payments = vec![fixture("fiscal/pago-ppd.xml")];
    let r = check(&policy("company"), &[e]).unwrap();
    assert_eq!(
        r.assessments[0].proof,
        Proof::Supported,
        "{:?}",
        codes(&r, 0)
    );
}

#[test]
fn a_matched_line_without_a_cfdi_is_payment_evidence_only() {
    let r = check(
        &policy("nonprofit"),
        &[expense(receipt("r-6", "2026-03-12", 116000), None)],
    )
    .unwrap();
    let a = &r.assessments[0];
    assert_eq!(a.proof, Proof::PaymentEvidenceOnly);
    let f = &a.findings[0];
    assert_eq!(f.code, "cfdi-missing");
    // For a non-profit, the cost of a missing CFDI is spelled out.
    assert!(
        f.message.contains("remanente distribuible"),
        "{}",
        f.message
    );
    assert!(f.fix.as_deref().unwrap().contains("XIA190128J61"));
}

#[test]
fn totals_and_kinds_must_agree() {
    let r = check(
        &policy("company"),
        &[expense(
            receipt("r-7", "2026-03-12", 99900),
            Some("receipts/cfdi-ingreso.xml"),
        )],
    )
    .unwrap();
    assert!(codes(&r, 0).contains(&"cfdi-total-mismatch"));

    let mut refund = receipt("r-8", "2026-03-12", 116000);
    refund.kind = ReceiptKind::Refund;
    let r = check(
        &policy("company"),
        &[expense(refund, Some("receipts/cfdi-ingreso.xml"))],
    )
    .unwrap();
    assert!(codes(&r, 0).contains(&"cfdi-kind-mismatch"));

    let r = check(
        &policy("company"),
        &[expense(
            receipt("r-9", "2026-03-20", 1),
            Some("fiscal/pago-ppd.xml"),
        )],
    )
    .unwrap();
    assert!(codes(&r, 0).contains(&"cfdi-not-an-expense"));
}

#[test]
fn an_unstamped_cfdi_is_not_proof() {
    let r = check(
        &policy("company"),
        &[expense(
            receipt("r-10", "2026-03-14", 58000),
            Some("fiscal/cfdi-unstamped.xml"),
        )],
    )
    .unwrap();
    assert_eq!(r.assessments[0].proof, Proof::NotSupported);
    assert_eq!(codes(&r, 0), ["cfdi-unstamped"]);
}

#[test]
fn sat_status_and_the_69b_list_count() {
    let mut e = expense(
        receipt("r-1", "2026-03-12", 116000),
        Some("receipts/cfdi-ingreso.xml"),
    );
    e.sat = Some(
        serde_json::from_value(json!({ "status": "cancelado", "checked": "2026-04-01T00:00:00Z" }))
            .unwrap(),
    );
    let r = check(&policy("company"), &[e]).unwrap();
    assert_eq!(codes(&r, 0), ["cfdi-cancelled"]);
    assert_eq!(r.assessments[0].meta["sat-checked"], "2026-04-01T00:00:00Z");

    let mut e = expense(
        receipt("r-1", "2026-03-12", 116000),
        Some("receipts/cfdi-ingreso.xml"),
    );
    e.sat = Some(
        serde_json::from_value(
            json!({ "status": "vigente", "efos": "definitivo", "checked": "2026-04-01T00:00:00Z" }),
        )
        .unwrap(),
    );
    let r = check(&policy("company"), &[e]).unwrap();
    assert_eq!(codes(&r, 0), ["issuer-69b-definitive"]);
    assert_eq!(r.assessments[0].proof, Proof::NotSupported);
}

#[test]
fn an_informal_group_takes_the_receipt_and_records_whose_cfdi_it_is() {
    let r = check(
        &policy("informal-group"),
        &[
            expense(
                receipt("r-2", "2026-03-14", 58000),
                Some("fiscal/cfdi-to-supporter.xml"),
            ),
            expense(receipt("r-6", "2026-03-12", 116000), None),
        ],
    )
    .unwrap();
    assert_eq!(
        r.assessments[0].proof,
        Proof::Supported,
        "{:?}",
        r.assessments[0].findings
    );
    assert_eq!(codes(&r, 0), ["cfdi-receiver"]);
    assert_eq!(r.assessments[0].meta["rfc-receptor"], "CACX7605101P8");
    assert_eq!(r.assessments[1].proof, Proof::Supported);
    assert!(r.assessments[1].findings.is_empty());
}

#[test]
fn an_unregistered_individual_is_treated_as_informal_with_a_warning() {
    let mut p = policy("individual");
    p.sat_registered = false;
    let r = check(&p, &[expense(receipt("r-6", "2026-03-12", 116000), None)]).unwrap();
    assert_eq!(r.assessments[0].proof, Proof::Supported);
    assert!(
        r.warnings.iter().any(|w| w.contains("not registered")),
        "{:?}",
        r.warnings
    );
}

#[test]
fn a_donataria_classifies_every_expense() {
    let mut e = expense(
        receipt("r-1", "2026-03-12", 116000),
        Some("receipts/cfdi-ingreso.xml"),
    );
    e.purpose = None;
    let r = check(&policy("donataria-autorizada"), &[e]).unwrap();
    assert_eq!(codes(&r, 0), ["purpose-missing", "class-missing"]);
    assert_eq!(r.assessments[0].proof, Proof::NeedsReview);
}

#[test]
fn a_foreign_invoice_goes_to_the_contador() {
    let mut e = expense(receipt("r-11", "2026-03-05", 42000), None);
    e.foreign = Some(serde_json::from_value(json!({ "vendor": "Example Software Ltd", "country": "IE", "description": "Annual licence" })).unwrap());
    let r = check(&policy("company"), &[e]).unwrap();
    let a = &r.assessments[0];
    assert_eq!(a.proof, Proof::NotSupported);
    let missing = a
        .findings
        .iter()
        .find(|f| f.code == "foreign-invoice-incomplete")
        .unwrap();
    assert!(
        missing.message.contains("tax_id") && missing.message.contains("buyer"),
        "{}",
        missing.message
    );
    assert!(codes(&r, 0).contains(&"foreign-invoice-review"));
}

#[test]
fn a_policy_with_an_rfc_needs_a_profile() {
    let mut p = policy("company");
    p.profile = None;
    let e = check(&p, &[]).unwrap_err();
    assert!(e.to_string().contains("profile"), "{e}");
}

#[test]
fn expenses_outside_the_fiscal_year_are_flagged() {
    let r = check(
        &policy("company"),
        &[expense(
            receipt("r-1", "2025-12-30", 116000),
            Some("receipts/cfdi-ingreso.xml"),
        )],
    )
    .unwrap();
    let c = codes(&r, 0);
    assert!(
        c.contains(&"outside-fiscal-year") && c.contains(&"cfdi-date-outside-window"),
        "{c:?}"
    );
}
