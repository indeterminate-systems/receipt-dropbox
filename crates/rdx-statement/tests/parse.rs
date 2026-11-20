//! Every format round-trips the synthetic statements, and statements that don't add up fail with
//! an error that points at the problem.
//!
//! `UPDATE_GOLDEN=1 cargo test -p rdx-statement --test parse` rewrites the OFX and PDF fixtures
//! under `fixtures/synthetic/small/statements/`; review the diff before committing it.

use std::path::{Path, PathBuf};

use rdx_model::synth::{Config, Scenario, statement_csv};
use rdx_model::{Digest, Statement};
use rdx_statement::render::{example_bank_pdf, ofx};
use rdx_statement::{Hints, Input, ParseError, Source, parse};
use serde_json::json;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn statements_dir() -> PathBuf {
    repo().join("fixtures/synthetic/small/statements")
}

/// Rows per page in the committed PDFs: small enough that the small scenario spans two pages.
const FIXTURE_ROWS: usize = 8;

fn golden(path: &Path, actual: &[u8]) {
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(path, actual).unwrap();
        return;
    }
    let expected = std::fs::read(path).unwrap_or_else(|_| {
        panic!(
            "{} is missing; run with UPDATE_GOLDEN=1 to create it",
            path.display()
        )
    });
    assert!(
        expected == actual,
        "{} differs from the renderer's output; run with UPDATE_GOLDEN=1 and review the diff",
        path.display()
    );
}

fn hints(st: &Statement, extra: serde_json::Value) -> Hints {
    let mut h = json!({ "supporter": st.supporter, "original": st.original });
    h.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    serde_json::from_value(h).unwrap()
}

fn without_references(st: &Statement) -> Statement {
    let mut st = st.clone();
    for l in &mut st.lines {
        l.reference = None;
    }
    st
}

#[test]
fn committed_fixtures_match_the_renderers() {
    let scenario = Scenario::generate(&Config::small());
    for st in &scenario.statements {
        golden(
            &statements_dir().join(format!("{}.ofx", st.id)),
            ofx(st).as_bytes(),
        );
        golden(
            &statements_dir().join(format!("{}.pdf", st.id)),
            &example_bank_pdf(st, FIXTURE_ROWS),
        );
    }
}

#[test]
fn csv_fixtures_parse_to_the_scenario() {
    let scenario = Scenario::generate(&Config::small());
    for st in &scenario.statements {
        let csv = std::fs::read_to_string(statements_dir().join(format!("{}.csv", st.id))).unwrap();
        let h = hints(st, json!({ "currency": "USD", "account": st.account }));
        let out = parse(&Input::Text(&csv), &h).unwrap();
        assert_eq!(out.parser, "csv");
        // CSV exports carry no reference column.
        assert_eq!(out.statement, without_references(st));
        assert_eq!(out.checks.opening, Source::Derived);
        assert_eq!(out.checks.closing, Source::Derived);
        assert_eq!(out.checks.running_balances, st.lines.len());
    }
}

#[test]
fn ofx_fixtures_parse_to_the_scenario() {
    let scenario = Scenario::generate(&Config::small());
    for st in &scenario.statements {
        let text =
            std::fs::read_to_string(statements_dir().join(format!("{}.ofx", st.id))).unwrap();
        let out = parse(&Input::Text(&text), &hints(st, json!({}))).unwrap();
        assert_eq!(out.parser, "ofx");
        assert_eq!(out.statement, *st);
        assert_eq!(out.checks.closing, Source::Printed);
    }
}

#[test]
fn pdf_fixtures_parse_to_the_scenario() {
    let scenario = Scenario::generate(&Config::small());
    for st in &scenario.statements {
        let bytes = std::fs::read(statements_dir().join(format!("{}.pdf", st.id))).unwrap();
        let text = rdx_pdf::text(&bytes).unwrap();
        assert!(text.pages.len() >= 2, "{} should span pages", st.id);
        let out = parse(&Input::Pdf(&text), &hints(st, json!({}))).unwrap();
        assert_eq!(out.parser, "example-bank");
        assert_eq!(out.statement, *st);
        assert_eq!(out.checks.opening, Source::Printed);
        assert_eq!(out.checks.closing, Source::Printed);
        assert!(out.checks.totals);
        assert_eq!(out.checks.running_balances, st.lines.len());
    }
}

#[test]
fn every_format_round_trips_many_generated_statements() {
    for seed in 0..20 {
        let scenario = Scenario::generate(&Config {
            seed,
            supporters: 2,
            months: 3,
            ..Config::small()
        });
        for st in &scenario.statements {
            let csv = statement_csv(st);
            let h = hints(st, json!({ "currency": "USD", "account": st.account }));
            assert_eq!(
                parse(&Input::Text(&csv), &h).unwrap().statement,
                without_references(st),
                "csv, seed {seed}, {}",
                st.id
            );
            let o = ofx(st);
            assert_eq!(
                parse(&Input::Text(&o), &hints(st, json!({})))
                    .unwrap()
                    .statement,
                *st
            );
            for rows in [3, 7, 40] {
                let text = rdx_pdf::text(&example_bank_pdf(st, rows)).unwrap();
                assert_eq!(
                    parse(&Input::Pdf(&text), &hints(st, json!({})))
                        .unwrap()
                        .statement,
                    *st,
                    "pdf, seed {seed}, {}, {rows} rows a page",
                    st.id
                );
            }
        }
    }
}

fn small_statement() -> Statement {
    Scenario::generate(&Config::small()).statements[0].clone()
}

#[test]
fn a_misread_amount_is_caught_by_the_running_balance() {
    let st = small_statement();
    let csv = statement_csv(&st);
    // Flip the sign of the second line's amount, as a misread would.
    let second = csv.lines().nth(2).unwrap();
    let amount = second.split(',').nth(2).unwrap();
    let flipped = second.replacen(amount, &amount.replacen('-', "", 1), 1);
    let tampered = csv.replacen(second, &flipped, 1);
    let h = hints(&st, json!({ "currency": "USD", "account": st.account }));
    let err = parse(&Input::Text(&tampered), &h).unwrap_err();
    let ParseError::Line { at, message } = &err else {
        panic!("{err}")
    };
    assert_eq!(at, "row 3");
    assert!(message.starts_with("printed balance"), "{message}");
}

#[test]
fn a_missing_line_is_caught_by_the_running_balance() {
    let st = small_statement();
    let mut text = rdx_pdf::text(&example_bank_pdf(&st, 40)).unwrap();
    // Lose one row, as a reader that skipped a line would.
    let removed = &st.lines[1].description;
    for page in &mut text.pages {
        page.lines.retain(|l| !l.text().contains(removed.as_str()));
    }
    let err = parse(&Input::Pdf(&text), &hints(&st, json!({}))).unwrap_err();
    assert!(err.to_string().contains("printed balance"), "{err}");
}

#[test]
fn a_missing_line_without_running_balances_is_caught_by_the_totals() {
    let st = small_statement();
    let mut text = rdx_pdf::text(&example_bank_pdf(&st, 40)).unwrap();
    let removed = &st.lines[1].description;
    for page in &mut text.pages {
        page.lines.retain(|l| !l.text().contains(removed.as_str()));
        // Drop the balance column too, so only the summary totals can notice.
        for line in &mut page.lines {
            if line.spans.len() >= 4 && line.spans[0].text.contains('/') {
                line.spans.pop();
            }
        }
    }
    let err = parse(&Input::Pdf(&text), &hints(&st, json!({}))).unwrap_err();
    assert!(
        err.to_string().contains("total debits")
            || err.to_string().contains("total credits")
            || err.to_string().contains("does not balance"),
        "{err}"
    );
}

#[test]
fn missing_facts_are_asked_for() {
    let st = small_statement();
    let csv = statement_csv(&st);
    let err = parse(
        &Input::Text(&csv),
        &hints(&st, json!({ "account": st.account })),
    )
    .unwrap_err();
    assert_eq!(
        err,
        ParseError::Missing(
            "currency",
            "the document doesn't say; pass `currency`, such as \"USD\""
        )
    );
    let err = parse(
        &Input::Text(&csv),
        &hints(&st, json!({ "currency": "USD" })),
    )
    .unwrap_err();
    assert!(err.to_string().starts_with("account is missing"), "{err}");

    let ambiguous = "date,description,amount,balance\n03/04/2026,X,-1.00,9.00\n";
    let h = hints(&st, json!({ "currency": "USD", "account": st.account }));
    let err = parse(&Input::Text(ambiguous), &h).unwrap_err();
    assert!(err.to_string().contains("date_order"), "{err}");
    let h = hints(
        &st,
        json!({ "currency": "USD", "account": st.account, "csv": { "date_order": "dmy" } }),
    );
    let out = parse(&Input::Text(ambiguous), &h).unwrap();
    assert_eq!(out.statement.lines[0].date.to_string(), "2026-04-03");
}

#[test]
fn the_document_and_the_hints_must_agree() {
    let st = small_statement();
    let o = ofx(&st);
    let err = parse(&Input::Text(&o), &hints(&st, json!({ "currency": "MXN" }))).unwrap_err();
    assert!(err.to_string().contains("the document says USD"), "{err}");
}

#[test]
fn debit_credit_columns_newest_first_and_semicolons() {
    let st = small_statement();
    let csv = "Fecha;Concepto;Cargos;Abonos;Saldo\n\
               13/03/2026;PAPERLEAF SUPPLY REFUND;;250,85;425,77\n\
               11/03/2026;PAPERLEAF SUPPLY #4159;279,89;;174,92\n";
    let h = hints(
        &st,
        json!({ "currency": "MXN", "account": st.account, "csv": { "decimal": "comma" } }),
    );
    let out = parse(&Input::Text(csv), &h).unwrap();
    let lines = &out.statement.lines;
    assert_eq!(lines[0].description, "PAPERLEAF SUPPLY #4159");
    assert_eq!(lines[0].amount.decimal(), "-279.89");
    assert_eq!(lines[1].amount.decimal(), "250.85");
    assert_eq!(out.statement.opening.decimal(), "454.81");
    assert_eq!(out.statement.period.start().to_string(), "2026-03-01");
    assert_eq!(out.statement.period.end().to_string(), "2026-03-31");
}

#[test]
fn the_original_is_the_document_digest() {
    let st = small_statement();
    let csv = statement_csv(&st);
    let digest = Digest::of_bytes(csv.as_bytes());
    let mut h = hints(&st, json!({ "currency": "USD", "account": st.account }));
    h.original = Some(digest.clone());
    assert_eq!(
        parse(&Input::Text(&csv), &h).unwrap().statement.original,
        Some(digest)
    );
}

#[test]
fn a_typst_rendered_pdf_parses_to_the_scenario() {
    // Another producer's PDF of the same layout: subset fonts with ToUnicode maps, kerning and
    // object streams. See fixtures/layouts/example-bank.typ for how it is made.
    let st = small_statement();
    let bytes = std::fs::read(statements_dir().join(format!("{}.typst.pdf", st.id))).unwrap();
    let text = rdx_pdf::text(&bytes).unwrap();
    let out = parse(&Input::Pdf(&text), &hints(&st, json!({}))).unwrap();
    assert_eq!(out.parser, "example-bank");
    assert_eq!(out.statement, st);
    assert!(out.checks.totals);
}
