# Code-mode entrypoint for publish-redacted-statement. Redacting and verifying have no effects, so a
# dry run shows exactly what the viewable statement will say; rendering writes the PDF to the blob
# store and runs only with --execute.
let rdx = import "rdx/api.ncl" in
# Synthetic data, so the skill runs (and is tested) before intake and the PWA exist.
let sample = import "../../../../fixtures/synthetic/small/scenario.json" in
{
  envelope
    | String
    | doc "The supporter's sealed statement, as a full `ur:envelope/…` string (not one already redacted)."
    | default
    = import "../../../../fixtures/synthetic/small/envelopes/st-2026-03.s-0001.full.ur" as 'Text,

  matches
    | doc "Matches for the statement (rdx/model.ncl `Match`). Only confirmed ones keep a line visible."
    | default
    = sample.matches,

  signer
    | String
    | doc "The supporter's public signing key, as a `ur:signing-public-key/…` string, from the organization's key registry."
    | default
    = import "../../../../fixtures/synthetic/small/envelopes/signer.ur" as 'Text,

  plan =
    # Inside a step's record, `envelope = envelope` would refer to that field itself (records are
    # recursive), so the inputs pass through function parameters.
    let publish = fun e m k =>
      [
        rdx.ops.envelope.redact { id = "redacted", envelope = e, matches = m },
        rdx.ops.envelope.verify { id = "view", envelope = rdx.ref "redacted", signer = k },
        rdx.ops.render.statement { id = "pdf", envelope = rdx.ref "redacted", signer = k },
      ]
    in
    rdx.plan {
      skill = "publish-redacted-statement",
      goal = "Redact a supporter's statement to its receipt-confirmed lines and render the viewable PDF",
      steps = publish envelope matches signer,
    },
}
