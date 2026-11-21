# Code-mode entrypoint for receive-email: one step with effects (it stores the encrypted email,
# may reply, and deletes the plaintext), so a dry run shows what it would run with and
# --execute runs it.
let rdx = import "rdx/api.ncl" in
let intake = import "rdx/intake.ncl" in
{
  message
    | String
    | doc "The email's blob digest (`rdx blob put mail.eml`), with the receiving server's Authentication-Results field on top.",

  policy
    | intake.Policy
    | doc "The organization's intake policy (rdx/intake.ncl).",

  plan =
    let m = message in
    let p = policy in
    rdx.plan {
      skill = "receive-email",
      goal = "Take one emailed statement or bill into intake, encrypted on arrival",
      steps = [
        rdx.ops.intake.receive { id = "receive", message = m, policy = p },
      ],
    },
}
