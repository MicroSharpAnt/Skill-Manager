import { useEffect, useRef, useState, type CSSProperties } from "react";
import { ChevronRight, ChevronDown, Check } from "lucide-react";
import "./skill-groups.css";

export const groupColors = [
  { id: "blue", name: "蓝色", value: "#3b82f6" },
  { id: "violet", name: "紫色", value: "#8b5cf6" },
  { id: "emerald", name: "绿色", value: "#10b981" },
  { id: "amber", name: "琥珀色", value: "#f59e0b" },
  { id: "rose", name: "玫红色", value: "#f43f5e" },
  { id: "cyan", name: "青色", value: "#06b6d4" },
  { id: "slate", name: "灰色", value: "#64748b" },
];

export function GroupHeading({ name, color = "slate", count, expanded, toggle }: {
  name: string; color?: string; count: number; expanded: boolean; toggle: () => void;
}) {
  return <button className="skill-group-heading" style={groupColorStyle(color)} aria-expanded={expanded} onClick={toggle}>
    <ChevronRight size={15} className={expanded ? "expanded" : ""} />
    <span className="skill-group-dot" style={{ background: groupColors.find(c => c.id === color)?.value ?? "#64748b" }} />
    <strong>{name}</strong><span className="skill-group-count">{count}</span>
  </button>;
}

export function GroupColorPicker({ value, onChange }: { value: string; onChange: (color: string) => void }) {
  return <div className="group-color-picker" role="group" aria-label="分组颜色">
    {groupColors.map(color => <button key={color.id} type="button" className="group-color-swatch"
      style={{ background: color.value }} aria-label={color.name} aria-pressed={value === color.id}
      onClick={() => onChange(color.id)}>{value === color.id && <Check size={16} />}</button>)}
  </div>;
}

export function groupColorStyle(color?: string): CSSProperties {
  return { "--group-color": groupColors.find(c => c.id === color)?.value ?? "#64748b" } as CSSProperties;
}

export function GroupFilter({ groups, value, onChange, label = "全局分组筛选" }: {
  groups: { id: string; name: string; color: string }[];
  value: string;
  label?: string;
  onChange: (value: string) => void;
}) {
  const [opened, setOpened] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const options = [{ id: "", name: "所有分组", color: "slate" }, ...groups, { id: "ungrouped", name: "未分组", color: "slate" }];
  const active = options.find(g => g.id === value) ?? options[0];
  useEffect(() => {
    if (!opened) return;
    const outside = (event: PointerEvent) => {
      if (!ref.current?.contains(event.target as Node)) setOpened(false);
    };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [opened]);
  return <div className="group-filter" ref={ref} onKeyDown={event => {
    if (event.key === "Escape" && opened) { event.stopPropagation(); setOpened(false); trigger.current?.focus(); }
  }} onBlur={event => {
    if (!event.currentTarget.contains(event.relatedTarget)) setOpened(false);
  }}>
    <button ref={trigger} className="group-filter-trigger" style={groupColorStyle(active.color)}
      aria-label={`${label}：${active.name}`} aria-expanded={opened} aria-controls={`${label}-options`}
      onClick={() => setOpened(previous => !previous)}>
      {value && <span className="skill-group-dot" style={{ background: "var(--group-color)" }} />}
      {active.name}<ChevronDown size={14} />
    </button>
    {opened && <div id={`${label}-options`} className="group-filter-options" role="group" aria-label="选择分组">
      {options.map(g => <button key={g.id} className="group-filter-option" style={groupColorStyle(g.color)}
        aria-pressed={value === g.id} onClick={() => { onChange(g.id); setOpened(false); trigger.current?.focus(); }}>
        <span className="skill-group-dot" style={{ background: "var(--group-color)" }} />
        <span>{g.name}</span>{value === g.id && <Check size={14} />}
      </button>)}
    </div>}
  </div>;
}
