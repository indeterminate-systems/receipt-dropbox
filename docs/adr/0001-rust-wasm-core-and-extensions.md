# ADR-0001: One Rust/WebAssembly core; extensions are Rust crates that also build as components

**Status:** Proposed
**Date:** 2026-11-19
**Deciders:** Ash

## Context

The same logic (parsing, matching, envelope encryption and elision, ledger writing) must run in a
browser PWA, on Cloudflare Workers, on Google and locally. The system must be extensible (new
statement layouts, matchers, ledger formats, intake channels) using only Rust that compiles to
WebAssembly. The browser cannot run wasmtime, and the WebAssembly component model has no native
browser support; running components there today means transpiling them with JavaScript tooling
(`jco`), which breaks the Rust-only rule.

## Decision

- All domain logic lives in pure Rust crates with no direct I/O, built for `wasm32-unknown-unknown`
  and `wasm32-wasip2`.
- Every extension point is a Rust trait with a WIT interface of the same shape in `wit/rdx.wit`.
- An extension is a Rust crate implementing the trait. It is **statically linked** into PWA and
  Worker builds (selected by Cargo features generated from the deployment's Nickel config) and can
  be **built as a component** (`component` feature, `wit-bindgen`) for hosts that embed wasmtime
  (local CLI, Cloud Run), which load it at runtime in a sandbox with only the capabilities config
  grants.

## Options considered

### A: Static linking everywhere
| Dimension | Assessment |
| :-- | :-- |
| Complexity | Low |
| Runtime extensibility | None: adding a parser means a rebuild |
| Portability | Full |

### B: Components everywhere
| Dimension | Assessment |
| :-- | :-- |
| Complexity | High |
| Runtime extensibility | Full on wasmtime hosts |
| Portability | Browser needs `jco` (JavaScript) and Workers support is immature |

### C: Dual build (chosen)
| Dimension | Assessment |
| :-- | :-- |
| Complexity | Medium: one trait, one WIT file, a thin adapter per extension |
| Runtime extensibility | On wasmtime hosts; rebuild for PWA and Workers |
| Portability | Full, Rust-only |

## Trade-off analysis

C costs a small adapter per extension and a test that the trait and the WIT interface stay in
step, in exchange for Rust-only builds on every host and runtime plug-ins where the runtime allows
them. When browsers or Workers gain usable component support, C becomes B without changing any
extension's code.

## Consequences

- Easier: one test suite for the core, run natively and under wasmtime.
- Harder: trait and WIT must change together; an `xtask` check will compare them.
- Revisit: component support in browsers and Workers.

## Action items

1. [ ] `rdx-host` crate with capability traits mirrored from `wit/rdx.wit`.
2. [ ] `cargo xtask check-wit` comparing traits and WIT.
3. [ ] First extension (a CSV statement parser) built both ways, with the same tests.
