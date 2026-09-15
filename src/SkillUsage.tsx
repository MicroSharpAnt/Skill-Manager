import { useEffect, useMemo, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { RefreshCw, Search } from "lucide-react";
import "./skill-usage.css";

type Row = { name: string; path: string; explicit: number; inferred: number; sessions: number; lastUsed: string };
type Report = { rows: Row[]; files: number; firstRecord: string | null; lastRecord: string | null; warnings: string[]; logRoot: string };
type Skill = { name: string; source: string };
const date = (value: string | null) => value ? new Date(value).toLocaleString("zh-CN", { hour12: false }) : "无数据";

export function SkillUsage({ skills }: { skills: Skill[] }) {
  const [days, setDays] = useState(30);
  const [report, setReport] = useState<Report | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState("count");
  const [revision, setRevision] = useState(0);
  const request = useRef(0);
  useEffect(() => {
    const id = ++request.current;
    setReport(null); setError("");
    if (!isTauri()) { setError("请在桌面应用中查看本机 Codex 使用统计。"); return; }
    setLoading(true);
    invoke<Report>("codex_skill_usage", { days }).then(data => {
      if (id === request.current) setReport(data);
    }).catch(e => { if (id === request.current) setError(String(e)); })
      .finally(() => { if (id === request.current) setLoading(false); });
    return () => { request.current++; };
  }, [days, revision]);

  const rows = useMemo(() => {
    if (!report) return [];
    const all: Row[] = [...report.rows];
    const paths = new Set(all.map(r => r.path));
    for (const skill of skills) {
      const path = skill.source.replace(/\/$/, "") + "/SKILL.md";
      if (!paths.has(path)) {
        all.push({ name: skill.name, path, explicit: 0, inferred: 0, sessions: 0, lastUsed: "" });
        paths.add(path);
      }
    }
    const q = query.trim().toLowerCase();
    return all.filter(r => !q || `${r.name} ${r.path}`.toLowerCase().includes(q)).sort((a, b) => {
      const result = sort === "recent" ? b.lastUsed.localeCompare(a.lastUsed)
        : sort === "sessions" ? b.sessions - a.sessions : (b.explicit + b.inferred) - (a.explicit + a.inferred);
      return result || a.name.localeCompare(b.name) || a.path.localeCompare(b.path);
    });
  }, [report, skills, query, sort]);
  const total = report?.rows.reduce((n, r) => n + r.explicit + r.inferred, 0) ?? 0;
  return <section className="skill-usage" aria-label="Codex Skill 使用统计" aria-busy={loading}>
    <div className="usage-heading"><div><h2>Skill 使用统计 <span>Codex</span></h2>
      <p>查看本机日志中各个技能的使用迹象，了解常用技能与最近活动。</p></div>
      <button className="secondary" disabled={loading || !isTauri()} onClick={() => setRevision(v => v + 1)}><RefreshCw size={15} />刷新统计</button>
    </div>
    <div className="usage-controls">
      <label>时间范围 <select aria-label="时间范围" value={days} disabled={loading} onChange={e => setDays(Number(e.target.value))}>
        <option value={7}>最近 7 天</option><option value={30}>最近 30 天</option><option value={0}>全部记录</option>
      </select></label>
      <label className="usage-search"><Search size={15} /><input aria-label="搜索技能名称或来源路径" placeholder="搜索技能名称或来源路径" value={query} onChange={e => setQuery(e.target.value)} /></label>
      <label>排序 <select aria-label="排序" value={sort} onChange={e => setSort(e.target.value)}><option value="count">使用次数</option><option value="sessions">涉及任务数</option><option value="recent">最近使用</option></select></label>
    </div>
    <p className="usage-method">同一任务的同一轮对话中，每个 Skill 最多计一次，直接加载优先。读取推断可能包含审阅或编辑前的读取，不代表成功执行；“无数据”不等于从未使用。</p>
    {loading && <div className="usage-empty" role="status">正在读取 Codex 会话日志…首次统计可能需要一点时间。</div>}
    {error && <div className="usage-error" role="alert">{error}</div>}
    {report && <>
      <div className="usage-summary"><div><strong>{total}</strong><span>使用迹象（含推断）</span></div><div><strong>{report.rows.length}</strong><span>有记录的 Skill</span></div><div><strong>{report.files}</strong><span>已读取日志文件</span></div></div>
      <details className="usage-coverage"><summary>数据范围与统计口径</summary>
        <p>日志时间：{date(report.firstRecord)} 至 {date(report.lastRecord)}。所选范围按最近 {days || "全部"}{days ? " 天（滚动时间）" : "记录"}筛选。</p>
        <p>直接加载：会话中的 Skill 注入块。读取推断：成功返回的受支持文件读取命令。同一轮反复分段读取会去重；不同任务分别计数。尚未覆盖所有工具和脚本读取方式，历史日志删除后无法补齐。</p>
        <p>来源：<code>{report.logRoot}</code> 中的 sessions 与 archived_sessions。仅在本机读取，不上传会话内容。历史来源也会保留，同名不同路径分别展示。</p>
      </details>
      {report.warnings.length > 0 && <details className="usage-warnings" open><summary>数据提示（{report.warnings.length}）</summary><ul>{report.warnings.map((w, i) => <li key={i}>{w}</li>)}</ul></details>}
      <div className="usage-table-wrap"><table className="usage-table"><thead><tr><th>Skill / 来源</th><th>使用次数</th><th>直接加载</th><th>读取推断</th><th>涉及任务</th><th>最近使用</th></tr></thead>
        <tbody>{rows.map(row => <tr key={row.path}><td><strong>{row.name}</strong><small title={row.path}>{row.path}</small></td>
          <td>{row.sessions ? row.explicit + row.inferred : <span className="usage-muted">无数据</span>}</td>
          <td>{row.sessions ? row.explicit : "—"}</td><td>{row.sessions ? row.inferred : "—"}</td><td>{row.sessions || "—"}</td><td>{date(row.lastUsed)}</td></tr>)}</tbody>
      </table></div>
      {rows.length === 0 && <div className="usage-empty">{query ? "没有匹配的技能" : "该范围内未发现可识别的使用记录"}</div>}
    </>}
  </section>;
}
