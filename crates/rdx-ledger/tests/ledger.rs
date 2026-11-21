//! Booking the synthetic scenario's confirmed matches, writing Beancount, reading it back, and
//! checking it with an independent parser.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rdx_ledger::{Accounts, BookError, Booking, Ledger, PostError, ReadError, book};
use rdx_model::{Account, Digest, Match, MatchStatus, Receipt, ReceiptId, Statement};
use serde::Deserialize;

#[derive(Deserialize)]
struct Scenario {
    statements: Vec<Statement>,
    receipts: Vec<Receipt>,
    matches: Vec<Match>,
    accounts: BTreeMap<ReceiptId, Account>,
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn scenario() -> Scenario {
    let text = std::fs::read_to_string(repo().join("fixtures/synthetic/small/scenario.json"))
        .expect("the synthetic scenario");
    serde_json::from_str(&text).unwrap()
}

fn accounts(s: &Scenario) -> Accounts {
    Accounts {
        fallback: Account::new("Expenses:Uncategorized").unwrap(),
        by_receipt: s.accounts.clone(),
        by_merchant: BTreeMap::new(),
    }
}

fn bookings(s: &Scenario) -> Vec<Booking> {
    s.statements
        .iter()
        .map(|st| book(st, &s.receipts, &s.matches, &accounts(s), None).unwrap())
        .collect()
}

fn ledger_of(bookings: &[Booking]) -> Ledger {
    let mut l = Ledger::default();
    for b in bookings {
        l.post(b).unwrap();
    }
    l
}

/// Compares with a golden file; `UPDATE_GOLDEN=1` rewrites it.
fn golden(path: &str, actual: &str) {
    let path = repo().join(path);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, actual).unwrap();
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e} (UPDATE_GOLDEN=1 to create)", path.display()));
    assert_eq!(
        actual,
        expected,
        "{} differs (UPDATE_GOLDEN=1 to accept)",
        path.display()
    );
}

#[test]
fn books_every_confirmed_match_with_its_audit_links() {
    let s = scenario();
    for (st, b) in s.statements.iter().zip(bookings(&s)) {
        let confirmed: Vec<&Match> = s
            .matches
            .iter()
            .filter(|m| m.line.statement == st.id && m.status == MatchStatus::Confirmed)
            .collect();
        assert!(!confirmed.is_empty());
        assert_eq!(b.entries.len(), confirmed.len());
        assert_eq!(b.summary.uncategorized, 0, "{:?}", b.notes);
        for e in &b.entries {
            assert_eq!(Some(&e.statement), st.original.as_ref());
            let r = s.receipts.iter().find(|r| r.id == e.receipt).unwrap();
            assert_eq!(e.receipt_source, r.source);
            assert_eq!(e.postings[0].account, s.accounts[&r.id]);
            assert_eq!(e.postings[1].account, Account::reimbursable(&st.supporter));
        }
    }
}

#[test]
fn a_refund_reverses_the_expense() {
    let s = scenario();
    let b = &bookings(&s)[0];
    let refund = b
        .entries
        .iter()
        .find(|e| e.line.line.as_str() == "l03")
        .expect("l03 is the scenario's refund");
    assert_eq!(refund.postings[0].amount.unwrap().minor(), -25085);
    assert_eq!(refund.postings[1].amount.unwrap().minor(), 25085);
}

#[test]
fn writes_the_golden_ledger() {
    let s = scenario();
    let text = ledger_of(&bookings(&s)).render().unwrap();
    golden("fixtures/ledger/synthetic-small.beancount", &text);
}

#[test]
fn the_file_does_not_depend_on_posting_order_and_reposting_changes_nothing() {
    let s = scenario();
    let mut bs = bookings(&s);
    let forward = ledger_of(&bs).render().unwrap();
    bs.reverse();
    let mut backward = ledger_of(&bs);
    assert_eq!(backward.render().unwrap(), forward);

    let report = backward.post(&bs[0]).unwrap();
    assert!(report.added.is_empty());
    assert!(!report.new_statement);
    assert_eq!(report.already_booked, bs[0].entries.len());
    assert_eq!(backward.render().unwrap(), forward);
}

#[test]
fn reads_back_what_it_writes() {
    let s = scenario();
    let ledger = ledger_of(&bookings(&s));
    let text = ledger.render().unwrap();
    assert_eq!(Ledger::parse(&text).unwrap(), ledger);
    assert_eq!(
        Ledger::parse(Ledger::empty_text()).unwrap(),
        Ledger::default()
    );
}

#[test]
fn refuses_a_ledger_edited_outside_rdx() {
    let s = scenario();
    let text = ledger_of(&bookings(&s)).render().unwrap();

    // A changed balance assertion: still Beancount, but not what the entries say.
    let balance = text.lines().find(|l| l.contains(" balance ")).unwrap();
    let edited = text.replacen(balance, &balance.replace(" -", " -1"), 1);
    assert!(matches!(
        Ledger::parse(&edited),
        Err(ReadError::NotCanonical { .. })
    ));

    // A changed posting: the entry no longer balances.
    let posting = text.lines().find(|l| l.starts_with("  Expenses:")).unwrap();
    let edited = text.replacen(posting, &posting.replacen('.', "1.", 1), 1);
    let err = Ledger::parse(&edited).unwrap_err();
    assert!(err.to_string().contains("sum to"), "{err}");

    // Another directive.
    let edited = format!("{text}\n2026-05-01 close Expenses:Office:Supplies\n");
    assert!(matches!(
        Ledger::parse(&edited),
        Err(ReadError::Syntax { .. })
    ));
}

#[test]
fn an_independent_beancount_parser_reads_it() {
    use beancount_parser::{BeancountFile, DirectiveContent, metadata::Value};

    let s = scenario();
    let ledger = ledger_of(&bookings(&s));
    let text = ledger.render().unwrap();
    let file: BeancountFile<f64> = text.parse().expect("valid Beancount");

    let mut opened = std::collections::BTreeSet::new();
    let (mut transactions, mut balances) = (0, 0);
    for d in &file.directives {
        match &d.content {
            DirectiveContent::Open(o) => {
                opened.insert(o.account.as_str().to_string());
            }
            DirectiveContent::Transaction(t) => {
                transactions += 1;
                let sum: f64 = t
                    .postings
                    .iter()
                    .map(|p| p.amount.as_ref().unwrap().value)
                    .sum();
                assert!(sum.abs() < 0.005, "unbalanced at line {}", d.line_number);
                for p in &t.postings {
                    assert!(
                        opened.contains(p.account.as_str()),
                        "{} not open",
                        p.account.as_str()
                    );
                }
                for key in [
                    "supporter",
                    "line",
                    "receipt",
                    "receipt-source",
                    "statement",
                ] {
                    assert!(
                        matches!(d.metadata.get(key), Some(Value::String(_))),
                        "line {}: no `{key}`",
                        d.line_number
                    );
                }
                let Some(Value::String(statement)) = d.metadata.get("statement") else {
                    unreachable!()
                };
                assert!(Digest::parse(statement).is_ok());
                assert_eq!(t.links.len(), 1);
            }
            DirectiveContent::Balance(_) => {
                balances += 1;
                assert!(matches!(
                    d.metadata.get("statement"),
                    Some(Value::String(_))
                ));
            }
            other => panic!("unexpected directive {other:?}"),
        }
    }
    assert_eq!(transactions, ledger.entries.len());
    assert_eq!(balances, s.statements.len());
}

#[test]
fn balance_assertions_hold() {
    let s = scenario();
    let ledger = ledger_of(&bookings(&s));
    for b in ledger.balances().unwrap() {
        let owed: i64 = ledger
            .entries
            .iter()
            .filter(|e| e.supporter == b.statement.supporter && e.date < b.date)
            .map(|e| e.postings[1].amount.unwrap().minor())
            .sum();
        assert_eq!(b.amount.minor(), owed);
        assert!(owed < 0, "the org owes its supporters");
    }
}

#[test]
fn proposed_and_rejected_matches_are_not_booked() {
    let s = scenario();
    let st = &s.statements[0];
    let mut matches = s.matches.clone();
    for m in matches
        .iter_mut()
        .filter(|m| m.line.statement == st.id)
        .take(2)
    {
        m.status = MatchStatus::Proposed;
    }
    let b = book(st, &s.receipts, &matches, &accounts(&s), None).unwrap();
    assert_eq!(b.skipped.len(), 2);
    assert!(b.skipped[0].reason.contains("confirm"));
    assert!(b.notes.iter().any(|n| n.contains("other statements")));
}

#[test]
fn unclassified_receipts_go_to_the_default_account_with_a_note() {
    let s = scenario();
    let mut a = accounts(&s);
    a.by_receipt.clear();
    a.by_merchant.insert(
        "  PAPERLEAF supply ".into(),
        Account::new("Expenses:Office").unwrap(),
    );
    let b = book(&s.statements[0], &s.receipts, &s.matches, &a, None).unwrap();
    assert!(b.summary.uncategorized > 0);
    assert!(b.notes.iter().any(|n| n.contains("Expenses:Uncategorized")));
    assert!(
        b.entries
            .iter()
            .any(|e| e.postings[0].account.as_str() == "Expenses:Office"),
        "by_merchant ignores case and spaces"
    );
}

#[test]
fn every_entry_needs_the_encrypted_original() {
    let s = scenario();
    let mut st = s.statements[0].clone();
    let original = st.original.take().unwrap();
    assert!(matches!(
        book(&st, &s.receipts, &s.matches, &accounts(&s), None),
        Err(BookError::NoOriginal(_))
    ));
    let b = book(&st, &s.receipts, &s.matches, &accounts(&s), Some(&original)).unwrap();
    assert!(b.entries.iter().all(|e| e.statement == original));
}

#[test]
fn bad_matches_are_refused() {
    let s = scenario();
    let st = &s.statements[0];
    let first = s
        .matches
        .iter()
        .find(|m| m.line.statement == st.id)
        .unwrap();

    let mut doubled = s.matches.clone();
    doubled.push(first.clone());
    assert!(matches!(
        book(st, &s.receipts, &doubled, &accounts(&s), None),
        Err(BookError::Duplicate { .. })
    ));

    let others: Vec<Receipt> = s
        .receipts
        .iter()
        .filter(|r| r.id != first.receipt)
        .cloned()
        .collect();
    assert!(matches!(
        book(st, &others, &s.matches, &accounts(&s), None),
        Err(BookError::UnknownReceipt(_))
    ));

    let mut over = s.matches.clone();
    let m = over.iter_mut().find(|m| m.line == first.line).unwrap();
    m.amount = rdx_model::Money::new(m.amount.minor() + 1, m.amount.currency());
    let err = book(st, &s.receipts, &over, &accounts(&s), None).unwrap_err();
    assert!(
        err.to_string().contains(&first.line.line.to_string()),
        "{err}"
    );
}

#[test]
fn a_booked_line_cannot_be_booked_differently() {
    let s = scenario();
    let bs = bookings(&s);
    let mut ledger = ledger_of(&bs);
    let mut changed = bs[0].clone();
    changed.entries[0].narration = "something else".into();
    assert!(matches!(
        ledger.post(&changed),
        Err(PostError::Conflict { .. })
    ));
    let mut other = bs[0].clone();
    other.statement.original = Digest::of_bytes(b"another original");
    assert!(matches!(
        ledger.post(&other),
        Err(PostError::OtherOriginal { .. })
    ));
}

#[test]
fn quotes_in_payees_cannot_break_the_file() {
    let s = scenario();
    let mut receipts = s.receipts.clone();
    for r in &mut receipts {
        r.merchant = format!("Bob's \"Best\" \\ Shop\n{}", r.merchant);
    }
    let b = book(&s.statements[0], &receipts, &s.matches, &accounts(&s), None).unwrap();
    let ledger = ledger_of(&[b]);
    let text = ledger.render().unwrap();
    assert_eq!(Ledger::parse(&text).unwrap(), ledger);
    let _: beancount_parser::BeancountFile<f64> = text.parse().unwrap();
}
