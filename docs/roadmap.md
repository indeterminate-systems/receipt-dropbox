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
| 7 | **Gordian Envelope statements and redaction** | One salted assertion per line, signing, elision of unmatched lines with the root digest unchanged, verification, encryption to recipients; `bc-envelope` on `wasm32-unknown-unknown` | `verify-statement` | 3 |
| 8 | **Keys and recovery** | Passkey PRF to seed derivation, device keys, address-to-keys registry with rotation and revocation, SSKR shares, re-encryption when a key is added | `onboard-supporter`, `recover-access` | 2, 7 |
| 9 | **Beancount ledger and exports** | `rdx-ledger` writer with `receipt:` and `statement:` metadata, balance assertions per statement, round-trip tests with a Beancount parser, CSV and XLSX export | `reconcile-statement` (posting step), `export-ledger` | 3, 6 |
| 10 | **Typst rendering** | Typst compiled into the core, a statement template fed only from elided envelope data: "Personal, redacted" rows, opening and closing balances, total redacted, root digest, UR/QR | `publish-redacted-statement` | 7 |
| 11 | **OCR fallback** | Pure-Rust OCR (for example `ocrs`) behind the `ocr` capability, used when a PDF has no text layer | adds a step to `ingest-statement` | 4 |
| 12 | **Audit trail and grants** | Append-only audit log linking ledger entries and viewable rows to receipts and encrypted originals; uploader-granted access to an original for an accountant | `grant-original`, `audit-trail` | 7, 9 |
| 13 | **Email intake on Cloudflare** | Allowlist and SPF/DKIM/DMARC checks, encrypt-on-arrival to sender keys, quarantine key with expiry, one rate-limited verification reply, expiry cleanup; a `workers-rs` host with R2 and KV. Email Routing hands the message to the Worker in memory, so plaintext never rests anywhere. (A hosted inbox such as AgentMail was considered: it stores messages in plaintext by default, which breaks "plaintext discarded"; it could be a later `intake-adapter` if deletion is shown to purge attachments.) | `triage-quarantine` | 2, 8 |
| 14 | **PWA** | Leptos client-side app in Rust: passkey unlock, OPFS storage, upload folder, match review, publishing, accountant view; installable and offline | – (people use it; agents use skills) | 2, 8; integrates 4 to 10 |
| 15 | **Fiscal proof and reimbursement claims** | A per-organization **proof policy** ([compliance](compliance.md#it-depends-on-what-kind-of-organization-it-is) R0): `company`, `nonprofit`, `donataria-autorizada`, `individual` (persona física, with a `sat-registered` flag; employees are supporters, payroll out of scope) or `informal-group` (no RFC: the receipt, or a CFDI to the supporter or *público en general*, is the proof). The policy picks which of R1 to R10 apply: CFDI 4.0 XML capture and parsing, receptor checks against the organization's fiscal profile, SAT status and art. 69-B checks as a host capability (re-run at month close and before the annual return), CFDI ↔ statement line ↔ reimbursement transfer matching, cash and fuel flags, fiscal ledger metadata, claims with approval and deadlines, viáticos export for payroll, a US accountable-plan profile; for a donataria, donation receipts, the administration cap and transparency report data; foreign vendors' invoices as proof with IVA and withholding flags (R17); thresholds and policies as Nickel per fiscal year | `verify-cfdi`, `claim-reimbursement` | 3, 5, 6, 9 |
| 16 | **Supporter data rights and retention** | [Compliance](compliance.md) R11 to R15: privacy notice and recorded consent (LFPDPPP), ARCO requests (export, correct, delete, object) from the PWA, partial and pre-redacted statement uploads, retention per document class with legal hold (delete originals, keep digests and redacted envelopes), worker-classification signals | `retention-sweep`, `handle-data-request` | 7, 8, 12 |

## Later (not scheduled)

| Piece | Delivers | Depends on |
| :-- | :-- | :-- |
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
wave 2 (now, in parallel):   4   5   6   7
wave 3:                      8   9   10  11
wave 4:                      12  13  14  15
wave 5:                      16
```

Done: piece 1 (#9), piece 2 (#10) and piece 3 (#4, #8), merged by 2026-11-20. The compliance
research behind pieces 15 and 16 is in (#11, #12, #15); their code is not.

Piece 15 is what makes a reimbursement hold up for tax (see [compliance](compliance.md)). Its
proof policy is per organization, so a group with no RFC still gets matching, redaction and the
audit trail, just without the CFDI requirement. The open questions listed in compliance go to a
contador before it ships.

Pieces in a wave don't depend on each other. Within a wave, start with the piece most others wait
on (2 before 4 and 5; 7 before 8 and 10).
