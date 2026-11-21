//! Synchronous stores in front of asynchronous ones: `kv` preloaded with the keys a call may
//! read, and a blob store that only records. The Worker reads before calling the intake core and
//! writes what changed after.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use rdx_host::{BlobStore, CapabilityError, Kv, Result};

/// `kv` with every key a call may touch loaded up front (`None`: absent). Reading a key that
/// wasn't loaded is an error, so a core change that reads more keys fails loudly in tests instead
/// of silently seeing nothing in production.
#[derive(Debug, Clone, Default)]
pub struct StagedKv(Rc<RefCell<KvState>>);

#[derive(Debug, Default)]
struct KvState {
    values: BTreeMap<String, Option<Vec<u8>>>,
    changed: BTreeMap<String, Option<Vec<u8>>>,
}

impl StagedKv {
    /// Records what the backing store holds for `key`.
    pub fn load(&self, key: &str, value: Option<Vec<u8>>) {
        self.0.borrow_mut().values.insert(key.to_owned(), value);
    }

    /// Writes to apply: `Some` puts, `None` deletes. Keys written back unchanged are left out.
    pub fn changes(&self) -> Vec<(String, Option<Vec<u8>>)> {
        self.0
            .borrow()
            .changed
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    fn set(&self, key: &str, value: Option<Vec<u8>>) -> Result<()> {
        let mut s = self.0.borrow_mut();
        let Some(original) = s.values.get(key).cloned() else {
            return Err(not_loaded(key));
        };
        if original == value {
            s.changed.remove(key);
        } else {
            s.changed.insert(key.to_owned(), value);
        }
        Ok(())
    }
}

fn not_loaded(key: &str) -> CapabilityError {
    CapabilityError::new(format!(
        "kv key `{key}` wasn't loaded before the call; add it to rdx_intake's key lists"
    ))
}

impl Kv for StagedKv {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let s = self.0.borrow();
        if let Some(v) = s.changed.get(key) {
            return Ok(v.clone());
        }
        s.values.get(key).cloned().ok_or_else(|| not_loaded(key))
    }

    fn put(&mut self, key: &str, value: &[u8]) -> Result<()> {
        self.set(key, Some(value.to_vec()))
    }

    fn delete(&mut self, key: &str) -> Result<()> {
        self.set(key, None)
    }
}

/// A blob store that records puts and deletes. Intake on the Worker never reads a blob.
#[derive(Debug, Clone, Default)]
pub struct StagedBlobs(Rc<RefCell<BlobState>>);

#[derive(Debug, Default)]
struct BlobState {
    puts: BTreeMap<String, Vec<u8>>,
    deletes: Vec<String>,
}

impl StagedBlobs {
    /// New blobs, by id.
    pub fn puts(&self) -> Vec<(String, Vec<u8>)> {
        self.0
            .borrow()
            .puts
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Blobs to delete once the kv writes that stop listing them are done.
    pub fn deletes(&self) -> Vec<String> {
        self.0.borrow().deletes.clone()
    }
}

impl BlobStore for StagedBlobs {
    fn put(&mut self, bytes: &[u8]) -> Result<String> {
        let id = rdx_host::sha256_digest(bytes);
        let mut s = self.0.borrow_mut();
        s.deletes.retain(|d| d != &id);
        s.puts.insert(id.clone(), bytes.to_vec());
        Ok(id)
    }

    fn get(&self, id: &str) -> Result<Option<Vec<u8>>> {
        match self.0.borrow().puts.get(id) {
            Some(b) => Ok(Some(b.clone())),
            None => Err(CapabilityError::new(format!(
                "blob `{id}`: the Worker doesn't read blobs during intake"
            ))),
        }
    }

    fn delete(&mut self, id: &str) -> Result<()> {
        let mut s = self.0.borrow_mut();
        if s.puts.remove(id).is_none() && !s.deletes.iter().any(|d| d == id) {
            s.deletes.push(id.to_owned());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use rdx_host::memory::{FixedClock, SeededRandom};
    use rdx_host::{Host, Kv};
    use rdx_intake::{Allow, Outcome, Policy};

    use super::*;

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
            max_bytes: 1 << 20,
        }
    }

    const RAW: &[u8] = b"Authentication-Results: mx.cloudflare.net; dmarc=pass header.from=example.org\r\n\
        From: ben@example.org\r\nMessage-ID: <m1@example.org>\r\nSubject: bill\r\n\r\nTotal 10.00\r\n";

    /// What the Worker does, against a map standing in for Workers KV.
    fn run(
        backing: &mut BTreeMap<String, Vec<u8>>,
        r2: &mut BTreeMap<String, Vec<u8>>,
        now: u64,
    ) -> rdx_intake::Received {
        let kv = StagedKv::default();
        let address = rdx_intake::sender(RAW).unwrap();
        for k in rdx_intake::receive_keys(&address) {
            kv.load(&k, backing.get(&k).cloned());
        }
        let blobs = StagedBlobs::default();
        let mut host = Host::new()
            .with_kv(kv.clone())
            .with_blob_store(blobs.clone())
            .with_clock(FixedClock(now))
            .with_random(SeededRandom::new(1));
        let got = rdx_intake::receive(&mut host, &policy(), RAW).unwrap();
        apply(backing, r2, &kv, &blobs);
        got
    }

    fn apply(
        backing: &mut BTreeMap<String, Vec<u8>>,
        r2: &mut BTreeMap<String, Vec<u8>>,
        kv: &StagedKv,
        blobs: &StagedBlobs,
    ) {
        r2.extend(blobs.puts());
        for (k, v) in kv.changes() {
            match v {
                Some(v) => backing.insert(k, v),
                None => backing.remove(&k),
            };
        }
        for d in blobs.deletes() {
            r2.remove(&d);
        }
    }

    #[test]
    fn intake_runs_against_staged_stores_and_its_writes_apply_afterwards() {
        let (mut backing, mut r2) = (BTreeMap::new(), BTreeMap::new());
        let t0 = FixedClock::GOLDEN;
        let first = run(&mut backing, &mut r2, t0);
        assert!(matches!(first.outcome, Outcome::Quarantined { .. }));
        assert!(first.reply.is_some());
        assert_eq!(r2.len(), 1);
        assert!(backing.contains_key("intake/quarantine/ben@example.org"));

        // The second email sees the first one's writes.
        let second = run(&mut backing, &mut r2, t0 + 1000);
        assert!(second.reply.is_none());
        assert_eq!(r2.len(), 2);

        // The cron sweep, after expiry, with the keys sweep_keys names.
        let kv = StagedKv::default();
        let index = backing.get(rdx_intake::QUARANTINE_INDEX).cloned();
        for k in rdx_intake::sweep_keys(index.as_deref()).unwrap() {
            kv.load(&k, backing.get(&k).cloned());
        }
        let blobs = StagedBlobs::default();
        let mut host = Host::new()
            .with_kv(kv.clone())
            .with_blob_store(blobs.clone())
            .with_clock(FixedClock(t0 + 8 * 86_400_000));
        let swept = rdx_intake::sweep(&mut host, &policy()).unwrap();
        assert_eq!(swept.notices.len(), 1);
        apply(&mut backing, &mut r2, &kv, &blobs);
        assert!(r2.is_empty(), "expired documents deleted from R2");
        assert!(!backing.contains_key("intake/quarantine/ben@example.org"));
        assert!(!backing.contains_key(rdx_intake::QUARANTINE_INDEX));
    }

    #[test]
    fn reading_a_key_that_was_not_loaded_fails() {
        let mut kv = StagedKv::default();
        assert!(kv.get("intake/inbox/x").is_err());
        assert!(kv.put("intake/inbox/x", b"1").is_err());
        kv.load("a", None);
        kv.put("a", b"1").unwrap();
        kv.delete("a").unwrap();
        assert!(
            kv.changes().is_empty(),
            "back to how it was: nothing to write"
        );
    }

    #[test]
    fn a_blob_put_then_deleted_is_never_written() {
        let mut b = StagedBlobs::default();
        let id = b.put(b"x").unwrap();
        b.delete(&id).unwrap();
        b.delete("sha256:old").unwrap();
        assert!(b.puts().is_empty());
        assert_eq!(b.deletes(), vec!["sha256:old".to_owned()]);
    }
}
