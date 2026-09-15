import { PlanDiff } from "./PlanDiff";
import { InstallStatusBadge, type InstallStatus } from "./InstallStatus";
import { PresetPanel } from "./PresetPanel";
import { SkillUsage } from "./SkillUsage";
import { DownloadDetails, SourceLink, sourceLabel, type Origin, type DownloadSource } from "./SkillSource";
import { GroupToggle } from "./GroupToggle";
import { clientGroupState } from "./groupState";
import { GroupHeading, GroupFilter, GroupColorPicker, groupColorStyle } from "./SkillGroups";
import { ContextMenu } from "./ContextMenu";
import { useGroupOrderDrag } from "./useGroupOrderDrag";
import { MarkdownPreview } from "./MarkdownPreview";
import { ResourceReader } from "./ResourceReader";
import { openRowDetails } from "./rowDetails";
import { OperationToast } from "./OperationToast";
import { useEffect, useRef, useState, type ReactNode, type MouseEvent } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Search,
  Plus,
  Link2,
  X,
  Download,
  RefreshCw,
  ArrowUpRight,
  FolderOpen,
  AlertTriangle,
  ArchiveRestore,
  GitBranch,
  GripVertical,
  Check,
} from "lucide-react";
import "./global.css";
import { Dialog } from "./GlobalDialog";
import { GlobalAdmin } from "./GlobalAdmin";
import type { AdminOverview, AdminPlan } from "./adminTypes";
const native = isTauri();
import { SelectionCheckbox } from "./SelectionCheckbox";
import { RemoteSkillDialog, type RemoteRequest } from "./RemoteSkillDialog";
import { globalClients as clients } from "./clients";
type Skill = {
  id: string;
  name: string;
  description: string;
  source: string;
  clients: Record<string, string>;
  origin: Origin | null;
  downloadSource: DownloadSource;
  localModified: boolean;
  managed: boolean;
  problem: string | null;
  drift: string[];
};
type Inventory = {
  skills: Skill[];
  warnings: string[];
  pending: boolean;
  library: string;
};
type Discovery = {
  token: string;
  repo: string;
  reference: string;
  candidates: { path: string; name: string; description: string }[];
};
type Plan = {
  token: string;
  kind: string;
  record: { id: string; name: string; source: string; origin: Origin | null };
  localModified: boolean;
  oldHash: string | null;
  newHash: string;
  files: { path: string; kind: string }[];
  oldContent: string;
  newContent: string;
};
type Market = {
  skills: { name: string; skillId: string; repo: string; installs: number }[];
  count: number;
  query: string;
};
type Update = {
  id: string;
  status: string;
  message: string;
  localModified: boolean;
};
function updateOrigin(skill: Skill): Origin | null {
  return skill.origin ?? (skill.downloadSource.kind === "remote" ? skill.downloadSource.origin : null);
}
type Backup = { token: string; name: string; created: number; digest: string };
const cellLabel: Record<string, string> = {
  copy: "已通过副本启用",
  modified: "副本有本地修改，不会覆盖",
  link: "已通过软链接启用",
  off: "未启用，点击创建软链接",
  source: "源目录：不能直接关闭",
  conflict: "同名内容冲突，不会覆盖",
  broken: "存在断开的链接，请先检查原路径",
};
export function GlobalSkills({ refreshKey }: { refreshKey: number }) {
  const [installStatuses, setInstallStatuses] = useState<InstallStatus[]>([]);
  const [admin, setAdmin] = useState<AdminOverview | null>(null);
  const [selected, setSelected] = useState<string[]>([]);
  const [collapsedGroups, setCollapsedGroups] = useState<Set<string>>(new Set());
  const toggleGroup = (id: string) => setCollapsedGroups(previous => {
    const next = new Set(previous);
    if (next.has(id)) next.delete(id); else next.add(id);
    return next;
  });
  const [groupFilter, setGroupFilter] = useState("");
  const [inventory, setInventory] = useState<Inventory | null>(null),
    [mode, setMode] = useState<"installed" | "discover" | "checks" | "manage" | "batch" | "usage">("installed");
  const [filter, setFilter] = useState(""),
    [query, setQuery] = useState(""),
    [market, setMarket] = useState<Market | null>(null),
    [offset, setOffset] = useState(0);
  const [repo, setRepo] = useState(""),
    [reference, setReference] = useState("HEAD"),
    [discovery, setDiscovery] = useState<Discovery | null>(null),
    [candidateFilter, setCandidateFilter] = useState("");
  const [busy, setBusy] = useState(""),
    [error, setError] = useState(""),
    [notice, setNotice] = useState(""),
    [updates, setUpdates] = useState<Update[]>([]);
  const [toggling, setToggling] = useState<{ id: string; clients: string[]; message: string } | null>(null);
  const [orderingGroups, setOrderingGroups] = useState(false);
  const [detailRevision, setDetailRevision] = useState(0);
  const loadedDetailId = useRef<string | null>(null);
  const [detail, setDetail] = useState<Skill | null>(null),
    [body, setBody] = useState(""),
    [backups, setBackups] = useState<Backup[]>([]),
    [target, setTarget] = useState<Skill | null>(null);
  const [diffReady, setDiffReady] = useState(false);
  const [plan, setPlan] = useState<Plan | null>(null),
    [allowChanges, setAllowChanges] = useState(false);
  const [adminBusy, setAdminBusy] = useState(false);
  const [creatingGroup, setCreatingGroup] = useState(false);
  const [newGroupName, setNewGroupName] = useState("");
  const [newGroupColor, setNewGroupColor] = useState("blue");
  const [batchDestination, setBatchDestination] = useState("");
  const [context, setContext] = useState<{ kind: "group" | "skill"; id: string; x: number; y: number } | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const contextOrigin = useRef<HTMLElement | null>(null);
  function closeContext() {
    setContext(null);
    contextOrigin.current?.focus({ preventScroll: true });
  }
  function openContext(event: MouseEvent<HTMLElement>, kind: "group" | "skill", id: string) {
    event.preventDefault();
    event.stopPropagation();
    if (running.current || inventory?.pending) return;
    clearDrag();
    contextOrigin.current = event.currentTarget.querySelector<HTMLElement>("button") ?? event.currentTarget;
    const rect = event.currentTarget.getBoundingClientRect();
    setRenameValue(admin?.config.groups.find(g => g.id === id)?.name ?? "");
    setContext({ kind, id, x: event.clientX || rect.left + 20, y: event.clientY || rect.top + 20 });
  }
  async function updateGroup(id: string, changes: { name?: string; color?: string }) {
    const group = admin?.config.groups.find(g => g.id === id);
    if (!group || running.current || inventory?.pending) return;
    closeContext();
    await act("正在保存分组", () => invoke("global_admin", {
      action: "save_group", args: { ...group, ...changes, members: null },
    }), "分组已保存");
  }
  const [draggedSkill, setDraggedSkill] = useState<string | null>(null);
  const [dropGroup, setDropGroup] = useState<string | null>(null);
  const dragId = useRef<string | null>(null);
  const suppressDragClick = useRef(false);
  const dragPoint = useRef<{ startX: number; startY: number; x: number; y: number } | null>(null);
  const matrixRef = useRef<HTMLDivElement>(null);
  const groupAtPoint = (x: number, y: number) => {
    const node = document.elementFromPoint(x, y)?.closest<HTMLElement>("tbody[data-skill-group]");
    return node && matrixRef.current?.contains(node) ? node.dataset.skillGroup ?? null : null;
  };
  function clearDrag() {
    dragId.current = null;
    dragPoint.current = null;
    setDraggedSkill(null);
    setDropGroup(null);
  }
  function moveToGroup(id: string, groupId: string | null) {
    if (!groupId || running.current || inventory?.pending || !skills.some(s => s.id === id)) return;
    const destination = groupId === "ungrouped" ? null : groupId;
    if (destination && !admin?.config.groups.some(g => g.id === destination)) return;
    if ((admin?.config.members[id] ?? null) === destination) return;
    const name = admin?.config.groups.find(g => g.id === destination)?.name ?? "未分组";
    void act("正在移动分组", async () => {
      await invoke("global_admin", { action: "move_group", args: { ids: [id], group: destination } });
    }, `已移至「${name}」`);
  }
  useEffect(() => {
    if (!draggedSkill) return;
    let frame = 0;
    const scroll = () => {
      const point = dragPoint.current, matrix = matrixRef.current;
      if (point && matrix) {
        const rect = matrix.getBoundingClientRect();
        if (point.x >= rect.left && point.x <= rect.right && point.y >= rect.top - 30 && point.y <= rect.bottom + 30) {
          const speed = point.y < rect.top + 40 ? -10 : point.y > rect.bottom - 40 ? 10 : 0;
          if (speed) matrix.scrollTop += speed;
        }
        setDropGroup(groupAtPoint(point.x, point.y));
      }
      frame = requestAnimationFrame(scroll);
    };
    const cancel = (event: KeyboardEvent) => { if (event.key === "Escape") clearDrag(); };
    frame = requestAnimationFrame(scroll);
    document.addEventListener("keydown", cancel);
    window.addEventListener("blur", clearDrag);
    return () => {
      cancelAnimationFrame(frame);
      document.removeEventListener("keydown", cancel);
      window.removeEventListener("blur", clearDrag);
    };
  }, [draggedSkill]);
  const [remoteRequest, setRemoteRequest] = useState<RemoteRequest | null>(
    null,
  );
  function showRemote(request: RemoteRequest) {
    setError("");
    setRemoteRequest(request);
  }
  const running = useRef(false),
    generation = useRef(0);
  const groupOrder = useGroupOrderDrag(matrixRef, admin?.config.groups.map(g => g.id) ?? [],
    !native || !!busy || !!toggling || orderingGroups || !!inventory?.pending || !!draggedSkill,
    ids => { void saveGroupOrder(ids); });
  async function saveGroupOrder(ids: string[]) {
    if (running.current || inventory?.pending) return;
    running.current = true;
    setOrderingGroups(true);
    setError("");
    setNotice("");
    try {
      await invoke("global_admin", { action: "reorder_groups", args: { ids } });
      setAdmin(await invoke<AdminOverview>("global_admin", { action: "overview", args: {} }));
      setNotice("分组顺序已保存");
    } catch (e) {
      setError(String(e));
    } finally {
      setOrderingGroups(false);
      running.current = false;
    }
  }
  async function refresh({ metadata = true, reloadDetails = true } = {}) {
    if (!native) return;
    const seq = ++generation.current;
    const data = await invoke<Inventory>("global_inventory");
    if (seq === generation.current) {
      setInventory(data);
      setSelected((ids) =>
        ids.filter((id) => data.skills.some((s) => s.id === id)),
      );
      if (metadata) {
        const overview = await invoke<AdminOverview>("global_admin", {
          action: "overview",
          args: {},
        });
        if (seq !== generation.current) return;
        setAdmin(overview);
      }
      setDetail((d) =>
        d ? (data.skills.find((s) => s.id === d.id) ?? null) : null,
      );
      if (reloadDetails) setDetailRevision(value => value + 1);
    }
  }
  async function act(title: string, fn: () => Promise<unknown>, message = "") {
    if (running.current) return;
    running.current = true;
    setBusy(title);
    setError("");
    setNotice("");
    try {
      await fn();
      if (message) setNotice(message);
    } catch (e) {
      setError(String(e));
    } finally {
      try {
        await refresh();
      } catch (e) {
        setError(String(e));
      }
      running.current = false;
      setBusy("");
    }
  }
  useEffect(() => {
    void refresh().catch((e) => setError(String(e)));
  }, [refreshKey]);
  useEffect(() => {
    const focus = () => {
      if (!running.current) void refresh().catch((e) => setError(String(e)));
    };
    window.addEventListener("focus", focus);
    return () => window.removeEventListener("focus", focus);
  }, []);
  useEffect(() => {
    let live = true;
    if (loadedDetailId.current !== (detail?.id ?? null)) {
      loadedDetailId.current = detail?.id ?? null;
      setBody("");
      setBackups([]);
    }
    if (detail) {
      Promise.all([
        invoke<string>("global_details", { id: detail.id }),
        invoke<Backup[]>("global_backups", { id: detail.id }),
      ])
        .then(([text, b]) => {
          if (live) {
            setBody(text);
            setBackups(b);
          }
        })
        .catch((e) => {
          if (live) setError(String(e));
        });
    }
    return () => {
      live = false;
    };
  }, [detail?.id, detail?.localModified, detailRevision]);
  useEffect(() => {
    setAllowChanges(false);
  }, [plan?.token]);
  const skills = inventory?.skills ?? [],
    visible = skills.filter(
      (s) =>
        (s.name + " " + s.description + " " + s.source + " " + sourceLabel(s.downloadSource))
          .toLowerCase()
          .includes(filter.toLowerCase()) &&
        (!groupFilter ||
          (groupFilter === "ungrouped"
            ? !admin?.config.members[s.id]
            : admin?.config.members[s.id] === groupFilter)),
    );
  useEffect(() => { setCollapsedGroups(new Set()); }, [filter, groupFilter]);
  const skillGroups = [
    ...(admin?.config.groups ?? []).map(g => ({ ...g, skills: visible.filter(s => admin?.config.members[s.id] === g.id) })),
    { id: "ungrouped", name: "未分组", color: "slate", skills: visible.filter(s => !admin?.config.groups.some(g => g.id === admin.config.members[s.id])) },
  ].filter(g => (!groupFilter || g.id === groupFilter) && (!filter || g.skills.length > 0));
  const available = updates.filter((u) => u.status === "available").length;
  async function toggle(skill: Skill, clientList: string[], enable: boolean) {
    if (running.current || !clientList.length) return;
    running.current = true;
    setToggling({ id: skill.id, clients: clientList,
      message: enable ? "正在启用客户端 Skill" : "正在关闭客户端 Skill" });
    setError("");
    setNotice("");
    try {
      await invoke("global_toggle", { id: skill.id, clients: clientList, enable });
      setNotice(enable ? "客户端 Skill 已启用" : "客户端 Skill 已关闭，源文件保留");
    } catch (e) {
      setError(String(e));
    } finally {
      try {
        // Reconcile real client states (including same-name conflicts) without
        // refreshing group metadata or reloading the open SKILL.md.
        await refresh({ metadata: false, reloadDetails: false });
      } catch (e) {
        setError(String(e));
      }
      running.current = false;
      setToggling(null);
    }
  }
  const fullGroupSkills = (id: string) => skills.filter(s => id === "ungrouped"
    ? !admin?.config.groups.some(g => g.id === admin.config.members[s.id])
    : admin?.config.members[s.id] === id);
  async function toggleClientGroup(id: string, client: string, enable: boolean) {
    if (running.current || inventory?.pending) return;
    const { eligible, excluded } = clientGroupState(fullGroupSkills(id), client);
    const changed = eligible.filter(s => ["link", "copy"].includes(s.clients[client] ?? "off") !== enable);
    if (!changed.length) return;
    await act(`正在${enable ? "开启" : "关闭"}分组 · ${client}`, async () => {
      const plan = await invoke<AdminPlan>("global_admin", { action: "sync_preview",
        args: { ids: changed.map(s => s.id), clients: [client], enable, method: null } });
      await invoke("global_admin", { action: "apply", args: { token: plan.token } });
    }, `${client}：已${enable ? "开启" : "关闭"} ${changed.length} 项${excluded ? `；${excluded} 项源目录或异常状态未改动` : ""}`);
  }
  function inspectRepo(name = repo, ref = reference) {
    setDiscovery(null);
    setCandidateFilter("");
    void act("正在下载并扫描 GitHub 仓库", async () => {
      const d = await invoke<Discovery>("global_discover", {
        repo: name.trim(),
        reference: ref.trim() || "HEAD",
      });
      setDiscovery(d);
      setInstallStatuses([]);
      setBusy("正在比较本地技能与仓库内容");
      await checkInstallStatuses("global_discovery_install_status", { discovery: d.token }, d.candidates.map(c => c.path));
      setRepo(d.repo);
      setReference(d.reference);
    });
  }
  async function checkInstallStatuses(command: string, args: Record<string, unknown>, keys: string[]) {
    try { setInstallStatuses(await invoke<InstallStatus[]>(command, args)); }
    catch (e) { setInstallStatuses(keys.map(key => ({ key, ids: [], status: "error", message: String(e) }))); }
  }
  function search(nextOffset = 0) {
    setDiscovery(null);
    void act("正在搜索 skills.sh", async () => {
      const data = await invoke<Market>("global_search", {
        query: query.trim(),
        offset: nextOffset,
      });
      setMarket(data);
      setOffset(nextOffset);
      setInstallStatuses([]);
      setBusy("正在检查搜索结果的更新状态");
      await checkInstallStatuses("global_market_install_status", { skills: data.skills }, data.skills.map(s => s.repo + "/" + s.skillId));
    });
  }
  function preview(command: string, args: Record<string, unknown>) {
    setDiffReady(false);
    void act("正在准备文件变更预览", async () =>
      setPlan(await invoke<Plan>(command, args)),
    );
  }
  function associate(skill: Skill) {
    setTarget(skill);
    setMode("discover");
    setDetail(null);
    setDiscovery(null);
    setMarket(null);
    setRepo(skill.origin?.repo ?? "");
    setReference(skill.origin?.reference ?? "HEAD");
    setQuery(skill.name);
  }
  return (
    <div className="global-page">
      {inventory?.pending && (
        <div className="message error">
          <AlertTriangle size={18} />
          <span>有中断的全局操作，先恢复后才能继续启停或升级。</span>
          <button
            className="secondary"
            disabled={!!busy}
            onClick={() =>
              void act(
                "正在回滚中断操作",
                () => invoke("global_recover"),
                "中断操作已恢复",
              )
            }
          >
            <ArchiveRestore size={15} />
            恢复中断操作
          </button>
        </div>
      )}
      {!detail && !plan && <OperationToast error={error} notice={notice} busy={busy || toggling?.message}
        clearError={() => setError("")} clearNotice={() => setNotice("")} />}
      <nav className="global-tabs global-workspace-tabs" aria-label="全局 Skill 工作区">
        {([["installed", "我的技能"], ["discover", "安装技能"], ["usage", "使用统计"], ["checks", "检查与修复"], ["manage", "管理设置"]] as const).map(([id, label]) =>
          <button key={id} className={mode === id || (mode === "batch" && id === "installed") ? "active" : ""}
            aria-current={mode === id || (mode === "batch" && id === "installed") ? "page" : undefined}
            disabled={!!busy || adminBusy || !!toggling || orderingGroups}
            onClick={() => setMode(id)}>{label}{id === "installed" && <span>{skills.length}</span>}</button>
        )}
        {(mode === "installed" || mode === "discover") && <div className="workspace-actions">
        <button
          className="global-check-updates"
          disabled={!!busy || !native}
          onClick={() => void act(
            "正在检查远端更新",
            async () => {
              const result = await invoke<Update[]>("global_check_updates");
              setUpdates(result);
              const changed = result.filter(u => u.status === "available").length;
              const failed = result.filter(u => u.status === "error").length;
              setNotice(`已检查 ${result.length} 个 Skill：${changed} 个可预览升级，${failed} 个检查失败；${skills.filter(s => !updateOrigin(s)).length} 个无远端来源`);
            },
          )}
        >
          <RefreshCw size={15} />
          检查更新{available > 0 && <b>{available}</b>}
        </button>
        <button
          className="global-import"
          disabled={!!busy || !native}
          onClick={async () => {
            const path = await open({
              directory: true,
              multiple: false,
              title: "选择含 SKILL.md 的目录",
            });
            if (typeof path === "string")
              preview("global_prepare_local", { path });
          }}
        >
          <FolderOpen size={15} />
          导入本地 Skill
        </button>
        </div>}
      </nav>
      {mode === "usage" && <SkillUsage skills={skills} />}
      <GlobalAdmin
        skills={skills}
        area={mode === "checks" || mode === "manage" || mode === "batch" ? mode : null}
        onBack={() => setMode("installed")}
        onBusyChange={setAdminBusy}
        disabled={!!busy || !!toggling || orderingGroups || !native}
        selected={selected}
        overview={admin}
        refresh={refresh}
        onDiscovery={d => { setTarget(null); setDiscovery(d); setMode("discover"); }}
      />
      <div hidden={mode !== "installed"}>
          <PresetPanel scope="global" clients={clients}
            groups={(admin?.config.groups ?? []).map(g => ({ id: g.id, name: g.name, members: skills.filter(s => admin?.config.members[s.id] === g.id).map(s => s.id) }))}
            resources={skills.map(s => ({ id: s.id, name: s.name, lockedClients: clients.filter(c => s.clients[c] === "source"), states: Object.fromEntries(clients.map(c => [c,
              ["link", "copy", "source"].includes(s.clients[c] ?? "off") ? true : (s.clients[c] ?? "off") === "off" ? false : null])) }))}
            disabled={!native || !!busy || !!toggling || orderingGroups || !!inventory?.pending || !inventory || !admin}
            apply={async changes => {
              if (running.current) throw new Error("已有操作正在执行");
              running.current = true; setBusy("正在应用预设方案");
              try {
                const plan = await invoke<AdminPlan>("global_admin", { action: "preset_preview",
                  args: { changes: changes.map(({ id, client, enable }) => ({ id, client, enable })) } });
                await invoke("global_admin", { action: "apply", args: { token: plan.token } });
              } finally { try { await refresh(); } finally { running.current = false; setBusy(""); } }
            }} />
          <div className="toolbar">
            <label className="search">
              <Search size={16} />
              <input
                autoCorrect="off"
                autoCapitalize="none"
                spellCheck={false}
                aria-label="搜索已安装 Skill"
                placeholder="搜索名称、描述、源路径…"
                value={filter}
                onChange={(e) => setFilter(e.target.value)}
              />
              {filter ? (
                <button aria-label="清空全局搜索" onClick={() => setFilter("")}>
                  <X size={14} />
                </button>
              ) : (
                <kbd>⌘ K</kbd>
              )}
            </label>
            <GroupFilter groups={admin?.config.groups ?? []} value={groupFilter} onChange={setGroupFilter} />
            <button className="secondary create-group" disabled={!!busy || !native}
              onClick={() => setCreatingGroup(value => !value)}>
              <Plus size={14} />创建分组
            </button>
            {(filter || groupFilter) && (
              <button
                className="reset-filters"
                onClick={() => {
                  setFilter("");
                  setGroupFilter("");
                }}
              >
                重置筛选
              </button>
            )}
            <span className="global-legend">
              <span>
                <Link2 size={12} />
                软链接
              </span>
              <span>源 = 实体目录</span>
              <span>! = 同名冲突</span>
            </span>
          </div>
          {creatingGroup && <form className="inline-group-create" onSubmit={event => {
            event.preventDefault();
            if (!newGroupName.trim()) return;
            void act("正在创建分组", async () => {
              await invoke("global_admin", { action: "save_group", args: { id: null, name: newGroupName.trim(), color: newGroupColor, members: null } });
              setCreatingGroup(false); setNewGroupName("");
            }, "分组已创建");
          }}>
            <label>分组名称<input autoFocus value={newGroupName} onChange={event => setNewGroupName(event.target.value)} placeholder="输入新分组名称" /></label>
            <fieldset className="admin-fields" disabled={!!busy}><GroupColorPicker value={newGroupColor} onChange={setNewGroupColor} /></fieldset>
            <button className="primary" disabled={!!busy || !newGroupName.trim()}>创建分组</button>
            <button type="button" className="quiet" onClick={() => setCreatingGroup(false)}>取消</button>
          </form>}
          {selected.length > 0 && (
            <div className="batch-bar global-batch-bar">
              <strong>已选 {selected.length} 项</strong>
              {selected.some((id) => !visible.some((s) => s.id === id)) && (
                <small>
                  含{" "}
                  {
                    selected.filter((id) => !visible.some((s) => s.id === id))
                      .length
                  }{" "}
                  项筛选外资源
                </small>
              )}
              <select aria-label="批量移动到分组" value={batchDestination} disabled={!!busy}
                onChange={event => setBatchDestination(event.target.value)}>
                <option value="">选择目标分组</option>
                {(admin?.config.groups ?? []).map(g => <option key={g.id} value={g.id}>{g.name}</option>)}
                <option value="ungrouped">未分组</option>
              </select>
              <button disabled={!!busy || !batchDestination || !!inventory?.pending} onClick={() => {
                void act("正在移动分组", () => invoke("global_admin", { action: "move_group", args: {
                  ids: selected, group: batchDestination === "ungrouped" ? null : batchDestination,
                } }), "已移动所选 Skill");
              }}>移动分组</button>
              <button
                disabled={!!busy}
                onClick={() => setMode("batch")}
              >
                客户端与更多操作
              </button>
              <button
                className="clear-selection"
                onClick={() => setSelected([])}
              >
                取消选择
              </button>
            </div>
          )}
          <div ref={matrixRef} className={"matrix-wrap global-matrix" + (draggedSkill || groupOrder.dragging ? " skill-drag-active" : "")}>
            <table className="matrix">
              <thead>
                <tr>
                  <th>
                    <SelectionCheckbox
                      mixed={
                        visible.some((s) => selected.includes(s.id)) &&
                        !visible.every((s) => selected.includes(s.id))
                      }
                      aria-label="选择当前全局资源"
                      checked={
                        visible.length > 0 &&
                        visible.every((s) => selected.includes(s.id))
                      }
                      onChange={(e) =>
                        setSelected(
                          e.target.checked
                            ? Array.from(
                                new Set([
                                  ...selected,
                                  ...visible.map((s) => s.id),
                                ]),
                              )
                            : selected.filter(
                                (id) => !visible.some((s) => s.id === id),
                              ),
                        )
                      }
                    />{" "}
                    SKILL / 共享源
                  </th>
                  {clients.map((c) => (
                    <th key={c}>{c}</th>
                  ))}
                </tr>
              </thead>
                {skillGroups.map(g => <tbody key={g.id}
                  className={(dropGroup === g.id ? "skill-group-drop-target " : "") + groupOrder.sectionClass(g.id)}
                  data-skill-group={g.id}
                >
                  <tr className="skill-group-row" onContextMenu={event => openContext(event, "group", g.id)}><td>
                    <div className={g.id === "ungrouped" ? "" : "sortable-group-heading"}>
                      <GroupHeading name={g.name} color={g.color} count={g.skills.length}
                        expanded={!collapsedGroups.has(g.id)} toggle={() => toggleGroup(g.id)} />
                      {g.id !== "ungrouped" && <button className="group-order-handle" {...groupOrder.handleProps(g.id, g.name)}>
                        <GripVertical size={15} />
                      </button>}
                    </div>
                  </td>{clients.map(client => {
                    const state = clientGroupState(fullGroupSkills(g.id), client);
                    return <td key={client} className="group-client-toggle">
                      <GroupToggle name={`${g.name} · ${client}`} values={state.values}
                        disabled={!native || !!busy || !!toggling || orderingGroups || !!inventory?.pending}
                        onChange={enable => { void toggleClientGroup(g.id, client, enable); }} />
                      {state.excluded > 0 && <small title="源目录、冲突或异常投影不能通过普通开关修改">{state.excluded} 项不可切换</small>}
                    </td>;
                  })}</tr>
                  {(!collapsedGroups.has(g.id)) && g.skills.length === 0 && <tr><td colSpan={clients.length + 1}><div className="skill-group-empty">暂无 Skill</div></td></tr>}
                  {(!collapsedGroups.has(g.id)) && g.skills.map((skill) => {
                  const update = updates.find((u) => u.id === skill.id);
                  return (
                    <tr
                      key={skill.id}
                      onContextMenu={event => openContext(event, "skill", skill.id)}
                      className={"skill-group-child " + (selected.includes(skill.id) ? "selected " : "") + (draggedSkill === skill.id ? "skill-dragging" : "")}
                      onPointerDown={event => {
                        if (event.button !== 0 || !native || running.current || inventory?.pending) return;
                        suppressDragClick.current = false;
                        const control = (event.target as Element).closest("button, input, a, label, select, textarea, [role='button'], [role='checkbox'], [role='switch']");
                        if (control && !control.matches(".skill-drag-handle, .global-name")) return;
                        event.preventDefault();
                        (event.target as Element).setPointerCapture(event.pointerId);
                        dragId.current = skill.id;
                        dragPoint.current = { startX: event.clientX, startY: event.clientY, x: event.clientX, y: event.clientY };
                      }}
                      onPointerMove={event => {
                        const point = dragPoint.current;
                        if (!point || !dragId.current) return;
                        point.x = event.clientX; point.y = event.clientY;
                        if (Math.hypot(point.x - point.startX, point.y - point.startY) < 5 && !draggedSkill) return;
                        suppressDragClick.current = true;
                        setDraggedSkill(dragId.current);
                        setDropGroup(groupAtPoint(point.x, point.y));
                      }}
                      onPointerUp={event => {
                        const id = dragId.current;
                        const point = dragPoint.current;
                        const destination = groupAtPoint(event.clientX, event.clientY);
                        const moved = point && suppressDragClick.current;
                        clearDrag();
                        if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
                        if (id && moved) moveToGroup(id, destination);
                      }}
                      onPointerCancel={clearDrag}
                      onLostPointerCapture={clearDrag}
                      onClickCapture={event => {
                        if (!suppressDragClick.current) return;
                        event.preventDefault();
                        event.stopPropagation();
                        suppressDragClick.current = false;
                      }}
                    >
                      <td className="detail-row" onClick={event => openRowDetails(event, () => {
                        setError("");
                        setDetail(skill);
                      })}>
                        <button className="skill-drag-handle"
                          disabled={!native || !!busy || inventory?.pending}
                          aria-label={`拖动 ${skill.name} 到其他分组`}
                          title="拖到目标分组以移动"
                        ><GripVertical size={15} /></button>
                        <input
                          type="checkbox"
                          aria-label={`选择全局 ${skill.name}`}
                          checked={selected.includes(skill.id)}
                          onChange={(e) =>
                            setSelected(
                              e.target.checked
                                ? [...selected, skill.id]
                                : selected.filter((id) => id !== skill.id),
                            )
                          }
                        />
                        <button
                          className="global-name"
                          onClick={() => {
                            setError("");
                            setDetail(skill);
                          }}
                        >
                          <strong>{skill.name}</strong>
                          {skill.localModified && (
                            <span className="local-badge">本地修改</span>
                          )}
                          {update?.status === "available" && (
                            <span className="update-badge">可升级</span>
                          )}
                          {skill.drift.length > 0 && (
                            <span className="local-badge">链接变化</span>
                          )}
                        </button>
                        <small title={skill.source}>
                          {skill.source.replace(/^\/Users\/[^/]+/, "~")}
                        </small>
                        <small className="skill-download-label" title={sourceLabel(skill.downloadSource)}>
                          下载来源：{sourceLabel(skill.downloadSource)}
                        </small>
                        {update && (
                          <div
                            className={"update-status " + update.status}
                            title={update.message}
                          >
                            {update.message}
                          </div>
                        )}
                      </td>
                      {clients.map((client) => {
                        const status = skill.clients[client] ?? "off";
                        return (
                          <td key={client}>
                            <button
                              className={"global-cell " + status}
                              aria-busy={toggling?.id === skill.id && toggling.clients.includes(client)}
                              aria-disabled={!!toggling || undefined}
                              aria-label={`${skill.name} · ${client}：${cellLabel[status]}`}
                              aria-pressed={["link", "copy"].includes(status)}
                              title={cellLabel[status]}
                              disabled={
                                !native ||
                                !!busy ||
                                inventory?.pending ||
                                !["link", "copy", "off"].includes(status)
                              }
                              onClick={() =>
                                toggle(
                                  skill,
                                  [client],
                                  !["link", "copy"].includes(status),
                                )
                              }
                            >
                              {status === "copy" ? (
                                "副"
                              ) : status === "link" ? (
                                <Link2 size={15} />
                              ) : status === "source" ? (
                                "源"
                              ) : status === "off" ? (
                                <Plus size={13} />
                              ) : (
                                <AlertTriangle size={14} />
                              )}
                            </button>
                          </td>
                        );
                      })}
                    </tr>
                  );
                })}
                </tbody>)}
            </table>
            {!visible.length && (
              <div className="empty-inline">
                {skills.length
                  ? "没有匹配的 Skill。"
                  : "未发现本机技能。可以搜索安装或导入一个本地目录。"}
              </div>
            )}
          </div>
          <div className="table-footer">
            <span>
              显示 {visible.length} / {skills.length} 项
            </span>
            <span>
              <Link2 size={13} />
              拖动 Skill 行可移动分组 · 点按客户端状态可启停
            </span>
          </div>
          {inventory?.warnings.length ? (
            <details className="global-warnings">
              <summary>{inventory.warnings.length} 项目录提示</summary>
              {inventory.warnings.map((w) => (
                <p key={w}>{w}</p>
              ))}
            </details>
          ) : null}
          <p className="global-footnote">
            同名但不同来源的版本分开显示；客户端同一名称只能使用一个版本。实体源目录不会被开关移除。
          </p>
      </div>
      <div hidden={mode !== "discover"}>
          {target && (
            <div className="associate-banner">
              <GitBranch size={18} />
              <div>
                为 <strong>{target.name}</strong> 选择升级来源
                <p>
                  选择仓库中的精确 Skill 后，先比较内容，再确认是否替换共享源。
                </p>
              </div>
              <button aria-label="取消关联来源" onClick={() => setTarget(null)}>
                <X size={16} />
              </button>
            </div>
          )}
          <form
            className="market-search"
            onSubmit={(e) => {
              e.preventDefault();
              search();
            }}
          >
            <label className="search">
              <Search size={18} />
              <input
                placeholder="搜索 skills.sh，例如 react、review、unity…"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
              />
            </label>
            <button
              className="primary"
              disabled={!!busy || !native || !query.trim()}
              type="submit"
            >
              搜索 Skill
            </button>
          </form>
          <form
            className="repo-search"
            onSubmit={(e) => {
              e.preventDefault();
              inspectRepo();
            }}
          >
            <GitBranch size={16} />
            <span>或直接查看 GitHub 仓库</span>
            <input
              aria-label="GitHub 仓库"
              placeholder="owner/repo"
              value={repo}
              onChange={(e) => setRepo(e.target.value)}
            />
            <input
              aria-label="仓库分支或标签"
              className="reference-input"
              placeholder="HEAD"
              value={reference}
              onChange={(e) => setReference(e.target.value)}
            />
            <button
              className="secondary"
              disabled={!!busy || !native || !repo.trim()}
            >
              读取仓库
            </button>
          </form>
          {(discovery || market) && <div className="market-status-refresh">
            <span>按下载源匹配本地技能，并比较完整文件内容。</span>
            <button className="quiet" disabled={!!busy} onClick={() => void act("正在重新检查安装与更新状态", async () => {
              setInstallStatuses([]);
              if (discovery) await checkInstallStatuses("global_discovery_install_status", { discovery: discovery.token }, discovery.candidates.map(c => c.path));
              else if (market) await checkInstallStatuses("global_market_install_status", { skills: market.skills }, market.skills.map(s => s.repo + "/" + s.skillId));
            })}><RefreshCw size={14} />重新检查</button>
          </div>}
          {discovery ? (
            <section className="discovery-list">
              <div className="discovery-heading">
                <div>
                  <strong>{discovery.repo}</strong>
                  <small>
                    {discovery.reference} · {discovery.candidates.length} 个
                    Skill
                  </small>
                </div>
                <label className="search">
                  <Search size={15} />
                  <input
                    placeholder="筛选仓库中的 Skill…"
                    value={candidateFilter}
                    onChange={(e) => setCandidateFilter(e.target.value)}
                  />
                </label>
              </div>
              {discovery.candidates
                .filter((c) =>
                  (c.name + " " + c.path)
                    .toLowerCase()
                    .includes(candidateFilter.toLowerCase()),
                )
                .map((c) => (
                  <div className="candidate detail-row" key={c.path}
                    onClick={event => { if (!busy) openRowDetails(event, () => showRemote({ discovery, path: c.path })); }}>
                    <div>
                      <button
                        className="candidate-title"
                        disabled={!!busy}
                        onClick={() => showRemote({ discovery, path: c.path })}
                      >
                        {c.name}
                      </button>
                      <code>{c.path === "." ? "仓库根目录" : c.path}</code>
                      <p>{c.description}</p>
                      <InstallStatusBadge value={installStatuses.find(s => s.key === c.path)} />
                    </div>
                    <button
                      className="secondary"
                      disabled={!!busy}
                      onClick={() => showRemote({ discovery, path: c.path })}
                    >
                      <Download size={15} />
                      查看详情
                    </button>
                  </div>
                ))}
            </section>
          ) : market ? (
            <>
              <div className="market-caption">
                “{market.query}” · {market.count} 条结果 · 来源 skills.sh
              </div>
              <div className="market-results">
                {market.skills.map((s) => (
                  <article className="detail-row" key={s.repo + "/" + s.skillId}
                    onClick={event => { if (!busy) openRowDetails(event, () => showRemote({ market: s })); }}>
                    <div className="market-icon">
                      <Download size={20} />
                    </div>
                    <div>
                      <h3>
                        <button
                          className="market-title"
                          disabled={!!busy}
                          onClick={() => showRemote({ market: s })}
                        >
                          {s.name}
                        </button>
                      </h3>
                      <code>{s.repo}</code>
                      <small>{s.installs.toLocaleString()} 次安装</small>
                      <InstallStatusBadge value={installStatuses.find(status => status.key === s.repo + "/" + s.skillId)} />
                    </div>
                    <button
                      className="quiet"
                      disabled={!!busy}
                      onClick={() => showRemote({ market: s })}
                    >
                      查看详情
                      <ArrowUpRight size={14} />
                    </button>
                  </article>
                ))}
              </div>
              <div className="market-pages">
                <button
                  className="secondary"
                  disabled={!!busy || offset === 0}
                  onClick={() => search(Math.max(0, offset - 30))}
                >
                  上一页
                </button>
                <span>第 {Math.floor(offset / 30) + 1} 页</span>
                <button
                  className="secondary"
                  disabled={
                    !!busy ||
                    offset + market.skills.length >= market.count ||
                    !market.skills.length
                  }
                  onClick={() => search(offset + 30)}
                >
                  下一页
                </button>
              </div>
              {!market.skills.length && (
                <div className="empty-inline">
                  没有匹配结果，可以更换关键词或直接输入仓库。
                </div>
              )}
            </>
          ) : (
            <div className="registry-empty">
              <Search size={30} />
              <h2>找到下一项技能</h2>
              <p>
                搜索公开技能目录，或输入 GitHub 仓库。
                <br />
                安装前可查看文件清单和 SKILL.md 内容。
              </p>
              <small>联网只在搜索、读取仓库和检查更新时发生。</small>
            </div>
          )}
      </div>
      {context && (() => {
        const group = admin?.config.groups.find(g => g.id === context.id);
        const skill = skills.find(s => s.id === context.id);
        const member = skill ? admin?.config.members[skill.id] ?? "ungrouped" : "";
        if (context.kind === "skill" && !skill) return null;
        return <ContextMenu key={`${context.kind}:${context.id}`} x={context.x} y={context.y}
          title={context.kind === "group" ? group?.name ?? "未分组" : skill!.name} close={closeContext}>
          {context.kind === "group" ? <>
            <button className="context-action" onClick={() => { toggleGroup(context.id); closeContext(); }}>
              {collapsedGroups.has(context.id) ? "展开分组" : "收起分组"}
            </button>
            <div className="context-section"><p>按客户端切换整个分组</p>
              {clients.map(client => <div className="context-client-toggle" key={client}><span>{client}</span>
                <GroupToggle name={`${group?.name ?? "未分组"} · ${client}`} values={clientGroupState(fullGroupSkills(context.id), client).values}
                  disabled={!native || !!busy || !!toggling || !!inventory?.pending}
                  onChange={enable => { const id = context.id; closeContext(); void toggleClientGroup(id, client, enable); }} />
              </div>)}
            </div>
            {group && <>
              <form className="context-section" onSubmit={event => {
                event.preventDefault(); void updateGroup(group.id, { name: renameValue.trim() });
              }}>
                <label htmlFor="context-group-name">重命名分组</label>
                <div className="context-rename">
                  <input id="context-group-name" value={renameValue} onChange={event => setRenameValue(event.target.value)} />
                  <button className="secondary" disabled={!renameValue.trim() || !!busy || !native}>保存</button>
                </div>
              </form>
              <div className="context-section"><p>分组颜色</p>
                <fieldset className="admin-fields" disabled={!!busy || !native}>
                  <GroupColorPicker value={group.color} onChange={color => { void updateGroup(group.id, { color }); }} />
                </fieldset>
              </div>
            </>}
          </> : <>
            <button className="context-action" onClick={() => { closeContext(); setError(""); setDetail(skill!); }}>
              查看详情
            </button>
            <div className="context-section"><p>移动到分组</p></div>
            <div className="context-destinations">
              {[...(admin?.config.groups ?? []), { id: "ungrouped", name: "未分组", color: "slate" }].map(g =>
                <button key={g.id} className="context-action" aria-pressed={member === g.id}
                  disabled={!!busy || !native || member === g.id} style={groupColorStyle(g.color)}
                  onClick={() => { closeContext(); moveToGroup(skill!.id, g.id); }}>
                  <span className="skill-group-dot" style={{ background: "var(--group-color)" }} />
                  <span>{g.name}</span>{member === g.id && <Check size={14} />}
                </button>)}
            </div>
          </>}
        </ContextMenu>;
      })()}
      {detail && (
        <Dialog title={detail.name} close={() => setDetail(null)} wide resizable>
          {!plan && <OperationToast error={error} notice={notice} busy={busy || toggling?.message}
            clearError={() => setError("")} clearNotice={() => setNotice("")} />}
          <div className="global-detail-info">
            <code>{detail.source}</code>
            <p>{detail.description}</p>
            <DownloadDetails source={detail.downloadSource} />
            {updateOrigin(detail) ? (
              <p>当前升级来源：<SourceLink origin={updateOrigin(detail)!} /> · {updateOrigin(detail)!.reference} · {updateOrigin(detail)!.path}</p>
            ) : (
              <p>尚未关联在线升级来源。</p>
            )}
            {detail.problem && (
              <p className="problem-count">{detail.problem}</p>
            )}
          </div>
          <div className="detail-actions">
            <button
              className="secondary"
              disabled={!!busy || inventory?.pending}
              onClick={() =>
                toggle(
                  detail,
                  clients.filter((c) => detail.clients[c] !== "source"),
                  true,
                )
              }
            >
              全部启用
            </button>
            <button
              className="secondary"
              disabled={!!busy || inventory?.pending}
              onClick={() =>
                toggle(
                  detail,
                  clients.filter((c) =>
                    ["link", "copy"].includes(detail.clients[c]),
                  ),
                  false,
                )
              }
            >
              全部关闭投影
            </button>
            <button
              className="primary"
              disabled={!!busy || inventory?.pending}
              onClick={() =>
                updateOrigin(detail)
                  ? preview("global_prepare_update", { id: detail.id })
                  : associate(detail)
              }
            >
              {updateOrigin(detail) ? "检查并预览升级" : "关联升级来源"}
            </button>
            {updateOrigin(detail) && (
              <button
                className="quiet"
                disabled={!!busy}
                onClick={() => associate(detail)}
              >
                更换来源
              </button>
            )}
          </div>
          <details className="backup-list">
            <summary>旧版本备份 · {backups.length}</summary>
            {backups.map((b) => (
              <div key={b.token}>
                <span>
                  {new Date(b.created * 1000).toLocaleString()}
                  <code>{b.digest.slice(0, 14)}</code>
                </span>
                <button
                  className="secondary"
                  disabled={!!busy}
                  onClick={() =>
                    preview("global_prepare_restore", {
                      id: detail.id,
                      token: b.token,
                    })
                  }
                >
                  预览恢复
                </button>
              </div>
            ))}
          </details>
          <ResourceReader key={detail.id} content={body} name={detail.name}
            comparisons={skills.filter(s => s.id !== detail.id && !s.problem).map(s => ({ name: s.name, source: s.source, target: { scope: "global" as const, id: s.id } }))}
            target={{ scope: "global", id: detail.id }} writable={!busy && !inventory?.pending && !detail.problem}
            onSaved={text => { setBody(text); void refresh().catch(e => setError(String(e))); }} />
        </Dialog>
      )}
      {remoteRequest && (
        <RemoteSkillDialog
          request={remoteRequest}
          close={() => setRemoteRequest(null)}
          busy={busy}
          error={error}
          upgrading={!!target}
          openLocal={id => { setRemoteRequest(null); setDetail(skills.find(s => s.id === id) ?? null); }}
          clearError={() => setError("")}
          install={(d, path, existing) =>
            preview("global_prepare_remote", {
              discovery: d.token,
              path,
              existing: target?.id ?? existing ?? null,
            })
          }
        />
      )}
      {plan && (
        <Dialog
          title={
            plan.kind === "update"
              ? "预览 Skill 升级"
              : plan.kind === "restore"
                ? "预览旧版本恢复"
                : "预览 Skill 安装"
          }
          close={() => !busy && setPlan(null)}
          resizable
          wide
        >
          {<OperationToast error={error} notice={notice} busy={busy}
            clearError={() => setError("")} clearNotice={() => setNotice("")} />}
          <div className="plan-target">
            <strong>{plan.record.name}</strong>
            <code>{plan.record.source}</code>
            {plan.record.origin && (
              <small>
                {plan.record.origin.repo} · {plan.record.origin.reference} ·{" "}
                {plan.record.origin.path}
              </small>
            )}
          </div>
          {plan.oldHash && (
            <div className="modal-note">
              <ArchiveRestore size={17} />
              旧版本将完整保留，可在 Skill
              详情中预览并恢复。链接到共享源的客户端会一起使用新内容。
            </div>
          )}
          <div className="file-changes">
            <strong>{plan.files.length} 项文件变更</strong>
            {plan.files.map((f) => (
              <div key={f.path}>
                <span className={f.kind}>
                  {f.kind === "added"
                    ? "新增"
                    : f.kind === "removed"
                      ? "移除"
                      : "修改"}
                </span>
                <code>{f.path || "目录"}</code>
              </div>
            ))}
          </div>
          {plan.oldHash ? <PlanDiff key={plan.token} token={plan.token} rightLabel={plan.kind === "restore" ? "待恢复版本" : "待安装版本"} onReady={setDiffReady} /> : (
            <div className="content-compare"><section><h3>待安装 SKILL.md</h3><pre>{plan.newContent}</pre></section></div>
          )}
          {plan.localModified && (
            <label className="local-confirm">
              <input
                type="checkbox"
                checked={allowChanges}
                onChange={(e) => setAllowChanges(e.target.checked)}
              />
              <span>
                此 Skill
                有本地修改，或是首次关联远端。我已核对内容，同意保留完整备份后替换。
              </span>
            </label>
          )}
          <footer>
            <button
              className="secondary"
              disabled={!!busy}
              onClick={() => setPlan(null)}
            >
              取消
            </button>
            <button
              className="primary"
              disabled={!!busy || (!!plan.oldHash && !diffReady) || (plan.localModified && !allowChanges)}
              onClick={() =>
                void act(
                  "正在提交文件变更",
                  async () => {
                    await invoke("global_apply", {
                      token: plan.token,
                      allowLocalChanges: allowChanges,
                    });
                    setPlan(null);
                    setRemoteRequest(null);
                    setTarget(null);
                    setMode("installed");
                    setMarket(null);
                    setDiscovery(null);
                    setInstallStatuses([]);
                    setDetail(null);
                    setUpdates((u) => u.filter((x) => x.id !== plan.record.id));
                  },
                  plan.oldHash
                    ? "新版本已应用，旧版本备份已保留"
                    : "Skill 已安装，点击客户端单元格启用软链接",
                )
              }
            >
              {plan.oldHash ? "备份并应用" : "确认安装到共享库"}
            </button>
          </footer>
        </Dialog>
      )}
    </div>
  );
}
