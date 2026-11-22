//! Claiming a quarantine over HTTP (`rdx_intake::claim`): what the server accepts and refuses.
//! The page's side runs in rdx-claim's tests; here the client is played by hand. Synthetic only.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use rdx_host::memory::{FixedClock, SeededRandom};
use rdx_host::{BlobStore, Host, Kv};
use rdx_intake::claim::{self, Opened};
use rdx_intake::{Allow, Error, Outcome, Policy, parse_claim_link, receive};
use rdx_keys::{Keys, Registry, Seed};

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
const MINUTE: u64 = 60 * 1000;
const BEN: &str = "ben@example.org";

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

fn email(from: &str, n: u32) -> Vec<u8> {
    format!(
        "Authentication-Results: mx.cloudflare.net; dmarc=pass header.from=example.org\r\n\
         From: <{from}>\r\n\
         To: receipts@intake.example.org\r\n\
         Subject: Receipt {n}\r\n\
         Message-ID: <synthetic-{n}@example.org>\r\n\
         \r\n\
         Total: MXN {n}00.00\r\n"
    )
    .into_bytes()
}

struct Server {
    host: Host,
    blobs: Store,
    kv: Store,
}

impl Server {
    fn new() -> Self {
        let (kv, blobs) = (Store::default(), Store::default());
        let host = Host::new()
            .with_kv(kv.clone())
            .with_blob_store(blobs.clone())
            .with_clock(FixedClock(T0))
            .with_random(SeededRandom::new(1));
        Self { host, blobs, kv }
    }

    fn at(&mut self, now: u64) {
        self.host.clock = Some(Box::new(FixedClock(now)));
    }

    /// Ben's mail arrives; returns the link's key.
    fn quarantine(&mut self, emails: u32) -> [u8; 32] {
        let mut secret = None;
        for n in 1..=emails {
            let r = receive(&mut self.host, &policy(), &email(BEN, n)).unwrap();
            assert!(matches!(r.outcome, Outcome::Quarantined { .. }));
            if let Some(reply) = r.reply {
                secret = Some(parse_claim_link(&reply.message.text).unwrap().1);
            }
        }
        secret.unwrap()
    }

    /// `open`, with the envelopes the Worker adds from R2.
    fn open(&self, request: &str) -> Result<Opened, Error> {
        let mut opened = claim::open(&self.host, request)?;
        opened.envelopes = opened
            .documents
            .iter()
            .map(|d| {
                String::from_utf8(BlobStore::get(&self.blobs, &d.id).unwrap().unwrap()).unwrap()
            })
            .collect();
        Ok(opened)
    }
}

/// What the page makes: a registry for `address` and the documents resealed to it.
fn client_release(
    opened: &Opened,
    address: &str,
    secret: &[u8; 32],
    at: u64,
    seed: u64,
) -> (claim::Release, Keys) {
    let mut random = SeededRandom::new(seed);
    let mut rng = rdx_keys::host_rng(&mut random).unwrap();
    let device = Keys::generate(&mut rng);
    let recovery = Seed::generate(&mut rng).recovery_keys();
    let registry =
        Registry::genesis(address, "ben", &recovery, "phone", &device.public(), at).unwrap();
    let recipients = registry.state().unwrap().recipients();
    let resealed = claim::reseal(opened, secret, &recipients, &mut random).unwrap();
    (
        claim::release_request(address, secret, at, &registry, &resealed),
        device,
    )
}

#[test]
fn the_link_s_key_opens_the_quarantine_and_releases_it_to_the_new_keys() {
    let mut s = Server::new();
    let secret = s.quarantine(2);
    let now = T0 + 60 * MINUTE;
    s.at(now);

    let opened = s.open(&claim::open_request(BEN, &secret, now)).unwrap();
    assert_eq!(opened.address, BEN);
    assert_eq!(opened.documents.len(), 2);
    let quarantined: Vec<String> = opened.documents.iter().map(|d| d.id.clone()).collect();

    let (release, device) = client_release(&opened, BEN, &secret, now, 1);
    let moved = claim::accept(&mut s.host, &release).unwrap();
    assert_eq!(moved.len(), 2);

    for id in &quarantined {
        assert!(
            BlobStore::get(&s.blobs, id).unwrap().is_none(),
            "quarantined copy deleted"
        );
    }
    assert!(
        Kv::get(&s.kv, &rdx_intake::quarantine_key(BEN))
            .unwrap()
            .is_none()
    );
    assert!(
        Kv::get(&s.kv, rdx_intake::QUARANTINE_INDEX)
            .unwrap()
            .is_none()
    );
    let inbox = rdx_intake::inbox(&s.host, BEN).unwrap();
    assert_eq!(inbox.len(), 2);
    for (entry, n) in inbox.iter().zip(1..) {
        let ur = String::from_utf8(BlobStore::get(&s.blobs, &entry.id).unwrap().unwrap()).unwrap();
        let sealed = rdx_keys::envelope_from_ur(&ur).unwrap();
        let opened = rdx_keys::decrypt(&sealed, &device).unwrap();
        assert_eq!(rdx_intake::message_bytes(&opened).unwrap(), email(BEN, n));
        assert!(rdx_keys::decrypt(&sealed, &Keys::from_secret(&secret)).is_err());
    }

    // The registry is published, so Ben's next email goes straight to his keys.
    let next = receive(&mut s.host, &policy(), &email(BEN, 3)).unwrap();
    assert!(matches!(next.outcome, Outcome::Delivered { .. }));

    // Replaying the release finds nothing left to claim.
    assert!(matches!(
        claim::accept(&mut s.host, &release),
        Err(Error::NothingQuarantined(_))
    ));
}

#[test]
fn requests_not_signed_by_the_current_link_are_refused() {
    let mut s = Server::new();
    s.quarantine(1);
    let forged = claim::open_request(BEN, &[9; 32], T0);
    assert!(matches!(s.open(&forged), Err(Error::WrongClaim(_))));
    assert!(matches!(
        s.open(&claim::open_request("cara@example.org", &[9; 32], T0)),
        Err(Error::NothingQuarantined(_))
    ));
    assert!(s.open("ur:envelope/nonsense").is_err());
}

#[test]
fn stale_requests_and_expired_quarantines_are_refused() {
    let mut s = Server::new();
    let secret = s.quarantine(1);
    s.at(T0 + 30 * MINUTE);
    let old = claim::open_request(BEN, &secret, T0);
    assert!(matches!(s.open(&old), Err(Error::ClaimLink(m)) if m.contains("too old")));

    let later = T0 + 7 * 24 * 60 * MINUTE;
    s.at(later);
    assert!(matches!(
        s.open(&claim::open_request(BEN, &secret, later)),
        Err(Error::NothingQuarantined(_))
    ));
}

#[test]
fn an_open_request_cannot_release_and_a_release_must_match_what_waits() {
    let mut s = Server::new();
    let secret = s.quarantine(1);
    let opened = s.open(&claim::open_request(BEN, &secret, T0)).unwrap();

    let not_release = claim::Release {
        request: claim::open_request(BEN, &secret, T0),
        envelopes: vec![],
    };
    assert!(claim::accept(&mut s.host, &not_release).is_err());

    // A registry for someone else's address, signed with Ben's link, is refused.
    let (other, _) = client_release(&opened, "cara@example.org", &secret, T0, 2);
    let (ben, _) = client_release(&opened, BEN, &secret, T0, 3);
    let swapped = claim::Release {
        request: {
            // Ben's request, but carrying Cara's registry: rebuild it signed by Ben's link.
            let mut random = SeededRandom::new(5);
            let mut rng = rdx_keys::host_rng(&mut random).unwrap();
            let device = Keys::generate(&mut rng);
            let recovery = Seed::generate(&mut rng).recovery_keys();
            let cara = Registry::genesis(
                "cara@example.org",
                "c",
                &recovery,
                "p",
                &device.public(),
                T0,
            )
            .unwrap();
            let resealed = claim::reseal(
                &opened,
                &secret,
                &cara.state().unwrap().recipients(),
                &mut random,
            )
            .unwrap();
            claim::release_request(BEN, &secret, T0, &cara, &resealed).request
        },
        envelopes: other.envelopes.clone(),
    };
    assert!(matches!(
        claim::accept(&mut s.host, &swapped),
        Err(Error::ClaimLink(m)) if m.contains("registry is for")
    ));

    // Envelopes that aren't the ones the signature names are refused.
    let tampered = claim::Release {
        request: ben.request.clone(),
        envelopes: other.envelopes.clone(),
    };
    assert!(claim::accept(&mut s.host, &tampered).is_err());

    // Mail that arrived after the page opened the quarantine must be claimed too.
    let r = receive(&mut s.host, &policy(), &email(BEN, 2)).unwrap();
    assert!(matches!(r.outcome, Outcome::Quarantined { .. }));
    assert!(matches!(
        claim::accept(&mut s.host, &ben),
        Err(Error::ClaimLink(m)) if m.contains("reload")
    ));
    assert_eq!(rdx_intake::inbox(&s.host, BEN).unwrap().len(), 0);
}

#[test]
fn an_address_that_already_has_keys_cannot_be_claimed_again() {
    let mut s = Server::new();
    let secret = s.quarantine(1);
    let opened = s.open(&claim::open_request(BEN, &secret, T0)).unwrap();
    let (release, _) = client_release(&opened, BEN, &secret, T0, 4);
    // Someone registered keys for Ben meanwhile (another device, recovery).
    let (existing, _) = client_release(&opened, BEN, &secret, T0, 5);
    let registry_json = {
        let content = rdx_keys::envelope_from_ur(&existing.request)
            .unwrap()
            .try_unwrap()
            .unwrap();
        content
            .extract_object_for_predicate::<String>("registry")
            .unwrap()
    };
    Kv::put(
        &mut s.kv.clone(),
        &format!("keys/registry/{BEN}"),
        registry_json.as_bytes(),
    )
    .unwrap();
    assert!(matches!(
        claim::accept(&mut s.host, &release),
        Err(Error::ClaimLink(m)) if m.contains("already has keys")
    ));
}

#[test]
fn the_kv_key_lists_cover_what_open_and_accept_touch() {
    assert_eq!(claim::open_keys(BEN), vec![rdx_intake::quarantine_key(BEN)]);
    let keys = claim::release_keys(BEN);
    assert!(keys.contains(&format!("keys/registry/{BEN}")));
    assert!(keys.contains(&rdx_intake::inbox_key(BEN)));
    assert!(keys.contains(&rdx_intake::QUARANTINE_INDEX.to_owned()));
    assert_eq!(
        claim::request_address(&claim::open_request(BEN, &[1; 32], T0)).unwrap(),
        BEN
    );
}
