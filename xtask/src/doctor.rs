//! `cargo xtask doctor`: checks that this shell can build, test and run everything the repo
//! needs. The dev shell (`flake.nix`) provides all of it; doctor says what is missing when it
//! doesn't, so a broken environment fails here and not halfway through CI.

use std::path::Path;
use std::process::Command;

/// The targets every crate must build for (see AGENTS.md).
pub const WASM_TARGETS: [&str; 2] = ["wasm32-unknown-unknown", "wasm32-wasip2"];

/// Tools the dev shell provides, and what each is for.
const TOOLS: [(&str, &str); 7] = [
    ("cargo-clippy", "lints (cargo clippy)"),
    ("rustfmt", "formatting (cargo fmt)"),
    (
        "wasmtime",
        "runs the wasm32-wasip2 tests (.cargo/config.toml runner)",
    ),
    ("wasm-tools", "inspects WebAssembly components"),
    ("wasm-bindgen", "browser bindings for the PWA"),
    ("nickel", "trying plans and contracts by hand"),
    ("typst", "rendering viewable statements"),
];

/// Runs every check and prints one line each; returns whether all passed.
pub fn run(root: &Path) -> bool {
    let mut ok = true;
    let mut line = |pass: bool, what: String| {
        println!("{} {what}", if pass { "ok  " } else { "FAIL" });
        ok &= pass;
    };

    let minimum = workspace_rust_version(root).unwrap_or_else(|| "0".into());
    match tool_version("rustc") {
        Some(v) => line(
            at_least(&v, &minimum),
            format!("rustc {v} (workspace needs {minimum} or later)"),
        ),
        None => line(false, "rustc not found".into()),
    }

    match sysroot() {
        Some(sysroot) => {
            let rustlib = sysroot.join("lib/rustlib");
            for target in WASM_TARGETS {
                line(
                    rustlib_has_target(&rustlib, target),
                    format!("Rust standard library for {target}"),
                );
            }
        }
        None => line(false, "rustc --print sysroot failed".into()),
    }

    for (bin, why) in TOOLS {
        match tool_version(bin) {
            Some(v) => line(true, format!("{bin} {v}: {why}")),
            None => line(false, format!("{bin} not found: {why}")),
        }
    }

    // The CLI refuses modules built by any other wasm-bindgen version, so the two must match
    // exactly once a crate depends on it.
    let lock = std::fs::read_to_string(root.join("Cargo.lock")).unwrap_or_default();
    if let Some(wanted) = lock_version(&lock, "wasm-bindgen") {
        let have = tool_version("wasm-bindgen");
        line(
            have.as_deref() == Some(wanted),
            format!(
                "wasm-bindgen CLI {} matches the crate in Cargo.lock ({wanted})",
                have.as_deref().unwrap_or("missing")
            ),
        );
    }

    if !ok {
        println!(
            "\nEnter the dev shell (`nix develop` or `direnv allow`), which provides all of these."
        );
    }
    ok
}

fn tool_version(bin: &str) -> Option<String> {
    let out = Command::new(bin).arg("--version").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    version_in(&text).map(str::to_owned)
}

fn sysroot() -> Option<std::path::PathBuf> {
    let out = Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().into())
}

fn workspace_rust_version(root: &Path) -> Option<String> {
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).ok()?;
    manifest.lines().find_map(|l| {
        let value = l
            .trim()
            .strip_prefix("rust-version")?
            .trim()
            .strip_prefix('=')?;
        Some(value.trim().trim_matches('"').to_owned())
    })
}

/// Whether `rustc`'s standard library for `target` is installed under `rustlib`.
pub fn rustlib_has_target(rustlib: &Path, target: &str) -> bool {
    rustlib.join(target).join("lib").is_dir()
}

/// The first `N.N[.N…]` word in a tool's `--version` output, without a `-stable`-style suffix.
pub fn version_in(output: &str) -> Option<&str> {
    output
        .split_whitespace()
        .map(|word| word.split(['-', '+']).next().unwrap_or(word))
        .find(|word| {
            let mut parts = word.split('.');
            let numeric = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
            parts.clone().count() >= 2 && parts.all(numeric)
        })
}

/// Whether dotted `version` is at least `minimum`, comparing numerically; false if either
/// doesn't parse.
pub fn at_least(version: &str, minimum: &str) -> bool {
    let parse = |v: &str| -> Option<Vec<u64>> { v.split('.').map(|p| p.parse().ok()).collect() };
    match (parse(version), parse(minimum)) {
        (Some(have), Some(want)) => have >= want,
        _ => false,
    }
}

/// The version of `package` in a `Cargo.lock`, if it is there.
pub fn lock_version<'a>(lock: &'a str, package: &str) -> Option<&'a str> {
    let wanted = format!("name = \"{package}\"");
    let mut lines = lock.lines();
    while let Some(line) = lines.next() {
        if line.trim() == wanted {
            let next = lines.next()?.trim();
            return next.strip_prefix("version = \"")?.strip_suffix('"');
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn finds_installed_targets_only() {
        let rustlib = std::env::temp_dir().join(format!("rdx-doctor-{}", std::process::id()));
        fs::create_dir_all(rustlib.join("wasm32-wasip2/lib")).unwrap();
        assert!(rustlib_has_target(&rustlib, "wasm32-wasip2"));
        assert!(!rustlib_has_target(&rustlib, "wasm32-unknown-unknown"));
        fs::remove_dir_all(&rustlib).unwrap();
    }

    #[test]
    fn reads_versions_from_tool_output() {
        assert_eq!(
            version_in("rustc 1.91.1 (ed61e7d7e 2025-11-07)"),
            Some("1.91.1")
        );
        assert_eq!(version_in("wasm-bindgen 0.2.105"), Some("0.2.105"));
        assert_eq!(
            version_in("wasmtime 38.0.3 (abc 2025-10-20)"),
            Some("38.0.3")
        );
        assert_eq!(
            version_in("nickel-lang-cli nickel 1.14.0 (rev 1a2b3c)"),
            Some("1.14.0")
        );
        assert_eq!(version_in("typst 0.14.0 (b33de9de)"), Some("0.14.0"));
        assert_eq!(
            version_in("rustfmt 1.9.0-stable (58cf8f9c3d 2026-10-13)"),
            Some("1.9.0")
        );
        assert_eq!(version_in("no version here"), None);
    }

    #[test]
    fn compares_versions_numerically() {
        assert!(at_least("1.91.0", "1.90"));
        assert!(at_least("1.90.0", "1.90"));
        assert!(at_least("1.100.0", "1.90"));
        assert!(!at_least("1.89.9", "1.90"));
        assert!(!at_least("garbage", "1.90"));
    }

    #[test]
    fn reads_package_versions_from_cargo_lock() {
        let lock = r#"
[[package]]
name = "wasm-bindgen"
version = "0.2.105"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "wasm-bindgen-macro"
version = "0.2.105"
"#;
        assert_eq!(lock_version(lock, "wasm-bindgen"), Some("0.2.105"));
        assert_eq!(lock_version(lock, "serde"), None);
    }
}
