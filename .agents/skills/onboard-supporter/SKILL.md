---
name: onboard-supporter
description: Gives a new supporter keys on their device, unlocked by their passkey, plus recovery shares for people or places they trust, and registers their email address so statements and bills sent from it are encrypted to them. Use when someone joins as a supporter, or asks to set up their account, device or recovery.
metadata:
  rdx-kind: product
---

# Onboard supporter

**Goal:** a supporter can receive and seal statements: this device holds a key wrapped by their
passkey, their address has a signed key registry, and a recovery key exists only as shares in
other hands.

## Run it

Run it on the supporter's own device (the PWA, or `rdx` locally); it needs the `secrets`
capability, which servers don't have.

```sh
rdx run onboard-supporter --input address='"ana@example.org"' --input supporter='"s-0001"' \
  --input device='"laptop"'                  # dry run: shows what would be created
rdx run onboard-supporter … --execute        # creates it
```

| Input | Meaning | Default |
| :-- | :-- | :-- |
| `address` | the email address they send statements and bills from | required |
| `supporter` | their supporter id | required |
| `device` | a name for this device (`laptop`, `phone`) | `this-device` |
| `threshold` | how many shares recover access | `2` |
| `holders` | who keeps each share, one share each | another device, printed card, org custodian |

Agree the holders with the supporter before running. A good default is 2 of 3: a second device
of theirs, a printed card they keep at home, and a custodian at the organization. No single
holder, the organization included, can recover access alone when the threshold is 2 or more.

## Read the report

A dry run shows the step `skipped` with the arguments it would use: confirm them with the
person, then rerun with `--execute`. After `--execute` the step's `output` has:

- `keys`: this device's public keys (`ur:crypto-pubkeys/…`), and `recovery`: the recovery key's.
  Neither is secret.
- `shares`: one `{ label, holder }` per share. The shares themselves are **not** in the report:
  the host showed each to the person under its label (the PWA displays it; locally it is the
  file `secrets/shown/<label>.txt` in the state directory). Tell the person to give each share
  to its holder, then delete the local copies. The seed behind them is already gone.
- `threshold`: how many shares recovery needs (`recover-access`).

## When it fails

- `already has keys`: this address is onboarded. To use another device, run `keys.create-device`
  there and `keys.add-device` here; if every device is lost, use `recover-access`.
- `threshold must be between 1 and the number of holders`: fix the inputs.
- `expected a device name`: lowercase letters, digits and `-`; `recovery` is reserved.
- A plan validation error naming `secrets`: this host can't hold keys (a server). Run it on the
  supporter's device.
