# ADR-0004: Community Mexican tax skills and connectors are references, not dependencies

**Status:** Proposed
**Date:** 2026-11-19
**Deciders:** Ash

## Context

Piece 15 (fiscal proof, see [compliance](../compliance.md#fiscal-proof-piece-15)) needs Mexican tax
knowledge: CFDI 4.0 structure, receptor checks, SAT status and art. 69-B checks, and for a
donataria the donation receipt, the administration cap and the transparency report. Nothing
official exists as an Agent Skill or MCP server, but three community offerings were suggested.
Each was traced to its source and read on 2026-11-19, without installing anything:

| Listing | Real source | Maintainer | License | What it is |
| :-- | :-- | :-- | :-- | :-- |
| Mx Cfdi (claudskills.com) | [`openaccountants/openaccountants`](https://github.com/openaccountants/openaccountants), `agent-skills/mx-cfdi/SKILL.md` at `cc44d7c` | Glimpse Ltd and the OpenAccountants community | Skill text: *Open Accountants Skills License 2.0* (source-available); code: AGPL-3.0-only | One `SKILL.md` (399 lines), plus nine sibling `mx-*` skills (income tax, RESICO, IMSS, payroll) |
| Skills MX (mcpmarket.com) | [`elimorals/skills_mx`](https://github.com/elimorals/skills_mx) at `fb24d5c` | elimorals | Apache-2.0 | 40 Claude Code plugins, 342 skills, 83 Python (FastMCP) servers, 17 hook scripts, a webhook receiver |
| Notsy Facturación (claudemarketplaces.com) | No public repository; remote MCP at `mcp.notsy.com.mx` | Notsy (no legal name published); registry namespace `io.github.heusai` | None stated | Hosted service that stamps CFDIs through an unnamed PAC; tools `whoami`, `resolver_claves_sat`, `preview_factura`, `emitir_cfdi` |

## Findings

### Mx Cfdi (OpenAccountants)

- **Accuracy.** The CFDI 4.0 field list, receptor checks, *c_RegimenFiscal* codes (603, 625, 626),
  FormaPago/MetodoPago rules, payment complement deadline (5th day of the next month, RMF 2026
  2.7.1.32) and cancellation rules (2.7.1.34 and 2.7.1.35, MXN 1,000 without acceptance) agree
  with our reading of the sources. Its own metadata says `review_status: pending_review` and
  "requires sign-off by a Mexican contador publico".
- **Gaps for us.** It is written for the *issuer* of a CFDI. It does not cover what piece 15
  needs as a *receiver*: SAT status verification, the art. 69-B list, *gastos por cuenta de
  terceros*, LISR art. 27 fr. III payment form, or donatarias (no skill in the set covers 603
  beyond the regime code). Its worked example 1 (a persona física in regime 612 invoicing a
  persona moral) omits the withholdings the persona moral must make (10% ISR and two thirds of
  IVA; 1.25% ISR for RESICO under LISR art. 113-J), which matters when a contractor supporter
  bills the organization for fees.
- **Safety.** No scripts or commands in the skill. Its closing block tells the agent to call
  `get_skill` on the OpenAccountants connector for the current version: a nudge to add a third
  party's network tool, harmless but not something an agent here should follow. The frontmatter
  license (`AGPL-3.0-or-later`, Guide License 1.0) disagrees with the repository's `LICENSING.md`.
- **License.** Free use is limited to personal use, professional reference, small businesses
  (five or fewer people), education and non-commercial redistribution; other organizations and
  commercial use need a paid licence from Glimpse Ltd. Vendoring it would put source-available
  text into this Apache-2.0 repository and bind every adopter to those terms.

### Skills MX (elimorals)

- **Accuracy.** The `donatarias-ongs-mx` skills are the most useful part for us and read
  correctly where we could check them: donation CFDI with the Donatarias 1.1 complement, D04 for a
  persona física donor, 7% deduction caps (LISR arts. 27 fr. I and 151 fr. III), the 5%
  administration cap (RLISR art. 138, which the skill itself marks "verificar"), the 15 February
  informative return (LISR art. 86), the May transparency report (art. 82 fr. VI) and monthly
  reporting of cash donations over $100,000 (art. 82 fr. VII). Several rule numbers are flagged
  by the author as medium confidence.
- **Safety.** No prompt injection or unexpected hosts found: outbound URLs in the code are SAT,
  Banxico, government portals and the payment or marketplace APIs each server wraps. But the
  bundle is broad and powerful: servers read the SAT **e.firma and CIEC** from environment
  variables, can mass-download CFDIs and change SAT obligations (behind
  `PLUGINS_MX_SAT_PERMITIR_ESCRITURA`), an opt-in CAPTCHA solver sends portal images to 2captcha
  (and to the Anthropic API as a fallback), and the repository's `.claude/settings.json` runs
  shell hooks on every `Write`/`Edit`. It is Python, so none of it can run in the core.
- **Useful reference.** `mcp-servers/mp_sat_portal/client.py` shows the two public sources R3
  needs: the SAT `ConsultaCFDIService` SOAP endpoint at
  `consultaqr.facturaelectronica.sat.gob.mx` (status by UUID, RFCs and total) and the art. 69-B
  open-data lists the SAT now publishes on Azure blob storage
  (`wu1agsprosta001.blob.core.windows.net/agsc-publicaciones/Datos_abiertos`).

### Notsy Facturación

- A hosted service that receives invoice data in plaintext, stamps it through a PAC it does not
  name, and keeps the account's e.firma "cifrada" on its side for SAT downloads. No published
  legal entity, retention period, subprocessor list or source code; its privacy notice did not
  load when read.
- It **issues** CFDIs. Receipt Dropbox receives and verifies them; the only CFDI an organization
  issues in our scope is a donataria's donation receipt, which it stamps with its own PAC.
- Sending supporters' or the organization's fiscal data to it breaks "servers never decrypt".

### Alternatives

- **Rust CFDI crates** for R1: [`cfdi`](https://crates.io/crates/cfdi) 0.1.4 (Serde, about
  4,500 downloads, last release 2024-12) and [`cfdi-cli`](https://crates.io/crates/cfdi-cli)
  0.1.0 (MIT, quick-xml, claims a WebAssembly build, 29 downloads). Neither is proven; piece 15
  should evaluate them against the Anexo 20 XSD before writing its own parser.
- Python `satcfdi` and Go `gobl.mx.cfdi` are mature but cannot run in the core; they are useful
  as test oracles for our parser.
- Other community skills found (FactuCat CLI, Odoo l10n compliance, a paid Carta Porte
  pre-checker) are issuer-side or tied to a product.

## Decision

Adopt none of them as a runtime dependency, an installed plugin or an MCP server, and vendor
none into `.agents/skills/`.

- They are **references**. Piece 15 ports the rules it needs into Nickel policy per fiscal year
  and Rust ops with tests, citing the primary source (CFF, LISR, RMF, Anexo 20), not the skill.
- `elimorals/skills_mx` (Apache-2.0) at `fb24d5c` may be read and quoted, with attribution, as a
  starting point for the donataria rules and the SAT endpoints.
- `openaccountants` skills may be read for orientation only; nothing from them is copied here.
- Notsy, or any hosted CFDI issuer, stays outside the system. If an organization wants an agent
  to issue its donation receipts, that is its own PAC account and its own tool, not an
  `rdx` capability.

## Consequences

- No third-party code or hooks run in an agent session on this repository, and no fiscal
  credential (e.firma, CIEC, CSD, PAC key) is ever asked for.
- Piece 15 does more writing up front: an `mx-cfdi` reference in `docs/reference/` and the
  `verify-cfdi` skill are ours, reviewed against primary sources and then by a contador.
- Two findings carry into [compliance](../compliance.md): withholdings on a contractor
  supporter's fees (open question 11), and the SAT endpoints above for R3.
- Revisit if an official SAT or Blockchain Commons–compatible library appears, or if
  OpenAccountants relicenses its skills under an open licence and a contador signs off the
  Mexican ones.
