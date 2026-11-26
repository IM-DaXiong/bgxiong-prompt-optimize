/**
 * 模块：公共组件 - 按钮（common/button/Btn）
 * 作用：全仓唯一 Btn 实现（INV-UI-BTN-01~07），支持 6 种 variant 及快捷别名。
 *
 * 主要导出：
 * - Btn: 核心按钮组件，支持 variant、disabled、ariaBusy、type、aria-label、shortcutActionId。
 * - PrimaryBtn / SecondaryBtn / DangerBtn: Btn 的快捷别名组件。
 *
 * 【色调语义 · 强制】
 * - variant="ai"（紫）：点击后直接 invoke AI / 创建 AI 任务（如 AI 提示词、批量 AI 提交）。
 * - variant="primary"（蓝）：保存、打开弹层/工作台、弹层内确认/取消、仅收集参数不立即发 AI 的入口。
 *
 * 【INV-UI-BTN-07】shortcutActionId 悬停只显示 chord（fade）；无 id 的 Btn 不挂 settings。
 *
 * 依赖：React, CSSProperties
 */
import {
  forwardRef,
  useCallback,
  useRef,
  useState,
  type ForwardedRef,
  type ReactNode,
  type CSSProperties,
  type MouseEventHandler,
  type PointerEventHandler,
} from "react";
import { typographyRefs } from "../../../theme/typographyScale";
import { useBtnShortcutChordLabel } from "./btnShortcutPort";
import { BtnShortcutHoverTip } from "./BtnShortcutHoverTip";

const VARIANT_STYLES: Record<string, CSSProperties> = {
  primary: {
    background: "var(--primary)",
    color: "var(--primary-fg)",
    fontWeight: 600,
    boxShadow: "0 1px 4px var(--primary-glow)",
  },
  secondary: {
    background: "var(--surface3)",
    color: "var(--text2)",
    border: "1px solid var(--border)",
    boxShadow: "0 1px 3px rgba(0,0,0,0.25)",
  },
  danger: {
    background: "var(--danger-bg)",
    color: "var(--danger)",
    border: "1px solid rgba(239,68,68,0.25)",
  },
  ghost: {
    background: "transparent",
    color: "var(--text3)",
  },
  accent: {
    background: "var(--accent-soft)",
    color: "var(--accent)",
    border: "1px solid rgba(245,158,11,0.3)",
  },
  ai: {
    background: "var(--ai)",
    color: "var(--ai-fg)",
    fontWeight: 600,
    boxShadow: "0 1px 4px var(--ai-glow)",
  },
};

type BtnPropsBase = {
  children: ReactNode;
  variant?: "primary" | "secondary" | "danger" | "ghost" | "accent" | "ai";
  size?: "compact";
  disabled?: boolean;
  style?: CSSProperties;
  /** 原生 title，用于禁用态说明。有有效 shortcut chord 时不设置（INV-UI-BTN-07）。 */
  title?: string;
  ariaBusy?: boolean;
  onPointerDown?: PointerEventHandler<HTMLButtonElement>;
  "aria-label"?: string;
  "aria-pressed"?: boolean;
  "aria-expanded"?: boolean;
  "aria-haspopup"?: "listbox";
  "aria-selected"?: boolean;
  "data-testid"?: string;
  role?: "tab";
  /** shortcutRegistry action id；悬停只显示 chord */
  shortcutActionId?: string;
  shortcutLabel?: string;
  "aria-keyshortcuts"?: string;
  tabIndex?: number;
};

type BtnProps =
  | (BtnPropsBase & {
      type: "submit" | "reset";
      onClick?: MouseEventHandler<HTMLButtonElement>;
    })
  | (BtnPropsBase & {
      type?: "button";
      onClick: MouseEventHandler<HTMLButtonElement>;
    });

const BtnCore = forwardRef<HTMLButtonElement, BtnProps & { nativeTitle?: string }>(
  function BtnCore(
    {
      onClick,
      children,
      variant = "primary",
      size,
      disabled,
      style,
      nativeTitle,
      ariaBusy,
      onPointerDown,
      type = "button",
      "aria-label": ariaLabel,
      "aria-pressed": ariaPressed,
      "aria-expanded": ariaExpanded,
      "aria-haspopup": ariaHasPopup,
      "aria-selected": ariaSelected,
      "data-testid": testId,
      role,
      tabIndex,
      "aria-keyshortcuts": ariaKeyShortcuts,
    },
    ref,
  ) {
    return (
      <button
        ref={ref}
        type={type}
        data-ui-btn-variant={variant}
        onClick={onClick}
        onPointerDown={onPointerDown}
        disabled={disabled}
        title={nativeTitle}
        aria-label={ariaLabel}
        aria-pressed={ariaPressed}
        aria-expanded={ariaExpanded}
        aria-haspopup={ariaHasPopup}
        aria-selected={ariaSelected}
        data-testid={testId}
        role={role}
        tabIndex={tabIndex}
        aria-keyshortcuts={ariaKeyShortcuts}
        aria-busy={ariaBusy === true ? true : undefined}
        style={{
          padding: size === "compact" ? "2px 6px" : "5px 12px",
          flexShrink: size === "compact" ? 0 : undefined,
          borderRadius: "var(--radius)",
          fontSize: typographyRefs.textSm,
          fontWeight: 500,
          whiteSpace: "nowrap",
          opacity: disabled ? 0.5 : 1,
          cursor: disabled ? "not-allowed" : "pointer",
          ...VARIANT_STYLES[variant],
          ...style,
        }}
      >
        {children}
      </button>
    );
  },
);

function BtnWithShortcutHover(
  props: BtnProps & { shortcutActionId?: string; forwardedRef: ForwardedRef<HTMLButtonElement> },
) {
  const { shortcutActionId, shortcutLabel, forwardedRef, title, ...rest } = props;
  const registeredLabel = useBtnShortcutChordLabel(shortcutActionId);
  const chordLabel = shortcutLabel ?? registeredLabel;
  const hostRef = useRef<HTMLSpanElement | null>(null);
  const [hover, setHover] = useState(false);
  const [focused, setFocused] = useState(false);
  const dismiss = useCallback(() => { setHover(false); setFocused(false); }, []);

  return (
    <span
      ref={hostRef}
      style={{ display: "inline-flex", verticalAlign: "middle" }}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      onFocus={() => setFocused(true)}
      onBlur={() => setFocused(false)}
    >
      <BtnCore
        ref={forwardedRef}
        {...rest}
        nativeTitle={chordLabel ? undefined : title}
      />
      {chordLabel ? (
        <BtnShortcutHoverTip hostRef={hostRef} label={chordLabel} open={hover || focused} onDismiss={dismiss} />
      ) : null}
    </span>
  );
}

export const Btn = forwardRef<HTMLButtonElement, BtnProps>(function Btn(props, ref) {
  const { shortcutActionId, title, ...rest } = props;
  if (!shortcutActionId && !rest.shortcutLabel) {
    return <BtnCore ref={ref} {...rest} nativeTitle={title} />;
  }
  return (
    <BtnWithShortcutHover
      {...rest}
      title={title}
      shortcutActionId={shortcutActionId}
      forwardedRef={ref}
    />
  );
});

export const PrimaryBtn = ({
  onClick,
  children,
  disabled,
  title,
  style,
  type = "button",
}: {
  onClick: MouseEventHandler<HTMLButtonElement>;
  children: ReactNode;
  disabled?: boolean;
  title?: string;
  style?: CSSProperties;
  type?: "button" | "submit" | "reset";
}) => (
  <Btn onClick={onClick} disabled={disabled} title={title} variant="primary" style={style} type={type}>
    {children}
  </Btn>
);

export const SecondaryBtn = ({
  onClick,
  children,
  disabled,
  title,
  style,
  type = "button",
}: {
  onClick: MouseEventHandler<HTMLButtonElement>;
  children: ReactNode;
  disabled?: boolean;
  title?: string;
  style?: CSSProperties;
  type?: "button" | "submit" | "reset";
}) => (
  <Btn onClick={onClick} disabled={disabled} title={title} variant="secondary" style={style} type={type}>
    {children}
  </Btn>
);

export const DangerBtn = ({
  onClick,
  children,
  style,
  type = "button",
}: {
  onClick: MouseEventHandler<HTMLButtonElement>;
  children: ReactNode;
  style?: CSSProperties;
  type?: "button" | "submit" | "reset";
}) => (
  <Btn onClick={onClick} variant="danger" style={style} type={type}>
    {children}
  </Btn>
);
