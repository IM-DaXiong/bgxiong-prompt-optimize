/**
 * T29: the shell bundles official repo skill trees only.
 * Destinations must stay relative (no development-machine drive letters).
 * Builtin SKILL.md bytes are compared against the same official directories
 * the main program ships (dev uses repository paths).
 */
import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";
import { fileURLToPath } from "node:url";
import assert from "node:assert/strict";

const appRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = path.resolve(appRoot, "../..");
const tauriDir = path.join(appRoot, "src-tauri");
const conf = JSON.parse(fs.readFileSync(path.join(tauriDir, "tauri.conf.json"), "utf8"));

const OFFICIAL = {
  "internal-prompt-skills/": path.join(
    repoRoot,
    "src-tauri/crates/bgx-internal-prompts/resources/internal-prompt-skills",
  ),
  "builtin-agent-skills/": path.join(repoRoot, "src-tauri/resources/builtin-agent-skills"),
};

const DIGEST_PACKAGES = ["h3-image-prompt-writing", "generic-video-prompt-writing"];

function sha256File(file) {
  return crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
}

function hasDriveLetter(s) {
  return /^[A-Za-z]:/.test(s) || s.startsWith("\\\\") || path.isAbsolute(s);
}

const resources = conf?.bundle?.resources;
assert.ok(resources && typeof resources === "object", "tauri.conf.json missing bundle.resources");

const destToSource = new Map();
for (const [src, dest] of Object.entries(resources)) {
  assert.equal(typeof dest, "string");
  assert.ok(!hasDriveLetter(dest), "bundle destination must not contain a drive letter: " + dest);
  assert.ok(!hasDriveLetter(src), "bundle source must stay repo-relative: " + src);
  const abs = path.resolve(tauriDir, src);
  destToSource.set(dest.replace(/\\/g, "/"), abs);
}

for (const [dest, official] of Object.entries(OFFICIAL)) {
  const mapped = destToSource.get(dest);
  assert.ok(mapped, "tauri.conf.json missing destination " + dest);
  assert.ok(fs.existsSync(official) && fs.statSync(official).isDirectory(), "official dir missing: " + official);
  const officialReal = fs.realpathSync(official);
  const mappedReal = fs.realpathSync(mapped);
  assert.equal(
    officialReal,
    mappedReal,
    dest + " must map to the official packaged directory\n  mapped=" + mappedReal + "\n  official=" + officialReal,
  );
}

const builtinOfficial = OFFICIAL["builtin-agent-skills/"];
const digests = {};
for (const name of DIGEST_PACKAGES) {
  const officialFile = path.join(builtinOfficial, name, "SKILL.md");
  const mappedFile = path.join(destToSource.get("builtin-agent-skills/"), name, "SKILL.md");
  assert.ok(fs.existsSync(officialFile), "missing official SKILL.md: " + officialFile);
  const officialDigest = sha256File(officialFile);
  const mappedDigest = sha256File(mappedFile);
  assert.equal(mappedDigest, officialDigest, name + " SKILL.md digest mismatch");
  digests[name] = "sha256:" + officialDigest;
}

console.log("resource bundle ok");
console.log(JSON.stringify({ officialRoots: OFFICIAL, skillMdDigests: digests }, null, 2));
