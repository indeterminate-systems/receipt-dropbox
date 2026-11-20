//! Plans written in Nickel against `nickel/rdx/api.ncl`: the contracts catch mistakes before the
//! executor sees the plan.

use std::path::PathBuf;

use rdx_plan::{LoadError, PlanSource, Registry, StepStatus, execute};

fn nickel_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../nickel")
}

fn load(body: &str) -> Result<rdx_plan::Plan, LoadError> {
    let code = format!(
        r#"let rdx = import "rdx/api.ncl" in
{{
  plan = rdx.plan {{
    skill = "test",
    goal = "Exercise the API",
    steps = {body},
  }},
}}"#
    );
    PlanSource::code(code).import_path(nickel_dir()).load()
}

fn nickel_message(err: LoadError) -> String {
    match err {
        LoadError::Nickel { message, .. } => message,
        other => panic!("expected a Nickel error, got {other:?}"),
    }
}

#[test]
fn a_valid_plan_loads_and_runs() {
    let plan = load(
        r#"[
      rdx.ops.core.note { id = "hello", message = "hi" },
      rdx.ops.core.digest { value = rdx.ref "hello" },
    ]"#,
    )
    .unwrap();
    assert!(plan.dry_run, "plans are dry runs unless they say otherwise");
    assert_eq!(plan.steps[1].args["value"]["ref"], "hello");

    let report = execute(&plan, &Registry::with_core(), &mut rdx_host::Host::new()).unwrap();
    assert!(report.ok);
    assert_eq!(report.notes, vec!["hi"]);
    assert!(report.steps.iter().all(|s| s.status == StepStatus::Done));
}

#[test]
fn contracts_reject_bad_arguments() {
    let message = nickel_message(load(r#"[ rdx.ops.core.note { message = 42 } ]"#).unwrap_err());
    assert!(message.contains("contract"), "{message}");
}

#[test]
fn contracts_reject_unknown_arguments() {
    let message = nickel_message(
        load(r#"[ rdx.ops.core.note { message = "x", colour = "red" } ]"#).unwrap_err(),
    );
    assert!(message.contains("colour"), "{message}");
}

#[test]
fn contracts_reject_references_to_later_steps() {
    let message = nickel_message(
        load(
            r#"[
      rdx.ops.core.digest { value = rdx.ref "later" },
      rdx.ops.core.note { id = "later", message = "x" },
    ]"#,
        )
        .unwrap_err(),
    );
    assert!(message.contains("not an earlier step"), "{message}");
}

#[test]
fn contracts_reject_duplicate_ids() {
    let message = nickel_message(
        load(
            r#"[
      rdx.ops.core.note { id = "a", message = "x" },
      rdx.ops.core.note { id = "a", message = "y" },
    ]"#,
        )
        .unwrap_err(),
    );
    assert!(message.contains("duplicate step id"), "{message}");
}

#[test]
fn inputs_override_top_level_fields() {
    let code = r#"let rdx = import "rdx/api.ncl" in
{
  who | String | default = "nobody",
  plan = rdx.plan {
    skill = "test",
    goal = "Greet",
    steps = [ rdx.ops.core.note { message = "hello %{who}" } ],
  },
}"#;
    let plan = PlanSource::code(code)
        .import_path(nickel_dir())
        .input("who", r#""Ash""#)
        .load()
        .unwrap();
    assert_eq!(plan.steps[0].args["message"], "hello Ash");
}

#[test]
fn host_op_constructors_build_steps() {
    let plan = load(
        r#"[
      rdx.ops.clock.now { id = "now" },
      rdx.ops.kv.get { id = "prev", key = "runs/last" },
      rdx.ops.kv.put { key = "runs/last", value = rdx.ref "now" },
      rdx.ops.mail.send { to = "treasurer@example.org", subject = "Run", text = "done" },
    ]"#,
    )
    .unwrap();
    let ops: Vec<&str> = plan.steps.iter().map(|s| s.op.as_str()).collect();
    assert_eq!(ops, ["clock.now", "kv.get", "kv.put", "mail.send"]);
    assert_eq!(plan.steps[2].args["value"]["ref"], "now");
}

#[test]
fn contracts_check_host_op_arguments() {
    for (body, expect) in [
        (
            r#"[ rdx.ops.mail.send { to = "nobody", subject = "s", text = "t" } ]"#,
            "email address",
        ),
        (
            r#"[ rdx.ops.mail.send { to = "a@example.org", text = "t" } ]"#,
            "subject",
        ),
        (r#"[ rdx.ops.kv.get { key = "" } ]"#, "non-empty"),
        (r#"[ rdx.ops.clock.now { zone = "UTC" } ]"#, "zone"),
    ] {
        let message = nickel_message(load(body).unwrap_err());
        assert!(message.contains(expect), "{body}: {message}");
    }
}
