# ADR-P11: OCR with ocrs, models as blobs

**Status:** Proposed
**Date:** 2026-11-21
**Deciders:** Ash

## Context

Some statements and receipts arrive as scans or phone photos, with no text layer for `rdx-pdf`
to read ([ADR-P04](ADR-P04-pdf-text-layer-reader.md)). Piece 11 reads them with OCR and feeds
the result to the existing parsers (`statement.parse`, `receipt.extract`) unchanged. The OCR has
to run where the plaintext is, which is the PWA in the browser or a local host: servers never
decrypt, so they never OCR. It must build for `wasm32-unknown-unknown` and `wasm32-wasip2` with
no JavaScript and no C, like the rest of the core.

## Decision

**Crate `rdx-ocr` wraps [ocrs](https://github.com/robertknight/ocrs) 0.14**, a pure-Rust OCR
engine (text detection and recognition networks run by its pure-Rust runtime, `rten`). Its op
`ocr.read { document, models?, ppi? }` returns the shape of `pdf.text` plus `source` and
`warnings`:

- a PDF with a text layer: that layer (`source: text-layer`); the models are never loaded;
- a scanned PDF: OCR of each page's images, placed by `rdx_pdf::page_images` (new, additive),
  positions converted to points on the page (`ocr`);
- a PNG or JPEG photo: OCR, positions in points at `ppi` (default 200) (`ocr`);
- anything else (CSV, OFX, email): nothing (`none`).

Words are grouped into the same lines and spans as the text layer (a gap over one character
height starts a span), so `example-bank`'s column positions work on scans. `statement.parse`
takes an optional `text`, an `ocr.read` output for the same document, used only when the
document has no text layer of its own; `receipt.extract` already took `pdf.text` output. The
`ingest-statement` and `ingest-receipt` plans read every document through `ocr.read` first.

**The models are blobs, not code.** The two `.rten` files (2.5 MB and 9.7 MB) are not compiled
in. `ocr.read` reads them from the blob store by digest; the defaults in `nickel/rdx/ops/ocr.ncl`
pin the exact files. The dev shell fetches them by hash into `$RDX_OCR_MODELS` (from the
author's bucket, or a Hugging Face mirror pinned to a commit), and tests read them from there,
natively and under wasmtime. A host puts them in its blob store once (`rdx blob put`); the PWA
can fetch and cache them on first use, so a supporter who never sends a scan never downloads
them. No host capability and no WIT change was needed.

**Repairs only where the value is certain.** OCR misreads amount separators (`1.076.98`,
`1:553.71`). A token whose groups fit the grouped-amount shape is rewritten (`1,076.98`), and an
amount written against the document's own decimal convention (`14,23` among `.` decimals) is
brought into line; each repair is listed in `warnings`. Nothing else is corrected: a statement
whose lines don't add up is refused by the existing balance checks, and a receipt missing a field
needs review.

## Options considered

| | ocrs (chosen) | Tesseract compiled to wasm | Browser or OS OCR |
| :-- | :-- | :-- | :-- |
| Language | Rust | C++ (Emscripten), JavaScript glue | JavaScript (Shape Detection API, where it exists) |
| Builds with the core for both wasm targets | Yes, checked by CI | No: separate build, needs a JS loader | No: browser only, not under wasmtime |
| Same result in PWA, local and tests | Yes: one engine, pinned models | Only where the JS build runs | No |
| Accuracy on synthetic scans | Every amount, date and balance of the test statement exact after repairs; a few letters in descriptions wrong | Typically better on dense or degraded text, many languages | Varies by platform |
| Size | ~12 MB of models, fetched on demand | ~10 MB of traineddata per language plus the engine | none |

## Consequences

- Scans of the synthetic statement parse with every amount and balance exact, and a synthetic
  receipt photo becomes a complete receipt (`crates/rdx-ocr/tests/plan.rs`). Descriptions can
  carry misread letters (`PAPERI FAF` for `PAPERLEAF`), which matching must tolerate; the
  `example-bank` footer misread as `Page L of 1` joined the last line's description.
- The models' licence: the author publishes them as CC-BY-SA-4.0 on Hugging Face (trained on
  HierText); the mirror used says MIT. Shipping them to supporters' browsers needs attribution
  and, to be safe, treating them as CC-BY-SA. This is an open question for Ash.
- The `rten` and `ocrs` crates are built optimized in the dev profile (workspace `Cargo.toml`),
  because unoptimized inference made a test take two minutes; the OCR tests now take seconds
  natively and about 35 s per test binary under wasmtime.
- Not handled yet: CCITT fax and JBIG2 scans (reported as skipped images), rotated or
  perspective-skewed photos beyond a degree or two, handwriting, and accuracy on real-world scans,
  which the SROIE and CORD receipt datasets in the roadmap can measure next.
- ocrs pulls `wasm-bindgen` on wasm32 for a logging macro it doesn't use here; the wasip2 tests
  run, so no vendored patch was needed. If a later ocrs release uses it, vendor it as
  `bc-shamir` is (vendor/README.md).
- If OCR quality on real documents is not good enough, the op's shape lets another engine replace
  ocrs (or run as a component extension) without changing skills or parsers.
