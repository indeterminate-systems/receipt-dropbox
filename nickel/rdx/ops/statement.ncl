# `statement.*`: parses bank statements (crate rdx-statement, piece 4).
let lib = import "../lib.ncl" in
let model = import "../model.ncl" in
let step = lib.step in
let DocumentC = lib.Document in
let DecimalText = lib.DecimalText in
let NonEmptyString = lib.NonEmptyString in

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
        },
}
