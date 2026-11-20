//! Finding amounts and dates in a line of text, and folding text for keyword matching.

use rdx_model::{Currency, Date};

use crate::DateOrder;

/// Lowercases and strips common Latin accents, one char for one char, so char offsets in the
/// folded text are offsets in the original.
pub fn fold(s: &str) -> String {
    s.chars().map(fold_char).collect()
}

fn fold_char(c: char) -> char {
    match c {
        'á' | 'à' | 'â' | 'ä' | 'ã' | 'Á' | 'À' | 'Â' | 'Ä' | 'Ã' => 'a',
        'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'e',
        'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => 'i',
        'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'Ó' | 'Ò' | 'Ô' | 'Ö' | 'Õ' => 'o',
        'ú' | 'ù' | 'û' | 'ü' | 'Ú' | 'Ù' | 'Û' | 'Ü' => 'u',
        'ñ' | 'Ñ' => 'n',
        'ç' | 'Ç' => 'c',
        '\u{a0}' => ' ',
        c => {
            let mut lower = c.to_lowercase();
            match (lower.next(), lower.next()) {
                (Some(l), None) => l,
                // Keep 1:1: a char whose lowercase is longer stays as it is.
                _ => c,
            }
        }
    }
}

/// Byte offset in `s` of its `n`th char.
pub fn byte_at(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map_or(s.len(), |(i, _)| i)
}

/// Whether `word` occurs in `folded` with no letter right before or after it.
pub fn has_word(folded: &str, word: &str) -> bool {
    word_at(folded, word).is_some()
}

/// Char offset of the first whole-word occurrence of `word` in `folded`.
pub fn word_at(folded: &str, word: &str) -> Option<usize> {
    let chars: Vec<char> = folded.chars().collect();
    let w: Vec<char> = word.chars().collect();
    if w.is_empty() || chars.len() < w.len() {
        return None;
    }
    (0..=chars.len() - w.len()).find(|&i| {
        chars[i..i + w.len()] == w[..]
            && (i == 0 || !chars[i - 1].is_alphabetic())
            && chars.get(i + w.len()).is_none_or(|c| !c.is_alphabetic())
    })
}

/// How an amount's currency was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marker {
    /// An ISO code or a symbol only one currency uses (`€`, `MX$`).
    Exact(Currency),
    /// `$` alone: dollars or pesos.
    Dollar,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Amount {
    /// Decimal text with `.` as the separator and no grouping, for `Money::parse`.
    pub decimal: String,
    pub marker: Marker,
    /// Char offset in the line.
    pub at: usize,
}

const SYMBOLS: &[(&str, &str)] = &[
    ("MX$", "MXN"),
    ("US$", "USD"),
    ("CA$", "CAD"),
    ("C$", "CAD"),
    ("AU$", "AUD"),
    ("A$", "AUD"),
    ("NZ$", "NZD"),
    ("R$", "BRL"),
    ("€", "EUR"),
    ("£", "GBP"),
    ("¥", "JPY"),
    ("₹", "INR"),
    ("₩", "KRW"),
];

/// A three-letter code the model knows, standing alone.
fn iso_code(word: &str) -> Option<Currency> {
    if word.len() == 3 && word.bytes().all(|b| b.is_ascii_uppercase()) {
        Currency::new(word).ok()
    } else {
        None
    }
}

fn marker_before(before: &str) -> Marker {
    let t = before.trim_end();
    for (sym, code) in SYMBOLS {
        if t.ends_with(sym) {
            return Marker::Exact(Currency::new(code).unwrap());
        }
    }
    if t.ends_with('$') {
        return Marker::Dollar;
    }
    let word_start = t
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii_alphabetic())
        .last()
        .map_or(t.len(), |(i, _)| i);
    iso_code(&t[word_start..]).map_or(Marker::None, Marker::Exact)
}

fn marker_after(after: &str) -> Marker {
    let t = after.trim_start();
    for (sym, code) in SYMBOLS {
        if t.starts_with(sym) {
            return Marker::Exact(Currency::new(code).unwrap());
        }
    }
    let word: String = t.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    iso_code(&word).map_or(Marker::None, Marker::Exact)
}

/// Amounts in a line: digits with `,` or `.` grouping and a decimal part, or whole numbers with a
/// currency marker. Numbers that are part of words, dates, times, percentages or references are
/// skipped.
pub fn amounts(line: &str) -> Vec<Amount> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        let mut end = i;
        while end < chars.len()
            && (chars[end].is_ascii_digit()
                || (matches!(chars[end], ',' | '.')
                    && chars.get(end + 1).is_some_and(|c| c.is_ascii_digit())))
        {
            end += 1;
        }
        i = end;
        let prev = start.checked_sub(1).map(|p| chars[p]);
        let next = chars.get(end).copied();
        if prev.is_some_and(|c| c.is_alphanumeric() || matches!(c, '-' | '/' | ':' | '#' | '_'))
            || next.is_some_and(|c| c.is_alphanumeric() || matches!(c, '-' | '/' | ':' | '%' | '_'))
        {
            continue;
        }
        let token: String = chars[start..end].iter().collect();
        let Some((decimal, has_fraction)) = normalize(&token) else {
            continue;
        };
        let before: String = chars[..start].iter().collect();
        let after: String = chars[end..].iter().collect();
        let marker = match marker_before(&before) {
            Marker::None => marker_after(&after),
            m => m,
        };
        if !has_fraction && marker == Marker::None {
            continue;
        }
        out.push(Amount {
            decimal,
            marker,
            at: start,
        });
    }
    out
}

/// `1,234.56`, `1.234,56`, `1234,5`, `1,234` to `1234.56`-style text, and whether it has a
/// fractional part. A lone separator followed by exactly three digits is grouping.
fn normalize(token: &str) -> Option<(String, bool)> {
    let seps: Vec<(usize, char)> = token
        .char_indices()
        .filter(|(_, c)| matches!(c, ',' | '.'))
        .collect();
    let digits_only = |s: &str| s.chars().filter(char::is_ascii_digit).collect::<String>();
    let Some(&(last_at, last)) = seps.last() else {
        return Some((token.to_owned(), false));
    };
    let after_last = token.len() - last_at - 1;
    let kinds = seps.iter().filter(|(_, c)| *c == last).count();
    let mixed = seps.iter().any(|(_, c)| *c != last);
    let last_is_decimal = if mixed {
        true
    } else if kinds > 1 {
        false
    } else {
        after_last != 3
    };
    if last_is_decimal {
        let whole = digits_only(&token[..last_at]);
        let frac = &token[last_at + 1..];
        if whole.is_empty() {
            return None;
        }
        Some((format!("{whole}.{frac}"), true))
    } else {
        Some((digits_only(token), false))
    }
}

/// The single currency a whole text names, by ISO code or unambiguous symbol.
pub fn document_currency(text: &str) -> Option<Currency> {
    let mut found: Option<Currency> = None;
    for line in text.lines() {
        for a in amounts(line) {
            if let Marker::Exact(c) = a.marker {
                if found.is_some_and(|f| f != c) {
                    return None;
                }
                found = Some(c);
            }
        }
    }
    found
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FoundDate {
    /// None when day and month could be swapped and nothing says which.
    pub date: Option<Date>,
    pub ambiguous: bool,
    pub at: usize,
}

const MONTHS: &[(&str, u8)] = &[
    ("january", 1),
    ("february", 2),
    ("march", 3),
    ("april", 4),
    ("may", 5),
    ("june", 6),
    ("july", 7),
    ("august", 8),
    ("september", 9),
    ("sept", 9),
    ("october", 10),
    ("november", 11),
    ("december", 12),
    ("enero", 1),
    ("febrero", 2),
    ("marzo", 3),
    ("abril", 4),
    ("mayo", 5),
    ("junio", 6),
    ("julio", 7),
    ("agosto", 8),
    ("septiembre", 9),
    ("setiembre", 9),
    ("octubre", 10),
    ("noviembre", 11),
    ("diciembre", 12),
    ("jan", 1),
    ("feb", 2),
    ("mar", 3),
    ("apr", 4),
    ("abr", 4),
    ("jun", 6),
    ("jul", 7),
    ("aug", 8),
    ("ago", 8),
    ("sep", 9),
    ("oct", 10),
    ("nov", 11),
    ("dec", 12),
    ("dic", 12),
    ("ene", 1),
];

fn month(word: &str) -> Option<u8> {
    let w = fold(word.trim_end_matches('.'));
    MONTHS.iter().find(|(name, _)| *name == w).map(|(_, m)| *m)
}

/// Dates in a line, in order: `2026-03-10`, `10/03/2026`, `March 10, 2026`, `10 Mar 2026`,
/// `3 de marzo de 2026`, `03/mar/2026`.
pub fn dates(line: &str, order: Option<DateOrder>) -> Vec<FoundDate> {
    let mut out = numeric_dates(line, order);
    out.extend(named_dates(line));
    out.sort_by_key(|d| d.at);
    out
}

fn numeric_dates(line: &str, order: Option<DateOrder>) -> Vec<FoundDate> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let digits = |from: usize| {
        let mut e = from;
        while e < chars.len() && chars[e].is_ascii_digit() {
            e += 1;
        }
        e
    };
    let num = |a: usize, b: usize| chars[a..b].iter().collect::<String>().parse::<u32>().ok();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() || (i > 0 && chars[i - 1].is_ascii_alphanumeric()) {
            i += 1;
            continue;
        }
        let e1 = digits(i);
        let Some(&sep) = chars.get(e1).filter(|c| matches!(c, '/' | '-' | '.')) else {
            i = e1;
            continue;
        };
        let e2 = digits(e1 + 1);
        if e2 == e1 + 1 || chars.get(e2) != Some(&sep) {
            i = e1;
            continue;
        }
        let e3 = digits(e2 + 1);
        if e3 == e2 + 1 || chars.get(e3).is_some_and(|c| c.is_ascii_digit()) {
            i = e1;
            continue;
        }
        let (l1, l3) = (e1 - i, e3 - e2 - 1);
        let (a, b, c) = (num(i, e1), num(e1 + 1, e2), num(e2 + 1, e3));
        let (Some(a), Some(b), Some(c)) = (a, b, c) else {
            i = e3;
            continue;
        };
        let make = |y: u32, m: u32, d: u32| Date::new(y as i32, m as u8, d as u8).ok();
        let found = if l1 == 4 && e2 - e1 - 1 <= 2 && l3 <= 2 {
            make(a, b, c).map(|d| FoundDate {
                date: Some(d),
                ambiguous: false,
                at: i,
            })
        } else if l1 <= 2 && e2 - e1 - 1 <= 2 && l3 == 4 {
            let dmy = make(c, b, a);
            let mdy = make(c, a, b);
            match (dmy, mdy) {
                (Some(x), Some(y)) if x == y => Some(FoundDate {
                    date: Some(x),
                    ambiguous: false,
                    at: i,
                }),
                (Some(x), Some(y)) => match order {
                    Some(DateOrder::Dmy) => Some(x),
                    Some(DateOrder::Mdy) => Some(y),
                    None => None,
                }
                .map_or(
                    Some(FoundDate {
                        date: None,
                        ambiguous: true,
                        at: i,
                    }),
                    |d| {
                        Some(FoundDate {
                            date: Some(d),
                            ambiguous: false,
                            at: i,
                        })
                    },
                ),
                (Some(x), None) | (None, Some(x)) => Some(FoundDate {
                    date: Some(x),
                    ambiguous: false,
                    at: i,
                }),
                (None, None) => None,
            }
        } else {
            None
        };
        out.extend(found);
        i = e3;
    }
    out
}

fn named_dates(line: &str) -> Vec<FoundDate> {
    // Words and numbers with their char offsets; `de` and punctuation are skipped.
    let mut tokens: Vec<(usize, String)> = Vec::new();
    let mut cur = String::new();
    let mut start = 0;
    for (i, c) in line.chars().enumerate() {
        if c.is_alphanumeric() {
            if cur.is_empty() {
                start = i;
            }
            cur.push(c);
        } else if !cur.is_empty() {
            tokens.push((start, std::mem::take(&mut cur)));
        }
    }
    if !cur.is_empty() {
        tokens.push((start, cur));
    }
    tokens.retain(|(_, t)| !matches!(fold(t).as_str(), "de" | "del" | "of" | "the"));

    let day = |t: &str| {
        let t = t
            .trim_end_matches("st")
            .trim_end_matches("nd")
            .trim_end_matches("rd")
            .trim_end_matches("th");
        (t.len() <= 2 && !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()))
            .then(|| t.parse::<u8>().ok())
            .flatten()
    };
    let year = |t: &str| {
        (t.len() == 4 && t.bytes().all(|b| b.is_ascii_digit()))
            .then(|| t.parse::<i32>().ok())
            .flatten()
    };
    let mut out = Vec::new();
    for (k, (at, tok)) in tokens.iter().enumerate() {
        let Some(m) = month(tok) else { continue };
        let get = |j: Option<usize>| j.and_then(|j| tokens.get(j)).map(|(_, t)| t.as_str());
        // `March 10, 2026`
        if let (Some(d), Some(y)) = (
            get(Some(k + 1)).and_then(day),
            get(Some(k + 2)).and_then(year),
        ) && let Ok(date) = Date::new(y, m, d)
        {
            out.push(FoundDate {
                date: Some(date),
                ambiguous: false,
                at: *at,
            });
            continue;
        }
        // `10 March 2026`, `3 de marzo de 2026`, `03/mar/2026`
        if let (Some(d), Some(y)) = (
            get(k.checked_sub(1)).and_then(day),
            get(Some(k + 1)).and_then(year),
        ) && let Ok(date) = Date::new(y, m, d)
        {
            out.push(FoundDate {
                date: Some(date),
                ambiguous: false,
                at: tokens[k - 1].0,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decimals(line: &str) -> Vec<String> {
        amounts(line).into_iter().map(|a| a.decimal).collect()
    }

    #[test]
    fn amounts_in_common_notations() {
        assert_eq!(decimals("Total $1,234.56"), ["1234.56"]);
        assert_eq!(decimals("Total 1.234,56 €"), ["1234.56"]);
        assert_eq!(decimals("Importe 42,50"), ["42.50"]);
        assert_eq!(decimals("TOTAL $ 145.00"), ["145.00"]);
        assert_eq!(decimals("Total USD 12"), ["12"]);
        assert_eq!(decimals("Total 1,200 JPY"), ["1200"]);
    }

    #[test]
    fn not_amounts() {
        assert!(decimals("IVA 16%").is_empty());
        assert!(decimals("Invoice INV-20417").is_empty());
        assert!(decimals("Fecha: 03/03/2026 13:42").is_empty());
        assert!(decimals("2 Café americano").is_empty());
        assert!(decimals("Refund date: 2026-03-18").is_empty());
    }

    #[test]
    fn currency_markers() {
        let m = |l: &str| amounts(l)[0].marker;
        assert_eq!(
            m("Total MX$ 10.00"),
            Marker::Exact(Currency::new("MXN").unwrap())
        );
        assert_eq!(m("Total $10.00"), Marker::Dollar);
        assert_eq!(
            m("Total 10.00 EUR"),
            Marker::Exact(Currency::new("EUR").unwrap())
        );
        assert_eq!(m("€10,00"), Marker::Exact(Currency::new("EUR").unwrap()));
        assert_eq!(m("Total 10.00"), Marker::None);
    }

    fn date_of(line: &str, order: Option<DateOrder>) -> Option<String> {
        dates(line, order)
            .first()
            .and_then(|d| d.date)
            .map(|d| d.to_string())
    }

    #[test]
    fn date_notations() {
        let d = |l: &str| date_of(l, None);
        assert_eq!(d("Date: 2026-03-10").as_deref(), Some("2026-03-10"));
        assert_eq!(d("2026-03-12T11:05:33").as_deref(), Some("2026-03-12"));
        assert_eq!(
            d("Invoice date: March 10, 2026").as_deref(),
            Some("2026-03-10")
        );
        assert_eq!(
            d("Wed, 18 Mar 2026 at 17:20").as_deref(),
            Some("2026-03-18")
        );
        assert_eq!(d("3 de marzo de 2026").as_deref(), Some("2026-03-03"));
        assert_eq!(d("03/mar/2026").as_deref(), Some("2026-03-03"));
        assert_eq!(d("25/03/2026").as_deref(), Some("2026-03-25"));
        assert_eq!(d("03/25/2026").as_deref(), Some("2026-03-25"));
        assert_eq!(d("Fecha: 03/03/2026 13:42").as_deref(), Some("2026-03-03"));
    }

    #[test]
    fn ambiguous_numeric_dates_need_an_order() {
        let found = dates("03/04/2026", None);
        assert_eq!(found[0].date, None);
        assert!(found[0].ambiguous);
        assert_eq!(
            date_of("03/04/2026", Some(DateOrder::Dmy)).as_deref(),
            Some("2026-04-03")
        );
        assert_eq!(
            date_of("03/04/2026", Some(DateOrder::Mdy)).as_deref(),
            Some("2026-03-04")
        );
    }

    #[test]
    fn folding_keeps_offsets() {
        assert_eq!(fold("CAFETERÍA Ñ"), "cafeteria n");
        assert_eq!(
            fold("CAFETERÍA").chars().count(),
            "CAFETERÍA".chars().count()
        );
        assert!(has_word("subtotal total", "total"));
        assert!(!has_word("subtotal", "total"));
    }
}
