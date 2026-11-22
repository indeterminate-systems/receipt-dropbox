# Code-mode entrypoint for grant-original. Looking up the grantee's keys has no effects; granting
# and revoking sign custody events and store or delete copies, so they run only with --execute.
let rdx = import "rdx/api.ncl" in
{
  statement
    | String
    | doc "The statement id, such as \"st-2026-03.s-0001\"."
    | default
    = "st-2026-03.s-0001",

  address
    | String
    | doc "The supporter's address: only their device can grant their statements."
    | default
    = "supporter1@example.org",

  grantee
    | String
    | doc "Who is granted (an accountant's or auditor's address); custody records this name."
    | default
    = "accountant@example.org",

  keys
    | Array String
    | doc "The grantee's `ur:crypto-pubkeys/…`. Empty: look them up in the grantee's key registry on this host (`keys.recipients`)."
    | default
    = [],

  reason
    | String
    | doc "Why the grantee needs the original, as custody records it."
    | default
    = "accountant review",

  original
    | Bool
    | doc "Grant the bank's document as well as the signed statement."
    | default
    = true,

  revoke
    | Bool
    | doc "End the grantee's grant instead: delete their copies and record it."
    | default
    = false,

  plan =
    let grant = fun st a g ks why orig =>
      let recipients = if ks == [] then rdx.ref "keys" else ks in
      (if ks == [] then [rdx.ops.keys.recipients { id = "keys", address = g }] else [])
      @ [
        rdx.ops.audit.grant {
          id = "grant",
          statement = st,
          address = a,
          to = recipients,
          grantee = g,
          reason = why,
          original = orig,
        },
      ]
    in
    let withdraw = fun st a g why =>
      [
        rdx.ops.audit.revoke { id = "revoke", statement = st, address = a, grantee = g, reason = why },
      ]
    in
    rdx.plan {
      skill = "grant-original",
      goal = "Let an accountant open a supporter's original statement, or end that access",
      steps =
        if revoke then
          withdraw statement address grantee (if reason == "accountant review" then "grant ended" else reason)
        else
          grant statement address grantee keys reason original,
    },
}
