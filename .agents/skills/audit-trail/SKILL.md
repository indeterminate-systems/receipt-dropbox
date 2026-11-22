---
name: audit-trail
description: Traces every ledger entry to its receipt, the supporter's encrypted original statement and its custody record (who kept it, who was granted it), and checks viewable redacted statements against the ledger, reporting anything that doesn't add up. Use when an accountant or auditor asks for the audit trail, where an entry came from, who has seen an original, or whether the books and the viewable statements agree.
metadata:
  rdx-kind: product
---

# Audit trail

**Goal:** answer an auditor's questions from the records alone: for each booked entry, which
receipt backs it, which signed statement and encrypted original it came from, whether the
viewable statement shows it, and who may open the original. It decrypts nothing, so it runs on
any host, the organization's server included.

## Run it

The entrypoint is [`scripts/main.ncl`](scripts/main.ncl). Inputs (Nickel expressions):

| Input | Meaning | Default |
| :-- | :-- | :-- |
| `ledger` | a ledger name, or a version digest `sha256:…` (`ledger-versions/<name>` in kv lists them) | `"main"` |
| `views` | viewable statements to check: `[{ envelope = "ur:envelope/…", signer = "ur:signing-public-key/…" }]` | `[]` |

```sh
rdx run audit-trail
rdx run audit-trail --input views='[{ envelope = import "redacted.ur" as '"'"'Text, signer = import "signer.ur" as '"'"'Text }]'
```

In this repository, `rdx` is `cargo run -p rdx-cli --`. The plan has no effects; a dry run is the
whole run. Take each `signer` from the organization's key registry, not from whoever sent the
statement.

## Read the report

`ledger` is the ledger read (`ledger.read`); `view-<n>` each viewable statement, verified. The
`trail` step:

- `ok`: true when `problems` is empty.
- `entries`, one per ledger entry: `line` (`<statement>/<line>`), `date`, `payee`, `postings`,
  `receipt` and `receipt_source` (the receipt's document digest; `receipt_stored` says whether it
  is in the blob store), `statement` (the original's digest the entry names), `root` and `source`
  (from custody: the signed envelope's root and the encrypted original's blob), `viewable`
  (`shown`, `redacted`, or `null` when no view of that statement was given) and `granted_to`
  (who may open the original now).
- `statements`, one per kept statement: `address`, `root`, `original`, `sealed`, `source`,
  `signer`, `kept_at` and `kept_by`, `entries` booked from it, and `grants` (grantee, reason,
  copies, `granted_at`, `granted_by`, and `revoked_at`/`revoked_reason` once ended).
- `problems`, each a sentence naming the statement or line. What to do:
  - `has no custody record`: the supporter hasn't kept that statement; ask them to run
    `keep-statement`.
  - `names original … but … was kept with`: the entry was booked from a different reading of the
    statement. Rebook from the kept one.
  - `is missing from the blob store`: a kept blob was deleted; restore it from a backup.
  - `signed by a key … never listed` or `after it was revoked`: someone other than the supporter
    wrote to the custody record. Treat that grant as forged and tell the organization.
  - `has root …, not the kept`: the viewable statement isn't the statement that was kept.
  - `shows … but no ledger entry books it` or `is booked but the viewable statement redacts it`:
    the books and the viewable statement disagree; reconcile again before relying on either.
