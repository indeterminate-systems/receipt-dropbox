# ADR-P10: Typst in the core, rendering only verified envelope data

**Status:** Proposed
**Date:** 2026-11-21
**Deciders:** Ash

## Context

Piece 10 turns a redacted statement envelope (piece 7) into the viewable statement accountants
read: shown lines, "Personal, redacted" rows, opening and closing balances, the total redacted,
the root digest and a QR/UR of the envelope. The project already chose Typst for rendering and
ruled out drawing boxes over the original PDF. Open questions were how Typst runs in a core that
builds for `wasm32-unknown-unknown` and `wasm32-wasip2` and does no I/O, what the template is
allowed to see, and how the QR code is made.

## Decision

`crates/rdx-render` links the `typst` compiler (0.15) and `typst-pdf` as libraries and renders
with a minimal `World` of its own:

- **One source, no files.** The template (`src/statement.typ`) is compiled into the crate. The
  world serves that source and nothing else: no file system, no packages, no system fonts, no
  date. Fonts come from `typst-assets` (Libertinus Serif and DejaVu Sans Mono are used).
- **Rust verifies, Typst lays out.** The only public entry point, `render_envelope(envelope,
  signer)`, runs `rdx_envelope::verify` first and renders the `StatementView` it returns. Every
  figure (balances, total redacted, amounts) is computed and formatted in Rust and reaches Typst
  as text in `sys.inputs.data`, read with `json(…)`. The template does no arithmetic, and data
  is never spliced into Typst source, so a bank description can't inject markup.
- **The PDF carries its proof.** The root digest is on every page. The envelope UR is in a QR
  code (uppercase, so it uses QR's alphanumeric mode, at medium error correction, falling back to
  low), attached to the PDF as `envelope.ur`, and printed on the last page in 64-character lines.
  The QR is drawn as SVG by `rdx-render` from `qrcode`'s module matrix (no image crates).
- **Reproducible.** No timestamp, and the PDF identifier is derived from the root digest: the
  same envelope gives the same bytes on every platform.
- **Op and skill.** `render.statement { envelope, signer }` writes the PDF to the blob store
  (effects, so dry runs skip it). `publish-redacted-statement` chains `envelope.redact`,
  `envelope.verify` and `render.statement`, so its dry run already shows every figure the PDF
  will print.

## Options considered

| | Typst as a library (chosen) | Typst CLI on the host | HTML/CSS view in the PWA |
| :-- | :-- | :-- | :-- |
| Runs in the browser and under wasmtime | Yes, both targets; tests run under wasmtime | No: a native process, not in the PWA or a Worker | Browser only |
| A PDF accountants can file | Yes | Yes | Print-to-PDF, varies by browser |
| Same bytes everywhere | Yes | Depends on installed fonts and version | No |
| Size | Large (below) | None in the core | Small |

A Typst QR package (for example `tiaoma`) was ruled out: packages are downloaded, and the core
fetches nothing. Passing a pre-built `StatementView` to the renderer was ruled out too: the
renderer would then trust its caller's figures, where verifying inside it means nothing
unverified can be rendered.

## Consequences

- **Size.** Typst brings its standard library's data (hyphenation, syntax highlighting,
  bibliography styles) and about 10 MB of fonts. The release `rdx.wasm` (wasm32-wasip2) grows
  from 20.1 MB to 66.5 MB, 3.4 MB to 19.8 MB gzipped. The PWA (piece 14) should load the
  renderer as a separate module when someone publishes, not with the app, and the Cloudflare
  Worker (piece 13) must not link `rdx-render`: it never renders, and Workers cap script size.
  Typst has no feature flags to drop that data; shrinking it is follow-up work.
- **Rust version.** Typst 0.15 needs Rust 1.92. `rdx-render` first set its own `rust-version`;
  the housekeeping PR after wave 3 raised the workspace floor to 1.92 and dropped the override.
- **Order of redacted rows.** Elision hides each line's position, so "Personal, redacted" rows
  come after the shown lines. The PDF says so.
- **Full envelopes render too.** `render.statement` verifies but can't tell a full envelope with
  every line matched from a careless call with an unredacted one, so publishing goes through the
  skill, which always redacts first.
- **Large statements.** A redacted envelope over 4,296 characters (QR's largest code; a statement
  of a few dozen lines, fewer when many are shown) won't fit one QR code; the PDF then says to use the attachment or the printed UR. Animated
  multi-part URs don't work on paper.
