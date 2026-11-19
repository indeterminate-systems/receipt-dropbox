use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// An ISO 4217 currency this system knows the minor-unit exponent of.
///
/// Only currencies in [`KNOWN`] are accepted: an unknown code would need a guess at its exponent,
/// and a wrong guess scales every amount by a power of ten.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Currency {
    code: [u8; 3],
    exponent: u8,
}

/// Code and number of minor-unit digits. Extend as organizations need them.
const KNOWN: &[(&str, u8)] = &[
    ("AUD", 2),
    ("BRL", 2),
    ("CAD", 2),
    ("CHF", 2),
    ("DKK", 2),
    ("EUR", 2),
    ("GBP", 2),
    ("INR", 2),
    ("JPY", 0),
    ("KRW", 0),
    ("KWD", 3),
    ("MXN", 2),
    ("NOK", 2),
    ("NZD", 2),
    ("SEK", 2),
    ("USD", 2),
];

impl Currency {
    pub const USD: Currency = Currency {
        code: *b"USD",
        exponent: 2,
    };

    pub fn new(code: &str) -> Result<Self, MoneyError> {
        KNOWN
            .iter()
            .find(|(c, _)| *c == code)
            .map(|(c, e)| Currency {
                code: c.as_bytes().try_into().unwrap(),
                exponent: *e,
            })
            .ok_or_else(|| MoneyError::UnknownCurrency(code.to_string()))
    }

    pub fn code(&self) -> &str {
        // Codes come from KNOWN, which is ASCII.
        std::str::from_utf8(&self.code).unwrap()
    }

    /// Digits after the decimal point in major units (2 for USD, 0 for JPY).
    pub fn exponent(&self) -> u8 {
        self.exponent
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl fmt::Debug for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl Serialize for Currency {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.code())
    }
}

impl<'de> Deserialize<'de> for Currency {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let code = String::deserialize(d)?;
        Currency::new(&code).map_err(serde::de::Error::custom)
    }
}

/// An amount in integer minor units (cents for USD) of one currency. Never floating point.
///
/// On a statement line, negative means money left the account (a debit).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Money {
    minor: i64,
    currency: Currency,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MoneyError {
    #[error("unknown currency `{0}`; use an ISO 4217 code listed in rdx_model::money")]
    UnknownCurrency(String),
    #[error("cannot combine {left} and {right} amounts")]
    CurrencyMismatch { left: Currency, right: Currency },
    #[error("amount out of range")]
    Overflow,
    #[error("`{text}` is not a decimal amount with at most {exponent} decimal places")]
    Parse { text: String, exponent: u8 },
}

impl Money {
    pub const fn new(minor: i64, currency: Currency) -> Self {
        Money { minor, currency }
    }

    pub fn zero(currency: Currency) -> Self {
        Money::new(0, currency)
    }

    /// Parses `-1234.5`-style decimal text exactly. Extra decimal places are an error, never
    /// rounded.
    pub fn parse(text: &str, currency: Currency) -> Result<Self, MoneyError> {
        let err = || MoneyError::Parse {
            text: text.to_string(),
            exponent: currency.exponent,
        };
        let (negative, digits) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let (whole, frac) = digits.split_once('.').unwrap_or((digits, ""));
        let all_digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
        if whole.is_empty()
            || !all_digits(whole)
            || !all_digits(frac)
            || (digits.contains('.') && frac.is_empty())
            || frac.len() > currency.exponent as usize
        {
            return Err(err());
        }
        let scale = 10i64.pow(currency.exponent as u32);
        let whole: i64 = whole.parse().map_err(|_| err())?;
        let frac_value: i64 = if frac.is_empty() {
            0
        } else {
            let padded = format!("{frac:0<width$}", width = currency.exponent as usize);
            padded.parse().map_err(|_| err())?
        };
        let minor = whole
            .checked_mul(scale)
            .and_then(|w| w.checked_add(frac_value))
            .ok_or(MoneyError::Overflow)?;
        Ok(Money::new(if negative { -minor } else { minor }, currency))
    }

    pub fn minor(&self) -> i64 {
        self.minor
    }

    pub fn currency(&self) -> Currency {
        self.currency
    }

    pub fn is_negative(&self) -> bool {
        self.minor < 0
    }

    pub fn abs(&self) -> Money {
        Money::new(self.minor.abs(), self.currency)
    }

    pub fn checked_neg(&self) -> Result<Money, MoneyError> {
        self.minor
            .checked_neg()
            .map(|m| Money::new(m, self.currency))
            .ok_or(MoneyError::Overflow)
    }

    pub fn checked_add(&self, other: Money) -> Result<Money, MoneyError> {
        if self.currency != other.currency {
            return Err(MoneyError::CurrencyMismatch {
                left: self.currency,
                right: other.currency,
            });
        }
        self.minor
            .checked_add(other.minor)
            .map(|m| Money::new(m, self.currency))
            .ok_or(MoneyError::Overflow)
    }

    pub fn checked_sub(&self, other: Money) -> Result<Money, MoneyError> {
        self.checked_add(other.checked_neg()?)
    }

    /// Sums amounts that must all be in `currency`.
    pub fn sum<'a>(
        currency: Currency,
        amounts: impl IntoIterator<Item = &'a Money>,
    ) -> Result<Money, MoneyError> {
        amounts
            .into_iter()
            .try_fold(Money::zero(currency), |acc, m| acc.checked_add(*m))
    }

    /// The amount in major units as decimal text, without the currency (`-42.17`).
    pub fn decimal(&self) -> String {
        let exp = self.currency.exponent as usize;
        let sign = if self.minor < 0 { "-" } else { "" };
        let abs = self.minor.unsigned_abs();
        if exp == 0 {
            return format!("{sign}{abs}");
        }
        let scale = 10u64.pow(exp as u32);
        format!("{sign}{}.{:0exp$}", abs / scale, abs % scale)
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.decimal(), self.currency)
    }
}

impl fmt::Debug for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
