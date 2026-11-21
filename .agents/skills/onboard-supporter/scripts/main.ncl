# Code-mode entrypoint for onboard-supporter. One step with effects (keys, registry, shares), so
# a dry run shows what would be created and `--execute` creates it.
let rdx = import "rdx/api.ncl" in
{
  address
    | String
    | doc "The supporter's email address: the one they send statements and bills from.",

  supporter
    | String
    | doc "The supporter's id, such as \"s-0001\".",

  device
    | String
    | doc "A name for this device, such as \"laptop\"; lowercase, digits and `-`."
    | default
    = "this-device",

  threshold
    | Number
    | doc "How many recovery shares recover access."
    | default
    = 2,

  holders
    | Array String
    | doc "Who keeps each recovery share; one share each."
    | default
    = ["another device", "printed card", "org custodian"],

  # Bound under other names: inside the step record below, the field names would mean themselves.
  plan =
    let a = address in
    let s = supporter in
    let d = device in
    let t = threshold in
    let h = holders in
    rdx.plan {
      skill = "onboard-supporter",
      goal = "Give a new supporter keys on this device and recovery shares for their holders",
      steps = [
        rdx.ops.keys.onboard {
          id = "keys",
          address = a,
          supporter = s,
          device = d,
          threshold = t,
          holders = h,
        },
      ],
    },
}
