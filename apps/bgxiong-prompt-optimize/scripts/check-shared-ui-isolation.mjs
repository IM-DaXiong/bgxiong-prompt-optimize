/**
 * Official shell frontend must resolve shared chrome from ui/src and packages
 * from this app's node_modules, never from ui/node_modules (which may be a
 * junction into the main checkout).
 */
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const appRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = path.resolve(appRoot, "../..");
const shellModules = path.join(appRoot, "node_modules");
const uiModules = path.join(repoRoot, "ui/node_modules");
const requireFromShell = createRequire(path.join(appRoot, "package.json"));

const starts = [
  path.join(appRoot, "src/main.tsx"),
  path.join(appRoot, "src/uiChrome.ts"),
];

const importRe = /(?:from|import)\s*["']([^"']+)["']/g;

function resolveRelative(fromFile, spec) {
  const base = path.resolve(path.dirname(fromFile), spec);
  const candidates = [
    base,
    base + ".ts",
    base + ".tsx",
    base + ".js",
    base + ".jsx",
    base + ".css",
    path.join(base, "index.ts"),
    path.join(base, "index.tsx"),
  ];
  for (const c of candidates) {
    if (fs.existsSync(c) && fs.statSync(c).isFile()) return c;
  }
  return null;
}

const seen = new Set();
const queue = [...starts];
const hits = [];
let walked = 0;
let packageSpecs = 0;

while (queue.length) {
  const file = queue.shift();
  if (seen.has(file)) continue;
  seen.add(file);
  if (!fs.existsSync(file) || !fs.statSync(file).isFile()) continue;
  const norm = file.replaceAll("\\", "/");
  if (norm.includes("/ui/node_modules/") || norm.includes("\\ui\\node_modules\\")) {
    hits.push("resolved under ui/node_modules: " + path.relative(repoRoot, file));
    continue;
  }
  if (!/\.(ts|tsx|js|jsx)$/.test(file)) continue;
  walked += 1;
  const text = fs.readFileSync(file, "utf8");
  let m;
  while ((m = importRe.exec(text))) {
    const spec = m[1];
    if (spec.startsWith(".")) {
      const resolved = resolveRelative(file, spec);
      if (resolved) queue.push(resolved);
      continue;
    }
    if (spec.startsWith("@/") || spec.startsWith("node:")) continue;
    packageSpecs += 1;
    let resolved;
    try {
      resolved = requireFromShell.resolve(spec);
    } catch {
      hits.push(path.relative(repoRoot, file) + " cannot resolve " + spec + " from the shell node_modules");
      continue;
    }
    const resolvedNorm = resolved.replaceAll("\\", "/");
    const shellNorm = shellModules.replaceAll("\\", "/");
    const uiNorm = uiModules.replaceAll("\\", "/");
    if (resolvedNorm.includes(uiNorm)) {
      hits.push(spec + " resolved under ui/node_modules: " + resolved);
    } else if (!resolvedNorm.startsWith(shellNorm) && !resolvedNorm.includes("/node_modules/")) {
      // builtins may resolve outside; ignore
    }
  }
}

if (!fs.existsSync(shellModules)) {
  console.error("shell node_modules missing; run npm ci in apps/bgxiong-prompt-optimize first");
  process.exit(1);
}

if (hits.length) {
  console.error("shared-ui isolation violations (" + hits.length + "):");
  for (const h of [...new Set(hits)].slice(0, 40)) console.error("  " + h);
  process.exit(1);
}
console.log("shared-ui isolation ok, walked " + walked + " files, package specs " + packageSpecs);
