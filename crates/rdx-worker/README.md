# rdx-worker: email intake on Cloudflare

An [Email Routing](https://developers.cloudflare.com/email-service/) Worker that takes every
email sent to the organization's receipts address through intake (`rdx-intake`): it checks the
sender, encrypts the email to their keys and stores only the ciphertext in R2, or holds it for an
allowed sender who hasn't set up keys and replies once with a setup link. A daily cron trigger
deletes what was never claimed. Design: [ADR-P13](../../docs/adr/ADR-P13-email-intake.md).

Nothing ever sits in a mailbox: Email Routing hands each message to the Worker in memory.

## Before deploying: test it locally

Everything the Worker decides is tested without Cloudflare:

```sh
nix develop
cargo test -p rdx-intake -p rdx-worker          # the rules, and the Worker's storage staging
rdx blob put mail.eml                           # a saved email with an Authentication-Results field on top
rdx run receive-email --input message='"sha256:…"' --input policy='{ … }' --execute
rdx run triage-quarantine --input policy='{ … }'
```

## Setup

You need a Cloudflare account (the free plan works) and a domain whose DNS is on Cloudflare.

1. **Put the domain's DNS on Cloudflare.** In the dashboard, *Add a domain*, pick the Free plan,
   and change the nameservers at your registrar to the two Cloudflare gives you. Wait until the
   domain shows *Active* (minutes to a day).
2. **Pick the intake address.** Use a subdomain only intake receives mail for, such as
   `receipts@receipts.yourorg.org`, so ordinary mail for the organization can be set up
   anywhere later without touching it.
3. **Turn on Email Routing for that subdomain.** Email Routing starts at the main domain:
   *Compute → Email Service → Email Routing → Onboard Domain*, choose the domain, and let
   Cloudflare add its records (if the domain already receives mail elsewhere, read Cloudflare's
   "Move to Email Routing from another provider" first). Then open that domain's *Settings*, and
   under *Subdomains* add `receipts.yourorg.org`; Cloudflare adds its MX, SPF and DKIM records.
   Add and verify one destination address of your own if it asks (intake never forwards to it).
4. **Write the policy.** Edit [`policy.ncl`](policy.ncl): the intake address, the PWA's claim
   URL, and who may send (`allow.domains` for whole domains, `allow.addresses` for volunteers'
   own mailboxes). `nickel export --import-path ../../nickel policy.ncl` checks it.
5. **Create the storage**, from this directory, in the dev shell plus Wrangler
   (`nix shell nixpkgs#wrangler nixpkgs#worker-build`):

   ```sh
   wrangler login
   wrangler kv namespace create RDX_KV         # paste the id into wrangler.toml
   wrangler r2 bucket create rdx-blobs
   ```

6. **Deploy:**

   ```sh
   wrangler deploy --var RDX_INTAKE_POLICY:"$(nickel export --format json --import-path ../../nickel policy.ncl)"
   ```

7. **Route the address to the Worker.** *Email Routing → Routing rules → Create address*:
   the intake address, action *Send to a Worker*, Worker `rdx-intake`.
8. **Try it.** Send an email from an allowed address that hasn't been onboarded: you should get
   "Verify your address to send receipts" back within a minute, and `wrangler tail` shows
   `{"status":"quarantined",…}`. From an address not on the list, you get a bounce saying so.

### Optional: expiry notices

When a quarantine expires the Worker deletes it. To also email the sender, enable *Email
Sending* for the intake subdomain (Workers Paid plan) and uncomment the `RDX_EMAIL` binding in
`wrangler.toml`. Without it, the setup reply's deadline is the only notice.

## What it can't do yet

- Claiming a quarantine and reading delivered mail need the PWA (piece 14): its claim page and
  the Worker routes it will call. Until then, mail from onboarded senders is stored and listed
  per address in KV (`intake/inbox/<address>`), and quarantined mail waits and expires.
- Auto-forwarding rules that keep the merchant's `From` are bounced; supporters forward by hand.

## Limits

- Messages up to `max_bytes` (default 10 MiB; Cloudflare's own limit is 25 MiB).
- Senders must pass DMARC for their `From` domain; most mail providers do.
- The bundled Worker is about 1.3 MB of WebAssembly (0.5 MB gzipped).
