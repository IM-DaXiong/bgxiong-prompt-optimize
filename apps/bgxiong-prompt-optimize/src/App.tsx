import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import * as api from "./api";
import SettingsDialog from "./SettingsDialog";
import { IMAGE_HOST, LITERARY_HOST, type OptimizeTarget, type ShellConfigInput,
  type ShellConfigPublic, type SkillListItem, type TextChannel } from "./types";
import { Btn, UiChromeSelect } from "./uiChrome";

const emptyConfig: ShellConfigInput = {
  providerKind: "",
  baseUrl: "",
  model: "",
  apiKey: "",
  timeoutSecs: "",
  maxOutputTokens: "",
  skillLibraryRoot: "",
  internalTemplateRoot: "",
  adapterPackageRoot: "",
  localModelPath: "",
  language: "zh",
  supportsVision: false,
};

function publicToInput(
  saved: Awaited<ReturnType<typeof api.loadConfig>>,
  apiKey: string,
): ShellConfigInput {
  return {
    providerKind: saved.providerKind,
    baseUrl: saved.baseUrl ?? "",
    model: saved.model ?? "",
    apiKey,
    timeoutSecs: saved.timeoutSecs == null ? "" : String(saved.timeoutSecs),
    maxOutputTokens: saved.maxOutputTokens == null ? "" : String(saved.maxOutputTokens),
    skillLibraryRoot: saved.skillLibraryRoot ?? "",
    internalTemplateRoot: saved.internalTemplateRoot ?? "",
    adapterPackageRoot: saved.adapterPackageRoot ?? "",
    localModelPath: saved.localModelPath ?? "",
    language: saved.language === "en" ? "en" : "zh",
    supportsVision: saved.supportsVision,
  };
}

export default function App() {
  const [saved, setSaved] = useState<ShellConfigPublic | null>(null);
  const [settings, setSettings] = useState<ShellConfigInput | null>(null);
  const [showSkillSettings, setShowSkillSettings] = useState(false);
  const [channels, setChannels] = useState<TextChannel[]>([]);
  const [targets, setTargets] = useState<OptimizeTarget[]>([]);
  const [targetId, setTargetId] = useState("");
  const [source, setSource] = useState("");
  const [resultText, setResultText] = useState("");
  const [busy, setBusy] = useState(false);
  const [jobId, setJobId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [legacyMethods, setLegacyMethods] = useState<SkillListItem[] | null>(null);
  const [legacyMethod, setLegacyMethod] = useState("");
  const [images, setImages] = useState<string[]>([]);
  const running = useRef(false);
  const cancelled = useRef(false);
  const selected = targets.find(target => target.targetId === targetId);
  const legacyHost = targetId === "image/general" ? IMAGE_HOST : targetId === "literary/general" ? LITERARY_HOST : null;
  const savedServiceMatches = saved?.providerKind === settings?.providerKind
    && (saved?.baseUrl ?? "") === settings?.baseUrl
    && (saved?.adapterPackageRoot ?? "") === settings?.adapterPackageRoot;

  useEffect(() => {
    const unlisten = listen<string>("optimize-started", event => {
      if (running.current) setJobId(event.payload);
    });
    void Promise.all([api.loadConfig(), api.listTextChannels(), api.listOptimizeTargets()])
      .then(([config, textChannels, choices]) => {
        setSaved(config);
        setChannels(textChannels);
        setTargets(choices);
        setTargetId(choices.find(choice => choice.targetId === "image/qwen-image-2.1" && choice.availability === "ready")?.targetId
          ?? choices.find(choice => choice.availability === "ready")?.targetId ?? "");
      })
      .catch(err => setError(api.errorMessage(err)));
    return () => { void unlisten.then(dispose => dispose()); };
  }, []);

  async function saveSettings() {
    if (!settings) return;
    setError(null);
    try {
      setSaved(await api.saveConfig(settings));
      setTargets(await api.listOptimizeTargets());
      setSettings(null);
    } catch (err) {
      setError(api.errorMessage(err));
    }
  }

  async function onRun() {
    if (running.current || !selected || selected.availability !== "ready") return;
    if (!source.trim()) {
      setError("请先填写原始提示词。");
      return;
    }
    if (!saved?.providerKind) {
      setSettings(emptyConfig);
      return;
    }
    running.current = true;
    cancelled.current = false;
    setBusy(true);
    setCopied(false);
    setError(null);
    try {
      let next: string;
      if (legacyHost === IMAGE_HOST && (legacyMethod || images.length)) {
        next = (await api.optimizeImage({
          userText: source, methodId: legacyMethod || null, language: saved.language === "en" ? "en" : "zh",
          imagePaths: images,
        })).text;
      } else if (legacyHost === LITERARY_HOST && legacyMethod) {
        next = (await api.optimizeLiterary({
          sourceText: source, creativeMethodId: legacyMethod, language: saved.language === "en" ? "en" : "zh",
        })).text;
      } else {
        next = (await api.optimizePrompt({
          targetId, sourcePrompt: source, expectedMethodDigest: selected.methodDigest ?? undefined,
        })).optimizedPrompt;
      }
      if (!cancelled.current) setResultText(next);
    } catch (err) {
      if (!cancelled.current) setError(api.errorMessage(err));
    } finally {
      running.current = false;
      setBusy(false);
      setJobId(null);
    }
  }

  async function onCancel() {
    if (!jobId) return;
    cancelled.current = true;
    try {
      await api.cancelOptimize(jobId);
    } catch (err) {
      setError(api.errorMessage(err));
    }
  }

  async function onCopy() {
    try {
      await api.copyText(resultText);
      setCopied(true);
    } catch (err) {
      setError(api.errorMessage(err));
    }
  }

  async function showLegacyMethods() {
    if (!legacyHost) return;
    try {
      setLegacyMethods(await api.listSkills(legacyHost));
    } catch (err) {
      setError(api.errorMessage(err));
    }
  }

  async function chooseImages() {
    try {
      setImages(await api.pickImages());
    } catch (err) {
      setError(api.errorMessage(err));
    }
  }

  function selectTarget(next: string) {
    setTargetId(next);
    setLegacyMethods(null);
    setLegacyMethod("");
    setImages([]);
  }

  return (
    <main className="shell">
      <header className="toolbar">
        <UiChromeSelect label="目标模型" labelLayout="inline" value={targetId}
          disabled={busy} onChange={event => selectTarget(event.target.value)}>
          <option value="" disabled>选择目标模型</option>
          {(["image", "video", "literary"] as const).map(kind => (
            <optgroup key={kind} label={{image:"图像", video:"视频", literary:"文学"}[kind]}>
              {targets.filter(target => target.kind === kind).map(target => (
                <option key={target.targetId} value={target.targetId}>
                  {target.label}{target.availability === "ready" ? "" : "（缺少 Skill）"}
                </option>
              ))}
            </optgroup>
          ))}
        </UiChromeSelect>
        <div className="toolbar-actions">
          <Btn type="button" variant="secondary" disabled={busy}
            onClick={() => { setShowSkillSettings(false); setSettings(saved ? publicToInput(saved, "") : emptyConfig); }}>
            设置
          </Btn>
          {busy ? (
            <Btn type="button" variant="secondary" disabled={!jobId} onClick={() => void onCancel()}>取消</Btn>
          ) : (
            <Btn type="button" variant="ai" disabled={selected?.availability !== "ready"}
              onClick={() => void onRun()}>优化提示词</Btn>
          )}
        </div>
      </header>
      {selected && selected.availability !== "ready" && (
        <div className="banner toolbar-actions">
          <span>此目标缺少 Skill。</span>
          <Btn type="button" variant="secondary" onClick={() => {
            setShowSkillSettings(true);
            setSettings(saved ? publicToInput(saved, "") : emptyConfig);
          }}>检查 Skill 更新</Btn>
        </div>
      )}
      {error && !settings && <div className="banner error" role="alert">{error}</div>}
      {legacyHost && (
        <details key={legacyHost} onToggle={event => {
          if (event.currentTarget.open && legacyMethods === null) void showLegacyMethods();
        }}>
          <summary>其他选项</summary>
          <div className="toolbar-actions">
            <UiChromeSelect label="写法 Skill" disabled={busy} value={legacyMethod}
              onChange={event => setLegacyMethod(event.target.value)}>
              <option value="">默认写法</option>
              {legacyMethods?.map(method => <option key={method.stableId} value={method.stableId}>
                {method.displayName || method.name}
              </option>)}
            </UiChromeSelect>
            {legacyHost === IMAGE_HOST && <Btn type="button" variant="secondary" disabled={busy}
              onClick={() => void chooseImages()}>{images.length ? `参考图（${images.length}）` : "选择参考图"}</Btn>}
          </div>
        </details>
      )}
      <div className="panes">
        <section className="pane">
          <label className="pane-heading" htmlFor="prompt-source">原始提示词</label>
          <div className="ui-edge-resize-shell__body prompt-editor">
            <textarea id="prompt-source" className="ui-edit-textarea" value={source}
              onChange={event => setSource(event.target.value)} spellCheck={false} />
          </div>
        </section>
        <section className="pane" aria-busy={busy}>
          <div className="pane-heading">
            <label htmlFor="prompt-result">优化后的提示词</label>
            <Btn type="button" variant="secondary" disabled={!resultText.trim()}
              onClick={() => void onCopy()}>{copied ? "已复制" : "复制"}</Btn>
          </div>
          <div className="ui-edge-resize-shell__body prompt-editor">
            <textarea id="prompt-result" className="ui-edit-textarea" value={resultText}
              onChange={event => { setResultText(event.target.value); setCopied(false); }}
              spellCheck={false} />
          </div>
        </section>
      </div>
      {settings && (
        <SettingsDialog config={settings} channels={channels}
          savedModels={savedServiceMatches ? saved?.savedModels ?? [] : []}
          hasApiKey={Boolean(savedServiceMatches && saved?.hasApiKey)}
          error={error}
          checkSkillsOnOpen={showSkillSettings}
          onSkillsChanged={async () => setTargets(await api.listOptimizeTargets())}
          onChange={setSettings} onSave={() => void saveSettings()}
          onClose={() => { setSettings(null); setError(null); }} />
      )}
    </main>
  );
}
