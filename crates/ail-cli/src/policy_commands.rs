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
//   policies_dir           — path to the policies directory in the file store
//   load_policy_rules      — deserialise the rules list from CBOR
//   save_policy_rules      — serialise and persist the rules list
//   parse_capability_policies — convert rule strings to CapabilityPolicy values

use std::path::PathBuf;

use ail_package::{CapabilityPolicy, CapabilityPolicyEnforcer, CapabilityPolicyVerdict};
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
            let violations = CapabilityPolicyEnforcer::check(&requested_caps, &capability_rules)
                .into_iter()
                .map(|violation| {
                    json!({
                        "capability": violation.capability,
                        "verdict": format!("{:?}", violation.verdict),
                    })
                })
                .collect::<Vec<_>>();
            let policy_ok = violations.is_empty();
            let human_msg = format!(
                "policy: {}\nprofile: {profile}\nchange: {}\nrules: {}\nviolations: {}",
                if policy_ok { "ok" } else { "failed" },
                change_id.as_deref().unwrap_or("(current graph)"),
                policies.len(),
                violations.len()
            );
            print_response(
                mode,
                &human_msg,
                json!({
                    "policy_ok": policy_ok,
                    "profile": profile,
                    "change_id": change_id,
                    "violations": violations,
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
