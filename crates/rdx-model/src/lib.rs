//! The Receipt Dropbox domain model: money, statements, receipts, matches and ledger entries.
//!
//! Every type serializes to the JSON shape that `nickel/rdx/model.ncl` describes, so plans and
//! fixtures written in Nickel are checked against the same rules. The crate does no I/O and builds
//! for `wasm32-unknown-unknown` and `wasm32-wasip2`.
//!
//! ```
//! use rdx_model::{Currency, Money};
//!
//! let paper = Money::parse("42.17", Currency::USD).unwrap();
//! assert_eq!(paper.minor(), 4217);
//! assert_eq!(paper.to_string(), "42.17 USD");
//! ```

mod date;
mod ids;
mod ledger;
mod matching;
mod money;
mod receipt;
mod statement;
pub mod synth;

pub use date::{Date, DateError, Period};
pub use ids::{Digest, IdError, LineId, ReceiptId, StatementId, SupporterId};
pub use ledger::{Account, AccountError, LedgerEntry, LedgerError, Posting};
pub use matching::{LineRef, Match, MatchStatus};
pub use money::{Currency, Money, MoneyError};
pub use receipt::{Receipt, ReceiptItem, ReceiptKind, ReceiptProblem};
pub use statement::{
    AccountRef, LineClass, RedactionSummary, Statement, StatementLine, StatementProblem, Supporter,
    SupporterProblem,
};
