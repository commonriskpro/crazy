// ── rollback_integration_tests.rs ────────────────────────────────────────
//
// TDD tests for:
//   1. RuntimeHost rollback-on-failure integration (CRITICAL)
//   2. Explicit compensation/idempotency model (WARNING)
//
// Per runtime.md §"Transactions and rollback":
//   Runtime supports transactional capability groups when handlers provide them.
//   Rules:
//   - transactional effects must declare commit/rollback semantics
//   - non-transactional external effects must be marked as such
//   - compensation actions must be explicit when needed
//
//   Example non-transactional:
//     payment.charge is non_rollbackable
//     requires idempotency + compensation/refund policy

use ail_runtime::profile::CapabilityId;
use ail_runtime::transaction::{
    CompensationPolicy, TransactionGroup, TransactionPolicy,
};

// ── CompensationPolicy ────────────────────────────────────────────────────

#[test]
fn compensation_policy_explicit_refund() {
    let policy = CompensationPolicy::ExplicitRefund {
        refund_capability: CapabilityId::new("payment.refund:PaymentProvider"),
    };
    match policy {
        CompensationPolicy::ExplicitRefund { refund_capability } => {
            assert_eq!(refund_capability.as_str(), "payment.refund:PaymentProvider");
        }
        _ => panic!("expected ExplicitRefund"),
    }
}

#[test]
fn compensation_policy_idempotent_retry() {
    let policy = CompensationPolicy::IdempotentRetry {
        idempotency_key: "order-42".to_string(),
    };
    match policy {
        CompensationPolicy::IdempotentRetry { idempotency_key } => {
            assert_eq!(idempotency_key, "order-42");
        }
        _ => panic!("expected IdempotentRetry"),
    }
}

#[test]
fn compensation_policy_none() {
    let policy = CompensationPolicy::None;
    assert!(matches!(policy, CompensationPolicy::None));
}

// ── TransactionGroup with CompensationPolicy ──────────────────────────────

#[test]
fn transaction_group_add_with_compensation() {
    let mut g = TransactionGroup::new("checkout_tx");

    // Transactional DB write — no compensation needed
    g.add_with_compensation(
        CapabilityId::new("database.write:Order"),
        TransactionPolicy::Transactional,
        CompensationPolicy::None,
    );

    // Non-rollbackable payment — needs explicit refund
    g.add_with_compensation(
        CapabilityId::new("payment.charge:PaymentProvider"),
        TransactionPolicy::NonRollbackable,
        CompensationPolicy::ExplicitRefund {
            refund_capability: CapabilityId::new("payment.refund:PaymentProvider"),
        },
    );

    assert_eq!(g.entries().len(), 2);
    let payment_entry = &g.entries()[1];
    assert_eq!(payment_entry.policy(), &TransactionPolicy::NonRollbackable);
    match payment_entry.compensation() {
        CompensationPolicy::ExplicitRefund { refund_capability } => {
            assert_eq!(refund_capability.as_str(), "payment.refund:PaymentProvider");
        }
        _ => panic!("expected ExplicitRefund"),
    }
}

#[test]
fn rollback_returns_non_rollbackable_with_compensation_info() {
    let mut g = TransactionGroup::new("checkout_tx");

    g.add_with_compensation(
        CapabilityId::new("database.write:Order"),
        TransactionPolicy::Transactional,
        CompensationPolicy::None,
    );
    g.add_with_compensation(
        CapabilityId::new("payment.charge:PaymentProvider"),
        TransactionPolicy::NonRollbackable,
        CompensationPolicy::ExplicitRefund {
            refund_capability: CapabilityId::new("payment.refund:PaymentProvider"),
        },
    );

    let rollback_result = g.rollback_with_compensation();
    assert_eq!(rollback_result.len(), 1);
    assert_eq!(
        rollback_result[0].capability.as_str(),
        "payment.charge:PaymentProvider"
    );
    match &rollback_result[0].compensation {
        CompensationPolicy::ExplicitRefund { refund_capability } => {
            assert_eq!(refund_capability.as_str(), "payment.refund:PaymentProvider");
        }
        _ => panic!("expected ExplicitRefund for payment entry"),
    }
}

// ── RuntimeHost rollback-on-failure ──────────────────────────────────────

#[test]
fn runtime_host_execute_with_rollback_commits_on_success() {
    use std::sync::Arc;
    use ail_runtime::host::RuntimeHost;
    use ail_runtime::manifest::{CapabilityManifest, blake3_hex_of};
    use ail_runtime::profile::{CapabilityGrant, ResourceLimits, RuntimeProfile};
    use ail_runtime::InMemoryHandler;
    use ail_runtime::transaction::TransactionStatus;

    let wasm = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
    let db_cap = CapabilityId::new("database.write:Order");
    let event_cap = CapabilityId::new("event.emit:OrderPaid");

    let manifest = CapabilityManifest {
        module: "test".to_string(),
        requires: vec![db_cap.clone(), event_cap.clone()],
    };
    let module_hash = blake3_hex_of(&wasm);
    let manifest_hash = manifest.blake3_hex().unwrap();
    let profile = RuntimeProfile::new(
        "test".to_string(),
        module_hash,
        "vr-hash".to_string(),
        manifest_hash,
        vec![
            CapabilityGrant { module: "test".to_string(), capability: db_cap.clone() },
            CapabilityGrant { module: "test".to_string(), capability: event_cap.clone() },
        ],
        ResourceLimits { max_memory_bytes: None, max_fuel: None },
    );

    let db_handler = Arc::new(InMemoryHandler::new(
        "db-handler",
        vec![db_cap.clone()],
        b"ok".to_vec(),
    ));
    let event_handler = Arc::new(InMemoryHandler::new(
        "event-handler",
        vec![event_cap.clone()],
        b"ok".to_vec(),
    ));

    let mut host = RuntimeHost::new()
        .with_handler(db_handler)
        .with_handler(event_handler);

    host.validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    let mut tx = TransactionGroup::new("checkout_tx");
    tx.add(db_cap.clone(), TransactionPolicy::Transactional);
    tx.add(event_cap.clone(), TransactionPolicy::Transactional);

    // execute_with_rollback: runs the closure; if Ok, commits tx; if Err, rolls back
    let result = host.execute_with_rollback(&mut tx, |h| {
        h.call_capability(&db_cap, "write", b"Order:1")?;
        h.call_capability(&event_cap, "emit", b"OrderPaid")?;
        Ok(())
    });

    assert!(result.is_ok(), "successful execution must not roll back");
    assert_eq!(tx.status(), TransactionStatus::Committed);
}

#[test]
fn runtime_host_execute_with_rollback_rolls_back_on_failure() {
    use std::sync::Arc;
    use ail_runtime::abi::HostError;
    use ail_runtime::host::RuntimeHost;
    use ail_runtime::manifest::{CapabilityManifest, blake3_hex_of};
    use ail_runtime::profile::{CapabilityGrant, ResourceLimits, RuntimeProfile};
    use ail_runtime::InMemoryHandler;
    use ail_runtime::transaction::TransactionStatus;

    let wasm = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
    let db_cap = CapabilityId::new("database.write:Order");
    let manifest = CapabilityManifest {
        module: "test".to_string(),
        requires: vec![db_cap.clone()],
    };
    let module_hash = blake3_hex_of(&wasm);
    let manifest_hash = manifest.blake3_hex().unwrap();
    let profile = RuntimeProfile::new(
        "test".to_string(),
        module_hash,
        "vr-hash".to_string(),
        manifest_hash,
        vec![CapabilityGrant { module: "test".to_string(), capability: db_cap.clone() }],
        ResourceLimits { max_memory_bytes: None, max_fuel: None },
    );

    let db_handler = Arc::new(InMemoryHandler::new(
        "db-handler",
        vec![db_cap.clone()],
        b"ok".to_vec(),
    ));

    let mut host = RuntimeHost::new().with_handler(db_handler);
    host.validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    let mut tx = TransactionGroup::new("failing_tx");
    tx.add(db_cap.clone(), TransactionPolicy::Transactional);

    let result: Result<(), _> = host.execute_with_rollback(&mut tx, |_h| {
        // Simulate execution failure
        Err(HostError { message: "simulated handler failure".to_string() })
    });

    assert!(result.is_err(), "execution failure must propagate error");
    assert_eq!(tx.status(), TransactionStatus::RolledBack,
        "execution failure must trigger rollback");
}

#[test]
fn runtime_host_execute_with_rollback_returns_non_rollbackable_on_failure() {
    use std::sync::Arc;
    use ail_runtime::abi::HostError;
    use ail_runtime::host::RuntimeHost;
    use ail_runtime::manifest::{CapabilityManifest, blake3_hex_of};
    use ail_runtime::profile::{CapabilityGrant, ResourceLimits, RuntimeProfile};
    use ail_runtime::InMemoryHandler;
    use ail_runtime::transaction::TransactionStatus;

    let wasm = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
    let db_cap = CapabilityId::new("database.write:Order");
    let pay_cap = CapabilityId::new("payment.charge:PaymentProvider");

    let manifest = CapabilityManifest {
        module: "test".to_string(),
        requires: vec![db_cap.clone(), pay_cap.clone()],
    };
    let module_hash = blake3_hex_of(&wasm);
    let manifest_hash = manifest.blake3_hex().unwrap();
    let profile = RuntimeProfile::new(
        "test".to_string(),
        module_hash,
        "vr-hash".to_string(),
        manifest_hash,
        vec![
            CapabilityGrant { module: "test".to_string(), capability: db_cap.clone() },
            CapabilityGrant { module: "test".to_string(), capability: pay_cap.clone() },
        ],
        ResourceLimits { max_memory_bytes: None, max_fuel: None },
    );

    let db_handler = Arc::new(InMemoryHandler::new(
        "db-handler",
        vec![db_cap.clone(), pay_cap.clone()],
        b"ok".to_vec(),
    ));

    let mut host = RuntimeHost::new().with_handler(db_handler);
    host.validate_and_instantiate(&wasm, &manifest, &profile)
        .expect("preflight must pass");

    // payment is non_rollbackable — requires compensation
    let mut tx = TransactionGroup::new("checkout_tx");
    tx.add(db_cap.clone(), TransactionPolicy::Transactional);
    tx.add(pay_cap.clone(), TransactionPolicy::NonRollbackable);

    let (result, non_rollbackable): (Result<(), _>, _) = host.execute_with_rollback_detail(&mut tx, |_h| {
        Err(HostError { message: "simulated failure after payment".to_string() })
    });

    assert!(result.is_err());
    assert_eq!(tx.status(), TransactionStatus::RolledBack);
    assert_eq!(non_rollbackable.len(), 1);
    assert_eq!(non_rollbackable[0].as_str(), "payment.charge:PaymentProvider");
}
