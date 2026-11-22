//! A statement's custody record: where its encrypted original is and who may read it.
//!
//! A record is a chain of signed events, each a Gordian Envelope, like the key registry:
//!
//! ```text
//! {
//!     "st-2026-03.s-0001" [
//!         'isA': "rdx.custody.event.v1"
//!         "event": "kept" | "granted" | "revoked"
//!         "sequence": 1                     "previous": "sha256:…"   (the event before)
//!         "at": 1767225600000               "address": "ana@example.org"
//!         "device": "laptop"                "signerKey": "ur:signing-public-key/…"
//!         "root": "sha256:…"  "sealed": "sha256:…"  "original": "sha256:…"  "source": "sha256:…"
//!                                           (kept)
//!         "grantee": "auditor@example.org"  "reason": …   (granted, revoked)
//!         "recipient": "ur:crypto-pubkeys/…" …  "sealedCopy": "sha256:…"  "sourceCopy": …
//!                                           (granted)
//!     ]
//! } [ 'signed': Signature ]
//! ```
//!
//! [`Custody::state`] replays the chain, checking every link and signature. The first event is
//! `kept`; every later one is signed by a device of the same address. Whether a signing key is in
//! the supporter's registry is the trail's question (`audit.trail`), since keys rotate.

use bc_components::SigningPublicKey;
use bc_envelope::prelude::*;
use rdx_keys::{Keys, envelope_from_ur, envelope_to_ur};
use serde::{Deserialize, Serialize};

/// The `'isA'` of a custody event.
pub const EVENT_TYPE: &str = "rdx.custody.event.v1";

mod pred {
    pub const EVENT: &str = "event";
    pub const SEQUENCE: &str = "sequence";
    pub const PREVIOUS: &str = "previous";
    pub const AT: &str = "at";
    pub const ADDRESS: &str = "address";
    pub const DEVICE: &str = "device";
    pub const SIGNER_KEY: &str = "signerKey";
    pub const ROOT: &str = "root";
    pub const SEALED: &str = "sealed";
    pub const ORIGINAL: &str = "original";
    pub const SOURCE: &str = "source";
    pub const SIGNER: &str = "signer";
    pub const GRANTEE: &str = "grantee";
    pub const REASON: &str = "reason";
    pub const RECIPIENT: &str = "recipient";
    pub const SEALED_COPY: &str = "sealedCopy";
    pub const SOURCE_COPY: &str = "sourceCopy";
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("custody record: {0}")]
pub struct Error(pub String);

fn bad(what: impl std::fmt::Display) -> Error {
    Error(what.to_string())
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// What `kept` records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kept {
    /// The statement envelope's root digest (the same for its redacted views).
    pub root: String,
    /// Blob of the sealed (signed, encrypted) statement envelope.
    pub sealed: String,
    /// Digest of the original as the bank sent it: what the signed statement and the ledger name.
    pub original: String,
    /// Blob of the encrypted original.
    pub source: String,
    /// `ur:signing-public-key/…` the statement envelope verifies with.
    pub signer: String,
}

/// What `granted` records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    pub grantee: String,
    pub reason: String,
    /// `ur:crypto-pubkeys/…` added to the copies.
    pub recipients: Vec<String>,
    /// Copy of the sealed statement the grantee can open.
    pub sealed: String,
    /// Copy of the encrypted original, when it was granted too.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// One event's common fields, as the trail reports them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signed {
    pub event: String,
    pub at: u64,
    pub device: String,
    /// `ur:signing-public-key/…` of the device that signed.
    pub signer_key: String,
}

/// A grant as it stands now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantState {
    #[serde(flatten)]
    pub grant: Grant,
    pub granted_at: u64,
    pub granted_by: String,
    /// When it was revoked, and why; `None` while the grantee may still open the copies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revoked_reason: Option<String>,
}

impl GrantState {
    pub fn is_active(&self) -> bool {
        self.revoked_at.is_none()
    }
}

/// What a custody record says now, after every event checked out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct State {
    pub statement: String,
    pub address: String,
    pub kept: Kept,
    pub kept_at: u64,
    /// Grants in the order they were made, revoked ones included.
    pub grants: Vec<GrantState>,
    /// Every event's signer, in order, for checking against the supporter's registry.
    pub events: Vec<Signed>,
    /// Digest of the last event, which the next one links to.
    #[serde(skip)]
    pub head: String,
}

impl State {
    /// The grant to `grantee` that is still active, if any.
    pub fn active_grant(&self, grantee: &str) -> Option<&GrantState> {
        self.grants
            .iter()
            .find(|g| g.grant.grantee == grantee && g.is_active())
    }
}

/// A custody record as stored: one `ur:envelope/…` per event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Custody {
    pub statement: String,
    pub events: Vec<String>,
}

/// The device signing an event.
pub struct By<'a> {
    pub device: &'a str,
    pub keys: &'a Keys,
}

fn digest_of(e: &Envelope) -> String {
    let hex: String = e
        .digest()
        .data()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("sha256:{hex}")
}

fn text(e: &Envelope, p: &str) -> Result<String> {
    e.extract_object_for_predicate::<String>(p)
        .map_err(|err| bad(format!("`{p}`: {err}")))
}

fn optional_text(e: &Envelope, p: &str) -> Result<Option<String>> {
    match e.optional_object_for_predicate(p) {
        Ok(Some(o)) => o
            .extract_subject::<String>()
            .map(Some)
            .map_err(|err| bad(format!("`{p}`: {err}"))),
        Ok(None) => Ok(None),
        Err(err) => Err(bad(format!("`{p}`: {err}"))),
    }
}

fn number(e: &Envelope, p: &str) -> Result<u64> {
    e.extract_object_for_predicate::<u64>(p)
        .map_err(|err| bad(format!("`{p}`: {err}")))
}

fn signing_key_from_ur(ur: &str) -> Result<SigningPublicKey> {
    rdx_envelope::public_key_from_ur(ur).map_err(bad)
}

impl Custody {
    /// A new record: the statement's original and sealed envelope are kept, encrypted.
    pub fn kept(statement: &str, address: &str, kept: &Kept, by: &By, at: u64) -> Result<Self> {
        let mut c = Self {
            statement: statement.to_owned(),
            events: Vec::new(),
        };
        let event = c
            .event("kept", address, by, at)
            .add_assertion(pred::ROOT, kept.root.as_str())
            .add_assertion(pred::SEALED, kept.sealed.as_str())
            .add_assertion(pred::ORIGINAL, kept.original.as_str())
            .add_assertion(pred::SOURCE, kept.source.as_str())
            .add_assertion(pred::SIGNER, kept.signer.as_str());
        c.append(event, by, None)?;
        Ok(c)
    }

    /// Records that `grant.grantee` may open the copies `grant` names.
    pub fn grant(&mut self, grant: &Grant, by: &By, at: u64) -> Result<()> {
        let state = self.state()?;
        let mut event = self
            .event("granted", &state.address, by, at)
            .add_assertion(pred::GRANTEE, grant.grantee.as_str())
            .add_assertion(pred::REASON, grant.reason.as_str())
            .add_assertion(pred::SEALED_COPY, grant.sealed.as_str());
        if let Some(source) = &grant.source {
            event = event.add_assertion(pred::SOURCE_COPY, source.as_str());
        }
        for r in &grant.recipients {
            event = event.add_assertion(pred::RECIPIENT, r.as_str());
        }
        self.append(event, by, Some(&state))
    }

    /// Records that `grantee`'s grant ended.
    pub fn revoke(&mut self, grantee: &str, reason: &str, by: &By, at: u64) -> Result<()> {
        let state = self.state()?;
        let event = self
            .event("revoked", &state.address, by, at)
            .add_assertion(pred::GRANTEE, grantee)
            .add_assertion(pred::REASON, reason);
        self.append(event, by, Some(&state))
    }

    fn event(&self, kind: &str, address: &str, by: &By, at: u64) -> Envelope {
        Envelope::new(self.statement.as_str())
            .add_assertion(known_values::IS_A, EVENT_TYPE)
            .add_assertion(pred::EVENT, kind)
            .add_assertion(pred::SEQUENCE, self.events.len() as u64)
            .add_assertion(pred::AT, at)
            .add_assertion(pred::ADDRESS, address)
            .add_assertion(pred::DEVICE, by.device)
            .add_assertion(
                pred::SIGNER_KEY,
                rdx_envelope::public_key_to_ur(&by.keys.signing_public()),
            )
    }

    /// Links, signs and appends an event, then replays the chain: an event the rules don't allow
    /// is refused and the record is left as it was.
    fn append(&mut self, mut event: Envelope, by: &By, state: Option<&State>) -> Result<()> {
        if let Some(s) = state {
            event = event.add_assertion(pred::PREVIOUS, s.head.as_str());
        }
        self.events.push(envelope_to_ur(&by.keys.sign(&event)));
        if let Err(e) = self.state() {
            self.events.pop();
            return Err(e);
        }
        Ok(())
    }

    /// Replays every event, checking its link, signature and rule.
    pub fn state(&self) -> Result<State> {
        let mut state: Option<State> = None;
        for (sequence, ur) in self.events.iter().enumerate() {
            let at_event = |e: Error| bad(format!("event {sequence}: {}", e.0));
            let signed = envelope_from_ur(ur).map_err(|e| at_event(bad(e)))?;
            let content = signed.try_unwrap().map_err(|e| at_event(bad(e)))?;
            apply(&mut state, &self.statement, sequence, &signed, &content).map_err(at_event)?;
        }
        state.ok_or_else(|| bad("it has no events"))
    }
}

fn apply(
    state: &mut Option<State>,
    statement: &str,
    sequence: usize,
    signed: &Envelope,
    content: &Envelope,
) -> Result<()> {
    let kind: String = content
        .extract_object_for_predicate(known_values::IS_A)
        .map_err(|_| bad(format!("not an `{EVENT_TYPE}`")))?;
    if kind != EVENT_TYPE {
        return Err(bad(format!("it is a `{kind}`, not an `{EVENT_TYPE}`")));
    }
    let subject: String = content.extract_subject().map_err(bad)?;
    if subject != statement {
        return Err(bad(format!("it is for `{subject}`, not `{statement}`")));
    }
    if number(content, pred::SEQUENCE)? != sequence as u64 {
        return Err(bad("out of sequence"));
    }
    let event = text(content, pred::EVENT)?;
    let at = number(content, pred::AT)?;
    let address = text(content, pred::ADDRESS)?;
    let device = text(content, pred::DEVICE)?;
    let signer_key = text(content, pred::SIGNER_KEY)?;
    let previous = optional_text(content, pred::PREVIOUS)?;
    signed
        .verify(&signing_key_from_ur(&signer_key)?)
        .map_err(|_| bad(format!("`{device}`'s signature does not verify")))?;
    let signer = Signed {
        event: event.clone(),
        at,
        device: device.clone(),
        signer_key,
    };
    let head = digest_of(signed);

    let Some(s) = state.as_mut() else {
        if event != "kept" || previous.is_some() {
            return Err(bad("the first event must be `kept`, linking to nothing"));
        }
        *state = Some(State {
            statement: statement.to_owned(),
            address,
            kept: Kept {
                root: text(content, pred::ROOT)?,
                sealed: text(content, pred::SEALED)?,
                original: text(content, pred::ORIGINAL)?,
                source: text(content, pred::SOURCE)?,
                signer: text(content, pred::SIGNER)?,
            },
            kept_at: at,
            grants: Vec::new(),
            events: vec![signer],
            head,
        });
        return Ok(());
    };

    if previous.as_deref() != Some(s.head.as_str()) {
        return Err(bad("it doesn't link to the event before it"));
    }
    if address != s.address {
        return Err(bad(format!(
            "it was signed for `{address}`, but the statement is `{}`'s",
            s.address
        )));
    }
    match event.as_str() {
        "granted" => {
            let grantee = text(content, pred::GRANTEE)?;
            if s.active_grant(&grantee).is_some() {
                return Err(bad(format!(
                    "`{grantee}` already has an active grant; revoke it first"
                )));
            }
            let recipients = content
                .objects_for_predicate(pred::RECIPIENT)
                .iter()
                .map(|o| o.extract_subject::<String>().map_err(bad))
                .collect::<Result<Vec<_>>>()?;
            if recipients.is_empty() {
                return Err(bad("a grant names at least one `recipient`"));
            }
            s.grants.push(GrantState {
                grant: Grant {
                    grantee,
                    reason: text(content, pred::REASON)?,
                    recipients,
                    sealed: text(content, pred::SEALED_COPY)?,
                    source: optional_text(content, pred::SOURCE_COPY)?,
                },
                granted_at: at,
                granted_by: device,
                revoked_at: None,
                revoked_reason: None,
            });
        }
        "revoked" => {
            let grantee = text(content, pred::GRANTEE)?;
            let reason = text(content, pred::REASON)?;
            let g = s
                .grants
                .iter_mut()
                .find(|g| g.grant.grantee == grantee && g.is_active())
                .ok_or_else(|| bad(format!("`{grantee}` has no active grant to revoke")))?;
            g.revoked_at = Some(at);
            g.revoked_reason = Some(reason);
        }
        "kept" => return Err(bad("a statement is kept once")),
        other => return Err(bad(format!("unknown event `{other}`"))),
    }
    s.events.push(signer);
    s.head = head;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(n: u8) -> Keys {
        Keys::from_secret(&[n; 32])
    }

    fn kept() -> Kept {
        Kept {
            root: "sha256:01".into(),
            sealed: "sha256:02".into(),
            original: "sha256:03".into(),
            source: "sha256:04".into(),
            signer: rdx_envelope::public_key_to_ur(&keys(1).signing_public()),
        }
    }

    fn grant(who: &str) -> Grant {
        Grant {
            grantee: who.into(),
            reason: "annual audit".into(),
            recipients: vec![rdx_keys::public_keys_to_ur(&keys(9).public())],
            sealed: "sha256:05".into(),
            source: Some("sha256:06".into()),
        }
    }

    #[test]
    fn a_record_replays_kept_grants_and_revocations() {
        let k = keys(1);
        let by = By {
            device: "laptop",
            keys: &k,
        };
        let mut c = Custody::kept("st-1", "ana@example.org", &kept(), &by, 10).unwrap();
        c.grant(&grant("auditor@example.org"), &by, 20).unwrap();
        c.revoke("auditor@example.org", "audit done", &by, 30)
            .unwrap();
        c.grant(&grant("auditor@example.org"), &by, 40).unwrap();

        let s = c.state().unwrap();
        assert_eq!(s.kept, kept());
        assert_eq!(s.address, "ana@example.org");
        assert_eq!(s.grants.len(), 2);
        assert_eq!(s.grants[0].revoked_at, Some(30));
        assert!(s.grants[1].is_active());
        assert_eq!(s.events.len(), 4);
        assert!(s.events.iter().all(|e| e.device == "laptop"));
    }

    #[test]
    fn rules_are_enforced() {
        let k = keys(1);
        let by = By {
            device: "laptop",
            keys: &k,
        };
        let mut c = Custody::kept("st-1", "ana@example.org", &kept(), &by, 10).unwrap();
        assert!(
            c.revoke("auditor@example.org", "x", &by, 20)
                .unwrap_err()
                .0
                .contains("no active grant")
        );
        c.grant(&grant("auditor@example.org"), &by, 20).unwrap();
        assert!(
            c.grant(&grant("auditor@example.org"), &by, 30)
                .unwrap_err()
                .0
                .contains("already has an active grant")
        );
        // Refused events leave the record as it was.
        assert_eq!(c.events.len(), 2);
    }

    #[test]
    fn tampering_is_detected() {
        let k = keys(1);
        let by = By {
            device: "laptop",
            keys: &k,
        };
        let mut c = Custody::kept("st-1", "ana@example.org", &kept(), &by, 10).unwrap();
        c.grant(&grant("auditor@example.org"), &by, 20).unwrap();

        // Dropping the first event breaks the chain.
        let mut dropped = c.clone();
        dropped.events.remove(0);
        assert!(dropped.state().is_err());

        // An event moved to another statement's record doesn't fit there.
        let mut moved = c.clone();
        moved.statement = "st-2".into();
        assert!(moved.state().unwrap_err().0.contains("not `st-2`"));

        // Replay checks each event against the key it names, so another key can still append a
        // well-formed event; `audit.trail` catches it by checking signer keys against the
        // supporter's registry.
        let other = keys(2);
        let mut forged = c.clone();
        forged
            .grant(
                &grant("mallory@example.org"),
                &By {
                    device: "laptop",
                    keys: &other,
                },
                30,
            )
            .unwrap();
        let s = forged.state().unwrap();
        assert_ne!(s.events[2].signer_key, s.events[0].signer_key);
    }
}
