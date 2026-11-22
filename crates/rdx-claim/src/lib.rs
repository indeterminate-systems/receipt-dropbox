//! The claim page: the smallest browser app that finishes email intake for a sender who hasn't
//! set up keys. It opens the "Verify your address" link, makes the sender a passkey and device
//! key, and moves the mail waiting in quarantine to those keys. The full PWA (piece 14) replaces
//! it; until then it is what the setup link opens.
//!
//! ```text
//!   link #address=…&key=…  (the fragment never reaches a server)
//!     │
//!     ├─ POST /claim/open     signed by the link's key ─▶ quarantined envelopes
//!     ├─ passkey (WebAuthn PRF) ─▶ keys.onboard: device key wrapped by the passkey, recovery
//!     │                            share shown once, registry signed by the recovery key
//!     ├─ reseal: decrypt with the link's key, encrypt to the registry's keys (in this page)
//!     └─ POST /claim/release  registry + resealed envelopes, signed by the link's key
//! ```
//!
//! The server side is `rdx_intake::claim`, run by the Worker (`rdx-worker`). This module is the
//! page's logic and runs anywhere; [`web`] (browser builds only) is the DOM, WebAuthn and fetch
//! around it.

use rdx_host::memory::{FixedClock, MemoryKv};
use rdx_host::{Host, Kv, Random, Secrets};
use rdx_intake::claim::{Opened, Release};
use rdx_keys::Registry;
use serde_json::json;
use zeroize::Zeroize;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod web;

/// The device name the page gives the first device. The PWA lets people name more.
pub const DEVICE: &str = "first-device";

/// One recovery share, held by the sender. The PWA (piece 14) offers holders and thresholds.
pub const HOLDER: &str = "you";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Intake(#[from] rdx_intake::Error),
    #[error("setting up keys failed: {0}")]
    Keys(String),
}

/// A claim link, read. The key stays in this value's memory and is wiped when it drops.
pub struct Claim {
    pub address: String,
    secret: [u8; 32],
}

impl Drop for Claim {
    fn drop(&mut self) {
        self.secret.zeroize();
    }
}

/// What [`Claim::finish`] gives the page: the request to send, the share to show, and this
/// device's wrapped key to keep.
pub struct Finished {
    pub release: Release,
    /// `ur:sskr/…`: the recovery share, shown once.
    pub share: String,
    /// The `keys/device/<address>` record (`{ device, wrapped }`): ciphertext only, for the
    /// browser's storage, where the PWA will find it.
    pub device_record: Vec<u8>,
    pub registry: Registry,
}

impl Claim {
    /// Reads a claim link, its fragment, or a pasted reply that holds it.
    pub fn parse(link: &str) -> Result<Self, Error> {
        let (address, secret) = rdx_intake::parse_claim_link(link)?;
        Ok(Self { address, secret })
    }

    /// The signed request for `POST /claim/open`.
    pub fn open_request(&self, now: u64) -> String {
        rdx_intake::claim::open_request(&self.address, &self.secret, now)
    }

    /// Onboards the address with the passkey's PRF output, re-encrypts what `opened` holds to the
    /// new keys and signs the release. Nothing leaves this memory except what [`Finished`] holds.
    pub fn finish(
        &self,
        opened: &Opened,
        prf: Vec<u8>,
        random: impl Random + 'static,
        now: u64,
    ) -> Result<Finished, Error> {
        if opened.address != self.address {
            return Err(Error::Keys(format!(
                "the server answered for `{}`, not `{}`",
                opened.address, self.address
            )));
        }
        let kv = MemoryKv::default();
        let secrets = PagePasskey::new(prf);
        let mut host = Host::new()
            .with_kv(kv.clone())
            .with_clock(FixedClock(now))
            .with_random(random)
            .with_secrets(secrets.clone());

        let mut ops = rdx_plan::Registry::new();
        rdx_keys::register(&mut ops);
        let onboard = ops
            .get("keys.onboard")
            .expect("rdx-keys registers keys.onboard");
        let args = json!({
            "address": self.address,
            "supporter": supporter_id(&self.address),
            "device": DEVICE,
            "threshold": 1,
            "holders": [HOLDER],
        });
        onboard
            .run(
                &args,
                &mut rdx_plan::RunContext {
                    notes: Vec::new(),
                    host: &mut host,
                },
            )
            .map_err(|e| Error::Keys(e.message))?;

        let registry = rdx_keys::ops::load_registry(&host, &self.address)
            .map_err(|e| Error::Keys(e.message))?
            .ok_or_else(|| Error::Keys("onboarding wrote no registry".into()))?;
        let recipients = registry
            .state()
            .map_err(|e| Error::Keys(e.to_string()))?
            .recipients();
        let random = host.random.as_deref_mut().expect("set above");
        let resealed = rdx_intake::claim::reseal(opened, &self.secret, &recipients, random)?;
        let release = rdx_intake::claim::release_request(
            &self.address,
            &self.secret,
            now,
            &registry,
            &resealed,
        );
        let share = secrets
            .take_shown()
            .ok_or_else(|| Error::Keys("onboarding showed no recovery share".into()))?;
        let device_record = kv
            .get(&format!("keys/device/{}", self.address))
            .map_err(|e| Error::Keys(e.to_string()))?
            .ok_or_else(|| Error::Keys("onboarding kept no device key".into()))?;
        Ok(Finished {
            release,
            share,
            device_record,
            registry,
        })
    }
}

/// A supporter id (`model.Id`: lowercase letters, digits, `.`, `_`, `-`) from the address, until
/// the organization assigns ids at onboarding (piece 14).
pub fn supporter_id(address: &str) -> String {
    let id: String = address
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'a'..='z' | '0'..='9' | '.' | '_' | '-' => c,
            _ => '.',
        })
        .take(128)
        .collect();
    id.trim_start_matches(['.', '_', '-']).to_owned()
}

/// The page's `secrets`: the PRF output the browser's passkey gave, and the share onboarding
/// shows, kept for the page to display once.
#[derive(Clone)]
struct PagePasskey(std::rc::Rc<std::cell::RefCell<PasskeyState>>);

struct PasskeyState {
    prf: Vec<u8>,
    shown: Option<String>,
}

impl Drop for PasskeyState {
    fn drop(&mut self) {
        self.prf.zeroize();
    }
}

impl PagePasskey {
    fn new(prf: Vec<u8>) -> Self {
        Self(std::rc::Rc::new(std::cell::RefCell::new(PasskeyState {
            prf,
            shown: None,
        })))
    }

    fn take_shown(&self) -> Option<String> {
        self.0.borrow_mut().shown.take()
    }
}

impl Secrets for PagePasskey {
    /// The browser evaluated the passkey's PRF with [`rdx_keys::PRF_SALT`] before onboarding
    /// started; any other salt is a programming error, not a second prompt.
    fn prf(&mut self, salt: &[u8]) -> rdx_host::Result<Vec<u8>> {
        if salt != rdx_keys::PRF_SALT {
            return Err(rdx_host::CapabilityError::new(
                "the claim page only evaluates the passkey for rdx.passkey.v1",
            ));
        }
        Ok(self.0.borrow().prf.clone())
    }

    fn show(&mut self, _label: &str, secret: &str) -> rdx_host::Result<()> {
        self.0.borrow_mut().shown = Some(secret.to_owned());
        Ok(())
    }

    fn ask(&mut self, _label: &str) -> rdx_host::Result<Option<String>> {
        Ok(None)
    }
}
