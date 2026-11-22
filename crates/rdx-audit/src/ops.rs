//! `audit.*` ops. Their constructors and argument contracts are in `nickel/rdx/ops/audit.ncl`.
//!
//! `keep`, `grant` and `revoke` sign custody events with this device's key, unlocked by its
//! passkey through `secrets`, so only the supporter's device can run them. `open-original` runs on
//! the grantee's device with theirs. `trail` decrypts nothing and runs anywhere.
//!
//! What the ops keep:
//!
//! ```text
//! kv   audit/custody/<statement id>   the statement's custody record (signed events)
//! blob <encrypted original>           rdx.original.v1, encrypted to the supporter's keys
//! blob <grant copies>                 the sealed statement and the original, with the grantee added
//! ```

use bc_components::PublicKeys;
use rdx_host::{BlobStore, Capability, Host};
use rdx_keys::ops::{recipients, this_device};
use rdx_keys::{Keys, envelope_from_ur, envelope_to_ur, host_rng, public_keys_from_ur};
use rdx_plan::{Op, OpError, Registry, RunContext};
use serde::Serialize;
use serde_json::{Value, json};

use crate::custody::{By, Custody, Grant, Kept};
use crate::original;

/// Adds the `audit.*` ops to a host's registry.
pub fn register(r: &mut Registry) -> &mut Registry {
    r.register(Keep)
        .register(GrantOp)
        .register(Revoke)
        .register(OpenOriginal)
        .register(crate::trail::Trail)
}

pub(crate) fn custody_key(statement: &str) -> String {
    format!("audit/custody/{statement}")
}

pub(crate) fn str_arg<'a>(op: &str, args: &'a Value, name: &str) -> Result<&'a str, OpError> {
    args.get(name)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| OpError::new(format!("{op} needs a non-empty string `{name}`")))
}

/// Statement ids and addresses name kv keys, so they can't hold whitespace or `/`.
fn name_arg<'a>(op: &str, args: &'a Value, name: &str) -> Result<&'a str, OpError> {
    let v = str_arg(op, args, name)?;
    if v.chars().any(|c| c.is_whitespace() || c == '/') {
        return Err(OpError::new(format!(
            "{op}: `{name}` `{v}` can't contain spaces or `/`"
        )));
    }
    Ok(v)
}

fn store(host: &mut Host) -> &mut dyn BlobStore {
    host.blob_store.as_deref_mut().expect("checked by needs()")
}

fn now(host: &Host) -> Result<u64, OpError> {
    Ok(host
        .clock
        .as_ref()
        .expect("checked by needs()")
        .now_unix_ms()?)
}

fn rng(host: &mut Host) -> Result<rdx_keys::Drbg, OpError> {
    Ok(host_rng(
        host.random.as_deref_mut().expect("checked by needs()"),
    )?)
}

/// The custody record for a statement, if it has one.
pub fn load_custody(host: &Host, statement: &str) -> Result<Option<Custody>, OpError> {
    host.kv
        .as_ref()
        .expect("checked by needs()")
        .get(&custody_key(statement))?
        .map(|b| {
            serde_json::from_slice(&b).map_err(|e| {
                OpError::new(format!(
                    "the custody record for `{statement}` is unreadable: {e}"
                ))
            })
        })
        .transpose()
}

fn save_custody(host: &mut Host, custody: &Custody) -> Result<(), OpError> {
    let bytes = serde_json::to_vec(custody).expect("records serialize");
    Ok(host
        .kv
        .as_mut()
        .expect("checked by needs()")
        .put(&custody_key(&custody.statement), &bytes)?)
}

fn existing_custody(op: &str, host: &Host, statement: &str) -> Result<Custody, OpError> {
    load_custody(host, statement)?.ok_or_else(|| {
        OpError::new(format!(
            "{op}: `{statement}` has no custody record; keep it first (`keep-statement`)"
        ))
    })
}

fn custody_failed(op: &str) -> impl Fn(crate::custody::Error) -> OpError + '_ {
    move |e| OpError::new(format!("{op}: {e}"))
}

fn keys_failed(op: &str) -> impl Fn(rdx_keys::Error) -> OpError + '_ {
    move |e| OpError::new(format!("{op}: {e}"))
}

fn get_envelope(op: &str, host: &mut Host, id: &str) -> Result<bc_envelope::Envelope, OpError> {
    let bytes = store(host)
        .get(id)?
        .ok_or_else(|| OpError::new(format!("{op}: no `{id}` in the blob store")))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| OpError::new(format!("{op}: `{id}` is not an envelope")))?;
    envelope_from_ur(&text).map_err(keys_failed(op))
}

/// `recipients`/`to` is a list of `ur:crypto-pubkeys/…`, or the output of a `keys.recipients`
/// step.
fn keys_arg(op: &str, args: &Value, name: &str) -> Result<Vec<PublicKeys>, OpError> {
    let value = args.get(name).cloned().unwrap_or(Value::Null);
    let list = value.get("recipients").cloned().unwrap_or(value);
    let urs: Vec<String> = serde_json::from_value(list).map_err(|_| {
        OpError::new(format!(
            "{op}: `{name}` must be a list of `ur:crypto-pubkeys/…` or a `keys.recipients` output"
        ))
    })?;
    if urs.is_empty() {
        return Err(OpError::new(format!("{op}: `{name}` names no keys")));
    }
    urs.iter()
        .map(|u| public_keys_from_ur(u).map_err(keys_failed(op)))
        .collect()
}

/// `audit.keep { sealed, address }`: encrypts the statement's original, records custody, deletes
/// the plaintext original.
struct Keep;

impl Op for Keep {
    fn id(&self) -> &'static str {
        "audit.keep"
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
            Capability::Clock,
        ]
    }

    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let op = self.id();
        let address = name_arg(op, args, "address")?;
        let sealed = args.get("sealed").cloned().unwrap_or(Value::Null);
        let field = |name: &str| -> Result<String, OpError> {
            sealed
                .get(name)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| {
                    OpError::new(format!(
                        "{op}: `sealed` must be an `envelope.seal` output, with `{name}`"
                    ))
                })
        };
        let blob = field("blob")?;
        let root = field("root")?;
        let signer = field("signer")?;
        let host = &mut *cx.host;

        // Open the sealed statement here, so custody never records an envelope that doesn't open
        // and verify, and read the original's digest from what the supporter signed.
        let encrypted = get_envelope(op, host, &blob)?;
        let (device, keys) = this_device(op, host, address)?;
        let signed = rdx_keys::decrypt(&encrypted, &keys).map_err(keys_failed(op))?;
        let key = rdx_envelope::public_key_from_ur(&signer)
            .map_err(|e| OpError::new(format!("{op}: {e}")))?;
        let view =
            rdx_envelope::verify(&signed, &key).map_err(|e| OpError::new(format!("{op}: {e}")))?;
        if view.root.as_str() != root {
            return Err(OpError::new(format!(
                "{op}: the sealed envelope's root is `{}`, not the `{root}` given",
                view.root.as_str()
            )));
        }
        let statement = view.statement.as_str().to_owned();
        if load_custody(host, &statement)?.is_some() {
            return Err(OpError::new(format!(
                "{op}: `{statement}` is already kept; a corrected statement needs a new id"
            )));
        }
        let original = view
            .original
            .as_ref()
            .map(|d| d.as_str().to_owned())
            .ok_or_else(|| {
                OpError::new(format!(
                    "{op}: `{statement}` names no original document; ingest it from a document in \
                 the blob store"
                ))
            })?;
        let bytes = store(host).get(&original)?.ok_or_else(|| {
            OpError::new(format!(
                "{op}: the original `{original}` is not in the blob store; put the bank's file \
                 there (locally, `rdx blob put FILE`) and keep the statement again"
            ))
        })?;

        let to = recipients(op, host, address)?;
        let enc = rdx_keys::encrypt(&original::wrap(&bytes, &statement), &to, &mut rng(host)?)
            .map_err(keys_failed(op))?;
        let source = store(host).put(envelope_to_ur(&enc).as_bytes())?;
        let kept = Kept {
            root,
            sealed: blob,
            original: original.clone(),
            source,
            signer,
        };
        let custody = Custody::kept(
            &statement,
            address,
            &kept,
            &By {
                device: &device,
                keys: &keys,
            },
            now(host)?,
        )
        .map_err(custody_failed(op))?;
        // Custody first: if deleting failed, the plaintext would linger but nothing would be lost.
        save_custody(host, &custody)?;
        store(host).delete(&original)?;
        cx.notes.push(format!(
            "{op}: the plaintext original `{original}` was deleted from the blob store; the \
             encrypted copy is `{}`",
            kept.source
        ));
        Ok(json!({
            "statement": statement,
            "address": address,
            "device": device,
            "root": kept.root,
            "sealed": kept.sealed,
            "original": kept.original,
            "source": kept.source,
            "recipients": to.len(),
        }))
    }
}

/// Adds keys to a copy of a kept envelope and stores the copy.
fn copy_to(
    op: &str,
    host: &mut Host,
    id: &str,
    keys: &Keys,
    to: &[PublicKeys],
) -> Result<String, OpError> {
    let encrypted = get_envelope(op, host, id)?;
    let more = rdx_keys::add_recipients(&encrypted, keys, to, &mut rng(host)?)
        .map_err(|e| OpError::new(format!("{op}: `{id}`: {e}")))?;
    Ok(store(host).put(envelope_to_ur(&more).as_bytes())?)
}

/// `audit.grant { statement, address, to, grantee, reason, original }`: lets the grantee's keys
/// open copies of a kept statement and its original.
struct GrantOp;

impl Op for GrantOp {
    fn id(&self) -> &'static str {
        "audit.grant"
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
            Capability::Clock,
        ]
    }

    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let op = self.id();
        let statement = name_arg(op, args, "statement")?;
        let address = name_arg(op, args, "address")?;
        let grantee = str_arg(op, args, "grantee")?;
        let reason = str_arg(op, args, "reason")?;
        let with_original = args
            .get("original")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let to = keys_arg(op, args, "to")?;
        let host = &mut *cx.host;
        let mut custody = existing_custody(op, host, statement)?;
        let state = custody.state().map_err(custody_failed(op))?;
        if state.address != address {
            return Err(OpError::new(format!(
                "{op}: `{statement}` belongs to `{}`; only that supporter can grant it",
                state.address
            )));
        }
        if state.active_grant(grantee).is_some() {
            return Err(OpError::new(format!(
                "{op}: `{grantee}` already has an active grant for `{statement}`; revoke it first"
            )));
        }
        let (device, keys) = this_device(op, host, address)?;
        let sealed = copy_to(op, host, &state.kept.sealed, &keys, &to)?;
        let source = if with_original {
            Some(copy_to(op, host, &state.kept.source, &keys, &to)?)
        } else {
            None
        };
        let grant = Grant {
            grantee: grantee.to_owned(),
            reason: reason.to_owned(),
            recipients: to.iter().map(rdx_keys::public_keys_to_ur).collect(),
            sealed,
            source,
        };
        custody
            .grant(
                &grant,
                &By {
                    device: &device,
                    keys: &keys,
                },
                now(host)?,
            )
            .map_err(custody_failed(op))?;
        save_custody(host, &custody)?;
        cx.notes.push(format!(
            "{op}: `{grantee}` opens the statement with `envelope.open` and the original with \
             `audit.open-original` on their own device, and checks the statement with `signer`"
        ));
        Ok(json!({
            "statement": statement,
            "grantee": grantee,
            "granted_by": device,
            "reason": reason,
            "sealed": grant.sealed,
            "source": grant.source,
            "root": state.kept.root,
            "original": state.kept.original,
            "signer": state.kept.signer,
            "recipients": to.len(),
        }))
    }
}

/// `audit.revoke { statement, address, grantee, reason }`: deletes a grantee's copies and records
/// that their grant ended.
struct Revoke;

impl Op for Revoke {
    fn id(&self) -> &'static str {
        "audit.revoke"
    }

    fn effects(&self) -> bool {
        true
    }

    fn needs(&self) -> &'static [Capability] {
        &[
            Capability::Kv,
            Capability::Secrets,
            Capability::BlobStore,
            Capability::Clock,
        ]
    }

    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let op = self.id();
        let statement = name_arg(op, args, "statement")?;
        let address = name_arg(op, args, "address")?;
        let grantee = str_arg(op, args, "grantee")?;
        let reason = str_arg(op, args, "reason")?;
        let host = &mut *cx.host;
        let mut custody = existing_custody(op, host, statement)?;
        let state = custody.state().map_err(custody_failed(op))?;
        if state.address != address {
            return Err(OpError::new(format!(
                "{op}: `{statement}` belongs to `{}`; only that supporter can revoke its grants",
                state.address
            )));
        }
        let grant = state.active_grant(grantee).cloned().ok_or_else(|| {
            OpError::new(format!(
                "{op}: `{grantee}` has no active grant for `{statement}`"
            ))
        })?;
        let (device, keys) = this_device(op, host, address)?;
        custody
            .revoke(
                grantee,
                reason,
                &By {
                    device: &device,
                    keys: &keys,
                },
                now(host)?,
            )
            .map_err(custody_failed(op))?;
        save_custody(host, &custody)?;
        let mut deleted = vec![grant.grant.sealed.clone()];
        deleted.extend(grant.grant.source.clone());
        for id in &deleted {
            store(host).delete(id)?;
        }
        cx.notes.push(format!(
            "{op}: the copies are deleted here; anything `{grantee}` already opened or saved \
             stays with them"
        ));
        Ok(json!({
            "statement": statement,
            "grantee": grantee,
            "revoked_by": device,
            "reason": reason,
            "deleted": deleted,
        }))
    }
}

#[derive(Serialize)]
struct Opened<'a> {
    document: &'a str,
    statement: &'a str,
    bytes: usize,
}

/// `audit.open-original { blob, address }`: decrypts a granted (or kept) original with this
/// device's key, checks it against its digest and puts the document in this device's blob store.
struct OpenOriginal;

impl Op for OpenOriginal {
    fn id(&self) -> &'static str {
        "audit.open-original"
    }

    // It writes the plaintext document to this device's blob store.
    fn effects(&self) -> bool {
        true
    }

    fn needs(&self) -> &'static [Capability] {
        &[Capability::Kv, Capability::Secrets, Capability::BlobStore]
    }

    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let op = self.id();
        let blob = str_arg(op, args, "blob")?;
        let address = name_arg(op, args, "address")?;
        let host = &mut *cx.host;
        let encrypted = get_envelope(op, host, blob)?;
        let (_, keys) = this_device(op, host, address)?;
        let opened = rdx_keys::decrypt(&encrypted, &keys).map_err(keys_failed(op))?;
        let doc = original::unwrap(&opened).map_err(|e| OpError::new(format!("{op}: {e}")))?;
        let id = store(host).put(&doc.bytes)?;
        // The blob store addresses by the same SHA-256, so this only fails on a broken store.
        if id != doc.digest {
            return Err(OpError::new(format!(
                "{op}: the blob store kept the document as `{id}`, not `{}`",
                doc.digest
            )));
        }
        cx.notes.push(format!(
            "{op}: compare `document` with the `original` the verified statement shows"
        ));
        Ok(serde_json::to_value(Opened {
            document: &doc.digest,
            statement: &doc.statement,
            bytes: doc.bytes.len(),
        })
        .expect("serializes"))
    }
}
