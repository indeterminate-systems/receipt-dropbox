# ADR-P12: Audit trail, custody records and grants

**Status:** Proposed
**Date:** 2026-11-22
**Deciders:** Ash

## Context

Roadmap piece 12: every ledger entry and every viewable row links to its receipt and to the
encrypted original statement, and an accountant sees an original only when the supporter who
uploaded it grants it. Servers may encrypt but never decrypt.

What earlier pieces left:

- Ledger entries (piece 9) carry `receipt`, `receipt-source` and `statement`, the digest the
  parsed statement calls its `original`: the digest of the document as the bank sent it.
- A sealed statement (piece 8) is kept encrypted, but nothing keeps the bank's document
  encrypted: `ingest-statement` reads a plaintext blob and leaves it there.
- Which sealed envelope belongs to which statement is written down only in `envelope.seal`'s
  run report.
- Every run report goes to the hash-chained audit log with each step's full output, so on the
  supporter's device the log holds statements in the clear (ADR-P08 left this to piece 12).
- Encrypting to more keys (`envelope.add-recipients`, `keys.recover`) stores the result under a
  new digest, so ciphertext digests change whenever recipients change.

## Decision

**A custody record per statement**, in kv `audit/custody/<statement id>`: a chain of signed
Gordian Envelope events, shaped like the key registry (ADR-P08). Each event names the statement,
its sequence number and the event before it, the time, the supporter's address, the device that
signed it and that device's signing key. Three kinds:

| Event | Written by | Records |
| :-- | :-- | :-- |
| `kept` | `audit.keep`, once | the envelope's root digest, the sealed envelope's blob, the original's digest, the encrypted original's blob |
| `granted` | `audit.grant` | who (`grantee`), the keys added, why, and the blobs of the copies they can open |
| `revoked` | `audit.revoke` | who, why; the copies are deleted |

Only a device holding the supporter's key can sign an event, so a server can store custody
records and show them but cannot add a grant.

**The ledger keeps linking to the original's plaintext digest.** The signed statement envelope
carries that digest (`original`), so the chain an auditor checks is: ledger entry → `statement`
digest → custody record → encrypted original; after a grant, decrypting it and hashing the bytes
gives the digest the supporter signed. A ciphertext digest would change each time a device is
added or a grant made. The digest of a document only confirms a guess at its exact bytes, which
for a bank PDF is no guess at all.

**`audit.keep` encrypts the original** (`rdx.original.v1`: the bytes as a byte string, their
digest and the statement id) to the supporter's keys, stores it, checks the sealed envelope
opens and verifies on this device, writes `kept`, and deletes the plaintext blob.

**A grant is a copy.** `audit.grant` adds the grantee's keys to copies of the sealed envelope and
the encrypted original (`rdx_keys::add_recipients`: the content is not re-encrypted) and stores
them under new digests. The supporter's own blobs, which custody and the ledger point at, never
change. Revoking deletes the copies; what the grantee already opened stays with them, which the
op says.

**The grantee opens a copy on their own device**: `envelope.open` for the statement (all lines,
since that is what was granted) and `audit.open-original` for the bank's document, which checks
the bytes against the signed digest and puts them in that device's blob store.

**`audit.trail`** reads a ledger (`ledger.read`'s output), the custody records it names and,
optionally, verified viewable statements (`envelope.verify` outputs), and reports, per entry, its
receipt, the original's digest, the envelope root, where the encrypted original is, whether the
viewable statement shows the line, and the grants. It needs `kv` and `blob-store`, decrypts
nothing, and runs on any host. It reports as problems: a booked statement with no custody record,
an entry whose original differs from the custody record's, a missing blob, a viewable statement
whose root isn't the kept one, a shown row nothing books, and a booked line the viewable statement
redacts. A custody event whose signing key is no longer in the supporter's registry (rotated or
lost) is a warning, not a problem: its signature still verifies.

**The audit log keeps digests, not outputs.** `rdx_plan::run` records each step's
`output_digest` and drops `output`; a skipped step's arguments become `args_digest`. The caller
still gets the full report. The log can then go to an auditor without carrying statement lines,
and each digest still pins what the step produced.

## Options considered

| Option | For | Against |
| :-- | :-- | :-- |
| Custody chain of signed events per statement (chosen) | One place per statement says where its original is and who may read it; servers can store it but not forge a grant | One more kv record to sync between hosts (piece 14) |
| Link ledger entries to the ciphertext digest | Literally "the hash of the encrypted original" | Changes on every device rotation or grant, so links rot; the signed statement names the plaintext digest anyway |
| Add the grantee to the supporter's own blobs | No extra copy | Revoking would mean re-encrypting under a new content key and moving every link |
| Mine the audit log for seals and grants | No new record | Ops can't read the log (by design), and after this piece it no longer holds outputs |
| Keep outputs in the audit log | Self-contained history | The log on a device holds plaintext statements and can't be shared |

## Consequences

- `keep-statement` replaces a bare `envelope.seal`: seal, then keep. The local demo's
  `seal-statement` plan can move to it.
- Receipts are not part of custody: they are the organization's proof and carry no personal
  lines. Their `receipt-source` digest is reported, with whether the blob is present.
- Notes in run reports still go to the audit log; ops must keep statement text out of notes.
- The PWA (piece 14) syncs `audit/custody/<id>` and the granted copies to the organization's host
  so an accountant's device can fetch them; two devices writing events at once fork the chain,
  and the later writer reloads and retries, as for the registry.
- A retention sweep (piece 16) deletes the encrypted original and its grant copies and records
  that in custody; the root digest still links ledger entries to what was signed.
