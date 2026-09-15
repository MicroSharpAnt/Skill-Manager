import { useState } from "react";
import { patchRows, splitPatchRows } from "./diffRows";
import "./skill-diff.css";
export type Side = {
  kind: string;
  permissions: number;
  bytes: number;
  lineEndings: string;
  notice: string | null;
};
export type Comparison = {
  leftSource: string;
  rightSource: string;
  files: { path: string; status: string }[];
  path: string | null;
  left: Side | null;
  right: Side | null;
  patch: string;
  notice: string | null;
};
function SideInfo({ side }: { side: Side | null }) {
  if (!side) return <span>此侧不存在</span>;
  return (
    <>
      <span>
        {{ file: "文件", directory: "目录", symlink: "软链接目标" }[
          side.kind
        ] ?? side.kind}{" "}
        · 权限 {side.permissions.toString(8)}
        {side.kind === "file" && ` · ${side.bytes.toLocaleString()} 字节`}
        {side.lineEndings && ` · ${side.lineEndings}`}
      </span>
      {side.notice && <strong>{side.notice}</strong>}
    </>
  );
}

export function TextDiff({ file, leftLabel = "左侧", rightLabel = "右侧", defaultLayout = "unified", allowLayout = false }: {
  file: Comparison; leftLabel?: string; rightLabel?: string; defaultLayout?: "split" | "unified"; allowLayout?: boolean;
}) {
  const [layout, setLayout] = useState(defaultLayout);
  return <>
    {allowLayout && <div className="text-diff-layout" role="group" aria-label="Diff 显示方式">
      <button className="quiet" aria-pressed={layout === "split"} onClick={() => setLayout("split")}>左右并排</button>
      <button className="quiet" aria-pressed={layout === "unified"} onClick={() => setLayout("unified")}>逐行对比</button>
      <span>红色 − 移除 · 绿色 + 新增 · 显示变更及相邻上下文</span>
    </div>}
    <div className="skill-diff-meta"><div><b>{leftLabel}</b><SideInfo side={file.left} /></div><div><b>{rightLabel}</b><SideInfo side={file.right} /></div></div>
    {file.notice && <p className="admin-help" role="status">{file.notice}</p>}
    {file.patch && (layout === "split" ? <div className="split-diff" aria-label="左右文本 Diff">
      <div className="split-diff-header"><b>{leftLabel}</b><b>{rightLabel}</b></div>
      {splitPatchRows(file.patch).map((row, i) => row.note ? <div className="split-diff-note" key={i}>{row.note}</div> : <div className="split-diff-row" key={i}>
        <div className={`split-diff-cell ${row.left?.type ?? "empty"}`}><span className="line-no">{row.left?.left}</span><span>{row.left?.mark}</span><code>{row.left?.text}</code></div>
        <div className={`split-diff-cell ${row.right?.type ?? "empty"}`}><span className="line-no">{row.right?.right}</span><span>{row.right?.mark}</span><code>{row.right?.text}</code></div>
      </div>)}
    </div> : <div className="skill-diff-code" aria-label="逐行 Diff">
      <div className="skill-diff-legend"><span>左行</span><span>右行</span><span /><span>文件内容</span></div>
      {patchRows(file.patch).map((row, i) => <div key={i} className={`skill-diff-line ${row.type}`}><span className="line-no">{row.left}</span><span className="line-no">{row.right}</span><span>{row.mark}</span><code>{row.text}</code></div>)}
    </div>)}
  </>;
}
