//! Host adapter: `OutboundError` -> `__BGX_ERR_JSON__` (INV-AITASK-ERR). Not inside outbound_http.
//! This crate copy omits the host `DbState` retry observer / task-local cancel attachment.

use super::http_transport::decorate_network_transient_wire;
use super::outbound_http::{
    clients, InterruptClass, OutboundError, SendOpts,
};

fn is_local_engine_provider(provider: &str) -> bool {
    matches!(
        provider.trim().to_ascii_lowercase().as_str(),
        "indextts" | "voxcpm" | "tts" | "music" | "sfx" | "llama" | "comfyui" | "sidecar"
    )
}

fn local_engine_interrupt_category(class: InterruptClass) -> (&'static str, bool) {
    match class {
        InterruptClass::Connect => ("local_engine_unreachable", true),
        InterruptClass::InFlightCut => ("local_engine_timeout", false),
    }
}

/// 用户可读的本机引擎名（userHint 用）。
fn local_engine_display_name(provider: &str) -> &'static str {
    match provider.trim().to_ascii_lowercase().as_str() {
        "comfyui" => "ComfyUI",
        "indextts" => "IndexTTS",
        "voxcpm" => "VoxCPM",
        "llama" => "本地文本模型服务",
        "tts" => "本地语音服务",
        "music" => "本地音乐服务",
        "sfx" => "本地音效服务",
        _ => "本地引擎",
    }
}

/// `local_engine_*` 一律带中文 userHint；`address` 是本次请求的 host:port（来自调用方配置的地址）。
pub fn local_engine_user_hint(provider: &str, category: &str, address: Option<&str>) -> String {
    let engine = local_engine_display_name(provider);
    let at = address
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .map(|a| format!("（{a}）"))
        .unwrap_or_default();
    match category {
        "local_engine_timeout" => {
            format!("{engine} 响应超时{at}，请确认 {engine} 仍在运行后重试。")
        }
        _ => format!("{engine} 未启动或无法连接{at}，请先启动 {engine}。"),
    }
}

/// 请求目标的 `host:port`（用于 userHint；只在失败路径调用）。
fn request_address<F>(client: &reqwest::Client, build: &F) -> Option<String>
where
    F: Fn(&reqwest::Client) -> reqwest::RequestBuilder,
{
    let req = build(client).build().ok()?;
    let url = req.url();
    let host = url.host_str()?;
    Some(match url.port_or_known_default() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    })
}

fn local_engine_wire(
    api_type: &str,
    provider: &str,
    raw: String,
    category: &str,
    retryable: bool,
    attempts: Option<u32>,
    address: Option<&str>,
) -> String {
    let hint = local_engine_user_hint(provider, category, address);
    decorate_network_transient_wire(
        api_type, provider, raw, category, &hint, retryable, attempts,
    )
}

/// `address`：本机引擎请求的 host:port（有则写进 userHint）；云端 provider 忽略。
pub fn map_outbound_error(
    api_type: &str,
    provider: &str,
    e: OutboundError,
    address: Option<&str>,
) -> String {
    let local = is_local_engine_provider(provider);
    match e {
        OutboundError::Cancelled => "已取消".into(),
        OutboundError::Transport { source } => {
            if local {
                local_engine_wire(
                    api_type,
                    provider,
                    source,
                    "local_engine_unreachable",
                    true,
                    None,
                    address,
                )
            } else {
                decorate_network_transient_wire(
                    api_type,
                    provider,
                    source,
                    "network_ambiguous",
                    "",
                    false,
                    None,
                )
            }
        }
        OutboundError::Interrupted { class, source, .. } => {
            if local {
                let (cat, retryable) = local_engine_interrupt_category(class);
                local_engine_wire(api_type, provider, source, cat, retryable, None, address)
            } else {
                let (cat, retryable) = match class {
                    InterruptClass::Connect | InterruptClass::InFlightCut => {
                        ("network_transient", true)
                    }
                };
                decorate_network_transient_wire(
                    api_type, provider, source, cat, "", retryable, None,
                )
            }
        }
        OutboundError::WindowExhausted {
            class,
            attempts,
            elapsed_ms,
        } => {
            let raw = format!("window exhausted class={class:?} elapsed_ms={elapsed_ms}");
            if local {
                let (cat, _) = local_engine_interrupt_category(class);
                local_engine_wire(api_type, provider, raw, cat, false, Some(attempts), address)
            } else {
                decorate_network_transient_wire(
                    api_type,
                    provider,
                    raw,
                    "network_transient",
                    "",
                    false,
                    Some(attempts),
                )
            }
        }
    }
}

pub fn is_window_exhausted_message(msg: &str) -> bool {
    msg.contains("window exhausted")
        || msg.contains("WindowExhausted")
        || (msg.contains("transport_retries_exhausted=") && msg.contains("network_transient"))
}

pub async fn send_with_opts<F>(
    api_type: &str,
    provider: &str,
    opts: SendOpts<'_>,
    build: F,
) -> Result<reqwest::Response, String>
where
    F: Fn(&reqwest::Client) -> reqwest::RequestBuilder,
{
    let fingerprint = opts.client;
    match crate::openai::outbound_http::send(opts, &build).await {
        Ok(res) => Ok(res),
        Err(e) => {
            let address = if is_local_engine_provider(provider) {
                request_address(&clients::get(fingerprint), &build)
            } else {
                None
            };
            Err(map_outbound_error(
                api_type,
                provider,
                e,
                address.as_deref(),
            ))
        }
    }
}

pub async fn send_cloud_post<F>(
    api_type: &str,
    provider: &str,
    build: F,
) -> Result<reqwest::Response, String>
where
    F: Fn(&reqwest::Client) -> reqwest::RequestBuilder,
{
    send_with_opts(api_type, provider, SendOpts::cloud_post(), build).await
}
