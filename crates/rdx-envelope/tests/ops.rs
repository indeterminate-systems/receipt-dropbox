//! `envelope.redact` and `envelope.verify` as plan steps.

use bc_rand::make_fake_random_number_generator;
use rdx_envelope::{public_key_to_ur, root_digest, seal, synth, to_ur};
use rdx_host::Host;
use rdx_model::synth::{Config, Scenario};
use rdx_plan::{Plan, Registry, StepStatus, execute};
use serde_json::{Value, json};

fn plan(steps: Value) -> Plan {
    serde_json::from_value(json!({
        "version": 1, "skill": "t", "goal": "g", "dry_run": true, "steps": steps
    }))
    .unwrap()
}

fn registry() -> Registry {
    let mut r = Registry::with_core();
    rdx_envelope::register(&mut r);
    r
}

#[test]
fn redact_then_verify_in_one_plan() {
    let s = Scenario::generate(&Config::small());
    let st = &s.statements[0];
    let sealed = seal(
        st,
        &synth::signer(),
        &mut make_fake_random_number_generator(),
    )
    .unwrap();
    let report = execute(
        &plan(json!([
            { "id": "redacted", "op": "envelope.redact",
              "args": { "envelope": to_ur(&sealed), "matches": s.matches } },
            { "id": "view", "op": "envelope.verify",
              "args": { "envelope": { "ref": "redacted" }, "signer": public_key_to_ur(&synth::verifier()) } },
        ])),
        &registry(),
        &mut Host::new(),
    )
    .unwrap();
    assert!(report.steps.iter().all(|s| s.status == StepStatus::Done));
    let redacted = report.steps[0].output.clone().unwrap();
    let view = report.steps[1].output.clone().unwrap();
    assert_eq!(
        redacted["root"],
        json!(root_digest(&sealed)),
        "elision keeps the root digest"
    );
    assert_eq!(view["root"], redacted["root"]);
    let summary = st.redaction_summary(&s.matches).unwrap();
    assert_eq!(view["summary"], serde_json::to_value(summary).unwrap());
    assert_eq!(view["redacted_rows"], redacted["redacted"]);
}

#[test]
fn errors_say_what_to_fix() {
    let report = execute(
        &plan(json!([
            { "op": "envelope.verify",
              "args": { "envelope": "ur:envelope/nonsense", "signer": public_key_to_ur(&synth::verifier()) } },
        ])),
        &registry(),
        &mut Host::new(),
    )
    .unwrap();
    let error = report.steps[0].error.clone().unwrap();
    assert!(
        error.starts_with("envelope.verify: not a valid `ur:envelope/…` string"),
        "{error}"
    );

    let report = execute(
        &plan(json!([{ "op": "envelope.redact", "args": { "matches": [] } }])),
        &registry(),
        &mut Host::new(),
    )
    .unwrap();
    assert_eq!(
        report.steps[0].error.as_deref(),
        Some(
            "envelope.redact needs `envelope`: a `ur:envelope/…` string, or the output of an \
             earlier `envelope.*` step"
        )
    );
}
