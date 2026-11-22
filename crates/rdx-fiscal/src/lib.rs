//! Fiscal proof of expenses (roadmap piece 15): whether each reimbursed expense has the evidence
//! the organization's proof policy asks for.
//!
//! In Mexico, an organization with an RFC proves an expense with a **CFDI issued to its RFC**; a
//! supporter's statement line only shows that the payment happened. A group with no RFC proves it
//! with the receipt. Which applies is the [`Policy`] (`nickel/rdx/fiscal.ncl`, per fiscal year),
//! and [`check`] applies it to each [`Expense`]: CFDI type, total and date against the receipt,
//! the receptor against the organization's fiscal profile, cash and fuel rules, PPD payment
//! complements, the SAT status and art. 69-B result a host supplies, foreign vendors' invoices,
//! and a non-profit's purpose and class. See docs/compliance.md R0 to R6 and R17.
//!
//! Every figure in the policy is to be confirmed by a contador; every report says so until the
//! year's rules are marked confirmed.
//!
//! Like every core crate, this one does no I/O and builds for `wasm32-unknown-unknown` and
//! `wasm32-wasip2`.

mod cfdi;
mod check;
mod op;
mod policy;

pub use cfdi::{Cfdi, CfdiError, Concept, Issuer, Payment, Receiver};
pub use check::{
    Assessment, Basis, Class, Efos, Expense, Finding, ForeignInvoice, PUBLICO_EN_GENERAL, PaidWith,
    PolicySummary, Proof, Report, SatCheck, SatStatus, Severity, Summary, check,
};
pub use op::{CheckOp, register};
pub use policy::{CashRule, Kind, Policy, PolicyError, Profile, Rules, Window};
