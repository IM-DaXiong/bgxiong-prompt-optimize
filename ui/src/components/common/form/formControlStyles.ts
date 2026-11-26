import type { CSSProperties } from "react";
import { typographyRefs } from "../../../theme/typographyScale";

export const UI_FORM_CONTROL_STYLE: CSSProperties = {
  boxSizing: "border-box",
  width: "100%",
  minWidth: 0,
  height: 38,
  padding: 9,
  fontSize: typographyRefs.textMd,
  lineHeight: "18px",
};

export const UI_FORM_FIELD_STYLE: CSSProperties = {
  display: "grid",
  gap: 4,
  minWidth: 0,
};
