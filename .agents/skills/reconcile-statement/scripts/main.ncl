# Code-mode entrypoint for reconcile-statement. Dry run only for now: it proposes matches and
# changes nothing. Piece 9 adds the step that books confirmed matches to the ledger.
let rdx = import "rdx/api.ncl" in
# Synthetic data, so the skill runs (and is tested) before statement and receipt intake exist.
let sample = import "../../../../fixtures/synthetic/small/scenario.json" in
{
  statement
    | doc "The statement to reconcile (rdx/model.ncl `Statement`): a value, or `rdx.ref` to the step that parsed it."
    | default
    = std.array.at 0 sample.statements,

  receipts
    | doc "Receipts and bills to match against (`Array Receipt`). Other supporters' receipts are ignored."
    | default
    = sample.receipts,

  decided
    | doc "Earlier decisions (`Array Match`): confirmed lines and receipts are left alone, rejected pairs are not proposed again."
    | default
    = [],

  policy
    | doc "Matching tolerances (rdx/matching.ncl `Policy`). Give only the fields you change."
    | default
    = {},

  plan =
    # Inside the step's record, `statement = statement` would refer to that field itself (records
    # are recursive), so the inputs pass through function parameters.
    let propose = fun s r d p =>
      rdx.ops.matching.propose {
        id = "proposals",
        statement = s,
        receipts = r,
        decided = d,
        policy = p,
      }
    in
    rdx.plan {
      skill = "reconcile-statement",
      goal = "Propose which statement lines the receipts explain, and flag what needs review",
      steps = [propose statement receipts decided policy],
    },
}
