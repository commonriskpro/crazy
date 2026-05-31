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

use crate::audit::{
    TRANSACTION_CATEGORY_COMMIT_AFTER_ROLLBACK, TRANSACTION_CATEGORY_COMMIT_ALREADY_COMMITTED,
    TRANSACTION_CATEGORY_COMMITTED, TRANSACTION_CATEGORY_ROLLBACK_AFTER_COMMIT,
    TRANSACTION_CATEGORY_ROLLBACK_ALREADY_ROLLED_BACK, TRANSACTION_CATEGORY_ROLLED_BACK,
};
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

impl TransactionStatus {
    /// Stable lowercase label for audit/report surfaces.
    pub fn as_str(&self) -> &'static str {
        match self {
            TransactionStatus::Pending => "pending",
            TransactionStatus::Committed => "committed",
            TransactionStatus::RolledBack => "rolled_back",
        }
    }
}

// ── TransactionAuditRecord ────────────────────────────────────────────────

/// Redacted, deterministic lifecycle record for transaction audit surfaces.
///
/// The record intentionally does **not** include the raw transaction group
/// name, capability names, idempotency keys, refund capabilities, payloads, or
/// handler errors. Those values can contain tenant IDs, secret names, order
/// IDs, or external-provider details. Use counts and stable categories for
/// operational decisions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransactionAuditRecord {
    /// Redacted shape of the transaction group name.
    pub group_name_shape: String,
    /// Requested lifecycle action (`"commit"` or `"rollback"`).
    pub action: &'static str,
    /// Stable machine-readable transaction category.
    pub category: &'static str,
    /// Status before the requested action.
    pub status_before: &'static str,
    /// Status after the requested action.
    pub status_after: &'static str,
    /// Number of capability entries tracked by the transaction.
    pub entry_count: usize,
    /// Number of entries marked non-rollbackable.
    pub non_rollbackable_count: usize,
    /// Number of non-rollbackable entries with explicit compensation.
    pub compensation_required_count: usize,
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

    /// Commit the group and return a redacted deterministic audit record.
    ///
    /// The transition remains idempotent like [`commit`](Self::commit), but
    /// callers can persist the returned record to an audit sink without
    /// leaking raw group names, capability names, idempotency keys, or refund
    /// capability names.
    pub fn commit_with_audit(&mut self) -> TransactionAuditRecord {
        let before = self.status.clone();
        self.commit();
        self.audit_record("commit", before)
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

    /// Roll back the group and return non-rollbackable capabilities plus a
    /// redacted deterministic audit record.
    ///
    /// The returned capability IDs preserve the existing rollback contract for
    /// compensation orchestration. The audit record intentionally exposes only
    /// counts and stable categories.
    pub fn rollback_with_audit(&mut self) -> (Vec<CapabilityId>, TransactionAuditRecord) {
        let before = self.status.clone();
        let non_rollbackable = self.rollback();
        let audit = self.audit_record("rollback", before);
        (non_rollbackable, audit)
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

    fn audit_record(
        &self,
        action: &'static str,
        status_before: TransactionStatus,
    ) -> TransactionAuditRecord {
        TransactionAuditRecord {
            group_name_shape: redacted_group_name_shape(&self.name).to_string(),
            action,
            category: transaction_audit_category(action, &status_before),
            status_before: status_before.as_str(),
            status_after: self.status.as_str(),
            entry_count: self.entries.len(),
            non_rollbackable_count: self.non_rollbackable_count(),
            compensation_required_count: self.compensation_required_count(),
        }
    }

    fn non_rollbackable_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.policy == TransactionPolicy::NonRollbackable)
            .count()
    }

    fn compensation_required_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| {
                entry.policy == TransactionPolicy::NonRollbackable
                    && entry.compensation != CompensationPolicy::None
            })
            .count()
    }
}

fn transaction_audit_category(action: &str, status_before: &TransactionStatus) -> &'static str {
    match (action, status_before) {
        ("commit", TransactionStatus::Pending) => TRANSACTION_CATEGORY_COMMITTED,
        ("commit", TransactionStatus::Committed) => TRANSACTION_CATEGORY_COMMIT_ALREADY_COMMITTED,
        ("commit", TransactionStatus::RolledBack) => TRANSACTION_CATEGORY_COMMIT_AFTER_ROLLBACK,
        ("rollback", TransactionStatus::Pending) => TRANSACTION_CATEGORY_ROLLED_BACK,
        ("rollback", TransactionStatus::Committed) => TRANSACTION_CATEGORY_ROLLBACK_AFTER_COMMIT,
        ("rollback", TransactionStatus::RolledBack) => {
            TRANSACTION_CATEGORY_ROLLBACK_ALREADY_ROLLED_BACK
        }
        _ => "transaction.unknown_action",
    }
}

fn redacted_group_name_shape(name: &str) -> &'static str {
    if name.is_empty() {
        return "empty";
    }
    if name.len() > 64 {
        return "too_long";
    }
    if !name.is_ascii() {
        return "non_ascii";
    }
    if name.bytes().any(|byte| byte.is_ascii_whitespace()) {
        return "contains_whitespace";
    }
    if name
        .bytes()
        .any(|byte| matches!(byte, b'/' | b'\\' | b':' | b'@'))
    {
        return "contains_separator";
    }
    if name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return "safe_label";
    }
    "other"
}
