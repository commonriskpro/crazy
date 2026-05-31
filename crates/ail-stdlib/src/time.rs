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
}
