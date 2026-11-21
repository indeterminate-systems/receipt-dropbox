//! A supporter's keys: device keys unlocked by a passkey, a seed for recovery split into SSKR
//! shares, the signed registry of which keys an address has, and encryption to those keys.
//!
//! ```text
//!   passkey ──PRF──▶ wrapping key ──unwraps──▶ device key   (one per device; Ed25519 + X25519)
//!
//!   seed ──HKDF──▶ recovery key                             (seed exists only as SSKR shares)
//!    └──SSKR──▶ shares (for example 2 of 3: another device, a printed card, an org custodian)
//!
//!   registry: address ─▶ [genesis, add, rotate, revoke, …]  (each event signed by an active key)
//!   documents: encrypted to every active device key and to the recovery key
//! ```
//!
//! A device holds only its own key, wrapped by its passkey, so a lost device exposes nothing else
//! and can be revoked. The seed is never stored: onboarding splits it into shares and forgets it.
//! Because every document is also encrypted to the recovery key, enough shares recover every
//! document after every device is lost, and enroll a new device ([`registry`]).
//!
//! The crate does no I/O and builds for `wasm32-unknown-unknown` and `wasm32-wasip2`. All
//! randomness comes from the caller's generator ([`host_rng`] seeds one from the host's `random`
//! capability), including the ephemeral keys and nonces bc-envelope would otherwise draw from the
//! platform. See [ADR-P08](../../docs/adr/ADR-P08-keys-and-recovery.md).

mod no_system_rng;
pub mod ops;
pub mod registry;

use bc_components::{
    EncapsulationCiphertext, EncapsulationPublicKey, Encrypter, HKDFRng, Nonce, PrivateKeyBase,
    PublicKeys, SSKRGroupSpec, SSKRSecret, SSKRShare, SSKRSpec, SigningPrivateKey,
    SigningPublicKey, SymmetricKey, X25519PrivateKey, X25519PublicKey, sskr_combine,
    sskr_generate_using,
};
use bc_crypto::hash::hkdf_hmac_sha256;
use bc_envelope::prelude::*;
use bc_rand::RandomNumberGenerator;

pub use ops::register;
pub use registry::Registry;

/// The salt a host passes to the passkey's PRF. A new wrapping scheme gets a new salt.
pub const PRF_SALT: &[u8] = b"rdx.passkey.v1";

/// The `'isA'` of a passkey-wrapped device key.
pub const WRAPPED_KEY_TYPE: &str = "rdx.wrapped-key.v1";

const KEY_LEN: usize = 32;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("this passkey doesn't unlock this key: a different passkey, or a changed record")]
    WrongPasskey,
    #[error("the passkey's PRF output must be at least 32 bytes, got {0}")]
    ShortPrf(usize),
    #[error("not a wrapped device key: {0}")]
    NotAWrappedKey(String),
    #[error("none of these keys can open this envelope: it wasn't encrypted to them")]
    NotARecipient,
    #[error("not an encrypted envelope: {0}")]
    NotEncrypted(String),
    #[error("recovery shares: {0}")]
    Shares(String),
    #[error("not a valid `{kind}` UR: {message}")]
    Ur { kind: &'static str, message: String },
    #[error("key registry: {0}")]
    Registry(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// A deterministic generator: HKDF-SHA-256 pages over a 32-byte seed. Seeded from the host's
/// `random` capability ([`host_rng`]) it is the only randomness keys and envelopes use; seeded
/// from a fixed value it makes reproducible fixtures.
pub struct Drbg(HKDFRng);

impl Drbg {
    pub fn new(seed: impl AsRef<[u8]>) -> Self {
        Self(HKDFRng::new(seed, "rdx.drbg.v1"))
    }
}

impl rand_core::RngCore for Drbg {
    fn next_u32(&mut self) -> u32 {
        u32::from_le_bytes(random_bytes(self))
    }

    fn next_u64(&mut self) -> u64 {
        u64::from_le_bytes(random_bytes(self))
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.0.fill_bytes(dest);
    }
}

impl rand_core::CryptoRng for Drbg {}
impl RandomNumberGenerator for Drbg {}

/// A generator seeded with 32 bytes of the host's randomness. One per op run, so no randomness
/// comes from the platform.
pub fn host_rng(random: &mut dyn rdx_host::Random) -> Result<Drbg, rdx_host::CapabilityError> {
    let mut seed = [0u8; KEY_LEN];
    random.fill(&mut seed)?;
    Ok(Drbg::new(seed))
}

fn random_bytes<const N: usize>(rng: &mut impl rand_core::RngCore) -> [u8; N] {
    let mut out = [0u8; N];
    rng.fill_bytes(&mut out);
    out
}

fn nonce(rng: &mut impl RandomNumberGenerator) -> Nonce {
    Nonce::from_data(random_bytes(rng))
}

/// A key pair set: Ed25519 for signing, X25519 for receiving encrypted envelopes. Device keys and
/// the recovery key are both `Keys`.
#[derive(Clone)]
pub struct Keys(PrivateKeyBase);

impl std::fmt::Debug for Keys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Keys(…)")
    }
}

impl Keys {
    pub fn generate(rng: &mut impl RandomNumberGenerator) -> Self {
        Self(PrivateKeyBase::from_data(random_bytes::<KEY_LEN>(rng)))
    }

    /// The Ed25519 key statements and registry events are signed with.
    pub fn signing_key(&self) -> SigningPrivateKey {
        self.0.ed25519_signing_private_key()
    }

    fn agreement(&self) -> X25519PrivateKey {
        self.0.x25519_private_key()
    }

    pub fn public(&self) -> PublicKeys {
        PublicKeys::new(
            self.signing_public(),
            EncapsulationPublicKey::X25519(self.agreement().public_key()),
        )
    }

    pub fn signing_public(&self) -> SigningPublicKey {
        self.signing_key()
            .public_key()
            .expect("Ed25519 keys always have a public key")
    }

    /// Signs an envelope (wrapping it first), as `Envelope::sign` does.
    pub fn sign(&self, envelope: &Envelope) -> Envelope {
        envelope.sign(&self.signing_key())
    }
}

/// The root of a supporter's recovery: 32 random bytes, never stored, only split into shares.
/// Zeroed when dropped.
#[derive(zeroize::ZeroizeOnDrop)]
pub struct Seed([u8; KEY_LEN]);

impl Seed {
    pub fn generate(rng: &mut impl RandomNumberGenerator) -> Self {
        Self(random_bytes(rng))
    }

    /// The recovery key every document is also encrypted to.
    pub fn recovery_keys(&self) -> Keys {
        Keys(PrivateKeyBase::from_data(hkdf_hmac_sha256(
            self.0,
            "rdx.recovery.v1",
            KEY_LEN,
        )))
    }

    /// Splits the seed into `count` shares, any `threshold` of which recover it, as
    /// `ur:sskr/…` strings.
    pub fn split(
        &self,
        threshold: usize,
        count: usize,
        rng: &mut impl RandomNumberGenerator,
    ) -> Result<Vec<String>> {
        let shares_err = |e: sskr::Error| Error::Shares(e.to_string());
        let group = SSKRGroupSpec::new(threshold, count).map_err(shares_err)?;
        let spec = SSKRSpec::new(1, vec![group]).map_err(shares_err)?;
        let secret = SSKRSecret::new(self.0).map_err(shares_err)?;
        let groups = sskr_generate_using(&spec, &secret, rng).map_err(shares_err)?;
        tags();
        Ok(groups
            .into_iter()
            .flatten()
            .map(|s| s.ur_string())
            .collect())
    }

    /// Recovers the seed from `ur:sskr/…` shares. Fails, without saying which, if there are too
    /// few or they come from different splits.
    pub fn combine(shares: &[String]) -> Result<Self> {
        tags();
        let shares = shares
            .iter()
            .map(|s| {
                SSKRShare::from_ur_string(s.trim()).map_err(|e| Error::Ur {
                    kind: "sskr",
                    message: e.to_string(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let secret = sskr_combine(&shares).map_err(|e| Error::Shares(e.to_string()))?;
        let bytes: [u8; KEY_LEN] = secret
            .as_ref()
            .try_into()
            .map_err(|_| Error::Shares("these shares don't hold a 32-byte seed".into()))?;
        Ok(Self(bytes))
    }
}

fn wrapping_key(prf: &[u8]) -> Result<SymmetricKey> {
    if prf.len() < KEY_LEN {
        return Err(Error::ShortPrf(prf.len()));
    }
    let key = hkdf_hmac_sha256(prf, "rdx.passkey-wrap.v1", KEY_LEN);
    Ok(SymmetricKey::from_data_ref(key).expect("HKDF gave 32 bytes"))
}

/// Encrypts a device key under a key derived from the passkey's PRF output. The result is
/// ciphertext, safe to keep in the device's `kv`.
pub fn wrap(keys: &Keys, prf: &[u8], rng: &mut impl RandomNumberGenerator) -> Result<Envelope> {
    let key = wrapping_key(prf)?;
    Envelope::new(ByteString::from(keys.0.as_bytes()))
        .add_assertion(known_values::IS_A, WRAPPED_KEY_TYPE)
        .wrap()
        .encrypt_subject_opt(&key, Some(nonce(rng)))
        .map_err(|e| Error::NotAWrappedKey(e.to_string()))
}

/// Unlocks a device key with the passkey's PRF output.
pub fn unwrap(wrapped: &Envelope, prf: &[u8]) -> Result<Keys> {
    let key = wrapping_key(prf)?;
    let inner = wrapped
        .decrypt_subject(&key)
        .map_err(|e| match e {
            bc_envelope::Error::InvalidDigest => Error::WrongPasskey,
            bc_envelope::Error::NotEncrypted => Error::NotAWrappedKey(e.to_string()),
            _ => Error::WrongPasskey,
        })?
        .try_unwrap()
        .map_err(|e| Error::NotAWrappedKey(e.to_string()))?;
    let kind: String = inner
        .extract_object_for_predicate(known_values::IS_A)
        .map_err(|e| Error::NotAWrappedKey(e.to_string()))?;
    if kind != WRAPPED_KEY_TYPE {
        return Err(Error::NotAWrappedKey(format!("it is a `{kind}`")));
    }
    let bytes: ByteString = inner
        .extract_subject()
        .map_err(|e| Error::NotAWrappedKey(e.to_string()))?;
    Ok(Keys(PrivateKeyBase::from_data(bytes.data())))
}

/// A recipient with an ephemeral key drawn from the caller's generator: bc-components would
/// otherwise draw it from the platform. The sealed message it makes is the standard one.
struct Ephemeral {
    recipient: X25519PublicKey,
    ephemeral: X25519PrivateKey,
}

impl Ephemeral {
    fn new(recipient: &PublicKeys, rng: &mut impl RandomNumberGenerator) -> Result<Self> {
        match recipient.enapsulation_public_key() {
            EncapsulationPublicKey::X25519(key) => Ok(Self {
                recipient: *key,
                ephemeral: X25519PrivateKey::new_using(rng),
            }),
            #[allow(unreachable_patterns)]
            _ => Err(Error::Ur {
                kind: "crypto-pubkeys",
                message: "only X25519 recipients are supported".into(),
            }),
        }
    }
}

impl Encrypter for Ephemeral {
    fn encapsulation_public_key(&self) -> EncapsulationPublicKey {
        EncapsulationPublicKey::X25519(self.recipient)
    }

    fn encapsulate_new_shared_secret(&self) -> (SymmetricKey, EncapsulationCiphertext) {
        (
            self.ephemeral.shared_key_with(&self.recipient),
            EncapsulationCiphertext::X25519(self.ephemeral.public_key()),
        )
    }
}

/// Encrypts a whole envelope (wrapped, so its assertions and signature are hidden too) to each
/// recipient: one fresh content key, sealed once per recipient as a `'hasRecipient'` assertion.
pub fn encrypt(
    envelope: &Envelope,
    recipients: &[PublicKeys],
    rng: &mut impl RandomNumberGenerator,
) -> Result<Envelope> {
    if recipients.is_empty() {
        return Err(Error::NotARecipient);
    }
    let content_key = SymmetricKey::from_data(random_bytes(rng));
    let mut e = envelope
        .wrap()
        .encrypt_subject_opt(&content_key, Some(nonce(rng)))
        .map_err(|e| Error::NotEncrypted(e.to_string()))?;
    for r in recipients {
        let eph = Ephemeral::new(r, rng)?;
        e = e.add_recipient_opt(&eph, &content_key, Some(nonce(rng)));
    }
    Ok(e)
}

fn content_key(envelope: &Envelope, keys: &Keys) -> Result<SymmetricKey> {
    let sealed = envelope
        .recipients()
        .map_err(|e| Error::NotEncrypted(e.to_string()))?;
    if sealed.is_empty() {
        return Err(Error::NotEncrypted("it has no recipients".into()));
    }
    let data = sealed
        .iter()
        .find_map(|s| s.decrypt(&keys.0).ok())
        .ok_or(Error::NotARecipient)?;
    SymmetricKey::from_tagged_cbor_data(data).map_err(|e| Error::NotEncrypted(e.to_string()))
}

/// Opens an envelope [`encrypt`] made, with one recipient's keys.
pub fn decrypt(envelope: &Envelope, keys: &Keys) -> Result<Envelope> {
    let key = content_key(envelope, keys)?;
    envelope
        .decrypt_subject(&key)
        .and_then(|e| e.try_unwrap())
        .map_err(|e| Error::NotEncrypted(e.to_string()))
}

/// Lets more keys open an encrypted envelope, without re-encrypting its content: `keys` must be
/// one of its recipients. The root digest changes (new assertions); the content doesn't.
pub fn add_recipients(
    envelope: &Envelope,
    keys: &Keys,
    new: &[PublicKeys],
    rng: &mut impl RandomNumberGenerator,
) -> Result<Envelope> {
    let key = content_key(envelope, keys)?;
    let mut e = envelope.clone();
    for r in new {
        let eph = Ephemeral::new(r, rng)?;
        e = e.add_recipient_opt(&eph, &key, Some(nonce(rng)));
    }
    Ok(e)
}

/// UR type names come from the global CBOR tag registry, which starts empty.
fn tags() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(register_tags);
}

pub fn envelope_to_ur(envelope: &Envelope) -> String {
    tags();
    envelope.ur_string()
}

pub fn envelope_from_ur(ur: &str) -> Result<Envelope> {
    tags();
    Envelope::from_ur_string(ur.trim()).map_err(|e| Error::Ur {
        kind: "envelope",
        message: e.to_string(),
    })
}

pub fn public_keys_to_ur(keys: &PublicKeys) -> String {
    tags();
    keys.ur_string()
}

pub fn public_keys_from_ur(ur: &str) -> Result<PublicKeys> {
    tags();
    PublicKeys::from_ur_string(ur.trim()).map_err(|e| Error::Ur {
        kind: "crypto-pubkeys",
        message: e.to_string(),
    })
}

pub fn signing_key_to_ur(key: &SigningPublicKey) -> String {
    tags();
    key.ur_string()
}
