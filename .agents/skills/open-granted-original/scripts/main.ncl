# Code-mode entrypoint for open-granted-original, run on the grantee's device. Opening the statement
# and verifying it have no effects; opening the bank's document writes it to this device's blob
# store, so that step runs only with --execute.
let rdx = import "rdx/api.ncl" in
{
  address
    | String
    | doc "The grantee's own address: their device key opens the copies."
    | default
    = "accountant@example.org",

  sealed
    | String
    | doc "The statement copy's blob digest (`sealed` in the grant)."
    | default
    = "sha256:0000000000000000000000000000000000000000000000000000000000000000",

  source
    | String
    | doc "The original document copy's blob digest (`source` in the grant); empty when only the statement was granted."
    | default
    = "",

  signer
    | String
    | doc "The supporter's public signing key, `ur:signing-public-key/…`, from the organization's key registry."
    | default
    = import "../../../../fixtures/synthetic/small/envelopes/signer.ur" as 'Text,

  plan =
    let open = fun a s src k =>
      [
        rdx.ops.envelope.open { id = "statement", blob = s, address = a },
        rdx.ops.envelope.verify { id = "view", envelope = rdx.ref "statement", signer = k },
      ]
      @ (if src == "" then [] else [rdx.ops.audit.open-original { id = "document", blob = src, address = a }])
    in
    rdx.plan {
      skill = "open-granted-original",
      goal = "Open and check a statement and original a supporter granted",
      steps = open address sealed source signer,
    },
}
