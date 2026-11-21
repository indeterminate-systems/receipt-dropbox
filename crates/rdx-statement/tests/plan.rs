//! `statement.parse` and `pdf.text` in plans: Nickel contracts first, then the ops on a host
//! whose blob store holds the documents.

use std::path::{Path, PathBuf};

use rdx_host::Host;
use rdx_plan::{LoadError, PlanSource, Registry, StepStatus, execute};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn registry() -> Registry {
    let mut r = Registry::with_core();
    rdx_pdf::register(&mut r);
    rdx_statement::register(&mut r);
    r
}

fn load(steps: &str) -> Result<rdx_plan::Plan, LoadError> {
    PlanSource::code(format!(
        r#"let rdx = import "rdx/api.ncl" in {{ plan = rdx.plan {{ skill = "t", goal = "g", steps = {steps} }} }}"#
    ))
    .import_path(repo().join("nickel"))
    .load()
}

/// A memory host holding the fixture, and the fixture's digest.
fn host_with(fixture: &str) -> (Host, String) {
    let bytes = std::fs::read(
        repo()
            .join("fixtures/synthetic/small/statements")
            .join(fixture),
    )
    .unwrap();
    let mut host = Host::memory();
    let digest = host.blob_store.as_mut().unwrap().put(&bytes).unwrap();
    (host, digest)
}

#[test]
fn parse_reads_a_pdf_from_the_blob_store() {
    let (mut host, digest) = host_with("st-2026-03.s-0001.pdf");
    let plan = load(&format!(
        r#"[ rdx.ops.statement.parse {{ id = "st", document = "{digest}", supporter = "s-0001" }} ]"#
    ))
    .unwrap();
    let report = execute(&plan, &registry(), &mut host).unwrap();
    assert!(report.ok, "{report:?}");
    let out = report.steps[0].output.as_ref().unwrap();
    assert_eq!(out["parser"], "example-bank");
    assert_eq!(out["statement"]["id"], "st-2026-03.s-0001");
    assert_eq!(out["statement"]["original"], digest.as_str());
    assert_eq!(out["checks"]["opening"], "printed");
}

#[test]
fn pdf_text_gives_lines_and_plain_text() {
    let (mut host, digest) = host_with("st-2026-03.s-0001.typst.pdf");
    let plan = load(&format!(
        r#"[ rdx.ops.pdf.text {{ document = "{digest}" }} ]"#
    ))
    .unwrap();
    let report = execute(&plan, &registry(), &mut host).unwrap();
    let out = report.steps[0].output.as_ref().unwrap();
    assert_eq!(out["document"], digest.as_str());
    let text = out["text"].as_str().unwrap();
    assert!(
        text.contains("03/03  LANTERN CAFE CATERING #7435  289.74  454.81"),
        "{text}"
    );
    assert!(out["pages"][0]["lines"][0]["spans"][0]["x_end"].is_number());
}

#[test]
fn a_missing_document_says_how_to_add_it() {
    let digest = format!("sha256:{}", "0".repeat(64));
    let plan = load(&format!(
        r#"[ rdx.ops.statement.parse {{ document = "{digest}", supporter = "s-0001" }} ]"#
    ))
    .unwrap();
    let report = execute(&plan, &registry(), &mut Host::memory()).unwrap();
    assert_eq!(report.steps[0].status, StepStatus::Failed);
    assert!(
        report.steps[0]
            .error
            .as_deref()
            .unwrap()
            .contains("rdx blob put")
    );
}

#[test]
fn contracts_reject_bad_arguments_before_anything_runs() {
    let digest = format!("sha256:{}", "0".repeat(64));
    for (args, expect) in [
        (
            r#"document = "march.pdf", supporter = "s-0001""#,
            "document digest",
        ),
        (
            format!(r#"document = "{digest}", supporter = "S 1""#).as_str(),
            "invalid id",
        ),
        (
            format!(r#"document = "{digest}", supporter = "s-1", currency = "XYZ""#).as_str(),
            "invalid currency",
        ),
        (
            format!(r#"document = "{digest}", supporter = "s-1", opening = 744.55"#).as_str(),
            "decimal text",
        ),
        (
            format!(r#"document = "{digest}", supporter = "s-1", format = "qif""#).as_str(),
            "unknown statement format",
        ),
    ] {
        let err = load(&format!("[ rdx.ops.statement.parse {{ {args} }} ]")).unwrap_err();
        let LoadError::Nickel { message, .. } = &err else {
            panic!("{err:?}")
        };
        assert!(message.contains(expect), "{args}: {message}");
    }
}

/// The scanned statement (piece 11's fixture): a page image, no text layer.
fn scan_host() -> (Host, String) {
    let bytes = std::fs::read(repo().join("fixtures/scans/st-2026-03.s-0001.scan.pdf")).unwrap();
    let mut host = Host::memory();
    let digest = host.blob_store.as_mut().unwrap().put(&bytes).unwrap();
    (host, digest)
}

fn run_json(plan: serde_json::Value, host: &mut Host) -> rdx_plan::RunReport {
    let plan: rdx_plan::Plan = serde_json::from_value(plan).unwrap();
    execute(&plan, &registry(), host).unwrap()
}

#[test]
fn a_scan_without_text_says_to_read_it_with_ocr() {
    let (mut host, digest) = scan_host();
    let plan = load(&format!(
        r#"[ rdx.ops.statement.parse {{ document = "{digest}", supporter = "s-0001" }} ]"#
    ))
    .unwrap();
    let report = execute(&plan, &registry(), &mut host).unwrap();
    let err = report.steps[0].error.as_deref().unwrap();
    assert!(err.contains("ocr.read"), "{err}");
}

#[test]
fn a_scan_parses_from_the_text_ocr_read_gives() {
    let (mut host, digest) = scan_host();
    // Stands in for `ocr.read`'s output (crate rdx-ocr, which tests the real thing): the same
    // statement's lines and positions, for this document.
    let typst = std::fs::read(
        repo().join("fixtures/synthetic/small/statements/st-2026-03.s-0001.typst.pdf"),
    )
    .unwrap();
    let pages = rdx_pdf::text(&typst).unwrap().pages;
    let text = serde_json::json!({ "document": digest, "source": "ocr", "pages": pages });
    let report = run_json(
        serde_json::json!({
            "version": 1, "skill": "t", "goal": "g",
            "steps": [{ "id": "st", "op": "statement.parse",
                        "args": { "document": digest, "supporter": "s-0001", "text": text } }],
        }),
        &mut host,
    );
    assert!(report.ok, "{report:?}");
    let out = report.steps[0].output.as_ref().unwrap();
    assert_eq!(out["parser"], "example-bank");
    assert_eq!(out["statement"]["original"], digest.as_str());
}

#[test]
fn text_read_from_another_document_is_refused() {
    let (mut host, digest) = scan_host();
    let text = serde_json::json!({ "document": format!("sha256:{}", "1".repeat(64)), "pages": [] });
    let report = run_json(
        serde_json::json!({
            "version": 1, "skill": "t", "goal": "g",
            "steps": [{ "op": "statement.parse",
                        "args": { "document": digest, "supporter": "s-0001", "text": text } }],
        }),
        &mut host,
    );
    let err = report.steps[0].error.as_deref().unwrap();
    assert!(err.contains("another document"), "{err}");
}

#[test]
fn a_text_layer_wins_over_text_passed_in() {
    let (mut host, digest) = host_with("st-2026-03.s-0001.pdf");
    // Junk "OCR" text: ignored, because the PDF has a text layer of its own.
    let text = serde_json::json!({ "document": digest, "pages": [{ "lines": [] }] });
    let report = run_json(
        serde_json::json!({
            "version": 1, "skill": "t", "goal": "g",
            "steps": [{ "op": "statement.parse",
                        "args": { "document": digest, "supporter": "s-0001", "text": text } }],
        }),
        &mut host,
    );
    assert!(report.ok, "{report:?}");
}
