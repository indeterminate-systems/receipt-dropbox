# ADR-P13: Email intake on Cloudflare, with claim links for unverified senders

**Status:** Proposed
**Date:** 2026-11-21
**Deciders:** Ash

## Context

Supporters forward bills and send statements to one address of the organization. The project's
rules for it: only allowlisted senders, authenticated with SPF/DKIM/DMARC; mail from a known
address encrypted to its keys on arrival with the plaintext discarded; mail from an allowed but
unverified address held under a quarantine key for a limited time, with one rate-limited reply
linking to the PWA to set up keys, and on expiry the key destroyed, the document deleted and the
sender notified. Servers can encrypt but never decrypt. The roadmap puts this on Cloudflare Email
Routing and a `workers-rs` Worker, after rejecting a hosted inbox (AgentMail) that stores
plaintext. The organization's domain has no mail yet and may not be on Cloudflare DNS yet, so
piece 13 must be testable without a deployment.

## Decision

**A core crate, `rdx-intake`, decides; a host crate, `rdx-worker`, does the I/O.** The core does
no I/O and builds for both WebAssembly targets like every core crate. Its `receive(host, policy,
raw)` returns an outcome (`delivered`, `quarantined`, `rejected` with the reason the sender
reads) and, at most once per address, the verification reply for the host to send. `sweep`
expires quarantines; `release` claims one; `status` lists them. The same functions are ops
(`intake.receive`, `intake.status`, `intake.sweep`, `intake.release`) for the local host, with
skills `receive-email` and `triage-quarantine`.

**Authentication is the receiving server's.** The core has no DNS. Cloudflare checks SPF, DKIM
and DMARC, rejects mail that fails the sender's DMARC policy or both SPF and DKIM, and prepends an
`Authentication-Results` field. Intake trusts only the topmost such field, and only if its
authserv-id is the policy's (`mx.cloudflare.net`); fields lower down may be the sender's own. It
requires `dmarc=pass` for the domain in `From`, then checks `From` (exactly one mailbox) against
the allowlist (whole domains and single addresses). DMARC is stricter than Cloudflare's own
floor (SPF or DKIM), because without alignment `From` could be anyone's and intake encrypts to
the keys `From` names.

**The whole email is the document.** Each email is one envelope, `'isA': rdx.intake.email.v1`,
subject the message as received, `receivedAt`, encrypted with `rdx_keys::encrypt` to every active
device and the recovery key (piece 8). The architecture said "each attachment": keeping the
message whole keeps bills that are only an HTML body, CFDI XML parts and forwarded headers
(the merchant, for `receipt.extract`), and the PWA splits it with `rdx-receipt`'s MIME reader
after decrypting. Envelopes are stored as `ur:envelope` text, as `rdx-keys` stores documents, and
listed per address in kv `intake/inbox/<address>`.

**The quarantine key lives only in the sender's link.** For an allowed address with no key
registry, intake draws 32 random bytes, derives keys from them (`Keys::from_secret`, new in
rdx-keys), encrypts the email to the public half, keeps only that public half
(`intake/quarantine/<address>`), and puts the 32 bytes in the reply's link:
`<claim_url>#address=<address>&key=<hex>`. Later emails from the address join the same quarantine
without another reply. Opening the link proves control of the mailbox and carries the key in the
URL fragment, which browsers never send to a server. The PWA onboards the address (piece 8),
then `release` decrypts each waiting email with the link's key, encrypts it afresh to the new
keys (so the link opens nothing any more), lists it in the inbox and deletes the quarantined
copy.

So the server never holds a key that opens anything: "destroy the quarantine key" on expiry is
already true, and expiry deletes the documents and the record. The link is as sensitive as the
mail it opens, and it sits in the mailbox that sent that mail, which already holds a plaintext
copy in its sent folder.

**One reply, rate-limited.** A reply goes only when a quarantine opens, at most once per
`reply_interval_days` per address (`intake/replied/<address>`). Inside the interval, mail from an
address without keys or quarantine is bounced with the date to try again. Automatic mail
(`Auto-Submitted` other than `no`) from an address without keys is bounced unanswered (RFC 3834).
On Cloudflare the reply uses Email Routing's `message.reply()`, which needs a DMARC pass (already
required), answers only the sender, and is free on every plan.

**Links are never opened; intake asks for the document.** An email that is only a Wise tracking
link (`wise.com/<lang>/track/<id>`), with no PDF, image or XML that could be its receipt, is kept
like any other email (encrypted to the sender's keys or the quarantine key), and its sender is
asked to send the transfer's PDF receipt from the Wise app. Fetching the link would put payment
details in plaintext on the server, and the page can change or expire. The ask is rate-limited
like the setup reply (`intake/asked/<address>`, `reply_interval_days`) and never answers
automatic mail. Email Routing allows one reply per email, so a first email from an address
without keys gets the ask inside its setup reply. If an ask alone can't be sent, the email is
still kept and the ask forgotten, so the next link tries again.

**Expiry notices are best effort.** A notice after the sender has written nothing is a new
email, not a reply: Cloudflare sends those to arbitrary addresses only through Email Sending, on
the Workers Paid plan. The Worker sends them when a `RDX_EMAIL` `send_email` binding exists; the
setup reply itself says when the mail will be deleted, so on the free plan that is the notice.

**The Worker stages storage around a synchronous core.** Workers KV and R2 are asynchronous;
`rdx-host`'s traits are not. The Worker loads the kv keys a call can touch (`receive_keys`,
`sweep_keys`) into `StagedKv`, runs the core, then writes new blobs, then kv, then blob
deletions, so kv never points at a missing blob. Reading a key that wasn't loaded is an error,
so a core change that needs more keys fails in `rdx-worker`'s tests. It replies before writing:
if the link can't be sent, nothing is kept and the sender is asked to send again.

**Policy is Nickel, exported to JSON at deploy.** `nickel/rdx/intake.ncl` is the contract and
the single source of defaults (7-day quarantine and reply interval, 10 MiB). The Worker gets
`nickel export`'s output as the `RDX_INTAKE_POLICY` variable, so it doesn't embed a Nickel
evaluator.

**`rdx-worker` lives in `crates/`, gated by target.** Its `worker` (workers-rs) dependency and
entry points compile only for `wasm32-unknown-unknown`; on other targets it is the staging layer
and its tests, so the workspace still tests natively and checks for `wasm32-wasip2`. The Worker
`worker-build` bundles is 1.3 MB of WebAssembly (0.5 MB gzipped), under the free plan's 3 MB. It
never links `rdx-render` or `rdx-ocr`, which need plaintext.

## Alternatives considered

- **Keep the quarantine private key on the server**, encrypted to an org key or deleted at
  expiry. The server could then decrypt quarantined mail, breaking "servers never decrypt".
- **Quarantine key held by an org custodian.** The custodian could read mail from people who
  never joined, and claiming would need the custodian online.
- **A code in the reply instead of a link.** Same secret, more typing; the PWA can still accept
  the whole link pasted in.
- **Check SPF/DKIM/DMARC in the core.** Needs DNS, which the core can't do; Cloudflare already
  does it before the Worker runs.
- **Encrypt each attachment separately.** Loses body-only bills and forwarded headers; see above.
- **A separate Worker workspace outside `crates/`.** A second lockfile and toolchain setup for
  one crate; the target gate is simpler.

## Consequences

- Senders must use the organization's addresses or allowlisted ones and send from their own
  mailbox; **auto-forwarding rules** that keep the merchant's `From` are rejected (the merchant is
  not on the list). Supporting them (for example by plus-addressing, `receipts+ana@…`) is later
  work.
- **The PWA (piece 14) owes:** the claim page (reads the fragment, onboards, runs `release`), and
  HTTP routes on the Worker to fetch inbox and quarantined blobs and to publish a new registry.
  Publishing a registry for an address must require proof of the mailbox, such as a signature by
  that address's quarantine key, or anyone could register keys for an allowed address and
  receive its mail.
- KV is eventually consistent and the inbox and index are read-modify-write: two emails from one
  sender within seconds, handled in different locations, could drop one inbox entry (the blob is
  kept). Fine at supporter volumes; Durable Objects or D1 if it ever matters.
- Large emails: the Worker holds the message and its UR copy (about twice the size) in memory
  (128 MB), hence the 10 MiB default; CPU time for big PDFs on the free plan is still to measure.
- Logs hold outcome status and blob ids, never addresses' mail or links.
