# ADR-P09: How the Beancount ledger is stored, posted and exported

**Status:** Proposed
**Date:** 2026-11-21
**Deciders:** Ash

## Context

[ADR-0003](0003-beancount-ledger.md) chose Beancount, with `receipt:` and `statement:`
metadata on every entry. Piece 9 has to decide where the ledger lives (the core does no I/O),
what a "balance assertion per statement" asserts when the supporter's personal account is not on
the organization's books, how entries reach the file without a person editing it, and how to
export a spreadsheet from WebAssembly.

## Decisions

1. **One entry per confirmed match**, `Expenses:<…>` against
   `Liabilities:Reimbursable:<SUPPORTER>`, built by `rdx_model::LedgerEntry::reimbursable` (a
   refund reverses the expense). Metadata: `supporter`, `line` (`<statement id>/<line id>`),
   `receipt`, `receipt-source` (digest of the receipt's document) and `statement` (digest of the
   encrypted original); the link `^<statement id>` groups a statement's entries in Fava. A
   statement without its encrypted original's digest is not booked.
2. **Expense accounts come from a Nickel policy** (`nickel/rdx/ledger.ncl`): `by_receipt`, then
   `by_merchant`, then `fallback` (`Expenses:Uncategorized`), whose use is counted and noted.
3. **Book, then post.** `ledger.book` has no effects, so a dry run shows the exact Beancount text;
   `ledger.post` has effects and adds it. Posting is idempotent per (line, receipt); booking a
   line differently is refused (corrections are new entries).
4. **Storage: versions in the blob store, a name in kv.** Each post stores the whole new file in
   the blob store (content-addressed) and points kv `ledger/<name>` at its digest. Every past
   version stays readable by digest, and each `ledger.post` run report (in the audit log) names
   `previous` and `digest`, so the history is a chain an auditor can walk. No new host capability.
5. **The file is derived and canonical.** `Ledger::render` writes the file from its entries and
   statements: `open` directives dated at first use, then balance assertions and transactions by
   date. So posting statements out of order gives the same file. `Ledger::parse` reads only that
   form and refuses any difference (with the first differing line), so a hand edit is caught
   instead of rewritten away.
6. **One balance assertion per statement** on the supporter's reimbursable account, dated the day
   after the statement period: what the organization owes that supporter by then. It carries the
   statement's id and original digest. Because it is recomputed on every post, `bean-check`
   catches an edit to entries made outside rdx, and an accountant can tie it to a reimbursement
   claim.
7. **Spreadsheets are written by hand**: CSV (RFC 4180, formula-looking payees prefixed with `'`)
   and a minimal XLSX (one sheet, inline strings, numeric amounts) in an uncompressed ZIP with a
   fixed timestamp. Same ledger, same bytes, same digest.

## Options considered

| Question | Chosen | Alternatives |
| :-- | :-- | :-- |
| Where the ledger lives | blob versions + kv name | a kv value holding the text (no history); a new `ledger` capability (WIT change for every host) |
| Updating the file | re-render from entries | append text (open dates and assertions go stale when statements arrive out of order); a full Beancount round-trip (would have to preserve arbitrary hand edits) |
| Balance assertion | supporter's reimbursable balance per statement | assert the personal bank account (puts personal accounts on the org's books, and needs the personal lines); a sub-account per statement (account sprawl for accountants) |
| Reader | own reader for the canonical form | `beancount-parser` as the reader (accepts files rdx can't rewrite faithfully); it is used in tests instead, as an independent check |
| XLSX | hand-written stored ZIP | `rust_xlsxwriter` (pulls a compression stack, and writes the time of creation, so output isn't reproducible) |

## Consequences

- Easier: the dry run shows the exact text to be booked; every version is addressable; exports are
  reproducible and the same on every host.
- Harder: nobody edits the ledger by hand. Corrections and reimbursement payments (piece 15) need
  their own entry kinds in `rdx-ledger`, which extend `render` and `parse` together.
- The whole file is re-read and re-written per post, which is fine for one organization's
  reimbursements; revisit (yearly files, `include`) if a ledger grows past a few MB.
- Spreadsheet dates are text (`YYYY-MM-DD`); a styles part would make them real dates.
