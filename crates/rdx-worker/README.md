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
4. **Write the policy.** Edit [`policy.ncl`](policy.ncl): the intake address, the claim
   URL (see step 6), and who may send (`allow.domains` for whole domains, `allow.addresses` for volunteers'
   own mailboxes). `nickel export --import-path ../../nickel policy.ncl` checks it.
   With a subdomain, set `intake_address` to an address at the main domain, such as
   `receipts@yourorg.org`: it is the sender of the setup reply, and Cloudflare refuses a Worker's
   reply from a subdomain ("mail from is not from the correct domain"). Route that address to
   the Worker too (step 7), so answers to the reply reach intake.
5. **Create the storage**, from this directory, in the dev shell plus Wrangler
   (`nix shell nixpkgs#wrangler`) and `worker-build` 0.9, which must match the `worker` crate
   (Nixpkgs has 0.8.7, which refuses to build it): `cargo install worker-build@~0.9.0 --locked`,
   with `pkg-config` and OpenSSL in the shell (`nix shell nixpkgs#pkg-config nixpkgs#openssl`):

   ```sh
   wrangler login
   wrangler kv namespace create RDX_KV         # paste the id into wrangler.toml
   wrangler r2 bucket create rdx-blobs
   ```

6. **Deploy:**

   ```sh
   wrangler deploy --var RDX_INTAKE_POLICY:"$(nickel export --format json --import-path ../../nickel policy.ncl)"
   ```

   The deploy also builds and uploads the claim page (`cargo xtask claim-page`, from crate
   `rdx-claim`), which the Worker serves at `/claim`. The policy's `claim_url` must point there:
   either `https://rdx-intake.<account>.workers.dev/claim`, or, on a domain of your own, a custom
   domain for the Worker (uncomment `routes` in `wrangler.toml`; Cloudflare creates the DNS record
   and certificate). Passkeys belong to the domain, so pick the one the PWA will use.

7. **Route the address to the Worker.** *Email Routing → Routing rules → Create address*:
   the intake address, action *Send to a Worker*, Worker `rdx-intake`; with a subdomain, also the
   main-domain `intake_address` from step 4. Or with Wrangler:
   `wrangler email routing rules create yourorg.org --match-type literal --match-field to
   --match-value receipts@receipts.yourorg.org --action-type worker --action-value rdx-intake`.
8. **Try it.** Send an email from an allowed address that hasn't been onboarded: you should get
   "Verify your address to send receipts" back within a minute, and `wrangler tail` shows
   `{"status":"quarantined",…}`. From an address not on the list, you get a bounce saying so.
   Open the reply's link, make a passkey, and save the recovery share it shows: the mail moves
   to your new keys (`claim released: 1 document(s)`), and your next email is delivered
   straight to them (`{"status":"delivered",…}`).

### The claim page

The setup link opens `/claim`, a small page in Rust (crate `rdx-claim`, compiled to
WebAssembly) that stands in for the PWA (piece 14) until it exists. It reads the address and the
quarantine key from the link's fragment, which the browser never sends, then:

1. `POST /claim/open` with a request signed by the quarantine key: the Worker checks the
   signature against the public half it kept and returns the quarantined envelopes, still
   encrypted.
2. Makes a passkey (WebAuthn with the PRF extension) and runs `keys.onboard` in the page: a
   device key wrapped by the passkey, one recovery share shown once, and a registry signed by the
   recovery key.
3. Decrypts each envelope with the quarantine key and encrypts it to the new keys, in the page.
4. `POST /claim/release` with the registry and the new envelopes, named by digest in a request
   signed by the quarantine key. The Worker checks the signature (the proof of the mailbox that
   publishing a registry needs), the registry, and that every waiting document is accounted for;
   then it stores the registry and the new envelopes, lists them in the inbox and deletes the
   quarantine.

The wrapped device key stays in the browser's storage (`rdx/keys/device/<address>`), so only
the passkey unlocks it. The passkey needs a browser and provider with the PRF extension (current
Chrome, Edge, Safari and Firefox with most platform passkeys and security keys); the page says so
if it is missing. Design and server checks: `rdx_intake::claim`.

### Optional: expiry notices

When a quarantine expires the Worker deletes it. To also email the sender, enable *Email
Sending* for the intake subdomain (Workers Paid plan) and uncomment the `RDX_EMAIL` binding in
`wrangler.toml`. Without it, the setup reply's deadline is the only notice.

## What it can't do yet

- Reading delivered mail needs the PWA (piece 14). Until then, mail is stored encrypted and
  listed per address in KV (`intake/inbox/<address>`). The claim page sets up one device with one
  recovery share; more devices, share holders and supporter ids are the PWA's.
- Auto-forwarding rules that keep the merchant's `From` are bounced; supporters forward by hand.

## Limits

- Messages up to `max_bytes` (default 10 MiB; Cloudflare's own limit is 25 MiB).
- Senders must pass DMARC for their `From` domain; most mail providers do.
- The bundled Worker is about 1.3 MB of WebAssembly (0.5 MB gzipped).
