//! Amounts as banks print them (`$1,234.56`, `(12.00)`, `1.234,56`, `45.00-`) to plain decimal
//! text (`-1234.56`) that `Money::parse` takes. No floating point anywhere.

use serde::{Deserialize, Serialize};

/// Which character separates the decimals.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecimalStyle {
    /// Decide per amount: the last `.` or `,` followed by one or two digits is the decimal mark.
    #[default]
    Auto,
    /// `1,234.56`
    Point,
    /// `1.234,56`
    Comma,
}

/// Plain decimal text for a printed amount, or `None` if it isn't one.
pub(crate) fn decimal(text: &str, style: DecimalStyle) -> Option<String> {
    let mut s: String = text
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '\u{a0}')
        .collect();
    let mut negative = false;
    if s.starts_with('(') && s.ends_with(')') {
        negative = true;
        s = s[1..s.len() - 1].to_string();
    }
    for suffix in ["CR", "cr"] {
        if let Some(rest) = s.strip_suffix(suffix) {
            s = rest.to_string();
        }
    }
    for suffix in ["DR", "dr"] {
        if let Some(rest) = s.strip_suffix(suffix) {
            s = rest.to_string();
            negative = !negative;
        }
    }
    if let Some(rest) = s.strip_suffix('-') {
        negative = !negative;
        s = rest.to_string();
    }
    if let Some(rest) = s.strip_prefix(['-', '−']) {
        negative = !negative;
        s = rest.to_string();
    }
    if let Some(rest) = s.strip_prefix('+') {
        s = rest.to_string();
    }
    // Currency marks before or after the number.
    let symbol = |c: char| matches!(c, '$' | '€' | '£' | '¥');
    let code = |t: &str| t.len() == 3 && rdx_model::Currency::new(t).is_ok();
    let mut s = s.trim_start_matches(symbol);
    if s.get(..3).is_some_and(code) {
        s = &s[3..];
    }
    let mut s = s.trim_start_matches(symbol).trim_end_matches(symbol);
    if s.len() >= 3 && s.get(s.len() - 3..).is_some_and(code) {
        s = &s[..s.len() - 3];
    }
    let s = s.trim_end_matches(symbol);
    // A sign may also sit after the currency mark: `$-12.00`.
    let (s, negative) = match s.strip_prefix(['-', '−']) {
        Some(rest) => (rest, !negative),
        None => (s, negative),
    };
    if s.is_empty()
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'.' || b == b',')
    {
        return None;
    }
    if !s.as_bytes()[0].is_ascii_digit() {
        return None;
    }

    let last_sep = s.rfind(['.', ',']);
    let decimal_mark = match style {
        DecimalStyle::Point => Some('.'),
        DecimalStyle::Comma => Some(','),
        DecimalStyle::Auto => last_sep.and_then(|i| {
            let digits_after = s.len() - i - 1;
            let mark = s.as_bytes()[i] as char;
            let other = if mark == '.' { ',' } else { '.' };
            // `1,234` is a thousands separator; `1,23` or `1.234,5` is a decimal mark.
            if digits_after == 3 && !s.contains(other) && s.matches(mark).count() >= 1 {
                // Lone `x.yyy`: in statements, three decimals are rarer than a thousands mark,
                // except with `.` and no other separator, which is how 3-decimal currencies
                // print. Treat `.` as decimal there, `,` as thousands.
                (mark == '.' && s.matches('.').count() == 1).then_some('.')
            } else {
                Some(mark)
            }
        }),
    };
    let (whole, frac) = match decimal_mark.and_then(|m| s.rfind(m).map(|i| (m, i))) {
        Some((_, i)) => (&s[..i], &s[i + 1..]),
        None => (s, ""),
    };
    let thousands = match decimal_mark {
        Some('.') => ',',
        Some(_) => '.',
        None => {
            if s.contains(',') {
                ','
            } else {
                '.'
            }
        }
    };
    if frac.contains(['.', ',']) || whole.contains(decimal_mark.unwrap_or('\0')) {
        return None;
    }
    // Thousands groups, if any, are three digits each after the first.
    let groups: Vec<&str> = whole.split(thousands).collect();
    if groups.len() > 1
        && (groups[0].is_empty() || groups[0].len() > 3 || groups[1..].iter().any(|g| g.len() != 3))
    {
        return None;
    }
    let whole: String = groups.concat();
    if whole.is_empty() || whole.contains(['.', ',']) {
        return None;
    }
    let mut out = String::new();
    let is_zero = whole.bytes().all(|b| b == b'0') && frac.bytes().all(|b| b == b'0');
    if negative && !is_zero {
        out.push('-');
    }
    out.push_str(&whole);
    if !frac.is_empty() {
        out.push('.');
        out.push_str(frac);
    }
    Some(out)
}

/// Whether a printed cell is an amount with decimals, as statement tables print money. Stricter
/// than [`decimal`], so reference numbers and years aren't taken for amounts.
pub(crate) fn looks_like_money(text: &str) -> bool {
    let t = text.trim();
    let digits_then_decimals = t
        .trim_end_matches(['-', ')'])
        .trim_end_matches("CR")
        .trim_end_matches("DR")
        .trim_end();
    let bytes = digits_then_decimals.as_bytes();
    let has_cents = bytes.len() >= 3
        && matches!(bytes[bytes.len() - 3], b'.' | b',')
        && bytes[bytes.len() - 2..].iter().all(u8::is_ascii_digit);
    has_cents && decimal(t, DecimalStyle::Auto).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Option<String> {
        decimal(s, DecimalStyle::Auto)
    }

    #[test]
    fn printed_forms() {
        assert_eq!(d("289.74").as_deref(), Some("289.74"));
        assert_eq!(d("$1,221.28").as_deref(), Some("1221.28"));
        assert_eq!(d("-$12.00").as_deref(), Some("-12.00"));
        assert_eq!(d("$-12.00").as_deref(), Some("-12.00"));
        assert_eq!(d("(45.10)").as_deref(), Some("-45.10"));
        assert_eq!(d("45.10-").as_deref(), Some("-45.10"));
        assert_eq!(d("1.234,56").as_deref(), Some("1234.56"));
        assert_eq!(d("12,5").as_deref(), Some("12.5"));
        assert_eq!(d("1,234").as_deref(), Some("1234"));
        assert_eq!(d("MXN 1,000.00").as_deref(), Some("1000.00"));
        assert_eq!(d("100.00 CR").as_deref(), Some("100.00"));
        assert_eq!(d("100.00 DR").as_deref(), Some("-100.00"));
        assert_eq!(d("-0.00").as_deref(), Some("0.00"));
    }

    #[test]
    fn not_amounts() {
        for s in [
            "",
            "03/03",
            "#7435",
            "INV-004932",
            "1,23,4.00",
            "12.34.56",
            "abc",
            ".5",
        ] {
            assert_eq!(d(s), None, "{s}");
        }
    }

    #[test]
    fn explicit_styles() {
        assert_eq!(
            decimal("1.234", DecimalStyle::Comma).as_deref(),
            Some("1234")
        );
        assert_eq!(
            decimal("1,234", DecimalStyle::Comma).as_deref(),
            Some("1.234")
        );
        assert_eq!(
            decimal("1,234.5", DecimalStyle::Point).as_deref(),
            Some("1234.5")
        );
    }

    #[test]
    fn money_cells() {
        assert!(looks_like_money("1,221.28"));
        assert!(looks_like_money("(45.10)"));
        assert!(!looks_like_money("2026"));
        assert!(!looks_like_money("03/03"));
        assert!(!looks_like_money("#7435"));
    }
}
