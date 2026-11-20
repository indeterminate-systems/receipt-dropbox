//! `example-bank`: the PDF layout of the synthetic Example Bank, the first PDF layout and the
//! model for the next ones.
//!
//! It has what real statements have: a header repeated on every page, a summary with opening and
//! closing balances and debit and credit totals, a table whose withdrawals and deposits differ
//! only by column, dates without a year, continuation rows (`Ref …`) under a transaction, and a
//! page footer. Real banks' layouts get their own parser next to this one, written against
//! anonymized samples.

use rdx_model::{AccountRef, Currency, Period};
use rdx_pdf::{Line, Span};

use crate::amount::{DecimalStyle, decimal, looks_like_money};
use crate::csv::negate;
use crate::{
    DateOrder, Hints, Input, ParseError, Parsed, ParsedLine, StatementParser, Totals, dates,
};

pub(crate) const INSTITUTION: &str = "Example Bank";

pub(crate) struct ExampleBank;

/// The amount columns, by the right edge of their (right-aligned) header.
struct Columns {
    withdrawals: f64,
    deposits: f64,
    balance: f64,
}

fn header_columns(line: &Line) -> Option<Columns> {
    let find = |name: &str| line.spans.iter().find(|s| s.text == name);
    let date = line.spans.first()?;
    if date.text != "Date" {
        return None;
    }
    find("Description")?;
    Some(Columns {
        withdrawals: find("Withdrawals")?.x_end,
        deposits: find("Deposits")?.x_end,
        balance: find("Balance")?.x_end,
    })
}

fn is_footer(line: &Line) -> bool {
    let t = line.text();
    let words: Vec<&str> = t.split_whitespace().collect();
    matches!(words.as_slice(), ["Page", n, "of", m] if n.parse::<u32>().is_ok() && m.parse::<u32>().is_ok())
}

fn money(span: &Span, at: &str) -> Result<String, ParseError> {
    decimal(&span.text, DecimalStyle::Point)
        .ok_or_else(|| ParseError::line(at, format!("`{}` is not an amount", span.text)))
}

/// A two-span summary row: `label` then an amount.
fn summary(lines: &[&Line], label: &str) -> Result<Option<String>, ParseError> {
    for line in lines {
        if let [l, v] = line.spans.as_slice()
            && l.text == label
        {
            return money(v, label).map(Some);
        }
    }
    Ok(None)
}

impl StatementParser for ExampleBank {
    fn id(&self) -> &'static str {
        "example-bank"
    }

    fn detect(&self, input: &Input) -> bool {
        let Input::Pdf(text) = input else {
            return false;
        };
        let mut lines = text.lines();
        lines.any(|l| l.spans.iter().any(|s| s.text == INSTITUTION))
            && text.lines().any(|l| header_columns(l).is_some())
    }

    fn parse(&self, input: &Input, _hints: &Hints) -> Result<Parsed, ParseError> {
        let Input::Pdf(text) = input else {
            return Err(ParseError::Document(
                "the example-bank layout is a PDF".into(),
            ));
        };
        let all: Vec<&Line> = text.lines().collect();
        let span_text = |prefix: &str| {
            all.iter()
                .flat_map(|l| l.spans.iter())
                .find_map(|s| s.text.strip_prefix(prefix).map(str::trim))
        };

        let mask = span_text("Account ending in ")
            .ok_or_else(|| ParseError::Document("no `Account ending in` line".into()))?;
        let currency = span_text("All amounts in ")
            .map(|c| Currency::new(c).map_err(|e| ParseError::Document(e.to_string())))
            .transpose()?;
        let period = all
            .iter()
            .flat_map(|l| l.spans.iter())
            .find_map(|s| {
                let (a, b) = s.text.split_once(" to ")?;
                Period::new(
                    dates::parse(a, DateOrder::Mdy)?,
                    dates::parse(b, DateOrder::Mdy)?,
                )
                .ok()
            })
            .ok_or_else(|| {
                ParseError::Document("no statement period (`<date> to <date>`)".into())
            })?;

        let mut lines: Vec<ParsedLine> = Vec::new();
        for (p, page) in text.pages.iter().enumerate() {
            let mut columns: Option<Columns> = None;
            for (i, line) in page.lines.iter().enumerate() {
                let at = format!("page {}, line {}", p + 1, i + 1);
                let Some(cols) = &columns else {
                    columns = header_columns(line);
                    continue;
                };
                if is_footer(line) {
                    break;
                }
                let Some(first) = line.spans.first() else {
                    continue;
                };
                let date = dates::day_month(&first.text, DateOrder::Mdy);
                let rest = if date.is_some() {
                    &line.spans[1..]
                } else {
                    &line.spans[..]
                };

                let mut description: Vec<&str> = Vec::new();
                let (mut out, mut inn, mut balance) = (None, None, None);
                for span in rest {
                    if !looks_like_money(&span.text) {
                        description.push(&span.text);
                        continue;
                    }
                    let nearest = [
                        (cols.withdrawals, &mut out),
                        (cols.deposits, &mut inn),
                        (cols.balance, &mut balance),
                    ]
                    .into_iter()
                    .min_by(|a, b| {
                        (a.0 - span.x_end)
                            .abs()
                            .total_cmp(&(b.0 - span.x_end).abs())
                    });
                    let Some((x, slot)) = nearest else { continue };
                    if (x - span.x_end).abs() > 30.0 {
                        return Err(ParseError::line(
                            &at,
                            format!("amount `{}` is under no amount column", span.text),
                        ));
                    }
                    *slot = Some(money(span, &at)?);
                }

                let Some((month, day)) = date else {
                    // A continuation row under the transaction above.
                    let Some(prev) = lines.last_mut() else {
                        continue;
                    };
                    let joined = description.join(" ");
                    match joined.strip_prefix("Ref ") {
                        Some(r) => prev.reference = Some(r.trim().to_owned()),
                        None if !joined.is_empty() => {
                            prev.description.push(' ');
                            prev.description.push_str(&joined);
                        }
                        None => {}
                    }
                    continue;
                };
                let year = if month >= period.start().month() {
                    period.start().year()
                } else {
                    period.end().year()
                };
                let date = rdx_model::Date::new(year, month, day)
                    .map_err(|e| ParseError::line(&at, e.to_string()))?;
                let amount = match (out, inn) {
                    (Some(o), None) => negate(&o),
                    (None, Some(i)) => i,
                    (Some(_), Some(_)) => {
                        return Err(ParseError::line(&at, "has both a withdrawal and a deposit"));
                    }
                    (None, None) => return Err(ParseError::line(&at, "has no amount")),
                };
                lines.push(ParsedLine {
                    date,
                    description: description.join(" "),
                    amount,
                    reference: None,
                    balance,
                    at,
                });
            }
        }

        Ok(Parsed {
            account: Some(AccountRef {
                institution: INSTITUTION.into(),
                mask: mask.to_owned(),
            }),
            currency,
            period: Some(period),
            opening: summary(&all, "Beginning balance")?,
            closing: summary(&all, "Ending balance")?,
            totals: Totals {
                debits: summary(&all, "Withdrawals")?,
                credits: summary(&all, "Deposits")?,
            },
            lines,
        })
    }
}
