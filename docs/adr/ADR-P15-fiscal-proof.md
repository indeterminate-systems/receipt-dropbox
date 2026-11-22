# ADR-P15: Fiscal proof

**Status:** accepted, 2026-11-22 (piece 15, first part). Reimbursement claims, the second part,
get their own ADR.

## Context

Piece 15 makes a reimbursement hold up for tax. [Compliance](../compliance.md) R0 makes what
counts as proof a per-organization policy, and R1 to R6 and R17 say what to check. Most figures
behind them (the cash threshold, which UsoCFDI to ask for, foreign invoices' minimum details) are
open questions for a contador, and the organization's own kind (open question 2) isn't known yet.
Piece 5 already reads a CFDI into a receipt, but not the receptor's postcode, regime or UsoCFDI,
the concepts' product codes or the Pagos complement.

## Decision

1. **A new crate, `rdx-fiscal`, with one op, `fiscal.check`.** It takes the policy and a list of
   expenses (a receipt plus the CFDI XML, Pagos complements, a foreign invoice, a SAT status
   result, how it was paid, purpose and class) and returns one assessment per receipt: `proof`
   (`supported`, `needs_review`, `payment_evidence_only`, `not_supported`), its `basis`, findings
   with stable codes and fixes, and the R6 ledger metadata. It is pure, so a dry run shows the
   real result. Skill `verify-cfdi` wraps it.
2. **The policy is Nickel, per fiscal year** (`nickel/rdx/fiscal.ncl`). `kind` picks the rules
   (R0); `by_year` holds each year's figures, with per-field defaults so one can change alone.
   Every year has `confirmed = false` until a contador confirms it, and every report says so.
   Rust has no defaults.
3. **`rdx-fiscal` parses the CFDI itself**, with `roxmltree` (pure Rust, already in the lock
   through the renderer), rather than growing `rdx-receipt`'s attribute scanner. Extraction stays
   piece 5's job; checking needs the whole document, including complements.
4. **The verdict is what the policy asks for, never "deductible".** Where the law is unsettled the
   finding is `review` and says who should look; an `error` is kept for what the research is
   clear on: a CFDI to someone else's RFC, an unstamped or cancelled CFDI, a definitive art. 69-B
   issuer, cash over the threshold, fuel in cash.
5. **Without an RFC, CFDI problems are reviews, not errors.** Under `informal-group` (and an
   `individual` with `sat_registered = false`) the receipt is the proof and a CFDI is recorded as
   its holder's.
6. **The SAT status is an input for now.** R3 needs the SAT's SOAP service and the 69-B open-data
   files, which a browser can't call: that is a new host capability (`rdx-host`, `wit/rdx.wit`),
   left for a follow-up so this piece doesn't touch shared files. Until then an unchecked CFDI is
   `needs_review` (`sat-unchecked`), so nothing reads as fully supported without a check.

## Not done here

- The SAT status and 69-B capability (decision 6), and re-checks at month close.
- Verifying the CFDI's seal against the issuer's certificate.
- Writing the R6 metadata into the ledger, and claims (R7), viáticos export (R8), the US profile
  (R9), donations (R10) and the donataria administration cap: the claims part of piece 15.
- `supporter-kind` in the metadata: the domain model has no supporter kind yet.

## Consequences

- An agent can tell a supporter, at upload, exactly which RFC, name, postcode, regime and UsoCFDI
  to ask for, which is the cheapest moment to get a CFDI right.
- When a contador answers the open questions, the answers land as a `by_year` change with
  `confirmed = true`, not as code.
