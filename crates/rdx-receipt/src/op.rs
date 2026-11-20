//! `receipt.extract`: the op plans use. Its constructor and argument contract are in
//! `nickel/rdx/api.ncl` under `ops.receipt`.

use rdx_model::{Currency, Digest, Receipt, ReceiptId, SupporterId};
use rdx_plan::{Op, OpError, Registry, RunContext};
use serde_json::{Value, json};

use crate::{DateOrder, Document, Hints, MediaType, Status, extract};

/// Registers the receipt ops with a host's registry.
pub fn register(r: &mut Registry) {
    r.register(ExtractOp);
}

/// `receipt.extract { supporter, document, media_type, receipt_id?, hints? }`.
///
/// `document` is the document's text, or the output of `pdf.text` (`{ document, text, … }`), whose
/// `document` digest is then the receipt's source. No effects and no capabilities: the result
/// goes in the run report, for a later step to store.
pub struct ExtractOp;

fn err(message: impl Into<String>) -> OpError {
    OpError::new(format!("receipt.extract: {}", message.into()))
}

impl Op for ExtractOp {
    fn id(&self) -> &'static str {
        "receipt.extract"
    }

    fn run(&self, args: &Value, _: &mut RunContext<'_>) -> Result<Value, OpError> {
        let supporter = args
            .get("supporter")
            .and_then(Value::as_str)
            .ok_or_else(|| err("needs a string `supporter`"))
            .and_then(|s| SupporterId::new(s).map_err(|e| err(e.to_string())))?;
        let media_type: MediaType = match args.get("media_type") {
            None | Some(Value::Null) => MediaType::Email,
            Some(v) => serde_json::from_value(v.clone()).map_err(|_| {
                err(format!(
                    "unknown media_type {v}; use text/plain, text/html, message/rfc822 or application/xml"
                ))
            })?,
        };
        let (doc, source) = match args.get("document") {
            Some(Value::String(text)) => {
                let doc = Document::new(media_type, text.clone());
                let source = doc.digest();
                (doc, source)
            }
            // The output of `pdf.text`: the text layer, and the digest of the PDF itself.
            Some(Value::Object(o)) => {
                let text = o
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| err("`document` is a record without a string `text`"))?;
                let source = o
                    .get("document")
                    .and_then(Value::as_str)
                    .map(Digest::parse)
                    .transpose()
                    .map_err(|e| err(e.to_string()))?
                    .unwrap_or_else(|| Digest::of_bytes(text.as_bytes()));
                (Document::new(MediaType::Text, text.to_owned()), source)
            }
            Some(Value::Null) => {
                return Err(err(
                    "`document` is null: the step it refers to was skipped or produced nothing",
                ));
            }
            _ => return Err(err("needs a `document`: text, or the output of pdf.text")),
        };
        let hints = hints(args.get("hints"))?;

        let x = extract(&doc, &hints);
        let receipt = match x.status {
            Status::Complete => {
                let id = match args.get("receipt_id").and_then(Value::as_str) {
                    Some(id) => id.to_owned(),
                    // Content-addressed, so extracting the same document twice gives one id.
                    None => format!("r-{}", &source.as_str()["sha256:".len()..][..16]),
                };
                let receipt = Receipt {
                    id: ReceiptId::new(id).map_err(|e| err(e.to_string()))?,
                    supporter,
                    kind: x.fields.kind.expect("complete extractions have a kind"),
                    merchant: x.fields.merchant.clone().expect("complete"),
                    date: x.fields.date.expect("complete"),
                    total: x.fields.total.expect("complete"),
                    reference: x.fields.reference.clone(),
                    items: Vec::new(),
                    source: source.clone(),
                };
                receipt.validate().map_err(|e| err(e.to_string()))?;
                Some(receipt)
            }
            Status::NeedsReview => None,
        };
        let mut out = serde_json::to_value(&x).expect("extractions serialize");
        let map = out.as_object_mut().expect("an object");
        map.insert("source".into(), json!(source));
        map.insert("receipt".into(), json!(receipt));
        Ok(out)
    }
}

fn hints(value: Option<&Value>) -> Result<Hints, OpError> {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return Ok(Hints::default());
    };
    let str_field = |name: &str| value.get(name).and_then(Value::as_str);
    Ok(Hints {
        currency: str_field("currency")
            .map(Currency::new)
            .transpose()
            .map_err(|e| err(e.to_string()))?,
        date_order: match str_field("date_order") {
            None => None,
            Some("dmy") => Some(DateOrder::Dmy),
            Some("mdy") => Some(DateOrder::Mdy),
            Some(other) => return Err(err(format!("date_order `{other}`: use dmy or mdy"))),
        },
        merchant: str_field("merchant").map(str::to_owned),
    })
}
