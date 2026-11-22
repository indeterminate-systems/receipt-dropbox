# Code-mode entrypoint for keep-statement. Both steps have effects (they sign, encrypt and store),
# so a dry run shows only the arguments they would run with; --execute keeps the statement.
let rdx = import "rdx/api.ncl" in
# Synthetic data, so the skill runs (and is tested) before intake and the PWA exist.
let sample = import "../../../../fixtures/synthetic/small/scenario.json" in
{
  statement
    | doc "The parsed statement: an `ingest-statement` report's `statement` output, whose `original` is the bank's document in the blob store."
    | default
    = std.array.at 0 sample.statements,

  address
    | String
    | doc "The supporter's email address: their device key signs, and the original is encrypted to their keys."
    | default
    = "supporter1@example.org",

  plan =
    # Inside a step's record, `address = address` would refer to that field itself (records are
    # recursive), so the inputs pass through function parameters.
    let keep = fun s a =>
      [
        rdx.ops.envelope.seal { id = "sealed", statement = s, address = a },
        rdx.ops.audit.keep { id = "kept", sealed = rdx.ref "sealed", address = a },
      ]
    in
    rdx.plan {
      skill = "keep-statement",
      goal = "Seal a supporter's statement and keep its original encrypted, with a custody record",
      steps = keep statement address,
    },
}
