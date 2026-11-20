use serde::Deserialize;
use serde_json::Value;

use rdx_model::{Match, Receipt, Statement};
use rdx_plan::{Op, OpError, Registry, RunContext};

use crate::{Policy, reconcile};

/// Registers the matching ops with a host's registry.
pub fn register(registry: &mut Registry) {
    registry.register(Propose);
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposeArgs {
    statement: Statement,
    receipts: Vec<Receipt>,
    #[serde(default)]
    decided: Vec<Match>,
    policy: Policy,
}

/// `matching.propose { statement, receipts, decided, policy }`: proposed matches for one
/// statement. No effects and no capabilities: it only reads its arguments, so a dry run shows the
/// real proposals.
struct Propose;

impl Op for Propose {
    fn id(&self) -> &'static str {
        "matching.propose"
    }

    fn run(&self, args: &Value, _: &mut RunContext<'_>) -> Result<Value, OpError> {
        let args = ProposeArgs::deserialize(args)
            .map_err(|e| OpError::new(format!("matching.propose: invalid arguments: {e}")))?;
        let result = reconcile(&args.statement, &args.receipts, &args.decided, &args.policy)
            .map_err(|e| OpError::new(format!("matching.propose: {e}")))?;
        Ok(serde_json::to_value(result).expect("reconciliations serialize"))
    }
}
