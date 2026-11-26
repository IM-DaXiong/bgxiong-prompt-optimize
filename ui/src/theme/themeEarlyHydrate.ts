/**
 * 主题早期水合逻辑（ui/src/theme/themeEarlyHydrate.ts）
 *
 * 职责：在 React bundle 加载前（HTML 内联脚本），读取 localStorage 设置 dataset.theme。
 * 上游：localStorage app_theme。
 * 下游：index.html / assistant.html 的 <head> 内联脚本（同源复制）。
 *
 * 此文件导出函数仅供 ThemeProvider 复用；HTML 内联脚本须手动复制同逻辑。
 */

const STORAGE_KEY = "app_theme";

/** 在 document.documentElement 上设置 dataset.theme（仅非 dark 主题触发） */
export function earlyHydrateTheme(): void {
  try {
    const t = localStorage.getItem(STORAGE_KEY);
    if (t === "light" || t === "sakura" || t === "ocean" || t === "fresh") {
      document.documentElement.dataset.theme = t;
    }
  } catch {
    // localStorage 不可用
  }
}
