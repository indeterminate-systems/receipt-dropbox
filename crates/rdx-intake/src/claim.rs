//! Claiming a quarantine over HTTP: the page that opens a claim link (the claim page now, the
//! PWA later) proves it holds the link's key, fetches what waited, onboards the address and sends
//! the mail back encrypted to the new keys. The server checks every step and decrypts nothing.
//!
//! ```text
//!   page                                            server (Worker)
//!   open_request(address, key) ──── POST open ────▶ open(): signed by the quarantine key?
//!                              ◀─── entries + envelopes (still encrypted to the link's key)
//!   onboard (passkey, keys.onboard) ─▶ registry
//!   reseal(envelopes, key, registry's recipients)
//!   release_request(...) ────────── POST release ─▶ accept(): signed by the quarantine key,
//!                                                    registry valid and for this address,
//!                                                    every document accounted for ─▶
//!                                                    registry stored, inbox listed,
//!                                                    quarantine and its blobs deleted
//! ```
//!
//! A request is an envelope whose subject is the address, signed with the quarantine key the
//! link holds. Only the sender's mailbox received that key, so a valid signature is the proof of
//! the mailbox ADR-P13 asks for before a registry is published: without it anyone could register
//! keys for an allowed address and receive its mail. Release carries the new documents' digests,
//! so the signature covers them too, and `at`, so an old request can't be replayed later.

use bc_envelope::prelude::*;
use rdx_host::Host;
use rdx_keys::{Keys, Registry};
use serde::{Deserialize, Serialize};

use crate::{
    Entry, Error, Inbox, Moved, QUARANTINE_INDEX, Quarantine, Result, forget_quarantine, inbox_key,
    load, quarantine_key, registry_key, save,
};

/// The `'isA'` of a claim request.
pub const CLAIM_TYPE: &str = "rdx.intake.claim.v1";
/// How far a request's `at` may be from the server's clock: enough for a slow passkey prompt.
pub const MAX_SKEW_MS: u64 = 15 * 60 * 1000;

const ACTION: &str = "action";
const AT: &str = "at";
const REGISTRY: &str = "registry";
const DOCUMENTS: &str = "documents";

/// `POST …/open`'s answer: the quarantine's entries and their envelopes, in the same order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Opened {
    pub address: String,
    pub expires_at: u64,
    pub documents: Vec<Entry>,
    /// `ur:envelope/…`, still encrypted to the link's key. The server fills these in.
    #[serde(default)]
    pub envelopes: Vec<String>,
}

/// `POST …/release`'s body: the signed request and the new envelopes it names by digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub request: String,
    pub envelopes: Vec<String>,
}

fn request(address: &str, action: &str, at: u64) -> Envelope {
    Envelope::new(address)
        .add_assertion(known_values::IS_A, CLAIM_TYPE)
        .add_assertion(ACTION, action)
        .add_assertion(AT, at)
}

fn sign(secret: &[u8; 32], envelope: &Envelope) -> String {
    rdx_keys::envelope_to_ur(&Keys::from_secret(secret).sign(envelope))
}

/// Asks for a quarantine's documents (`ur:envelope/…`). Made where the link was opened.
pub fn open_request(address: &str, secret: &[u8; 32], at: u64) -> String {
    sign(secret, &request(address, "open", at))
}

/// Decrypts each quarantined envelope with the link's key and encrypts it to `recipients` (the
/// new registry's). Made where the link was opened; the plaintext stays in that memory.
pub fn reseal(
    opened: &Opened,
    secret: &[u8; 32],
    recipients: &[bc_components::PublicKeys],
    random: &mut dyn rdx_host::Random,
) -> Result<Vec<(Moved, String)>> {
    if opened.envelopes.len() != opened.documents.len() {
        return Err(Error::ClaimLink(
            "the server sent a different number of documents and envelopes".into(),
        ));
    }
    let qkeys = Keys::from_secret(secret);
    let mut rng = rdx_keys::host_rng(random)?;
    let mut out = Vec::new();
    for (entry, ur) in opened.documents.iter().zip(&opened.envelopes) {
        if rdx_host::sha256_digest(ur.as_bytes()) != entry.id {
            return Err(Error::NotAnEmail(format!(
                "the envelope sent for `{}` isn't that document",
                entry.id
            )));
        }
        let sealed = rdx_keys::envelope_from_ur(ur).map_err(|e| Error::Keys(e.to_string()))?;
        let plain = rdx_keys::decrypt(&sealed, &qkeys).map_err(|e| Error::Keys(e.to_string()))?;
        crate::message_bytes(&plain)?;
        let resealed = rdx_keys::encrypt(&plain, recipients, &mut rng)
            .map_err(|e| Error::Keys(e.to_string()))?;
        let new = rdx_keys::envelope_to_ur(&resealed);
        out.push((
            Moved {
                from: entry.id.clone(),
                to: rdx_host::sha256_digest(new.as_bytes()),
            },
            new,
        ));
    }
    Ok(out)
}

/// Publishes `registry` for the address and moves the quarantine to it: the documents
/// [`reseal`] made, named by digest under the link key's signature.
pub fn release_request(
    address: &str,
    secret: &[u8; 32],
    at: u64,
    registry: &Registry,
    resealed: &[(Moved, String)],
) -> Release {
    let moves: Vec<&Moved> = resealed.iter().map(|(m, _)| m).collect();
    let envelope = request(address, "release", at)
        .add_assertion(
            REGISTRY,
            serde_json::to_string(registry).expect("registries serialize"),
        )
        .add_assertion(
            DOCUMENTS,
            serde_json::to_string(&moves).expect("moves serialize"),
        );
    Release {
        request: sign(secret, &envelope),
        envelopes: resealed.iter().map(|(_, ur)| ur.clone()).collect(),
    }
}

/// The address a request names, unchecked, so a host knows which kv keys to load first.
pub fn request_address(request: &str) -> Result<String> {
    let content = parse(request)?.1;
    content
        .extract_subject::<String>()
        .map_err(|e| Error::ClaimLink(format!("the request names no address: {e}")))
}

/// Every kv key [`open`] reads.
pub fn open_keys(address: &str) -> Vec<String> {
    vec![quarantine_key(address)]
}

/// Every kv key [`accept`] may read or write.
pub fn release_keys(address: &str) -> Vec<String> {
    vec![
        registry_key(address),
        inbox_key(address),
        quarantine_key(address),
        QUARANTINE_INDEX.to_owned(),
    ]
}

fn parse(request: &str) -> Result<(Envelope, Envelope)> {
    let bad = |e: String| Error::ClaimLink(format!("the request isn't a signed claim: {e}"));
    let signed = rdx_keys::envelope_from_ur(request).map_err(|e| bad(e.to_string()))?;
    let content = signed.try_unwrap().map_err(|e| bad(e.to_string()))?;
    Ok((signed, content))
}

fn text(content: &Envelope, predicate: &str) -> Result<String> {
    content
        .extract_object_for_predicate::<String>(predicate)
        .map_err(|e| Error::ClaimLink(format!("the request's `{predicate}`: {e}")))
}

fn number(content: &Envelope, predicate: &str) -> Result<u64> {
    content
        .extract_object_for_predicate::<u64>(predicate)
        .map_err(|e| Error::ClaimLink(format!("the request's `{predicate}`: {e}")))
}

/// Checks a request: its type, action, freshness, and that the address's current quarantine key
/// signed it. Returns the address, its quarantine and the request's content.
fn verify(host: &Host, request: &str, action: &str) -> Result<(String, Quarantine, Envelope)> {
    let (signed, content) = parse(request)?;
    let kind: String = content
        .extract_object_for_predicate(known_values::IS_A)
        .map_err(|_| Error::ClaimLink("it isn't a claim request".into()))?;
    if kind != CLAIM_TYPE {
        return Err(Error::ClaimLink(format!("it is a `{kind}`, not a claim")));
    }
    let address: String = content
        .extract_subject()
        .map_err(|e| Error::ClaimLink(format!("the request names no address: {e}")))?;
    let asked = text(&content, ACTION)?;
    if asked != action {
        return Err(Error::ClaimLink(format!(
            "this is a `{asked}` request, not `{action}`"
        )));
    }
    let at = number(&content, AT)?;
    let now = crate::now(host)?;
    if now.abs_diff(at) > MAX_SKEW_MS {
        return Err(Error::ClaimLink(
            "the request is too old, or this device's clock is wrong; try again".into(),
        ));
    }
    let key = quarantine_key(&address);
    let q: Quarantine =
        load(host, &key)?.ok_or_else(|| Error::NothingQuarantined(address.clone()))?;
    if q.expires_at <= now {
        return Err(Error::NothingQuarantined(address));
    }
    let keys = rdx_keys::public_keys_from_ur(&q.keys).map_err(|e| Error::Record {
        key,
        message: e.to_string(),
    })?;
    signed
        .verify(keys.signing_public_key())
        .map_err(|_| Error::WrongClaim(address.clone()))?;
    Ok((address, q, content))
}

/// Server side of `open`: the quarantine's entries, if its key signed the request. The host
/// adds the envelopes from its blob store. Needs `kv` and clock.
pub fn open(host: &Host, request: &str) -> Result<Opened> {
    let (address, q, _) = verify(host, request, "open")?;
    Ok(Opened {
        address,
        expires_at: q.expires_at,
        documents: q.documents,
        envelopes: Vec::new(),
    })
}

/// Server side of `release`: checks the request and the new registry, stores the re-encrypted
/// documents, lists them in the address's inbox and deletes the quarantine. Needs `kv`, blob
/// store and clock.
pub fn accept(host: &mut Host, release: &Release) -> Result<Vec<Moved>> {
    let (address, q, content) = verify(host, &release.request, "release")?;

    let registry: Registry = serde_json::from_str(&text(&content, REGISTRY)?)
        .map_err(|e| Error::ClaimLink(format!("the request's registry: {e}")))?;
    if registry.address != address {
        return Err(Error::ClaimLink(format!(
            "the registry is for `{}`, not `{address}`",
            registry.address
        )));
    }
    registry
        .state()
        .map_err(|e| Error::ClaimLink(format!("the registry doesn't check out: {e}")))?;
    if load::<Registry>(host, &registry_key(&address))?.is_some() {
        return Err(Error::ClaimLink(format!(
            "`{address}` already has keys; add this device from one that has them"
        )));
    }

    let moves: Vec<Moved> = serde_json::from_str(&text(&content, DOCUMENTS)?)
        .map_err(|e| Error::ClaimLink(format!("the request's documents: {e}")))?;
    let mut froms: Vec<&str> = moves.iter().map(|m| m.from.as_str()).collect();
    let mut waiting: Vec<&str> = q.documents.iter().map(|d| d.id.as_str()).collect();
    froms.sort_unstable();
    waiting.sort_unstable();
    if froms != waiting {
        return Err(Error::ClaimLink(
            "the documents don't match what is waiting (new mail may have arrived); reload the \
             page"
                .into(),
        ));
    }
    if release.envelopes.len() != moves.len() {
        return Err(Error::ClaimLink(
            "the request names a different number of documents than it carries".into(),
        ));
    }
    for (m, ur) in moves.iter().zip(&release.envelopes) {
        if rdx_host::sha256_digest(ur.as_bytes()) != m.to {
            return Err(Error::ClaimLink(format!(
                "the document sent for `{}` isn't the one the request names",
                m.to
            )));
        }
        rdx_keys::envelope_from_ur(ur).map_err(|e| Error::Keys(e.to_string()))?;
    }

    let store = crate::blobs(host)?;
    for ur in &release.envelopes {
        store.put(ur.as_bytes())?;
    }
    save(host, &registry_key(&address), &registry)?;
    let mut inbox: Inbox = load(host, &inbox_key(&address))?.unwrap_or_default();
    for m in &moves {
        let entry = q
            .documents
            .iter()
            .find(|d| d.id == m.from)
            .expect("checked above");
        inbox.documents.push(Entry {
            id: m.to.clone(),
            ..entry.clone()
        });
    }
    save(host, &inbox_key(&address), &inbox)?;
    let store = crate::blobs(host)?;
    for d in &q.documents {
        store.delete(&d.id)?;
    }
    forget_quarantine(host, &address)?;
    Ok(moves)
}
