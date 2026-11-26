//! T28d vendor-adapter text. `resolve` and `complete` take an explicit package root;
//! credentials come only from the package host-config. No story.db, no tauri, no wasmtime.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::openai::shared::{
    TextServiceConfig, TextTransportContent, TextTransportMessage, ThinkingRequest,
    VENDOR_ADAPTER_MODEL_ID_PREFIX,
};
use crate::openai::ChatCompletionOutput;
use crate::types::{AdapterCompleteOptions, PackageFileReader};

const MODEL_PAYLOAD_PREFIX: &str = "sdk-v2.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Selection {
    pub plugin_id: String,
    pub version: String,
    pub model_id: String,
    pub capability_key: String,
    pub operation_id: String,
    pub operation_version: u64,
    pub input_schema_digest: String,
}

#[derive(Debug, Clone)]
pub struct AdapterPackage {
    pub plugin_id: String,
    pub version: String,
    pub root: PathBuf,
    pub dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct AdapterCapabilityEntry {
    pub plugin_id: String,
    pub version: String,
    pub capability_key: String,
    pub model_ids: Vec<String>,
    pub operation_id: String,
    pub operation_version: u64,
    pub input_schema_digest: String,
    pub input_schema: Value,
    pub media_slots: Value,
    pub output_slots: Value,
    pub execution: Value,
}

/// Host-supplied synchronous runner. Process execution stays in the binary
/// (`process::run_synchronous` / `spawn_blocking`).
pub trait AdapterTextRunner {
    fn run_synchronous(
        &self,
        package: &AdapterPackage,
        request_id: &str,
        payload: &Value,
    ) -> Result<Value, String>;
}

pub fn parse_adapter_model_id(model_id: &str) -> Result<(String, String), String> {
    let raw = model_id.trim();
    let prefix = VENDOR_ADAPTER_MODEL_ID_PREFIX;
    if !raw.starts_with(prefix) {
        return Err(format!("model 须以 {prefix} 开头"));
    }
    let rest = &raw[prefix.len()..];
    let sep = rest
        .find(':')
        .ok_or_else(|| "model 缺少 pluginId/vendorModelId 分隔".to_string())?;
    if sep == 0 || sep + 1 >= rest.len() {
        return Err("adapter modelId 分段为空".to_string());
    }
    Ok((rest[..sep].to_string(), rest[sep + 1..].to_string()))
}

pub fn is_model(model: &str) -> bool {
    parse_adapter_model_id(model).is_ok_and(|(_, vendor)| vendor.starts_with(MODEL_PAYLOAD_PREFIX))
}

fn selection(model: &str) -> Result<Selection, String> {
    let (plugin, vendor) = parse_adapter_model_id(model)?;
    let encoded = vendor
        .strip_prefix(MODEL_PAYLOAD_PREFIX)
        .ok_or("Not an SDK v2 text selection")?;
    let bytes =
        decode_base64url_nopad(encoded).map_err(|_| "Invalid SDK v2 text selection".to_string())?;
    let selected: Selection =
        serde_json::from_slice(&bytes).map_err(|e| format!("Invalid SDK v2 text binding: {e}"))?;
    if selected.plugin_id != plugin || selected.capability_key != "text.generate" {
        return Err("SDK v2 text selection identity mismatch".into());
    }
    Ok(selected)
}

fn decode_base64url_nopad(input: &str) -> Result<Vec<u8>, ()> {
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'-' => Some(62),
            b'_' => Some(63),
            _ => None,
        }
    }
    if input.bytes().any(|b| b == b'+' || b == b'/' || b == b'=') {
        return Err(());
    }
    let pad = match input.len() % 4 {
        0 => 0,
        2 => 2,
        3 => 1,
        _ => return Err(()),
    };
    let mut padded = input.as_bytes().to_vec();
    padded.extend(std::iter::repeat(b'A').take(pad));
    let mut out = Vec::with_capacity(padded.len() / 4 * 3);
    for chunk in padded.chunks_exact(4) {
        let v0 = val(chunk[0]).ok_or(())?;
        let v1 = val(chunk[1]).ok_or(())?;
        let v2 = val(chunk[2]).ok_or(())?;
        let v3 = val(chunk[3]).ok_or(())?;
        let n = ((v0 as u32) << 18) | ((v1 as u32) << 12) | ((v2 as u32) << 6) | (v3 as u32);
        out.push(((n >> 16) & 0xff) as u8);
        out.push(((n >> 8) & 0xff) as u8);
        out.push((n & 0xff) as u8);
    }
    for _ in 0..pad {
        out.pop();
    }
    Ok(out)
}

fn plain_segment(segment: &str) -> Result<(), String> {
    if segment.is_empty() || segment.contains(['/', '\\', ':']) || segment.starts_with('.') {
        return Err(format!("非法的包标识段：{segment}"));
    }
    Ok(())
}

fn read_json(reader: &dyn PackageFileReader, path: &Path) -> Result<Value, String> {
    let bytes = reader
        .read(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Resolve a selection against an explicit adapter-v2 root
/// (`<package_root>/packages/<pluginId>/<version>/`).
pub fn resolve_package(package_root: &Path, reader: &dyn PackageFileReader, model: &str) -> Result<(AdapterPackage, AdapterCapabilityEntry, Selection), String> {
    if package_root.as_os_str().is_empty() {
        return Err("未选择模型或找不到适配包：包根目录为空".into());
    }
    let selected = selection(model)?;
    plain_segment(&selected.plugin_id)?;
    plain_segment(&selected.version)?;
    let dir = package_root
        .join("packages")
        .join(&selected.plugin_id)
        .join(&selected.version);
    if !reader.is_file(&dir.join("package").join("manifest.json")) {
        return Err(format!(
            "未安装适配包 {}@{}",
            selected.plugin_id, selected.version
        ));
    }
    let package = AdapterPackage {
        plugin_id: selected.plugin_id.clone(),
        version: selected.version.clone(),
        root: dir.join("package"),
        dir: dir.clone(),
    };
    let entry = read_catalog(reader, &package)?
        .into_iter()
        .find(|e| {
            supported(e)
                && e.operation_id == selected.operation_id
                && e.operation_version == selected.operation_version
                && e.input_schema_digest == selected.input_schema_digest
                && e.model_ids.contains(&selected.model_id)
        })
        .ok_or("所选SDK v2文本操作或schema已不可用，请重新安装并选择模型")?;
    if transport_slot(reader, &package, &entry.capability_key)? != "text" {
        return Err("SDK v2 operation is not a text transport".into());
    }
    Ok((package, entry, selected))
}

fn supported(entry: &AdapterCapabilityEntry) -> bool {
    entry.capability_key == "text.generate"
        && entry.execution["mode"] == "synchronous"
        && entry.execution["streaming"] == false
        && entry.execution["resumable"] == false
        && entry.media_slots == json!([])
        && entry.output_slots
            == json!([{"role":"answer","kind":"text","minCount":1,"maxCount":1}])
        && entry.input_schema["properties"]
            .as_object()
            .is_some_and(|p| {
                p.contains_key("messages")
                    && p.contains_key("parameters")
                    && p.keys()
                        .all(|k| ["messages", "parameters"].contains(&k.as_str()))
            })
        && entry.input_schema["properties"]["parameters"]["properties"]
            .as_object()
            .is_some_and(|p| {
                p.keys()
                    .all(|k| ["temperature", "maxTokens", "thinking"].contains(&k.as_str()))
            })
}

pub fn read_catalog(reader: &dyn PackageFileReader, package: &AdapterPackage) -> Result<Vec<AdapterCapabilityEntry>, String> {
    let data = read_json(reader, &package.dir.join("catalog.json"))?;
    if data["pluginId"] != package.plugin_id.as_str()
        || data["pluginVersion"] != package.version.as_str()
    {
        return Err("capability catalog package identity differs from the install record".into());
    }
    let contracts = data["capabilityContracts"]
        .as_array()
        .ok_or("capability catalog lacks capabilityContracts")?;
    let mut out = Vec::new();
    for c in contracts {
        let schemas = c["schemas"]
            .as_array()
            .ok_or("capability contract lacks its schema records")?;
        for op in c["operations"].as_array().into_iter().flatten() {
            let reference = &op["inputSchema"];
            let record = schemas
                .iter()
                .find(|s| {
                    s["id"].is_string()
                        && s["id"] == reference["id"]
                        && s["digest"].is_string()
                        && s["digest"] == reference["digest"]
                })
                .ok_or("operation input schema reference is unresolved")?;
            let schema_json = record["schemaJson"]
                .as_str()
                .ok_or("input schema record lacks schemaJson")?;
            let input_schema = serde_json::from_str(schema_json)
                .map_err(|e| format!("input schema JSON is invalid: {e}"))?;
            out.push(AdapterCapabilityEntry {
                plugin_id: package.plugin_id.clone(),
                version: package.version.clone(),
                capability_key: c["capabilityKey"].as_str().unwrap_or_default().into(),
                model_ids: c["modelIds"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|m| m.as_str().map(str::to_owned))
                    .collect(),
                operation_id: op["id"].as_str().unwrap_or_default().into(),
                operation_version: op["version"].as_u64().unwrap_or(0),
                input_schema_digest: op["inputSchema"]["digest"]
                    .as_str()
                    .unwrap_or_default()
                    .into(),
                input_schema,
                media_slots: op["mediaSlots"].clone(),
                output_slots: op["outputSlots"].clone(),
                execution: op["execution"].clone(),
            });
        }
    }
    Ok(out)
}

pub fn transport_slot(reader: &dyn PackageFileReader, package: &AdapterPackage, capability_key: &str) -> Result<String, String> {
    let data = read_json(reader, &package.dir.join("catalog.json"))?;
    data["capabilityContracts"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|c| c["capabilityKey"] == capability_key)
        .and_then(|c| c["transportSlot"].as_str().map(str::to_owned))
        .ok_or_else(|| format!("能力目录中没有 {capability_key}"))
}

pub fn explicit_host_config(reader: &dyn PackageFileReader, package: &AdapterPackage) -> Result<Value, String> {
    if reader.is_file(&package.dir.join("managed-host.json")) {
        return Err(
            "This text operation requires explicit package connection configuration".into(),
        );
    }
    read_json(reader, &package.dir.join("host-config.json"))
}

pub fn operation_input(
    messages: &[TextTransportMessage],
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    thinking: Option<ThinkingRequest>,
    response_format_json: bool,
    has_output_schema: bool,
) -> Result<Value, String> {
    if response_format_json || has_output_schema {
        return Err(
            "此SDK v2文本操作尚未声明JSON结构化输出，请选择已支持该能力的文本操作".into(),
        );
    }
    let mut wire_messages = Vec::new();
    for message in messages {
        let TextTransportContent::Text(content) = &message.content else {
            return Err("此SDK v2文本操作只支持普通文本消息，不能提交图片或视频附件".into());
        };
        let mut value = json!({"role":message.role,"content":content});
        if let Some(reasoning) = &message.reasoning_content {
            value["reasoningContent"] = json!(reasoning);
        }
        wire_messages.push(value);
    }
    let mut parameters = serde_json::Map::new();
    if let Some(value) = temperature {
        parameters.insert("temperature".into(), json!(value));
    }
    if let Some(value) = max_tokens {
        parameters.insert("maxTokens".into(), json!(value));
    }
    if let Some(value) = thinking {
        parameters.insert("thinking".into(), json!(value.enabled));
    }
    Ok(json!({"messages":wire_messages,"parameters":parameters}))
}

pub fn parse_sdk_text_result(response: &Value) -> Result<ChatCompletionOutput, String> {
    if let Some(error) = response.get("error") {
        return Err(format!(
            "{}: {}",
            error["code"].as_str().unwrap_or("SDK_ERROR"),
            error["message"]
                .as_str()
                .unwrap_or("SDK text request failed")
        ));
    }
    let data: Value =
        serde_json::from_str(response["dataJson"].as_str().ok_or("SDK文本没有结果JSON")?)
            .map_err(|e| e.to_string())?;
    let outputs = data["outputs"]
        .as_array()
        .filter(|a| a.len() == 1)
        .ok_or("SDK文本须返回一项answer")?;
    if data["kind"] != "completed" || outputs[0]["kind"] != "text" || outputs[0]["role"] != "answer"
    {
        return Err("SDK文本未返回同步answer文本".into());
    }
    let content = outputs[0]["text"]
        .as_str()
        .ok_or("SDK文本answer不是字符串")?
        .to_owned();
    let optional = |key: &str| -> Result<Option<String>, String> {
        match data["data"].get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.clone())),
            _ => Err(format!("SDK文本结果{key}类型不受支持")),
        }
    };
    Ok(ChatCompletionOutput {
        content,
        finish_reason: optional("finishReason")?,
        reasoning_content: optional("reasoningContent")?,
        raw: json!({"sdkV2Usage":data["usage"],"sdkV2Result":data}),
    })
}

fn origin_key(url: &str) -> Result<String, String> {
    let url = url.trim();
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| format!("invalid URL: {url}"))?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(format!("unsupported URL scheme: {scheme}"));
    }
    let hostport = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    if hostport.is_empty() {
        return Err("URL host is empty".into());
    }
    Ok(format!("{scheme}://{hostport}"))
}

fn next_call_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(1);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("text.{millis:x}-{n}")
}

/// Complete text via an installed SDK v2 package under `package_root`.
/// Does not fall back to a cloud OpenAI-compatible endpoint.
pub fn complete(
    package_root: &Path,
    reader: &dyn PackageFileReader,
    service: &TextServiceConfig,
    messages: &[TextTransportMessage],
    options: AdapterCompleteOptions<'_>,
    runner: &dyn AdapterTextRunner,
) -> Result<ChatCompletionOutput, String> {
    if package_root.as_os_str().is_empty() {
        return Err("未选择模型或找不到适配包：包根目录为空".into());
    }
    let model = service
        .model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("SDK文本未选择模型")?;
    let (package, _entry, selected) = resolve_package(package_root, reader, model)?;
    let host = explicit_host_config(reader, &package)?;
    if service
        .api_key
        .as_deref()
        .is_some_and(|s| !s.is_empty())
    {
        return Err("此SDK模型使用适配包显式凭证配置，请勿同时指定另一账号的API Key".into());
    }
    if let Some(base) = service.base_url.as_deref().filter(|s| !s.is_empty()) {
        let origin = origin_key(base)?;
        let allowed = host["network"]["origins"]
            .as_array()
            .filter(|a| a.len() == 1)
            .and_then(|a| a[0].as_str())
            .map(origin_key)
            .transpose()?;
        if allowed.as_ref() != Some(&origin) {
            return Err("SDK适配包地址与所选文本连接不一致".into());
        }
    }
    let value = operation_input(
        messages,
        options.temperature,
        options.max_tokens,
        options.thinking,
        options.response_format_json,
        options.has_output_schema,
    )?;
    let call = next_call_id();
    let payload = json!({
        "schemaVersion": 2,
        "modelId": selected.model_id,
        "capabilityKey": selected.capability_key,
        "operationId": selected.operation_id,
        "operationVersion": selected.operation_version,
        "inputSchemaDigest": selected.input_schema_digest,
        "input": value,
        "configuration": host["configuration"],
        "media": [],
        "uploads": []
    });
    if let Some(audit) = options.on_audit {
        let sent = serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())?;
        audit(&sent);
    }
    let response = runner.run_synchronous(&package, &call, &payload)?;
    parse_sdk_text_result(&response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AdapterCompleteOptions, MemoryPackageFileReader};

    fn empty_reader() -> MemoryPackageFileReader {
        MemoryPackageFileReader::new()
    }

    #[test]
    fn empty_package_root_is_missing_package_error() {
        let reader = empty_reader();
        let err = resolve_package(Path::new(""), &reader, "adapter:demo:sdk-v2.abc").unwrap_err();
        assert!(err.contains("未选择模型") || err.contains("找不到") || err.contains("包根"), "{err}");
        let err = complete(
            Path::new(""),
            &reader,
            &TextServiceConfig {
                model: Some("adapter:demo:sdk-v2.abc".into()),
                ..Default::default()
            },
            &[],
            AdapterCompleteOptions::default(),
            &RejectRunner,
        )
        .unwrap_err();
        assert!(err.contains("未选择模型") || err.contains("找不到") || err.contains("包根"), "{err}");
    }

    #[test]
    fn missing_model_is_not_a_cloud_fallback() {
        let err = complete(
            Path::new("/tmp/adapter-v2-missing"),
            &empty_reader(),
            &TextServiceConfig::default(),
            &[],
            AdapterCompleteOptions::default(),
            &RejectRunner,
        )
        .unwrap_err();
        assert_eq!(err, "SDK文本未选择模型");
    }

    #[test]
    fn missing_package_directory_does_not_use_cloud() {
        let err = complete(
            Path::new("/tmp/definitely-not-an-adapter-root"),
            &empty_reader(),
            &TextServiceConfig {
                model: Some("not-an-adapter-model".into()),
                ..Default::default()
            },
            &[],
            AdapterCompleteOptions::default(),
            &RejectRunner,
        )
        .unwrap_err();
        assert!(err.contains("须以 adapter:") || err.contains("未安装适配包") || err.contains("Not an SDK"), "{err}");
        assert!(!err.to_ascii_lowercase().contains("openai"));
        assert!(!err.contains("chat/completions"));
    }

    #[test]
    fn text_result_preserves_reasoning_and_rejects_async() {
        let value = json!({
            "kind":"completed",
            "outputs":[{"kind":"text","role":"answer","text":"原文\n🐻"}],
            "data":{"finishReason":"length","reasoningContent":"思考"},
            "usage":{"measurements":[{"metric":"output_tokens","unit":"tokens","amount":"3","source":"reported"}]}
        });
        let output = parse_sdk_text_result(&json!({"dataJson":value.to_string()})).unwrap();
        assert_eq!(output.content, "原文\n🐻");
        assert_eq!(output.finish_reason.as_deref(), Some("length"));
        assert_eq!(output.reasoning_content.as_deref(), Some("思考"));
        assert_eq!(output.raw["sdkV2Usage"], value["usage"]);
        assert!(parse_sdk_text_result(&json!({"dataJson":"{\"kind\":\"accepted\"}"})).is_err());
        let mut bad = value;
        bad["outputs"][0]["kind"] = json!("media");
        assert!(parse_sdk_text_result(&json!({"dataJson":bad.to_string()})).is_err());
    }

    #[test]
    fn operation_input_rejects_media() {
        let err = operation_input(
            &[TextTransportMessage {
                role: "user".into(),
                content: TextTransportContent::Parts(vec![
                    crate::openai::shared::LlmContentPart::Text {
                        text: "look".into(),
                    },
                ]),
                reasoning_content: None,
            }],
            None,
            None,
            None,
            false,
            false,
        )
        .unwrap_err();
        assert!(err.contains("不能提交图片或视频附件"), "{err}");
    }

    struct RejectRunner;
    impl AdapterTextRunner for RejectRunner {
        fn run_synchronous(
            &self,
            _package: &AdapterPackage,
            _request_id: &str,
            _payload: &Value,
        ) -> Result<Value, String> {
            Err("runner should not be called".into())
        }
    }
}
