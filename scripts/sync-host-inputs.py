"""Import the optimizer's shared inputs from an immutable host checkout."""
import argparse
from collections import deque
import hashlib
import json
from pathlib import Path, PurePosixPath
import posixpath
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
LOCK = ROOT / "deps/host-inputs.lock.json"
CRATES = "src-tauri/crates"
APP = "apps/bgxiong-prompt-optimize"
CORE = f"{CRATES}/bgx-prompt-optimize"
SHARED_CRATES = (
    "bgx-agent-skills-core", "bgx-contract-gen", "bgx-creator-contract",
    "bgx-text-http", "bgx-video-prompt-contract", "bgx-skill-distribution",
)
TEMPLATES = (
    "image-prompt-optimize", "text-prompt-optimize", "video-source-understanding",
    "video-content-optimize", "video-content-review", "video-expression",
    "video-expression-review", "lyric-source-understanding", "shared",
)
METHODS = (
    "generic-video-prompt-writing", "h3-image-prompt-writing", "h3-prompt-writing",
    "kling-prompt-writing", "ltx-2-5-prompt-writing", "qwen-image-2-1-prompt-writing",
    "seedance-prompt-writing", "seedream-5-5-prompt-writing", "wan-prompt-writing",
    "literary-clip-storyboard-planning", "action-clip-storyboard-planning",
)
UI_ROOTS = (
    "ui/src/components/ui-chrome-select/UiChromeSelect.tsx",
    "ui/src/components/common/button/Btn.tsx",
    "ui/src/theme/themeEarlyHydrate.ts",
    "ui/src/domain/video-prompt-pipeline/projectionContract.generated.ts",
    "ui/src/index.css", "ui/src/styles/ui-edge-resize-shell.css",
)
DEPENDENCY = re.compile(r'''(?:\bfrom\s*|\bimport\s*(?:\(\s*)?|@import\s*|url\(\s*)["'](\.[^"']+)["']''')


def git(source, *args, data=None):
    return subprocess.run(["git", "-C", str(source), *args], input=data, capture_output=True, check=True).stdout


def read_tree(source, revision, paths):
    listing = git(source, "ls-tree", "-rz", revision, "--", *paths)
    names = {}
    for entry in listing.split(b"\0"):
        if not entry:
            continue
        info, name = entry.split(b"\t", 1)
        mode, kind, object_id = info.split()
        if kind == b"blob" and mode in (b"100644", b"100755"):
            names[name.decode()] = object_id
    blobs = git(source, "cat-file", "--batch", data=b"\n".join(names.values()) + b"\n")
    offset = 0
    files = {}
    for name, expected in names.items():
        end = blobs.index(b"\n", offset)
        object_id, kind, size = blobs[offset:end].split()
        if object_id != expected or kind != b"blob":
            raise ValueError(f"Unexpected Git object for {name}")
        offset = end + 1 + int(size)
        files[name] = blobs[end + 1:offset]
        offset += 1
    return files


def ui_closure(files):
    pending = deque(UI_ROOTS)
    selected = {}
    parents = {}
    while pending:
        path = pending.popleft()
        if path in selected:
            continue
        selected[path] = files[path]
        if not path.endswith((".ts", ".tsx", ".css")):
            continue
        code = re.sub(r"/\*[\s\S]*?\*/|^\s*//[^\n]*", "", files[path].decode("utf-8-sig"), flags=re.M)
        for spec in DEPENDENCY.findall(code):
            relative = posixpath.normpath(posixpath.join(posixpath.dirname(path), spec.split("?", 1)[0]))
            resolved = next((relative + suffix for suffix in (
                "", ".ts", ".tsx", ".js", "/index.ts", "/index.tsx",
            ) if relative + suffix in files), None)
            if not resolved:
                trace = [path]
                while trace[-1] in parents and len(trace) < 22:
                    trace.append(parents[trace[-1]])
                raise ValueError(f"{path}: unresolved shared UI input {spec}\n" + " <- ".join(trace))
            if resolved not in UI_ROOTS:
                parents.setdefault(resolved, path)
            pending.append(resolved)
    return selected


def digest(data):
    return hashlib.sha256(data).hexdigest()


def tree_digest(files):
    return digest("".join(f"{name}\0{sha}\n" for name, sha in sorted(files.items())).encode())


def verify(lock):
    if lock["treeSha256"] != tree_digest(lock["files"]):
        raise ValueError("Invalid input tree digest")
    for name, sha in lock["files"].items():
        path = ROOT / name
        if not path.is_file() or digest(path.read_bytes()) != sha:
            raise ValueError(f"Missing or changed shared input: {name}")
    print(f'{len(lock["files"])} inputs verified at {lock["sourceCommit"]}')


def safe_path(name):
    path = PurePosixPath(name)
    if path.is_absolute() or ".." in path.parts or ":" in name:
        raise ValueError(f"Invalid input path: {name}")
    return ROOT / name


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--source", type=Path)
    parser.add_argument("--revision", default="HEAD")
    parser.add_argument("--bootstrap", action="store_true")
    args = parser.parse_args()
    previous = json.loads(LOCK.read_text(encoding="utf-8")) if LOCK.exists() else None
    if args.check:
        verify(previous)
        return
    if args.source is None:
        parser.error("--source is required to import inputs")
    commit = git(args.source, "rev-parse", f"{args.revision}^{{commit}}").decode().strip()
    inputs = read_tree(args.source, commit, [
        *(f"{CRATES}/{name}" for name in SHARED_CRATES),
        *(f"{CRATES}/bgx-internal-prompts/{path}" for path in ("Cargo.toml", "build.rs", "src", "contracts")),
        *(f"{CRATES}/bgx-internal-prompts/resources/internal-prompt-skills/{name}" for name in TEMPLATES),
        *(f"src-tauri/resources/builtin-agent-skills/{name}" for name in METHODS),
        "src-tauri/resources/video-prompt-adapters", "src-tauri/icons/icon.ico",
        "src-tauri/icons/icon.png", "packages/app-contracts/v1",
    ])
    ui_inputs = read_tree(args.source, commit, [
        "ui/src", "ui/public", "ui/package.json", "contracts", "resources/locales", "runtimes",
    ])
    inputs.update(ui_closure({**inputs, **ui_inputs}))
    if previous:
        verify(previous)
    old_files = previous["files"] if previous else {}
    for name, data in inputs.items():
        path = safe_path(name)
        if path.exists() and name not in old_files and path.read_bytes() != data:
            raise ValueError(f"Import would overwrite unowned input: {name}")
    for name in old_files.keys() - inputs.keys():
        safe_path(name).unlink()
    for name, data in inputs.items():
        path = safe_path(name)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
    hashes = {name: digest(data) for name, data in sorted(inputs.items())}
    host_version = json.loads(git(args.source, "show", f"{commit}:src-tauri/tauri.conf.json"))["version"]
    lock = {"sourceCommit": commit, "hostVersion": host_version, "treeSha256": tree_digest(hashes), "files": hashes}
    LOCK.parent.mkdir(parents=True, exist_ok=True)
    LOCK.write_text(json.dumps(lock, indent=2) + "\n", encoding="utf-8", newline="\n")
    if args.bootstrap:
        for name, data in read_tree(args.source, commit, [APP, CORE]).items():
            path = safe_path(name)
            if path.exists():
                raise ValueError(f"Bootstrap destination already exists: {name}")
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    verify(lock)


if __name__ == "__main__":
    main()
