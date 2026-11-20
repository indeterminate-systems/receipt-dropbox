# Code-mode entrypoint for ingest-receipt. No effects: the extraction goes in the run report.
# A PDF is read from the blob store by `pdf.text` first. Storing the receipt (encrypted to the
# supporter's keys) is added with piece 8.
let rdx = import "rdx/api.ncl" in
{
  supporter
    | String
    | doc "Id of the supporter who sent the receipt, such as `s-0001`.",

  document
    | String
    | doc m%"
      The receipt's text as received. From a file: `import "path/to/bill.eml" as 'Text`
      (absolute, or relative to the current directory). For a PDF, its blob digest instead
      (locally: `rdx blob put FILE`).
    "%,

  media_type
    | String
    | doc "`message/rfc822` (an .eml), `text/html`, `text/plain`, `application/xml` (CFDI) or `application/pdf`."
    | default
    = "message/rfc822",

  hints
    | { _ | Dyn }
    | doc m%"
      What the organization knows that the receipt may not say: `currency` (for `$` or bare
      amounts), `date_order` (`dmy` or `mdy`, for dates like 03/04/2026), `merchant`.
    "%
    | default
    = {},

  # Bound under other names: inside the step's record, `document = document` would refer to
  # the step's own field (Nickel records are recursive).
  plan =
    let who = supporter in
    let text = document in
    let media = media_type in
    let known = hints in
    rdx.plan {
      skill = "ingest-receipt",
      goal = "Read a receipt or bill a supporter sent and turn it into a receipt record",
      steps =
        if media == "application/pdf" then
          [
            rdx.ops.pdf.text { id = "pdf", document = text },
            rdx.ops.receipt.extract {
              id = "receipt",
              supporter = who,
              document = rdx.ref "pdf",
              hints = known,
            },
          ]
        else
          [
            rdx.ops.receipt.extract {
              id = "receipt",
              supporter = who,
              document = text,
              media_type = media,
              hints = known,
            },
          ],
    },
}
