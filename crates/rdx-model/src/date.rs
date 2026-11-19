use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A calendar date (proleptic Gregorian), serialized as `YYYY-MM-DD`.
///
/// Statement and receipt dates carry no time or zone, so a date type without them avoids pulling a
/// time-zone database into the WebAssembly core.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    // Field order makes the derived Ord chronological.
    year: i32,
    month: u8,
    day: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not a valid date; expected YYYY-MM-DD")]
pub struct DateError(String);

fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: i32, m: u8) -> u8 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(y) => 29,
        2 => 28,
        _ => 0,
    }
}

impl Date {
    pub fn new(year: i32, month: u8, day: u8) -> Result<Self, DateError> {
        if !(1..=9999).contains(&year)
            || !(1..=12).contains(&month)
            || day == 0
            || day > days_in_month(year, month)
        {
            return Err(DateError(format!("{year:04}-{month:02}-{day:02}")));
        }
        Ok(Date { year, month, day })
    }

    pub fn year(&self) -> i32 {
        self.year
    }
    pub fn month(&self) -> u8 {
        self.month
    }
    pub fn day(&self) -> u8 {
        self.day
    }

    /// Days since 1970-01-01 (Howard Hinnant's `days_from_civil`).
    fn to_days(self) -> i64 {
        let y = i64::from(self.year) - i64::from(self.month <= 2);
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let m = i64::from(self.month);
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(self.day) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146097 + doe - 719468
    }

    fn from_days(z: i64) -> Self {
        let z = z + 719468;
        let era = z.div_euclid(146097);
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
        let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
        Date { year, month, day }
    }

    pub fn add_days(&self, days: i64) -> Date {
        Date::from_days(self.to_days() + days)
    }

    /// `self - earlier` in days (negative if `earlier` is later).
    pub fn days_since(&self, earlier: Date) -> i64 {
        self.to_days() - earlier.to_days()
    }

    /// The last day of this date's month.
    pub fn end_of_month(&self) -> Date {
        Date {
            day: days_in_month(self.year, self.month),
            ..*self
        }
    }
}

impl FromStr for Date {
    type Err = DateError;
    fn from_str(s: &str) -> Result<Self, DateError> {
        let err = || DateError(s.to_string());
        let b = s.as_bytes();
        if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
            return Err(err());
        }
        let num = |r: std::ops::Range<usize>| -> Result<u32, DateError> {
            let part = &s[r];
            if part.bytes().all(|c| c.is_ascii_digit()) {
                part.parse().map_err(|_| err())
            } else {
                Err(err())
            }
        };
        Date::new(num(0..4)? as i32, num(5..7)? as u8, num(8..10)? as u8).map_err(|_| err())
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl fmt::Debug for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl Serialize for Date {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Date {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// An inclusive range of dates, such as a statement period.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawPeriod")]
pub struct Period {
    start: Date,
    end: Date,
}

#[derive(Deserialize)]
struct RawPeriod {
    start: Date,
    end: Date,
}

impl TryFrom<RawPeriod> for Period {
    type Error = DateError;
    fn try_from(r: RawPeriod) -> Result<Self, DateError> {
        Period::new(r.start, r.end)
    }
}

impl Period {
    pub fn new(start: Date, end: Date) -> Result<Self, DateError> {
        if end < start {
            return Err(DateError(format!(
                "period {start}..{end} ends before it starts"
            )));
        }
        Ok(Period { start, end })
    }
    pub fn start(&self) -> Date {
        self.start
    }
    pub fn end(&self) -> Date {
        self.end
    }
    pub fn contains(&self, d: Date) -> bool {
        self.start <= d && d <= self.end
    }
}
