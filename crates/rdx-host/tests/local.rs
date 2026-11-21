//! The local host's directory-backed capabilities.
#![cfg(feature = "local")]

use rdx_host::{BlobStore, Capability, Kv, MailOut, Message, Random, local};

#[test]
fn local_host_offers_everything_and_persists_in_its_state_dir() {
    let dir = tempfile::tempdir().unwrap();
    let id = {
        let mut host = local::host(dir.path()).unwrap();
        assert!(Capability::ALL.iter().all(|c| host.offers(*c)));
        host.kv
            .as_mut()
            .unwrap()
            .put("supporters/ash", b"{}")
            .unwrap();
        host.audit_log.as_mut().unwrap().append("entry").unwrap();
        host.blob_store.as_mut().unwrap().put(b"cipher").unwrap()
    };
    let host = local::host(dir.path()).unwrap();
    assert_eq!(
        host.blob_store
            .as_ref()
            .unwrap()
            .get(&id)
            .unwrap()
            .as_deref(),
        Some(&b"cipher"[..])
    );
    assert_eq!(
        host.kv
            .as_ref()
            .unwrap()
            .get("supporters/ash")
            .unwrap()
            .as_deref(),
        Some(&b"{}"[..])
    );
    assert_eq!(
        host.audit_log.as_ref().unwrap().last().unwrap().as_deref(),
        Some("entry")
    );
}

#[test]
fn blob_ids_that_are_not_digests_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let store = local::DirBlobStore::new(dir.path().join("blobs")).unwrap();
    for bad in ["../../etc/passwd", "sha256:../x", "sha256:ABC", "md5:00"] {
        assert!(store.get(bad).is_err(), "{bad}");
    }
}

#[test]
fn kv_keys_never_escape_the_directory() {
    let dir = tempfile::tempdir().unwrap();
    let mut kv = local::DirKv::new(dir.path().join("kv")).unwrap();
    kv.put("../escape", b"x").unwrap();
    assert!(!dir.path().join("escape").exists());
    assert_eq!(kv.get("../escape").unwrap().as_deref(), Some(&b"x"[..]));
    kv.delete("../escape").unwrap();
    assert_eq!(kv.get("../escape").unwrap(), None);
    kv.delete("never-set").unwrap();
}

#[test]
fn mail_is_spooled_to_the_outbox_not_sent() {
    let dir = tempfile::tempdir().unwrap();
    let mut out = local::OutboxDir::new(dir.path().join("outbox")).unwrap();
    let msg = Message {
        to: "a@example.org".into(),
        subject: "s".into(),
        text: "t".into(),
    };
    out.send(&msg).unwrap();
    out.send(&msg).unwrap();
    let files: Vec<_> = std::fs::read_dir(dir.path().join("outbox"))
        .unwrap()
        .collect();
    assert_eq!(files.len(), 2);
    let text = std::fs::read_to_string(files[0].as_ref().unwrap().path()).unwrap();
    let spooled: Message = serde_json::from_str(&text).unwrap();
    assert_eq!(spooled, msg);
}

#[test]
fn os_random_fills() {
    let mut r = local::OsRandom;
    let mut a = [0u8; 32];
    r.fill(&mut a).unwrap();
    assert_ne!(a, [0u8; 32]);
}

#[test]
fn dir_secrets_keep_their_passkey_and_files() {
    use rdx_host::Secrets;
    let dir = tempfile::tempdir().unwrap();
    let mut s = local::DirSecrets::new(dir.path().join("secrets")).unwrap();
    let prf = s.prf(b"salt").unwrap();
    assert_eq!(prf.len(), 32);
    let mut again = local::DirSecrets::new(dir.path().join("secrets")).unwrap();
    assert_eq!(
        again.prf(b"salt").unwrap(),
        prf,
        "the passkey file persists"
    );

    s.show("recovery-share-1", "ur:sskr/x").unwrap();
    let shown = dir.path().join("secrets/shown/recovery-share-1.txt");
    assert_eq!(std::fs::read_to_string(shown).unwrap(), "ur:sskr/x");

    assert_eq!(s.ask("recovery-share-1").unwrap(), None);
    std::fs::write(
        dir.path().join("secrets/answers/recovery-share-1.txt"),
        "ur:sskr/y\n",
    )
    .unwrap();
    assert_eq!(
        s.ask("recovery-share-1").unwrap().as_deref(),
        Some("ur:sskr/y")
    );
    assert!(s.show("../escape", "x").is_err(), "labels can't name paths");
}
