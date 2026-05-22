use ail_stdlib::time::{
    Instant, LocalDate, LocalDateTime, LocalTime, StdDuration, TimeZone, ZonedDateTime,
};

#[test]
fn instant_from_unix() {
    let i = Instant::from_unix(1_000_000, 500_000_000);
    assert_eq!(i.secs, 1_000_000);
    assert_eq!(i.nanos, 500_000_000);
}

#[test]
fn instant_ordering() {
    let a = Instant::from_unix(100, 0);
    let b = Instant::from_unix(200, 0);
    assert!(a < b);
}

#[test]
fn duration_from_secs() {
    let d = StdDuration::from_secs(5);
    assert_eq!(d.secs, 5);
    assert_eq!(d.nanos, 0);
    assert!(!d.is_negative());
}

#[test]
fn duration_from_millis() {
    let d = StdDuration::from_millis(1500);
    assert_eq!(d.secs, 1);
    assert_eq!(d.nanos, 500_000_000);
}

#[test]
fn duration_as_secs_f64() {
    let d = StdDuration::from_secs(2);
    assert!((d.as_secs_f64() - 2.0).abs() < 1e-9);
}

#[test]
fn timezone_utc() {
    let tz = TimeZone::utc();
    assert_eq!(tz.offset_minutes, 0);
}

#[test]
fn timezone_offset() {
    let tz = TimeZone::from_offset(-5, 0);
    assert_eq!(tz.offset_minutes, -300);
}

#[test]
fn local_date_format() {
    let d = LocalDate::new(2026, 5, 21);
    assert_eq!(d.format(), "2026-05-21");
}

#[test]
fn local_time_format() {
    let t = LocalTime::new(13, 45, 7);
    assert_eq!(t.format(), "13:45:07");
}

#[test]
fn local_datetime_format() {
    let dt = LocalDateTime::new(LocalDate::new(2026, 1, 1), LocalTime::new(0, 0, 0));
    assert_eq!(dt.format(), "2026-01-01T00:00:00");
}

#[test]
fn zoned_datetime_format_utc() {
    let dt = LocalDateTime::new(LocalDate::new(2026, 6, 15), LocalTime::new(12, 0, 0));
    let zdt = ZonedDateTime::new(dt, TimeZone::utc());
    assert_eq!(zdt.format(), "2026-06-15T12:00:00+00:00");
}

#[test]
fn zoned_datetime_format_positive_offset() {
    let dt = LocalDateTime::new(LocalDate::new(2026, 1, 1), LocalTime::new(9, 0, 0));
    let zdt = ZonedDateTime::new(dt, TimeZone::from_offset(5, 30));
    assert_eq!(zdt.format(), "2026-01-01T09:00:00+05:30");
}

#[test]
fn zoned_datetime_format_negative_offset() {
    let dt = LocalDateTime::new(LocalDate::new(2026, 1, 1), LocalTime::new(9, 0, 0));
    let zdt = ZonedDateTime::new(dt, TimeZone::from_offset(-8, 0));
    assert_eq!(zdt.format(), "2026-01-01T09:00:00-08:00");
}

#[test]
fn instant_now_is_positive() {
    let i = Instant::now();
    assert!(i.secs > 0);
}
