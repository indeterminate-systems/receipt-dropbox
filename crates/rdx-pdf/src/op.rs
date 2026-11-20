//! `pdf.text { document }`: the text layer of a PDF in the blob store.

use rdx_plan::rdx_host::Capability;
use rdx_plan::{Op, OpError, Registry, RunContext};
use serde_json::{Value, json};

/// Registers this crate's ops.
pub fn register(r: &mut Registry) -> &mut Registry {
    r.register(PdfText)
}

/// Reads a document from the blob store by its digest in `args.document`, for ops that take one.
pub fn read_document(
    op: &str,
    args: &Value,
    cx: &RunContext<'_>,
) -> Result<(String, Vec<u8>), OpError> {
    let digest = args
        .get("document")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            OpError::new(format!("{op} needs `document`, a blob digest (`sha256:…`)"))
        })?;
    let store = cx
        .host
        .blob_store
        .as_ref()
        .ok_or_else(|| OpError::new(format!("{op} ran without the `blob-store` capability")))?;
    let bytes = store.get(digest)?.ok_or_else(|| {
        OpError::new(format!(
            "no document `{digest}` in the blob store; add the file first (locally: `rdx blob put FILE`)"
        ))
    })?;
    Ok((digest.to_owned(), bytes))
}

/// `pdf.text { document }` → `{ document, pages, text }`: `pages` keeps each line's spans and
/// positions, `text` is the same as plain lines. An empty `text` means the PDF has no text layer.
pub struct PdfText;

impl Op for PdfText {
    fn id(&self) -> &'static str {
        "pdf.text"
    }

    fn needs(&self) -> &'static [Capability] {
        &[Capability::BlobStore]
    }

    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let (digest, bytes) = read_document(self.id(), args, cx)?;
        let text = crate::text(&bytes).map_err(|e| OpError::new(format!("{digest}: {e}")))?;
        Ok(json!({
            "document": digest,
            "pages": text.pages,
            "text": text.to_string(),
        }))
    }
}
