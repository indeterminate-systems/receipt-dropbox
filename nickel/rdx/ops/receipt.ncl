# `receipt.*`: reads receipts and bills (crate rdx-receipt, piece 5).
let lib = import "../lib.ncl" in
let model = import "../model.ncl" in
let step = lib.step in
let ValueOrRef = lib.ValueOrRef in

let MediaType =
  std.contract.from_validator (fun t =>
    let known = ["message/rfc822", "text/html", "text/plain", "application/xml"] in
    if std.is_string t && std.array.elem t known then
      'Ok
    else
      'Error {
        message = "unsupported media type for receipt extraction",
        notes = [
          "Use one of: %{std.string.join ", " known}.",
          "For a PDF, read its text layer with `pdf.text` and pass that step's output as `document`.",
        ],
      }
  )
in

let DateOrder =
  std.contract.from_validator (fun o =>
    if o == "dmy" || o == "mdy" then
      'Ok
    else
      'Error {
        message = "invalid date order",
        notes = ["Use `dmy` (03/04 is 3 April) or `mdy` (March 4)."],
      }
  )
in

let ReceiptHints = {
  currency
    | model.Currency
    | doc "Currency for amounts with no code, or only `$` (dollars or pesos)."
    | optional,
  date_order
    | DateOrder
    | doc "How to read numeric dates whose day and month could be swapped."
    | optional,
  merchant
    | String
    | doc "Merchant to use when the document doesn't name one. Emails use the sender."
    | optional,
}
in

{
  extract
    | doc m%"
      Extracts merchant, date, total, reference and kind from a receipt or bill: an email
      (MIME, forwarded or not), HTML, plain text or CFDI XML, or the output of `pdf.text`.
      Outputs `status` (`complete` or `needs_review`), the `fields` found, what is `missing`,
      `warnings`, CFDI `fiscal` data, email `attachments` it didn't read, the `source`
      digest and, when complete, a `receipt` in the domain model's shape. No effects.
    "%
    =
      step
        "receipt.extract"
        {
          supporter
            | model.Id
            | doc "Who sent the receipt, and so whose statement it should match.",
          document
            | ValueOrRef
            | doc "The document's text (`import \"bill.eml\" as 'Text`), or `rdx.ref` to a `pdf.text` step.",
          media_type
            | MediaType
            | doc "Ignored when `document` is a `pdf.text` output."
            | default
            = "message/rfc822",
          receipt_id
            | model.Id
            | doc "Id for the receipt. Default: `r-` and 16 hex digits of the source digest."
            | optional,
          hints | ReceiptHints | default = {},
        },
}
