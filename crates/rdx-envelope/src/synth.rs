//! A published signing key for synthetic fixtures and tests.
//!
//! Its seed is in this file, so anyone can sign as it: it proves nothing and must never sign a
//! real statement. Real supporters' keys come from their passkeys (roadmap piece 8).

use bc_components::{Ed25519PrivateKey, SigningPrivateKey, SigningPublicKey};

const SEED: &[u8] = b"rdx synthetic fixtures: a published key that proves nothing";

/// An Ed25519 key derived from `seed`. For tests only: a key is only as secret as its seed.
pub fn signer_from_seed(seed: &[u8]) -> SigningPrivateKey {
    SigningPrivateKey::new_ed25519(Ed25519PrivateKey::derive_from_key_material(seed))
}

/// The fixtures' signing key.
pub fn signer() -> SigningPrivateKey {
    signer_from_seed(SEED)
}

/// The fixtures' public key, which `verify` checks signatures against.
pub fn verifier() -> SigningPublicKey {
    signer()
        .public_key()
        .expect("an Ed25519 private key always has a public key")
}
