---
name: plan-smoke-test
description: Checks that a Receipt Dropbox host can evaluate a Nickel plan, pass outputs between steps and write a run report with digests. Use after deploying or upgrading a host (PWA, Worker, Cloud Run, local CLI), or when plans fail for reasons that look like the host rather than the plan.
metadata:
  rdx-kind: product
---

# Plan smoke test

**Goal:** prove this host's plan pipeline works end to end, without touching any data.

## Run it

The entrypoint is [`scripts/main.ncl`](scripts/main.ncl). It takes one input, `host`, a label
that goes into the run notes:

```sh
rdx run .agents/skills/plan-smoke-test --input host='"cloudflare-staging"'
```

Until the `rdx` CLI exists, `cargo xtask check plan-smoke-test` evaluates and dry-runs the same
plan against the cases in `tests/`.

## Read the report

- `ok: true` and every step `done`: the host evaluates Nickel, resolves references and digests
  outputs. Nothing else is implied: this skill doesn't touch storage, keys or mail.
- A Nickel error before any step ran: the host can't find `nickel/rdx/api.ncl` (check its import
  path) or its `nickel-embed` is older than the API.
- `unknown op core.…`: the host registry was built without the core ops.
- `plan_digest` differs between hosts for the same inputs: the canonical JSON or plan format
  differs; compare the hosts' `rdx-plan` versions.
