"""Assemble a complete standalone optimizer runtime from one source revision."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
CORE = ROOT / "src-tauri/crates/bgx-prompt-optimize"
APP = ROOT / "apps/bgxiong-prompt-optimize"
TEMPLATES = "src-tauri/crates/bgx-internal-prompts/resources/internal-prompt-skills"
METHODS = "src-tauri/resources/builtin-agent-skills"


def digest(file):
    with file.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--gui", type=Path, required=True)
    parser.add_argument("--cli", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--profile", choices=("debug", "release"), required=True)
    args = parser.parse_args()
    git = lambda *words: subprocess.check_output(["git", "-C", str(ROOT), *words], text=True).strip()
    commit = git("rev-parse", "HEAD")
    if git("status", "--porcelain"):
        raise ValueError("Use a frozen source checkout to stage a component")
    lock = json.loads((ROOT / "deps/host-inputs.lock.json").read_text("utf-8"))
    config = json.loads((APP / "src-tauri/tauri.conf.json").read_text("utf-8"))
    mapped = {
        "bgxiong-prompt-optimize.exe": args.gui,
        "bgxiong-prompt-optimize-cli.exe": args.cli,
        "sdk/schema.json": CORE / "protocol/v1/schema.json",
        "sdk/examples.jsonl": CORE / "protocol/v1/examples.jsonl",
        "sdk/optimize.py": ROOT / "examples/python/optimize.py",
    }
    for source, expected in lock["files"].items():
        for prefix, destination in ((TEMPLATES, "internal-prompt-skills"), (METHODS, "builtin-agent-skills")):
            if source.startswith(prefix + "/"):
                file = ROOT / source
                if digest(file) != expected:
                    raise ValueError(f"Runtime input changed: {source}")
                mapped[destination + source[len(prefix):]] = file
    if args.output.exists():
        raise ValueError(f"Choose a new staging directory: {args.output}")
    missing = [file for file in mapped.values() if not file.is_file()]
    if missing:
        raise ValueError(f"Missing component files: {missing}")
    files = {}
    for path, file in sorted(mapped.items()):
        target = args.output / path
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(file, target)
        sha256 = digest(target)
        if sha256 != digest(file):
            raise ValueError(f"Component changed during staging: {path}")
        files[path] = {"bytes": target.stat().st_size, "sha256": sha256}
    receipt = {"schemaVersion": 1, "protocolVersion": 1, "sourceCommit": commit, "sourceDirty": False,
               "hostInputsCommit": lock["sourceCommit"], "hostInputsTreeSha256": lock["treeSha256"],
               "productVersion": config["version"], "profile": args.profile, "platform": "windows-x64",
               "files": files}
    (args.output / "component.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    print(f"{len(files)} files staged at {args.output}")


if __name__ == "__main__":
    main()
