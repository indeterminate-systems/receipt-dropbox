# ADR-0006: A small PDF text-layer reader of our own

**Status:** Proposed
**Date:** 2026-11-20
**Deciders:** Ash

## Context

Statements (piece 4) and receipts (piece 5) mostly arrive as PDFs. The core must read their text
layer in Rust that builds for `wasm32-unknown-unknown` (PWA, Workers) and `wasm32-wasip2`, with
no JavaScript and no I/O. Statement parsing also needs positions, not just text: a withdrawal and
a deposit differ only by the column the amount sits in.

## Decision

`crates/rdx-pdf`: a reader of our own, about 1,500 lines, that does only what text extraction
needs. It scans for objects rather than trusting the cross-reference table, reads object
streams, inflates Flate streams (`miniz_oxide`, pure Rust), interprets the text operators of
content streams and form XObjects, maps codes to Unicode through ToUnicode CMaps or simple
encodings, and groups glyphs into lines and spans with their left and right edges. Encrypted
PDFs are refused with a message; scans give an empty text layer for OCR (piece 11).

## Options considered

| | lopdf + own extraction | pdf-extract | Own reader |
| :-- | :-- | :-- | :-- |
| wasm32-unknown-unknown | No: lopdf depends on getrandom, which needs its JavaScript backend there | No: built on lopdf | Yes |
| Positions for columns | Ours to write anyway | Text only (positions through a custom output hook) | Spans with edges |
| Encrypted PDFs | Yes | Yes | No (refused) |
| Size of dependency tree | Large (nom, encoding_rs, crypto) | Larger | `miniz_oxide` |

## Trade-off analysis

The libraries do more (encryption, images, writing) than extraction needs, and the one hard
constraint, building for the browser target without JavaScript, rules them out today. The text
operators and font mapping are the part any option would make us write, because column positions
matter. The cost is owning a parser of untrusted input: it is bounded (nesting depth, stream size,
page-tree depth), has a garbage-input test, and was checked on Typst output and on five public
synthetic statements from the Bankstatemently benchmark (Singapore, US, Dutch, Hong Kong with CJK
text, Canadian French), which it read cleanly.

## Consequences

- Password-protected statements need an unencrypted copy until encryption support is added
  (RC4 and AES with an empty user password would cover most bank downloads).
- If lopdf drops getrandom or makes it optional, this can be revisited; the public API
  (`rdx_pdf::text`, `Text`, `Line`, `Span`) would stay.
- Fuzzing the reader is worth adding before the PWA accepts uploads from the public.
