# Compliance: tax proof for reimbursements, and supporter data

**Status:** research notes, 2026-11-19. General information, not legal or tax advice. Every rule
below must be confirmed by a contador (Mexico) or CPA (US) against the law in force for the fiscal
year, and the open questions at the end need their answer before piece 15 ships.

Receipt Dropbox exists so that an organization can reimburse supporters who paid its expenses from
personal accounts. That only holds up for tax and audit if the paperwork is the kind the tax
authority accepts. This document records what that paperwork is (Mexico first, the US second),
what it means for the product, and the other legal points raised alongside it.

## The short answer

- **It depends on the organization.** A group with no RFC can't receive CFDIs, so what counts as
  proof is a per-organization **proof policy** (five kinds, [below](#it-depends-on-what-kind-of-organization-it-is)).
- **Mexico, organization with an RFC:** the proof of an expense is a **CFDI issued to the
  organization's RFC**, not a receipt and not a bank statement line. The supporter's statement line is evidence that the payment
  happened and how; the CFDI is what makes it a valid expense. Payments over $2,000 MXN must be made
  by bank transfer, nominative cheque, card or e-wallet, never cash (fuel: never cash at any
  amount). The reimbursement to the supporter should go by transfer from the organization's own
  account. Keep it all for at least five years.
- **For a donataria autorizada** the stakes are higher: since the 2021 reform an expense without a
  CFDI is treated as *remanente distribuible*, on which the organization itself pays ISR.
- **US:** reimbursements are tax-free to the payee only under an **accountable plan**: business
  connection, substantiation within a reasonable time (amount, date, place, business purpose, with
  receipts for lodging and anything $75 or more) and return of any excess.

## It depends on what kind of organization it is

Everything about CFDIs below assumes the organization has its own RFC, because a CFDI can only be
issued to a taxpayer that exists. Not every group that reimburses supporters does. So the
organization's **proof policy** is a setting, chosen at setup and changeable per fiscal year, and
it decides what counts as proof of an expense. Matching, redaction, the ledger and the audit trail
are the same under every policy; only what a matched expense needs before it is "supported"
changes.

| Proof policy | Who it is for | Proof of an expense | What else applies |
| :-- | :-- | :-- | :-- |
| `company` | A for-profit company with its own RFC (S.A., S. de R.L., S.A.S.) | A CFDI issued to the company's RFC with the right *UsoCFDI* (sections 1 to 7). A receipt alone is not deductible | LISR art. 27 fr. III payment rules (over $2,000 MXN by card or transfer, never cash); electronic accounting, monthly and annual returns |
| `nonprofit` | A non-profit with legal standing (A.C., I.A.P.) that is not a donataria autorizada | As `company`: a CFDI to the organization's RFC | Electronic accounting and the annual informative return for non-profits; an unsupported expense, or one outside the organization's purpose, is *remanente distribuible* and taxed (section 5) |
| `donataria-autorizada` | A non-profit authorized by the SAT to issue deductible donation receipts | As `nonprofit` | Everything in `nonprofit`, plus CFDI de donativos for donations received, the administration cap (about 5% of donations, to confirm) with administration and mission spending kept apart in the ledger, and the annual SAT transparency report (section 5) |
| `individual` | A persona física acting as the organization: freelancer, sole trader, RESICO | A CFDI issued to that person's RFC | The person's own regime decides what is deductible (RESICO, for example, deducts little or nothing; to confirm). "Personal" versus "organization" spending is the owner's choice, not a legal boundary, so redaction protects privacy rather than separating two taxpayers. Supporters other than the owner are reimbursed against CFDIs to the owner's RFC |
| `informal-group` | A collective, club or volunteer team with no RFC of its own | The receipt or bill itself; or a CFDI issued to the supporter's own RFC or to *público en general* (RFC XAXX010101000) | No SAT deductibility, no CFDI de donativos and no SAT filings by the group. The ledger is for internal accountability to members and funders, so the redacted, verifiable statements carry more of the weight |

Every figure in this table (the $2,000 threshold, the 5% cap, regime rules) is to be confirmed by
a contador for the fiscal year, and lives in Nickel policy rather than code.

Notes on `individual`, including an owner with employees:

- A persona física who runs business activity and has employees is still `individual`: the
  "business" is legally that person (*persona física con actividad empresarial*), and no separate
  entity is needed. The app keeps the business as its own set of books (its own ledger and
  accounts) under the owner's RFC, apart from the owner's personal spending.
- **Employees are supporters,** like volunteers: they pay, upload and are reimbursed the same way.
  A CFDI for an employee's expense must name the owner's RFC, not the employee's, or the owner
  can't deduct it. Reimbursements must match real expenses one for one; regular or round amounts
  without CFDIs look like hidden salary (R15).
- Having employees brings employer obligations (IMSS registration, CFDI de nómina, ISR withholding,
  INFONAVIT, the state payroll tax). **Payroll is out of scope:** the app only exports viáticos data
  for whoever runs payroll (R8).
- The policy carries a **`sat-registered`** flag. When the owner is not registered with the SAT
  for the activity, the app behaves like `informal-group` day to day (the receipt is the proof,
  nothing is filed) and shows a standing warning that CFDIs to the owner's RFC aren't possible
  yet and that unregistered business activity with employees is a compliance risk for the owner.
  Turning the flag on switches the owner to full `individual` rules from that fiscal year on.
  This case in particular needs a contador (open question 10).

Notes on `informal-group`:

- A supporter who is a taxpayer may want the CFDI in their own name for their own records; the
  app accepts it as proof but records whose RFC it names, so nobody later mistakes it for an
  organization expense.
- With no CFDI to key on, matching relies on the receipt's amount, date and merchant (pieces 5 and
  6), and the statement line becomes the main evidence that the supporter paid. Statements are
  therefore less optional than under the other policies (compare R13).
- Retention is the group's own rule (and any funder's), not CFF art. 30. Five years is a sensible
  default.
- If the group later incorporates and gets an RFC, it switches policy from that fiscal year on;
  earlier years keep the policy they were booked under.
- A funder or grant may impose its own proof rules on an informal group (for example, CFDIs to a
  fiscal sponsor's RFC). That is a fiscal sponsor arrangement: the sponsor's RFC goes in the
  organization's fiscal profile and the sponsor's policy (usually `nonprofit` or
  `donataria-autorizada`) applies to those expenses (open
  question 8).

The rest of the Mexico section applies to `company`, `nonprofit` and `donataria-autorizada`, and to
`individual` with the person's RFC in place of the organization's.

## Mexico

### 1. The CFDI is the proof, and it must name the organization

- An expense is supported by a CFDI (Comprobante Fiscal Digital por Internet) that meets CFF
  arts. 29 and 29-A. For the organization to treat it as its own expense, the **receptor** must be
  the organization: its RFC, name, fiscal postcode, tax regime (603, *Personas Morales con Fines no
  Lucrativos*, for a non-profit) and a *UsoCFDI* such as G03 (gastos en general). CFDI 4.0 rejects
  a mismatch of RFC, name and postcode at issuance.
- A CFDI issued to the supporter's own RFC (common with freelancers who ask for an invoice out of
  habit) is the supporter's expense, not the organization's. Fixing that needs the supporter to
  re-invoice the organization (with the *Pago de erogaciones a través de terceros* complement),
  which is exactly the burden the app should help avoid by asking for the organization's RFC at
  the till.
- **Gastos por cuenta de terceros.** LISR art. 18 fr. VIII and RLISR art. 41: money a person
  receives to cover expenses on someone else's behalf is not their income *if the expense is
  backed by CFDIs issued to the one who bears it*. So a supporter's reimbursement is not taxable
  income to them when the CFDI names the organization. The RMF rule implementing this (2.7.1.13,
  later renumbered 2.7.1.12) covers both orders: advance then spend, or spend then reimburse.

### 2. A bank statement line is not enough on its own

- The old option to use a bank statement in place of a CFDI (CFF art. 29-B fr. II, with an RMF
  ceiling of $50,000) dates from before the 2014 CFDI reform; it appears to have been repealed
  (to confirm). Even where it applied, it required the statement to carry the RFCs of both
  parties, which a supporter's personal statement never does.
- What the statement line *does* prove: the date, the amount and the payment method (LISR art. 27
  fr. III). That is useful audit evidence, and the matching engine's job, but it complements the
  CFDI rather than replacing it.

### 3. How it was paid

- **LISR art. 27 fr. III:** expenses over **$2,000 MXN** must be paid by electronic transfer from
  an account in the taxpayer's name, nominative cheque, credit, debit or service card, or an
  SAT-authorized e-wallet; a cash payment over the threshold is not deductible even with a CFDI.
  **Fuel** may never be paid in cash, whatever the amount.
- **Grey zone:** the rule speaks of payment *by the taxpayer*. When a supporter pays with their own
  card, the payment is electronic but not from the organization's account. Practice tolerates this
  for reimbursed expenses with a CFDI in the organization's name, but sources disagree; this is
  open question 1.
- The **reimbursement** itself should always go by transfer from the organization's account to
  the supporter, so that leg is traceable.

### 4. Employees: viáticos go through payroll

- Travel expenses (*gastos de viaje*, LISR art. 28 fr. V) are deductible only for lodging, food,
  transport, car rental and mileage, more than 50 km from the organization's premises, with daily
  caps for food, car rental and foreign lodging (amounts to confirm for the year).
- For an **employee**, viáticos are reported in the **CFDI de nómina** (viáticos node). The
  employee may leave up to 20% of each trip's viáticos unsupported, capped at $15,000 a year, not
  for lodging or air fares (LISR art. 93 fr. XVII); the rest needs CFDIs. Unspent amounts returned
  by the employee are not reported on the payroll CFDI.
- Volunteers and freelancers are not on payroll, so their reimbursements follow section 1 instead.

### 5. Donatarias autorizadas and other non-profits

- A *persona moral con fines no lucrativos* (LISR Title III) does not deduct expenses as a company
  does, but since the reform in force from 2021, **an expense without its CFDI counts as
  remanente distribuible** (LISR arts. 79 and 86), and the organization pays ISR on it. The SAT's
  own guidance for donatarias says so.
- Donatarias may spend at most **5% of donations received on administration** (RLISR art. 138, to
  confirm), so each expense should be classified as administration or mission (*objeto social*).
- A supporter who would rather **donate** the amount than be reimbursed: a donation receipt (CFDI
  de donativo) covers money or goods the donataria actually receives. Whether waiving a
  reimbursement can be receipted as a donation, or must be reimbursed and then donated back, is
  open question 3.

### 6. CFDI status can change after the fact

- A CFDI can be **cancelled** by its issuer after it was received (often needing the receiver's
  acceptance). The SAT's verification service answers, for a UUID, issuer RFC, receiver RFC and
  total, whether a CFDI is *Vigente* or *Cancelado*.
- An issuer on the SAT's **art. 69-B list** (EFOS: companies that invoice simulated operations)
  makes its CFDIs worthless for the receiver unless the receiver proves the operation was real.

### 7. How long to keep records

- **CFF art. 30:** accounting records and their supporting documents are kept for **five years**
  from the date the related return was filed or was due; some records (equity, losses, contributions
  with long-term effects) must be kept longer, and some information (CFF arts. 32-B fr. V and
  32-B Bis) for six years.
- Persons morales, non-profits included, keep electronic accounting (CFF art. 28; catalogue and
  trial balance sent through the SAT portal unless exempt).

### 8. Crossing borders: foreign accounts and foreign vendors

Two cases come up when supporters or vendors are outside Mexico. Neither changes matching,
redaction or the audit trail; both are to be confirmed by a contador.

**A foreign account paying a Mexican business** (for example, a supporter's US card at a Mexican
shop):

- The proof is unchanged: a CFDI from the Mexican business to the organization's RFC. The country
  of the card doesn't matter to the CFDI.
- The foreign statement is still payment evidence (date, amount, that the supporter paid). A card
  is an electronic means of payment for LISR art. 27 fr. III; whether a *personal* card is
  acceptable is open question 1.
- The amounts differ: the CFDI is in pesos (or carries its own *TipoCambio* if issued in another
  currency) and the statement is in the card's currency, after the bank's rate, margin and any
  foreign transaction fee. Matching needs a rate for the date and a tolerance; the fee is the
  supporter's cost unless the organization's policy reimburses it.
- For the books, foreign-currency amounts are converted at the rate the CFF uses (art. 20: the
  Banxico rate published in the DOF for the previous business day, to confirm). The reimbursement
  is booked in pesos at what the organization actually pays the supporter, and any difference is
  an exchange gain or loss.

**A Mexican organization paying a foreign vendor** that cannot issue a CFDI (software, a venue or
supplier abroad, an online platform):

- A foreign resident without a permanent establishment in Mexico doesn't issue CFDIs. The proof is
  the vendor's own invoice, which must carry minimum details set by the RMF (vendor name, address
  and tax id, description, amount, date and the organization as buyer; to confirm the current
  rule).
- **VAT on imported services:** when the organization uses in Mexico a service or intangible from
  abroad, it may have to self-assess IVA (LIVA arts. 24 and 26). **Digital services** from foreign
  platforms registered with the SAT (LIVA art. 18-B onward) are different: the platform charges
  IVA itself and issues a receipt, not a CFDI.
- **Withholding:** some payments abroad (fees, royalties, some services) can carry ISR withholding
  under LISR Title V. The app flags foreign-vendor expenses for the contador and never computes
  the tax.
- Under `informal-group` there is no SAT side, so a foreign invoice is simply the receipt.

## United States (secondary)

- **Accountable plan** (IRC § 62(c), Treas. Reg. § 1.62-2, IRS Publication 463 ch. 6, Publication
  15 for employers): reimbursements are excluded from wages only if all three hold:
  1. **Business connection:** the expense was paid in performing services for the organization.
  2. **Substantiation within a reasonable period:** for travel, meals, gifts and listed property,
     the IRC § 274(d) elements (amount, date, place, business purpose) with documentary evidence
     for lodging and for any other expense of **$75 or more** (Treas. Reg. § 1.274-5(c)(2)(iii)).
  3. **Return of excess within a reasonable period.**
  Safe harbours: the *fixed date method* (advance within 30 days of the expense, substantiate
  within 60 days, return excess within 120 days) or the *periodic statement method* (statements
  at least quarterly, 120 days to substantiate or return).
- Failing a test makes the payment wages (withholding and payroll taxes). One failed claim does not
  disqualify the whole plan.
- **Volunteers:** reimbursed, substantiated out-of-pocket costs are not income; unreimbursed ones
  may be a charitable deduction for the volunteer (Publication 526), with a written acknowledgment
  from the organization for $250 or more.
- **Contractors:** if a contractor adequately accounts for expenses, reimbursements generally stay
  off their income; otherwise they belong on the Form 1099-NEC (to confirm per case).
- A **card or bank statement** shows amount and date but not place and business purpose; it
  supports a claim alongside a receipt or a written record, not instead of one.
- **Retention:** generally three years from filing (Publication 583), four years for employment
  tax records, longer in some cases.

## What this means for Receipt Dropbox

These requirements feed roadmap pieces 15 and 16 (see [roadmap](roadmap.md)). Thresholds, caps and
deadlines are **jurisdiction policy in Nickel, versioned by fiscal year**, never constants in code.

### Fiscal proof (piece 15)

- **R0.** A per-organization **proof policy** (`company`, `nonprofit`, `donataria-autorizada`,
  `individual`, `informal-group`; see [above](#it-depends-on-what-kind-of-organization-it-is)) in
  Nickel, per fiscal year. It selects which of R1 to R10 apply:
  - `company`, `nonprofit`: all of them; `nonprofit` adds the purpose check (an expense outside the
    organization's purpose is flagged as *remanente distribuible*).
  - `donataria-autorizada`: as `nonprofit`, plus donation receipts, the administration/mission
    `class` as required metadata with the cap tracked, and data for the transparency report.
  - `individual`: all of them, checked against the person's RFC and regime; claims from the owner
    to themself are skipped (there is no one to reimburse), employees' claims go through as
    supporters'. With `sat-registered = false` it applies the `informal-group` rules below and
    raises the unregistered-activity warning on every run.
  - `informal-group`: R1 and R2 become optional (a CFDI is accepted and whose RFC it names is
    recorded, never required), R3 runs only on CFDIs that are present, and R4 and R6 key on the
    receipt instead of the CFDI.
  R5 and R7 apply to every policy.
- **R1.** Capture CFDI **XML** (from upload, email or a supporter's SAT download), not only the PDF:
  the XML is the legal document. Parse CFDI 4.0, including the Pagos and *erogaciones a través de
  terceros* complements.
- **R2.** Check the receptor against the organization's fiscal profile (RFC, name, postcode,
  regime, UsoCFDI). A CFDI issued to the supporter's RFC is flagged with how to fix it, never
  silently accepted.
- **R3.** Check status with the SAT verification service on intake, again at month close and before
  the annual return; check the issuer against the art. 69-B list. Store the result and the time
  of each check. (The SAT endpoint does not allow browser calls, so this is a host capability,
  run by a Worker or the local host. Status comes from the SAT `ConsultaCFDIService` SOAP service and the
  69-B list from the SAT's open-data files; see [ADR-0004](adr/0004-community-mexican-tax-skills.md).)
- **R4.** Match **CFDI ↔ statement line ↔ reimbursement transfer**: UUID-keyed, on total, date
  window and payment method (*FormaPago* 04/28 card, 03 transfer against the statement's line).
  A matched statement line with no CFDI is "payment evidence only" and is not posted as a
  supported expense in Mexico.
- **R5.** Flag payments over the cash threshold made in cash, and fuel paid in cash at any amount.
- **R6.** Ledger metadata on every expense: `cfdi`: UUID, `rfc-emisor`, `forma-pago`,
  `metodo-pago`, `sat-status` and `sat-checked`, `supporter`, `supporter-kind`
  (employee/volunteer/contractor), `purpose` (US: business purpose), `class`
  (administration/mission), alongside the existing `receipt` and `statement` links.
- **R7.** **Reimbursement claims** as first-class records: claim, approval, payment by transfer
  from the organization's account, liability cleared in the ledger. Deadlines per policy (US
  60/120 days); an overdue advance surfaces for return.
- **R8.** **Employees' viáticos** export as data for the payroll CFDI (the app does not stamp
  payroll), with the 20%/annual-cap tracking per employee.
- **R9.** For the US profile, require a business purpose per claim and a receipt for lodging and
  for items of $75 or more; a statement line alone does not substantiate.
- **R10.** "Donate instead of reimburse" exists only behind an organization setting, off until a
  contador confirms the mechanism (open question 3).

- **R16. Multi-currency matching** (piece 6). A statement line and a receipt or CFDI in different
  currencies match on the converted amount within a tolerance; the rate source (Banxico FIX per
  the DOF, or the card network's rate) and the tolerance are Nickel policy. Store both amounts,
  the rate and its date; book foreign-transaction fees and exchange differences to their own
  accounts. The redacted statement shows the original currency and the booked pesos.
- **R17. Foreign invoices** (piece 15). Accept a foreign vendor's invoice as proof when the
  vendor is not a Mexican taxpayer, check it for the RMF's minimum details, and flag it for the
  contador: possible IVA self-assessment on imported services, possible withholding, and whether
  the vendor is a SAT-registered digital platform. Applies to every proof policy except
  `informal-group`, where it is just a receipt.

### Supporter data and retention (piece 16)

- **R11. Privacy notice and consent (LFPDPPP).** Show the organization's *aviso de privacidad* at
  onboarding and before each statement upload; record express consent for financial data as a
  signed, timestamped envelope assertion. Note: Mexico enacted a new LFPDPPP in March 2025
  (oversight moved from INAI to the federal government); confirm which articles apply.
- **R12. ARCO rights.** A supporter can export, correct and request deletion of their data from the
  PWA, and object to processing; requests and outcomes go to the audit log.
- **R13. Minimize third-party data.** Let supporters upload a date range or a partial statement,
  or pre-redact lines before upload; unmatched lines are never decrypted outside the supporter's
  device; counterparty names in elided rows are never indexed. Matching on CFDIs means a statement
  is supporting evidence, so it should be optional where the organization's policy allows.
- **R14. Retention per document class.** CFDIs, ledger and reimbursement records: five years from
  the annual return (MX), per the US profile otherwise. Supporters' original statements: delete
  the encrypted original once its audit window closes, keeping its digest and the redacted
  envelope (elision keeps the root digest, so the audit link survives). A legal hold suspends
  deletion. Deletion is logged.
- **R15. Worker classification signals.** Flag patterns that look like pay rather than
  reimbursement: regular fixed amounts, claims without CFDIs, reimbursements to a contractor
  bundled with fees. The app reports; it never decides classification.

## Open questions for a contador or lawyer

1. When a supporter pays an organization expense over $2,000 with their **personal card** and the
   CFDI names the organization, is it deductible (or, for a donataria, not *remanente
   distribuible*), given LISR art. 27 fr. III?
2. Which **proof policy** fits the organization: does it have its own RFC, is it a **donataria
   autorizada**, and which regime and *UsoCFDI* should supporters ask for at the till? If it has
   no RFC, is a receipt or a CFDI to *público en general* enough for its funders?
3. Can a supporter's **waived reimbursement** be receipted as a donation, or must it be paid and
   donated back?
4. Do **volunteers'** reimbursements need any CFDI or contract beyond the expense CFDIs?
5. Which retention period applies to **supporters' personal statements** held only as evidence:
   CFF art. 30's five years, or the shorter period data protection prefers?
6. Which **LFPDPPP** version and obligations apply, and does the organization need a data
   processing agreement with whoever hosts the Cloudflare deployment?
7. For a **US** entity or US supporters: which plan method (fixed date or periodic statement) and
   which forms apply to contractors' reimbursements?
8. For an **informal group** funded through a grant: does a fiscal sponsor need the CFDIs in its
   name, and how are the sponsor's and the group's records kept apart?
9. For an **individual** acting as the organization: which of their regime's rules (RESICO or
   *actividad empresarial*) limit what supporters' reimbursed expenses can be deducted?
10. For a persona física with **employees who is not registered** with the SAT for that activity:
    what must they do to regularize (RFC activity, IMSS, payroll), and can expenses from before
    registration be supported retroactively?
11. When a **contractor** supporter bills the organization for fees, which withholdings apply
    (10% ISR and two thirds of IVA, or 1.25% ISR under RESICO, LISR art. 113-J), and does the app
    need to track them or leave them to the organization's accounting?
12. For **foreign-currency payments**: which exchange rate books the expense and the
    reimbursement, and does the organization reimburse the supporter's foreign transaction fees?
13. For **foreign vendors**: what must a foreign invoice show, when does the organization owe IVA
    on an imported service or withhold ISR, and how are SAT-registered digital platforms'
    receipts handled?

## Sources

Community Agent Skills and MCP servers for Mexican tax were reviewed and not adopted; see
[ADR-0004](adr/0004-community-mexican-tax-skills.md).

Official texts (check the version in force): [CFF](https://www.diputados.gob.mx/LeyesBiblio/pdf/CFF.pdf),
[LISR](https://www.diputados.gob.mx/LeyesBiblio/pdf/LISR.pdf), [SAT obligations of donatarias
autorizadas](https://wwwmat.sat.gob.mx/consulta/70078/conoce-las-principales-obligaciones-fiscales-de-las-donatarias-autorizadas),
[SAT CFDI verification](https://verificacfdi.facturaelectronica.sat.gob.mx/),
[IRS Publication 463](https://www.irs.gov/publications/p463),
[IRS: accountable plan rules](https://www.irs.gov/node/458),
[IRS Publication 15](https://www.irs.gov/publications/p15),
[IRS Publication 526](https://www.irs.gov/publications/p526),
[IRS Publication 583](https://www.irs.gov/publications/p583).

Secondary (professional commentary used to locate the rules):
[IDC: requisitos para deducir viáticos](https://idconline.mx/fiscal-contable/2025/02/26/son-deducibles-los-viaticos-requisitos-fiscales-basicos),
[IDC: viáticos 2026](https://idconline.mx/fiscal-contable/2026/08/26/viaticos-2026-requisitos-para-deducirlos-y-evitar-isr-al-trabajador),
[elconta: gastos por cuenta de terceros](https://elconta.mx/gastos-cuenta-terceros-fundamentos-casos-practicos-cfdi-reembolso/),
[IDC: erogaciones a través de terceros](https://idconline.mx/fiscal-contable/2019/09/24/modificaciones-al-cfdi-por-erogaciones-a-traves-de-terceros),
[elcontribuyente: gastos de más de $2,000](https://www.elcontribuyente.mx/2021/03/las-deducciones-de-gastos-superiores-a-2-mil-pesos-deben-cumplir-estos-requisitos/),
[IDC: remanente ficto para 2021](https://idconline.mx/fiscal-contable/2020/09/21/para-2021-todos-los-no-deducibles-serian-remanente-ficto),
[Cemefi: obligaciones de donatarias](https://www.cemefi.org/images/pdf/reformas_fiscales_donatarias_2021_sat.pdf),
[contadormx: plazos de conservación (CFF 30)](https://contadormx.com/plazos-para-conservacion-de-la-contabilidad-y-documentacion-del-cff/),
[elconta: deducir con estado de cuenta](https://elconta.mx/deducir-solo-estado-cuenta-sin-factura/),
[cfdibills: SAT ConsultaCFDIService parameters](https://cfdibills.readthedocs.io/en/latest/_modules/cfdibills/api.html).
