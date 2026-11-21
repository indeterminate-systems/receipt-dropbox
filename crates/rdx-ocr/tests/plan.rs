//! `ocr.read` in plans, with the real models in the blob store: scans feed the statement parser
//! and the receipt extractor exactly as text PDFs do.

mod common;

use std::path::Path;

use common::{fixture, model_bytes, repo};
use rdx_host::Host;
use rdx_plan::{PlanSource, Registry, RunReport, StepStatus, execute};

fn registry() -> Registry {
    let mut r = Registry::with_core();
    rdx_pdf::register(&mut r);
    rdx_ocr::register(&mut r);
    rdx_statement::register(&mut r);
    rdx_receipt::register(&mut r);
    r
}

/// A memory host holding `file` and, if `with_models`, the OCR models.
fn host_with(file: &Path, with_models: bool) -> (Host, String) {
    let mut host = Host::memory();
    let store = host.blob_store.as_mut().unwrap();
    if with_models {
        let (d, r) = model_bytes();
        store.put(&d).unwrap();
        store.put(&r).unwrap();
    }
    let digest = store.put(&std::fs::read(file).unwrap()).unwrap();
    (host, digest)
}

fn run(steps: &str, host: &mut Host) -> RunReport {
    let plan = PlanSource::code(format!(
        r#"let rdx = import "rdx/api.ncl" in {{ plan = rdx.plan {{ skill = "t", goal = "g", steps = {steps} }} }}"#
    ))
    .import_path(repo().join("nickel"))
    .load()
    .unwrap();
    execute(&plan, &registry(), host).unwrap()
}

#[test]
fn a_scanned_statement_parses_and_adds_up() {
    let (mut host, digest) = host_with(&fixture("st-2026-03.s-0001.scan.pdf"), true);
    let report = run(
        &format!(
            r#"[
              rdx.ops.ocr.read {{ id = "text", document = "{digest}" }},
              rdx.ops.statement.parse {{ id = "st", document = "{digest}", supporter = "s-0001", text = rdx.ref "text" }},
            ]"#
        ),
        &mut host,
    );
    assert!(report.ok, "{report:#?}");
    let read = report.steps[0].output.as_ref().unwrap();
    assert_eq!(read["source"], "ocr");
    let out = report.steps[1].output.as_ref().unwrap();
    assert_eq!(out["parser"], "example-bank");

    // The same statement as its text PDF gives, apart from the original it links to and OCR's
    // misreadings of descriptions (`PAPERI FAF` for `PAPERLEAF`; the page footer joined to the
    // last line). Every date and amount is exact: the balance checks would refuse it otherwise.
    let (mut text_host, text_digest) = host_with(
        &repo().join("fixtures/synthetic/small/statements/st-2026-03.s-0001.pdf"),
        false,
    );
    let from_text = run(
        &format!(
            r#"[ rdx.ops.statement.parse {{ document = "{text_digest}", supporter = "s-0001" }} ]"#
        ),
        &mut text_host,
    );
    let mut expected = from_text.steps[0].output.clone().unwrap()["statement"].clone();
    expected["original"] = digest.clone().into();
    let mut actual = out["statement"].clone();
    let descriptions = |st: &mut serde_json::Value| -> Vec<String> {
        st["lines"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .map(|l| l["description"].take().as_str().unwrap().to_owned())
            .collect()
    };
    let (read, truth) = (descriptions(&mut actual), descriptions(&mut expected));
    assert_eq!(actual, expected);
    let same = read.iter().zip(&truth).filter(|(a, b)| a == b).count();
    assert!(same >= truth.len() - 2, "{read:#?}");
}

#[test]
fn a_receipt_photo_becomes_a_receipt() {
    let (mut host, digest) = host_with(&fixture("receipt-photo.png"), true);
    let report = run(
        &format!(
            r#"[
              rdx.ops.ocr.read {{ id = "text", document = "{digest}" }},
              rdx.ops.receipt.extract {{ id = "receipt", supporter = "s-0001", document = rdx.ref "text" }},
            ]"#
        ),
        &mut host,
    );
    assert!(report.ok, "{report:#?}");
    let out = report.steps[1].output.as_ref().unwrap();
    assert_eq!(out["status"], "complete", "{out:#}");
    assert_eq!(out["source"], digest.as_str());
    assert_eq!(out["receipt"]["total"]["minor"], 34278, "{out:#}");
    assert_eq!(out["receipt"]["total"]["currency"], "MXN");
    assert_eq!(out["receipt"]["date"], "2026-03-19");
}

#[test]
fn missing_models_say_how_to_add_them() {
    let (mut host, digest) = host_with(&fixture("receipt-photo.png"), false);
    let report = run(
        &format!(r#"[ rdx.ops.ocr.read {{ document = "{digest}" }} ]"#),
        &mut host,
    );
    assert_eq!(report.steps[0].status, StepStatus::Failed);
    let err = report.steps[0].error.as_deref().unwrap();
    assert!(err.contains("rdx blob put text-detection.rten"), "{err}");
}

#[test]
fn a_text_pdf_needs_no_models() {
    let (mut host, digest) = host_with(&repo().join("fixtures/receipts/receipt-pdf.pdf"), false);
    let report = run(
        &format!(r#"[ rdx.ops.ocr.read {{ document = "{digest}" }} ]"#),
        &mut host,
    );
    assert!(report.ok, "{report:#?}");
    assert_eq!(
        report.steps[0].output.as_ref().unwrap()["source"],
        "text-layer"
    );
}
