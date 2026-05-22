use ail_stdlib::decimal::{Decimal, DecimalError, NonNegativeDecimal, Percentage};

#[test]
fn decimal_from_int() {
    let d = Decimal::from_int(42);
    assert_eq!(d.mantissa, 42);
    assert_eq!(d.scale(), 0);
}

#[test]
fn decimal_add_same_scale() {
    let a = Decimal::new(100, 2); // 1.00
    let b = Decimal::new(50, 2); // 0.50
    let c = a.add(&b).unwrap();
    assert_eq!(c.mantissa, 150);
    assert_eq!(c.scale(), 2);
}

#[test]
fn decimal_add_scale_mismatch() {
    let a = Decimal::new(100, 2);
    let b = Decimal::new(50, 1);
    assert_eq!(
        a.add(&b),
        Err(DecimalError::ScaleMismatch { lhs: 2, rhs: 1 })
    );
}

#[test]
fn decimal_sub() {
    let a = Decimal::new(200, 2);
    let b = Decimal::new(75, 2);
    let c = a.sub(&b).unwrap();
    assert_eq!(c.mantissa, 125);
}

#[test]
fn decimal_mul() {
    let a = Decimal::new(10, 1); // 1.0
    let b = Decimal::new(20, 1); // 2.0
    let c = a.mul(&b).unwrap();
    assert_eq!(c.mantissa, 200);
    assert_eq!(c.scale(), 2); // scale = 1+1
}

#[test]
fn decimal_rescale_up() {
    let d = Decimal::new(1, 0); // 1
    let r = d.rescale(2).unwrap();
    assert_eq!(r.mantissa, 100);
    assert_eq!(r.scale(), 2);
}

#[test]
fn decimal_rescale_down() {
    let d = Decimal::new(150, 2); // 1.50
    let r = d.rescale(1).unwrap();
    assert_eq!(r.mantissa, 15);
    assert_eq!(r.scale(), 1);
}

#[test]
fn decimal_is_negative() {
    assert!(Decimal::new(-1, 0).is_negative());
    assert!(!Decimal::new(0, 0).is_negative());
    assert!(!Decimal::new(1, 0).is_negative());
}

#[test]
fn non_negative_decimal_rejects_negative() {
    let neg = Decimal::new(-1, 0);
    assert_eq!(
        NonNegativeDecimal::new(neg),
        Err(DecimalError::NegativeValue)
    );
}

#[test]
fn non_negative_decimal_accepts_zero_and_positive() {
    assert!(NonNegativeDecimal::new(Decimal::new(0, 0)).is_ok());
    assert!(NonNegativeDecimal::new(Decimal::new(5, 2)).is_ok());
}

#[test]
fn percentage_wraps_decimal() {
    let p = Percentage::new(Decimal::new(75, 2)); // 0.75 = 75%
    assert_eq!(p.0.mantissa, 75);
}
