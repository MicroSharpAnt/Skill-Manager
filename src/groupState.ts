export function groupState(values: boolean[]): "on" | "off" | "mixed" {
  if (!values.length || values.every(value => !value)) return "off";
  return values.every(Boolean) ? "on" : "mixed";
}

export function clientGroupState<T extends { id: string; clients: Record<string, string> }>(skills: T[], client: string) {
  const eligible = skills.filter(s => ["off", "link", "copy"].includes(s.clients[client] ?? "off"));
  return { eligible, excluded: skills.length - eligible.length,
    values: eligible.map(s => ["link", "copy"].includes(s.clients[client] ?? "off")) };
}
