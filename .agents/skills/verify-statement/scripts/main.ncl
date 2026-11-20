# Code-mode entrypoint for verify-statement. No effects and no host capabilities: it only checks a
# signature and reads what the envelope shows.
let rdx = import "rdx/api.ncl" in
{
  envelope
    | String
    | doc "The statement envelope to check, as a `ur:envelope/…` string (redacted or full)."
    | default
    = import "../../../../fixtures/synthetic/small/envelopes/st-2026-03.s-0001.redacted.ur" as 'Text,

  signer
    | String
    | doc "The supporter's public signing key, as a `ur:signing-public-key/…` string."
    | default
    = import "../../../../fixtures/synthetic/small/envelopes/signer.ur" as 'Text,

  # Bound under other names: inside the step record below, `envelope` would mean the field itself.
  plan = let statement = envelope in
    let key = signer in
    rdx.plan {
    skill = "verify-statement",
    goal = "Check a statement envelope's signature and read what its viewable statement shows",
    steps = [
      rdx.ops.envelope.verify { id = "view", envelope = statement, signer = key },
    ],
  },
}
