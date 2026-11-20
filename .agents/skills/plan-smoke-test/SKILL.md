---
name: plan-smoke-test
description: Checks that a Receipt Dropbox host can evaluate a Nickel plan, pass outputs between steps, reach its clock and kv capabilities and write a run report with digests to its audit log. Use after deploying or upgrading a host (PWA, Worker, local CLI), or when plans fail for reasons that look like the host rather than the plan.
metadata:
  rdx-kind: product
---

# Plan smoke test

**Goal:** prove this host's plan pipeline and capabilities work end to end, without touching any
financial data.

## Run it

The entrypoint is [`scripts/main.ncl`](scripts/main.ncl). It takes one input, `host`, a label
that goes into the run notes and the kv record. Inputs are Nickel expressions, so strings need
quotes:

```sh
rdx run plan-smoke-test --input host='"cloudflare-staging"'            # dry run
rdx run plan-smoke-test --input host='"cloudflare-staging"' --execute  # writes one kv record
```

In this repository, `rdx` is `cargo run -p rdx-cli --`. The report is JSON on stdout; progress
and the audit digest go to stderr. The dry run changes nothing; `--execute` writes one record,
`plan-smoke-test/last-run`, to the host's kv store.

## Read the report

- `ok: true`, every step `done` except `kv.put`, which is `skipped` in a dry run with the `args`
  it would write: the host evaluates Nickel, resolves references, digests outputs and reads its
  clock and kv. Rerun with `--execute` to check that kv writes work too.
- After an executed run, the next run's `previous` step shows the record the last run wrote.
- Exit status 2 and `needs the \`kv\` capability` (or `clock`): the host was built without that
  capability. That is a host configuration problem, not a plan problem.
- Exit status 2 with a Nickel error and no report: the host can't find `nickel/rdx/api.ncl`
  (check `--root`) or its `nickel-embed` is older than the API.
- `unknown op …`: the host registry was built without the built-in ops.
- Exit status 3: the run finished but the audit log could not be written; run
  `rdx audit verify`.
- `plan_digest` differs between hosts for the same inputs and flags: the canonical JSON or plan
  format differs; compare the hosts' `rdx-plan` versions.
