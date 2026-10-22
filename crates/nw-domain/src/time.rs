//! Checked exact time; JSON integers are decimal strings on every client.
use crate::{DomainError, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "WireRational", into = "WireRational")]
pub struct Rational { num: i64, den: i64 }
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct WireRational { num: String, den: String }
impl TryFrom<WireRational> for Rational {
    type Error = DomainError;
    fn try_from(v: WireRational) -> Result<Self> {
        Self::new(v.num.parse().map_err(|_| invalid())?, v.den.parse().map_err(|_| invalid())?)
    }
}
impl From<Rational> for WireRational {
    fn from(v: Rational) -> Self { Self { num: v.num.to_string(), den: v.den.to_string() } }
}
fn invalid() -> DomainError { DomainError::new("INVALID_TIME", "Expected finite i64 numerator and positive denominator") }
fn gcd(mut a: i128, mut b: i128) -> i128 { while b != 0 { (a,b) = (b,a%b); } a.abs() }
impl Rational {
    pub const ZERO: Self = Self { num: 0, den: 1 };
    pub fn new(num: i64, den: i64) -> Result<Self> { Self::from_wide(i128::from(num), i128::from(den)) }
    fn from_wide(num: i128, den: i128) -> Result<Self> {
        if den <= 0 { return Err(invalid()); }
        let g = gcd(num, den);
        Ok(Self { num: (num/g).try_into().map_err(|_| invalid())?, den: (den/g).try_into().map_err(|_| invalid())? })
    }
    pub fn numerator(self) -> i64 { self.num }
    pub fn denominator(self) -> i64 { self.den }
    pub fn checked_add(self, rhs: Self) -> Result<Self> { Self::from_wide(i128::from(self.num)*i128::from(rhs.den)+i128::from(rhs.num)*i128::from(self.den), i128::from(self.den)*i128::from(rhs.den)) }
    pub fn checked_sub(self, rhs: Self) -> Result<Self> { Self::from_wide(i128::from(self.num)*i128::from(rhs.den)-i128::from(rhs.num)*i128::from(self.den), i128::from(self.den)*i128::from(rhs.den)) }
    pub fn checked_mul(self, rhs: Self) -> Result<Self> { Self::from_wide(i128::from(self.num)*i128::from(rhs.num), i128::from(self.den)*i128::from(rhs.den)) }
}
impl PartialOrd for Rational { fn partial_cmp(&self, rhs: &Self) -> Option<Ordering> { Some(self.cmp(rhs)) } }
impl Ord for Rational { fn cmp(&self, rhs: &Self) -> Ordering { (i128::from(self.num)*i128::from(rhs.den)).cmp(&(i128::from(rhs.num)*i128::from(self.den))) } }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Clock { Source, Clip, Sequence, Output }
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExactTime { pub domain: Clock, #[serde(flatten)] pub value: Rational }
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TimeRange { pub start: Rational, pub end: Rational }
impl TimeRange {
    pub fn validate(&self) -> Result<()> {
        if self.start < Rational::ZERO || self.end <= self.start { return Err(invalid()); } Ok(())
    }
    pub fn contains(self, time: Rational) -> bool { self.start <= time && time < self.end }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_vfr_keep_and_retime() {
        let ms = |n| Rational::new(n, 1000).unwrap();
        let ranges = [TimeRange { start: ms(40), end: ms(160) }, TimeRange { start: ms(220), end: ms(330) }];
        let frames = [0,40,100,160,220,300];
        let mut output = Vec::new();
        let mut base = Rational::ZERO;
        for range in ranges {
            for t in frames { if range.contains(ms(t)) { output.push(base.checked_add(ms(t).checked_sub(range.start).unwrap()).unwrap().checked_mul(Rational::new(1,2).unwrap()).unwrap()); } }
            base = base.checked_add(range.end.checked_sub(range.start).unwrap()).unwrap();
        }
        assert_eq!(output, [0,30,60,100].map(ms));
        assert_eq!(base.checked_mul(Rational::new(1,2).unwrap()).unwrap(), ms(115));
    }
    #[test]
    fn wire_is_lossless_and_invalid_values_fail() {
        let value = Rational::new(i64::MIN, 2).unwrap();
        assert_eq!(serde_json::from_str::<Rational>(&serde_json::to_string(&value).unwrap()).unwrap(), value);
        assert!(Rational::new(1, 0).is_err());
        assert!(Rational::new(i64::MAX, 1).unwrap().checked_add(Rational::new(1,1).unwrap()).is_err());
        assert!(serde_json::from_str::<Rational>(r#"{"num":"NaN","den":"1"}"#).is_err());
    }
}
