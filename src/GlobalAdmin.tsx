import { OperationToast } from "./OperationToast";
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Archive,
  FolderInput,
} from "lucide-react";
import { Dialog } from "./GlobalDialog";
import { BrokenLinks } from "./BrokenLinks";
import { DuplicateSkills } from "./DuplicateSkills";
import type { AdminOverview, AdminPlan, Repo } from "./adminTypes";
import "./admin.css";
type Skill = {
  id: string;
  name: string;
  source: string;
  managed: boolean;
  clients: Record<string, string>;
};
type Discovery = {
  token: string;
  repo: string;
  reference: string;
  candidates: { path: string; name: string; description: string }[];
};
import { globalClients as clients } from "./clients";
const tabs = [
  ["duplicates", "重复检查"],
  ["repair", "修复断链"],
  ["storage", "存储与客户端"],
  ["repos", "仓库订阅"],
  ["archive", "导入与导出"],
  ["backups", "备份中心"],
  ["migration", "cc-switch 迁移"],
  ["batch", "批量与接管"],
] as const;
export function GlobalAdmin({
  skills,
  area,
  onBack,
  onBusyChange,
  disabled = false,
  selected,
  overview,
  refresh,
  onDiscovery,
}: {
  skills: Skill[];
  area: "checks" | "manage" | "batch" | null;
  onBack: () => void;
  onBusyChange: (busy: boolean) => void;
  disabled?: boolean;
  selected: string[];
  overview: AdminOverview | null;
  refresh: () => Promise<void>;
  onDiscovery: (d: Discovery) => void;
}) {
  const [tab, setTab] = useState("duplicates"),
    [busy, setBusy] = useState(""),
    [error, setError] = useState(""),
    [notice, setNotice] = useState("");
  const [plan, setPlan] = useState<AdminPlan | null>(null),
    [confirmed, setConfirmed] = useState(false),
    [ccDir, setCcDir] = useState(""),
    [target, setTarget] = useState("independent");
  const [ids, setIds] = useState<string[]>([]),
    [batchClients, setBatchClients] = useState(["Codex"]);
  const [repo, setRepo] = useState(""),
    [reference, setReference] = useState("HEAD"),
    [discovered, setDiscovered] = useState<{ Ok?: Discovery; Err?: string }[]>(
      [],
    );
  const [archive, setArchive] = useState<Discovery | null>(null),
    [paths, setPaths] = useState<string[]>([]),
    [client, setClient] = useState("Codex"),
    [clientPath, setClientPath] = useState("");
  const [repairRevision, setRepairRevision] = useState(0);
  const [deleteToken, setDeleteToken] = useState<string | null>(null);
  const [deletedHistory, setDeletedHistory] = useState<string | null>(null);
  const [visitedChecks, setVisitedChecks] = useState<Set<string>>(new Set());
  const lastTabs = useRef({ checks: "duplicates", manage: "storage" });
  const cfg = overview?.config;
  useEffect(() => {
    if (area === "checks") chooseTab(lastTabs.current.checks);
    if (area === "manage") chooseTab(lastTabs.current.manage);
    if (area === "batch") { chooseTab("batch"); setIds(selected); }
  }, [area]);
  useEffect(() => { onBusyChange(!!busy); }, [busy, onBusyChange]);
  useEffect(() => {
    if (overview && !ccDir) setCcDir(overview.ccDir);
  }, [overview?.ccDir]);
  useEffect(
    () => setClientPath(cfg?.clients[client] ?? ""),
    [client, cfg?.clients[client]],
  );
  async function command<T = unknown>(
    action: string,
    args: Record<string, unknown> = {},
  ): Promise<T> {
    return invoke<T>("global_admin", { action, args });
  }
  async function act(title: string, fn: () => Promise<unknown>) {
    if (busy) return;
    setBusy(title);
    setError("");
    setNotice("");
    try {
      await fn();
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy("");
    }
  }
  function preview(action: string, args: Record<string, unknown>) {
    void act("生成操作预览", async () => {
      setPlan(await command<AdminPlan>(action, args));
      setConfirmed(false);
    });
  }
  function chooseTab(value: string) {
    setTab(value);
    if (value === "duplicates" || value === "repair") {
      lastTabs.current.checks = value;
      setVisitedChecks(previous => new Set([...previous, value]));
    } else if (value !== "batch") {
      lastTabs.current.manage = value;
    }
    setError("");
    setNotice("");
  }
  const clientPicker = (
    <div className="admin-clients">
      {clients.map((c) => (
        <label key={c}>
          <input
            type="checkbox"
            checked={batchClients.includes(c)}
            onChange={(e) =>
              setBatchClients(
                e.target.checked
                  ? [...batchClients, c]
                  : batchClients.filter((x) => x !== c),
              )
            }
          />
          {c}
        </label>
      ))}
    </div>
  );
  return (
    <>
      <section hidden={!area} className={`admin-workspace ${area === "batch" ? "admin-workspace-batch" : ""}`} aria-label="技能管理工作区">
        {area === "batch" ? <header className="admin-panel-header">
          <div><h2>操作已选的 {ids.length} 项 Skill</h2><p className="admin-help">沿用列表中的选择，选择客户端或操作后预览变更。</p></div>
          <button className="secondary" disabled={!!busy} onClick={onBack}>返回技能列表</button>
        </header> : <nav className="admin-workspace-nav" aria-label={area === "checks" ? "检查项目" : "管理设置"}>
          {tabs.filter(([id]) => area === "checks" ? ["duplicates", "repair"].includes(id) : !["duplicates", "repair", "batch"].includes(id)).map(([id, label]) =>
            <button key={id} disabled={!!busy || disabled} aria-current={tab === id ? "page" : undefined}
              className={tab === id ? "active" : ""} onClick={() => chooseTab(id)}>{label}</button>
          )}
        </nav>}
        <div className="admin-workspace-content">
          {!plan && !deleteToken && <OperationToast error={error} notice={notice} busy={busy}
            clearError={() => setError("")} clearNotice={() => setNotice("")} />}
          <fieldset disabled={!!busy || disabled} className="admin-fields">
            {visitedChecks.has("duplicates") && <div hidden={tab !== "duplicates"}>
              <DuplicateSkills
                preview={preview}
                showPlan={(p) => {
                  setError("");
                  setNotice("");
                  setConfirmed(false);
                  setPlan(p);
                }}
                revision={skills.map((s) => s.id).join("|")}
              />
            </div>}
            {visitedChecks.has("repair") && <div hidden={tab !== "repair"}>
              <BrokenLinks
                revision={repairRevision}
                showPlan={(p) => {
                  setError("");
                  setNotice("");
                  setConfirmed(false);
                  setPlan(p);
                }}
              />
            </div>}
            {tab === "migration" && (
              <>
                <div className="admin-intro">
                  <FolderInput size={25} />
                  <div>
                    <h3>把 cc-switch 的 Skill 完整带过来</h3>
                    <p>
                      读取 Skill
                      元数据、分组、仓库及历史备份。迁入独立库后，已有客户端软链接同步改指新位置。
                    </p>
                  </div>
                </div>
                <label>
                  cc-switch 数据目录
                  <input
                    aria-label="cc-switch 数据目录"
                    value={ccDir}
                    onChange={(e) => setCcDir(e.target.value)}
                  />
                </label>
                <label>
                  迁移方式
                  <select
                    value={target}
                    onChange={(e) => setTarget(e.target.value)}
                  >
                    <option value="independent">
                      独立技能库 · ~/.skill-manager/skills（推荐）
                    </option>
                    <option value="unified">
                      统一技能库 · ~/.agents/skills
                    </option>
                    <option value="keep">保留源位置，仅接管元数据</option>
                  </select>
                </label>
                <p className="admin-help">
                  预览不会移动源文件。执行前请退出
                  cc-switch，避免它重建旧链接；原数据库和历史备份会保留。无法确定的来源、缺失文件和状态差异会列出说明。
                </p>
                <button
                  className="primary"
                  onClick={() => preview("cc_preview", { ccDir, target })}
                >
                  扫描并预览迁移
                </button>
              </>
            )}
            {tab === "repos" && (
              <>
                <div className="admin-form-row">
                  <label>
                    GitHub 仓库
                    <input
                      placeholder="owner/repo、GitHub URL 或 ccswitch:// Skill 分享链接"
                      value={repo}
                      onChange={(e) => setRepo(e.target.value)}
                    />
                  </label>
                  <label>
                    分支 / 标签
                    <input
                      value={reference}
                      onChange={(e) => setReference(e.target.value)}
                    />
                  </label>
                  <button
                    className="primary"
                    disabled={!repo.trim()}
                    onClick={() =>
                      void act("保存仓库", () =>
                        command("save_repo", {
                          repo: { repo, reference, enabled: true },
                          remove: false,
                        }),
                      )
                    }
                  >
                    保存
                  </button>
                </div>
                <div className="admin-list">
                  {cfg?.repos.map((r) => (
                    <div key={r.repo}>
                      <label>
                        <input
                          type="checkbox"
                          checked={r.enabled}
                          onChange={(e) =>
                            void act("更新订阅", () =>
                              command("save_repo", {
                                repo: { ...r, enabled: e.target.checked },
                                remove: false,
                              }),
                            )
                          }
                        />
                        <span>
                          {r.repo}
                          <small>{r.reference}</small>
                        </span>
                      </label>
                      <button
                        onClick={() => {
                          setRepo(r.repo);
                          setReference(r.reference);
                        }}
                      >
                        编辑
                      </button>
                      <button
                        onClick={() =>
                          void act("移除仓库", () =>
                            command("save_repo", { repo: r, remove: true }),
                          )
                        }
                      >
                        移除
                      </button>
                    </div>
                  ))}
                </div>
                <button
                  className="secondary"
                  disabled={!cfg?.repos.some((r) => r.enabled)}
                  onClick={() =>
                    void act("扫描已启用仓库", async () =>
                      setDiscovered(await command("discover_repos")),
                    )
                  }
                >
                  发现订阅仓库中的 Skill
                </button>
                {discovered.map((r, i) => (
                  <div className="admin-result" key={i}>
                    {r.Err ? (
                      <p className="message error">{r.Err}</p>
                    ) : (
                      <>
                        <strong>{r.Ok?.repo}</strong>
                        <span>{r.Ok?.candidates.length} 个 Skill</span>
                        <button
                          className="primary"
                          onClick={() => {
                            onDiscovery(r.Ok!);
                          }}
                        >
                          查看并安装
                        </button>
                      </>
                    )}
                  </div>
                ))}
              </>
            )}
            {tab === "archive" && (
              <>
                <p className="admin-help">
                  支持根目录 Skill 和多个子目录
                  Skill。先查看名称与描述，再批量安装；同名冲突会停止整个批次。
                </p>
                <button
                  className="primary"
                  onClick={() =>
                    void act("读取本地归档", async () => {
                      const path = await open({
                        multiple: false,
                        filters: [
                          { name: "Skill 归档", extensions: ["zip", "skill"] },
                        ],
                      });
                      if (typeof path === "string") {
                        const d = await command<Discovery>("archive_inspect", {
                          path,
                        });
                        setArchive(d);
                        setPaths(d.candidates.map((c) => c.path));
                      }
                    })
                  }
                >
                  <Archive size={14} />
                  选择 ZIP / .skill
                </button>
                {archive && (
                  <>
                    <div className="admin-skill-picker">
                      {archive.candidates.map((c) => (
                        <label key={c.path}>
                          <input
                            type="checkbox"
                            checked={paths.includes(c.path)}
                            onChange={(e) =>
                              setPaths(
                                e.target.checked
                                  ? [...paths, c.path]
                                  : paths.filter((p) => p !== c.path),
                              )
                            }
                          />
                          <span>
                            {c.name}
                            <small>
                              {c.path} · {c.description}
                            </small>
                          </span>
                        </label>
                      ))}
                    </div>
                    <button
                      className="primary"
                      disabled={!paths.length}
                      onClick={() =>
                        preview("archive_preview", {
                          discovery: archive.token,
                          paths,
                        })
                      }
                    >
                      预览安装 {paths.length} 项
                    </button>
                  </>
                )}
              </>
            )}
            {tab === "backups" && (
              <>
                <p className="admin-help">
                  卸载和升级都会保留完整内容。已安装版本的升级回退可在 Skill
                  详情中操作；此处可将已卸载版本重新安装。
                </p>
                {clientPicker}
                <div className="admin-list">
                  {overview?.backups.map((b) => (
                    <div key={b.token}>
                      <span>
                        <strong>{b.name}</strong>
                        <small>
                          {new Date(b.created * 1000).toLocaleString()} ·{" "}
                          {b.token}
                        </small>
                      </span>
                      <button
                        className="secondary"
                        onClick={() =>
                          preview("restore_preview", {
                            token: b.token,
                            clients: batchClients,
                          })
                        }
                      >
                        预览重新安装
                      </button>
                      <button
                        className="quiet"
                        onClick={() => setDeleteToken(b.token)}
                      >
                        删除备份
                      </button>
                    </div>
                  ))}
                </div>
                {!overview?.backups.length && (
                  <p className="admin-help">暂无版本备份。</p>
                )}
              </>
            )}
            {tab === "storage" && (
              <>
                <h3>源文件存储</h3>
                <p className="admin-help">
                  当前：{cfg?.library || "~/.cc-switch/skills"}。迁移已登记的
                  Skill，并重建指向源的客户端链接。
                </p>
                <div className="admin-buttons">
                  {[
                    ["independent", "独立库"],
                    ["unified", "~/.agents/skills"],
                    ["cc_switch", "~/.cc-switch/skills"],
                  ].map(([target, label]) => (
                    <button
                      key={target}
                      className="secondary"
                      onClick={() =>
                        preview("storage_preview", {
                          target,
                          ids: null,
                          adopt: false,
                        })
                      }
                    >
                      迁移至 {label}
                    </button>
                  ))}
                </div>
                <h3>新投影默认方式</h3>
                <div className="admin-buttons">
                  {[
                    ["symlink", "软链接"],
                    ["copy", "复制"],
                  ].map(([method, label]) => (
                    <button
                      key={method}
                      className={
                        cfg?.syncMethod === method ? "primary" : "secondary"
                      }
                      onClick={() =>
                        void act("保存同步方式", () =>
                          command("sync_method", { method }),
                        )
                      }
                    >
                      {label}
                    </button>
                  ))}
                </div>
                <p className="admin-help">
                  不会立即改写现有投影。在“我的技能”中勾选资源，再进入“客户端与更多操作”切换指定方式；被修改的副本会保留并提示。
                </p>
                <h3>客户端 Skill 目录</h3>
                <div className="admin-form-row">
                  <select
                    value={client}
                    onChange={(e) => setClient(e.target.value)}
                  >
                    {clients.map((c) => (
                      <option key={c}>{c}</option>
                    ))}
                  </select>
                  <input
                    aria-label="客户端 Skill 目录"
                    placeholder={overview?.clientDefaults[client] ?? "正在读取默认目录…"}
                    aria-describedby="client-directory-help"
                    value={clientPath}
                    onChange={(e) => setClientPath(e.target.value)}
                  />
                  <button
                    className="secondary"
                    onClick={() =>
                      void act("保存客户端目录", () =>
                        command("client_path", { client, path: clientPath }),
                      )
                    }
                  >
                    保存
                  </button>
                </div>
                <p id="client-directory-help" className="admin-help">
                  留空时使用提示中的默认目录；自定义目录请填写绝对路径。
                </p>
                <h3>缓存管理</h3>
                <p className="admin-help">
                  清理下载内容和未执行的预览，保留已安装资源、历史备份和已完成的迁移记录。
                </p>
                <button
                  className="secondary"
                  onClick={() =>
                    void act("清理缓存", async () => {
                      await command("clear_cache");
                      setArchive(null);
                      setDiscovered([]);
                      setNotice("下载和预览缓存已清理");
                    })
                  }
                >
                  清理下载与预览缓存
                </button>
                <h3>迁移与管理历史</h3>
                <div className="admin-list">
                  {overview?.history.map((h) => (
                    <div key={h.token}>
                      <span>
                        {h.kind === "deduplicate"
                          ? "重复 Skill 合并"
                          : h.kind === "repair-links"
                            ? "修复断开的软链接"
                            : h.kind}
                        <small>
                          {h.summary[0]} · {h.token}
                        </small>
                      </span>
                      <button
                        className="secondary"
                        onClick={() =>
                          preview("undo_preview", { token: h.token })
                        }
                      >
                        预览整批回退
                      </button>
                      <button className="quiet" aria-label={`删除历史记录 ${h.token}`}
                        onClick={() => void act("删除历史记录", async () => {
                          await command("delete_history", { token: h.token });
                          setDeletedHistory(h.token);
                          setNotice("历史记录已删除，备份和已安装 Skill 保留");
                        })}>
                        删除记录
                      </button>
                    </div>
                  ))}
                  {!overview?.history.length && <p className="admin-help">暂无迁移与管理历史。</p>}
                </div>
                {deletedHistory && <div className="admin-buttons" role="status">
                  <span>已从列表删除一条历史记录</span>
                  <button className="secondary" onClick={() => void act("恢复历史记录", async () => {
                    await command("restore_history", { token: deletedHistory });
                    setDeletedHistory(null);
                    setNotice("历史记录已恢复");
                  })}>撤销上次删除</button>
                </div>}
                <p className="admin-help">
                  整批回退仅在后续管理状态未变化时可执行；有冲突会停止，避免覆盖新操作。
                  删除记录仅将其从历史列表移除，不清理备份或改变已安装 Skill。
                </p>
              </>
            )}
            {tab === "batch" && (
              <>
                <details className="batch-selection-summary"><summary>查看已选的 {ids.length} 项 Skill</summary>
                  <ul>{skills.filter(s => ids.includes(s.id)).map(s => <li key={s.id}>{s.name}<small>{s.source}</small></li>)}</ul>
                </details>
                <h3>选择要操作的客户端</h3>
                {clientPicker}
                <div className="admin-buttons">
                  <button
                    className="primary"
                    disabled={!ids.length || !batchClients.length}
                    onClick={() =>
                      preview("sync_preview", {
                        ids,
                        clients: batchClients,
                        enable: true,
                        method: "symlink",
                      })
                    }
                  >
                    以软链接启用
                  </button>
                  <button
                    className="secondary"
                    disabled={!ids.length || !batchClients.length}
                    onClick={() =>
                      preview("sync_preview", {
                        ids,
                        clients: batchClients,
                        enable: true,
                        method: "copy",
                      })
                    }
                  >
                    以副本启用
                  </button>
                  <button
                    className="secondary"
                    disabled={!ids.length || !batchClients.length}
                    onClick={() =>
                      preview("sync_preview", {
                        ids,
                        clients: batchClients,
                        enable: false,
                        method: null,
                      })
                    }
                  >
                    关闭指定客户端
                  </button>
                </div>
                <div className="admin-buttons">
                  <button
                    className="secondary"
                    disabled={!ids.length}
                    onClick={() =>
                      preview("storage_preview", {
                        ids,
                        target: "independent",
                        adopt: true,
                      })
                    }
                  >
                    接管到独立库
                  </button>
                  <button
                    className="secondary"
                    disabled={!ids.length}
                    onClick={() => preview("uninstall_preview", { ids })}
                  >
                    备份并卸载
                  </button>
                </div>
                <p className="admin-help">
                  接管会将客户端实体源或共享源移入独立库，并保留对应客户端入口。相同名称的不同来源不会合并覆盖。
                </p>
              </>
            )}
          </fieldset>
        </div>
      </section>
      {plan && (
        <Dialog
          title={
            plan.kind === "deduplicate"
              ? "确认保留版本与软链接变更"
              : plan.kind === "repair-links"
                ? "确认修复断开的软链接"
                : "核对文件与管理信息变更"
          }
          close={() => !busy && setPlan(null)}
          wide
        >
          {!deleteToken && <OperationToast error={error} notice={notice} busy={busy}
            clearError={() => setError("")} clearNotice={() => setNotice("")} />}
          <p className="admin-help">
            {plan.moves.length} 项文件移动 · 管理记录{" "}
            {Object.keys(plan.before.records).length} →{" "}
            {Object.keys(plan.after.records).length}
          </p>
          <div
            className={`admin-preview ${plan.kind === "repair-links" ? "repair-preview" : ""}`}
          >
            {plan.summary.map((s, i) => (
              <p key={i}>{s}</p>
            ))}
          </div>
          {plan.warnings.length > 0 && (
            <div className="admin-warnings">
              {plan.warnings.map((w, i) => (
                <p key={i}>{w}</p>
              ))}
            </div>
          )}
          <details className="admin-moves">
            <summary>查看完整文件路径</summary>
            {plan.moves.map((m, i) => (
              <p key={i}>
                <code>{m.from}</code>
                <span>→</span>
                <code>{m.to}</code>
              </p>
            ))}
          </details>
          <label className="local-confirm">
            <input
              type="checkbox"
              checked={confirmed}
              onChange={(e) => setConfirmed(e.target.checked)}
            />
            <span>
              我已核对来源、目标和提示，同意执行以上变更。
              {plan.kind === "cc-migration" && "迁移时已退出 cc-switch。"}
            </span>
          </label>
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
              disabled={!confirmed || !!busy}
              onClick={() =>
                void act("执行并验证操作", async () => {
                  await command("apply", { token: plan.token });
                  setPlan(null);
                  setRepairRevision((value) => value + 1);
                  setNotice("操作完成，文件及管理信息已验证");
                })
              }
            >
              {plan.kind === "deduplicate"
                ? "确认应用并建立软链接"
                : plan.kind === "repair-links"
                  ? "确认修复软链接"
                  : "确认执行"}
            </button>
          </footer>
        </Dialog>
      )}
      {deleteToken && (
        <Dialog
          title="永久删除这份备份？"
          close={() => !busy && setDeleteToken(null)}
        >
          <p className="admin-help">
            仅删除备份 {deleteToken}，不影响正在使用的
            Skill。删除后不能用此备份恢复。
          </p>
          <OperationToast error={error} notice={notice} busy={busy}
            clearError={() => setError("")} clearNotice={() => setNotice("")} />
          <footer>
            <button
              className="secondary"
              disabled={!!busy}
              onClick={() => setDeleteToken(null)}
            >
              取消
            </button>
            <button
              className="primary"
              disabled={!!busy}
              onClick={() =>
                void act("删除备份", async () => {
                  await command("delete_backup", { token: deleteToken });
                  setDeleteToken(null);
                })
              }
            >
              永久删除备份
            </button>
          </footer>
        </Dialog>
      )}
    </>
  );
}
