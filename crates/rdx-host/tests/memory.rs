//! The in-memory host: what tests and `cargo xtask check` run plans against.

use rdx_host::{
    AuditLog, BlobStore, Capability, Clock, Host, Kv, MailOut, Message, Random,
    memory::{FixedClock, MemoryAuditLog, MemoryBlobStore, MemoryKv, Outbox, SeededRandom},
};

#[test]
fn blobs_are_addressed_by_their_sha256() {
    let mut store = MemoryBlobStore::default();
    let id = store.put(b"ciphertext").unwrap();
    // echo -n ciphertext | sha256sum
    assert_eq!(
        id,
        "sha256:305531dcc50ebca31cf1d5b31e9fc76ed51f66b3b6dd5a030c6539ae6532f979"
    );
    assert_eq!(id, rdx_host::sha256_digest(b"ciphertext"));
    assert_eq!(store.put(b"ciphertext").unwrap(), id, "puts are idempotent");
    assert_eq!(store.get(&id).unwrap().as_deref(), Some(&b"ciphertext"[..]));
    store.delete(&id).unwrap();
    assert_eq!(store.get(&id).unwrap(), None);
}

#[test]
fn kv_puts_gets_and_deletes() {
    let mut kv = MemoryKv::default();
    assert_eq!(kv.get("a").unwrap(), None);
    kv.put("a", b"1").unwrap();
    assert_eq!(kv.get("a").unwrap().as_deref(), Some(&b"1"[..]));
    kv.delete("a").unwrap();
    assert_eq!(kv.get("a").unwrap(), None);
}

#[test]
fn handles_share_state_so_tests_can_inspect_a_host() {
    let kv = MemoryKv::default();
    let mut host = Host::new().with_kv(kv.clone());
    host.kv.as_mut().unwrap().put("k", b"v").unwrap();
    assert_eq!(kv.get("k").unwrap().as_deref(), Some(&b"v"[..]));
}

#[test]
fn outbox_keeps_sent_mail() {
    let outbox = Outbox::default();
    let mut mail = outbox.clone();
    let msg = Message {
        to: "treasurer@example.org".into(),
        subject: "hi".into(),
        text: "body".into(),
    };
    mail.send(&msg).unwrap();
    assert_eq!(outbox.sent(), vec![msg]);
}

#[test]
fn seeded_random_is_deterministic() {
    let mut a = SeededRandom::new(7);
    let mut b = SeededRandom::new(7);
    let (mut x, mut y, mut z) = ([0u8; 40], [0u8; 40], [0u8; 40]);
    a.fill(&mut x).unwrap();
    b.fill(&mut y).unwrap();
    a.fill(&mut z).unwrap();
    assert_eq!(x, y);
    assert_ne!(x, z, "successive fills differ");
}

#[test]
fn fixed_clock_does_not_move() {
    let clock = FixedClock(1_763_510_400_000);
    assert_eq!(clock.now_unix_ms().unwrap(), 1_763_510_400_000);
}

#[test]
fn audit_log_appends_lines() {
    let log = MemoryAuditLog::default();
    let mut writer = log.clone();
    assert_eq!(writer.last().unwrap(), None);
    writer.append("one").unwrap();
    writer.append("two").unwrap();
    assert_eq!(writer.last().unwrap().as_deref(), Some("two"));
    assert_eq!(log.lines(), vec!["one", "two"]);
}

#[test]
fn hosts_say_what_they_offer() {
    let empty = Host::new();
    assert!(Capability::ALL.iter().all(|c| !empty.offers(*c)));
    assert!(empty.audit_log.is_none());

    let memory = Host::memory();
    assert!(Capability::ALL.iter().all(|c| memory.offers(*c)));
    assert!(memory.audit_log.is_some());

    let partial = Host::new().with_clock(FixedClock(0));
    assert_eq!(partial.offered(), vec![Capability::Clock]);
}

#[test]
fn capabilities_use_their_wit_names() {
    let names: Vec<String> = Capability::ALL.iter().map(|c| c.to_string()).collect();
    assert_eq!(names, ["blob-store", "kv", "mail-out", "clock", "random"]);
    assert_eq!(
        serde_json::to_value(Capability::MailOut).unwrap(),
        "mail-out"
    );
}
