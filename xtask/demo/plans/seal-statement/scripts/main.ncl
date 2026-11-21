# Demo: sign the parsed statement with this device's key, encrypt it to the supporter's keys
# and keep it in the blob store. No product skill does this yet.
let rdx = import "rdx/api.ncl" in
{
  statement | doc "A parsed statement (Statement).",
  address | String,
  plan =
    let s = statement in
    let a = address in
    rdx.plan {
      skill = "seal-statement",
      goal = "Sign and encrypt a supporter's statement",
      steps = [rdx.ops.envelope.seal { id = "sealed", statement = s, address = a }],
    },
}
