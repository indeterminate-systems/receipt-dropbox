//! The claim page's flow against the server side, as the browser and the Worker run it: link,
//! open, passkey, onboard, reseal, release. Synthetic only.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use rdx_claim::Claim;
use rdx_host::memory::{FixedClock, SeededRandom};
use rdx_host::{BlobStore, Host, Kv};
use rdx_intake::{Allow, Outcome, Policy, receive};

#[derive(Clone, Default)]
struct Store(Rc<RefCell<BTreeMap<String, Vec<u8>>>>);

impl BlobStore for Store {
    fn put(&mut self, bytes: &[u8]) -> rdx_host::Result<String> {
        let id = rdx_host::sha256_digest(bytes);
        self.0.borrow_mut().insert(id.clone(), bytes.to_vec());
        Ok(id)
    }
    fn get(&self, id: &str) -> rdx_host::Result<Option<Vec<u8>>> {
        Ok(self.0.borrow().get(id).cloned())
    }
    fn delete(&mut self, id: &str) -> rdx_host::Result<()> {
        self.0.borrow_mut().remove(id);
        Ok(())
    }
}

impl Kv for Store {
    fn get(&self, key: &str) -> rdx_host::Result<Option<Vec<u8>>> {
        Ok(self.0.borrow().get(key).cloned())
    }
    fn put(&mut self, key: &str, value: &[u8]) -> rdx_host::Result<()> {
        self.0.borrow_mut().insert(key.to_owned(), value.to_vec());
        Ok(())
    }
    fn delete(&mut self, key: &str) -> rdx_host::Result<()> {
        self.0.borrow_mut().remove(key);
        Ok(())
    }
}

const T0: u64 = FixedClock::GOLDEN;
const ADDRESS: &str = "Ana.Ruiz@Example.org";

fn email() -> Vec<u8> {
    format!(
        "Authentication-Results: mx.cloudflare.net; dmarc=pass header.from=example.org\r\n\
         From: Ana Ruiz <{ADDRESS}>\r\n\
         To: receipts@intake.example.org\r\n\
         Subject: Fwd: Your receipt from Lantern Catering\r\n\
         \r\n\
         Total: MXN 1,250.00\r\n"
    )
    .into_bytes()
}

fn policy() -> Policy {
    Policy {
        intake_address: "receipts@intake.example.org".into(),
        claim_url: "https://app.example.org/claim".into(),
        allow: Allow {
            addresses: vec![],
            domains: vec!["example.org".into()],
        },
        authserv_id: "mx.cloudflare.net".into(),
        quarantine_days: 7,
        reply_interval_days: 7,
        max_bytes: 1024 * 1024,
    }
}

#[test]
fn the_page_claims_a_quarantine_with_a_passkey_and_keeps_only_wrapped_keys() {
    let (kv, blobs) = (Store::default(), Store::default());
    let mut server = Host::new()
        .with_kv(kv.clone())
        .with_blob_store(blobs.clone())
        .with_clock(FixedClock(T0))
        .with_random(SeededRandom::new(1));
    let received = receive(&mut server, &policy(), &email()).unwrap();
    let link = received.reply.expect("a setup link").message.text;

    // The browser: the link's fragment, then POST /claim/open.
    let claim = Claim::parse(&link).unwrap();
    assert_eq!(claim.address, "ana.ruiz@example.org");
    let now = T0 + 5 * 60 * 1000;
    server.clock = Some(Box::new(FixedClock(now)));
    let mut opened = rdx_intake::claim::open(&server, &claim.open_request(now)).unwrap();
    opened.envelopes = opened
        .documents
        .iter()
        .map(|d| String::from_utf8(BlobStore::get(&blobs, &d.id).unwrap().unwrap()).unwrap())
        .collect();
    let wire = serde_json::to_string(&opened).unwrap();
    let opened = serde_json::from_str(&wire).unwrap();

    // The passkey's PRF output, then everything else happens in the page.
    let prf = vec![42u8; 32];
    let finished = claim
        .finish(&opened, prf.clone(), SeededRandom::new(77), now)
        .unwrap();
    assert!(finished.share.starts_with("ur:sskr/"));
    let state = finished.registry.state().unwrap();
    assert_eq!(state.supporter, "ana.ruiz.example.org");
    assert!(state.devices.contains_key(rdx_claim::DEVICE));

    // POST /claim/release.
    let wire = serde_json::to_string(&finished.release).unwrap();
    let moved =
        rdx_intake::claim::accept(&mut server, &serde_json::from_str(&wire).unwrap()).unwrap();
    assert_eq!(moved.len(), 1);

    // What the browser keeps is ciphertext; the passkey unlocks it, and it opens the mail.
    let record: serde_json::Value = serde_json::from_slice(&finished.device_record).unwrap();
    let wrapped = rdx_keys::envelope_from_ur(record["wrapped"].as_str().unwrap()).unwrap();
    assert!(rdx_keys::unwrap(&wrapped, &[1u8; 32]).is_err());
    let device = rdx_keys::unwrap(&wrapped, &prf).unwrap();
    let inbox = rdx_intake::inbox(&server, "ana.ruiz@example.org").unwrap();
    let ur = String::from_utf8(BlobStore::get(&blobs, &inbox[0].id).unwrap().unwrap()).unwrap();
    let mail = rdx_keys::decrypt(&rdx_keys::envelope_from_ur(&ur).unwrap(), &device).unwrap();
    assert_eq!(rdx_intake::message_bytes(&mail).unwrap(), email());

    // And the recovery share alone recovers the recovery key every document is encrypted to.
    let recovery = rdx_keys::Seed::combine(&[finished.share])
        .unwrap()
        .recovery_keys();
    assert!(rdx_keys::decrypt(&rdx_keys::envelope_from_ur(&ur).unwrap(), &recovery).is_ok());

    let next = receive(&mut server, &policy(), &email()).unwrap();
    assert!(matches!(next.outcome, Outcome::Delivered { .. }));
}

#[test]
fn a_link_without_a_key_is_explained() {
    let err = Claim::parse("https://app.example.org/claim#address=a%40example.org")
        .err()
        .unwrap();
    assert!(err.to_string().contains("no key"));
    assert_eq!(
        rdx_claim::supporter_id("Ana+Box@Example.org"),
        "ana.box.example.org"
    );
}
