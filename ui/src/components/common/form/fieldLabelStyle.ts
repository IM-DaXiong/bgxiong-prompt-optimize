/**
 * Field-name / section-heading label SSOT (INV-THEME).
 * Use for form field titles, workbench column titles, and fontWeight 600 section headings.
 * Hints / empty states / timestamps stay --text3 / --text4 (no heavy fontWeight).
 */
import type { CSSProperties } from "react";
import { typographyRefs } from "../../../theme/typographyScale";

export const UI_FIELD_LABEL_STYLE: CSSProperties = {
  fontSize: typographyRefs.textSm,
  fontWeight: 600,
  color: "var(--text1)",
  lineHeight: "16px",
};
