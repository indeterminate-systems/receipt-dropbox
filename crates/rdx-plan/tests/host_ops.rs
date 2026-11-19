//! Ops that reach the host: capability checks, dry runs and effects.

use rdx_host::{
    Capability, Host, Kv,
    memory::{FixedClock, MemoryKv, Outbox},
};
use rdx_plan::{Plan, PlanError, Registry, StepStatus, execute};
use serde_json::{Value, json};

fn plan(dry_run: bool, steps: Value) -> Plan {
    serde_json::from_value(json!({
        "version": 1, "skill": "t", "goal": "g", "dry_run": dry_run, "steps": steps
    }))
    .unwrap()
}

#[test]
fn a_plan_needing_a_capability_the_host_lacks_runs_nothing() {
    let outbox = Outbox::default();
    let mut host = Host::new().with_mail_out(outbox.clone());
    let err = execute(
        &plan(
            false,
            json!([
                { "op": "mail.send", "args": { "to": "a@example.org", "subject": "s", "text": "t" } },
                { "op": "kv.put", "args": { "key": "k", "value": 1 } }
            ]),
        ),
        &Registry::with_core(),
        &mut host,
    )
    .unwrap_err();
    assert_eq!(
        err,
        PlanError::MissingCapability {
            op: "kv.put".into(),
            capability: Capability::Kv
        }
    );
    assert_eq!(
        err.to_string(),
        "op `kv.put` needs the `kv` capability, which this host does not offer"
    );
    assert!(outbox.sent().is_empty());
}

#[test]
fn dry_runs_show_what_effects_would_do_and_leave_the_host_alone() {
    let kv = MemoryKv::default();
    let mut host = Host::new().with_kv(kv.clone()).with_clock(FixedClock(42));
    let report = execute(
        &plan(
            true,
            json!([
                { "id": "now", "op": "clock.now" },
                { "op": "kv.put", "args": { "key": "last", "value": { "ref": "now" } } }
            ]),
        ),
        &Registry::with_core(),
        &mut host,
    )
    .unwrap();
    assert!(report.ok);
    assert_eq!(report.steps[0].output, Some(json!({ "unix_ms": 42 })));
    assert_eq!(report.steps[1].status, StepStatus::Skipped);
    assert_eq!(
        report.steps[1].args,
        Some(json!({ "key": "last", "value": { "unix_ms": 42 } }))
    );
    assert_eq!(kv.get("last").unwrap(), None);
}

#[test]
fn executed_runs_perform_effects() {
    let kv = MemoryKv::default();
    let outbox = Outbox::default();
    let mut host = Host::new()
        .with_kv(kv.clone())
        .with_mail_out(outbox.clone());
    let report = execute(
        &plan(
            false,
            json!([
                { "op": "kv.put", "args": { "key": "greeting", "value": { "text": "hi" } } },
                { "id": "got", "op": "kv.get", "args": { "key": "greeting" } },
                { "op": "mail.send", "args": { "to": "a@example.org", "subject": "s", "text": "t" } }
            ]),
        ),
        &Registry::with_core(),
        &mut host,
    )
    .unwrap();
    assert!(report.ok, "{report:?}");
    assert!(report.steps.iter().all(|s| s.status == StepStatus::Done));
    assert_eq!(report.steps[1].output, Some(json!({ "text": "hi" })));
    assert!(
        report.steps[0].args.is_none(),
        "only skipped steps echo their arguments"
    );
    assert_eq!(
        kv.get("greeting").unwrap().as_deref(),
        Some(&br#"{"text":"hi"}"#[..])
    );
    assert_eq!(outbox.sent().len(), 1);
}

#[test]
fn missing_kv_values_are_null() {
    let mut host = Host::new().with_kv(MemoryKv::default());
    let report = execute(
        &plan(
            true,
            json!([{ "op": "kv.get", "args": { "key": "nothing" } }]),
        ),
        &Registry::with_core(),
        &mut host,
    )
    .unwrap();
    assert_eq!(report.steps[0].output, Some(Value::Null));
}

#[test]
fn capability_errors_fail_the_step() {
    let kv = MemoryKv::default();
    let mut raw = kv.clone();
    raw.put("bad", b"not json").unwrap();
    let mut host = Host::new().with_kv(kv);
    let report = execute(
        &plan(true, json!([{ "op": "kv.get", "args": { "key": "bad" } }])),
        &Registry::with_core(),
        &mut host,
    )
    .unwrap();
    assert!(!report.ok);
    assert!(
        report.steps[0]
            .error
            .as_deref()
            .unwrap()
            .contains("not JSON"),
        "{:?}",
        report.steps[0].error
    );
}
