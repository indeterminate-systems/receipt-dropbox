use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::plan::{Plan, PlanError, as_ref};

/// One operation the executor can run. Implementations live in the crate that owns the domain
/// (statement parsing in `rdx-statement`, and so on) and are registered by the host.
pub trait Op {
    /// The id plans use, such as `core.digest`.
    fn id(&self) -> &'static str;

    /// Whether the op changes anything outside the run (storage, mail, the ledger). Ops with
    /// effects are skipped in a dry run.
    fn effects(&self) -> bool {
        false
    }

    /// Runs the op. `args` has every `{"ref": …}` already replaced by the referenced output.
    fn run(&self, args: &Value, cx: &mut RunContext) -> Result<Value, OpError>;
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct OpError {
    pub message: String,
}

impl OpError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// What ops can reach while running. Host capabilities (blob store, kv, mail) are added here as
/// their crates arrive; see docs/architecture.md.
#[derive(Debug, Default)]
pub struct RunContext {
    pub notes: Vec<String>,
}

/// The ops a host offers.
#[derive(Default)]
pub struct Registry {
    ops: HashMap<&'static str, Box<dyn Op>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// A registry with the built-in `core.*` ops.
    pub fn with_core() -> Self {
        let mut r = Self::new();
        crate::ops::register_core(&mut r);
        r
    }

    pub fn register(&mut self, op: impl Op + 'static) -> &mut Self {
        self.ops.insert(op.id(), Box::new(op));
        self
    }

    pub fn get(&self, id: &str) -> Option<&dyn Op> {
        self.ops.get(id).map(|b| b.as_ref())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Done,
    /// An op with effects in a dry run.
    Skipped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub op: String,
    pub status: StepStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The record of one run, written to the audit log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunReport {
    pub skill: String,
    pub plan_digest: String,
    pub dry_run: bool,
    pub ok: bool,
    pub steps: Vec<StepReport>,
    pub notes: Vec<String>,
}

/// Validates a plan against itself and the registry, then runs it in order. A plan that fails
/// validation runs nothing. A failing step stops the run; the report says which.
pub fn execute(plan: &Plan, registry: &Registry) -> Result<RunReport, PlanError> {
    plan.validate()?;
    if let Some(step) = plan.steps.iter().find(|s| registry.get(&s.op).is_none()) {
        return Err(PlanError::UnknownOp(step.op.clone()));
    }

    let mut cx = RunContext::default();
    let mut outputs: BTreeMap<&str, Value> = BTreeMap::new();
    let mut steps = Vec::with_capacity(plan.steps.len());
    let mut ok = true;

    for step in &plan.steps {
        let op = registry.get(&step.op).expect("checked above");
        let mut report = StepReport {
            id: step.id.clone(),
            op: step.op.clone(),
            status: StepStatus::Done,
            output: None,
            output_digest: None,
            error: None,
        };
        if plan.dry_run && op.effects() {
            report.status = StepStatus::Skipped;
            // Later steps that refer to a skipped step see null.
            if let Some(id) = &step.id {
                outputs.insert(id, Value::Null);
            }
            steps.push(report);
            continue;
        }
        let args = resolve(&step.args, &outputs);
        match op.run(&args, &mut cx) {
            Ok(output) => {
                report.output_digest = Some(crate::digest(&output));
                if let Some(id) = &step.id {
                    outputs.insert(id, output.clone());
                }
                report.output = Some(output);
                steps.push(report);
            }
            Err(e) => {
                report.status = StepStatus::Failed;
                report.error = Some(e.message);
                steps.push(report);
                ok = false;
                break;
            }
        }
    }

    Ok(RunReport {
        skill: plan.skill.clone(),
        plan_digest: plan.digest(),
        dry_run: plan.dry_run,
        ok,
        steps,
        notes: cx.notes,
    })
}

fn resolve(value: &Value, outputs: &BTreeMap<&str, Value>) -> Value {
    if let Some(target) = as_ref(value) {
        // validate() guarantees the target is an earlier step.
        return outputs.get(target).cloned().unwrap_or(Value::Null);
    }
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), resolve(v, outputs)))
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(|v| resolve(v, outputs)).collect()),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Send;
    impl Op for Send {
        fn id(&self) -> &'static str {
            "test.send"
        }
        fn effects(&self) -> bool {
            true
        }
        fn run(&self, _: &Value, cx: &mut RunContext) -> Result<Value, OpError> {
            cx.notes.push("sent".into());
            Ok(json!("sent"))
        }
    }

    struct Fail;
    impl Op for Fail {
        fn id(&self) -> &'static str {
            "test.fail"
        }
        fn run(&self, _: &Value, _: &mut RunContext) -> Result<Value, OpError> {
            Err(OpError::new("boom"))
        }
    }

    fn registry() -> Registry {
        let mut r = Registry::with_core();
        r.register(Send).register(Fail);
        r
    }

    fn plan(dry_run: bool, steps: Value) -> Plan {
        serde_json::from_value(json!({
            "version": 1, "skill": "t", "goal": "g", "dry_run": dry_run, "steps": steps
        }))
        .unwrap()
    }

    #[test]
    fn dry_run_skips_effects_and_real_run_performs_them() {
        let steps = json!([{ "op": "test.send" }]);
        let dry = execute(&plan(true, steps.clone()), &registry()).unwrap();
        assert_eq!(dry.steps[0].status, StepStatus::Skipped);
        assert!(dry.notes.is_empty());

        let real = execute(&plan(false, steps), &registry()).unwrap();
        assert_eq!(real.steps[0].status, StepStatus::Done);
        assert_eq!(real.notes, vec!["sent"]);
    }

    #[test]
    fn refs_pass_outputs_forward() {
        let report = execute(
            &plan(
                true,
                json!([
                    { "id": "n", "op": "core.note", "args": { "message": "hello" } },
                    { "id": "d", "op": "core.digest", "args": { "value": { "ref": "n" } } }
                ]),
            ),
            &registry(),
        )
        .unwrap();
        let note_output = report.steps[0].output.clone().unwrap();
        assert_eq!(
            report.steps[1].output,
            Some(json!(crate::digest(&note_output)))
        );
    }

    #[test]
    fn unknown_ops_fail_before_anything_runs() {
        let err = execute(
            &plan(false, json!([{ "op": "test.send" }, { "op": "nope" }])),
            &registry(),
        )
        .unwrap_err();
        assert_eq!(err, PlanError::UnknownOp("nope".into()));
    }

    #[test]
    fn a_failing_step_stops_the_run() {
        let report = execute(
            &plan(
                true,
                json!([
                    { "op": "test.fail" },
                    { "op": "core.note", "args": { "message": "never" } }
                ]),
            ),
            &registry(),
        )
        .unwrap();
        assert!(!report.ok);
        assert_eq!(report.steps.len(), 1);
        assert_eq!(report.steps[0].error.as_deref(), Some("boom"));
    }

    #[test]
    fn report_carries_the_plan_digest() {
        let p = plan(true, json!([]));
        assert_eq!(execute(&p, &registry()).unwrap().plan_digest, p.digest());
    }
}
