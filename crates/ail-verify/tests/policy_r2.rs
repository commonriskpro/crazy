// ── ail-verify::policy Round-2 tests ─────────────────────────────────────
//
// Strict TDD — RED phase.  All tests below must FAIL before the implementation
// is updated.  They encode the missing/warning gaps from the Round-1 verifier
// report:
//
//   MISSING:
//     R2-1  Strict-by-default fallback (unknown profile → conservative/block)
//     R2-2  PolicyInput: structural_diff, capability_grants, public_api_changes,
//           package_trust_metadata fields
//     R2-3  Assumed gating: approved vs unapproved distinctions
//
//   PARTIALLY COVERED → FULLY COVERED:
//     R2-4  Draft profile — warnings for Unverified, Assumed must be annotated
//     R2-5  Dev profile — assumed with boundary, private-unverified-annotated-only
//     R2-6  Test profile — test-only assumptions model
//     R2-7  Staging — unapproved Assumed blocks
//     R2-8  Prod — unapproved Assumed blocks; security exception for Unsafe
//     R2-9  Critical — strong-approval/weak-assumption/runtime_checked gate
//     R2-10 Report integration — richer policy/approvals audit sections
//     R2-11 Pipeline — ChangeSetOp::Verify wired end-to-end with checker→policy

#[path = "policy_r2/approval.rs"]
mod approval;
#[path = "policy_r2/blocking.rs"]
mod blocking;
#[path = "policy_r2/helpers.rs"]
mod helpers;
#[path = "policy_r2/profiles.rs"]
mod profiles;
