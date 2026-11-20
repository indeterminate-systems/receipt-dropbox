# Code-mode entrypoint for plan-smoke-test. The only effect is one kv write, which a dry run skips.
let rdx = import "rdx/api.ncl" in
{
  host
    | String
    | doc "Label for the host under test; appears in the run notes and the kv record."
    | default
    = "local",

  plan = rdx.plan {
    skill = "plan-smoke-test",
    goal = "Check that this host can evaluate, link and report a plan, and reach its capabilities",
    steps = [
      rdx.ops.core.note { id = "hello", message = "plan smoke test on %{host}" },
      rdx.ops.core.digest { id = "linked", value = rdx.ref "hello" },
      rdx.ops.core.digest { value = [rdx.ref "hello", rdx.ref "linked"] },
      rdx.ops.clock.now { id = "now" },
      rdx.ops.kv.get { id = "previous", key = "plan-smoke-test/last-run" },
      rdx.ops.kv.put {
        key = "plan-smoke-test/last-run",
        value = { label = host, at = rdx.ref "now", previous = rdx.ref "previous" },
      },
    ],
  },
}
