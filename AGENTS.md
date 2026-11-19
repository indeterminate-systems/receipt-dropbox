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
| `cargo xtask doctor` | checks the shell: Rust version, both wasm targets, wasmtime, wasm-bindgen (matching `Cargo.lock`), Nickel, Typst |
| `cargo xtask ci` | everything CI runs, in CI's order; run it before pushing |
| `cargo test` | all unit and integration tests, natively |
| `cargo test -p <crate> --target wasm32-wasip2` | the same tests as WebAssembly, under wasmtime (runner in `.cargo/config.toml`) |
| `cargo check -p <crate> --target wasm32-unknown-unknown` (and `wasm32-wasip2`) | the WebAssembly builds CI requires |
| `cargo xtask check [SKILL…]` | validates skills against the Agent Skills spec and repo rules, evaluates each product skill's `scripts/main.ncl` and compares its dry-run reports with `tests/*.json` |
| `UPDATE_GOLDEN=1 cargo xtask check <skill>` | accepts new golden reports (review the diff) |
| `cargo fmt`, `cargo clippy --all-targets` | formatting and lints; CI runs both |

The dev shell (`nix develop`, or `direnv allow`) provides the toolchain with both wasm targets and
every tool above; CI runs `nix develop --command cargo xtask ci`, so a green local run predicts a
green CI. `nix flake check` builds the dev shell and checks the Nix formatting. The same shell is
the `rust-wasm-nickel` template in
[nix-dev-environment-templates](https://github.com/bahiamovementcenter/nix-dev-environment-templates);
change both together.

## Layout

| Path | Contents |
| :-- | :-- |
| `crates/rdx-plan` | plans (code-mode entrypoint): types, validation, executor, Nickel loading |
| `nickel/rdx/api.ncl` | plan contracts and op constructors agents write against |
| `wit/rdx.wit` | capability and extension interfaces (mirrors the Rust traits) |
| `.agents/skills/` | Agent Skills; `.claude/skills` links here |
| `xtask/` | repo checks |
| `docs/` | architecture, workflow, roadmap, ADRs, references |

## Code mode

Agents act on the system by running a skill's `scripts/main.ncl`, which evaluates to a plan. Don't
add one-off tools or scripts that bypass plans: add an op (Rust `Op` + constructor in `api.ncl`)
and use it from a skill. Plans are dry runs unless `dry_run = false`; show the person the dry-run
report before running anything with effects.

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
