# `envelope.*`: Gordian Envelope statements (crate rdx-envelope, piece 7).
let lib = import "../lib.ncl" in
let step = lib.step in
let ValueOrRef = lib.ValueOrRef in
let UrOf = lib.UrOf in
let Document = lib.Document in
let NonEmptyString = lib.NonEmptyString in

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

  seal
    | doc m%"
      Signs a statement with this device's key (one salted assertion per line), encrypts it to
      every key in the address's registry and keeps it in the blob store. Returns `{ blob,
      root, statement, device, signer, recipients }`: `signer` is what `verify` checks
      against. Refuses a statement that doesn't balance. Needs `kv`, `secrets`, `random`,
      `blob-store`. Effects.
    "%
    =
      step
        "envelope.seal"
        {
          statement
            | ValueOrRef
            | doc "A statement, or the output of a `statement.parse` step.",
          address | NonEmptyString | doc "The supporter's address, whose registry lists the keys.",
        },

  encrypt
    | doc m%"
      Encrypts an envelope to public keys: a fresh content key, sealed to each recipient.
      `recipients` is a list of `ur:crypto-pubkeys/…` or the output of `keys.recipients`.
      Returns `{ envelope, recipients }` (the count). Servers can run it. Needs `random`. No
      effects.
    "%
    =
      step
        "envelope.encrypt"
        {
          envelope | ValueOrRef,
          recipients | ValueOrRef,
        },

  open
    | doc m%"
      Decrypts an envelope kept in the blob store with this device's key, returning
      `{ envelope, root }` in the clear for `redact` or `verify`. Runs only where the supporter
      is. Needs `kv`, `secrets`, `blob-store`. No effects.
    "%
    =
      step
        "envelope.open"
        {
          blob | Document | doc "The encrypted envelope's blob digest, from `seal`.",
          address | NonEmptyString,
        },

  add-recipients
    | doc m%"
      Lets more keys (a newly added or rotated device) read an envelope kept in the blob
      store, without re-encrypting its content; this device must already be a recipient.
      Stores the result under a new digest: `{ from, to, added }`. Needs `kv`, `secrets`,
      `random`, `blob-store`. Effects.
    "%
    =
      step
        "envelope.add-recipients"
        {
          blob | Document,
          address | NonEmptyString,
          recipients | ValueOrRef | doc "`ur:crypto-pubkeys/…` list, or a `keys.*` step's output.",
        },
}
