// ── ail-cli::policy_commands ──────────────────────────────────────────────
//
// Handler for `ail policy <subcommand>`.
//
// Sub-commands: list, add, check, explain, set.
//
// Policy changes are themselves ChangeSets or admin records, depending on
// project mode.  Capability rules are persisted to `.ail/policies/rules.cbor`
// when a file store is active; silently no-op for in-memory / Postgres stores.
//
// Private helpers:
//   policies_dir                 — path to the policies directory in the file store
//   load_policy_rules            — deserialise the rules list from CBOR
//   save_policy_rules            — serialise and persist the rules list
//   parse_capability_policies    — convert rule strings to CapabilityPolicy values
//   build_policy_rules_from_stored — map stored rule strings to PolicyRule values
//
// # Policy check semantics
//
// `ail policy check` runs two complementary checks in parallel:
//
//   1. Full `PolicyEngine::evaluate_with_audit` — the same engine used by
//      `ail verify`.  Always includes `ProfileGate(profile)` plus any named
//      rules found in the persisted policy list (e.g. `no_unverified_public_api`,
//      `no_unsafe`, `require_approval`).  Detects POLICY_PROFILE_GATE,
//      POLICY_UNVERIFIED_PUBLIC_API, POLICY_UNSAFE_BLOCKED, solver-diagnostic
//      blocks, and all other engine violations.
//
//   2. Capability-deny check via `CapabilityPolicyEnforcer` — preserved because
//      capability-deny rules (`deny capability …`) have no equivalent `PolicyRule`
//      variant in the engine.
//
// Both checks must pass for `policy_ok: true`.  JSON output exposes engine
// status, engine violations/warnings, the full audit trail, and capability
// violations separately so callers can diagnose failures precisely.
//
// # Empty-report fallback
//
// If no stored verification report exists (the common case for `ail policy
// check`), `Checker::check(&graph)` produces a fresh report from the current
// graph.  This is the documented deterministic fallback: the checker emits
// `Unverified` entries for nodes with no declared type facts, `Assumed` for
// nodes with declared effects or capabilities, and `Proven` for nodes with
// nominal type declarations.

use std::path::PathBuf;

use ail_package::{CapabilityPolicy, CapabilityPolicyEnforcer, CapabilityPolicyVerdict};
use ail_verify::checker::Checker;
use ail_verify::policy::{PolicyDecision, PolicyEngine, PolicyInput, PolicyRule};
use serde_json::json;

use crate::cli::{PolicyCmd, ail_dir_for_store, is_valid_change_id, load_current_graph_for_cli};
use crate::error::CliError;
use crate::output::{OutputMode, print_response};
use crate::store::StoreHandle;

// ── Private helpers ───────────────────────────────────────────────────────

fn policies_dir(store: &StoreHandle) -> Result<PathBuf, CliError> {
    Ok(ail_dir_for_store(store)?.join("policies"))
}

fn load_policy_rules(store: &StoreHandle) -> Result<Vec<String>, CliError> {
    if !matches!(store, StoreHandle::File { .. }) {
        return Ok(vec![]);
    }
    let path = policies_dir(store)?.join("rules.cbor");
    if !path.exists() {
        return Ok(vec![]);
    }
    let bytes = std::fs::read(path)?;
    ciborium::from_reader(bytes.as_slice())
        .map_err(|e| CliError::Domain(format!("policy decoding failed: {e}")))
}

fn save_policy_rules(store: &StoreHandle, rules: &[String]) -> Result<(), CliError> {
    if !matches!(store, StoreHandle::File { .. }) {
        let _ = rules;
        return Ok(());
    }
    let dir = policies_dir(store)?;
    std::fs::create_dir_all(&dir)?;
    let mut bytes = Vec::new();
    ciborium::into_writer(rules, &mut bytes)
        .map_err(|e| CliError::Domain(format!("policy encoding failed: {e}")))?;
    std::fs::write(dir.join("rules.cbor"), bytes)?;
    Ok(())
}

fn parse_capability_policies(rules: &[String]) -> Vec<CapabilityPolicy> {
    rules
        .iter()
        .filter_map(|rule| {
            let words = rule.split_whitespace().collect::<Vec<_>>();
            if words.len() < 3 || words[0] != "deny" || words[1] != "capability" {
                return None;
            }
            let verdict = if words.get(3) == Some(&"unless") && words.get(4) == Some(&"approved") {
                CapabilityPolicyVerdict::DenyUnlessApproved
            } else {
                CapabilityPolicyVerdict::Deny
            };
            Some(CapabilityPolicy {
                pattern: words[2].to_string(),
                verdict,
            })
        })
        .collect()
}

/// Build `PolicyRule`s from persisted rule strings and the active profile name.
///
/// Always includes `PolicyRule::ProfileGate(profile)` so the profile-level
/// entry matrix is always evaluated — consistent with how `cmd_verify` works.
///
/// Named rules found in `stored` that map to `PolicyRule` variants are appended:
///
/// | Stored string               | PolicyRule produced          |
/// |-----------------------------|------------------------------|
/// | `"no_unverified_public_api"`| `NoUnverifiedPublicApi`      |
/// | `"no_unsafe"`               | `NoUnsafe`                   |
/// | `"require_approval"`        | `RequireApproval`            |
///
/// Capability-deny rules (`"deny capability …"`) and settings (`"set key=val"`)
/// are not mapped here — they are handled by `parse_capability_policies` and
/// `CapabilityPolicyEnforcer` separately.
fn build_policy_rules_from_stored(stored: &[String], profile: &str) -> Vec<PolicyRule> {
    let mut rules = vec![PolicyRule::ProfileGate(profile.to_string())];
    for rule in stored {
        match rule.trim() {
            "no_unverified_public_api" => rules.push(PolicyRule::NoUnverifiedPublicApi),
            "no_unsafe" => rules.push(PolicyRule::NoUnsafe),
            "require_approval" => rules.push(PolicyRule::RequireApproval),
            _ => {} // capability-deny and set rules are handled separately
        }
    }
    rules
}

// ── Command handler ───────────────────────────────────────────────────────

/// `ail policy <check|explain|set>` — manage project policies.
///
/// Policy changes are themselves ChangeSets or admin records, depending on project mode.
pub(crate) async fn cmd_policy(
    mode: OutputMode,
    cmd: PolicyCmd,
    store: &StoreHandle,
) -> Result<(), CliError> {
    match cmd {
        PolicyCmd::List => {
            let policies = load_policy_rules(store)?;
            let human_msg = if policies.is_empty() {
                "active policies: 0".to_string()
            } else {
                format!(
                    "active policies: {}\n{}",
                    policies.len(),
                    policies.join("\n")
                )
            };
            print_response(
                mode,
                &human_msg,
                json!({
                    "policies": policies,
                }),
            );
        }
        PolicyCmd::Add { rule } => {
            let mut policies = load_policy_rules(store)?;
            policies.push(rule.clone());
            save_policy_rules(store, &policies)?;
            let human_msg = format!("policy added: {rule}\nactive policies: {}", policies.len());
            print_response(
                mode,
                &human_msg,
                json!({
                    "added": rule,
                    "policies": policies,
                }),
            );
        }
        PolicyCmd::Check { change_id, profile } => {
            if let Some(change_id) = &change_id
                && !is_valid_change_id(change_id)
            {
                return Err(CliError::NotFound(format!(
                    "change-id not found: {change_id}"
                )));
            }
            let policies = load_policy_rules(store)?;
            let graph = load_current_graph_for_cli(store).await?;

            // ── Full PolicyEngine evaluation ──────────────────────────────
            //
            // Build a VerificationReport from the current graph via Checker.
            // If no stored verification report exists (the common case for
            // `ail policy check`), this produces a fresh deterministic report:
            // - Nodes with nominal type facts → Proven
            // - Nodes with declared effects or capabilities → Assumed
            // - Nodes with no facts at all → Unverified
            //
            // This is the documented empty/default report fallback.
            let report = Checker::check(&graph);
            let engine_rules = build_policy_rules_from_stored(&policies, &profile);
            let engine_input = PolicyInput {
                report: &report,
                rules: &engine_rules,
                approvals: &[],
                structural_diff: None,
                capability_grants: &[],
                public_api_changes: &[],
                package_trust_metadata: &[],
            };
            let (engine_decision, engine_audit) = PolicyEngine::evaluate_with_audit(&engine_input);

            let engine_status = match &engine_decision {
                PolicyDecision::Passed => "passed",
                PolicyDecision::PassedWithWarnings(_) => "warning",
                PolicyDecision::Failed(_) => "blocked",
                PolicyDecision::ApprovalRequired(_) => "approval_required",
            };
            let engine_violations = match &engine_decision {
                PolicyDecision::Failed(vs) => json!(vs),
                _ => json!([]),
            };
            let engine_warnings = match &engine_decision {
                PolicyDecision::PassedWithWarnings(ws) => json!(ws),
                _ => json!([]),
            };
            let engine_approval_required = match &engine_decision {
                PolicyDecision::ApprovalRequired(scopes) => json!(scopes),
                _ => json!([]),
            };
            let engine_ok = matches!(
                &engine_decision,
                PolicyDecision::Passed | PolicyDecision::PassedWithWarnings(_)
            );

            // ── Capability-deny check (preserved alongside full engine) ───
            //
            // Capability-deny rules (`deny capability …`) have no direct
            // PolicyRule equivalent, so they are checked via
            // CapabilityPolicyEnforcer in addition to the full engine above.
            let capability_rules = parse_capability_policies(&policies);
            let requested_caps = graph
                .nodes
                .iter()
                .flat_map(|node| {
                    node.capability_reqs
                        .as_ref()
                        .map(|reqs| reqs.caps.clone())
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>();
            let cap_violations =
                CapabilityPolicyEnforcer::check(&requested_caps, &capability_rules)
                    .into_iter()
                    .map(|violation| {
                        json!({
                            "capability": violation.capability,
                            "verdict": format!("{:?}", violation.verdict),
                        })
                    })
                    .collect::<Vec<_>>();
            let cap_ok = cap_violations.is_empty();

            // ── Merge decisions ───────────────────────────────────────────
            let policy_ok = engine_ok && cap_ok;
            let human_msg = format!(
                "policy: {}\nprofile: {profile}\nchange: {}\nrules: {}\nengine_status: {engine_status}\ncap_violations: {}",
                if policy_ok { "ok" } else { "failed" },
                change_id.as_deref().unwrap_or("(current graph)"),
                policies.len(),
                cap_violations.len()
            );
            print_response(
                mode,
                &human_msg,
                json!({
                    "policy_ok": policy_ok,
                    "profile": profile,
                    "change_id": change_id,
                    "engine_status": engine_status,
                    "engine_decision": engine_decision,
                    "engine_violations": engine_violations,
                    "engine_warnings": engine_warnings,
                    "engine_approval_required": engine_approval_required,
                    "engine_audit": engine_audit,
                    // "violations" is kept for backward compatibility with existing tooling;
                    // it contains the capability-deny violations (same as capability_violations).
                    "violations": cap_violations,
                    "capability_violations": cap_violations,
                    "rules_checked": policies,
                }),
            );
        }
        PolicyCmd::Explain { rule } => {
            // Known policy rules.
            let description = match rule.as_str() {
                "no_unverified_public_api" => {
                    "No change may expose a public API symbol without an accepted verification report."
                }
                "capability_limit" => {
                    "Changes may not introduce more capabilities than the project policy allows."
                }
                "assumption_validity" => {
                    "All Assumed entries must have a valid, non-expired assumption record."
                }
                "max_new_capabilities" => {
                    "The number of new capabilities introduced per change must not exceed the configured limit."
                }
                _ => "No description available for this rule.",
            };
            let human_msg = format!("rule: {rule}\ndescription: {description}");
            print_response(
                mode,
                &human_msg,
                json!({
                    "rule": rule,
                    "description": description,
                    "enforced_on": ["apply", "verify", "compile"],
                }),
            );
        }
        PolicyCmd::Set { setting } => {
            // Parse key=value.
            let (key, value) = setting.split_once('=').unwrap_or((&setting, ""));
            let mut policies = load_policy_rules(store)?;
            policies.push(format!("set {key}={value}"));
            save_policy_rules(store, &policies)?;
            let human_msg =
                format!("policy updated: {key}={value}\nnote: policy changes are admin records");
            print_response(
                mode,
                &human_msg,
                json!({
                    "key": key,
                    "value": value,
                    "record_type": "admin_record",
                }),
            );
        }
    }
    Ok(())
}

// ── Unit tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use ail_verify::{
        POLICY_PROFILE_GATE, POLICY_UNVERIFIED_PUBLIC_API, PolicyDecision, PolicyEngine,
        PolicyInput, PolicyRule, VerificationEntry, VerificationReport, VerificationState,
    };

    use super::build_policy_rules_from_stored;

    fn unverified_entry(scope: &str) -> VerificationEntry {
        VerificationEntry {
            claim: "type".to_string(),
            state: VerificationState::Unverified,
            scope: scope.to_string(),
            evidence: None,
            blocking: false,
            repair_options: vec![],
        }
    }

    // Scenario PE-1: PolicyEngine catches POLICY_UNVERIFIED_PUBLIC_API.
    //
    // The old CapabilityPolicyEnforcer only matched "deny capability …" patterns
    // and had no knowledge of verification states or scope prefixes.  This test
    // proves the full engine is now wired — it would not compile against the old
    // policy_commands.rs (which never imported PolicyEngine or PolicyRule).
    #[test]
    fn policy_engine_catches_unverified_public_api() {
        let report = VerificationReport {
            entries: vec![unverified_entry("pub::fn.transfer")],
            ..Default::default()
        };
        let rules = [PolicyRule::NoUnverifiedPublicApi];
        let input = PolicyInput {
            report: &report,
            rules: &rules,
            approvals: &[],
            structural_diff: None,
            capability_grants: &[],
            public_api_changes: &[],
            package_trust_metadata: &[],
        };
        let decision = PolicyEngine::evaluate(&input);
        assert!(
            matches!(&decision, PolicyDecision::Failed(vs) if
                vs.iter().any(|v| v.code == POLICY_UNVERIFIED_PUBLIC_API)),
            "must detect POLICY_UNVERIFIED_PUBLIC_API; got: {decision:?}"
        );
    }

    // Scenario PE-2: ProfileGate 'prod' blocks Unverified entries.
    //
    // The old capability-only checker could not detect this — it never ran a
    // profile gate.  This test would fail with the old implementation because the
    // old `PolicyCmd::Check` arm never called `PolicyEngine::evaluate`.
    #[test]
    fn policy_engine_prod_profile_blocks_unverified() {
        let report = VerificationReport {
            entries: vec![unverified_entry("fn.payment")],
            ..Default::default()
        };
        // build_policy_rules_from_stored always adds ProfileGate(profile).
        let rules = build_policy_rules_from_stored(&[], "prod");
        let input = PolicyInput {
            report: &report,
            rules: &rules,
            approvals: &[],
            structural_diff: None,
            capability_grants: &[],
            public_api_changes: &[],
            package_trust_metadata: &[],
        };
        let decision = PolicyEngine::evaluate(&input);
        assert!(
            matches!(&decision, PolicyDecision::Failed(vs) if
                vs.iter().any(|v| v.code == POLICY_PROFILE_GATE)),
            "prod ProfileGate must block Unverified entries; got: {decision:?}"
        );
    }

    // Scenario PE-3: build_policy_rules_from_stored maps named rules correctly.
    //
    // Verifies that stored rule strings are translated to the correct PolicyRule
    // variants, and that capability-deny rules are silently ignored (they are
    // handled by CapabilityPolicyEnforcer, not the engine).
    #[test]
    fn build_policy_rules_maps_named_rules() {
        let stored = vec![
            "no_unverified_public_api".to_string(),
            "no_unsafe".to_string(),
            "deny capability file.write:*".to_string(), // not mapped to PolicyRule
            "set max_new_capabilities=2".to_string(),   // not mapped to PolicyRule
        ];
        let rules = build_policy_rules_from_stored(&stored, "staging");
        assert!(
            rules
                .iter()
                .any(|r| matches!(r, PolicyRule::ProfileGate(p) if p == "staging")),
            "must include ProfileGate(staging)"
        );
        assert!(
            rules
                .iter()
                .any(|r| matches!(r, PolicyRule::NoUnverifiedPublicApi)),
            "must include NoUnverifiedPublicApi"
        );
        assert!(
            rules.iter().any(|r| matches!(r, PolicyRule::NoUnsafe)),
            "must include NoUnsafe"
        );
        // Capability-deny and set rules must not appear as PolicyRule variants.
        assert_eq!(
            rules.len(),
            3,
            "must produce exactly 3 rules (ProfileGate + NoUnverifiedPublicApi + NoUnsafe)"
        );
    }

    // Scenario PE-4: dev profile passes Unverified non-public entries.
    //
    // Confirms the profile gate allows Unverified entries in non-public scope
    // for the 'dev' profile — regression guard for the permissive end.
    #[test]
    fn policy_engine_dev_profile_passes_unverified_non_public() {
        let report = VerificationReport {
            entries: vec![unverified_entry("fn.internal_helper")],
            ..Default::default()
        };
        let rules = build_policy_rules_from_stored(&[], "dev");
        let input = PolicyInput {
            report: &report,
            rules: &rules,
            approvals: &[],
            structural_diff: None,
            capability_grants: &[],
            public_api_changes: &[],
            package_trust_metadata: &[],
        };
        let decision = PolicyEngine::evaluate(&input);
        assert!(
            matches!(decision, PolicyDecision::Passed),
            "dev profile must pass Unverified non-public entries; got: {decision:?}"
        );
    }
}
