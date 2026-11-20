//! Dates as statements print them: ISO, numeric in either order, and with month names in English
//! or Spanish.

use rdx_model::Date;

use crate::DateOrder;

/// `Mar`, `March`, `mar.`, `ENE`, `septiembre` → month number.
pub(crate) fn month_name(s: &str) -> Option<u8> {
    let s = s.trim_end_matches('.').to_lowercase();
    if s.len() < 3 {
        return None;
    }
    const NAMES: [(&str, &str, u8); 12] = [
        ("january", "enero", 1),
        ("february", "febrero", 2),
        ("march", "marzo", 3),
        ("april", "abril", 4),
        ("may", "mayo", 5),
        ("june", "junio", 6),
        ("july", "julio", 7),
        ("august", "agosto", 8),
        ("september", "septiembre", 9),
        ("october", "octubre", 10),
        ("november", "noviembre", 11),
        ("december", "diciembre", 12),
    ];
    NAMES
        .iter()
        .find(|(en, es, _)| {
            en.starts_with(&s) || es.starts_with(&s) || (s == "sept" && *en == "september")
        })
        .map(|(_, _, m)| *m)
}

fn year(s: &str) -> Option<i32> {
    if !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    match s.len() {
        4 => s.parse().ok(),
        // Two-digit years on statements are this century.
        2 => s.parse::<i32>().ok().map(|y| 2000 + y),
        _ => None,
    }
}

fn small(s: &str) -> Option<u8> {
    (!s.is_empty() && s.len() <= 2 && s.bytes().all(|b| b.is_ascii_digit()))
        .then(|| s.parse().ok())
        .flatten()
}

/// A complete date: ISO `2026-03-01`, numeric `03/01/2026` in the given order, or with a month
/// name (`Mar 1, 2026`, `1 Mar 2026`, `01-MAR-26`, `1 de marzo de 2026`).
pub(crate) fn parse(s: &str, order: DateOrder) -> Option<Date> {
    let s = s.trim();
    if let Ok(d) = s.parse::<Date>() {
        return Some(d);
    }
    let parts: Vec<&str> = s
        .split(['/', '-', '.', ' ', ','])
        .filter(|p| !p.is_empty() && *p != "de" && *p != "del")
        .collect();
    if parts.len() != 3 {
        return None;
    }
    let named = parts
        .iter()
        .position(|p| month_name(p).is_some() && small(p).is_none());
    let (y, m, d) = match named {
        Some(0) => (year(parts[2])?, month_name(parts[0])?, small(parts[1])?),
        Some(1) => (year(parts[2])?, month_name(parts[1])?, small(parts[0])?),
        Some(_) => return None,
        None => {
            let (a, b, c) = (parts[0], parts[1], parts[2]);
            match order {
                DateOrder::Ymd => (year(a)?, small(b)?, small(c)?),
                DateOrder::Mdy => (year(c)?, small(a)?, small(b)?),
                DateOrder::Dmy => (year(c)?, small(b)?, small(a)?),
            }
        }
    };
    Date::new(y, m, d).ok()
}

/// A day and month without a year (`03/14`, `14 Mar`), as transaction tables print them.
pub(crate) fn day_month(s: &str, order: DateOrder) -> Option<(u8, u8)> {
    let parts: Vec<&str> = s
        .trim()
        .split(['/', '-', '.', ' '])
        .filter(|p| !p.is_empty())
        .collect();
    let [a, b] = parts.as_slice() else {
        return None;
    };
    let (m, d) = match (
        month_name(a).filter(|_| small(a).is_none()),
        month_name(b).filter(|_| small(b).is_none()),
    ) {
        (Some(m), None) => (m, small(b)?),
        (None, Some(m)) => (m, small(a)?),
        _ => match order {
            DateOrder::Dmy => (small(b)?, small(a)?),
            _ => (small(a)?, small(b)?),
        },
    };
    ((1..=12).contains(&m) && (1..=31).contains(&d)).then_some((m, d))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(s: &str) -> Date {
        s.parse().unwrap()
    }

    #[test]
    fn forms() {
        assert_eq!(
            parse("2026-03-01", DateOrder::Mdy),
            Some(date("2026-03-01"))
        );
        assert_eq!(
            parse("03/14/2026", DateOrder::Mdy),
            Some(date("2026-03-14"))
        );
        assert_eq!(
            parse("14/03/2026", DateOrder::Dmy),
            Some(date("2026-03-14"))
        );
        assert_eq!(
            parse("Mar 1, 2026", DateOrder::Mdy),
            Some(date("2026-03-01"))
        );
        assert_eq!(
            parse("1 March 2026", DateOrder::Mdy),
            Some(date("2026-03-01"))
        );
        assert_eq!(parse("01-MAR-26", DateOrder::Mdy), Some(date("2026-03-01")));
        assert_eq!(
            parse("1 de marzo de 2026", DateOrder::Dmy),
            Some(date("2026-03-01"))
        );
        assert_eq!(parse("31/02/2026", DateOrder::Dmy), None);
        assert_eq!(parse("LANTERN CAFE", DateOrder::Mdy), None);
    }

    #[test]
    fn day_and_month() {
        assert_eq!(day_month("03/14", DateOrder::Mdy), Some((3, 14)));
        assert_eq!(day_month("14/03", DateOrder::Dmy), Some((3, 14)));
        assert_eq!(day_month("14 MAR", DateOrder::Dmy), Some((3, 14)));
        assert_eq!(day_month("Mar 14", DateOrder::Mdy), Some((3, 14)));
        assert_eq!(day_month("13/14", DateOrder::Mdy), None);
        assert_eq!(day_month("#7435", DateOrder::Mdy), None);
    }
}
