# `fiscal.*`: fiscal proof of expenses (crate rdx-fiscal, piece 15).
let lib = import "../lib.ncl" in
let fiscal = import "../fiscal.ncl" in
let step = lib.step in
let ValueOrRef = lib.ValueOrRef in

{
  check
    | doc m%"
      Checks whether each expense has the proof the organization's `policy` (rdx/fiscal.ncl)
      asks for. Each expense is `{ receipt, cfdi?, payments?, foreign?, sat?, paid_with?,
      purpose?, class? }`: `receipt` is a receipt or a `receipt.extract` output, `cfdi` the
      CFDI's XML text, `payments` the XML of its Pagos complements, `foreign` a foreign
      vendor's invoice details, `sat` a SAT status check result (`status`, `efos`, `checked`).
      Output: `assessments` (per receipt: `proof` of `supported`, `needs_review`,
      `payment_evidence_only` or `not_supported`, its `basis`, `findings` with `fix`es and
      ledger `meta`), policy `warnings` and a `summary`. No effects.
    "%
    =
      step
        "fiscal.check"
        {
          policy | fiscal.Policy,
          # Usually built from earlier steps' outputs, so checked in Rust.
          expenses | ValueOrRef,
        },
}
