---
name: receive-email
description: Takes one email a supporter sent to the organization's receipts address (an .eml file) through intake on the local host - checks the sender, encrypts the email to their keys and deletes the plaintext, or holds it for a sender who hasn't set up keys and sends them a setup link. Use when running intake locally or testing it with a saved email; on Cloudflare the Worker does this for every email.
metadata:
  rdx-kind: product
---

# Receive email

**Goal:** an emailed statement or bill is stored only encrypted, to the right people, or bounced
with a reason the sender can act on.

## Run it

```sh
rdx blob put mail.eml                       # prints sha256:…
rdx run receive-email --input message='"sha256:…"' --input policy='{
  intake_address = "receipts@receipts.example.org", claim_url = "https://app.example.org/claim",
  allow.domains = ["example.org"] }'          # dry run: shows the step's arguments
rdx run receive-email … --execute             # receives it
```

The .eml must start with the `Authentication-Results` field of the server that received it,
naming `policy.authserv_id` (Cloudflare writes `mx.cloudflare.net`). Intake has no DNS of its own,
so without it every email is rejected: DMARC can't be checked.

## Read the report

The step's `output.status`:

- `delivered`: encrypted to every active device of `address` and its recovery key; `document` is
  the encrypted copy, listed in kv `intake/inbox/<address>`.
- `quarantined`: the sender is allowed but has no keys. The email is encrypted to a quarantine
  key until `expires_at`. `reply = "sent"` means this was their first email and the setup link
  went out (locally, to the outbox); later emails join the same quarantine with no reply. They
  open the link, set up their keys, and `intake.release` moves their mail to them.
  `triage-quarantine` deletes it if they don't.
- `rejected`: nothing kept; `reason` is what the sender is told. Not on the allow list, DMARC
  failed, too large, several senders in From, a link already sent recently, or an automatic
  email from an address without keys.

`asked = "wise-receipt"`: the email (delivered or quarantined) holds a Wise tracking link and no
receipt, so the reply asks the sender to send the transfer's PDF receipt. Intake never opens the
link. A sender is asked at most once per `reply_interval_days`; a first email from an address
without keys gets the request inside its setup reply.

`message_deleted` is true: the plaintext .eml is gone from the blob store whatever the outcome.

## When it fails

- `no sha256:… in the blob store`: put the .eml there first.
- A contract error on `policy`: fix the field it names (see rdx/intake.ncl).
- `… key registry …`: the sender's registry doesn't verify; see `recover-access`.
