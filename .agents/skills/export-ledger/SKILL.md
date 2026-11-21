---
name: export-ledger
description: Reads the organization's Beancount ledger (what has been booked from supporters' statements and receipts, and what is owed to each supporter) and exports it as a CSV or XLSX spreadsheet with every entry's audit links. Use when someone asks for the ledger, the books, a spreadsheet or export for an accountant, or what has been booked or is owed.
metadata:
  rdx-kind: product
---

# Export ledger

**Goal:** give a person the ledger, or a spreadsheet of it, with each entry still linked to its
receipt and to the encrypted original statement.

## Run it

[`scripts/main.ncl`](scripts/main.ncl) takes these inputs (Nickel expressions):

| Input | Meaning | Default |
| :-- | :-- | :-- |
| `ledger` | a ledger name (its current version) or a version digest `sha256:…` (a past version, as an audit needs) | `"main"` |
| `format` | `'csv` or `'xlsx` | `'csv` |

```sh
rdx run export-ledger                                   # dry run: shows the ledger
rdx run export-ledger --input format="'xlsx" --execute  # writes the spreadsheet
```

In this repository, `rdx` is `cargo run -p rdx-cli --`. The `ledger` step (`ledger.read`) has
no effects and runs in a dry run; the `export` step (`ledger.export`) stores a file, so it runs
only with `--execute`.

## Read the report

The `ledger` step's output:

- `entries`: every booked entry (date, payee, narration, supporter, `line`, `receipt`,
  `receipt_source`, `statement` digest, postings). Answer "what was booked" from these.
- `balances`: one per statement, what the organization owes the supporter
  (`Liabilities:Reimbursable:<supporter>`, negative means owed) at the day after the statement
  period. Answer "what do we owe" from the latest balance per supporter.
- `text`: the Beancount file itself, for an accountant using Fava or `bean-check`.
- `digest`: this version's digest; `null` and a note "nothing posted yet" for an empty ledger.
- The step fails with `edited outside rdx`: the stored ledger is not what rdx wrote. Don't fix
  it by hand; tell the person, and point the kv entry back at the last good version (each
  `ledger.post` in the audit log names `previous` and `digest`).

With `--execute`, the `export` step's output names the file: `document` (its digest in the blob
store), `file_name`, `media_type` and `rows` (one row per posting). On the local host the file is
`.rdx/blobs/<digest without sha256:>`; copy it to `file_name` and give it to the person. Each row
has `date, payee, narration, account, amount, currency, supporter, statement_id, line, receipt,
receipt_source, statement`; amounts are exact decimals, and payees that look like formulas start
with `'`.
