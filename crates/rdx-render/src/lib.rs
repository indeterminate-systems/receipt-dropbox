//! Viewable statements: a verified, redacted statement envelope rendered to PDF by Typst.
//!
//! [`render_envelope`] checks the envelope's signature with `rdx-envelope` and renders what that
//! check read ([`StatementView`]): the shown lines, one "Personal, redacted" row per elided line,
//! the opening and closing balances, the total redacted, the root digest, and the envelope itself
//! as a QR code, a PDF attachment and printed text, so the PDF carries what verifies it.
//!
//! Rust does every check and every sum; the Typst template (`statement.typ`) only lays out text it
//! is given. It never sees the original statement or an elided line, because they aren't in the
//! envelope it renders: redaction happens before rendering, never by drawing over a PDF.
//!
//! Typst runs in-process with its fonts compiled in, so the crate does no I/O and builds for
//! `wasm32-unknown-unknown` and `wasm32-wasip2`. The output has no timestamp or random id: the
//! same envelope always gives the same bytes. See ADR-P10.

mod ops;
mod qr;
mod world;

use bc_components::SigningPublicKey;
use bc_envelope::Envelope;
use rdx_envelope::StatementView;
use serde::Serialize;
use typst_layout::PagedDocument;

pub use ops::register;

/// The template, compiled into the crate so a host needs no files to render.
const TEMPLATE: &str = include_str!("statement.typ");

/// Characters per printed line of the envelope's UR on the last page.
const UR_LINE: usize = 64;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The envelope didn't verify or isn't a statement: nothing is rendered from it.
    #[error(transparent)]
    Envelope(#[from] rdx_envelope::Error),
    /// The template failed: a bug in this crate, never in the input.
    #[error("Typst could not render the statement: {0}")]
    Typst(String),
}

/// A rendered viewable statement.
#[derive(Debug, Clone)]
pub struct Rendered {
    pub pdf: Vec<u8>,
    pub pages: usize,
    /// Whether the envelope fit in a QR code. When it doesn't, the PDF says to use the attachment
    /// or the printed UR instead.
    pub qr: bool,
    /// What was rendered: the verified view of the envelope.
    pub view: StatementView,
}

/// Verifies `envelope` against the supporter's key and renders what it shows.
///
/// Pass the redacted envelope (`envelope.redact`'s output). A full envelope verifies too and
/// would render every line, so whoever publishes must redact first; `publish-redacted-statement`
/// does both.
pub fn render_envelope(envelope: &Envelope, signer: &SigningPublicKey) -> Result<Rendered, Error> {
    let view = rdx_envelope::verify(envelope, signer)?;
    render(view, &rdx_envelope::to_ur(envelope))
}

/// Renders a view that [`rdx_envelope::verify`] returned for the envelope `ur`. Private: callers
/// go through [`render_envelope`], so nothing unverified reaches the template.
fn render(view: StatementView, ur: &str) -> Result<Rendered, Error> {
    let qr = qr::svg(ur);
    let data = Data::new(&view, ur, qr.clone());
    let json = serde_json::to_string(&data).expect("template data serializes");
    let world = world::StatementWorld::new(TEMPLATE, json);
    let document: PagedDocument = typst::compile(&world)
        .output
        .map_err(|errors| Error::Typst(messages(&errors)))?;
    let options = typst_pdf::PdfOptions {
        // A stable document id from the statement, not from the time or a random source.
        ident: typst::foundations::Smart::Custom(format!("rdx:{}", view.root)),
        ..Default::default()
    };
    let pdf =
        typst_pdf::pdf(&document, &options).map_err(|errors| Error::Typst(messages(&errors)))?;
    Ok(Rendered {
        pdf,
        pages: document.pages().len(),
        qr: qr.is_some(),
        view,
    })
}

fn messages(errors: &[typst::diag::SourceDiagnostic]) -> String {
    errors
        .iter()
        .map(|e| e.message.to_string())
        .collect::<Vec<_>>()
        .join("; ")
}

/// What the template reads: every value already formatted, so Typst only places text.
#[derive(Serialize)]
struct Data<'a> {
    statement: &'a str,
    supporter: &'a str,
    institution: &'a str,
    mask: &'a str,
    currency: String,
    period_start: String,
    period_end: String,
    original: Option<&'a str>,
    lines: Vec<Line<'a>>,
    redacted_rows: usize,
    opening: String,
    shown: String,
    redacted: String,
    closing: String,
    root: &'a str,
    envelope: &'a str,
    envelope_width: usize,
    envelope_lines: Vec<&'a str>,
    qr: Option<String>,
}

#[derive(Serialize)]
struct Line<'a> {
    date: String,
    description: &'a str,
    reference: Option<&'a str>,
    amount: String,
}

impl<'a> Data<'a> {
    fn new(view: &'a StatementView, ur: &'a str, qr: Option<String>) -> Self {
        let s = &view.summary;
        Self {
            statement: view.statement.as_str(),
            supporter: view.supporter.as_str(),
            institution: &view.account.institution,
            mask: &view.account.mask,
            currency: view.currency.to_string(),
            period_start: view.period.start().to_string(),
            period_end: view.period.end().to_string(),
            original: view.original.as_ref().map(|d| d.as_str()),
            lines: view
                .lines
                .iter()
                .map(|l| Line {
                    date: l.date.to_string(),
                    description: &l.description,
                    // Banks often print the reference inside the description already.
                    reference: l
                        .reference
                        .as_deref()
                        .filter(|r| !l.description.contains(r)),
                    amount: l.amount.decimal(),
                })
                .collect(),
            redacted_rows: view.redacted_rows,
            opening: s.opening.decimal(),
            shown: s.shown.decimal(),
            redacted: s.redacted.decimal(),
            closing: s.closing.decimal(),
            root: view.root.as_str(),
            envelope: ur,
            envelope_width: UR_LINE,
            // URs are ASCII, so byte chunks are character chunks.
            envelope_lines: ur
                .as_bytes()
                .chunks(UR_LINE)
                .map(|c| std::str::from_utf8(c).expect("URs are ASCII"))
                .collect(),
            qr,
        }
    }
}
