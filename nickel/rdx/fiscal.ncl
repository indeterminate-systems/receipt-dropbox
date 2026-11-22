# Proof policy: what counts as proof of an organization expense (docs/compliance.md R0), checked
# by `fiscal.check` (crate rdx-fiscal, piece 15).
#
# An organization picks its `kind` and fiscal year; the rules for that year come from `by_year`:
#
#   let fiscal = import "rdx/fiscal.ncl" in
#   {
#     kind = "nonprofit",
#     fiscal_year = 2026,
#     profile = { rfc = "XIA190128J61", name = "ORGANIZACIÓN DE PRUEBA", postcode = "06000", regime = "603" },
#   } | fiscal.Policy
#
# To change one rule, merge into the year's rules: `rules = fiscal.by_year."2026" & { cash.threshold.minor = 100000 }`.
#
# Every figure here is research, not advice: `confirmed = false` until a contador confirms the
# year's rules, and `fiscal.check` says so on every run. Amounts are minor units, never floats.
let m = import "rdx/model.ncl" in

let kinds = ["company", "nonprofit", "donataria-autorizada", "individual", "informal-group"] in

let KindC =
  std.contract.from_validator (fun k =>
    if std.is_string k && std.array.elem k kinds then
      'Ok
    else
      'Error {
        message = "unknown proof policy",
        notes = [
          "Use one of: %{std.string.join ", " kinds}.",
          "An organization with no RFC of its own is `informal-group`; see docs/compliance.md.",
        ],
      }
  )
in

let RfcC =
  std.contract.from_validator (fun s =>
    if std.is_string s && std.string.is_match "^[A-ZÑ&]{3,4}[0-9]{6}[A-Z0-9]{3}$" s then
      'Ok
    else
      'Error {
        message = "invalid RFC",
        notes = ["An RFC is 12 characters for a persona moral or 13 for a persona física, in capitals, such as `XIA190128J61`."],
      }
  )
in

let Digits = fun n what =>
  std.contract.from_validator (fun s =>
    if std.is_string s && std.string.is_match "^[0-9]{%{std.to_string n}}$" s then
      'Ok
    else
      'Error { message = "expected %{what}: %{std.to_string n} digits as a string" }
  )
in

let Code =
  std.contract.from_validator (fun s =>
    if std.is_string s && std.string.is_match "^[A-Z0-9]{2,4}$" s then
      'Ok
    else
      'Error { message = "expected a SAT catalog code such as `01` or `G03`" }
  )
in

let NonNegative =
  std.contract.from_validator (fun n =>
    if std.is_number n && std.number.is_integer n && n >= 0 then
      'Ok
    else
      'Error { message = "expected a whole number, 0 or more" }
  )
in

let ForeignField =
  std.contract.from_validator (fun f =>
    let known = ["vendor", "tax_id", "address", "description", "buyer"] in
    if std.is_string f && std.array.elem f known then
      'Ok
    else
      'Error {
        message = "unknown foreign invoice field",
        notes = ["Use any of: %{std.string.join ", " known}. Date and amount always come from the receipt."],
      }
  )
in

let ProfileC = {
  rfc | RfcC | doc "The organization's RFC (or, for `individual`, the owner's).",
  name | String | doc "The name exactly as the SAT's Constancia de Situación Fiscal prints it.",
  postcode | Digits 5 "a postcode" | doc "Domicilio fiscal postcode.",
  regime | Digits 3 "a tax regime" | doc "SAT `c_RegimenFiscal`, such as 603 (Personas Morales con Fines no Lucrativos).",
  uso_cfdi
    | Array Code
    | doc "The UsoCFDI values accepted on expense CFDIs. G03 (gastos en general) unless a contador says otherwise."
    | default
    = ["G03"],
}
in

let RulesC = {
  confirmed
    | Bool
    | doc "Whether a contador has confirmed these rules for the year. While false, every run warns."
    | default
    = false,
  cash = {
    forms
      | Array Code
      | doc "`c_FormaPago` codes that mean cash (LISR art. 27 fr. III)."
      | default
      = ["01"],
    threshold
      | m.Money
      | doc "Above this, a cash payment is not deductible even with a CFDI."
      # Per-field defaults, so overriding `minor` keeps `currency`.
      = { minor | default = 200000, currency | default = "MXN" },
  },
  fuel_product_codes
    | Array String
    | doc "`ClaveProdServ` prefixes that are fuel, which may never be paid in cash."
    | default
    = ["151015"],
  cfdi_window = {
    days_before
      | NonNegative
      | doc "How many days before the receipt's date a CFDI may be dated without review."
      | default
      = 3,
    days_after
      | NonNegative
      | doc "How many days after the receipt's date a CFDI may be dated without review."
      | default
      = 31,
  },
  foreign_invoice_fields
    | Array ForeignField
    | doc "What a foreign vendor's invoice must show to be accepted as proof (R17; the RMF's minimum, to confirm)."
    | default
    = ["vendor", "tax_id", "address", "description", "buyer"],
}
in

# Per fiscal year, so a change in the law is a new entry, and earlier years keep their rules.
let by_year_ = {
  "2025" = {} | RulesC,
  "2026" = {} | RulesC,
}
in

{
  Kind = KindC,
  Rfc = RfcC,
  Profile = ProfileC,
  Rules = RulesC,
  by_year = by_year_,

  Policy = {
    kind | KindC | doc "What kind of organization this is, which decides what proof an expense needs.",
    sat_registered
      | Bool
      | doc "For `individual`: whether the owner is registered with the SAT for this activity. When false, the `informal-group` rules apply, with a warning."
      | default
      = true,
    fiscal_year | Number | doc "The fiscal year whose rules apply.",
    profile
      | ProfileC
      | doc "The organization's fiscal profile. Required for every kind but `informal-group`."
      | optional,
    rules
      | RulesC
      | doc "The year's rules. Default: `by_year` for `fiscal_year`."
      | default
      = let year = std.to_string fiscal_year in
        if std.record.has_field year by_year_ then
          by_year_."%{year}"
        else
          std.fail_with "no proof rules for fiscal year %{year}: add them to `by_year` in rdx/fiscal.ncl, or give `rules`",
  },
}
