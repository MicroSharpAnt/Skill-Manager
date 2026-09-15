import { useEffect, useRef, type ReactNode } from "react";
import { X } from "lucide-react";
import { usePanelWidth } from "./PanelWidth";

export function Dialog({
  title,
  children,
  close,
  wide = false,
  resizable = false,
}: {
  title: string;
  children: ReactNode;
  close: () => void;
  wide?: boolean;
  resizable?: boolean;
}) {
  const panelWidth = usePanelWidth("skill-manager-detail-dialog-width", 920, true);
  const ref = useRef<HTMLDivElement>(null);
  const closeRef = useRef(close);
  closeRef.current = close;
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    const node = ref.current;
    const focusable = () =>
      [
        ...(node?.querySelectorAll<HTMLElement>(
          'button,input,select,textarea,summary,a[href],[tabindex="0"]',
        ) ?? []),
      ].filter(
        (el) =>
          !el.matches(":disabled,[hidden]") && el.getClientRects().length > 0,
      );
    const topmost = () =>
      [...document.querySelectorAll('[role="dialog"]')]
        .filter((el) => el.getClientRects().length > 0)
        .at(-1) === node;
    (
      focusable().find((el) =>
        el.matches('input:not([type="checkbox"]),textarea'),
      ) ??
      focusable()[0] ??
      node
    )?.focus();
    const listener = (e: KeyboardEvent) => {
      if (!topmost()) return;
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        closeRef.current();
      }
      if (e.key === "Tab") {
        const all = focusable();
        const first = all[0],
          last = all.at(-1);
        if (!all.length) {
          e.preventDefault();
          node?.focus();
        } else if (
          e.shiftKey &&
          (document.activeElement === first ||
            !node?.contains(document.activeElement))
        ) {
          e.preventDefault();
          last?.focus();
        } else if (
          !e.shiftKey &&
          (document.activeElement === last ||
            !node?.contains(document.activeElement))
        ) {
          e.preventDefault();
          first?.focus();
        }
      }
    };
    const containFocus = (e: FocusEvent) => {
      if (topmost() && !node?.contains(e.target as Node))
        (focusable()[0] ?? node)?.focus();
    };
    document.addEventListener("keydown", listener);
    document.addEventListener("focusin", containFocus);
    return () => {
      document.removeEventListener("keydown", listener);
      document.removeEventListener("focusin", containFocus);
      if (previous?.isConnected && previous.getClientRects().length)
        previous.focus();
    };
  }, []);
  return (
    <div
      className="modal-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) closeRef.current();
      }}
    >
      <div
        ref={ref}
        className={"modal global-modal " + (wide ? "wide" : "")}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        tabIndex={-1}
        style={resizable ? panelWidth.style : undefined}
      >
        <header>
          <h2>{title}</h2>
          <button aria-label="关闭对话框" onClick={close}>
            <X size={18} />
          </button>
        </header>
        {children}
        {resizable && panelWidth.handle}
      </div>
    </div>
  );
}
