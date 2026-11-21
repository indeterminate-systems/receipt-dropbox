# `statement.*`: parses bank statements (crate rdx-statement, piece 4).
let lib = import "../lib.ncl" in
let model = import "../model.ncl" in
let step = lib.step in
let DocumentC = lib.Document in
let DecimalText = lib.DecimalText in
let NonEmptyString = lib.NonEmptyString in

# Addresses name kv keys, so no spaces or `/` (the same rule as `keys.*`).
let Address =
  std.contract.from_validator (fun a =>
    if std.is_string a && std.string.is_match "^[^\\s/]+$" a then
      'Ok
    else
      'Error {
        message = "expected an email address with no spaces or `/`",
        notes = ["The address the supporter's keys are registered under, like `ana@example.org`."],
      }
  )
in

let StatementFormat =
  std.contract.from_validator (fun f =>
    if std.is_string f && std.array.elem f ["auto", "csv", "ofx", "example-bank"] then
      'Ok
    else
      'Error {
        message = "unknown statement format",
        notes = ["Use auto (detect), csv, ofx or example-bank."],
      }
  )
in

{
  parse
    | doc m%"
      Parses a bank statement (CSV, OFX, or a PDF in a known layout) from the blob store into
      a checked statement: `{ statement, parser, checks }`. Fails, saying what to fix, if the
      lines don't add up (running balances, printed totals, opening + lines = closing). Facts
      the document doesn't state are passed here; facts it does state must agree. Needs
      `blob-store`. No effects.
    "%
    =
      step
        "statement.parse"
        {
          document | DocumentC | doc "The statement file's blob digest.",
          supporter | model.Id | doc "Whose account this is.",
          statement_id
            | model.Id
            | doc "Defaults to `st-<YYYY-MM of the period start>.<supporter>`."
            | optional,
          format
            | StatementFormat
            | doc "Parser to use; detected from the document when absent."
            | optional,
          currency | model.Currency | doc "Needed when the document doesn't say (CSV)." | optional,
          period | model.Period | doc "Needed when the lines don't fix it." | optional,
          account
            | { institution | NonEmptyString, mask | String }
            | doc "Bank and last digits of the account; needed for CSV."
            | optional,
          opening | DecimalText | doc "When the document prints no balances." | optional,
          closing | DecimalText | doc "When the document prints no balances." | optional,
          csv
            | {
              delimiter | String | optional,
              date_order | [| 'ymd, 'mdy, 'dmy |] | optional,
              decimal | [| 'auto, 'point, 'comma |] | optional,
            }
            | doc "How to read a CSV whose header and values don't settle it."
            | optional,
          text
            | lib.ValueOrRef
            | doc "For a scan or photo: `rdx.ref` to an `ocr.read` step on the same document; for a password-protected PDF, to a `statement.unlock` step. Ignored when the PDF has a text layer."
            | optional,
        },

  unlock
    | doc m%"
      Reads a PDF statement that may be password-protected, on the supporter's device: the
      same output as `ocr.read` (`{ document, source, pages, text, warnings }`) for
      `statement.parse`'s `text`, plus `password`: `none` (not protected), `remembered` (with
      `account`, whose remembered password opened it) or `asked`. The password comes from one
      remembered for one of `address`'s bank accounts, or else from the person, asked through
      `secrets` under the label `statement-password-<first 12 hex digits of the digest>`; it
      never appears in arguments, outputs or the audit log. With `remember = true`, an asked
      password is returned as `remember`, encrypted to `address`'s keys, for
      `statement.remember-password`. The original is never changed. Needs `blob-store`, `kv`,
      `secrets`, `random`, so it runs only on the supporter's device. No effects.
    "%
    =
      step
        "statement.unlock"
        {
          document | DocumentC | doc "The statement PDF's blob digest.",
          address
            | Address
            | doc "The supporter's address: its remembered passwords are tried first, and `remember` encrypts to its keys."
            | optional,
          remember
            | Bool
            | doc "Return an asked password encrypted to `address`'s keys, to keep for this bank account."
            | default
            = false,
        },

  remember-password
    | doc m%"
      Keeps the password a `statement.unlock` step was given (its `remember`, already
      encrypted to `address`'s keys) for the bank account of `statement`, so that account's
      next statements open without asking: `{ remembered }`, the account's name
      (`"<institution> <mask>"`), or null when there was nothing new to keep. Needs `kv`.
      Effects.
    "%
    =
      step
        "statement.remember-password"
        {
          address | Address | doc "The supporter's address.",
          unlock | lib.ValueOrRef | doc "`rdx.ref` to the `statement.unlock` step.",
          statement
            | lib.ValueOrRef
            | doc "`rdx.ref` to the `statement.parse` step on the same document.",
        },
}
