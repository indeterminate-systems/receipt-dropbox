//! Matching: pairs a statement's lines with the receipts and bills that explain them.
//!
//! [`reconcile`] proposes matches under a [`Policy`] whose tolerances (amount, date window per
//! document kind, merchant similarity, exchange rates) come from `nickel/rdx/matching.ncl`, so
//! organizations tune them without code changes. It never confirms anything: every match is
//! `proposed`, and anything short of an unambiguous exact match is flagged for review with the
//! reasons. In order, it looks for:
//!
//! 1. one line, one receipt, amounts within tolerance (strongest evidence first: a printed
//!    reference, then amount, date and merchant);
//! 2. splits: one line that paid several receipts;
//! 3. instalments: one receipt paid over several lines;
//! 4. partials: a line that paid part of a receipt.
//!
//! The crate is pure: no I/O, no clock, no floating point, so every host gets the same answer.
//! `matching.propose` ([`register`]) exposes it to plans.

mod engine;
mod merchant;
mod op;
mod policy;

pub use engine::{
    Evidence, FxApplied, MatchError, Proposal, ProposalKind, Reconciliation, Summary, reconcile,
};
pub use merchant::similarity;
pub use op::register;
pub use policy::{
    AmountPolicy, DatePolicy, FxPolicy, FxRate, MerchantPolicy, PartialPolicy, Policy, PolicyError,
    SplitPolicy, Window,
};
