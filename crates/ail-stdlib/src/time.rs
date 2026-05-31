// ── ail-stdlib::time ──────────────────────────────────────────────────────
//
// Timestamp and duration types for the AIL `std.time` module.
//
// # Capabilities (from docs/stdlib.md)
//
// - clock.now   — wall-clock time (not pure)
// - clock.monotonic — monotonic duration
//
// # Rules
//
// - now() is not pure
// - no implicit global timezone
// - DST ambiguity requires policy

use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ── Clock capabilities / operation descriptors ──────────────────────────

/// Enumeration of clock capabilities required by std.time operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockCapability {
    Now,
    Monotonic,
}

impl ClockCapability {
    /// Stable runtime capability label required by the host boundary.
    pub fn label(self) -> &'static str {
        match self {
            ClockCapability::Now => "clock.now",
            ClockCapability::Monotonic => "clock.monotonic",
        }
    }
}

impl std::fmt::Display for ClockCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// Stable std.time clock sources used by diagnostics and registries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockSourceKind {
    WallClock,
    Monotonic,
}

impl ClockSourceKind {
    /// Deterministic source label for low-cardinality diagnostics.
    pub fn label(self) -> &'static str {
        match self {
            ClockSourceKind::WallClock => "wall-clock",
            ClockSourceKind::Monotonic => "monotonic",
        }
    }

    /// Capability that must be granted before reading this clock.
    pub fn required_capability(self) -> ClockCapability {
        match self {
            ClockSourceKind::WallClock => ClockCapability::Now,
            ClockSourceKind::Monotonic => ClockCapability::Monotonic,
        }
    }
}

/// Stable shape categories for std.time instants that avoid leaking values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstantShape {
    Canonical,
    NanosecondsOutOfRange,
}

impl InstantShape {
    /// Diagnostic label that does not include the timestamp itself.
    pub fn label(self) -> &'static str {
        match self {
            InstantShape::Canonical => "canonical",
            InstantShape::NanosecondsOutOfRange => "nanos-out-of-range",
        }
    }

    /// Whether this instant shape can cross the std.time boundary.
    pub fn is_allowed(self) -> bool {
        matches!(self, InstantShape::Canonical)
    }
}

/// Error produced when an instant has an unsupported structural shape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstantShapeError {
    pub shape: InstantShape,
    pub expected: &'static str,
}

impl std::fmt::Display for InstantShapeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "time instant shape mismatch: expected {}, got {}",
            self.expected,
            self.shape.label()
        )
    }
}

impl std::error::Error for InstantShapeError {}

/// Descriptor proving std.time clock reads are capability-gated, not ambient.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClockReadDescriptor {
    pub source: ClockSourceKind,
    pub capability: ClockCapability,
    pub capability_label: &'static str,
    pub monotonic: bool,
    pub grant_required: bool,
    pub ambient_access: bool,
    pub timezone_policy_explicit: bool,
}

impl ClockReadDescriptor {
    /// Build a descriptor for a clock read without granting access.
    pub fn new(source: ClockSourceKind) -> Self {
        let capability = source.required_capability();
        Self {
            source,
            capability,
            capability_label: capability.label(),
            monotonic: source == ClockSourceKind::Monotonic,
            grant_required: true,
            ambient_access: false,
            timezone_policy_explicit: source == ClockSourceKind::WallClock,
        }
    }

    /// Descriptor for reading wall-clock time.
    pub fn wall_clock() -> Self {
        Self::new(ClockSourceKind::WallClock)
    }

    /// Descriptor for reading monotonic time.
    pub fn monotonic() -> Self {
        Self::new(ClockSourceKind::Monotonic)
    }

    /// Deterministic low-cardinality descriptor suitable for logs/registries.
    pub fn diagnostic_key(&self) -> String {
        format!(
            "std.time.clock_read:{}:{}",
            self.source.label(),
            self.capability_label
        )
    }
}

/// Return the stable structural shape for an instant without exposing it.
pub fn instant_shape(instant: &Instant) -> InstantShape {
    if instant.nanos >= 1_000_000_000 {
        InstantShape::NanosecondsOutOfRange
    } else {
        InstantShape::Canonical
    }
}

/// Validate an already-classified instant shape.
pub fn validate_instant_shape(shape: InstantShape) -> Result<(), InstantShapeError> {
    if shape.is_allowed() {
        Ok(())
    } else {
        Err(InstantShapeError {
            shape,
            expected: "seconds plus nanoseconds where 0 <= nanos < 1000000000",
        })
    }
}

/// Validate an instant against std.time boundary metadata.
pub fn validate_instant_contract(instant: &Instant) -> Result<(), InstantShapeError> {
    validate_instant_shape(instant_shape(instant))
}

// ── Temporal contract diagnostics ────────────────────────────────────────

/// Stable std.time value domains for redacted diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TemporalValueKind {
    Instant,
    Duration,
    LocalDate,
    LocalTime,
    TimeZone,
}

impl TemporalValueKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Instant => "instant",
            Self::Duration => "duration",
            Self::LocalDate => "local-date",
            Self::LocalTime => "local-time",
            Self::TimeZone => "timezone",
        }
    }
}

/// Stable std.time contract issue kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TemporalIssueKind {
    NanosecondsOutOfRange,
    MonthOutOfRange,
    DayOutOfRange,
    HourOutOfRange,
    MinuteOutOfRange,
    SecondOutOfRange,
    OffsetOutOfRange,
    DurationNotNormalized,
}

impl TemporalIssueKind {
    pub const fn code(self) -> &'static str {
        match self {
            Self::NanosecondsOutOfRange => "std.time.nanos.out_of_range",
            Self::MonthOutOfRange => "std.time.date.month_out_of_range",
            Self::DayOutOfRange => "std.time.date.day_out_of_range",
            Self::HourOutOfRange => "std.time.time.hour_out_of_range",
            Self::MinuteOutOfRange => "std.time.time.minute_out_of_range",
            Self::SecondOutOfRange => "std.time.time.second_out_of_range",
            Self::OffsetOutOfRange => "std.time.timezone.offset_out_of_range",
            Self::DurationNotNormalized => "std.time.duration.not_normalized",
        }
    }

    pub const fn category(self) -> &'static str {
        match self {
            Self::NanosecondsOutOfRange
            | Self::MonthOutOfRange
            | Self::DayOutOfRange
            | Self::HourOutOfRange
            | Self::MinuteOutOfRange
            | Self::SecondOutOfRange
            | Self::OffsetOutOfRange => "range",
            Self::DurationNotNormalized => "normalization",
        }
    }
}

/// Machine-readable std.time contract issue that exposes shape, not values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TemporalIssue {
    pub value: TemporalValueKind,
    pub kind: TemporalIssueKind,
    pub field: &'static str,
    pub expected: &'static str,
}

impl TemporalIssue {
    pub const fn code(&self) -> &'static str {
        self.kind.code()
    }

    pub const fn category(&self) -> &'static str {
        self.kind.category()
    }

    pub const fn value_label(&self) -> &'static str {
        self.value.label()
    }
}

fn temporal_issue(
    value: TemporalValueKind,
    kind: TemporalIssueKind,
    field: &'static str,
    expected: &'static str,
) -> TemporalIssue {
    TemporalIssue {
        value,
        kind,
        field,
        expected,
    }
}

fn sort_temporal_issues(issues: &mut Vec<TemporalIssue>) {
    issues.sort_by_key(|issue| (issue.value, issue.field, issue.kind));
    issues.dedup();
}

/// Validate a local date without leaking the date value.
pub fn diagnose_local_date(date: &LocalDate) -> Vec<TemporalIssue> {
    let mut issues = Vec::new();
    if !(1..=12).contains(&date.month) {
        issues.push(temporal_issue(
            TemporalValueKind::LocalDate,
            TemporalIssueKind::MonthOutOfRange,
            "month",
            "1 <= month <= 12",
        ));
    }
    if !(1..=31).contains(&date.day) {
        issues.push(temporal_issue(
            TemporalValueKind::LocalDate,
            TemporalIssueKind::DayOutOfRange,
            "day",
            "1 <= day <= 31",
        ));
    }
    sort_temporal_issues(&mut issues);
    issues
}

/// Validate a local time without leaking the time value.
pub fn diagnose_local_time(time: &LocalTime) -> Vec<TemporalIssue> {
    let mut issues = Vec::new();
    if time.hour > 23 {
        issues.push(temporal_issue(
            TemporalValueKind::LocalTime,
            TemporalIssueKind::HourOutOfRange,
            "hour",
            "0 <= hour <= 23",
        ));
    }
    if time.minute > 59 {
        issues.push(temporal_issue(
            TemporalValueKind::LocalTime,
            TemporalIssueKind::MinuteOutOfRange,
            "minute",
            "0 <= minute <= 59",
        ));
    }
    if time.second > 59 {
        issues.push(temporal_issue(
            TemporalValueKind::LocalTime,
            TemporalIssueKind::SecondOutOfRange,
            "second",
            "0 <= second <= 59",
        ));
    }
    if time.nanos >= 1_000_000_000 {
        issues.push(temporal_issue(
            TemporalValueKind::LocalTime,
            TemporalIssueKind::NanosecondsOutOfRange,
            "nanos",
            "0 <= nanos < 1000000000",
        ));
    }
    sort_temporal_issues(&mut issues);
    issues
}

/// Validate a timezone offset without leaking local environment data.
pub fn diagnose_timezone(timezone: &TimeZone) -> Vec<TemporalIssue> {
    let mut issues = Vec::new();
    if !(-24 * 60..=24 * 60).contains(&timezone.offset_minutes) {
        issues.push(temporal_issue(
            TemporalValueKind::TimeZone,
            TemporalIssueKind::OffsetOutOfRange,
            "offset_minutes",
            "-1440 <= offset_minutes <= 1440",
        ));
    }
    issues
}

/// Validate duration normalization without leaking exact duration values.
pub fn diagnose_duration(duration: &StdDuration) -> Vec<TemporalIssue> {
    let mut issues = Vec::new();
    if duration.nanos <= -1_000_000_000 || duration.nanos >= 1_000_000_000 {
        issues.push(temporal_issue(
            TemporalValueKind::Duration,
            TemporalIssueKind::DurationNotNormalized,
            "nanos",
            "-1000000000 < nanos < 1000000000",
        ));
    }
    issues
}

/// Validate a full zoned date-time contract with deterministic issue ordering.
pub fn diagnose_zoned_datetime(datetime: &ZonedDateTime) -> Vec<TemporalIssue> {
    let mut issues = Vec::new();
    issues.extend(diagnose_local_date(&datetime.datetime.date));
    issues.extend(diagnose_local_time(&datetime.datetime.time));
    issues.extend(diagnose_timezone(&datetime.timezone));
    sort_temporal_issues(&mut issues);
    issues
}

// ── Instant ───────────────────────────────────────────────────────────────

/// A moment in time represented as seconds + nanoseconds since UNIX epoch.
///
/// Not pure: obtaining the current instant requires the `clock.now` capability.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Instant {
    pub secs: i64,
    pub nanos: u32,
}

impl Instant {
    /// Construct an `Instant` from seconds and nanoseconds since UNIX epoch.
    pub fn from_unix(secs: i64, nanos: u32) -> Self {
        Self { secs, nanos }
    }

    /// Return the current wall-clock time.
    ///
    /// Requires `clock.now` capability in production. In the AIL model this
    /// must be dependency-injected; this helper is provided for host code.
    pub fn now() -> Self {
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO);
        Self {
            secs: t.as_secs() as i64,
            nanos: t.subsec_nanos(),
        }
    }

    /// Compute the duration between two instants (`self - earlier`).
    pub fn duration_since(&self, earlier: &Instant) -> StdDuration {
        let secs = self.secs - earlier.secs;
        let nanos = self.nanos as i64 - earlier.nanos as i64;
        StdDuration::from_secs_nanos(secs, nanos)
    }
}

// ── StdDuration ───────────────────────────────────────────────────────────

/// A signed duration in seconds and nanoseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct StdDuration {
    pub secs: i64,
    pub nanos: i64,
}

impl StdDuration {
    pub fn from_secs(s: i64) -> Self {
        Self { secs: s, nanos: 0 }
    }
    pub fn from_millis(ms: i64) -> Self {
        Self {
            secs: ms / 1000,
            nanos: (ms % 1000) * 1_000_000,
        }
    }
    pub fn from_secs_nanos(secs: i64, nanos: i64) -> Self {
        Self { secs, nanos }
    }
    pub fn as_secs_f64(&self) -> f64 {
        self.secs as f64 + self.nanos as f64 / 1_000_000_000.0
    }
    pub fn zero() -> Self {
        Self { secs: 0, nanos: 0 }
    }
    pub fn is_negative(&self) -> bool {
        self.secs < 0 || (self.secs == 0 && self.nanos < 0)
    }
}

// ── TimeZone ──────────────────────────────────────────────────────────────

/// A timezone offset from UTC (no implicit global timezone).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeZone {
    /// Offset from UTC in minutes.
    pub offset_minutes: i32,
}

impl TimeZone {
    pub fn utc() -> Self {
        Self { offset_minutes: 0 }
    }
    pub fn from_offset(hours: i32, minutes: i32) -> Self {
        Self {
            offset_minutes: hours * 60 + minutes,
        }
    }
}

// ── LocalDate ─────────────────────────────────────────────────────────────

/// A calendar date (no time-of-day, no timezone).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LocalDate {
    pub year: i32,
    pub month: u8, // 1–12
    pub day: u8,   // 1–31
}

impl LocalDate {
    pub fn new(year: i32, month: u8, day: u8) -> Self {
        Self { year, month, day }
    }

    /// Format as `YYYY-MM-DD`.
    pub fn format(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

// ── LocalTime ─────────────────────────────────────────────────────────────

/// A time of day (no date, no timezone).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LocalTime {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub nanos: u32,
}

impl LocalTime {
    pub fn new(hour: u8, minute: u8, second: u8) -> Self {
        Self {
            hour,
            minute,
            second,
            nanos: 0,
        }
    }

    /// Format as `HH:MM:SS`.
    pub fn format(&self) -> String {
        format!("{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
    }
}

// ── LocalDateTime ─────────────────────────────────────────────────────────

/// A date + time (no timezone).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalDateTime {
    pub date: LocalDate,
    pub time: LocalTime,
}

impl LocalDateTime {
    pub fn new(date: LocalDate, time: LocalTime) -> Self {
        Self { date, time }
    }

    /// Format as `YYYY-MM-DDTHH:MM:SS`.
    pub fn format(&self) -> String {
        format!("{}T{}", self.date.format(), self.time.format())
    }
}

// ── ZonedDateTime ─────────────────────────────────────────────────────────

/// A date + time + timezone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZonedDateTime {
    pub datetime: LocalDateTime,
    pub timezone: TimeZone,
}

impl ZonedDateTime {
    pub fn new(datetime: LocalDateTime, timezone: TimeZone) -> Self {
        Self { datetime, timezone }
    }

    /// Format as `YYYY-MM-DDTHH:MM:SS+HH:MM`.
    pub fn format(&self) -> String {
        let off = self.timezone.offset_minutes;
        let sign = if off >= 0 { '+' } else { '-' };
        let abs = off.unsigned_abs();
        format!(
            "{}{}{:02}:{:02}",
            self.datetime.format(),
            sign,
            abs / 60,
            abs % 60
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_capability_labels_are_stable() {
        assert_eq!(ClockCapability::Now.label(), "clock.now");
        assert_eq!(ClockCapability::Monotonic.label(), "clock.monotonic");
    }

    #[test]
    fn clock_read_descriptors_make_denied_by_default_semantics_visible() {
        let wall = ClockReadDescriptor::wall_clock();
        let monotonic = ClockReadDescriptor::monotonic();

        assert_eq!(wall.source, ClockSourceKind::WallClock);
        assert_eq!(wall.capability, ClockCapability::Now);
        assert_eq!(wall.capability_label, "clock.now");
        assert!(!wall.monotonic);
        assert!(wall.grant_required);
        assert!(!wall.ambient_access);
        assert!(wall.timezone_policy_explicit);
        assert_eq!(
            wall.diagnostic_key(),
            "std.time.clock_read:wall-clock:clock.now"
        );

        assert_eq!(monotonic.source, ClockSourceKind::Monotonic);
        assert_eq!(monotonic.capability, ClockCapability::Monotonic);
        assert_eq!(monotonic.capability_label, "clock.monotonic");
        assert!(monotonic.monotonic);
        assert!(monotonic.grant_required);
        assert!(!monotonic.ambient_access);
        assert!(!monotonic.timezone_policy_explicit);
        assert_eq!(
            monotonic.diagnostic_key(),
            "std.time.clock_read:monotonic:clock.monotonic"
        );
    }

    #[test]
    fn instant_shape_validation_rejects_noncanonical_nanos_without_leaking_value() {
        let canonical = Instant::from_unix(-1, 999_999_999);
        let invalid = Instant::from_unix(1_717_171_717, 1_000_000_000);

        assert_eq!(instant_shape(&canonical), InstantShape::Canonical);
        assert_eq!(validate_instant_contract(&canonical), Ok(()));

        assert_eq!(instant_shape(&invalid), InstantShape::NanosecondsOutOfRange);
        assert_eq!(
            validate_instant_contract(&invalid),
            Err(InstantShapeError {
                shape: InstantShape::NanosecondsOutOfRange,
                expected: "seconds plus nanoseconds where 0 <= nanos < 1000000000",
            })
        );
        assert_eq!(
            InstantShape::NanosecondsOutOfRange.label(),
            "nanos-out-of-range"
        );
    }

    #[test]
    fn temporal_diagnostics_are_stable_and_redacted() {
        let zoned = ZonedDateTime::new(
            LocalDateTime::new(
                LocalDate::new(2026, 13, 0),
                LocalTime {
                    hour: 24,
                    minute: 60,
                    second: 60,
                    nanos: 1_000_000_000,
                },
            ),
            TimeZone {
                offset_minutes: 2_000,
            },
        );

        let issues = diagnose_zoned_datetime(&zoned);
        let codes: Vec<_> = issues.iter().map(TemporalIssue::code).collect();
        let labels: Vec<_> = issues.iter().map(TemporalIssue::value_label).collect();

        assert_eq!(
            codes,
            vec![
                "std.time.date.day_out_of_range",
                "std.time.date.month_out_of_range",
                "std.time.time.hour_out_of_range",
                "std.time.time.minute_out_of_range",
                "std.time.nanos.out_of_range",
                "std.time.time.second_out_of_range",
                "std.time.timezone.offset_out_of_range",
            ]
        );
        assert_eq!(
            labels,
            vec![
                "local-date",
                "local-date",
                "local-time",
                "local-time",
                "local-time",
                "local-time",
                "timezone",
            ]
        );
        assert!(issues.iter().all(|issue| issue.category() == "range"));
    }

    #[test]
    fn duration_diagnostic_uses_shape_not_duration_value() {
        let issues = diagnose_duration(&StdDuration::from_secs_nanos(1, 1_000_000_000));

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code(), "std.time.duration.not_normalized");
        assert_eq!(issues[0].category(), "normalization");
        assert_eq!(issues[0].field, "nanos");
        assert_eq!(issues[0].value_label(), "duration");
    }
}
