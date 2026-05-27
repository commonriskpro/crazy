use super::helpers::*;

// ── RetentionPolicy::is_retained ──────────────────────────────────────────

// GIVEN keep_releases = true
// WHEN snapshot has parent_id = None (genesis)
// THEN is_retained returns true
#[test]
fn retention_keeps_genesis_when_keep_releases_true() {
    let policy = RetentionPolicy {
        max_age_days: None,
        keep_releases: true,
        keep_tagged: false,
    };
    let snap = snapshot("genesis", 0, None, None);
    assert!(
        policy.is_retained(&snap, 1_000_000_000_000),
        "genesis must be retained when keep_releases is true"
    );
}

// GIVEN keep_releases = false
// WHEN snapshot has parent_id = None (genesis)
// THEN is_retained returns false (no other rule applies)
#[test]
fn retention_releases_genesis_when_keep_releases_false() {
    let policy = RetentionPolicy {
        max_age_days: None,
        keep_releases: false,
        keep_tagged: false,
    };
    let snap = snapshot("old-genesis", 0, None, None);
    assert!(
        !policy.is_retained(&snap, 1_000_000_000_000),
        "genesis must not be retained when keep_releases is false and no age rule applies"
    );
}

// GIVEN keep_tagged = true
// WHEN snapshot has applied_change_id = Some(_)
// THEN is_retained returns true
#[test]
fn retention_keeps_tagged_when_keep_tagged_true() {
    let policy = RetentionPolicy {
        max_age_days: None,
        keep_releases: false,
        keep_tagged: true,
    };
    let snap = snapshot("tagged", 0, Some("parent"), Some("change-42"));
    assert!(
        policy.is_retained(&snap, 1_000_000_000_000),
        "snapshot with applied_change_id must be retained when keep_tagged is true"
    );
}

// GIVEN keep_tagged = false
// WHEN snapshot has applied_change_id = Some(_)
// THEN is_retained returns false (no other rule applies)
#[test]
fn retention_releases_tagged_when_keep_tagged_false() {
    let policy = RetentionPolicy {
        max_age_days: None,
        keep_releases: false,
        keep_tagged: false,
    };
    let snap = snapshot("tagged-old", 0, Some("parent"), Some("change-42"));
    assert!(
        !policy.is_retained(&snap, 1_000_000_000_000),
        "snapshot must not be retained when keep_tagged is false and no age rule applies"
    );
}

// GIVEN max_age_days = Some(30)  and  now = 30 days since created_at
// WHEN snapshot created_at == now - 25 days (within 30 days)
// THEN is_retained returns true
#[test]
fn retention_keeps_young_snapshot_within_max_age() {
    let now_ms: u64 = 30 * 86_400_000; // 30 days in ms
    let policy = RetentionPolicy {
        max_age_days: Some(30),
        keep_releases: false,
        keep_tagged: false,
    };
    // Created 25 days ago — younger than 30 days
    let created_at = now_ms - 25 * 86_400_000;
    let snap = snapshot("young", created_at, Some("p"), None);
    assert!(
        policy.is_retained(&snap, now_ms),
        "snapshot younger than max_age_days must be retained"
    );
}

// GIVEN max_age_days = Some(30)
// WHEN snapshot created_at is older than 30 days
// THEN is_retained returns false
#[test]
fn retention_removes_old_snapshot_beyond_max_age() {
    let now_ms: u64 = 60 * 86_400_000; // 60 days in ms
    let policy = RetentionPolicy {
        max_age_days: Some(30),
        keep_releases: false,
        keep_tagged: false,
    };
    // Created 45 days ago — older than 30-day window
    let created_at = now_ms - 45 * 86_400_000;
    let snap = snapshot("old", created_at, Some("p"), None);
    assert!(
        !policy.is_retained(&snap, now_ms),
        "snapshot older than max_age_days must not be retained"
    );
}

// GIVEN max_age_days = None
// WHEN snapshot has any created_at
// THEN age rule does not protect it (returns false if no other rule matches)
#[test]
fn retention_none_max_age_does_not_protect_by_age() {
    let policy = RetentionPolicy {
        max_age_days: None,
        keep_releases: false,
        keep_tagged: false,
    };
    let snap = snapshot("recent", 999_999_999_999, Some("p"), None);
    assert!(
        !policy.is_retained(&snap, 1_000_000_000_000),
        "None max_age_days must not protect a snapshot by age alone"
    );
}

// ── gc_unreferenced ───────────────────────────────────────────────────────

// GIVEN an empty store
// WHEN gc_unreferenced is called
// THEN report has all zero counts
