# The TDD to skills workflow

Every piece of functionality goes through the same five stages. Each stage ends with something
that a test or `cargo xtask check` can verify, so an agent always knows whether it is done.

```
 1. Contract      2. Failing test     3. Op            4. Skill            5. Skill test
 nickel/rdx/  →   crates/*/tests  →   Rust Op impl →   .agents/skills/ →   fixtures +
 contract for     golden fixture      passes tests;    <goal>/scripts/     golden plan
 args + output    + expected output   ops/<domain>.ncl main.ncl composes   run report
                                                       ops into a plan
```

## 1. Contract first

Write the Nickel contract for the op's arguments and output in `nickel/rdx/` (see
[the Nickel quick reference](reference/nickel.md)). The contract is the op's specification: it
documents fields with `doc`, gives defaults, and rejects bad input with a message an agent can act
on.

## 2. A failing test

Add a test in the crate that will own the op, driven by a synthetic fixture in `fixtures/` and a
golden expected output. Run it and watch it fail for the right reason. Never put real statements
or personal data in `fixtures/`.

## 3. The op

Implement the `Op` trait (`rdx_plan::Op`): `id()`, `effects()` (does it change anything outside
the run?) and `run()`. Put its constructor, applying the contract from stage 1, in the domain's
own `nickel/rdx/ops/<domain>.ncl`, which skills reach as `rdx.ops.<domain>.<op>`. The test from
stage 2 passes; so does the crate's `wasm32` check.

A new domain crate is wired in without editing any shared list beyond one line in each of two
indexes, which merge with `union` (`.gitattributes`) so parallel pieces don't conflict:

- `crates/rdx-cli/Cargo.toml`: a dependency on the crate, which sets
  `[package.metadata.rdx] ops = true` and exports `pub fn register(&mut Registry)`;
  `crates/rdx-cli/build.rs` generates the calls, so `rdx` and `cargo xtask check` run its ops.
- `nickel/rdx/ops.ncl`: `<domain> = import "ops/<domain>.ncl",`.

`crates/rdx-cli/tests/api.rs` fails if a registered op has no constructor or the reverse.

## 4. The skill

A skill is built around **one goal** a person would ask for ("reconcile this month's statement",
"onboard a new supporter"), not around one op. It lives in `.agents/skills/<name>/` and follows
[the Agent Skills spec](reference/agent-skills.md):

```
.agents/skills/<name>/
  SKILL.md            goal, when to use, how to read the run report
  scripts/main.ncl    the code-mode entrypoint: evaluates to a plan
  scripts/*.ncl       other plans or helpers main.ncl imports (optional)
  references/         op details the agent loads only when needed (optional)
  tests/              inputs and the expected run report for each case
```

`scripts/main.ncl` takes its inputs as overridable fields, so one file serves every run:

```nickel
let rdx = import "rdx/api.ncl" in
{
  statement | String | default = "fixtures/statements/sample.csv",
  plan = rdx.plan {
    skill = "reconcile-statement",
    goal = "Match a statement to receipts and book the matches",
    steps = [ rdx.ops.statement.parse { id = "stmt", input = statement }, … ],
  },
}
```

## 5. The skill test

`cargo xtask check` evaluates every skill's `scripts/main.ncl`, checks the result against the
`Plan` contract, and runs each case in `tests/` as a dry run, comparing the run report with the
golden file. A skill is done when it validates, its cases pass and its `SKILL.md` tells an agent
how to act on the report.

## Personas

Two development skills ship with the scaffold:

- `skill-author`: stages 4 and 5, and the spec rules.
- `op-author`: stages 1 to 3.
