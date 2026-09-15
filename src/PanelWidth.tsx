import { useEffect, useRef, useState, type CSSProperties } from "react";
import "./panel-width.css";

export function usePanelWidth(key: string, initial: number, centered = false) {
  const maximum = () => Math.floor(window.innerWidth * (centered ? 0.94 : 0.9));
  const clamp = (value: number) => Math.min(maximum(), Math.max(360, value));
  const [width, setWidth] = useState(() => {
    try {
      const saved = Number(localStorage.getItem(key));
      return clamp(Number.isFinite(saved) && saved > 0 ? saved : initial);
    } catch {
      return clamp(initial);
    }
  });
  const drag = useRef<{ x: number; width: number } | null>(null);
  const [dragging, setDragging] = useState(false);
  const [max, setMax] = useState(maximum);
  useEffect(() => {
    const resize = () => {
      setMax(maximum());
      setWidth((value) => clamp(value));
    };
    window.addEventListener("resize", resize);
    return () => window.removeEventListener("resize", resize);
  }, [centered]);
  useEffect(() => {
    try { localStorage.setItem(key, String(width)); } catch { /* Storage is optional. */ }
  }, [key, width]);
  const style: CSSProperties = { width, maxWidth: max };
  const handle = (
    <div
      className={`panel-width-handle${dragging ? " dragging" : ""}`}
      role="separator"
      aria-label="调整查看面板宽度"
      aria-orientation="vertical"
      aria-valuemin={Math.min(360, max)}
      aria-valuemax={max}
      aria-valuenow={Math.round(width)}
      tabIndex={0}
      title="拖动调整宽度，双击恢复默认；方向键微调"
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        event.preventDefault();
        event.currentTarget.focus();
        event.currentTarget.setPointerCapture(event.pointerId);
        drag.current = { x: event.clientX, width };
        setDragging(true);
      }}
      onPointerMove={(event) => {
        if (!drag.current) return;
        setWidth(clamp(drag.current.width + (drag.current.x - event.clientX) * (centered ? 2 : 1)));
      }}
      onPointerUp={(event) => {
        drag.current = null;
        setDragging(false);
        if (event.currentTarget.hasPointerCapture(event.pointerId))
          event.currentTarget.releasePointerCapture(event.pointerId);
      }}
      onLostPointerCapture={() => { drag.current = null; setDragging(false); }}
      onPointerCancel={() => { drag.current = null; setDragging(false); }}
      onDoubleClick={() => setWidth(clamp(initial))}
      onKeyDown={(event) => {
        if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
        event.preventDefault();
        setWidth((value) => clamp(event.key === "Home" ? 360 : event.key === "End" ? max : value + (event.key === "ArrowLeft" ? 24 : -24)));
      }}
    />
  );
  return { style, handle };
}
