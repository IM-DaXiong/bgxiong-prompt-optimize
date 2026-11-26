import type React from "react";

/**
 * 排版字号 SSOT — TS 引用层（ui/src/theme/typographyScale.ts）
 *
 * 职责：输出 "var(--text-*)" 字符串，供 TSX inline style / *Styles.ts 消费。
 * 上游：index.css :root --text-*。
 * 下游：业务组件、utility class。
 *
 * INV-TYPE-02：TS = 变量名引用，禁止数字 export 给业务层。
 * INV-TYPE-07：typographyRefs + utility class 为唯一对外 API。
 * INV-TYPE-08：业务用 className="ui-text-sm" 或 style={{ fontSize: typographyRefs.textSm }}。
 * 禁止把 textSm/textMd 等字符串 token 展开进 style（会变成 CSS 数字下标，主窗崩溃）。仅 caption 是 CSSProperties 可展开。
 *
 * 禁止：向此文件导出数字常量（画布屏幕空间走 typographyScreenPx.ts）。
 * 禁止：typographyRefs["2xs"] 式数字 key（用 text2xs 合法标识符）。
 */

export const typographyRefs = {
  text2xs: "var(--text-2xs)",
  textXs: "var(--text-xs)",
  textSm: "var(--text-sm)",
  textMd: "var(--text-md)",
  textLg: "var(--text-lg)",
  textXl: "var(--text-xl)",
  text2xl: "var(--text-2xl)",
  text3xl: "var(--text-3xl)",
  caption: { fontSize: "var(--text-xs)" } as React.CSSProperties,
} as const;

export type TypographySize = keyof typeof typographyRefs;
