//! `rdx`: the local host. It runs a skill's code-mode entrypoint (`scripts/main.ncl`) against
//! directory-backed capabilities and records every run in an audit log.
//!
//! The same source builds natively and for `wasm32-wasip2`, so the core can be run as
//! WebAssembly under wasmtime: `wasmtime run --dir . rdx.wasm run plan-smoke-test`.

use std::io::Write;
use std::path::{Path, PathBuf};

use rdx_host::local;
use rdx_plan::{PlanSource, Registry, RunError, StepStatus, audit};

pub const USAGE: &str = "\
rdx: run Receipt Dropbox skills (code mode)

Usage:
  rdx run <SKILL> [--input FIELD=VALUE]... [--execute]
  rdx audit verify
  rdx --help

SKILL is a directory with scripts/main.ncl, or the name of one in .agents/skills/.
VALUE is a Nickel expression: strings need quotes, as in --input host='\"ci\"'.

Runs are dry runs: ops with effects are skipped and the report shows what they would do.
--execute performs them. Review a dry run's report before executing.

The report is JSON on stdout. Every run is appended to the audit log in the state directory.

Options:
  --root DIR    repository root (default: the nearest directory above with nickel/rdx/api.ncl)
  --state DIR   where the local host keeps blobs, kv, outbox and audit.jsonl (default: ROOT/.rdx)

Exit status: 0 ok, 1 a step or the audit check failed, 2 the plan or command was rejected
before anything ran, 3 the run finished but could not be recorded.
";

/// Every op this host offers: the built-in ones and each domain crate's. `cargo xtask check` runs
/// skill tests with the same registry, so register a new domain's ops here once.
pub fn registry() -> Registry {
    let mut registry = Registry::with_core();
    rdx_receipt::register(&mut registry);
    registry
}

/// Runs the command line and returns the exit status.
pub fn main(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    match parse(args) {
        Ok(Command::Help) => {
            let _ = out.write_all(USAGE.as_bytes());
            0
        }
        Ok(cmd) => match dispatch(cmd, out, err) {
            Ok(code) => code,
            Err(Failure(code, message)) => {
                let _ = writeln!(err, "rdx: {message}");
                code
            }
        },
        Err(message) => {
            let _ = writeln!(err, "rdx: {message}\n\n{USAGE}");
            2
        }
    }
}

struct Failure(u8, String);

fn rejected(message: impl ToString) -> Failure {
    Failure(2, message.to_string())
}

enum Command {
    Help,
    Run {
        skill: String,
        inputs: Vec<(String, String)>,
        execute: bool,
        dirs: Dirs,
    },
    AuditVerify {
        dirs: Dirs,
    },
}

#[derive(Default)]
struct Dirs {
    root: Option<PathBuf>,
    state: Option<PathBuf>,
}

fn parse(args: &[String]) -> Result<Command, String> {
    let mut positional = Vec::new();
    let mut inputs = Vec::new();
    let mut execute = false;
    let mut dirs = Dirs::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |flag: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match arg.as_str() {
            "-h" | "--help" | "help" => return Ok(Command::Help),
            "--execute" => execute = true,
            "--input" | "-i" => {
                let kv = value(arg)?;
                let (field, code) = kv
                    .split_once('=')
                    .filter(|(f, _)| !f.is_empty())
                    .ok_or_else(|| format!("--input takes FIELD=VALUE, got `{kv}`"))?;
                inputs.push((field.to_owned(), code.to_owned()));
            }
            "--root" => dirs.root = Some(value(arg)?.into()),
            "--state" => dirs.state = Some(value(arg)?.into()),
            flag if flag.starts_with('-') => return Err(format!("unknown option `{flag}`")),
            _ => positional.push(arg.as_str()),
        }
    }
    match positional.as_slice() {
        ["run", skill] => Ok(Command::Run {
            skill: (*skill).to_owned(),
            inputs,
            execute,
            dirs,
        }),
        ["run"] => Err("run needs a skill".into()),
        ["audit", "verify"] => Ok(Command::AuditVerify { dirs }),
        [] => Err("no command".into()),
        other => Err(format!("unknown command `{}`", other.join(" "))),
    }
}

fn find_root(explicit: Option<PathBuf>) -> Result<PathBuf, Failure> {
    let marker = Path::new("nickel/rdx/api.ncl");
    if let Some(root) = explicit {
        return if root.join(marker).is_file() {
            Ok(root)
        } else {
            Err(rejected(format!(
                "{} has no {}",
                root.display(),
                marker.display()
            )))
        };
    }
    // Relative paths first: under WASI the current directory may only be reachable as `.`.
    let mut dir = PathBuf::from(".");
    for _ in 0..32 {
        if dir.join(marker).is_file() {
            return Ok(dir);
        }
        dir = dir.join("..");
    }
    Err(rejected(
        "not inside a Receipt Dropbox repository (no nickel/rdx/api.ncl above here); pass --root",
    ))
}

fn state_dir(root: &Path, explicit: Option<PathBuf>) -> PathBuf {
    explicit.unwrap_or_else(|| root.join(".rdx"))
}

fn skill_dir(root: &Path, skill: &str) -> Result<PathBuf, Failure> {
    let as_path = PathBuf::from(skill);
    let dir = if skill.contains('/') || as_path.is_dir() {
        as_path
    } else {
        root.join(".agents/skills").join(skill)
    };
    if dir.join("scripts/main.ncl").is_file() {
        return Ok(dir);
    }
    let mut known: Vec<String> = std::fs::read_dir(root.join(".agents/skills"))
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().join("scripts/main.ncl").is_file())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    known.sort();
    Err(rejected(format!(
        "no skill `{skill}` ({} has no scripts/main.ncl). Skills with an entrypoint: {}",
        dir.display(),
        if known.is_empty() {
            "none".to_owned()
        } else {
            known.join(", ")
        }
    )))
}

fn dispatch(cmd: Command, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, Failure> {
    match cmd {
        Command::Help => unreachable!("handled by main"),
        Command::Run {
            skill,
            inputs,
            execute,
            dirs,
        } => run(&skill, inputs, execute, dirs, out, err),
        Command::AuditVerify { dirs } => audit_verify(dirs, out, err),
    }
}

fn run(
    skill: &str,
    inputs: Vec<(String, String)>,
    execute: bool,
    dirs: Dirs,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Failure> {
    let root = find_root(dirs.root)?;
    let dir = skill_dir(&root, skill)?;

    let mut source =
        PlanSource::file(dir.join("scripts/main.ncl")).import_path(root.join("nickel"));
    for (field, code) in inputs {
        source = source.input(field, code);
    }
    let mut plan = source.load().map_err(rejected)?;
    // The command line decides, not the plan: effects need an explicit --execute.
    plan.dry_run = !execute;

    let state = state_dir(&root, dirs.state);
    let mut host = local::host(&state).map_err(rejected)?;
    let outcome = match rdx_plan::run(&plan, &registry(), &mut host) {
        Ok(outcome) => outcome,
        Err(RunError::Plan(e)) => return Err(rejected(e)),
        Err(e @ RunError::Audit(_)) => return Err(Failure(3, e.to_string())),
    };

    let report = &outcome.report;
    let json = serde_json::to_string_pretty(report).expect("reports serialize");
    let _ = writeln!(out, "{json}");

    let skipped = report
        .steps
        .iter()
        .filter(|s| s.status == StepStatus::Skipped)
        .count();
    if let Some(failed) = report.steps.iter().find(|s| s.status == StepStatus::Failed) {
        let _ = writeln!(
            err,
            "rdx: step `{}` ({}) failed: {}",
            failed.id.as_deref().unwrap_or("-"),
            failed.op,
            failed.error.as_deref().unwrap_or("")
        );
    }
    if skipped > 0 {
        let _ = writeln!(
            err,
            "rdx: dry run: {skipped} step(s) with effects skipped; review the report, then rerun with --execute"
        );
    }
    if let Some(digest) = &outcome.audit {
        let _ = writeln!(
            err,
            "rdx: recorded in {} as {digest}",
            state.join("audit.jsonl").display()
        );
    }
    Ok(if report.ok { 0 } else { 1 })
}

fn audit_verify(dirs: Dirs, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, Failure> {
    let root = find_root(dirs.root)?;
    let path = state_dir(&root, dirs.state).join("audit.jsonl");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(rejected(format!("cannot read {}: {e}", path.display()))),
    };
    match audit::verify(text.lines()) {
        Ok(n) => {
            let _ = writeln!(out, "{}: {n} entries, chain intact", path.display());
            Ok(0)
        }
        Err(e) => {
            let _ = writeln!(err, "rdx: {}: {e}", path.display());
            Ok(1)
        }
    }
}
