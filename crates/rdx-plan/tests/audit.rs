//! Every run is recorded in the host's audit log as a hash chain.

use rdx_host::{AuditLog, Host, memory::MemoryAuditLog};
use rdx_plan::{AuditError, Plan, Registry, audit, run};
use serde_json::{Value, json};

fn plan(message: &str) -> Plan {
    serde_json::from_value(json!({
        "version": 1, "skill": "t", "goal": "g",
        "steps": [{ "op": "core.note", "args": { "message": message } }]
    }))
    .unwrap()
}

#[test]
fn runs_are_chained_in_the_audit_log() {
    let log = MemoryAuditLog::default();
    let mut host = Host::memory().with_audit_log(log.clone());
    let first = run(&plan("one"), &Registry::with_core(), &mut host).unwrap();
    let second = run(&plan("two"), &Registry::with_core(), &mut host).unwrap();

    let lines = log.lines();
    assert_eq!(lines.len(), 2);
    let entries: Vec<Value> = lines
        .iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(entries[0]["entry"]["prev"], Value::Null);
    assert_eq!(entries[1]["entry"]["prev"], entries[0]["digest"]);
    assert_eq!(entries[0]["digest"], json!(first.audit.clone().unwrap()));
    assert_eq!(entries[1]["entry"]["report"]["notes"], json!(["two"]));
    assert_eq!(
        entries[0]["entry"]["at_unix_ms"],
        json!(rdx_host::memory::FixedClock::GOLDEN)
    );
    assert_eq!(second.report.notes, vec!["two"]);

    assert_eq!(audit::verify(lines.iter().map(String::as_str)), Ok(2));
}

#[test]
fn tampering_breaks_the_chain() {
    let log = MemoryAuditLog::default();
    let mut host = Host::memory().with_audit_log(log.clone());
    for m in ["one", "two", "three"] {
        run(&plan(m), &Registry::with_core(), &mut host).unwrap();
    }
    let mut lines = log.lines();
    lines[1] = lines[1].replace("two", "TWO");
    assert_eq!(
        audit::verify(lines.iter().map(String::as_str)),
        Err(AuditError::Digest { line: 2 })
    );

    let mut lines = log.lines();
    lines.remove(1);
    assert_eq!(
        audit::verify(lines.iter().map(String::as_str)),
        Err(AuditError::Chain { line: 2 })
    );
}

#[test]
fn hosts_without_an_audit_log_still_run() {
    let mut host = Host::new();
    let outcome = run(&plan("x"), &Registry::with_core(), &mut host).unwrap();
    assert!(outcome.report.ok);
    assert_eq!(outcome.audit, None);
}

#[test]
fn a_last_line_that_is_not_an_entry_is_an_error() {
    let mut log = MemoryAuditLog::default();
    log.append("garbage").unwrap();
    let mut host = Host::memory().with_audit_log(log);
    assert!(run(&plan("x"), &Registry::with_core(), &mut host).is_err());
}

#[test]
fn the_log_keeps_digests_of_outputs_and_skipped_arguments_not_the_values() {
    let log = MemoryAuditLog::default();
    let mut host = Host::memory().with_audit_log(log.clone());
    host.kv
        .as_mut()
        .unwrap()
        .put("statement", br#"{"l05":"PHARMACY -312.40"}"#)
        .unwrap();
    // A dry run: the read's output and the skipped write's arguments both hold the line.
    let plan: Plan = serde_json::from_value(json!({
        "version": 1, "skill": "t", "goal": "g", "dry_run": true,
        "steps": [
            { "id": "secret", "op": "kv.get", "args": { "key": "statement" } },
            { "op": "kv.put", "args": { "key": "copy", "value": { "ref": "secret" } } }
        ]
    }))
    .unwrap();
    let outcome = run(&plan, &Registry::with_core(), &mut host).unwrap();
    // The caller still gets everything.
    let note = &outcome.report.steps[0];
    assert_eq!(note.output, Some(json!({ "l05": "PHARMACY -312.40" })));
    assert!(outcome.report.steps[1].args.is_some());

    let line = &log.lines()[0];
    assert!(!line.contains("PHARMACY"), "{line}");
    let entry: Value = serde_json::from_str(line).unwrap();
    let steps = &entry["entry"]["report"]["steps"];
    assert_eq!(steps[0].get("output"), None);
    assert_eq!(steps[0]["output_digest"], json!(note.output_digest));
    assert_eq!(steps[1].get("args"), None);
    assert_eq!(
        steps[1]["args_digest"],
        json!(rdx_plan::digest(
            outcome.report.steps[1].args.as_ref().unwrap()
        ))
    );
}
