// SPDX-License-Identifier: Apache-2.0
//! Exact money: US dollars held as integer micro-dollars (1 USD =
//! 1,000,000). Nothing here touches a floating-point number. Decimal
//! strings are parsed digit by digit, token prices multiply in 128-bit
//! integers and round *up*, so the ledger never charges itself less than
//! the provider will.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use thiserror::Error;

/// Micro-dollars in one dollar.
pub const MICROS_PER_USD: u64 = 1_000_000;
/// Largest number of integer digits a decimal string may carry (a
/// trillion dollars; keeps every product inside `u128`).
const MAX_INTEGER_DIGITS: usize = 12;

/// A non-negative amount of US dollars with micro-dollar precision.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Usd(u64);

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MoneyError {
    #[error("not a decimal amount: {0:?} (digits with at most one '.', no sign, no exponent)")]
    NotDecimal(String),
    #[error("more than 6 decimal places: {0:?}")]
    TooPrecise(String),
    #[error("amount too large: {0:?}")]
    TooLarge(String),
}

impl Usd {
    pub const ZERO: Usd = Usd(0);

    /// `micros` micro-dollars.
    pub const fn from_micros(micros: u64) -> Self {
        Usd(micros)
    }

    /// Whole dollars.
    pub const fn from_dollars(dollars: u64) -> Self {
        Usd(dollars * MICROS_PER_USD)
    }

    pub const fn micros(self) -> u64 {
        self.0
    }

    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// Parses a plain decimal such as `10`, `10.00`, `0.25` or
    /// `4.000001` exactly. Signs, exponents, thousands separators and more
    /// than six decimals are refused rather than rounded.
    pub fn parse(text: &str) -> Result<Self, MoneyError> {
        let trimmed = text.trim();
        let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
        let (int_part, frac_part) = match trimmed.split_once('.') {
            // Both sides of a point must carry digits: "1." and ".5" are refused.
            Some((i, f)) if digits(i) && digits(f) => (i, f),
            None if digits(trimmed) => (trimmed, ""),
            _ => return Err(MoneyError::NotDecimal(text.to_string())),
        };
        if frac_part.len() > 6 {
            return Err(MoneyError::TooPrecise(text.to_string()));
        }
        if int_part.trim_start_matches('0').len() > MAX_INTEGER_DIGITS {
            return Err(MoneyError::TooLarge(text.to_string()));
        }
        let mut micros: u64 = 0;
        for b in int_part.bytes() {
            micros = micros * 10 + u64::from(b - b'0');
        }
        micros *= MICROS_PER_USD;
        let mut frac: u64 = 0;
        for b in frac_part.bytes() {
            frac = frac * 10 + u64::from(b - b'0');
        }
        for _ in frac_part.len()..6 {
            frac *= 10;
        }
        Ok(Usd(micros + frac))
    }

    pub fn checked_add(self, other: Usd) -> Option<Usd> {
        self.0.checked_add(other.0).map(Usd)
    }

    /// Addition that sticks at the maximum instead of wrapping: a sum
    /// that overflows reads as "more than we can represent", which the
    /// caps treat as exhausted.
    pub fn saturating_add(self, other: Usd) -> Usd {
        Usd(self.0.saturating_add(other.0))
    }

    pub fn saturating_sub(self, other: Usd) -> Usd {
        Usd(self.0.saturating_sub(other.0))
    }

    /// The price of `tokens` at `per_million` dollars per million tokens,
    /// rounded up to the next micro-dollar.
    pub fn price(tokens: u64, per_million: Usd) -> Usd {
        let product = u128::from(tokens) * u128::from(per_million.0);
        let micros = product.div_ceil(u128::from(MICROS_PER_USD));
        Usd(u64::try_from(micros).unwrap_or(u64::MAX))
    }

    /// `self × num / den`, rounded down; `den` must be non-zero.
    pub fn mul_div(self, num: u64, den: u64) -> Usd {
        assert!(den != 0, "division by zero");
        let value = u128::from(self.0) * u128::from(num) / u128::from(den);
        Usd(u64::try_from(value).unwrap_or(u64::MAX))
    }

    /// `self × ppm / 1,000,000`, rounded down.
    pub fn mul_ppm(self, ppm: Ppm) -> Usd {
        self.mul_div(u64::from(ppm.0), MICROS_PER_USD)
    }

    /// Fixed six-decimal rendering, e.g. `1.880000`.
    pub fn to_fixed(self) -> String {
        format!("{}.{:06}", self.0 / MICROS_PER_USD, self.0 % MICROS_PER_USD)
    }
}

impl fmt::Display for Usd {
    /// At least two decimals, more only when they are non-zero:
    /// `10.00`, `1.88`, `0.000024`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let dollars = self.0 / MICROS_PER_USD;
        let mut frac = format!("{:06}", self.0 % MICROS_PER_USD);
        while frac.len() > 2 && frac.ends_with('0') {
            frac.pop();
        }
        write!(f, "{dollars}.{frac}")
    }
}

impl fmt::Debug for Usd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Usd({})", self.to_fixed())
    }
}

impl Serialize for Usd {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_fixed())
    }
}

impl<'de> Deserialize<'de> for Usd {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Usd::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// A fraction in parts per million (0 ..= 1,000,000), parsed from a
/// decimal such as `0.25`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Ppm(u32);

impl Ppm {
    pub const ONE: Ppm = Ppm(1_000_000);

    pub const fn parts_per_million(self) -> u32 {
        self.0
    }

    /// Parses `0`, `0.25`, `1`, `1.0`; anything above one or with more than
    /// six decimals is refused.
    pub fn parse(text: &str) -> Result<Self, MoneyError> {
        let usd = Usd::parse(text)?;
        if usd.0 > u64::from(Self::ONE.0) {
            return Err(MoneyError::TooLarge(text.to_string()));
        }
        Ok(Ppm(
            u32::try_from(usd.0).expect("bounded above by one million")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_decimal_strings_exactly() {
        assert_eq!(Usd::parse("10").unwrap().micros(), 10_000_000);
        assert_eq!(Usd::parse("10.00").unwrap().micros(), 10_000_000);
        assert_eq!(Usd::parse("0.20").unwrap().micros(), 200_000);
        assert_eq!(Usd::parse("4.000001").unwrap().micros(), 4_000_001);
        assert_eq!(Usd::parse(" 60.5 ").unwrap().micros(), 60_500_000);
        assert_eq!(Usd::parse("0").unwrap(), Usd::ZERO);
        for bad in [
            "",
            ".",
            "1.",
            ".5",
            "-1",
            "+1",
            "1e3",
            "1,000",
            "abc",
            "1.0000001",
        ] {
            assert!(Usd::parse(bad).is_err(), "{bad:?} should be refused");
        }
        assert!(matches!(
            Usd::parse("1.2345678"),
            Err(MoneyError::TooPrecise(_))
        ));
        assert!(matches!(
            Usd::parse("1000000000000000"),
            Err(MoneyError::TooLarge(_))
        ));
    }

    #[test]
    fn prices_round_up_and_never_use_floats() {
        let input = Usd::parse("4.00").unwrap();
        let output = Usd::parse("20.00").unwrap();
        let cached = Usd::parse("0.20").unwrap();
        assert_eq!(Usd::price(1_000_000, input).micros(), 4_000_000);
        assert_eq!(Usd::price(1, input).micros(), 4);
        assert_eq!(Usd::price(1, output).micros(), 20);
        // 0.2 micro-dollars rounds up to a whole micro-dollar.
        assert_eq!(Usd::price(1, cached).micros(), 1);
        assert_eq!(Usd::price(1_000, cached).micros(), 200);
        assert_eq!(Usd::price(0, output), Usd::ZERO);
        assert_eq!(Usd::price(94_000, output).to_fixed(), "1.880000");
    }

    #[test]
    fn display_and_serde_round_trip() {
        let v = Usd::parse("1.88").unwrap();
        assert_eq!(v.to_string(), "1.88");
        assert_eq!(Usd::parse("10").unwrap().to_string(), "10.00");
        assert_eq!(Usd::from_micros(24).to_string(), "0.000024");
        assert_eq!(Usd::from_micros(1_500_000).to_string(), "1.50");
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(json, "\"1.880000\"");
        assert_eq!(serde_json::from_str::<Usd>(&json).unwrap(), v);
        assert!(
            serde_json::from_str::<Usd>("1.88").is_err(),
            "numbers are refused"
        );
    }

    #[test]
    fn ppm_parses_fractions() {
        assert_eq!(Ppm::parse("0.25").unwrap().parts_per_million(), 250_000);
        assert_eq!(Ppm::parse("1").unwrap(), Ppm::ONE);
        assert!(Ppm::parse("1.5").is_err());
        assert_eq!(
            Usd::parse("10")
                .unwrap()
                .mul_ppm(Ppm::parse("0.25").unwrap()),
            Usd::parse("2.5").unwrap()
        );
        assert_eq!(
            Usd::parse("10").unwrap().mul_div(3600, 86_400),
            Usd::parse("0.416666").unwrap()
        );
    }
}
