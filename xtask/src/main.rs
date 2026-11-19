//! `cargo xtask check [SKILL…]`: validates Agent Skills against the spec and this repo's rules,
//! evaluates each product skill's code-mode entrypoint and runs its test cases as dry runs.
//!
//! `UPDATE_GOLDEN=1 cargo xtask check` rewrites the expected reports instead of comparing them.
//!
//! `cargo xtask smoke-wasm`: builds `rdx` for `wasm32-wasip2` and runs the `plan-smoke-test`
//! skill with it under `wasmtime`, so the core is exercised as WebAssembly, not only natively.
//!
//! `cargo xtask check-wit`: the host capabilities in `wit/rdx.wit` (world `core`) match the ones
//! `rdx-host` implements (`rdx_host::WIT_CAPABILITIES`).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rdx_host::Host;
use rdx_plan::{PlanSource, Registry, execute};
use serde::Deserialize;
use serde_json::Value;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("check") => run_check(&args[1..]),
        Some("check-wit") => run_check_wit(),
        Some("smoke-wasm") => run_smoke_wasm(),
        _ => {
            eprintln!("usage: cargo xtask check [SKILL…] | check-wit | smoke-wasm");
            ExitCode::from(2)
        }
    }
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn run_check(only: &[String]) -> ExitCode {
    let root = root();
    let mut problems = Vec::new();

    check_symlink(&root, &mut problems);

    let skills_dir = root.join(".agents/skills");
    let mut dirs: Vec<PathBuf> = fs::read_dir(&skills_dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default();
    dirs.sort();
    for dir in dirs {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        if !only.is_empty() && !only.contains(&name) {
            continue;
        }
        let before = problems.len();
        check_skill(&root, &dir, &name, &mut problems);
        if problems.len() == before {
            println!("ok   {name}");
        }
    }

    if problems.is_empty() {
        ExitCode::SUCCESS
    } else {
        for p in &problems {
            eprintln!("FAIL {p}");
        }
        ExitCode::FAILURE
    }
}

fn check_symlink(root: &Path, problems: &mut Vec<String>) {
    match fs::read_link(root.join(".claude/skills")) {
        Ok(target) if target == Path::new("../.agents/skills") => {}
        Ok(target) => problems.push(format!(
            ".claude/skills points to {}, expected ../.agents/skills",
            target.display()
        )),
        Err(_) => problems.push(".claude/skills must be a symlink to ../.agents/skills".into()),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Frontmatter {
    name: String,
    description: String,
    #[serde(default)]
    license: Option<String>,
    #[serde(default)]
    compatibility: Option<String>,
    #[serde(default)]
    metadata: BTreeMap<String, String>,
    #[serde(default, rename = "allowed-tools")]
    allowed_tools: Option<String>,
}

/// Rules from https://agentskills.io/specification (see docs/reference/agent-skills.md).
fn spec_problems(fm: &Frontmatter, dir_name: &str, body_lines: usize) -> Vec<String> {
    let mut out = Vec::new();
    let n = &fm.name;
    let name_ok = (1..=64).contains(&n.len())
        && n.chars().all(|c| matches!(c, 'a'..='z' | '0'..='9' | '-'))
        && !n.starts_with('-')
        && !n.ends_with('-')
        && !n.contains("--");
    if !name_ok {
        out.push(format!(
            "name `{n}`: 1-64 of a-z, 0-9 and single hyphens, not at either end"
        ));
    }
    if n != dir_name {
        out.push(format!("name `{n}` must match its directory `{dir_name}`"));
    }
    if fm.description.trim().is_empty() || fm.description.chars().count() > 1024 {
        out.push("description: 1-1024 characters".into());
    }
    if fm
        .compatibility
        .as_ref()
        .is_some_and(|c| c.is_empty() || c.chars().count() > 500)
    {
        out.push("compatibility: 1-500 characters when present".into());
    }
    if body_lines > 500 {
        out.push(format!(
            "SKILL.md body is {body_lines} lines; keep it under 500"
        ));
    }
    let _ = (&fm.license, &fm.allowed_tools);
    out
}

fn parse_skill_md(text: &str) -> Result<(Frontmatter, usize), String> {
    let rest = text
        .strip_prefix("---\n")
        .ok_or("SKILL.md must start with `---` frontmatter")?;
    let end = rest
        .find("\n---\n")
        .ok_or("frontmatter is not closed with `---`")?;
    let fm: Frontmatter =
        serde_yaml::from_str(&rest[..end]).map_err(|e| format!("frontmatter: {e}"))?;
    let body_lines = rest[end + 5..].lines().count();
    Ok((fm, body_lines))
}

fn check_skill(root: &Path, dir: &Path, name: &str, problems: &mut Vec<String>) {
    let mut report = |msg: String| problems.push(format!("{name}: {msg}"));

    let text = match fs::read_to_string(dir.join("SKILL.md")) {
        Ok(t) => t,
        Err(_) => return report("missing SKILL.md".into()),
    };
    let (fm, body_lines) = match parse_skill_md(&text) {
        Ok(v) => v,
        Err(e) => return report(e),
    };
    for p in spec_problems(&fm, name, body_lines) {
        report(p);
    }

    // House rule: `metadata.rdx-kind` is `product` (a goal agents run through code mode) or
    // `dev` (how to work on this repo). Product skills need an entrypoint and test cases.
    match fm.metadata.get("rdx-kind").map(String::as_str) {
        Some("dev") => {}
        Some("product") => check_product_skill(root, dir, name, &mut report),
        other => report(format!(
            "metadata.rdx-kind must be `product` or `dev`, got {other:?}"
        )),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    /// Top-level fields of `scripts/main.ncl` to override, as Nickel expressions.
    #[serde(default)]
    inputs: BTreeMap<String, String>,
    /// The expected dry-run report.
    #[serde(default)]
    expect: Value,
}

fn check_product_skill(root: &Path, dir: &Path, name: &str, report: &mut impl FnMut(String)) {
    let main = dir.join("scripts/main.ncl");
    if !main.is_file() {
        return report("product skills need scripts/main.ncl (the code-mode entrypoint)".into());
    }
    let cases_dir = dir.join("tests");
    let mut cases: Vec<PathBuf> = fs::read_dir(&cases_dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "json"))
                .collect()
        })
        .unwrap_or_default();
    cases.sort();
    if cases.is_empty() {
        return report("product skills need at least one tests/<case>.json".into());
    }

    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    for case_path in cases {
        let case_name = case_path
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let mut fail = |msg: String| report(format!("case `{case_name}`: {msg}"));
        let case: Case = match fs::read_to_string(&case_path)
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
        {
            Ok(c) => c,
            Err(e) => {
                fail(e);
                continue;
            }
        };

        let mut source = PlanSource::file(&main).import_path(root.join("nickel"));
        for (field, code) in &case.inputs {
            source = source.input(field, code);
        }
        let plan = match source.load() {
            Ok(p) => p,
            Err(e) => {
                fail(e.to_string());
                continue;
            }
        };
        if plan.skill != name {
            fail(format!("plan.skill is `{}`, expected `{name}`", plan.skill));
        }
        if !plan.dry_run {
            fail("test cases must be dry runs".into());
            continue;
        }
        // A fresh in-memory host per case: fixed clock, seeded randomness, empty stores, so
        // golden reports are reproducible and cases don't see each other's writes.
        let run = match execute(&plan, &Registry::with_core(), &mut Host::memory()) {
            Ok(r) => serde_json::to_value(r).expect("reports serialize"),
            Err(e) => {
                fail(e.to_string());
                continue;
            }
        };
        if update {
            let updated = serde_json::json!({ "inputs": case.inputs, "expect": run });
            let text = serde_json::to_string_pretty(&updated).unwrap() + "\n";
            if let Err(e) = fs::write(&case_path, text) {
                fail(e.to_string());
            }
        } else if run != case.expect {
            fail(format!(
                "report differs from the golden file (UPDATE_GOLDEN=1 to accept):\n{}",
                serde_json::to_string_pretty(&run).unwrap()
            ));
        }
    }
}

fn run_smoke_wasm() -> ExitCode {
    use std::process::Command;

    let root = root();
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let built = Command::new(cargo)
        .current_dir(&root)
        .args([
            "build",
            "--release",
            "-p",
            "rdx-cli",
            "--target",
            "wasm32-wasip2",
        ])
        .status();
    if !built.is_ok_and(|s| s.success()) {
        eprintln!("FAIL building rdx for wasm32-wasip2");
        return ExitCode::FAILURE;
    }
    let wasm = root.join("target/wasm32-wasip2/release/rdx.wasm");
    let state = std::env::temp_dir().join(format!("rdx-smoke-wasm-{}", std::process::id()));
    let _ = fs::remove_dir_all(&state);
    if let Err(e) = fs::create_dir_all(&state) {
        eprintln!("FAIL {}: {e}", state.display());
        return ExitCode::FAILURE;
    }
    let mut ok = true;
    for execute in [false, true] {
        let mut cmd = Command::new("wasmtime");
        cmd.arg("run")
            .arg("--dir")
            .arg(format!("{}::/repo", root.display()))
            .arg("--dir")
            .arg(format!("{}::/state", state.display()))
            .arg(&wasm)
            .args([
                "run",
                "plan-smoke-test",
                "--root",
                "/repo",
                "--state",
                "/state",
            ])
            .args(["--input", r#"host="wasmtime""#]);
        if execute {
            cmd.arg("--execute");
        }
        let label = if execute { "execute" } else { "dry run" };
        let out = match cmd.output() {
            Ok(out) => out,
            Err(e) => {
                eprintln!("FAIL running wasmtime (is it installed?): {e}");
                return ExitCode::FAILURE;
            }
        };
        let report: Option<Value> = serde_json::from_slice(&out.stdout).ok();
        let report_ok = report
            .as_ref()
            .is_some_and(|r| r["ok"] == true && r["dry_run"] == !execute);
        if out.status.success() && report_ok {
            println!("ok   rdx.wasm plan-smoke-test ({label})");
        } else {
            ok = false;
            eprintln!(
                "FAIL rdx.wasm plan-smoke-test ({label}): {}\n{}{}",
                out.status,
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
    let _ = fs::remove_dir_all(&state);
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn run_check_wit() -> ExitCode {
    let path = root().join("wit/rdx.wit");
    match wit_capabilities(&path) {
        Err(e) => {
            eprintln!("FAIL {}: {e}", path.display());
            ExitCode::FAILURE
        }
        Ok(found) => {
            let problems = compare_wit(&found, rdx_host::WIT_CAPABILITIES);
            if problems.is_empty() {
                println!("ok   wit/rdx.wit matches rdx-host");
                ExitCode::SUCCESS
            } else {
                for p in problems {
                    eprintln!("FAIL {p}");
                }
                ExitCode::FAILURE
            }
        }
    }
}

/// Interfaces imported by world `core` that declare functions, with their function names.
fn wit_capabilities(path: &Path) -> Result<BTreeMap<String, Vec<String>>, String> {
    let mut resolve = wit_parser::Resolve::default();
    resolve.push_file(path).map_err(|e| format!("{e:#}"))?;
    let (_, world) = resolve
        .worlds
        .iter()
        .find(|(_, w)| w.name == "core")
        .ok_or("no world `core`")?;
    let mut out = BTreeMap::new();
    for item in world.imports.values() {
        if let wit_parser::WorldItem::Interface { id, .. } = item {
            let iface = &resolve.interfaces[*id];
            if iface.functions.is_empty() {
                continue;
            }
            let name = iface
                .name
                .clone()
                .ok_or("inline interface in world `core`")?;
            let mut funcs: Vec<String> = iface.functions.keys().cloned().collect();
            funcs.sort();
            out.insert(name, funcs);
        }
    }
    Ok(out)
}

fn compare_wit(found: &BTreeMap<String, Vec<String>>, rust: &[(&str, &[&str])]) -> Vec<String> {
    let mut problems = Vec::new();
    for (iface, funcs) in rust {
        let mut expected: Vec<String> = funcs.iter().map(|f| f.to_string()).collect();
        expected.sort();
        match found.get(*iface) {
            None => problems.push(format!(
                "rdx-host has `{iface}`, but world `core` in wit/rdx.wit does not import it"
            )),
            Some(actual) if *actual != expected => problems.push(format!(
                "`{iface}`: wit/rdx.wit has {actual:?}, rdx-host has {expected:?}"
            )),
            Some(_) => {}
        }
    }
    for iface in found.keys() {
        if !rust.iter().any(|(name, _)| name == iface) {
            problems.push(format!(
                "world `core` imports `{iface}`, which rdx-host::WIT_CAPABILITIES does not list"
            ));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fm(name: &str) -> Frontmatter {
        Frontmatter {
            name: name.into(),
            description: "Does a thing. Use when a thing is needed.".into(),
            license: None,
            compatibility: None,
            metadata: BTreeMap::new(),
            allowed_tools: None,
        }
    }

    #[test]
    fn names_follow_the_spec() {
        for good in ["pdf-processing", "a", "x9"] {
            assert!(spec_problems(&fm(good), good, 10).is_empty(), "{good}");
        }
        for bad in [
            "PDF-Processing",
            "-pdf",
            "pdf-",
            "pdf--processing",
            "pdf_processing",
        ] {
            assert!(!spec_problems(&fm(bad), bad, 10).is_empty(), "{bad}");
        }
    }

    #[test]
    fn name_must_match_directory() {
        assert!(!spec_problems(&fm("one"), "two", 10).is_empty());
    }

    #[test]
    fn the_repo_wit_matches_rdx_host() {
        let found = wit_capabilities(&root().join("wit/rdx.wit")).unwrap();
        assert_eq!(
            compare_wit(&found, rdx_host::WIT_CAPABILITIES),
            Vec::<String>::new()
        );
    }

    #[test]
    fn wit_drift_is_reported() {
        let found = BTreeMap::from([
            ("kv".to_owned(), vec!["get".to_owned(), "put".to_owned()]),
            ("extra".to_owned(), vec!["f".to_owned()]),
        ]);
        let problems = compare_wit(
            &found,
            &[("kv", &["get", "put", "delete"]), ("gone", &["x"])],
        );
        assert_eq!(problems.len(), 3, "{problems:?}");
    }

    #[test]
    fn long_bodies_are_flagged() {
        assert!(!spec_problems(&fm("a"), "a", 501).is_empty());
    }

    #[test]
    fn parses_frontmatter() {
        let (f, lines) = parse_skill_md(
            "---\nname: a\ndescription: d\nmetadata:\n  rdx-kind: dev\n---\n\n# A\nbody\n",
        )
        .unwrap();
        assert_eq!(f.name, "a");
        assert_eq!(f.metadata["rdx-kind"], "dev");
        assert_eq!(lines, 3);
    }
}
