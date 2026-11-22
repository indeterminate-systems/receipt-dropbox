---
name: open-granted-original
description: For an accountant or auditor a supporter granted access to - opens the granted copy of the full signed statement, verifies the supporter's signature, and opens the bank's original document, checking it is the one the supporter signed. Use when an accountant received a grant, or asks to see or check an original statement.
metadata:
  rdx-kind: product
---

# Open granted original

**Goal:** on the accountant's own device, open what a supporter granted (`grant-original`):
the full statement, checked against the supporter's signature, and the bank's document, checked
against the digest the supporter signed. Together they show the viewable statement left nothing
out and the statement came from that document.

## Run it

The entrypoint is [`scripts/main.ncl`](scripts/main.ncl). It needs the accountant's passkey, so it
runs on their device, which must have the copies in its blob store (the PWA fetches them).
Inputs (Nickel expressions), from the grant the supporter sent:

| Input | Meaning |
| :-- | :-- |
| `address` | the accountant's own address |
| `sealed` | the statement copy (`sealed` in the grant) |
| `source` | the document copy (`source` in the grant); `""` if only the statement was granted |
| `signer` | the supporter's `ur:signing-public-key/…`, from the organization's key registry, not from the grant |

```sh
rdx run open-granted-original --input address='"auditor@example.org"' --input sealed='"sha256:…"' \
  --input source='"sha256:…"' --input signer='"ur:signing-public-key/…"' --execute
```

In this repository, `rdx` is `cargo run -p rdx-cli --`. A dry run opens and verifies the
statement and skips writing the document; `--execute` also writes the document to this device's
blob store (locally, `.rdx/blobs/<digest without sha256:>`).

## Read the report

- `statement` (`envelope.open`): the full signed statement. It contains personal lines; show it
  to the accountant, don't copy it elsewhere.
- `view` (`envelope.verify`): every line (`redacted_rows` is 0 for a granted original), the
  balances, `root` (compare with the viewable statement's root: they must be equal) and
  `original`, the digest of the bank's document the supporter signed.
- `document` (`audit.open-original`): `document` must equal `view.original`. If it does, the
  document is the one the statement was read from.

Errors: `wasn't encrypted to them` (this device's key isn't the one granted: ask the supporter to
grant this address's current keys), `no … in the blob store` (fetch the copies first, or the grant
was revoked), `signature does not verify` (the copy was changed or the signer key is wrong:
stop and tell the organization), `hash to` (the document doesn't match what was signed: stop).
