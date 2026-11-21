# `keys.*`: a supporter's keys (crate rdx-keys, piece 8).
#
# Secrets never appear here: passkey PRF output and recovery shares pass between the op and the
# person through the host's `secrets` capability, so no argument, output or audit entry holds one.
let lib = import "../lib.ncl" in
let model = import "../model.ncl" in
let step = lib.step in
let ValueOrRef = lib.ValueOrRef in
let NonEmptyString = lib.NonEmptyString in
let UrOf = lib.UrOf in
let Document = lib.Document in

# Addresses name kv keys, so no spaces or `/`.
let Address =
  std.contract.from_validator (fun a =>
    if std.is_string a && std.string.is_match "^[^\\s/]+$" a then
      'Ok
    else
      'Error {
        message = "expected an email address with no spaces or `/`",
        notes = ["The address supporters send mail from, like `ana@example.org`."],
      }
  )
in

let Device =
  std.contract.from_validator (fun d =>
    if std.is_string d && std.string.is_match "^[a-z0-9][a-z0-9-]*$" d && d != "recovery" then
      'Ok
    else
      'Error {
        message = "expected a device name: lowercase letters, digits and `-`, not `recovery`",
        notes = ["Name it after the device, like `laptop` or `phone-2`; names are never reused."],
      }
  )
in

{
  onboard
    | doc m%"
      Gives a new supporter keys: a key for this device, wrapped by its passkey; a recovery
      key from a fresh seed, which is split into one SSKR share per holder and then forgotten;
      and a signed registry listing both. Each share is shown to the person (label
      `recovery-share-<n>`) to hand to its holder, never put in the output. Returns
      `{ address, supporter, device, keys, recovery, threshold, shares = [{ label, holder }] }`.
      Fails if the address already has keys. Needs `kv`, `random`, `secrets`, `clock`. Effects.
    "%
    =
      step
        "keys.onboard"
        {
          address | Address | doc "The supporter's email address.",
          supporter | model.Id | doc "The supporter's id.",
          device | Device | doc "This device's name." | default = "this-device",
          threshold
            | std.number.PosNat
            | doc "How many shares recover access."
            | default
            = 2,
          holders
            | Array NonEmptyString
            | doc "Who keeps each share; one share each."
            | default
            = ["another device", "printed card", "org custodian"],
        },

  create-device
    | doc m%"
      Makes keys for this (new) device and wraps them with its passkey. Returns
      `{ address, device, keys }`; an active device then enrolls `keys` with `add-device`.
      Needs `kv`, `random`, `secrets`. Effects.
    "%
    =
      step
        "keys.create-device"
        {
          address | Address,
          device | Device,
        },

  add-device
    | doc m%"
      This active device enrolls another device's public keys in the registry, signing the
      event. Returns `{ address, device, added_by, recipients }`. Documents encrypted earlier
      need `envelope.add-recipients` before the new device can read them. Needs `kv`,
      `secrets`, `clock`. Effects.
    "%
    =
      step
        "keys.add-device"
        {
          address | Address,
          device | Device,
          keys | ValueOrRef | doc "The new device's `ur:crypto-pubkeys/…` from `create-device`.",
        },

  rotate
    | doc m%"
      Replaces this device's keys, signed by its current key, and rewraps them with its
      passkey. Returns `{ address, device, keys }`. Needs `kv`, `random`, `secrets`, `clock`.
      Effects.
    "%
    =
      step
        "keys.rotate"
        { address | Address },

  revoke
    | doc m%"
      Retires a device (lost, sold, compromised): nothing new is encrypted to it and it can't
      sign. Signed by this device. Returns `{ address, device, revoked_by, recipients }`.
      Needs `kv`, `secrets`, `clock`. Effects.
    "%
    =
      step
        "keys.revoke"
        {
          address | Address,
          device | Device,
          reason | NonEmptyString | default = "lost",
        },

  recover
    | doc m%"
      Recovers access after devices are lost: asks the person for recovery shares
      (`recovery-share-1`, `-2`, …) until they recover the registry's recovery key, then
      enrolls this device, revokes `revoke`, and lets the new key read each of `documents`
      (stored again under a new digest). Returns `{ address, device, keys, revoked,
      documents = [{ from, to }], recipients }`. Needs `kv`, `random`, `secrets`, `clock`,
      `blob-store`. Effects.
    "%
    =
      step
        "keys.recover"
        {
          address | Address,
          device | Device | doc "A new name for this device.",
          revoke | Array Device | doc "Lost devices to retire." | default = [],
          documents
            | Array Document
            | doc "Encrypted envelopes in the blob store the new key should read."
            | default
            = [],
        },

  recipients
    | doc m%"
      The public keys to encrypt to for an address: every active device and the recovery key,
      from a registry whose every signature checked out. Returns `{ address, supporter,
      recipients, recovery, devices = [{ device, keys, added_at, rotations, status }] }`.
      Servers can run it. Needs `kv`. No effects.
    "%
    =
      step
        "keys.recipients"
        { address | Address },
}
