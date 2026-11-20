//! `statement.parse`: a statement document in the blob store to a checked statement.

use rdx_plan::rdx_host::Capability;
use rdx_plan::{Op, OpError, Registry, RunContext};
use serde_json::{Value, json};

use crate::{Hints, Input, parse};

/// Registers this crate's ops.
pub fn register(r: &mut Registry) -> &mut Registry {
    r.register(StatementParse)
}

/// `statement.parse { document, supporter, .. }` → `{ statement, parser, checks }`.
///
/// `document` is the blob digest of a CSV, OFX or PDF file. The other arguments are [`Hints`].
/// The statement's `original` is the document's digest, linking it to what was uploaded. Fails,
/// with what to fix, when the statement doesn't add up.
pub struct StatementParse;

impl Op for StatementParse {
    fn id(&self) -> &'static str {
        "statement.parse"
    }

    fn needs(&self) -> &'static [Capability] {
        &[Capability::BlobStore]
    }

    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let (digest, bytes) = rdx_pdf::read_document(self.id(), args, cx)?;
        let mut hint_args = args.clone();
        if let Some(map) = hint_args.as_object_mut() {
            map.remove("document");
            map.insert("original".into(), Value::String(digest.clone()));
        }
        let hints: Hints = serde_json::from_value(hint_args)
            .map_err(|e| OpError::new(format!("{}: bad arguments: {e}", self.id())))?;

        let fail = |e: &dyn std::fmt::Display| OpError::new(format!("{digest}: {e}"));
        let outcome = if rdx_pdf::is_pdf(&bytes) {
            let text = rdx_pdf::text(&bytes).map_err(|e| fail(&e))?;
            if text.is_empty() {
                return Err(fail(
                    &"the PDF has no text layer (a scan?); OCR is not available yet",
                ));
            }
            parse(&Input::Pdf(&text), &hints)
        } else {
            let text = text_of(&bytes);
            parse(&Input::Text(&text), &hints)
        }
        .map_err(|e| fail(&e))?;

        Ok(json!({
            "statement": outcome.statement,
            "parser": outcome.parser,
            "checks": outcome.checks,
        }))
    }
}

/// UTF-8, or else Latin-1, which many banks' CSV and OFX exports still use (`CHARSET:1252`). The
/// two agree on ASCII, so dates and amounts read the same either way; only accented
/// descriptions differ.
fn text_of(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(t) => t.to_owned(),
        Err(_) => bytes.iter().map(|b| *b as char).collect(),
    }
}
