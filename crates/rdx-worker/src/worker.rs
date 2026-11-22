//! The Workers entry points. Bindings (see wrangler.toml):
//!
//! - `RDX_KV` (KV): key registries, inboxes, quarantines. Shared with the PWA's sync later.
//! - `RDX_BLOBS` (R2): encrypted envelopes, keyed by their `sha256:` id.
//! - `RDX_INTAKE_POLICY` (var): the policy as JSON, exported from Nickel at deploy time.
//! - `RDX_EMAIL` (send_email, optional): expiry notices. Sending to arbitrary addresses needs the
//!   Workers Paid plan; without the binding, the setup reply's own warning is the only notice.

use rdx_host::{CapabilityError, Clock, Host, Random};
use rdx_intake::{Outcome, Policy};
use wasm_bindgen::JsCast;
use worker::*;

use crate::staged::{StagedBlobs, StagedKv};

const KV: &str = "RDX_KV";
const BLOBS: &str = "RDX_BLOBS";
const POLICY: &str = "RDX_INTAKE_POLICY";
const EMAIL: &str = "RDX_EMAIL";

/// Bounced when storage fails: Email Routing tells the sender, who can send again.
const TRY_AGAIN: &str = "this email couldn't be stored right now; please send it again later";

struct JsClock;

impl Clock for JsClock {
    fn now_unix_ms(&self) -> rdx_host::Result<u64> {
        Ok(Date::now().as_millis())
    }
}

/// The runtime's CSPRNG (`crypto.getRandomValues`).
struct JsRandom;

impl Random for JsRandom {
    fn fill(&mut self, buf: &mut [u8]) -> rdx_host::Result<()> {
        let crypto: web_sys::Crypto = js_sys::Reflect::get(&js_sys::global(), &"crypto".into())
            .map_err(|_| CapabilityError::new("no crypto in this runtime"))?
            .unchecked_into();
        // getRandomValues fills at most 65536 bytes per call.
        for chunk in buf.chunks_mut(65_536) {
            crypto
                .get_random_values_with_u8_array(chunk)
                .map_err(|_| CapabilityError::new("crypto.getRandomValues failed"))?;
        }
        Ok(())
    }
}

fn policy(env: &Env) -> Result<Policy> {
    let json = env.var(POLICY)?.to_string();
    serde_json::from_str(&json).map_err(|e| {
        Error::RustError(format!(
            "{POLICY} doesn't fit rdx/intake.ncl's Policy: {e}; export it with `nickel export`"
        ))
    })
}

async fn load(kv: &kv::KvStore, keys: &[String]) -> Result<StagedKv> {
    let staged = StagedKv::default();
    for k in keys {
        staged.load(k, kv.get(k).bytes().await?);
    }
    Ok(staged)
}

/// New blobs first, then kv (which lists them), then deletions (which kv no longer lists), so a
/// failure part way never leaves kv pointing at a missing blob.
async fn write(env: &Env, kv: &StagedKv, blobs: &StagedBlobs) -> Result<()> {
    let bucket = env.bucket(BLOBS)?;
    let store = env.kv(KV)?;
    for (id, bytes) in blobs.puts() {
        bucket.put(id, bytes).execute().await?;
    }
    for (key, value) in kv.changes() {
        match value {
            Some(v) => store.put_bytes(&key, &v)?.execute().await?,
            None => store.delete(&key).await?,
        }
    }
    for id in blobs.deletes() {
        bucket.delete(id).await?;
    }
    Ok(())
}

#[event(email)]
async fn email(message: ForwardableEmailMessage, env: Env, _ctx: Context) -> Result<()> {
    let policy = policy(&env)?;
    // Refuse before reading the stream, so a huge message never sits in memory.
    if message.raw_size() > policy.max_bytes as f64 {
        message.set_reject(&format!(
            "this email is larger than {} MB; send large statements as separate emails or \
             upload them in the app",
            policy.max_bytes / 1_000_000
        ));
        return Ok(());
    }
    let raw = message.raw_bytes().await?;
    let keys = rdx_intake::sender(&raw)
        .map(|a| rdx_intake::receive_keys(&a))
        .unwrap_or_default();
    let kv = load(&env.kv(KV)?, &keys).await?;
    let blobs = StagedBlobs::default();
    let mut host = Host::new()
        .with_kv(kv.clone())
        .with_blob_store(blobs.clone())
        .with_clock(JsClock)
        .with_random(JsRandom);
    let received = rdx_intake::receive(&mut host, &policy, &raw);
    // The plaintext goes now; only the encrypted copy is written below.
    drop(raw);
    let received = match received {
        Ok(r) => r,
        Err(e) => {
            console_error!("intake failed: {e}");
            message.set_reject(TRY_AGAIN);
            return Ok(());
        }
    };
    if let Outcome::Rejected { reason, .. } = &received.outcome {
        console_log!("rejected: {reason}");
        message.set_reject(reason);
        return Ok(());
    }
    // Reply before writing: if the setup link can't be sent, nothing is kept and the sender is
    // asked to try again, rather than holding mail no one can claim.
    if let Some(reply) = &received.reply {
        // No In-Reply-To or References: Email Routing sets both from the incoming email, and
        // rejects a reply whose References differ from its own (the original's References plus
        // its Message-ID), so threading headers built from the Message-ID alone fail.
        let mime = rdx_intake::mime(
            &reply.message,
            &policy.intake_address,
            Date::now().as_millis(),
            None,
        );
        let sent = match EmailMessage::new(&policy.intake_address, &reply.message.to, &mime) {
            Ok(m) => message.reply(&m).await.map(|_| ()),
            Err(e) => Err(e),
        };
        if let Err(e) = sent {
            console_error!("reply failed: {e:?}");
            message.set_reject(TRY_AGAIN);
            return Ok(());
        }
    }
    if let Err(e) = write(&env, &kv, &blobs).await {
        console_error!("storage failed: {e}");
        message.set_reject(TRY_AGAIN);
        return Ok(());
    }
    // Status and ids only: never the message or the link.
    let mut log = serde_json::to_value(&received.outcome).unwrap_or_default();
    if let Some(o) = log.as_object_mut() {
        o.remove("address");
    }
    console_log!("{log}");
    Ok(())
}

#[event(scheduled)]
async fn scheduled(_event: ScheduledEvent, env: Env, _ctx: ScheduleContext) {
    if let Err(e) = sweep(&env).await {
        console_error!("sweep failed: {e}");
    }
}

async fn sweep(env: &Env) -> Result<()> {
    let policy = policy(env)?;
    let store = env.kv(KV)?;
    let index = store.get(rdx_intake::QUARANTINE_INDEX).bytes().await?;
    let keys =
        rdx_intake::sweep_keys(index.as_deref()).map_err(|e| Error::RustError(e.to_string()))?;
    let kv = load(&store, &keys).await?;
    let blobs = StagedBlobs::default();
    let mut host = Host::new()
        .with_kv(kv.clone())
        .with_blob_store(blobs.clone())
        .with_clock(JsClock);
    let swept =
        rdx_intake::sweep(&mut host, &policy).map_err(|e| Error::RustError(e.to_string()))?;
    write(env, &kv, &blobs).await?;
    console_log!("sweep: {} quarantine(s) expired", swept.expired.len());

    let Ok(mailer) = env.send_email(EMAIL) else {
        return Ok(());
    };
    let now = Date::now().as_millis();
    for notice in &swept.notices {
        let mime = rdx_intake::mime(notice, &policy.intake_address, now, None);
        let sent = match EmailMessage::new(&policy.intake_address, &notice.to, &mime) {
            Ok(m) => mailer.send(&m).await.map(|_| ()),
            Err(e) => Err(e),
        };
        if let Err(e) = sent {
            console_error!("expiry notice not sent: {e:?}");
        }
    }
    Ok(())
}
