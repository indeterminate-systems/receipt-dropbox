//! OFX (Open Financial Exchange) downloads, version 1 (SGML, unclosed leaf tags) and 2 (XML).
//! Bank and credit-card statements both work: they share the transaction list.

use rdx_model::{AccountRef, Currency, Date, Period};

use crate::amount::{DecimalStyle, decimal};
use crate::{Hints, Input, ParseError, Parsed, ParsedLine, StatementParser};

pub(crate) struct Ofx;

/// The value of the first `<TAG>` leaf in `block`: the text up to the next tag.
fn leaf<'a>(block: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let start = find_ci(block, &open)? + open.len();
    let rest = &block[start..];
    let end = rest.find('<').unwrap_or(rest.len());
    let v = rest[..end].trim();
    (!v.is_empty()).then_some(v)
}

/// The text between `<TAG>` and `</TAG>`, for aggregates.
fn aggregate<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = find_ci(text, &open)? + open.len();
    let end = find_ci(&text[start..], &close).map_or(text.len(), |e| start + e);
    Some(&text[start..end])
}

fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    // OFX tags are upper case by spec; some producers write lower case anyway.
    hay.find(needle)
        .or_else(|| hay.to_ascii_uppercase().find(&needle.to_ascii_uppercase()))
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}

/// `YYYYMMDD[hhmmss[.xxx]][[tz]]`: only the date counts.
fn ofx_date(s: &str) -> Option<Date> {
    let d = s.get(..8)?;
    format!("{}-{}-{}", &d[..4], &d[4..6], &d[6..8])
        .parse()
        .ok()
}

impl StatementParser for Ofx {
    fn id(&self) -> &'static str {
        "ofx"
    }

    fn detect(&self, input: &Input) -> bool {
        let Input::Text(text) = input else {
            return false;
        };
        let head = &text[..text.len().min(4096)].to_ascii_uppercase();
        head.contains("OFXHEADER") || head.contains("<OFX>")
    }

    fn parse(&self, input: &Input, _hints: &Hints) -> Result<Parsed, ParseError> {
        let Input::Text(text) = input else {
            return Err(ParseError::Document("OFX needs text, not a PDF".into()));
        };
        let stmt = aggregate(text, "STMTRS")
            .or_else(|| aggregate(text, "CCSTMTRS"))
            .ok_or_else(|| {
                ParseError::Document("no bank or credit-card statement (STMTRS) in the OFX".into())
            })?;
        let currency = leaf(stmt, "CURDEF")
            .map(|c| Currency::new(c).map_err(|e| ParseError::Document(format!("CURDEF: {e}"))))
            .transpose()?;
        let acct = aggregate(stmt, "BANKACCTFROM").or_else(|| aggregate(stmt, "CCACCTFROM"));
        let account = acct.and_then(|a| leaf(a, "ACCTID")).map(|id| {
            let digits: Vec<char> = id.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
            // Keep only what banks print: the last four characters.
            let mask: String = digits[digits.len().saturating_sub(4)..].iter().collect();
            AccountRef {
                institution: leaf(text, "ORG")
                    .map(unescape)
                    .or_else(|| acct.and_then(|a| leaf(a, "BANKID")).map(str::to_owned))
                    .unwrap_or_else(|| "Unknown".into()),
                mask,
            }
        });
        let list = aggregate(stmt, "BANKTRANLIST").unwrap_or(stmt);
        let period = match (
            leaf(list, "DTSTART").and_then(ofx_date),
            leaf(list, "DTEND").and_then(ofx_date),
        ) {
            (Some(s), Some(e)) => Period::new(s, e).ok(),
            _ => None,
        };

        let mut lines = Vec::new();
        let mut rest = list;
        let mut n = 0;
        while let Some(block) = aggregate(rest, "STMTTRN") {
            n += 1;
            let at = format!("transaction {n}");
            let consumed = block.as_ptr() as usize - rest.as_ptr() as usize + block.len();
            rest = &rest[consumed.min(rest.len())..];
            let date = leaf(block, "DTPOSTED")
                .and_then(ofx_date)
                .ok_or_else(|| ParseError::line(&at, "has no DTPOSTED date"))?;
            let amount_text =
                leaf(block, "TRNAMT").ok_or_else(|| ParseError::line(&at, "has no TRNAMT"))?;
            // OFX amounts are always `-1234.56`, whatever the locale.
            let amount = decimal(amount_text, DecimalStyle::Point).ok_or_else(|| {
                ParseError::line(&at, format!("TRNAMT `{amount_text}` is not an amount"))
            })?;
            let name = leaf(block, "NAME").map(unescape);
            let memo = leaf(block, "MEMO").map(unescape);
            let description = match (name, memo) {
                (Some(n), Some(m)) if !n.contains(&m) => format!("{n} {m}"),
                (Some(n), _) => n,
                (None, Some(m)) => m,
                (None, None) => String::new(),
            };
            let reference = leaf(block, "REFNUM")
                .or_else(|| leaf(block, "CHECKNUM"))
                .map(unescape);
            lines.push(ParsedLine {
                date,
                description,
                amount,
                reference,
                balance: None,
                at,
            });
        }

        let closing = aggregate(stmt, "LEDGERBAL")
            .and_then(|b| leaf(b, "BALAMT"))
            .map(|b| {
                decimal(b, DecimalStyle::Point).ok_or_else(|| {
                    ParseError::Document(format!("LEDGERBAL `{b}` is not an amount"))
                })
            })
            .transpose()?;
        Ok(Parsed {
            account,
            currency,
            period,
            // OFX gives the closing (ledger) balance only; the opening follows from the lines.
            opening: None,
            closing,
            totals: Default::default(),
            lines,
        })
    }
}
