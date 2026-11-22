---
name: verify-cfdi
description: Checks whether a CFDI (Mexican electronic invoice), or the receipt or foreign invoice standing in for one, proves an organization expense under its proof policy - issued to the organization's RFC, stamped, current with the SAT, paid in a way that counts - and says how to fix what doesn't. Use when a supporter sends a factura or CFDI XML, before booking or reimbursing an expense, or when asked whether an expense is deductible or properly invoiced.
metadata:
  rdx-kind: product
---

# Verify CFDI

**Goal:** tell the person whether one expense has the proof their organization's policy needs,
and if not, exactly what to ask for.

What counts depends on the organization (docs/compliance.md R0): with an RFC, the proof is a
CFDI issued to that RFC; a group with no RFC (`informal-group`) proves an expense with the
receipt. The policy (`rdx/fiscal.ncl`) says which, per fiscal year.

## Run it

| Input | Meaning | Default |
| :-- | :-- | :-- |
| `supporter` | id of the supporter who paid, such as `"s-0001"` | required |
| `policy` | `{ kind = "nonprofit", fiscal_year = 2026, profile = { rfc = "…", name = "…", postcode = "…", regime = "603" } }` | required |
| `cfdi` | the XML: `import "/abs/path/factura.xml" as 'Text` (never the PDF) | `null` |
| `receipt` | the receipt the CFDI should prove (a ticket matched to a statement line); without one, it is read from the CFDI | `null` |
| `payments` | Pagos complement XMLs for a PPD CFDI, as an array of imports | `[]` |
| `sat` | the SAT check: `{ status = "vigente", efos = "none", checked = "2026-03-20T10:00:00Z" }` | `null` |
| `foreign` | a foreign vendor's invoice: `{ vendor, tax_id, address, description, buyer, country, digital_platform }` | `null` |
| `paid_with` | `"cash"`, `"card"`, `"transfer"` or `"other"`, from the matched statement line | `null` |
| `purpose`, `class` | what it was for; `"administration"` or `"mission"` (a donataria needs both) | `null` |

```sh
rdx run verify-cfdi --input supporter='"s-0001"' \
  --input policy='{ kind = "nonprofit", fiscal_year = 2026, profile = { rfc = "XIA190128J61", name = "ORGANIZACIÓN DE PRUEBA", postcode = "06000", regime = "603" } }' \
  --input cfdi="import \"$PWD/factura.xml\" as 'Text" \
  --input purpose='"Paper for the March workshop"'
```

In this repository `rdx` is `cargo run -p rdx-cli --`. Nothing has effects, so the dry run is
the whole run. Take `policy` from the organization's settings, never from a guess: the profile
must be the organization's data exactly as its Constancia de Situación Fiscal shows it.

The SAT status check isn't automatic yet (no host runs it): until one does, ask the person to
check the CFDI at the SAT's verification site and pass the result as `sat`, or leave it `null`
and the expense stays `needs_review` with `sat-unchecked`.

## Read the report

The `proof` step's output has one assessment:

- `proof`:
  - `supported`: the evidence meets the policy. Say so, and what it rests on (`basis`).
  - `needs_review`: nothing is wrong that a person can't settle; list the `review` findings.
  - `payment_evidence_only`: no CFDI under a policy that needs one. The statement shows the
    supporter paid, but it isn't a supported expense; give the person the finding's `fix`.
  - `not_supported`: an `error` finding. Give its `message` and `fix`.
- `findings`, each with a `code`, `severity` (`error`, `review`, `info`), `message` and maybe a
  `fix`. Pass the `fix` on as written: it names the RFC, name, postcode, regime and UsoCFDI to ask
  for. Codes include `cfdi-to-other-rfc`, `cfdi-to-general-public`, `uso-cfdi-not-allowed`,
  `cfdi-missing`, `cfdi-unstamped`, `cfdi-total-mismatch`, `cash-over-threshold`,
  `fuel-paid-cash`, `ppd-unpaid`, `sat-unchecked`, `cfdi-cancelled`, `issuer-69b-definitive`,
  `foreign-invoice-review`, `purpose-missing`, `class-missing`.
- `meta`: the ledger metadata the expense will carry (`cfdi`, `rfc-emisor`, `forma-pago`,
  `sat-status`, `proof`, …). Not for the person; booking uses it.
- `warnings` at the top: while the year's rules aren't confirmed by a contador, every run says
  so. Repeat it to the person once. Never present a result as tax advice.

Don't tell the person an expense is "deductible": say whether it has the proof the policy asks
for. Questions the findings send to a contador (foreign invoices, art. 69-B, cash rules) go to
one.
