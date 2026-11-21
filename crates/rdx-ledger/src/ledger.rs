use std::collections::BTreeMap;
use std::fmt::Write as _;

use rdx_model::{Account, Date, LedgerEntry, LineRef, Money, MoneyError, ReceiptId, StatementId};
use serde::{Deserialize, Serialize};

use crate::book::clean_text;
use crate::{Booking, StatementRecord};

/// Every booked entry and every statement booked from. The Beancount text ([`Ledger::render`]) is
/// derived from this, so the file is the same whatever order statements were posted in.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ledger {
    pub entries: Vec<LedgerEntry>,
    pub statements: BTreeMap<StatementId, StatementRecord>,
}

/// A statement's balance assertion: what the organization owes the supporter, in the statement's
/// currency, at the start of the day after the statement period.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Balance {
    pub date: Date,
    pub account: Account,
    pub amount: Money,
    pub statement: StatementRecord,
}

/// What `ledger.post` changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostReport {
    pub statement: StatementId,
    /// Entries newly booked, as `statement/line receipt`.
    pub added: Vec<String>,
    /// Entries already in the ledger exactly as given (posting again is harmless).
    pub already_booked: usize,
    /// Whether the statement itself (its balance assertion) is new to the ledger.
    pub new_statement: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PostError {
    #[error(
        "statement `{id}` is already in the ledger with original {existing}, not {given}; a \
         statement is booked from one original"
    )]
    OtherOriginal {
        id: StatementId,
        existing: String,
        given: String,
    },
    #[error(
        "line `{line}` with receipt `{receipt}` is already booked differently; the ledger is \
         append-only, so book a correcting entry instead of re-posting"
    )]
    Conflict { line: String, receipt: ReceiptId },
    #[error("entry for line `{line}` belongs to statement `{entry}`, not `{booking}`")]
    WrongStatement {
        line: String,
        entry: StatementId,
        booking: StatementId,
    },
    #[error("entry for line `{line}`, receipt `{receipt}` is not valid: {reason}")]
    Invalid {
        line: String,
        receipt: ReceiptId,
        reason: String,
    },
    #[error(transparent)]
    Money(#[from] MoneyError),
}

pub(crate) const HEADER: &str = "\
; Receipt Dropbox ledger (Beancount). Written by rdx's `ledger.post`, which rewrites the whole
; file from its entries: change it through rdx, since `ledger.read` refuses hand edits.
";

fn key(e: &LedgerEntry) -> (&LineRef, &ReceiptId) {
    (&e.line, &e.receipt)
}

impl Ledger {
    /// Adds a booking. Idempotent: an entry already booked identically is counted, not added
    /// again. Everything is checked before anything changes.
    pub fn post(&mut self, booking: &Booking) -> Result<PostReport, PostError> {
        let id = &booking.statement.id;
        let new_statement = match self.statements.get(id) {
            Some(existing) if existing.original != booking.statement.original => {
                return Err(PostError::OtherOriginal {
                    id: id.clone(),
                    existing: existing.original.to_string(),
                    given: booking.statement.original.to_string(),
                });
            }
            Some(_) => false,
            None => true,
        };
        let mut added = Vec::new();
        let mut already_booked = 0;
        let mut fresh = Vec::new();
        for e in &booking.entries {
            let line = format!("{}/{}", e.line.statement, e.line.line);
            if &e.line.statement != id {
                return Err(PostError::WrongStatement {
                    line,
                    entry: e.line.statement.clone(),
                    booking: id.clone(),
                });
            }
            let invalid = |reason: String| PostError::Invalid {
                line: line.clone(),
                receipt: e.receipt.clone(),
                reason,
            };
            e.validate().map_err(|err| invalid(err.to_string()))?;
            if e.supporter != booking.statement.supporter
                || e.statement != booking.statement.original
            {
                return Err(invalid(
                    "its supporter or statement digest differs from the booking's statement".into(),
                ));
            }
            if e.payee != clean_text(&e.payee) || e.narration != clean_text(&e.narration) {
                return Err(invalid(
                    "payee and narration must be single-line text without `\"` or `\\`; book \
                     entries with ledger.book"
                        .into(),
                ));
            }
            match self.entries.iter().find(|x| key(x) == key(e)) {
                Some(x) if x == e => already_booked += 1,
                Some(_) => {
                    return Err(PostError::Conflict {
                        line,
                        receipt: e.receipt.clone(),
                    });
                }
                None => {
                    added.push(format!("{line} {}", e.receipt));
                    fresh.push(e.clone());
                }
            }
        }
        self.entries.extend(fresh);
        self.statements
            .insert(id.clone(), booking.statement.clone());
        self.sort();
        Ok(PostReport {
            statement: id.clone(),
            added,
            already_booked,
            new_statement,
        })
    }

    pub(crate) fn sort(&mut self) {
        self.entries
            .sort_by(|a, b| (a.date, key(a)).cmp(&(b.date, key(b))));
    }

    /// One balance assertion per statement: the supporter's reimbursable account, summed over
    /// every entry dated before the assertion. Recomputed on every render, so posting statements
    /// out of order keeps them true; `bean-check` then catches an edit made outside rdx.
    pub fn balances(&self) -> Result<Vec<Balance>, MoneyError> {
        self.statements
            .values()
            .map(|s| {
                let date = s.period_end.add_days(1);
                let account = Account::reimbursable(&s.supporter);
                let amounts: Vec<Money> = self
                    .entries
                    .iter()
                    .filter(|e| e.date < date)
                    .flat_map(|e| &e.postings)
                    .filter(|p| p.account == account)
                    .filter_map(|p| p.amount)
                    .filter(|a| a.currency() == s.currency)
                    .collect();
                Ok(Balance {
                    date,
                    amount: Money::sum(s.currency, &amounts)?,
                    account,
                    statement: s.clone(),
                })
            })
            .collect()
    }

    /// The ledger as canonical Beancount: `open` directives (dated each account's first use),
    /// then balance assertions and transactions by date.
    pub fn render(&self) -> Result<String, MoneyError> {
        let balances = self.balances()?;
        let mut opens: BTreeMap<&Account, Date> = BTreeMap::new();
        let uses = self
            .entries
            .iter()
            .flat_map(|e| e.postings.iter().map(move |p| (&p.account, e.date)))
            .chain(balances.iter().map(|b| (&b.account, b.date)));
        for (account, date) in uses {
            let first = opens.entry(account).or_insert(date);
            *first = (*first).min(date);
        }

        // Balance assertions apply at the start of their day, so they sort before that day's
        // transactions, as Beancount itself orders them.
        enum Item<'a> {
            Balance(&'a Balance),
            Entry(&'a LedgerEntry),
        }
        let mut items: Vec<Item<'_>> = balances
            .iter()
            .map(Item::Balance)
            .chain(self.entries.iter().map(Item::Entry))
            .collect();
        items.sort_by(|a, b| {
            let k = |i: &Item<'_>| match i {
                Item::Balance(b) => (b.date, 0, b.statement.id.to_string(), String::new()),
                Item::Entry(e) => (
                    e.date,
                    1,
                    format!("{}/{}", e.line.statement, e.line.line),
                    e.receipt.to_string(),
                ),
            };
            k(a).cmp(&k(b))
        });

        let mut out = String::from(HEADER);
        if !opens.is_empty() {
            out.push('\n');
            for (account, date) in &opens {
                writeln!(out, "{date} open {account}").unwrap();
            }
        }
        for item in items {
            out.push('\n');
            match item {
                Item::Balance(b) => {
                    writeln!(out, "{} balance {} {}", b.date, b.account, b.amount).unwrap();
                    meta(&mut out, "statement-id", b.statement.id.as_str());
                    meta(&mut out, "supporter", b.statement.supporter.as_str());
                    meta(&mut out, "statement", b.statement.original.as_str());
                }
                Item::Entry(e) => {
                    writeln!(
                        out,
                        "{} * \"{}\" \"{}\" ^{}",
                        e.date, e.payee, e.narration, e.line.statement
                    )
                    .unwrap();
                    meta(&mut out, "supporter", e.supporter.as_str());
                    meta(
                        &mut out,
                        "line",
                        &format!("{}/{}", e.line.statement, e.line.line),
                    );
                    meta(&mut out, "receipt", e.receipt.as_str());
                    meta(&mut out, "receipt-source", e.receipt_source.as_str());
                    meta(&mut out, "statement", e.statement.as_str());
                    for p in &e.postings {
                        match p.amount {
                            Some(a) => writeln!(out, "  {} {}", p.account, a).unwrap(),
                            None => writeln!(out, "  {}", p.account).unwrap(),
                        }
                    }
                }
            }
        }
        Ok(out)
    }
}

fn meta(out: &mut String, key: &str, value: &str) {
    writeln!(out, "  {key}: \"{value}\"").unwrap();
}
