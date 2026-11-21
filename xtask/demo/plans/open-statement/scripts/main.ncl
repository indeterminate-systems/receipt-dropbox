# Demo: decrypt the sealed statement on the supporter's device, for redaction.
let rdx = import "rdx/api.ncl" in
{
  blob | String,
  address | String,
  plan =
    let b = blob in
    let a = address in
    rdx.plan {
      skill = "open-statement",
      goal = "Open a supporter's sealed statement on their device",
      steps = [rdx.ops.envelope.open { id = "opened", blob = b, address = a }],
    },
}
