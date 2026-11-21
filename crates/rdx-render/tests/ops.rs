//! `render.statement` as a plan step, chained after `envelope.redact`.

use bc_rand::make_fake_random_number_generator;
use rdx_envelope::{public_key_to_ur, seal, synth, to_ur};
use rdx_host::Host;
use rdx_model::synth::{Config, Scenario};
use rdx_plan::{Plan, Registry, StepStatus, execute};
use serde_json::{Value, json};

fn plan(dry_run: bool, steps: Value) -> Plan {
    serde_json::from_value(json!({
        "version": 1, "skill": "t", "goal": "g", "dry_run": dry_run, "steps": steps
    }))
    .unwrap()
}

fn registry() -> Registry {
    let mut r = Registry::with_core();
    rdx_envelope::register(&mut r);
    rdx_render::register(&mut r);
    r
}

fn steps() -> Value {
    let s = Scenario::generate(&Config::small());
    let sealed = seal(
        &s.statements[0],
        &synth::signer(),
        &mut make_fake_random_number_generator(),
    )
    .unwrap();
    let signer = public_key_to_ur(&synth::verifier());
    json!([
        { "id": "redacted", "op": "envelope.redact",
          "args": { "envelope": to_ur(&sealed), "matches": s.matches } },
        { "id": "pdf", "op": "render.statement",
          "args": { "envelope": { "ref": "redacted" }, "signer": signer } },
    ])
}

#[test]
fn stores_the_pdf_and_reports_what_it_shows() {
    let mut host = Host::memory();
    let report = execute(&plan(false, steps()), &registry(), &mut host).unwrap();
    assert!(report.ok, "{report:?}");
    let redacted = report.steps[0].output.clone().unwrap();
    let out = report.steps[1].output.clone().unwrap();
    assert_eq!(out["root"], redacted["root"]);
    assert_eq!(out["shown"], redacted["shown"]);
    assert_eq!(out["redacted_rows"], redacted["redacted"]);
    assert_eq!(out["qr"], json!(true));
    let digest = out["document"].as_str().unwrap();
    let pdf = host
        .blob_store
        .as_ref()
        .unwrap()
        .get(digest)
        .unwrap()
        .unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
}

#[test]
fn a_dry_run_stores_nothing() {
    let mut host = Host::memory();
    let report = execute(&plan(true, steps()), &registry(), &mut host).unwrap();
    assert_eq!(report.steps[1].status, StepStatus::Skipped);
}

#[test]
fn says_what_is_wrong_with_the_arguments() {
    let report = execute(
        &plan(
            false,
            json!([{ "op": "render.statement", "args": { "envelope": 3, "signer": "x" } }]),
        ),
        &registry(),
        &mut Host::memory(),
    )
    .unwrap();
    let err = report.steps[0].error.clone().unwrap();
    assert!(err.contains("needs `envelope`"), "{err}");
}
