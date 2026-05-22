use ail_stdlib::runtime::{
    ArtifactEntry, ArtifactManifest, AuditEvent, AuditOutcome, LimitConfig, ReplayConfig,
    RuntimeProfile, RuntimeReport,
};

#[test]
fn runtime_profile_display() {
    assert_eq!(RuntimeProfile::Production.to_string(), "production");
    assert_eq!(
        RuntimeProfile::Custom("edge".into()).to_string(),
        "custom:edge"
    );
}

#[test]
fn strict_limits_are_bounded() {
    let limits = LimitConfig::strict();

    assert_eq!(limits.max_fuel, Some(10_000_000));
    assert_eq!(limits.max_tasks, Some(64));
}

#[test]
fn audit_event_new_defaults_capability_to_none() {
    let event = AuditEvent::new("A1", "grant", "mod.app", 123, AuditOutcome::Allowed);

    assert_eq!(event.id, "A1");
    assert!(event.capability.is_none());
}

#[test]
fn runtime_report_new_exits_ok() {
    let report = RuntimeReport::new();

    assert!(report.exit_ok);
    assert!(report.audit_events.is_empty());
}

#[test]
fn replay_config_defaults_disabled() {
    let replay = ReplayConfig::default();

    assert!(replay.seed.is_none());
    assert!(!replay.capture_io);
    assert!(!replay.replay_io);
}

#[test]
fn artifact_manifest_adds_entries() {
    let mut manifest = ArtifactManifest::new();
    manifest.add(ArtifactEntry {
        id: "bin".into(),
        path: "target/app".into(),
        hash: Some("abc".into()),
        size_bytes: Some(42),
    });

    assert_eq!(manifest.artifacts.len(), 1);
    assert_eq!(manifest.artifacts[0].id, "bin");
}
