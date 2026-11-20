# Receipt Dropbox code-mode API.
#
# A skill's `scripts/main.ncl` imports this file and evaluates to a record whose `plan` field is a
# `Plan`. Contracts here check the whole plan (step arguments, unique ids, references to earlier
# steps) before the core runs anything. `rdx-plan` repeats the structural checks in Rust, because
# plans can also arrive as JSON.
#
# Each op has a constructor under `ops`. Adding an op: see docs/workflow.md.
let StepId =
  std.contract.from_validator (fun id =>
    if std.is_string id && std.string.is_match "^[a-z][a-z0-9_-]*$" id then
      'Ok
    else
      'Error {
        message = "invalid step id",
        notes = ["Step ids start with a lowercase letter and contain only a-z, 0-9, `_` and `-`."],
      }
  )
in

let StepC = {
  id
    | StepId
    | doc "Name later steps use to refer to this step's output with `ref`."
    | optional,
  op
    | String
    | doc "Operation id, such as `core.digest`. The host's registry must know it.",
  args
    | { _ | Dyn }
    | doc "Arguments, already checked by the op's constructor."
    | default
    = {},
}
in

# A reference to an earlier step's output. Exported as `{ "ref": "<id>" }`.
let RefC = { ref | StepId } in

let rec refs_in = fun value =>
  if std.is_record value then
    if std.record.fields value == ["ref"] then
      [value.ref]
    else
      std.array.flat_map (fun f => refs_in value."%{f}") (std.record.fields value)
  else if std.is_array value then
    std.array.flat_map refs_in value
  else
    []
in

# Walks the steps in order, tracking the ids seen so far.
let check_steps = fun steps =>
  std.array.fold_left
    (fun acc step =>
      if acc.error != null then
        acc
      else
        let missing =
          std.array.filter
            (fun r => !(std.array.elem r acc.seen))
            (refs_in (std.record.get_or "args" {} step))
        in
        let has_id = std.record.has_field "id" step in
        if missing != [] then
          {
            seen = acc.seen,
            error = "step `%{step.op}` refers to `%{std.array.first missing}`, which is not an earlier step",
          }
        else if has_id && std.array.elem step.id acc.seen then
          { seen = acc.seen, error = "duplicate step id `%{step.id}`" }
        else
          { seen = acc.seen @ (if has_id then [step.id] else []), error = null }
    )
    { seen = [], error = null }
    steps
in

let ValidSteps =
  std.contract.from_validator (fun steps =>
    let result = check_steps steps in
    if result.error == null then 'Ok else 'Error { message = result.error }
  )
in

let PlanC = {
  version
    | std.contract.Equal 1
    | doc "Plan format version."
    | default
    = 1,
  skill
    | String
    | doc "The skill this plan belongs to (its directory name).",
  goal
    | String
    | doc "One sentence: what running this plan achieves.",
  dry_run
    | Bool
    | doc "When true (the default), ops with effects report what they would do instead of doing it."
    | default
    = true,
  steps
    | Array StepC
    | ValidSteps
    | doc "Steps in execution order.",
}
in

# Builds a step constructor: takes `{ id ? , ..args }`, checks args against `Args`.
let step = fun op_id Args => fun input =>
  let has_id = std.record.has_field "id" input in
  # Bound under another name: inside the record below, `args` would mean the field itself.
  let given = if has_id then std.record.remove "id" input else input in
  (if has_id then { id = input.id } else {})
  & { op = op_id, args = given | Args }
in

# A value or a reference to an earlier step's output.
let ValueOrRef = Dyn in

let NonEmptyString =
  std.contract.from_validator (fun s =>
    if std.is_string s && s != "" then 'Ok else 'Error { message = "expected a non-empty string" }
  )
in

# `{}` as a contract accepts any record, so ops without arguments say so explicitly.
let NoArgs =
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

let UrOf = fun kind =>
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

# Deliberately loose: one `@` with text on both sides. The mail host does the real checking.
let EmailAddress =
  std.contract.from_validator (fun s =>
    if std.is_string s && std.string.is_match "^[^@\\s]+@[^@\\s]+$" s then
      'Ok
    else
      'Error { message = "expected an email address like `name@example.org`" }
  )
in

{
  # Records are recursive in Nickel, so the contracts are bound under other names above.
  Step = StepC,
  Ref = RefC,
  Plan = PlanC,

  plan
    | doc "Checks a plan record against `Plan`."
    = fun p => p | PlanC,

  ref
    | doc "Refers to the output of the earlier step with this id."
    = fun id => { ref = id } | RefC,

  ops = {
    core = {
      note
        | doc "Adds a message to the run report. No effects."
        = step "core.note" { message | String },

      digest
        | doc m%"
          SHA-256 of a value's canonical JSON (keys sorted, no whitespace), as `sha256:<hex>`.
          Use it to record what a step produced in the audit trail. No effects.
        "%
        = step "core.digest" { value | ValueOrRef },
    },

    envelope = {
      redact
        | doc m%"
          Elides every line of a sealed statement envelope that confirmed matches don't fully
          cover. The root digest and signature stay valid. Returns `{ envelope, root, shown,
          redacted, review }`; `review` lists lines that matches cover only in part. `envelope`
          is a `ur:envelope/…` string or the output of an earlier `envelope.*` step. No effects.
        "%
        = step "envelope.redact" {
          envelope | ValueOrRef,
          matches
            | ValueOrRef
            | doc "Matches as in `model.ncl`; only confirmed ones count."
            | default
            = [],
        },

      verify
        | doc m%"
          Checks a statement envelope's signature against the supporter's public key and returns
          what the viewable statement shows: header, shown lines, `redacted_rows` and
          `summary` (`opening + shown + redacted = closing`), and the envelope's `root` digest.
          Fails if the envelope was changed or someone else signed it. No effects.
        "%
        = step "envelope.verify" {
          envelope | ValueOrRef,
          signer | UrOf "signing-public-key",
        },
    },

    # Ops below need host capabilities; a host without one rejects the plan before it runs.

    clock = {
      now
        | doc "The host's current time as `{ unix_ms }`. Needs `clock`. No effects."
        = step "clock.now" NoArgs,
    },

    kv = {
      get
        | doc "The JSON value stored under `key`, or null. Needs `kv`. No effects."
        = step "kv.get" { key | NonEmptyString },

      put
        | doc m%"
          Stores a JSON value under `key`, replacing any value there. Needs `kv`. Has effects:
          skipped in a dry run, where the report shows the arguments it would use.
        "%
        = step "kv.put" { key | NonEmptyString, value | ValueOrRef },
    },

    mail = {
      send
        | doc m%"
          Sends a plain-text email. Needs `mail-out`. Has effects: skipped in a dry run. The
          local host writes mail to its outbox directory instead of sending it.
        "%
        = step "mail.send" {
          to | EmailAddress,
          subject | NonEmptyString,
          text | String,
        },
    },
  },
}
