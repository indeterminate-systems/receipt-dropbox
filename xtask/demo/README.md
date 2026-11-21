# Demo inputs

Inputs for `cargo xtask demo [OUT]` (see `xtask/src/demo.rs`), which runs the pipeline end to end
with the local host: onboard a supporter, read the scanned `example-bank` statement and a receipt
photo with OCR, seal the statement, read receipts and bills, propose and confirm matches, book them,
publish the redacted PDF, verify it and export the ledger as XLSX. Run it in the dev shell; OUT
(default `target/demo`) gets every report, the PDF, `ledger.beancount`, `ledger.xlsx` and
`transcript.md`.

| Path | What it is |
| :-- | :-- |
| `inbox/*.txt` | synthetic receipts and a bill for the lines of `st-2026-03.s-0001` that `fixtures/` has no document for; the Paperleaf bill is `fixtures/receipts/bill-email.eml` |
| `plans/seal-statement`, `plans/open-statement` | `envelope.seal` and `envelope.open` as plans: no product skill seals or opens a statement yet |
| `accounts.ncl` | expense account per merchant, for booking |
