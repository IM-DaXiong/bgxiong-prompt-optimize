"""Call the standalone optimizer with a caller-owned text model."""

import argparse
import json
from pathlib import Path
import subprocess


def optimize(executable, source, complete):
    request = {
        "v": 1,
        "id": "demo",
        "op": "optimize",
        "args": {
            "targetId": "image/qwen-image-2.1",
            "sourcePrompt": source,
            "textMode": "callback",
        },
    }
    with subprocess.Popen(
        [str(Path(executable).resolve()), "--stdio"],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, encoding="utf-8",
    ) as process:
        def send(frame):
            process.stdin.write(json.dumps(frame, ensure_ascii=False) + "\n")
            process.stdin.flush()

        try:
            send(request)
            for line in process.stdout:
                frame = json.loads(line)
                if frame["v"] != 1 or frame["id"] != request["id"]:
                    raise RuntimeError("Unexpected optimizer frame")
                if frame["event"] == "text.request":
                    send({
                        "v": 1, "id": frame["id"], "op": "text.reply",
                        "callId": frame["callId"], "reply": complete(frame["call"]),
                    })
                elif frame["event"] == "done":
                    return frame["result"]["optimizedPrompt"]
                elif frame["event"] == "error":
                    error = frame["error"]
                    raise RuntimeError(f"{error['code']}: {error['message']}")
            raise RuntimeError("Optimizer exited before sending a result")
        finally:
            process.stdin.close()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.terminate()


def fixture_model(call):
    assert call["messages"] and call["schema"]["type"] == "object"
    return {"content": json.dumps({"optimizedPrompt": "雨夜街口，红色公交车驶过湿润路面。"}, ensure_ascii=False)}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cli", required=True)
    parser.add_argument("--self-test", action="store_true", required=True)
    args = parser.parse_args()
    result = optimize(args.cli, "一辆红色公交车驶过雨夜街口。", fixture_model)
    assert result == "雨夜街口，红色公交车驶过湿润路面。"
    print(result)
