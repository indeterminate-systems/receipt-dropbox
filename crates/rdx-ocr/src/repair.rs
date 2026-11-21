//! Repairs to amounts OCR commonly misreads, made only where the value is not in doubt.
//!
//! Recognition confuses separators: `1,076.98` comes back as `1.076.98` or `1:553.71`. Such a
//! token is no valid number, but its last two digits after the last separator are the cents and
//! every earlier group has three digits, so the amount is certain. And a document writes its
//! decimals one way: when nearly all its amounts use `.` (or `,`), a lone `14,23` is `14.23`.
//! Everything else is left as read, and statement balance checks catch what remains.

/// Returns the repaired text and, for each repair, `(read, written)`.
pub(crate) fn amounts(text: &str) -> (String, Vec<(String, String)>) {
    let mut fixes = Vec::new();
    let out: Vec<String> = text
        .split(' ')
        .map(|tok| match amount(tok) {
            Some(fixed) => {
                fixes.push((tok.to_owned(), fixed.clone()));
                fixed
            }
            None => tok.to_owned(),
        })
        .collect();
    (out.join(" "), fixes)
}

fn amount(tok: &str) -> Option<String> {
    // Keep a leading sign or currency symbol and trailing punctuation as they are.
    let start = tok.find(|c: char| c.is_ascii_digit())?;
    let end = tok.rfind(|c: char| c.is_ascii_digit())? + 1;
    let (head, body, tail) = (&tok[..start], &tok[start..end], &tok[end..]);

    let mut groups: Vec<&str> = Vec::new();
    let mut seps: Vec<char> = Vec::new();
    let mut from = 0;
    for (i, c) in body.char_indices() {
        if !c.is_ascii_digit() {
            if !matches!(c, '.' | ',' | ':' | ';') {
                return None;
            }
            groups.push(&body[from..i]);
            seps.push(c);
            from = i + c.len_utf8();
        }
    }
    groups.push(&body[from..]);

    let (last, rest) = groups.split_last()?;
    let shape_ok = last.len() == 2
        && rest.len() >= 2
        && (1..=3).contains(&rest[0].len())
        && rest[1..].iter().all(|g| g.len() == 3);
    if !shape_ok {
        return None;
    }
    let decimal = *seps.last()?;
    let grouping = &seps[..seps.len() - 1];
    // Already well formed: one grouping character that differs from a `.` or `,` decimal.
    let well_formed = matches!(decimal, '.' | ',')
        && grouping.iter().all(|s| *s == grouping[0])
        && matches!(grouping[0], '.' | ',')
        && grouping[0] != decimal;
    if well_formed {
        return None;
    }
    let decimal = match decimal {
        '.' | ',' => decimal,
        // `:` or `;` as the decimal: take the grouping character that was read correctly.
        _ => match grouping.iter().find(|s| matches!(s, '.' | ',')) {
            Some('.') => ',',
            Some(_) => '.',
            None => return None,
        },
    };
    let grouping = if decimal == '.' { ',' } else { '.' };
    let mut fixed = String::from(head);
    fixed.push_str(&rest.join(&grouping.to_string()));
    fixed.push(decimal);
    fixed.push_str(last);
    fixed.push_str(tail);
    Some(fixed)
}

/// The decimal separator nearly every amount in `texts` uses, if there is one: at least five
/// amounts, and at most one in five written the other way.
pub(crate) fn decimal_convention<'a>(texts: impl Iterator<Item = &'a str>) -> Option<char> {
    let (mut point, mut comma) = (0usize, 0usize);
    for tok in texts.flat_map(|t| t.split(' ')) {
        match decimal_of(tok) {
            Some('.') => point += 1,
            Some(',') => comma += 1,
            _ => {}
        }
    }
    let (n, other, c) = if point >= comma {
        (point, comma, '.')
    } else {
        (comma, point, ',')
    };
    (n >= 5 && other * 5 <= n + other).then_some(c)
}

/// Rewrites amounts with a single separator before two digits to the document's convention.
pub(crate) fn to_convention(text: &str, decimal: char) -> (String, Vec<(String, String)>) {
    let mut fixes = Vec::new();
    let out: Vec<String> = text
        .split(' ')
        .map(|tok| {
            let body = tok.trim_start_matches(['-', '+', '$']);
            let simple = body.len() >= 4
                && body.bytes().filter(|b| !b.is_ascii_digit()).count() == 1
                && body.as_bytes()[body.len() - 3] != decimal as u8
                && matches!(body.as_bytes()[body.len() - 3], b'.' | b',')
                && body.bytes().last().is_some_and(|b| b.is_ascii_digit());
            if simple {
                let cut = tok.len() - 3;
                let fixed = format!("{}{decimal}{}", &tok[..cut], &tok[cut + 1..]);
                fixes.push((tok.to_owned(), fixed.clone()));
                fixed
            } else {
                tok.to_owned()
            }
        })
        .collect();
    (out.join(" "), fixes)
}

/// The decimal separator of a token that looks like an amount with cents.
fn decimal_of(tok: &str) -> Option<char> {
    let body = tok.trim_start_matches(['-', '+', '$']);
    let b = body.as_bytes();
    if b.len() < 4 || !b.iter().all(|c| c.is_ascii_digit() || *c == b'.' || *c == b',') {
        return None;
    }
    let sep = b[b.len() - 3];
    (matches!(sep, b'.' | b',')
        && b[b.len() - 2..].iter().all(u8::is_ascii_digit)
        && b[0].is_ascii_digit())
    .then_some(sep as char)
}

#[cfg(test)]
mod tests {
    use super::{amounts, decimal_convention, to_convention};

    #[test]
    fn a_lone_amount_written_against_the_documents_convention_follows_it() {
        let lines = ["289.74 454.81", "279.89 174.92", "14,23 411.54", "$1,076.98 1302.86"];
        assert_eq!(decimal_convention(lines.iter().copied()), Some('.'));
        assert_eq!(to_convention("14,23", '.').0, "14.23");
        assert_eq!(to_convention("-$14,23", '.').0, "-$14.23");
        // Grouping, dates and times are not decimals.
        for s in ["1,302", "03/13", "12:30", "1,302.86", "2026-03-13"] {
            assert_eq!(to_convention(s, '.').0, s, "{s}");
        }
        // Too few amounts, or no clear majority: no convention.
        assert_eq!(decimal_convention(["1.00 2,00"].into_iter()), None);
        let mixed = ["1.00 2.00 3.00 4.00 5.00", "6,00 7,00"];
        assert_eq!(decimal_convention(mixed.into_iter()), None);
    }

    fn fix(s: &str) -> String {
        amounts(s).0
    }

    #[test]
    fn misread_separators_are_repaired_when_the_value_is_certain() {
        assert_eq!(fix("$1.076.98"), "$1,076.98");
        assert_eq!(fix("$1:553.71"), "$1,553.71");
        assert_eq!(fix("-12;345,678.90"), "-12,345,678.90");
        assert_eq!(fix("1,076,98"), "1.076,98");
        assert_eq!(fix("Total 1.234.567,89"), "Total 1.234.567,89");
    }

    #[test]
    fn anything_doubtful_is_left_as_read() {
        for s in [
            "1,076.98",
            "1.076,98",
            "454.81",
            "295 50",
            "2026-03-19",
            "03/13",
            "12:30",
            "1.2.3",
            "10:45:00",
            "EKU9003173C9",
            "1,07.98",
        ] {
            assert_eq!(fix(s), s, "{s}");
        }
    }

    #[test]
    fn repairs_are_reported() {
        let (text, fixes) = amounts("Deposits $1:553.71 total");
        assert_eq!(text, "Deposits $1,553.71 total");
        assert_eq!(fixes, [("$1:553.71".into(), "$1,553.71".into())]);
    }
}
