# ADR-P13: A minimal claim page on the intake Worker

**Status:** Proposed
**Date:** 2026-11-22
**Deciders:** Ash

## Context

Email intake (ADR-P13, email intake) is live: an allowed sender without keys gets a "Verify your
address" reply whose link (`<claim_url>#address=…&key=…`) holds the quarantine key. Opening it
was left to the PWA (piece 14), which also owed the Worker routes it would call and a proof of the
mailbox before a registry is published. Without a page the link leads nowhere and the
quarantined mail expires after seven days. Piece 14 is not started; the intake needs to work end
to end now, with the smallest page that keeps every rule: keys made client-side under a passkey,
servers never decrypt, registries published only with proof of the mailbox.

## Decision

**The intake Worker serves the page and its API.** The page is static assets on the same Worker
(`[assets]` in `wrangler.toml`): `claim.html` plus the WebAssembly of a new crate, `rdx-claim`,
built by `cargo xtask claim-page` before `worker-build`. Cloudflare serves the files without
running the Worker; the Worker's new `fetch` handler answers only `POST /claim/open` and
`POST /claim/release`. The policy's `claim_url` points at the Worker: its `workers.dev` address, or
a custom domain on the organization's zone (for Costa Combat, `app.costacombat.com`, which the
first replies already link to).

**Every request is signed by the quarantine key.** The server kept the quarantine key's public
half (`intake/quarantine/<address>`), so a request signed with the link's key proves the sender
read the reply, which only their mailbox received. That is the proof of the mailbox the intake
ADR asked for. Requests are envelopes (`'isA': rdx.intake.claim.v1`, subject the address,
`action`, `at`), signed with the key's Ed25519 half; `at` must be within 15 minutes of the
server's clock.

1. **open** returns the quarantine's entries and envelopes, still encrypted to the link's key.
2. The page makes a passkey (WebAuthn PRF, salt `rdx.passkey.v1`) and runs `keys.onboard` in the
   browser: a device key wrapped by the passkey, a recovery key whose seed becomes one SSKR share
   shown once, and a registry signed by the recovery key. It then decrypts each envelope with the
   link's key and encrypts it to the registry's keys (`rdx_intake::claim::reseal`).
3. **release** carries the registry and the new envelopes. The signed request names each move
   (`from` quarantined id, `to` new digest), so the server checks the envelopes are the signed
   ones, that every waiting document is accounted for (mail that arrived since `open` makes it
   fail: reload), that the registry checks out and is for this address, and that the address has
   no registry yet. Then it stores the envelopes, the registry and the inbox entries, and deletes
   the quarantine and its blobs. A replay finds no quarantine.

The checks are `rdx_intake::claim` (no I/O, tested natively and as WebAssembly); the Worker stages
KV around them as it does for `receive`, with `open_keys` and `release_keys`.

**Smallest onboarding.** One device (`first-device`), one recovery share held by the sender
(threshold 1 of 1), supporter id derived from the address. The wrapped device key stays in the
browser's `localStorage` (`rdx/keys/device/<address>`), ciphertext only. Holders, more devices,
organization-assigned supporter ids and reading the inbox are piece 14's.

## Alternatives considered

- **Wait for the PWA.** The live intake's quarantines would keep expiring unclaimed.
- **A separate Pages site or Worker for the page.** A second deploy and a cross-origin API, for
  a page that only exists to talk to this Worker.
- **Release on the server with the link's key.** The browser would send the quarantine key to the
  server, which could then decrypt: breaks "servers never decrypt".
- **Unsigned requests, trusting possession of the link.** Anyone could publish a registry for an
  allowed address that has mail waiting (the address is not secret) and receive its future mail.
- **A one-time code by email instead of the link's key.** A second email and a second secret, when
  the link already carries one the server can verify.
- **Leptos or another UI framework.** The page is one button and three messages; the PWA (piece
  14) brings the framework.

## Consequences

- A setup link now works end to end: claim, then the sender's next email is delivered to their
  keys.
- The page needs a passkey provider with the PRF extension; without it the page says so and
  nothing changes (the mail still waits until expiry).
- The sender's only copy of the recovery seed is the share the page shows once. Losing both the
  passkey and the share loses access to that mail; the PWA should let them re-split it with
  holders (piece 14).
- The Worker grows by the claim routes (no new dependencies); the page is about 1.5 MB of
  WebAssembly, loaded only by people opening a setup link.
- Piece 14 replaces `/claim` and should read the device record the page left in `localStorage`
  on the same origin, or recover with the share.
