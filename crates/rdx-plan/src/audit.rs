//! Recording runs in the host's audit log.
//!
//! Each line is the canonical JSON of `{ "digest": d, "entry": e }`, where `e` is
//! `{ "at_unix_ms", "prev", "report" }`, `prev` is the previous line's digest (null for the first)
//! and `d` is `digest(e)`. Changing, removing or reordering a line breaks the chain, which
//! [`verify`] reports. Piece 12 of the roadmap links ledger entries to these digests.

use rdx_host::{CapabilityError, Host};
use serde_json::{Value, json};

use crate::exec::{Registry, RunReport, execute};
use crate::plan::{Plan, PlanError};

/// A run's report and, when the host keeps an audit log, the digest of its entry.
#[derive(Debug, Clone, PartialEq)]
pub struct RunOutcome {
    pub report: RunReport,
    pub audit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RunError {
    #[error(transparent)]
    Plan(#[from] PlanError),
    /// The run happened but could not be recorded. Treat effects as done and fix the log.
    #[error("the run finished but was not recorded in the audit log: {0}")]
    Audit(String),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuditError {
    #[error("audit log line {line} is not an audit entry")]
    Parse { line: usize },
    #[error("audit log line {line} does not match its digest: it was changed")]
    Digest { line: usize },
    #[error(
        "audit log line {line} does not follow the line before it: lines were removed or reordered"
    )]
    Chain { line: usize },
}

/// Executes a plan ([`execute`]) and records the report in the host's audit log, if it has one.
/// A plan that fails validation runs nothing and records nothing.
pub fn run(plan: &Plan, registry: &Registry, host: &mut Host) -> Result<RunOutcome, RunError> {
    let report = execute(plan, registry, host)?;
    let audit = record(&report, host).map_err(|e| RunError::Audit(e.message))?;
    Ok(RunOutcome { report, audit })
}

fn record(report: &RunReport, host: &mut Host) -> Result<Option<String>, CapabilityError> {
    let at = match &host.clock {
        Some(clock) => json!(clock.now_unix_ms()?),
        None => Value::Null,
    };
    let Some(log) = host.audit_log.as_mut() else {
        return Ok(None);
    };
    let prev = match log.last()? {
        None => Value::Null,
        Some(line) => {
            let (digest, _) = parse_line(&line).ok_or_else(|| {
                CapabilityError::new(
                    "the audit log's last line is not an audit entry; run `rdx audit verify`",
                )
            })?;
            Value::String(digest)
        }
    };
    let entry = json!({
        "at_unix_ms": at,
        "prev": prev,
        "report": serde_json::to_value(report).expect("reports serialize"),
    });
    let digest = crate::digest(&entry);
    log.append(&crate::canonical_json(
        &json!({ "digest": digest, "entry": entry }),
    ))?;
    Ok(Some(digest))
}

fn parse_line(line: &str) -> Option<(String, Value)> {
    let mut v: Value = serde_json::from_str(line).ok()?;
    let digest = v.get("digest")?.as_str()?.to_owned();
    let entry = v.get_mut("entry")?.take();
    Some((digest, entry))
}

/// Checks every line's digest and that each links to the one before. Returns the number of
/// entries. Blank lines are ignored.
pub fn verify<'a>(lines: impl IntoIterator<Item = &'a str>) -> Result<usize, AuditError> {
    let mut prev = Value::Null;
    let mut count = 0;
    for (i, line) in lines.into_iter().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let n = i + 1;
        let (digest, entry) = parse_line(line).ok_or(AuditError::Parse { line: n })?;
        if crate::digest(&entry) != digest {
            return Err(AuditError::Digest { line: n });
        }
        if entry.get("prev") != Some(&prev) {
            return Err(AuditError::Chain { line: n });
        }
        prev = Value::String(digest);
        count += 1;
    }
    Ok(count)
}
