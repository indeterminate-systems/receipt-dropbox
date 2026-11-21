---
name: reconcile-statement
description: Matches a supporter's bank statement lines to their receipts and bills and proposes which lines are organization expenses, flagging splits, instalments, partial payments, other currencies and near-misses for review. Use when someone asks to reconcile, match or check a statement against receipts, or asks which lines still lack a receipt.
metadata:
  rdx-kind: product
---

# Reconcile statement

**Goal:** propose which lines of one statement the receipts explain, so a person can confirm
them, list what is still unexplained, and book what the person has confirmed to the ledger.

## Run it

[`scripts/main.ncl`](scripts/main.ncl) takes these inputs (Nickel expressions):

| Input | Meaning | Default |
| :-- | :-- | :-- |
| `statement` | the statement (`Statement` in `nickel/rdx/model.ncl`) | the first synthetic statement in `fixtures/synthetic/small` |
| `receipts` | receipts and bills (`Array Receipt`); other supporters' are ignored | the synthetic receipts |
| `decided` | earlier decisions (`Array Match`): confirmed lines and receipts are left alone, rejected pairs never come back | `[]` |
| `policy` | tolerances (`Policy` in `nickel/rdx/matching.ncl`); give only what you change | `{}` (the defaults) |
| `accounts` | expense accounts for booking (`Accounts` in `nickel/rdx/ledger.ncl`): `by_receipt`, `by_merchant`, `fallback` | the synthetic scenario's accounts |
| `ledger` | the ledger confirmed matches are posted to | `"main"` |

```sh
rdx run reconcile-statement --input statement='import "/path/to/statement.json"' \
  --input receipts='import "/path/to/receipts.json"'
rdx run reconcile-statement --input policy='{ amount.tolerance_bps = 2000 }'   # allow 20% tips
```

In this repository, `rdx` is `cargo run -p rdx-cli --`. With no confirmed match in `decided`, the
plan has no effects: the report's `proposals` step output is the result. When `decided` confirms
matches, two more steps book them: `booking` (`ledger.book`, which runs in a dry run too) and
`posted` (`ledger.post`, skipped until you run with `--execute`).

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

## Book what is confirmed

Only matches the person confirmed are booked, never proposals. Pass every decision so far in
`decided`; the `booking` step's output shows what would be written:

- `entries` and `text`: the Beancount entries (expense against
  `Liabilities:Reimbursable:<supporter>`, a refund reversing it), each with `receipt`,
  `receipt-source`, `statement` (digest of the encrypted original) and `line` metadata. Show
  the person `text` and `summary.reimbursable` (what the organization owes for this statement).
- `skipped`: rejected or proposed matches, not booked.
- `notes` mentioning `Expenses:Uncategorized`: receipts with no account. Ask the person which
  account each belongs to and pass it in `accounts.by_receipt` (or `by_merchant`) before
  executing.
- The step fails with `has no digest of its encrypted original`: the statement must be stored
  encrypted first (its `original`); don't book without it, since the audit trail needs it.

Once the person agrees, run again with `--execute`. The `posted` step reports `added` (new
entries), `already_booked` (posting twice is harmless) and the ledger's new `digest`. A
`booked differently` error means that line was already booked another way: corrections are new
entries, not re-posts. To read or export the ledger, use `export-ledger`.
