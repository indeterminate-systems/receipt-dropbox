use std::collections::BTreeMap;
use std::str::FromStr;

use rdx_model::{
    Account, Currency, Date, Digest, LedgerEntry, LineId, LineRef, Money, Posting, ReceiptId,
    StatementId, SupporterId,
};

use crate::ledger::HEADER;
use crate::{Ledger, StatementRecord};

/// Why a ledger's text could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReadError {
    #[error("line {line}: {message}")]
    Syntax { line: usize, message: String },
    #[error(
        "the ledger is not in the form rdx writes (first difference at line {line}: expected \
         `{expected}`, found `{found}`); it was edited outside rdx. Restore the version the kv \
         entry pointed at, or book a correcting entry through rdx"
    )]
    NotCanonical {
        line: usize,
        expected: String,
        found: String,
    },
}

fn syntax(line: usize, message: impl Into<String>) -> ReadError {
    ReadError::Syntax {
        line,
        message: message.into(),
    }
}

/// A transaction or balance being read, with its metadata.
struct Open {
    line: usize,
    date: Date,
    head: Head,
    meta: BTreeMap<String, String>,
    postings: Vec<Posting>,
}

enum Head {
    Txn { payee: String, narration: String },
    Balance { account: Account, amount: Money },
}

impl Ledger {
    /// Reads Beancount text that [`Ledger::render`] wrote. Anything else (a hand edit, another
    /// tool's file) is refused with the line at fault, rather than half-read and then rewritten.
    pub fn parse(text: &str) -> Result<Ledger, ReadError> {
        let mut ledger = Ledger::default();
        let mut current: Option<Open> = None;
        for (i, raw) in text.lines().enumerate() {
            let n = i + 1;
            if let Some(rest) = raw.strip_prefix("  ") {
                let cur = current
                    .as_mut()
                    .ok_or_else(|| syntax(n, "indented line outside a transaction or balance"))?;
                if let Some((key, value)) = rest.split_once(": ") {
                    let value =
                        unquote(value).ok_or_else(|| syntax(n, "expected a quoted value"))?;
                    cur.meta.insert(key.to_string(), value.to_string());
                } else {
                    if matches!(cur.head, Head::Balance { .. }) {
                        return Err(syntax(n, "a balance assertion has no postings"));
                    }
                    cur.postings.push(posting(rest).map_err(|m| syntax(n, m))?);
                }
                continue;
            }
            if let Some(done) = current.take() {
                finish(done, &mut ledger)?;
            }
            if raw.is_empty() || raw.starts_with(';') {
                continue;
            }
            let (date, rest) = raw
                .split_once(' ')
                .ok_or_else(|| syntax(n, "expected `DATE directive …`"))?;
            let date = Date::from_str(date).map_err(|e| syntax(n, e.to_string()))?;
            if let Some(account) = rest.strip_prefix("open ") {
                // `open` directives are derived from the entries; render() checks them below.
                Account::new(account).map_err(|e| syntax(n, e.to_string()))?;
            } else if let Some(rest) = rest.strip_prefix("balance ") {
                let mut parts = rest.split(' ');
                let (Some(account), Some(amount), Some(currency), None) =
                    (parts.next(), parts.next(), parts.next(), parts.next())
                else {
                    return Err(syntax(n, "expected `DATE balance ACCOUNT AMOUNT CURRENCY`"));
                };
                current = Some(Open {
                    line: n,
                    date,
                    head: Head::Balance {
                        account: Account::new(account).map_err(|e| syntax(n, e.to_string()))?,
                        amount: money(amount, currency).map_err(|m| syntax(n, m))?,
                    },
                    meta: BTreeMap::new(),
                    postings: Vec::new(),
                });
            } else if let Some(rest) = rest.strip_prefix("* ") {
                let (payee, rest) = quoted(rest).ok_or_else(|| syntax(n, "expected a payee"))?;
                let rest = rest
                    .strip_prefix(' ')
                    .ok_or_else(|| syntax(n, "expected a narration"))?;
                let (narration, rest) =
                    quoted(rest).ok_or_else(|| syntax(n, "expected a narration"))?;
                // The link is the statement id, which `line` metadata also carries.
                if !rest.starts_with(" ^") {
                    return Err(syntax(n, "expected the statement link `^<statement id>`"));
                }
                current = Some(Open {
                    line: n,
                    date,
                    head: Head::Txn {
                        payee: payee.to_string(),
                        narration: narration.to_string(),
                    },
                    meta: BTreeMap::new(),
                    postings: Vec::new(),
                });
            } else {
                return Err(syntax(
                    n,
                    "rdx's ledger has only `open`, `balance` and `*` (cleared transaction) \
                     directives",
                ));
            }
        }
        if let Some(done) = current.take() {
            finish(done, &mut ledger)?;
        }
        ledger.sort();

        // The text must be exactly what rdx would write for these entries: that catches edits to
        // balances, open dates, links and order, which the entries alone don't carry.
        let expected = ledger.render().map_err(|e| syntax(0, e.to_string()))?;
        if expected != text {
            let (mut a, mut b) = (expected.lines(), text.lines());
            let mut line = 1;
            let (expected, found) = loop {
                match (a.next(), b.next()) {
                    (Some(x), Some(y)) if x == y => line += 1,
                    // Same lines: only the line endings or the final newline differ.
                    (None, None) => break ("<lines ending in \\n>", "<other line endings>"),
                    (x, y) => break (x.unwrap_or("<end of file>"), y.unwrap_or("<end of file>")),
                }
            };
            return Err(ReadError::NotCanonical {
                line,
                expected: expected.to_string(),
                found: found.to_string(),
            });
        }
        Ok(ledger)
    }

    /// The text of an empty ledger.
    pub fn empty_text() -> &'static str {
        HEADER
    }
}

fn finish(open: Open, ledger: &mut Ledger) -> Result<(), ReadError> {
    let n = open.line;
    let mut meta = open.meta;
    let mut take = |key: &str| {
        meta.remove(key)
            .ok_or_else(|| syntax(n, format!("missing `{key}` metadata")))
    };
    match open.head {
        Head::Balance { account, amount } => {
            let id =
                StatementId::new(take("statement-id")?).map_err(|e| syntax(n, e.to_string()))?;
            let supporter =
                SupporterId::new(take("supporter")?).map_err(|e| syntax(n, e.to_string()))?;
            let original =
                Digest::parse(&take("statement")?).map_err(|e| syntax(n, e.to_string()))?;
            if account != Account::reimbursable(&supporter) {
                return Err(syntax(
                    n,
                    "a balance assertion is on the supporter's reimbursable account",
                ));
            }
            let record = StatementRecord {
                id: id.clone(),
                supporter,
                currency: amount.currency(),
                period_end: open.date.add_days(-1),
                original,
            };
            if ledger.statements.insert(id.clone(), record).is_some() {
                return Err(syntax(
                    n,
                    format!("statement `{id}` has two balance assertions"),
                ));
            }
        }
        Head::Txn { payee, narration } => {
            let line = take("line")?;
            let (statement, line_id) = line
                .split_once('/')
                .ok_or_else(|| syntax(n, "`line` is `<statement id>/<line id>`"))?;
            let entry = LedgerEntry {
                date: open.date,
                payee,
                narration,
                supporter: SupporterId::new(take("supporter")?)
                    .map_err(|e| syntax(n, e.to_string()))?,
                line: LineRef {
                    statement: StatementId::new(statement).map_err(|e| syntax(n, e.to_string()))?,
                    line: LineId::new(line_id).map_err(|e| syntax(n, e.to_string()))?,
                },
                receipt: ReceiptId::new(take("receipt")?).map_err(|e| syntax(n, e.to_string()))?,
                receipt_source: Digest::parse(&take("receipt-source")?)
                    .map_err(|e| syntax(n, e.to_string()))?,
                statement: Digest::parse(&take("statement")?)
                    .map_err(|e| syntax(n, e.to_string()))?,
                postings: open.postings,
            };
            entry.validate().map_err(|e| syntax(n, e.to_string()))?;
            ledger.entries.push(entry);
        }
    }
    if let Some(key) = meta.keys().next() {
        return Err(syntax(n, format!("unexpected metadata `{key}`")));
    }
    Ok(())
}

fn posting(text: &str) -> Result<Posting, String> {
    let mut parts = text.split(' ');
    let account = parts.next().unwrap_or_default();
    let account = Account::new(account).map_err(|e| e.to_string())?;
    let amount = match (parts.next(), parts.next(), parts.next()) {
        (None, ..) => None,
        (Some(a), Some(c), None) => Some(money(a, c)?),
        _ => return Err("expected `ACCOUNT AMOUNT CURRENCY`".into()),
    };
    Ok(Posting { account, amount })
}

fn money(amount: &str, currency: &str) -> Result<Money, String> {
    let currency = Currency::new(currency).map_err(|e| e.to_string())?;
    Money::parse(amount, currency).map_err(|e| e.to_string())
}

/// `"text"` → `text`; the whole value must be one quoted string.
fn unquote(s: &str) -> Option<&str> {
    match quoted(s)? {
        (inner, "") => Some(inner),
        _ => None,
    }
}

/// Splits a leading quoted string (rdx never writes quotes inside one) from the rest.
fn quoted(s: &str) -> Option<(&str, &str)> {
    let rest = s.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some((&rest[..end], &rest[end + 1..]))
}
