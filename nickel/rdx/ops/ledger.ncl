# `ledger.*`: books confirmed matches to the Beancount ledger, reads it and exports it (crate
# rdx-ledger, piece 9). Ledger versions live in the blob store; kv `ledger/<name>` names the
# current one.
let lib = import "../lib.ncl" in
let contracts = import "../ledger.ncl" in
let step = lib.step in
let ValueOrRef = lib.ValueOrRef in

{
  book
    | doc m%"
      Turns one statement's confirmed matches into ledger entries: each expense (a refund
      reverses one) against `Liabilities:Reimbursable:<supporter>`, with `receipt`,
      `receipt-source`, `statement` (digest of the encrypted original) and `line` metadata.
      Proposed and rejected matches are listed in `skipped`, never booked. `original` is needed
      only when the statement has no `original` digest. Output: `entries`, `statement`,
      `skipped`, `notes`, `summary` and `text` (the Beancount it adds). No effects.
    "%
    =
      step
        "ledger.book"
        {
          # Usually earlier steps' outputs (`rdx.ref`); the op checks them in Rust.
          statement | ValueOrRef,
          receipts | ValueOrRef,
          matches | ValueOrRef,
          accounts | contracts.Accounts | default = {},
          original | lib.Document | optional,
        },

  post
    | doc m%"
      Adds a `ledger.book` output (`booking`, usually `rdx.ref` to that step) to the named
      ledger, stores the new version in the blob store, points kv `ledger/<name>` at it and
      appends it to `ledger-versions/<name>`.
      Re-posting is harmless: entries already booked identically are counted, a line booked
      differently is refused. Output: `digest`, `previous`, `added`, `already_booked`. Has effects.
    "%
    =
      step
        "ledger.post"
        {
          booking | ValueOrRef,
          ledger | contracts.LedgerName | default = "main",
        },

  read
    | doc m%"
      Reads a ledger by name (current version) or digest (any past version): its `entries`,
      `balances` (one assertion per statement) and Beancount `text`. No effects.
    "%
    =
      step
        "ledger.read"
        {
          ledger | contracts.LedgerName | default = "main",
        },

  export
    | doc m%"
      Writes a ledger as a spreadsheet, one row per posting with its audit links, to the blob
      store. Output: `document` (the file's digest), `media_type`, `file_name`, `rows`. Has effects.
    "%
    =
      step
        "ledger.export"
        {
          ledger | contracts.LedgerName | default = "main",
          format | [| 'csv, 'xlsx |] | default = 'csv,
        },
}
