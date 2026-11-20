---
name: ingest-receipt
description: Reads a receipt, emailed bill, refund note or CFDI a supporter sent (an .eml, HTML, plain text or CFDI XML) and turns it into a receipt record with merchant, date, total, reference and kind, or says what is missing. Use when a supporter forwards or uploads proof of an org expense, or when asked to read, parse or extract a receipt or bill.
metadata:
  rdx-kind: product
---

# Ingest receipt

**Goal:** turn one document a supporter sent into a receipt record that matching (piece 6) can
use, without guessing anything the document doesn't say.

## Run it

`scripts/main.ncl` takes these inputs. Inputs are Nickel expressions, so strings need quotes.

| Input | Meaning | Default |
| :-- | :-- | :-- |
| `supporter` | id of the supporter who sent it, such as `"s-0001"` | required |
| `document` | the document's text: `import "/path/to/bill.eml" as 'Text` | required |
| `media_type` | `"message/rfc822"`, `"text/html"`, `"text/plain"` or `"application/xml"` (CFDI) | `"message/rfc822"` |
| `hints` | `{ currency = "MXN", date_order = "dmy", merchant = "…" }`, any subset | `{}` |

```sh
rdx run ingest-receipt \
  --input supporter='"s-0001"' \
  --input document="import \"$PWD/inbox/bill.eml\" as 'Text" \
  --input hints='{ currency = "USD" }'
```

In this repository `rdx` is `cargo run -p rdx-cli --`. The step has no effects, so a dry run
is the whole run. Use an absolute path in `import`; a relative one is looked up from the
current directory and the repository's `nickel/` directory.

For a PDF receipt, the text layer comes from `pdf.text` (piece 4): pass that step's output as
`document` (`document = rdx.ref "pdf"`) and the receipt's `source` is the PDF's digest.

Hints come from the organization's settings, not from the person's guess: an organization in
Mexico uses `currency = "MXN", date_order = "dmy"`. Hints never override what the document says.

## Read the report

The `receipt` step's output:

- `status: "complete"` and a `receipt`: merchant, date, total and kind were all found. The
  `receipt` has the domain model's shape; its `id` is derived from the `source` digest, so the
  same document always gets the same id. Show the person the merchant, date and total.
- `status: "needs_review"` and `receipt: null`: `missing` lists what wasn't found (`merchant`,
  `date`, `total`, `currency`) and `warnings` say why. Act on them:
  - `currency` missing: the total has `$` or no currency. Rerun with the organization's
    `currency` hint.
  - a warning that the date is ambiguous: rerun with `date_order`.
  - `total` or `merchant` missing: ask the person for the value, or for a better copy of the
    receipt. Don't invent one.
- `fields` is what was found either way, so a partial result can be shown and corrected.
- `extractor` is `cfdi` when a CFDI was read (it is authoritative), `text` otherwise.
- `fiscal` (CFDI only): the folio fiscal `uuid`, issuer and receiver RFC, payment form and
  method. Piece 15 checks these against the organization's fiscal profile and SAT.
- `attachments`: email parts that weren't read (PDFs, images), with their digests. If the
  email body had no total, the receipt is probably in one of them: read it with `pdf.text`.
- A warning that the email's Date header was used: the document itself has no date, so check it.
- `ok: false` with an error from `receipt.extract`: the arguments were wrong (an unknown
  supporter id shape, or `document` refers to a step that was skipped); the message says which.
