//! Password-protected statements in plans: `statement.unlock` asks the supporter for the password
//! on their device (the `secrets` capability) or uses one remembered for the bank account,
//! encrypted to their keys; `statement.remember-password` keeps one. The password itself never
//! appears in a plan, an output or the audit log.

use std::path::{Path, PathBuf};

use rdx_host::Host;
use rdx_host::memory::MemorySecrets;
use rdx_plan::{Plan, Registry, RunReport, StepStatus, execute};
use serde_json::{Value, json};

const ADDR: &str = "ana@example.org";
const PASSWORD: &str = "XAXX010101000";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn registry() -> Registry {
    let mut r = Registry::with_core();
    rdx_keys::register(&mut r);
    rdx_pdf::register(&mut r);
    rdx_statement::register(&mut r);
    r
}

fn run(host: &mut Host, steps: Value) -> RunReport {
    let plan: Plan = serde_json::from_value(json!({
        "version": 1, "skill": "t", "goal": "g", "dry_run": false, "steps": steps
    }))
    .unwrap();
    let report = execute(&plan, &registry(), host).unwrap();
    // The password must never reach a report (and so the audit log).
    let text = serde_json::to_string(&report).unwrap();
    assert!(!text.contains(PASSWORD), "the password leaked: {text}");
    report
}

fn ok(report: &RunReport) {
    for s in &report.steps {
        assert_eq!(s.status, StepStatus::Done, "{}: {:?}", s.op, s.error);
    }
}

fn out<'a>(report: &'a RunReport, id: &str) -> &'a Value {
    report
        .steps
        .iter()
        .find(|s| s.id.as_deref() == Some(id))
        .and_then(|s| s.output.as_ref())
        .unwrap()
}

fn error(report: &RunReport) -> String {
    report
        .steps
        .iter()
        .find_map(|s| s.error.clone())
        .expect("a step failed")
}

/// A supporter's device with keys, holding the locked statement, and the statement's digest.
fn device(secrets: &MemorySecrets) -> (Host, String) {
    let mut host = Host::memory().with_secrets(secrets.clone());
    ok(&run(
        &mut host,
        json!([{ "op": "keys.onboard", "args": {
            "address": ADDR, "supporter": "s-0001", "device": "laptop",
            "threshold": 2, "holders": ["phone", "card", "custodian"] } }]),
    ));
    let bytes = std::fs::read(
        repo().join("fixtures/synthetic/small/statements/st-2026-03.s-0001.locked.pdf"),
    )
    .unwrap();
    let digest = host.blob_store.as_mut().unwrap().put(&bytes).unwrap();
    (host, digest)
}

/// The label the device asks the person under, for the document `digest`.
fn label(digest: &str) -> String {
    format!("statement-password-{}", &digest["sha256:".len()..][..12])
}

fn ingest(digest: &str, remember: bool) -> Value {
    json!([
        { "id": "text", "op": "statement.unlock",
          "args": { "document": digest, "address": ADDR, "remember": remember } },
        { "id": "statement", "op": "statement.parse",
          "args": { "document": digest, "supporter": "s-0001", "text": { "ref": "text" } } },
        { "id": "kept", "op": "statement.remember-password",
          "args": { "address": ADDR, "unlock": { "ref": "text" }, "statement": { "ref": "statement" } } },
    ])
}

#[test]
fn parse_refuses_a_locked_pdf_and_says_how_to_unlock_it() {
    let secrets = MemorySecrets::default();
    let (mut host, digest) = device(&secrets);
    let report = run(
        &mut host,
        json!([{ "op": "statement.parse",
                 "args": { "document": digest, "supporter": "s-0001" } }]),
    );
    let e = error(&report);
    assert!(e.contains("password-protected"), "{e}");
    assert!(e.contains("statement.unlock"), "{e}");
}

#[test]
fn unlock_asks_for_the_password_on_the_device() {
    let secrets = MemorySecrets::default();
    let (mut host, digest) = device(&secrets);
    secrets.answer(&label(&digest), PASSWORD);

    let report = run(&mut host, ingest(&digest, false));
    ok(&report);
    let text = out(&report, "text");
    assert_eq!(text["password"], "asked");
    assert_eq!(text["document"], digest.as_str());
    assert!(text["text"].as_str().unwrap().contains("Example Bank"));
    assert!(text.get("remember").is_none());
    let st = out(&report, "statement");
    assert_eq!(st["parser"], "example-bank");
    assert_eq!(st["statement"]["original"], digest.as_str());
    assert_eq!(out(&report, "kept")["remembered"], Value::Null);
    // Nothing was kept: the next statement asks again.
    assert_eq!(
        host.kv
            .as_ref()
            .unwrap()
            .get(&format!("statement-passwords/{ADDR}"))
            .unwrap(),
        None
    );
}

#[test]
fn without_an_answer_unlock_says_what_to_ask_for() {
    let secrets = MemorySecrets::default();
    let (mut host, digest) = device(&secrets);
    let report = run(&mut host, ingest(&digest, false));
    let e = error(&report);
    assert!(e.starts_with(&digest), "{e}");
    assert!(e.contains("RFC"), "{e}");
    assert!(e.contains(&label(&digest)), "{e}");
}

#[test]
fn a_wrong_password_is_refused_with_the_label() {
    let secrets = MemorySecrets::default();
    let (mut host, digest) = device(&secrets);
    secrets.answer(&label(&digest), "XAXX010101001");
    let e = error(&run(&mut host, ingest(&digest, false)));
    assert!(e.contains("doesn't open"), "{e}");
}

#[test]
fn a_remembered_password_opens_the_next_statement_without_asking() {
    let secrets = MemorySecrets::default();
    let (mut host, digest) = device(&secrets);
    secrets.answer(&label(&digest), PASSWORD);

    let first = run(&mut host, ingest(&digest, true));
    ok(&first);
    let sealed = out(&first, "text")["remember"].as_str().unwrap();
    assert!(sealed.starts_with("ur:envelope/"), "{sealed}");
    assert_eq!(out(&first, "kept")["remembered"], "Example Bank 0001");
    let kept = host
        .kv
        .as_ref()
        .unwrap()
        .get(&format!("statement-passwords/{ADDR}"))
        .unwrap()
        .expect("kept in kv");
    let kept: Value = serde_json::from_slice(&kept).unwrap();
    assert_eq!(kept["Example Bank 0001"], sealed);
    assert!(!String::from_utf8_lossy(&kept.to_string().into_bytes()).contains(PASSWORD));

    // The person is no longer there to answer: the remembered password is used.
    let secrets_later = MemorySecrets::default();
    let mut later = Host::memory().with_secrets(secrets_later);
    later.kv = host.kv.take();
    later.blob_store = host.blob_store.take();
    let second = run(&mut later, ingest(&digest, true));
    ok(&second);
    assert_eq!(out(&second, "text")["password"], "remembered");
    assert_eq!(out(&second, "text")["account"], "Example Bank 0001");
    assert!(out(&second, "text").get("remember").is_none());
    assert_eq!(out(&second, "kept")["remembered"], Value::Null);
}

#[test]
fn another_passkey_cannot_use_a_remembered_password() {
    let secrets = MemorySecrets::default();
    let (mut host, digest) = device(&secrets);
    secrets.answer(&label(&digest), PASSWORD);
    ok(&run(&mut host, ingest(&digest, true)));

    // Same stores, a different passkey: this device can't unwrap the keys, so it can't read
    // the remembered password either.
    let mut thief = Host::memory().with_secrets(MemorySecrets::new([9; 32]));
    thief.kv = host.kv.take();
    thief.blob_store = host.blob_store.take();
    let e = error(&run(&mut thief, ingest(&digest, false)));
    assert!(e.contains("passkey"), "{e}");
}

#[test]
fn an_unprotected_pdf_passes_through_unlock() {
    let secrets = MemorySecrets::default();
    let (mut host, _) = device(&secrets);
    let bytes =
        std::fs::read(repo().join("fixtures/synthetic/small/statements/st-2026-03.s-0001.pdf"))
            .unwrap();
    let digest = host.blob_store.as_mut().unwrap().put(&bytes).unwrap();
    let report = run(&mut host, ingest(&digest, true));
    ok(&report);
    assert_eq!(out(&report, "text")["password"], "none");
    assert_eq!(out(&report, "kept")["remembered"], Value::Null);
}

#[test]
fn remember_needs_an_address() {
    let secrets = MemorySecrets::default();
    let (mut host, digest) = device(&secrets);
    secrets.answer(&label(&digest), PASSWORD);
    let report = run(
        &mut host,
        json!([{ "op": "statement.unlock", "args": { "document": digest, "remember": true } }]),
    );
    assert!(error(&report).contains("address"), "{}", error(&report));
}
