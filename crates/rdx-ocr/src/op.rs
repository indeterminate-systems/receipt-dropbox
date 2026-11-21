//! `ocr.read { document, models, ppi? }`: a document's text, by OCR when it has no text layer.

use rdx_plan::rdx_host::Capability;
use rdx_plan::{Op, OpError, Registry, RunContext};
use serde_json::{Value, json};

use crate::{Models, OcrError, Options, read};

/// Registers this crate's ops.
pub fn register(r: &mut Registry) -> &mut Registry {
    r.register(OcrRead)
}

/// `ocr.read { document, models: { detection, recognition }, ppi? }` →
/// `{ document, source, pages, text, warnings }`, the shape of `pdf.text` plus where the text
/// came from (`text-layer`, `ocr` or `none`) and what to check.
///
/// `models` are blob digests of the two `.rten` files; they are read only when OCR is needed.
pub struct OcrRead;

impl Op for OcrRead {
    fn id(&self) -> &'static str {
        "ocr.read"
    }

    fn needs(&self) -> &'static [Capability] {
        &[Capability::BlobStore]
    }

    fn run(&self, args: &Value, cx: &mut RunContext<'_>) -> Result<Value, OpError> {
        let (digest, bytes) = rdx_pdf::read_document(self.id(), args, cx)?;
        let mut options = Options::default();
        if let Some(ppi) = args.get("ppi") {
            options.ppi = ppi
                .as_f64()
                .filter(|p| *p >= 50.0 && *p <= 1200.0)
                .ok_or_else(|| OpError::new("ocr.read: `ppi` must be a number from 50 to 1200"))?;
        }
        let model = |key: &str, file: &str| -> Result<Vec<u8>, OcrError> {
            let id = args
                .get("models")
                .and_then(|m| m.get(key))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    OcrError::Models(format!("`models.{key}` is missing: the blob digest of {file}"))
                })?;
            let store = cx.host.blob_store.as_ref().ok_or_else(|| {
                OcrError::Models("ran without the `blob-store` capability".into())
            })?;
            store
                .get(id)
                .map_err(|e| OcrError::Models(e.to_string()))?
                .ok_or_else(|| {
                    OcrError::Models(format!(
                        "{file} (`{id}`) is not in the blob store; add it first (locally: \
                         `rdx blob put {file}`; the dev shell has it in $RDX_OCR_MODELS)"
                    ))
                })
        };
        let reading = read(&bytes, &options, || {
            Ok(Models {
                detection: model("detection", "text-detection.rten")?,
                recognition: model("recognition", "text-recognition.rten")?,
            })
        })
        .map_err(|e| OpError::new(format!("{digest}: {e}")))?;
        Ok(json!({
            "document": digest,
            "source": reading.source,
            "pages": reading.text.pages,
            "text": reading.text.to_string(),
            "warnings": reading.warnings,
        }))
    }
}
