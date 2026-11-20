//! `receipt.extract` in a Nickel plan, against `nickel/rdx/api.ncl`.

use std::path::PathBuf;

use rdx_host::Host;
use rdx_plan::{LoadError, PlanSource, Registry, StepStatus, execute};
use serde_json::json;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn load(steps: &str) -> Result<rdx_plan::Plan, LoadError> {
    let code = format!(
        r#"let rdx = import "rdx/api.ncl" in
{{
  plan = rdx.plan {{ skill = "test", goal = "Extract a receipt", steps = {steps} }},
}}"#
    );
    PlanSource::code(code)
        .import_path(root().join("nickel"))
        .load()
}

fn registry() -> Registry {
    let mut r = Registry::with_core();
    rdx_receipt::register(&mut r);
    r
}

fn nickel_message(err: LoadError) -> String {
    match err {
        LoadError::Nickel { message, .. } => message,
        other => panic!("expected a Nickel error, got {other:?}"),
    }
}

#[test]
fn extracts_an_imported_email_into_a_receipt() {
    let fixture = root().join("fixtures/receipts/bill-email.eml");
    let plan = load(&format!(
        r#"[
      rdx.ops.receipt.extract {{
        id = "receipt",
        supporter = "s-0001",
        document = import "{}" as 'Text,
        hints = {{ currency = "USD" }},
      }},
    ]"#,
        fixture.display()
    ))
    .unwrap();
    let report = execute(&plan, &registry(), &mut Host::new()).unwrap();
    assert!(report.ok, "{report:#?}");
    let step = &report.steps[0];
    assert_eq!(step.status, StepStatus::Done);
    let out = step.output.as_ref().unwrap();
    assert_eq!(out["status"], "complete");
    let receipt = &out["receipt"];
    assert_eq!(receipt["supporter"], "s-0001");
    assert_eq!(receipt["merchant"], "Paperleaf Supply");
    assert_eq!(
        receipt["total"],
        json!({ "minor": 27989, "currency": "USD" })
    );
    assert_eq!(receipt["kind"], "bill");
    assert_eq!(receipt["source"], out["source"]);
    let source = out["source"].as_str().unwrap();
    assert_eq!(receipt["id"], format!("r-{}", &source[7..23]));
}

#[test]
fn takes_the_output_of_pdf_text() {
    // What `pdf.text` (piece 4) returns: the PDF's digest and its text layer.
    let pdf = format!("sha256:{}", "ab".repeat(32));
    let plan = load(&format!(
        r#"[
      rdx.ops.receipt.extract {{
        supporter = "s-0001",
        receipt_id = "r-pdf-1",
        document = {{ document = "{pdf}", pages = 1, text = "Shop\nDate: 2026-03-04\nTotal EUR 9.50\n" }},
      }},
    ]"#
    ))
    .unwrap();
    let report = execute(&plan, &registry(), &mut Host::new()).unwrap();
    let out = report.steps[0].output.as_ref().unwrap();
    assert_eq!(out["receipt"]["id"], "r-pdf-1");
    assert_eq!(out["receipt"]["source"], pdf.as_str());
    assert_eq!(out["receipt"]["total"]["currency"], "EUR");
}

#[test]
fn needs_review_has_no_receipt() {
    let plan = load(
        r#"[ rdx.ops.receipt.extract { supporter = "s-0001", media_type = "text/plain", document = "Shop\nTotal $5.00\n" } ]"#,
    )
    .unwrap();
    let report = execute(&plan, &registry(), &mut Host::new()).unwrap();
    assert!(
        report.ok,
        "a document that needs review is a result, not a failure"
    );
    let out = report.steps[0].output.as_ref().unwrap();
    assert_eq!(out["status"], "needs_review");
    assert_eq!(out["receipt"], serde_json::Value::Null);
    assert_eq!(out["missing"], json!(["date", "currency"]));
}

#[test]
fn contract_rejects_an_unknown_media_type() {
    let message = nickel_message(
        load(r#"[ rdx.ops.receipt.extract { supporter = "s-0001", media_type = "application/pdf", document = "x" } ]"#)
            .unwrap_err(),
    );
    assert!(message.contains("media type"), "{message}");
}

#[test]
fn contract_rejects_a_bad_currency_hint() {
    let message = nickel_message(
        load(r#"[ rdx.ops.receipt.extract { supporter = "s-0001", document = "x", hints = { currency = "usd" } } ]"#)
            .unwrap_err(),
    );
    assert!(message.contains("currency"), "{message}");
}
