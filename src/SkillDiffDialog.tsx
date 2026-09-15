import { TextDiff, type Comparison } from "./TextDiff";
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ArrowLeftRight, RefreshCw } from "lucide-react";
import { Dialog } from "./GlobalDialog";
import "./skill-diff.css";
import type { AdminPlan } from "./adminTypes";

type Source = { id: string; source: string; problem: string | null };
const labels: Record<string, string> = {
  added: "仅右侧",
  removed: "仅左侧",
  changed: "内容 / 类型",
  permissions: "权限",
  same: "相同",
};
export function SkillDiffDialog({
  name,
  sources,
  initialLeft,
  initialRight,
  showPlan,
  close,
}: {
  name: string;
  sources: Source[];
  initialLeft: string;
  initialRight: string;
  showPlan: (plan: AdminPlan) => void;
  close: () => void;
}) {
  const [left, setLeft] = useState(initialLeft),
    [right, setRight] = useState(initialRight);
  const [path, setPath] = useState<string | null>(null);
  const [result, setResult] = useState<{
    pair: string;
    data: Comparison;
  } | null>(null);
  const [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  const [retry, setRetry] = useState(0),
    [changedOnly, setChangedOnly] = useState(true);
  const [preparing, setPreparing] = useState(false);
  const [previewError, setPreviewError] = useState("");
  const pendingPreview = useRef(false);
  const pair = `${left}:${right}`;
  const data = result?.pair === pair ? result.data : null;
  const file = data?.path === path && path !== null && !busy ? data : null;
  useEffect(() => {
    let live = true;
    setBusy(true);
    setError("");
    invoke<Comparison>("global_admin", {
      action: "duplicate_compare",
      args: { leftId: left, rightId: right, path },
    })
      .then((data) => {
        if (!live) return;
        setResult({ pair, data });
        if (path === null)
          setPath(
            data.files.find((f) => f.path === "SKILL.md" && f.status !== "same")
              ?.path ??
              data.files.find((f) => f.status !== "same")?.path ??
              data.files.find((f) => f.path === "SKILL.md")?.path ??
              data.files[0]?.path ??
              null,
          );
      })
      .catch((e) => {
        if (live) {
          setError(String(e));
          setResult(null);
        }
      })
      .finally(() => {
        if (live) setBusy(false);
      });
    return () => {
      live = false;
    };
  }, [left, right, path, retry]);
  function changePair(a: string, b: string) {
    if (pendingPreview.current) return;
    setPreviewError("");
    setLeft(a);
    setRight(b);
    setPath(null);
    setResult(null);
  }
  async function prepareLink(keepId: string, replaceId: string) {
    if (pendingPreview.current) return;
    pendingPreview.current = true;
    setPreparing(true);
    setPreviewError("");
    try {
      const plan = await invoke<AdminPlan>("global_admin", {
        action: "duplicates_preview",
        args: {
          choices: [
            { keepId, duplicateIds: [replaceId], allowDifferent: true },
          ],
        },
      });
      showPlan(plan);
    } catch (e) {
      setPreviewError(String(e));
    } finally {
      pendingPreview.current = false;
      setPreparing(false);
    }
  }
  const cannotApply =
    preparing ||
    busy ||
    !!error ||
    !data ||
    sources.some((s) => [left, right].includes(s.id) && s.problem != null);
  const visible =
    data?.files.filter((f) => !changedOnly || f.status !== "same") ?? [];
  return (
    <Dialog
      title={`${name} · 版本 Diff`}
      wide
      close={() => !pendingPreview.current && close()}
    >
      <p className="admin-help">
        从 {sources.length} 个实际来源中任选两份比较。红色 − 为仅左侧的行，绿色
        + 为仅右侧的行。
      </p>
      <div className="skill-diff-sources">
        <label>
          左侧来源
          <select
            disabled={preparing}
            aria-label="Diff 左侧来源"
            value={left}
            onChange={(e) =>
              changePair(
                e.target.value,
                e.target.value === right ? left : right,
              )
            }
          >
            {sources.map((s) => (
              <option value={s.id} key={s.id}>
                {s.source}
              </option>
            ))}
          </select>
        </label>
        <button
          className="secondary"
          disabled={preparing}
          aria-label="交换 Diff 左右来源"
          onClick={() => changePair(right, left)}
        >
          <ArrowLeftRight size={15} />
        </button>
        <label>
          右侧来源
          <select
            disabled={preparing}
            aria-label="Diff 右侧来源"
            value={right}
            onChange={(e) =>
              changePair(e.target.value === left ? right : left, e.target.value)
            }
          >
            {sources.map((s) => (
              <option value={s.id} key={s.id}>
                {s.source}
              </option>
            ))}
          </select>
        </label>
      </div>
      <section className="skill-diff-apply" aria-label="应用当前比较版本">
        <div>
          <strong>选好版本后，直接应用</strong>
          <p>
            点击后预览备份和软链接变更，确认后生效。
            {sources.length > 2 && "仅处理当前左右两份，其他来源保持不变。"}
          </p>
        </div>
        <div className="skill-diff-apply-buttons">
          <button
            className="secondary"
            disabled={cannotApply}
            onClick={() => void prepareLink(left, right)}
          >
            保留左侧，右侧改为软链接
          </button>
          <button
            className="secondary"
            disabled={cannotApply}
            onClick={() => void prepareLink(right, left)}
          >
            保留右侧，左侧改为软链接
          </button>
        </div>
        {preparing && <p role="status">正在准备备份与软链接预览…</p>}
        {previewError && (
          <div className="message error" role="alert">
            {previewError}
          </div>
        )}
      </section>
      <div className="skill-diff-toolbar">
        <label>
          <input
            type="checkbox"
            checked={changedOnly}
            onChange={(e) => setChangedOnly(e.target.checked)}
          />
          仅显示差异文件
        </label>
        <span>
          {data
            ? `${data.files.filter((f) => f.status !== "same").length} 项差异 / ${data.files.length} 项目录条目`
            : ""}
        </span>
        <button
          className="quiet"
          onClick={() => setRetry(retry + 1)}
          disabled={busy}
        >
          <RefreshCw size={14} />
          重新读取
        </button>
      </div>
      {error && (
        <div className="message error" role="alert">
          {error}
        </div>
      )}
      <div className="skill-diff-workspace">
        <nav className="skill-diff-files" aria-label="Diff 文件列表">
          {visible.map((f) => (
            <button
              key={f.path}
              className={path === f.path ? "active" : ""}
              onClick={() => setPath(f.path)}
              aria-pressed={path === f.path}
            >
              <code>{f.path || "根目录"}</code>
              <small>{labels[f.status]}</small>
            </button>
          ))}
          {data && !visible.length && (
            <p>没有差异条目。可取消筛选查看相同文件。</p>
          )}
        </nav>
        <div className="skill-diff-view">
          <h3>{path === "" ? "根目录" : (path ?? "选择文件")}</h3>
          {busy && (
            <p className="admin-help" role="status">
              正在读取并比较文件…
            </p>
          )}
          {file && <TextDiff file={file} />}
        </div>
      </div>
      <footer className="skill-diff-footer">
        <span>原副本会完整备份，已应用的操作可在备份中心回退。</span>
        <button className="secondary" disabled={preparing} onClick={close}>
          返回重复检查
        </button>
      </footer>
    </Dialog>
  );
}
