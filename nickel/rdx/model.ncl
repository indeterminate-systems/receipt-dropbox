# Receipt Dropbox domain model: contracts for the JSON shapes `rdx-model` serializes.
#
# Op contracts in api.ncl reuse these, so a statement or receipt in a plan is checked before the
# core sees it. They repeat the Rust checks that a Nickel author can get wrong by hand (integer
# minor units, known currencies, balanced statements); the Rust types stay authoritative.
#
#   let m = import "rdx/model.ncl" in
#   { minor = 4217, currency = "USD" } | m.Money
#
# Contracts are bound with a `C` suffix and exported without it: a record field named like the
# variable it refers to would refer to itself (Nickel records are recursive).
let one_of = fun what values =>
  std.contract.from_validator (fun v =>
    if std.is_string v && std.array.elem v values then
      'Ok
    else
      'Error {
        message = "invalid %{what}",
        notes = ["Expected one of: %{std.string.join ", " values}."],
      }
  )
in

let matching = fun what regex hint =>
  std.contract.from_validator (fun v =>
    if std.is_string v && std.string.is_match regex v then
      'Ok
    else
      'Error { message = "invalid %{what}", notes = [hint] }
  )
in

# Keep in step with KNOWN in crates/rdx-model/src/money.rs.
let CurrencyC =
  one_of "currency" [
    "AUD", "BRL", "CAD", "CHF", "DKK", "EUR", "GBP", "INR",
    "JPY", "KRW", "KWD", "MXN", "NOK", "NZD", "SEK", "USD",
  ]
in

let MoneyC = {
  minor
    | std.number.Integer
    | doc "Integer minor units (cents for USD). Never a decimal: 42.17 USD is `4217`.",
  currency | CurrencyC,
}
in

let DateC = matching "date" "^[0-9]{4}-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])$" "Use YYYY-MM-DD." in

let DigestC = matching "digest" "^sha256:[0-9a-f]{64}$" "Use `sha256:` and 64 lowercase hex digits." in

let IdC =
  matching
    "id"
    "^[a-z0-9][a-z0-9._-]{0,127}$"
    "Use lowercase letters, digits, `.`, `_` and `-`, starting with a letter or digit."
in

let AccountC =
  matching
    "account"
    "^(Assets|Liabilities|Equity|Income|Expenses)(:[A-Z0-9][A-Za-z0-9-]*)+$"
    "Beancount accounts start with Assets, Liabilities, Equity, Income or Expenses, then `:Component`s that start with an uppercase letter or digit."
in

let PeriodC = { start | DateC, end | DateC } in

let StatementLineC = {
  id | IdC,
  date | DateC,
  description | String,
  amount | MoneyC | doc "Negative for a debit (money leaving the account).",
  reference | String | optional,
}
in

let BalancedC =
  std.contract.from_validator (fun s =>
    let total = std.array.fold_left (fun acc l => acc + l.amount.minor) s.opening.minor s.lines in
    if total == s.closing.minor then
      'Ok
    else
      'Error {
        message = "statement `%{s.id}` does not balance",
        notes = [
          "Opening plus lines is %{std.to_string total} minor units, but closing is %{std.to_string s.closing.minor}.",
        ],
      }
  )
in

let StatementC =
  std.contract.Sequence [
    {
      id | IdC,
      supporter | IdC,
      account | { institution | String, mask | String },
      currency | CurrencyC,
      period | PeriodC,
      opening | MoneyC,
      closing | MoneyC,
      lines | Array StatementLineC,
      original | DigestC | doc "Digest of the encrypted original." | optional,
    },
    BalancedC,
  ]
in

let ReceiptC = {
  id | IdC,
  supporter | IdC,
  kind | one_of "receipt kind" ["receipt", "bill", "refund"],
  merchant | String,
  date | DateC,
  total | MoneyC | doc "Always positive; a refund's total is what came back.",
  reference | String | optional,
  items | Array { description | String, amount | MoneyC } | optional,
  source | DigestC,
}
in

let LineRefC = { statement | IdC, line | IdC } in

let MatchC = {
  line | LineRefC,
  receipt | IdC,
  amount | MoneyC | doc "How much of the line the receipt explains, positive.",
  status | one_of "match status" ["proposed", "confirmed", "rejected"],
}
in

let ScenarioC = {
  config | {
    seed | std.number.Integer,
    supporters | std.number.Integer,
    months | std.number.Integer,
    start | DateC,
    currency | CurrencyC,
  },
  supporters | Array { id | IdC, display_name | String },
  statements | Array StatementC,
  receipts | Array ReceiptC,
  matches | Array MatchC,
  accounts | { _ | AccountC },
  cases
    | Array {
      kind
        | one_of
          "case kind"
          ["date_lag", "bill_reference", "refund", "missing_receipt", "orphan_receipt", "personal"],
      line | LineRefC | optional,
      receipt | IdC | optional,
    },
}
in

{
  Currency = CurrencyC,
  Money = MoneyC,
  Date = DateC,
  Digest = DigestC,
  Id = IdC,
  Account = AccountC,
  Period = PeriodC,
  StatementLine = StatementLineC,
  Statement = StatementC,
  Receipt = ReceiptC,
  LineRef = LineRefC,
  Match = MatchC,
  Scenario = ScenarioC,
}
