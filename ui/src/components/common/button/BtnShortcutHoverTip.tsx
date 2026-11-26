/**
 * INV-UI-BTN-07: body-portal shortcut hover chip (fade in/out, chord text only).
 */
import { useEffect, useLayoutEffect, useState, type RefObject } from "react";
import { createPortal } from "react-dom";
import { MODAL_Z_INDEX } from "../../modal/modalZIndex";
import { typographyRefs } from "../../../theme/typographyScale";

export const SHORTCUT_HOVER_TIP_FADE_MS = 140;

export function BtnShortcutHoverTip(props: {
  hostRef: RefObject<HTMLElement | null>;
  label: string;
  open: boolean;
  onDismiss: () => void;
}) {
  const { hostRef, label, open, onDismiss } = props;
  const [present, setPresent] = useState(open);
  const [opaque, setOpaque] = useState(false);
  const [box, setBox] = useState<{ left: number; top: number } | null>(null);

  useLayoutEffect(() => {
    if (!open) return;
    const el = hostRef.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    setBox({ left: r.left + r.width / 2, top: r.top });
  }, [open, hostRef, label]);

  useEffect(() => {
    if (open) {
      setPresent(true);
      const id = window.requestAnimationFrame(() => setOpaque(true));
      return () => window.cancelAnimationFrame(id);
    }
    setOpaque(false);
    const t = window.setTimeout(() => setPresent(false), SHORTCUT_HOVER_TIP_FADE_MS);
    return () => window.clearTimeout(t);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const hide = () => onDismiss();
    window.addEventListener("scroll", hide, true);
    window.addEventListener("resize", hide);
    return () => {
      window.removeEventListener("scroll", hide, true);
      window.removeEventListener("resize", hide);
    };
  }, [open, onDismiss]);

  if (!present || !box || typeof document === "undefined") return null;

  return createPortal(
    <span
      role="tooltip"
      aria-hidden
      style={{
        position: "fixed",
        left: box.left,
        top: box.top,
        transform: "translate(-50%, calc(-100% - 6px))",
        opacity: opaque ? 1 : 0,
        transition: `opacity ${SHORTCUT_HOVER_TIP_FADE_MS}ms ease-out`,
        pointerEvents: "none",
        zIndex: MODAL_Z_INDEX.SHORTCUT_HOVER_TIP,
        padding: "4px 8px",
        borderRadius: "var(--radius)",
        border: "1px solid var(--border)",
        background: "var(--surface)",
        color: "var(--text1)",
        fontSize: typographyRefs.textSm,
        fontWeight: 500,
        whiteSpace: "nowrap",
        boxShadow: "var(--shadow-md)",
      }}
    >
      {label}
    </span>,
    hostRef.current?.closest('dialog[open]') ?? document.body,
  );
}
