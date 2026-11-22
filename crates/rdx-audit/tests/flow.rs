//! Keeping, granting, opening, revoking and tracing a statement through Nickel plans, on one
//! in-memory host that plays the supporter's device, the accountant's device and the store.

use std::path::{Path, PathBuf};

use rdx_plan::rdx_host::Host;
use rdx_plan::rdx_host::memory::{MemoryBlobStore, MemoryKv};
use rdx_plan::{PlanError, PlanSource, Registry, RunReport, StepStatus, execute};
use serde_json::Value;

const SUPPORTER: &str = "ana@example.org";
const AUDITOR: &str = "auditor@example.org";
const STATEMENT: &str = "st-2026-03.s-0001";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn registry() -> Registry {
    let mut r = Registry::with_core();
    rdx_keys::register(&mut r);
    rdx_envelope::register(&mut r);
    rdx_ledger::register(&mut r);
    rdx_audit::register(&mut r);
    r
}

/// Runs steps (Nickel, with `s` the synthetic scenario and `rdx` the API) for real.
fn run(host: &mut Host, steps: &str) -> Result<RunReport, PlanError> {
    let scenario = repo().join("fixtures/synthetic/small/scenario.json");
    let code = format!(
        r#"let rdx = import "rdx/api.ncl" in
let s = import "{}" in
{{ plan = rdx.plan {{ skill = "test", goal = "Audit", dry_run = false, steps = {steps} }} }}"#,
        scenario.display()
    );
    let plan = PlanSource::code(code)
        .import_path(repo().join("nickel"))
        .load()
        .unwrap_or_else(|e| panic!("{e}"));
    execute(&plan, &registry(), host)
}

fn ok(host: &mut Host, steps: &str) -> RunReport {
    let report = run(host, steps).unwrap();
    assert!(report.ok, "{report:#?}");
    report
}

fn out<'a>(report: &'a RunReport, id: &str) -> &'a Value {
    let s = report
        .steps
        .iter()
        .find(|s| s.id.as_deref() == Some(id))
        .unwrap_or_else(|| panic!("no step {id}"));
    assert_eq!(s.status, StepStatus::Done, "{s:?}");
    s.output.as_ref().unwrap()
}

fn error(report: &RunReport) -> String {
    assert!(!report.ok);
    report.steps.last().unwrap().error.clone().unwrap()
}

/// The bytes the synthetic statement names as its original (rdx_model::synth).
fn original_bytes() -> Vec<u8> {
    format!("synthetic encrypted original {STATEMENT}").into_bytes()
}

struct Kept {
    original: String,
    sealed: String,
    source: String,
    root: String,
    signer: String,
}

/// Onboards the supporter, puts the bank's document in the store, seals, keeps and books the
/// first statement.
fn keep_and_book(host: &mut Host) -> Kept {
    let original = host
        .blob_store
        .as_mut()
        .unwrap()
        .put(&original_bytes())
        .unwrap();
    let report = ok(
        host,
        &format!(
            r#"[
  rdx.ops.keys.onboard {{ address = "{SUPPORTER}", supporter = "s-0001", device = "laptop" }},
  rdx.ops.envelope.seal {{ id = "sealed", statement = std.array.at 0 s.statements, address = "{SUPPORTER}" }},
  rdx.ops.audit.keep {{ id = "kept", sealed = rdx.ref "sealed", address = "{SUPPORTER}" }},
  rdx.ops.ledger.book {{ id = "book", statement = std.array.at 0 s.statements, receipts = s.receipts, matches = s.matches, accounts = {{ by_receipt = s.accounts }} }},
  rdx.ops.ledger.post {{ booking = rdx.ref "book" }},
]"#
        ),
    );
    let kept = out(&report, "kept");
    assert_eq!(kept["original"], original.as_str());
    assert_eq!(kept["statement"], STATEMENT);
    assert_eq!(kept["device"], "laptop");
    assert_eq!(kept["recipients"], 2, "the device key and the recovery key");
    let sealed = out(&report, "sealed");
    assert_eq!(kept["root"], sealed["root"]);
    Kept {
        original,
        sealed: kept["sealed"].as_str().unwrap().into(),
        source: kept["source"].as_str().unwrap().into(),
        root: kept["root"].as_str().unwrap().into(),
        signer: sealed["signer"].as_str().unwrap().into(),
    }
}

/// The trail of the main ledger, with the viewable (redacted) statement verified as an
/// accountant would.
fn trail(host: &mut Host, k: &Kept) -> Value {
    let report = ok(
        host,
        &format!(
            r#"[
  rdx.ops.envelope.open {{ id = "full", blob = "{}", address = "{SUPPORTER}" }},
  rdx.ops.envelope.redact {{ id = "redacted", envelope = rdx.ref "full", matches = s.matches }},
  rdx.ops.envelope.verify {{ id = "view", envelope = rdx.ref "redacted", signer = "{}" }},
  rdx.ops.ledger.read {{ id = "ledger" }},
  rdx.ops.audit.trail {{ id = "trail", ledger = rdx.ref "ledger", views = [rdx.ref "view"] }},
]"#,
            k.sealed, k.signer
        ),
    );
    out(&report, "trail").clone()
}

fn blob(host: &Host, id: &str) -> Option<Vec<u8>> {
    host.blob_store.as_ref().unwrap().get(id).unwrap()
}

#[test]
fn keeping_encrypts_the_original_and_deletes_the_plaintext() {
    let mut host = Host::memory();
    let k = keep_and_book(&mut host);
    assert_eq!(blob(&host, &k.original), None, "the plaintext is gone");
    let source = String::from_utf8(blob(&host, &k.source).unwrap()).unwrap();
    assert!(source.starts_with("ur:envelope/"));
    assert!(
        !source.contains("synthetic encrypted original"),
        "the kept original is ciphertext"
    );

    // The supporter's device can still open it, and it is the document that was signed.
    let report = ok(
        &mut host,
        &format!(
            r#"[ rdx.ops.audit.open-original {{ id = "doc", blob = "{}", address = "{SUPPORTER}" }} ]"#,
            k.source
        ),
    );
    assert_eq!(out(&report, "doc")["document"], k.original.as_str());
    assert_eq!(blob(&host, &k.original), Some(original_bytes()));

    // Keeping twice is refused.
    let again = run(
        &mut host,
        &format!(
            r#"[ rdx.ops.audit.keep {{ sealed = {{ blob = "{}", root = "{}", signer = "{}" }}, address = "{SUPPORTER}" }} ]"#,
            k.sealed, k.root, k.signer
        ),
    )
    .unwrap();
    assert!(error(&again).contains("already kept"), "{again:?}");
}

#[test]
fn the_trail_links_entries_and_viewable_rows_to_receipts_and_originals() {
    let mut host = Host::memory();
    let k = keep_and_book(&mut host);
    let t = trail(&mut host, &k);
    assert_eq!(t["problems"], serde_json::json!([]), "{t:#}");
    assert_eq!(t["ok"], true);
    let entries = t["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 5);
    for e in entries {
        assert_eq!(e["statement"], k.original.as_str());
        assert_eq!(e["root"], k.root.as_str());
        assert_eq!(e["source"], k.source.as_str());
        assert_eq!(e["viewable"], "shown");
        assert!(e["receipt"].as_str().unwrap().starts_with("r-"));
        assert!(e["receipt_source"].as_str().unwrap().starts_with("sha256:"));
    }
    let st = &t["statements"][0];
    assert_eq!(st["statement"], STATEMENT);
    assert_eq!(st["address"], SUPPORTER);
    assert_eq!(st["kept_by"], "laptop");
    assert_eq!(st["entries"], 5);
}

#[test]
fn a_grant_lets_the_accountant_open_copies_until_it_is_revoked() {
    let mut host = Host::memory();
    let k = keep_and_book(&mut host);
    // The accountant has keys of their own (on their device; here the same host).
    ok(
        &mut host,
        &format!(
            r#"[ rdx.ops.keys.onboard {{ address = "{AUDITOR}", supporter = "acct-1", device = "desk" }} ]"#
        ),
    );
    let report = ok(
        &mut host,
        &format!(
            r#"[
  rdx.ops.keys.recipients {{ id = "keys", address = "{AUDITOR}" }},
  rdx.ops.audit.grant {{ id = "grant", statement = "{STATEMENT}", address = "{SUPPORTER}", to = rdx.ref "keys", grantee = "{AUDITOR}", reason = "2026 annual audit" }},
]"#
        ),
    );
    let grant = out(&report, "grant").clone();
    assert_eq!(grant["granted_by"], "laptop");
    assert_eq!(grant["root"], k.root.as_str());
    assert_eq!(grant["signer"], k.signer.as_str());
    let copy = grant["sealed"].as_str().unwrap().to_owned();
    let source_copy = grant["source"].as_str().unwrap().to_owned();
    assert_ne!(
        copy, k.sealed,
        "a grant is a copy; the kept blob doesn't change"
    );

    // The accountant opens the copies, not the supporter's own blob.
    let opened = ok(
        &mut host,
        &format!(
            r#"[
  rdx.ops.envelope.open {{ id = "full", blob = "{copy}", address = "{AUDITOR}" }},
  rdx.ops.envelope.verify {{ id = "view", envelope = rdx.ref "full", signer = "{}" }},
  rdx.ops.audit.open-original {{ id = "doc", blob = "{source_copy}", address = "{AUDITOR}" }},
]"#,
            k.signer
        ),
    );
    let view = out(&opened, "view");
    assert_eq!(view["redacted_rows"], 0, "the full statement was granted");
    assert_eq!(view["original"], k.original.as_str());
    assert_eq!(out(&opened, "doc")["document"], view["original"]);
    let own = run(
        &mut host,
        &format!(
            r#"[ rdx.ops.envelope.open {{ blob = "{}", address = "{AUDITOR}" }} ]"#,
            k.sealed
        ),
    )
    .unwrap();
    assert!(error(&own).contains("wasn't encrypted to them"), "{own:?}");

    // The trail shows the grant; a second grant to the same person is refused.
    let t = trail(&mut host, &k);
    assert_eq!(t["entries"][0]["granted_to"], serde_json::json!([AUDITOR]));
    assert_eq!(
        t["statements"][0]["grants"][0]["reason"],
        "2026 annual audit"
    );
    let twice = run(
        &mut host,
        &format!(
            r#"[
  rdx.ops.keys.recipients {{ id = "keys", address = "{AUDITOR}" }},
  rdx.ops.audit.grant {{ statement = "{STATEMENT}", address = "{SUPPORTER}", to = rdx.ref "keys", grantee = "{AUDITOR}", reason = "again" }},
]"#
        ),
    )
    .unwrap();
    assert!(error(&twice).contains("already has an active grant"));

    // Revoking deletes the copies and leaves the kept blobs.
    let report = ok(
        &mut host,
        &format!(
            r#"[ rdx.ops.audit.revoke {{ id = "revoke", statement = "{STATEMENT}", address = "{SUPPORTER}", grantee = "{AUDITOR}", reason = "audit closed" }} ]"#
        ),
    );
    assert_eq!(
        out(&report, "revoke")["deleted"],
        serde_json::json!([copy, source_copy])
    );
    assert_eq!(blob(&host, &copy), None);
    assert_eq!(blob(&host, &source_copy), None);
    assert!(blob(&host, &k.sealed).is_some());
    let t = trail(&mut host, &k);
    assert_eq!(t["ok"], true, "{t:#}");
    assert_eq!(t["entries"][0]["granted_to"], serde_json::json!([]));
    assert_eq!(
        t["statements"][0]["grants"][0]["revoked_reason"],
        "audit closed"
    );
}

#[test]
fn only_the_supporters_device_can_grant() {
    let mut host = Host::memory();
    keep_and_book(&mut host);
    ok(
        &mut host,
        &format!(
            r#"[ rdx.ops.keys.onboard {{ address = "{AUDITOR}", supporter = "acct-1", device = "desk" }} ]"#
        ),
    );
    // Someone else's device: the supporter named is not the statement's.
    let other = run(
        &mut host,
        &format!(
            r#"[
  rdx.ops.keys.recipients {{ id = "keys", address = "{AUDITOR}" }},
  rdx.ops.audit.grant {{ statement = "{STATEMENT}", address = "{AUDITOR}", to = rdx.ref "keys", grantee = "{AUDITOR}", reason = "self-service" }},
]"#
        ),
    )
    .unwrap();
    assert!(error(&other).contains(&format!("belongs to `{SUPPORTER}`")));

    // A server (no `secrets`) can't run a grant at all.
    let mut server = Host::new()
        .with_kv(MemoryKv::default())
        .with_blob_store(MemoryBlobStore::default());
    let refused = run(
        &mut server,
        &format!(
            r#"[ rdx.ops.audit.grant {{ statement = "{STATEMENT}", address = "{SUPPORTER}", to = ["ur:crypto-pubkeys/x"], grantee = "{AUDITOR}", reason = "x" }} ]"#
        ),
    )
    .unwrap_err();
    assert!(
        matches!(refused, PlanError::MissingCapability { .. }),
        "{refused:?}"
    );
}

#[test]
fn the_trail_reports_what_does_not_add_up() {
    let mut host = Host::memory();
    let k = keep_and_book(&mut host);

    // A forged grant, signed by a key the supporter's registry never listed.
    ok(
        &mut host,
        &format!(
            r#"[ rdx.ops.keys.onboard {{ address = "{AUDITOR}", supporter = "acct-1", device = "desk" }} ]"#
        ),
    );
    let kv = host.kv.as_mut().unwrap();
    let key = format!("audit/custody/{STATEMENT}");
    let mut custody: rdx_audit::custody::Custody =
        serde_json::from_slice(&kv.get(&key).unwrap().unwrap()).unwrap();
    let mallory = rdx_keys::Keys::from_secret(&[7; 32]);
    custody
        .grant(
            &rdx_audit::custody::Grant {
                grantee: "mallory@example.org".into(),
                reason: "forged".into(),
                recipients: vec![rdx_keys::public_keys_to_ur(&mallory.public())],
                sealed: k.sealed.clone(),
                source: None,
            },
            &rdx_audit::custody::By {
                device: "laptop",
                keys: &mallory,
            },
            1,
        )
        .unwrap();
    kv.put(&key, &serde_json::to_vec(&custody).unwrap())
        .unwrap();

    // The second statement is booked but nobody kept it.
    ok(
        &mut host,
        r#"[
  rdx.ops.ledger.book { id = "book", statement = std.array.at 1 s.statements, receipts = s.receipts, matches = s.matches, accounts = { by_receipt = s.accounts } },
  rdx.ops.ledger.post { booking = rdx.ref "book" },
]"#,
    );
    let t = trail(&mut host, &k);
    assert_eq!(t["ok"], false);
    let problems: Vec<&str> = t["problems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap())
        .collect();
    assert!(
        problems
            .iter()
            .any(|p| p.contains("`granted` event was signed by a key")),
        "{problems:#?}"
    );
    assert!(
        problems
            .iter()
            .any(|p| p.contains("st-2026-03.s-0002` has no custody record")),
        "{problems:#?}"
    );
}

#[test]
fn a_view_that_shows_unbooked_lines_is_reported() {
    let mut host = Host::memory();
    let k = keep_and_book(&mut host);
    // Verify the full statement as if it were the viewable one: it shows every line, booked or
    // not.
    let report = ok(
        &mut host,
        &format!(
            r#"[
  rdx.ops.envelope.open {{ id = "full", blob = "{}", address = "{SUPPORTER}" }},
  rdx.ops.envelope.verify {{ id = "view", envelope = rdx.ref "full", signer = "{}" }},
  rdx.ops.ledger.read {{ id = "ledger" }},
  rdx.ops.audit.trail {{ id = "trail", ledger = rdx.ref "ledger", views = [rdx.ref "view"] }},
]"#,
            k.sealed, k.signer
        ),
    );
    let t = out(&report, "trail");
    assert_eq!(t["ok"], false);
    assert!(
        t["problems"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p.as_str().unwrap().contains("no ledger entry books it")),
        "{t:#}"
    );
}
