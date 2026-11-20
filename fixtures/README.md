# Fixtures

Synthetic test data only: generated statements, receipts and golden outputs. Never commit a real
statement, receipt, email or anything with a real person's or account's details, even redacted.

## `synthetic/`

Written by `rdx_model::synth`, a seeded generator, and checked by
`crates/rdx-model/tests/fixtures.rs`: the test fails if a committed file differs from what the
generator produces. After changing the generator, run

```
UPDATE_GOLDEN=1 cargo test -p rdx-model --test fixtures
```

and review the diff.

| Path | Contents |
| :-- | :-- |
| `synthetic/small/scenario.json` | `Config::small()`: 2 supporters, 1 month. Supporters, statements, receipts, the correct matches, the expense account each matched receipt books to, and `cases`, which point at each hard case |
| `synthetic/small/statements/*.csv` | the same statements as `date,description,amount,balance` CSV, for statement parsers |
| `synthetic/small/envelopes/` | each statement sealed as a Gordian Envelope (`*.full.ur`) and redacted with the scenario's matches (`*.redacted.ur`), signed with the published synthetic key in `signer.ur` (`rdx_envelope::synth`, which proves nothing). Written by `crates/rdx-envelope/tests/fixtures.rs`: `UPDATE_GOLDEN=1 cargo test -p rdx-envelope --test fixtures` |

Each statement contains:

| Case | What it is | Expected outcome |
| :-- | :-- | :-- |
| `date_lag` | receipt dated 1 to 3 days before the card payment posts | matched |
| `bill_reference` | emailed bill whose number the bank prints on the line | matched |
| `refund` | credit on the statement with a refund receipt | matched, booked as a reversal |
| `missing_receipt` | org merchant's debit with no receipt | redacted until a receipt arrives |
| `orphan_receipt` | receipt paid from another account | matches nothing |
| `personal` | personal spending and deposits | redacted |

For larger runs, call `Scenario::generate` with another `Config` (seed, supporters, months) in the
test itself rather than committing more files. Public datasets for parsing and extraction tests are
listed in [the roadmap](../docs/roadmap.md#test-corpora); they are fetched, never committed.
