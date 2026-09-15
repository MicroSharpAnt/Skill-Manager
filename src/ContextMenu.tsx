import { useLayoutEffect, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";
import "./context-menu.css";

export function ContextMenu({ x, y, title, close, children }: {
  x: number; y: number; title: string; close: () => void; children: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const closeRef = useRef(close);
  closeRef.current = close;
  useLayoutEffect(() => {
    const node = ref.current!;
    const rect = node.getBoundingClientRect();
    node.style.left = `${Math.max(8, Math.min(x, window.innerWidth - rect.width - 8))}px`;
    node.style.top = `${Math.max(8, Math.min(y, window.innerHeight - rect.height - 8))}px`;
    node.querySelector<HTMLElement>("input,button:not(:disabled)")?.focus();
    const outside = (event: PointerEvent) => {
      if (!node.contains(event.target as Node)) closeRef.current();
    };
    const dismiss = () => closeRef.current();
    const scroll = (event: Event) => {
      if (!node.contains(event.target as Node)) dismiss();
    };
    document.addEventListener("pointerdown", outside);
    document.addEventListener("scroll", scroll, true);
    window.addEventListener("resize", dismiss);
    window.addEventListener("blur", dismiss);
    return () => {
      document.removeEventListener("pointerdown", outside);
      document.removeEventListener("scroll", scroll, true);
      window.removeEventListener("resize", dismiss);
      window.removeEventListener("blur", dismiss);
    };
  }, [x, y]);
  return createPortal(<div ref={ref} className="skill-context-menu" role="dialog"
    aria-label={title} style={{ left: x, top: y }}
    onContextMenu={event => event.preventDefault()}
    onKeyDown={event => {
      if (event.key === "Escape") { event.stopPropagation(); close(); }
    }} onBlur={event => {
      if (event.relatedTarget && !event.currentTarget.contains(event.relatedTarget)) close();
    }}>
    <div className="context-title">{title}</div>
    {children}
  </div>, document.body);
}
