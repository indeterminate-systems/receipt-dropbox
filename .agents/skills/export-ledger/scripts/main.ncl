# Code-mode entrypoint for export-ledger: reads a ledger (no effects) and exports it as a
# spreadsheet (an effect: it stores a file, so it runs only with --execute).
let rdx = import "rdx/api.ncl" in
{
  ledger
    | doc "A ledger name (its current version) or a version digest `sha256:…`."
    | default
    = "main",

  format
    | doc "Spreadsheet format: 'csv or 'xlsx."
    | default
    = 'csv,

  plan =
    # Inside a step's record, `ledger = ledger` would refer to that field itself, so the inputs
    # pass through function parameters.
    let read = fun l => rdx.ops.ledger.read { id = "ledger", ledger = l } in
    let export = fun l f => rdx.ops.ledger.export { id = "export", ledger = l, format = f } in
    rdx.plan {
      skill = "export-ledger",
      goal = "Show the ledger and export it as a spreadsheet with its audit links",
      steps = [read ledger, export ledger format],
    },
}
