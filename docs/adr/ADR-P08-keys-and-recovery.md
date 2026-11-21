# ADR-P08: Keys and recovery

**Status:** Proposed
**Date:** 2026-11-21
**Deciders:** Ash

## Context

Roadmap piece 8 gives supporters keys without asking them to manage any: a passkey (WebAuthn
PRF) unlocks keys on each device, SSKR shares recover access, an address maps to several device
keys with rotation and revocation, servers encrypt but never decrypt, and documents are
re-encrypted when a key is added. It also takes over what piece 7 deferred
([ADR-P07](ADR-P07-statement-envelopes.md)): sealing a statement from a plan, and encrypting
envelopes to recipients.

Two constraints shape it. The core does no I/O and takes randomness only from the host. And a
plan's arguments and outputs are recorded in the run report and the audit log, so no secret may
pass through them.

## Decision

**A device holds only its own key; the seed exists only as shares.** Each device has its own key
set (Ed25519 for signing, X25519 for receiving), generated from the host's randomness and kept in
the device's `kv` encrypted under a key derived (HKDF-SHA-256) from its passkey's PRF output for
the salt `rdx.passkey.v1`. Onboarding draws a 32-byte seed, derives the *recovery key* from it,
splits the seed into SSKR shares (one group, `threshold` of `holders`) and forgets it. Every
document is encrypted to every active device key **and** the recovery key, so enough shares open
everything after every device is lost.

**The registry is a signed chain.** An address's keys are a list of Gordian Envelope events
(`genesis`, `add`, `rotate`, `revoke`), each naming its sequence number and the digest of the
event before it, signed by an active device or the recovery key. Replaying the chain checks every
link and signature, so a server can store and serve the registry (public keys only) and use it to
encrypt, without being able to add a key of its own. Device ids are never reused; `rotate` is
signed by the device itself or the recovery key; `genesis` by the recovery key.

**A `secrets` host capability.** `prf(salt)` returns the passkey's PRF output, `show(label,
secret)` hands a recovery share to the person, `ask(label)` takes one back. Ops reach the passkey
and the shares only through it, so nothing secret is in a plan. Only hosts where the person is
present offer it (the PWA; the local host, whose "passkey" is a file, for development); a plan
that needs to decrypt fails validation on a server, before any step runs. It is in `rdx-host`
and `wit/rdx.wit` like the other capabilities.

**All randomness from the host.** bc-envelope's recipient encryption draws content keys, nonces
and ephemeral X25519 keys from the platform. `rdx-keys` draws them from a generator seeded with 32
bytes of the host's `random` (HKDF pages, `Drbg`): it supplies the content key and nonces through
bc-envelope's `_opt` functions, and the ephemeral key through its own `Encrypter`, whose sealed
messages are the standard ones any Gordian tool opens. So the browser build still has no
platform randomness, and the same generator gives the same ciphertext everywhere.

**Ops.** `keys.onboard`, `keys.create-device` (on the new device) and `keys.add-device` (on an
active one), `keys.rotate`, `keys.revoke`, `keys.recover` (asks for shares, enrolls this device,
revokes lost ones and lets the new key read listed documents) and `keys.recipients` (servers can
run it). In `rdx-envelope`: `envelope.seal` signs a statement with this device's key, encrypts it
to the registry's recipients and keeps it as `ur:envelope/…` text in the blob store;
`envelope.open` decrypts it on the device; `envelope.encrypt` encrypts to given public keys (no
private key, so servers can); `envelope.add-recipients` lets a new key read a kept envelope
without re-encrypting its content. Skills `onboard-supporter` and `recover-access`.

## Options considered

| Option | For | Against |
| :-- | :-- | :-- |
| Device keys plus a share-only recovery key (chosen) | A lost device exposes only what it already had; the seed is never at rest anywhere; revocation means something | Every document carries one more recipient; recovery needs shares, not just a passkey |
| One seed wrapped by every device's passkey, device keys derived from it (architecture §Key management, as first sketched) | One secret; any device can derive any key | Every device holds the root, so revoking a stolen device protects nothing unless the seed and every document are rotated |
| Device key derived straight from the PRF output, nothing stored | No wrapped blob to keep | Rotation needs a new passkey; one passkey can't unlock a key another passkey made |
| A JSON registry with detached signatures | Simpler to read | A second signature format beside Gordian Envelope; envelopes are what the rest of the core verifies |
| getrandom's JS backend for encryption in the browser | Uses bc-envelope's ordinary functions | Ties the core to `wasm-bindgen` and the platform, against ADR-P07 |
| Encrypt to recipients by passing secrets as plan inputs | No new capability | Shares and PRF output would land in run reports and the audit log |

## Consequences

- The architecture's "unwraps the supporter's seed" becomes "unwraps the device's key"; the
  housekeeping PR updates architecture §Key management to match.
- Sealed statements are stored encrypted. `envelope.open` outputs the signed statement in the
  clear, and run reports record outputs, so on the PWA the audit log (local) holds it like
  `statement.parse`'s output already does. Piece 12 should decide whether reports keep
  plaintext outputs or only their digests.
- Registry sync between devices and the organization's host is not here: piece 13 (Workers KV)
  and piece 14 (PWA) move `keys/registry/<address>` between hosts. Two devices adding events at
  once would fork the chain; the later writer must reload and retry.
- bc-envelope's `_opt` functions are `#[doc(hidden)]`. If a release drops them, `rdx-keys` keeps
  its own encryption over `bc-components` (`SymmetricKey::encrypt`, `SealedMessage`); the tests
  pin the behaviour.
- The local host's passkey is a file under the state directory: development only.
- PRF support varies by browser and authenticator; the PWA (piece 14) needs a fallback, which can
  be recovery shares.
