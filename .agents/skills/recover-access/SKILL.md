---
name: recover-access
description: Restores a supporter's access after they lose or replace their devices, using recovery shares from their holders, enrolling the new device, retiring the lost ones and letting the new device read their stored statements. Use when a supporter lost their phone or laptop, reset a passkey, or can't unlock their statements.
metadata:
  rdx-kind: product
---

# Recover access

**Goal:** the supporter's new device holds a key their registry lists, the lost devices are
revoked, and the statements they had stored can be opened again.

## Before running

Recovery needs the shares made by `onboard-supporter`. Ask the supporter to collect enough of
them (the threshold, usually 2) from their holders and enter them on the new device, which gives
them to the host's `secrets` capability: the PWA asks for them; locally, put each in
`secrets/answers/recovery-share-<n>.txt` under the state directory (`-1`, `-2`, … in any order
of holders). Shares never go into the plan's inputs.

The new device needs the address's registry (synced from the organization's host) and, for each
document to reopen, the encrypted envelope in its blob store.

## Run it

```sh
rdx run recover-access --input address='"ana@example.org"' --input device='"phone-2"' \
  --input revoke='["laptop"]' --input documents='["sha256:…"]'      # dry run
rdx run recover-access … --execute
```

| Input | Meaning | Default |
| :-- | :-- | :-- |
| `address` | the supporter's address | required |
| `device` | a **new** name for this device; names are never reused | required |
| `revoke` | lost or replaced devices to retire | `[]` |
| `documents` | blob digests of sealed statements this device should read | `[]` |

## Read the report

A dry run shows the step `skipped` with its arguments; confirm the devices to revoke with the
person before `--execute`. After it, the output has:

- `keys`: the new device's public keys; `recipients`: what new documents are encrypted to now.
- `revoked`: devices retired. They can't sign or receive anything new. Anything they already
  held stays readable to whoever has that device *and* its passkey, so ask the person whether
  the device may be in someone else's hands.
- `documents`: `{ from, to }` per document: `to` is the same document, now readable by this
  device. Use `to` from now on; `from` can be deleted.

Ask the person to delete the share files after a successful run.

## When it fails

- `don't recover … keys: too few, mistyped, or from another supporter`: get one more share, or
  check each was copied whole (`ur:sskr/…`). Shares of another supporter never work.
- `has no keys yet`: the address was never onboarded here, or the registry hasn't synced.
- `device … already exists; ids are never reused`: pick a new `device` name.
- `is not an active device`: a name in `revoke` is wrong or already revoked.
- `no … in the blob store`: fetch the encrypted document onto this device first.
