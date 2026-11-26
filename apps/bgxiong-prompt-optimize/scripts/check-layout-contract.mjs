import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const read = name => readFileSync(new URL("../src/" + name, import.meta.url), "utf8");
const app = read("App.tsx");
const chrome = read("uiChrome.ts");
const main = read("main.tsx");
const settings = read("SettingsDialog.tsx");

assert.match(chrome, /components\/common\/button\/Btn/);
assert.match(chrome, /components\/ui-chrome-select\/UiChromeSelect/);
assert.match(app, /<UiChromeSelect label="目标模型"/);
assert.equal((app.match(/<textarea /g) ?? []).length, 2);
assert.match(app, /<textarea id="prompt-source" className="ui-edit-textarea"/);
assert.match(app, /<textarea id="prompt-result" className="ui-edit-textarea"/);
assert.doesNotMatch(app, /readOnly|VideoTargetForm/);
assert.match(app, /api\.copyText\(resultText\)/);
assert.match(app, /api\.optimizePrompt\(/);
assert.match(settings, /<UiChromeSelect label="文本模型"/);
assert.match(main, /ui\/src\/index\.css/);
assert.match(main, /ui-edge-resize-shell\.css/);
console.log("layout contract ok");
