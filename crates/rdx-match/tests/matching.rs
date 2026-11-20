//! Matching against the synthetic generator's ground truth, and hand-made cases for what the
//! generator doesn't produce (splits, instalments, partials, other currencies, earlier decisions).

// Amounts are written as `dollars_cents` (`5_00` is 5.00) so they read as money.
#![allow(clippy::inconsistent_digit_grouping)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use nickel_embed::{ExportFormat, Program, Source};
use rdx_match::*;
use rdx_model::synth::{CaseKind, Config, Scenario};
use rdx_model::*;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Evaluates Nickel with `nickel/` on the import path and parses the JSON result.
fn eval<T: serde::de::DeserializeOwned>(code: &str) -> T {
    let json = Program::builder()
        .source(Source::code(code))
        .import_paths([repo().join("nickel")])
        .build()
        .and_then(|mut p| p.export(ExportFormat::Json))
        .unwrap_or_else(|e| panic!("{e}"));
    serde_json::from_str(&json).unwrap()
}

/// The default policy, from the single place defaults are defined.
fn policy() -> Policy {
    eval(r#"(import "rdx/matching.ncl").defaults"#)
}

fn usd(minor: i64) -> Money {
    Money::new(minor, Currency::USD)
}

fn date(s: &str) -> Date {
    s.parse().unwrap()
}

fn line(id: &str, on: &str, description: &str, minor: i64) -> StatementLine {
    StatementLine {
        id: LineId::new(id).unwrap(),
        date: date(on),
        description: description.into(),
        amount: usd(minor),
        reference: None,
    }
}

fn statement(lines: Vec<StatementLine>) -> Statement {
    let opening = usd(10_000_00);
    let closing = Money::sum(Currency::USD, lines.iter().map(|l| &l.amount))
        .unwrap()
        .checked_add(opening)
        .unwrap();
    Statement {
        id: StatementId::new("st-test").unwrap(),
        supporter: SupporterId::new("s-1").unwrap(),
        account: AccountRef {
            institution: "Example Bank".into(),
            mask: "0001".into(),
        },
        currency: Currency::USD,
        period: Period::new(date("2026-03-01"), date("2026-03-31")).unwrap(),
        opening,
        closing,
        lines,
        original: None,
    }
}

fn receipt(id: &str, kind: ReceiptKind, merchant: &str, on: &str, total: Money) -> Receipt {
    Receipt {
        id: ReceiptId::new(id).unwrap(),
        supporter: SupporterId::new("s-1").unwrap(),
        kind,
        merchant: merchant.into(),
        date: date(on),
        total,
        reference: None,
        items: vec![],
        source: Digest::of_bytes(id.as_bytes()),
    }
}

fn shop(id: &str, merchant: &str, on: &str, minor: i64) -> Receipt {
    receipt(id, ReceiptKind::Receipt, merchant, on, usd(minor))
}

/// `(line, receipt, amount)`.
type Pair = (String, String, i64);

/// `(kind, review, pairs)` for each proposal, for compact assertions.
fn summary(r: &Reconciliation) -> Vec<(ProposalKind, bool, Vec<Pair>)> {
    r.proposals
        .iter()
        .map(|p| {
            (
                p.kind,
                p.review,
                p.matches
                    .iter()
                    .map(|m| {
                        assert_eq!(m.status, MatchStatus::Proposed);
                        (
                            m.line.line.to_string(),
                            m.receipt.to_string(),
                            m.amount.minor(),
                        )
                    })
                    .collect(),
            )
        })
        .collect()
}

fn pair(line: &str, receipt: &str, minor: i64) -> Pair {
    (line.into(), receipt.into(), minor)
}

#[test]
fn the_nickel_defaults_are_a_valid_policy() {
    let p = policy();
    assert_eq!(p.validate(), Ok(()));
    assert_eq!(p.merchant.min_similarity, 60);
    assert_eq!(p.date.receipt.days_before, 5);
    // Overriding one field keeps the other defaults.
    let tuned: Policy =
        eval(r#"let m = import "rdx/matching.ncl" in { amount.tolerance_minor = 50 } | m.Policy"#);
    assert_eq!(tuned.amount.tolerance_minor, 50);
    assert_eq!(tuned.date, p.date);
}

#[test]
fn the_contract_rejects_floats_and_bad_rates() {
    let reject = |code: &str| {
        let full = format!(r#"let m = import "rdx/matching.ncl" in ({code}) | m.Policy"#);
        Program::builder()
            .source(Source::code(full))
            .import_paths([repo().join("nickel")])
            .build()
            .and_then(|mut p| p.export(ExportFormat::Json))
            .unwrap_err()
            .to_string()
    };
    assert!(reject("{ amount.tolerance_minor = 0.5 }").contains("whole number"));
    assert!(reject("{ merchant.min_similarity = 101 }").contains("percentage"));
    let bad_rate = reject(
        r#"{ fx.rates = [{ from = "EUR", to = "USD", rate = 1.08, date = "2026-03-01" }] }"#,
    );
    assert!(bad_rate.contains("exchange rate"), "{bad_rate}");
}

/// Every right match is proposed with confidence, and nothing confident is wrong, across many
/// generated months.
#[test]
fn recovers_the_generators_ground_truth() {
    let policy = policy();
    for seed in 0..100 {
        let s = Scenario::generate(&Config {
            seed,
            supporters: 3,
            months: 2,
            ..Config::small()
        });
        let truth: BTreeSet<(&LineRef, &ReceiptId)> =
            s.matches.iter().map(|m| (&m.line, &m.receipt)).collect();
        for st in &s.statements {
            let r = reconcile(st, &s.receipts, &[], &policy).unwrap();
            for p in &r.proposals {
                for m in &p.matches {
                    let right = truth.contains(&(&m.line, &m.receipt));
                    assert!(
                        right || p.review,
                        "seed {seed}: confident but wrong: {m:?} ({:?})",
                        p.reasons
                    );
                    if right {
                        assert_eq!(p.kind, ProposalKind::Exact, "seed {seed}: {p:?}");
                        assert!(!p.review, "seed {seed}: {:?}", p.reasons);
                    }
                }
            }
            let proposed: BTreeSet<(&LineRef, &ReceiptId)> = r
                .proposals
                .iter()
                .flat_map(|p| p.matches.iter().map(|m| (&m.line, &m.receipt)))
                .collect();
            for t in truth.iter().filter(|(l, _)| l.statement == st.id) {
                assert!(proposed.contains(t), "seed {seed}: missed {t:?}");
            }
            // Personal lines are never proposed, even for review.
            for case in s.cases.iter().filter(|c| c.kind == CaseKind::Personal) {
                let l = case.line.as_ref().unwrap();
                if l.statement == st.id {
                    assert!(r.unmatched_lines.contains(&l.line), "seed {seed}: {l:?}");
                }
            }
            // Another supporter's receipts are noted, never matched.
            assert_eq!(r.notes.len(), 1, "seed {seed}: {:?}", r.notes);
        }
    }
}

#[test]
fn the_committed_fixture_reconciles_completely() {
    let text =
        std::fs::read_to_string(repo().join("fixtures/synthetic/small/scenario.json")).unwrap();
    let s: Scenario = serde_json::from_str(&text).unwrap();
    let st = &s.statements[0];
    let r = reconcile(st, &s.receipts, &[], &policy()).unwrap();
    let expected: Vec<&Match> = s
        .matches
        .iter()
        .filter(|m| m.line.statement == st.id)
        .collect();
    let got: Vec<Match> = r
        .proposals
        .iter()
        .flat_map(|p| p.matches.iter().cloned())
        .map(|m| Match {
            status: MatchStatus::Confirmed,
            ..m
        })
        .collect();
    assert_eq!(got.iter().collect::<Vec<_>>(), expected);
    // Confirming them makes the redaction summary reconcile.
    let summary = st.redaction_summary(&got).unwrap();
    assert_eq!(
        summary
            .opening
            .checked_add(summary.shown)
            .unwrap()
            .checked_add(summary.redacted)
            .unwrap(),
        summary.closing
    );
    assert_eq!(r.summary.to_review, 0);
    assert_eq!(r.summary.proposed_lines, expected.len());
}

#[test]
fn exact_matches_respect_the_date_window_and_merchant() {
    let st = statement(vec![
        line("l1", "2026-03-10", "PAPERLEAF SUPPLY #0001", -42_00),
        line("l2", "2026-03-20", "CORNER GROCERY #0002", -42_00),
    ]);
    let receipts = [
        shop("r-ok", "Paperleaf Supply", "2026-03-07", 42_00),
        // Right merchant, but dated 8 days before l1: outside the 5-day window.
        shop("r-old", "Paperleaf Supply", "2026-03-02", 42_00),
    ];
    let r = reconcile(&st, &receipts, &[], &policy()).unwrap();
    assert_eq!(
        summary(&r),
        vec![(ProposalKind::Exact, false, vec![pair("l1", "r-ok", 42_00)])]
    );
    assert_eq!(r.unmatched_lines, vec![LineId::new("l2").unwrap()]);
    assert_eq!(r.unmatched_receipts, vec![ReceiptId::new("r-old").unwrap()]);
    assert_eq!(r.proposals[0].evidence[0].days, 3);
    assert_eq!(r.proposals[0].evidence[0].merchant_similarity, 100);
}

#[test]
fn amount_tolerance_is_policy_and_always_reviewed() {
    let st = statement(vec![line("l1", "2026-03-10", "LANTERN CAFE", -55_00)]);
    let receipts = [shop("r1", "Lantern Cafe Catering", "2026-03-10", 50_00)];
    // Exact only by default: a 5.00 tip is not a match.
    let r = reconcile(&st, &receipts, &[], &policy()).unwrap();
    assert!(r.proposals.is_empty());

    let mut tuned = policy();
    tuned.amount.tolerance_bps = 2000; // 20%: tips
    let r = reconcile(&st, &receipts, &[], &tuned).unwrap();
    assert_eq!(
        summary(&r),
        vec![(ProposalKind::Tolerance, true, vec![pair("l1", "r1", 55_00)])]
    );
    assert_eq!(r.proposals[0].evidence[0].difference, usd(5_00));
    assert!(r.proposals[0].reasons[0].contains("5.00 USD more"));
}

#[test]
fn a_printed_reference_beats_the_merchant_name_and_a_different_one_rules_out() {
    let mut paid = line("l1", "2026-03-20", "ONLINE PAYMENT", -80_00);
    paid.reference = Some("INV-000123".into());
    let mut other = line("l2", "2026-03-21", "NORTHWIND HOSTING", -80_00);
    other.reference = Some("INV-999999".into());
    let st = statement(vec![paid, other]);
    let mut bill = receipt(
        "r1",
        ReceiptKind::Bill,
        "Northwind Hosting",
        "2026-03-05",
        usd(80_00),
    );
    bill.reference = Some("inv 000123".into());
    let r = reconcile(&st, &[bill], &[], &policy()).unwrap();
    assert_eq!(
        summary(&r),
        vec![(ProposalKind::Exact, false, vec![pair("l1", "r1", 80_00)])]
    );
    assert!(r.proposals[0].evidence[0].reference);
}

#[test]
fn refunds_match_credits_and_purchases_match_debits() {
    let st = statement(vec![
        line("l1", "2026-03-10", "HARBOR HARDWARE", -30_00),
        line("l2", "2026-03-12", "HARBOR HARDWARE REFUND", 30_00),
    ]);
    let receipts = [
        receipt(
            "r-refund",
            ReceiptKind::Refund,
            "Harbor Hardware",
            "2026-03-11",
            usd(30_00),
        ),
        shop("r-buy", "Harbor Hardware", "2026-03-10", 30_00),
    ];
    let r = reconcile(&st, &receipts, &[], &policy()).unwrap();
    assert_eq!(
        summary(&r),
        vec![
            (ProposalKind::Exact, false, vec![pair("l1", "r-buy", 30_00)]),
            (
                ProposalKind::Exact,
                false,
                vec![pair("l2", "r-refund", 30_00)]
            ),
        ]
    );
}

#[test]
fn two_receipts_that_fit_one_line_are_flagged() {
    let st = statement(vec![line("l1", "2026-03-10", "BEANERY COFFEE", -4_50)]);
    let receipts = [
        shop("r1", "Beanery Coffee", "2026-03-10", 4_50),
        shop("r2", "Beanery Coffee", "2026-03-09", 4_50),
    ];
    let r = reconcile(&st, &receipts, &[], &policy()).unwrap();
    // The closer date wins, but a person checks.
    assert_eq!(
        summary(&r),
        vec![(ProposalKind::Exact, true, vec![pair("l1", "r1", 4_50)])]
    );
    assert_eq!(r.unmatched_receipts, vec![ReceiptId::new("r2").unwrap()]);
}

#[test]
fn one_line_paying_several_receipts_is_a_split_for_review() {
    let st = statement(vec![line("l1", "2026-03-10", "PAPERLEAF SUPPLY", -100_00)]);
    let receipts = [
        shop("r1", "Paperleaf Supply", "2026-03-09", 60_00),
        shop("r2", "Paperleaf Supply", "2026-03-09", 40_00),
        shop("r3", "Paperleaf Supply", "2026-03-09", 15_00),
    ];
    let r = reconcile(&st, &receipts, &[], &policy()).unwrap();
    assert_eq!(
        summary(&r),
        vec![(
            ProposalKind::Split,
            true,
            vec![pair("l1", "r1", 60_00), pair("l1", "r2", 40_00)]
        )]
    );
    assert_eq!(r.unmatched_receipts, vec![ReceiptId::new("r3").unwrap()]);

    let mut off = policy();
    off.split.max_parts = 1;
    off.partial.enabled = false;
    assert!(
        reconcile(&st, &receipts, &[], &off)
            .unwrap()
            .proposals
            .is_empty()
    );
}

#[test]
fn one_receipt_paid_over_several_lines_is_instalments_for_review() {
    let st = statement(vec![
        line("l1", "2026-03-05", "CLEARWAVE TELECOM", -50_00),
        line("l2", "2026-03-25", "CLEARWAVE TELECOM", -50_00),
    ]);
    let receipts = [receipt(
        "r1",
        ReceiptKind::Bill,
        "Clearwave Telecom",
        "2026-03-01",
        usd(100_00),
    )];
    let r = reconcile(&st, &receipts, &[], &policy()).unwrap();
    assert_eq!(
        summary(&r),
        vec![(
            ProposalKind::Instalments,
            true,
            vec![pair("l1", "r1", 50_00), pair("l2", "r1", 50_00)]
        )]
    );
}

#[test]
fn a_line_paying_part_of_a_receipt_is_partial_for_review() {
    let st = statement(vec![line(
        "l1",
        "2026-03-10",
        "LANTERN CAFE CATERING",
        -200_00,
    )]);
    let receipts = [shop("r1", "Lantern Cafe Catering", "2026-03-08", 500_00)];
    let r = reconcile(&st, &receipts, &[], &policy()).unwrap();
    assert_eq!(
        summary(&r),
        vec![(ProposalKind::Partial, true, vec![pair("l1", "r1", 200_00)])]
    );
    assert!(r.proposals[0].reasons[0].contains("300.00 USD"));

    let mut off = policy();
    off.partial.enabled = false;
    assert!(
        reconcile(&st, &receipts, &[], &off)
            .unwrap()
            .proposals
            .is_empty()
    );
}

#[test]
fn other_currencies_match_at_the_policy_rate() {
    let st = statement(vec![line("l1", "2026-03-10", "METRO RIDESHARE", -10_95)]);
    let eur = Currency::new("EUR").unwrap();
    let receipts = [receipt(
        "r1",
        ReceiptKind::Receipt,
        "Metro Rideshare",
        "2026-03-09",
        Money::new(10_00, eur),
    )];
    // No rate: noted, not matched.
    let r = reconcile(&st, &receipts, &[], &policy()).unwrap();
    assert!(r.proposals.is_empty());
    assert!(r.notes[0].contains("no EUR to USD rate"), "{:?}", r.notes);

    let mut p = policy();
    for (on, rate) in [("2026-02-01", "1.2000"), ("2026-03-09", "1.0842")] {
        p.fx.rates.push(FxRate {
            from: eur,
            to: Currency::USD,
            rate: rate.into(),
            date: date(on),
            source: "ecb".into(),
        });
    }
    let r = reconcile(&st, &receipts, &[], &p).unwrap();
    // 10.00 EUR × 1.0842 = 10.84 USD; the card charged 10.95 (1%), within the 3% fx tolerance.
    assert_eq!(
        summary(&r),
        vec![(ProposalKind::Fx, true, vec![pair("l1", "r1", 10_95)])]
    );
    let fx = r.proposals[0].evidence[0].fx.as_ref().unwrap();
    assert_eq!(
        (fx.total, fx.rate.as_str(), fx.converted),
        (Money::new(10_00, eur), "1.0842", usd(10_84))
    );
    assert_eq!(fx.date, date("2026-03-09"));
}

#[test]
fn earlier_decisions_are_respected() {
    let st = statement(vec![
        line("l1", "2026-03-10", "PAPERLEAF SUPPLY", -20_00),
        line("l2", "2026-03-11", "HARBOR HARDWARE", -35_00),
    ]);
    let receipts = [
        shop("r1", "Paperleaf Supply", "2026-03-10", 20_00),
        shop("r2", "Harbor Hardware", "2026-03-11", 35_00),
    ];
    let decided = |line: &str, receipt: &str, status| Match {
        line: LineRef {
            statement: st.id.clone(),
            line: LineId::new(line).unwrap(),
        },
        receipt: ReceiptId::new(receipt).unwrap(),
        amount: usd(1),
        status,
    };
    let r = reconcile(
        &st,
        &receipts,
        &[
            decided("l1", "r1", MatchStatus::Confirmed),
            decided("l2", "r2", MatchStatus::Rejected),
        ],
        &policy(),
    )
    .unwrap();
    assert!(r.proposals.is_empty());
    assert_eq!(r.unmatched_lines, vec![LineId::new("l2").unwrap()]);
    assert_eq!(r.unmatched_receipts, vec![ReceiptId::new("r2").unwrap()]);
    assert_eq!(r.summary.decided_lines, 1);
}

#[test]
fn inconsistent_input_is_an_error_an_agent_can_act_on() {
    let mut st = statement(vec![line("l1", "2026-03-10", "X", -1_00)]);
    st.closing = usd(0);
    let err = reconcile(&st, &[], &[], &policy()).unwrap_err();
    assert!(err.to_string().contains("does not balance"), "{err}");

    let st = statement(vec![]);
    let r = shop("r1", "X", "2026-03-10", 1_00);
    let err = reconcile(&st, &[r.clone(), r], &[], &policy()).unwrap_err();
    assert_eq!(
        err,
        MatchError::DuplicateReceipt(ReceiptId::new("r1").unwrap())
    );
}
