use serde::{Deserialize, Serialize};

use crate::{LineId, Money, ReceiptId, StatementId};

/// A statement line, addressed globally.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct LineRef {
    pub statement: StatementId,
    pub line: LineId,
}

/// A link between a statement line and a receipt that explains (part of) it.
///
/// A split payment is several matches on one line; one receipt paid in instalments is several
/// matches with the same receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Match {
    pub line: LineRef,
    pub receipt: ReceiptId,
    /// How much of the line this receipt explains. Positive, whatever the line's sign.
    pub amount: Money,
    pub status: MatchStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchStatus {
    /// Suggested by the matching engine; not yet shown or booked.
    Proposed,
    /// Accepted by the supporter or a reviewer: the line is shown and booked.
    Confirmed,
    /// Rejected, kept so the engine doesn't suggest it again.
    Rejected,
}
