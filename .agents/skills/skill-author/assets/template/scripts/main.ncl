let rdx = import "rdx/api.ncl" in
{
  # Inputs: top-level fields with a contract, a doc and a default where sensible.
  example
    | String
    | doc "TODO"
    | default
    = "TODO",

  plan = rdx.plan {
    skill = "TODO-goal-name",
    goal = "TODO",
    steps = [
      rdx.ops.core.note { message = "TODO replace with real ops: %{example}" },
    ],
  },
}
