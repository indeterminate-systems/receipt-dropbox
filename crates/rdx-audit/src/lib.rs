//! The audit trail: custody records of encrypted originals, grants, and the links from every
//! ledger entry and viewable row to its receipt and encrypted original.
//!
//! ```text
//!   ledger entry ── statement: sha256:D ──▶ custody record audit/custody/<statement>
//!        │                                    kept:    root, sealed blob, original D, source blob
//!        └─ receipt, receipt-source           granted: grantee, keys, copies   revoked: …
//!   viewable row ── root ───────────────────▶ (same root)
//! ```
//!
//! The signed statement names `D`, the digest of the bank's document, and `audit.keep` stores that
//! document encrypted (`source`) and deletes the plaintext. A grant adds the grantee's keys to
//! copies of the sealed statement and the encrypted original, so the supporter's own blobs and
//! every link to them never change. Custody events are signed by the supporter's device, so a
//! server can hold them but not forge a grant. See
//! [ADR-P12](../../docs/adr/ADR-P12-audit-trail-and-grants.md).
//!
//! The crate does no I/O and builds for `wasm32-unknown-unknown` and `wasm32-wasip2`.

pub mod custody;
pub mod ops;
pub mod original;
mod trail;

pub use ops::register;
