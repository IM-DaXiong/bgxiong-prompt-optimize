//! Generate vision caps, cite templates, H3 audio-usage, and H3 CIR field keys from contract JSON.
//! Do not hard-code maxIpcRels or cite dialects in library source.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let repo = manifest_dir.join("../../..");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));

    let vision = repo.join("packages/app-contracts/v1/prompt-optimize-vision-limits.json");
    println!("cargo:rerun-if-changed={}", vision.display());
    let raw = fs::read_to_string(&vision)
        .unwrap_or_else(|err| panic!("read {}: {err}", vision.display()));
    let max = parse_u32_field(&raw, "maxIpcRels")
        .unwrap_or_else(|err| panic!("prompt-optimize-vision-limits.json: {err}"));
    fs::write(
        out_dir.join("prompt_optimize_vision_limits.rs"),
        format!(
            "/// Generated from packages/app-contracts/v1/prompt-optimize-vision-limits.json `maxIpcRels`.\npub const PROMPT_OPT_VISION_IPC_MAX_RELS: u32 = {max};\npub const MAX_IPC_RELS: u32 = PROMPT_OPT_VISION_IPC_MAX_RELS;\n"
        ),
    )
    .expect("write vision limits");

    let cites = repo.join("src-tauri/resources/video-prompt-adapters/cite-templates.json");
    println!("cargo:rerun-if-changed={}", cites.display());
    let cite_raw = fs::read_to_string(&cites)
        .unwrap_or_else(|err| panic!("read {}: {err}", cites.display()));
    fs::write(
        out_dir.join("video_prompt_cite_templates.rs"),
        generate_cite_templates(&cite_raw),
    )
    .expect("write cite templates");

    let h3 = repo.join("packages/app-contracts/v1/h3-reference-audio-usage.json");
    println!("cargo:rerun-if-changed={}", h3.display());
    let h3_raw = fs::read_to_string(&h3)
        .unwrap_or_else(|err| panic!("read {}: {err}", h3.display()));
    fs::write(
        out_dir.join("h3_reference_audio_usage.rs"),
        generate_h3_usage(&h3_raw),
    )
    .expect("write h3 audio usage");

    emit_minimax_h3_cir_field_keys(&repo, &out_dir);
}

fn parse_u32_field(raw: &str, key: &str) -> Result<u32, String> {
    let needle = format!("\"{key}\"");
    let idx = raw.find(&needle).ok_or_else(|| format!("missing {key}"))?;
    let after = raw[idx + needle.len()..].trim_start();
    let after = after
        .strip_prefix(':')
        .ok_or_else(|| format!("{key} is not a field"))?
        .trim_start();
    let digits: String = after.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    if digits.is_empty() {
        return Err(format!("{key} is not a number"));
    }
    digits
        .parse::<u32>()
        .map_err(|err| format!("{key} parse: {err}"))
}

fn json_string_value(raw: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let idx = raw.find(&needle)?;
    json_string_after_colon(&raw[idx + needle.len()..])
}

fn generate_cite_templates(raw: &str) -> String {
    let mut constants = Vec::new();
    let mut image_templates = Vec::new();
    for chunk in raw.split("\"rustConst\"").skip(1) {
        let rust_const = match json_string_after_colon(chunk) {
            Some(value) if !value.is_empty() => value,
            _ => continue,
        };
        if let Some(image) = named_template(chunk, "image") {
            let name = format!("CITE_TPL_{rust_const}_IMAGE");
            constants.push(format!(
                "pub const {name}: &str = \"{}\";",
                rust_escape(&image)
            ));
            image_templates.push(name);
        }
        if let Some(video) = named_template(chunk, "video") {
            constants.push(format!(
                "pub const CITE_TPL_{rust_const}_VIDEO: &str = \"{}\";",
                rust_escape(&video)
            ));
        }
        if let Some(audio) = named_template(chunk, "audio") {
            constants.push(format!(
                "pub const CITE_TPL_{rust_const}_AUDIO: &str = \"{}\";",
                rust_escape(&audio)
            ));
        }
    }
    let mut src = String::from(
        "/* generated from src-tauri/resources/video-prompt-adapters/cite-templates.json */\n",
    );
    for line in &constants {
        src.push_str(line);
        src.push('\n');
    }
    src.push_str("pub const CITE_IMAGE_TEMPLATES: &[&str] = &[\n");
    for name in &image_templates {
        src.push_str(&format!("    {name},\n"));
    }
    src.push_str("];\n");
    src
}

fn json_string_after_colon(chunk: &str) -> Option<String> {
    let after = chunk.trim_start().strip_prefix(':')?.trim_start();
    if !after.starts_with('"') {
        return None;
    }
    let mut out = String::new();
    let mut chars = after[1..].chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
            continue;
        }
        if ch == '"' {
            break;
        }
        out.push(ch);
    }
    Some(out)
}

fn named_template(chunk: &str, slot: &str) -> Option<String> {
    let needle = format!("\"{slot}\"");
    let idx = chunk.find(&needle)?;
    json_string_after_colon(&chunk[idx + needle.len()..])
}

fn rust_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn generate_h3_usage(raw: &str) -> String {
    let default = json_string_value(raw, "default").unwrap_or_else(|| "voice_timbre".into());
    let mut src = String::from(
        "/* generated from packages/app-contracts/v1/h3-reference-audio-usage.json */\n",
    );
    src.push_str("pub const H3_REFERENCE_AUDIO_USAGE_VOICE_TIMBRE: &str = \"voice_timbre\";\n");
    src.push_str(
        "pub const H3_REFERENCE_AUDIO_USAGE_SOUNDTRACK_COPY: &str = \"soundtrack_copy\";\n",
    );
    src.push_str(&format!(
        "pub const H3_REFERENCE_AUDIO_USAGE_DEFAULT: &str = \"{}\";\n",
        rust_escape(&default)
    ));
    src
}

/// Port of `src-tauri/build.rs` `emit_minimax_h3_cir_field_keys`.
/// Writes into *this crate's* OUT_DIR so `projectors/h3/compile.rs` does not
/// read the host package's generated file.
fn emit_minimax_h3_cir_field_keys(repo: &PathBuf, out_dir: &PathBuf) {
    let path = repo.join("packages/app-contracts/v1/minimax-h3-cir-field-keys.json");
    println!("cargo:rerun-if-changed={}", path.display());
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
    require_schema_version_1(&raw, "minimax-h3-cir-field-keys.json");
    let base = require_json_str_array(&raw, "baseKeys");
    let refs = require_json_str_array(&raw, "refKeys");
    if base.len() != 3 {
        panic!("minimax-h3-cir-field-keys.json baseKeys must have exactly 3 entries");
    }
    if refs.len() != 6 {
        panic!("minimax-h3-cir-field-keys.json refKeys must have exactly 6 entries");
    }
    for (list, keys) in [("baseKeys", &base), ("refKeys", &refs)] {
        let mut seen = std::collections::BTreeSet::new();
        for k in keys.iter() {
            if k.is_empty() || !k.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
                panic!("minimax-h3-cir-field-keys.json {list} entry {k:?} must be a non-empty snake_case key");
            }
            if !seen.insert(k.as_str()) {
                panic!("minimax-h3-cir-field-keys.json {list} repeats {k:?}");
            }
        }
    }
    // Both layouts end with the same soundscape and music fields (one H3 section each).
    if refs[4..] != base[1..] {
        panic!("minimax-h3-cir-field-keys.json refKeys must end with baseKeys[1..] (soundscape, music)");
    }
    let body = format!(
        "pub const CIR_BASE_KEYS: &[&str] = &{base};\npub const CIR_REF_KEYS: &[&str] = &{refs};\n",
        base = rust_str_slice(&base),
        refs = rust_str_slice(&refs),
    );
    fs::write(out_dir.join("app_contracts_minimax_h3_cir_field_keys.rs"), body)
        .expect("write h3 cir field keys");
}

fn require_schema_version_1(raw: &str, name: &str) {
    let ver = parse_u32_field(raw, "schemaVersion")
        .unwrap_or_else(|err| panic!("{name} missing schemaVersion: {err}"));
    if ver != 1 {
        panic!("{name} schemaVersion must be 1 (got {ver})");
    }
}

fn require_json_str_array(raw: &str, key: &str) -> Vec<String> {
    let needle = format!("\"{key}\"");
    let idx = raw
        .find(&needle)
        .unwrap_or_else(|| panic!("app-contract missing array field {key}"));
    let after = raw[idx + needle.len()..].trim_start();
    let after = after
        .strip_prefix(':')
        .unwrap_or_else(|| panic!("{key} is not a field"))
        .trim_start();
    let after = after
        .strip_prefix('[')
        .unwrap_or_else(|| panic!("{key} must be an array"));
    let end = after
        .find(']')
        .unwrap_or_else(|| panic!("{key} array is not closed"));
    let inner = &after[..end];
    let mut items = Vec::new();
    let mut rest = inner;
    loop {
        rest = rest.trim_start().trim_start_matches(',').trim_start();
        if rest.is_empty() {
            break;
        }
        let value = json_string_after_colon(&format!(": {rest}"))
            .unwrap_or_else(|| panic!("{key} entries must be strings"));
        if value.is_empty()
            || value
                .chars()
                .any(|c| !c.is_ascii() || c.is_ascii_control() || c.is_ascii_whitespace())
        {
            panic!("{key} entries must be printable ASCII without whitespace");
        }
        items.push(value.clone());
        let token = format!("\"{value}\"");
        let pos = rest
            .find(&token)
            .unwrap_or_else(|| panic!("{key} failed to consume entry"));
        rest = &rest[pos + token.len()..];
    }
    if items.is_empty() {
        panic!("{key} must be non-empty");
    }
    items
}

fn rust_str_slice(items: &[String]) -> String {
    let inner = items
        .iter()
        .map(|s| format!("\"{}\"", rust_escape(s)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{inner}]")
}
