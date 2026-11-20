# ADR-0005: Statement envelopes, and Gordian Envelope without C

**Status:** Proposed
**Date:** 2026-11-20
**Deciders:** Ash

## Context

Roadmap piece 7 turns a parsed statement into a signed Gordian Envelope that can be redacted by
elision: unmatched lines disappear, the root digest and signature stay valid, and an accountant
can still check that the shown lines, the balances and one "total redacted" figure reconcile.
The core must build for `wasm32-unknown-unknown` and `wasm32-wasip2` and does no I/O, randomness
included.

Two problems surfaced building `bc-envelope` 0.43 for WebAssembly:

1. `bc-crypto`'s default features link `secp256k1-sys`, which is C. `bc-envelope` and
   `bc-components` turn those defaults off, but `bc-shamir` (through `sskr`, which
   `bc-components` always depends on) leaves them on, and never uses them.
2. `getrandom` 0.2 and 0.3, reached through the RustCrypto crates, refuse to build for the browser
   target until a backend is chosen.

## Decision

**Layout.** One envelope per statement, subject the statement id, `'isA': "rdx.statement.v1"`,
header assertions (supporter, institution, account mask, currency, period, opening and closing
balances in minor units, the encrypted original's digest, `lineCount`) and one `"line"` assertion
per line. Each line is its own envelope (id, position, date, description, amount, reference)
with a salt, so an elided line's digest can't be confirmed by guessing its contents. The whole is
wrapped and signed with Ed25519 (deterministic, so fixtures are reproducible).

**Redaction** elides whole `"line"` assertions. Nothing about an elided line survives, not its
date or position: the viewable statement shows one "Personal, redacted" row per elided line.
**Verification** derives the total redacted as `closing - opening - shown`. That figure is only
right if the statement balanced when signed, so sealing refuses an unbalanced statement.

**No C.** `vendor/bc-shamir` is the published 0.13.0 with one change, `bc-crypto` without default
features, patched in through `[patch.crates-io]`; it goes when a release upstream does the same.
`bc-envelope` and `bc-components` are built with only the features statements use (`salt`,
`signature`, `ed25519`, `known_value`).

**No platform randomness.** Salt comes from the caller's generator (`add_salt_using`), which hosts
back with their `random` capability. In the browser build getrandom's backends always fail:
`getrandom_backend="unsupported"` for 0.3 (`.cargo/config.toml`) and a custom function returning
an error for 0.2 (`rdx-envelope/src/no_system_rng.rs`). An accidental call is an error, not a
silent dependency on the platform.

**Scope.** Sealing needs the supporter's key, and encrypting to recipients needs fresh content
keys and ephemeral keys, which `bc-envelope` draws from the system generator. Both move to piece 8
(keys), which can thread the host's randomness through or pick a browser backend for getrandom.

## Options considered

| Option | For | Against |
| :-- | :-- | :-- |
| Vendor `bc-shamir` with one changed line (chosen) | Pure Rust; no toolchain change; the same build everywhere | 900 lines of vendored code to drop later |
| Build `secp256k1-sys` for wasm | No vendored code | Needs clang with a wasm sysroot in the dev shell, in both repos' flakes and in every PWA and Worker build; `secp256k1-sys` 0.11's wasm sysroot also lacks `memmove`, so a `CFLAGS` workaround too |
| getrandom's JS backend (`wasm_js`) | Real randomness in the browser | Ties core crates to `wasm-bindgen` and the platform, against "capabilities come from the host" |
| One salt per field instead of per line | Could later reveal a line's amount but hide its description | No requirement for it yet; it can be a `rdx.statement.v2` |

## Consequences

- Statements travel as `ur:envelope/…` strings, keys as `ur:signing-public-key/…`.
- The root digest is printed as `sha256:<hex>`: Gordian Envelope digests are SHA-256, so it is the
  same value the envelope tools show.
- Piece 10 renders from `envelope.verify`'s output only, never from the full statement.
- Piece 16's retention sweep can delete an encrypted original and keep the redacted envelope: its
  root digest still links ledger entries to what was signed.
