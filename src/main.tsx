import { PresetPanel } from "./PresetPanel";
import { GroupToggle } from "./GroupToggle";
import { ContextMenu } from "./ContextMenu";
import { useGroupOrderDrag } from "./useGroupOrderDrag";
import { useResourceGroupDrag } from "./useResourceGroupDrag";
import { GroupHeading, GroupColorPicker, GroupFilter, groupColorStyle } from "./SkillGroups";
import { MarkdownPreview } from "./MarkdownPreview";
import { ResourceReader } from "./ResourceReader";
import { SettingsPage, SettingsProvider, useSettings } from "./SettingsPanel";
import { openRowDetails } from "./rowDetails";
import { OperationToast } from "./OperationToast";
import { usePanelWidth } from "./PanelWidth";
import React, { useEffect, useState, useRef } from "react";
import { createRoot } from "react-dom/client";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  GripVertical,
  Blocks,
  FolderGit2,
  Globe2,
  ArchiveRestore,
  Plus,
  Search,
  RefreshCw,
  ShieldCheck,
  ShieldAlert,
  ChevronRight,
  FileCode2,
  ArrowUpRight,
  X,
  FolderOpen,
  RotateCcw,
  CheckCheck,
  Trash2,
  AlertTriangle,
  Settings2,
} from "lucide-react";
import type { Project, Snapshot, Resource, Collection, Change } from "./types";
import "./style.css";
import { GlobalSkills } from "./GlobalSkills";
import { Dialog as Modal } from "./GlobalDialog";
import { SelectionCheckbox } from "./SelectionCheckbox";
import "./themes.css";

const desktop = isTauri();
function UpdatedAt({ value }: { value: number | null }) {
  const date = value == null ? null : new Date(value);
  if (!date || !Number.isFinite(date.getTime())) {
    return <span className="resource-updated">最后更新：未知</span>;
  }
  const label = `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
  return <time className="resource-updated" dateTime={date.toISOString()}
    title={`本地文件最后修改时间：${date.toLocaleString("zh-CN")}（Skill 包含目录内附属文件）`}>
    最后更新：{label}
  </time>;
}
const labels: Record<string, string> = {
  enabled: "已启用",
  disabled: "已关闭",
  conflict: "路径冲突",
  recovery: "需要恢复",
  drift: "Git 状态变化",
  unsupported: "含软链接",
};
function App() {
  const { page, setPage, openSettings } = useSettings();
  const [collapsedGroups, setCollapsedGroups] = useState<Set<string>>(new Set());
  const toggleResourceGroup = (id: string) => setCollapsedGroups(previous => {
    const next = new Set(previous);
    if (next.has(id)) next.delete(id); else next.add(id);
    return next;
  });
  const detailWidth = usePanelWidth("skill-manager-detail-sidebar-width", 460);
  const [projects, setProjects] = useState<Project[]>([]),
    [root, setRoot] = useState("");
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null),
    [collections, setCollections] = useState<Collection[]>([]),
    [globalRefresh, setGlobalRefresh] = useState(0);
  const [query, setQuery] = useState(""),
    [kind, setKind] = useState("all"),
    [status, setStatus] = useState("all"),
    [group, setGroup] = useState<number | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set()),
    [detail, setDetail] = useState<Resource | null>(null),
    [content, setContent] = useState("");
  const [busy, setBusy] = useState(false),
    [error, setError] = useState(""),
    [notice, setNotice] = useState("");
  const [plan, setPlan] = useState<Change[] | null>(null),
    [hookDialog, setHookDialog] = useState(false),
    [saveKind, setSaveKind] = useState<"group" | null>(null),
    [name, setName] = useState("");
  const [context, setContext] = useState<{ kind: "group" | "resource"; id: string; x: number; y: number } | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const [newGroupColor, setNewGroupColor] = useState("blue");
  const [batchDestination, setBatchDestination] = useState("");
  const contextOrigin = useRef<HTMLElement | null>(null);
  const resourceList = useRef<HTMLDivElement>(null);
  const running = useRef(false);
  const generation = useRef(0);
  const currentRoot = useRef(root);
  currentRoot.current = root;
  const resources = snapshot?.resources ?? [],
    disabled = resources.filter((r) => !r.enabled),
    problems = resources.filter(
      (r) => !["enabled", "disabled"].includes(r.status),
    );
  const activeGroup = collections.find((c) => c.id === group);
  const filtered = resources.filter(
    (r) =>
      (kind === "all" || r.kind === kind) &&
      (status === "all" || (status === "enabled" ? r.enabled : !r.enabled)) &&
      (group === -1 ? !collections.some(c => c.kind === "group" && c.paths.includes(r.path)) : !activeGroup || activeGroup.paths.includes(r.path)) &&
      (r.name + " " + r.description + " " + r.path)
        .toLowerCase()
        .includes(query.toLowerCase()),
  );
  useEffect(() => { setCollapsedGroups(new Set()); }, [root, query, group, kind, status]);
  const projectGroups = collections.filter(c => c.kind === "group");
  const resourceGroups = [
    ...projectGroups.filter(c => group === null || c.id === group).map(c => ({
      id: String(c.id), name: c.name, color: c.color, items: filtered.filter(r => c.paths.includes(r.path)),
    })),
    ...(group === null || group === -1 ? [{ id: "ungrouped", name: "未分组", color: "slate", items: filtered.filter(r => !projectGroups.some(c => c.paths.includes(r.path))) }] : []),
  ].filter(g => (!query && kind === "all" && status === "all") || g.items.length > 0);
  const fullGroupResources = (id: string) => resources.filter(r => id === "ungrouped"
    ? !projectGroups.some(c => c.paths.includes(r.path))
    : projectGroups.find(c => String(c.id) === id)?.paths.includes(r.path));
  const groupDisabled = (items: Resource[]) => !desktop || busy || !!snapshot?.pending ||
    snapshot?.hook !== "protected" || items.some(r => !["enabled", "disabled"].includes(r.status));
  function moveResources(paths: string[], destination: string) {
    if (running.current || !desktop || snapshot?.pending) return;
    void run(() => invoke("move_project_group", { root, paths,
      destination: destination === "ungrouped" ? null : Number(destination) }), "已移动分组，启用状态保持不变");
  }
  const resourceDrag = useResourceGroupDrag(resourceList, !desktop || busy || !!snapshot?.pending || page !== "project",
    (path, destination) => moveResources(selected.has(path) ? [...selected] : [path], destination));
  const groupOrder = useGroupOrderDrag(resourceList, projectGroups.map(c => String(c.id)),
    !desktop || busy || !!snapshot?.pending || !!resourceDrag.dragging || page !== "project",
    ids => { void run(() => invoke("reorder_project_groups", { root, ids: ids.map(Number) }), "分组顺序已保存"); });
  function closeContext() { setContext(null); contextOrigin.current?.focus({ preventScroll: true }); }
  function openContext(event: React.MouseEvent<HTMLElement>, kind: "group" | "resource", id: string) {
    event.preventDefault(); event.stopPropagation();
    if (running.current || snapshot?.pending) return;
    resourceDrag.clear();
    contextOrigin.current = event.currentTarget.querySelector<HTMLElement>("button") ?? event.currentTarget;
    const rect = event.currentTarget.getBoundingClientRect();
    setRenameValue(projectGroups.find(c => String(c.id) === id)?.name ?? "");
    setContext({ kind, id, x: event.clientX || rect.left + 20, y: event.clientY || rect.top + 20 });
  }
  function updateProjectGroup(c: Collection, changes: { name?: string; color?: string }) {
    closeContext();
    void run(() => invoke("update_project_group", { root, id: c.id, name: c.name, color: c.color, ...changes }), "分组已保存");
  }
  useEffect(() => { setContext(null); resourceDrag.clear(); }, [root, page]);

  async function refresh(target = root) {
    if (!desktop || !target || target !== currentRoot.current) return;
    const seq = ++generation.current;
    try {
      const [s, c] = await Promise.all([
        invoke<Snapshot>("snapshot", { root: target }),
        invoke<Collection[]>("collections", { root: target }),
      ]);
      if (seq === generation.current && target === currentRoot.current) {
        setSnapshot(s);
        setCollections(c);
        setDetail((d) =>
          d ? (s.resources.find((r) => r.path === d.path) ?? null) : null,
        );
      }
    } catch (e) {
      if (seq === generation.current && target === currentRoot.current) {
        setError(String(e));
        setSnapshot(null);
      }
    }
  }
  async function run(fn: () => Promise<unknown>, success = "") {
    if (running.current) return;
    running.current = true;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      await fn();
      if (success) setNotice(success);
    } catch (e) {
      setError(String(e));
    } finally {
      await refresh();
      running.current = false;
      setBusy(false);
    }
  }
  useEffect(() => {
    if (desktop) {
      invoke<Project[]>("projects")
        .then((p) => {
          setProjects(p);
          if (p.length) setRoot(p[0].root);
        })
        .catch((e) => setError(String(e)));
    } else {
      fetch("/preview.json")
        .then((r) => (r.ok ? r.json() : null))
        .then((s) => {
          if (s) {
            setProjects([{ root: s.root, name: s.root.split("/").at(-1) }]);
            setRoot(s.root);
            setSnapshot(s);
          }
        })
        .catch(() => {});
    }
  }, []);
  useEffect(() => {
    if (desktop) setSnapshot(null);
    setCollections([]);
    setSelected(new Set());
    setDetail(null);
    setGroup(null);
    setQuery("");
    void refresh(root);
  }, [root]);
  useEffect(() => {
    const focus = () => {
      if (!busy) void refresh();
    };
    window.addEventListener("focus", focus);
    return () => window.removeEventListener("focus", focus);
  }, [root, busy]);
  useEffect(() => {
    let live = true;
    setContent("");
    if (detail && desktop)
      invoke<string>("content", { root, path: detail.path })
        .then((c) => {
          if (live) setContent(c);
        })
        .catch((e) => {
          if (live) { setContent(""); setError(String(e)); }
        });
    return () => {
      live = false;
    };
  }, [detail?.path, detail?.enabled, detail?.digest, root]);

  useEffect(() => {
    const fn = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !document.querySelector('[role="dialog"]')) {
        setPlan(null);
        setHookDialog(false);
        setSaveKind(null);
        setDetail(null);
      }
    };
    window.addEventListener("keydown", fn);
    return () => window.removeEventListener("keydown", fn);
  }, []);

  useEffect(() => {
    setDetail(null);
  }, [page]);
  useEffect(() => {
    setSelected(
      (old) =>
        new Set(
          [...old].filter((path) => resources.some((r) => r.path === path)),
        ),
    );
  }, [snapshot]);
  useEffect(() => {
    const search = (event: KeyboardEvent) => {
      if (
        (event.metaKey || event.ctrlKey) &&
        event.key.toLowerCase() === "k" &&
        !document.querySelector('[role="dialog"]')
      ) {
        const input = [
          ...document.querySelectorAll<HTMLInputElement>(".search input"),
        ].find((el) => el.getClientRects().length > 0);
        if (input) {
          event.preventDefault();
          input.focus();
          input.select();
        }
      }
    };
    window.addEventListener("keydown", search);
    return () => window.removeEventListener("keydown", search);
  }, []);

  async function addProject(path?: string) {
    if (!desktop) return;
    const picked =
      path ??
      (await open({
        directory: true,
        multiple: false,
        title: "选择项目的 Git 根目录",
      }));
    if (!picked || typeof picked !== "string") return;
    await run(async () => {
      const target = await invoke<string>("add_project", { path: picked });
      setProjects(await invoke<Project[]>("projects"));
      setRoot(target);
      setPage("project");
    }, "项目已添加，尚未修改项目文件");
  }
  function prepare(paths: string[], enable: boolean) {
    const changes = paths
      .filter((p) =>
        resources.some((r) => r.path === p && r.enabled !== enable),
      )
      .map((path) => ({ path, enable }));
    if (!changes.length) {
      setNotice("所选资源已处于目标状态");
      return;
    }
    setPlan(changes);
  }
  function toggleSelected(path: string) {
    setSelected((old) => {
      const n = new Set(old);
      n.has(path) ? n.delete(path) : n.add(path);
      return n;
    });
  }
  async function confirmPlan() {
    if (!plan) return;
    const changes = plan;
    setPlan(null);
    await run(
      () => invoke("apply", { root, changes }),
      "操作完成，文件状态已重新核对",
    );
    setSelected(new Set());
  }

  return (
    <div className="app-shell">
      <div className="titlebar" data-tauri-drag-region>
        <span data-tauri-drag-region>SKILL MANAGER</span>
        <span className="titlebar-right" data-tauri-drag-region>
          本地工作空间 · v0.4.7
        </span>
      </div>
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-icon">
            <Blocks size={23} />
          </div>
          <div>
            Skill Manager<small>为每个项目准备合适的技能</small>
          </div>
        </div>
        <div className="nav-label">资源管理</div>
        <button
          aria-label="全局 Skill"
          title="全局 Skill"
          className={"nav-item " + (page === "global" ? "active" : "")}
          onClick={() => {
            setPage("global");
            setQuery("");
          }}
        >
          <Globe2 size={18} />
          全局 Skill
          <ArrowUpRight size={14} />
        </button>
        <button
          aria-label="项目资源"
          title="项目资源"
          className={"nav-item " + (page === "project" ? "active" : "")}
          onClick={() => {
            setPage("project");
            setQuery("");
          }}
        >
          <FolderGit2 size={18} />
          项目资源<span>{projects.length}</span>
        </button>
        <button
          aria-label="恢复中心"
          title="恢复中心"
          className={"nav-item " + (page === "recovery" ? "active" : "")}
          onClick={() => setPage("recovery")}
        >
          <ArchiveRestore size={18} />
          恢复中心
          {(disabled.length > 0 || snapshot?.pending) && (
            <i className="nav-dot" />
          )}
        </button>
        <button className={"nav-item " + (page === "settings" ? "active" : "")} aria-label="设置" title="设置" aria-current={page === "settings" ? "page" : undefined} onClick={() => openSettings()}>
          <Settings2 size={18} />设置
        </button>
        <div className="nav-label project-label">
          我的项目
          <button
            title="添加项目"
            onClick={() => void addProject()}
            disabled={!desktop || busy}
          >
            <Plus size={16} />
          </button>
        </div>
        <div className="project-list">
          {projects.map((p) => (
            <button
              key={p.root}
              className={"project-item " + (root === p.root ? "chosen" : "")}
              onClick={() => {
                setRoot(p.root);
                setPage("project");
              }}
              title={p.root}
            >
              <span className="project-initial">
                {p.name.slice(0, 1).toUpperCase()}
              </span>
              <span>
                {p.name}
                <small>{p.root.replace(/^\/Users\/[^/]+/, "~")}</small>
              </span>
              {root === p.root && <span className="live-dot" />}
            </button>
          ))}
        </div>
        <button
          className="add-project"
          onClick={() => void addProject()}
          disabled={!desktop || busy}
        >
          <Plus size={16} />
          添加本地项目
        </button>
        <div className="sidebar-bottom">
          <div>
            <span className="live-dot" />
            本地管理 · 按需联网
          </div>
          <p>
            资源留在本机
            <br />
            关闭的内容始终可以找回
          </p>
        </div>
      </aside>
      <div className="window-tools" role="group" aria-label="窗口工具">
            <button
              className="quiet"
              disabled={busy || page === "settings"}
              onClick={() =>
                page === "global"
                  ? setGlobalRefresh((value) => value + 1)
                  : void run(() => refresh())
              }
            >
              <RefreshCw size={15} className={busy ? "spin" : ""} />
              刷新
            </button>
      </div>
      <main>
        {!desktop && (
          <div className="preview-note">
            界面预览 · 文件开关仅在桌面应用内可用
          </div>
        )}
        {page !== "global" && <OperationToast error={error} notice={notice}
          busy={busy ? "正在核对与处理" : ""}
          clearError={() => setError("")} clearNotice={() => setNotice("")} />}
        <div hidden={page !== "global"}>
          <GlobalSkills refreshKey={globalRefresh} />
        </div>
        {page === "global" ? null : page === "settings" ? <SettingsPage /> : page === "recovery" ? (
          <>
            <div className="page-heading">
              <div className="eyebrow">文件保管与恢复</div>
              <h1>恢复中心</h1>
              <p>
                当前项目：
                {projects.find((p) => p.root === root)?.name ?? "请先选择项目"}
                。暂存文件独立于应用保存。
              </p>
            </div>
            <div className="recovery-hero">
              <ArchiveRestore size={34} />
              <div>
                <h2>
                  {snapshot?.pending
                    ? "有中断的批量操作"
                    : disabled.length
                      ? `${disabled.length} 项资源已暂存`
                      : "没有待恢复资源"}
                </h2>
                <p>
                  {snapshot?.pending
                    ? "先回滚中断的批次，再恢复其他已关闭资源。"
                    : "恢复到原路径后，再检查 Git 差异并提交代码。"}
                </p>
              </div>
              <button
                className="primary"
                disabled={!desktop || busy || !root}
                onClick={() =>
                  snapshot?.pending
                    ? void run(
                        () => invoke("recover", { root }),
                        "中断操作已回滚",
                      )
                    : prepare(
                        disabled.map((r) => r.path),
                        true,
                      )
                }
              >
                {snapshot?.pending ? "恢复中断操作" : "全部恢复"}
              </button>
            </div>
            <section className="recovery-location">
              <h3>备份存放位置</h3>
              <code>
                {snapshot?.vault ?? "项目 Git 私有目录 / skill-manager"}
              </code>
              <p>
                state.json 记录原路径，resources
                保存完整内容。即使应用无法启动，也可以依据 RECOVERY.md
                手动恢复。
              </p>
              <button
                className="secondary"
                disabled={!desktop || busy || !root}
                onClick={() =>
                  void run(() => invoke("recover", { root }), "恢复记录已核对")
                }
              >
                核对恢复记录 / 修复提交凭据
              </button>
            </section>
            {disabled.map((r) => (
              <div className="recovery-row" key={r.path}>
                <FileCode2 size={18} />
                <div>
                  <strong>{r.name}</strong>
                  <code>{r.path}</code>
                </div>
                <span className={"badge " + r.status}>{labels[r.status]}</span>
                <button
                  className="secondary"
                  disabled={!desktop || busy}
                  onClick={() => prepare([r.path], true)}
                >
                  恢复
                </button>
              </div>
            ))}
            <p className="muted recovery-note">
              恢复遇到同名文件、分支变化或暂存区变化时会停止，保留双方内容。当前版本不自动覆盖冲突。
            </p>
          </>
        ) : !root ? (
          <div className="welcome">
            <div className="welcome-icon">
              <FolderGit2 size={38} />
            </div>
            <div className="eyebrow">项目技能，随工作切换</div>
            <h1>先添加一个项目</h1>
            <p>
              集中查看项目的 Skill 和 Rule。开发时按需关闭，
              <br />
              提交前一键恢复，项目文件始终由你掌握。
            </p>
            <button
              className="primary"
              disabled={!desktop || busy}
              onClick={() => void addProject()}
            >
              <Plus size={17} />
              选择项目目录
            </button>
            <div className="supported">
              支持 Cursor · Codex · Claude 项目 Skill，以及 Cursor Rule
            </div>
          </div>
        ) : (
          <>
            <div className="page-heading heading-row">
              <div>
                <div className="eyebrow">项目资源</div>
                <h1>
                  {projects.find((p) => p.root === root)?.name}
                  <span className="branch">
                    <FolderGit2 size={13} />
                    {snapshot?.branch || "未命名分支"}
                  </span>
                </h1>
                <p className="mono">{root}</p>
              </div>
              <button
                className="primary"
                disabled={
                  !desktop || busy || !disabled.length || snapshot?.pending
                }
                onClick={() =>
                  prepare(
                    disabled.map((r) => r.path),
                    true,
                  )
                }
              >
                <RotateCcw size={16} />
                提交前全部恢复
                {disabled.length > 0 && <span>{disabled.length}</span>}
              </button>
            </div>
            <div
              className={
                "safety-strip " +
                (snapshot?.hook === "protected" ? "protected" : "unprotected")
              }
            >
              <div className="safety-icon">
                {snapshot?.hook === "protected" ? (
                  <ShieldCheck size={24} />
                ) : (
                  <ShieldAlert size={24} />
                )}
              </div>
              <div>
                <strong>
                  {snapshot?.hook === "protected"
                    ? disabled.length
                      ? "提交已保护 · 恢复后再提交"
                      : "提交保护已启用"
                    : "先为项目启用提交保护"}
                </strong>
                <p>
                  {snapshot?.hook === "protected"
                    ? "关闭的文件暂存于 Git 私有目录；有未恢复资源时会拦截普通提交。"
                    : "避免临时关闭的资源被误提交为删除。添加项目和查看内容不会修改文件。"}
                </p>
              </div>
              {snapshot?.hook === "protected" ? (
                <span className="safety-state">
                  <span className="live-dot" />
                  {disabled.length ? "等待恢复" : "可以提交"}
                </span>
              ) : (
                <button
                  className="secondary"
                  disabled={!desktop || busy || !snapshot}
                  onClick={() => setHookDialog(true)}
                >
                  启用保护
                  <ChevronRight size={15} />
                </button>
              )}
            </div>
            {snapshot?.pending && (
              <div className="message error">
                <AlertTriangle size={18} />
                <span>检测到中断操作，请先在恢复中心回滚。</span>
                <button
                  className="secondary"
                  onClick={() => setPage("recovery")}
                >
                  前往恢复
                </button>
              </div>
            )}
            <div className="resource-overview">
              <span>
                <b>{resources.length}</b> 项资源
              </span>
              <span className="green-dot">
                {resources.length - disabled.length} 已启用
              </span>
              <span className="amber-dot">{disabled.length} 已关闭</span>
              {problems.length > 0 && (
                <span className="problem-count">
                  {problems.length} 项需检查
                </span>
              )}
            </div>
            <PresetPanel key={root} scope={root}
              groups={projectGroups.map(c => ({ id: String(c.id), name: c.name, members: c.paths }))}
              resources={resources.map(r => ({ id: r.path, name: r.name, states: { project: ["enabled", "disabled"].includes(r.status) ? r.enabled : null } }))}
              disabled={!desktop || busy || !!snapshot?.pending || !snapshot} applyDisabled={snapshot?.hook !== "protected"}
              apply={async changes => {
                if (running.current) throw new Error("已有操作正在执行");
                running.current = true; setBusy(true);
                try { await invoke("apply", { root, changes: changes.map(c => ({ path: c.id, enable: c.enable })) }); }
                finally { await refresh(); running.current = false; setBusy(false); }
              }} />
            <div className="resource-panel">
              <div className="resource-tabs">
                <div>
                  {[
                    ["all", "全部资源"],
                    ["skill", "Skills"],
                    ["rule", "Rules"],
                  ].map(([value, label]) => (
                    <button
                      className={kind === value ? "tab active" : "tab"}
                      key={value}
                      onClick={() => setKind(value)}
                    >
                      {label}
                      <span>
                        {value === "all"
                          ? resources.length
                          : resources.filter((r) => r.kind === value).length}
                      </span>
                    </button>
                  ))}
                </div>
                <select
                  aria-label="筛选启用状态"
                  value={status}
                  onChange={(e) => setStatus(e.target.value)}
                >
                  <option value="all">全部状态</option>
                  <option value="enabled">已启用</option>
                  <option value="disabled">已关闭</option>
                </select>
              </div>
              <div className="toolbar">
                <label className="search">
                  <Search size={17} />
                  <input
                    autoCorrect="off"
                    autoCapitalize="none"
                    spellCheck={false}
                    aria-label="搜索项目资源"
                    placeholder="搜索名称、描述或路径…"
                    value={query}
                    onChange={(e) => setQuery(e.target.value)}
                  />
                  {!query && <kbd>⌘ K</kbd>}
                  {query && (
                    <button aria-label="清空搜索" onClick={() => setQuery("")}>
                      <X size={14} />
                    </button>
                  )}
                </label>
                <GroupFilter groups={projectGroups.map(c => ({ id: String(c.id), name: c.name, color: c.color }))}
                  label="项目分组筛选" value={group === -1 ? "ungrouped" : group === null ? "" : String(group)}
                  onChange={value => setGroup(value === "ungrouped" ? -1 : value ? Number(value) : null)} />
                <button className="secondary create-group" disabled={!desktop || busy}
                  onClick={() => { setSaveKind("group"); setName(""); setNewGroupColor("blue"); }}><Plus size={14} />创建分组</button>
                {(query ||
                  status !== "all" ||
                  kind !== "all" ||
                  group !== null) && (
                  <button
                    className="reset-filters"
                    onClick={() => {
                      setQuery("");
                      setKind("all");
                      setStatus("all");
                      setGroup(null);
                    }}
                  >
                    重置筛选
                  </button>
                )}
                {activeGroup && (
                  <button
                    className="icon-button"
                    title="删除分组（保留资源）"
                    onClick={() =>
                      void run(() =>
                        invoke("delete_collection", {
                          id: activeGroup.id,
                        }).then(() => setGroup(null)),
                      )
                    }
                  >
                    <Trash2 size={15} />
                  </button>
                )}
              </div>
              {selected.size > 0 && (
                <div className="batch-bar">
                  <strong>已选 {selected.size} 项</strong>
                  {[...selected].some(
                    (path) => !filtered.some((r) => r.path === path),
                  ) && (
                    <small>
                      含{" "}
                      {
                        [...selected].filter(
                          (path) => !filtered.some((r) => r.path === path),
                        ).length
                      }{" "}
                      项筛选外资源
                    </small>
                  )}
                  <select aria-label="批量移动到项目分组" value={batchDestination} disabled={busy}
                    onChange={event => setBatchDestination(event.target.value)}>
                    <option value="">移动到分组…</option>
                    {projectGroups.map(c => <option key={c.id} value={String(c.id)}>{c.name}</option>)}
                    <option value="ungrouped">未分组</option>
                  </select>
                  <button disabled={!desktop || busy || !batchDestination || snapshot?.pending}
                    onClick={() => moveResources([...selected], batchDestination)}>移动</button>
                  <button
                    onClick={() => prepare([...selected], false)}
                    disabled={!desktop || busy}
                  >
                    批量关闭
                  </button>
                  <button
                    onClick={() => prepare([...selected], true)}
                    disabled={!desktop || busy}
                  >
                    批量启用
                  </button>
                  <button
                    onClick={() => {
                      setName("");
                      setSaveKind("group");
                    }}
                    disabled={!desktop || busy}
                  >
                    保存为分组
                  </button>
                  <button
                    className="clear-selection"
                    onClick={() => setSelected(new Set())}
                  >
                    取消选择
                  </button>
                </div>
              )}
              <div className="table-head">
                <SelectionCheckbox
                  mixed={
                    filtered.some((r) => selected.has(r.path)) &&
                    !filtered.every((r) => selected.has(r.path))
                  }
                  aria-label="选择当前筛选结果"
                  checked={
                    filtered.length > 0 &&
                    filtered.every((r) => selected.has(r.path))
                  }
                  onChange={(e) =>
                    setSelected(
                      e.target.checked
                        ? new Set([...selected, ...filtered.map((r) => r.path)])
                        : new Set(
                            [...selected].filter(
                              (p) => !filtered.some((r) => r.path === p),
                            ),
                          ),
                    )
                  }
                />
                <span>资源名称 / 描述</span>
                <span>来源</span>
                <span>状态</span>
                <span>启用</span>
              </div>
              <div ref={resourceList} className={"resource-list project-groups" + (resourceDrag.dragging || groupOrder.dragging ? " skill-drag-active" : "")}>
                {resourceGroups.map(g => <section key={g.id} data-skill-group={g.id}
                  style={groupColorStyle(g.color)} className={(resourceDrag.destination === g.id ? "skill-group-drop-target " : "") + groupOrder.sectionClass(g.id)}>
                  <div className="skill-group-row project-group-header" onContextMenu={event => openContext(event, "group", g.id)}>
                    <div className={g.id === "ungrouped" ? "" : "sortable-group-heading"}>
                      <GroupHeading name={g.name} color={g.color} count={g.items.length}
                        expanded={!collapsedGroups.has(root + g.id)} toggle={() => toggleResourceGroup(root + g.id)} />
                      {g.id !== "ungrouped" && <button className="group-order-handle" {...groupOrder.handleProps(g.id, g.name)}><GripVertical size={15} /></button>}
                    </div>
                    <GroupToggle name={g.name} values={fullGroupResources(g.id).map(r => r.enabled)}
                      disabled={groupDisabled(fullGroupResources(g.id))}
                      onChange={enable => prepare(fullGroupResources(g.id).map(r => r.path), enable)} />
                  </div>
                  {(!collapsedGroups.has(root + g.id)) && g.items.length === 0 && <div className="skill-group-empty">暂无资源</div>}
                  {(!collapsedGroups.has(root + g.id)) && g.items.map((r) => (
                  <div
                    className={
                      "resource-row detail-row skill-group-child " + (selected.has(r.path) ? "selected " : "") + (resourceDrag.dragging === r.path ? "skill-dragging" : "")
                    }
                    key={r.path}
                    {...resourceDrag.rowProps(r.path)}
                    onContextMenu={event => openContext(event, "resource", r.path)}
                    onClick={event => openRowDetails(event, () => setDetail(r))}
                  >
                    <button className="skill-drag-handle" aria-label={`拖动 ${r.name} 到分组`} title="拖动到分组；右键也可移动"
                      disabled={!desktop || busy || snapshot?.pending}><GripVertical size={14} /></button>
                    <input
                      type="checkbox"
                      checked={selected.has(r.path)}
                      onChange={() => toggleSelected(r.path)}
                      aria-label={`选择 ${r.name}`}
                    />
                    <button
                      className="resource-title"
                      onClick={() => setDetail(r)}
                    >
                      <span className={"resource-glyph " + r.kind}>
                        {r.kind === "skill" ? (
                          <Blocks size={18} />
                        ) : (
                          <FileCode2 size={18} />
                        )}
                      </span>
                      <span>
                        <strong>
                          {r.name}
                          <span className="kind-tag">
                            {r.kind === "skill" ? "SKILL" : "RULE"}
                          </span>
                        </strong>
                        <small>{r.description || r.path}</small>
                        <UpdatedAt value={r.updatedAt} />
                      </span>
                    </button>
                    <span className="provider">{r.provider}</span>
                    <span className={"badge " + r.status}>
                      {labels[r.status] ?? r.status}
                    </span>
                    <button
                      role="switch"
                      aria-checked={r.enabled}
                      aria-label={`${r.enabled ? "关闭" : "启用"} ${r.name}`}
                      className={"switch " + (r.enabled ? "on" : "")}
                      disabled={
                        !desktop ||
                        busy ||
                        snapshot?.pending ||
                        snapshot?.hook !== "protected" ||
                        r.status === "unsupported"
                      }
                      onClick={() => prepare([r.path], !r.enabled)}
                    >
                      <span />
                    </button>
                  </div>
                ))}
                </section>)}
                {!filtered.length && (
                  <div className="empty-inline">
                    <Search size={24} />
                    <h3>
                      {resources.length ? "没有匹配的资源" : "尚未发现项目资源"}
                    </h3>
                    <p>
                      {resources.length
                        ? "调整关键词或筛选条件。"
                        : "扫描 .cursor/skills、.cursor/rules、.agents/skills 和 .claude/skills。"}
                    </p>
                  </div>
                )}
              </div>
              <div className="table-footer">
                <span>
                  显示 {filtered.length} / {resources.length} 项
                </span>
                <span>
                  <ArchiveRestore size={13} />
                  关闭仅暂存 · 原路径恢复
                </span>
              </div>
            </div>
            {snapshot?.warnings.map((w) => (
              <div className="source-warning" key={w}>
                <FileCode2 size={14} />
                {w}
              </div>
            ))}
          </>
        )}
      </main>
      {detail && (
        <div className="detail-panel" style={detailWidth.style}>
          {detailWidth.handle}
          <header>
            <span>资源详情</span>
            <button aria-label="关闭详情" onClick={() => setDetail(null)}>
              <X size={18} />
            </button>
          </header>
          <div className="detail-meta">
            <span className={"badge " + detail.status}>
              {labels[detail.status]}
            </span>
            <h2>{detail.name}</h2>
            <code>{detail.path}</code>
            <p>{detail.description}</p>
            <UpdatedAt value={detail.updatedAt} />
            <small>SHA-256 · {detail.digest.slice(0, 16) || "不可用"}</small>
          </div>
          <ResourceReader key={root + detail.path + detail.enabled} content={content}
            name={detail.name} target={{ scope: "project", root, path: detail.path }}
            comparisons={resources.filter(r => r.path !== detail.path).map(r => ({ name: r.name, source: r.path, target: { scope: "project" as const, root, path: r.path } }))}
            writable={detail.enabled && detail.status === "enabled" && !busy && !snapshot?.pending}
            onSaved={text => { setContent(text); void refresh(); }} />
        </div>
      )}
      {plan && (
        <Modal title="确认资源变更" close={() => setPlan(null)}>
          <p>
            本次修改 {plan.length}{" "}
            项资源。关闭时保留完整内容；恢复时若原路径有同名文件，将停止整个批次。
          </p>
          <div className="change-list">
            {plan.map((c) => (
              <div key={c.path}>
                <span
                  className={"badge " + (c.enable ? "enabled" : "disabled")}
                >
                  {c.enable ? "恢复启用" : "暂存关闭"}
                </span>
                <code>{c.path}</code>
              </div>
            ))}
          </div>
          <div className="modal-note">
            <ShieldCheck size={17} />
            操作不会修改 Git 暂存区。未恢复完毕时，普通提交将被拦截。
          </div>
          <footer>
            <button className="secondary" onClick={() => setPlan(null)}>
              取消
            </button>
            <button
              className="primary"
              disabled={!plan.length || busy || !desktop}
              onClick={() => void confirmPlan()}
            >
              确认执行
            </button>
          </footer>
        </Modal>
      )}
      {hookDialog && (
        <Modal title="启用 Git 提交保护" close={() => setHookDialog(false)}>
          <p>
            将在此仓库安装本地 pre-commit
            检查。有已关闭资源或恢复异常时，它会阻止普通提交，提示先恢复文件。
          </p>
          {snapshot?.hook === "existing" ? (
            <div className="modal-note warning">
              <AlertTriangle size={20} />
              检测到已有 Hook。确认后将先保留原文件为
              pre-commit.skill-manager.previous，并在保护检查通过后执行原 Hook。
            </div>
          ) : snapshot?.hook === "custom" ? (
            <div className="modal-note warning">
              项目配置了自定义
              core.hooksPath，当前版本不自动接管。请手动整合提交保护后再使用开关。
            </div>
          ) : null}
          <p className="muted">
            同一仓库的 worktree 共用 Hook，需要分别添加并启用保护。使用
            --no-verify 可以显式绕过检查。
          </p>
          <footer>
            <button className="secondary" onClick={() => setHookDialog(false)}>
              取消
            </button>
            <button
              className="primary"
              disabled={busy || snapshot?.hook === "custom"}
              onClick={() => {
                setHookDialog(false);
                void run(
                  () =>
                    invoke("install_hook", {
                      root,
                      chainExisting: snapshot?.hook === "existing",
                    }),
                  "提交保护已启用",
                );
              }}
            >
              {snapshot?.hook === "existing"
                ? "备份并串联已有 Hook"
                : "启用保护"}
            </button>
          </footer>
        </Modal>
      )}
      {context && (() => {
        const c = projectGroups.find(c => String(c.id) === context.id);
        const r = resources.find(r => r.path === context.id);
        const members = context.kind === "group" ? fullGroupResources(context.id) :
          r ? (selected.has(r.path) ? resources.filter(item => selected.has(item.path)) : [r]) : [];
        if (context.kind === "resource" && !r) return null;
        const paths = members.map(r => r.path);
        return <ContextMenu x={context.x} y={context.y} title={context.kind === "group" ? c?.name ?? "未分组" : r!.name} close={closeContext}>
          {context.kind === "group" ? <>
            <button className="context-action" onClick={() => { toggleResourceGroup(root + context.id); closeContext(); }}>
              {collapsedGroups.has(root + context.id) ? "展开分组" : "收起分组"}</button>
            <button className="context-action" onClick={() => { setSelected(new Set(paths)); closeContext(); }}>选择整组 {paths.length} 项</button>
            {c && <>
              <form className="context-section" onSubmit={event => { event.preventDefault(); updateProjectGroup(c, { name: renameValue.trim() }); }}>
                <label htmlFor="project-group-name">重命名分组</label><div className="context-rename">
                  <input id="project-group-name" maxLength={60} value={renameValue} onChange={event => setRenameValue(event.target.value)} />
                  <button className="secondary" disabled={!desktop || busy || !renameValue.trim()}>保存</button>
                </div>
              </form>
              <div className="context-section"><p>分组颜色</p><fieldset className="admin-fields" disabled={!desktop || busy}>
                <GroupColorPicker value={c.color} onChange={color => updateProjectGroup(c, { color })} /></fieldset></div>
              <button className="context-action" disabled={!desktop || busy} onClick={() => { closeContext(); void run(async () => {
                await invoke("delete_collection", { id: c.id }); if (group === c.id) setGroup(null);
              }, "分组已删除，资源保留"); }}>删除分组（保留资源）</button>
            </>}
          </> : <button className="context-action" onClick={() => { closeContext(); setDetail(r!); }}>查看详情</button>}
          <div className="context-section"><p>{context.kind === "group" ? "整个分组" : `所选 ${paths.length} 项`}</p>
            <button className="context-action" disabled={!paths.length || groupDisabled(members)} onClick={() => { closeContext(); prepare(paths, true); }}>全部启用</button>
            <button className="context-action" disabled={!paths.length || groupDisabled(members)} onClick={() => { closeContext(); prepare(paths, false); }}>全部关闭</button>
          </div>
          {context.kind === "resource" && <><div className="context-section"><p>移动到分组</p></div><div className="context-destinations">
            {[...projectGroups.map(c => ({ id: String(c.id), name: c.name, color: c.color })), { id: "ungrouped", name: "未分组", color: "slate" }].map(g =>
              <button key={g.id} className="context-action" style={groupColorStyle(g.color)} disabled={!desktop || busy}
                onClick={() => { closeContext(); moveResources(paths, g.id); }}><span className="skill-group-dot" style={{ background: "var(--group-color)" }} />{g.name}</button>)}
          </div></>}
        </ContextMenu>;
      })()}
      {saveKind && (
        <Modal
          title="保存选中项为分组"
          close={() => setSaveKind(null)}
        >
          <p>
            分组只组织资源，不会立即改变它们的启用状态。同一资源可以属于多个分组。
          </p>
          <form
            onSubmit={(e) => {
              e.preventDefault();
              e.currentTarget
                .querySelector<HTMLButtonElement>("button.primary")
                ?.click();
            }}
          >
            <label className="form-label">
              分组名称
              <input
                autoFocus
                value={name}
                maxLength={60}
                placeholder="例如：账号与支付"
                onChange={(e) => setName(e.target.value)}
              />
            </label>
            {saveKind === "group" && <GroupColorPicker value={newGroupColor} onChange={setNewGroupColor} />}
            <small className="muted">
              同名分组将更新为当前集合。
            </small>
            <footer>
              <button
                type="button"
                className="secondary"
                onClick={() => setSaveKind(null)}
              >
                取消
              </button>
              <button
                className="primary"
                type="button"
                disabled={!name.trim() || busy}
                onClick={() => {
                  const k = saveKind;
                  setSaveKind(null);
                  void run(
                    async () => {
                      await invoke("save_collection", {
                        root,
                        name,
                        kind: k,
                        paths: [...selected],
                      });
                      if (k === "group") {
                        const groups = await invoke<Collection[]>("collections", { root });
                        const saved = groups.find(c => c.kind === "group" && c.name === name.trim());
                        if (saved) await invoke("update_project_group", { root, id: saved.id, name: saved.name, color: newGroupColor });
                      }
                    },
                    "已保存",
                  );
                }}
              >
                保存
              </button>
            </footer>
          </form>
        </Modal>
      )}

    </div>
  );
}
createRoot(document.getElementById("root")!).render(<SettingsProvider><App /></SettingsProvider>);
