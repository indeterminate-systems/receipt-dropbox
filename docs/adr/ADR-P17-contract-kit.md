# ADR-P17: Contract kit: requests, receipts and settlements both sides sign

**Status:** Proposed (design only; no code yet)
**Date:** 2026-11-22
**Deciders:** Ash

## Context

A funder pays for projects whose costs other people spend from money they hold for a while. The
pieces so far make proof cheap to send and check, but nothing makes anyone send it: the effort
falls on the spender, the benefit on the funder, and once money is sent the funder has no
leverage. Unproven money that is let go is likely taxable to whoever kept it and an unsupported
expense for the organization. Ash asked for money to move only against something signed, with
incentives built in, terms abstract enough to adapt and assembled from parts ("Contract Lego");
then for the starting point to be just a request, with terms agreed at settlement releasing both
parties from everything until then, bricks added only as they prove necessary, and the person who
received the money doing all the filling-in. The full design is
[docs/design/contract-kit.md](../design/contract-kit.md).

## Decision

1. **Four steps are the core: request, acknowledge, accumulate, settle.** A request is a signed
   ask that binds no one. When money or goods move, the receiver signs a one-line
   acknowledgement; this is the only mandatory clause, and it is what makes the time before
   settlement binding. Everything between the pair accumulates in the ledger. A statement of
   account, signed by both, releases both from everything up to its cutoff; its net balance is
   carried forward, repaid, or explicitly forgiven (booked as a payment, flagged as likely
   income). Disputed lines carry forward so the rest can settle.
2. **The receiver does all the work.** The system drafts the statement from the ledger; the
   receiver completes it, explains gaps and signs first; the funder reviews and signs with one
   tap or marks lines disputed. The incentives sit with the receiver: their own release, their
   next request, and any bonus depend on a complete settled statement.
3. **Bricks are added when friction shows up, not up front.** Gate, deadline, deemed acceptance,
   reimbursement below a threshold, settlement bonus with an income route, holdback, offset, trust
   ladder, promissory note, framework loan, organization card, governing law, privacy: one file
   each, added by an amendment both sign that applies from then on. Kits are named starting
   stacks. `contract.compose` checks requirements, conflicts, determinable amounts and employee
   deduction limits before anyone signs.
4. **Every step is a signed Gordian Envelope**, encrypted to the parties and the accountant and
   chained like piece 12's custody records; each statement cites the previous one. Statements
   and terms render to PDF with Typst. Handwriting, e.firma and NOM-151 timestamps can be added as
   attestations.
5. **The app never holds or moves money.**
6. **Legal text is a lawyer's.** The acknowledgement, the statement of account and every brick
   ship with placeholder wording, usable only as `draft` until counsel has drafted them.

## Options considered

| Option | For | Against |
| :-- | :-- | :-- |
| Reimbursement only (piece 15's claims) | Simplest; nothing is ever owed | Doesn't cover people who must receive money before spending, which is where proof goes missing. Kept as the `reimburse-below` brick |
| Master loan agreement up front, a promissory note per draw, incentive clauses from day one (this ADR's first draft) | Strongest legal position from the start | Heavy for small, informal relationships; asks people to agree to rules before anyone knows which are needed. Its clauses survive as bricks |
| Organization cards for everyone | No money moves first; the statement is the organization's | Not everything can be paid by card; still needs settlement. Kept as the `org-card` brick |
| Smart contracts on a blockchain | Automatic enforcement | Would hold or move money, has no standing as a signed agreement without the same legal work, and publishes amounts and parties |
| **Request, acknowledge, accumulate, settle (chosen)** | Starts with almost no paperwork; the numbers are always ones both signed; the work and the incentives sit with the same person; clauses grow with need | Rests on the acknowledgement and the signed statement having the legal effect intended, which counsel must confirm |

## Consequences

- Unproven money is visible per person at any date, as the balance of their account, and each
  signed statement is checked against the books by a balance assertion.
- The funder's effort is a review and a tap; the receiver's effort is what clears their debt.
- Amounts are rarely abstract in the core: each acknowledgement and each settlement states its
  own number and is signed. Determinability matters only for bricks with formulas or ceilings.
- Until counsel has reviewed the acknowledgement and the statement wording, only draft use is
  possible. The bonus, holdback and offset bricks also need a contador.
- Piece 15's claims and `fiscal.check` decide what proof counts, so piece 15 lands first.
- Statements of account are personal data, so piece 16's notice, consent and retention apply.
