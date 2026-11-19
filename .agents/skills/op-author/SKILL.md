---
name: op-author
description: Adds or changes a Receipt Dropbox op - the Nickel contract for its arguments, a failing Rust test, the Rust `Op` implementation and its constructor in nickel/rdx/api.ncl - so skills can use it in plans. Use when a goal needs something the core cannot do yet (parse, match, encrypt, write), before writing the skill with skill-author.
metadata:
  rdx-kind: dev
---

# Op author

You add capabilities to the core, one op at a time. Read [docs/workflow.md](../../../docs/workflow.md)
first: this skill is stages 1 to 3 of it. `skill-author` takes over at stage 4.

## Before you start

Run `cargo xtask doctor` in the dev shell (`nix develop`). Every line must say `ok`: an op is not
done until it passes the WebAssembly checks, and those need the wasm targets and wasmtime.

## 1. Contract

In `nickel/rdx/api.ncl`, write the contract for the op's arguments: every field with a type, a
`doc`, and a `default` where one is sensible. Reject bad input with a message that tells an agent
what to change (`std.contract.from_validator` with `message` and `notes`). Ops live in a
namespace per area (`statement.parse`, `ledger.post`), never in `core` unless they are about plans.

## 2. Failing test

In the crate that will own the op, add a test that loads a plan using the constructor from a
synthetic fixture in `fixtures/` and asserts the step's output (see
`crates/rdx-plan/tests/nickel_plans.rs`). Also test that the contract rejects one bad argument.
Run `cargo test -p <crate>` and confirm it fails because the op is missing, not for another
reason.

## 3. The op

Implement `rdx_plan::Op` (`crates/rdx-plan/src/exec.rs`):

- `id()` is the id the constructor uses, such as `statement.parse`.
- `effects()` returns true if the op changes anything outside the run (storage, mail, ledger,
  keys). Ops with effects are skipped in dry runs, so the dry-run report must still say what
  would happen.
- `run()` takes arguments already checked by the contract, with refs resolved. Re-check what
  Rust relies on and return an `OpError` an agent can act on.
- Do no I/O directly: storage, mail, time and randomness come from host capabilities
  (`wit/rdx.wit`). Money is integer minor units plus a currency; never floats.

Register it with the crate's `Registry` and add the constructor under `ops` in `api.ncl`:

```nickel
statement = {
  parse
    | doc "Parses a statement into lines and balances. No effects."
    = step "statement.parse" StatementParseArgs,
},
```

## Done when

`cargo xtask ci` passes in the dev shell. It runs the new test natively and as WebAssembly
under wasmtime (`cargo test --target wasm32-wasip2`), checks both wasm targets, and checks every
skill. Then hand over to `skill-author` to use the op in a goal.
