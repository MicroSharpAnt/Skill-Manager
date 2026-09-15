import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { TextDiff, type Comparison } from "./TextDiff";

const labels: Record<string, string> = { added: "新增", removed: "移除", changed: "修改", permissions: "权限变化", same: "相同" };
export function PlanDiff({ token, rightLabel, onReady }: { token: string; rightLabel: string; onReady: (ready: boolean) => void }) {
  const [path, setPath] = useState("SKILL.md");
  const [file, setFile] = useState<Comparison | null>(null);
  const [files, setFiles] = useState<Comparison["files"]>([]);
  const [error, setError] = useState("");
  const [retry, setRetry] = useState(0);
  useEffect(() => {
    let live = true;
    setFile(null); setError(""); onReady(false);
    invoke<Comparison>("global_admin", { action: "plan_compare", args: { token, path } })
      .then(result => { if (live) { setFile(result); setFiles(result.files); onReady(true); } })
      .catch(e => { if (live) setError(String(e)); });
    return () => { live = false; };
  }, [token, path, retry, onReady]);
  return <section className="plan-diff" aria-label="更新文件 Diff">
    <h3>版本差异</h3>
    {error && <div className="message error" role="alert">{error}<button className="secondary" onClick={() => setRetry(n => n + 1)}>重新读取 Diff</button></div>}
    <div className="skill-diff-workspace">
      <nav className="skill-diff-files" aria-label="更新 Diff 文件列表">
        {files.map(f => <button key={f.path} aria-pressed={path === f.path} className={path === f.path ? "active" : ""} onClick={() => setPath(f.path)}><code>{f.path || "根目录"}</code><small>{labels[f.status]}</small></button>)}
      </nav>
      <div className="skill-diff-view">
        <h3>{path || "根目录"}</h3>
        {!file && !error && <p className="admin-help" role="status">正在生成文本 Diff…</p>}
        {file && <TextDiff file={file} leftLabel="当前本地版本" rightLabel={rightLabel} defaultLayout="split" allowLayout />}
      </div>
    </div>
  </section>;
}
