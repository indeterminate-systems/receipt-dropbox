//! `render.statement`: a verified, redacted envelope to a PDF in the host's blob store.

use rdx_plan::rdx_host::Capability;
use rdx_plan::{Op, OpError, Registry, RunContext};
use serde_json::{Value, json};

use crate::render_envelope;

/// Registers this crate's ops.
pub fn register(r: &mut Registry) -> &mut Registry {
    r.register(RenderStatement)
}

/// `render.statement { envelope, signer }` → `{ document, pages, qr, root, statement, shown,
/// redacted_rows }`.
///
/// `envelope` is a `ur:envelope/…` string or the output of an `envelope.*` step (a record with an
/// `envelope` field). The PDF goes to the blob store, so the step has effects: a dry run skips it.
pub struct RenderStatement;

impl Op for RenderStatement {
    fn id(&self) -> &'static str {
        "render.statement"
    }

    fn effects(&self) -> bool {
        true
    }

    fn needs(&self) -> &'static [Capability] {
        &[Capability::BlobStore]
    }

    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let id = self.id();
        let fail = |e: &dyn std::fmt::Display| OpError::new(format!("{id}: {e}"));
        let value = args.get("envelope");
        let ur = value
            .and_then(Value::as_str)
            .or_else(|| value?.get("envelope")?.as_str())
            .ok_or_else(|| {
                OpError::new(format!(
                    "{id} needs `envelope`: a `ur:envelope/…` string, or the output of an \
                     earlier `envelope.*` step"
                ))
            })?;
        let signer = args
            .get("signer")
            .and_then(Value::as_str)
            .ok_or_else(|| OpError::new(format!("{id} needs a string `signer`")))?;
        let envelope = rdx_envelope::from_ur(ur).map_err(|e| fail(&e))?;
        let signer = rdx_envelope::public_key_from_ur(signer).map_err(|e| fail(&e))?;
        let out = render_envelope(&envelope, &signer).map_err(|e| fail(&e))?;
        let store = cx
            .host
            .blob_store
            .as_mut()
            .expect("the executor checks `needs` first");
        let document = store.put(&out.pdf)?;
        Ok(json!({
            "document": document,
            "pages": out.pages,
            "qr": out.qr,
            "root": out.view.root,
            "statement": out.view.statement,
            "shown": out.view.lines.len(),
            "redacted_rows": out.view.redacted_rows,
        }))
    }
}
