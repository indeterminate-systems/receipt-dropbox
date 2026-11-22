//! The Workers entry points. Bindings (see wrangler.toml):
//!
//! - `RDX_KV` (KV): key registries, inboxes, quarantines. Shared with the PWA's sync later.
//! - `RDX_BLOBS` (R2): encrypted envelopes, keyed by their `sha256:` id.
//! - `RDX_INTAKE_POLICY` (var): the policy as JSON, exported from Nickel at deploy time.
//! - `RDX_EMAIL` (send_email, optional): expiry notices. Sending to arbitrary addresses needs the
//!   Workers Paid plan; without the binding, the setup reply's own warning is the only notice.
//! - Static assets (`[assets]`): the claim page (`/claim`), built by `cargo xtask claim-page`.
//!   Cloudflare serves them without running the Worker; `fetch` gets only the API routes below.

use rdx_host::{CapabilityError, Clock, Host, Random};
use rdx_intake::claim::{self, Release};
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

/// The claim page's API (`rdx_intake::claim`). Requests are signed by the claim link's key and
/// carry only ciphertext; nothing here can decrypt. Everything else is a static asset or 404.
#[event(fetch)]
async fn fetch(mut req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let route = (req.method(), req.path());
    let answer = match (&route.0, route.1.as_str()) {
        (Method::Post, "/claim/open") => match req.json::<serde_json::Value>().await {
            Ok(body) => match body["request"].as_str() {
                Some(r) => claim_open(&env, r).await,
                None => Err(Refused(400, "the body needs a `request`".into())),
            },
            Err(_) => Err(Refused(400, "the body isn't JSON".into())),
        },
        (Method::Post, "/claim/release") => match req.json::<Release>().await {
            Ok(release) => claim_release(&env, &release).await,
            Err(_) => Err(Refused(400, "the body isn't a release request".into())),
        },
        _ => return Response::error("Not found", 404),
    };
    match answer {
        Ok(r) => Ok(r),
        Err(Refused(status, why)) => {
            // Status only: the address and request stay out of the logs.
            console_log!("claim {}: {status}", route.1);
            Ok(Response::from_json(&serde_json::json!({ "error": why }))?.with_status(status))
        }
    }
}

/// A refusal the page shows: an HTTP status and the reason, which carries no secrets.
struct Refused(u16, String);

impl From<rdx_intake::Error> for Refused {
    fn from(e: rdx_intake::Error) -> Self {
        use rdx_intake::Error as E;
        let status = match &e {
            E::ClaimLink(_) => 400,
            E::WrongClaim(_) => 403,
            E::NothingQuarantined(_) => 404,
            _ => {
                console_error!("claim failed: {e}");
                return Refused(500, "something went wrong on the server; try again".into());
            }
        };
        Refused(status, e.to_string())
    }
}

impl From<Error> for Refused {
    fn from(e: Error) -> Self {
        console_error!("claim storage failed: {e}");
        Refused(500, "storage is unavailable right now; try again".into())
    }
}

async fn claim_open(env: &Env, request: &str) -> std::result::Result<Response, Refused> {
    let address = claim::request_address(request)?;
    let kv = load(&env.kv(KV)?, &claim::open_keys(&address)).await?;
    let host = Host::new().with_kv(kv).with_clock(JsClock);
    let mut opened = claim::open(&host, request)?;
    let bucket = env.bucket(BLOBS)?;
    for d in &opened.documents {
        let bytes = match bucket.get(&d.id).execute().await? {
            Some(object) => match object.body() {
                Some(body) => body.bytes().await?,
                None => Vec::new(),
            },
            None => Vec::new(),
        };
        opened.envelopes.push(
            String::from_utf8(bytes)
                .map_err(|_| Refused(500, format!("document `{}` is missing or damaged", d.id)))?,
        );
    }
    Ok(Response::from_json(&opened)?)
}

async fn claim_release(env: &Env, release: &Release) -> std::result::Result<Response, Refused> {
    let address = claim::request_address(&release.request)?;
    let kv = load(&env.kv(KV)?, &claim::release_keys(&address)).await?;
    let blobs = StagedBlobs::default();
    let mut host = Host::new()
        .with_kv(kv.clone())
        .with_blob_store(blobs.clone())
        .with_clock(JsClock);
    let moved = claim::accept(&mut host, release)?;
    write(env, &kv, &blobs).await?;
    console_log!("claim released: {} document(s)", moved.len());
    Ok(Response::from_json(
        &serde_json::json!({ "moved": moved.len() }),
    )?)
}
