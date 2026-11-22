---
name: keep-statement
description: Seals a supporter's parsed bank statement with their device key and keeps the bank's original document encrypted to their keys, with a signed custody record, deleting the plaintext. Use after ingest-statement, when a statement should be stored, or when asked to seal, keep, store or encrypt a statement or its original.
metadata:
  rdx-kind: product
---

# Keep statement

**Goal:** after a statement is read, keep it the way the organization's audit trail needs it:
the statement signed and encrypted, the bank's document encrypted, a custody record saying where
both are, and no plaintext left in the blob store.

## Run it

The entrypoint is [`scripts/main.ncl`](scripts/main.ncl). Run it on the supporter's device (the
PWA, or the local host): sealing and encrypting need their passkey. Inputs (Nickel expressions):

| Input | Meaning | Default |
| :-- | :-- | :-- |
| `statement` | the parsed statement, from `ingest-statement`'s `statement` step | the synthetic one |
| `address` | the supporter's address, already onboarded (`onboard-supporter`) | `"supporter1@example.org"` |

```sh
rdx run keep-statement --input statement='import "statement.json"' --input address='"ana@example.org"' --execute
```

In this repository, `rdx` is `cargo run -p rdx-cli --`. Both steps have effects, so a dry run
lists them as skipped with their arguments; check the statement and address there, then run with
`--execute`.

The statement's `original` (the digest `ingest-statement` read the document from) must still be
in the blob store: `audit.keep` encrypts that document and then deletes the plaintext.

## Read the report

- `sealed` (`envelope.seal`): `blob` (the encrypted, signed statement), `root` (its root digest,
  which every redacted view keeps) and `signer` (the key accountants verify it with; give it to
  them with the viewable statement).
- `kept` (`audit.keep`): `original` (the digest ledger entries name), `source` (the encrypted
  original), `sealed`, `root`, `device`, and `recipients` (the supporter's active devices plus
  the recovery key). A note says the plaintext original was deleted.

Errors, and what to do:

- `already kept`: this statement has a custody record. A corrected statement needs a new id.
- `is not in the blob store`: the bank's document was deleted or never stored here; put it back
  (`rdx blob put FILE`, the same file, so the same digest) and run again. If the digest differs,
  the statement was read from another file: ingest it again.
- `has no keys yet` or `holds no key`: onboard the supporter on this device first
  (`onboard-supporter`, `recover-access`).

Nothing is booked here: `reconcile-statement` books matches, and `audit-trail` later checks that
every booked line leads back to this custody record.
