//! Shell-owned AdapterTextRunner. Process / wasmtime stay out of http-client.
//!
//! The main program runner cannot be linked here (it depends on DbState). This
//! port reuses the published `bgx-adapter-v2` CLI layout and flag set that the
//! host already uses for synchronous text (`submit` without `--host-checkpoints`).
//! If the official runtime is not next to this shell, it fails with a locate
//! error, not a permanent "unsupported" stub.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use bgx_text_http::{AdapterPackage, AdapterTextRunner};
use serde_json::{json, Value};

pub struct ShellAdapterTextRunner;

impl AdapterTextRunner for ShellAdapterTextRunner {
    fn run_synchronous(
        &self,
        package: &AdapterPackage,
        request_id: &str,
        payload: &Value,
    ) -> Result<Value, String> {
        let (cli, worker, sdk) = locate_adapter_runtime()?;
        let host = read_json(&package.dir.join("host-config.json"))?;
        if package.dir.join("managed-host.json").is_file() {
            return Err("This text operation requires explicit package connection configuration".into());
        }
        let budget_ms = host["budgetMs"]["submit"]
            .as_u64()
            .filter(|budget| *budget > 0)
            .ok_or_else(|| {
                "host budgetMs.submit must be a positive integer".to_string()
            })?;
        let features = negotiated_features(&package.root)?;
        let invocation = json!({
            "protocolVersion": 2,
            "operation": "submit",
            "profileId": host["bindingId"],
            "capabilitySlot": "text",
            "requestId": request_id,
            "budgetMs": budget_ms,
            "payloadJson": payload.to_string(),
            "bindingId": host["bindingId"],
            "negotiatedVersion": 2,
            "enabledFeatures": features,
        });
        let run_dir = package.dir.join("runs").join(request_id);
        fs::create_dir_all(&run_dir)
            .map_err(|error| format!("{}: {error}", run_dir.display()))?;
        let config_path = run_dir.join("config.json");
        let input_path = run_dir.join("invocation.json");
        let mut config = host.clone();
        if let Some(obj) = config.as_object_mut() {
            obj.remove("budgetMs");
            obj.insert("media".into(), json!([]));
        }
        fs::write(&config_path, config.to_string())
            .map_err(|error| format!("{}: {error}", config_path.display()))?;
        fs::write(&input_path, invocation.to_string())
            .map_err(|error| format!("{}: {error}", input_path.display()))?;
        let workspace = package.dir.join("text-sessions").join(request_id);
        let create = !workspace.exists();
        if create {
            if let Some(parent) = workspace.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| format!("{}: {error}", parent.display()))?;
            }
        }
        let output = workspace
            .join("receipts")
            .join(format!("{request_id}.response.json"));
        let mut cmd = Command::new(&cli);
        cmd.arg("submit")
            .arg("--sdk")
            .arg(&sdk)
            .arg("--worker")
            .arg(&worker)
            .arg("--package")
            .arg(&package.root)
            .arg("--workspace")
            .arg(&workspace)
            .arg("--config")
            .arg(&config_path)
            .arg("--input")
            .arg(&input_path)
            .arg("--output")
            .arg(&output);
        if create {
            cmd.arg("--create-workspace");
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let mut child = cmd
            .spawn()
            .map_err(|error| format!("启动适配运行时失败：{error}"))?;
        let mut stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let mut stderr = child.stderr.take().expect("piped stderr");
        let stderr_reader = std::thread::spawn(move || {
            let mut err = String::new();
            let _ = stderr.read_to_string(&mut err);
            err
        });
        let mut response: Option<Value> = None;
        for line in BufReader::new(stdout).lines() {
            let line = line.map_err(|error| format!("读取适配运行时输出失败：{error}"))?;
            let value: Value = serde_json::from_str(&line)
                .map_err(|_| "SDK CLI 输出不是 JSON 行".to_string())?;
            if value.get("hostCheckpoint").is_some() {
                return Err("独立壳的同步文本会话不提供数据库 checkpoint".into());
            }
            response = Some(value);
        }
        drop(stdin);
        let status = child
            .wait()
            .map_err(|error| format!("等待适配运行时失败：{error}"))?;
        let err = stderr_reader.join().unwrap_or_default();
        match response {
            Some(value) if value.get("protocolVersion").is_some() => Ok(value),
            _ => {
                if output.is_file() {
                    return read_json(&output);
                }
                Err(format!(
                    "SDK CLI 退出 {status}：{}",
                    err.trim()
                ))
            }
        }
    }
}

fn locate_adapter_runtime() -> Result<(PathBuf, PathBuf, PathBuf), String> {
    let dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .ok_or_else(|| "无法解析本程序目录，找不到适配运行时。".to_string())?;
    let dir = match std::env::var_os("BGX_PROMPT_OPTIMIZE_ADAPTER_RUNTIME_DIR") {
        Some(configured) => {
            let path = PathBuf::from(configured);
            if !path.is_absolute() {
                return Err("适配运行时目录必须是绝对路径。".into());
            }
            path
        }
        None => dir,
    };
    let sdk = dir.join("adapter-sdk-v2");
    let cli = dir.join(exe_name("bgx-adapter-v2"));
    let worker = sdk.join(exe_name("bgx-adapter-v2-worker"));
    if !cli.is_file() || !worker.is_file() || !sdk.join("build-receipt.json").is_file() {
        return Err(
            "找不到适配运行时（本程序目录下的 bgx-adapter-v2 与 adapter-sdk-v2）。请放入与主程序相同的适配运行时，而不是使用未实现桩。".into(),
        );
    }
    Ok((cli, worker, sdk))
}

fn exe_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_string()
    }
}

fn read_json(path: &Path) -> Result<Value, String> {
    let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("{}: {error}", path.display()))
}

fn negotiated_features(package_root: &Path) -> Result<Vec<String>, String> {
    let manifest = read_json(&package_root.join("manifest.json"))?;
    let mut enabled = Vec::new();
    for field in ["requiredFeatures", "optionalFeatures"] {
        let values = manifest
            .get(field)
            .and_then(Value::as_array)
            .ok_or_else(|| "manifest feature list missing".to_string())?;
        for value in values {
            let feature = value
                .as_str()
                .ok_or_else(|| "manifest feature is not text".to_string())?;
            if feature == "stream-events-v1" && !enabled.iter().any(|item| item == feature) {
                enabled.push(feature.to_owned());
            } else if matches!(
                feature,
                "image-projection-v1"
                    | "media-projection-v1"
                    | "media-target-selection-v1"
                    | "video-target-keyframes-v1"
                    | "submission-rejection-v1"
                    | "video-speech-v1"
                    | "operation-preflight-v1"
                    | "submission-resolution-v1"
                    | "opaque-file-upload-name-v1"
            ) {
                // Adopted in host trust, not as a WIT feature enum.
            } else if field == "requiredFeatures" {
                return Err(format!("host does not adopt required SDK feature {feature}"));
            }
        }
    }
    Ok(enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_runtime_is_a_locate_error_not_an_unsupported_stub() {
        // When this test binary is not next to the official SDK, locate must
        // explain where to put it and must not say "未实现".
        match locate_adapter_runtime() {
            Ok(_) => {}
            Err(message) => {
                assert!(message.contains("bgx-adapter-v2"), "{message}");
                assert!(
                    message.contains("找不到适配运行时"),
                    "locate error must say the runtime is missing: {message}"
                );
                assert!(
                    !message.starts_with("未实现") && message != "未实现",
                    "must not be an unsupported stub: {message}"
                );
            }
        }
    }
}
