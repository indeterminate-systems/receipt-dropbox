# ADR-P04: Password-protected statements

**Status:** Proposed
**Date:** 2026-11-21
**Deciders:** Ash

## Context

Mexican banks often email statements as PDFs that open only with a password the account holder
knows, usually their RFC or date of birth. The PDF reader refused them
([ADR-P04 text layer](ADR-P04-pdf-text-layer-reader.md)), so supporters had to print an
unprotected copy. That copy is not what the bank sent, which weakens the audit trail, and it is
a chore every month.

Three constraints shape the fix. The reader is our own, chosen because lopdf (which decrypts)
can't build for `wasm32-unknown-unknown`. Servers never decrypt, and a password opens the
statement, so it is as sensitive as the plaintext. And plan arguments and outputs go to run
reports and the audit log, so no secret may pass through them.

## Decision

**The reader decrypts in memory; the original is never rewritten.** `rdx-pdf` implements the PDF
standard security handler (revisions 2 to 6: RC4 40- and 128-bit, AES-128 and AES-256) with
RustCrypto's pure-Rust `aes`, `md-5` and `sha2`, which need no randomness to decrypt and build for
both wasm targets. `text_with_password` and `page_images_with_password` take the user or the
owner password. A PDF whose user password is empty (protected only against editing) opens with
plain `text`, as viewers open it. The blob store keeps the bank's file byte for byte, and
`statement.original` is its digest.

**The password comes from the person, through `secrets`.** `statement.unlock` (crate
`rdx-statement`) asks under the label `statement-password-<first 12 hex digits of the
digest>`, one per document so an old answer is never reused silently. Its output is the text
layer (the same shape as `ocr.read`, so `statement.parse` takes it as `text`) and how it opened,
never the password. Servers offer no `secrets`, so the op fails validation there.

**Remembering is per bank account, encrypted to the supporter's keys.** With `remember`,
`statement.unlock` returns the asked password as an envelope encrypted (rdx-keys) to every
active device and the recovery key of the supporter's address; `statement.remember-password`, a
separate step with effects, keeps it in `kv` under `statement-passwords/<address>`, keyed by the
parsed statement's account (`"<institution> <mask>"`). The next statement tries those passwords
first, after the device's passkey unlocks its key. Recovery restores them with everything else.

Both paths ship together: asking is the fallback whenever nothing remembered opens the file.

## Options considered

| Option | For | Against |
| :-- | :-- | :-- |
| Own standard-security decryption in `rdx-pdf` (chosen) | Builds for both wasm targets; the reader stays one crate; tested against qpdf's output for every revision | About 400 lines of crypto plumbing to maintain (the primitives are RustCrypto's) |
| lopdf for protected PDFs only | Mature | Same getrandom problem as before: no `wasm32-unknown-unknown` build without JavaScript |
| Ask for an unprotected copy (before) | Nothing to build | A chore each month, and the kept original isn't the bank's file |
| Password as a plan input | Simple | It would be recorded in run reports and the audit log |
| Store a decrypted copy of the PDF | Later steps unchanged | Plaintext at rest; two "originals" |
| Remembered passwords keyed by document or bank name only | No parse needed first | The account is what the password belongs to; one bank can hold accounts of several people |

## Consequences

- `PdfError::Encrypted` is replaced by `PasswordRequired` and `WrongPassword`; certificate
  (public-key) encryption is reported as unsupported.
- The remembered-password envelope appears in `statement.unlock`'s output and the audit log. It
  is ciphertext to the supporter's own keys, the same as what `kv` keeps.
- A password-protected scan (no text layer) is refused: `ocr.read` doesn't take a password yet.
  Adding `password` support there would go through `statement.unlock` too.
- Revision 6 should normalise passwords with SASLprep; bank passwords are ASCII, which SASLprep
  leaves unchanged, so the UTF-8 bytes are used. Revisit if a bank uses non-ASCII passwords.
- Remembered passwords encrypted before a device was enrolled can't be read on it; asking again
  with `remember` re-encrypts them to the current keys. Forgetting one is not an op yet.
- The PWA (piece 14) shows the `statement-password-…` prompt; email intake (piece 13) needs
  nothing new, since it never opens the PDF.
