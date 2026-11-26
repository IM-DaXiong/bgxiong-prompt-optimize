use std::collections::HashSet;
use std::io::{self, BufRead, Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender, SyncSender};

use bgx_prompt_optimize::sdk::protocol::{self, OptimizeArgs, TextMode};
use bgx_prompt_optimize::sdk::{self, OptimizeOutput};
use bgx_prompt_optimize::{OptimizeError, TextCall, TextCompleter, TextReply};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    cancel, config::ShellConfig, materials::StandaloneMaterials, paths,
    text_port::ShellTextCompleter,
};

const MAX_FRAME_BYTES: u64 = 4 * 1024 * 1024;

enum Event {
    Input(Vec<u8>),
    End,
    Text {
        id: String,
        call: TextCall,
        reply: SyncSender<Result<TextReply, OptimizeError>>,
    },
    Finished {
        id: String,
        result: Result<OptimizeOutput, OptimizeError>,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    v: u64,
    id: String,
    op: String,
    #[serde(default)]
    args: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct CancelArgs {
    request_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct CallbackReply {
    v: u64,
    id: String,
    op: String,
    call_id: String,
    reply: Option<TextReply>,
    error: Option<CallbackError>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct CallbackError {
    code: String,
    message: String,
    #[serde(rename = "stepLabel")]
    _step_label: Option<String>,
}

struct ActiveTask {
    id: String,
    cancellation_id: String,
    sequence: u64,
    pending: Option<(String, SyncSender<Result<TextReply, OptimizeError>>)>,
}

struct CallbackText {
    id: String,
    events: Sender<Event>,
}

impl TextCompleter for CallbackText {
    fn check_cancel(&self) -> Result<(), OptimizeError> {
        cancel::check_cancel()
    }

    fn complete(&mut self, call: TextCall) -> Result<TextReply, OptimizeError> {
        self.check_cancel()?;
        let (reply, receiver) = mpsc::sync_channel(1);
        self.events
            .send(Event::Text {
                id: self.id.clone(),
                call,
                reply,
            })
            .map_err(|_| cancelled())?;
        let result = receiver.recv().map_err(|_| cancelled())?;
        self.check_cancel()?;
        result
    }
}

fn cancelled() -> OptimizeError {
    OptimizeError::new("OPTIMIZE_CANCELLED", "已取消")
}

fn config_path() -> Result<PathBuf, OptimizeError> {
    let root = match std::env::var_os("BGX_PROMPT_OPTIMIZE_DATA_DIR") {
        Some(path) => paths::require_absolute(&PathBuf::from(path), "工具数据目录")?,
        None => dirs::data_dir()
            .ok_or_else(|| OptimizeError::new("SHELL_APP_DATA", "找不到工具数据目录。"))?
            .join("com.bgxiong.prompt-optimize")
            .join(paths::APP_DATA_DIR_NAME),
    };
    paths::reject_host_pointers(&root)?;
    Ok(root.join(paths::CONFIG_FILE_NAME))
}

fn configuration(required: bool) -> Result<ShellConfig, OptimizeError> {
    let path = config_path()?;
    if !required && !path.is_file() {
        return Ok(ShellConfig::default());
    }
    ShellConfig::load_file(&path)
}

fn materials(config: &ShellConfig) -> Result<StandaloneMaterials, OptimizeError> {
    let exe = std::env::current_exe()
        .map_err(|_| OptimizeError::new("SHELL_APP_DATA", "找不到命令行程序目录。"))?;
    let config_path = config_path()?;
    let data_dir = config_path.parent().ok_or_else(|| invalid_frame())?;
    let resource_dir = std::env::var_os("BGX_PROMPT_OPTIMIZE_RESOURCE_DIR").map(PathBuf::from);
    let resource_dir = resource_dir
        .as_deref()
        .map(|path| paths::require_absolute(path, "工具资源目录"))
        .transpose()?;
    StandaloneMaterials::new(config, data_dir, resource_dir.as_deref(), exe.parent())
}

fn optimize_job(
    id: &str,
    args: OptimizeArgs,
    events: Sender<Event>,
) -> Result<OptimizeOutput, OptimizeError> {
    let callback = args.text_mode == TextMode::Callback;
    let config = configuration(!callback)?;
    let skill_materials = materials(&config)?;
    if callback {
        return sdk::optimize(
            id,
            args,
            config.language(),
            &skill_materials,
            &mut CallbackText {
                id: id.into(),
                events,
            },
        );
    }
    let mut text = ShellTextCompleter::new(config.clone(), false)?;
    sdk::optimize(id, args, config.language(), &skill_materials, &mut text)
}

fn write_frame(out: &mut impl Write, frame: Value) -> io::Result<()> {
    serde_json::to_writer(&mut *out, &frame)?;
    out.write_all(b"\n")?;
    out.flush()
}

fn invalid_frame() -> OptimizeError {
    OptimizeError::new("INVALID_FRAME", "无效的提示词优化协议消息。")
}

fn accept_reply(value: Value, active: &mut Option<ActiveTask>) {
    let accepted = (|| {
        let reply: CallbackReply = serde_json::from_value(value).ok()?;
        let task = active.as_mut()?;
        let (expected, _) = task.pending.as_ref()?;
        if reply.v != protocol::PROTOCOL_VERSION
            || reply.id != task.id
            || reply.op != "text.reply"
            || *expected != reply.call_id
        {
            return None;
        }
        let result = match (reply.reply, reply.error) {
            (Some(result), None) => Ok(result),
            (None, Some(error)) => Err(OptimizeError::new(error.code, error.message)),
            _ => return None,
        };
        let (_, sender) = task.pending.take()?;
        let _ = sender.send(result);
        Some(())
    })();
    if accepted.is_none() {
        eprintln!("UNEXPECTED_CALL_ID: invalid or stale text reply");
    }
}

fn read_events(input: impl BufRead, events: &Sender<Event>) {
    let mut input = input;
    loop {
        let mut frame = Vec::new();
        match (&mut input)
            .take(MAX_FRAME_BYTES + 1)
            .read_until(b'\n', &mut frame)
        {
            Ok(0) | Err(_) => break,
            Ok(_) if frame.len() as u64 > MAX_FRAME_BYTES => {
                let _ = events.send(Event::Input(Vec::new()));
                break;
            }
            Ok(_) => {
                if events.send(Event::Input(frame)).is_err() {
                    break;
                }
            }
        }
    }
    let _ = events.send(Event::End);
}

pub fn run_stdio() -> io::Result<()> {
    let (events, queue) = mpsc::channel();
    let input_events = events.clone();
    std::thread::spawn(move || read_events(io::stdin().lock(), &input_events));
    let mut out = io::stdout().lock();
    let mut active: Option<ActiveTask> = None;
    let mut seen = HashSet::new();
    for event in queue {
        match event {
            Event::Input(frame) => {
                let Ok(value) = serde_json::from_slice::<Value>(&frame) else {
                    write_frame(&mut out, protocol::failure(None, &invalid_frame()))?;
                    continue;
                };
                if value.get("op").and_then(Value::as_str) == Some("text.reply") {
                    accept_reply(value, &mut active);
                    continue;
                }
                let id = value
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty() && id.chars().count() <= 128)
                    .map(str::to_owned);
                if id.as_ref().is_some_and(|id| !seen.insert(id.clone())) {
                    eprintln!("DUPLICATE_ID: repeated request ID");
                    continue;
                }
                let result = serde_json::from_value::<Request>(value)
                    .map_err(|_| invalid_frame())
                    .and_then(|request| handle_request(request, &events, &mut active));
                match result {
                    Ok(Some(result)) => write_frame(
                        &mut out,
                        protocol::success(id.as_deref().unwrap_or_default(), result),
                    )?,
                    Ok(None) => {}
                    Err(error) => write_frame(&mut out, protocol::failure(id.as_deref(), &error))?,
                }
            }
            Event::Text { id, call, reply } => {
                let Some(task) = active.as_mut().filter(|task| task.id == id) else {
                    continue;
                };
                if cancel::is_cancelled() {
                    continue;
                }
                task.sequence += 1;
                let call_id = task.sequence.to_string();
                task.pending = Some((call_id.clone(), reply));
                write_frame(
                    &mut out,
                    json!({
                        "v":protocol::PROTOCOL_VERSION,"id":id,"event":"text.request","callId":call_id,
                        "call":{"messages":call.messages,"jsonMode":call.json_mode,"schema":call.schema,
                            "stepLabel":call.step_label,"imageCountPolicy":call.image_count_policy}
                    }),
                )?;
            }
            Event::Finished { id, result } => {
                if active.as_ref().is_none_or(|task| task.id != id) {
                    continue;
                }
                let task = active.take().expect("matching active job");
                let result = if cancel::is_cancelled() {
                    Err(cancelled())
                } else {
                    result
                };
                cancel::finish_job(&task.cancellation_id);
                let frame = match result {
                    Ok(output) => protocol::success(&id, output),
                    Err(error) => protocol::failure(Some(&id), &error),
                };
                write_frame(&mut out, frame)?;
            }
            Event::End => {
                if let Some(task) = active.take() {
                    let _ = cancel::request_cancel(&task.cancellation_id);
                }
                break;
            }
        }
    }
    Ok(())
}

fn handle_request(
    request: Request,
    events: &Sender<Event>,
    active: &mut Option<ActiveTask>,
) -> Result<Option<Value>, OptimizeError> {
    if request.id.is_empty() || request.id.chars().count() > 128 {
        return Err(invalid_frame());
    }
    if request.v != protocol::PROTOCOL_VERSION {
        return Err(OptimizeError::new(
            "UNSUPPORTED_PROTOCOL_VERSION",
            "此工具支持提示词优化协议 v1。",
        ));
    }
    if active.is_some() && matches!(request.op.as_str(), "skills.check" | "skills.install") {
        return Err(OptimizeError::new("BUSY", "请等待提示词优化完成。"));
    }
    match request.op.as_str() {
        "targets.list" if request.args.is_null() || request.args == json!({}) => {
            let config = configuration(false)?;
            let materials = materials(&config)?;
            let candidates = materials.skills.cached_candidates()?;
            let targets = sdk::list_targets(&materials, |id| {
                materials.skills.can_install(&candidates, id)
            })?;
            Ok(Some(json!({"targets":targets})))
        }
        "targets.list" => Err(invalid_frame()),
        "skills.check" if request.args.is_null() || request.args == json!({}) => {
            let candidates = materials(&configuration(false)?)?.skills.check()?;
            Ok(Some(json!({"candidates":candidates})))
        }
        "skills.check" => Err(invalid_frame()),
        "skills.install" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct InstallArgs {
                candidate_id: String,
                #[serde(default)]
                confirmed_new_install: bool,
            }
            let args: InstallArgs =
                serde_json::from_value(request.args).map_err(|_| invalid_frame())?;
            let result = materials(&configuration(false)?)?
                .skills
                .install(&args.candidate_id, args.confirmed_new_install)?;
            Ok(Some(json!(result)))
        }
        "optimize" => {
            let args: OptimizeArgs =
                serde_json::from_value(request.args).map_err(|_| invalid_frame())?;
            args.validate()?;
            let cancellation_id = cancel::start_job()?;
            let id = request.id;
            *active = Some(ActiveTask {
                id: id.clone(),
                cancellation_id,
                sequence: 0,
                pending: None,
            });
            let events = events.clone();
            std::thread::spawn(move || {
                let result = optimize_job(&id, args, events.clone());
                let _ = events.send(Event::Finished { id, result });
            });
            Ok(None)
        }
        "cancel" => {
            let args: CancelArgs =
                serde_json::from_value(request.args).map_err(|_| invalid_frame())?;
            let task = active
                .as_mut()
                .filter(|task| task.id == args.request_id)
                .ok_or_else(|| {
                    OptimizeError::new("OPTIMIZE_JOB_NOT_FOUND", "没有找到此优化任务。")
                })?;
            cancel::request_cancel(&task.cancellation_id)?;
            task.pending.take();
            Ok(Some(json!({"cancelRequestedFor":args.request_id})))
        }
        _ => Err(OptimizeError::new(
            "UNKNOWN_OPERATION",
            "无效的提示词优化操作。",
        )),
    }
}
