export type Preset = {
  id: string; name: string; defaultState: "keep" | "on" | "off";
  groups: Record<string, boolean>; resources: Record<string, boolean>; clients: string[];
};
export type PresetResource = { id: string; name: string; states: Record<string, boolean | null>; lockedClients?: string[] };
export type PresetGroup = { id: string; name: string; members: string[] };
export type PresetChange = { id: string; client: string; enable: boolean; name: string };
export function resolvePreset(preset: Preset, groups: PresetGroup[], resources: PresetResource[], clients: string[]) {
  const errors: string[] = [], changes: PresetChange[] = [];
  for (const id of Object.keys(preset.groups)) if (!groups.some(g => g.id === id)) errors.push(`分组已不存在：${id}，请编辑方案`);
  for (const group of groups.filter(g => preset.groups[g.id] !== undefined)) {
    for (const id of group.members) if (!resources.some(r => r.id === id)) errors.push(`${group.name} 中的资源已不存在：${id}，请更新分组`);
  }
  for (const id of Object.keys(preset.resources)) if (!resources.some(r => r.id === id)) errors.push(`资源已不存在：${id}，请编辑方案`);
  if (!clients.length) errors.push("请选择目标客户端");
  for (const resource of resources) {
    const rules = groups.filter(g => g.members.includes(resource.id) && preset.groups[g.id] !== undefined).map(g => preset.groups[g.id]);
    const override = preset.resources[resource.id];
    if (override === undefined && rules.some(value => value !== rules[0])) {
      errors.push(`${resource.name} 所属分组设置冲突，请设置单项覆盖`); continue;
    }
    const target = override ?? rules[0] ?? (preset.defaultState === "keep" ? undefined : preset.defaultState === "on");
    if (target === undefined) continue;
    for (const client of clients) {
      const current = resource.states[client];
      if (current == null) { errors.push(`${resource.name} · ${client} 为源目录或异常状态，无法切换`); continue; }
      if (current !== target && resource.lockedClients?.includes(client)) { errors.push(`${resource.name} · ${client} 是实体源目录，不能关闭`); continue; }
      if (current !== target) changes.push({ id: resource.id, name: resource.name, client, enable: target });
    }
  }
  return { errors, changes };
}
