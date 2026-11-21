//! `envelope.*` ops. Their constructors and argument contracts are in
//! `nickel/rdx/ops/envelope.ncl`.
//!
//! `redact`, `verify` and `encrypt` are pure: envelopes travel through plans as `ur:envelope/…`
//! strings. `seal`, `open` and `add-recipients` use this device's key (`rdx-keys`), unlocked with
//! its passkey through the `secrets` capability, so they run only where the supporter is: the PWA
//! or the local host. A sealed statement is kept encrypted, as `ur:envelope/…` text in the blob
//! store, to every key the supporter's registry lists.

use bc_components::PublicKeys;
use rdx_host::{BlobStore, Capability, Host};
use rdx_keys::ops::{recipients, this_device};
use rdx_keys::{host_rng, public_keys_from_ur, signing_key_to_ur};
use rdx_model::{Match, Statement};
use rdx_plan::{Op, OpError, Registry, RunContext};
use serde_json::{Value, json};

use crate::{from_ur, public_key_from_ur, redact, root_digest, seal, to_ur, verify};

/// Adds the `envelope.*` ops to a host's registry.
pub fn register(r: &mut Registry) -> &mut Registry {
    r.register(Redact)
        .register(Verify)
        .register(Seal)
        .register(Encrypt)
        .register(Open)
        .register(AddRecipients)
}

fn str_arg<'a>(op: &str, args: &'a Value, name: &str) -> Result<&'a str, OpError> {
    args.get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| OpError::new(format!("{op} needs a string `{name}`")))
}

/// `envelope` is a `ur:envelope/…` string, or the output of an earlier `envelope.*` step (a record
/// with an `envelope` field), so plans can chain steps with a plain `ref`.
fn envelope_arg(op: &str, args: &Value) -> Result<bc_envelope::Envelope, OpError> {
    let value = args.get("envelope");
    let ur = value
        .and_then(Value::as_str)
        .or_else(|| value?.get("envelope")?.as_str())
        .ok_or_else(|| {
            OpError::new(format!(
                "{op} needs `envelope`: a `ur:envelope/…` string, or the output of an earlier \
                 `envelope.*` step"
            ))
        })?;
    from_ur(ur).map_err(failed(op))
}

fn keys_failed(op: &str) -> impl Fn(rdx_keys::Error) -> OpError + '_ {
    move |e| OpError::new(format!("{op}: {e}"))
}

fn rng(host: &mut Host) -> Result<rdx_keys::Drbg, OpError> {
    Ok(host_rng(
        host.random.as_deref_mut().expect("checked by needs()"),
    )?)
}

fn store(host: &mut Host) -> &mut dyn BlobStore {
    host.blob_store.as_deref_mut().expect("checked by needs()")
}

/// An encrypted envelope kept in the blob store as `ur:envelope/…` text.
fn get_envelope(op: &str, host: &mut Host, id: &str) -> Result<bc_envelope::Envelope, OpError> {
    let bytes = store(host)
        .get(id)?
        .ok_or_else(|| OpError::new(format!("{op}: no `{id}` in the blob store")))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| OpError::new(format!("{op}: `{id}` is not an envelope")))?;
    from_ur(&text).map_err(failed(op))
}

fn put_envelope(host: &mut Host, envelope: &bc_envelope::Envelope) -> Result<String, OpError> {
    Ok(store(host).put(to_ur(envelope).as_bytes())?)
}

/// `recipients` is a list of `ur:crypto-pubkeys/…`, or the output of a `keys.recipients` step.
fn recipients_arg(op: &str, args: &Value) -> Result<Vec<PublicKeys>, OpError> {
    let value = args.get("recipients");
    let list = value
        .filter(|v| v.is_array())
        .or_else(|| value?.get("recipients"))
        .and_then(Value::as_array)
        .ok_or_else(|| {
            OpError::new(format!(
                "{op} needs `recipients`: a list of `ur:crypto-pubkeys/…`, or the output of a \
                 `keys.recipients` step"
            ))
        })?;
    list.iter()
        .map(|v| {
            let ur = v.as_str().ok_or_else(|| {
                OpError::new(format!(
                    "{op}: `recipients` holds something that isn't a UR"
                ))
            })?;
            public_keys_from_ur(ur).map_err(keys_failed(op))
        })
        .collect()
}

fn failed(op: &str) -> impl Fn(crate::Error) -> OpError + '_ {
    move |e| OpError::new(format!("{op}: {e}"))
}

/// `envelope.redact { envelope, matches }`: elides the lines confirmed matches don't fully cover.
struct Redact;

impl Op for Redact {
    fn id(&self) -> &'static str {
        "envelope.redact"
    }

    fn run(&self, args: &Value, _: &mut RunContext<'_>) -> Result<Value, OpError> {
        let id = self.id();
        let envelope = envelope_arg(id, args)?;
        let matches: Vec<Match> =
            serde_json::from_value(args.get("matches").cloned().unwrap_or(json!([])))
                .map_err(|e| OpError::new(format!("{id}: `matches`: {e}")))?;
        let r = redact(&envelope, &matches).map_err(failed(id))?;
        Ok(json!({
            "envelope": to_ur(&r.envelope),
            "root": root_digest(&r.envelope),
            "shown": r.shown,
            "redacted": r.redacted,
            "review": r.review,
        }))
    }
}

/// `envelope.verify { envelope, signer }`: checks the signature and reads the viewable statement.
struct Verify;

impl Op for Verify {
    fn id(&self) -> &'static str {
        "envelope.verify"
    }

    fn run(&self, args: &Value, _: &mut RunContext<'_>) -> Result<Value, OpError> {
        let id = self.id();
        let envelope = envelope_arg(id, args)?;
        let signer = public_key_from_ur(str_arg(id, args, "signer")?).map_err(failed(id))?;
        let view = verify(&envelope, &signer).map_err(failed(id))?;
        serde_json::to_value(view).map_err(|e| OpError::new(format!("{id}: {e}")))
    }
}

/// `envelope.seal { statement, address }`: signs a statement with this device's key, encrypts it
/// to every key in the address's registry and keeps it in the blob store.
struct Seal;

impl Op for Seal {
    fn id(&self) -> &'static str {
        "envelope.seal"
    }

    fn effects(&self) -> bool {
        true
    }

    fn needs(&self) -> &'static [Capability] {
        &[
            Capability::Kv,
            Capability::Secrets,
            Capability::Random,
            Capability::BlobStore,
        ]
    }

    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let id = self.id();
        let address = str_arg(id, args, "address")?;
        // A statement, or the output of a `statement.parse` step.
        let value = args.get("statement").cloned().unwrap_or(Value::Null);
        let value = value.get("statement").cloned().unwrap_or(value);
        let statement: Statement = serde_json::from_value(value).map_err(|e| {
            OpError::new(format!(
                "{id}: `statement` must be a statement or the output of `statement.parse`: {e}"
            ))
        })?;
        let host = &mut *cx.host;
        let to = recipients(id, host, address)?;
        let (device, keys) = this_device(id, host, address)?;
        let mut rng = rng(host)?;
        let signed = seal(&statement, &keys.signing_key(), &mut rng).map_err(failed(id))?;
        let encrypted = rdx_keys::encrypt(&signed, &to, &mut rng).map_err(keys_failed(id))?;
        let blob = put_envelope(host, &encrypted)?;
        Ok(json!({
            "blob": blob,
            "root": root_digest(&signed),
            "statement": statement.id,
            "device": device,
            "signer": signing_key_to_ur(&keys.signing_public()),
            "recipients": to.len(),
        }))
    }
}

/// `envelope.encrypt { envelope, recipients }`: encrypts an envelope to public keys. Servers can
/// run it: it needs no private key.
struct Encrypt;

impl Op for Encrypt {
    fn id(&self) -> &'static str {
        "envelope.encrypt"
    }

    fn needs(&self) -> &'static [Capability] {
        &[Capability::Random]
    }

    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let id = self.id();
        let envelope = envelope_arg(id, args)?;
        let to = recipients_arg(id, args)?;
        let encrypted =
            rdx_keys::encrypt(&envelope, &to, &mut rng(cx.host)?).map_err(keys_failed(id))?;
        Ok(json!({ "envelope": to_ur(&encrypted), "recipients": to.len() }))
    }
}

/// `envelope.open { blob, address }`: decrypts a kept envelope with this device's key. Its
/// output is the signed statement in the clear, so it runs only on the supporter's device.
struct Open;

impl Op for Open {
    fn id(&self) -> &'static str {
        "envelope.open"
    }

    fn needs(&self) -> &'static [Capability] {
        &[Capability::Kv, Capability::Secrets, Capability::BlobStore]
    }

    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let id = self.id();
        let address = str_arg(id, args, "address")?;
        let blob = str_arg(id, args, "blob")?;
        let host = &mut *cx.host;
        let encrypted = get_envelope(id, host, blob)?;
        let (_, keys) = this_device(id, host, address)?;
        let signed = rdx_keys::decrypt(&encrypted, &keys).map_err(keys_failed(id))?;
        Ok(json!({ "envelope": to_ur(&signed), "root": root_digest(&signed) }))
    }
}

/// `envelope.add-recipients { blob, address, recipients }`: lets more keys read a kept envelope
/// (a newly added or rotated device), without re-encrypting its content. Stores the result as a
/// new blob; the old one stays until a retention sweep removes it.
struct AddRecipients;

impl Op for AddRecipients {
    fn id(&self) -> &'static str {
        "envelope.add-recipients"
    }

    fn effects(&self) -> bool {
        true
    }

    fn needs(&self) -> &'static [Capability] {
        &[
            Capability::Kv,
            Capability::Secrets,
            Capability::Random,
            Capability::BlobStore,
        ]
    }

    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let id = self.id();
        let address = str_arg(id, args, "address")?;
        let blob = str_arg(id, args, "blob")?;
        let new = recipients_arg(id, args)?;
        let host = &mut *cx.host;
        let encrypted = get_envelope(id, host, blob)?;
        let (_, keys) = this_device(id, host, address)?;
        let more = rdx_keys::add_recipients(&encrypted, &keys, &new, &mut rng(host)?)
            .map_err(keys_failed(id))?;
        let to = put_envelope(host, &more)?;
        Ok(json!({ "from": blob, "to": to, "added": new.len() }))
    }
}
