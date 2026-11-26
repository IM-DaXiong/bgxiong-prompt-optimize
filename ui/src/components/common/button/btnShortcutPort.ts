/**
 * Optional shortcut-chord hook for Btn hover tips.
 *
 * The display button itself does not import i18n or app settings. The main
 * program registers `useShortcutChordLabel` via `installMainRichPlainHost`.
 * Standalone apps leave this unregistered, so no chord tip is shown.
 */
export type BtnShortcutChordHook = (actionId: string | undefined) => string | null;

export function useNoBtnShortcutChord(_actionId: string | undefined): string | null {
  return null;
}

let registered: BtnShortcutChordHook | null = null;

export function registerBtnShortcutChordHook(hook: BtnShortcutChordHook): void {
  registered = hook;
}

export function useBtnShortcutChordLabel(actionId: string | undefined): string | null {
  const hook = registered ?? useNoBtnShortcutChord;
  return hook(actionId);
}
