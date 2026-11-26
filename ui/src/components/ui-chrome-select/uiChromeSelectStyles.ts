/**
 * UiChromeSelect — 原生 select 视觉 SSOT（全站工作台 / 生成参数 / 产品语言共用）。
 */

import type React from "react";
import { typographyRefs } from "../../theme/typographyScale";
import { UI_FIELD_LABEL_STYLE } from "../common/form/fieldLabelStyle";

/** 与 select 对齐的控件统一高度 */
export const UI_CHROME_SELECT_CONTROL_HEIGHT_PX = 28;

/** 顶栏/工具条 Btn 与 UiChromeSelect 同高（Clips/Voice/PanelHeader.actions 正例） */
export const UI_TOOLBAR_CONTROL_HEIGHT_PX = UI_CHROME_SELECT_CONTROL_HEIGHT_PX;

export const UI_PANEL_HEADER_BTN_STYLE: React.CSSProperties = {
  fontSize: typographyRefs.textSm,
  padding: "4px 12px",
  height: UI_TOOLBAR_CONTROL_HEIGHT_PX,
  lineHeight: "18px",
  boxSizing: "border-box",
};

/** select 基础样式（不含 enabled/disabled 背景） */
export const UI_CHROME_SELECT_BASE_STYLE: React.CSSProperties = {
  width: "100%",
  padding: "4px 8px",
  borderRadius: 4,
  border: "1px solid var(--border)",
  fontSize: typographyRefs.textSm,
  lineHeight: "18px",
  boxSizing: "border-box",
  minHeight: UI_CHROME_SELECT_CONTROL_HEIGHT_PX,
};

/** 字段标题 */
export const UI_CHROME_SELECT_LABEL_STYLE: React.CSSProperties = {
  ...UI_FIELD_LABEL_STYLE,
};

/** 同排 flex 列壳（可折行） */
export const UI_CHROME_SELECT_FIELD_COL_STYLE: React.CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 3,
  flex: "1 1 120px",
  minWidth: 110,
  maxWidth: "100%",
  boxSizing: "border-box",
};

/** 同排 grid 列壳（minWidth 0，等分窄列） */
export const UI_CHROME_SELECT_INLINE_FIELD_COL_STYLE: React.CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 3,
  minWidth: 0,
  maxWidth: "100%",
  boxSizing: "border-box",
};

/** Workbench: label left, control right (field wraps as a unit). */
export const UI_CHROME_SELECT_LABEL_INLINE_FIELD_COL_STYLE: React.CSSProperties = {
  display: "flex",
  flexDirection: "row",
  alignItems: "center",
  gap: 8,
  flex: "1 1 160px",
  minWidth: 140,
  maxWidth: "100%",
  boxSizing: "border-box",
};

export const UI_CHROME_SELECT_LABEL_INLINE_STYLE: React.CSSProperties = {
  ...UI_CHROME_SELECT_LABEL_STYLE,
  whiteSpace: "nowrap",
  flexShrink: 0,
};

/** select 完整样式（含 disabled 态）；固定 height 与 number 等单行控件对齐（禁写入 BASE，只读折行框需可增高） */
export function uiChromeSelectStyle(disabled?: boolean): React.CSSProperties {
  return {
    ...UI_CHROME_SELECT_BASE_STYLE,
    height: UI_CHROME_SELECT_CONTROL_HEIGHT_PX,
    background: disabled ? "var(--surface2)" : "var(--surface)",
    color: "var(--text1)",
    opacity: disabled ? 0.6 : 1,
  };
}

/** 只读值框（与 select 同高，用于模型固定分辨率等；长文案自动折行） */
export function uiChromeSelectReadonlyValueStyle(): React.CSSProperties {
  return {
    ...UI_CHROME_SELECT_BASE_STYLE,
    display: "block",
    whiteSpace: "normal",
    wordBreak: "break-word",
    overflowWrap: "anywhere",
    lineHeight: 1.4,
    background: "var(--surface2)",
    color: "var(--text1)",
  };
}

/** 带边框控件壳（滑杆、只读值行内壳） */
export const UI_CHROME_SELECT_INSET_SHELL_STYLE: React.CSSProperties = {
  ...UI_CHROME_SELECT_BASE_STYLE,
  display: "flex",
  alignItems: "center",
  gap: 6,
  padding: "0 8px",
  background: "var(--surface)",
};
