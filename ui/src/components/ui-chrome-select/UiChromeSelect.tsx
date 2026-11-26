/**
 * UiChromeSelect — 统一原生 select 外观（与 UiChromeTextarea 同命名族）。
 *
 * 业务层只组装 option/optgroup children；禁止再写内联 select style。
 */
import { useId, type CSSProperties, type ReactNode, type SelectHTMLAttributes } from "react";
import { UI_FORM_CONTROL_STYLE, UI_FORM_FIELD_STYLE } from "../common/form/formControlStyles";
import {
  UI_CHROME_SELECT_FIELD_COL_STYLE,
  UI_CHROME_SELECT_LABEL_INLINE_FIELD_COL_STYLE,
  UI_CHROME_SELECT_LABEL_INLINE_STYLE,
  UI_CHROME_SELECT_LABEL_STYLE,
  uiChromeSelectStyle,
} from "./uiChromeSelectStyles";

export type UiChromeSelectLabelLayout = "stacked" | "inline";

export type UiChromeSelectProps = Omit<SelectHTMLAttributes<HTMLSelectElement>, "children"> & {
  /** 字段标题；hideLabel 时用作 aria-label */
  label: string;
  /** 工作台同排等场景：不渲染可见 label */
  hideLabel?: boolean;
  /** stacked = label above control (default); inline = label left of control */
  labelLayout?: UiChromeSelectLabelLayout;
  /** Use the full-height form control geometry in settings and dialogs. */
  controlSize?: "compact" | "form";
  /** 列壳样式；默认随 labelLayout */
  fieldColStyle?: CSSProperties;
  /** 悬停提示挂在列壳上（disabled select 仍可显示） */
  fieldTitle?: string;
  children: ReactNode;
};

export function UiChromeSelect({
  label,
  hideLabel = false,
  labelLayout = "stacked",
  controlSize = "compact",
  fieldColStyle,
  fieldTitle,
  disabled,
  id,
  style,
  children,
  ...selectProps
}: UiChromeSelectProps) {
  const generatedId = useId();
  const selectId = id ?? generatedId;
  const inline = labelLayout === "inline";
  const formSize = controlSize === "form";
  const mergedStyle: CSSProperties = {
    ...(formSize ? { ...UI_FORM_CONTROL_STYLE, paddingRight: 28, opacity: disabled ? 0.6 : 1 } : uiChromeSelectStyle(disabled)),
    ...(inline ? { flex: "1 1 auto", minWidth: 0, width: "auto" } : null),
    ...style,
  };

  const selectEl = (
    <select
      {...selectProps}
      id={selectId}
      disabled={disabled}
      data-ui-form-select={formSize ? "true" : undefined}
      aria-label={hideLabel ? label : selectProps["aria-label"]}
      style={mergedStyle}
    >
      {children}
    </select>
  );

  if (hideLabel) {
    return (
      <div style={fieldColStyle ?? { minWidth: 0, width: "100%" }} title={fieldTitle}>
        {selectEl}
      </div>
    );
  }

  return (
    <div
      style={
        fieldColStyle ??
        (inline ? UI_CHROME_SELECT_LABEL_INLINE_FIELD_COL_STYLE : formSize ? UI_FORM_FIELD_STYLE : UI_CHROME_SELECT_FIELD_COL_STYLE)
      }
      title={fieldTitle}
    >
      <label htmlFor={selectId} style={inline ? UI_CHROME_SELECT_LABEL_INLINE_STYLE : UI_CHROME_SELECT_LABEL_STYLE}>
        {label}
      </label>
      {selectEl}
    </div>
  );
}
