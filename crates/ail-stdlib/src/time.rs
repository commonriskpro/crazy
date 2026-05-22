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
