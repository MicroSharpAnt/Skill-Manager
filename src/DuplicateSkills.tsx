import { MarkdownPreview } from "./MarkdownPreview";
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Check, Files, RefreshCw, Search } from "lucide-react";
import { Dialog } from "./GlobalDialog";
import { SkillDiffDialog } from "./SkillDiffDialog";
import type { AdminPlan } from "./adminTypes";
import "./duplicates.css";

type Source = {
  id: string;
  source: string;
  digest: string | null;
  tree: Record<string, string> | null;
  permissions: Record<string, number> | null;
  problem: string | null;
  managed: boolean;
  origin: { repo: string; reference: string; path: string } | null;
};
type Group = { name: string; sources: Source[] };
type Report = { groups: Group[]; warnings: string[] };
function differences(keep: Source, other: Source) {
  if (!keep.tree || !other.tree) return [];
  const paths = [
    ...new Set([...Object.keys(keep.tree), ...Object.keys(other.tree)]),
  ].sort();
  const content = (s: string | undefined) =>
    s?.replace(/^file:(true|false):/, "file:");
  return paths.flatMap((path) => {
    const labels: string[] = [];
    if (!(path in keep.tree!)) labels.push("仅此版本存在");
    else if (!(path in other.tree!)) labels.push("此版本缺少");
    else {
      if (content(keep.tree![path]) !== content(other.tree![path]))
        labels.push("内容或类型不同");
      if (keep.permissions?.[path] !== other.permissions?.[path])
        labels.push(
          `权限不同：${other.permissions?.[path]?.toString(8) ?? "未知"} → ${keep.permissions?.[path]?.toString(8) ?? "未知"}`,
        );
    }
    return labels.length
      ? [{ path: path === "/" ? "根目录" : path, label: labels.join("；") }]
      : [];
  });
}
function groupReason(g: Group) {
  if (g.sources.some((s) => s.problem)) return "无法比较（见具体原因）";
  const changes = g.sources
    .slice(1)
    .flatMap((s) => differences(g.sources[0], s));
  if (!changes.length) return "内容完全相同";
  return changes.every((d) => d.label.startsWith("权限不同"))
    ? "仅权限不同"
    : "文件内容或结构不同";
}
export function DuplicateSkills({
  preview,
  showPlan,
  revision,
}: {
  preview: (action: string, args: Record<string, unknown>) => void;
  revision: unknown;
  showPlan: (plan: AdminPlan) => void;
}) {
  const [report, setReport] = useState<Report | null>(null);
  const [kept, setKept] = useState<Record<string, string>>({});
  const [unify, setUnify] = useState<Record<string, boolean>>({});
  const [selected, setSelected] = useState<string[]>([]);
  const [query, setQuery] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [detail, setDetail] = useState<{
    name: string;
    source: string;
    body: string;
  } | null>(null);
  const [comparison, setComparison] = useState<{
    group: Group;
    left: string;
    right: string;
  } | null>(null);
  const generation = useRef(0);
  async function scan() {
    const seq = ++generation.current;
    setBusy(true);
    setError("");
    setSelected([]);
    setUnify({});
    setReport(null);
    try {
      const data = await invoke<Report>("global_admin", {
        action: "duplicates",
        args: {},
      });
      if (seq !== generation.current) return;
      setReport(data);
      setKept(
        Object.fromEntries(
          data.groups.map((g) => [
            g.name,
            (g.sources.find((s) => !s.problem) ?? g.sources[0]).id,
          ]),
        ),
      );
    } catch (e) {
      if (seq === generation.current) setError(String(e));
    } finally {
      if (seq === generation.current) setBusy(false);
    }
  }
  useEffect(() => {
    setComparison(null);
    void scan();
    return () => {
      generation.current++;
    };
  }, [revision]);
  function matches(g: Group) {
    const keep = g.sources.find((s) => s.id === kept[g.name]);
    if (!keep?.digest || (unify[g.name] && g.sources.some((s) => s.problem)))
      return [];
    return g.sources.filter(
      (s) => s.id !== keep.id && (unify[g.name] || s.digest === keep.digest),
    );
  }
  const groups = report?.groups ?? [];
  const visible = groups.filter((g) =>
    (g.name + " " + g.sources.map((s) => s.source).join(" "))
      .toLowerCase()
      .includes(query.toLowerCase()),
  );
  const eligible = visible.filter((g) => matches(g).length > 0);
  const choices = groups
    .filter((g) => selected.includes(g.name) && matches(g).length > 0)
    .map((g) => ({
      keepId: kept[g.name],
      duplicateIds: matches(g).map((s) => s.id),
      allowDifferent: !!unify[g.name],
    }));
  const copies = choices.reduce((n, c) => n + c.duplicateIds.length, 0);
  async function read(g: Group, s: Source) {
    setDetail({ name: g.name, source: s.source, body: "正在读取内容…" });
    try {
      const body = await invoke<string>("global_details", { id: s.id });
      setDetail((d) => (d?.source === s.source ? { ...d, body } : d));
    } catch (e) {
      setDetail((d) =>
        d?.source === s.source ? { ...d, body: String(e) } : d,
      );
    }
  }
  return (
    <section className="duplicates">
      <div className="duplicate-intro">
        <Files size={22} />
        <div>
          <h3>检查重复 Skill</h3>
          <p>
            同一实际目录的软链接已自动归为一项。这里检查不同目录中的同名
            Skill，比较全部文件与权限。
          </p>
        </div>
      </div>
      <div className="duplicate-toolbar">
        <label className="search">
          <Search size={15} />
          <input
            autoCorrect="off"
            autoCapitalize="none"
            spellCheck={false}
            aria-label="搜索重复 Skill"
            placeholder="搜索名称或实际路径…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </label>
        <button
          className="secondary"
          disabled={busy}
          onClick={() => void scan()}
        >
          <RefreshCw size={14} className={busy ? "spin" : ""} />
          重新检查
        </button>
      </div>
      {error && (
        <div className="message error" role="alert">
          {error}
        </div>
      )}
      {busy && (
        <p role="status" className="admin-help">
          正在比较 Skill 目录…
        </p>
      )}
      {report && (
        <>
          <div className="duplicate-selection">
            <span>
              {groups.length} 组同名来源 · 当前可处理 {eligible.length} 组
            </span>
            <div>
              <button
                className="quiet"
                disabled={!eligible.length}
                onClick={() =>
                  setSelected([
                    ...new Set([...selected, ...eligible.map((g) => g.name)]),
                  ])
                }
              >
                选择当前可处理项
              </button>
              <button
                className="quiet"
                disabled={!selected.length}
                onClick={() => setSelected([])}
              >
                清空选择
              </button>
            </div>
          </div>
          {report.warnings.length > 0 && (
            <details className="admin-warnings">
              <summary>扫描提示（{report.warnings.length}）</summary>
              {report.warnings.map((w, i) => (
                <p key={i}>{w}</p>
              ))}
            </details>
          )}
          <div className="duplicate-groups">
            {visible.map((g) => {
              const keep = g.sources.find((s) => s.id === kept[g.name])!;
              const peers = matches(g);
              const reason = groupReason(g);
              return (
                <article className="duplicate-group" key={g.name}>
                  <div className="duplicate-group-heading">
                    <label>
                      <input
                        type="checkbox"
                        aria-label={`合并 ${g.name}`}
                        checked={selected.includes(g.name)}
                        disabled={!peers.length}
                        onChange={(e) =>
                          setSelected(
                            e.target.checked
                              ? [...selected, g.name]
                              : selected.filter((n) => n !== g.name),
                          )
                        }
                      />
                      <strong>{g.name}</strong>
                    </label>
                    <span
                      className={
                        reason === "内容完全相同"
                          ? "duplicate-equal"
                          : "duplicate-different"
                      }
                    >
                      {reason}
                    </span>
                  </div>
                  <p className="duplicate-hint">
                    选择保留源 · {g.sources.length} 个实际目录
                    {peers.length > 0
                      ? ` · 将处理 ${peers.length} 份${unify[g.name] ? "其他版本" : "相同副本"}`
                      : " · 当前模式下没有可处理的副本"}
                  </p>
                  <div className="duplicate-compare-entry">
                    <button
                      className="secondary"
                      onClick={() =>
                        setComparison({
                          group: g,
                          left: keep.id,
                          right: g.sources.find((s) => s.id !== keep.id)!.id,
                        })
                      }
                    >
                      比较版本 Diff
                    </button>
                  </div>
                  <label className="duplicate-mode">
                    处理方式
                    <select
                      aria-label={`${g.name} 的处理方式`}
                      value={unify[g.name] ? "unify" : "identical"}
                      onChange={(e) => {
                        setUnify({
                          ...unify,
                          [g.name]: e.target.value === "unify",
                        });
                        setSelected(selected.filter((n) => n !== g.name));
                      }}
                    >
                      <option value="identical">仅合并相同副本</option>
                      <option value="unify">仅保留所选版本</option>
                    </select>
                  </label>
                  {unify[g.name] && (
                    <p className="duplicate-hint duplicate-unify">
                      其他版本将移出原目录并完整备份，使用入口改为所选版本的软链接。
                      {g.sources.some((s) => s.problem) &&
                        "有目录无法读取，请先根据提示修复后重新检查。"}
                    </p>
                  )}
                  <div role="radiogroup" aria-label={`${g.name} 的保留源`}>
                    {g.sources.map((s) => (
                      <div
                        key={s.id}
                        className={
                          "duplicate-source " +
                          (keep.id === s.id ? "is-kept" : "")
                        }
                      >
                        <label>
                          <input
                            type="radio"
                            name={`keep-${g.name}`}
                            checked={keep.id === s.id}
                            disabled={!!s.problem}
                            aria-label={`保留 ${s.source}`}
                            onChange={() => {
                              setKept({ ...kept, [g.name]: s.id });
                              setSelected(selected.filter((n) => n !== g.name));
                            }}
                          />
                          <span>
                            <code>{s.source}</code>
                            {s.origin && (
                              <small>
                                {s.origin.repo} · {s.origin.reference} ·{" "}
                                {s.origin.path}
                              </small>
                            )}
                            {s.problem && (
                              <small className="duplicate-problem">
                                {s.problem}
                              </small>
                            )}
                          </span>
                        </label>
                        <span className="duplicate-role">
                          {keep.id === s.id ? (
                            <>
                              <Check size={13} />
                              保留源
                            </>
                          ) : s.problem ? (
                            "需先修复"
                          ) : unify[g.name] ? (
                            "备份移除 → 使用保留源"
                          ) : s.digest && s.digest === keep.digest ? (
                            "相同副本 → 软链接"
                          ) : (
                            "保留独立版本"
                          )}
                        </span>
                        {s.id !== keep.id &&
                          !s.problem &&
                          differences(keep, s).length > 0 && (
                            <details className="duplicate-diff">
                              <summary>
                                与保留源的差异（{differences(keep, s).length}{" "}
                                项）
                              </summary>
                              {differences(keep, s)
                                .slice(0, 50)
                                .map((d) => (
                                  <p key={d.path}>
                                    <code>{d.path}</code>
                                    <span>{d.label}</span>
                                  </p>
                                ))}
                              {differences(keep, s).length > 50 && (
                                <p>仅展示前 50 项；比较和备份覆盖整个目录。</p>
                              )}
                            </details>
                          )}
                        <button
                          className="quiet"
                          onClick={() => void read(g, s)}
                        >
                          查看内容
                        </button>
                      </div>
                    ))}
                  </div>
                </article>
              );
            })}
          </div>
          {!visible.length && (
            <div className="empty-inline">
              {groups.length
                ? "没有匹配项，请调整搜索。"
                : "未发现不同实际目录中的同名 Skill。"}
            </div>
          )}
        </>
      )}
      <div className="duplicate-footer">
        <div>
          <strong>
            已选 {choices.length} 组 · {copies} 份副本
          </strong>
          <small>先预览再执行。其他版本完整备份，可从备份中心回退。</small>
        </div>
        <button
          className="primary"
          disabled={busy || !choices.length}
          onClick={() => preview("duplicates_preview", { choices })}
        >
          预览处理
        </button>
      </div>
      {comparison && (
        <SkillDiffDialog
          name={comparison.group.name}
          sources={comparison.group.sources}
          initialLeft={comparison.left}
          initialRight={comparison.right}
          showPlan={showPlan}
          close={() => setComparison(null)}
        />
      )}
      {detail && (
        <Dialog title={detail.name} wide resizable close={() => setDetail(null)}>
          <p className="admin-help">
            <code>{detail.source}</code>
          </p>
          <MarkdownPreview content={detail.body} />
        </Dialog>
      )}
    </section>
  );
}
