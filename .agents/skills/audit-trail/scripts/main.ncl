# Code-mode entrypoint for audit-trail. No effects and no decryption: it reads the ledger, the
# custody records and the given viewable statements, so any host (a server included) can run it.
let rdx = import "rdx/api.ncl" in
{
  ledger
    | String
    | doc "A ledger name (its current version) or a version digest `sha256:…`."
    | default
    = "main",

  views
    | Array { envelope | String, signer | String }
    | doc "Viewable (redacted) statements to check against the ledger: each `{ envelope = \"ur:envelope/…\", signer = \"ur:signing-public-key/…\" }`."
    | default
    = [],

  plan =
    let trail = fun l vs =>
      let checks =
        std.array.generate
          (fun i =>
            let v = std.array.at i vs in
            rdx.ops.envelope.verify { id = "view-%{std.to_string i}", envelope = v.envelope, signer = v.signer }
          )
          (std.array.length vs)
      in
      [rdx.ops.ledger.read { id = "ledger", ledger = l }]
      @ checks
      @ [
        rdx.ops.audit.trail {
          id = "trail",
          ledger = rdx.ref "ledger",
          views = std.array.map (fun c => rdx.ref c.id) checks,
        },
      ]
    in
    rdx.plan {
      skill = "audit-trail",
      goal = "Trace every ledger entry and viewable row to its receipt, encrypted original and grants",
      steps = trail ledger views,
    },
}
