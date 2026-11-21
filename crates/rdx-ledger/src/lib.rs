//! The ledger: plain-text double-entry bookkeeping in Beancount, the organization's source of
//! truth ([ADR-0003](../../docs/adr/0003-beancount-ledger.md),
//! [ADR-P09](../../docs/adr/ADR-P09-beancount-ledger.md)).
//!
//! - [`book`] turns one statement's confirmed matches into [`LedgerEntry`]s: the expense (or a
//!   refund's reversal) against what the organization owes the supporter, each carrying its audit
//!   links (`receipt`, `receipt-source`, `statement` digest of the encrypted original, `line`).
//! - [`Ledger`] holds the booked entries and the statements they came from. [`Ledger::render`]
//!   writes canonical Beancount, with `open` directives and one balance assertion per statement;
//!   [`Ledger::parse`] reads that text back and refuses anything that isn't canonical, so an edit
//!   made outside rdx is caught instead of silently rewritten.
//! - [`export`] writes a ledger as CSV or XLSX, one row per posting.
//!
//! The crate does no I/O. Its ops ([`register`]) keep each version of a ledger in the host's blob
//! store, named by digest, with a kv entry `ledger/<name>` pointing at the current one.
//!
//! [`LedgerEntry`]: rdx_model::LedgerEntry

mod book;
pub mod export;
mod ledger;
mod op;
mod read;

pub use book::{Accounts, BookError, Booking, BookingSummary, Skipped, StatementRecord, book};
pub use ledger::{Balance, Ledger, PostError, PostReport};
pub use op::register;
pub use read::ReadError;
