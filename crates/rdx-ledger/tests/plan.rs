//! The `ledger.*` ops in Nickel plans: contracts, a dry run, and an executed run against an
//! in-memory host.

use std::path::{Path, PathBuf};

use rdx_plan::rdx_host::Host;
use rdx_plan::{LoadError, Plan, PlanSource, Registry, RunReport, StepStatus, execute};
use serde_json::Value;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A plan over the synthetic scenario; `steps` may use `s` (the scenario) and `rdx`.
fn load(dry_run: bool, steps: &str) -> Result<Plan, LoadError> {
    let scenario = repo().join("fixtures/synthetic/small/scenario.json");
    let code = format!(
        r#"let rdx = import "rdx/api.ncl" in
let s = import "{}" in
let book = fun i a =>
  rdx.ops.ledger.book {{
    id = "book%{{std.to_string i}}",
    statement = std.array.at i s.statements,
    receipts = s.receipts,
    matches = s.matches,
    accounts = a,
  }}
in
{{
  plan = rdx.plan {{
    skill = "test",
    goal = "Book and export",
    dry_run = {dry_run},
    steps = {steps},
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
    rdx_ledger::register(&mut r);
    r
}

fn step<'a>(report: &'a RunReport, id: &str) -> &'a Value {
    let s = report
        .steps
        .iter()
        .find(|s| s.id.as_deref() == Some(id))
        .unwrap_or_else(|| panic!("no step {id}: {report:?}"));
    assert_eq!(s.status, StepStatus::Done, "{s:?}");
    s.output.as_ref().unwrap()
}

const BOOK_BOTH: &str = r#"[
  book 0 { by_receipt = s.accounts },
  rdx.ops.ledger.post { id = "post0", booking = rdx.ref "book0" },
  book 1 { by_receipt = s.accounts },
  rdx.ops.ledger.post { id = "post1", booking = rdx.ref "book1" },
  rdx.ops.ledger.read { id = "read" },
  rdx.ops.ledger.export { id = "csv" },
  rdx.ops.ledger.export { id = "xlsx", format = 'xlsx },
]"#;

#[test]
fn a_dry_run_shows_the_entries_and_writes_nothing() {
    let plan = load(true, BOOK_BOTH).unwrap();
    let mut host = Host::memory();
    let report = execute(&plan, &registry(), &mut host).unwrap();
    assert!(report.ok, "{report:?}");
    let book = step(&report, "book0");
    assert_eq!(book["summary"]["entries"], 5);
    assert!(
        book["text"]
            .as_str()
            .unwrap()
            .contains("receipt: \"r-2026-03.s-0001.03\"")
    );
    let post = report
        .steps
        .iter()
        .find(|s| s.id.as_deref() == Some("post0"))
        .unwrap();
    assert_eq!(post.status, StepStatus::Skipped);
    // Nothing posted, so the ledger reads as empty.
    assert_eq!(step(&report, "read")["entries"], serde_json::json!([]));
    assert!(
        host.kv
            .as_ref()
            .unwrap()
            .get("ledger/main")
            .unwrap()
            .is_none()
    );
}

#[test]
fn an_executed_run_posts_reads_and_exports() {
    let plan = load(false, BOOK_BOTH).unwrap();
    let mut host = Host::memory();
    let report = execute(&plan, &registry(), &mut host).unwrap();
    assert!(report.ok, "{report:?}");

    let post0 = step(&report, "post0");
    assert_eq!(post0["previous"], Value::Null);
    assert_eq!(post0["added"].as_array().unwrap().len(), 5);
    let post1 = step(&report, "post1");
    assert_eq!(post1["previous"], post0["digest"]);

    let golden =
        std::fs::read_to_string(repo().join("fixtures/ledger/synthetic-small.beancount")).unwrap();
    let read = step(&report, "read");
    assert_eq!(read["text"], golden);
    assert_eq!(read["digest"], post1["digest"]);
    assert_eq!(read["balances"].as_array().unwrap().len(), 2);

    let kv = host
        .kv
        .as_ref()
        .unwrap()
        .get("ledger/main")
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&kv).unwrap(),
        post1["digest"]
    );

    // Every version, oldest first: the audit log keeps only digests of outputs (ADR-P12), so
    // this is where an earlier version is found.
    let versions = host
        .kv
        .as_ref()
        .unwrap()
        .get("ledger-versions/main")
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&versions).unwrap(),
        serde_json::json!([post0["digest"], post1["digest"]])
    );

    let store = host.blob_store.as_ref().unwrap();
    let csv = step(&report, "csv");
    assert_eq!(csv["media_type"], "text/csv");
    let bytes = store
        .get(csv["document"].as_str().unwrap())
        .unwrap()
        .unwrap();
    let expected = std::fs::read(repo().join("fixtures/ledger/synthetic-small.csv")).unwrap();
    assert_eq!(bytes, expected);
    let xlsx = step(&report, "xlsx");
    assert!(xlsx["file_name"].as_str().unwrap().ends_with(".xlsx"));
    let bytes = store
        .get(xlsx["document"].as_str().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(&bytes[..4], b"PK\x03\x04");

    // Posting the same bookings again adds nothing, and a past version stays readable.
    let again = execute(&plan, &registry(), &mut host).unwrap();
    assert_eq!(step(&again, "post0")["already_booked"], 5);
    assert_eq!(step(&again, "post1")["digest"], post1["digest"]);
    let first = format!(
        "[ rdx.ops.ledger.read {{ id = \"v1\", ledger = {} }} ]",
        post0["digest"]
    );
    let v1 = execute(&load(true, &first).unwrap(), &registry(), &mut host).unwrap();
    assert_eq!(step(&v1, "v1")["entries"].as_array().unwrap().len(), 5);
}

#[test]
fn accounts_default_to_uncategorized() {
    let plan = load(true, "[ book 0 {} ]").unwrap();
    assert_eq!(
        plan.steps[0].args["accounts"]["fallback"],
        "Expenses:Uncategorized"
    );
    let report = execute(&plan, &registry(), &mut Host::memory()).unwrap();
    assert_eq!(step(&report, "book0")["summary"]["uncategorized"], 5);
}

#[test]
fn contracts_reject_bad_accounts_and_ledger_names() {
    let err = load(true, r#"[ book 0 { fallback = "Uncategorized" } ]"#).unwrap_err();
    assert!(err.to_string().contains("Beancount accounts"), "{err}");
    let err = load(
        true,
        r#"[ rdx.ops.ledger.read { ledger = "Main Ledger" } ]"#,
    )
    .unwrap_err();
    assert!(err.to_string().contains("ledger name"), "{err}");
    let err = load(true, r#"[ rdx.ops.ledger.export { format = 'pdf } ]"#).unwrap_err();
    assert!(err.to_string().contains("pdf"), "{err}");
}

#[test]
fn a_ledger_edited_outside_rdx_is_refused() {
    let mut host = Host::memory();
    let edited = std::fs::read_to_string(repo().join("fixtures/ledger/synthetic-small.beancount"))
        .unwrap()
        .replace("-616.40 USD", "-600.00 USD");
    let digest = host
        .blob_store
        .as_mut()
        .unwrap()
        .put(edited.as_bytes())
        .unwrap();
    host.kv
        .as_mut()
        .unwrap()
        .put(
            "ledger/main",
            serde_json::to_string(&digest).unwrap().as_bytes(),
        )
        .unwrap();
    let plan = load(true, "[ rdx.ops.ledger.read { id = \"read\" } ]").unwrap();
    let report = execute(&plan, &registry(), &mut host).unwrap();
    assert!(!report.ok);
    let error = report.steps[0].error.as_deref().unwrap();
    assert!(error.contains("edited outside rdx"), "{error}");
}
