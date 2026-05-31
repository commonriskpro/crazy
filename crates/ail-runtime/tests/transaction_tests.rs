// ── transaction_tests.rs ─────────────────────────────────────────────────
//
// TDD tests for ail-runtime transaction/rollback groups (G29).
// Written BEFORE implementation — RED phase.

use ail_runtime::profile::CapabilityId;
use ail_runtime::transaction::{
    CompensationPolicy, TransactionGroup, TransactionPolicy, TransactionStatus,
};
use ail_runtime::{
    AuditEvent, AuditLog,
    audit::{
        TRANSACTION_CATEGORY_COMMIT_AFTER_ROLLBACK, TRANSACTION_CATEGORY_COMMITTED,
        TRANSACTION_CATEGORY_ROLLBACK_ALREADY_ROLLED_BACK, TRANSACTION_CATEGORY_ROLLED_BACK,
    },
};

// ── TransactionPolicy ─────────────────────────────────────────────────────

#[test]
fn policy_transactional_is_rollbackable() {
    assert!(TransactionPolicy::Transactional.is_rollbackable());
}

#[test]
fn policy_non_rollbackable_is_not_rollbackable() {
    assert!(!TransactionPolicy::NonRollbackable.is_rollbackable());
}

// ── TransactionGroup: construction ───────────────────────────────────────

#[test]
fn new_group_has_name_and_no_entries() {
    let g = TransactionGroup::new("checkout_tx");
    assert_eq!(g.name(), "checkout_tx");
    assert_eq!(g.entries().len(), 0);
    assert_eq!(g.status(), TransactionStatus::Pending);
}

#[test]
fn add_entries_to_group() {
    let mut g = TransactionGroup::new("checkout_tx");
    g.add(
        CapabilityId::new("database.write:Order"),
        TransactionPolicy::Transactional,
    );
    g.add(
        CapabilityId::new("event.emit:OrderPaid"),
        TransactionPolicy::Transactional,
    );
    g.add(
        CapabilityId::new("payment.charge:PaymentProvider"),
        TransactionPolicy::NonRollbackable,
    );

    assert_eq!(g.entries().len(), 3);
    assert_eq!(g.entries()[2].policy(), &TransactionPolicy::NonRollbackable);
}

// ── TransactionGroup: commit ──────────────────────────────────────────────

#[test]
fn commit_marks_group_committed() {
    let mut g = TransactionGroup::new("checkout_tx");
    g.add(
        CapabilityId::new("database.write:Order"),
        TransactionPolicy::Transactional,
    );
    g.commit();
    assert_eq!(g.status(), TransactionStatus::Committed);
}

#[test]
fn commit_idempotent_on_empty_group() {
    let mut g = TransactionGroup::new("empty_tx");
    g.commit();
    assert_eq!(g.status(), TransactionStatus::Committed);
}

// ── TransactionGroup: rollback ────────────────────────────────────────────

#[test]
fn rollback_marks_group_rolled_back() {
    let mut g = TransactionGroup::new("checkout_tx");
    g.add(
        CapabilityId::new("database.write:Order"),
        TransactionPolicy::Transactional,
    );
    g.rollback();
    assert_eq!(g.status(), TransactionStatus::RolledBack);
}

#[test]
fn rollback_returns_non_rollbackable_capabilities() {
    let mut g = TransactionGroup::new("checkout_tx");
    g.add(
        CapabilityId::new("database.write:Order"),
        TransactionPolicy::Transactional,
    );
    g.add(
        CapabilityId::new("payment.charge:PaymentProvider"),
        TransactionPolicy::NonRollbackable,
    );
    g.add(
        CapabilityId::new("event.emit:OrderPaid"),
        TransactionPolicy::Transactional,
    );

    let non_rollbackable = g.rollback();
    assert_eq!(non_rollbackable.len(), 1);
    assert_eq!(
        non_rollbackable[0].as_str(),
        "payment.charge:PaymentProvider"
    );
}

#[test]
fn rollback_with_all_transactional_returns_empty() {
    let mut g = TransactionGroup::new("safe_tx");
    g.add(
        CapabilityId::new("database.write:Order"),
        TransactionPolicy::Transactional,
    );
    g.add(
        CapabilityId::new("event.emit:OrderPaid"),
        TransactionPolicy::Transactional,
    );
    let non_rollbackable = g.rollback();
    assert!(non_rollbackable.is_empty());
}

// ── TransactionGroup: status cannot go backwards ──────────────────────────

#[test]
fn committed_group_cannot_be_rolled_back() {
    let mut g = TransactionGroup::new("tx");
    g.commit();
    // rollback after commit should be a no-op (status stays Committed)
    let _ = g.rollback();
    assert_eq!(g.status(), TransactionStatus::Committed);
}

// ── Transaction audit records ─────────────────────────────────────────────

#[test]
fn commit_with_audit_returns_stable_redacted_record() {
    let mut g = TransactionGroup::new("tenant-42/checkout_tx");
    g.add(
        CapabilityId::new("database.write:Order"),
        TransactionPolicy::Transactional,
    );
    g.add_with_compensation(
        CapabilityId::new("payment.charge:PaymentProvider"),
        TransactionPolicy::NonRollbackable,
        CompensationPolicy::ExplicitRefund {
            refund_capability: CapabilityId::new("payment.refund:PaymentProvider"),
        },
    );

    let audit = g.commit_with_audit();

    assert_eq!(g.status(), TransactionStatus::Committed);
    assert_eq!(audit.action, "commit");
    assert_eq!(audit.category, TRANSACTION_CATEGORY_COMMITTED);
    assert_eq!(audit.status_before, "pending");
    assert_eq!(audit.status_after, "committed");
    assert_eq!(audit.group_name_shape, "contains_separator");
    assert_eq!(audit.entry_count, 2);
    assert_eq!(audit.non_rollbackable_count, 1);
    assert_eq!(audit.compensation_required_count, 1);

    let debug = format!("{audit:?}");
    assert!(
        !debug.contains("tenant-42") && !debug.contains("payment.refund"),
        "transaction audit record must not leak raw labels or compensation capabilities"
    );
}

#[test]
fn rollback_with_audit_classifies_compensation_and_idempotent_repeats() {
    let mut g = TransactionGroup::new("checkout_tx");
    g.add(
        CapabilityId::new("database.write:Order"),
        TransactionPolicy::Transactional,
    );
    g.add_with_compensation(
        CapabilityId::new("payment.charge:PaymentProvider"),
        TransactionPolicy::NonRollbackable,
        CompensationPolicy::IdempotentRetry {
            idempotency_key: "order-42-secret-key".to_string(),
        },
    );

    let (non_rollbackable, first_audit) = g.rollback_with_audit();
    let (second_non_rollbackable, second_audit) = g.rollback_with_audit();

    assert_eq!(non_rollbackable.len(), 1);
    assert_eq!(second_non_rollbackable.len(), 0);
    assert_eq!(first_audit.category, TRANSACTION_CATEGORY_ROLLED_BACK);
    assert_eq!(
        second_audit.category,
        TRANSACTION_CATEGORY_ROLLBACK_ALREADY_ROLLED_BACK
    );
    assert_eq!(first_audit.status_before, "pending");
    assert_eq!(first_audit.status_after, "rolled_back");
    assert_eq!(second_audit.status_before, "rolled_back");
    assert_eq!(second_audit.status_after, "rolled_back");
    assert_eq!(first_audit.group_name_shape, "safe_label");
    assert_eq!(first_audit.compensation_required_count, 1);

    let debug = format!("{first_audit:?} {second_audit:?}");
    assert!(
        !debug.contains("order-42-secret-key") && !debug.contains("payment.charge"),
        "transaction audit record must not leak idempotency keys or capability names"
    );
}

#[test]
fn commit_after_rollback_gets_stable_failure_category() {
    let mut g = TransactionGroup::new("tx");
    let _ = g.rollback_with_audit();

    let audit = g.commit_with_audit();

    assert_eq!(g.status(), TransactionStatus::RolledBack);
    assert_eq!(audit.action, "commit");
    assert_eq!(audit.status_before, "rolled_back");
    assert_eq!(audit.status_after, "rolled_back");
    assert_eq!(audit.category, TRANSACTION_CATEGORY_COMMIT_AFTER_ROLLBACK);
}

#[test]
fn transaction_lifecycle_event_shape_can_be_pushed_without_raw_payloads() {
    let mut log = AuditLog::new();
    let mut g = TransactionGroup::new("customer@example.com/checkout");
    g.add_with_compensation(
        CapabilityId::new("payment.charge:PaymentProvider"),
        TransactionPolicy::NonRollbackable,
        CompensationPolicy::ExplicitRefund {
            refund_capability: CapabilityId::new("payment.refund:PaymentProvider"),
        },
    );

    let (_, audit) = g.rollback_with_audit();
    log.push(AuditEvent::TransactionLifecycle {
        group_name_shape: audit.group_name_shape,
        action: audit.action.to_string(),
        category: audit.category.to_string(),
        status_before: audit.status_before.to_string(),
        status_after: audit.status_after.to_string(),
        entry_count: audit.entry_count,
        non_rollbackable_count: audit.non_rollbackable_count,
        compensation_required_count: audit.compensation_required_count,
    });

    assert_eq!(log.len(), 1);
    match &log.events()[0] {
        AuditEvent::TransactionLifecycle {
            group_name_shape,
            category,
            status_before,
            status_after,
            entry_count,
            non_rollbackable_count,
            compensation_required_count,
            ..
        } => {
            assert_eq!(group_name_shape, "contains_separator");
            assert_eq!(category, TRANSACTION_CATEGORY_ROLLED_BACK);
            assert_eq!(status_before, "pending");
            assert_eq!(status_after, "rolled_back");
            assert_eq!(*entry_count, 1);
            assert_eq!(*non_rollbackable_count, 1);
            assert_eq!(*compensation_required_count, 1);
        }
        other => panic!("expected transaction lifecycle audit event, got {other:?}"),
    }

    let debug = format!("{:?}", log.events());
    assert!(
        !debug.contains("customer@example.com")
            && !debug.contains("payment.refund")
            && !debug.contains("PaymentProvider"),
        "transaction lifecycle audit event must stay redacted"
    );
}
