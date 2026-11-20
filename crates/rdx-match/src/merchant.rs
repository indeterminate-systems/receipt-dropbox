//! How alike a receipt's merchant and a statement line's description are.
//!
//! Banks print a shortened, uppercased descriptor with store numbers and trip ids
//! (`PAPERLEAF SUPPLY #4159`, `BRIGHTLINE PRINT`), so the measure works on words: the share of
//! the shorter name's words that appear in the other, where a word also counts if one is a prefix
//! of the other (banks truncate). Words with digits and legal suffixes are noise and dropped.
//! Integer percent, so every host gets the same answer.

/// Words that say nothing about who the merchant is.
const NOISE: &[&str] = &[
    "the", "and", "inc", "llc", "ltd", "co", "corp", "sa", "de", "cv", "sas", "gmbh", "pos",
    "purchase", "debit", "card",
];

/// Shortest truncated word that still counts as a match (`SUP` for `Supply`).
const MIN_PREFIX: usize = 3;

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() > 1 && !w.chars().any(|c| c.is_ascii_digit()))
        .map(str::to_lowercase)
        .filter(|w| !NOISE.contains(&w.as_str()))
        .collect()
}

fn same_word(a: &str, b: &str) -> bool {
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    short == long || (short.chars().count() >= MIN_PREFIX && long.starts_with(short))
}

/// 0 to 100. Empty names are 0: they carry no evidence either way.
pub fn similarity(merchant: &str, description: &str) -> u8 {
    let a = words(merchant);
    let b = words(description);
    let (short, long) = if a.len() <= b.len() {
        (&a, &b)
    } else {
        (&b, &a)
    };
    if short.is_empty() {
        return 0;
    }
    let found = short
        .iter()
        .filter(|w| long.iter().any(|v| same_word(w, v)))
        .count();
    (found * 100 / short.len()) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bank_descriptors_match_their_merchants() {
        assert_eq!(
            similarity("Paperleaf Supply", "PAPERLEAF SUPPLY #4159"),
            100
        );
        assert_eq!(
            similarity("Metro Rideshare", "METRO RIDESHARE TRIP #2030"),
            100
        );
        // Truncated by the bank.
        assert_eq!(similarity("Brightline Print Shop", "BRIGHTLINE PRINT"), 100);
        assert_eq!(similarity("Harbor Hardware", "HARBOR HARDW"), 100);
        assert_eq!(
            similarity(
                "Clearwave Telecom S.A. de C.V.",
                "CLEARWAVE TELECOM INV-920207"
            ),
            100
        );
    }

    #[test]
    fn other_merchants_do_not() {
        assert_eq!(similarity("Paperleaf Supply", "CORNER GROCERY #0001"), 0);
        assert_eq!(similarity("Harbor Hardware", "HARBOR CAFE"), 50);
        // Two letters are not a word worth matching.
        assert_eq!(similarity("Pa", "PAPERLEAF"), 0);
        assert_eq!(similarity("", "PAPERLEAF"), 0);
        assert_eq!(similarity("#123", "PAPERLEAF"), 0);
    }
}
