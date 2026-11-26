//! Request-local model reference numbers. Persistent identities stay with the frozen rows.
use std::collections::{BTreeMap, BTreeSet};

use bgx_video_prompt_contract::validate::ProjectionContractError;
use bgx_video_prompt_contract::ProjectionClosedSet;

pub struct ReferenceNumbers {
    by_id: BTreeMap<String, String>,
    by_number: BTreeMap<String, String>,
}

const CONTRACT: &str = "The exact reference IDs required by the format method are the \
short values in references[].referenceUseId for this request. Copy those values \
into structured reference fields. Use the selected format's media labels in prompt prose.";

impl ReferenceNumbers {
    pub fn new<'a>(rows: impl IntoIterator<Item = (&'a str, &'a str, u32)>) -> Self {
        let rows: Vec<_> = rows.into_iter().collect();
        let ids: BTreeSet<_> = rows.iter().map(|(id, _, _)| *id).collect();
        let mut by_id = BTreeMap::new();
        let mut by_number = BTreeMap::new();
        let mut fallback = 1;
        let mut positions = BTreeMap::new();
        for (id, kind, ordinal) in rows {
            let kind = kind.trim();
            let position = positions.entry(kind).or_insert(0);
            *position += 1;
            // Older inventory inputs omit the ordinal. Their position within the
            // original kind is the same ordinal calculated by the host freeze.
            let ordinal = if ordinal == 0 { *position } else { ordinal };
            if by_id.contains_key(id) {
                continue;
            }
            let prefix = match kind {
                "reference_image" | "image" => "image",
                "reference_video" | "video" => "video",
                "reference_audio" | "audio" => "audio",
                "first_frame" => "firstFrame",
                "last_frame" => "lastFrame",
                _ => "ref",
            };
            let candidate = format!("{prefix}{ordinal}");
            let number = if prefix != "ref"
                && !by_number.contains_key(&candidate)
                && !ids.contains(candidate.as_str())
            {
                candidate
            } else {
                loop {
                    let candidate = format!("ref{fallback}");
                    fallback += 1;
                    if !by_number.contains_key(&candidate) && !ids.contains(candidate.as_str()) {
                        break candidate;
                    }
                }
            };
            by_number.insert(number.clone(), id.to_owned());
            by_id.insert(id.to_owned(), number);
        }
        Self { by_id, by_number }
    }

    pub fn number(&self, id: &str) -> Option<&str> {
        self.by_id.get(id).map(String::as_str)
    }

    pub fn resolve(&self, number: &str) -> Option<&str> {
        self.by_number.get(number).map(String::as_str)
    }

    pub(crate) fn model_contract(&self) -> Option<&'static str> {
        (!self.by_id.is_empty()).then_some(CONTRACT)
    }

    pub(crate) fn model_closed_set(&self, closed: &ProjectionClosedSet) -> ProjectionClosedSet {
        let mut model = closed.clone();
        model.reference_use_ids = self.encode_set(&closed.reference_use_ids);
        model.voice_reference_use_ids = self.encode_set(&closed.voice_reference_use_ids);
        model
    }

    fn encode_set(&self, ids: &BTreeSet<String>) -> BTreeSet<String> {
        ids.iter()
            .map(|id| self.number(id).expect("closed set uses the same frozen rows").to_owned())
            .collect()
    }

    pub(crate) fn restore(
        &self,
        number: &mut String,
        path: String,
    ) -> Result<(), ProjectionContractError> {
        let id = self.resolve(number).ok_or_else(|| ProjectionContractError::NotInClosedSet {
            path,
            id: number.clone(),
        })?;
        *number = id.to_owned();
        Ok(())
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_preserve_kinds_and_positions_and_never_accept_a_persistent_id() {
        let numbers = ReferenceNumbers::new([
            ("image1", "reference_image", 1),
            ("second", "reference_image", 0),
            ("other-kind", "image", 2),
            ("frame", "first_frame", 1),
            ("tail", "last_frame", 1),
            ("sound", "reference_audio", 3),
            ("motion", "reference_video", 1),
        ]);
        for (id, number) in [
            ("image1", "ref1"), ("other-kind", "ref2"), ("second", "image2"),
            ("frame", "firstFrame1"), ("tail", "lastFrame1"),
            ("sound", "audio3"), ("motion", "video1"),
        ] {
            assert_eq!(numbers.number(id), Some(number));
            assert_eq!(numbers.resolve(number), Some(id));
            assert_eq!(numbers.resolve(id), None);
        }
    }
}
