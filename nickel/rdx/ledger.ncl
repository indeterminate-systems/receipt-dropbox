# The ledger's contracts (crate rdx-ledger, piece 9): which expense account each receipt books to,
# and how plans name a ledger. Defaults live here, not in Rust.
let model = import "model.ncl" in

let LedgerNameC =
  std.contract.from_validator (fun s =>
    if std.is_string s
    && (
      std.string.is_match "^[a-z0-9][a-z0-9_-]{0,63}$" s
      || std.string.is_match "^sha256:[0-9a-f]{64}$" s
    ) then
      'Ok
    else
      'Error {
        message = "expected a ledger name like \"main\" or a version digest `sha256:<64 hex digits>`",
        notes = [
          "A name (lowercase letters, digits, `-`, `_`) means the ledger's current version; a digest means one past version.",
        ],
      }
  )
in

{
  Accounts = {
    fallback
      | model.Account
      | doc "Where a receipt books when neither `by_receipt` nor `by_merchant` names it. Entries booked here are counted and noted, so someone classifies them."
      | default
      = "Expenses:Uncategorized",
    by_receipt
      | { _ | model.Account }
      | doc "Expense account per receipt id: a person's classification, which wins over `by_merchant`."
      | default
      = {},
    by_merchant
      | { _ | model.Account }
      | doc "Expense account per merchant name, compared ignoring case and surrounding spaces."
      | default
      = {},
  },

  LedgerName = LedgerNameC,
}
