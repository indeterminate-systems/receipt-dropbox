//! Built-in `core.*` ops. Their constructors and argument contracts are in
//! `nickel/rdx/api.ncl` under `ops.core`.

use serde_json::{Value, json};

use crate::exec::{Op, OpError, Registry, RunContext};

pub(crate) fn register_core(r: &mut Registry) {
    r.register(Note).register(DigestOp);
}

/// `core.note { message }`: adds a message to the run report.
struct Note;

impl Op for Note {
    fn id(&self) -> &'static str {
        "core.note"
    }

    fn run(&self, args: &Value, cx: &mut RunContext) -> Result<Value, OpError> {
        let message = args
            .get("message")
            .and_then(Value::as_str)
            .ok_or_else(|| OpError::new("core.note needs a string `message`"))?;
        cx.notes.push(message.to_owned());
        Ok(json!({ "message": message }))
    }
}

/// `core.digest { value }`: `sha256:` digest of the value's canonical JSON.
struct DigestOp;

impl Op for DigestOp {
    fn id(&self) -> &'static str {
        "core.digest"
    }

    fn run(&self, args: &Value, _: &mut RunContext) -> Result<Value, OpError> {
        let value = args
            .get("value")
            .ok_or_else(|| OpError::new("core.digest needs a `value`"))?;
        Ok(Value::String(crate::digest(value)))
    }
}
