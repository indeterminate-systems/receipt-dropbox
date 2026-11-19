use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    Currency, Date, Digest, LineRef, Match, MatchStatus, Money, Receipt, Statement, SupporterId,
};

/// A Beancount account name (ADR-0003): one of the five root types, then one or more components
/// that start with an uppercase letter or digit.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Account(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "invalid account `{0}`: start with Assets, Liabilities, Equity, Income or Expenses, then `:Component`s that start with an uppercase letter or digit"
)]
pub struct AccountError(String);

const ROOTS: [&str; 5] = ["Assets", "Liabilities", "Equity", "Income", "Expenses"];

impl Account {
    pub fn new(name: impl Into<String>) -> Result<Self, AccountError> {
        let name = name.into();
        let mut parts = name.split(':');
        let root_ok = parts.next().is_some_and(|r| ROOTS.contains(&r));
        let rest: Vec<&str> = parts.collect();
        let rest_ok = !rest.is_empty()
            && rest.iter().all(|c| {
                let mut b = c.bytes();
                matches!(b.next(), Some(b'A'..=b'Z' | b'0'..=b'9'))
                    && b.all(|x| x.is_ascii_alphanumeric() || x == b'-')
            });
        if root_ok && rest_ok {
            Ok(Account(name))
        } else {
            Err(AccountError(name))
        }
    }

    /// What the organization owes a supporter for costs they paid: `Liabilities:Reimbursable:S-…`.
    pub fn reimbursable(supporter: &SupporterId) -> Account {
        let component: String = supporter
            .as_str()
            .chars()
            .map(|c| match c {
                '.' | '_' => '-',
                c => c.to_ascii_uppercase(),
            })
            .collect();
        // Supporter ids start with a letter or digit and contain only slug characters, so the
        // mapped component is always valid.
        Account::new(format!("Liabilities:Reimbursable:{component}")).unwrap()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Account {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for Account {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Account({})", self.0)
    }
}

impl<'de> Deserialize<'de> for Account {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Account::new(String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Posting {
    pub account: Account,
    /// `None` lets Beancount infer the balancing amount (at most one per entry).
    pub amount: Option<Money>,
}

/// One ledger transaction, with the audit links every entry must carry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub date: Date,
    pub payee: String,
    pub narration: String,
    pub supporter: SupporterId,
    /// The statement line this entry books.
    pub line: LineRef,
    pub receipt: crate::ReceiptId,
    /// Digest of the receipt's original document.
    pub receipt_source: Digest,
    /// Digest of the encrypted original statement.
    pub statement: Digest,
    pub postings: Vec<Posting>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LedgerError {
    #[error("only confirmed matches are booked")]
    NotConfirmed,
    #[error("the match points at {0:?}, which is not a line of this statement")]
    NoSuchLine(LineRef),
    #[error("the match is for receipt `{matched}`, not `{given}`")]
    WrongReceipt { matched: String, given: String },
    #[error("postings in {currency} sum to {sum}, not zero")]
    Unbalanced { currency: Currency, sum: Money },
    #[error("more than one posting leaves its amount to be inferred")]
    TooManyInferred,
    #[error("an entry needs at least two postings")]
    TooFewPostings,
    #[error(transparent)]
    Money(#[from] crate::MoneyError),
}

impl LedgerEntry {
    /// Books a confirmed match: the expense (or, for a refund, its reversal) against what the
    /// organization owes the supporter.
    pub fn reimbursable(
        statement: &Statement,
        receipt: &Receipt,
        m: &Match,
        encrypted_original: Digest,
        expense: Account,
    ) -> Result<LedgerEntry, LedgerError> {
        if m.status != MatchStatus::Confirmed {
            return Err(LedgerError::NotConfirmed);
        }
        if m.receipt != receipt.id {
            return Err(LedgerError::WrongReceipt {
                matched: m.receipt.to_string(),
                given: receipt.id.to_string(),
            });
        }
        let line = (m.line.statement == statement.id)
            .then(|| statement.line(&m.line.line))
            .flatten()
            .ok_or_else(|| LedgerError::NoSuchLine(m.line.clone()))?;
        // A debit on the statement is an expense the org owes back; a credit (refund) reverses it.
        let expense_amount = if line.amount.is_negative() {
            m.amount
        } else {
            m.amount.checked_neg()?
        };
        let entry = LedgerEntry {
            date: line.date,
            payee: receipt.merchant.clone(),
            narration: line.description.clone(),
            supporter: statement.supporter.clone(),
            line: m.line.clone(),
            receipt: receipt.id.clone(),
            receipt_source: receipt.source.clone(),
            statement: encrypted_original,
            postings: vec![
                Posting {
                    account: expense,
                    amount: Some(expense_amount),
                },
                Posting {
                    account: Account::reimbursable(&statement.supporter),
                    amount: Some(expense_amount.checked_neg()?),
                },
            ],
        };
        entry.validate()?;
        Ok(entry)
    }

    /// Double entry: postings in each currency sum to zero, unless one posting is inferred.
    pub fn validate(&self) -> Result<(), LedgerError> {
        if self.postings.len() < 2 {
            return Err(LedgerError::TooFewPostings);
        }
        let inferred = self.postings.iter().filter(|p| p.amount.is_none()).count();
        if inferred > 1 {
            return Err(LedgerError::TooManyInferred);
        }
        if inferred == 1 {
            return Ok(());
        }
        let mut sums: BTreeMap<Currency, Money> = BTreeMap::new();
        for amount in self.postings.iter().filter_map(|p| p.amount) {
            let sum = sums
                .entry(amount.currency())
                .or_insert(Money::zero(amount.currency()));
            *sum = sum.checked_add(amount)?;
        }
        match sums.into_iter().find(|(_, s)| s.minor() != 0) {
            Some((currency, sum)) => Err(LedgerError::Unbalanced { currency, sum }),
            None => Ok(()),
        }
    }
}
