# AGENTS.md

Receipt Dropbox: local-first expense reconciliation for organizations whose supporters pay costs
from personal accounts. Statements are matched to receipts, matches are booked to a Beancount
ledger, originals stay encrypted, and viewable statements are redacted with Gordian Envelope
elision. Read [docs/architecture.md](docs/architecture.md) before changing anything structural.

## Ground rules

- **Rust only, and it must build for WebAssembly.** Every crate under `crates/` builds for
  `wasm32-unknown-unknown` and `wasm32-wasip2`. Core crates do no I/O: they get storage, mail,
  time and randomness through host capabilities. No JavaScript, Python or shell in the product;
  repo tooling is `cargo xtask`.
- **Nickel describes, Rust does.** Config, policy and agent plans are Nickel checked by contracts
  (`nickel/rdx/`), evaluated with `nickel-embed`.
- **Servers never decrypt.** Only the PWA's passkey-derived keys can. Never log, store or test
  with plaintext statements on a server path, and never commit real financial data: `fixtures/`
  is synthetic only.
- **Test first.** Follow [docs/workflow.md](docs/workflow.md): contract, failing test, op, skill,
  skill test.

## Build and test

| Command | What it does |
| :-- | :-- |
| `cargo test` | all unit and integration tests, natively |
| `cargo run -p rdx-cli -- run <skill> [--input FIELD=VALUE] [--execute]` | runs a skill's plan on the local host (dry run unless `--execute`); see `rdx --help` |
| `cargo check -p <crate> --target wasm32-unknown-unknown` (and `wasm32-wasip2`) | the WebAssembly builds CI requires |
| `cargo xtask check [SKILL…]` | validates skills against the Agent Skills spec and repo rules, evaluates each product skill's `scripts/main.ncl` and compares its dry-run reports with `tests/*.json` |
| `UPDATE_GOLDEN=1 cargo xtask check <skill>` | accepts new golden reports (review the diff) |
| `cargo xtask check-wit` | the capability interfaces in `wit/rdx.wit` match the `rdx-host` traits |
| `cargo xtask smoke-wasm` | builds `rdx` for `wasm32-wasip2` and runs `plan-smoke-test` with it under `wasmtime` |
| `cargo fmt`, `cargo clippy --all-targets` | formatting and lints; CI runs both |

The dev shell (`nix develop`, or `direnv allow`) provides the toolchain with both wasm targets.

## Layout

| Path | Contents |
| :-- | :-- |
| `crates/rdx-host` | host capability traits (blob store, kv, mail-out, clock, random, audit log), in-memory and local (directory) implementations |
| `crates/rdx-plan` | plans (code-mode entrypoint): types, validation, executor, built-in ops, audit chain, Nickel loading |
| `crates/rdx-cli` | `rdx`, the local host: `rdx run <skill>`, `rdx audit verify` |
| `crates/rdx-model` | domain model (money, dates, statements, receipts, matches, ledger entries) and the synthetic data generator (`rdx_model::synth`) |
| `nickel/rdx/api.ncl` | plan contracts and op constructors agents write against |
| `nickel/rdx/model.ncl` | contracts for the domain model's JSON, for op contracts to reuse |
| `fixtures/synthetic/` | generated fixtures; `UPDATE_GOLDEN=1 cargo test -p rdx-model --test fixtures` rewrites them |
| `wit/rdx.wit` | capability and extension interfaces (mirrors the Rust traits) |
| `.agents/skills/` | Agent Skills; `.claude/skills` links here |
| `xtask/` | repo checks |
| `docs/` | architecture, workflow, roadmap, ADRs, references |

## Code mode

Agents act on the system by running a skill's `scripts/main.ncl`, which evaluates to a plan, with
`rdx run <skill>`. Don't
add one-off tools or scripts that bypass plans: add an op (Rust `Op` + constructor in `api.ncl`)
and use it from a skill. `rdx run` always dry-runs unless given `--execute`: show the person the
dry-run report (skipped steps list the arguments they would use) before running anything with
effects. An op that touches the outside world declares the capabilities it needs (`Op::needs`) and
reaches them only through `RunContext::host`.

## Code style

- Money is integer minor units plus a currency code, never floating point.
- Digests are `sha256:<hex>` of canonical JSON (`rdx_plan::digest`) unless a format (Gordian
  Envelope) defines its own.
- Comments say why, not what. Pin anything fetched (git dependencies by `rev`), and say in a
  comment when a workaround can be dropped.
- Errors an agent may see carry enough context to fix the input (Nickel contract errors already
  carry source locations).

## Commits and pull requests

- Work on a branch and open a pull request against `main`.
- Commit subjects start with the area: `plan: …`, `skills: …`, `docs: …`, `CI: …`.
- Record decisions with real alternatives as ADRs in `docs/adr/`.

## Reference

- [Agent Skills quick reference](docs/reference/agent-skills.md)
- [Nickel quick reference](docs/reference/nickel.md) (with the Context7 ids used to refresh it)
- For anything the quick references don't cover, query Context7 (configured in `.mcp.json`) with
  `/websites/nickel-lang_stdlib` or `/websites/nickel-lang` rather than answering from memory, and
  add what you learn to the quick reference.
- [Roadmap](docs/roadmap.md): the functionality pieces and their order
