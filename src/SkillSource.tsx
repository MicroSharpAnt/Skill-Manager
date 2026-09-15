import { useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { ArrowUpRight } from "lucide-react";
import "./skill-source.css";

export type Origin = { repo: string; reference: string; path: string };
export type DownloadSource =
  | { kind: "remote"; origin: Origin }
  | { kind: "local"; path: string }
  | { kind: "unknown" };

export function sourceLabel(source?: DownloadSource) {
  if (source?.kind === "remote") return source.origin.repo;
  if (source?.kind === "local") return `本地导入 · ${source.path}`;
  return "来源未记录";
}

export function sourceUrl(origin: Origin) {
  const segments = [...origin.repo.split("/"), "tree", origin.reference,
    ...(origin.path === "." ? [] : origin.path.split("/"))];
  return "https://github.com/" + segments.map(encodeURIComponent).join("/");
}

export function SourceLink({ origin, label }: { origin: Origin; label?: string }) {
  const [error, setError] = useState("");
  return <span className="skill-source-link">
    <a href={sourceUrl(origin)} target="_blank" rel="noopener noreferrer"
      onClick={async event => {
        if (!isTauri()) return;
        event.preventDefault();
        setError("");
        try { await invoke("open_skill_source", { origin }); }
        catch (e) { setError(String(e)); }
      }}>
      {label ?? origin.repo}<ArrowUpRight size={14} />
    </a>
    {error && <span className="skill-source-error" role="alert">{error}</span>}
  </span>;
}

export function DownloadDetails({ source }: { source?: DownloadSource }) {
  if (source?.kind === "remote") return <div className="skill-download-details">
    <span>最初下载来源：</span><SourceLink origin={source.origin} />
    <small>{source.origin.reference} · {source.origin.path === "." ? "仓库根目录" : source.origin.path}</small>
  </div>;
  if (source?.kind === "local") return <div className="skill-download-details">
    <span>最初来源：本地导入</span><code>{source.path}</code>
  </div>;
  return <div className="skill-download-details"><span>最初下载来源：未记录</span>
    <small>历史安装未保存来源地址，无法确定原始下载网页。</small>
  </div>;
}
