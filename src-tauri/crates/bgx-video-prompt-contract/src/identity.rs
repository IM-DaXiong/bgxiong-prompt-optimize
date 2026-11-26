//! Identity lock line + lock-template compiler (PLAN 20261013 R14b).
//! Pure: no host/Tauri/DB. Projector tokens come from [`ReferenceTokenResolver`].

use crate::inventory::DeclaredIdentityV1;
use crate::projection::VideoPromptTargetSnapshot;

include!(concat!(
    env!("OUT_DIR"),
    "/app_contracts_video_prompt_identity_lock.rs"
));

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityLockLine {
    pub shot_id: String,
    pub subject_id: String,
    pub binding_id: String,
    pub text: String,
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockTemplatePart {
    Lit(String),
    Name,
    Token,
}

/// Tagged lock template: exactly one `{name}` and one `{token}`.
pub fn parse_lock_template(tpl: &str) -> Result<Vec<LockTemplatePart>, String> {
    parse_template(tpl, 1)
}

/// Neutral lock template (the target writes no reference tag, CF-21): one `{name}`, no `{token}`.
pub fn parse_neutral_lock_template(tpl: &str) -> Result<Vec<LockTemplatePart>, String> {
    parse_template(tpl, 0)
}

fn parse_template(tpl: &str, token_slots: usize) -> Result<Vec<LockTemplatePart>, String> {
    let mut parts = Vec::new();
    let mut lit = String::new();
    let mut rest = tpl;
    let mut name_count = 0usize;
    let mut token_count = 0usize;
    while !rest.is_empty() {
        if let Some(stripped) = rest.strip_prefix("{name}") {
            if !lit.is_empty() {
                parts.push(LockTemplatePart::Lit(std::mem::take(&mut lit)));
            }
            parts.push(LockTemplatePart::Name);
            name_count += 1;
            rest = stripped;
            continue;
        }
        if let Some(stripped) = rest.strip_prefix("{token}") {
            if !lit.is_empty() {
                parts.push(LockTemplatePart::Lit(std::mem::take(&mut lit)));
            }
            parts.push(LockTemplatePart::Token);
            token_count += 1;
            rest = stripped;
            continue;
        }
        let mut chars = rest.chars();
        let ch = chars.next().unwrap_or_default();
        if ch == '{' || ch == '}' {
            return Err("TEMPLATE_INVALID".into());
        }
        lit.push(ch);
        rest = chars.as_str();
    }
    if !lit.is_empty() {
        parts.push(LockTemplatePart::Lit(lit));
    }
    if name_count != 1 || token_count != token_slots {
        return Err("TEMPLATE_INVALID".into());
    }
    Ok(parts)
}

pub fn render_lock_template(parts: &[LockTemplatePart], name: &str, token: &str) -> String {
    let mut out = String::new();
    for part in parts {
        match part {
            LockTemplatePart::Lit(text) => out.push_str(text),
            LockTemplatePart::Name => out.push_str(name),
            LockTemplatePart::Token => out.push_str(token),
        }
    }
    out
}

pub fn lock_template_for_language(language: &str) -> Result<Vec<LockTemplatePart>, String> {
    let raw = if language.eq_ignore_ascii_case("en") {
        IDENTITY_LOCK_TEMPLATE_EN
    } else {
        IDENTITY_LOCK_TEMPLATE_ZH
    };
    parse_lock_template(raw)
}

pub fn neutral_lock_template_for_language(language: &str) -> Result<Vec<LockTemplatePart>, String> {
    let raw = if language.eq_ignore_ascii_case("en") {
        IDENTITY_LOCK_TEMPLATE_NEUTRAL_EN
    } else {
        IDENTITY_LOCK_TEMPLATE_NEUTRAL_ZH
    };
    parse_neutral_lock_template(raw)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShotSubjectV1 {
    pub shot_id: String,
    pub subject_id: String,
    pub display_name: String,
    pub entity_kind: String,
    pub entity_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceUseV2 {
    pub use_id: String,
    pub item_id: String,
    pub use_kind: String,
    pub subject_id: Option<String>,
    pub panel_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityReferenceBindingV2 {
    pub binding_id: String,
    pub shot_id: String,
    pub subject_id: String,
    pub reference_use_id: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrozenInventoryItem {
    pub item_id: String,
    pub media_kind: String,
    pub ordinal_within_kind: u32,
    pub declared: Option<DeclaredIdentityV1>,
    pub label: String,
    /// Closed-set usage for reference audio. Images stay None.
    pub audio_usage: Option<String>,
}

/// Slice view for [`compile_identity_lock_lines`] (host may own richer PreparedIdentity).
pub struct LockCompileView<'a> {
    pub items: &'a [FrozenInventoryItem],
    pub subjects: &'a [ShotSubjectV1],
    pub uses: &'a [ReferenceUseV2],
    pub bindings: &'a [IdentityReferenceBindingV2],
    pub target: &'a VideoPromptTargetSnapshot,
}

/// Adapter/host supplies reference markers without pulling Port into this crate.
pub trait ReferenceTokenResolver {
    fn reference_token(&self, media_kind: &str, ordinal_within_kind: u32) -> Option<String>;
}

fn expression_language_wire(language: &crate::projection::VideoPromptExpressionLanguage) -> &'static str {
    use crate::projection::VideoPromptExpressionLanguage;
    match language {
        VideoPromptExpressionLanguage::Zh => "zh",
        VideoPromptExpressionLanguage::En => "en",
    }
}

fn neutral_role_label(language: &str, ordinal: usize) -> String {
    if language.starts_with("zh") {
        format!("角色{ordinal}")
    } else {
        format!("Character {ordinal}")
    }
}

/// Compile frozen identity lock lines for one projection target.
pub fn compile_identity_lock_lines(
    view: &LockCompileView<'_>,
    tokens: &dyn ReferenceTokenResolver,
) -> Result<Vec<IdentityLockLine>, String> {
    let language = expression_language_wire(&view.target.expression_language);
    let tagged = lock_template_for_language(language)?;
    let neutral = neutral_lock_template_for_language(language)?;
    let mut lines = Vec::new();
    for binding in view.bindings {
        let subject = view
            .subjects
            .iter()
            .find(|s| s.subject_id == binding.subject_id && s.shot_id == binding.shot_id)
            .ok_or_else(|| "人物与参考图的关联已失效，请重新关联".to_string())?;
        let use_row = view
            .uses
            .iter()
            .find(|u| u.use_id == binding.reference_use_id)
            .ok_or_else(|| "人物与参考图的关联已失效，请重新关联".to_string())?;
        let item = view
            .items
            .iter()
            .find(|i| i.item_id == use_row.item_id)
            .ok_or_else(|| "人物与参考图的关联已失效，请重新关联".to_string())?;
        if !view.target.reference_use_ids.contains(&item.item_id) {
            continue;
        }
        let neutral_label;
        let name = if subject.display_name.trim().is_empty() {
            neutral_label = neutral_role_label(language, lines.len() + 1);
            neutral_label.as_str()
        } else {
            subject.display_name.as_str()
        };
        let (text, token) = match tokens.reference_token(&item.media_kind, item.ordinal_within_kind) {
            Some(token) => (render_lock_template(&tagged, name, &token), token),
            None => (render_lock_template(&neutral, name, ""), String::new()),
        };
        lines.push(IdentityLockLine {
            shot_id: binding.shot_id.clone(),
            subject_id: binding.subject_id.clone(),
            binding_id: binding.binding_id.clone(),
            text,
            token,
        });
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projection::VideoPromptExpressionLanguage;

    #[test]
    fn identity_lock_line_fields() {
        let line = IdentityLockLine {
            shot_id: "S1".into(),
            subject_id: "sub1".into(),
            binding_id: "b1".into(),
            text: "Alice is @Image1".into(),
            token: "@Image1".into(),
        };
        assert_eq!(line.binding_id, "b1");
        assert!(!line.token.is_empty());
    }

    #[test]
    fn parse_and_render_zh_template() {
        let parts = parse_lock_template(IDENTITY_LOCK_TEMPLATE_ZH).unwrap();
        let rendered = render_lock_template(&parts, "角色", "<Picture 2>");
        assert!(rendered.contains("角色"));
        assert!(rendered.contains("<Picture 2>"));
        assert!(parse_lock_template("{name}{name}{token}").is_err());
        assert!(parse_neutral_lock_template(IDENTITY_LOCK_TEMPLATE_NEUTRAL_ZH).is_ok());
    }

    struct FixedToken(&'static str);
    impl ReferenceTokenResolver for FixedToken {
        fn reference_token(&self, _media_kind: &str, _ordinal_within_kind: u32) -> Option<String> {
            Some(self.0.to_string())
        }
    }

    #[test]
    fn compile_lock_lines_uses_resolver() {
        let target = VideoPromptTargetSnapshot {
            caps_digest: "a".repeat(64),
            duration_seconds: None,
            expression_language: VideoPromptExpressionLanguage::Zh,
            input_mode: "text".into(),
            profile_id: "p".into(),
            profile_version: 1,
            reference_use_ids: vec!["item1".into()],
        };
        let items = [FrozenInventoryItem {
            item_id: "item1".into(),
            media_kind: "reference_image".into(),
            ordinal_within_kind: 1,
            declared: None,
            label: "Alice".into(),
            audio_usage: None,
        }];
        let subjects = [ShotSubjectV1 {
            shot_id: "S1".into(),
            subject_id: "sub1".into(),
            display_name: "Alice".into(),
            entity_kind: "role".into(),
            entity_id: "r1".into(),
        }];
        let uses = [ReferenceUseV2 {
            use_id: "u1".into(),
            item_id: "item1".into(),
            use_kind: "portrait".into(),
            subject_id: Some("sub1".into()),
            panel_id: None,
        }];
        let bindings = [IdentityReferenceBindingV2 {
            binding_id: "b1".into(),
            shot_id: "S1".into(),
            subject_id: "sub1".into(),
            reference_use_id: "u1".into(),
            source: "user".into(),
        }];
        let view = LockCompileView {
            items: &items,
            subjects: &subjects,
            uses: &uses,
            bindings: &bindings,
            target: &target,
        };
        let lines = compile_identity_lock_lines(&view, &FixedToken("@Image1")).unwrap();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].token, "@Image1");
        assert!(lines[0].text.contains("Alice"));
    }
}
