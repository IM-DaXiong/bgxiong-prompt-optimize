/**
 * Video target / compress fields stay in the reconstructed backend API.
 * Current standalone user entry does not mount this form.
 */
import type { ReferenceItem, VideoProfileDto, VideoTargetForm as Target } from "./types";
import { REFERENCE_USAGES } from "./types";

type Props = {
  profiles: VideoProfileDto[];
  profilesError: string | null;
  target: Target | null;
  onChange: (next: Target) => void;
  references: ReferenceItem[];
  onReferencesChange: (next: ReferenceItem[]) => void;
  onPickReferences: () => void;
  durationText: string;
  onDurationText: (value: string) => void;
};

function fileName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] ?? path;
}

export function missingMedia(profile: VideoProfileDto | undefined, inputMode: string, references: ReferenceItem[]): string[] {
  const mode = profile?.inputModes.find((item) => item.id === inputMode);
  if (!mode) {
    return [];
  }
  const present = new Set(references.map((item) => item.mediaKind));
  return mode.requiredMedia.filter((kind) => !present.has(kind));
}

export default function VideoTargetForm(props: Props) {
  if (props.profilesError) {
    return (
      <div className="banner error" role="alert">
        {props.profilesError}
      </div>
    );
  }
  if (props.profiles.length === 0) {
    return (
      <p className="muted">
        还没有从核心注册表拿到档案列表。档案名单不会在界面里手写备份。
      </p>
    );
  }

  const selected = props.profiles.find(
    (item) =>
      props.target !== null &&
      item.profileId === props.target.profileId &&
      item.profileVersion === props.target.profileVersion,
  );
  const modes = selected?.inputModes ?? [];
  const missing = selected && props.target ? missingMedia(selected, props.target.inputMode, props.references) : [];

  return (
    <div className="stack">
      <label>
        目标档案
        <select
          value={props.target ? `${props.target.profileId}@${props.target.profileVersion}` : ""}
          onChange={(event) => {
            const key = event.target.value;
            const found = props.profiles.find((item) => `${item.profileId}@${item.profileVersion}` === key);
            if (!found) {
              return;
            }
            const firstMode = found.inputModes[0]?.id ?? "";
            props.onChange({
              profileId: found.profileId,
              profileVersion: found.profileVersion,
              inputMode: firstMode,
              durationSeconds: parseDuration(props.durationText),
              expressionLanguage: props.target?.expressionLanguage ?? "zh",
              referenceUseIds: props.references.map((_, index) => `ref-${index + 1}`),
            });
          }}
        >
          <option value="">请选择档案</option>
          {props.profiles.map((item) => (
            <option key={`${item.profileId}@${item.profileVersion}`} value={`${item.profileId}@${item.profileVersion}`}>
              {item.displayKey || `${item.profileId}@${item.profileVersion}`}
            </option>
          ))}
        </select>
      </label>

      <label>
        输入模式
        <select
          value={props.target?.inputMode ?? ""}
          disabled={!selected}
          onChange={(event) => {
            if (!props.target) {
              return;
            }
            props.onChange({ ...props.target, inputMode: event.target.value });
          }}
        >
          <option value="">请选择模式</option>
          {modes.map((mode) => (
            <option key={mode.id} value={mode.id}>
              {mode.id}
            </option>
          ))}
        </select>
      </label>

      <label>
        时长（秒，可空）
        <input
          value={props.durationText}
          placeholder="空白表示不填，不会改成默认秒数"
          onChange={(event) => {
            props.onDurationText(event.target.value);
            if (!props.target) {
              return;
            }
            props.onChange({
              ...props.target,
              durationSeconds: parseDuration(event.target.value),
            });
          }}
        />
      </label>

      <div className="row-head">
        <span>参考素材</span>
        <button type="button" className="btn" onClick={props.onPickReferences}>
          选择文件
        </button>
      </div>
      {props.references.length === 0 ? (
        <p className="muted">还没有选择参考文件。需要参考素材的模式会在此列出缺少的类型。</p>
      ) : (
        <ul className="ref-list">
          {props.references.map((item, index) => (
            <li key={`${item.path}-${index}`}>
              <span className="file" title={item.path}>
                {fileName(item.path)}
              </span>
              <select
                value={item.usage}
                onChange={(event) => {
                  const usage = event.target.value;
                  const meta = REFERENCE_USAGES.find((entry) => entry.id === usage);
                  const next = props.references.slice();
                  next[index] = {
                    ...item,
                    usage,
                    mediaKind: meta?.mediaKind ?? usage,
                  };
                  props.onReferencesChange(next);
                }}
              >
                {REFERENCE_USAGES.map((entry) => (
                  <option key={entry.id} value={entry.id}>
                    {entry.label}
                  </option>
                ))}
              </select>
              <button
                type="button"
                className="btn"
                onClick={() => {
                  const next = props.references.filter((_, i) => i !== index);
                  props.onReferencesChange(next);
                }}
              >
                移除
              </button>
            </li>
          ))}
        </ul>
      )}

      {missing.length > 0 ? (
        <p className="banner error" role="status">
          当前模式还缺少：{missing.join("、")}。请先用文件框补齐后再开始优化。
        </p>
      ) : null}
    </div>
  );
}

export function parseDuration(raw: string): number | null {
  const trimmed = raw.trim();
  if (trimmed === "") {
    return null;
  }
  const value = Number(trimmed);
  if (!Number.isFinite(value) || value <= 0) {
    return Number.NaN;
  }
  return value;
}

export function durationError(raw: string): string | null {
  const trimmed = raw.trim();
  if (trimmed === "") {
    return null;
  }
  const value = Number(trimmed);
  if (!Number.isFinite(value) || value <= 0) {
    return "时长必须是大于 0 的数字，空白表示不填，不会改成默认秒数。";
  }
  return null;
}
