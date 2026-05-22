// ── transaction_tests.rs ─────────────────────────────────────────────────
//
// TDD tests for ail-runtime transaction/rollback groups (G29).
// Written BEFORE implementation — RED phase.

use ail_runtime::profile::CapabilityId;
use ail_runtime::transaction::{TransactionGroup, TransactionPolicy, TransactionStatus};

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
