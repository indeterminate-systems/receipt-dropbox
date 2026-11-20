//! CSV exports: one row per transaction, columns found by header names (English or Spanish).

use rdx_model::Date;
use serde::{Deserialize, Serialize};

use crate::amount::{DecimalStyle, decimal};
use crate::{Hints, Input, ParseError, Parsed, ParsedLine, StatementParser, dates};

/// How to read a CSV export when its header and values don't settle it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CsvOptions {
    /// `,`, `;` or tab; detected from the header when absent.
    #[serde(default)]
    pub delimiter: Option<char>,
    /// Order of numeric dates such as `03/04/2026`; detected when the values allow it.
    #[serde(default)]
    pub date_order: Option<DateOrder>,
    #[serde(default)]
    pub decimal: DecimalStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DateOrder {
    /// `2026/03/04`
    Ymd,
    /// `03/04/2026`, US style.
    Mdy,
    /// `04/03/2026`, most of the world, Mexico included.
    Dmy,
}

pub(crate) struct Csv;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Col {
    Date,
    Description,
    Amount,
    Debit,
    Credit,
    Balance,
    Reference,
}

fn column(header: &str) -> Option<Col> {
    let h = header.trim().trim_matches('"').trim().to_lowercase();
    let h = h.as_str();
    Some(match h {
        "date"
        | "posting date"
        | "posted date"
        | "post date"
        | "transaction date"
        | "fecha"
        | "fecha de operación"
        | "fecha operación"
        | "fecha de operacion"
        | "fecha operacion" => Col::Date,
        "description" | "details" | "detail" | "memo" | "payee" | "name" | "narrative"
        | "transaction" | "concepto" | "descripción" | "descripcion" | "movimiento" => {
            Col::Description
        }
        "amount" | "importe" | "monto" => Col::Amount,
        "debit" | "debits" | "withdrawal" | "withdrawals" | "money out" | "cargo" | "cargos"
        | "retiro" | "retiros" => Col::Debit,
        "credit" | "credits" | "deposit" | "deposits" | "money in" | "abono" | "abonos"
        | "depósito" | "depósitos" | "deposito" | "depositos" => Col::Credit,
        "balance" | "running balance" | "saldo" => Col::Balance,
        "reference" | "ref" | "ref." | "referencia" | "check number" | "check no" => Col::Reference,
        _ => return None,
    })
}

fn first_line(text: &str) -> &str {
    text.trim_start_matches('\u{feff}')
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
}

fn sniff_delimiter(header: &str) -> char {
    [',', ';', '\t']
        .into_iter()
        .max_by_key(|d| header.matches(*d).count())
        .unwrap_or(',')
}

/// RFC 4180 records: quoted fields may hold delimiters, doubled quotes and newlines.
fn records(text: &str, delim: char) -> Vec<(usize, Vec<String>)> {
    let mut out = Vec::new();
    let mut field = String::new();
    let mut row = Vec::new();
    let mut quoted = false;
    let mut line = 1;
    let mut row_line = 1;
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                '\n' => {
                    line += 1;
                    field.push(c);
                }
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => quoted = true,
            c if c == delim => row.push(std::mem::take(&mut field)),
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                if row.iter().any(|f| !f.trim().is_empty()) {
                    out.push((row_line, std::mem::take(&mut row)));
                } else {
                    row.clear();
                }
                line += 1;
                row_line = line;
            }
            _ => field.push(c),
        }
    }
    row.push(field);
    if row.iter().any(|f| !f.trim().is_empty()) {
        out.push((row_line, row));
    }
    out
}

/// Picks the numeric date order the values allow: a first part over 12 means day first.
fn detect_order(values: &[&str]) -> Result<DateOrder, ParseError> {
    let mut first_over_12 = false;
    let mut second_over_12 = false;
    let mut numeric = false;
    for v in values {
        if v.trim().parse::<Date>().is_ok() {
            continue;
        }
        let parts: Vec<&str> = v.trim().split(['/', '-', '.']).collect();
        if parts.len() == 3 && parts.iter().all(|p| p.bytes().all(|b| b.is_ascii_digit())) {
            numeric = true;
            if parts[0].len() == 4 {
                return Ok(DateOrder::Ymd);
            }
            first_over_12 |= parts[0].parse::<u8>().is_ok_and(|n| n > 12);
            second_over_12 |= parts[1].parse::<u8>().is_ok_and(|n| n > 12);
        }
    }
    match (numeric, first_over_12, second_over_12) {
        (false, _, _) => Ok(DateOrder::Ymd),
        (true, true, false) => Ok(DateOrder::Dmy),
        (true, false, true) => Ok(DateOrder::Mdy),
        _ => Err(ParseError::Document(
            "can't tell whether dates are day/month or month/day; pass `csv = { date_order = \"dmy\" }` (or \"mdy\")"
                .into(),
        )),
    }
}

impl StatementParser for Csv {
    fn id(&self) -> &'static str {
        "csv"
    }

    fn detect(&self, input: &Input) -> bool {
        let Input::Text(text) = input else {
            return false;
        };
        let header = first_line(text);
        let delim = sniff_delimiter(header);
        let cols: Vec<Col> = header.split(delim).filter_map(column).collect();
        cols.contains(&Col::Date)
            && (cols.contains(&Col::Amount)
                || cols.contains(&Col::Debit)
                || cols.contains(&Col::Credit))
    }

    fn parse(&self, input: &Input, hints: &Hints) -> Result<Parsed, ParseError> {
        let Input::Text(text) = input else {
            return Err(ParseError::Document("CSV needs text, not a PDF".into()));
        };
        let opts = &hints.csv;
        let delim = opts
            .delimiter
            .unwrap_or_else(|| sniff_delimiter(first_line(text)));
        let rows = records(text, delim);
        let Some(((_, header), body)) = rows.split_first() else {
            return Err(ParseError::Document("the CSV is empty".into()));
        };
        let cols: Vec<Option<Col>> = header.iter().map(|h| column(h)).collect();
        let find = |c: Col| cols.iter().position(|x| *x == Some(c));
        let date_col = find(Col::Date).ok_or_else(|| {
            ParseError::Document(format!(
                "no date column in the header `{}`",
                header.join(",")
            ))
        })?;
        let amount_col = find(Col::Amount);
        let (debit_col, credit_col) = (find(Col::Debit), find(Col::Credit));
        if amount_col.is_none() && debit_col.is_none() && credit_col.is_none() {
            return Err(ParseError::Document(format!(
                "no amount, debit or credit column in the header `{}`",
                header.join(",")
            )));
        }
        let desc_col = find(Col::Description);
        let balance_col = find(Col::Balance);
        let ref_col = find(Col::Reference);

        let date_values: Vec<&str> = body
            .iter()
            .filter_map(|(_, r)| r.get(date_col).map(String::as_str))
            .collect();
        let order = match opts.date_order {
            Some(o) => o,
            None => detect_order(&date_values)?,
        };

        let mut lines = Vec::new();
        for (line_no, row) in body {
            let at = format!("row {line_no}");
            let cell = |i: Option<usize>| {
                i.and_then(|i| row.get(i))
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
            };
            let date_text =
                cell(Some(date_col)).ok_or_else(|| ParseError::line(&at, "has no date"))?;
            let date = dates::parse(date_text, order)
                .ok_or_else(|| ParseError::line(&at, format!("`{date_text}` is not a date")))?;
            let num = |i: Option<usize>, what: &str| -> Result<Option<String>, ParseError> {
                match cell(i) {
                    None => Ok(None),
                    Some(t) => decimal(t, opts.decimal).map(Some).ok_or_else(|| {
                        ParseError::line(&at, format!("{what} `{t}` is not an amount"))
                    }),
                }
            };
            let amount = match num(amount_col, "amount")? {
                Some(a) => a,
                None => {
                    let debit = num(debit_col, "debit")?;
                    let credit = num(credit_col, "credit")?;
                    match (debit, credit) {
                        // Debit columns print positive numbers for money out.
                        (Some(d), None) => negate(&d),
                        (None, Some(c)) => c,
                        (Some(_), Some(_)) => {
                            return Err(ParseError::line(&at, "has both a debit and a credit"));
                        }
                        (None, None) => return Err(ParseError::line(&at, "has no amount")),
                    }
                }
            };
            lines.push(ParsedLine {
                date,
                description: cell(desc_col).unwrap_or("").to_owned(),
                amount,
                reference: cell(ref_col).map(str::to_owned),
                balance: num(balance_col, "balance")?,
                at,
            });
        }
        // Some banks export newest first; statements read oldest first.
        if lines
            .first()
            .zip(lines.last())
            .is_some_and(|(f, l)| f.date > l.date)
        {
            lines.reverse();
        }
        Ok(Parsed {
            lines,
            ..Parsed::default()
        })
    }
}

pub(crate) fn negate(d: &str) -> String {
    match d.strip_prefix('-') {
        Some(rest) => rest.to_owned(),
        None if d.bytes().all(|b| b == b'0' || b == b'.') => d.to_owned(),
        None => format!("-{d}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_fields() {
        let r = records("a,b\n\"x, y\",\"say \"\"hi\"\"\"\n\n1,2", ',');
        assert_eq!(r.len(), 3);
        assert_eq!(r[1].1, vec!["x, y", "say \"hi\""]);
        assert_eq!(r[2].0, 4);
    }

    #[test]
    fn date_order_from_values() {
        assert_eq!(
            detect_order(&["14/03/2026", "02/03/2026"]).unwrap(),
            DateOrder::Dmy
        );
        assert_eq!(detect_order(&["03/14/2026"]).unwrap(), DateOrder::Mdy);
        assert!(detect_order(&["03/04/2026"]).is_err());
        assert_eq!(detect_order(&["2026-03-04"]).unwrap(), DateOrder::Ymd);
    }
}
