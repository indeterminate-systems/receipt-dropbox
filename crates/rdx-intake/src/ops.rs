//! `intake.*` ops, so the local host (and agents) can run intake as plans. Their constructors and
//! argument contracts are in `nickel/rdx/ops/intake.ncl`; the policy contract is
//! `nickel/rdx/intake.ncl`.
//!
//! The Cloudflare Worker calls [`crate::receive`] and [`crate::sweep`] directly instead, because
//! it answers through Email Routing (`reply`, `setReject`) rather than the `mail-out` capability.
//!
//! The claim link never passes through a plan: `intake.receive` sends it straight to `mail-out`,
//! and `intake.release` asks the person for it through `secrets` (label `claim-link`).

use rdx_host::{Capability, Host};
use rdx_plan::{Op, OpError, Registry, RunContext};
use serde_json::{Value, json};

use crate::{Outcome, Policy, ReplyStatus};

/// Adds the `intake.*` ops to a host's registry.
pub fn register(r: &mut Registry) -> &mut Registry {
    r.register(Receive)
        .register(Status)
        .register(Sweep)
        .register(Release)
}

fn err(op: &str) -> impl Fn(crate::Error) -> OpError + '_ {
    move |e| OpError::new(format!("{op}: {e}"))
}

fn policy(op: &str, args: &Value) -> Result<Policy, OpError> {
    serde_json::from_value(args.get("policy").cloned().unwrap_or(Value::Null)).map_err(|e| {
        OpError::new(format!(
            "{op}: `policy` doesn't fit rdx/intake.ncl's Policy ({e}); build it with that contract"
        ))
    })
}

fn str_arg<'a>(op: &str, args: &'a Value, name: &str) -> Result<&'a str, OpError> {
    args.get(name)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| OpError::new(format!("{op} needs a non-empty string `{name}`")))
}

fn mail(host: &mut Host, msg: &rdx_host::Message) -> Result<(), OpError> {
    Ok(host
        .mail_out
        .as_mut()
        .expect("checked by needs()")
        .send(msg)?)
}

/// `intake.receive`: one email from the blob store through intake.
struct Receive;

impl Op for Receive {
    fn id(&self) -> &'static str {
        "intake.receive"
    }

    fn effects(&self) -> bool {
        true
    }

    fn needs(&self) -> &'static [Capability] {
        &[
            Capability::BlobStore,
            Capability::Kv,
            Capability::Clock,
            Capability::Random,
            Capability::MailOut,
        ]
    }

    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let op = self.id();
        let policy = policy(op, args)?;
        let message = str_arg(op, args, "message")?;
        let keep = args.get("keep_message").and_then(Value::as_bool) == Some(true);
        let host = &mut *cx.host;
        let store = host.blob_store.as_mut().expect("checked by needs()");
        let raw = store.get(message)?.ok_or_else(|| {
            OpError::new(format!(
                "{op}: no `{message}` in the blob store; put the .eml there first (`rdx blob put`)"
            ))
        })?;
        let received = crate::receive(host, &policy, &raw).map_err(err(op))?;
        let reply = match &received.reply {
            Some(r) => {
                mail(host, &r.message)?;
                ReplyStatus::Sent
            }
            None => ReplyStatus::None,
        };
        // The plaintext copy goes, whatever the outcome, unless asked to keep it.
        if !keep {
            host.blob_store
                .as_mut()
                .expect("checked by needs()")
                .delete(message)?;
        }
        if let Outcome::Rejected { reason, .. } = &received.outcome {
            cx.notes.push(format!("{op}: rejected: {reason}"));
        }
        let mut out = serde_json::to_value(&received.outcome).expect("outcomes serialize");
        out["reply"] = json!(reply);
        if let Some(ask) = received.asked {
            out["asked"] = json!(ask);
        }
        out["message_deleted"] = json!(!keep);
        Ok(out)
    }
}

/// `intake.status`: every quarantine and whether it has expired.
struct Status;

impl Op for Status {
    fn id(&self) -> &'static str {
        "intake.status"
    }

    fn needs(&self) -> &'static [Capability] {
        &[Capability::Kv, Capability::Clock]
    }

    fn run(&self, _args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let quarantined = crate::status(cx.host).map_err(err(self.id()))?;
        let expired = quarantined.iter().filter(|q| q.expired).count();
        Ok(json!({ "quarantined": quarantined, "expired": expired }))
    }
}

/// `intake.sweep`: deletes expired quarantines and tells their senders.
struct Sweep;

impl Op for Sweep {
    fn id(&self) -> &'static str {
        "intake.sweep"
    }

    fn effects(&self) -> bool {
        true
    }

    fn needs(&self) -> &'static [Capability] {
        &[
            Capability::BlobStore,
            Capability::Kv,
            Capability::Clock,
            Capability::MailOut,
        ]
    }

    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let op = self.id();
        let policy = policy(op, args)?;
        let swept = crate::sweep(cx.host, &policy).map_err(err(op))?;
        for notice in &swept.notices {
            mail(cx.host, notice)?;
        }
        Ok(json!({ "expired": swept.expired, "notices_sent": swept.notices.len() }))
    }
}

/// `intake.release`: claims a quarantine with the link its sender received.
struct Release;

impl Op for Release {
    fn id(&self) -> &'static str {
        "intake.release"
    }

    fn effects(&self) -> bool {
        true
    }

    fn needs(&self) -> &'static [Capability] {
        &[
            Capability::BlobStore,
            Capability::Kv,
            Capability::Random,
            Capability::Secrets,
        ]
    }

    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let op = self.id();
        let address = str_arg(op, args, "address")?.to_lowercase();
        let link = cx
            .host
            .secrets
            .as_mut()
            .expect("checked by needs()")
            .ask("claim-link")?
            .ok_or_else(|| {
                OpError::new(format!(
                    "{op}: no claim link given; paste the link from the email `{address}` received"
                ))
            })?;
        let (linked, mut secret) = crate::parse_claim_link(&link).map_err(err(op))?;
        if linked != address {
            return Err(OpError::new(format!(
                "{op}: that link is for `{linked}`, not `{address}`"
            )));
        }
        let moved = crate::release(cx.host, &address, &secret);
        zeroize::Zeroize::zeroize(&mut secret);
        let moved = moved.map_err(err(op))?;
        Ok(json!({ "address": address, "documents": moved }))
    }
}
