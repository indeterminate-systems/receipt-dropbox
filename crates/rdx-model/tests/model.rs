//! Behaviour of the domain types, written before them (docs/workflow.md, stage 2).

// Amounts are written as `dollars_cents` (`42_17` is 42.17) so they read as money.
#![allow(clippy::inconsistent_digit_grouping)]

use rdx_model::*;

fn usd(minor: i64) -> Money {
    Money::new(minor, Currency::USD)
}

fn date(s: &str) -> Date {
    s.parse().unwrap()
}

// ---------------------------------------------------------------- money

#[test]
fn money_parses_decimal_strings_exactly() {
    assert_eq!(Money::parse("42.17", Currency::USD).unwrap(), usd(4217));
    assert_eq!(Money::parse("-0.05", Currency::USD).unwrap(), usd(-5));
    assert_eq!(Money::parse("7", Currency::USD).unwrap(), usd(700));
    let jpy = Currency::new("JPY").unwrap();
    assert_eq!(Money::parse("1200", jpy).unwrap().minor(), 1200);
    // More decimals than the currency has would need rounding, which is never silent.
    assert!(Money::parse("1.005", Currency::USD).is_err());
    assert!(Money::parse("1.5", jpy).is_err());
    assert!(Money::parse("1e3", Currency::USD).is_err());
}

#[test]
fn money_displays_in_major_units() {
    assert_eq!(usd(4217).to_string(), "42.17 USD");
    assert_eq!(usd(-5).to_string(), "-0.05 USD");
    let kwd = Currency::new("KWD").unwrap();
    assert_eq!(Money::new(1500, kwd).to_string(), "1.500 KWD");
}

#[test]
fn money_never_mixes_currencies() {
    let eur = Money::new(100, Currency::new("EUR").unwrap());
    assert_eq!(
        usd(100).checked_add(eur),
        Err(MoneyError::CurrencyMismatch {
            left: Currency::USD,
            right: Currency::new("EUR").unwrap()
        })
    );
    assert_eq!(usd(100).checked_add(usd(-30)), Ok(usd(70)));
    assert!(usd(i64::MAX).checked_add(usd(1)).is_err());
}

#[test]
fn unknown_currency_codes_are_rejected() {
    assert!(Currency::new("usd").is_err());
    assert!(Currency::new("XYZ").is_err());
}

#[test]
fn money_serializes_as_minor_units_and_code() {
    let json = serde_json::to_value(usd(4217)).unwrap();
    assert_eq!(
        json,
        serde_json::json!({ "minor": 4217, "currency": "USD" })
    );
    let back: Money = serde_json::from_value(json).unwrap();
    assert_eq!(back, usd(4217));
    assert!(serde_json::from_str::<Money>(r#"{"minor":42.17,"currency":"USD"}"#).is_err());
}

// ---------------------------------------------------------------- dates

#[test]
fn dates_parse_validate_and_count_days() {
    assert!("2026-02-29".parse::<Date>().is_err());
    assert!("2028-02-29".parse::<Date>().is_ok());
    assert!("2026-13-01".parse::<Date>().is_err());
    assert!("2026-1-01".parse::<Date>().is_err());
    assert_eq!(date("2026-03-01").add_days(-1), date("2026-02-28"));
    assert_eq!(date("2026-12-31").add_days(1), date("2027-01-01"));
    assert_eq!(date("2026-03-01").days_since(date("2026-02-01")), 28);
    assert!(date("2026-03-01") > date("2026-02-28"));
    assert_eq!(date("2026-03-01").to_string(), "2026-03-01");
    assert_eq!(
        serde_json::to_value(date("2026-03-01")).unwrap(),
        "2026-03-01"
    );
}

// ---------------------------------------------------------------- digests and ids

#[test]
fn digests_are_checked_and_computed() {
    assert!(Digest::parse("sha256:abc").is_err());
    assert!(Digest::parse(&format!("sha256:{}", "A".repeat(64))).is_err());
    // echo -n hello | sha256sum
    assert_eq!(
        Digest::of_bytes(b"hello").as_str(),
        "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
    // Same canonical form as rdx_plan::digest.
    let v = serde_json::json!({"b": 1, "a": 2});
    assert_eq!(Digest::of_json(&v).as_str(), rdx_plan::digest(&v));
}

#[test]
fn ids_are_lowercase_slugs() {
    assert!(SupporterId::new("s-0042").is_ok());
    assert!(StatementId::new("st-2026-03.s-0042").is_ok());
    assert!(ReceiptId::new("Receipt 1").is_err());
    assert!(LineId::new("").is_err());
    assert!(serde_json::from_str::<SupporterId>(r#""S 1""#).is_err());
}

// ---------------------------------------------------------------- statements

fn line(id: &str, d: &str, desc: &str, minor: i64) -> StatementLine {
    StatementLine {
        id: LineId::new(id).unwrap(),
        date: date(d),
        description: desc.into(),
        amount: usd(minor),
        reference: None,
    }
}

fn statement(lines: Vec<StatementLine>, closing: i64) -> Statement {
    Statement {
        id: StatementId::new("st-1").unwrap(),
        supporter: SupporterId::new("s-0001").unwrap(),
        account: AccountRef {
            institution: "Example Bank".into(),
            mask: "0001".into(),
        },
        currency: Currency::USD,
        period: Period::new(date("2026-03-01"), date("2026-03-31")).unwrap(),
        opening: usd(100_00),
        closing: usd(closing),
        lines,
        original: None,
    }
}

#[test]
fn a_statement_balances_when_opening_plus_lines_equals_closing() {
    let s = statement(
        vec![
            line("l1", "2026-03-02", "PAPER CO", -42_17),
            line("l2", "2026-03-05", "TRANSFER IN", 50_00),
        ],
        107_83,
    );
    assert_eq!(s.validate(), Ok(()));
}

#[test]
fn statement_problems_are_all_reported() {
    let mut l3 = line("l3", "2026-04-02", "LATE", -1_00);
    l3.amount = Money::new(-100, Currency::new("EUR").unwrap());
    let s = statement(
        vec![
            line("l1", "2026-03-02", "PAPER CO", -42_17),
            line("l1", "2026-03-03", "DUPLICATE ID", -1_00),
            l3,
        ],
        0,
    );
    let problems = s.validate().unwrap_err();
    assert!(problems.contains(&StatementProblem::DuplicateLine(LineId::new("l1").unwrap())));
    assert!(problems.contains(&StatementProblem::OutsidePeriod(LineId::new("l3").unwrap())));
    assert!(problems.contains(&StatementProblem::Currency(LineId::new("l3").unwrap())));
    assert!(
        problems
            .iter()
            .any(|p| matches!(p, StatementProblem::Unbalanced { .. }))
    );
}

#[test]
fn a_period_cannot_end_before_it_starts() {
    assert!(Period::new(date("2026-03-31"), date("2026-03-01")).is_err());
}

// ---------------------------------------------------------------- receipts

fn receipt(id: &str, total: i64) -> Receipt {
    Receipt {
        id: ReceiptId::new(id).unwrap(),
        supporter: SupporterId::new("s-0001").unwrap(),
        kind: ReceiptKind::Receipt,
        merchant: "Paper Co".into(),
        date: date("2026-03-01"),
        total: usd(total),
        reference: None,
        items: vec![],
        source: Digest::of_bytes(id.as_bytes()),
    }
}

#[test]
fn receipt_items_must_add_up_to_the_total() {
    let mut r = receipt("r1", 42_17);
    assert_eq!(r.validate(), Ok(()));
    r.items = vec![
        ReceiptItem {
            description: "Paper".into(),
            amount: usd(40_00),
        },
        ReceiptItem {
            description: "Tax".into(),
            amount: usd(2_00),
        },
    ];
    assert_eq!(
        r.validate(),
        Err(ReceiptProblem::ItemsDoNotAddUp {
            items: usd(42_00),
            total: usd(42_17)
        })
    );
    assert_eq!(
        receipt("r2", 0).validate(),
        Err(ReceiptProblem::NotPositive)
    );
}

// ---------------------------------------------------------------- matches and the redacted view

fn confirmed(line: &str, receipt: &str, minor: i64) -> Match {
    Match {
        line: LineRef {
            statement: StatementId::new("st-1").unwrap(),
            line: LineId::new(line).unwrap(),
        },
        receipt: ReceiptId::new(receipt).unwrap(),
        amount: usd(minor),
        status: MatchStatus::Confirmed,
    }
}

#[test]
fn lines_are_org_only_when_confirmed_matches_cover_them() {
    let s = statement(
        vec![
            line("l1", "2026-03-02", "PAPER CO", -42_17),
            line("l2", "2026-03-03", "GROCER", -20_00),
            line("l3", "2026-03-04", "HARDWARE", -30_00),
            line("l4", "2026-03-05", "PRINTER", -10_00),
        ],
        -2_17,
    );
    let mut proposed = confirmed("l4", "r4", 10_00);
    proposed.status = MatchStatus::Proposed;
    let matches = [
        confirmed("l1", "r1", 42_17),
        confirmed("l3", "r3", 10_00),
        proposed,
    ];
    let classes = s.classify(&matches);
    assert_eq!(
        classes,
        vec![
            LineClass::Org,
            LineClass::Personal,
            LineClass::PartlyMatched,
            LineClass::Personal
        ]
    );

    // The redacted view still reconciles: opening + shown + redacted = closing.
    let summary = s.redaction_summary(&matches).unwrap();
    assert_eq!(summary.shown, usd(-42_17));
    assert_eq!(summary.redacted, usd(-60_00));
    assert_eq!(
        summary
            .opening
            .checked_add(summary.shown)
            .and_then(|m| m.checked_add(summary.redacted)),
        Ok(summary.closing)
    );
}

#[test]
fn matches_on_other_statements_are_ignored() {
    let s = statement(vec![line("l1", "2026-03-02", "PAPER CO", -42_17)], 57_83);
    let mut other = confirmed("l1", "r1", 42_17);
    other.line.statement = StatementId::new("st-2").unwrap();
    assert_eq!(s.classify(&[other]), vec![LineClass::Personal]);
}

// ---------------------------------------------------------------- ledger

#[test]
fn beancount_account_names_are_checked() {
    assert!(Account::new("Expenses:Office:Supplies").is_ok());
    assert!(Account::new("Liabilities:Reimbursable:S-0042").is_ok());
    assert!(Account::new("Expenses:office").is_err());
    assert!(Account::new("Spending:Office").is_err());
    assert!(Account::new("Expenses").is_err());
    assert_eq!(
        Account::reimbursable(&SupporterId::new("s-0042").unwrap()).as_str(),
        "Liabilities:Reimbursable:S-0042"
    );
}

#[test]
fn a_reimbursable_entry_links_receipt_and_statement_and_balances() {
    let s = statement(vec![line("l1", "2026-03-02", "PAPER CO", -42_17)], 57_83);
    let r = receipt("r1", 42_17);
    let m = confirmed("l1", "r1", 42_17);
    let original = Digest::of_bytes(b"encrypted original");
    let entry = LedgerEntry::reimbursable(
        &s,
        &r,
        &m,
        original.clone(),
        Account::new("Expenses:Office:Supplies").unwrap(),
    )
    .unwrap();
    assert_eq!(entry.date, date("2026-03-02"));
    assert_eq!(entry.payee, "Paper Co");
    assert_eq!(entry.statement, original);
    assert_eq!(entry.receipt_source, r.source);
    assert_eq!(entry.postings.len(), 2);
    assert_eq!(entry.postings[0].amount, Some(usd(42_17)));
    assert_eq!(entry.postings[1].amount, Some(usd(-42_17)));
    assert_eq!(entry.validate(), Ok(()));
}

#[test]
fn reimbursable_entries_need_a_confirmed_match_for_that_line() {
    let s = statement(vec![line("l1", "2026-03-02", "PAPER CO", -42_17)], 57_83);
    let r = receipt("r1", 42_17);
    let mut m = confirmed("l1", "r1", 42_17);
    m.status = MatchStatus::Proposed;
    let account = Account::new("Expenses:Office:Supplies").unwrap();
    let original = Digest::of_bytes(b"x");
    assert!(LedgerEntry::reimbursable(&s, &r, &m, original.clone(), account.clone()).is_err());
    let mut m = confirmed("l9", "r1", 42_17);
    m.status = MatchStatus::Confirmed;
    assert!(LedgerEntry::reimbursable(&s, &r, &m, original, account).is_err());
}

#[test]
fn unbalanced_entries_are_rejected() {
    let s = statement(vec![line("l1", "2026-03-02", "PAPER CO", -42_17)], 57_83);
    let r = receipt("r1", 42_17);
    let m = confirmed("l1", "r1", 42_17);
    let mut entry = LedgerEntry::reimbursable(
        &s,
        &r,
        &m,
        Digest::of_bytes(b"x"),
        Account::new("Expenses:Office:Supplies").unwrap(),
    )
    .unwrap();
    entry.postings[1].amount = Some(usd(-40_00));
    assert!(entry.validate().is_err());
    // One posting may leave its amount for Beancount to infer.
    entry.postings[1].amount = None;
    assert_eq!(entry.validate(), Ok(()));
}
