---
name: ingest-statement
description: Reads a supporter's bank statement (CSV export, OFX download, or PDF in a known bank layout, including a scan or photo of one) into checked statement lines with opening and closing balances, refusing statements that don't add up. Use when a supporter drops or emails a statement, or when an agent needs a statement's lines before matching receipts to them.
metadata:
  rdx-kind: product
---

# Ingest statement

**Goal:** turn one statement document into a `Statement` whose lines and balances are known to
add up, linked to the document by its digest.

## Run it

The document goes into the blob store first; the skill takes its digest. Locally:

```sh
rdx blob put ~/Downloads/march.pdf                     # prints sha256:…
rdx run ingest-statement --input document='"sha256:…"' --input supporter='"s-0001"'
```

In this repository, `rdx` is `cargo run -p rdx-cli --`. Inputs are Nickel expressions, so
strings need quotes.

| Input | Meaning | Default |
| :-- | :-- | :-- |
| `document` | blob digest of the statement file | required |
| `supporter` | id of the supporter whose account it is | required |
| `hints` | facts the document doesn't state (below) | `{}` |
| `protected` | the PDF is password-protected (below) | `false` |
| `remember_password` | keep the password for this bank account (below) | `false` |
| `address` | the supporter's email address, for remembered passwords | none |

`hints` is a record of extra `statement.parse` arguments. PDFs and OFX state most facts
themselves; CSV exports usually need `currency` and `account`:

```sh
--input hints='{ currency = "MXN", account = { institution = "Banco Ejemplo", mask = "4821" } }'
```

Other hints: `period = { start = "2026-03-01", end = "2026-03-31" }`, `opening` and `closing` as
decimal text when the file prints no balances, `format` (`csv`, `ofx`, `example-bank`) to skip
detection, `statement_id`, and `csv = { delimiter = ";", date_order = 'dmy, decimal = 'comma }`
for exports whose header and values don't settle it. A hint the document contradicts is an
error, never silently preferred.

The plan has no effects: it reads and reports. Nothing is stored or encrypted yet (later pieces
add those steps), so a dry run and `--execute` give the same report. The one exception is
`remember_password`, below.

## Password-protected PDFs

Mexican banks often send statements as PDFs that open only with a password, usually the
holder's RFC or date of birth. Run the skill as usual first; if the `text` step fails with
`the PDF is password-protected`, run it again with `--input protected=true` and, when you know
it, `--input address='"ana@example.org"'`. The first step is then `statement.unlock`, which reads
the PDF on the supporter's device:

1. If the bank only protected the PDF against editing, it opens without a password
   (`password: "none"`).
2. Otherwise it tries the passwords remembered for the supporter's bank accounts (needs
   `address`; the device's passkey unlocks them) and says which account's opened it
   (`password: "remembered"`, `account`).
3. Otherwise the device asks the person, under the label
   `statement-password-<first 12 hex digits of the digest>` (`password: "asked"`). The
   password is never in the plan, the report or the audit log, and never reaches a server:
   servers have no `secrets` capability, so the step can't run there.

The original stays exactly as the bank sent it, password and all; only its text is read, in
memory. Locally the host "asks" by reading a file: put the password in
`secrets/answers/statement-password-<…>.txt` under the state directory (the error names the label), run, then delete
the file.

To ask once per bank account, add `--input remember_password=true` (with `address`). The asked
password is encrypted to the address's keys (every active device and the recovery key) and
`statement.remember-password` keeps it for the statement's account (`"Example Bank 0001"`), so
the account's next statements open without asking. That step has effects: a dry run shows it
skipped, with the encrypted password it would keep; run with `--execute` to keep it. Ask the
person before remembering a password for them.

## Scans and photos

The first step, `ocr.read`, gives the document's text: a PDF's own text layer when it has one,
OCR when it is a scan or a PNG or JPEG photo, nothing for CSV and OFX. Its `source` says which
(`text-layer`, `ocr`, `none`). OCR needs the two ocrs models in the blob store, once per host;
locally, from the dev shell:

```sh
rdx blob put "$RDX_OCR_MODELS/text-detection.rten"
rdx blob put "$RDX_OCR_MODELS/text-recognition.rten"
```

When `source` is `ocr`, tell the person the statement was read from an image, and show
`warnings`: amounts whose separators OCR misread and were repaired (`read `$1.076.98` as
`$1,076.98``), and images that couldn't be read. Descriptions may carry OCR misreadings
(`PAPERI FAF` for `PAPERLEAF`); dates and amounts can't go wrong silently, because the balance
checks below refuse a statement that doesn't add up.

## Read the report

The `text` step is described above. The `statement` step's `output` has:

- `statement`: the parsed statement in the domain model (`rdx/model.ncl` `Statement`): lines
  `l01`, `l02`, … in document order, amounts in minor units, negative for money out, and
  `original` set to the document's digest.
- `parser`: which reader recognised the document (`csv`, `ofx`, `example-bank`).
- `checks`: how far the statement was cross-checked. `opening` and `closing` say where each
  balance came from: `printed`, `given` (a hint) or `derived`. `running_balances` counts lines
  whose printed running balance matched, and `totals` says whether printed debit and credit
  totals matched. A PDF with both balances printed, running balances and totals is fully
  checked. An OFX file prints only the closing balance, so its opening is derived and
  `opening + lines = closing` holds by construction: tell the person the statement was read but
  could not be cross-checked, and ask for the PDF if they have it.

The last step is the digest of that output, for the audit trail.

## When it fails

`ok: false` with a step `failed`; its `error` starts with the document digest. Act on the
rest of the message:

- `currency is missing`, `account is missing`, `opening balance is missing`, `period is
  missing`: the document doesn't say. Ask the person (or read it from their earlier
  statements) and rerun with that hint.
- `can't tell whether dates are day/month or month/day`: ask, then pass
  `csv = { date_order = 'dmy }` (or `'mdy`).
- `row N: printed balance … but the lines so far add up to …` (or `page P, line L: …`): a
  line was misread, is missing, or a hint is wrong (for example a wrong `opening`). Don't
  guess. Show the person the line and ask them to check the document; if the export is
  incomplete, ask for a complete one.
- `the document's total debits is …`: the table and the summary disagree; same as above.
- `no parser recognises this document`: the bank's layout isn't supported yet. Ask for a CSV or
  OFX export from the same bank. A new PDF layout is a new parser in `crates/rdx-statement`.
- `text-detection.rten … is not in the blob store` (from `text`): the document is a scan or
  photo and this host doesn't have the OCR models yet; add them as above and rerun.
- `page P: skipped an image: images encoded with CCITTFaxDecode` (a `warnings` entry) and then
  `no parser recognises this document`: a fax-style scan OCR can't decode yet. Ask for the
  bank's downloadable PDF, a CSV, or a photo of the page.
- `printed balance … but the lines so far add up to …` on a statement read by OCR: most likely
  an amount was misread. Show the person the line next to the image and ask them to check it,
  or ask for the bank's downloadable PDF.
- `the PDF is password-protected` (from `text` or `statement`): rerun with `protected=true`, as
  above.
- `the PDF is password-protected and no password was given`: the person hasn't given it yet.
  Ask them for it on their device (the label is in the message); suggest their RFC or date of
  birth if they don't know which.
- `the password given for … doesn't open this PDF`: ask them to check it. A remembered password
  that stopped working (the bank changed it) is replaced when they give the new one with
  `remember_password=true`.
- `this passkey doesn't unlock this key` or `this device holds no key`: remembered passwords are
  readable only on the supporter's enrolled devices; rerun without `address` to be asked, or
  enroll this device (`recover-access`).
- `protected scans can't be read yet`: a password-protected PDF with no text layer. Ask for the
  bank's downloadable statement or a CSV.
- `encryption handler … (only password protection is supported)`: the PDF is encrypted to a
  certificate, not a password. Ask for an unprotected copy.
- `no document … in the blob store`: put the file there first.
