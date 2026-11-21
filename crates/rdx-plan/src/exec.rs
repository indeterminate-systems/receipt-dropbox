use std::collections::{BTreeMap, HashMap};

use rdx_host::{Capability, Host};
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

    /// The host capabilities the op uses. A plan with this op fails validation on a host that
    /// lacks any of them, before any step runs.
    fn needs(&self) -> &'static [Capability] {
        &[]
    }

    /// Runs the op. `args` has every `{"ref": …}` already replaced by the referenced output.
    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError>;
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

impl From<rdx_host::CapabilityError> for OpError {
    fn from(e: rdx_host::CapabilityError) -> Self {
        Self::new(e.message)
    }
}

/// What ops can reach while running: the run's notes and the host's capabilities. An op only
/// touches the capabilities it lists in [`Op::needs`], which the executor has checked exist.
pub struct RunContext<'h> {
    pub notes: Vec<String>,
    pub host: &'h mut Host,
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

    /// The ids of every registered op, sorted.
    pub fn ids(&self) -> Vec<&'static str> {
        let mut ids: Vec<_> = self.ops.keys().copied().collect();
        ids.sort_unstable();
        ids
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
    /// For a skipped step only: the arguments it would have run with, so a dry run shows what
    /// would happen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
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

/// Validates a plan against itself, the registry and the host, then runs it in order. A plan that
/// fails validation runs nothing. A failing step stops the run; the report says which.
///
/// This doesn't write to the audit log; [`run`](crate::run) does both.
pub fn execute(plan: &Plan, registry: &Registry, host: &mut Host) -> Result<RunReport, PlanError> {
    plan.validate()?;
    for step in &plan.steps {
        let op = registry
            .get(&step.op)
            .ok_or_else(|| PlanError::UnknownOp(step.op.clone()))?;
        if let Some(missing) = op.needs().iter().find(|c| !host.offers(**c)) {
            return Err(PlanError::MissingCapability {
                op: step.op.clone(),
                capability: *missing,
            });
        }
    }

    let mut cx = RunContext {
        notes: Vec::new(),
        host,
    };
    let mut outputs: BTreeMap<&str, Value> = BTreeMap::new();
    let mut steps = Vec::with_capacity(plan.steps.len());
    let mut ok = true;

    for step in &plan.steps {
        let op = registry.get(&step.op).expect("checked above");
        let mut report = StepReport {
            id: step.id.clone(),
            op: step.op.clone(),
            status: StepStatus::Done,
            args: None,
            output: None,
            output_digest: None,
            error: None,
        };
        let args = resolve(&step.args, &outputs);
        if plan.dry_run && op.effects() {
            report.status = StepStatus::Skipped;
            report.args = Some(args);
            // Later steps that refer to a skipped step see null.
            if let Some(id) = &step.id {
                outputs.insert(id, Value::Null);
            }
            steps.push(report);
            continue;
        }
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
        fn run(&self, _: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
            cx.notes.push("sent".into());
            Ok(json!("sent"))
        }
    }

    struct Fail;
    impl Op for Fail {
        fn id(&self) -> &'static str {
            "test.fail"
        }
        fn run(&self, _: &Value, _: &mut RunContext<'_>) -> Result<Value, OpError> {
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
        let dry = execute(&plan(true, steps.clone()), &registry(), &mut Host::new()).unwrap();
        assert_eq!(dry.steps[0].status, StepStatus::Skipped);
        assert!(dry.notes.is_empty());

        let real = execute(&plan(false, steps), &registry(), &mut Host::new()).unwrap();
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
            &mut Host::new(),
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
            &mut Host::new(),
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
            &mut Host::new(),
        )
        .unwrap();
        assert!(!report.ok);
        assert_eq!(report.steps.len(), 1);
        assert_eq!(report.steps[0].error.as_deref(), Some("boom"));
    }

    #[test]
    fn report_carries_the_plan_digest() {
        let p = plan(true, json!([]));
        assert_eq!(
            execute(&p, &registry(), &mut Host::new())
                .unwrap()
                .plan_digest,
            p.digest()
        );
    }
}
