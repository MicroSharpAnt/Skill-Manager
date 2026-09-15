import { useCallback, useEffect, useRef, useState, type ButtonHTMLAttributes, type RefObject } from "react";

type Destination = { id: string; side: "before" | "after" };

// Pointer capture works in the desktop WebView as well as with touch input.
export function useGroupOrderDrag(
  matrix: RefObject<HTMLDivElement>,
  ids: string[],
  disabled: boolean,
  save: (ids: string[]) => void,
) {
  const [dragging, setDragging] = useState<string | null>(null);
  const [destination, setDestination] = useState<Destination | null>(null);
  const pointer = useRef<{ id: string; startX: number; startY: number; x: number; y: number; moved: boolean } | null>(null);
  const clear = useCallback(() => {
    pointer.current = null;
    setDragging(null);
    setDestination(null);
  }, []);
  const locate = useCallback((x: number, y: number): Destination | null => {
    const section = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-skill-group]");
    const header = section?.querySelector(".skill-group-row");
    if (!section || !header || !matrix.current?.contains(section)) return null;
    const id = section.dataset.skillGroup!;
    if (id === "ungrouped") {
      const last = matrix.current.querySelectorAll<HTMLElement>("[data-skill-group]:not([data-skill-group='ungrouped'])");
      return last.length ? { id: last[last.length - 1].dataset.skillGroup!, side: "after" } : null;
    }
    const rect = header.getBoundingClientRect();
    return { id, side: y < rect.top + rect.height / 2 ? "before" : "after" };
  }, [matrix]);
  function reorder(id: string, target: Destination | null) {
    if (!target || target.id === id || !ids.includes(id) || !ids.includes(target.id)) return;
    const next = ids.filter(value => value !== id);
    next.splice(next.indexOf(target.id) + (target.side === "after" ? 1 : 0), 0, id);
    if (next.some((value, index) => value !== ids[index])) save(next);
  }
  useEffect(() => {
    if (!dragging) return;
    let frame = 0;
    const scroll = () => {
      const point = pointer.current, container = matrix.current;
      if (point && container) {
        const rect = container.getBoundingClientRect();
        if (point.x >= rect.left && point.x <= rect.right && point.y >= rect.top - 30 && point.y <= rect.bottom + 30) {
          const speed = point.y < rect.top + 40 ? -10 : point.y > rect.bottom - 40 ? 10 : 0;
          if (speed) container.scrollTop += speed;
        }
        const target = locate(point.x, point.y);
        setDestination(previous => previous?.id === target?.id && previous?.side === target?.side ? previous : target);
      }
      frame = requestAnimationFrame(scroll);
    };
    const cancel = (event: KeyboardEvent) => { if (event.key === "Escape") clear(); };
    frame = requestAnimationFrame(scroll);
    document.addEventListener("keydown", cancel);
    window.addEventListener("blur", clear);
    return () => {
      cancelAnimationFrame(frame);
      document.removeEventListener("keydown", cancel);
      window.removeEventListener("blur", clear);
    };
  }, [dragging, matrix, locate, clear]);
  useEffect(() => { if (disabled) clear(); }, [disabled, clear]);

  function handleProps(id: string, name: string): ButtonHTMLAttributes<HTMLButtonElement> {
    return {
      disabled,
      "aria-label": `调整 ${name} 分组顺序`,
      title: "拖动调整分组顺序，也可使用上下方向键",
      onClick: event => event.stopPropagation(),
      onPointerDown: event => {
        if (disabled || event.button !== 0) return;
        event.preventDefault();
        event.currentTarget.focus({ preventScroll: true });
        event.currentTarget.setPointerCapture(event.pointerId);
        pointer.current = { id, startX: event.clientX, startY: event.clientY, x: event.clientX, y: event.clientY, moved: false };
      },
      onPointerMove: event => {
        const point = pointer.current;
        if (!point) return;
        point.x = event.clientX; point.y = event.clientY;
        if (!point.moved && Math.hypot(point.x - point.startX, point.y - point.startY) < 5) return;
        point.moved = true;
        setDragging(point.id);
        setDestination(locate(point.x, point.y));
      },
      onPointerUp: event => {
        const point = pointer.current;
        const target = locate(event.clientX, event.clientY);
        clear();
        if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
        if (point?.moved) reorder(point.id, target);
      },
      onPointerCancel: clear,
      onLostPointerCapture: clear,
      onKeyDown: event => {
        if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
        event.preventDefault();
        const offset = event.key === "ArrowUp" ? -1 : 1;
        const target = ids[ids.indexOf(id) + offset];
        if (!disabled && target) reorder(id, { id: target, side: offset < 0 ? "before" : "after" });
      },
    };
  }
  const sectionClass = (id: string) => [
    dragging === id ? "group-order-dragging" : "",
    destination?.id === id && dragging !== id ? `group-order-${destination.side}` : "",
  ].join(" ");
  return { dragging, handleProps, sectionClass };
}
