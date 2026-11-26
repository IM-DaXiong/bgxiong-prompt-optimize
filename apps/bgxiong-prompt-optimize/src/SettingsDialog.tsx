/**
 * Model BaseURL / Key / timeout / paths / official maxOutputTokens live here,
 * not on the main two-pane surface.
 */
import { useState, type ChangeEvent, type FormEvent } from "react";
import * as api from "./api";
import { Btn, UiChromeSelect } from "./uiChrome";
import type { ShellConfigInput, SkillCandidate, TextChannel } from "./types";

type SettingsDialogProps = {
  config: ShellConfigInput;
  channels: TextChannel[];
  hasApiKey: boolean;
  savedModels: string[];
  error: string | null;
  checkSkillsOnOpen: boolean;
  onSkillsChanged: () => Promise<void>;
  onChange: (next: ShellConfigInput) => void;
  onSave: () => void;
  onClose: () => void;
};

export default function SettingsDialog({
  config,
  channels,
  hasApiKey,
  savedModels,
  error,
  checkSkillsOnOpen,
  onSkillsChanged,
  onChange,
  onSave,
  onClose,
}: SettingsDialogProps) {
  const [addModel, setAddModel] = useState(false);
  const [candidates, setCandidates] = useState<SkillCandidate[] | null>(null);
  const [checking, setChecking] = useState(false);
  const [skillError, setSkillError] = useState<string | null>(null);
  const selectedChannel = channels.find(channel => channel.providerKind === config.providerKind);
  const customModel = addModel || (config.model !== "" && !savedModels.includes(config.model));

  async function checkSkills() {
    setChecking(true);
    setSkillError(null);
    try {
      setCandidates(await api.checkPromptSkills());
      await onSkillsChanged();
    } catch (err) {
      setSkillError(api.errorMessage(err));
    } finally {
      setChecking(false);
    }
  }

  async function installSkill(candidate: SkillCandidate) {
    setChecking(true);
    setSkillError(null);
    try {
      await api.installPromptSkill(candidate.candidateId, candidate.needsConfirm);
      setCandidates(current => current?.filter(item => item.candidateId !== candidate.candidateId) ?? []);
      await onSkillsChanged();
    } catch (err) {
      setSkillError(api.errorMessage(err));
    } finally {
      setChecking(false);
    }
  }

  function patch(partial: Partial<ShellConfigInput>) {
    onChange({ ...config, ...partial });
  }

  function handleSubmit(event: FormEvent) {
    event.preventDefault();
    onSave();
  }

  return (
    <div className="settings-backdrop" role="presentation" onClick={onClose}>
      <form
        className="settings-dialog"
        role="dialog"
        aria-labelledby="settings-title"
        onClick={(event) => event.stopPropagation()}
        onSubmit={handleSubmit}
      >
        <h2 id="settings-title">设置</h2>
        {checkSkillsOnOpen && skillUpdates()}
        <UiChromeSelect
          label="文本服务"
          value={config.providerKind}
          onChange={(event: ChangeEvent<HTMLSelectElement>) => {
            setAddModel(false);
            patch({
              providerKind: event.target.value,
              model: "",
              baseUrl: "",
              apiKey: "",
              maxOutputTokens: "",
            });
          }}
        >
          <option value=""> </option>
          {channels.map((item) => (
            <option key={item.providerKind} value={item.providerKind}>
              {item.label}
            </option>
          ))}
        </UiChromeSelect>
        {selectedChannel?.group !== "local" && (
          <>
            <UiChromeSelect label="文本模型"
              value={customModel ? "new" : config.model ? "model:" + config.model : ""}
              onChange={event => {
                const option = event.target.value;
                setAddModel(option === "new");
                patch({ model: option.startsWith("model:") ? option.slice(6) : "" });
              }}>
              <option value="" disabled>选择文本模型</option>
              {savedModels.map(model => <option key={model} value={"model:" + model}>{model}</option>)}
              <option value="new">输入模型名称…</option>
            </UiChromeSelect>
            {customModel && <label>
              模型名称
              <input value={config.model} onChange={event => patch({ model: event.target.value })} />
            </label>}
          </>
        )}
        {selectedChannel?.needsBaseUrl && <>
        <label>
          BaseURL
          <input
            value={config.baseUrl}
            onChange={(event) => patch({ baseUrl: event.target.value })}
          />
        </label>
        <label>
          Key{hasApiKey ? "（已保存）" : ""}
          <input
            type="password"
            autoComplete="off"
            value={config.apiKey}
            onChange={(event) => patch({ apiKey: event.target.value })}
          />
        </label>
        </>}
        {error && <div className="banner error" role="alert">{error}</div>}
        <label>
          超时秒数
          <input
            inputMode="numeric"
            value={config.timeoutSecs}
            onChange={(event) => patch({ timeoutSecs: event.target.value })}
          />
        </label>
        <label>
          官方最大输出(maxOutputTokens)
          <input
            inputMode="numeric"
            value={config.maxOutputTokens}
            onChange={(event) => patch({ maxOutputTokens: event.target.value })}
          />
        </label>
        <label>
          本地模型路径
          <input
            value={config.localModelPath}
            onChange={(event) => patch({ localModelPath: event.target.value })}
          />
        </label>
        <label>
          适配包目录
          <input
            value={config.adapterPackageRoot}
            onChange={(event) => patch({ adapterPackageRoot: event.target.value })}
          />
        </label>
        <label>
          技能库根
          <input
            value={config.skillLibraryRoot}
            onChange={(event) => patch({ skillLibraryRoot: event.target.value })}
          />
        </label>
        <label>
          内部模板根
          <input
            value={config.internalTemplateRoot}
            onChange={(event) => patch({ internalTemplateRoot: event.target.value })}
          />
        </label>
        <UiChromeSelect
          label="语言"
          value={config.language}
          onChange={(event: ChangeEvent<HTMLSelectElement>) =>
            patch({ language: event.target.value === "en" ? "en" : "zh" })
          }
        >
          <option value="zh">zh</option>
          <option value="en">en</option>
        </UiChromeSelect>
        <label className="check">
          <input
            type="checkbox"
            checked={config.supportsVision}
            onChange={(event) => patch({ supportsVision: event.target.checked })}
          />
          看图
        </label>
        {!checkSkillsOnOpen && skillUpdates()}
        <div className="settings-actions">
          <Btn type="button" variant="secondary" onClick={onClose}>
            关闭
          </Btn>
          <Btn type="submit" variant="primary">
            保存
          </Btn>
        </div>
      </form>
    </div>
  );

  function skillUpdates() {
    return (
      <section aria-label="Skill 更新">
        <Btn type="button" variant="secondary" disabled={checking} onClick={() => void checkSkills()}>
          {checking ? "正在检查…" : "检查 Skill 更新"}
        </Btn>
        {skillError && <div className="banner error" role="alert">{skillError}</div>}
        {candidates?.length === 0 && !checking && <div role="status">没有可安装的更新</div>}
        {candidates?.map(candidate => (
          <div className="toolbar-actions" key={candidate.candidateId}>
            <span>{candidate.displayName} · {candidate.version}</span>
            <Btn type="button" variant="secondary" disabled={checking}
              onClick={() => void installSkill(candidate)}>{candidate.needsConfirm ? "安装" : "更新"}</Btn>
          </div>
        ))}
      </section>
    );
  }
}
