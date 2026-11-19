---
name: skill-author
description: Writes or changes a Receipt Dropbox product skill - one goal, a Nickel code-mode entrypoint that composes ops into a plan, and golden test cases - following the Agent Skills spec. Use when turning working ops into something an agent can run, or when a skill's plan, inputs or SKILL.md need to change.
metadata:
  rdx-kind: dev
---

# Skill author

You turn ops into goals. Read [docs/workflow.md](../../../docs/workflow.md) first: this skill is
stages 4 and 5 of it. The ops a skill uses must already exist with tests (stages 1 to 3).

## Shape of a product skill

Copy [`assets/template/`](assets/template/) to `.agents/skills/<name>/` and fill it in.

- **One goal per skill**, named for what a person asks for: `reconcile-statement`,
  `onboard-supporter`, `triage-quarantine`. Not one skill per op.
- `SKILL.md` frontmatter: `name` equals the directory (a-z, 0-9, single hyphens, at most 64),
  `description` says what it does *and when to use it* in at most 1024 characters, and
  `metadata.rdx-kind: product`. Keep the body under 500 lines; move op details to `references/`.
  The full rules are in [docs/reference/agent-skills.md](../../../docs/reference/agent-skills.md).
- `scripts/main.ncl` is the only entrypoint. Inputs are top-level fields with a contract, a `doc`
  and, where sensible, a `default`; `plan` is built with `rdx.plan` from `rdx.ops.*`
  constructors. Name steps with `id` only when a later step refers to them with `rdx.ref`.
- Leave `dry_run` at its default (true). The person or host decides to run for real.
- The body of `SKILL.md` explains how to read the run report: what `ok`, `skipped` and each
  likely failure mean, and what to do next.

## Test first

1. Write `tests/<case>.json` with the `inputs` for the case and `"expect": null`.
2. Run `cargo xtask check <name>` and watch it fail (the report differs from `null`).
3. Write `scripts/main.ncl` until the plan evaluates, then read the report it prints. When it is
   what the goal needs, accept it with `UPDATE_GOLDEN=1 cargo xtask check <name>`.
4. Review the golden diff like code: a changed `plan_digest` means the plan changed.
5. Run it as an agent would: `cargo run -p rdx-cli -- run <name>` (dry run on the local host,
   state in `.rdx/`). Golden cases run on an in-memory host with a fixed clock
   (2026-01-01T00:00:00Z), seeded randomness and empty stores, so their reports never change
   between machines.

Add a case for each input that changes the plan's shape, and one per failure the `SKILL.md` body
tells agents how to handle.

## Nickel

Use [docs/reference/nickel.md](../../../docs/reference/nickel.md). Two traps that bite here:

- Records are recursive: inside `{ args = args | C }` the right-hand `args` is the field itself
  (infinite recursion). Bind values under a different name first.
- Exported enums become strings; enum variants with arguments don't export. Use records.

## Before you finish

- `cargo xtask check` passes for every skill, and `cargo test` passes.
- The `description` would make an agent pick this skill for the goal, and not for neighbouring
  goals.
