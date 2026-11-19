# ADR-0002: Agents drive the system through Nickel plans (code mode)

**Status:** Proposed
**Date:** 2026-11-19
**Deciders:** Ash

## Context

Agents will run reconciliation, onboarding, quarantine triage and publishing. Exposing each
operation as a separate tool makes agents chain many calls, pass intermediate data through their
context and fail halfway through effects. "Code mode" instead has the agent write a short program
against a typed API, run once. The program must be checkable before anything happens, must not
need a general-purpose runtime in every host, and must leave an audit record.

## Decision

A skill's single entrypoint is `scripts/main.ncl`, a Nickel program that imports
`nickel/rdx/api.ncl` and evaluates to a **plan**: a list of steps, each an op id plus arguments,
where later steps refer to earlier ones by id. The core evaluates the program with `nickel-embed`,
contracts check it, the executor validates ops and capabilities, and the plan runs as a dry run
unless `dry_run = false`. The run report records the plan's digest.

## Options considered

### A: One MCP tool per operation
Familiar, but many round trips, intermediate data in the agent's context, no whole-plan check.

### B: Agents write Rust or JavaScript against an SDK
Most expressive, but needs a compiler or JS runtime in each host, and JS breaks the Rust-only rule.

### C: Nickel plans (chosen)
Nickel is pure (no effects during evaluation), has contracts with precise error locations, is
already embedded through `nickel-embed`, and builds for WebAssembly. Plans are data, so they can
be hashed, diffed, stored and replayed.

| Dimension | A | B | C |
| :-- | :-- | :-- | :-- |
| Pre-execution checking | Per call | Compile-time | Whole plan, by contracts |
| Runs in every host | Yes | No | Yes |
| Auditable artefact | Call log | Source | Plan + digest |
| Expressiveness | Low | High | Medium (no loops over effect results) |

## Trade-off analysis

Plans can't branch on the result of an effect, because evaluation finishes before execution. That
is deliberate: branching happens inside ops (for example the matcher decides what matched), and an
agent that needs a decision point runs two plans. The constraint keeps every run reviewable as a
dry run.

## Consequences

- Easier: an agent fixes its plan from contract errors with locations, before any effect.
- Easier: the same plan runs in the PWA, a Worker or the CLI.
- Harder: each new op needs a Nickel constructor and contract next to its Rust code.
- Revisit: if many skills need branching, add a small `when` step form evaluated by the executor.

## Action items

1. [x] `rdx-plan`: plan types, reference checking, executor with dry run and plan digest.
2. [x] `nickel/rdx/api.ncl`: `plan`, `Step`, reference contract and a `core.note` op.
3. [ ] `rdx run` CLI as the local code-mode entrypoint.
4. [ ] Structured diagnostics from `nickel-embed` surfaced in the run report.
