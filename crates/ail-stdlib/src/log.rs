// ── ail-stdlib::log ───────────────────────────────────────────────────────
//
// Structured logging for the AIL `std.log` module.
//
// # Capabilities (from docs/stdlib.md)
//
// - log.write
//
// # Rules
//
// - logs are effects
// - PII/secrets redacted by policy
// - runtime audit separate from app logs

use std::collections::BTreeMap;

// ── LogLevel ──────────────────────────────────────────────────────────────

/// Log severity level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Trace => write!(f, "TRACE"),
            LogLevel::Debug => write!(f, "DEBUG"),
            LogLevel::Info => write!(f, "INFO"),
            LogLevel::Warn => write!(f, "WARN"),
            LogLevel::Error => write!(f, "ERROR"),
        }
    }
}

// ── LogEvent ──────────────────────────────────────────────────────────────

/// A structured log event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogEvent {
    pub level: LogLevel,
    pub message: String,
    pub fields: BTreeMap<String, String>,
}

impl LogEvent {
    pub fn new(level: LogLevel, message: impl Into<String>) -> Self {
        Self {
            level,
            message: message.into(),
            fields: BTreeMap::new(),
        }
    }

    pub fn with_field(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.fields.insert(key.into(), value.into());
        self
    }

    /// Format the event as a human-readable string.
    pub fn format(&self) -> String {
        let fields: Vec<String> = self
            .fields
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        if fields.is_empty() {
            format!("[{}] {}", self.level, self.message)
        } else {
            format!(
                "[{}] {} {{{}}}",
                self.level,
                self.message,
                fields.join(", ")
            )
        }
    }
}

// ── Logger ────────────────────────────────────────────────────────────────

/// A logger that emits `LogEvent`s.
///
/// In the AIL model, the logger requires the `log.write` capability.
/// PII and secrets must be redacted by policy before logging.
pub struct Logger {
    pub min_level: LogLevel,
    pub sink: Box<dyn LogSink>,
}

/// Trait for log output destinations.
pub trait LogSink: Send + Sync {
    fn emit(&self, event: &LogEvent);
}

/// A no-op sink that discards all log events.
pub struct NoopSink;

impl LogSink for NoopSink {
    fn emit(&self, _: &LogEvent) {}
}

/// A sink that collects events into a `Vec` for testing.
#[derive(Default)]
pub struct CapturingLogSink {
    pub events: std::sync::Mutex<Vec<LogEvent>>,
}

impl LogSink for CapturingLogSink {
    fn emit(&self, event: &LogEvent) {
        self.events.lock().unwrap().push(event.clone());
    }
}

impl Logger {
    pub fn new(min_level: LogLevel, sink: Box<dyn LogSink>) -> Self {
        Self { min_level, sink }
    }

    pub fn noop() -> Self {
        Self::new(LogLevel::Error, Box::new(NoopSink))
    }

    pub fn emit(&self, event: LogEvent) {
        if event.level >= self.min_level {
            self.sink.emit(&event);
        }
    }

    pub fn info(&self, msg: impl Into<String>) {
        self.emit(LogEvent::new(LogLevel::Info, msg));
    }

    pub fn warn(&self, msg: impl Into<String>) {
        self.emit(LogEvent::new(LogLevel::Warn, msg));
    }

    pub fn error(&self, msg: impl Into<String>) {
        self.emit(LogEvent::new(LogLevel::Error, msg));
    }
}
