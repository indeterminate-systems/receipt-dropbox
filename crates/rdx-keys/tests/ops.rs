//! `keys.*` as plan steps, on in-memory hosts standing in for a supporter's devices.

use bc_envelope::prelude::*;
use rdx_host::memory::{MemoryBlobStore, MemoryKv, MemorySecrets};
use rdx_host::{Host, Kv};
use rdx_keys::ops::{recipients, this_device};
use rdx_keys::{Drbg, decrypt, encrypt, envelope_from_ur, envelope_to_ur};
use rdx_plan::{Plan, Registry, RunReport, StepStatus, execute};
use serde_json::{Value, json};

const ADDR: &str = "ana@example.org";

fn registry() -> Registry {
    let mut r = Registry::with_core();
    rdx_keys::register(&mut r);
    r
}

/// A device: its own kv and passkey, a seeded generator and a fixed clock.
struct Device {
    host: Host,
    kv: MemoryKv,
    secrets: MemorySecrets,
    blobs: MemoryBlobStore,
}

fn device(passkey: u8) -> Device {
    let kv = MemoryKv::default();
    let secrets = MemorySecrets::new([passkey; 32]);
    let blobs = MemoryBlobStore::default();
    let host = Host::memory()
        .with_kv(kv.clone())
        .with_secrets(secrets.clone())
        .with_blob_store(blobs.clone())
        .with_random(rdx_host::memory::SeededRandom::new(passkey as u64));
    Device {
        host,
        kv,
        secrets,
        blobs,
    }
}

/// Copies the registry from one device to another, as syncing would.
fn sync(from: &Device, to: &mut Device) {
    let key = format!("keys/registry/{ADDR}");
    let value = from.kv.get(&key).unwrap().unwrap();
    to.kv.clone().put(&key, &value).unwrap();
}

fn run(d: &mut Device, op: &str, args: Value) -> RunReport {
    let plan: Plan = serde_json::from_value(json!({
        "version": 1, "skill": "t", "goal": "g", "dry_run": false,
        "steps": [{ "id": "s", "op": op, "args": args }]
    }))
    .unwrap();
    execute(&plan, &registry(), &mut d.host).unwrap()
}

fn ok(d: &mut Device, op: &str, args: Value) -> Value {
    let report = run(d, op, args);
    let step = &report.steps[0];
    assert_eq!(step.status, StepStatus::Done, "{op}: {:?}", step.error);
    assert_no_secrets(&report);
    step.output.clone().unwrap()
}

fn fails(d: &mut Device, op: &str, args: Value) -> String {
    let report = run(d, op, args);
    assert_eq!(report.steps[0].status, StepStatus::Failed);
    report.steps[0].error.clone().unwrap()
}

/// Shares and wrapped keys never reach a report (and so never the audit log).
fn assert_no_secrets(report: &RunReport) {
    let text = serde_json::to_string(report).unwrap();
    assert!(!text.contains("ur:sskr/"), "a share leaked: {text}");
}

fn onboard(laptop: &mut Device) -> Value {
    ok(
        laptop,
        "keys.onboard",
        json!({ "address": ADDR, "supporter": "s-0001", "device": "laptop", "threshold": 2,
                "holders": ["phone", "printed card", "org custodian"] }),
    )
}

#[test]
fn onboarding_shows_the_shares_to_the_person_not_the_report() {
    let mut laptop = device(1);
    let out = onboard(&mut laptop);
    assert_eq!(out["threshold"], 2);
    assert_eq!(
        out["shares"][2],
        json!({ "label": "recovery-share-3", "holder": "org custodian" })
    );
    let shown = laptop.secrets.shown();
    assert_eq!(shown.len(), 3);
    assert!(shown.iter().all(|(_, s)| s.starts_with("ur:sskr/")));

    let r = ok(&mut laptop, "keys.recipients", json!({ "address": ADDR }));
    assert_eq!(r["recipients"].as_array().unwrap().len(), 2);
    assert_eq!(r["recipients"][1], out["recovery"]);
    assert_eq!(r["devices"][0]["status"], "active");

    let again = fails(
        &mut laptop,
        "keys.onboard",
        json!({ "address": ADDR, "supporter": "s-0001", "device": "x", "threshold": 1, "holders": ["a"] }),
    );
    assert!(again.contains("already has keys"), "{again}");
}

#[test]
fn a_second_device_is_created_there_and_added_from_an_active_one() {
    let mut laptop = device(1);
    let mut phone = device(2);
    onboard(&mut laptop);
    let created = ok(
        &mut phone,
        "keys.create-device",
        json!({ "address": ADDR, "device": "phone" }),
    );
    let added = ok(
        &mut laptop,
        "keys.add-device",
        json!({ "address": ADDR, "device": "phone", "keys": created["keys"] }),
    );
    assert_eq!(added["added_by"], "laptop");
    assert_eq!(added["recipients"].as_array().unwrap().len(), 3);

    // The phone unlocks its own key with its own passkey, and can sign once it has the registry.
    sync(&laptop, &mut phone);
    let (id, _) = this_device("t", &mut phone.host, ADDR).unwrap();
    assert_eq!(id, "phone");
    ok(
        &mut phone,
        "keys.revoke",
        json!({ "address": ADDR, "device": "laptop", "reason": "sold" }),
    );
}

#[test]
fn another_passkey_cannot_use_this_devices_key() {
    let mut laptop = device(1);
    onboard(&mut laptop);
    let mut thief = Device {
        host: Host::memory()
            .with_kv(laptop.kv.clone())
            .with_secrets(MemorySecrets::new([66; 32])),
        kv: laptop.kv.clone(),
        secrets: MemorySecrets::new([66; 32]),
        blobs: MemoryBlobStore::default(),
    };
    let e = fails(
        &mut thief,
        "keys.revoke",
        json!({ "address": ADDR, "device": "laptop", "reason": "x" }),
    );
    assert!(e.contains("passkey"), "{e}");
}

#[test]
fn rotation_keeps_the_device_and_changes_its_keys() {
    let mut laptop = device(1);
    let first = onboard(&mut laptop);
    let rotated = ok(&mut laptop, "keys.rotate", json!({ "address": ADDR }));
    assert_eq!(rotated["device"], "laptop");
    assert_ne!(rotated["keys"], first["keys"]);
    let r = ok(&mut laptop, "keys.recipients", json!({ "address": ADDR }));
    assert_eq!(r["recipients"][0], rotated["keys"]);
    assert_eq!(r["devices"][0]["rotations"], 1);
}

#[test]
fn recovery_shares_enroll_a_new_device_revoke_the_lost_one_and_reopen_documents() {
    let mut laptop = device(1);
    onboard(&mut laptop);
    let shares = laptop.secrets.shown();

    // A document encrypted to the address while the laptop was alive.
    let doc = Envelope::new("statement st-2026-03");
    let to = recipients("t", &laptop.host, ADDR).unwrap();
    let encrypted = encrypt(&doc, &to, &mut Drbg::new("doc")).unwrap();

    // The laptop is lost. A new phone has the registry, the document and two shares.
    let mut phone = device(2);
    sync(&laptop, &mut phone);
    let id = phone.blobs.clone().put_ur(&envelope_to_ur(&encrypted));
    phone.secrets.answer("recovery-share-1", &shares[2].1);
    phone.secrets.answer("recovery-share-2", &shares[0].1);

    let out = ok(
        &mut phone,
        "keys.recover",
        json!({ "address": ADDR, "device": "phone", "revoke": ["laptop"], "documents": [id] }),
    );
    assert_eq!(out["revoked"], json!(["laptop"]));
    let r = ok(&mut phone, "keys.recipients", json!({ "address": ADDR }));
    assert_eq!(
        r["recipients"].as_array().unwrap().len(),
        2,
        "phone and recovery"
    );
    assert_eq!(r["devices"][0]["status"], "revoked");

    let (_, keys) = this_device("t", &mut phone.host, ADDR).unwrap();
    let new_id = out["documents"][0]["to"].as_str().unwrap();
    let bytes = rdx_host::BlobStore::get(&phone.blobs, new_id)
        .unwrap()
        .unwrap();
    let reopened = envelope_from_ur(std::str::from_utf8(&bytes).unwrap()).unwrap();
    assert!(decrypt(&reopened, &keys).unwrap().is_identical_to(&doc));
}

#[test]
fn too_few_or_wrong_shares_recover_nothing() {
    let mut laptop = device(1);
    onboard(&mut laptop);
    let shares = laptop.secrets.shown();
    let mut phone = device(2);
    sync(&laptop, &mut phone);
    phone.secrets.answer("recovery-share-1", &shares[0].1);
    let e = fails(
        &mut phone,
        "keys.recover",
        json!({ "address": ADDR, "device": "phone" }),
    );
    assert!(e.contains("don't recover"), "{e}");

    // Shares of someone else's seed.
    let mut other = device(3);
    onboard(&mut other);
    let theirs = other.secrets.shown();
    phone.secrets.answer("recovery-share-1", &theirs[0].1);
    phone.secrets.answer("recovery-share-2", &theirs[1].1);
    let e = fails(
        &mut phone,
        "keys.recover",
        json!({ "address": ADDR, "device": "phone" }),
    );
    assert!(e.contains("don't recover"), "{e}");
}

#[test]
fn errors_say_what_to_fix() {
    let mut d = device(1);
    let e = fails(&mut d, "keys.recipients", json!({ "address": ADDR }));
    assert!(e.contains("onboard it first"), "{e}");
    let e = fails(
        &mut d,
        "keys.onboard",
        json!({ "address": ADDR, "supporter": "s", "device": "laptop", "threshold": 4, "holders": ["a"] }),
    );
    assert!(e.contains("threshold"), "{e}");
    let e = fails(
        &mut d,
        "keys.onboard",
        json!({ "address": "a/b", "supporter": "s", "device": "laptop", "threshold": 1, "holders": ["a"] }),
    );
    assert!(e.contains("`/`"), "{e}");
    let e = fails(
        &mut d,
        "keys.create-device",
        json!({ "address": ADDR, "device": "recovery" }),
    );
    assert!(e.contains("recovery key"), "{e}");
}

trait PutUr {
    fn put_ur(&mut self, ur: &str) -> String;
}

impl PutUr for MemoryBlobStore {
    fn put_ur(&mut self, ur: &str) -> String {
        rdx_host::BlobStore::put(self, ur.as_bytes()).unwrap()
    }
}
