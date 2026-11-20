//! getrandom 0.2's backend in the browser build: always an error.
//!
//! bc-envelope's crypto crates link getrandom, which has no browser backend unless the final
//! binary picks one. The core never takes randomness from it: salt comes from the caller's
//! generator, which hosts back with their `random` capability. Failing here turns an accidental
//! call into an error instead of a silent dependency on the platform. getrandom 0.3 gets the same
//! treatment from `.cargo/config.toml`. Drop this when the PWA (roadmap piece 14) chooses a real
//! backend for the browser.

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn no_system_rng(_: &mut [u8]) -> Result<(), getrandom::Error> {
    Err(getrandom::Error::UNSUPPORTED)
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
getrandom::register_custom_getrandom!(no_system_rng);
