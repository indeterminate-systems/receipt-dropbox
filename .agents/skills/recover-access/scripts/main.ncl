# Code-mode entrypoint for recover-access. One step with effects (a new device key, registry
# events, re-stored documents); the person gives the recovery shares to the host, not the plan.
let rdx = import "rdx/api.ncl" in
{
  address
    | String
    | doc "The supporter's email address.",

  device
    | String
    | doc "A new name for this device, such as \"phone-2\"; names are never reused.",

  revoke
    | Array String
    | doc "Devices that were lost or replaced, to retire."
    | default
    = [],

  documents
    | Array String
    | doc "Blob digests of encrypted envelopes (sealed statements) this device should read."
    | default
    = [],

  plan =
    let a = address in
    let d = device in
    let r = revoke in
    let docs = documents in
    rdx.plan {
      skill = "recover-access",
      goal = "Recover a supporter's access with recovery shares after losing devices",
      steps = [
        rdx.ops.keys.recover { id = "recovered", address = a, device = d, revoke = r, documents = docs },
      ],
    },
}
