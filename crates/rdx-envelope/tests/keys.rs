//! Sealing, opening and encrypting statements with a supporter's keys (`rdx-keys`), as plan steps.

use rdx_host::Host;
use rdx_host::memory::MemorySecrets;
use rdx_model::synth::{Config, Scenario};
use rdx_plan::{Plan, Registry, RunReport, StepStatus, execute};
use serde_json::{Value, json};

const ADDR: &str = "ana@example.org";

fn registry() -> Registry {
    let mut r = Registry::with_core();
    rdx_keys::register(&mut r);
    rdx_envelope::register(&mut r);
    r
}

fn run(host: &mut Host, steps: Value) -> RunReport {
    let plan: Plan = serde_json::from_value(json!({
        "version": 1, "skill": "t", "goal": "g", "dry_run": false, "steps": steps
    }))
    .unwrap();
    let report = execute(&plan, &registry(), host).unwrap();
    for s in &report.steps {
        assert_eq!(s.status, StepStatus::Done, "{}: {:?}", s.op, s.error);
    }
    report
}

fn out(report: &RunReport, id: &str) -> Value {
    report
        .steps
        .iter()
        .find(|s| s.id.as_deref() == Some(id))
        .and_then(|s| s.output.clone())
        .unwrap()
}

fn onboarded() -> Host {
    let mut host = Host::memory();
    run(
        &mut host,
        json!([{ "op": "keys.onboard", "args": {
            "address": ADDR, "supporter": "s-0001", "device": "laptop",
            "threshold": 2, "holders": ["phone", "card", "custodian"] } }]),
    );
    host
}

#[test]
fn a_sealed_statement_is_kept_encrypted_and_opens_verifies_and_redacts_on_the_device() {
    let s = Scenario::generate(&Config::small());
    let st = &s.statements[0];
    let mut host = onboarded();
    let report = run(
        &mut host,
        json!([
            { "id": "sealed", "op": "envelope.seal",
              "args": { "statement": { "statement": st }, "address": ADDR } },
        ]),
    );
    let sealed = out(&report, "sealed");
    assert_eq!(sealed["recipients"], 2, "the laptop and the recovery key");
    assert_eq!(sealed["device"], "laptop");
    let stored = rdx_host::BlobStore::get(
        host.blob_store.as_deref().unwrap(),
        sealed["blob"].as_str().unwrap(),
    )
    .unwrap()
    .unwrap();
    let text = String::from_utf8(stored).unwrap();
    assert!(!text.contains(st.lines[0].description.as_str()));

    let report = run(
        &mut host,
        json!([
            { "id": "opened", "op": "envelope.open",
              "args": { "blob": sealed["blob"], "address": ADDR } },
            { "id": "redacted", "op": "envelope.redact",
              "args": { "envelope": { "ref": "opened" }, "matches": s.matches } },
            { "id": "view", "op": "envelope.verify",
              "args": { "envelope": { "ref": "redacted" }, "signer": sealed["signer"] } },
        ]),
    );
    assert_eq!(out(&report, "opened")["root"], sealed["root"]);
    assert_eq!(out(&report, "view")["root"], sealed["root"]);
    assert_eq!(out(&report, "view")["statement"], json!(st.id));
}

#[test]
fn a_server_encrypts_to_the_registry_without_any_private_key() {
    let mut device = onboarded();
    // The server holds the registry (public keys) but no device key and no secrets.
    let mut server = Host::memory().with_kv(rdx_host::memory::MemoryKv::default());
    server.secrets = None;
    let key = format!("keys/registry/{ADDR}");
    let value = device.kv.as_ref().unwrap().get(&key).unwrap().unwrap();
    server.kv.as_mut().unwrap().put(&key, &value).unwrap();

    let doc = rdx_envelope::to_ur(&bc_envelope::Envelope::new("an emailed bill"));
    let report = run(
        &mut server,
        json!([
            { "id": "to", "op": "keys.recipients", "args": { "address": ADDR } },
            { "id": "enc", "op": "envelope.encrypt",
              "args": { "envelope": doc, "recipients": { "ref": "to" } } },
        ]),
    );
    let enc = out(&report, "enc");
    assert_eq!(enc["recipients"], 2);

    // The device can read it once it is in its blob store.
    let id = rdx_host::BlobStore::put(
        device.blob_store.as_deref_mut().unwrap(),
        enc["envelope"].as_str().unwrap().as_bytes(),
    )
    .unwrap();
    let report = run(
        &mut device,
        json!([{ "id": "o", "op": "envelope.open", "args": { "blob": id, "address": ADDR } }]),
    );
    assert_eq!(out(&report, "o")["envelope"], json!(doc));
}

#[test]
fn decrypting_needs_secrets_so_a_server_plan_fails_before_it_runs() {
    let mut server = Host::memory();
    server.secrets = None;
    let plan: Plan = serde_json::from_value(json!({
        "version": 1, "skill": "t", "goal": "g", "dry_run": false,
        "steps": [{ "op": "envelope.open", "args": { "blob": "sha256:00", "address": ADDR } }]
    }))
    .unwrap();
    let err = execute(&plan, &registry(), &mut server).unwrap_err();
    assert!(err.to_string().contains("secrets"), "{err}");
}

#[test]
fn a_new_device_is_let_into_a_kept_statement() {
    let s = Scenario::generate(&Config::small());
    let mut laptop = onboarded();
    let report = run(
        &mut laptop,
        json!([{ "id": "sealed", "op": "envelope.seal",
                 "args": { "statement": s.statements[0], "address": ADDR } }]),
    );
    let blob = out(&report, "sealed")["blob"].clone();

    // A phone (another passkey) creates its key; the laptop adds it and lets it in.
    let mut phone = Host::memory()
        .with_secrets(MemorySecrets::new([2; 32]))
        .with_random(rdx_host::memory::SeededRandom::new(2));
    let created = run(
        &mut phone,
        json!([{ "id": "c", "op": "keys.create-device", "args": { "address": ADDR, "device": "phone" } }]),
    );
    let keys = out(&created, "c")["keys"].clone();
    let report = run(
        &mut laptop,
        json!([
            { "op": "keys.add-device", "args": { "address": ADDR, "device": "phone", "keys": keys } },
            { "id": "more", "op": "envelope.add-recipients",
              "args": { "blob": blob, "address": ADDR, "recipients": [keys] } },
        ]),
    );
    let more = out(&report, "more");
    assert_eq!(more["from"], blob);

    // Move the new blob and the registry to the phone, which can now open it.
    let to = more["to"].as_str().unwrap();
    let bytes = rdx_host::BlobStore::get(laptop.blob_store.as_deref().unwrap(), to)
        .unwrap()
        .unwrap();
    rdx_host::BlobStore::put(phone.blob_store.as_deref_mut().unwrap(), &bytes).unwrap();
    let key = format!("keys/registry/{ADDR}");
    let reg = laptop.kv.as_ref().unwrap().get(&key).unwrap().unwrap();
    phone.kv.as_mut().unwrap().put(&key, &reg).unwrap();
    run(
        &mut phone,
        json!([{ "op": "envelope.open", "args": { "blob": to, "address": ADDR } }]),
    );
}
