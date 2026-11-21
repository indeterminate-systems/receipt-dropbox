---
name: triage-quarantine
description: Lists email waiting in quarantine for senders who haven't set up their keys yet, and deletes what has expired, telling each sender. Use when someone asks what mail is waiting, why a supporter's emailed receipts didn't arrive, or to clean up expired intake mail (a server can do this on a schedule).
metadata:
  rdx-kind: product
---

# Triage quarantine

**Goal:** everyone can see which emailed documents are waiting for their senders, and nothing
waits past its date.

Mail from an allowed sender whose address has no keys yet is held encrypted to a quarantine key.
The sender got one email with a link that holds that key; the server kept only its public half.
When they open the link and set up their keys, their mail moves to those keys. If they don't, it
expires: this skill deletes it and tells them.

## Run it

```sh
rdx run triage-quarantine --input policy='{ intake_address = "receipts@receipts.example.org",
  claim_url = "https://app.example.org/claim", allow.domains = ["example.org"] }'
                                             # dry run: lists quarantines, deletes nothing
rdx run triage-quarantine … --execute        # also deletes what has expired
```

| Input | Meaning |
| :-- | :-- |
| `policy` | the organization's intake policy ([rdx/intake.ncl](../../../nickel/rdx/intake.ncl)); the same one the Worker runs with |

The Cloudflare Worker sweeps on its own schedule (its cron trigger), so on Cloudflare this skill
is for looking, or for sweeping right away.

## Read the report

- Step `status`: `quarantined` lists `{ address, documents, opened_at, expires_at, expired }`
  (times are Unix milliseconds, UTC). `expired` counts those past their date.
- Step `sweep` (`skipped` in a dry run): `expired` lists each deleted quarantine's `address` and
  the blob ids `deleted`; `notices_sent` counts the emails telling senders.

What to tell people:

- A supporter whose mail is waiting should open the link in the email they got ("Verify your
  address to send receipts"), on the device they'll use. If they lost it, they wait until
  `expires_at`, then send one more email to get a new link.
- An address that should not be sending at all: take it off the policy's `allow` list; its
  quarantine still expires on its own.
- Nothing here can open a quarantined email: only the sender's link can.

## When it fails

- A contract error on `policy`: fix the field it names (see rdx/intake.ncl).
- `the intake record … is unreadable`: the kv value was changed by hand; restore it or delete
  that quarantine's record and its documents.
