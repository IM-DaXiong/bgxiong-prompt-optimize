//! ProjectionClosedSet (PLAN 20261013 R14c). dual_slot pure helpers live in channels.

use std::collections::{BTreeMap, BTreeSet};

use crate::channels::{expanded_document, resolve_channels, FrozenNegativeContext};
use crate::document::DocumentDraftV1;
use crate::projection::{
    VideoPromptExpansionPolicy, VideoPromptProjectionDraft, VideoPromptTargetSnapshot,
};
use crate::validate::{check_text, invalid, limits, ProjectionContractError};

#[derive(Clone)]
pub struct ShotIds {
    pub facts: BTreeSet<String>,
    pub quotes: BTreeSet<String>,
}

#[derive(Clone)]
pub struct ProjectionClosedSet {
    pub shots: BTreeMap<String, ShotIds>,
    pub all_facts: BTreeSet<String>,
    pub all_quotes: BTreeSet<String>,
    pub reference_use_ids: BTreeSet<String>,
    pub speaker_ids: BTreeSet<String>,
    pub voice_reference_use_ids: BTreeSet<String>,
    pub speaker_subject_ids: BTreeSet<String>,
    pub mother: DocumentDraftV1,
    pub expansion_policy: Option<VideoPromptExpansionPolicy>,
    pub negative_context: Option<FrozenNegativeContext>,
}

impl ProjectionClosedSet {
    pub fn with_expansion_policy(mut self, policy: Option<VideoPromptExpansionPolicy>) -> Self {
        self.expansion_policy = policy;
        self
    }
    pub fn with_negative_context(mut self, context: Option<FrozenNegativeContext>) -> Self {
        self.negative_context = context;
        self
    }
    pub fn expanded(
        &self,
        draft: &VideoPromptProjectionDraft,
    ) -> Result<Self, ProjectionContractError> {
        let document = expanded_document(&self.mother, draft, self.expansion_policy.as_ref())
            .map_err(|_| invalid("proposedFacts", "invalidExpansionSourceOrFact"))?;
        resolve_channels(self.negative_context.as_ref(), draft)
            .map_err(|_| invalid("negative", "invalidNegativeOperationOrSource"))?;
        let mut result = self.clone();
        for shot in document.shots {
            let facts: BTreeSet<_> = shot.facts.iter().map(|f| f.fact_id.clone()).collect();
            result.all_facts.extend(facts.iter().cloned());
            if let Some(ids) = result.shots.get_mut(&shot.shot_id) {
                ids.facts = facts;
            }
        }
        Ok(result)
    }
    pub fn shot_ids(&self) -> Vec<String> {
        self.shots.keys().cloned().collect()
    }
    pub fn fact_ids(&self) -> Vec<String> {
        self.all_facts.iter().cloned().collect()
    }
    pub fn entity_ids(&self) -> Vec<String> {
        self.mother
            .shots
            .iter()
            .flat_map(|shot| &shot.facts)
            .flat_map(|fact| fact.entity_ref_ids.iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
    pub fn quote_ids(&self) -> Vec<String> {
        self.all_quotes.iter().cloned().collect()
    }
    pub fn reference_use_ids(&self) -> Vec<String> {
        self.reference_use_ids.iter().cloned().collect()
    }
    pub fn speaker_ids(&self) -> Vec<String> {
        self.speaker_ids.iter().cloned().collect()
    }
    pub fn voice_reference_use_ids(&self) -> Vec<String> {
        self.voice_reference_use_ids.iter().cloned().collect()
    }
    pub fn speaker_subject_ids(&self) -> Vec<String> {
        self.speaker_subject_ids.iter().cloned().collect()
    }
    pub fn with_speaker_voice_binding(
        mut self,
        voice_reference_use_ids: Vec<String>,
        subject_ids: Vec<String>,
    ) -> Self {
        self.voice_reference_use_ids = voice_reference_use_ids
            .into_iter()
            .filter(|id| self.reference_use_ids.contains(id))
            .collect();
        if self.voice_reference_use_ids.is_empty() {
            self.speaker_subject_ids.clear();
        } else {
            self.speaker_subject_ids = subject_ids.into_iter().collect();
        }
        self
    }
    pub fn speaker_voice_binding_offer(&self) -> serde_json::Value {
        if self.voice_reference_use_ids.is_empty() || self.speaker_ids.is_empty() {
            return serde_json::Value::Null;
        }
        serde_json::json!({
            "speakerIds": self.speaker_ids(),
            "voiceReferenceUseIds": self.voice_reference_use_ids(),
            "subjectIds": self.speaker_subject_ids(),
        })
    }
    pub fn shot_fact_ids(&self, shot_id: &str) -> Vec<String> {
        self.shots
            .get(shot_id)
            .map(|shot| shot.facts.iter().cloned().collect())
            .unwrap_or_default()
    }
    pub fn shot_quote_ids(&self, shot_id: &str) -> Vec<String> {
        self.shots
            .get(shot_id)
            .map(|shot| shot.quotes.iter().cloned().collect())
            .unwrap_or_default()
    }
    pub fn new(mother: &DocumentDraftV1, target: &VideoPromptTargetSnapshot) -> Self {
        let mut shots = BTreeMap::new();
        let mut all_facts = BTreeSet::new();
        let mut all_quotes = BTreeSet::new();
        let mut speaker_ids = BTreeSet::new();
        for shot in &mother.shots {
            let facts: BTreeSet<String> = shot.facts.iter().map(|f| f.fact_id.clone()).collect();
            let quotes: BTreeSet<String> = shot.quotes.iter().map(|q| q.quote_id.clone()).collect();
            all_facts.extend(facts.iter().cloned());
            all_quotes.extend(quotes.iter().cloned());
            for quote in &shot.quotes {
                if let Some(speaker_id) = quote.speaker_id.as_ref() {
                    let trimmed = speaker_id.trim();
                    if !trimmed.is_empty() {
                        speaker_ids.insert(trimmed.to_string());
                    }
                }
            }
            shots.insert(shot.shot_id.clone(), ShotIds { facts, quotes });
        }
        ProjectionClosedSet {
            shots,
            all_facts,
            all_quotes,
            reference_use_ids: target.reference_use_ids.iter().cloned().collect(),
            speaker_ids,
            voice_reference_use_ids: BTreeSet::new(),
            speaker_subject_ids: BTreeSet::new(),
            mother: mother.clone(),
            expansion_policy: None,
            negative_context: None,
        }
    }

    pub fn shot(
        &self,
        path: &str,
        shot_id: &str,
    ) -> Result<&ShotIds, ProjectionContractError> {
        check_text(path, shot_id, limits().local_id_max)?;
        self.shots
            .get(shot_id)
            .ok_or_else(|| ProjectionContractError::NotInClosedSet {
                path: path.to_string(),
                id: shot_id.to_string(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{DocumentDraftV1, PromptFactV1, ShotDraftV1};
    use crate::projection::VideoPromptExpressionLanguage;

    #[test]
    fn closed_set_indexes_mother_facts() {
        let mother = DocumentDraftV1 {
            shots: vec![ShotDraftV1 {
                shot_id: "S1".into(),
                facts: vec![PromptFactV1 {
                    fact_id: "f1".into(),
                    kind: "action".into(),
                    text: "walks".into(),
                    semantic_slot: "picture".into(),
                    polarity: None,
                    provenance: "user_text".into(),
                    source_fact_id: None,
                    entity_ref_ids: vec![],
                    source_unit_ids: vec![],
                }],
                quotes: vec![],
            }],
        };
        let target = VideoPromptTargetSnapshot {
            caps_digest: "a".repeat(64),
            duration_seconds: None,
            expression_language: VideoPromptExpressionLanguage::Zh,
            input_mode: "text".into(),
            profile_id: "p".into(),
            profile_version: 1,
            reference_use_ids: vec!["r1".into()],
        };
        let closed = ProjectionClosedSet::new(&mother, &target);
        assert_eq!(closed.shot_ids(), vec!["S1".to_string()]);
        assert_eq!(closed.fact_ids(), vec!["f1".to_string()]);
        assert_eq!(closed.reference_use_ids(), vec!["r1".to_string()]);
        assert!(closed.shot("lines[0].shotId", "S1").is_ok());
        assert!(closed.shot("lines[0].shotId", "missing").is_err());
    }
}
