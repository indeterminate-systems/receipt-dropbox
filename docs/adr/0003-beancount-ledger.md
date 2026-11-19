# ADR-0003: Beancount as the ledger format

**Status:** Proposed
**Date:** 2026-11-19
**Deciders:** Ash

## Context

The ledger is plain-text double-entry bookkeeping and the source of truth. Every entry must link
to its receipt and to the digest of the encrypted original statement. The core writes the ledger
in Rust (WebAssembly), and a spreadsheet export is generated on request.

## Decision

Use Beancount syntax. Each transaction carries metadata:

```beancount
2026-03-04 * "Office Depot" "Printer paper"
  receipt: "ur:envelope/…"            ; or the receipt's digest
  statement: "sha256:9f2c…"           ; digest of the encrypted original
  supporter: "s-0042"
  Expenses:Office:Supplies     42.17 USD
  Liabilities:Reimbursable:s-0042
```

## Options considered

| Dimension | Ledger | hledger | Beancount |
| :-- | :-- | :-- | :-- |
| Strictness | Loose (implicit accounts) | Medium | Strict: accounts must be opened, balances asserted |
| Structured metadata | Tags in comments | Tags in comments | Typed `key: value` metadata per entry and posting |
| Rust parser | None mature | None mature | `beancount-parser-lima` (and a non-GPL fork) |
| Viewer for accountants | Few | hledger-web | Fava |

## Trade-off analysis

Beancount's strictness (open directives, balance assertions) catches reconciliation mistakes, its
metadata gives the audit links a real field rather than a comment convention, and a Rust parser
exists for round-trip tests. hledger reads a large subset of Beancount through conversion, so the
choice doesn't lock accountants out.

## Consequences

- Easier: balance assertions per statement period check that books match statements.
- Harder: account names must follow Beancount's capitalized, colon-separated rules.
- Revisit: if accountants require hledger or Ledger, add a `ledger-writer` extension (ADR-0001).

## Action items

1. [ ] `rdx-ledger` writer with golden tests, parsed back with `beancount-parser-lima`.
2. [ ] Spreadsheet export (CSV first, then XLSX) from the parsed ledger.
