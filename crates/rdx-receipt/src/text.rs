//! The generic extractor: labelled lines and keywords in English and Spanish, for receipts,
//! emailed bills and a PDF's text layer.

use rdx_model::{Money, ReceiptKind};

use crate::scan::{self, Amount, Marker};
use crate::{Fields, Found, Hints, ReceiptExtractor};

/// Reads plain text (and HTML, which the caller converts to text first).
pub struct Text;

impl ReceiptExtractor for Text {
    fn name(&self) -> &'static str {
        "text"
    }

    fn detect(&self, _text: &str, media_type: &str) -> bool {
        matches!(media_type, "text/plain" | "text/html")
    }

    fn extract(&self, text: &str, media_type: &str, hints: &Hints) -> Found {
        let text = if media_type == "text/html" {
            crate::html::to_text(text)
        } else {
            text.to_owned()
        };
        let lines: Vec<Line> = text
            .lines()
            .map(|l| l.replace('\t', "  ").replace('\u{a0}', " "))
            .map(|l| l.trim().to_owned())
            .filter(|l| !l.is_empty())
            .map(|raw| Line {
                folded: scan::fold(&raw),
                raw,
            })
            .collect();
        let mut found = Found::default();
        let mut fields = Fields {
            merchant: merchant(&lines, hints),
            reference: reference(&lines),
            kind: Some(kind(&lines)),
            ..Fields::default()
        };
        match date(&lines, hints) {
            DateResult::Found(d) => fields.date = Some(d),
            DateResult::Ambiguous => found.date_ambiguous = true,
            DateResult::None => {}
        }
        if let Some((amount, line)) = total(&lines) {
            let currency = match amount.marker {
                Marker::Exact(c) => Some(c),
                Marker::Dollar | Marker::None => scan::document_currency(&text).or(hints.currency),
            };
            match currency {
                Some(c) => match Money::parse(&amount.decimal, c) {
                    Ok(m) => fields.total = Some(m),
                    Err(e) => found
                        .warnings
                        .push(format!("total on `{line}` doesn't fit {c}: {e}")),
                },
                None => found.currency_unknown = true,
            }
        }
        found.fields = fields;
        found
    }
}

struct Line {
    raw: String,
    folded: String,
}

impl Line {
    /// The text after `label` at the start of the line, past `:`, `#` or `.`, if the line starts
    /// with it as a whole word.
    fn after(&self, label: &str) -> Option<&str> {
        let f = self.folded.trim_start_matches(['*', '-', '•', ' ']);
        let skipped = self.folded.chars().count() - f.chars().count();
        if !f.starts_with(label) {
            return None;
        }
        let n = label.chars().count();
        if f.chars().nth(n).is_some_and(char::is_alphanumeric) {
            return None;
        }
        let rest = &self.raw[scan::byte_at(&self.raw, skipped + n)..];
        Some(
            rest.trim_start_matches([' ', ':', '#', '.', '-', '–'])
                .trim_start_matches(' '),
        )
    }
}

/// Labels whose value is the merchant.
const MERCHANT_LABELS: &[&str] = &[
    "merchant",
    "vendor",
    "sold by",
    "seller",
    "supplier",
    "proveedor",
    "emisor",
    "razon social",
    "comercio",
    "establecimiento",
];

/// A first line starting with one of these is a heading or greeting, not the merchant.
const NOT_MERCHANT: &[&str] = &[
    "receipt",
    "invoice",
    "tax invoice",
    "bill",
    "factura",
    "ticket",
    "recibo",
    "nota",
    "comprobante",
    "thank",
    "thanks",
    "gracias",
    "hello",
    "hi",
    "dear",
    "hola",
    "estimado",
    "order",
    "your",
    "su ",
    "tu ",
    "date",
    "fecha",
    "from",
    "to",
    "de:",
    "para",
    "subject",
    "asunto",
    "sent",
    "refund",
    "reembolso",
    "forward",
    "see ",
    "please",
    "---",
];

fn merchant(lines: &[Line], hints: &Hints) -> Option<String> {
    for line in lines {
        for label in MERCHANT_LABELS {
            if let Some(v) = line.after(label).filter(|v| !v.is_empty()) {
                return Some(v.to_owned());
            }
        }
    }
    if let Some(m) = &hints.merchant {
        return Some(m.clone());
    }
    // The first line that looks like a name: receipts open with the merchant.
    lines
        .iter()
        .take(8)
        .find(|l| {
            l.raw.chars().filter(|c| c.is_alphabetic()).count() >= 2
                && l.raw.chars().count() <= 60
                && !l.raw.contains('@')
                && !l.raw.contains("://")
                && !l.raw.ends_with(['.', '!', '?', ',', ':'])
                && !NOT_MERCHANT.iter().any(|p| {
                    l.folded.starts_with(p)
                        && (p.ends_with([' ', ':']) || scan::word_at(&l.folded, p) == Some(0))
                })
                && scan::amounts(&l.raw).is_empty()
                && scan::dates(&l.raw, None).is_empty()
        })
        .map(|l| l.raw.clone())
}

/// Labels whose next token is the document number, longest first so `invoice number` wins over
/// `invoice`.
const REFERENCE_LABELS: &[&str] = &[
    "numero de factura",
    "no. de factura",
    "confirmation number",
    "booking reference",
    "transaction id",
    "invoice number",
    "receipt number",
    "order number",
    "reference number",
    "invoice no",
    "receipt no",
    "order no",
    "confirmation",
    "reference",
    "invoice",
    "receipt",
    "order",
    "factura",
    "pedido",
    "folio",
    "ticket",
    "ref",
];

fn reference(lines: &[Line]) -> Option<String> {
    for line in lines {
        for label in REFERENCE_LABELS {
            let Some(rest) = line.after(label) else {
                continue;
            };
            let rest = rest.trim_start_matches(['#', ':', ' ']);
            let token: String = rest
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_end_matches(['.', ',', ';', ')'])
                .to_owned();
            let valid = token.len() >= 3
                && token.chars().any(|c| c.is_ascii_digit())
                && token
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '/' | '.'));
            if valid {
                return Some(token);
            }
            // `invoice date: …` starts with `invoice` too: try the next label.
        }
    }
    None
}

fn kind(lines: &[Line]) -> ReceiptKind {
    let any = |words: &[&str]| {
        lines
            .iter()
            .any(|l| words.iter().any(|w| scan::has_word(&l.folded, w)))
    };
    if any(&[
        "refund",
        "refunded",
        "reembolso",
        "devolucion",
        "credit note",
        "nota de credito",
        "reintegro",
    ]) {
        ReceiptKind::Refund
    } else if any(&[
        "invoice",
        "amount due",
        "total due",
        "balance due",
        "factura",
        "your bill",
        "billing statement",
        "total a pagar",
        "fecha limite de pago",
    ]) {
        ReceiptKind::Bill
    } else {
        ReceiptKind::Receipt
    }
}

enum DateResult {
    Found(rdx_model::Date),
    Ambiguous,
    None,
}

const DATE_LABELS: &[&str] = &[
    "date",
    "fecha",
    "issued",
    "paid on",
    "emitido",
    "invoice date",
    "order date",
    "trip date",
    "refund date",
    "purchase date",
    "transaction date",
    "fecha de emision",
    "fecha de compra",
];

const DUE_WORDS: &[&str] = &[
    "due",
    "vencimiento",
    "fecha limite",
    "vence",
    "pagar antes",
    "next",
    "proximo",
    "expires",
    "valid until",
];

fn date(lines: &[Line], hints: &Hints) -> DateResult {
    let not_due = |l: &&Line| !DUE_WORDS.iter().any(|w| scan::has_word(&l.folded, w));
    let labelled = |l: &&Line| DATE_LABELS.iter().any(|w| scan::has_word(&l.folded, w));
    let mut ambiguous = false;
    // Labelled lines first, then any line; due dates never.
    for pass in [true, false] {
        for (i, line) in lines.iter().enumerate().filter(|(_, l)| not_due(l)) {
            if pass && !labelled(&line) {
                continue;
            }
            let mut found = scan::dates(&line.raw, hints.date_order);
            // A label in one table cell, the date in the next line.
            if found.is_empty()
                && pass
                && let Some(next) = lines.get(i + 1)
            {
                found = scan::dates(&next.raw, hints.date_order);
            }
            for f in found {
                match f.date {
                    Some(d) => return DateResult::Found(d),
                    None => ambiguous |= f.ambiguous,
                }
            }
        }
        if ambiguous {
            return DateResult::Ambiguous;
        }
    }
    DateResult::None
}

/// Keywords for the total, strongest first. Plain `total` must not be `subtotal`.
const TOTAL_STRONG: &[&str] = &[
    "grand total",
    "total due",
    "amount due",
    "balance due",
    "total charged",
    "total paid",
    "amount paid",
    "amount charged",
    "amount refunded",
    "refund total",
    "total refunded",
    "refund amount",
    "total amount",
    "total a pagar",
    "importe total",
    "monto total",
    "importe a pagar",
    "monto a pagar",
    "total pagado",
    "importe reembolsado",
];

const NOT_TOTAL: &[&str] = &[
    "total tax",
    "total taxes",
    "total savings",
    "total discount",
    "total items",
    "total de articulos",
];

fn total(lines: &[Line]) -> Option<(Amount, String)> {
    let mut best: Option<(u8, Amount, String)> = None;
    for (i, line) in lines.iter().enumerate() {
        if NOT_TOTAL.iter().any(|w| scan::has_word(&line.folded, w)) {
            continue;
        }
        let strong = TOTAL_STRONG
            .iter()
            .filter_map(|w| scan::word_at(&line.folded, w).map(|at| at + w.chars().count()))
            .max();
        let (tier, after) = match strong {
            Some(at) => (2, at),
            None => match scan::word_at(&line.folded, "total") {
                Some(at) => (1, at + 5),
                None => continue,
            },
        };
        let amount = scan::amounts(&line.raw)
            .into_iter()
            .rfind(|a| a.at >= after)
            .or_else(|| {
                // `Total` alone, its amount on the next line (a table read column by column).
                let next = lines.get(i + 1)?;
                if scan::has_word(&next.folded, "total") {
                    return None;
                }
                scan::amounts(&next.raw).into_iter().next()
            });
        if let Some(a) = amount {
            // Later lines win within a tier: totals come after subtotals.
            if best.as_ref().is_none_or(|(t, _, _)| tier >= *t) {
                best = Some((tier, a, line.raw.clone()));
            }
        }
    }
    best.map(|(_, a, l)| (a, l))
}
