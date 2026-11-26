//! Mother-author input projection (PLAN 20261013 R17).
//! Strips host-only speaker bindings before the mother model sees the ledger.
//! No host, Tauri, DB, or model dispatch.

use serde_json::Value;

/// Only project known typed vocal collections. User prose and every original host field remain
/// untouched; the host restores the speaker binding after strict output validation.
pub fn project_mother_input(input: &Value) -> Value {
    let mut view = input.clone();
    for path in [
        "/vocalOccurrences",
        "/frozenSource/vocalOccurrences",
        "/ledger/vocalOccurrences",
        "/ledger/frozenSource/vocalOccurrences",
    ] {
        if let Some(rows) = view.pointer_mut(path).and_then(Value::as_array_mut) {
            for row in rows {
                if let Some(row) = row.as_object_mut() {
                    row.remove("speakerId");
                }
            }
        }
    }
    if let Some(shots) = view.get_mut("previousShots").and_then(Value::as_array_mut) {
        for shot in shots {
            if let Some(quotes) = shot.get_mut("quotes").and_then(Value::as_array_mut) {
                for quote in quotes {
                    if let Some(quote) = quote.as_object_mut() {
                        quote.remove("speakerId");
                    }
                }
            }
        }
    }
    view
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn strips_speaker_id_without_touching_other_fields() {
        let ledger = json!({
            "vocalOccurrences": [{"speakerId": "S1", "exactText": "站住"}],
            "keep": 1
        });
        let original = ledger.clone();
        let view = project_mother_input(&ledger);
        assert_eq!(ledger, original);
        assert!(view["vocalOccurrences"][0].get("speakerId").is_none());
        assert_eq!(view["vocalOccurrences"][0]["exactText"], json!("站住"));
        assert_eq!(view["keep"], json!(1));
    }

    #[test]
    fn strips_previous_shots_quote_speaker() {
        let input = json!({
            "previousShots": [{
                "quotes": [{"quoteId": "Q1", "speakerId": "S1", "exactText": "hi"}]
            }]
        });
        let view = project_mother_input(&input);
        assert!(view["previousShots"][0]["quotes"][0].get("speakerId").is_none());
        assert_eq!(view["previousShots"][0]["quotes"][0]["exactText"], json!("hi"));
    }
}
