//! `keys.*` ops. Their constructors and argument contracts are in `nickel/rdx/ops/keys.ncl`.
//!
//! Secrets never pass through a plan: passkey PRF output, the seed and recovery shares go between
//! the op and the person through the host's `secrets` capability, so they appear in no argument,
//! output or audit entry. Outputs hold public keys (`ur:crypto-pubkeys/…`), ids and labels only.
//!
//! What the ops keep in `kv`:
//!
//! ```text
//! keys/registry/<address>   the address's signed registry (public keys only; servers hold it too)
//! keys/device/<address>     this device's key for the address, wrapped by its passkey
//! ```

use bc_components::PublicKeys;
use rdx_host::{Capability, Host};
use rdx_plan::{Op, OpError, Registry as Ops, RunContext};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::registry::{Authority, RECOVERY, State};
use crate::{
    Keys, PRF_SALT, Registry, Seed, add_recipients, envelope_from_ur, envelope_to_ur, host_rng,
    public_keys_from_ur, public_keys_to_ur, unwrap, wrap,
};

/// Adds the `keys.*` ops to a host's registry.
pub fn register(r: &mut Ops) -> &mut Ops {
    r.register(Onboard)
        .register(CreateDevice)
        .register(AddDevice)
        .register(Rotate)
        .register(Revoke)
        .register(Recover)
        .register(Recipients)
}

/// Most recovery shares `keys.recover` asks for before giving up.
const MAX_SHARES: usize = 16;

fn registry_key(address: &str) -> String {
    format!("keys/registry/{address}")
}

fn device_key(address: &str) -> String {
    format!("keys/device/{address}")
}

/// This device's key for an address, as kept in `kv`: ciphertext only.
#[derive(Serialize, Deserialize)]
struct DeviceRecord {
    device: String,
    /// `ur:envelope/…`: the device's keys, encrypted under its passkey.
    wrapped: String,
}

fn err(op: &str) -> impl Fn(crate::Error) -> OpError + '_ {
    move |e| OpError::new(format!("{op}: {e}"))
}

fn str_arg<'a>(op: &str, args: &'a Value, name: &str) -> Result<&'a str, OpError> {
    args.get(name)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| OpError::new(format!("{op} needs a non-empty string `{name}`")))
}

fn strings(op: &str, args: &Value, name: &str) -> Result<Vec<String>, OpError> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(v) => serde_json::from_value(v.clone())
            .map_err(|_| OpError::new(format!("{op}: `{name}` must be a list of strings"))),
    }
}

/// An address names kv keys, so it can't hold whitespace or `/`.
fn address_arg<'a>(op: &str, args: &'a Value) -> Result<&'a str, OpError> {
    let a = str_arg(op, args, "address")?;
    if a.chars().any(|c| c.is_whitespace() || c == '/') {
        return Err(OpError::new(format!(
            "{op}: `address` `{a}` can't contain spaces or `/`"
        )));
    }
    Ok(a)
}

fn device_arg<'a>(op: &str, args: &'a Value) -> Result<&'a str, OpError> {
    let d = str_arg(op, args, "device")?;
    if d == RECOVERY {
        return Err(OpError::new(format!(
            "{op}: `{RECOVERY}` names the recovery key; pick another device name"
        )));
    }
    Ok(d)
}

fn kv_get(host: &Host, key: &str) -> Result<Option<Vec<u8>>, OpError> {
    Ok(host.kv.as_ref().expect("checked by needs()").get(key)?)
}

fn kv_put(host: &mut Host, key: &str, value: &impl Serialize) -> Result<(), OpError> {
    let bytes = serde_json::to_vec(value).expect("records serialize");
    Ok(host
        .kv
        .as_mut()
        .expect("checked by needs()")
        .put(key, &bytes)?)
}

fn now(host: &Host) -> Result<u64, OpError> {
    Ok(host
        .clock
        .as_ref()
        .expect("checked by needs()")
        .now_unix_ms()?)
}

fn rng(host: &mut Host) -> Result<crate::Drbg, OpError> {
    Ok(host_rng(
        host.random.as_deref_mut().expect("checked by needs()"),
    )?)
}

fn prf(host: &mut Host) -> Result<Vec<u8>, OpError> {
    Ok(host
        .secrets
        .as_mut()
        .expect("checked by needs()")
        .prf(PRF_SALT)?)
}

/// The registry for an address, if there is one. Callers check it with [`Registry::state`].
pub fn load_registry(host: &Host, address: &str) -> Result<Option<Registry>, OpError> {
    kv_get(host, &registry_key(address))?
        .map(|b| {
            serde_json::from_slice(&b).map_err(|e| {
                OpError::new(format!(
                    "the key registry for `{address}` is unreadable: {e}"
                ))
            })
        })
        .transpose()
}

fn registry_state(op: &str, host: &Host, address: &str) -> Result<(Registry, State), OpError> {
    let registry = load_registry(host, address)?.ok_or_else(|| {
        OpError::new(format!(
            "{op}: `{address}` has no keys yet; onboard it first (`onboard-supporter`)"
        ))
    })?;
    let state = registry.state().map_err(err(op))?;
    Ok((registry, state))
}

/// The keys a new document for `address` is encrypted to: every active device and the recovery
/// key, from a registry whose every signature checked out. Needs `kv`.
pub fn recipients(op: &str, host: &Host, address: &str) -> Result<Vec<PublicKeys>, OpError> {
    Ok(registry_state(op, host, address)?.1.recipients())
}

/// This device's id and keys for `address`, unlocked with its passkey. Needs `kv` and `secrets`.
pub fn this_device(op: &str, host: &mut Host, address: &str) -> Result<(String, Keys), OpError> {
    let record: DeviceRecord = kv_get(host, &device_key(address))?
        .map(|b| serde_json::from_slice(&b))
        .transpose()
        .map_err(|e| OpError::new(format!("{op}: this device's key record is unreadable: {e}")))?
        .ok_or_else(|| {
            OpError::new(format!(
                "{op}: this device holds no key for `{address}`: onboard, create a device key, \
                 or recover access"
            ))
        })?;
    let wrapped = envelope_from_ur(&record.wrapped).map_err(err(op))?;
    let keys = unwrap(&wrapped, &prf(host)?).map_err(err(op))?;
    Ok((record.device, keys))
}

/// Generates keys for this device, wraps them with its passkey and keeps them in `kv`.
fn new_device_keys(
    op: &str,
    host: &mut Host,
    address: &str,
    device: &str,
) -> Result<Keys, OpError> {
    let mut rng = rng(host)?;
    let keys = Keys::generate(&mut rng);
    let wrapped = wrap(&keys, &prf(host)?, &mut rng).map_err(err(op))?;
    kv_put(
        host,
        &device_key(address),
        &DeviceRecord {
            device: device.to_owned(),
            wrapped: envelope_to_ur(&wrapped),
        },
    )?;
    Ok(keys)
}

fn urs(keys: &[PublicKeys]) -> Vec<String> {
    keys.iter().map(public_keys_to_ur).collect()
}

fn devices_json(state: &State) -> Value {
    Value::Array(
        state
            .devices
            .values()
            .map(|d| {
                let mut v = json!({
                    "device": d.id,
                    "keys": public_keys_to_ur(&d.keys),
                    "added_at": d.added_at,
                    "rotations": d.rotations,
                });
                let status = serde_json::to_value(&d.status).expect("statuses serialize");
                v.as_object_mut()
                    .unwrap()
                    .extend(status.as_object().unwrap().clone());
                v
            })
            .collect(),
    )
}

/// `keys.onboard`: a new supporter's first device, recovery key, shares and registry.
struct Onboard;

impl Op for Onboard {
    fn id(&self) -> &'static str {
        "keys.onboard"
    }

    fn effects(&self) -> bool {
        true
    }

    fn needs(&self) -> &'static [Capability] {
        &[
            Capability::Kv,
            Capability::Random,
            Capability::Secrets,
            Capability::Clock,
        ]
    }

    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let op = self.id();
        let address = address_arg(op, args)?;
        let supporter = str_arg(op, args, "supporter")?;
        let device = device_arg(op, args)?;
        let holders = strings(op, args, "holders")?;
        let threshold = args
            .get("threshold")
            .and_then(Value::as_u64)
            .ok_or_else(|| OpError::new(format!("{op} needs a number `threshold`")))?
            as usize;
        if !(1..=holders.len()).contains(&threshold) || holders.len() > MAX_SHARES {
            return Err(OpError::new(format!(
                "{op}: `threshold` must be between 1 and the number of `holders` ({}), with at \
                 most {MAX_SHARES} holders",
                holders.len()
            )));
        }
        let host = &mut *cx.host;
        if load_registry(host, address)?.is_some() {
            return Err(OpError::new(format!(
                "{op}: `{address}` already has keys; add a device or recover access instead"
            )));
        }

        let keys = new_device_keys(op, host, address, device)?;
        let mut rng = rng(host)?;
        let seed = Seed::generate(&mut rng);
        let recovery = seed.recovery_keys();
        let shares = seed
            .split(threshold, holders.len(), &mut rng)
            .map_err(err(op))?;
        drop(seed);
        let registry = Registry::genesis(
            address,
            supporter,
            &recovery,
            device,
            &keys.public(),
            now(host)?,
        )
        .map_err(err(op))?;
        kv_put(host, &registry_key(address), &registry)?;

        let secrets = host.secrets.as_mut().expect("checked by needs()");
        let mut shown = Vec::new();
        for (n, (share, holder)) in shares.iter().zip(&holders).enumerate() {
            let label = format!("recovery-share-{}", n + 1);
            secrets.show(&label, share)?;
            shown.push(json!({ "label": label, "holder": holder }));
        }
        cx.notes.push(format!(
            "{op}: give each recovery share to its holder; any {threshold} of {} recover access",
            holders.len()
        ));
        Ok(json!({
            "address": address,
            "supporter": supporter,
            "device": device,
            "keys": public_keys_to_ur(&keys.public()),
            "recovery": public_keys_to_ur(&recovery.public()),
            "threshold": threshold,
            "shares": shown,
        }))
    }
}

/// `keys.create-device`: keys for a second device, waiting for an active device to add them.
struct CreateDevice;

impl Op for CreateDevice {
    fn id(&self) -> &'static str {
        "keys.create-device"
    }

    fn effects(&self) -> bool {
        true
    }

    fn needs(&self) -> &'static [Capability] {
        &[Capability::Kv, Capability::Random, Capability::Secrets]
    }

    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let op = self.id();
        let address = address_arg(op, args)?;
        let device = device_arg(op, args)?;
        let host = &mut *cx.host;
        if kv_get(host, &device_key(address))?.is_some() {
            return Err(OpError::new(format!(
                "{op}: this device already holds a key for `{address}`; rotate it instead"
            )));
        }
        let keys = new_device_keys(op, host, address, device)?;
        cx.notes.push(format!(
            "{op}: on a device that is already active, run `keys.add-device` with these keys"
        ));
        Ok(json!({
            "address": address,
            "device": device,
            "keys": public_keys_to_ur(&keys.public()),
        }))
    }
}

/// `keys.add-device`: this (active) device enrolls another device's keys.
struct AddDevice;

impl Op for AddDevice {
    fn id(&self) -> &'static str {
        "keys.add-device"
    }

    fn effects(&self) -> bool {
        true
    }

    fn needs(&self) -> &'static [Capability] {
        &[Capability::Kv, Capability::Secrets, Capability::Clock]
    }

    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let op = self.id();
        let address = address_arg(op, args)?;
        let device = device_arg(op, args)?;
        let new = public_keys_from_ur(str_arg(op, args, "keys")?).map_err(err(op))?;
        let host = &mut *cx.host;
        let (mut registry, _) = registry_state(op, host, address)?;
        let (me, keys) = this_device(op, host, address)?;
        registry
            .add(device, &new, &Authority::Device(&me, &keys), now(host)?)
            .map_err(err(op))?;
        kv_put(host, &registry_key(address), &registry)?;
        cx.notes.push(format!(
            "{op}: documents encrypted before now don't include `{device}` yet; use \
             `envelope.add-recipients` to let it read them"
        ));
        let state = registry.state().map_err(err(op))?;
        Ok(json!({
            "address": address,
            "device": device,
            "added_by": me,
            "recipients": urs(&state.recipients()),
        }))
    }
}

/// `keys.rotate`: new keys for this device, signed over by its current key.
struct Rotate;

impl Op for Rotate {
    fn id(&self) -> &'static str {
        "keys.rotate"
    }

    fn effects(&self) -> bool {
        true
    }

    fn needs(&self) -> &'static [Capability] {
        &[
            Capability::Kv,
            Capability::Random,
            Capability::Secrets,
            Capability::Clock,
        ]
    }

    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let op = self.id();
        let address = address_arg(op, args)?;
        let host = &mut *cx.host;
        let (mut registry, _) = registry_state(op, host, address)?;
        let (me, old) = this_device(op, host, address)?;
        let mut rng = rng(host)?;
        let new = Keys::generate(&mut rng);
        registry
            .rotate(
                &me,
                &new.public(),
                &Authority::Device(&me, &old),
                now(host)?,
            )
            .map_err(err(op))?;
        // The registry first: if the device record were written first and this failed, the
        // device would hold keys the registry doesn't know.
        kv_put(host, &registry_key(address), &registry)?;
        let wrapped = wrap(&new, &prf(host)?, &mut rng).map_err(err(op))?;
        kv_put(
            host,
            &device_key(address),
            &DeviceRecord {
                device: me.clone(),
                wrapped: envelope_to_ur(&wrapped),
            },
        )?;
        cx.notes.push(format!(
            "{op}: documents encrypted to the old key still open with the recovery key; \
             `envelope.add-recipients` lets the new key read them"
        ));
        Ok(json!({
            "address": address,
            "device": me,
            "keys": public_keys_to_ur(&new.public()),
        }))
    }
}

/// `keys.revoke`: this device retires another (or itself).
struct Revoke;

impl Op for Revoke {
    fn id(&self) -> &'static str {
        "keys.revoke"
    }

    fn effects(&self) -> bool {
        true
    }

    fn needs(&self) -> &'static [Capability] {
        &[Capability::Kv, Capability::Secrets, Capability::Clock]
    }

    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let op = self.id();
        let address = address_arg(op, args)?;
        let device = device_arg(op, args)?;
        let reason = str_arg(op, args, "reason")?;
        let host = &mut *cx.host;
        let (mut registry, _) = registry_state(op, host, address)?;
        let (me, keys) = this_device(op, host, address)?;
        registry
            .revoke(device, reason, &Authority::Device(&me, &keys), now(host)?)
            .map_err(err(op))?;
        kv_put(host, &registry_key(address), &registry)?;
        let state = registry.state().map_err(err(op))?;
        Ok(json!({
            "address": address,
            "device": device,
            "revoked_by": me,
            "recipients": urs(&state.recipients()),
        }))
    }
}

/// `keys.recover`: recovery shares from the person enroll this device, revoke lost ones and let
/// the new key read existing documents.
struct Recover;

impl Op for Recover {
    fn id(&self) -> &'static str {
        "keys.recover"
    }

    fn effects(&self) -> bool {
        true
    }

    fn needs(&self) -> &'static [Capability] {
        &[
            Capability::Kv,
            Capability::Random,
            Capability::Secrets,
            Capability::Clock,
            Capability::BlobStore,
        ]
    }

    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let op = self.id();
        let address = address_arg(op, args)?;
        let device = device_arg(op, args)?;
        let revoke = strings(op, args, "revoke")?;
        let documents = strings(op, args, "documents")?;
        let host = &mut *cx.host;
        let (mut registry, state) = registry_state(op, host, address)?;

        // Ask for shares one at a time until they recover the seed this registry names.
        let mut shares = Vec::new();
        let mut recovery = None;
        for n in 1..=MAX_SHARES {
            let label = format!("recovery-share-{n}");
            let asked = host
                .secrets
                .as_mut()
                .expect("checked by needs()")
                .ask(&label)?;
            let Some(share) = asked else { break };
            shares.push(share);
            if let Ok(seed) = Seed::combine(&shares) {
                let keys = seed.recovery_keys();
                if keys.public() == state.recovery {
                    recovery = Some(keys);
                    break;
                }
            }
        }
        let recovery = recovery.ok_or_else(|| {
            OpError::new(format!(
                "{op}: the {} recovery share(s) given don't recover `{address}`'s keys: too \
                 few, mistyped, or from another supporter",
                shares.len()
            ))
        })?;

        let by = Authority::Recovery(&recovery);
        let at = now(host)?;
        let keys = Keys::generate(&mut rng(host)?);
        registry
            .add(device, &keys.public(), &by, at)
            .map_err(err(op))?;
        for lost in &revoke {
            registry.revoke(lost, "lost", &by, at).map_err(err(op))?;
        }
        kv_put(host, &registry_key(address), &registry)?;
        let mut rng = rng(host)?;
        let wrapped = wrap(&keys, &prf(host)?, &mut rng).map_err(err(op))?;
        kv_put(
            host,
            &device_key(address),
            &DeviceRecord {
                device: device.to_owned(),
                wrapped: envelope_to_ur(&wrapped),
            },
        )?;

        // Every document was also encrypted to the recovery key, which can let the new key in.
        let store = host.blob_store.as_mut().expect("checked by needs()");
        let mut moved = Vec::new();
        for id in &documents {
            let bytes = store.get(id)?.ok_or_else(|| {
                OpError::new(format!("{op}: no document `{id}` in the blob store"))
            })?;
            let ur = String::from_utf8(bytes)
                .map_err(|_| OpError::new(format!("{op}: `{id}` is not an encrypted envelope")))?;
            let envelope = envelope_from_ur(&ur).map_err(err(op))?;
            let more = add_recipients(&envelope, &recovery, &[keys.public()], &mut rng)
                .map_err(|e| OpError::new(format!("{op}: `{id}`: {e}")))?;
            let to = store.put(envelope_to_ur(&more).as_bytes())?;
            moved.push(json!({ "from": id, "to": to }));
        }
        drop(recovery);

        let state = registry.state().map_err(err(op))?;
        Ok(json!({
            "address": address,
            "device": device,
            "keys": public_keys_to_ur(&keys.public()),
            "revoked": revoke,
            "documents": moved,
            "recipients": urs(&state.recipients()),
        }))
    }
}

/// `keys.recipients`: the keys to encrypt to for an address, from its checked registry.
struct Recipients;

impl Op for Recipients {
    fn id(&self) -> &'static str {
        "keys.recipients"
    }

    fn needs(&self) -> &'static [Capability] {
        &[Capability::Kv]
    }

    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let op = self.id();
        let address = address_arg(op, args)?;
        let (_, state) = registry_state(op, cx.host, address)?;
        let active = state.devices.values().filter(|d| d.is_active()).count();
        if active == 0 {
            cx.notes.push(format!(
                "{op}: `{address}` has no active device; only the recovery key can read new \
                 documents until one is recovered"
            ));
        }
        Ok(json!({
            "address": address,
            "supporter": state.supporter,
            "recipients": urs(&state.recipients()),
            "recovery": public_keys_to_ur(&state.recovery),
            "devices": devices_json(&state),
        }))
    }
}
