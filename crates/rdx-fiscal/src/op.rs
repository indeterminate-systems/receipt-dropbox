//! `fiscal.check`: the op plans use. Its constructor is in `nickel/rdx/ops/fiscal.ncl`.

use rdx_plan::{Op, OpError, Registry, RunContext};
use serde::Deserialize;
use serde_json::Value;

use crate::{Expense, Policy, check};

/// Registers the fiscal ops with a host's registry.
pub fn register(r: &mut Registry) {
    r.register(CheckOp);
}

fn err(message: impl Into<String>) -> OpError {
    OpError::new(format!("fiscal.check: {}", message.into()))
}

/// `fiscal.check { policy, expenses }`. No effects and no capabilities: a dry run shows the real
/// assessments.
pub struct CheckOp;

impl Op for CheckOp {
    fn id(&self) -> &'static str {
        "fiscal.check"
    }

    fn run(&self, args: &Value, _: &mut RunContext<'_>) -> Result<Value, OpError> {
        let policy = Policy::deserialize(args.get("policy").unwrap_or(&Value::Null))
            .map_err(|e| err(format!("invalid `policy`: {e}")))?;
        let Some(list) = args.get("expenses").and_then(Value::as_array) else {
            return Err(err("needs `expenses`, an array"));
        };
        let expenses = list
            .iter()
            .enumerate()
            .map(|(i, e)| expense(e).map_err(|m| err(format!("expense {i}: {m}"))))
            .collect::<Result<Vec<_>, _>>()?;
        let report = check(&policy, &expenses).map_err(|e| err(e.to_string()))?;
        Ok(serde_json::to_value(report).expect("reports serialize"))
    }
}

/// An expense, where `receipt` may also be a `receipt.extract` output (`{ receipt, status, … }`),
/// so a plan can pass `rdx.ref` to that step.
fn expense(value: &Value) -> Result<Expense, String> {
    let mut value = value.clone();
    if let Some(r) = value.get_mut("receipt") {
        let extracted = r.get("status").is_some() && r.get("source").is_some();
        if extracted {
            let inner = r.get("receipt").cloned().unwrap_or(Value::Null);
            if inner.is_null() {
                let missing = r.get("missing").cloned().unwrap_or(Value::Null);
                return Err(format!(
                    "`receipt` is a receipt.extract output that needs review (missing {missing}), so there is no receipt to check yet"
                ));
            }
            *r = inner;
        } else if r.is_null() {
            return Err(
                "`receipt` is null: the step it refers to was skipped or produced nothing".into(),
            );
        }
    }
    Expense::deserialize(&value).map_err(|e| format!("invalid: {e}"))
}
