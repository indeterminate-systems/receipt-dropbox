# Agent Skills quick reference

Summary of the [Agent Skills specification](https://agentskills.io/specification), read
2026-11-19. The spec shows no version number or date. `cargo xtask check` enforces the rules marked
**(checked)**; this repo's own rules are at the end.

## Layout

```
skill-name/
├── SKILL.md          required: YAML frontmatter + Markdown instructions
├── scripts/          optional: executable code
├── references/       optional: docs loaded on demand
├── assets/           optional: templates, images, data
└── …                 any other files
```

## Frontmatter

| Field | Required | Rule |
| :-- | :-- | :-- |
| `name` | yes | 1-64 characters; `a-z`, `0-9` and `-`; not starting or ending with `-`; no `--`; **must match the directory name** **(checked)** |
| `description` | yes | 1-1024 characters; says what the skill does **and when to use it**, with keywords agents will match **(checked)** |
| `license` | no | a license name or a bundled file, kept short |
| `compatibility` | no | 1-500 characters; environment requirements (product, packages, network). Most skills omit it **(checked)** |
| `metadata` | no | map of string keys to string values; use distinctive key names **(checked: strings only)** |
| `allowed-tools` | no | experimental; space-separated pre-approved tools, e.g. `Bash(git:*) Read` |

Unknown frontmatter fields are rejected by `cargo xtask check`.

## Body

No format restrictions. Recommended: step-by-step instructions, input and output examples,
edge cases. Keep `SKILL.md` **under 500 lines** **(checked)** and move detail to referenced files.

## Progressive disclosure

1. **Discovery** (~100 tokens): `name` and `description` of every skill are loaded at startup.
2. **Activation** (under ~5000 tokens recommended): the whole `SKILL.md` body.
3. **Execution**: files in `scripts/`, `references/`, `assets/` only when needed.

So the `description` decides whether a skill is ever used, and the body should stay lean.

## Files

- `scripts/`: self-contained or with documented dependencies, helpful errors, edge cases handled.
- `references/`: focused files (smaller is cheaper to load).
- `assets/`: templates, images, lookup tables, schemas.
- Link files by **relative path from the skill root**, and keep references **one level deep**.

## Validation

The spec's reference validator is
[`skills-ref`](https://github.com/agentskills/agentskills/tree/main/skills-ref):
`skills-ref validate ./my-skill`. This repo uses `cargo xtask check`, which covers the same
frontmatter rules, so the toolchain stays Rust-only.

## This repo's rules

- Skills live in `.agents/skills/`; `.claude/skills` is a symlink to it **(checked)**.
- `metadata.rdx-kind` is `product` (a goal agents run) or `dev` (how to work on the repo) **(checked)**.
- Product skills have `scripts/main.ncl`, the code-mode entrypoint, which evaluates to a record
  with a `plan` field **(checked)**, and at least one `tests/<case>.json` golden dry run **(checked)**.
- One goal per product skill. See [the workflow](../workflow.md).
