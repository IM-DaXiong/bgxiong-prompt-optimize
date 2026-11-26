//! Cross-field effect policy rules shared by the build, client-pack verifier and runtime.
//! Invalid declarations must fail before a host can be built, not while its kernel lock is held.

use serde_json::Value;
use std::collections::BTreeSet;

pub fn verify(policy: &Value) -> Result<(), String> {
    let entries = policy["entries"]
        .as_array()
        .ok_or("creator-effect-policy.json: entries must be an array")?;
    let mut keys = BTreeSet::new();
    for entry in entries {
        let tool = entry["toolId"].as_str().ok_or("effect policy toolId missing")?;
        let action = match &entry["actionType"] {
            Value::Null => None,
            Value::String(value) if !value.is_empty() => Some(value.as_str()),
            _ => return Err(format!("effect policy {tool}: invalid actionType")),
        };
        let at = format!("effect policy {tool}#{action:?}");
        if !keys.insert((tool, action)) {
            return Err(format!("effect policy duplicates {tool}#{action:?}"));
        }
        let field = |key: &str| {
            entry[key].as_str().ok_or_else(|| format!("{at}: {key} must be a string"))
        };
        let kind = field("receiptKind")?;
        let grade = field("transactionGrade")?;
        let scope = field("scopeMapper")?;
        let refresh = field("refreshRequirement")?;
        let replay = field("replaySemantics")?;
        let outcomes = entry["outcomeStates"].as_array()
            .filter(|items| !items.is_empty() && items.iter().all(Value::is_string))
            .ok_or_else(|| format!("{at} has no valid outcomeStates"))?;
        if field("receiptMapper")?.trim().is_empty() {
            return Err(format!("{at} has an empty receiptMapper"));
        }
        if let Some(rule) = entry.get("resultClassification").filter(|rule| !rule.is_null()) {
            let valid = action.is_some()
                && rule["pointer"].as_str().is_some_and(|pointer| pointer.starts_with('/'))
                && rule["outcomes"].as_object().is_some_and(|mapping| {
                    !mapping.is_empty() && mapping.iter().all(|(state, receipt)| {
                        outcomes.iter().any(|outcome| outcome.as_str() == Some(state))
                            && receipt.as_str().is_some()
                            && policy["receiptKinds"].as_array()
                                .is_some_and(|kinds| kinds.contains(receipt))
                    })
                });
            if !valid {
                return Err(format!("{at} invalid structured result classification"));
            }
        }
        if kind == "observation" && (grade != "none" || scope != "none" || refresh != "none") {
            return Err(format!("{at} is an observation but declares transaction/scope/refresh"));
        }
        if refresh == "scopes" && scope == "none" {
            return Err(format!("{at} requires scope refresh without a scopeMapper"));
        }
        if matches!(kind, "mutation" | "inverse")
            && (!matches!(grade, "singleDbTransaction" | "compensated") || refresh != "scopes")
        {
            return Err(format!("{at} is a durable mutation without a transactional grade and scope refresh"));
        }
        let expected_replay = match kind {
            "observation" => Some("readCurrent"),
            "plan" => Some("planToken"),
            "requestControl" | "sessionControl" => Some("requestCas"),
            "mutation" | "inverse" | "fileEffect" | "taskAccepted" => Some("idempotentReceipt"),
            "uiDraft" | "planCancel" | "taskControl" | "refreshDelivery" => None,
            _ => return Err(format!("{at} has unknown receiptKind {kind}")),
        };
        if let Some(expected) = expected_replay {
            if replay != expected {
                return Err(format!("{at} is {kind} but replays as {replay} (must be {expected})"));
            }
        }
    }
    Ok(())
}
