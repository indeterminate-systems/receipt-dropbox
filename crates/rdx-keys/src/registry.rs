//! The address-to-keys registry: which device keys an address has, and its recovery key.
//!
//! A registry is a chain of signed events, each a Gordian Envelope:
//!
//! ```text
//! {
//!     "ana@example.org" [
//!         'isA': "rdx.keys.event.v1"
//!         "event": "genesis" | "add" | "rotate" | "revoke"
//!         "sequence": 3                    "previous": "sha256:…"   (the event before)
//!         "at": 1767225600000              "device": "phone"
//!         "keys": "ur:crypto-pubkeys/…"    (genesis, add, rotate)
//!         "supporter": "s-0001"  "recovery": "ur:crypto-pubkeys/…"   (genesis)
//!         "reason": "lost"                 (revoke)
//!         "signer": "laptop" | "recovery"
//!     ]
//! } [ 'signed': Signature ]
//! ```
//!
//! [`Registry::state`] replays the chain and checks every link and signature, so a server can hold
//! the registry (it is public keys only) without being able to change it. The rules:
//!
//! - **genesis** comes first, names the supporter, the recovery key and the first device, and is
//!   signed by the recovery key (which only exists while onboarding or recovering).
//! - **add** enrolls a new device id; ids are never reused.
//! - **rotate** replaces an active device's keys; signed by that device's current key or the
//!   recovery key.
//! - **revoke** retires an active device; signed by any active device or the recovery key.
//! - Every other event is signed by an active device or the recovery key.

use std::collections::BTreeMap;

use bc_components::{PublicKeys, SigningPublicKey};
use bc_envelope::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    Error, Keys, Result, envelope_from_ur, envelope_to_ur, public_keys_from_ur, public_keys_to_ur,
};

/// The `'isA'` of a registry event.
pub const EVENT_TYPE: &str = "rdx.keys.event.v1";

/// The signer name of the recovery key.
pub const RECOVERY: &str = "recovery";

mod pred {
    pub const EVENT: &str = "event";
    pub const SEQUENCE: &str = "sequence";
    pub const PREVIOUS: &str = "previous";
    pub const AT: &str = "at";
    pub const DEVICE: &str = "device";
    pub const KEYS: &str = "keys";
    pub const SUPPORTER: &str = "supporter";
    pub const RECOVERY: &str = "recovery";
    pub const REASON: &str = "reason";
    pub const SIGNER: &str = "signer";
}

fn bad(what: impl std::fmt::Display) -> Error {
    Error::Registry(what.to_string())
}

/// Who signs an event: a device with its keys, or the recovery key.
pub enum Authority<'a> {
    Device(&'a str, &'a Keys),
    Recovery(&'a Keys),
}

impl Authority<'_> {
    fn name(&self) -> &str {
        match self {
            Authority::Device(id, _) => id,
            Authority::Recovery(_) => RECOVERY,
        }
    }

    fn keys(&self) -> &Keys {
        match self {
            Authority::Device(_, k) | Authority::Recovery(k) => k,
        }
    }
}

/// A device's place in the registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum DeviceStatus {
    Active,
    Revoked { at: u64, reason: String },
}

#[derive(Debug, Clone)]
pub struct Device {
    pub id: String,
    pub keys: PublicKeys,
    pub added_at: u64,
    /// How many times its keys were rotated.
    pub rotations: usize,
    pub status: DeviceStatus,
}

impl Device {
    pub fn is_active(&self) -> bool {
        self.status == DeviceStatus::Active
    }
}

/// What a registry says now, after every event checked out.
#[derive(Debug, Clone)]
pub struct State {
    pub address: String,
    pub supporter: String,
    pub recovery: PublicKeys,
    /// Devices by id, revoked ones included.
    pub devices: BTreeMap<String, Device>,
    /// Digest of the last event, which the next one links to.
    pub head: String,
}

impl State {
    /// The keys new documents for this address are encrypted to: every active device key, then
    /// the recovery key.
    pub fn recipients(&self) -> Vec<PublicKeys> {
        self.devices
            .values()
            .filter(|d| d.is_active())
            .map(|d| d.keys.clone())
            .chain(std::iter::once(self.recovery.clone()))
            .collect()
    }

    /// The signing key of an active device, or of the recovery key.
    pub fn signer(&self, name: &str) -> Option<&SigningPublicKey> {
        if name == RECOVERY {
            return Some(self.recovery.signing_public_key());
        }
        self.devices
            .get(name)
            .filter(|d| d.is_active())
            .map(|d| d.keys.signing_public_key())
    }
}

/// A registry's events, as stored: one `ur:envelope/…` per event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registry {
    pub address: String,
    pub events: Vec<String>,
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

impl Registry {
    /// A new registry: the supporter, their recovery key and their first device, signed by the
    /// recovery key.
    pub fn genesis(
        address: &str,
        supporter: &str,
        recovery: &Keys,
        device: &str,
        device_keys: &PublicKeys,
        at: u64,
    ) -> Result<Self> {
        let mut r = Self {
            address: address.to_owned(),
            events: Vec::new(),
        };
        let event = r
            .event("genesis", device, at)
            .add_assertion(pred::KEYS, public_keys_to_ur(device_keys))
            .add_assertion(pred::SUPPORTER, supporter)
            .add_assertion(pred::RECOVERY, public_keys_to_ur(&recovery.public()));
        r.append(event, &Authority::Recovery(recovery))?;
        Ok(r)
    }

    /// Enrolls a new device.
    pub fn add(&mut self, device: &str, keys: &PublicKeys, by: &Authority, at: u64) -> Result<()> {
        let event = self
            .event("add", device, at)
            .add_assertion(pred::KEYS, public_keys_to_ur(keys));
        self.append(event, by)
    }

    /// Replaces an active device's keys.
    pub fn rotate(
        &mut self,
        device: &str,
        keys: &PublicKeys,
        by: &Authority,
        at: u64,
    ) -> Result<()> {
        let event = self
            .event("rotate", device, at)
            .add_assertion(pred::KEYS, public_keys_to_ur(keys));
        self.append(event, by)
    }

    /// Retires an active device: nothing new is encrypted to it.
    pub fn revoke(&mut self, device: &str, reason: &str, by: &Authority, at: u64) -> Result<()> {
        let event = self
            .event("revoke", device, at)
            .add_assertion(pred::REASON, reason);
        self.append(event, by)
    }

    fn event(&self, kind: &str, device: &str, at: u64) -> Envelope {
        Envelope::new(self.address.as_str())
            .add_assertion(known_values::IS_A, EVENT_TYPE)
            .add_assertion(pred::EVENT, kind)
            .add_assertion(pred::SEQUENCE, self.events.len() as u64)
            .add_assertion(pred::AT, at)
            .add_assertion(pred::DEVICE, device)
    }

    /// Links, signs and appends an event, then replays the whole chain: an event the rules don't
    /// allow is refused and the registry is left as it was.
    fn append(&mut self, mut event: Envelope, by: &Authority) -> Result<()> {
        if !self.events.is_empty() {
            event = event.add_assertion(pred::PREVIOUS, self.state()?.head);
        }
        let signed = by
            .keys()
            .sign(&event.add_assertion(pred::SIGNER, by.name()));
        self.events.push(envelope_to_ur(&signed));
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
            let at_event = |e: Error| bad(format!("event {sequence}: {e}"));
            let signed = envelope_from_ur(ur).map_err(at_event)?;
            let content = signed.try_unwrap().map_err(|e| at_event(bad(e)))?;
            apply(&mut state, &self.address, sequence, &signed, &content).map_err(at_event)?;
        }
        state.ok_or_else(|| bad("it has no events"))
    }
}

fn apply(
    state: &mut Option<State>,
    address: &str,
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
    if subject != address {
        return Err(bad(format!("it is for `{subject}`, not `{address}`")));
    }
    if number(content, pred::SEQUENCE)? != sequence as u64 {
        return Err(bad("out of sequence"));
    }
    let event = text(content, pred::EVENT)?;
    let device = text(content, pred::DEVICE)?;
    let at = number(content, pred::AT)?;
    let signer = text(content, pred::SIGNER)?;
    let previous = optional_text(content, pred::PREVIOUS)?;
    let keys = optional_text(content, pred::KEYS)?
        .map(|k| public_keys_from_ur(&k))
        .transpose()?;
    let need_keys = || {
        keys.clone()
            .ok_or_else(|| bad(format!("`{event}` needs `keys`")))
    };
    let head = digest_of(signed);

    let Some(s) = state.as_mut() else {
        if event != "genesis" {
            return Err(bad("the first event must be `genesis`"));
        }
        if signer != RECOVERY || previous.is_some() {
            return Err(bad(
                "`genesis` is signed by the recovery key and links to nothing",
            ));
        }
        let recovery = public_keys_from_ur(&text(content, pred::RECOVERY)?)?;
        signed
            .verify(recovery.signing_public_key())
            .map_err(|_| bad("the recovery key's signature does not verify"))?;
        let mut devices = BTreeMap::new();
        devices.insert(
            device.clone(),
            Device {
                id: device,
                keys: need_keys()?,
                added_at: at,
                rotations: 0,
                status: DeviceStatus::Active,
            },
        );
        *state = Some(State {
            address: address.to_owned(),
            supporter: text(content, pred::SUPPORTER)?,
            recovery,
            devices,
            head,
        });
        return Ok(());
    };

    if previous.as_deref() != Some(s.head.as_str()) {
        return Err(bad("it doesn't link to the event before it"));
    }
    let key = s.signer(&signer).ok_or_else(|| {
        bad(format!(
            "`{signer}` is not an active device or the recovery key"
        ))
    })?;
    signed
        .verify(key)
        .map_err(|_| bad(format!("`{signer}`'s signature does not verify")))?;
    match event.as_str() {
        "add" => {
            if s.devices.contains_key(&device) {
                return Err(bad(format!(
                    "device `{device}` already exists; ids are never reused"
                )));
            }
            s.devices.insert(
                device.clone(),
                Device {
                    id: device,
                    keys: need_keys()?,
                    added_at: at,
                    rotations: 0,
                    status: DeviceStatus::Active,
                },
            );
        }
        "rotate" => {
            if signer != device && signer != RECOVERY {
                return Err(bad(format!(
                    "only `{device}` itself or the recovery key can rotate its keys"
                )));
            }
            let d = active(s, &device)?;
            d.keys = need_keys()?;
            d.rotations += 1;
        }
        "revoke" => {
            let reason = text(content, pred::REASON)?;
            active(s, &device)?.status = DeviceStatus::Revoked { at, reason };
        }
        other => return Err(bad(format!("unknown event `{other}`"))),
    }
    s.head = head;
    Ok(())
}

fn active<'s>(s: &'s mut State, device: &str) -> Result<&'s mut Device> {
    s.devices
        .get_mut(device)
        .filter(|d| d.is_active())
        .ok_or_else(|| bad(format!("`{device}` is not an active device")))
}
