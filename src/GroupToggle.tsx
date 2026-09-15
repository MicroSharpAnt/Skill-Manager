import { groupState } from "./groupState";

export function GroupToggle({ name, values, disabled, onChange }: {
  name: string; values: boolean[]; disabled?: boolean; onChange: (enabled: boolean) => void;
}) {
  const state = groupState(values);
  return <button role="switch" aria-checked={state === "mixed" ? "mixed" : state === "on"}
    aria-label={`${name}：${state === "mixed" ? "部分开启" : state === "on" ? "全部开启" : "全部关闭"}`}
    title={`${state === "on" ? "关闭" : "开启"}整个分组（包含筛选外成员）`}
    className={`switch group-switch ${state === "on" ? "on" : state === "mixed" ? "mixed" : ""}`}
    disabled={disabled || !values.length} onClick={() => onChange(state !== "on")}><span /></button>;
}
