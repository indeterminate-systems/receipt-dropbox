//! Statement parsing: a bank statement document in, a checked [`rdx_model::Statement`] out.
//!
//! A document is CSV or OFX text, or a PDF whose text layer [`rdx_pdf`] reads. Each format or
//! bank layout is a [`StatementParser`], the `statement-parser` extension point (mirrored in
//! `wit/rdx.wit`). A parser reports what the document says; [`build`] then fills gaps from
//! [`Hints`], numbers the lines, and checks the result: every printed running balance, the
//! printed totals, and `opening + lines = closing`. A statement that doesn't add up is an error,
//! never a guess, because a misread amount would end up in the ledger.
//!
//! ```
//! use rdx_statement::{Hints, Input, parse};
//!
//! let csv = "date,description,amount,balance\n\
//!            2026-03-03,LANTERN CAFE CATERING #7435,-289.74,454.81\n\
//!            2026-03-13,PAPERLEAF SUPPLY REFUND,250.85,705.66\n";
//! let hints: Hints = serde_json::from_value(serde_json::json!({
//!     "supporter": "s-0001",
//!     "currency": "USD",
//!     "account": { "institution": "Example Bank", "mask": "0001" },
//! })).unwrap();
//! let parsed = parse(&Input::Text(csv), &hints).unwrap();
//! assert_eq!(parsed.parser, "csv");
//! let st = parsed.statement;
//! assert_eq!(st.id.as_str(), "st-2026-03.s-0001");
//! assert_eq!(st.opening.decimal(), "744.55");
//! assert_eq!(st.closing.decimal(), "705.66");
//! ```

mod amount;
mod csv;
mod dates;
mod example_bank;
mod ofx;
mod op;
#[cfg(feature = "render")]
pub mod render;
pub mod unlock;

use rdx_model::{
    AccountRef, Currency, Date, Digest, LineId, Money, Period, Statement, StatementId,
    StatementLine, SupporterId,
};
use serde::{Deserialize, Serialize};

pub use amount::DecimalStyle;
pub use csv::{CsvOptions, DateOrder};
pub use op::{StatementParse, register};
pub use unlock::{RememberPassword, StatementUnlock};

/// What a parser reads: text (CSV, OFX) or a PDF's text layer.
pub enum Input<'a> {
    Text(&'a str),
    Pdf(&'a rdx_pdf::Text),
}

/// The `statement-parser` extension point: one per format or bank layout.
pub trait StatementParser {
    /// Stable id, used in `format` arguments and reports: `csv`, `ofx`, `example-bank`.
    fn id(&self) -> &'static str;
    /// Whether this parser recognises the document. Cheap; looks at headers and markers.
    fn detect(&self, input: &Input) -> bool;
    fn parse(&self, input: &Input, hints: &Hints) -> Result<Parsed, ParseError>;
}

/// The parsers built into the core, in detection order (most specific first).
pub fn parsers() -> Vec<Box<dyn StatementParser>> {
    vec![
        Box::new(ofx::Ofx),
        Box::new(example_bank::ExampleBank),
        Box::new(csv::Csv),
    ]
}

/// What the person or agent knows that the document may not say. Fields the document states
/// are cross-checked against these where both exist.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Hints {
    /// Whose statement it is. Required: documents never say it in a form the model uses.
    pub supporter: Option<SupporterId>,
    /// The statement's id; defaults to `st-<YYYY-MM of the period start>.<supporter>`.
    #[serde(default)]
    pub statement_id: Option<StatementId>,
    /// Parser id to use instead of detecting one.
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub currency: Option<Currency>,
    #[serde(default)]
    pub period: Option<Period>,
    #[serde(default)]
    pub account: Option<AccountRef>,
    /// Balances as decimal text, for documents that don't print them (most CSV exports).
    #[serde(default)]
    pub opening: Option<String>,
    #[serde(default)]
    pub closing: Option<String>,
    #[serde(default)]
    pub csv: CsvOptions,
    /// Digest of the original document, recorded as the statement's `original`.
    #[serde(default)]
    pub original: Option<Digest>,
}

/// What a parser found. Amounts are decimal text (`-1234.56`) until the currency is known.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Parsed {
    pub account: Option<AccountRef>,
    pub currency: Option<Currency>,
    pub period: Option<Period>,
    pub opening: Option<String>,
    pub closing: Option<String>,
    /// Totals the document prints, as (label, decimal): checked against the lines.
    pub totals: Totals,
    pub lines: Vec<ParsedLine>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Totals {
    /// Sum of debits as a positive amount.
    pub debits: Option<String>,
    /// Sum of credits.
    pub credits: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedLine {
    pub date: Date,
    pub description: String,
    /// Negative for a debit.
    pub amount: String,
    pub reference: Option<String>,
    /// The running balance printed after this line, if any.
    pub balance: Option<String>,
    /// Where in the document the line was read, for error messages: `row 4`, `page 2, line 11`.
    pub at: String,
}

/// A parsed, checked statement and how it was read.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Outcome {
    pub statement: Statement,
    /// The parser that read it.
    pub parser: &'static str,
    pub checks: Checks,
}

/// Which checks ran. Every check that can run must pass, or parsing fails; these say which ones
/// the document allowed, so a reader knows how much the statement was cross-checked.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Checks {
    /// Where the opening balance came from.
    pub opening: Source,
    /// Where the closing balance came from.
    pub closing: Source,
    /// Lines whose printed running balance matched the lines before them.
    pub running_balances: usize,
    /// Whether printed debit and credit totals were checked against the lines.
    pub totals: bool,
}

/// Where a balance came from. `opening + lines = closing` only proves something when at most
/// one of the two balances is `derived`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Printed in the document.
    Printed,
    /// Given in the hints.
    Given,
    /// Worked out from the other balance or a running balance and the lines.
    Derived,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    #[error("no parser recognises this document (tried {tried}); pass `format` to choose one")]
    Unrecognised { tried: String },
    #[error("unknown format `{0}`; known formats: csv, ofx, example-bank")]
    UnknownFormat(String),
    #[error("{0}")]
    Document(String),
    #[error("{0} is missing: {1}")]
    Missing(&'static str, &'static str),
    #[error("{at}: {message}")]
    Line { at: String, message: String },
    #[error("statement does not hold together: {}", .0.join("; "))]
    Problems(Vec<String>),
}

impl ParseError {
    pub(crate) fn line(at: &str, message: impl Into<String>) -> Self {
        ParseError::Line {
            at: at.to_owned(),
            message: message.into(),
        }
    }
}

/// Detects (or takes from `hints.format`) a parser, parses, and builds the checked statement.
pub fn parse(input: &Input, hints: &Hints) -> Result<Outcome, ParseError> {
    let all = parsers();
    let parser = match &hints.format {
        Some(f) if f != "auto" => all
            .iter()
            .find(|p| p.id() == f)
            .ok_or_else(|| ParseError::UnknownFormat(f.clone()))?,
        _ => all
            .iter()
            .find(|p| p.detect(input))
            .ok_or_else(|| ParseError::Unrecognised {
                tried: all.iter().map(|p| p.id()).collect::<Vec<_>>().join(", "),
            })?,
    };
    let parsed = parser.parse(input, hints)?;
    build(parsed, parser.id(), hints)
}

fn money(text: &str, currency: Currency, what: &str) -> Result<Money, ParseError> {
    Money::parse(text, currency).map_err(|e| ParseError::Document(format!("{what}: {e}")))
}

/// Turns what a parser found into a checked statement.
pub fn build(parsed: Parsed, parser: &'static str, hints: &Hints) -> Result<Outcome, ParseError> {
    let supporter = hints.supporter.clone().ok_or(ParseError::Missing(
        "supporter",
        "pass the supporter id whose account this is",
    ))?;
    let currency = match (hints.currency, parsed.currency) {
        (Some(h), Some(p)) if h != p => {
            return Err(ParseError::Document(format!(
                "the document says {p}, but currency {h} was given"
            )));
        }
        (Some(c), _) | (None, Some(c)) => c,
        (None, None) => {
            return Err(ParseError::Missing(
                "currency",
                "the document doesn't say; pass `currency`, such as \"USD\"",
            ));
        }
    };
    let account = hints
        .account
        .clone()
        .or(parsed.account)
        .ok_or(ParseError::Missing(
            "account",
            "the document doesn't say; pass `account = { institution, mask }`",
        ))?;
    if parsed.lines.is_empty() && parsed.opening.is_none() {
        return Err(ParseError::Document("no statement lines found".into()));
    }

    let mut lines = Vec::with_capacity(parsed.lines.len());
    let width = parsed.lines.len().to_string().len().max(2);
    for (i, l) in parsed.lines.iter().enumerate() {
        let id = LineId::new(format!("l{:0width$}", i + 1)).expect("l<digits> is a valid id");
        lines.push(StatementLine {
            id,
            date: l.date,
            description: l.description.clone(),
            amount: money(&l.amount, currency, &l.at)?,
            reference: l.reference.clone(),
        });
    }

    let sum_lines = || -> Result<Money, ParseError> {
        Money::sum(currency, lines.iter().map(|l| &l.amount))
            .map_err(|e| ParseError::Document(e.to_string()))
    };
    let given = |printed: &Option<String>, hint: &Option<String>, what: &str| match (printed, hint)
    {
        (Some(p), _) => money(p, currency, what).map(|m| Some((m, Source::Printed))),
        (None, Some(h)) => money(h, currency, what).map(|m| Some((m, Source::Given))),
        (None, None) => Ok(None),
    };
    let opening_known = given(&parsed.opening, &hints.opening, "opening balance")?;
    let closing_known = given(&parsed.closing, &hints.closing, "closing balance")?;

    // Opening: printed, given, worked back from the first running balance, or from the closing.
    let (opening, opening_source) = match opening_known {
        Some(o) => o,
        None => match (
            parsed.lines.first().and_then(|l| l.balance.as_ref()),
            &closing_known,
        ) {
            (Some(b), _) => (
                money(b, currency, &parsed.lines[0].at)?
                    .checked_sub(lines[0].amount)
                    .map_err(|e| ParseError::Document(e.to_string()))?,
                Source::Derived,
            ),
            (None, Some((c, _))) => (
                c.checked_sub(sum_lines()?)
                    .map_err(|e| ParseError::Document(e.to_string()))?,
                Source::Derived,
            ),
            (None, None) => {
                return Err(ParseError::Missing(
                    "opening balance",
                    "the document prints no balances; pass `opening` and `closing`",
                ));
            }
        },
    };

    // Every printed running balance must follow from the one before.
    let mut running = opening;
    let mut running_checked = 0;
    for (l, line) in parsed.lines.iter().zip(&lines) {
        running = running
            .checked_add(line.amount)
            .map_err(|e| ParseError::line(&l.at, e.to_string()))?;
        if let Some(b) = &l.balance {
            let printed = money(b, currency, &l.at)?;
            if printed != running {
                return Err(ParseError::line(
                    &l.at,
                    format!(
                        "printed balance {} but the lines so far add up to {}; an amount or sign was misread, or a line is missing",
                        printed.decimal(),
                        running.decimal()
                    ),
                ));
            }
            running_checked += 1;
        }
    }

    let (closing, closing_source) = match closing_known {
        Some(c) => c,
        None if parsed.lines.last().is_some_and(|l| l.balance.is_some()) => {
            (running, Source::Derived)
        }
        None => {
            return Err(ParseError::Missing(
                "closing balance",
                "the document prints none; pass `closing`",
            ));
        }
    };

    let totals_checked = check_totals(&parsed.totals, &lines, currency)?;

    let period = match hints.period.or(parsed.period) {
        Some(p) => p,
        None => infer_period(&lines).ok_or(ParseError::Missing(
            "period",
            "the document doesn't say and has no lines; pass `period = { start, end }`",
        ))?,
    };
    if let (Some(h), Some(p)) = (hints.period, parsed.period)
        && h != p
    {
        return Err(ParseError::Document(format!(
            "the document's period is {} to {}, but {} to {} was given",
            p.start(),
            p.end(),
            h.start(),
            h.end()
        )));
    }
    let id = match &hints.statement_id {
        Some(id) => id.clone(),
        None => StatementId::new(format!(
            "st-{:04}-{:02}.{}",
            period.start().year(),
            period.start().month(),
            supporter
        ))
        .map_err(|e| ParseError::Document(e.to_string()))?,
    };

    let statement = Statement {
        id,
        supporter,
        account,
        currency,
        period,
        opening,
        closing,
        lines,
        original: hints.original.clone(),
    };
    statement.validate().map_err(|problems| {
        ParseError::Problems(problems.iter().map(|p| p.to_string()).collect())
    })?;
    Ok(Outcome {
        statement,
        parser,
        checks: Checks {
            opening: opening_source,
            closing: closing_source,
            running_balances: running_checked,
            totals: totals_checked,
        },
    })
}

fn check_totals(
    totals: &Totals,
    lines: &[StatementLine],
    currency: Currency,
) -> Result<bool, ParseError> {
    let mut checked = false;
    let sum = |debit: bool| -> i64 {
        lines
            .iter()
            .map(|l| l.amount.minor())
            .filter(|m| (*m < 0) == debit && *m != 0)
            .map(i64::abs)
            .sum()
    };
    for (label, printed, debit) in [
        ("total debits", &totals.debits, true),
        ("total credits", &totals.credits, false),
    ] {
        let Some(printed) = printed else { continue };
        let printed = money(printed, currency, label)?;
        let actual = Money::new(sum(debit), currency);
        if printed.abs() != actual {
            return Err(ParseError::Document(format!(
                "the document's {label} is {}, but the lines read add up to {}; a line was missed or misread",
                printed.abs().decimal(),
                actual.decimal()
            )));
        }
        checked = true;
    }
    Ok(checked)
}

/// The calendar month the lines fall in, or the span of their dates.
fn infer_period(lines: &[StatementLine]) -> Option<Period> {
    let first = lines.iter().map(|l| l.date).min()?;
    let last = lines.iter().map(|l| l.date).max()?;
    let start = if (first.year(), first.month()) == (last.year(), last.month()) {
        Date::new(first.year(), first.month(), 1).ok()?
    } else {
        first
    };
    let end = if start.day() == 1 {
        last.end_of_month()
    } else {
        last
    };
    Period::new(start, end).ok()
}
