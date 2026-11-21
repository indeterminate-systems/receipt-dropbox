//! The synthetic generator and the fixtures committed from it.
//!
//! `UPDATE_GOLDEN=1 cargo test -p rdx-model --test fixtures` rewrites `fixtures/` from the
//! generator; review the diff before committing it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rdx_model::synth::{CaseKind, Config, Scenario};
use rdx_model::*;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn golden(path: &Path, actual: &str) {
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(path).unwrap_or_else(|_| {
        panic!(
            "{} is missing; run with UPDATE_GOLDEN=1 to create it",
            path.display()
        )
    });
    assert!(
        expected == actual,
        "{} differs from the generator's output; run with UPDATE_GOLDEN=1 and review the diff",
        path.display()
    );
}

#[test]
fn the_generator_is_deterministic() {
    let a = Scenario::generate(&Config::small());
    let b = Scenario::generate(&Config::small());
    assert_eq!(a, b);
    let c = Scenario::generate(&Config {
        seed: 2,
        ..Config::small()
    });
    assert_ne!(a, c);
}

#[test]
fn generated_data_is_internally_consistent() {
    for seed in 0..50 {
        let config = Config {
            seed,
            supporters: 3,
            months: 2,
            ..Config::small()
        };
        let s = Scenario::generate(&config);
        assert_eq!(s.supporters.len(), 3);
        for sp in &s.supporters {
            assert_eq!(sp.validate(), Ok(()), "seed {seed}, {}", sp.id);
        }
        assert_eq!(s.statements.len(), 6);
        for st in &s.statements {
            assert_eq!(st.validate(), Ok(()), "seed {seed}, {}", st.id);
        }
        for r in &s.receipts {
            assert_eq!(r.validate(), Ok(()), "seed {seed}, {}", r.id);
        }
        let receipts: BTreeMap<_, _> = s.receipts.iter().map(|r| (&r.id, r)).collect();
        let mut used = BTreeSet::new();
        for m in &s.matches {
            assert_eq!(m.status, MatchStatus::Confirmed);
            let st = s
                .statements
                .iter()
                .find(|st| st.id == m.line.statement)
                .unwrap();
            let line = st.lines.iter().find(|l| l.id == m.line.line).unwrap();
            let r = receipts[&m.receipt];
            assert_eq!(r.supporter, st.supporter, "seed {seed}");
            assert_eq!(m.amount.minor(), line.amount.minor().abs());
            assert_eq!(r.total, m.amount);
            assert!(used.insert(&m.receipt), "receipt used twice");
        }
        // Every org expense has a ledger account the matching and ledger pieces can book to.
        for m in &s.matches {
            assert!(s.accounts.contains_key(&m.receipt));
        }
    }
}

#[test]
fn the_small_scenario_covers_the_hard_cases() {
    let s = Scenario::generate(&Config::small());
    let kinds: BTreeSet<_> = s.cases.iter().map(|c| c.kind).collect();
    for kind in [
        CaseKind::DateLag,
        CaseKind::BillReference,
        CaseKind::Refund,
        CaseKind::MissingReceipt,
        CaseKind::OrphanReceipt,
        CaseKind::Personal,
    ] {
        assert!(kinds.contains(&kind), "no {kind:?} case");
    }
    // The redacted view of every statement reconciles.
    for st in &s.statements {
        let v = st.redaction_summary(&s.matches).unwrap();
        assert_eq!(
            v.opening
                .checked_add(v.shown)
                .unwrap()
                .checked_add(v.redacted),
            Ok(v.closing)
        );
    }
}

#[test]
fn committed_fixtures_match_the_generator() {
    let s = Scenario::generate(&Config::small());
    let dir = repo().join("fixtures/synthetic/small");
    golden(
        &dir.join("scenario.json"),
        &(serde_json::to_string_pretty(&s).unwrap() + "\n"),
    );
    for st in &s.statements {
        golden(
            &dir.join("statements").join(format!("{}.csv", st.id)),
            &rdx_model::synth::statement_csv(st),
        );
    }
}

#[test]
fn the_nickel_contract_accepts_the_fixture_and_rejects_floats() {
    use nickel_embed::{ExportFormat, Program, Source};
    let fixture = repo().join("fixtures/synthetic/small/scenario.json");
    let check = |expr: String| {
        Program::builder()
            .source(Source::code(expr))
            .import_paths(vec![repo().join("nickel")])
            .build()
            .and_then(|mut p| p.export(ExportFormat::Json))
    };
    // Pass the fixture as a Nickel literal rather than `import "scenario.json"`: in
    // nickel-lang-core 0.19, a record contract on JSON-imported (or `std.deserialize`d) data with
    // nested records panics (`value.is_constant()` in eval/fixpoint.rs). Minimal repro:
    // `(std.deserialize 'Json m%"{"a":{"b":1}}"%) | { a }`. Import the file directly once
    // nickel-embed moves to a release without the bug.
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&fixture).unwrap()).unwrap();
    let ok = check(format!(
        r#"let m = import "rdx/model.ncl" in {} | m.Scenario"#,
        to_nickel(&json)
    ));
    assert!(ok.is_ok(), "{}", ok.unwrap_err());

    let float = check(
        r#"let m = import "rdx/model.ncl" in { minor = 42.17, currency = "USD" } | m.Money"#.into(),
    );
    assert!(float.is_err());

    let unbalanced = check(
        r#"let m = import "rdx/model.ncl" in {
          id = "st-1", supporter = "s-0001",
          account = { institution = "Example Bank", mask = "0001" },
          currency = "USD",
          period = { start = "2026-03-01", end = "2026-03-31" },
          opening = { minor = 100, currency = "USD" },
          closing = { minor = 99, currency = "USD" },
          lines = [],
        } | m.Statement"#
            .into(),
    );
    let err = unbalanced.unwrap_err().to_string();
    assert!(err.contains("does not balance"), "{err}");

    let supporter = |fields: &str| {
        check(format!(
            r#"let m = import "rdx/model.ncl" in {{ id = "s-0001", {fields} }} | m.Supporter"#
        ))
    };
    assert!(supporter(r#"display_name = "Ana""#).is_ok());
    let named =
        supporter(r#"display_name = "Ana R.", full_name = "Ana Rodríguez", alias = "Anita""#);
    assert!(named.is_ok(), "{}", named.unwrap_err());
    let blank = supporter(r#"display_name = "Ana", alias = " ""#)
        .unwrap_err()
        .to_string();
    assert!(blank.contains("name"), "{blank}");
}

/// Renders JSON as a Nickel expression.
fn to_nickel(v: &serde_json::Value) -> String {
    use serde_json::Value;
    let string = |s: &str| {
        // JSON escapes are valid Nickel escapes; `%{` would start an interpolation.
        Value::String(s.into()).to_string().replace('%', "\\%")
    };
    match v {
        Value::Object(map) => {
            let fields: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{} = {}", string(k), to_nickel(v)))
                .collect();
            format!("{{ {} }}", fields.join(", "))
        }
        Value::Array(items) => {
            let items: Vec<String> = items.iter().map(to_nickel).collect();
            format!("[{}]", items.join(", "))
        }
        Value::String(s) => string(s),
        other => other.to_string(),
    }
}
