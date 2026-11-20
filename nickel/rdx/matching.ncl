# Matching policy: the tolerances `matching.propose` uses to pair statement lines with receipts.
#
# Every field has a default, so `{} | Policy` is the default policy and an organization overrides
# only what it tunes:
#
#   let matching = import "rdx/matching.ncl" in
#   { amount.tolerance_minor = 50, merchant.min_similarity = 70 } | matching.Policy
#
# Numbers are integers on purpose: the WebAssembly core must give the same answer on every host,
# so amounts are minor units, shares are basis points (1/100 of a percent) or percent, and
# exchange rates are decimal strings. The Rust side (`rdx_match::Policy`) has no defaults of its
# own: this file is the single source of them.
let m = import "rdx/model.ncl" in

let NonNegative =
  std.contract.from_validator (fun n =>
    if std.is_number n && std.number.is_integer n && n >= 0 then
      'Ok
    else
      'Error { message = "expected a whole number, 0 or more" }
  )
in

let Percent =
  std.contract.from_validator (fun n =>
    if std.is_number n && std.number.is_integer n && n >= 0 && n <= 100 then
      'Ok
    else
      'Error { message = "expected a whole percentage from 0 to 100" }
  )
in

let Rate =
  std.contract.from_validator (fun s =>
    if std.is_string s && std.string.is_match "^[0-9]{1,12}(\\.[0-9]{1,12})?$" s && !(std.string.is_match "^[0.]+$" s) then
      'Ok
    else
      'Error {
        message = "invalid exchange rate",
        notes = [
          "Write the rate as a positive decimal string, such as \"17.2345\" (units of `to` per one unit of `from`), never a float.",
        ],
      }
  )
in

# A date window with per-field defaults, so overriding `days_before` keeps `days_after`.
let window = fun before after =>
  {
    days_before
      | NonNegative
      | doc "How many days before the statement line's posting date the document may be dated."
      | default
      = before,
    days_after
      | NonNegative
      | doc "How many days after the posting date the document may be dated."
      | default
      = after,
  }
in

let PolicyC = {
  amount
    | doc "How far a line's amount may be from a same-currency document's total and still match. The larger of the two tolerances applies. Any difference sends the match to review."
    = {
      tolerance_minor
        | NonNegative
        | doc "Absolute tolerance in the statement currency's minor units (cents)."
        | default
        = 0,
      tolerance_bps
        | NonNegative
        | doc "Relative tolerance in basis points of the document's total (100 = 1%), for tips and card surcharges."
        | default
        = 0,
    },

  date
    | doc "Date windows per document kind, around the line's posting date. Card payments post a few days after the receipt; bills are paid days or weeks after they are issued."
    = {
      receipt = window 5 2,
      bill = window 45 10,
      refund = window 10 3,
    },

  merchant
    | doc "How alike the document's merchant and the line's description must be."
    = {
      min_similarity
        | Percent
        | doc "Share of the shorter name's words found in the other (truncated words count). A matching reference number waives it."
        | default
        = 60,
    },

  split
    | doc "Split payments (one line, several documents) and instalments (one document, several lines). Always sent to review."
    = {
      max_parts
        | NonNegative
        | doc "The most documents one line, or lines one document, may be split into. 0 or 1 turns the search off."
        | default
        = 3,
    },

  partial
    | doc "A line that pays part of one document (a deposit, or the rest paid elsewhere). Always sent to review."
    = {
      enabled | Bool | default = true,
    },

  fx
    | doc "Documents in another currency than the statement ([compliance](../../docs/compliance.md) R16). Always sent to review."
    = {
      tolerance_bps
        | NonNegative
        | doc "Tolerance in basis points of the converted amount, for the card network's rate and fees (300 = 3%)."
        | default
        = 300,
      rates
        | Array {
          from | m.Currency | doc "The document's currency.",
          to | m.Currency | doc "The statement's currency.",
          rate | Rate | doc "Units of `to` for one unit of `from`, as a decimal string.",
          date | m.Date | doc "The day the rate applies to (for example the Banxico FIX of that day).",
          source | String | doc "Where the rate comes from, recorded with the match." | default = "policy",
        }
        | doc "Exchange rates. For each document, the rate for its pair dated closest to the line's posting date is used; without one, the document stays unmatched and the report says why."
        | default
        = [],
    },
}
in

{
  Policy = PolicyC,

  defaults
    | doc "The default policy, fully expanded."
    = {} | PolicyC,
}
