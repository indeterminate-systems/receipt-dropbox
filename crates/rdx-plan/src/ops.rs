//! Built-in ops: `core.*` (pure) and the ops that reach host capabilities (`clock.*`, `kv.*`,
//! `mail.*`). Their constructors and argument contracts are in `nickel/rdx/api.ncl` under `ops`.

use rdx_host::{Capability, Message};
use serde_json::{Value, json};

use crate::exec::{Op, OpError, Registry, RunContext};

pub(crate) fn register_core(r: &mut Registry) {
    r.register(Note)
        .register(DigestOp)
        .register(ClockNow)
        .register(KvGet)
        .register(KvPut)
        .register(MailSend);
}

fn str_arg<'a>(op: &str, args: &'a Value, name: &str) -> Result<&'a str, OpError> {
    args.get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| OpError::new(format!("{op} needs a string `{name}`")))
}

/// The executor checks `needs()` before running, so a missing capability here is a bug in the
/// op's `needs()`, reported rather than panicking.
fn missing(op: &str, c: Capability) -> OpError {
    OpError::new(format!("{op} ran without the `{c}` capability"))
}

/// `core.note { message }`: adds a message to the run report.
struct Note;

impl Op for Note {
    fn id(&self) -> &'static str {
        "core.note"
    }

    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let message = str_arg(self.id(), args, "message")?;
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

    fn run(&self, args: &Value, _: &mut RunContext<'_>) -> Result<Value, OpError> {
        let value = args
            .get("value")
            .ok_or_else(|| OpError::new("core.digest needs a `value`"))?;
        Ok(Value::String(crate::digest(value)))
    }
}

/// `clock.now {}`: `{ unix_ms }` from the host clock.
struct ClockNow;

impl Op for ClockNow {
    fn id(&self) -> &'static str {
        "clock.now"
    }

    fn needs(&self) -> &'static [Capability] {
        &[Capability::Clock]
    }

    fn run(&self, _: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let clock = cx
            .host
            .clock
            .as_ref()
            .ok_or_else(|| missing(self.id(), Capability::Clock))?;
        Ok(json!({ "unix_ms": clock.now_unix_ms()? }))
    }
}

/// `kv.get { key }`: the JSON value stored under `key`, or null.
struct KvGet;

impl Op for KvGet {
    fn id(&self) -> &'static str {
        "kv.get"
    }

    fn needs(&self) -> &'static [Capability] {
        &[Capability::Kv]
    }

    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let key = str_arg(self.id(), args, "key")?;
        let kv = cx
            .host
            .kv
            .as_ref()
            .ok_or_else(|| missing(self.id(), Capability::Kv))?;
        match kv.get(key)? {
            None => Ok(Value::Null),
            Some(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| OpError::new(format!("kv value at `{key}` is not JSON: {e}"))),
        }
    }
}

/// `kv.put { key, value }`: stores `value` as canonical JSON. Has effects.
struct KvPut;

impl Op for KvPut {
    fn id(&self) -> &'static str {
        "kv.put"
    }

    fn effects(&self) -> bool {
        true
    }

    fn needs(&self) -> &'static [Capability] {
        &[Capability::Kv]
    }

    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let key = str_arg(self.id(), args, "key")?;
        let value = args
            .get("value")
            .ok_or_else(|| OpError::new("kv.put needs a `value`"))?;
        let kv = cx
            .host
            .kv
            .as_mut()
            .ok_or_else(|| missing(self.id(), Capability::Kv))?;
        kv.put(key, crate::canonical_json(value).as_bytes())?;
        Ok(json!({ "key": key, "value_digest": crate::digest(value) }))
    }
}

/// `mail.send { to, subject, text }`: sends a plain-text email. Has effects.
struct MailSend;

impl Op for MailSend {
    fn id(&self) -> &'static str {
        "mail.send"
    }

    fn effects(&self) -> bool {
        true
    }

    fn needs(&self) -> &'static [Capability] {
        &[Capability::MailOut]
    }

    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let msg = Message {
            to: str_arg(self.id(), args, "to")?.to_owned(),
            subject: str_arg(self.id(), args, "subject")?.to_owned(),
            text: str_arg(self.id(), args, "text")?.to_owned(),
        };
        let mail = cx
            .host
            .mail_out
            .as_mut()
            .ok_or_else(|| missing(self.id(), Capability::MailOut))?;
        mail.send(&msg)?;
        Ok(json!({ "to": msg.to }))
    }
}
