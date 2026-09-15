import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { Link2Off, LoaderCircle } from "lucide-react";
import type { AdminPlan } from "./adminTypes";

type Report = {
  targetRoot: string;
  links: {
    path: string;
    name: string;
    oldTarget: string;
    target: string | null;
    problem: string | null;
  }[];
  warnings: string[];
};

export function BrokenLinks({
  revision,
  showPlan,
}: {
  revision: number;
  showPlan: (plan: AdminPlan) => void;
}) {
  const [targetRoot, setTargetRoot] = useState("");
  const [report, setReport] = useState<Report | null>(null);
  const [selected, setSelected] = useState<string[]>([]);
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");
  const request = useRef(0);
  const pending = useRef(false);

  async function scan(root: string) {
    const current = ++request.current;
    pending.current = true;
    setBusy("扫描并匹配同名 Skill");
    setError("");
    setReport(null);
    setSelected([]);
    try {
      const result = await invoke<Report>("global_admin", {
        action: "broken_links",
        args: { targetRoot: root },
      });
      if (current !== request.current) return;
      setReport(result);
      setTargetRoot(result.targetRoot);
    } catch (e) {
      if (current === request.current) setError(String(e));
    } finally {
      if (current === request.current) {
        pending.current = false;
        setBusy("");
      }
    }
  }
  useEffect(() => {
    void scan(targetRoot);
    return () => {
      request.current++;
    };
  }, [revision]);

  async function preview() {
    if (pending.current || !report || !selected.length) return;
    pending.current = true;
    const current = ++request.current;
    setBusy("生成软链接修复预览");
    setError("");
    try {
      const plan = await invoke<AdminPlan>("global_admin", {
        action: "repair_links_preview",
        args: { targetRoot: report.targetRoot, paths: selected },
      });
      if (current === request.current) showPlan(plan);
    } catch (e) {
      if (current === request.current) setError(String(e));
    } finally {
      if (current === request.current) {
        pending.current = false;
        setBusy("");
      }
    }
  }
  function changeRoot(value: string) {
    setTargetRoot(value);
    setReport(null);
    setSelected([]);
    setError("");
  }
  const matched = report?.links.filter((link) => link.target) ?? [];
  return (
    <section className="broken-links">
      <div className="admin-intro">
        <Link2Off size={25} />
        <div>
          <h3>修复失效的 Skill 链接</h3>
          <p>
            自动检查断链并寻找可用的同名 Skill。选中需要修复的项目，再预览新指向；原链接会备份。
          </p>
        </div>
      </div>
      {error && (
        <div className="message error" role="alert">
          {error}
        </div>
      )}
      <fieldset className="admin-fields" disabled={!!busy}>
        <details className="repair-match-settings">
          <summary>匹配目录与扫描设置</summary>
        <label htmlFor="repair-target-directory">目标技能目录</label>
        <div className="repair-directory">
          <input
            id="repair-target-directory"
            aria-label="断链修复目标技能目录"
            placeholder="例如 ~/.skill-manager/skills"
            value={targetRoot}
            onChange={(e) => changeRoot(e.target.value)}
          />
          <button
            className="secondary"
            onClick={() =>
              void (async () => {
                try {
                  const path = await open({
                    directory: true,
                    multiple: false,
                    title: "选择存放同名 Skill 的父目录",
                  });
                  if (typeof path === "string") {
                    changeRoot(path);
                    void scan(path);
                  }
                } catch (e) {
                  setError(String(e));
                }
              })()
            }
          >
            选择目录
          </button>
          <button className="secondary" onClick={() => void scan(targetRoot)}>
            扫描匹配
          </button>
        </div>
        <p className="admin-help">
          扫描全局技能库、共享 Skill 目录和客户端目录中的断链；匹配“目标目录 /
          同名文件夹 / SKILL.md”，使用解析后的实际目录建立链接。
        </p>
        </details>
        {report && (
          <>
            <div className="repair-toolbar">
              <span>
                发现 {report.links.length} 个断链 · 可修复 {matched.length} 个 ·
                已选 {selected.length} 个
              </span>
              <button
                className="quiet"
                disabled={!matched.length}
                onClick={() =>
                  setSelected(
                    selected.length === matched.length
                      ? []
                      : matched.map((link) => link.path),
                  )
                }
              >
                {selected.length && selected.length === matched.length
                  ? "取消全选"
                  : "全选可修复项"}
              </button>
              <button
                className="primary"
                disabled={!selected.length}
                onClick={() => void preview()}
              >
                预览修复{selected.length ? ` ${selected.length} 个软链接` : ""}
              </button>
            </div>
            <div className="repair-list">
              {report.links.map((link) => (
                <label
                  key={link.path}
                  className={`repair-row ${link.target ? "" : "unmatched"}`}
                >
                  <input
                    type="checkbox"
                    aria-label={`修复 ${link.path}`}
                    disabled={!link.target}
                    checked={selected.includes(link.path)}
                    onChange={(e) =>
                      setSelected(
                        e.target.checked
                          ? [...selected, link.path]
                          : selected.filter((path) => path !== link.path),
                      )
                    }
                  />
                  <span>
                    <strong>{link.name}</strong>
                    <small>
                      断链位置 <code>{link.path}</code>
                    </small>
                    <small>
                      原指向 <code>{link.oldTarget}</code>
                    </small>
                    {link.target ? (
                      <small className="repair-target">
                        新指向 <code>{link.target}</code>
                      </small>
                    ) : (
                      <small className="repair-problem">
                        无法匹配：{link.problem}
                      </small>
                    )}
                  </span>
                </label>
              ))}
              {!report.links.length && (
                <div className="empty-inline">
                  没有发现断开的 Skill 软链接。
                </div>
              )}
            </div>
            {report.warnings.length > 0 && (
              <div className="admin-warnings">
                {report.warnings.map((w, i) => (
                  <p key={i}>{w}</p>
                ))}
              </div>
            )}
          </>
        )}
      </fieldset>
      {busy && (
        <div className="global-progress" role="status">
          <LoaderCircle size={14} className="spin" />
          {busy}…
        </div>
      )}
    </section>
  );
}
