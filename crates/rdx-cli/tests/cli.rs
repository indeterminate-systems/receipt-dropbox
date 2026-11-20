//! `rdx` end to end: the binary, the repo's real skills and a throwaway state directory.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rdx(state: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rdx"))
        .current_dir(repo())
        .args(args)
        .arg("--state")
        .arg(state)
        .output()
        .unwrap()
}

fn stdout_json(out: &Output) -> Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "stdout is not JSON ({e}):\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn audit_lines(state: &Path) -> Vec<String> {
    std::fs::read_to_string(state.join("audit.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn kv_files(state: &Path) -> usize {
    std::fs::read_dir(state.join("kv"))
        .map(|d| d.count())
        .unwrap_or(0)
}

#[test]
fn runs_are_dry_by_default_and_recorded() {
    let state = tempfile::tempdir().unwrap();
    let out = rdx(state.path(), &["run", "plan-smoke-test"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let report = stdout_json(&out);
    assert_eq!(report["ok"], true);
    assert_eq!(report["dry_run"], true);
    assert_eq!(report["skill"], "plan-smoke-test");
    let skipped: Vec<&Value> = report["steps"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["status"] == "skipped")
        .collect();
    assert!(
        !skipped.is_empty(),
        "the smoke test has a step with effects"
    );
    assert!(stderr(&out).contains("--execute"), "{}", stderr(&out));
    assert_eq!(kv_files(state.path()), 0, "a dry run changes nothing");
    assert_eq!(audit_lines(state.path()).len(), 1);
}

#[test]
fn execute_performs_effects() {
    let state = tempfile::tempdir().unwrap();
    let out = rdx(state.path(), &["run", "plan-smoke-test", "--execute"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let report = stdout_json(&out);
    assert_eq!(report["dry_run"], false);
    assert!(
        report["steps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["status"] == "done")
    );
    assert_eq!(kv_files(state.path()), 1);
}

#[test]
fn inputs_are_nickel_expressions() {
    let state = tempfile::tempdir().unwrap();
    let out = rdx(
        state.path(),
        &[
            "run",
            ".agents/skills/plan-smoke-test",
            "--input",
            r#"host="ci""#,
        ],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stdout_json(&out)["notes"][0], "plan smoke test on ci");
}

#[test]
fn contract_errors_stop_before_anything_runs() {
    let state = tempfile::tempdir().unwrap();
    let out = rdx(
        state.path(),
        &["run", "plan-smoke-test", "--input", "host=42"],
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert!(stderr(&out).contains("contract"), "{}", stderr(&out));
    assert!(audit_lines(state.path()).is_empty());
}

#[test]
fn unknown_skills_list_the_known_ones() {
    let state = tempfile::tempdir().unwrap();
    let out = rdx(state.path(), &["run", "no-such-skill"]);
    assert_eq!(out.status.code(), Some(2));
    let err = stderr(&out);
    assert!(err.contains("no-such-skill"), "{err}");
    assert!(err.contains("plan-smoke-test"), "{err}");
}

#[test]
fn malformed_inputs_are_usage_errors() {
    let state = tempfile::tempdir().unwrap();
    let out = rdx(state.path(), &["run", "plan-smoke-test", "--input", "host"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("FIELD=VALUE"), "{}", stderr(&out));
}

#[test]
fn audit_verify_detects_tampering() {
    let state = tempfile::tempdir().unwrap();
    for _ in 0..2 {
        assert!(
            rdx(state.path(), &["run", "plan-smoke-test"])
                .status
                .success()
        );
    }
    let ok = rdx(state.path(), &["audit", "verify"]);
    assert_eq!(ok.status.code(), Some(0), "{}", stderr(&ok));
    assert!(String::from_utf8_lossy(&ok.stdout).contains("2 entries"));

    let log = state.path().join("audit.jsonl");
    let text = std::fs::read_to_string(&log).unwrap();
    std::fs::write(
        &log,
        text.replacen("plan smoke test", "plan smoke tests", 1),
    )
    .unwrap();
    let bad = rdx(state.path(), &["audit", "verify"]);
    assert_eq!(bad.status.code(), Some(1));
    assert!(stderr(&bad).contains("line 1"), "{}", stderr(&bad));
}

#[test]
fn help_and_usage() {
    let state = tempfile::tempdir().unwrap();
    let help = rdx(state.path(), &["--help"]);
    assert_eq!(help.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&help.stdout).contains("rdx run"));
    assert_eq!(rdx(state.path(), &["frobnicate"]).status.code(), Some(2));
}

#[test]
fn blob_put_then_ingest_a_statement() {
    let state = tempfile::tempdir().unwrap();
    let out = rdx(
        state.path(),
        &[
            "blob",
            "put",
            "fixtures/synthetic/small/statements/st-2026-03.s-0001.pdf",
        ],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let digest = String::from_utf8(out.stdout).unwrap().trim().to_owned();
    assert!(digest.starts_with("sha256:"), "{digest}");

    let doc = format!("document=\"{digest}\"");
    let out = rdx(
        state.path(),
        &[
            "run",
            "ingest-statement",
            "--input",
            &doc,
            "--input",
            r#"supporter="s-0001""#,
        ],
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let report = stdout_json(&out);
    assert_eq!(report["steps"][0]["output"]["parser"], "example-bank");
    assert_eq!(
        report["steps"][0]["output"]["statement"]["original"],
        digest.as_str()
    );
}
