use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{json, Value};

struct Session {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Session {
    fn send(&mut self, frame: Value) {
        writeln!(self.input, "{frame}").unwrap();
        self.input.flush().unwrap();
    }

    fn read(&mut self) -> Value {
        let mut line = String::new();
        assert!(self.output.read_line(&mut line).unwrap() > 0);
        serde_json::from_str(&line).unwrap()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn callback_optimize_and_cancel_work_without_a_host() {
    let data_dir = tempfile::tempdir().unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let resources = repo.join("src-tauri/resources");
    let templates =
        repo.join("src-tauri/crates/bgx-internal-prompts/resources/internal-prompt-skills");
    let exe = env!("CARGO_BIN_EXE_bgxiong-prompt-optimize-cli");
    let mut child = Command::new(exe)
        .arg("--stdio")
        .current_dir(data_dir.path())
        .env("BGX_PROMPT_OPTIMIZE_DATA_DIR", data_dir.path())
        .env("BGXIONG_INTERNAL_PROMPT_ROOT", templates)
        .env("BGX_PROMPT_OPTIMIZE_RESOURCE_DIR", resources)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let input = child.stdin.take().unwrap();
    let output = BufReader::new(child.stdout.take().unwrap());
    let mut session = Session {
        child,
        input,
        output,
    };
    session.send(json!({"v":1,"id":"targets","op":"targets.list"}));
    let targets = session.read();
    let target = targets["result"]["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["targetId"] == "image/qwen-image-2.1")
        .unwrap();
    assert_eq!(target["availability"], "ready");
    for id in ["complete", "cancelled"] {
        session.send(json!({"v":1,"id":id,"op":"optimize","args":{
            "targetId":target["targetId"],"expectedMethodDigest":target["methodDigest"],
            "sourcePrompt":"雨夜公交车","textMode":"callback"
        }}));
        let request = session.read();
        assert_eq!(request["event"], "text.request", "{request}");
        assert_eq!(request["id"], id);
        assert!(request["call"]["schema"].is_object());
        if id == "complete" {
            session.send(
                json!({"v":1,"id":id,"op":"text.reply","callId":request["callId"],
                "reply":{"content":json!({"optimizedPrompt":"红色公交车的倒影"}).to_string()}}),
            );
            let result = session.read();
            assert_eq!(
                result["result"]["optimizedPrompt"], "红色公交车的倒影",
                "{result}"
            );
            assert_eq!(
                result["result"]["receipt"]["methodDigest"],
                target["methodDigest"]
            );
        } else {
            session.send(json!({"v":1,"id":"control","op":"cancel","args":{"requestId":id}}));
            assert_eq!(session.read()["result"]["cancelRequestedFor"], id);
            assert_eq!(session.read()["error"]["code"], "OPTIMIZE_CANCELLED");
        }
    }
    session.send(json!({"v":2,"id":"foreign","op":"targets.list"}));
    assert_eq!(
        session.read()["error"]["code"],
        "UNSUPPORTED_PROTOCOL_VERSION"
    );

    session.send(json!({"v":1,"id":"video","op":"optimize","args":{
        "targetId":"video/kling.omni@2","sourcePrompt":"她步行。","textMode":"callback"
    }}));
    let stages = [
        (
            "video-content-optimize",
            json!({"shots":[{"shotId":"shot-1","facts":[{
            "factId":"f1","kind":"action","text":"她步行。","semanticSlot":"action_interaction",
            "polarity":"positive","entityRefIds":[],"sourceUnitIds":[]
        }],"quotes":[]}]}),
        ),
        ("video-content-review", json!({"findings":[]})),
        (
            "video-expression",
            json!({"lines":[{"shotId":"shot-1","factIds":["f1"],
            "quoteIds":[],"referenceUseIds":[],"text":"她步行。"}],"conflicts":[]}),
        ),
        ("video-expression-review", json!({"findings":[]})),
    ];
    for (stage, response) in stages {
        let frame = session.read();
        assert_eq!(frame["event"], "text.request", "{frame}");
        assert_eq!(frame["call"]["stepLabel"], stage);
        session.send(
            json!({"v":1,"id":"video","op":"text.reply","callId":frame["callId"],
            "reply":{"content":response.to_string(),"finishReason":"stop"}}),
        );
    }
    let result = session.read();
    assert_eq!(result["result"]["optimizedPrompt"], "她步行。", "{result}");
    assert_eq!(
        result["result"]["receipt"]["methodId"],
        "kling-prompt-writing"
    );
}
