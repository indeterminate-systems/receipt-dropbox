# Code-mode entrypoint for reconcile-statement: proposes matches for what is still open and, when
# `decided` confirms some, books them to the ledger (`ledger.book`, then `ledger.post`, which a dry
# run skips).
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

  accounts
    | doc "Expense accounts for the booking (rdx/ledger.ncl `Accounts`): `by_receipt`, `by_merchant` and `fallback`. Give only what you set."
    | default
    = { by_receipt = sample.accounts },

  ledger
    | doc "The ledger confirmed matches are posted to (a name; kv `ledger/<name>` holds its current version)."
    | default
    = "main",

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
    let book = fun s r d a =>
      rdx.ops.ledger.book {
        id = "booking",
        statement = s,
        receipts = r,
        matches = d,
        accounts = a,
      }
    in
    let post = fun l => rdx.ops.ledger.post { id = "posted", booking = rdx.ref "booking", ledger = l } in
    # Booking steps only when there is something to book, so proposing alone never needs a ledger
    # (or the statement's encrypted original).
    let confirmed =
      std.is_array decided
      && std.array.any (fun m => std.is_record m && std.record.get_or "status" null m == "confirmed") decided
    in
    rdx.plan {
      skill = "reconcile-statement",
      goal = "Propose which statement lines the receipts explain, flag what needs review, and book what is confirmed",
      steps =
        [propose statement receipts decided policy]
        @ (if confirmed then [book statement receipts decided accounts, post ledger] else []),
    },
}
