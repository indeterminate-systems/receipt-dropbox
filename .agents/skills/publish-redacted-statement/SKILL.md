---
name: publish-redacted-statement
description: Redacts a supporter's signed bank statement to the lines confirmed receipts cover and renders the viewable PDF accountants get - shown lines, "Personal, redacted" rows, opening and closing balances, total redacted, root digest and a QR code of the envelope. Use when a supporter has confirmed matches for a statement and wants to share it with the organization, or when asked to publish, render or print a redacted statement.
metadata:
  rdx-kind: product
---

# Publish redacted statement

**Goal:** turn a supporter's sealed statement and their confirmed matches into the viewable
statement the organization sees: a PDF that shows only receipt-confirmed lines and proves, with
the supporter's signature, that nothing else was changed.

## Run it

The entrypoint is [`scripts/main.ncl`](scripts/main.ncl). Inputs (Nickel expressions, so strings
need quotes):

- `envelope`: the supporter's full sealed statement, `ur:envelope/…`. Redaction needs the full
  envelope, which only the supporter's device holds; an envelope that is already redacted is
  refused.
- `matches`: the statement's matches (`Match` in `rdx/model.ncl`), usually from
  `reconcile-statement` after the supporter confirmed them. Only `confirmed` matches that cover a
  line's whole amount keep it visible.
- `signer`: the supporter's public signing key, `ur:signing-public-key/…`, from the
  organization's key registry.

```sh
rdx run publish-redacted-statement --input envelope='"ur:envelope/…"' --input signer='"ur:signing-public-key/…"' --input matches='[…]'
```

In this repository, `rdx` is `cargo run -p rdx-cli --`. Without inputs it uses the synthetic
fixtures. Show the person the dry run first: it already has every figure the PDF will print.
Then run again with `--execute` to write the PDF.

## Read the report

- `redacted` (`envelope.redact`): the redacted envelope and its `root` digest, how many lines
  are `shown` and `redacted`, and `review`: lines that confirmed matches cover only in part.
  Those are redacted too; ask the supporter to confirm the rest of the amount (a split or a
  second receipt) and publish again if they should show.
- `view` (`envelope.verify`): what the PDF will show, checked against `signer`: `lines`,
  `redacted_rows`, and `summary` (`opening + shown + redacted = closing`).
- `pdf` (`render.statement`): `skipped` in a dry run, with the arguments it would use. With
  `--execute`, `document` is the PDF's digest in the blob store (locally,
  `.rdx/blobs/<hex digits>`), plus `pages`, `qr` (whether the envelope fit in a QR code) and the
  same `root`, `shown` and `redacted_rows`.

The PDF carries its own proof: the root digest on every page, the envelope as a QR code (when it
fits), as an attached `envelope.ur` file and printed on the last page. Anyone with the
supporter's public key can check it with `verify-statement`.

Failures (`ok: false`, the step `failed`):

- `redacted`, `not an rdx statement envelope: it is already redacted`: pass the full envelope.
- `view`, `the signature does not verify with this key`: wrong `signer`, or the envelope was
  changed. Don't publish; get the key from the registry, or ask the supporter to seal again.
- `pdf`, `Typst could not render the statement`: a bug in `rdx-render`, not in the input. Report
  it with the run report.
