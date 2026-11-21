# Roadmap: functionality pieces

Each piece is sized for one thread and one pull request (or a short series). Each one follows
[the TDD to skills workflow](workflow.md): contracts and failing tests first, then ops, then the
skill named in the table (if any), with `cargo test`, the `wasm32` checks and `cargo xtask check`
green.

| # | Piece | Delivers | Skill | Depends on |
| :-- | :-- | :-- | :-- | :-- |
| 1 | **Dev environment and CI** | `flake.nix` dev shell (Rust with both wasm targets, wasmtime, wasm-bindgen-cli, Nickel, Typst), a matching template in nix-dev-environment-templates, CI for tests, wasm builds and skill checks | `op-author` (dev) | – |
| 2 | **Host layer and `rdx` CLI** | `rdx-host` capability traits mirrored in `wit/rdx.wit` (blob store, kv, clock, random, mail-out), a local host, `rdx run <skill> --input k=v [--execute]` as the code-mode entrypoint, run reports to an audit log | `plan-smoke-test` runs through `rdx` | – |
| 3 | **Domain model and fixtures** | `rdx-model`: money in minor units with currency, statements, lines, balances, receipts, supporters, digests; a synthetic statement and receipt generator for tests. Receipts carry an optional fiscal id (CFDI UUID, issuer and receiver RFC, payment form) supporters a kind (employee, volunteer, contractor), and the organization an optional fiscal profile (RFC, regime) since an informal group has none, so piece 15 doesn't reshape the model | – | – |
| 4 | **Statement parsing** | PDF text-layer extraction in Rust, the `statement-parser` extension point, CSV and OFX parsers, one PDF layout, balance checks (opening + lines = closing) | `ingest-statement` | 2, 3 |
| 5 | **Receipt and bill extraction** | Amount, date, merchant and reference from PDF, HTML and email bodies; the `receipt-extractor` extension point | `ingest-receipt` | 2, 3 |
| 6 | **Matching engine** | Candidate generation and scoring, tolerances (amount, date window, merchant similarity) as Nickel policy, split and partial payments flagged for review; amounts in different currencies matched at a policy exchange rate and tolerance ([compliance](compliance.md) R16) | `reconcile-statement` (dry run: proposed matches) | 3 (fixtures from 4 and 5 help) |
| 7 | **Gordian Envelope statements and redaction** | `rdx-envelope`: one salted assertion per line, Ed25519 signing, elision of unmatched lines with the root digest unchanged, verification that derives the total redacted from the signed balances; `envelope.redact` and `envelope.verify` ops; `bc-envelope` on both wasm targets without C ([ADR-P07](adr/ADR-P07-statement-envelopes.md)) | `verify-statement` | 3 |
| 8 | **Keys and recovery** | Passkey PRF to seed derivation, device keys, address-to-keys registry with rotation and revocation, SSKR shares, re-encryption when a key is added; sealing a statement with the supporter's key (an `envelope.seal` op) and encrypting envelopes to recipients, both moved here from piece 7 because they need keys and the host's randomness | `onboard-supporter`, `recover-access` | 2, 7 |
| 9 | **Beancount ledger and exports** | `rdx-ledger` writer with `receipt:` and `statement:` metadata, balance assertions per statement, round-trip tests with a Beancount parser, CSV and XLSX export | `reconcile-statement` (posting step), `export-ledger` | 3, 6 |
| 10 | **Typst rendering** | Typst compiled into the core, a statement template fed only from elided envelope data: "Personal, redacted" rows, opening and closing balances, total redacted, root digest, UR/QR | `publish-redacted-statement` | 7 |
| 11 | **OCR fallback** | Pure-Rust OCR (`ocrs`) as the `ocr.read` op, used when a PDF has no text layer and for photos | adds a step to `ingest-statement` | 4 |
| 12 | **Audit trail and grants** | Append-only audit log linking ledger entries and viewable rows to receipts and encrypted originals; uploader-granted access to an original for an accountant | `grant-original`, `audit-trail` | 7, 9 |
| 13 | **Email intake on Cloudflare** | Allowlist and SPF/DKIM/DMARC checks, encrypt-on-arrival to sender keys, quarantine key with expiry, one rate-limited verification reply, expiry cleanup; a `workers-rs` host with R2 and KV. Email Routing hands the message to the Worker in memory, so plaintext never rests anywhere. (A hosted inbox such as AgentMail was considered: it stores messages in plaintext by default, which breaks "plaintext discarded"; it could be a later `intake-adapter` if deletion is shown to purge attachments.) The Worker never links `rdx-render` (piece 10) and runs no OCR (piece 11): both need plaintext. | `triage-quarantine` | 2, 8 |
| 14 | **PWA** | Leptos client-side app in Rust: passkey unlock, OPFS storage, upload folder, match review, publishing, accountant view; installable and offline. Loads the Typst renderer (piece 10) only when someone publishes; fetches and caches the OCR models (piece 11) by digest and shows their CC-BY-SA-4.0 attribution | – (people use it; agents use skills) | 2, 8; integrates 4 to 10 |
| 15 | **Fiscal proof and reimbursement claims** | A per-organization **proof policy** ([compliance](compliance.md#it-depends-on-what-kind-of-organization-it-is) R0): `company`, `nonprofit`, `donataria-autorizada`, `individual` (persona física, with a `sat-registered` flag; employees are supporters, payroll out of scope) or `informal-group` (no RFC: the receipt, or a CFDI to the supporter or *público en general*, is the proof). The policy picks which of R1 to R10 apply: CFDI 4.0 XML capture and parsing, receptor checks against the organization's fiscal profile, SAT status and art. 69-B checks as a host capability (re-run at month close and before the annual return), CFDI ↔ statement line ↔ reimbursement transfer matching, cash and fuel flags, fiscal ledger metadata, claims with approval and deadlines, viáticos export for payroll, a US accountable-plan profile; for a donataria, donation receipts, the administration cap and transparency report data; foreign vendors' invoices as proof with IVA and withholding flags (R17); thresholds and policies as Nickel per fiscal year | `verify-cfdi`, `claim-reimbursement` | 3, 5, 6, 9 |
| 16 | **Supporter data rights and retention** | [Compliance](compliance.md) R11 to R15: privacy notice and recorded consent (LFPDPPP), ARCO requests (export, correct, delete, object) from the PWA, partial and pre-redacted statement uploads, retention per document class with legal hold (delete originals, keep digests and redacted envelopes), worker-classification signals | `retention-sweep`, `handle-data-request` | 7, 8, 12 |

## Later (not scheduled)

| Piece | Delivers | Depends on |
| :-- | :-- | :-- |
| **Matching strategies as extensions** | The `match-strategy` extension point (architecture §3) as a trait and WIT interface, so an organization can add a strategy (for example per-merchant rules) without changing `rdx-match`; piece 6 built the strategies in. | 6 |
| **Real bank layouts** | A `statement-parser` per bank the organization's supporters use, written against anonymized samples (never committed: synthetic look-alikes go in `fixtures/`), starting with the Mexican banks. Found by piece 4, which ships one synthetic layout (`example-bank`) | 4 |
| **Encrypted PDFs** | Read PDFs with an owner password and an empty user password (RC4, AES-128/256), which many bank downloads are; `rdx-pdf` refuses them today ([ADR-P04](adr/ADR-P04-pdf-text-layer-reader.md)). Found by piece 4 | 4 |
| **Fuzzing the PDF reader** | `cargo fuzz` targets for `rdx_pdf::text` and the statement parsers, before the PWA takes uploads from the public. Found by piece 4 | 4, 14 |
| **Receipt extraction follow-ups** | Line items on receipts (`items` is empty today); scoring `receipt.extract` against the invoice benchmark below at a pinned revision, to tune its keywords | 5 |
| **OCR accuracy on real scans** | Score `ocr.read` and the parsers that use it against the SROIE and CORD receipt datasets below at a pinned revision; tune repairs and preprocessing from the results. Found by piece 11, whose results are on synthetic scans only | 11 |
| **More scan formats** | CCITT fax and JBIG2 images inside scanned PDFs, which `ocr.read` reports as skipped today ([ADR-P11](adr/ADR-P11-ocr.md)). Found by piece 11 | 11 |
| **Photo deskew** | Straighten rotated or perspective-skewed phone photos before OCR; beyond a degree or two of skew, reading fails today. Found by piece 11 | 11 |
| **OCR in skill goldens** | `cargo xtask check` evaluates skills without the OCR models, so the scan cases' goldens show the missing-models error; give xtask the models (from `$RDX_OCR_MODELS`) so goldens cover a real OCR read. Found by piece 11 | 11 |
| **Slimmer renderer** | Fewer fonts and a separately loaded render module, so the PWA downloads Typst only when someone publishes (release `rdx.wasm` is 66.5 MB with it, 20.1 MB without). Found by piece 10 | 10, 14 |
| **`rdx blob get`** | Read a blob (such as a rendered PDF) out of the local host's store by digest; today it is at `.rdx/blobs/<hex>`. Found by piece 10 | 2, 10 |
| **Google deployment** | Cloud Run service embedding wasmtime, Gmail push through Pub/Sub, Drive folders as intake and blob store. Deferred: no Google Cloud account yet. The host layer (piece 2) keeps it possible. | 13 |

## Test corpora

Public synthetic datasets on Hugging Face supplement the generated fixtures (piece 3). They are
fetched at a pinned revision by a test-only command, never committed, and only permissively
licensed ones are used:

| Dataset | License | Used by |
| :-- | :-- | :-- |
| [Bankstatemently/bank-statement-parsing-benchmark](https://hf.co/datasets/Bankstatemently/bank-statement-parsing-benchmark): synthetic statement PDFs, 12 countries, 8 languages (ground truth not published) | MIT | 4 |
| [drew-ipp/invoice-extraction-benchmark](https://hf.co/datasets/drew-ipp/invoice-extraction-benchmark): 181 synthetic invoices with answer keys | CC-BY-4.0 | 5 |
| [Voxel51/scanned_receipts](https://hf.co/datasets/Voxel51/scanned_receipts) (SROIE), [Voxel51/consolidated_receipt_dataset](https://hf.co/datasets/Voxel51/consolidated_receipt_dataset) (CORD): scanned receipts with labels | CC-BY-4.0 | 5, 11 |

None of them pairs statements with their receipts, so the matching tests (6, 9) rely on piece 3's
generator.

## Order

```
wave 1 (done):               1   2   3
wave 2 (done):               4   5   6   7
wave 3 (done):               8   9   10  11
wave 4 (next):               12  13  14  15
wave 5:                      16
```

Done, waves 1 and 2 merged by 2026-11-20, wave 3 by 2026-11-21:

- Piece 1 (#9), piece 2 (#10) and piece 3 (#4, #8).
- Piece 4 (#20): crates `rdx-pdf` (op `pdf.text`) and `rdx-statement` (op `statement.parse`:
  CSV, OFX and the synthetic `example-bank` PDF layout, with balance checks), skill
  `ingest-statement`; documents are blob digests ([ADR-P04](adr/ADR-P04-pdf-text-layer-reader.md)).
- Piece 5 (#18, #22): crate `rdx-receipt`, op `receipt.extract`, skill `ingest-receipt`, reading
  emails, PDFs (through piece 4's `pdf.text`), HTML, text and CFDI XML.
- Piece 6 (#17): crate `rdx-match`, op `matching.propose`, policy in `nickel/rdx/matching.ncl`,
  skill `reconcile-statement` (proposals only; piece 9 adds booking).
- Piece 7 (#19): crate `rdx-envelope`, ops `envelope.redact` and `envelope.verify`, skill
  `verify-statement`; sealing and encryption to recipients moved to piece 8
  ([ADR-P07](adr/ADR-P07-statement-envelopes.md)).

- Piece 8 (#24): crate `rdx-keys` (passkey-wrapped device keys, SSKR recovery, a key registry
  of signed Gordian Envelope events), the `secrets` host capability, ops `keys.*` and
  `envelope.seal`, `envelope.open`, `envelope.encrypt`, `envelope.add-recipients`; skills
  `onboard-supporter` and `recover-access` ([ADR-P08](adr/ADR-P08-keys-and-recovery.md)).
- Piece 9 (#25): crate `rdx-ledger`, ops `ledger.book`, `ledger.post`, `ledger.read` and
  `ledger.export` (CSV, XLSX); `reconcile-statement` books confirmed matches, new skill
  `export-ledger` ([ADR-P09](adr/ADR-P09-beancount-ledger.md)).
- Piece 10 (#28): crate `rdx-render` (Typst 0.15 as a library), op `render.statement`, skill
  `publish-redacted-statement`; reproducible PDFs with the root digest and the envelope as a QR
  code ([ADR-P10](adr/ADR-P10-typst-rendering.md)). The renderer roughly triples `rdx.wasm`, so
  the PWA loads it only to publish and the Worker never links it (Ash, 2026-11-21).
- Piece 11 (#27): crate `rdx-ocr` (ocrs, pure Rust), op `ocr.read`, with the models as blobs
  rather than a host capability; `ingest-statement` and `ingest-receipt` read scans and photos
  ([ADR-P11](adr/ADR-P11-ocr.md)). The models are CC-BY-SA-4.0; Ash accepted attribution and
  share-alike (2026-11-21).

The compliance research behind pieces 15 and 16 is in (#11, #12, #15); their code is not.

From wave 3 on, pieces follow AGENTS.md "Parallel work": a domain crate registers its own ops
and Nickel constructors (one line each in `crates/rdx-cli/Cargo.toml` and `nickel/rdx/ops.ncl`),
and shared docs such as this file change only in housekeeping PRs.

Piece 15 is what makes a reimbursement hold up for tax (see [compliance](compliance.md)). Its
proof policy is per organization, so a group with no RFC still gets matching, redaction and the
audit trail, just without the CFDI requirement. The open questions listed in compliance go to a
contador before it ships.

Pieces in a wave don't depend on each other. Within a wave, start with the piece most others wait
on (2 before 4 and 5; 7 before 8 and 10).
