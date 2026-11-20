# Receipt Dropbox

Local-first expense reconciliation for organizations whose supporters pay costs from personal
accounts. Supporters drop bank statements and receipts; Receipt Dropbox matches them, books the
matches to a Beancount ledger, keeps the original statements encrypted, and publishes redacted
statements that show only receipt-confirmed organization transactions, with every other line
elided (Gordian Envelope) so totals still reconcile.

One Rust core compiled to WebAssembly runs in the browser (PWA), on Cloudflare Workers, on Google
Cloud Run and locally. Agents drive it through Nickel **plans**: each skill's single code-mode
entrypoint.

**Status:** architecture and scaffold. See [the roadmap](docs/roadmap.md).

- [Architecture](docs/architecture.md) and [decision records](docs/adr/)
- [The TDD to skills workflow](docs/workflow.md)
- [AGENTS.md](AGENTS.md): how to build, test and contribute (for people too)

```sh
nix develop            # or: direnv allow
cargo xtask doctor     # is the shell complete?
cargo xtask ci         # what CI runs
cargo test
cargo xtask check      # validate and dry-run every skill
cargo run -p rdx-cli -- run plan-smoke-test   # run a skill on the local host (dry run)
```
