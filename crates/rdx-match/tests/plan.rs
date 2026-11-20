//! `matching.propose` in a Nickel plan: the constructor's contracts and the op's output.

use std::path::{Path, PathBuf};

use rdx_plan::{LoadError, PlanSource, Registry, StepStatus, execute};
use serde_json::Value;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn load(args: &str) -> Result<rdx_plan::Plan, LoadError> {
    let scenario = repo().join("fixtures/synthetic/small/scenario.json");
    let code = format!(
        r#"let rdx = import "rdx/api.ncl" in
let s = import "{}" in
{{
  plan = rdx.plan {{
    skill = "test",
    goal = "Propose matches",
    steps = [
      rdx.ops.matching.propose {{
        id = "proposals",
        statement = std.array.at 0 s.statements,
        receipts = s.receipts,
        {args}
      }},
    ],
  }},
}}"#,
        scenario.display()
    );
    PlanSource::code(code)
        .import_path(repo().join("nickel"))
        .load()
}

fn registry() -> Registry {
    let mut r = Registry::with_core();
    rdx_match::register(&mut r);
    r
}

#[test]
fn proposes_matches_in_a_dry_run() {
    let plan = load("").unwrap();
    let report = execute(&plan, &registry(), &mut rdx_plan::rdx_host::Host::new()).unwrap();
    assert!(report.ok, "{report:?}");
    let step = &report.steps[0];
    assert_eq!(
        step.status,
        StepStatus::Done,
        "no effects, so it runs in a dry run"
    );
    let output = step.output.as_ref().unwrap();
    assert_eq!(output["statement"], "st-2026-03.s-0001");
    assert_eq!(output["summary"]["to_review"], 0);
    let kinds: Vec<&Value> = output["proposals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| &p["kind"])
        .collect();
    assert!(kinds.iter().all(|k| *k == "exact"), "{kinds:?}");
}

#[test]
fn the_policy_is_tuned_in_nickel() {
    let plan =
        load("policy = { amount.tolerance_minor = 25, date.receipt.days_before = 0 },").unwrap();
    let policy = &plan.steps[0].args["policy"];
    assert_eq!(policy["amount"]["tolerance_minor"], 25);
    assert_eq!(policy["date"]["receipt"]["days_before"], 0);
    // Untouched tolerances keep their defaults.
    assert_eq!(policy["merchant"]["min_similarity"], 60);
    let report = execute(&plan, &registry(), &mut rdx_plan::rdx_host::Host::new()).unwrap();
    assert!(report.ok, "{report:?}");
}

#[test]
fn the_contract_rejects_a_bad_policy() {
    let err = load("policy = { merchant.min_similarity = 0.5 },").unwrap_err();
    let LoadError::Nickel { message, .. } = err else {
        panic!("expected a Nickel error, got {err:?}");
    };
    assert!(message.contains("percentage"), "{message}");
}

#[test]
fn the_op_rejects_json_arguments_it_cannot_read() {
    let plan: rdx_plan::Plan = serde_json::from_value(serde_json::json!({
        "version": 1, "skill": "t", "goal": "g", "dry_run": true,
        "steps": [{ "op": "matching.propose", "args": { "statement": {}, "receipts": [] } }]
    }))
    .unwrap();
    let report = execute(&plan, &registry(), &mut rdx_plan::rdx_host::Host::new()).unwrap();
    assert!(!report.ok);
    let error = report.steps[0].error.as_deref().unwrap();
    assert!(
        error.starts_with("matching.propose: invalid arguments"),
        "{error}"
    );
}
