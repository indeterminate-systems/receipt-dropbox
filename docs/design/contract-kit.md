# Contract kit: requests, receipts and settlements both sides sign

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
- **Once the money is sent, the funder has no leverage.** Money handed over with a request
  attached is, in practice, a gift.
- **Exposure grows silently.** Nobody sees the running total of unproven money per person.
- **Letting it go later is not free.** Unproven money that is let go is likely income to the
  person who kept it, and an unsupported expense for the organization (compliance §5,
  *remanente distribuible*) **[contador]**.

Inflows have the opposite shape: whoever is owed money chases it. The design gives outflows the
same shape.

## The core: four steps

Nothing needs to be negotiated up front. Everything starts with a request, every movement is
acknowledged, everything accumulates, and from time to time the two parties settle.

```
  A (asks, receives, does the work)                 B (funds, reviews)
  │ 1. Request ── "5,000 MXN for the venue deposit" ──────────►│  signed by A
  │ ◄──────────────────────────────── transfer 5,000 MXN ──────│
  │ 2. Acknowledge ── "received 5,000 MXN, re request R" ─────►│  signed by A   (the only must)
  │ 3. Accumulate ── CFDI 4,800 · repayment 200 · more requests…   in the ledger, per pair
  │ 4. Settle ── A drafts the statement of account ───────────►│
  │ ◄────────────────────────────── one tap: sign ─────────────│  both signatures release
  │                                                              everything up to the cutoff
```

1. **Request.** A asks B for something: money, goods, a payment on A's behalf. A signed envelope:
   who asks whom, what, an amount or ceiling, a purpose, optionally a date. B may answer it, or
   not; the request alone binds no one. Either side can start: a funder can also ask a supporter
   to buy something.
2. **Acknowledge.** When money or goods move, **the receiver signs a one-line receipt**: "I
   received 5,000 MXN from B on 2027-01-10, under request R, to be accounted for by proof of
   spending or repaid." This is the only mandatory brick. It is what makes the time before
   settlement binding: from here the money is in A's account with B until settled. The amount is
   the one that actually moved, so a request's "up to" never has to be resolved. **[counsel:
   whether this wording, alone, creates an enforceable obligation to account for or repay the
   money (a *mutuo*, or a *mandato* with its duty to *rendir cuentas*), and what it needs to say]**
3. **Accumulate.** Everything between the pair builds up in the ledger: acknowledged receipts,
   proofs of spending (receipts and CFDIs, judged by `fiscal.check`), repayments, more requests.
   The running balance of what A still has to account for is a number anyone with access can
   read at any date.
4. **Settle.** When A wants to (or B asks), the system drafts a **statement of account** from the
   ledger up to a cutoff date: the balance carried in from the last settlement, every movement
   with its evidence, and the net balance. A completes it and signs; B reviews and signs. **Both
   signatures release both parties from everything up to the cutoff**, and the net balance gets
   one of three dispositions, chosen in the statement:
   - **carried forward** as the opening balance of the next period;
   - **repaid**, by a transfer that the next settlement shows;
   - **forgiven**, explicitly, which the engine books as a payment to A and flags as likely income
     to A **[contador]**. Never by silence.

   **[counsel]** The legal shape this borrows is a running account (*contrato de cuenta
   corriente*, LGTOC art. 302 ff.) closed by a settlement both sign (*transacción*, CCF art. 2944,
   or a *finiquito*). Whether a statement of account signed by both has that effect, and what it
   must state, is for a lawyer.

Disagreement doesn't block a settlement. B marks lines as disputed; disputed lines are carried
forward as open items, and the rest settles. A fixes them (finds the CFDI, repays) before the next
one.

## The lazy principle: the receiver does all the work

The person who received the money fills everything in. They upload the receipts and CFDIs (by
email or the PWA), draft the statement of account, explain any gap, choose the disposition and
sign first. The funder's whole job is to read a short statement and tap to sign, or to mark a
line disputed.

The work sits with the receiver because the benefits do too:

- **Their own release.** Until a statement is signed, every acknowledged receipt stands as money
  they hold for the funder. Settling is how they stop owing it.
- **Their next request.** The funder sees, next to any new request, the requester's unsettled
  balance and how old it is. A clean, recently settled account gets money faster; with a brick
  (`gate`, below) it can be the rule rather than the funder's judgement.
- **Their bonus**, if the pair has added one (`settlement-bonus`, below).

And the system makes the work small: it drafts the statement from the ledger, says which lines
lack proof and exactly which CFDI to ask the vendor for (piece 15), and refuses to send a
statement with a gap the receiver hasn't explained.

**Deemed acceptance** is an optional brick: if B neither signs nor disputes within N days of
receiving a complete statement, it counts as accepted. It protects the receiver from a funder who
never reviews. **[counsel: whether a silence clause can have this effect, and what notice it
needs]**

## Bricks: added when real friction shows up

The four steps work with no other clause. Bricks are extra clauses a pair adds when they find
they need one, never up front by default. Each is one file (`nickel/rdx/contract/bricks/<id>.ncl`,
prose beside it in Spanish and English), versioned and hashed, with a Nickel contract for its
parameters and what it requires or conflicts with. Adding one is an **amendment**: a terms
envelope both sign, pointing at the one it replaces, applying from then on, never to movements
already acknowledged.

| Brick | Add it when | What it does |
| :-- | :-- | :-- |
| `gate` | Someone keeps asking for more while old money is unsettled | New requests are refused while the unsettled balance is older than N days or above a limit |
| `deadline` | Settlements drift | Each acknowledged amount must be settled within N days; overdue triggers `gate`, `holdback` |
| `deemed-acceptance` | The funder doesn't review | Silence for N days after a complete statement counts as signing **[counsel]** |
| `reimburse-below` | Small items create too many receipts | Below a threshold, no money moves first: A pays, proves, and is reimbursed (piece 15's claims) |
| `settlement-bonus` | Proof still arrives late | A small payment for complete, timely, CFDI-backed settlements (below) |
| `income-route` | A bonus is added | How it is paid lawfully: payroll for employees, monthly fee CFDI for contractors **[contador]** |
| `holdback` | Overdue money and the organization also pays A | A share of each payout to A is held while anything is overdue (below) |
| `offset` | Overdue money is never repaid | Held money, or amounts owed to A, settle the overdue balance (*compensación*) **[counsel]** |
| `trust-ladder` | Exposure to new people is too high | Limits on open requests rise with clean settlements, computed from the ledger |
| `promissory-note` | A large sum needs a stronger instrument | A *pagaré* for a fixed sum signed with the acknowledgement **[counsel: LGTOC, electronic versus handwritten]** |
| `framework-loan` | A long relationship with large sums | A master *mutuo* or *apertura de crédito* with a ceiling and term **[counsel]** |
| `org-card` | Spending is recurring and card-payable | Spending on an organization card instead of moving money first (see [Cards](#where-a-card-is-better)) |
| `governing-law`, `privacy` | The pair formalizes | Jurisdiction and language; piece 16's notice and consent |

**Kits** are named stacks for common cases, also one file each, used as starting points: a
`volunteer` kit is the core plus `reimburse-below` and nothing else (no bonus: it would make them
paid); a `coordinator` kit adds `deadline`, `gate`, `settlement-bonus` with `service-fees`,
`holdback` and `offset`; `employee` is the same with `income-route = payroll`.

**Checks before signing.** `contract.compose` refuses terms with a missing requirement or a
conflict; an amount that isn't **determinable** (fixed, a ceiling, a formula over recorded facts,
a table, or fixed later by something both sign; never "what B decides", CCF arts. 1797 and 1825
**[counsel]**); a bonus without an `income-route` or for a volunteer; a holdback or offset beyond
an employee's legal deduction limit; and lawyer-placeholder prose outside a `draft`. In the core
the question rarely comes up: the numbers are the ones each acknowledgement and each settlement
states, and both are signed.

### Settlement bonus

```nickel
settlement_bonus = {
  rate_bp = 100,                  # 1% of what proof settles, in basis points
  floor = { minor = 1000 },       # at least 10.00 per settlement
  cap_per_settlement = { minor = 20000 },
  cap_per_month = { minor = 100000 },
  full_within_days = 7,           # full if proven within 7 days of the acknowledgement
  zero_at_days = 30,              # then falls linearly to nothing
  clawback_days = 90,             # a CFDI cancelled within 90 days takes its share back
  pay_on = "month-end",
}
```

Receiving 5,000 MXN and settling it with a 4,800 MXN CFDI and 200 MXN repaid on day 5 earns 1% of
4,800 = 48 MXN; on day 20, 48 × (30 − 20) / (30 − 7) = 20.86 MXN, rounded down to the centavo.
Repayment earns nothing: the bonus pays for proof. Only proof `fiscal.check` calls `supported`
counts, and a CFDI the SAT later reports cancelled is clawed back from the next payout. To the
funder it is bounded overhead on documented spending: 1% of 100,000 MXN a month is 1,000 MXN.
**[contador]** It is income to the receiver; regular bonuses to someone not on payroll are the
pattern compliance R15 flags, hence `income-route`.

### Holdback and offset

- **Holdback:** while anything is overdue, a share of each payout to A (reimbursements, fees,
  salary) is held, up to the overdue amount, and released when nothing is overdue. It stays A's,
  booked as a liability to them.
- **Offset:** after notice, held money (and, if the terms allow, amounts owed to A) settles the
  overdue balance; the next statement of account shows it.

**[counsel]** For employees, deductions for debts to the employer are capped (LFT art. 110: with
consent, at most 30% of the excess over the minimum wage, to confirm), and expense reimbursements
may not count as wages. For contractors, a written clause. The engine computes the limit per
payout and never exceeds it.

## Envelopes

Every step is a signed Gordian Envelope (piece 7), signed with device keys (piece 8), encrypted to
the two parties and the accountant, and stored and chained like piece 12's custody records, so a
server keeps them without reading them:

| Envelope | Signed by | Cites |
| :-- | :-- | :-- |
| `rdx.contract.request.v1` | requester | the terms in force, if any |
| `rdx.contract.receipt.v1` | receiver | the request; the bank line or goods it covers |
| `rdx.contract.statement.v1` | receiver, then funder | the previous statement, every movement's digest, disputed lines, the net balance and its disposition |
| `rdx.contract.terms.v1` | both | the bricks with parameters and prose digests; `supersedes` the previous terms |

A statement's digest is the settlement's identity; the next statement starts from it, so the
history is a chain anyone with access can verify. The statement's PDF is rendered by Typst
(piece 10) from the same data, so what people sign is what the engine computed. **Legal weight:**
device signatures prove who signed what but may not be a legal signature on their own
**[counsel]**; a handwritten signature on the printed statement (keeping the scan's digest), the
person's e.firma, or a NOM-151 timestamp can be added as attestations.

## Ledger

Illustrative entries (account names to settle with piece 15's claims):

```beancount
2027-01-10 * "Received under request R-12: venue deposit" #receipt
  request: "sha256:…"
  acknowledged: "sha256:…"
  Assets:Accounts:S-ana            5000.00 MXN
  Assets:Bank:Org                 -5000.00 MXN

2027-01-15 * "Salón Luna" #proof
  receipt: "sha256:…"
  Expenses:Venue                   4800.00 MXN
  Assets:Accounts:S-ana           -4800.00 MXN

2027-01-15 * "Repaid unspent" #repayment
  Assets:Bank:Org                   200.00 MXN
  Assets:Accounts:S-ana            -200.00 MXN

2027-01-31 * "Settlement S-3, both signed" #settlement
  statement: "sha256:…"
  disposition: "carried-forward"

2027-02-01 balance Assets:Accounts:S-ana   0.00 MXN
```

`Assets:Accounts:<person>` is what that person holds for the organization; a settlement adds a
balance assertion at its cutoff, so the ledger itself checks that the signed statement and the
books agree. A forgiven balance is a payment entry, never a deleted one.

## Where a card is better

For recurring small spending, an organization card (a business account with cards per person,
such as Wise Business or Airwallex, which also suit funders paying in another currency) avoids
moving money first: the money is the organization's until spent and the statement is the
organization's own. Proof is still needed, so `org-card` keeps the same settlement. Money moved
first remains for what a card can't pay: cash-only vendors, people, deposits.

## Built on what exists

| Piece | What the kit uses |
| :-- | :-- |
| 7, 8 | Envelopes, signing with device keys, encryption to parties |
| 9 | Per-pair accounts, balance assertions at settlements |
| 10 | The statement of account and terms as PDFs |
| 12 | Signed event chains; the trail from ledger entry to statement |
| 13 | Proofs arriving by email |
| 15 | `fiscal.check` decides what proof counts; claims are `reimburse-below`; the SAT status capability drives clawbacks |
| 16 | Privacy notice, consent, retention of statements of account |

New, when built: crate `rdx-contract`, ops `contract.request`, `contract.acknowledge`,
`contract.balance`, `contract.draft-statement`, `contract.sign-statement` and later
`contract.compose`, in `nickel/rdx/ops/contract.ncl`; skills `request-funds`,
`acknowledge-receipt`, `settle-account` and `account-balance`.

## Phasing

1. **Design** (this document).
2. **The core:** request, acknowledge, balance, statement of account and its signatures, ledger
   entries, the statement PDF. Needs piece 15's claims for proofs. Usable with a placeholder
   wording only after counsel has looked at the acknowledgement and the statement.
3. **Bricks as friction shows up:** likely `gate` and `deadline` first, then `settlement-bonus`
   with `income-route`, `holdback` and `offset`, each with counsel's prose.
4. **Legal signatures:** e.firma and NOM-151 attestations, once counsel says which are needed.

## Risks

- **Weak legal footing for the core.** Everything rests on the acknowledgement and the signed
  statement meaning what this document says; counsel reviews both before real use.
- **A funder who never signs.** Leaves the receiver unreleased; `deemed-acceptance` addresses it.
- **Relationships.** "Putting people on the hook" can drive volunteers away; for them the
  `volunteer` kit moves no money first, so nothing is ever owed.
- **Disguised pay.** Regular bonuses and holdbacks can make a contractor look like an employee
  (R15); `income-route` keeps pay explicit.
- **Gaming.** Splitting purchases under `reimburse-below`, or buying CFDIs for a bonus: monthly
  thresholds, 69-B and SAT status checks (piece 15) and clawback.
- **Privacy.** Statements of account are personal data: encrypted to the parties and the
  accountant, retained per piece 16.

## Open questions

For a lawyer:

1. Does a signed one-line acknowledgement ("received X from B under request R, to be accounted for
   by proof of spending or repaid") create an enforceable obligation to account for or repay, and
   what must it state? Which figure is it (*mutuo*, *mandato* with *rendición de cuentas*,
   *depósito*), and does it change when the funder is a person paying for an organization's
   project, or for an informal group?
2. Does a statement of account signed by both, closing a running account (LGTOC art. 302 ff.),
   work as a *transacción* (CCF art. 2944) or *finiquito* releasing both up to its cutoff? What
   must it say, including about disputed items carried forward?
3. Can silence after N days count as accepting a statement (`deemed-acceptance`), and with what
   notice?
4. Are amounts fixed by later signed documents, formulas, ceilings and tables "determinable"
   under CCF arts. 1797 and 1825?
5. Offset and holdback: what a contractor clause needs, and for employees the LFT art. 110 limits,
   consent, and whether expense reimbursements are wages.
6. Which signature evidence a court needs: device signatures, e.firma, NOM-151, handwriting; and
   can a *pagaré* be electronic and still be a *título ejecutivo*?
7. Does recording requests, receipts and settlements, without holding or moving money, keep the
   organization and the app outside lending or payments regulation?
8. The Spanish wording of the acknowledgement, the statement of account and every brick.

For a contador:

9. Is spending proven in a settled statement (CFDIs to the organization's RFC) a clean expense
   for the organization, and what shows the money received was not income to the receiver?
10. Is a forgiven balance taxable income to the receiver, and what must the organization issue or
    withhold?
11. How should a settlement bonus be paid and invoiced: payroll, monthly fee CFDI (RESICO?), and
    can a volunteer receive one at all?
12. Under `informal-group`, with no RFC, what does the funder need to justify what they fund?
