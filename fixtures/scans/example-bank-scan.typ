// The `example-bank` statement as a scanned page: the same layout (layouts/example-bank.typ) in
// DejaVu Sans, a font the bank might print in, rendered to an image with no text layer. OCR
// (piece 11) reads it back; crates/rdx-ocr/tests/fixtures.rs wraps it in a PDF as a scanner would.
//
// Regenerate the committed PNG from the repository root in the dev shell:
//   typst compile --root . --input statement=fixtures/layouts/st-2026-03.s-0001.json \
//     --format png --ppi 200 fixtures/scans/example-bank-scan.typ \
//     fixtures/scans/st-2026-03.s-0001.png

#set text(font: "DejaVu Sans")
#include "/fixtures/layouts/example-bank.typ"
