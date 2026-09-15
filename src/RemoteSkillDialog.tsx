import { InstallStatusBadge, type InstallStatus } from "./InstallStatus";
import { SourceLink } from "./SkillSource";
import { MarkdownPreview } from "./MarkdownPreview";
import { useEffect, useState } from "react";
import { Download, FileText, LoaderCircle, RefreshCw } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { Dialog } from "./GlobalDialog";

type Discovery = {
  token: string;
  repo: string;
  reference: string;
  candidates: { path: string; name: string; description: string }[];
};
type Details = {
  name: string;
  description: string;
  repo: string;
  reference: string;
  path: string;
  content: string;
  files: string[];
};
export type RemoteRequest =
  | {
      market: { name: string; repo: string; skillId: string };
      discovery?: never;
      path?: never;
    }
  | { discovery: Discovery; path: string; market?: never };

export function RemoteSkillDialog({
  request,
  close,
  install,
  busy,
  error,
  clearError,
  upgrading,
  openLocal,
}: {
  request: RemoteRequest;
  close: () => void;
  install: (discovery: Discovery, path: string, existing?: string) => void;
  openLocal: (id: string) => void;
  busy: string;
  error: string;
  clearError: () => void;
  upgrading: boolean;
}) {
  const [discovery, setDiscovery] = useState<Discovery | null>(
    request.discovery ?? null,
  );
  const [path, setPath] = useState(request.path ?? "");
  const [details, setDetails] = useState<Details | null>(null);
  const [loadingRepo, setLoadingRepo] = useState(!request.discovery);
  const [loadingContent, setLoadingContent] = useState(false);
  const [loadError, setLoadError] = useState("");
  const [installStatus, setInstallStatus] = useState<InstallStatus | undefined>();
  const [retry, setRetry] = useState(0);
  useEffect(() => {
    let live = true;
    setLoadError("");
    setDetails(null);
    setDiscovery(request.discovery ?? null);
    setPath(request.path ?? "");
    if (request.discovery) {
      setLoadingRepo(false);
      return;
    }
    setLoadingRepo(true);
    invoke<Discovery>("global_discover", {
      repo: request.market.repo,
      reference: "HEAD",
    })
      .then((d) => {
        if (!live) return;
        setDiscovery(d);
        // Search gives a repository and a Skill identifier, not a guaranteed file path.
        // Only pick a unique exact identifier match; ambiguous results require a choice.
        const matches = d.candidates.filter(
          (c) =>
            c.path === request.market.skillId ||
            c.name === request.market.skillId,
        );
        setPath(matches.length === 1 ? matches[0].path : "");
      })
      .catch((e) => {
        if (live) setLoadError(String(e));
      })
      .finally(() => {
        if (live) setLoadingRepo(false);
      });
    return () => {
      live = false;
    };
  }, [request, retry]);
  useEffect(() => {
    let live = true;
    setDetails(null);
    setInstallStatus(undefined);
    if (!discovery || !path) {
      setLoadingContent(false);
      return;
    }
    setLoadError("");
    setLoadingContent(true);
    invoke<Details>("global_discovery_details", {
      discovery: discovery.token,
      path,
    })
      .then(async (d) => {
        if (!live) return;
        setDetails(d);
        const statuses = await invoke<InstallStatus[]>("global_discovery_install_status", { discovery: discovery.token });
        if (live) setInstallStatus(statuses.find(s => s.key === path));
      })
      .catch((e) => {
        if (live) {
          setLoadError(String(e));
          setInstallStatus({ key: path, ids: [], status: "error", message: String(e) });
        }
      })
      .finally(() => {
        if (live) setLoadingContent(false);
      });
    return () => {
      live = false;
    };
  }, [discovery, path, retry]);
  const loading = loadingRepo || loadingContent;
  return (
    <Dialog
      title={details?.name ?? request.market?.name ?? "Skill 详情"}
      close={() => !busy && close()}
      wide
      resizable
    >
      <div className="remote-source">
        <span>
          <FileText size={16} /> 安装前查看
        </span>
        <code>
          {discovery?.repo ?? request.market?.repo} ·{" "}
          {discovery?.reference ?? "HEAD"}
        </code>
      </div>
      {loadingRepo && (
        <div className="global-progress" role="status">
          <LoaderCircle size={16} className="spin" />
          正在读取仓库内容…
        </div>
      )}
      {discovery && (!path || discovery.candidates.length > 1) && (
        <label className="remote-candidate-picker">
          {path ? "仓库内的 Skill" : "未找到唯一匹配项，请选择要查看的 Skill"}
          <select
            aria-label="选择要查看的远端 Skill"
            value={path}
            disabled={!!busy}
            onChange={(e) => {
              clearError();
              setPath(e.target.value);
            }}
          >
            <option value="" disabled>
              选择具体 Skill 路径
            </option>
            {discovery.candidates.map((c) => (
              <option key={c.path} value={c.path}>
                {c.name} · {c.path}
              </option>
            ))}
          </select>
        </label>
      )}
      {loadingContent && (
        <div className="global-progress" role="status">
          <LoaderCircle size={16} className="spin" />
          正在读取 SKILL.md…
        </div>
      )}
      {loadError && (
        <div className="message error" role="alert">
          <div>{loadError}</div>
          <button
            className="secondary"
            onClick={() => {
              clearError();
              setRetry((n) => n + 1);
            }}
          >
            <RefreshCw size={14} />
            重新读取
          </button>
        </div>
      )}
      {error && (
        <div className="message error" role="alert">
          {error}
        </div>
      )}
      {busy && (
        <div className="global-progress" role="status">
          <LoaderCircle size={16} className="spin" />
          {busy}…
        </div>
      )}
      {details && (
        <>
          <div className="global-detail-info">
            <InstallStatusBadge value={installStatus} />
            {installStatus && installStatus.status !== "unmatched" && <p>{installStatus.message}</p>}
            <p>下载来源：<SourceLink origin={{ repo: details.repo, reference: details.reference, path: details.path }} /></p>
            <p>
              {details.description ||
                "此 Skill 未提供简介，可直接阅读下方完整内容。"}
            </p>
            <p>
              实际路径：
              <code>{details.path === "." ? "仓库根目录" : details.path}</code>
            </p>
          </div>
          <details className="remote-files">
            <summary>文件清单 · {details.files.length} 项</summary>
            <ul>
              {details.files.map((file) => (
                <li key={file}>
                  <code>{file}</code>
                </li>
              ))}
            </ul>
          </details>
          <h3 className="remote-content-title">
            SKILL.md <span>完整内容 · 只读</span>
          </h3>
          <MarkdownPreview
            className="markdown-preview-remote"
            label="远端 Skill 完整内容"
            content={details.content || "SKILL.md 为空。"}
          />
        </>
      )}
      <footer>
        <button className="secondary" disabled={!!busy} onClick={close}>
          返回结果
        </button>
        <button
          className="primary"
          disabled={
            !details ||
            details.path !== path ||
            loading ||
            !!loadError ||
            !installStatus ||
            (!upgrading && ["multiple", "error", "unknown"].includes(installStatus.status)) ||
            !!busy
          }
          onClick={() => {
            if (!discovery || !details) return;
            if (!upgrading && installStatus?.ids.length === 1 && ["latest", "local"].includes(installStatus.status)) {
              openLocal(installStatus.ids[0]);
            } else {
              install(discovery, details.path, installStatus?.ids.length === 1 ? installStatus.ids[0] : undefined);
            }
          }}
        >
          <Download size={15} />
          {upgrading ? "比较并升级" : installStatus?.ids.length === 1 ? (["latest", "local"].includes(installStatus.status) ? "查看本地技能" : "比较并更新") : installStatus?.status === "multiple" ? "请在我的技能中选择版本" : "预览安装"}
        </button>
      </footer>
    </Dialog>
  );
}
