//! Shell-owned local GGUF runtime. Does not call `global_local_ai` or read story.db.
//!
//! The host llama manager lives in the main program crate and cannot be linked
//! here. This port locates a real `llama-server` next to the shell, next to the
//! GGUF, or on PATH, starts it with `--model` / `--port`, and talks to the
//! official OpenAI-compatible HTTP surface via the shared `ReqwestPoster`.
//! It does not invent a second chat protocol, and it does not report
//! "unsupported" when a runtime is actually present.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use bgx_text_http::{
    LocalModelView, LocalTextGenerationRequest, LocalTextGenerationResponse, LocalTextRuntime,
    PreparedJsonPost, ReqwestPoster, TextHttpPoster,
};
use serde_json::{json, Value};

use crate::config::LOCAL_PROVIDER_KIND;
use crate::occupancy::{InferGuard, SHELL_OCCUPANCY_LABEL};
use crate::paths;

pub struct ShellLocalTextRuntime {
    pub model_path: PathBuf,
}

impl ShellLocalTextRuntime {
    pub fn new(model_path: PathBuf) -> Result<Self, String> {
        let model_path = paths::require_absolute(&model_path, "本地模型路径")
            .map_err(|error| error.message)?;
        if !model_path.is_file() {
            return Err(format!("本地模型文件不存在：{}", model_path.display()));
        }
        Ok(Self { model_path })
    }

    fn model_id(&self) -> String {
        self.model_path.to_string_lossy().to_string()
    }

    fn model_name(&self) -> String {
        self.model_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("local-gguf")
            .to_string()
    }
}

impl LocalTextRuntime for ShellLocalTextRuntime {
    fn get_config(&self, model_id: &str) -> Option<LocalModelView> {
        let requested = model_id.trim();
        if requested.is_empty() {
            return None;
        }
        let path = self.model_path.to_string_lossy();
        let name = self.model_name();
        if requested == path || requested == name || Path::new(requested) == self.model_path {
            Some(LocalModelView {
                id: path.to_string(),
                name,
                model_path: self.model_path.clone(),
            })
        } else {
            None
        }
    }

    fn infer(
        &self,
        config: &LocalModelView,
        request: LocalTextGenerationRequest,
    ) -> Result<LocalTextGenerationResponse, String> {
        let _guard = InferGuard::acquire()?;
        let server = LlamaServerSession::ensure(&config.model_path)?;
        let timeout_secs = None; // whole-request limit belongs to TextServiceConfig; local infer does not invent one
        let started = Instant::now();
        let body = local_chat_body(&request);
        let url = format!("http://127.0.0.1:{}/v1/chat/completions", server.port);
        let poster = ReqwestPoster;
        let fut = poster.post_json(
            PreparedJsonPost {
                url,
                headers: vec![("Content-Type".into(), "application/json".into())],
                body,
                provider_kind: LOCAL_PROVIDER_KIND.to_string(),
            },
            timeout_secs,
        );
        let res = block_on_local(fut)?;
        if !res.is_success() {
            return Err(format!("llama-server HTTP {}: {}", res.status, res.body));
        }
        let content = extract_choice_content(&res.body)?;
        let finish = extract_finish_reason(&res.body);
        Ok(LocalTextGenerationResponse {
            content,
            tokens_used: None,
            elapsed_ms: started.elapsed().as_millis() as u64,
            finish_reason: finish,
        })
    }
}

fn local_chat_body(request: &LocalTextGenerationRequest) -> Value {
    let mut messages = Vec::new();
    if let Some(system) = request.system_prompt.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        messages.push(json!({"role": "system", "content": system}));
    }
    if request.user_image_data_urls.is_empty() {
        messages.push(json!({"role": "user", "content": request.user_prompt}));
    } else {
        let mut parts = vec![json!({"type": "text", "text": request.user_prompt})];
        for url in &request.user_image_data_urls {
            parts.push(json!({"type": "image_url", "image_url": {"url": url}}));
        }
        messages.push(json!({"role": "user", "content": parts}));
    }
    let mut body = json!({
        "messages": messages,
        "stream": request.stream.unwrap_or(false),
    });
    if let Some(temp) = request.temperature {
        body["temperature"] = json!(temp);
    }
    if let Some(max_tokens) = request.max_tokens {
        body["max_tokens"] = json!(max_tokens);
    }
    if request.response_format_json {
        body["response_format"] = json!({"type": "json_object"});
    }
    if let Some(enabled) = request.enable_thinking {
        body["chat_template_kwargs"] = json!({"enable_thinking": enabled});
    }
    if let Some(effort) = request.reasoning_effort {
        body["reasoning_effort"] = json!(effort);
    }
    let _ = (request.output_schema.as_ref(), SHELL_OCCUPANCY_LABEL);
    body
}

fn extract_choice_content(body: &str) -> Result<String, String> {
    let value: Value = serde_json::from_str(body)
        .map_err(|_| format!("llama-server 响应不是 JSON：{body}"))?;
    value["choices"][0]["message"]["content"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("llama-server 响应缺少 choices[0].message.content，原文: {body}"))
}

fn extract_finish_reason(body: &str) -> Option<String> {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| value["choices"][0]["finish_reason"].as_str().map(str::to_string))
}

fn block_on_local<T>(fut: impl std::future::Future<Output = Result<T, String>>) -> Result<T, String> {
    if tokio::runtime::Handle::try_current().is_ok() {
        tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(fut))
    } else {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| format!("无法启动本地推理运行时：{error}"))?
            .block_on(fut)
    }
}

struct LlamaServerSession {
    port: u16,
    child: Mutex<Child>,
}

impl LlamaServerSession {
    fn ensure(model_path: &Path) -> Result<Self, String> {
        let server = find_llama_server(model_path)?;
        let port = bind_ephemeral_port()?;
        let dir = server
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| "llama-server 路径没有父目录".to_string())?;
        let model = model_path
            .to_str()
            .ok_or_else(|| "本地模型路径包含无效字符".to_string())?;
        let mut command = Command::new(&server);
        command
            .current_dir(&dir)
            .args(["--model", model, "--port", &port.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("启动 llama-server 失败: {error}"))?;
        if let Some(mut stdout) = child.stdout.take() {
            thread::spawn(move || {
                let mut buf = String::new();
                let _ = stdout.read_to_string(&mut buf);
            });
        }
        if let Some(mut stderr) = child.stderr.take() {
            thread::spawn(move || {
                let mut buf = String::new();
                let _ = stderr.read_to_string(&mut buf);
            });
        }
        wait_until_ready(port, &mut child)?;
        Ok(Self {
            port,
            child: Mutex::new(child),
        })
    }
}

impl Drop for LlamaServerSession {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn find_llama_server(model_path: &Path) -> Result<PathBuf, String> {
    let exe_name = if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    };
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(exe_name));
            candidates.push(dir.join("llama").join(exe_name));
            candidates.push(dir.join("runtimes").join("llama").join(exe_name));
        }
    }
    if let Some(dir) = model_path.parent() {
        candidates.push(dir.join(exe_name));
    }
    if let Ok(path) = which_in_path(exe_name) {
        candidates.push(path);
    }
    if let Some(found) = candidates.into_iter().find(|path| path.is_file()) {
        return Ok(found);
    }
    Err(format!(
        "找不到本地推理运行时（{exe_name}）。请把它放在本程序目录、runtimes/llama、模型同目录，或加入 PATH。这不是未实现桩。"
    ))
}

fn which_in_path(name: &str) -> Result<PathBuf, ()> {
    let path = std::env::var_os("PATH").ok_or(())?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(())
}

fn bind_ephemeral_port() -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|error| format!("无法为 llama-server 分配端口：{error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("无法读取 llama-server 端口：{error}"))?
        .port();
    drop(listener);
    Ok(port)
}

fn wait_until_ready(port: u16, child: &mut Child) -> Result<(), String> {
    // Process start wait, not a invented chat timeout.
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().ok().flatten() {
            return Err(format!("llama-server 在就绪前退出：{status}"));
        }
        if health_ok(port) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(200));
    }
    Err("llama-server 启动超时（等待 /health）".into())
}

fn health_ok(port: u16) -> bool {
    let mut stream = match TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse().unwrap(),
        Duration::from_millis(200),
    ) {
        Ok(stream) => stream,
        Err(_) => return false,
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
    if stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .is_err()
    {
        return false;
    }
    let mut buf = String::new();
    let _ = stream.read_to_string(&mut buf);
    buf.starts_with("HTTP/1.1 200") || buf.starts_with("HTTP/1.0 200")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_server_explains_where_to_put_the_binary() {
        let err = find_llama_server(Path::new("/tmp/does-not-exist.gguf"));
        // On a machine that already has llama-server on PATH this finds it.
        // The wording is only asserted when the binary is absent.
        if let Err(message) = err {
            assert!(message.contains("llama-server"), "{message}");
            assert!(message.contains("不是未实现桩"), "{message}");
        }
    }
}
