# Code-mode entrypoint for ingest-statement. No effects: it reads the document and reports the
# parsed statement; storing and encrypting it are later pieces' steps. `ocr.read` runs first so a
# scanned PDF or a photo is read with OCR; for a text PDF, CSV or OFX it costs nothing.
let rdx = import "rdx/api.ncl" in
{
  document
    | String
    | doc "Blob digest of the statement file (CSV, OFX, PDF, or a scan or photo). Locally: `rdx blob put FILE`.",

  supporter
    | String
    | doc "Id of the supporter whose account this is, such as \"s-0001\".",

  hints
    | { _ | Dyn }
    | doc m%"
      Facts the document may not state, passed to `statement.parse`: `currency`, `account`
      (`{ institution, mask }`), `period` (`{ start, end }`), `opening` and `closing` (decimal
      text), `format`, `statement_id`, `csv` (`{ delimiter, date_order, decimal }`).
    "%
    | default
    = {},

  # Bound under other names: inside the record below, `document` would mean that record's field.
  plan =
    let digest = document in
    let who = supporter in
    rdx.plan {
    skill = "ingest-statement",
    goal = "Read a bank statement into checked lines and balances",
    steps = [
      rdx.ops.ocr.read { id = "text", document = digest },
      rdx.ops.statement.parse (
        { id = "statement", document = digest, supporter = who, text = rdx.ref "text" } & hints
      ),
      rdx.ops.core.digest { value = rdx.ref "statement" },
    ],
  },
}
