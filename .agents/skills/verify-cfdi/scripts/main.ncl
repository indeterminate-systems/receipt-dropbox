# Code-mode entrypoint for verify-cfdi. No effects: the assessment goes in the run report.
#
# With a CFDI and no `receipt`, the CFDI is read into a receipt first (`receipt.extract`), so
# the check covers the CFDI on its own: is it the organization's, stamped, current and paid in a
# way that counts. With a `receipt` (a ticket matched to a statement line), the CFDI is checked
# against that purchase too.
let rdx = import "rdx/api.ncl" in
{
  supporter
    | String
    | doc "Id of the supporter who paid, such as `s-0001`.",

  policy
    | doc m%"
      The organization's proof policy (rdx/fiscal.ncl `Policy`): `kind`, `fiscal_year` and,
      unless it is an `informal-group`, its fiscal `profile`. The year's rules are the default.
    "%,

  cfdi
    | doc "The CFDI's XML text: `import \"/path/to/factura.xml\" as 'Text`. `null` when there is none (a foreign invoice, or only a receipt)."
    | default
    = null,

  receipt
    | doc "The receipt this CFDI should prove (rdx/model.ncl `Receipt`), or `null` to read it from the CFDI."
    | default
    = null,

  payments
    | doc "XML texts of Pagos complements (CFDI type P) that pay a PPD CFDI."
    | default
    = [],

  sat
    | doc "The SAT status check: `{ status = \"vigente\" | \"cancelado\" | \"no-encontrado\", efos = \"none\" | \"presunto\" | \"definitivo\" | …, checked = \"<RFC 3339 time>\" }`, or `null` if not checked yet."
    | default
    = null,

  foreign
    | doc "For a foreign vendor that can't issue a CFDI: `{ vendor, tax_id, address, description, buyer, country, digital_platform }`."
    | default
    = null,

  paid_with
    | doc "How the supporter paid, from the matched statement line: `cash`, `card`, `transfer` or `other`."
    | default
    = null,

  purpose
    | doc "What the expense was for. Non-profits need it."
    | default
    = null,

  class
    | doc "For a donataria autorizada: `administration` or `mission`."
    | default
    = null,

  # Inputs pass through function parameters: inside the plan's records, `cfdi = cfdi` would refer
  # to the field itself (records are recursive).
  plan =
    let build = fun who pol xml given pays status abroad paid why cls =>
      let extract = xml != null && given == null in
      let opt = fun name value => if value == null then {} else { "%{name}" = value } in
      let expense =
        { receipt = if extract then rdx.ref "receipt" else given, payments = pays }
        & opt "cfdi" xml
        & opt "sat" status
        & opt "foreign" abroad
        & opt "paid_with" paid
        & opt "purpose" why
        & opt "class" cls
      in
      rdx.plan {
        skill = "verify-cfdi",
        goal = "Check that a CFDI (or what stands in for one) proves an organization expense under its proof policy",
        steps =
          (if extract then
             [
               rdx.ops.receipt.extract {
                 id = "receipt",
                 supporter = who,
                 document = xml,
                 media_type = "application/xml",
               },
             ]
           else
             [])
          @ [rdx.ops.fiscal.check { id = "proof", policy = pol, expenses = [expense] }],
      }
    in
    build supporter policy cfdi receipt payments sat foreign paid_with purpose class,
}
