# `intake.*`: email sent to the organization's receipts address (crate rdx-intake, piece 13).
#
# The Cloudflare Worker runs intake itself on each email; these ops run it on the local host and
# let agents look after quarantines. The claim link that unlocks a quarantine never appears here:
# `receive` mails it to the sender, and `release` asks the person for it through `secrets`.
let lib = import "../lib.ncl" in
let intake = import "../intake.ncl" in
let step = lib.step in
let DocumentC = lib.Document in
let NoArgs = lib.NoArgs in

{
  receive
    | doc m%"
      Takes one email (an .eml in the blob store) through intake under `policy`
      (rdx/intake.ncl): rejected (`status = 'rejected`, with the `reason` the sender would get);
      encrypted to the sender's keys (`delivered`); or, for an allowed sender without keys,
      encrypted to a quarantine key (`quarantined`, with `expires_at`) and, the first time, a
      reply with a claim link sent through `mail-out` (`reply = "sent"`). Output also has
      `address`, `document` (the encrypted copy's digest), `bytes` and `message_deleted`: the
      plaintext .eml is deleted from the blob store unless `keep_message`. Needs `blob-store`,
      `kv`, `clock`, `random`, `mail-out`. Effects.
    "%
    =
      step
        "intake.receive"
        {
          message | DocumentC | doc "The email as received (.eml), with the receiving server's Authentication-Results on top.",
          policy | intake.Policy,
          keep_message
            | Bool
            | doc "Keep the plaintext .eml in the blob store (tests only)."
            | default
            = false,
        },

  status
    | doc m%"
      Every quarantine: `{ quarantined = [{ address, documents, opened_at, expires_at, expired }],
      expired }` (`expired` counts those past their date, which `sweep` deletes). Needs `kv`,
      `clock`. No effects.
    "%
    = step "intake.status" NoArgs,

  sweep
    | doc m%"
      Deletes every expired quarantine's documents and record and mails each sender a notice:
      `{ expired = [{ address, deleted }], notices_sent }`. The quarantine keys were never
      stored, so nothing else remains. Needs `blob-store`, `kv`, `clock`, `mail-out`. Effects.
    "%
    = step "intake.sweep" { policy | intake.Policy },

  release
    | doc m%"
      Claims `address`'s quarantine: asks the person for the claim link (secret `claim-link`),
      re-encrypts each waiting email to the address's keys, lists it in the address's inbox and
      deletes the quarantined copy. Onboard the address first (`keys.onboard`). Output:
      `{ address, documents = [{ from, to }] }`. Needs `blob-store`, `kv`, `random`, `secrets`.
      Effects.
    "%
    =
      step
        "intake.release"
        {
          address | String | doc "The address the claim link was sent to.",
        },
}
