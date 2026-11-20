//! Host capabilities: everything the Receipt Dropbox core needs from the outside world.
//!
//! The core does no I/O. A host (the PWA, a Cloudflare Worker, the local `rdx` CLI) builds a
//! [`Host`] from the capabilities it can offer, and the plan executor hands it to ops. An op
//! declares the capabilities it needs, so a plan that needs one the host lacks fails validation
//! before anything runs.
//!
//! Each trait mirrors an interface in `wit/rdx.wit` with the same names (`cargo xtask check-wit`
//! keeps them in step), so a capability can be supplied by statically linked Rust or by a
//! WebAssembly component host. Clock and randomness come from WASI on component hosts, so they
//! have Rust traits but no WIT interface of their own.

use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[cfg(feature = "local")]
pub mod local;
pub mod memory;

/// A failure inside a capability, with a message an agent can act on.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct CapabilityError {
    pub message: String,
}

impl CapabilityError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

pub type Result<T, E = CapabilityError> = std::result::Result<T, E>;

/// Stores ciphertext by content address. WIT: `blob-store`.
pub trait BlobStore {
    /// Stores bytes and returns their `sha256:<hex>` digest, which is also their address.
    fn put(&mut self, bytes: &[u8]) -> Result<String>;
    fn get(&self, id: &str) -> Result<Option<Vec<u8>>>;
    /// Deleting a blob that isn't there is not an error.
    fn delete(&mut self, id: &str) -> Result<()>;
}

/// Small values by key: address-to-key registries, settings, quarantine records. WIT: `kv`.
pub trait Kv {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;
    fn put(&mut self, key: &str, value: &[u8]) -> Result<()>;
    /// Deleting a key that isn't there is not an error.
    fn delete(&mut self, key: &str) -> Result<()>;
}

/// An outgoing plain-text email. WIT: `mail-out.message`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub to: String,
    pub subject: String,
    pub text: String,
}

/// Sends mail. WIT: `mail-out`.
pub trait MailOut {
    fn send(&mut self, msg: &Message) -> Result<()>;
}

/// Wall-clock time. WASI `wasi:clocks/wall-clock` on component hosts.
pub trait Clock {
    /// Milliseconds since the Unix epoch.
    fn now_unix_ms(&self) -> Result<u64>;
}

/// Cryptographically secure randomness. WASI `wasi:random` on component hosts.
pub trait Random {
    fn fill(&mut self, buf: &mut [u8]) -> Result<()>;
}

/// Where run reports go: an append-only list of lines. WIT: `audit-log`.
///
/// Not a [`Capability`]: ops never write to it; the runner records every run.
pub trait AuditLog {
    fn append(&mut self, line: &str) -> Result<()>;
    /// The most recent line, which the next entry chains to.
    fn last(&self) -> Result<Option<String>>;
}

/// A capability an op can need. Names match `wit/rdx.wit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    BlobStore,
    Kv,
    MailOut,
    Clock,
    Random,
}

impl Capability {
    pub const ALL: [Capability; 5] = [
        Capability::BlobStore,
        Capability::Kv,
        Capability::MailOut,
        Capability::Clock,
        Capability::Random,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Capability::BlobStore => "blob-store",
            Capability::Kv => "kv",
            Capability::MailOut => "mail-out",
            Capability::Clock => "clock",
            Capability::Random => "random",
        }
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The capabilities one host offers. A missing one is `None`.
#[derive(Default)]
pub struct Host {
    pub blob_store: Option<Box<dyn BlobStore>>,
    pub kv: Option<Box<dyn Kv>>,
    pub mail_out: Option<Box<dyn MailOut>>,
    pub clock: Option<Box<dyn Clock>>,
    pub random: Option<Box<dyn Random>>,
    pub audit_log: Option<Box<dyn AuditLog>>,
}

impl Host {
    /// A host with no capabilities: only pure ops can run.
    pub fn new() -> Self {
        Self::default()
    }

    /// Every capability in memory, with a fixed clock and seeded randomness, so runs are
    /// reproducible. Golden tests use it.
    pub fn memory() -> Self {
        use memory::*;
        Self::new()
            .with_blob_store(MemoryBlobStore::default())
            .with_kv(MemoryKv::default())
            .with_mail_out(Outbox::default())
            .with_clock(FixedClock(FixedClock::GOLDEN))
            .with_random(SeededRandom::new(0))
            .with_audit_log(MemoryAuditLog::default())
    }

    pub fn with_blob_store(mut self, c: impl BlobStore + 'static) -> Self {
        self.blob_store = Some(Box::new(c));
        self
    }

    pub fn with_kv(mut self, c: impl Kv + 'static) -> Self {
        self.kv = Some(Box::new(c));
        self
    }

    pub fn with_mail_out(mut self, c: impl MailOut + 'static) -> Self {
        self.mail_out = Some(Box::new(c));
        self
    }

    pub fn with_clock(mut self, c: impl Clock + 'static) -> Self {
        self.clock = Some(Box::new(c));
        self
    }

    pub fn with_random(mut self, c: impl Random + 'static) -> Self {
        self.random = Some(Box::new(c));
        self
    }

    pub fn with_audit_log(mut self, c: impl AuditLog + 'static) -> Self {
        self.audit_log = Some(Box::new(c));
        self
    }

    pub fn offers(&self, capability: Capability) -> bool {
        match capability {
            Capability::BlobStore => self.blob_store.is_some(),
            Capability::Kv => self.kv.is_some(),
            Capability::MailOut => self.mail_out.is_some(),
            Capability::Clock => self.clock.is_some(),
            Capability::Random => self.random.is_some(),
        }
    }

    pub fn offered(&self) -> Vec<Capability> {
        Capability::ALL
            .into_iter()
            .filter(|c| self.offers(*c))
            .collect()
    }
}

/// `sha256:<hex>` of raw bytes: the address of a blob.
pub fn sha256_digest(bytes: &[u8]) -> String {
    let hash = Sha256::digest(bytes);
    let mut out = String::with_capacity(7 + 64);
    out.push_str("sha256:");
    for b in hash {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Whether `id` is a well-formed `sha256:<64 lowercase hex>` digest.
pub fn is_sha256_digest(id: &str) -> bool {
    id.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    })
}

/// The interfaces and functions in `wit/rdx.wit` that the traits above mirror.
/// `cargo xtask check-wit` fails when the two drift apart; change both together.
pub const WIT_CAPABILITIES: &[(&str, &[&str])] = &[
    ("blob-store", &["put", "get", "delete"]),
    ("kv", &["get", "put", "delete"]),
    ("mail-out", &["send"]),
    ("audit-log", &["append", "last"]),
];
