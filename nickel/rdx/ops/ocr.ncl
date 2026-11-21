# `ocr.*`: reads scans and photos (crate rdx-ocr, piece 11).
let lib = import "../lib.ncl" in
let step = lib.step in
let DocumentC = lib.Document in

let Ppi =
  std.contract.from_validator (fun p =>
    if std.is_number p && p >= 50 && p <= 1200 then
      'Ok
    else
      'Error { message = "expected pixels per inch from 50 to 1200, such as 200" }
  )
in

{
  read
    | doc m%"
      A document's text, by OCR when it has no text layer: `{ document, source, pages, text,
      warnings }`, the shape of `pdf.text`. A PDF with a text layer gives that layer (`source`
      `text-layer`, no OCR); a scanned PDF or a PNG or JPEG photo is read with OCR (`ocr`); any
      other document gives nothing (`none`). `warnings` lists repaired amounts and images that
      couldn't be read. Pass the output to `receipt.extract` as `document`, or to
      `statement.parse` as `text`. Needs `blob-store`, which must also hold the models when OCR
      runs. No effects.
    "%
    =
      step
        "ocr.read"
        {
          document | DocumentC | doc "The PDF or image's blob digest.",
          models
            | { detection | DocumentC, recognition | DocumentC }
            | doc m%"
              Blob digests of text-detection.rten and text-recognition.rten. The defaults are
              the files the dev shell fetches into $RDX_OCR_MODELS (ADR-P11); a host puts them in
              its blob store once.
            "%
            | default
            = {
              detection = "sha256:f15cfb56bd02c4bf478a20343986504a1f01e1665c2b3a0ad66340f054b1b5ca",
              recognition = "sha256:e484866d4cce403175bd8d00b128feb08ab42e208de30e42cd9889d8f1735a6e",
            },
          ppi
            | Ppi
            | doc "Resolution of a photo, for positions in points (PDFs state their page size)."
            | optional,
        },
}
