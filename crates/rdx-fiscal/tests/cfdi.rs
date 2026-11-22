//! CFDI 4.0 parsing: the fields fiscal checks need, from the synthetic fixtures.

use std::path::{Path, PathBuf};

use rdx_fiscal::{Cfdi, CfdiError};
use rdx_model::{Currency, Date, Money};

fn fixture(name: &str) -> String {
    let base: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    std::fs::read_to_string(base.join(name)).unwrap()
}

fn mxn(minor: i64) -> Money {
    Money::new(minor, Currency::new("MXN").unwrap())
}

#[test]
fn reads_an_invoice_to_the_organization() {
    let c = Cfdi::parse(&fixture("receipts/cfdi-ingreso.xml")).unwrap();
    assert_eq!(c.uuid, "6F1E2D3C-4B5A-4978-8899-AABBCCDDEEFF");
    assert_eq!(c.version, "4.0");
    assert_eq!(c.cfdi_type, "I");
    assert_eq!(c.issued, "2026-03-12".parse::<Date>().unwrap());
    assert_eq!(c.issuer.rfc, "EKU9003173C9");
    assert_eq!(c.receiver.rfc, "XIA190128J61");
    assert_eq!(c.receiver.name.as_deref(), Some("ORGANIZACIÓN DE PRUEBA"));
    assert_eq!(c.receiver.postcode.as_deref(), Some("06000"));
    assert_eq!(c.receiver.regime.as_deref(), Some("603"));
    assert_eq!(c.receiver.uso.as_deref(), Some("G03"));
    assert_eq!(c.total, Some(mxn(116000)));
    assert_eq!(c.payment_form.as_deref(), Some("04"));
    assert_eq!(c.payment_method.as_deref(), Some("PUE"));
    assert_eq!(c.concepts.len(), 1);
    assert_eq!(c.concepts[0].product_code, "14111500");
    assert_eq!(c.concepts[0].amount, mxn(100000));
    assert!(c.payments.is_empty());
}

#[test]
fn reads_concepts_with_more_decimals_than_the_currency() {
    let c = Cfdi::parse(&fixture("fiscal/cfdi-fuel-cash.xml")).unwrap();
    assert_eq!(c.payment_form.as_deref(), Some("01"));
    assert_eq!(c.concepts[0].product_code, "15101514");
    assert_eq!(c.total, Some(mxn(250000)));
}

#[test]
fn reads_a_payment_complement() {
    let p = Cfdi::parse(&fixture("fiscal/pago-ppd.xml")).unwrap();
    assert_eq!(p.cfdi_type, "P");
    // Moneda XXX: a payment complement has no total of its own.
    assert_eq!(p.total, None);
    assert_eq!(p.payments.len(), 1);
    // UUIDs compare case-insensitively: the complement wrote it in lower case.
    assert_eq!(p.payments[0].uuid, "1A2B3C4D-0000-4000-8000-000000000004");
    assert_eq!(p.payments[0].amount, mxn(580000));
    assert_eq!(p.payments[0].date, "2026-03-19".parse::<Date>().unwrap());
    assert_eq!(p.payments[0].form.as_deref(), Some("04"));
}

#[test]
fn refuses_what_was_never_stamped() {
    let e = Cfdi::parse(&fixture("fiscal/cfdi-unstamped.xml")).unwrap_err();
    assert_eq!(e, CfdiError::Unstamped);
    assert!(e.to_string().contains("never stamped"), "{e}");
}

#[test]
fn refuses_other_xml() {
    assert_eq!(
        Cfdi::parse("<invoice total=\"1\"/>").unwrap_err(),
        CfdiError::NotACfdi
    );
    assert!(matches!(
        Cfdi::parse("not xml").unwrap_err(),
        CfdiError::Xml(_)
    ));
}
