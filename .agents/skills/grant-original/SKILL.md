---
name: grant-original
description: Lets an accountant or auditor open a supporter's full signed statement and the bank's original document, by adding their keys to copies, with a signed custody record of who was granted what and why; or ends that access. Use when an accountant asks to see an original statement, when a supporter agrees to share one, or when access should be withdrawn.
metadata:
  rdx-kind: product
---

# Grant original

**Goal:** the organization's accountant sees an original only when the supporter who uploaded it
grants it. This skill is that grant: it runs on the supporter's device, adds the accountant's
keys to copies of the sealed statement and of the bank's document, and records the grant in the
statement's custody record. With `revoke`, it ends the grant.

Ask the supporter before granting: the full statement shows every line, personal ones included.
Never run it on the accountant's behalf without the supporter's say-so.

## Run it

The entrypoint is [`scripts/main.ncl`](scripts/main.ncl). It needs the supporter's passkey, so it
runs on their device. Inputs (Nickel expressions):

| Input | Meaning | Default |
| :-- | :-- | :-- |
| `statement` | the statement id | `"st-2026-03.s-0001"` |
| `address` | the supporter's address | `"supporter1@example.org"` |
| `grantee` | the accountant's address, as custody records it | `"accountant@example.org"` |
| `keys` | the accountant's `ur:crypto-pubkeys/…`; empty looks them up in their registry here | `[]` |
| `reason` | why, as custody records it | `"accountant review"` |
| `original` | also grant the bank's document, not only the signed statement | `true` |
| `revoke` | end the grant instead | `false` |

```sh
rdx run grant-original --input statement='"st-2026-03.s-0001"' --input address='"ana@example.org"' \
  --input grantee='"auditor@example.org"' --input reason='"2026 annual audit"' --execute
rdx run grant-original --input statement='"st-2026-03.s-0001"' --input address='"ana@example.org"' \
  --input grantee='"auditor@example.org"' --input revoke=true --execute
```

In this repository, `rdx` is `cargo run -p rdx-cli --`. The accountant needs keys of their own
(`onboard-supporter` works for an accountant too); take their public keys from their registry or
from them directly, never from a third party. A dry run shows the grant with its arguments; run
with `--execute` once the supporter agrees.

## Read the report

- `keys` (`keys.recipients`, when `keys` is empty): the accountant's keys the copies are
  encrypted to. A failure `has no keys yet` means the accountant isn't onboarded on this host:
  pass their `keys` instead.
- `grant` (`audit.grant`): `sealed` and `source`, the copies to give the accountant (blob
  digests), `signer` and `root` to verify the statement with, and `original`, the digest the
  bank's document must have. Send the accountant those values; they run `open-granted-original`.
- `revoke` (`audit.revoke`): `deleted`, the copies removed. Anything the accountant already
  opened stays with them; say so to the supporter.

Errors: `has no custody record` (keep the statement first with `keep-statement`), `already has
an active grant` (revoke it first, or leave it), `belongs to` (only that supporter can grant),
`has no active grant` (nothing to revoke).
