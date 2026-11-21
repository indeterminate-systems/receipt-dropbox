// A till receipt photographed on a desk: no text layer, so only OCR can read it (piece 11).
// Made-up merchant; `EKU9003173C9` is SAT's published test RFC.
//
// Regenerate the committed PNG from the repository root in the dev shell:
//   typst compile --root . --format png --ppi 200 fixtures/scans/receipt.typ \
//     fixtures/scans/receipt-photo.png

#set page(width: 3.4in, height: 4.6in, margin: 0.25in, fill: rgb("#d9d4c7"))
#set text(font: "DejaVu Sans Mono", size: 9pt)

#rotate(-0.6deg, block(fill: rgb("#fbfaf6"), inset: 12pt, width: 100%)[
  #align(center)[
    #text(12pt, weight: "bold")[HARBOR HARDWARE] \
    RFC EKU9003173C9 \
    Av. del Puerto 118
  ]
  #v(6pt)
  Ticket: HH-20417 \
  Date: 2026-03-19
  #v(6pt)
  #grid(
    columns: (1fr, auto),
    row-gutter: 5pt,
    [Extension cord 5m], [189.00],
    [Cable ties x100], [64.50],
    [Masking tape], [42.00],
    [Subtotal], [295.50],
    [IVA 16%], [47.28],
    text(weight: "bold")[TOTAL MXN], text(weight: "bold")[342.78],
  )
  #v(6pt)
  #align(center)[Paid by card \*\*\*\* 0001]
])
