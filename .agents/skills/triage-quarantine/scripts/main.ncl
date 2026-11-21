# Code-mode entrypoint for triage-quarantine. `intake.status` has no effects, so a dry run already
# lists every quarantine and which have expired; `intake.sweep` deletes the expired ones and
# mails their senders, so it runs only with --execute.
let rdx = import "rdx/api.ncl" in
let intake = import "rdx/intake.ncl" in
{
  policy
    | intake.Policy
    | doc "The organization's intake policy (rdx/intake.ncl). Sweep uses its intake address to sign the notices.",

  plan =
    # Bound under another name: inside the step record, `policy` would mean the field itself.
    let p = policy in
    rdx.plan {
      skill = "triage-quarantine",
      goal = "Show mail waiting for unverified senders and delete what has expired",
      steps = [
        rdx.ops.intake.status { id = "status" },
        rdx.ops.intake.sweep { id = "sweep", policy = p },
      ],
    },
}
