// ── ail-stdlib::decimal ───────────────────────────────────────────────────
//
// Fixed-point decimal arithmetic for the AIL `std.decimal` module.
//
// # Design
//
// `Decimal` stores a value as an unscaled `i64` mantissa and a `u8` scale
// (number of decimal places).  All arithmetic is exact within the i64 range;
// overflow returns `Err` — no silent narrowing.
//
// Domain types (`Money<C>`, `Percentage`, `NonNegativeDecimal`) are defined
// as newtype wrappers.

use crate::numeric::NarrowError;
use std::fmt;

// ── Decimal ───────────────────────────────────────────────────────────────

/// Fixed-point decimal value: `mantissa × 10^(-scale)`.
///
/// Examples:
/// - `Decimal { mantissa: 123, scale: 2 }` represents `1.23`
/// - `Decimal { mantissa: -456, scale: 0 }` represents `-456`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decimal {
    pub mantissa: i64,
    pub scale: u8,
}

/// Error from decimal operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecimalError {
    /// Arithmetic overflow.
    Overflow,
    /// Division by zero.
    DivisionByZero,
    /// Incompatible scales without explicit rescaling.
    ScaleMismatch { lhs: u8, rhs: u8 },
    /// Value is negative where only non-negative is allowed.
    NegativeValue,
    /// Narrowing conversion failed.
    NarrowError(String),
}

impl fmt::Display for DecimalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecimalError::Overflow => write!(f, "decimal overflow"),
            DecimalError::DivisionByZero => write!(f, "division by zero"),
            DecimalError::ScaleMismatch { lhs, rhs } => {
                write!(f, "scale mismatch: {lhs} vs {rhs}")
            }
            DecimalError::NegativeValue => write!(f, "negative value"),
            DecimalError::NarrowError(msg) => write!(f, "narrow error: {msg}"),
        }
    }
}

impl std::error::Error for DecimalError {}

impl From<NarrowError> for DecimalError {
    fn from(e: NarrowError) -> Self {
        DecimalError::NarrowError(e.to_string())
    }
}

impl Decimal {
    /// Construct a new `Decimal` from mantissa and scale.
    pub fn new(mantissa: i64, scale: u8) -> Self {
        Self { mantissa, scale }
    }

    /// Construct from an integer (scale = 0).
    pub fn from_int(n: i64) -> Self {
        Self {
            mantissa: n,
            scale: 0,
        }
    }

    /// Return the scale (number of decimal places).
    pub fn scale(&self) -> u8 {
        self.scale
    }

    /// Rescale to a new scale. Returns `Err(Overflow)` if the mantissa would
    /// overflow an i64 during upscaling.
    pub fn rescale(&self, new_scale: u8) -> Result<Self, DecimalError> {
        if self.scale == new_scale {
            return Ok(*self);
        }
        if new_scale > self.scale {
            let diff = new_scale - self.scale;
            let factor = 10i64
                .checked_pow(diff as u32)
                .ok_or(DecimalError::Overflow)?;
            let new_mantissa = self
                .mantissa
                .checked_mul(factor)
                .ok_or(DecimalError::Overflow)?;
            Ok(Self {
                mantissa: new_mantissa,
                scale: new_scale,
            })
        } else {
            let diff = self.scale - new_scale;
            let factor = 10i64.pow(diff as u32);
            Ok(Self {
                mantissa: self.mantissa / factor,
                scale: new_scale,
            })
        }
    }

    /// Add two decimals. Both must have the same scale.
    pub fn add(&self, other: &Self) -> Result<Self, DecimalError> {
        if self.scale != other.scale {
            return Err(DecimalError::ScaleMismatch {
                lhs: self.scale,
                rhs: other.scale,
            });
        }
        let m = self
            .mantissa
            .checked_add(other.mantissa)
            .ok_or(DecimalError::Overflow)?;
        Ok(Self {
            mantissa: m,
            scale: self.scale,
        })
    }

    /// Subtract two decimals. Both must have the same scale.
    pub fn sub(&self, other: &Self) -> Result<Self, DecimalError> {
        if self.scale != other.scale {
            return Err(DecimalError::ScaleMismatch {
                lhs: self.scale,
                rhs: other.scale,
            });
        }
        let m = self
            .mantissa
            .checked_sub(other.mantissa)
            .ok_or(DecimalError::Overflow)?;
        Ok(Self {
            mantissa: m,
            scale: self.scale,
        })
    }

    /// Multiply two decimals. Result scale = lhs.scale + rhs.scale.
    pub fn mul(&self, other: &Self) -> Result<Self, DecimalError> {
        let m = self
            .mantissa
            .checked_mul(other.mantissa)
            .ok_or(DecimalError::Overflow)?;
        let new_scale = self
            .scale
            .checked_add(other.scale)
            .ok_or(DecimalError::Overflow)?;
        Ok(Self {
            mantissa: m,
            scale: new_scale,
        })
    }

    /// Check if this decimal is negative.
    pub fn is_negative(&self) -> bool {
        self.mantissa < 0
    }

    /// Check if this decimal is zero.
    pub fn is_zero(&self) -> bool {
        self.mantissa == 0
    }
}

// ── Domain types ──────────────────────────────────────────────────────────

/// Non-negative decimal value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NonNegativeDecimal(pub Decimal);

impl NonNegativeDecimal {
    /// Construct from a `Decimal`, returning `Err` if negative.
    pub fn new(d: Decimal) -> Result<Self, DecimalError> {
        if d.is_negative() {
            Err(DecimalError::NegativeValue)
        } else {
            Ok(Self(d))
        }
    }
}

/// Percentage value (stored as a decimal).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Percentage(pub Decimal);

impl Percentage {
    /// Construct a percentage from a decimal value.
    pub fn new(d: Decimal) -> Self {
        Self(d)
    }
}
