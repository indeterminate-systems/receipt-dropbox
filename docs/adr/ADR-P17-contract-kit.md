# ADR-P17: Contract kit: money released against signed, composable agreements

**Status:** Proposed (design only; no code yet)
**Date:** 2026-11-22
**Deciders:** Ash

## Context

A funder pays for projects whose costs other people spend from money they hold for a while. The
pieces so far make proof cheap to send and check, but nothing makes anyone send it: the effort
falls on the spender, the benefit on the funder, and once money is sent the funder has no
leverage. Unproven advances that are let go are likely taxable to the person who kept them and
unsupported expenses for the organization. Ash asked for a model in which money is released only
against agreements signed in advance, with incentives built in (a bonus for settling, holdback
from payouts), agreements abstract enough to adapt, and assembled from parts ("Contract Lego").
The full design, with the incentive analysis, bricks, kits, examples and open questions, is
[docs/design/contract-kit.md](../design/contract-kit.md).

## Decision

1. **Above a threshold, money is advanced as a debt; below it, reimbursed after proof.** A master
   loan agreement is signed once; each draw is a disbursement under it with its own promissory
   note for a fixed sum. A draw is settled by proof that `fiscal.check` (piece 15) calls
   `supported`, by repayment, or by offset, and never forgiven silently: a write-off is an
   explicit signed decision booked as a payment to the recipient.
2. **Incentives are clauses, not policy outside the agreement.** A settlement bonus (rate with
   floor and caps, decaying with lateness, only for complete supported settlements, clawed back if
   a CFDI is cancelled, paid monthly through a lawful income route), holdback and offset against
   payouts, and a trust ladder of limits computed from the ledger.
3. **Agreements are composed from bricks.** Each brick is one clause: prose, a Nickel contract for
   its parameters, requires and conflicts, and its effects. Kits are ready stacks of bricks. One
   file per brick and per kit, so nothing central is edited to add one.
4. **Composition is checked before signing.** `contract.compose` refuses missing requirements,
   conflicts, amounts that aren't determinable (only fixed, ceiling, formula over recorded facts,
   table, or a later order signed by both), bonuses without an income route or for volunteers,
   and holdbacks or offsets beyond an employee's legal limit.
5. **Agreements are Ricardian Gordian Envelopes.** Prose digest, bricks with parameters, parties
   and signatures in one signed envelope whose digest is the agreement's identity; orders, draws,
   settlements and amendments are envelopes citing it. Stored and chained like piece 12's custody
   records, encrypted to the parties and the accountant. Legal signatures (handwriting, e.firma,
   NOM-151 timestamps) are attestations added to the envelope.
6. **The app never holds or moves money.** It says what may be sent, records and checks what was,
   and books it.
7. **Legal text is a lawyer's.** Bricks ship with placeholder prose, and an agreement with a
   placeholder can only be sealed as a `draft`.

## Options considered

| Option | For | Against |
| :-- | :-- | :-- |
| Reimbursement only (piece 15's claims) | Simplest; no debt; already planned | Doesn't cover people who must spend before being paid back, which is where proof goes missing |
| Advances under a written spending policy, no agreement per person | Little paperwork | No legal claim on unproven money, so no leverage; forgiving it is still taxable to someone |
| Organization cards for everyone | No advances at all; the statement is the organization's | Not everything can be paid by card (cash vendors, deposits, people); still needs proof rules. Kept as the `org-card` brick and preferred where it fits |
| One fixed contract template | Easy to review once | Every new case means a new template; parameters can't adapt; no machine checks |
| Smart contracts on a blockchain | Automatic enforcement | Would hold or move money, has no standing as a signed agreement under Mexican law without the same legal work, and publishes amounts and parties |
| **Composable signed agreements (chosen)** | Rules match the situation; the engine enforces what people signed; same envelopes, keys and ledger as everything else | More to build; enforceability depends on counsel's drafting |

## Consequences

- Unproven money is visible per person at any date, as the balance of their advances account.
- Proof becomes the cheapest way out of a debt, and the earliest proof the most rewarded; the
  funder's cost is a bounded bonus on documented spending.
- The design depends on counsel and a contador for the clauses' prose and the tax treatment of
  bonuses, write-offs and offsets; until then only phase 2 (agreements without money) can be
  built.
- Piece 15's claims must land first: they are the `reimburse-below` path, and the SAT status
  capability drives clawbacks.
- Agreements and balances are personal data, so piece 16's notice, consent and retention rules
  apply to them.
