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

// ── Decimal contracts ───────────────────────────────────────────────────

/// Stable decimal operation families for diagnostics and registry checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DecimalOperationKind {
    Rescale,
    Add,
    Subtract,
    Multiply,
    NonNegative,
}

impl DecimalOperationKind {
    pub fn label(self) -> &'static str {
        match self {
            DecimalOperationKind::Rescale => "rescale",
            DecimalOperationKind::Add => "add",
            DecimalOperationKind::Subtract => "subtract",
            DecimalOperationKind::Multiply => "multiply",
            DecimalOperationKind::NonNegative => "non-negative",
        }
    }
}

/// Redacted decimal value shape. Does not expose mantissas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DecimalValueShape {
    Negative,
    Zero,
    Positive,
}

impl DecimalValueShape {
    pub fn label(self) -> &'static str {
        match self {
            DecimalValueShape::Negative => "negative",
            DecimalValueShape::Zero => "zero",
            DecimalValueShape::Positive => "positive",
        }
    }
}

/// Stable scale relationship categories.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DecimalScaleShape {
    Same,
    Different,
    Upscale,
    Downscale,
}

impl DecimalScaleShape {
    pub fn label(self) -> &'static str {
        match self {
            DecimalScaleShape::Same => "same-scale",
            DecimalScaleShape::Different => "different-scale",
            DecimalScaleShape::Upscale => "upscale",
            DecimalScaleShape::Downscale => "downscale",
        }
    }
}

/// Stable decimal contract issue kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DecimalIssueKind {
    Overflow,
    DivisionByZero,
    ScaleMismatch,
    NegativeValue,
    Narrowing,
}

impl DecimalIssueKind {
    pub fn code(self) -> &'static str {
        match self {
            DecimalIssueKind::Overflow => "DECIMAL_OVERFLOW",
            DecimalIssueKind::DivisionByZero => "DECIMAL_DIVISION_BY_ZERO",
            DecimalIssueKind::ScaleMismatch => "DECIMAL_SCALE_MISMATCH",
            DecimalIssueKind::NegativeValue => "DECIMAL_NEGATIVE_VALUE",
            DecimalIssueKind::Narrowing => "DECIMAL_NARROWING",
        }
    }

    pub fn category(self) -> &'static str {
        match self {
            DecimalIssueKind::Overflow | DecimalIssueKind::Narrowing => "range-shape",
            DecimalIssueKind::DivisionByZero => "zero-shape",
            DecimalIssueKind::ScaleMismatch => "scale-shape",
            DecimalIssueKind::NegativeValue => "sign-shape",
        }
    }
}

/// Redacted decimal diagnostic issue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecimalIssue {
    pub operation: DecimalOperationKind,
    pub kind: DecimalIssueKind,
    pub code: &'static str,
    pub category: &'static str,
}

impl DecimalIssue {
    pub fn new(operation: DecimalOperationKind, kind: DecimalIssueKind) -> Self {
        Self {
            operation,
            kind,
            code: kind.code(),
            category: kind.category(),
        }
    }

    pub fn diagnostic_key(&self) -> String {
        format!(
            "std.decimal.{}:{}:{}",
            self.operation.label(),
            self.category,
            self.code
        )
    }
}

/// Descriptor for decimal operations. Keeps values redacted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecimalOperationDescriptor {
    pub operation: DecimalOperationKind,
    pub lhs_shape: DecimalValueShape,
    pub rhs_shape: Option<DecimalValueShape>,
    pub scale_shape: DecimalScaleShape,
}

impl DecimalOperationDescriptor {
    pub fn binary(operation: DecimalOperationKind, lhs: Decimal, rhs: Decimal) -> Self {
        Self {
            operation,
            lhs_shape: decimal_value_shape(lhs),
            rhs_shape: Some(decimal_value_shape(rhs)),
            scale_shape: if lhs.scale == rhs.scale {
                DecimalScaleShape::Same
            } else {
                DecimalScaleShape::Different
            },
        }
    }

    pub fn rescale(value: Decimal, target_scale: u8) -> Self {
        Self {
            operation: DecimalOperationKind::Rescale,
            lhs_shape: decimal_value_shape(value),
            rhs_shape: None,
            scale_shape: decimal_rescale_shape(value.scale, target_scale),
        }
    }

    pub fn non_negative(value: Decimal) -> Self {
        Self {
            operation: DecimalOperationKind::NonNegative,
            lhs_shape: decimal_value_shape(value),
            rhs_shape: None,
            scale_shape: DecimalScaleShape::Same,
        }
    }

    pub fn diagnostic_key(&self) -> String {
        let rhs = self
            .rhs_shape
            .map(DecimalValueShape::label)
            .unwrap_or("none");
        format!(
            "std.decimal.{}:{}:{}:{}",
            self.operation.label(),
            self.lhs_shape.label(),
            rhs,
            self.scale_shape.label()
        )
    }
}

pub fn decimal_value_shape(value: Decimal) -> DecimalValueShape {
    if value.mantissa < 0 {
        DecimalValueShape::Negative
    } else if value.mantissa == 0 {
        DecimalValueShape::Zero
    } else {
        DecimalValueShape::Positive
    }
}

pub fn decimal_rescale_shape(current: u8, target: u8) -> DecimalScaleShape {
    if target == current {
        DecimalScaleShape::Same
    } else if target > current {
        DecimalScaleShape::Upscale
    } else {
        DecimalScaleShape::Downscale
    }
}

pub fn decimal_issue_for(operation: DecimalOperationKind, error: &DecimalError) -> DecimalIssue {
    let kind = match error {
        DecimalError::Overflow => DecimalIssueKind::Overflow,
        DecimalError::DivisionByZero => DecimalIssueKind::DivisionByZero,
        DecimalError::ScaleMismatch { .. } => DecimalIssueKind::ScaleMismatch,
        DecimalError::NegativeValue => DecimalIssueKind::NegativeValue,
        DecimalError::NarrowError(_) => DecimalIssueKind::Narrowing,
    };
    DecimalIssue::new(operation, kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_descriptors_are_redacted_and_stable() {
        let lhs = Decimal::new(1250, 2);
        let rhs = Decimal::new(-5, 0);
        let descriptor = DecimalOperationDescriptor::binary(DecimalOperationKind::Add, lhs, rhs);

        assert_eq!(descriptor.lhs_shape, DecimalValueShape::Positive);
        assert_eq!(descriptor.rhs_shape, Some(DecimalValueShape::Negative));
        assert_eq!(descriptor.scale_shape, DecimalScaleShape::Different);
        assert_eq!(
            descriptor.diagnostic_key(),
            "std.decimal.add:positive:negative:different-scale"
        );
        assert!(!descriptor.diagnostic_key().contains("1250"));
    }

    #[test]
    fn decimal_errors_have_stable_issue_codes() {
        let err = Decimal::new(1, 2)
            .add(&Decimal::new(1, 3))
            .expect_err("scale mismatch");
        let issue = decimal_issue_for(DecimalOperationKind::Add, &err);

        assert_eq!(issue.kind, DecimalIssueKind::ScaleMismatch);
        assert_eq!(issue.code, "DECIMAL_SCALE_MISMATCH");
        assert_eq!(issue.category, "scale-shape");
        assert_eq!(
            issue.diagnostic_key(),
            "std.decimal.add:scale-shape:DECIMAL_SCALE_MISMATCH"
        );
    }

    #[test]
    fn non_negative_descriptor_redacts_value() {
        let value = Decimal::new(-42, 2);
        let descriptor = DecimalOperationDescriptor::non_negative(value);
        let err = NonNegativeDecimal::new(value).expect_err("negative value");
        let issue = decimal_issue_for(DecimalOperationKind::NonNegative, &err);

        assert_eq!(descriptor.lhs_shape, DecimalValueShape::Negative);
        assert_eq!(
            descriptor.diagnostic_key(),
            "std.decimal.non-negative:negative:none:same-scale"
        );
        assert_eq!(issue.code, "DECIMAL_NEGATIVE_VALUE");
        assert!(!descriptor.diagnostic_key().contains("42"));
    }
}
