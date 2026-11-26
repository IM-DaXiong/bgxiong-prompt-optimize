/**
 * Walk the standalone shell TS import graph and fail if a main-window / DB
 * module is reached. This is a source-graph check, not a full tsc compile.
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const appRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = path.resolve(appRoot, "../..");
const starts = [
  path.join(appRoot, "src/main.tsx"),
  path.join(appRoot, "src/App.tsx"),
  path.join(appRoot, "src/uiChrome.ts"),
  path.join(appRoot, "src/SettingsDialog.tsx"),
];

const forbidden = [
  "/ui/src/api.ts",
  "/ui/src/api/",
  "/domain/rich-plain-text/persist.ts",
  "/domain/app-toast-sink",
  "/shortcuts/ShortcutContext",
  "/shortcuts/useShortcutChordLabel",
  "/i18n/index.ts",
  "/utils/logger.ts",
  "/settings/appSettings",
  "/settings/preferences/storage",
  "/settings/appSettingsReactContext",
  "/theme/ThemeProvider",
  "/rich-plain-editor/defaultHostPorts",
  "/rich-plain-editor/installMainRichPlainHost",
  "/rich-plain-editor/RichPlainEditor.tsx",
  "@tauri-apps/api",
];

const allowedTauri = ["@tauri-apps/api/core", "@tauri-apps/api/event", "@tauri-apps/plugin-clipboard-manager", "@tauri-apps/plugin-dialog"];

const importRe = /(?:from|import)\s*["']([^"']+)["']/g;

function resolveImport(fromFile, spec) {
  if (spec.startsWith("@tauri-apps/")) return spec;
  if (!spec.startsWith(".")) return null;
  const base = path.resolve(path.dirname(fromFile), spec);
  const candidates = [
    base,
    base + ".ts",
    base + ".tsx",
    base + ".js",
    base + ".jsx",
    path.join(base, "index.ts"),
    path.join(base, "index.tsx"),
  ];
  for (const c of candidates) {
    if (fs.existsSync(c) && fs.statSync(c).isFile()) return c;
  }
  return base;
}

const seen = new Set();
const queue = [...starts];
const hits = [];
let walked = 0;

while (queue.length) {
  const file = queue.shift();
  if (seen.has(file)) continue;
  seen.add(file);
  if (typeof file !== "string" || file.startsWith("@tauri-apps/")) {
    const rel = String(file);
    if (rel.startsWith("@tauri-apps/") && !allowedTauri.some((a) => rel === a || rel.startsWith(a + "/"))) {
      if (rel.includes("/api/core") || rel.includes("/api/event") || rel.includes("plugin-")) continue;
      hits.push("package " + rel);
    }
    continue;
  }
  if (!fs.existsSync(file) || !fs.statSync(file).isFile()) continue;
  if (!/\.(ts|tsx|js|jsx)$/.test(file)) continue;
  walked += 1;
  const rel = file.replaceAll("\\", "/");
  for (const needle of forbidden) {
    if (rel.replaceAll("\\", "/").includes(needle.replaceAll("\\", "/"))) {
      hits.push(rel);
    }
  }
  const text = fs.readFileSync(file, "utf8");
  // Skip type-only imports for runtime graph, but still honor them for tsc
  // boundary: type imports of hosted wrappers would pull persist into tsc.
  const typeRe = /import\s+type\s+[^;]*from\s*["']([^"']+)["']/g;
  const specs = [];
  let m;
  const runtimeText = text.replace(/import\s+type\s+[^;]*from\s*["'][^"']+["'];?/g, (stmt) => {
    const sm = stmt.match(/from\s*["']([^"']+)["']/);
    if (sm) specs.push({ spec: sm[1], typeOnly: true, stmt });
    return "";
  });
  while ((m = importRe.exec(runtimeText))) {
    specs.push({ spec: m[1], typeOnly: false });
  }
  for (const item of specs) {
    if (item.spec.startsWith("@tauri-apps/")) {
      const relSpec = item.spec;
      const allowed = allowedTauri.some((a) => relSpec === a || relSpec.startsWith(a + "/"));
      if (!allowed) hits.push(path.relative(repoRoot, file) + " imports " + relSpec);
      continue;
    }
    const resolved = resolveImport(file, item.spec);
    if (!resolved) continue;
    if (typeof resolved === "string" && resolved.startsWith("@")) continue;
    queue.push(resolved);
    const resolvedRel = String(resolved).replaceAll("\\", "/");
    for (const needle of forbidden) {
      if (resolvedRel.includes(needle.replaceAll("\\", "/"))) {
        hits.push(path.relative(repoRoot, file) + " -> " + path.relative(repoRoot, resolved));
      }
    }
  }
}

if (hits.length) {
  console.error("import boundary violations (" + hits.length + "):");
  for (const h of [...new Set(hits)].slice(0, 40)) console.error("  " + h);
  process.exit(1);
}
console.log("import boundary ok, walked " + walked + " files");
