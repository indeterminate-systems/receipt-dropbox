# Contract kit: money released against signed agreements

**Status:** design, 2026-11-22 (piece 17). Not legal or tax advice: every point marked
**[counsel]** needs a Mexican lawyer, and every point marked **[contador]** an accountant, before
anything here is used with real money. The decision record is
[ADR-P17](../adr/ADR-P17-contract-kit.md).

## The problem: outflows have no incentive to be documented

A funder (an individual such as the organization's patron, or the organization itself) pays for
projects whose costs are spent by other people: supporters, coordinators, intermediaries who hold
the money for a while. Pieces 4 to 15 make documenting a cost cheap and checkable, but they only
work on what people send in. Today nothing makes them send it:

- **The cost of proof falls on the spender, the benefit on the funder.** Asking a vendor for a
  CFDI, keeping the receipt and uploading it is work; skipping it costs the spender nothing.
- **Once the money is sent, the funder has no leverage.** An advance is, in practice, a gift with
  a request attached.
- **Exposure grows silently.** Nobody sees the running total of unproven money per person.
- **Forgiving it later is not free.** An unproven advance that is let go is, for tax, likely
  income to the person who kept it, and an unsupported expense for the organization
  (compliance §5, *remanente distribuible*) **[contador]**.

Inflows have the opposite shape: whoever is owed money chases it. The design gives outflows the
same shape by making unproven money the holder's problem, making proof pay, and making the whole
thing automatic so nobody has to chase anyone.

## The model in one paragraph

Money leaves the funder only under an agreement the recipient signed beforehand. Below a small
threshold nothing is advanced: people pay and are reimbursed after proof (piece 15's claims).
Above it, money is advanced as a **debt** of the recipient, under a master loan agreement signed
once, with each **draw** backed by its own signed promissory note for a fixed sum. A draw is
settled by **proof** (receipts and CFDIs that `fiscal.check` calls `supported`), by
**repayment** of what wasn't spent, or by **offset** against money the organization owes that
person, and never by quietly forgiving it. Settling completely and on time earns a small
**settlement bonus**; overdue draws trigger a **holdback** on that person's payouts. How much
someone can draw grows with their record (**trust ladder**). Each of these rules is a **brick**:
a clause of prose with machine-readable parameters, signed and hashed; an agreement is a stack of
bricks, and a **kit** is a ready-made stack for a common case.

```
 funder                         recipient (supporter / intermediary)
   │  master agreement  ◄────────── signs once (bricks + parameters) ─────────►│
   │                                                                          │
   │  ◄──── order: "venue deposit, up to 5,000 MXN, by 2027-02-15" (both sign) │
   │  ──── draw D-0007: 5,000 MXN ───────────────────────────────────────────► │ signs pagaré for 5,000
   │                                                                          │ spends, asks for CFDI
   │  ◄──── proof: CFDI 4,800 (fiscal.check: supported) ───────────────────── │
   │  ◄──── repayment: 200 ────────────────────────────────────────────────── │
   │  D-0007 settled on day 5 → bonus 48 MXN accrues, paid with the month's payout │
```

The app never holds or moves money. It drafts and checks agreements, records signatures, tells
the funder what may be sent and to whom, matches proofs and repayments, computes bonuses,
holdbacks and offsets, and books it all in the ledger. Payments are made by people, from their
own bank, as today **[counsel: confirms the app is not a payment or lending service]**.

## Why each lever

| Lever | What it changes | Brick |
| :-- | :-- | :-- |
| Unproven money is the holder's debt | Not documenting now costs the spender: they owe it back | `advance`, `promissory-note` |
| Proof is the normal way to repay | Documenting is the cheapest way out of the debt | `settle-by-proof` |
| A bonus for complete, on-time proof | Documenting pays, and pays more when early | `settlement-bonus` |
| Holdback and offset | Nobody has to chase: overdue money is recovered from what the organization owes the person anyway | `holdback`, `offset` |
| Limits that grow with a clean record | New or unreliable holders can't build up large exposure; reliable ones get more freedom | `trust-ladder` |
| Small things are reimbursed, not advanced | Most spending never creates a debt at all | `reimburse-below` |
| Organization cards where possible | No advance exists: the card's statement is the organization's own (see [Cards](#where-a-card-is-better)) | `org-card` |

The levers pull the same way: the cheapest path for the recipient (prove it, early) is the one the
funder wants. The bonus is the only new cost to the funder, and it is bounded per draw, per person
and per month.

## Bricks

A brick is one clause. It has an id and version (`advance@1`), prose (Spanish, with an English
reading copy), a Nickel contract for its parameters, what it requires and conflicts with, which
parties it applies to, and the effects the engine computes from it. Its identity is the digest of
all of that, so a signed agreement pins the exact wording.

| Brick | Parameters (examples) | Requires | Notes |
| :-- | :-- | :-- | :-- |
| `parties` | funder, recipient, organization, roles (who approves orders) | – | Every agreement has it. Who the creditor is matters when the funder is a person paying for an organization's project **[counsel]** |
| `framework-loan` | ceiling, currency, term, interest (default none), purpose | `parties` | Master *contrato de mutuo* or *apertura de crédito* **[counsel: which fits, and whether interest-free between related parties has tax effects]** |
| `order` | how orders are made: who signs, what each must state (purpose, ceiling, deadline) | `framework-loan` | Lets amounts be chosen later without leaving them to one party (see [Abstract agreements](#abstract-agreements-amounts-chosen-later)) |
| `advance` | per-draw maximum, which payment methods, to which account | `framework-loan` | Each draw is a disbursement under the master |
| `promissory-note` | one note per draw, fixed sum, due date, place of payment | `advance` | A *pagaré* must state a fixed sum, so each draw gets its own **[counsel: LGTOC requirements, electronic versus handwritten]** |
| `settle-by-proof` | which proof policy (piece 15's `kind`), how lines are matched, partial settlement allowed | `advance` | Proof is judged by `fiscal.check`; only `supported` settles. `needs_review` waits for a person |
| `deadline` | days to settle after disbursement or after the spending date, grace days | `advance` | Overdue starts holdback and stops new draws |
| `repayment` | how unspent money is returned (transfer to which account), rounding | `advance` | Repayment is matched like any other statement line |
| `reimburse-below` | threshold per item and per month | `parties` | Below it: no advance, piece 15 claims only |
| `settlement-bonus` | rate, floor, cap per draw, cap per month, decay, clawback window, payout day | `settle-by-proof`, `income-route` | See [Bonus](#settlement-bonus) |
| `income-route` | how the bonus is paid lawfully: `payroll` (employee) or `service-fees` (contractor, monthly CFDI) | `parties` | A bonus is income to the recipient **[contador]**; volunteers can't get one without becoming one of these |
| `holdback` | percentage of each payout held while anything is overdue, maximum held, release rule | `deadline` | See [Holdback and offset](#holdback-and-offset) |
| `offset` | which amounts owed to the recipient can be applied to overdue draws, notice days | `deadline` | *Compensación* **[counsel: written clause for contractors; LFT limits for employees]** |
| `trust-ladder` | levels (open-draw limit, per-draw limit), what moves someone up or down | `advance` | See [Trust ladder](#trust-ladder) |
| `org-card` | card holder, monthly limit, merchant categories, proof deadline | `parties`, `settle-by-proof` | Spending on the organization's card; no debt, same proof rules |
| `amendment` | how parameters change: who signs, notice, what can't change retroactively | – | Every agreement has it |
| `governing-law` | jurisdiction, courts, language that prevails | – | Every agreement has it |
| `privacy` | notice and consent for the data the agreement uses | – | Links piece 16's notice (R11) |

Bricks live one file each, `nickel/rdx/contract/bricks/<id>.ncl` (prose beside it as
`<id>.es.md` and `<id>.en.md`), so adding one touches no shared list. The legal prose of every
brick is to be **drafted by a lawyer**; the repository ships placeholders that say so and the
engine refuses to seal an agreement while any brick is a placeholder, unless the agreement is
marked `draft`.

### Kits

A kit is a stack of bricks with defaults, also one file each (`nickel/rdx/contract/kits/<id>.ncl`):

| Kit | For | Bricks |
| :-- | :-- | :-- |
| `volunteer` | Volunteers spending small amounts | `parties`, `reimburse-below` (everything), `settle-by-proof`, `privacy`, `amendment`, `governing-law`. No advances, no bonus (a bonus would make them paid) |
| `coordinator` | A contractor who runs a project's spending | the above plus `framework-loan`, `order`, `advance`, `promissory-note`, `deadline`, `repayment`, `settlement-bonus` (`service-fees`), `holdback`, `offset`, `trust-ladder` |
| `employee` | An employee who spends for the organization | as `coordinator`, with `income-route = payroll` and `offset`/`holdback` limited by the wage rules |
| `big-purchase` | One large purchase (equipment, a venue) | `framework-loan` with a one-order ceiling, `advance`, `promissory-note`, `settle-by-proof`, `deadline`, `repayment`; no ladder |
| `card-holder` | Someone given an organization card | `parties`, `org-card`, `settle-by-proof`, `deadline`, `holdback`, `privacy`, `amendment`, `governing-law` |

### Composition checks

`contract.compose` merges a kit (or bricks) with parameters and refuses, before anyone signs:

- a brick whose `requires` is missing, or two that conflict;
- an amount that is not **determinable** (next section);
- a bonus without an `income-route`, or a bonus in a volunteer agreement;
- for `employee`: a holdback or offset that could exceed the legal deduction limit for the
  recipient's stated wage, and a bonus that isn't routed through payroll;
- parameters outside the brick's contract (a rate over its maximum, a deadline in the past);
- a placeholder brick in an agreement not marked `draft`;
- a currency the funder's and recipient's accounts can't both hold, without an exchange rule.

These are Nickel contract errors with source locations, so an agent can fix the input.

## Abstract agreements: amounts chosen later

Ash asked whether an agreement can leave the amount for later so it adapts. It can, within two
constraints from the civil code as the research reads it **[counsel]**: the object must be
**determinable** (CCF art. 1825), and its validity and performance can't be left to the will of
one party (art. 1797). So a master agreement may leave amounts open only in these shapes, which
the engine enforces:

| Shape | Example | How it becomes a number |
| :-- | :-- | :-- |
| Fixed | 5,000 MXN | Already one |
| Ceiling | "up to 20,000 MXN in total" | Each order picks a value under it |
| Formula over named inputs | "1% of the settled amount, at least 10, at most 200 MXN" | Computed from recorded facts |
| Table | trust-ladder levels | Looked up from the recipient's record |
| Later order signed by both | "the deposit for the venue, by 2027-02-15, up to the ceiling" | The order states the number and both sign it |

What it refuses: "an amount the funder sets", "a bonus at the funder's discretion", or any formula
input that only one party controls and the other can't see. Changes to parameters go through the
`amendment` brick: a new version signed by both, pointing at the one it replaces, never applied
to draws already disbursed.

## Settlement bonus

The bonus is what makes proof pay. One parameter set, for example:

```nickel
settlement_bonus = {
  rate_bp = 100,                  # 1% of what proof settles, in basis points
  floor = { minor = 1000 },       # at least 10.00 per draw
  cap_per_draw = { minor = 20000 },
  cap_per_month = { minor = 100000 },
  full_within_days = 7,           # full bonus if settled within 7 days of spending
  zero_at_days = 30,              # falls linearly to nothing at the deadline
  clawback_days = 90,             # a CFDI cancelled within 90 days takes its share back
  pay_on = "month-end",
}
```

A 5,000 MXN draw settled by a 4,800 MXN CFDI and 200 MXN repaid on day 5 earns 1% of 4,800 =
48 MXN. Settled on day 20 it earns 48 × (30 − 20) / (30 − 7) = 20.86 MXN (rounded down to the
centavo). Repayment earns nothing: the bonus pays for proof, not for returning money.

Rules: only a **complete** settlement (the whole draw proven or repaid) earns it; only proof that
`fiscal.check` calls `supported` counts; it accrues as a liability and is paid at month end
through the `income-route`; if the SAT later reports a CFDI cancelled (piece 15's SAT status
capability), that CFDI's share is clawed back from the next payout. To the funder it is overhead
of at most `rate` on documented spending, capped per person and month; 1% on 100,000 MXN of
documented spending a month is 1,000 MXN.

**[contador]** The bonus is income to the recipient: for an employee, salary (ISR withheld, IMSS);
for a contractor, a fee they invoice monthly (RESICO or their regime). Regular bonuses to someone
not on payroll are exactly the pattern compliance R15 flags, so the `coordinator` kit pairs them
with a service agreement.

## Holdback and offset

Most intermediaries are also owed money by the organization: reimbursements, fees, salary. Two
bricks use that, so recovering an overdue draw needs nobody to ask:

- **Holdback:** while any draw is overdue, a percentage of each payout to that person is held,
  up to the overdue amount, and released when nothing is overdue. Held money stays the
  recipient's, booked as a liability to them.
- **Offset:** after a notice period, held money (and, if the agreement allows, amounts owed to
  them) is applied to the overdue draw, settling it by *compensación*. Each offset is a ledger
  entry citing the draw and the clause, and the recipient gets a statement of it.

**[counsel]** For employees, wage deductions for debts to the employer are capped (LFT art. 110:
with consent, at most 30% of the excess over the minimum wage, to confirm); reimbursements of
expenses may not count as wages, and the engine should treat them separately. For contractors, a
written offset clause. The engine computes the limit per payout and never exceeds it.

## Trust ladder

| Level | Open draws at once | Per draw | Up after | Down when |
| :-- | :-- | :-- | :-- | :-- |
| 0 | none (reimbursement only) | – | signed agreement | – |
| 1 | 1 | 2,000 MXN | 3 settled on time | any overdue |
| 2 | 3 | 10,000 MXN | 10 settled on time, none overdue in 90 days | overdue over 7 days |
| 3 | 5 | 30,000 MXN | – | overdue over 7 days, or a clawback |

The figures are examples in the kit's defaults. The level is computed from the ledger, never
stored by hand, so anyone can recompute it.

## Agreements as signed, hashed documents

Each agreement is a Ricardian contract: the prose a court reads, the parameters the engine reads,
and the signatures, bound together by one digest. In this codebase that is a Gordian Envelope
(piece 7), with the same tooling as statements:

```
'isA': rdx.contract.agreement.v1
  'brick': { id: "advance@1", digest: sha256:…, params: {…} }   (one per brick)
  'prose': sha256:…   (the rendered PDF of the whole text, Spanish)
  'kit': "coordinator@1"
  'parties': [funder, recipient]
  'supersedes': sha256:…   (on an amendment)
  'signed': [funder device key, recipient device key]
```

- **Orders** and **draws** are their own envelopes, citing the agreement's digest; a draw also
  carries its promissory note's digest. **Settlements**, **offsets**, **bonuses** and
  **clawbacks** are events citing the draw, signed by whoever records them.
- They are stored and chained like custody records (piece 12): encrypted to the parties and the
  accountant, signed with device keys (piece 8), kept by servers that can't read them.
- The prose PDF is rendered by Typst (piece 10) from the bricks' text and parameters, so the
  document a person signs is generated from exactly what the engine enforces.
- **Legal weight:** device signatures prove who agreed to what, but are probably not a legal
  signature on their own **[counsel]**. Options, cheapest first: print and sign by hand
  (especially the *pagaré*), keeping the scan's digest; sign with the person's e.firma (FIEL);
  and timestamp with a NOM-151 *constancia de conservación* from an accredited provider. Each is
  recorded as an attestation on the envelope. Which one a court needs is a question for counsel,
  and the design supports all three.

## Ledger

Illustrative entries (account names to settle with piece 15's claims):

```beancount
2027-01-10 * "Draw D-0007: venue deposit" #advance
  agreement: "sha256:…"
  draw: "sha256:…"
  Assets:Advances:S-ana            5000.00 MXN
  Assets:Bank:Org                 -5000.00 MXN

2027-01-15 * "Settle D-0007: Salón Luna" #settlement
  draw: "sha256:…"
  receipt: "sha256:…"
  Expenses:Venue                   4800.00 MXN
  Assets:Advances:S-ana           -4800.00 MXN

2027-01-15 * "Repay D-0007: unspent" #settlement
  Assets:Bank:Org                   200.00 MXN
  Assets:Advances:S-ana            -200.00 MXN

2027-01-31 * "Settlement bonus, January" #bonus
  Expenses:Incentives:Settlement     48.00 MXN
  Liabilities:Payable:S-ana         -48.00 MXN
```

The balance of `Assets:Advances:<supporter>` is what that person owes, at any date; an overdue
draw is one whose balance isn't zero after its deadline. Writing off a draw is possible only as
an explicit, signed decision that the engine books as a payment to the recipient, flagged as
probably taxable income to them **[contador]**, never as a silent deletion.

## Where a card is better

For recurring small spending, an organization card (a business account with cards per person,
such as Wise Business or Airwallex, which also cover funders paying in another currency) removes
the advance altogether: the money is the organization's until spent, the statement is the
organization's own and needs no redaction, and the card can be limited per person and per
merchant category. Proof is still needed, so `org-card` reuses `settle-by-proof`, `deadline` and
`holdback`. Advances remain for what a card can't pay (cash-only vendors, transfers to people,
deposits). Which providers issue cards to an organization of this kind, in Mexico, is to check
when it matters; nothing in the kit depends on a provider.

## Built on what exists

| Piece | What the kit uses |
| :-- | :-- |
| 7, 8 | Envelopes, signing with device keys, encryption to parties |
| 9 | Ledger entries and balances; advances as receivables |
| 10 | The prose PDF of an agreement and of each promissory note |
| 12 | Custody-style signed event chains; the audit trail from ledger entry to agreement |
| 13 | Proof arriving by email |
| 15 | `fiscal.check` decides what proof settles; claims are the `reimburse-below` path; the SAT status capability drives clawbacks |
| 16 | Privacy notice and consent; retention of agreements and notes (likely longer than receipts) |

New, when built: crate `rdx-contract` (compose, seal, order, draw, settle, accrue, offset,
balance), ops `contract.*` in `nickel/rdx/ops/contract.ncl`, the brick and kit files above, and
skills `draft-agreement`, `request-advance`, `settle-advance` and `supporter-balance`.

## Phasing

1. **Design** (this document).
2. **Bricks and agreements:** `rdx-contract` with `contract.compose` and `contract.seal`, the
   brick and kit files with placeholder prose, the agreement envelope and its PDF. No money.
3. **Draws and settlement:** orders, draws, promissory notes, settlement by proof and repayment,
   ledger entries, `supporter-balance`. Needs piece 15's claims.
4. **Incentives:** bonus accrual and clawback, holdback, offset, the trust ladder.
5. **Legal signatures:** e.firma signing and NOM-151 timestamps as attestations, once counsel has
   said which are needed.

Nothing past phase 2 should run with real money before counsel has drafted the bricks' prose.

## Risks

- **Unenforceable or harmful clauses.** Mitigated by lawyer-drafted prose, the composition checks
  and `draft` marking; residual until counsel reviews.
- **Relationships.** Putting volunteers "on the hook" can drive them away. The `volunteer` kit has
  no debt at all; debt is for people who hold larger sums.
- **Disguised pay.** Regular bonuses and holdbacks can make a contractor look like an employee
  (R15). `income-route` and the per-kit rules keep pay explicit.
- **Gaming.** Splitting purchases under `reimburse-below`, or buying CFDIs to earn bonuses: the
  monthly threshold, 69-B and SAT status checks (piece 15) and clawback address these.
- **Privacy.** Agreements and balances are personal data: encrypted to the parties and the
  accountant, never readable by a server, retained per piece 16.

## Open questions

For a lawyer:

1. Which master agreement fits: *contrato de mutuo*, *apertura de crédito*, or *gastos a comprobar*
   under a service or employment contract? Does it change if the funder is a person paying for an
   organization's project, or an informal group with no legal personality?
2. Can each draw's *pagaré* be electronic (e.firma) and still be enforceable as a *título
   ejecutivo*, or must it be signed by hand?
3. Are parameterized clauses (ceilings, formulas, later orders signed by both, the trust-ladder
   table) "determinable" enough under CCF arts. 1797 and 1825?
4. What do offset and holdback clauses need: for contractors (written *compensación* clause,
   notice), and for employees (LFT art. 110 limits, consent form, whether expense reimbursements
   are wages)?
5. Which signature evidence does a court need for the master agreement: device signatures plus
   e.firma, a NOM-151 timestamp, or handwriting?
6. Does recording advances and computing offsets, without holding or moving money, keep the
   organization and the app outside lending or payments regulation?
7. The Spanish prose of every brick.

For a contador:

8. Is a settled advance (proved by CFDIs to the organization's RFC) a clean expense for the
   organization, and what proves the advance itself was not income to the recipient?
9. Is a written-off advance taxable income to the recipient, and what must the organization issue
   or withhold?
10. How should the settlement bonus be paid and invoiced: payroll for employees, monthly fee CFDI
    for contractors (RESICO?), and can a volunteer receive one at all?
11. Does an interest-free advance between related parties (funder and recipient) have tax
    effects?
12. Under `informal-group`, with no RFC, what does the funder need to deduct or justify what they
    fund, if anything?
