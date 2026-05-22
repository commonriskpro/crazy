// ── ail-runtime::transaction ──────────────────────────────────────────────
//
// Transaction/rollback groups for capability calls (G29).
//
// Per runtime.md:
//   transaction group checkout_tx
//     database.write:Order
//     event.emit:OrderPaid
//   end
//
//   Rules:
//   - transactional effects must declare commit/rollback semantics
//   - non-transactional external effects must be marked as such
//   - compensation actions must be explicit when needed
//
//   Example non-transactional:
//     payment.charge is non_rollbackable
//     requires idempotency + compensation/refund policy
//
// `TransactionGroup` tracks a named set of capability calls with their
// rollback and compensation policies.

use crate::profile::CapabilityId;

// ── TransactionPolicy ─────────────────────────────────────────────────────

/// Rollback policy for a capability within a transaction group.
///
/// `Transactional` means the capability supports commit/rollback semantics
/// (e.g. database writes behind a transaction boundary).
///
/// `NonRollbackable` means the capability cannot be rolled back once executed
/// (e.g. a payment charge that requires an explicit refund/compensation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransactionPolicy {
    /// Capability supports transactional commit/rollback.
    Transactional,
    /// Capability cannot be rolled back; requires idempotency + compensation.
    NonRollbackable,
}

impl TransactionPolicy {
    /// `true` if the policy allows rollback without compensation.
    pub fn is_rollbackable(&self) -> bool {
        matches!(self, TransactionPolicy::Transactional)
    }
}

// ── CompensationPolicy ────────────────────────────────────────────────────

/// Explicit compensation/idempotency policy for non-rollbackable capabilities.
///
/// Per runtime.md §"Transactions and rollback":
/// > compensation actions must be explicit when needed
///
/// When a `NonRollbackable` capability has been invoked and the transaction
/// needs to be undone, the caller must execute the declared compensation
/// action (e.g. issue a refund, replay the idempotent call, etc.).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompensationPolicy {
    /// No compensation required (used for `Transactional` capabilities).
    None,

    /// An explicit refund/undo must be issued via `refund_capability`.
    ///
    /// Example: `payment.charge` → `payment.refund`
    ExplicitRefund {
        /// The capability to invoke to undo the original call.
        refund_capability: CapabilityId,
    },

    /// The original call is idempotent and can be retried with the same key.
    ///
    /// Example: an event-emission that deduplicates on `idempotency_key`.
    IdempotentRetry {
        /// Key that ensures retried calls produce the same outcome.
        idempotency_key: String,
    },
}

// ── CompensationRequired ─────────────────────────────────────────────────

/// The compensation information for one non-rollbackable capability entry.
///
/// Returned by [`TransactionGroup::rollback_with_compensation`] so callers
/// know which capabilities require explicit compensation actions.
#[derive(Clone, Debug)]
pub struct CompensationRequired {
    /// The capability that cannot be rolled back automatically.
    pub capability: CapabilityId,
    /// The declared compensation/idempotency policy for this capability.
    pub compensation: CompensationPolicy,
}

// ── TransactionStatus ─────────────────────────────────────────────────────

/// Lifecycle status of a [`TransactionGroup`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransactionStatus {
    /// No commit or rollback has been called yet.
    Pending,
    /// `commit()` was called successfully.
    Committed,
    /// `rollback()` was called.
    RolledBack,
}

// ── TransactionEntry ──────────────────────────────────────────────────────

/// One capability in a [`TransactionGroup`], with its rollback and compensation
/// policies.
#[derive(Clone, Debug)]
pub struct TransactionEntry {
    /// The capability being tracked.
    capability: CapabilityId,
    /// Whether this capability supports rollback.
    policy: TransactionPolicy,
    /// Compensation/idempotency policy (for `NonRollbackable` capabilities).
    compensation: CompensationPolicy,
}

impl TransactionEntry {
    /// The capability tracked by this entry.
    pub fn capability(&self) -> &CapabilityId {
        &self.capability
    }

    /// The rollback policy for this capability.
    pub fn policy(&self) -> &TransactionPolicy {
        &self.policy
    }

    /// The compensation/idempotency policy for this entry.
    ///
    /// For `Transactional` capabilities this is always [`CompensationPolicy::None`].
    pub fn compensation(&self) -> &CompensationPolicy {
        &self.compensation
    }
}

// ── TransactionGroup ─────────────────────────────────────────────────────

/// A named group of capability calls with transactional semantics.
///
/// Groups track which capabilities were called and their rollback/compensation
/// policies.  `commit()` and `rollback()` are idempotent state transitions.
///
/// On `rollback()`:
/// - The group transitions to [`TransactionStatus::RolledBack`].
/// - Returns the list of [`CapabilityId`]s that are `NonRollbackable` — the
///   caller must apply explicit compensation (e.g. issue a refund).
///
/// Use [`add_with_compensation`](TransactionGroup::add_with_compensation) to
/// attach explicit [`CompensationPolicy`] to non-rollbackable entries.
/// Use [`rollback_with_compensation`](TransactionGroup::rollback_with_compensation)
/// to roll back and receive the full [`CompensationRequired`] details.
///
/// # Example
///
/// ```rust
/// use ail_runtime::profile::CapabilityId;
/// use ail_runtime::transaction::{CompensationPolicy, TransactionGroup, TransactionPolicy};
///
/// let mut g = TransactionGroup::new("checkout_tx");
/// g.add(CapabilityId::new("database.write:Order"), TransactionPolicy::Transactional);
/// g.add(CapabilityId::new("payment.charge:PaymentProvider"), TransactionPolicy::NonRollbackable);
///
/// let non_rollbackable = g.rollback();
/// assert_eq!(non_rollbackable.len(), 1); // payment.charge requires compensation
/// ```
pub struct TransactionGroup {
    name: String,
    entries: Vec<TransactionEntry>,
    status: TransactionStatus,
}

impl TransactionGroup {
    /// Create a new transaction group with `name` and no entries.
    pub fn new(name: impl Into<String>) -> Self {
        TransactionGroup {
            name: name.into(),
            entries: Vec::new(),
            status: TransactionStatus::Pending,
        }
    }

    /// Group name (human-readable label).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Ordered entries in this group.
    pub fn entries(&self) -> &[TransactionEntry] {
        &self.entries
    }

    /// Current lifecycle status.
    pub fn status(&self) -> TransactionStatus {
        self.status.clone()
    }

    /// Add a capability with the given rollback policy to this group.
    ///
    /// Compensation defaults to [`CompensationPolicy::None`].
    pub fn add(&mut self, capability: CapabilityId, policy: TransactionPolicy) {
        self.entries.push(TransactionEntry {
            capability,
            policy,
            compensation: CompensationPolicy::None,
        });
    }

    /// Add a capability with explicit rollback **and** compensation policies.
    ///
    /// Use this for `NonRollbackable` capabilities that require an explicit
    /// compensation action (refund, idempotent retry, etc.) on rollback.
    pub fn add_with_compensation(
        &mut self,
        capability: CapabilityId,
        policy: TransactionPolicy,
        compensation: CompensationPolicy,
    ) {
        self.entries.push(TransactionEntry {
            capability,
            policy,
            compensation,
        });
    }

    /// Commit the transaction group.
    ///
    /// Transitions status from `Pending` → `Committed`.
    /// No-op if already committed or rolled back.
    pub fn commit(&mut self) {
        if self.status == TransactionStatus::Pending {
            self.status = TransactionStatus::Committed;
        }
    }

    /// Roll back the transaction group.
    ///
    /// Transitions status from `Pending` → `RolledBack`.
    /// Returns the list of [`CapabilityId`]s that are `NonRollbackable`.
    ///
    /// No-op (returns empty vec) if already committed or rolled back.
    pub fn rollback(&mut self) -> Vec<CapabilityId> {
        if self.status != TransactionStatus::Pending {
            return vec![];
        }
        self.status = TransactionStatus::RolledBack;
        self.entries
            .iter()
            .filter(|e| e.policy == TransactionPolicy::NonRollbackable)
            .map(|e| e.capability.clone())
            .collect()
    }

    /// Roll back the transaction group, returning full compensation details.
    ///
    /// Transitions status from `Pending` → `RolledBack`.
    /// Returns [`CompensationRequired`] entries for each `NonRollbackable`
    /// capability in the group so callers can apply explicit compensation.
    ///
    /// No-op (returns empty vec) if already committed or rolled back.
    pub fn rollback_with_compensation(&mut self) -> Vec<CompensationRequired> {
        if self.status != TransactionStatus::Pending {
            return vec![];
        }
        self.status = TransactionStatus::RolledBack;
        self.entries
            .iter()
            .filter(|e| e.policy == TransactionPolicy::NonRollbackable)
            .map(|e| CompensationRequired {
                capability: e.capability.clone(),
                compensation: e.compensation.clone(),
            })
            .collect()
    }
}
