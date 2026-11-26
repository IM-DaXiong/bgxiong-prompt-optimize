//! Reference-marker templates of the video prompt target adapters (VS48, CF-21 / CF-47).
//!
//! - Single source: `src-tauri/resources/video-prompt-adapters/cite-templates.json`, one entry
//!   per projection profile (`profileId@version`); build.rs emits `CITE_TPL_<STEM>_<SLOT>` for
//!   each slot an entry declares. Kling Omni old (`kling.omni@1`, `<<<image_N>>>`) and new
//!   (`kling.omni@2`, `@image_N` / `@video_N`) interfaces are separate entries.
//! - Which template a target writes is decided only by its projector (`compile` /
//!   `reference_token` in `video_prompt_adapters`). Nothing here maps a provider kind, model id or
//!   display name to a dialect, and there is no default dialect.

include!(concat!(env!("OUT_DIR"), "/video_prompt_cite_templates.rs"));

/// Fill the `{n}` placeholder of one template.
pub fn apply_cite_template(tpl: &str, n: u32) -> String {
    tpl.replace("{n}", &n.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageCiteHit {
    pub start: usize,
    pub end: usize,
    pub ordinal: u32,
}

/// Image markers of every registered template (`CITE_IMAGE_TEMPLATES`) in `text`, longest
/// prefix first so `图片2` is not read as `图`. Not a prose guess.
pub fn find_image_cites(text: &str) -> Vec<ImageCiteHit> {
    let patterns = image_cite_patterns();
    let mut hits = Vec::new();
    let mut i = 0;
    while i < text.len() {
        if let Some((end, ordinal)) = match_image_cite_at(text, i, &patterns) {
            hits.push(ImageCiteHit {
                start: i,
                end,
                ordinal,
            });
            i = end;
            continue;
        }
        let step = text[i..]
            .chars()
            .next()
            .map(|ch| ch.len_utf8())
            .unwrap_or(1);
        i += step;
    }
    hits
}

/// `(prefix, suffix)` around `{n}` of each image template; build.rs guarantees exactly one
/// placeholder and a non-empty prefix.
fn image_cite_patterns() -> Vec<(&'static str, &'static str)> {
    let mut patterns: Vec<(&'static str, &'static str)> = CITE_IMAGE_TEMPLATES
        .iter()
        .filter_map(|tpl| tpl.split_once("{n}"))
        .collect();
    patterns.sort_by(|a, b| {
        b.0.len()
            .cmp(&a.0.len())
            .then_with(|| b.1.len().cmp(&a.1.len()))
    });
    patterns
}

fn match_image_cite_at(
    text: &str,
    i: usize,
    patterns: &[(&'static str, &'static str)],
) -> Option<(usize, u32)> {
    let rest = &text[i..];
    patterns.iter().find_map(|(prefix, suffix)| {
        take_digits_between(rest, prefix, suffix)
            .map(|(n, width)| (i + prefix.len() + width + suffix.len(), n))
    })
}

/// Ordinal (> 0) and digit byte width right after `prefix`, followed by `suffix`.
fn take_digits_between(rest: &str, prefix: &str, suffix: &str) -> Option<(u32, usize)> {
    let after = rest.strip_prefix(prefix)?;
    let width = after
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .map(|ch| ch.len_utf8())
        .sum::<usize>();
    if width == 0 {
        return None;
    }
    let n: u32 = after[..width].parse().ok()?;
    if n == 0 {
        return None;
    }
    if !after[width..].starts_with(suffix) {
        return None;
    }
    Some((n, width))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_cites_keep_ordinals_across_dialects() {
        let hits = find_image_cites("图片2 <Picture 3> 图1 <<<image_4>>> @image_5");
        let ordinals: Vec<u32> = hits.iter().map(|hit| hit.ordinal).collect();
        assert_eq!(ordinals, vec![2, 3, 1, 4, 5]);
    }

    #[test]
    fn image_cites_cover_every_registered_image_template() {
        for tpl in CITE_IMAGE_TEMPLATES {
            let token = apply_cite_template(tpl, 7);
            let hits = find_image_cites(&format!("x {token} y"));
            assert_eq!(hits.len(), 1, "{tpl}");
            assert_eq!(hits[0].ordinal, 7, "{tpl}");
            assert_eq!(&format!("x {token} y")[hits[0].start..hits[0].end], token);
        }
    }

    #[test]
    fn image_cites_need_a_complete_marker() {
        assert!(find_image_cites("<<<image_2 <Picture 0> @image_ 图片").is_empty());
    }

    /// CF-20 / CF-47: kling.omni@2 has its own entry; it is not the classic template by name.
    #[test]
    fn kling_omni_versions_have_their_own_templates() {
        assert_eq!(apply_cite_template(CITE_TPL_KLING_OMNI_V2_IMAGE, 1), "@image_1");
        assert_eq!(apply_cite_template(CITE_TPL_KLING_OMNI_V2_VIDEO, 2), "@video_2");
        assert_eq!(apply_cite_template(CITE_TPL_KLING_OMNI_IMAGE, 1), "<<<image_1>>>");
        assert_eq!(apply_cite_template(CITE_TPL_KLING_OMNI_VIDEO, 1), "<<<video_1>>>");
    }
}
