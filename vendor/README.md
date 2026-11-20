# Vendored crates

Crates patched in through `[patch.crates-io]` in the workspace `Cargo.toml`. Each is the published
release with the smallest change that lets the core build for WebAssembly; the change is marked
`rdx:` in its `Cargo.toml`. Drop a crate from here as soon as a release upstream makes the patch
unnecessary.

| Crate | Release | Change | Drop when |
| :-- | :-- | :-- | :-- |
| `bc-shamir` | 0.13.0 (crates.io) | depends on `bc-crypto` without default features | a bc-shamir release does the same: its default features pull in `secp256k1-sys`, C code that doesn't build for `wasm32-unknown-unknown` or `wasm32-wasip2` without a C toolchain and sysroot, and bc-shamir never uses it |
