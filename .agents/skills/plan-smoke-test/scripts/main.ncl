# Code-mode entrypoint for plan-smoke-test. Pure: only core ops, no effects.
let rdx = import "rdx/api.ncl" in
{
  host
    | String
    | doc "Label for the host under test; appears in the run notes."
    | default
    = "local",

  plan = rdx.plan {
    skill = "plan-smoke-test",
    goal = "Check that this host can evaluate, link and report a plan",
    steps = [
      rdx.ops.core.note { id = "hello", message = "plan smoke test on %{host}" },
      rdx.ops.core.digest { id = "linked", value = rdx.ref "hello" },
      rdx.ops.core.digest { value = [rdx.ref "hello", rdx.ref "linked"] },
    ],
  },
}
