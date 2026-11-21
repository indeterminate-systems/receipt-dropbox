# `envelope.*`: Gordian Envelope statements (crate rdx-envelope, piece 7).
let lib = import "../lib.ncl" in
let step = lib.step in
let ValueOrRef = lib.ValueOrRef in
let UrOf = lib.UrOf in

{
  redact
    | doc m%"
      Elides every line of a sealed statement envelope that confirmed matches don't fully
      cover. The root digest and signature stay valid. Returns `{ envelope, root, shown,
      redacted, review }`; `review` lists lines that matches cover only in part. `envelope`
      is a `ur:envelope/…` string or the output of an earlier `envelope.*` step. No effects.
    "%
    =
      step
        "envelope.redact"
        {
          envelope | ValueOrRef,
          matches
            | ValueOrRef
            | doc "Matches as in `model.ncl`; only confirmed ones count."
            | default
            = [],
        },

  verify
    | doc m%"
      Checks a statement envelope's signature against the supporter's public key and returns
      what the viewable statement shows: header, shown lines, `redacted_rows` and
      `summary` (`opening + shown + redacted = closing`), and the envelope's `root` digest.
      Fails if the envelope was changed or someone else signed it. No effects.
    "%
    =
      step
        "envelope.verify"
        {
          envelope | ValueOrRef,
          signer | UrOf "signing-public-key",
        },
}
