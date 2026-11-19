# Nickel quick reference

What this repo uses from [Nickel](https://nickel-lang.org), via `nickel-embed` (on
`nickel-lang-core` 0.19). Read 2026-11-19 from the Nickel docs through Context7.

**Refreshing this file:** query Context7 with these library ids, then update the examples:

| Context7 id | Covers |
| :-- | :-- |
| `/websites/nickel-lang_stdlib` | the standard library (`std.contract`, `std.record`, `std.array`, `std.string`, `std.enum`) |
| `/websites/nickel-lang` | the manual: merging, contracts, typing, metadata |
| `/websites/nickel-lang_getting-started` | syntax basics, `nickel export` |

## Values and syntax

```nickel
let base = 10 in                          # let … in
{
  name = "rdx",                           # record fields end with commas
  tags = ["a", "b"],                      # arrays
  greeting = "hello %{name}",             # interpolation (fields of the same record are in scope)
  text = m%"
    multi-line, indentation stripped
  "%,
  double = fun x => x * 2,                # functions
  tag = 'csv,                             # enum tag; exports as the string "csv"
  nested.path.field = 1,                  # same as nested = { path = { field = 1 } }
  data = import "contributors.yaml",      # imports .ncl, .json, .yaml, .toml
}
```

- `if … then … else …`, `match { 'a => 1, _ => 2 }`, `x |> f`, `a @ b` (array concat),
  `r."%{key}"` (dynamic field), `let rec f = …` (recursive let).
- **Records are recursive.** Inside `{ a = 1, b = a }`, `a` is the field. Writing
  `{ args = args | C }` refers to itself and fails with *infinite recursion*: bind the outer
  value under another name first. Same for a field named like a `let` you want to reference.

## Contracts

```nickel
let Port = std.contract.from_predicate (fun p => std.is_number p && p > 0 && p < 65536) in

let IsZero = std.contract.from_validator (match {
  0 => 'Ok,
  n if std.is_number n => 'Error { message = "expected 0, got %{std.to_string n}", notes = ["…"] },
  v => 'Error { message = "expected a number, got a %{std.typeof v}" },
}) in

let Server = {
  host | String | doc "Host name.",
  port | Port | default = 8080,
  mode | [| 'dev, 'prod |] | default = 'dev,   # enum contract
  tls | Bool | optional,                         # may be absent
} in
{ host = "x" } | Server
```

- Apply with `value | Contract`; several in a row: `field | Array Step | ValidSteps`.
- Record contracts are **closed**: unknown fields fail. Add `..` to allow extra fields:
  `{ host | String, .. }`.
- `{ _ | Dyn }` is a dictionary of anything; `Array String`, `std.contract.Equal 1`,
  `std.contract.any_of [String, Number]` are handy built-ins.
- Prefer `from_predicate` (yes/no) and `from_validator` (custom messages); `std.contract.custom`
  only for delayed or higher-order contracts.
- Errors report the blamed value's source location; `nickel-embed` exposes them as
  `Error::diagnostics()` (file, line, column, message), which `rdx-plan` passes through.

## Metadata and merging

- `| doc "…"` documents a field (shown by `nickel query`, `Program::query`, `Program::docs`).
- `| default = v` is a value that any other definition overrides when records are merged.
- `| force = v` wins over everything except another `force`. `| priority 10 = v` sets a numeric
  priority.
- `a & b` merges records recursively; two plain values for one field conflict unless equal.
  This is how deployment config layers: `base.ncl & cloudflare.ncl & local-overrides.ncl`.

## Static typing

Opt-in with `: Type`. Inside a typed block Nickel checks types statically:

```nickel
let add : Number -> Number -> Number = fun a b => a + b in add 1 2
```

Contracts check data at run time; types check code. Plans and config use contracts.

## Standard library highlights

| Function | Use |
| :-- | :-- |
| `std.record.fields r`, `std.record.has_field "f" r`, `std.record.get_or "f" default r`, `std.record.remove "f" r`, `std.record.map (fun k v => …) r` | records |
| `std.array.map`, `filter`, `fold_left`, `flat_map`, `elem`, `first`, `length` | arrays |
| `std.string.is_match regex s`, `std.string.join sep xs`, `std.to_string v` | strings |
| `std.enum.to_tag_and_arg`, `std.enum.from_tag_and_arg` | enums to and from records |
| `std.is_record`, `std.is_array`, `std.is_string`, `std.is_number`, `std.typeof` | type tests |

## Export

`nickel export file.ncl --format json|yaml|toml`; in Rust,
`Program::builder().source(Source::file(p)).field("plan".parse()?).build()?.export(ExportFormat::Json)`.
Enum tags export as strings. **Enum variants with arguments (`'ref "x"`) do not export**: use
records such as `{ ref = "x" }`, as `api.ncl` does.

## In this repo

- `nickel/rdx/api.ncl`: plan contracts and op constructors (the code-mode API).
- `rdx_plan::PlanSource`: evaluates a skill's `scripts/main.ncl` with `nickel/` on the import path,
  sets inputs with `.input("field", "<nickel expr>")`, and returns a checked `Plan`.
