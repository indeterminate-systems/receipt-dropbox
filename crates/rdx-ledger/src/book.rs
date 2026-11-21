use std::collections::{BTreeMap, BTreeSet};

use rdx_model::{
    Account, Currency, Date, Digest, LedgerEntry, LedgerError, LineRef, Match, MatchStatus, Money,
    MoneyError, Receipt, ReceiptId, Statement, StatementId, StatementProblem, SupporterId,
};
use serde::{Deserialize, Serialize};

/// Which expense account a receipt books to, from `nickel/rdx/ledger.ncl` (which holds the
/// defaults; this type has none, so Rust and Nickel can't disagree).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Accounts {
    /// Used when neither `by_receipt` nor `by_merchant` names the receipt.
    pub fallback: Account,
    /// A person's classification of one receipt; wins over everything else.
    #[serde(default)]
    pub by_receipt: BTreeMap<ReceiptId, Account>,
    /// Merchant names (compared case-insensitively, ignoring surrounding spaces).
    #[serde(default)]
    pub by_merchant: BTreeMap<String, Account>,
}

impl Accounts {
    /// The account for a receipt, and whether it fell through to the default.
    fn for_receipt(&self, r: &Receipt) -> (Account, bool) {
        if let Some(a) = self.by_receipt.get(&r.id) {
            return (a.clone(), false);
        }
        let merchant = r.merchant.trim().to_lowercase();
        match self
            .by_merchant
            .iter()
            .find(|(m, _)| m.trim().to_lowercase() == merchant)
        {
            Some((_, a)) => (a.clone(), false),
            None => (self.fallback.clone(), true),
        }
    }
}

/// What the ledger keeps about a statement it has booked from: enough for the statement's balance
/// assertion and its audit link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatementRecord {
    pub id: StatementId,
    pub supporter: SupporterId,
    pub currency: Currency,
    /// Last day of the statement period; the balance assertion is dated the day after.
    pub period_end: Date,
    /// Digest of the encrypted original statement.
    pub original: Digest,
}

/// A confirmed-match candidate that was not booked, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Skipped {
    pub line: LineRef,
    pub receipt: ReceiptId,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BookingSummary {
    pub entries: usize,
    /// Sum of the expense postings (refunds negative), in the statement's currency: what the
    /// organization owes the supporter for this statement.
    pub reimbursable: Money,
    /// Entries that fell through to `accounts.fallback`.
    pub uncategorized: usize,
}

/// One statement's confirmed matches, ready for `ledger.post`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Booking {
    pub statement: StatementRecord,
    pub entries: Vec<LedgerEntry>,
    #[serde(default)]
    pub skipped: Vec<Skipped>,
    #[serde(default)]
    pub notes: Vec<String>,
    pub summary: BookingSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BookError {
    #[error("statement `{statement}` is not consistent: {}", join(problems))]
    Statement {
        statement: StatementId,
        problems: Vec<StatementProblem>,
    },
    #[error(
        "statement `{0}` has no digest of its encrypted original (`original`); store the original \
         first, or pass `original`, so every entry links to it"
    )]
    NoOriginal(StatementId),
    #[error(
        "statement `{statement}` names original `{on_statement}` but `original` is `{given}`; pass \
         only one, or the same digest"
    )]
    OriginalMismatch {
        statement: StatementId,
        on_statement: Digest,
        given: Digest,
    },
    #[error("a confirmed match names receipt `{0}`, which is not among `receipts`")]
    UnknownReceipt(ReceiptId),
    #[error(
        "receipt `{receipt}` belongs to supporter `{receipt_supporter}`, not the statement's \
         `{statement_supporter}`"
    )]
    OtherSupporter {
        receipt: ReceiptId,
        receipt_supporter: SupporterId,
        statement_supporter: SupporterId,
    },
    #[error("line `{line}` and receipt `{receipt}` are confirmed more than once")]
    Duplicate { line: String, receipt: ReceiptId },
    #[error(
        "confirmed matches on line `{line}` add up to {booked}, more than the line's {line_amount}"
    )]
    OverBooked {
        line: String,
        booked: Money,
        line_amount: Money,
    },
    #[error(
        "line `{line}`, receipt `{receipt}`: the match amount {amount} must be positive and in \
         the statement's currency"
    )]
    BadAmount {
        line: String,
        receipt: ReceiptId,
        amount: Money,
    },
    #[error("line `{line}`, receipt `{receipt}`: {error}")]
    Entry {
        line: String,
        receipt: ReceiptId,
        error: LedgerError,
    },
    #[error(transparent)]
    Money(#[from] MoneyError),
}

fn join(problems: &[StatementProblem]) -> String {
    problems
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join("; ")
}

/// Books one statement's confirmed matches. Proposed and rejected matches, and matches for other
/// statements, are listed in `skipped` or `notes` and never booked.
pub fn book(
    statement: &Statement,
    receipts: &[Receipt],
    matches: &[Match],
    accounts: &Accounts,
    original: Option<&Digest>,
) -> Result<Booking, BookError> {
    statement
        .validate()
        .map_err(|problems| BookError::Statement {
            statement: statement.id.clone(),
            problems,
        })?;
    let original = match (&statement.original, original) {
        (Some(a), Some(b)) if a != b => {
            return Err(BookError::OriginalMismatch {
                statement: statement.id.clone(),
                on_statement: a.clone(),
                given: b.clone(),
            });
        }
        (Some(d), _) | (None, Some(d)) => d.clone(),
        (None, None) => return Err(BookError::NoOriginal(statement.id.clone())),
    };
    let by_id: BTreeMap<&ReceiptId, &Receipt> = receipts.iter().map(|r| (&r.id, r)).collect();

    let mut skipped = Vec::new();
    let mut notes = Vec::new();
    let mut other_statements = 0;
    let mut seen = BTreeSet::new();
    let mut per_line: BTreeMap<&rdx_model::LineId, i64> = BTreeMap::new();
    let mut entries = Vec::new();
    let mut uncategorized = 0;

    for m in matches {
        if m.line.statement != statement.id {
            other_statements += 1;
            continue;
        }
        if m.status != MatchStatus::Confirmed {
            skipped.push(Skipped {
                line: m.line.clone(),
                receipt: m.receipt.clone(),
                reason: format!(
                    "{} matches are not booked; confirm it first",
                    match m.status {
                        MatchStatus::Proposed => "proposed",
                        _ => "rejected",
                    }
                ),
            });
            continue;
        }
        let line_name = format!("{}/{}", m.line.statement, m.line.line);
        if !seen.insert((&m.line.line, &m.receipt)) {
            return Err(BookError::Duplicate {
                line: line_name,
                receipt: m.receipt.clone(),
            });
        }
        if m.amount.currency() != statement.currency || m.amount.minor() <= 0 {
            return Err(BookError::BadAmount {
                line: line_name,
                receipt: m.receipt.clone(),
                amount: m.amount,
            });
        }
        let receipt = by_id
            .get(&m.receipt)
            .ok_or_else(|| BookError::UnknownReceipt(m.receipt.clone()))?;
        if receipt.supporter != statement.supporter {
            return Err(BookError::OtherSupporter {
                receipt: receipt.id.clone(),
                receipt_supporter: receipt.supporter.clone(),
                statement_supporter: statement.supporter.clone(),
            });
        }
        let (account, fell_through) = accounts.for_receipt(receipt);
        if fell_through {
            uncategorized += 1;
        }
        let mut entry = LedgerEntry::reimbursable(statement, receipt, m, original.clone(), account)
            .map_err(|error| BookError::Entry {
                line: line_name.clone(),
                receipt: m.receipt.clone(),
                error,
            })?;
        entry.payee = clean_text(&entry.payee);
        entry.narration = clean_text(&entry.narration);

        // LedgerEntry::reimbursable checked the line exists and the currency matches.
        let line = statement
            .line(&m.line.line)
            .expect("checked by reimbursable");
        let booked = per_line.entry(&m.line.line).or_insert(0);
        *booked += m.amount.minor();
        if *booked > line.amount.minor().abs() {
            return Err(BookError::OverBooked {
                line: line_name,
                booked: Money::new(*booked, line.amount.currency()),
                line_amount: line.amount.abs(),
            });
        }
        entries.push(entry);
    }

    if other_statements > 0 {
        notes.push(format!(
            "{other_statements} match(es) for other statements were not considered"
        ));
    }
    if uncategorized > 0 {
        notes.push(format!(
            "{uncategorized} entr(y/ies) booked to {} because no account was given for the \
             receipt or merchant; add them to `accounts.by_receipt` or `accounts.by_merchant`",
            accounts.fallback
        ));
    }
    entries.sort_by(|a, b| (a.date, &a.line, &a.receipt).cmp(&(b.date, &b.line, &b.receipt)));
    let reimbursable = Money::sum(
        statement.currency,
        entries
            .iter()
            .filter_map(|e| e.postings.first().and_then(|p| p.amount.as_ref())),
    )?;

    Ok(Booking {
        statement: StatementRecord {
            id: statement.id.clone(),
            supporter: statement.supporter.clone(),
            currency: statement.currency,
            period_end: statement.period.end(),
            original,
        },
        summary: BookingSummary {
            entries: entries.len(),
            reimbursable,
            uncategorized,
        },
        entries,
        skipped,
        notes,
    })
}

/// Payee and narration become Beancount strings. Quotes and backslashes are replaced rather than
/// escaped (not every Beancount reader handles escapes) and control characters become spaces, so
/// what is booked is exactly what reads back.
pub(crate) fn clean_text(s: &str) -> String {
    let mapped: String = s
        .chars()
        .map(|c| match c {
            '"' => '\'',
            '\\' => '/',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    mapped.split_whitespace().collect::<Vec<_>>().join(" ")
}
