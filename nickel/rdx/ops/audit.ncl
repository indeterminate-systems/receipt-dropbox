# `audit.*`: custody of encrypted originals, grants, and the audit trail (crate rdx-audit,
# piece 12). Custody records live in kv `audit/custody/<statement id>`.
let lib = import "../lib.ncl" in
let step = lib.step in
let ValueOrRef = lib.ValueOrRef in
let NonEmptyString = lib.NonEmptyString in
let Document = lib.Document in

# Addresses and statement ids name kv keys, so no spaces or `/`.
let Name =
  std.contract.from_validator (fun a =>
    if std.is_string a && std.string.is_match "^[^\\s/]+$" a then
      'Ok
    else
      'Error {
        message = "expected an address or statement id with no spaces or `/`",
        notes = ["Like `ana@example.org` or `st-2026-03.s-0001`."],
      }
  )
in

{
  keep
    | doc m%"
      Keeps a sealed statement's original encrypted: opens and verifies the sealed envelope on
      this device, encrypts the bank's document (the `original` the statement names, which must
      be in the blob store) to the supporter's keys, records custody in kv
      `audit/custody/<statement>` (event `kept`, signed by this device) and deletes the
      plaintext document. Returns `{ statement, address, device, root, sealed, original,
      source, recipients }`. Refuses a statement already kept. Needs `kv`, `secrets`, `random`,
      `blob-store`, `clock`. Effects.
    "%
    =
      step
        "audit.keep"
        {
          sealed | ValueOrRef | doc "The output of an `envelope.seal` step.",
          address | Name | doc "The supporter's address, whose keys the original is encrypted to.",
        },

  grant
    | doc m%"
      Lets someone (an accountant, an auditor) open a kept statement and its original: adds
      their keys to copies of both, stores the copies and records a signed `granted` event.
      Only the supporter's device can run it. Returns `{ statement, grantee, granted_by,
      reason, sealed, source, root, original, signer, recipients }`: the grantee opens `sealed`
      with `envelope.open`, checks it with `envelope.verify` and `signer`, and opens `source`
      with `audit.open-original`. Needs `kv`, `secrets`, `random`, `blob-store`, `clock`.
      Effects.
    "%
    =
      step
        "audit.grant"
        {
          statement | Name | doc "The statement id.",
          address | Name | doc "The supporter's address.",
          to
            | ValueOrRef
            | doc "The grantee's `ur:crypto-pubkeys/…` list, or a `keys.recipients` output.",
          grantee | NonEmptyString | doc "Who is granted, as custody records it (their address).",
          reason | NonEmptyString | doc "Why, as custody records it.",
          original
            | Bool
            | doc "Grant the bank's document as well as the signed statement."
            | default
            = true,
        },

  revoke
    | doc m%"
      Ends a grant: deletes the grantee's copies and records a signed `revoked` event. What the
      grantee already opened stays with them. Returns `{ statement, grantee, revoked_by, reason,
      deleted }`. Needs `kv`, `secrets`, `blob-store`, `clock`. Effects.
    "%
    =
      step
        "audit.revoke"
        {
          statement | Name,
          address | Name,
          grantee | NonEmptyString,
          reason | NonEmptyString | default = "grant ended",
        },

  open-original
    | doc m%"
      Opens an encrypted original (a granted copy, or the supporter's own) with this device's
      key for `address`, checks the bytes against the digest the statement was signed with, and
      puts the document in this device's blob store. Returns `{ document, statement, bytes }`;
      `document` equals the verified statement's `original`. Needs `kv`, `secrets`,
      `blob-store`. Effects (it writes the plaintext document).
    "%
    =
      step
        "audit.open-original"
        {
          blob | Document | doc "The encrypted original's blob digest (`source` from `grant`).",
          address | Name | doc "The address whose device key opens it (the grantee's).",
        },

  trail
    | doc m%"
      Follows every ledger entry to its receipt, its statement's custody record (envelope root,
      encrypted original, grants) and, for statements in `views`, whether the viewable statement
      shows it. Reports problems: a booked statement nobody kept, an original that differs from
      the kept one, missing blobs, custody events signed by keys the supporter's registry never
      listed, a view whose root isn't the kept one, a shown row nothing books, a booked line the
      view redacts. Returns `{ ok, ledger, entries, statements, problems }`. Decrypts nothing.
      Needs `kv`, `blob-store`. No effects.
    "%
    =
      step
        "audit.trail"
        {
          ledger | ValueOrRef | doc "A `ledger.read` output (usually `rdx.ref`), or a list of entries.",
          views
            | ValueOrRef
            | doc "`envelope.verify` outputs of the viewable statements to check."
            | default
            = [],
        },
}
