//! Plans: the single typed entrypoint agents and automation use to drive Receipt Dropbox.
//!
//! A plan is a list of steps, each an op id plus JSON arguments. Arguments may contain
//! `{"ref": "<id>"}`, which the executor replaces with the output of the earlier step of that id.
//! Plans are usually written in Nickel against `nickel/rdx/api.ncl` (feature `nickel`), whose
//! contracts check them first; [`Plan::validate`] repeats the structural checks for plans that
//! arrive as JSON.
//!
//! ```
//! use rdx_plan::{Plan, Registry, execute};
//!
//! let plan: Plan = serde_json::from_str(r#"{
//!   "version": 1, "skill": "example", "goal": "Show the executor", "dry_run": true,
//!   "steps": [
//!     { "id": "hello", "op": "core.note", "args": { "message": "hi" } },
//!     { "op": "core.digest", "args": { "value": { "ref": "hello" } } }
//!   ]
//! }"#).unwrap();
//! let report = execute(&plan, &Registry::with_core()).unwrap();
//! assert_eq!(report.steps.len(), 2);
//! ```

mod canonical;
mod exec;
#[cfg(feature = "nickel")]
mod load;
mod ops;
mod plan;

pub use canonical::{canonical_json, digest};
pub use exec::{Op, OpError, Registry, RunContext, RunReport, StepReport, StepStatus, execute};
#[cfg(feature = "nickel")]
pub use load::{LoadError, PlanSource};
pub use plan::{Plan, PlanError, Step};
