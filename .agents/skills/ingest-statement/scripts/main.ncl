# Code-mode entrypoint for ingest-statement. No effects unless a password is remembered: it reads
# the document and reports the parsed statement; storing and encrypting it are later pieces'
# steps. `ocr.read` runs first so a scanned PDF or a photo is read with OCR; for a text PDF, CSV
# or OFX it costs nothing. A password-protected PDF is read by `statement.unlock` instead, which
# asks the supporter for the password on their device or uses one remembered for the account.
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

  protected
    | Bool
    | doc "The PDF is password-protected: read it with `statement.unlock` on the supporter's device."
    | default
    = false,

  remember_password
    | Bool
    | doc "Keep the password the supporter gives for this bank account, encrypted to their keys (implies `protected`; needs `address`)."
    | default
    = false,

  address
    | String
    | doc "The supporter's email address, whose keys remembered passwords are encrypted to. Without it, `statement.unlock` always asks."
    | default
    = "",

  # Bound under other names: inside the record below, `document` would mean that record's field.
  plan =
    let digest = document in
    let who = supporter in
    let mailbox = address in
    let locked = protected || remember_password in
    let addr = if mailbox == "" then {} else { address = mailbox } in
    let keep =
      if remember_password && mailbox == "" then
        std.fail_with "remember_password needs `address`: the password is encrypted to that address's keys"
      else
        remember_password
    in
    rdx.plan {
      skill = "ingest-statement",
      goal = "Read a bank statement into checked lines and balances",
      steps =
        [
          if locked then
            rdx.ops.statement.unlock ({ id = "text", document = digest, remember = keep } & addr)
          else
            rdx.ops.ocr.read { id = "text", document = digest },
          rdx.ops.statement.parse (
            { id = "statement", document = digest, supporter = who, text = rdx.ref "text" } & hints
          ),
        ]
        @ (
          if keep then
            [
              rdx.ops.statement.remember-password {
                id = "remembered",
                address = mailbox,
                unlock = rdx.ref "text",
                statement = rdx.ref "statement",
              },
            ]
          else
            []
        )
        @ [rdx.ops.core.digest { value = rdx.ref "statement" }],
    },
}
