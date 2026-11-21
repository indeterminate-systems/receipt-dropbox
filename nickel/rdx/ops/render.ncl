# `render.*`: viewable statements rendered by Typst (crate rdx-render, piece 10).
let lib = import "../lib.ncl" in
let step = lib.step in
let ValueOrRef = lib.ValueOrRef in
let UrOf = lib.UrOf in

{
  statement
    | doc m%"
      Verifies a redacted statement envelope against the supporter's public key and renders
      what it shows to a PDF in the blob store: shown lines, one "Personal, redacted" row per
      elided line, opening and closing balances, the total redacted, the root digest, and the
      envelope as a QR code, an attachment and printed text. Returns `{ document, pages, qr,
      root, statement, shown, redacted_rows }`. Pass `envelope.redact`'s output: a full
      envelope would render every line. Effects: writes the PDF to the blob store.
    "%
    =
      step
        "render.statement"
        {
          envelope | ValueOrRef,
          signer | UrOf "signing-public-key",
        },
}
