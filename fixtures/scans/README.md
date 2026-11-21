# Scans

Synthetic documents with no text layer, for OCR (piece 11, crate `rdx-ocr`). Made with Typst,
which embeds its fonts, so they regenerate the same anywhere; the commands are in each `.typ`
file.

| File | What it is |
| :-- | :-- |
| `receipt.typ`, `receipt-photo.png` | a Mexican hardware store's till receipt photographed on a desk, slightly rotated: merchant, `Date: 2026-03-19`, `TOTAL MXN 342.78` |
| `example-bank-scan.typ`, `st-2026-03.s-0001.png` | statement `st-2026-03.s-0001` in the `example-bank` layout (`../layouts/example-bank.typ`) at 200 ppi, in DejaVu Sans |
| `st-2026-03.s-0001.scan.pdf` | that page as a grayscale JPEG on a PDF page, as a scanner saves it; written by `crates/rdx-ocr/tests/fixtures.rs` (`UPDATE_GOLDEN=1 cargo test -p rdx-ocr --test fixtures`) |

The tests that read them need the OCR models, which the dev shell puts in `$RDX_OCR_MODELS`.
The skills' golden cases name the files by digest, so regenerating one means accepting new golden
reports too (`UPDATE_GOLDEN=1 cargo xtask check ingest-statement ingest-receipt`).
