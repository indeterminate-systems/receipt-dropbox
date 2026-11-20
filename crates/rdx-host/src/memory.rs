//! In-memory capabilities for tests, golden runs and hosts with nothing better.
//!
//! Each type is a cheap handle onto shared state, so a test can keep a clone and inspect what a
//! run did after handing the other clone to a [`Host`](crate::Host).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use sha2::{Digest, Sha256};

use crate::{AuditLog, BlobStore, CapabilityError, Clock, Kv, MailOut, Message, Random, Result};

#[derive(Debug, Clone, Default)]
pub struct MemoryBlobStore(Rc<RefCell<BTreeMap<String, Vec<u8>>>>);

impl BlobStore for MemoryBlobStore {
    fn put(&mut self, bytes: &[u8]) -> Result<String> {
        let id = crate::sha256_digest(bytes);
        self.0.borrow_mut().insert(id.clone(), bytes.to_vec());
        Ok(id)
    }

    fn get(&self, id: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.0.borrow().get(id).cloned())
    }

    fn delete(&mut self, id: &str) -> Result<()> {
        self.0.borrow_mut().remove(id);
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub struct MemoryKv(Rc<RefCell<BTreeMap<String, Vec<u8>>>>);

impl Kv for MemoryKv {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.0.borrow().get(key).cloned())
    }

    fn put(&mut self, key: &str, value: &[u8]) -> Result<()> {
        self.0.borrow_mut().insert(key.to_owned(), value.to_vec());
        Ok(())
    }

    fn delete(&mut self, key: &str) -> Result<()> {
        self.0.borrow_mut().remove(key);
        Ok(())
    }
}

/// Keeps sent mail instead of sending it.
#[derive(Debug, Clone, Default)]
pub struct Outbox(Rc<RefCell<Vec<Message>>>);

impl Outbox {
    pub fn sent(&self) -> Vec<Message> {
        self.0.borrow().clone()
    }
}

impl MailOut for Outbox {
    fn send(&mut self, msg: &Message) -> Result<()> {
        self.0.borrow_mut().push(msg.clone());
        Ok(())
    }
}

/// A clock stopped at one instant (milliseconds since the Unix epoch).
#[derive(Debug, Clone, Copy)]
pub struct FixedClock(pub u64);

impl FixedClock {
    /// 2026-01-01T00:00:00Z, the instant golden runs see.
    pub const GOLDEN: u64 = 1_767_225_600_000;
}

impl Clock for FixedClock {
    fn now_unix_ms(&self) -> Result<u64> {
        Ok(self.0)
    }
}

/// Deterministic bytes (SHA-256 in counter mode over a seed). Never use it for keys.
#[derive(Debug, Clone)]
pub struct SeededRandom {
    seed: u64,
    counter: u64,
}

impl SeededRandom {
    pub fn new(seed: u64) -> Self {
        Self { seed, counter: 0 }
    }
}

impl Random for SeededRandom {
    fn fill(&mut self, buf: &mut [u8]) -> Result<()> {
        for chunk in buf.chunks_mut(32) {
            let mut h = Sha256::new();
            h.update(self.seed.to_le_bytes());
            h.update(self.counter.to_le_bytes());
            self.counter = self
                .counter
                .checked_add(1)
                .ok_or_else(|| CapabilityError::new("seeded random exhausted"))?;
            chunk.copy_from_slice(&h.finalize()[..chunk.len()]);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub struct MemoryAuditLog(Rc<RefCell<Vec<String>>>);

impl MemoryAuditLog {
    pub fn lines(&self) -> Vec<String> {
        self.0.borrow().clone()
    }
}

impl AuditLog for MemoryAuditLog {
    fn append(&mut self, line: &str) -> Result<()> {
        self.0.borrow_mut().push(line.to_owned());
        Ok(())
    }

    fn last(&self) -> Result<Option<String>> {
        Ok(self.0.borrow().last().cloned())
    }
}
