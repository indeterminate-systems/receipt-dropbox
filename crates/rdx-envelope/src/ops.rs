//! `envelope.*` ops. Their constructors and argument contracts are in `nickel/rdx/api.ncl`.
//!
//! Both are pure: envelopes travel through plans as `ur:envelope/…` strings, and neither op needs
//! a host capability. Sealing needs the supporter's key, so its op comes with keys (piece 8).

use rdx_model::Match;
use rdx_plan::{Op, OpError, Registry, RunContext};
use serde_json::{Value, json};

use crate::{from_ur, public_key_from_ur, redact, root_digest, to_ur, verify};

/// Adds the `envelope.*` ops to a host's registry.
pub fn register(r: &mut Registry) -> &mut Registry {
    r.register(Redact).register(Verify)
}

fn str_arg<'a>(op: &str, args: &'a Value, name: &str) -> Result<&'a str, OpError> {
    args.get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| OpError::new(format!("{op} needs a string `{name}`")))
}

/// `envelope` is a `ur:envelope/…` string, or the output of an earlier `envelope.*` step (a record
/// with an `envelope` field), so plans can chain steps with a plain `ref`.
fn envelope_arg(op: &str, args: &Value) -> Result<bc_envelope::Envelope, OpError> {
    let value = args.get("envelope");
    let ur = value
        .and_then(Value::as_str)
        .or_else(|| value?.get("envelope")?.as_str())
        .ok_or_else(|| {
            OpError::new(format!(
                "{op} needs `envelope`: a `ur:envelope/…` string, or the output of an earlier \
                 `envelope.*` step"
            ))
        })?;
    from_ur(ur).map_err(failed(op))
}

fn failed(op: &str) -> impl Fn(crate::Error) -> OpError + '_ {
    move |e| OpError::new(format!("{op}: {e}"))
}

/// `envelope.redact { envelope, matches }`: elides the lines confirmed matches don't fully cover.
struct Redact;

impl Op for Redact {
    fn id(&self) -> &'static str {
        "envelope.redact"
    }

    fn run(&self, args: &Value, _: &mut RunContext<'_>) -> Result<Value, OpError> {
        let id = self.id();
        let envelope = envelope_arg(id, args)?;
        let matches: Vec<Match> =
            serde_json::from_value(args.get("matches").cloned().unwrap_or(json!([])))
                .map_err(|e| OpError::new(format!("{id}: `matches`: {e}")))?;
        let r = redact(&envelope, &matches).map_err(failed(id))?;
        Ok(json!({
            "envelope": to_ur(&r.envelope),
            "root": root_digest(&r.envelope),
            "shown": r.shown,
            "redacted": r.redacted,
            "review": r.review,
        }))
    }
}

/// `envelope.verify { envelope, signer }`: checks the signature and reads the viewable statement.
struct Verify;

impl Op for Verify {
    fn id(&self) -> &'static str {
        "envelope.verify"
    }

    fn run(&self, args: &Value, _: &mut RunContext<'_>) -> Result<Value, OpError> {
        let id = self.id();
        let envelope = envelope_arg(id, args)?;
        let signer = public_key_from_ur(str_arg(id, args, "signer")?).map_err(failed(id))?;
        let view = verify(&envelope, &signer).map_err(failed(id))?;
        serde_json::to_value(view).map_err(|e| OpError::new(format!("{id}: {e}")))
    }
}
