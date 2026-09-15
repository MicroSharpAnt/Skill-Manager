import { useEffect, useRef, useState, type HTMLAttributes, type RefObject } from "react";

export function useResourceGroupDrag(container: RefObject<HTMLDivElement>, disabled: boolean,
  move: (id: string, group: string) => void) {
  const [dragging, setDragging] = useState<string | null>(null);
  const [destination, setDestination] = useState<string | null>(null);
  const point = useRef<{ id: string; x: number; y: number; startX: number; startY: number; moved: boolean } | null>(null);
  const suppressed = useRef(false);
  const locate = (x: number, y: number) => {
    const section = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-skill-group]");
    return section && container.current?.contains(section) ? section.dataset.skillGroup ?? null : null;
  };
  function clear() { point.current = null; setDragging(null); setDestination(null); }
  useEffect(() => { if (disabled) clear(); }, [disabled]);
  useEffect(() => {
    if (!dragging) return;
    let frame = 0;
    const tick = () => {
      const p = point.current, node = container.current;
      if (p && node) {
        const rect = node.getBoundingClientRect();
        if (p.x >= rect.left && p.x <= rect.right && p.y >= rect.top - 30 && p.y <= rect.bottom + 30) {
          node.scrollTop += p.y < rect.top + 40 ? -10 : p.y > rect.bottom - 40 ? 10 : 0;
        }
        setDestination(locate(p.x, p.y));
      }
      frame = requestAnimationFrame(tick);
    };
    const cancel = (event: KeyboardEvent) => { if (event.key === "Escape") clear(); };
    frame = requestAnimationFrame(tick);
    document.addEventListener("keydown", cancel); window.addEventListener("blur", clear);
    return () => { cancelAnimationFrame(frame); document.removeEventListener("keydown", cancel); window.removeEventListener("blur", clear); };
  }, [dragging]);
  function rowProps(id: string): HTMLAttributes<HTMLDivElement> {
    return {
      onPointerDown: event => {
        if (event.button !== 0 || disabled) return;
        suppressed.current = false;
        const control = (event.target as Element).closest("button,input,a,label,select,textarea,[role='switch']");
        if (control && !control.matches(".resource-title,.skill-drag-handle")) return;
        event.preventDefault(); event.currentTarget.setPointerCapture(event.pointerId);
        point.current = { id, x: event.clientX, y: event.clientY, startX: event.clientX, startY: event.clientY, moved: false };
      },
      onPointerMove: event => {
        const p = point.current; if (!p) return;
        p.x = event.clientX; p.y = event.clientY;
        if (!p.moved && Math.hypot(p.x - p.startX, p.y - p.startY) < 5) return;
        p.moved = true; suppressed.current = true; setDragging(p.id); setDestination(locate(p.x, p.y));
      },
      onPointerUp: event => {
        const p = point.current, dest = locate(event.clientX, event.clientY); clear();
        if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
        if (p?.moved && dest) move(p.id, dest);
      },
      onPointerCancel: clear, onLostPointerCapture: clear,
      onClickCapture: event => {
        if (suppressed.current) { event.preventDefault(); event.stopPropagation(); suppressed.current = false; }
      },
    };
  }
  return { dragging, destination, rowProps, clear };
}
