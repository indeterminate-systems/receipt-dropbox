# Fiscal fixtures (piece 15)

Synthetic CFDI 4.0 documents for `rdx-fiscal`. RFCs are the SAT's published test RFCs or
invented; UUIDs, seals and certificates are fake, so none of these would verify with the SAT.

| File | What it exercises |
| :-- | :-- |
| `../receipts/cfdi-ingreso.xml` | A CFDI to the organization (`XIA190128J61`), card, PUE: valid proof |
| `cfdi-to-supporter.xml` | Issued to a supporter's own RFC: flagged, with how to fix it |
| `cfdi-publico-general.xml` | A global CFDI to *público en general* (`XAXX010101000`, UsoCFDI S01) |
| `cfdi-fuel-cash.xml` | Fuel paid in cash, over the cash threshold |
| `cfdi-ppd.xml`, `pago-ppd.xml` | Paid later (PPD) and its Pagos 2.0 complement |
| `cfdi-unstamped.xml` | No `TimbreFiscalDigital`: never stamped, not an invoice |
