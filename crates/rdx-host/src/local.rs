//! Directory-backed capabilities for the local host (`rdx`), on native and `wasm32-wasip2`
//! (where the directory is a WASI preopen).
//!
//! Layout under the state directory:
//!
//! ```text
//! blobs/<hex>         blob contents, named by their SHA-256
//! kv/<hex of key>     values; keys are hex-encoded so no key can name a path
//! outbox/<n>.json     mail that would have been sent (the local host never sends mail)
//! audit.jsonl         one run per line
//! ```

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    AuditLog, BlobStore, CapabilityError, Clock, Host, Kv, MailOut, Message, Random, Result,
};

fn io_err(what: &str, path: &Path, e: io::Error) -> CapabilityError {
    CapabilityError::new(format!("{what} {}: {e}", path.display()))
}

fn ensure_dir(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir).map_err(|e| io_err("cannot create", dir, e))
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_err("cannot read", path, e)),
    }
}

fn remove_optional(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io_err("cannot delete", path, e)),
    }
}

/// Writes through a temporary file and a rename, so a crash never leaves half a value.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes).map_err(|e| io_err("cannot write", &tmp, e))?;
    fs::rename(&tmp, path).map_err(|e| io_err("cannot write", path, e))
}

/// Every capability, kept under `state_dir` (created if missing).
pub fn host(state_dir: &Path) -> Result<Host> {
    ensure_dir(state_dir)?;
    Ok(Host::new()
        .with_blob_store(DirBlobStore::new(state_dir.join("blobs"))?)
        .with_kv(DirKv::new(state_dir.join("kv"))?)
        .with_mail_out(OutboxDir::new(state_dir.join("outbox"))?)
        .with_clock(SystemClock)
        .with_random(OsRandom)
        .with_audit_log(FileAuditLog::new(state_dir.join("audit.jsonl"))))
}

pub struct DirBlobStore {
    dir: PathBuf,
}

impl DirBlobStore {
    pub fn new(dir: PathBuf) -> Result<Self> {
        ensure_dir(&dir)?;
        Ok(Self { dir })
    }

    fn path(&self, id: &str) -> Result<PathBuf> {
        if !crate::is_sha256_digest(id) {
            return Err(CapabilityError::new(format!(
                "`{id}` is not a blob id; blob ids are `sha256:` and 64 lowercase hex digits"
            )));
        }
        Ok(self.dir.join(&id["sha256:".len()..]))
    }
}

impl BlobStore for DirBlobStore {
    fn put(&mut self, bytes: &[u8]) -> Result<String> {
        let id = crate::sha256_digest(bytes);
        let path = self.path(&id)?;
        if !path.exists() {
            write_atomic(&path, bytes)?;
        }
        Ok(id)
    }

    fn get(&self, id: &str) -> Result<Option<Vec<u8>>> {
        read_optional(&self.path(id)?)
    }

    fn delete(&mut self, id: &str) -> Result<()> {
        remove_optional(&self.path(id)?)
    }
}

pub struct DirKv {
    dir: PathBuf,
}

impl DirKv {
    pub fn new(dir: PathBuf) -> Result<Self> {
        ensure_dir(&dir)?;
        Ok(Self { dir })
    }

    fn path(&self, key: &str) -> PathBuf {
        let name: String = key.bytes().map(|b| format!("{b:02x}")).collect();
        // An empty key still needs a file name.
        self.dir.join(format!("k{name}"))
    }
}

impl Kv for DirKv {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        read_optional(&self.path(key))
    }

    fn put(&mut self, key: &str, value: &[u8]) -> Result<()> {
        write_atomic(&self.path(key), value)
    }

    fn delete(&mut self, key: &str) -> Result<()> {
        remove_optional(&self.path(key))
    }
}

/// Spools mail as numbered JSON files instead of sending it.
pub struct OutboxDir {
    dir: PathBuf,
}

impl OutboxDir {
    pub fn new(dir: PathBuf) -> Result<Self> {
        ensure_dir(&dir)?;
        Ok(Self { dir })
    }
}

impl MailOut for OutboxDir {
    fn send(&mut self, msg: &Message) -> Result<()> {
        let json = serde_json::to_vec_pretty(msg).expect("messages serialize");
        // create_new never overwrites; on a race, take the next number.
        for n in 1.. {
            let path = self.dir.join(format!("{n:06}.json"));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut f) => {
                    return f
                        .write_all(&json)
                        .map_err(|e| io_err("cannot write", &path, e));
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(io_err("cannot write", &path, e)),
            }
        }
        unreachable!("the outbox has a free number")
    }
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now_unix_ms(&self) -> Result<u64> {
        let since = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| CapabilityError::new("the system clock is before 1970"))?;
        Ok(since.as_millis() as u64)
    }
}

/// The operating system's (or WASI's) random source.
pub struct OsRandom;

impl Random for OsRandom {
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    fn fill(&mut self, buf: &mut [u8]) -> Result<()> {
        getrandom::fill(buf).map_err(|e| CapabilityError::new(format!("no randomness: {e}")))
    }

    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    fn fill(&mut self, _: &mut [u8]) -> Result<()> {
        Err(CapabilityError::new(
            "the local host has no randomness in the browser; use the PWA host",
        ))
    }
}

/// Appends one line per run to a file.
pub struct FileAuditLog {
    path: PathBuf,
}

impl FileAuditLog {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl AuditLog for FileAuditLog {
    fn append(&mut self, line: &str) -> Result<()> {
        if line.contains('\n') {
            return Err(CapabilityError::new("audit entries are single lines"));
        }
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| io_err("cannot open", &self.path, e))?;
        writeln!(f, "{line}").map_err(|e| io_err("cannot write", &self.path, e))
    }

    fn last(&self) -> Result<Option<String>> {
        let bytes = read_optional(&self.path)?.unwrap_or_default();
        let text = String::from_utf8(bytes)
            .map_err(|_| CapabilityError::new(format!("{} is not UTF-8", self.path.display())))?;
        Ok(text
            .lines()
            .rev()
            .find(|l| !l.is_empty())
            .map(str::to_owned))
    }
}
