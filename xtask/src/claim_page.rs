//! `cargo xtask claim-page [OUT]`: builds the claim page (crate `rdx-claim`) for the browser into
//! `OUT` (default `crates/rdx-worker/site`), which the intake Worker serves as static assets:
//!
//! ```text
//! OUT/claim.html                 the page (/claim)
//! OUT/claim-app/rdx_claim.js     wasm-bindgen's loader
//! OUT/claim-app/rdx_claim_bg.wasm
//! ```
//!
//! The Worker's `wrangler.toml` runs this before `worker-build`, so a deploy always ships the
//! page that matches the Worker's API.

use std::fs;
use std::path::Path;
use std::process::{Command, ExitCode};

pub fn run(root: &Path, out: Option<&str>) -> ExitCode {
    let out = out.map_or_else(|| root.join("crates/rdx-worker/site"), |o| o.into());
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let built = Command::new(cargo)
        .current_dir(root)
        .args([
            "build",
            "--release",
            "-p",
            "rdx-claim",
            "--lib",
            "--target",
            "wasm32-unknown-unknown",
        ])
        .status();
    if !built.is_ok_and(|s| s.success()) {
        eprintln!("FAIL building rdx-claim for wasm32-unknown-unknown");
        return ExitCode::FAILURE;
    }
    let app = out.join("claim-app");
    let _ = fs::remove_dir_all(&app);
    let wasm = root.join("target/wasm32-unknown-unknown/release/rdx_claim.wasm");
    // The dev shell's wasm-bindgen matches Cargo.lock's (`cargo xtask doctor` checks).
    let bound = Command::new("wasm-bindgen")
        .arg("--target")
        .arg("web")
        .arg("--no-typescript")
        .arg("--out-dir")
        .arg(&app)
        .arg(&wasm)
        .status();
    if !bound.is_ok_and(|s| s.success()) {
        eprintln!("FAIL wasm-bindgen on {}", wasm.display());
        return ExitCode::FAILURE;
    }
    let page = root.join("crates/rdx-claim/site/claim.html");
    if let Err(e) = fs::copy(&page, out.join("claim.html")) {
        eprintln!("FAIL copying {}: {e}", page.display());
        return ExitCode::FAILURE;
    }
    println!("claim page built in {}", out.display());
    ExitCode::SUCCESS
}
