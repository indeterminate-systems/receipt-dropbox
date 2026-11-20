---
name: verify-statement
description: Checks that a Receipt Dropbox statement envelope (full or redacted) is signed by the supporter's key and reads what its viewable statement shows - shown lines, "Personal, redacted" rows, opening and closing balances and the total redacted. Use when an accountant or auditor receives a redacted statement, before rendering or booking from one, or when a statement's integrity is in doubt.
metadata:
  rdx-kind: product
---

# Verify statement

**Goal:** confirm that a statement envelope is unchanged since the supporter signed it, and get
the figures its viewable statement must show.

## Run it

The entrypoint is [`scripts/main.ncl`](scripts/main.ncl). It takes two inputs, both UR strings
(Nickel expressions, so they need quotes):

- `envelope`: the statement, `ur:envelope/…`, redacted or full.
- `signer`: the supporter's public signing key, `ur:signing-public-key/…`. Get it from the
  organization's key registry, never from whoever sent the envelope.

```sh
rdx run verify-statement --input envelope='"ur:envelope/…"' --input signer='"ur:signing-public-key/…"'
```

In this repository, `rdx` is `cargo run -p rdx-cli --`. Without inputs it checks the synthetic
fixture in `fixtures/synthetic/small/envelopes/`. The plan has no effects and needs no host
capabilities, so a dry run is the whole run.

## Read the report

The `view` step's output is the viewable statement:

- `statement`, `supporter`, `account`, `currency`, `period`, and `original` (the digest of the
  encrypted original, when present) identify what was signed.
- `lines`: the shown lines, in statement order. Each has confirmed receipts covering its whole
  amount.
- `redacted_rows`: how many "Personal, redacted" rows to show. Elided lines reveal nothing else,
  not even their dates.
- `summary`: `opening + shown + redacted = closing`. `redacted` is derived from the signed
  balances and the shown lines, so it is exact whenever the signature verifies.
- `root`: the envelope's root digest. It is the same for the full envelope and every redaction of
  it, so it links the viewable statement to the supporter's original and to ledger entries.

Failures (`ok: false`, the step `failed`):

- `the signature does not verify with this key`: the envelope was changed after signing, or a
  different key signed it. Don't use its figures; ask the supporter to publish it again.
- `not an rdx statement envelope`: a valid envelope that isn't a statement, or one with lines or
  balances removed rather than elided.
- `not a valid ur:envelope/… string`: the input was truncated or mangled in transit.
- A Nickel contract error before anything runs: `signer` isn't a `ur:signing-public-key/…` string.
