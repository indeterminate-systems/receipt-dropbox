# Helpers for op constructors, shared by api.ncl and the per-domain files in ops/.
#
# A domain's constructors live in `ops/<domain>.ncl`, listed in ops.ncl, so a piece that adds a
# domain adds a file instead of editing a shared one.

# Builds a step constructor: takes `{ id ? , ..args }`, checks args against `Args`.
let step_ = fun op_id Args =>
  fun input =>
    let has_id = std.record.has_field "id" input in
    # Bound under another name: inside the record below, `args` would mean the field itself.
    let given = if has_id then std.record.remove "id" input else input in
    (if has_id then { id = input.id } else {})
    & { op = op_id, args = given | Args }
in

# A value or a reference to an earlier step's output.
let ValueOrRefC = Dyn in

let NonEmptyStringC =
  std.contract.from_validator (fun s =>
    if std.is_string s && s != "" then 'Ok else 'Error { message = "expected a non-empty string" }
  )
in

# `{}` as a contract accepts any record, so ops without arguments say so explicitly.
let NoArgsC =
  std.contract.from_validator (fun r =>
    if std.is_record r && r == {} then
      'Ok
    else
      'Error {
        message = "this op takes no arguments",
        notes = ["Unexpected: %{std.string.join ", " (std.record.fields r)}"],
      }
  )
in

let UrOfC = fun kind =>
  std.contract.from_validator (fun s =>
    if std.is_string s && std.string.is_match "^\\s*ur:%{kind}/[a-z]+\\s*$" s then
      'Ok
    else
      'Error {
        message = "expected a `ur:%{kind}/…` string",
        notes = ["URs are lowercase letters after the type, like `ur:%{kind}/lfaohdcx…`."],
      }
  )
in

# A document in the host's blob store: its digest, or a reference to a step that produced one.
let DocumentC =
  std.contract.from_validator (fun d =>
    if std.is_string d && std.string.is_match "^sha256:[0-9a-f]{64}$" d then
      'Ok
    else if std.is_record d && std.record.fields d == ["ref"] then
      'Ok
    else
      'Error {
        message = "expected a document digest like `sha256:<64 hex digits>`",
        notes = ["Put the file in the blob store first; locally, `rdx blob put FILE` prints its digest."],
      }
  )
in

let DecimalTextC =
  std.contract.from_validator (fun s =>
    if std.is_string s && std.string.is_match "^-?[0-9]+([.][0-9]+)?$" s then
      'Ok
    else
      'Error {
        message = "expected an amount as decimal text, like \"744.55\" or \"-12.00\"",
        notes = ["Amounts are text so they are never floating point."],
      }
  )
in

# Records are recursive in Nickel, so the helpers are bound under other names above.
{
  step = step_,
  ValueOrRef = ValueOrRefC,
  NonEmptyString = NonEmptyStringC,
  NoArgs = NoArgsC,
  UrOf = UrOfC,
  Document = DocumentC,
  DecimalText = DecimalTextC,
}
