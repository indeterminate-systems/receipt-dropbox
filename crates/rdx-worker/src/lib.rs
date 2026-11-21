//! The Cloudflare host: an Email Routing Worker that runs email intake ([`rdx_intake`]) on every
//! email sent to the organization's receipts address, and sweeps expired quarantines on a cron
//! trigger.
//!
//! ```text
//!   Email Routing ─▶ email() ─▶ load kv keys ─▶ rdx_intake::receive (in memory) ─▶ reply?
//!                                                        │                   setReject?
//!                                                        ▼
//!                                         write R2 blobs, then KV, then delete blobs
//!   cron ─▶ scheduled() ─▶ rdx_intake::sweep ─▶ same writes ─▶ notices (if RDX_EMAIL is bound)
//! ```
//!
//! The intake core is synchronous and Workers storage is not, so each call runs against
//! [`staged`] stores: the kv keys the call can touch are read first, writes are collected, and
//! the Worker applies them after the core returns. The plaintext email exists only in this
//! invocation's memory; what reaches R2 is the encrypted envelope.
//!
//! Setup and deployment: [README.md](../README.md). Design: ADR-P13.

pub mod staged;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod worker;
