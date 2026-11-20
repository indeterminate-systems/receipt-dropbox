---
name: reconcile-statement
description: Matches a supporter's bank statement lines to their receipts and bills and proposes which lines are organization expenses, flagging splits, instalments, partial payments, other currencies and near-misses for review. Use when someone asks to reconcile, match or check a statement against receipts, or asks which lines still lack a receipt.
metadata:
  rdx-kind: product
---

# Reconcile statement

**Goal:** propose which lines of one statement the receipts explain, so a person can confirm
them, and list what is still unexplained.

## Run it

[`scripts/main.ncl`](scripts/main.ncl) takes these inputs (Nickel expressions):

| Input | Meaning | Default |
| :-- | :-- | :-- |
| `statement` | the statement (`Statement` in `nickel/rdx/model.ncl`) | the first synthetic statement in `fixtures/synthetic/small` |
| `receipts` | receipts and bills (`Array Receipt`); other supporters' are ignored | the synthetic receipts |
| `decided` | earlier decisions (`Array Match`): confirmed lines and receipts are left alone, rejected pairs never come back | `[]` |
| `policy` | tolerances (`Policy` in `nickel/rdx/matching.ncl`); give only what you change | `{}` (the defaults) |

```sh
rdx run reconcile-statement --input statement='import "/path/to/statement.json"' \
  --input receipts='import "/path/to/receipts.json"'
rdx run reconcile-statement --input policy='{ amount.tolerance_bps = 2000 }'   # allow 20% tips
```

In this repository, `rdx` is `cargo run -p rdx-cli --`. The plan has no effects: the report's
`proposals` step output is the result, and nothing is confirmed or booked.

## Read the report

The `proposals` step's output:

- `proposals`: one per explanation, in statement order. Each has `kind` (`exact`, `tolerance`,
  `fx`, `split`, `instalments`, `partial`), `matches` (all `proposed`), `evidence` per line and
  receipt pair (days apart, merchant similarity, whether the reference was printed, amount
  difference, and for `fx` both amounts, the rate, its date and source) and `review`.
- `review: false` (only `exact`, unambiguous matches): show them as a list the person can accept
  in one go.
- `review: true`: show each one with its `reasons` and ask. Never confirm these on the person's
  behalf.
- `unmatched_lines`: lines that stay "Personal, redacted". If the person says one was an
  organization expense, ask for its receipt.
- `unmatched_receipts`: the supporter's receipts no line explains; usually paid from another
  account, or the statement for that date hasn't been uploaded.
- `notes`: inputs that could not be considered. "no EUR to USD rate" means adding a rate to
  `policy.fx.rates`; "receipts from other supporters" is informational.
- The step fails with `matching.propose: statement … is not consistent`: the statement does not
  balance or has duplicate lines; it was misparsed, so re-ingest it rather than editing it.
- A Nickel contract error naming a policy field: fix that field (whole numbers only; exchange
  rates are strings such as `"17.2345"`).

When the person rejects a proposal, pass it back in `decided` with `status = "rejected"` so it is
not proposed again; confirmed ones go in with `status = "confirmed"`.
