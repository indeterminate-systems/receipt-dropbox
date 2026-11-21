# `matching.*`: proposes statement line ↔ receipt matches (crate rdx-match, piece 6).
# `match` is a Nickel keyword, hence `matching`.
let lib = import "../lib.ncl" in
let matching_policy = import "../matching.ncl" in
let step = lib.step in
let ValueOrRef = lib.ValueOrRef in

{
  propose
    | doc m%"
      Proposes matches between one statement's lines and its supporter's receipts and bills,
      under a matching `policy` (rdx/matching.ncl; omit it for the defaults, or override only
      the tolerances you tune). `decided` lists earlier matches: confirmed lines and receipts
      are left alone and rejected pairs are not proposed again. Output: `proposals` (each
      with `kind`, `review`, `reasons`, `matches`, `evidence`), `unmatched_lines`,
      `unmatched_receipts`, `notes` and a `summary`. Every match is `proposed`, never
      confirmed. No effects.
    "%
    =
      step
        "matching.propose"
        {
          # Usually earlier steps' outputs (`rdx.ref`), so not checked against rdx/model.ncl
          # here; the op checks them in Rust. (Applying those contracts to imported JSON also
          # trips a nickel-lang-core 0.19 panic: see docs/reference/nickel.md.)
          statement | ValueOrRef,
          receipts | ValueOrRef,
          decided | ValueOrRef | default = [],
          policy | matching_policy.Policy | default = {},
        },
}
