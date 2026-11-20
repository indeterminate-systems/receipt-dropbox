//! `cargo xtask ci`: the steps CI runs, in order, stopping at the first failure. The workflow
//! runs this inside the dev shell, so `nix develop --command cargo xtask ci` locally is the same
//! run.

use std::path::Path;
use std::process::{Command, ExitCode};

use crate::doctor::WASM_TARGETS;

/// Crates whose tests can't run under WASI, and what covers them as WebAssembly instead.
const NATIVE_ONLY_TESTS: [(&str, &str); 1] = [(
    "rdx-cli",
    "its tests spawn the `rdx` binary, which WASI can't; `cargo xtask smoke-wasm` runs `rdx` \
     under wasmtime instead",
)];

pub fn run(root: &Path) -> ExitCode {
    let crates = core_crates(root);
    let mut steps: Vec<(String, Vec<String>)> = Vec::new();
    let mut add = |what: &str, args: &[&str]| {
        steps.push((what.into(), args.iter().map(|a| a.to_string()).collect()));
    };

    add("Dev environment", &["xtask", "doctor"]);
    add("Formatting", &["fmt", "--all", "--check"]);
    add(
        "Lints",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    );
    add("Tests", &["test", "--workspace"]);
    for krate in &crates {
        for target in WASM_TARGETS {
            add(
                &format!("{krate} builds for {target}"),
                &["check", "-p", krate, "--target", target],
            );
            add(
                &format!("{krate} builds for {target} without default features"),
                &[
                    "check",
                    "-p",
                    krate,
                    "--target",
                    target,
                    "--no-default-features",
                ],
            );
            add(
                &format!("{krate} builds for {target} with all features"),
                &["check", "-p", krate, "--target", target, "--all-features"],
            );
        }
        // Type-checking for wasm is not running on wasm: run the tests under wasmtime too
        // (the runner is set in .cargo/config.toml).
        if let Some((_, why)) = NATIVE_ONLY_TESTS.iter().find(|(name, _)| name == krate) {
            println!("skip {krate} tests as WebAssembly: {why}");
            continue;
        }
        add(
            &format!("{krate} tests run as WebAssembly (wasm32-wasip2, wasmtime)"),
            &["test", "-p", krate, "--target", "wasm32-wasip2"],
        );
    }
    add("Skills", &["xtask", "check"]);
    add("WIT matches rdx-host", &["xtask", "check-wit"]);
    add(
        "rdx as WebAssembly under wasmtime",
        &["xtask", "smoke-wasm"],
    );

    for (what, args) in steps {
        println!("\n==> {what}: cargo {}", args.join(" "));
        let status = Command::new(env!("CARGO"))
            .args(&args)
            .current_dir(root)
            .status();
        match status {
            Ok(s) if s.success() => {}
            Ok(s) => {
                eprintln!("\nFAIL {what} ({s})");
                return ExitCode::FAILURE;
            }
            Err(e) => {
                eprintln!("\nFAIL {what}: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    println!("\nAll CI steps passed.");
    ExitCode::SUCCESS
}

/// The packages under `crates/`: everything there must build for WebAssembly.
fn core_crates(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(root.join("crates"))
        .into_iter()
        .flatten()
        .filter_map(|e| std::fs::read_to_string(e.ok()?.path().join("Cargo.toml")).ok())
        .filter_map(|manifest| package_name(&manifest).map(str::to_owned))
        .collect();
    names.sort();
    names
}

fn package_name(manifest: &str) -> Option<&str> {
    let package = manifest.split("[package]").nth(1)?;
    package
        .lines()
        .take_while(|l| !l.trim_start().starts_with('['))
        .find_map(|l| {
            let value = l.trim().strip_prefix("name")?.trim().strip_prefix('=')?;
            Some(value.trim().trim_matches('"'))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_package_name() {
        let manifest = "[package]\nname = \"rdx-plan\"\nversion = \"0.1.0\"\n\n[dependencies]\nname = \"not-this\"\n";
        assert_eq!(package_name(manifest), Some("rdx-plan"));
        assert_eq!(package_name("[workspace]\nmembers = []\n"), None);
    }

    #[test]
    fn every_core_crate_is_listed() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        assert!(core_crates(&root).contains(&"rdx-plan".to_string()));
    }
}
