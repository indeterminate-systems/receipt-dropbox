use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{
    Currency, Date, Digest, LineId, Match, MatchStatus, Money, MoneyError, Period, StatementId,
    SupporterId,
};

/// Someone who pays org costs from a personal account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Supporter {
    pub id: SupporterId,
    /// Shown to the people who reconcile; never written to the ledger (which uses `id`).
    pub display_name: String,
}

/// Which account a statement is for, without its number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountRef {
    pub institution: String,
    /// The last few characters of the account number, as banks print them.
    pub mask: String,
}

/// One account's statement for one period, as parsed from the original document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Statement {
    pub id: StatementId,
    pub supporter: SupporterId,
    pub account: AccountRef,
    pub currency: Currency,
    pub period: Period,
    pub opening: Money,
    pub closing: Money,
    pub lines: Vec<StatementLine>,
    /// Digest of the encrypted original, once it is stored (roadmap piece 7). Ledger entries and
    /// viewable rows link to it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original: Option<Digest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatementLine {
    pub id: LineId,
    /// The posting date the bank prints.
    pub date: Date,
    pub description: String,
    /// Negative for money leaving the account (a debit), positive for a credit.
    pub amount: Money,
    /// A reference the bank prints, such as a bill or invoice number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
}

/// Why a statement doesn't hold together. A parser that produces one of these has misread the
/// document, or the document is incomplete.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StatementProblem {
    #[error("line id `{0}` appears more than once")]
    DuplicateLine(LineId),
    #[error("line `{0}` is dated outside the statement period")]
    OutsidePeriod(LineId),
    #[error("line `{0}` is not in the statement's currency")]
    Currency(LineId),
    #[error("opening or closing balance is not in the statement's currency")]
    BalanceCurrency,
    #[error("does not balance: opening plus lines is {actual}, but closing is {expected}")]
    Unbalanced { expected: Money, actual: Money },
    #[error(transparent)]
    Money(#[from] MoneyError),
}

/// How a statement line appears in the viewable statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineClass {
    /// Confirmed matches cover the whole amount: shown.
    Org,
    /// Confirmed matches cover part of it: redacted, and flagged for review.
    PartlyMatched,
    /// No confirmed match: redacted as "Personal, redacted".
    Personal,
}

/// The totals a redacted statement shows so that it still reconciles:
/// `opening + shown + redacted = closing`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedactionSummary {
    pub opening: Money,
    pub closing: Money,
    pub shown: Money,
    pub redacted: Money,
}

impl Statement {
    /// Checks everything a statement must satisfy on its own and reports every problem found.
    pub fn validate(&self) -> Result<(), Vec<StatementProblem>> {
        let mut problems = Vec::new();
        let mut seen = BTreeSet::new();
        let mut total = Ok(self.opening);
        for line in &self.lines {
            if !seen.insert(&line.id) {
                problems.push(StatementProblem::DuplicateLine(line.id.clone()));
            }
            if !self.period.contains(line.date) {
                problems.push(StatementProblem::OutsidePeriod(line.id.clone()));
            }
            if line.amount.currency() != self.currency {
                problems.push(StatementProblem::Currency(line.id.clone()));
            } else {
                total = total.and_then(|t| t.checked_add(line.amount));
            }
        }
        if self.opening.currency() != self.currency || self.closing.currency() != self.currency {
            problems.push(StatementProblem::BalanceCurrency);
        } else {
            match total {
                Ok(actual) if actual != self.closing => {
                    problems.push(StatementProblem::Unbalanced {
                        expected: self.closing,
                        actual,
                    })
                }
                Ok(_) => {}
                Err(e) => problems.push(e.into()),
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems)
        }
    }

    /// Classifies each line (in order) by the confirmed matches that point at it. Matches for
    /// other statements, and matches that are not confirmed, are ignored.
    pub fn classify(&self, matches: &[Match]) -> Vec<LineClass> {
        self.lines
            .iter()
            .map(|line| {
                let covered: i64 = matches
                    .iter()
                    .filter(|m| {
                        m.status == MatchStatus::Confirmed
                            && m.line.statement == self.id
                            && m.line.line == line.id
                            && m.amount.currency() == line.amount.currency()
                    })
                    .map(|m| m.amount.minor())
                    .sum();
                if covered == 0 {
                    LineClass::Personal
                } else if covered == line.amount.minor().abs() {
                    LineClass::Org
                } else {
                    LineClass::PartlyMatched
                }
            })
            .collect()
    }

    /// Totals for the viewable statement: what is shown (org lines) and the single redacted figure.
    pub fn redaction_summary(&self, matches: &[Match]) -> Result<RedactionSummary, MoneyError> {
        let mut shown = Money::zero(self.currency);
        let mut redacted = Money::zero(self.currency);
        for (line, class) in self.lines.iter().zip(self.classify(matches)) {
            match class {
                LineClass::Org => shown = shown.checked_add(line.amount)?,
                _ => redacted = redacted.checked_add(line.amount)?,
            }
        }
        Ok(RedactionSummary {
            opening: self.opening,
            closing: self.closing,
            shown,
            redacted,
        })
    }

    pub fn line(&self, id: &LineId) -> Option<&StatementLine> {
        self.lines.iter().find(|l| &l.id == id)
    }
}
