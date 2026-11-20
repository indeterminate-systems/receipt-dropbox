use serde::{Deserialize, Serialize};

use rdx_model::{Currency, Date, Money, ReceiptKind};

/// The tolerances matching uses. Its JSON shape and every default are defined by the `Policy`
/// contract in `nickel/rdx/matching.ncl`; this type has no defaults of its own, so a plan that
/// arrives as JSON must spell the policy out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub amount: AmountPolicy,
    pub date: DatePolicy,
    pub merchant: MerchantPolicy,
    pub split: SplitPolicy,
    pub partial: PartialPolicy,
    pub fx: FxPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AmountPolicy {
    pub tolerance_minor: u64,
    pub tolerance_bps: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Window {
    pub days_before: u32,
    pub days_after: u32,
}

impl Window {
    /// Whether a document dated `doc` may explain a line posted on `posted`.
    pub fn contains(&self, posted: Date, doc: Date) -> bool {
        let lag = posted.days_since(doc);
        (lag >= 0 && lag <= i64::from(self.days_before))
            || (lag < 0 && -lag <= i64::from(self.days_after))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatePolicy {
    pub receipt: Window,
    pub bill: Window,
    pub refund: Window,
}

impl DatePolicy {
    pub fn window(&self, kind: ReceiptKind) -> Window {
        match kind {
            ReceiptKind::Receipt => self.receipt,
            ReceiptKind::Bill => self.bill,
            ReceiptKind::Refund => self.refund,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MerchantPolicy {
    /// Percent, 0 to 100.
    pub min_similarity: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SplitPolicy {
    pub max_parts: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartialPolicy {
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FxPolicy {
    pub tolerance_bps: u32,
    pub rates: Vec<FxRate>,
}

/// Units of `to` for one unit of `from`, on `date`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FxRate {
    pub from: Currency,
    pub to: Currency,
    /// A positive decimal string, kept as text so the audit trail shows the rate as given.
    pub rate: String,
    pub date: Date,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PolicyError {
    #[error("merchant.min_similarity is {0}; use a percentage from 0 to 100")]
    Similarity(u8),
    #[error(
        "fx rate `{0}` is not a positive decimal with at most 12 digits on each side of the point"
    )]
    Rate(String),
}

impl Policy {
    /// Re-checks what the Nickel contract checks, for policies that arrive as JSON.
    pub fn validate(&self) -> Result<(), PolicyError> {
        if self.merchant.min_similarity > 100 {
            return Err(PolicyError::Similarity(self.merchant.min_similarity));
        }
        for r in &self.fx.rates {
            Decimal::parse(&r.rate)?;
        }
        Ok(())
    }

    /// The amount tolerance for a document total, in minor units: the larger of the absolute and
    /// relative ones.
    pub(crate) fn amount_tolerance(&self, total: Money) -> i64 {
        let relative = bps_of(total.minor(), self.amount.tolerance_bps);
        relative.max(self.amount.tolerance_minor.min(i64::MAX as u64) as i64)
    }

    /// The rate for `from` → `to` dated closest to `on` (the earlier one on a tie).
    pub(crate) fn rate(&self, from: Currency, to: Currency, on: Date) -> Option<&FxRate> {
        self.fx
            .rates
            .iter()
            .filter(|r| r.from == from && r.to == to)
            .min_by_key(|r| (on.days_since(r.date).abs(), r.date))
    }
}

/// `bps` basis points of `minor`, rounded down.
pub(crate) fn bps_of(minor: i64, bps: u32) -> i64 {
    (i128::from(minor.abs()) * i128::from(bps) / 10_000).min(i128::from(i64::MAX)) as i64
}

/// A decimal rate as an integer and a power of ten, so conversion stays exact integer arithmetic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Decimal {
    digits: u128,
    scale: u32,
}

impl Decimal {
    pub(crate) fn parse(text: &str) -> Result<Self, PolicyError> {
        let err = || PolicyError::Rate(text.to_string());
        let (whole, frac) = text.split_once('.').unwrap_or((text, ""));
        let ok = |s: &str, min: usize| {
            s.len() >= min && s.len() <= 12 && s.bytes().all(|b| b.is_ascii_digit())
        };
        if !ok(whole, 1) || !ok(frac, usize::from(text.contains('.'))) {
            return Err(err());
        }
        let digits: u128 = format!("{whole}{frac}").parse().map_err(|_| err())?;
        if digits == 0 {
            return Err(err());
        }
        Ok(Decimal {
            digits,
            scale: frac.len() as u32,
        })
    }

    /// Converts `amount` into `to` at this rate, rounding half away from zero to `to`'s minor
    /// units. `None` if the result does not fit.
    pub(crate) fn convert(&self, amount: Money, to: Currency) -> Option<Money> {
        // minor_to = minor_from * rate * 10^(exp_to - exp_from)
        let mut num = i128::from(amount.minor()).checked_mul(i128::try_from(self.digits).ok()?)?;
        let mut den: i128 = 10i128.checked_pow(self.scale)?;
        let (e_from, e_to) = (
            u32::from(amount.currency().exponent()),
            u32::from(to.exponent()),
        );
        if e_to >= e_from {
            num = num.checked_mul(10i128.checked_pow(e_to - e_from)?)?;
        } else {
            den = den.checked_mul(10i128.checked_pow(e_from - e_to)?)?;
        }
        let q = num / den;
        let r = num % den;
        let rounded = if 2 * r.abs() >= den {
            q + num.signum()
        } else {
            q
        };
        Some(Money::new(i64::try_from(rounded).ok()?, to))
    }
}

#[cfg(test)]
// Amounts are written as `dollars_cents` (`10_00` is 10.00) so they read as money.
#[allow(clippy::inconsistent_digit_grouping)]
mod tests {
    use super::*;

    fn cur(c: &str) -> Currency {
        Currency::new(c).unwrap()
    }

    #[test]
    fn rates_convert_exactly_and_round_half_away_from_zero() {
        let usd_mxn = Decimal::parse("17.2345").unwrap();
        // 10.00 USD = 172.345 MXN → 172.35
        assert_eq!(
            usd_mxn.convert(Money::new(10_00, cur("USD")), cur("MXN")),
            Some(Money::new(172_35, cur("MXN")))
        );
        // Exponents differ: 1000 JPY at 0.0067 USD = 6.70 USD.
        assert_eq!(
            Decimal::parse("0.0067")
                .unwrap()
                .convert(Money::new(1000, cur("JPY")), cur("USD")),
            Some(Money::new(6_70, cur("USD")))
        );
        // And the other way: 6.70 USD at 149.25 JPY = 999.975 → 1000 JPY.
        assert_eq!(
            Decimal::parse("149.25")
                .unwrap()
                .convert(Money::new(6_70, cur("USD")), cur("JPY")),
            Some(Money::new(1000, cur("JPY")))
        );
    }

    #[test]
    fn bad_rates_are_rejected() {
        for bad in [
            "",
            "0",
            "0.000",
            "1.",
            ".5",
            "-1",
            "1e3",
            "1,5",
            "1234567890123",
        ] {
            assert!(Decimal::parse(bad).is_err(), "{bad:?}");
        }
        assert!(Decimal::parse("1").is_ok());
    }

    #[test]
    fn windows_include_their_edges() {
        let w = Window {
            days_before: 3,
            days_after: 1,
        };
        let posted = Date::new(2026, 3, 10).unwrap();
        assert!(w.contains(posted, posted.add_days(-3)));
        assert!(!w.contains(posted, posted.add_days(-4)));
        assert!(w.contains(posted, posted.add_days(1)));
        assert!(!w.contains(posted, posted.add_days(2)));
    }
}
