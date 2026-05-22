use ail_stdlib::log::{CapturingLogSink, LogEvent, LogLevel, Logger, NoopSink};
use std::sync::Arc;

#[test]
fn log_level_ordering() {
    assert!(LogLevel::Trace < LogLevel::Debug);
    assert!(LogLevel::Debug < LogLevel::Info);
    assert!(LogLevel::Info < LogLevel::Warn);
    assert!(LogLevel::Warn < LogLevel::Error);
}

#[test]
fn log_level_display() {
    assert_eq!(format!("{}", LogLevel::Info), "INFO");
    assert_eq!(format!("{}", LogLevel::Error), "ERROR");
}

#[test]
fn log_event_format_no_fields() {
    let e = LogEvent::new(LogLevel::Info, "hello");
    assert_eq!(e.format(), "[INFO] hello");
}

#[test]
fn log_event_format_with_fields() {
    let e = LogEvent::new(LogLevel::Warn, "warning")
        .with_field("module", "auth")
        .with_field("user_id", "42");
    let formatted = e.format();
    assert!(formatted.contains("[WARN] warning"));
    assert!(formatted.contains("module=auth"));
    assert!(formatted.contains("user_id=42"));
}

#[test]
fn logger_noop_does_not_emit() {
    let logger = Logger::noop();
    logger.info("should be discarded");
    logger.error("also discarded");
    // No panic = pass
}

#[test]
fn logger_capturing_sink_captures() {
    let sink = Arc::new(CapturingLogSink::default());
    let logger = Logger::new(
        LogLevel::Info,
        Box::new({
            struct ArcSink(Arc<CapturingLogSink>);
            impl ail_stdlib::log::LogSink for ArcSink {
                fn emit(&self, e: &LogEvent) {
                    self.0.emit(e);
                }
            }
            ArcSink(Arc::clone(&sink))
        }),
    );
    logger.info("captured message");
    logger.warn("also captured");
    let events = sink.events.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].level, LogLevel::Info);
    assert_eq!(events[1].level, LogLevel::Warn);
}

#[test]
fn logger_filters_below_min_level() {
    let sink = Arc::new(CapturingLogSink::default());
    let logger = Logger::new(
        LogLevel::Warn,
        Box::new({
            struct ArcSink(Arc<CapturingLogSink>);
            impl ail_stdlib::log::LogSink for ArcSink {
                fn emit(&self, e: &LogEvent) {
                    self.0.emit(e);
                }
            }
            ArcSink(Arc::clone(&sink))
        }),
    );
    logger.info("should be filtered");
    logger.warn("should pass");
    let events = sink.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].level, LogLevel::Warn);
}

#[test]
fn noop_sink_compiles() {
    let _sink: Box<dyn ail_stdlib::log::LogSink> = Box::new(NoopSink);
}
