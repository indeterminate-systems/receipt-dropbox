//! Receipt and bill extraction: merchant, date, total, reference and kind from the documents
//! supporters send in (roadmap piece 5).
//!
//! A [`Document`] is the text of a receipt as received: an email (`message/rfc822`, with MIME
//! parts, quoted-printable and base64), an HTML page, plain text (including a PDF's text layer,
//! which `rdx-pdf` reads), or CFDI XML. [`extract`] runs the built-in
//! [`ReceiptExtractor`]s over it and returns an [`Extraction`]: the fields it found, the ones it
//! could not, and why. It never guesses an ambiguous date or a currency the document doesn't
//! state: those come from [`Hints`] (organization policy) or the extraction asks for review.
//!
//! ```
//! use rdx_receipt::{Document, Hints, MediaType, Status, extract};
//!
//! let doc = Document::new(MediaType::Text, "Corner Store\nDate: 2026-03-04\nTotal USD 12.00\n".into());
//! let x = extract(&doc, &Hints::default());
//! assert_eq!(x.status, Status::Complete);
//! assert_eq!(x.fields.total.unwrap().minor(), 1200);
//! ```
//!
//! Like every core crate, this one does no I/O and builds for `wasm32-unknown-unknown` and
//! `wasm32-wasip2`.

mod cfdi;
mod html;
mod mime;
mod op;
mod scan;
mod text;

use rdx_model::{Currency, Date, Digest, Money, ReceiptKind};
use serde::{Deserialize, Serialize};

pub use cfdi::Cfdi;
pub use html::to_text as html_to_text;
pub use mime::{Email, parse as parse_email};
pub use op::{ExtractOp, register};
pub use text::Text;

/// What a document is, by the media types the extractor reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaType {
    #[serde(rename = "text/plain")]
    Text,
    #[serde(rename = "text/html")]
    Html,
    #[serde(rename = "message/rfc822")]
    Email,
    #[serde(rename = "application/xml")]
    Xml,
}

impl MediaType {
    pub fn as_str(&self) -> &'static str {
        match self {
            MediaType::Text => "text/plain",
            MediaType::Html => "text/html",
            MediaType::Email => "message/rfc822",
            MediaType::Xml => "application/xml",
        }
    }
}

/// A receipt as received, before any decoding.
#[derive(Debug, Clone)]
pub struct Document {
    pub media_type: MediaType,
    pub content: String,
}

impl Document {
    pub fn new(media_type: MediaType, content: String) -> Self {
        Self {
            media_type,
            content,
        }
    }

    /// Digest of the document as received, which becomes the receipt's `source`.
    pub fn digest(&self) -> Digest {
        Digest::of_bytes(self.content.as_bytes())
    }
}

/// Day and month order for numeric dates such as `03/04/2026`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DateOrder {
    Dmy,
    Mdy,
}

/// What the organization knows that a document may not say. Nothing here overrides what the
/// document states explicitly.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hints {
    /// For amounts with no currency, or only `$` (dollars or pesos).
    pub currency: Option<Currency>,
    /// For numeric dates whose day and month could be swapped.
    pub date_order: Option<DateOrder>,
    /// Used when the document doesn't label its merchant. Emails fill it from the sender.
    pub merchant: Option<String>,
}

/// The receipt fields an extractor found. Any of them can be missing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Fields {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merchant: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<Date>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<Money>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<ReceiptKind>,
}

impl Fields {
    /// Fills fields this one lacks from `other`.
    fn fill_from(&mut self, other: Fields) {
        self.merchant = self.merchant.take().or(other.merchant);
        self.date = self.date.or(other.date);
        self.total = self.total.or(other.total);
        self.reference = self.reference.take().or(other.reference);
        self.kind = self.kind.or(other.kind);
    }
}

/// Mexican fiscal data from a CFDI. Not part of the receipt model yet: piece 15 (fiscal proof)
/// decides where it lives; until then it travels next to the receipt in the extraction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fiscal {
    /// The SAT folio fiscal (`TimbreFiscalDigital/@UUID`).
    pub uuid: String,
    pub version: String,
    /// `TipoDeComprobante`: I (ingreso), E (egreso), P (pago), T (traslado), N (nómina).
    pub cfdi_type: String,
    pub issuer_rfc: String,
    pub receiver_rfc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receiver_name: Option<String>,
    /// SAT catalog `c_FormaPago`, such as `04` (credit card) or `01` (cash).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payment_form: Option<String>,
    /// `PUE` (paid in one installment) or `PPD` (paid in installments or later).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payment_method: Option<String>,
}

/// What one extractor found in one text.
#[derive(Debug, Clone, Default)]
pub struct Found {
    pub fields: Fields,
    pub fiscal: Option<Fiscal>,
    pub warnings: Vec<String>,
    /// An amount was found but its currency is unknown (no code and no hint).
    pub currency_unknown: bool,
    /// A date was found but day and month could be swapped (no hint).
    pub date_ambiguous: bool,
}

/// A source of receipt fields for one kind of text. Mirrors `receipt-extractor` in
/// `wit/rdx.wit`, so vendor-specific extractors can also be built as components.
pub trait ReceiptExtractor {
    /// Short name recorded in the extraction (`text`, `cfdi`).
    fn name(&self) -> &'static str;
    /// Whether this extractor reads `text` of this media type.
    fn detect(&self, text: &str, media_type: &str) -> bool;
    fn extract(&self, text: &str, media_type: &str, hints: &Hints) -> Found;
}

/// A non-text part of an email (a PDF or image), listed so a later step can read it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Attachment {
    pub media_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    /// Digest of the decoded bytes.
    pub digest: Digest,
    pub size: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Merchant, date and total (with its currency) were all found.
    Complete,
    /// Something is missing or ambiguous: see `missing` and `warnings`.
    NeedsReview,
}

#[derive(Debug, Clone, Serialize)]
pub struct Extraction {
    pub status: Status,
    /// The extractor that supplied the most fields' first source (`cfdi` wins over `text`).
    pub extractor: &'static str,
    pub fields: Fields,
    /// `merchant`, `date`, `total` or `currency`, in that order.
    pub missing: Vec<&'static str>,
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fiscal: Option<Fiscal>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<Attachment>,
}

/// Extracts with the built-in extractors: CFDI first (it is authoritative when present), then the
/// generic text extractor to fill in anything else.
pub fn extract(doc: &Document, hints: &Hints) -> Extraction {
    extract_with(doc, hints, &[&Cfdi, &Text])
}

/// Extracts with `extractors`, in priority order: a field found by an earlier extractor is kept.
pub fn extract_with(
    doc: &Document,
    hints: &Hints,
    extractors: &[&dyn ReceiptExtractor],
) -> Extraction {
    let mut hints = hints.clone();
    let mut warnings = Vec::new();
    let mut attachments = Vec::new();
    let mut header_date = None;
    // (media type, text) pairs the extractors look at.
    let bodies: Vec<(String, String)> = match doc.media_type {
        MediaType::Text => vec![("text/plain".into(), doc.content.clone())],
        MediaType::Html => vec![("text/plain".into(), html::to_text(&doc.content))],
        MediaType::Xml => vec![("application/xml".into(), doc.content.clone())],
        MediaType::Email => {
            let email = mime::parse(&doc.content);
            if hints.merchant.is_none() {
                hints.merchant = email.merchant_sender();
            }
            header_date = email
                .date
                .as_deref()
                .and_then(|d| scan::dates(d, None).into_iter().find_map(|f| f.date));
            attachments = email.attachments;
            let mut bodies = Vec::new();
            if !email.text.trim().is_empty() {
                bodies.push(("text/plain".into(), email.text));
            }
            bodies.extend(email.xml.into_iter().map(|x| ("application/xml".into(), x)));
            bodies
        }
    };

    let mut fields = Fields::default();
    let mut fiscal = None;
    let mut extractor = None;
    let mut currency_unknown = false;
    let mut date_ambiguous = false;
    for ex in extractors {
        for (media_type, text) in &bodies {
            if !ex.detect(text, media_type) {
                continue;
            }
            let found = ex.extract(text, media_type, &hints);
            if extractor.is_none() && found.fields != Fields::default() {
                extractor = Some(ex.name());
            }
            currency_unknown |= found.currency_unknown && fields.total.is_none();
            date_ambiguous |= found.date_ambiguous && fields.date.is_none();
            fiscal = fiscal.or(found.fiscal);
            warnings.extend(found.warnings);
            fields.fill_from(found.fields);
        }
    }

    if fields.date.is_none()
        && !date_ambiguous
        && let Some(d) = header_date
    {
        fields.date = Some(d);
        warnings.push(format!(
            "no date in the document; used the email's Date header ({d})"
        ));
    }
    if fields.merchant.is_none() {
        fields.merchant = hints.merchant.clone();
    }
    if fields.kind.is_none() && fields.total.is_some() {
        fields.kind = Some(ReceiptKind::Receipt);
    }
    if bodies.is_empty() && !attachments.is_empty() {
        warnings.push(
            "the email has no text body; read its attachments (PDF text layer, OCR) and extract from those"
                .into(),
        );
    }

    let mut missing = Vec::new();
    if fields.merchant.is_none() {
        missing.push("merchant");
    }
    if fields.date.is_none() {
        missing.push("date");
    }
    if fields.total.is_none() {
        missing.push(if currency_unknown {
            "currency"
        } else {
            "total"
        });
    }
    if date_ambiguous && fields.date.is_none() {
        warnings.push(
            "the date is ambiguous (day-first or month-first); set the `date_order` hint".into(),
        );
    }
    if currency_unknown && fields.total.is_none() {
        warnings.push(
            "found a total but not its currency (`$` alone could be dollars or pesos); set the `currency` hint"
                .into(),
        );
    }
    Extraction {
        status: if missing.is_empty() {
            Status::Complete
        } else {
            Status::NeedsReview
        },
        extractor: extractor.unwrap_or("none"),
        fields,
        missing,
        warnings,
        fiscal,
        attachments,
    }
}
