# Receipt Dropbox architecture

Status: proposed (2026-11-19). Decisions with real alternatives are recorded as ADRs in [`adr/`](adr/).

## What the system does

Supporters (employees, volunteers) pay organization expenses from personal accounts. They drop
bank statements and receipts into Receipt Dropbox. The system matches statement lines to receipts,
books the matched ones into a plain-text double-entry ledger, keeps the original statement
encrypted, and publishes a redacted statement that shows only receipt-confirmed organization
transactions. Every other line appears as "Personal, redacted", and a single "total redacted"
figure makes the opening and closing balances reconcile. Servers can encrypt but never decrypt.

## Principles

1. **One core, many hosts.** All domain logic is Rust compiled to WebAssembly. The same core runs
   in the PWA, on Cloudflare Workers, on Google Cloud Run and locally under wasmtime. Hosts only
   supply capabilities (storage, mail, clock, randomness, UI).
2. **Rust is the only extension language.** Every extension point is a Rust trait with a matching
   WIT interface. An extension is a Rust crate that compiles both ways: linked statically into
   browser and Worker builds, or loaded as a WebAssembly component by wasmtime hosts. See
   [ADR-0001](adr/0001-rust-wasm-core-and-extensions.md).
3. **Nickel describes, Rust does.** Configuration, policy (matching tolerances, allowlists,
   retention) and agent workflows are Nickel, checked by contracts before anything runs. Rust
   evaluates Nickel through [`nickel-embed`](https://github.com/costacombat/rust-nickel).
4. **Code mode for agents.** Agents don't call dozens of tools. Each skill has one entrypoint: a
   Nickel program that evaluates to a typed **plan**, which the core validates and executes. See
   [ADR-0002](adr/0002-nickel-code-mode-plans.md).
5. **Local-first and zero-knowledge servers.** Decryption happens only on a supporter's device,
   with keys derived from a passkey. Servers handle ciphertext, public keys and digests.
6. **Everything is auditable.** Every ledger entry and every row of a viewable statement links to
   its receipt and to the digest of the encrypted original. Every plan run records the plan's own
   digest.

## System context

```
 Supporter ──(PWA: passkey)──┐                     ┌── Accountant (PWA, read-only; sees an
 Supporter ──(email)───┐     │                     │    original only when the uploader grants it)
                       ▼     ▼                     │
              ┌────────────────────────┐           │
              │   Hosts                │           │
              │  • PWA (browser, OPFS) │◄──────────┘
              │  • CF Worker (+R2, KV, │
              │    Email Routing)      │
              │  • Cloud Run (+Drive,  │
              │    Gmail push)         │
              │  • Local (wasmtime CLI)│
              └──────────┬─────────────┘
                         │ capabilities (WIT "host" interfaces)
              ┌──────────▼─────────────┐
              │  rdx core (Rust→WASM)  │
              │  plan executor + ops   │
              └──────────┬─────────────┘
                         │ extension interfaces (Rust traits = WIT worlds)
     statement parsers · receipt extractors · matchers · ledger writers · intake adapters
```

## Layers

### 1. Domain and core crates (pure, no I/O)

Each crate compiles for `wasm32-unknown-unknown` (browser, Workers) and `wasm32-wasip2`
(components, wasmtime), and natively for tests. None touches the network, the file system or a
clock directly; those come through the capability traits in layer 2.

| Crate | Responsibility |
| :-- | :-- |
| `rdx-model` | Money (integer minor units plus currency), `Statement`, `StatementLine`, `Receipt`, `Match`, `Supporter`, identifiers and digests |
| `rdx-plan` | Plan types, plan validation and the executor. **Scaffolded in this PR.** |
| `rdx-config` | Loads Nickel configuration and policy through `nickel-embed`, against the contracts in `nickel/rdx/` |
| `rdx-statement` | PDF text-layer extraction and statement line parsing; OCR fallback through the `ocr` capability |
| `rdx-receipt` | Receipt and emailed-bill extraction (amount, date, merchant, reference) |
| `rdx-match` | Candidate generation and scoring between statement lines and receipts, with tolerances from policy |
| `rdx-envelope` | Gordian Envelope: one salted assertion per transaction, signing, elision of unmatched lines, encryption to recipients (`bc-envelope`, `bc-components`) |
| `rdx-keys` | Key hierarchy: passkey PRF to seed, device keys, SSKR recovery shares (`sskr`), rotation and revocation records |
| `rdx-ledger` | Beancount writer and reader, with audit metadata on every entry. See [ADR-0003](adr/0003-beancount-ledger.md) |
| `rdx-render` | Typst rendering of elided envelope data, including the root digest and a UR/QR of the envelope |
| `rdx-intake` | Intake rules: allowlist, SPF/DKIM/DMARC results, quarantine lifecycle, rate-limited replies |
| `rdx-audit` | Append-only audit log: plan digests, input and output digests, who granted what |

### 2. Capabilities (what a host provides)

Defined once as Rust traits in the `rdx-host` crate and mirrored in [`wit/rdx.wit`](../wit/rdx.wit);
`cargo xtask check-wit` fails when the two drift apart. A host is an `rdx_host::Host`: one optional
implementation per capability. Each op declares the capabilities it needs (`Op::needs`), so a plan
that needs one the host lacks is rejected before any step runs. Every run is appended to the
host's audit log as a hash chain (`rdx_plan::run`, `rdx audit verify`).

| Capability | PWA | Cloudflare | Google | Local |
| :-- | :-- | :-- | :-- | :-- |
| `blob-store` | OPFS | R2 | Drive | directory |
| `kv` | IndexedDB | Workers KV / D1 | Firestore | SQLite or directory |
| `mail-out` | none | Email Workers send | Gmail API | outbox directory (never sent) |
| `clock`, `random` | Web APIs | Workers runtime | WASI | WASI |
| `secrets` | WebAuthn PRF unwrap | none (encrypt only) | none (encrypt only) | passkey or OS keychain |
| `ocr` | Tesseract WASM | not offered | not offered | Tesseract WASM under wasmtime |

The PWA is the only host with a `secrets` capability that can decrypt. A server host that is
asked to run a step needing decryption fails the plan at validation time, before any effect.

### 3. Extensions

An extension implements one trait from the table below. The trait and its WIT interface carry the
same names, so the crate can be built two ways:

- **Statically linked** (default for the PWA and Workers): the host crate depends on the extension
  crate behind a Cargo feature, chosen from the deployment's Nickel config at build time.
- **As a component** (wasmtime hosts): `cargo build --target wasm32-wasip2` with the crate's
  `component` feature, which wraps the trait in `wit-bindgen` exports. The local and Cloud Run
  hosts load components listed in config, sandboxed, with only the capabilities config grants.

| Extension point | Example extensions |
| :-- | :-- |
| `statement-parser` | one per bank or statement layout; CSV, OFX, CAMT.053 |
| `receipt-extractor` | generic PDF/HTML bill, vendor-specific layouts |
| `match-strategy` | exact amount and date window, fuzzy merchant, split payments |
| `ledger-writer` | Beancount (default); hledger or Ledger later |
| `intake-adapter` | upload folder, email (Workers, Gmail), Drive folder |

### 4. Hosts

| Host | Build | Notes |
| :-- | :-- | :-- |
| PWA | Rust UI (Leptos, client-side rendering) + core, `wasm32-unknown-unknown` via `wasm-bindgen` | WebAuthn PRF through `web-sys`; OPFS storage; Typst and Tesseract run in the page. The service worker is generated, not hand-written. |
| Cloudflare | `worker` crate (`workers-rs`) | Email Routing hands mail to the Worker; encrypt on arrival to the sender's public keys from KV, store in R2, discard plaintext. |
| Google (later) | Cloud Run service embedding wasmtime | Gmail push notifications through Pub/Sub; Drive folders as intake and blob store. Not scheduled; see the roadmap. |
| Local | `rdx` (crate `rdx-cli`), built natively and as `rdx.wasm`, a WASI command that `wasmtime run` executes | Runs plans from the command line; the agents' code-mode entrypoint. State (blobs, kv, outbox, `audit.jsonl`) lives in `.rdx/`. `cargo xtask smoke-wasm` runs the smoke-test skill through `rdx.wasm` in CI. |

The local host is itself a WebAssembly program: `rdx` compiles to `wasm32-wasip2` and runs under
stock `wasmtime`, with its state directory as a WASI preopen, so the behaviour under test is the
WebAssembly core. The native build of the same source is a development convenience. A host that
embeds wasmtime as a library (the Cloud Run service, or `rdx` once it loads component extensions)
is added with the first extension built as a component; until then, nothing needs it.

## Code mode: plans

A **plan** is the single typed entrypoint for agents and for automation. A skill's
`scripts/main.ncl` imports the API in [`nickel/rdx/api.ncl`](../nickel/rdx/api.ncl) and evaluates
to a plan:

```nickel
let rdx = import "rdx/api.ncl" in
rdx.plan {
  skill = "reconcile-statement",
  goal = "Match March statement lines to receipts and book the matches",
  steps = [
    rdx.ops.statement.parse { id = "stmt", input = "inbox/2026-03.pdf" },
    rdx.ops.receipt.collect { id = "rcpts", since = "2026-02-25" },
    rdx.ops.match.run { id = "matches", statement = 'ref "stmt", receipts = 'ref "rcpts" },
    rdx.ops.ledger.post { matches = 'ref "matches" },
  ],
}
```

1. **Evaluate.** `nickel-embed` evaluates the program. Contracts check each step's arguments, that
   step ids are unique and that every reference points to an earlier step. Errors carry source
   locations from `nickel-embed`'s structured diagnostics, so an agent can fix its plan without
   running anything.
2. **Validate.** The executor checks every op exists in the host's registry and that the host has
   the capabilities each op needs.
3. **Execute.** Plans run as a dry run by default: ops marked as having effects report what they
   would do. `dry_run = false` performs them. The run report records the plan digest, each step's
   input and output digests and any skipped effects, and goes to the audit log.

A new op is added in three places, test first: a Rust `Op` implementation with unit tests, its
constructor and argument contract in `api.ncl`, and a golden plan test. A skill is then a goal
composed from ops: see [the TDD to skills workflow](workflow.md).

## Data flows

### Statement upload (PWA)

1. The supporter picks a PDF. The core hashes it, encrypts it to the supporter's device keys and
   writes ciphertext to the blob store. The ciphertext digest becomes the statement's audit id.
2. The text layer is read; if it is empty, OCR runs in the page.
3. A statement-parser extension turns text into lines with opening and closing balances.
4. Each line becomes a salted assertion in a signed Gordian Envelope.

### Email intake (Cloudflare or Google)

1. Check SPF/DKIM/DMARC results and the org allowlist. Reject anything not allowlisted.
2. Known sender: encrypt each attachment to every active device key for that address, store
   ciphertext, discard plaintext.
3. Unknown but allowlisted sender: encrypt to a quarantine key with an expiry, reply once (rate
   limited) with a PWA link to verify the address and create keys. On verification the PWA
   re-encrypts to the new keys; on expiry the quarantine key is destroyed, the document deleted
   and the sender notified.

### Reconciliation and publishing (PWA)

1. Decrypt locally, match lines against receipts with policy tolerances.
2. Post matched lines to the Beancount ledger with `receipt:` and `statement:` digest metadata.
3. Elide unmatched assertions (the root digest is unchanged), render the viewable statement with
   Typst from the elided envelope, showing "Personal, redacted" rows, opening and closing
   balances, a total redacted figure, the root digest and a UR/QR for verification.
4. Accountants see the viewable statement and ledger. An original is shared only when the uploader
   grants it, by encrypting the envelope to the accountant's key.

## Key management

- A passkey with the WebAuthn PRF extension yields a per-device secret; the core derives a
  wrapping key from it and unwraps the supporter's seed. Users never see keys.
- Each device has its own key pair; the address-to-keys map in `kv` lists active keys and records
  rotations and revocations, signed by an existing device key.
- Recovery: the seed is split with SSKR into shares (for example 2-of-3: another device, a printed
  card, an org custodian). The custodian's share alone cannot decrypt.

## Repository layout

```
AGENTS.md              agent instructions (CLAUDE.md imports it)
docs/
  architecture.md      this file
  workflow.md          the TDD to skills workflow
  roadmap.md           functionality pieces and their dependencies
  adr/                 decision records
  reference/           quick references: Agent Skills spec, Nickel
crates/                Rust crates (rdx-plan today)
nickel/rdx/            Nickel API and contracts for plans and config
wit/                   WIT interfaces for capabilities and extensions
.agents/skills/        Agent Skills (.claude/skills links here)
xtask/                 cargo xtask: repo checks written in Rust
fixtures/              test statements, receipts and golden outputs (synthetic only)
```

## Risks and open questions

- **Nickel in the browser.** `nickel-embed` (on `nickel-lang-core` 0.19, default features)
  type-checks for both `wasm32-unknown-unknown` and `wasm32-wasip2` (checked 2026-11-19), and CI
  keeps checking `rdx-plan` for both. Binary size in the PWA needs watching; if it is too large,
  the PWA can load plans as pre-evaluated JSON and keep Nickel for local and server hosts.
- **WebAuthn PRF support** varies by browser and authenticator. The PWA needs a fallback path
  (for example a recovery-share unlock) and a support matrix.
- **Gordian crates on `wasm32-unknown-unknown`** need checking for randomness (`getrandom`'s `js`
  backend) early in the envelope thread.
- **Statement formats** vary by bank. The parser extension point exists for this; the first
  thread should collect real (anonymized) layouts before tuning.
- **Workers email limits** (message size, CPU time for encryption of large PDFs) need measuring.
