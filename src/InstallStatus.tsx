export type InstallStatus = {
  key: string;
  ids: string[];
  status: string;
  message: string;
};
export function InstallStatusBadge({ value }: { value?: InstallStatus }) {
  if (value?.status === "unmatched") return null;
  const labels: Record<string, string> = {
    latest: "已安装 · 无需更新",
    available: "已安装 · 可更新",
    local: "已安装 · 本地有修改",
    multiple: "已安装 · 多个本地版本",
    error: "更新状态检查失败",
    unknown: "暂无法确认更新状态",
  };
  return <small className={`install-status ${value?.status ?? "checking"}`} title={value?.message} role="status">
    {value ? labels[value.status] ?? value.message : "正在检查安装与更新状态…"}
  </small>;
}
