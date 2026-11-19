use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The plan format version this crate understands.
pub const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub version: u32,
    pub skill: String,
    pub goal: String,
    #[serde(default = "yes")]
    pub dry_run: bool,
    pub steps: Vec<Step>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub op: String,
    #[serde(default = "empty_object")]
    pub args: Value,
}

fn empty_object() -> Value {
    Value::Object(Default::default())
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    #[error("unsupported plan version {0} (this core reads version {VERSION})")]
    Version(u32),
    #[error("invalid step id `{0}`: start with a-z, then a-z, 0-9, `_` or `-`")]
    InvalidId(String),
    #[error("duplicate step id `{0}`")]
    DuplicateId(String),
    #[error("step `{op}` refers to `{target}`, which is not an earlier step")]
    DanglingRef { op: String, target: String },
    #[error("step `{op}`: arguments must be an object")]
    ArgsNotObject { op: String },
    #[error("unknown op `{0}`")]
    UnknownOp(String),
}

impl Plan {
    /// The `sha256:` digest of this plan's canonical JSON, recorded in every run report.
    pub fn digest(&self) -> String {
        crate::digest(&serde_json::to_value(self).expect("plans always serialize"))
    }

    /// Structural checks: version, step ids, and references only to earlier steps.
    pub fn validate(&self) -> Result<(), PlanError> {
        if self.version != VERSION {
            return Err(PlanError::Version(self.version));
        }
        let mut seen = HashSet::new();
        for step in &self.steps {
            if !step.args.is_object() {
                return Err(PlanError::ArgsNotObject {
                    op: step.op.clone(),
                });
            }
            for target in refs_in(&step.args) {
                if !seen.contains(target) {
                    return Err(PlanError::DanglingRef {
                        op: step.op.clone(),
                        target: target.to_owned(),
                    });
                }
            }
            if let Some(id) = &step.id {
                if !valid_id(id) {
                    return Err(PlanError::InvalidId(id.clone()));
                }
                if !seen.insert(id.as_str()) {
                    return Err(PlanError::DuplicateId(id.clone()));
                }
            }
        }
        Ok(())
    }
}

fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    matches!(chars.next(), Some('a'..='z'))
        && chars.all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_' | '-'))
}

/// The target of a `{"ref": "<id>"}` object, if `value` is one.
pub(crate) fn as_ref(value: &Value) -> Option<&str> {
    match value {
        Value::Object(map) if map.len() == 1 => map.get("ref")?.as_str(),
        _ => None,
    }
}

fn refs_in(value: &Value) -> Vec<&str> {
    let mut out = Vec::new();
    collect_refs(value, &mut out);
    out
}

fn collect_refs<'a>(value: &'a Value, out: &mut Vec<&'a str>) {
    if let Some(target) = as_ref(value) {
        out.push(target);
        return;
    }
    match value {
        Value::Object(map) => map.values().for_each(|v| collect_refs(v, out)),
        Value::Array(items) => items.iter().for_each(|v| collect_refs(v, out)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn plan(steps: Value) -> Plan {
        serde_json::from_value(json!({
            "version": 1, "skill": "t", "goal": "g", "steps": steps
        }))
        .unwrap()
    }

    #[test]
    fn dry_run_defaults_to_true() {
        assert!(plan(json!([])).dry_run);
    }

    #[test]
    fn accepts_backward_refs() {
        let p = plan(json!([
            { "id": "a", "op": "core.note", "args": { "message": "x" } },
            { "op": "core.digest", "args": { "value": [{ "ref": "a" }] } }
        ]));
        assert_eq!(p.validate(), Ok(()));
    }

    #[test]
    fn rejects_forward_and_unknown_refs() {
        let p = plan(json!([
            { "op": "core.digest", "args": { "value": { "ref": "b" } } },
            { "id": "b", "op": "core.note", "args": { "message": "x" } }
        ]));
        assert_eq!(
            p.validate(),
            Err(PlanError::DanglingRef {
                op: "core.digest".into(),
                target: "b".into()
            })
        );
    }

    #[test]
    fn rejects_duplicate_and_malformed_ids() {
        let dup = plan(json!([
            { "id": "a", "op": "core.note", "args": { "message": "x" } },
            { "id": "a", "op": "core.note", "args": { "message": "y" } }
        ]));
        assert_eq!(dup.validate(), Err(PlanError::DuplicateId("a".into())));
        let bad = plan(json!([{ "id": "A", "op": "core.note" }]));
        assert_eq!(bad.validate(), Err(PlanError::InvalidId("A".into())));
    }

    #[test]
    fn rejects_other_versions() {
        let mut p = plan(json!([]));
        p.version = 2;
        assert_eq!(p.validate(), Err(PlanError::Version(2)));
    }
}
