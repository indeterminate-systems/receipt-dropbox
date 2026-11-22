//! Email intake: decides what happens to each email sent to the organization's receipts
//! address, encrypts what it keeps the moment it arrives, and runs the quarantine that holds
//! mail from senders who haven't set up keys yet.
//!
//! ```text
//!   email ─▶ size, Authentication-Results (DMARC), one From, allowlist ─▶ rejected (bounce)
//!     │
//!     ├─ From has keys (keys/registry/<address>) ─▶ encrypt to its devices + recovery key
//!     │                                             ─▶ blob, listed in intake/inbox/<address>
//!     │
//!     └─ no keys yet ─▶ encrypt to a quarantine key for the address ─▶ blob, listed in
//!                       intake/quarantine/<address>; the first time, reply once with a claim
//!                       link that holds the quarantine key (the server keeps only its public
//!                       half). Claimed (`release`, or over HTTP: `claim`) ─▶
//!                       re-encrypted to the new keys.
//!                       Expired (`sweep`) ─▶ deleted, sender told.
//! ```
//!
//! An email that is only a Wise tracking link, with no receipt, is kept the same way, and its
//! sender is asked (once per reply interval) to send the transfer's PDF receipt instead. Intake
//! never opens the link ([`wise`]).
//!
//! The whole message is encrypted as one document (`rdx.intake.email.v1`), so the receipt
//! extractor later sees exactly what was sent: body, attachments, forwarded headers. Plaintext
//! exists only in the caller's memory during [`receive`].
//!
//! The crate does no I/O and builds for both WebAssembly targets. Hosts give it `kv`, a blob
//! store, a clock and randomness; the Cloudflare Worker (`rdx-worker`) stages them in memory
//! around each call because Workers storage is asynchronous ([`receive_keys`], [`sweep_keys`]).
//! See [ADR-P13](../../docs/adr/ADR-P13-email-intake.md).

mod auth;
pub mod claim;
mod headers;
mod mail;
pub mod ops;
mod wise;

use bc_envelope::prelude::*;
use rdx_host::{CapabilityError, Host, Message};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

pub use mail::{Ask, Reply, ReplyKind, ReplyStatus, claim_link, mime, parse_claim_link};
pub use ops::register;

/// The `'isA'` of an encrypted email.
pub const EMAIL_TYPE: &str = "rdx.intake.email.v1";
/// The predicate holding when the email arrived (Unix milliseconds).
pub const RECEIVED_AT: &str = "receivedAt";

const DAY_MS: u64 = 24 * 60 * 60 * 1000;

/// Who may send and how long unverified mail waits. Its contract and defaults are
/// `nickel/rdx/intake.ncl`; this struct has no defaults of its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// The address supporters write to; replies come from it.
    pub intake_address: String,
    /// The PWA page that claims a quarantine; the link adds `#address=…&key=…`.
    pub claim_url: String,
    pub allow: Allow,
    /// The server whose `Authentication-Results` count (Cloudflare: `mx.cloudflare.net`).
    pub authserv_id: String,
    pub quarantine_days: u32,
    /// The least time between two verification replies to one address.
    pub reply_interval_days: u32,
    pub max_bytes: u64,
}

/// Senders the organization accepts: whole domains and single addresses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Allow {
    pub addresses: Vec<String>,
    pub domains: Vec<String>,
}

impl Allow {
    pub fn allows(&self, address: &str) -> bool {
        let domain = address.rsplit_once('@').map_or("", |(_, d)| d);
        self.addresses
            .iter()
            .any(|a| a.eq_ignore_ascii_case(address))
            || self.domains.iter().any(|d| d.eq_ignore_ascii_case(domain))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Capability(#[from] CapabilityError),
    #[error("{0}")]
    Keys(String),
    #[error("the intake record `{key}` is unreadable: {message}")]
    Record { key: String, message: String },
    #[error("not an intake email: {0}")]
    NotAnEmail(String),
    #[error("this claim link doesn't work: {0}")]
    ClaimLink(String),
    #[error(
        "this claim link isn't for `{0}`'s current quarantine: it expired, was claimed already, \
         or is from an older email"
    )]
    WrongClaim(String),
    #[error("`{0}` has nothing in quarantine: it expired or was claimed already")]
    NothingQuarantined(String),
    #[error("the host has no `{0}` capability")]
    Missing(&'static str),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// What became of one email. Holds no secrets: reports and logs can show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum Outcome {
    /// Encrypted to the sender's keys.
    Delivered {
        address: String,
        document: String,
        bytes: u64,
    },
    /// Encrypted to the address's quarantine key until it is claimed or expires.
    Quarantined {
        address: String,
        document: String,
        bytes: u64,
        expires_at: u64,
    },
    /// Not kept. The host bounces it with `reason`, which the sender reads.
    Rejected {
        reason: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        address: Option<String>,
    },
}

/// [`receive`]'s result: the outcome, and the reply the host must send, if any.
#[derive(Debug)]
pub struct Received {
    pub outcome: Outcome,
    pub reply: Option<Reply>,
    /// What `reply` asks the sender to send, if anything. Holds no secrets.
    pub asked: Option<Ask>,
}

/// One stored document, as listed in an inbox or a quarantine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// The blob id (digest) of the encrypted envelope.
    pub id: String,
    pub received_at: u64,
    /// Size of the message as received.
    pub bytes: u64,
}

/// `intake/quarantine/<address>`: one address's quarantine. Only the key's public half is kept.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Quarantine {
    /// `ur:crypto-pubkeys/…`
    keys: String,
    opened_at: u64,
    expires_at: u64,
    documents: Vec<Entry>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Inbox {
    documents: Vec<Entry>,
}

pub fn inbox_key(address: &str) -> String {
    format!("intake/inbox/{address}")
}

pub fn quarantine_key(address: &str) -> String {
    format!("intake/quarantine/{address}")
}

/// Every address with a quarantine, so a sweep can find them without listing keys.
pub const QUARANTINE_INDEX: &str = "intake/quarantine";

/// When the last verification reply went to an address (Unix ms).
pub fn replied_key(address: &str) -> String {
    format!("intake/replied/{address}")
}

fn registry_key(address: &str) -> String {
    format!("keys/registry/{address}")
}

/// When an address was last asked to send something instead of what it sent (Unix ms).
pub fn asked_key(address: &str) -> String {
    format!("intake/asked/{address}")
}

/// The sender [`receive`] would act for, if the message names exactly one, so a host can load
/// [`receive_keys`] before calling it.
pub fn sender(raw: &[u8]) -> Option<String> {
    headers::from_address(&headers::parse(raw)).ok()
}

/// Every kv key [`receive`] may read or write for `address`.
pub fn receive_keys(address: &str) -> Vec<String> {
    vec![
        registry_key(address),
        inbox_key(address),
        quarantine_key(address),
        replied_key(address),
        asked_key(address),
        QUARANTINE_INDEX.to_owned(),
    ]
}

/// Every kv key [`sweep`] or [`status`] may read or write, given the value of
/// [`QUARANTINE_INDEX`].
pub fn sweep_keys(index: Option<&[u8]>) -> Result<Vec<String>> {
    let mut keys = vec![QUARANTINE_INDEX.to_owned()];
    if let Some(bytes) = index {
        let addresses: Vec<String> = decode(QUARANTINE_INDEX, bytes)?;
        keys.extend(addresses.iter().map(|a| quarantine_key(a)));
    }
    Ok(keys)
}

/// Decides what to do with one email and does it. The caller sends `reply`, if any, and bounces
/// a `Rejected` outcome with its reason. Needs `kv`, blob store, clock and random.
pub fn receive(host: &mut Host, policy: &Policy, raw: &[u8]) -> Result<Received> {
    let reject = |reason: String, address: Option<String>| Received {
        outcome: Outcome::Rejected { reason, address },
        reply: None,
        asked: None,
    };
    if raw.len() as u64 > policy.max_bytes {
        return Ok(reject(
            format!(
                "this email is larger than {} MB; send large statements as separate emails or \
                 upload them in the app",
                policy.max_bytes / 1_000_000
            ),
            None,
        ));
    }
    let fields = headers::parse(raw);
    let address = match headers::from_address(&fields) {
        Ok(a) => a,
        Err(headers::FromError::Several) => {
            return Ok(reject(
                "an email here must have exactly one sender in From".into(),
                None,
            ));
        }
        Err(e) => {
            return Ok(reject(
                format!("this email's From can't be read ({e:?})"),
                None,
            ));
        }
    };
    if let Err(why) = authenticated(policy, &fields, &address) {
        return Ok(reject(why, Some(address)));
    }
    if !policy.allow.allows(&address) {
        return Ok(reject(
            format!(
                "{address} is not on this organization's list of senders; ask the organization \
                 to add it"
            ),
            Some(address),
        ));
    }

    let now = now(host)?;
    if let Some(registry) =
        rdx_keys::ops::load_registry(host, &address).map_err(|e| Error::Keys(e.message))?
    {
        let state = registry
            .state()
            .map_err(|e| Error::Keys(format!("`{address}`'s key registry: {e}")))?;
        let entry = store(host, raw, now, &state.recipients())?;
        let mut inbox: Inbox = load(host, &inbox_key(&address))?.unwrap_or_default();
        inbox.documents.push(entry.clone());
        save(host, &inbox_key(&address), &inbox)?;
        let (reply, asked) = ask(host, policy, &fields, &address, raw, now)?;
        return Ok(Received {
            outcome: Outcome::Delivered {
                address,
                document: entry.id,
                bytes: entry.bytes,
            },
            reply,
            asked,
        });
    }

    // No keys yet: quarantine.
    let key = quarantine_key(&address);
    let mut current: Option<Quarantine> = load(host, &key)?;
    if current.as_ref().is_some_and(|q| q.expires_at <= now) {
        // Expired but not swept yet: its documents go now, and this email starts afresh.
        expire(host, &address)?;
        current = None;
    }
    if let Some(mut q) = current {
        let keys = rdx_keys::public_keys_from_ur(&q.keys).map_err(|e| Error::Record {
            key: key.clone(),
            message: e.to_string(),
        })?;
        let entry = store(host, raw, now, &[keys])?;
        q.documents.push(entry.clone());
        save(host, &key, &q)?;
        let (reply, asked) = ask(host, policy, &fields, &address, raw, now)?;
        return Ok(Received {
            outcome: Outcome::Quarantined {
                address,
                document: entry.id,
                bytes: entry.bytes,
                expires_at: q.expires_at,
            },
            reply,
            asked,
        });
    }

    let interval = u64::from(policy.reply_interval_days) * DAY_MS;
    if let Some(last) = load::<u64>(host, &replied_key(&address))?
        && now < last.saturating_add(interval)
    {
        return Ok(reject(
            format!(
                "{address} isn't set up to send receipts yet, and a setup link was already sent \
                 to it on {}; try again after {}",
                mail::date(last),
                mail::date(last + interval)
            ),
            Some(address),
        ));
    }
    // RFC 3834: never answer an automatic message, and without an answer nobody could claim it.
    if automatic(&fields) {
        return Ok(reject(
            format!("{address} isn't set up yet, and automatic emails can't set it up"),
            Some(address),
        ));
    }

    let mut secret = [0u8; 32];
    random(host)?.fill(&mut secret)?;
    let qkeys = rdx_keys::Keys::from_secret(&secret);
    let entry = store(host, raw, now, &[qkeys.public()])?;
    let expires_at = now + u64::from(policy.quarantine_days) * DAY_MS;
    save(
        host,
        &key,
        &Quarantine {
            keys: rdx_keys::public_keys_to_ur(&qkeys.public()),
            opened_at: now,
            expires_at,
            documents: vec![entry.clone()],
        },
    )?;
    let mut index: Vec<String> = load(host, QUARANTINE_INDEX)?.unwrap_or_default();
    if !index.contains(&address) {
        index.push(address.clone());
        save(host, QUARANTINE_INDEX, &index)?;
    }
    save(host, &replied_key(&address), &now)?;
    // Email Routing allows one reply per email, so the setup reply carries any ask too.
    let asked = wise::link_without_receipt(raw).then_some(Ask::WiseReceipt);
    if asked.is_some() {
        save(host, &asked_key(&address), &now)?;
    }
    let in_reply_to = headers::first(&fields, "message-id").map(str::to_owned);
    let reply = mail::verification(policy, &address, &secret, expires_at, in_reply_to, asked);
    secret.zeroize();
    Ok(Received {
        outcome: Outcome::Quarantined {
            address,
            document: entry.id,
            bytes: entry.bytes,
            expires_at,
        },
        reply: Some(reply),
        asked,
    })
}

/// RFC 3834: never answer an automatic message.
fn automatic(fields: &[(String, String)]) -> bool {
    headers::first(fields, "auto-submitted").is_some_and(|v| !v.eq_ignore_ascii_case("no"))
}

/// For an email that was kept: a reply asking for what it lacks, at most once per reply
/// interval, so intake can't be used to flood a mailbox.
fn ask(
    host: &mut Host,
    policy: &Policy,
    fields: &[(String, String)],
    address: &str,
    raw: &[u8],
    now: u64,
) -> Result<(Option<Reply>, Option<Ask>)> {
    if automatic(fields) || !wise::link_without_receipt(raw) {
        return Ok((None, None));
    }
    let interval = u64::from(policy.reply_interval_days) * DAY_MS;
    if load::<u64>(host, &asked_key(address))?
        .is_some_and(|last| now < last.saturating_add(interval))
    {
        return Ok((None, None));
    }
    save(host, &asked_key(address), &now)?;
    let in_reply_to = headers::first(fields, "message-id").map(str::to_owned);
    let ask = Ask::WiseReceipt;
    Ok((
        Some(mail::ask(policy, address, ask, in_reply_to)),
        Some(ask),
    ))
}

/// DMARC must pass, as judged by the configured server in the topmost results field, for the
/// domain in `From`.
fn authenticated(
    policy: &Policy,
    fields: &[(String, String)],
    address: &str,
) -> Result<(), String> {
    let Some(value) = headers::first(fields, "authentication-results") else {
        return Err(format!(
            "this email has no authentication results from {}; DMARC can't be checked",
            policy.authserv_id
        ));
    };
    let results = auth::parse(value);
    if !results
        .authserv_id
        .eq_ignore_ascii_case(&policy.authserv_id)
    {
        return Err(format!(
            "this email's authentication results aren't from {}; DMARC can't be checked",
            policy.authserv_id
        ));
    }
    let domain = address.rsplit_once('@').map_or("", |(_, d)| d);
    match results.dmarc() {
        Some(("pass", None)) => Ok(()),
        Some(("pass", Some(d))) if d.eq_ignore_ascii_case(domain) => Ok(()),
        Some(("pass", Some(d))) => Err(format!("DMARC passed for {d}, not for {domain} in From")),
        Some((result, _)) => Err(format!(
            "this email failed the DMARC check for {domain} (dmarc={result}); send it from \
             your own mail account, not through a forwarding service"
        )),
        None => Err(format!(
            "no DMARC result for {domain}; it can't be authenticated"
        )),
    }
}

/// Encrypts the email to `recipients` and stores it. The envelope's subject is the message as
/// received; nothing else of it is kept.
fn store(
    host: &mut Host,
    raw: &[u8],
    now: u64,
    recipients: &[bc_components::PublicKeys],
) -> Result<Entry> {
    let mut rng = rdx_keys::host_rng(random(host)?)?;
    let envelope = Envelope::new(ByteString::from(raw))
        .add_assertion(known_values::IS_A, EMAIL_TYPE)
        .add_assertion(RECEIVED_AT, now);
    let sealed = rdx_keys::encrypt(&envelope, recipients, &mut rng)
        .map_err(|e| Error::Keys(e.to_string()))?;
    let id = blobs(host)?.put(rdx_keys::envelope_to_ur(&sealed).as_bytes())?;
    Ok(Entry {
        id,
        received_at: now,
        bytes: raw.len() as u64,
    })
}

/// The message inside a decrypted intake envelope.
pub fn message_bytes(opened: &Envelope) -> Result<Vec<u8>> {
    let kind: String = opened
        .extract_object_for_predicate(known_values::IS_A)
        .map_err(|e| Error::NotAnEmail(e.to_string()))?;
    if kind != EMAIL_TYPE {
        return Err(Error::NotAnEmail(format!("it is a `{kind}`")));
    }
    let bytes: ByteString = opened
        .extract_subject()
        .map_err(|e| Error::NotAnEmail(e.to_string()))?;
    Ok(bytes.data().to_vec())
}

/// The documents delivered to an address, oldest first.
pub fn inbox(host: &Host, address: &str) -> Result<Vec<Entry>> {
    Ok(load::<Inbox>(host, &inbox_key(address))?
        .unwrap_or_default()
        .documents)
}

/// One quarantine, as [`status`] shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QuarantineStatus {
    pub address: String,
    pub documents: usize,
    pub opened_at: u64,
    pub expires_at: u64,
    pub expired: bool,
}

/// Every quarantine, and whether it has expired. Needs `kv` and clock.
pub fn status(host: &Host) -> Result<Vec<QuarantineStatus>> {
    let now = now(host)?;
    let index: Vec<String> = load(host, QUARANTINE_INDEX)?.unwrap_or_default();
    let mut out = Vec::new();
    for address in index {
        if let Some(q) = load::<Quarantine>(host, &quarantine_key(&address))? {
            out.push(QuarantineStatus {
                documents: q.documents.len(),
                opened_at: q.opened_at,
                expires_at: q.expires_at,
                expired: q.expires_at <= now,
                address,
            });
        }
    }
    Ok(out)
}

/// One quarantine a sweep deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Expired {
    pub address: String,
    pub deleted: Vec<String>,
}

#[derive(Debug, Default)]
pub struct Swept {
    pub expired: Vec<Expired>,
    /// One per expired quarantine, for the host to send if it can reach any address.
    pub notices: Vec<Message>,
}

/// Deletes every expired quarantine's documents and record. Its key was never stored, so nothing
/// else needs destroying. Needs `kv`, blob store and clock.
pub fn sweep(host: &mut Host, policy: &Policy) -> Result<Swept> {
    let now = now(host)?;
    let index: Vec<String> = load(host, QUARANTINE_INDEX)?.unwrap_or_default();
    let mut swept = Swept::default();
    for address in index {
        let Some(q) = load::<Quarantine>(host, &quarantine_key(&address))? else {
            continue;
        };
        if q.expires_at > now {
            continue;
        }
        let deleted = expire(host, &address)?;
        swept.notices.push(mail::expiry_notice(
            policy,
            &address,
            deleted.len(),
            q.expires_at,
        ));
        swept.expired.push(Expired { address, deleted });
    }
    Ok(swept)
}

/// Deletes an address's quarantined documents and record; returns the deleted blob ids.
fn expire(host: &mut Host, address: &str) -> Result<Vec<String>> {
    let key = quarantine_key(address);
    let Some(q) = load::<Quarantine>(host, &key)? else {
        return Ok(Vec::new());
    };
    let store = blobs(host)?;
    for d in &q.documents {
        store.delete(&d.id)?;
    }
    forget_quarantine(host, address)?;
    Ok(q.documents.into_iter().map(|d| d.id).collect())
}

fn forget_quarantine(host: &mut Host, address: &str) -> Result<()> {
    kv_mut(host)?.delete(&quarantine_key(address))?;
    let mut index: Vec<String> = load(host, QUARANTINE_INDEX)?.unwrap_or_default();
    index.retain(|a| a != address);
    if index.is_empty() {
        kv_mut(host)?.delete(QUARANTINE_INDEX)?;
    } else {
        save(host, QUARANTINE_INDEX, &index)?;
    }
    Ok(())
}

/// One document moved out of quarantine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Moved {
    pub from: String,
    pub to: String,
}

/// Claims an address's quarantine with the key from its claim link: each document is decrypted
/// and encrypted afresh to the address's keys (so the link opens nothing any more), listed in its
/// inbox, and the quarantined copy deleted. The address must have keys already (onboard it
/// first). Runs where the link was opened: the PWA, or `rdx` locally. Needs `kv`, blob store and
/// random.
pub fn release(host: &mut Host, address: &str, secret: &[u8; 32]) -> Result<Vec<Moved>> {
    let key = quarantine_key(address);
    let q: Quarantine =
        load(host, &key)?.ok_or_else(|| Error::NothingQuarantined(address.into()))?;
    let qkeys = rdx_keys::Keys::from_secret(secret);
    if rdx_keys::public_keys_to_ur(&qkeys.public()) != q.keys {
        return Err(Error::WrongClaim(address.into()));
    }
    let recipients = rdx_keys::ops::recipients("intake.release", host, address)
        .map_err(|e| Error::Keys(e.message))?;
    let mut rng = rdx_keys::host_rng(random(host)?)?;
    let mut inbox: Inbox = load(host, &inbox_key(address))?.unwrap_or_default();
    let mut moved = Vec::new();
    for d in &q.documents {
        let store = blobs(host)?;
        let bytes = store.get(&d.id)?.ok_or_else(|| {
            Error::NotAnEmail(format!("`{}` is missing from the blob store", d.id))
        })?;
        let ur = String::from_utf8(bytes)
            .map_err(|_| Error::NotAnEmail(format!("`{}` isn't an envelope", d.id)))?;
        let sealed = rdx_keys::envelope_from_ur(&ur).map_err(|e| Error::Keys(e.to_string()))?;
        let opened = rdx_keys::decrypt(&sealed, &qkeys).map_err(|e| Error::Keys(e.to_string()))?;
        message_bytes(&opened)?;
        let resealed = rdx_keys::encrypt(&opened, &recipients, &mut rng)
            .map_err(|e| Error::Keys(e.to_string()))?;
        let to = store.put(rdx_keys::envelope_to_ur(&resealed).as_bytes())?;
        store.delete(&d.id)?;
        inbox.documents.push(Entry {
            id: to.clone(),
            ..d.clone()
        });
        moved.push(Moved {
            from: d.id.clone(),
            to,
        });
    }
    save(host, &inbox_key(address), &inbox)?;
    forget_quarantine(host, address)?;
    Ok(moved)
}

fn now(host: &Host) -> Result<u64> {
    Ok(host
        .clock
        .as_ref()
        .ok_or(Error::Missing("clock"))?
        .now_unix_ms()?)
}

fn random(host: &mut Host) -> Result<&mut (dyn rdx_host::Random + 'static)> {
    host.random.as_deref_mut().ok_or(Error::Missing("random"))
}

fn blobs(host: &mut Host) -> Result<&mut (dyn rdx_host::BlobStore + 'static)> {
    host.blob_store
        .as_deref_mut()
        .ok_or(Error::Missing("blob-store"))
}

fn kv_mut(host: &mut Host) -> Result<&mut (dyn rdx_host::Kv + 'static)> {
    host.kv.as_deref_mut().ok_or(Error::Missing("kv"))
}

fn decode<T: for<'de> Deserialize<'de>>(key: &str, bytes: &[u8]) -> Result<T> {
    serde_json::from_slice(bytes).map_err(|e| Error::Record {
        key: key.to_owned(),
        message: e.to_string(),
    })
}

fn load<T: for<'de> Deserialize<'de>>(host: &Host, key: &str) -> Result<Option<T>> {
    let kv = host.kv.as_ref().ok_or(Error::Missing("kv"))?;
    kv.get(key)?.map(|b| decode(key, &b)).transpose()
}

fn save(host: &mut Host, key: &str, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(value).expect("intake records serialize");
    kv_mut(host)?.put(key, &bytes)?;
    Ok(())
}
