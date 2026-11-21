# `pdf.*`: PDF text layer (crate rdx-pdf, piece 4).
let lib = import "../lib.ncl" in
let step = lib.step in
let DocumentC = lib.Document in

{
  text
    | doc m%"
      The text layer of a PDF in the blob store: `{ document, pages, text }`. `pages` holds
      each line's spans (runs of words split at wide gaps, with left and right edges in
      points); `text` is the same as plain lines. Empty `text` means no text layer (a scan).
      Needs `blob-store`. No effects.
    "%
    = step "pdf.text" { document | DocumentC },
}
