import { useEffect, useRef, useState } from "react";
import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import { Languages, Sparkles, Settings2 } from "lucide-react";
import { MarkdownPreview } from "./MarkdownPreview";
import { useSettings } from "./SettingsPanel";
import { Dialog } from "./GlobalDialog";
import "./llm.css";
import { analysisLabels, reviewInput, type AnalysisMode } from "./llmReview";
import { type SentencePair } from "./translation";
import { runTranslation, partialTranslation } from "./translationRunner";
import { useTranslationStyle } from "./translationPreferences";

type Target = { scope: "project"; root: string; path: string } | { scope: "global"; id: string };
type View = "original" | "translate" | AnalysisMode;
type Comparison = { name: string; source: string; target: Target };
export function ResourceReader({ content, target, name, writable = true, comparisons = [], onSaved }: {
  content: string; target: Target; name: string; writable?: boolean; comparisons?: Comparison[]; onSaved: (text: string) => void;
}) {
  const [view, setView] = useState<View>("original");
  const [translation, setTranslation] = useState("");
  const [pairs, setPairs] = useState<SentencePair[]>([]);
  const [progress, setProgress] = useState("");
  const [complete, setComplete] = useState(false);
  const [elapsed, setElapsed] = useState(0);
  const style = useTranslationStyle();
  const [analyses, setAnalyses] = useState<Partial<Record<AnalysisMode, string>>>({});
  const [finished, setFinished] = useState<Partial<Record<AnalysisMode, boolean>>>({});
  const [selected, setSelected] = useState<string[]>([]);
  const [comparisonFilter, setComparisonFilter] = useState("");
  const identity = JSON.stringify(target);
  const analysisMode = view !== "original" && view !== "translate" ? view : null;
  function clearAnalyses() { setAnalyses({}); setFinished({}); }
  const [language, setLanguage] = useState("简体中文");
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");
  const { openSettings } = useSettings();
  const [confirm, setConfirm] = useState(false);
  const [editing, setEditing] = useState(false);
  const [backup, setBackup] = useState("");
  const [undo, setUndo] = useState<{ before: string; after: string } | null>(null);
  const seq = useRef(0);
  const alive = useRef(true);
  useEffect(() => {
    setElapsed(0);
    if (!busy) return;
    const started = Date.now();
    const timer = window.setInterval(() => setElapsed(Math.floor((Date.now() - started) / 1000)), 1000);
    return () => window.clearInterval(timer);
  }, [busy]);
  useEffect(() => { alive.current = true; return () => { alive.current = false; seq.current++; }; }, []);
  useEffect(() => {
    seq.current++; setTranslation(""); setComplete(false); setPairs([]); clearAnalyses(); setBusy(""); setError(""); setConfirm(false); setEditing(false);
  }, [content, identity]);
  useEffect(() => { setSelected([]); setComparisonFilter(""); setUndo(null); setBackup(""); setView("original"); }, [identity]);
  async function generate(mode: "translate" | AnalysisMode) {
    const request = ++seq.current;
    setBusy(mode); setError(""); setView(mode); setProgress("");
    try {
      if (mode === "translate") {
        if (new TextEncoder().encode(content).length > 256 * 1024) throw new Error("内容超过 256 KiB，请缩小文档后重试");
        setTranslation(""); setPairs([]); setComplete(false); setEditing(false);
        const cancelled = () => !alive.current || request !== seq.current;
        const result = await runTranslation(content,
          segments => invoke("llm_translate_segments", { segments, language }),
          (ready, total) => {
            if (cancelled()) return;
            setProgress(`${ready.length} / ${total} 句（最多两批同时处理）`);
            if (ready.length) { setTranslation(partialTranslation(content, ready)); setPairs(ready); }
          }, cancelled);
        if (cancelled()) return;
        setTranslation(result.text); setPairs(result.pairs); setComplete(true); setEditing(false);
        return;
      }
      setAnalyses(old => ({ ...old, [mode]: "" }));
      setFinished(old => ({ ...old, [mode]: false }));
      let text = content;
      if (mode !== "overview") {
        const docs = [{ name, source: target.scope === "project" ? target.path : target.id, content }];
        if (mode === "conflicts") {
          setProgress("正在读取对照文档…");
          for (const source of selected) {
            const candidate = comparisons.find(item => item.source === source);
            if (!candidate) throw new Error("对照文档已变化，请重新选择");
            const other = candidate.target;
            const body = await invoke<string>(other.scope === "project" ? "content" : "global_details",
              other.scope === "project" ? { root: other.root, path: other.path } : { id: other.id });
            if (!alive.current || request !== seq.current) return;
            docs.push({ name: candidate.name, source: candidate.source, content: body });
          }
        }
        text = reviewInput(docs);
      }
      const onEvent = new Channel<{ kind: "status"; message: string } | { kind: "delta"; text: string }>();
      onEvent.onmessage = event => {
        if (!alive.current || request !== seq.current) return;
        if (event.kind === "status") setProgress(event.message);
        else { setProgress(`正在接收${analysisLabels[mode]}…`); setAnalyses(old => ({ ...old, [mode]: (old[mode] || "") + event.text })); }
      };
      const result = await invoke<string>("llm_generate", { text, mode, language, onEvent });
      if (!alive.current || request !== seq.current) return;
      setAnalyses(old => ({ ...old, [mode]: result }));
      setFinished(old => ({ ...old, [mode]: true }));
    } catch (e) { if (alive.current && request === seq.current) setError(String(e)); }
    finally { if (alive.current && request === seq.current) setBusy(""); }
  }
  async function replace(restoring = false) {
    const before = restoring && undo ? undo.after : content;
    const after = restoring && undo ? undo.before : translation;
    setBusy("replace"); setError("");
    try {
      const result = await invoke<{ backupPath: string }>("llm_replace", { target, expected: before, replacement: after });
      if (!alive.current) return;
      setBackup(result.backupPath); setUndo(restoring ? null : { before, after });
      setConfirm(false); setView("original"); setTranslation(""); clearAnalyses();
      onSaved(after);
    } catch (e) { if (alive.current) setError(String(e)); }
    finally { if (alive.current) setBusy(""); }
  }
  const result = view === "translate" ? translation : analysisMode ? analyses[analysisMode] || "" : "";
  return <section className="resource-reader">
    <div className="reader-toolbar">
      <div className="reader-tabs" role="tablist" aria-label="文档视图">
        {([["original", "原文"], ["translate", "翻译"], ["overview", "解读概览"], ["conflicts", "冲突检查"], ["quality", "质量检查"], ["improvements", "改进意见"]] as const).map(([id, label]) =>
          <button key={id} role="tab" aria-selected={view === id} className={view === id ? "active" : ""} onClick={() => setView(id)}>{label}</button>)}
      </div>
      <button className="reader-settings" title="设置" aria-label="设置" disabled={!isTauri()} onClick={() => openSettings()}><Settings2 size={16} /></button>
    </div>
    {view === "translate" && <div className="reader-actions">
      <select aria-label="翻译目标语言" value={language} disabled={!!busy} onChange={e => { setLanguage(e.target.value); setTranslation(""); setPairs([]); setComplete(false); }}>
        <option>简体中文</option><option>繁體中文</option><option>English</option><option>日本語</option><option>한국어</option>
      </select>
      <button className="secondary" disabled={!isTauri() || !!busy || !content} onClick={() => void generate("translate")}><Languages size={14} />{translation ? "重新翻译" : "翻译内容"}</button>
    </div>}
    {view === "overview" && <div className="reader-actions">
      <button className="secondary" disabled={!isTauri() || !!busy || !content} onClick={() => void generate("overview")}><Sparkles size={14} />{analyses.overview ? "重新解读" : "生成概览"}</button>
    </div>}
    {analysisMode && analysisMode !== "overview" && <div className="reader-review">
      {view === "conflicts" && <>
        <p className="reader-hint">默认检查当前文档内部冲突；可选最多 8 份对照文档一起检查。请选取会同时生效的 Skill 或 Rule。</p>
        <details>
          <summary>对照文档 · 已选 {selected.length} / 8</summary>
          <input aria-label="搜索对照文档" placeholder="按名称或路径搜索" value={comparisonFilter} onChange={e => setComparisonFilter(e.target.value)} />
          <div className="reader-comparisons">
            {comparisons.filter(item => `${item.name} ${item.source}`.toLowerCase().includes(comparisonFilter.toLowerCase())).map(item => <label key={item.source}>
              <input type="checkbox" checked={selected.includes(item.source)} disabled={!!busy || (!selected.includes(item.source) && selected.length >= 8)} onChange={e => {
                setSelected(old => e.target.checked ? [...old, item.source] : old.filter(source => source !== item.source));
                setAnalyses(old => ({ ...old, conflicts: "" })); setFinished(old => ({ ...old, conflicts: false }));
              }} /><span>{item.name}<small>{item.source}</small></span>
            </label>)}
            {!comparisons.length && <p>当前范围没有其他可对照文档。</p>}
          </div>
        </details>
      </>}
      <div className="reader-actions"><button className="secondary" disabled={!isTauri() || !!busy || !content} onClick={() => void generate(analysisMode)}><Sparkles size={14} />{result ? "重新生成" : "开始"}{analysisLabels[analysisMode]}</button></div>
    </div>}
    <p className="reader-hint">点击生成时，将当前文档及冲突检查选中的对照文档发送至配置的 LLM。检查与建议仅供核对，不修改原文。</p>
    {busy && <p className="reader-status" role="status">{busy === "replace" ? "正在备份并写入…" : busy === "translate" ? `正在逐句翻译… ${progress}` : progress || `正在生成${analysisLabels[busy as AnalysisMode] || "结果"}…`} · 已等待 {elapsed} 秒
      {busy !== "replace" && <button onClick={() => { seq.current++; setBusy(""); }}>停止等待</button>}</p>}
    {error && <p className="llm-error" role="alert">{error}
      {error.includes("请先配置 LLM 的 API 地址和模型名称") && <button
        type="button" className="llm-config-link" onClick={() => openSettings("llm")}>去配置</button>}
    </p>}
    {backup && <div className="reader-backup" role="status">已保存，原文备份：<code>{backup}</code>{undo && <button disabled={!!busy || !writable} onClick={() => void replace(true)}>撤销本次覆盖</button>}</div>}
    {view === "original" ? <MarkdownPreview content={content || "加载内容…"} /> : result ? <>
      {analysisMode && !finished[analysisMode] && <p className="reader-hint">结果尚未完整生成，仅供预览；停止或失败后可重新生成。</p>}
      {view === "translate" && !complete && <p className="reader-hint">译文尚未完整生成，已完成部分先展示，其余保留原文；全部完成后才可编辑或覆盖。</p>}
      {view === "translate" && <div className="reader-actions">
        <button className="secondary" onClick={() => openSettings("translation")}>显示：{style === "sentences" ? "逐句对照" : style === "columns" ? "左右对照" : "仅译文"}</button>
        <button className="secondary" onClick={() => setEditing(!editing)} disabled={!!busy || !complete}>{editing ? "预览译文" : "编辑译文"}</button>
        <button className="primary" disabled={!!busy || !complete || !writable || translation === content || !translation.trim()} onClick={() => setConfirm(true)}>用译文覆盖原文…</button>
        {!writable && <small>当前资源不可写；暂存资源需先启用。</small>}
      </div>}
      {view === "translate" && editing ? <textarea className="translation-editor" aria-label="编辑翻译文本" value={translation} onChange={e => { setTranslation(e.target.value); setPairs([]); }} /> :
        view === "translate" && style !== "translation" ? (
          style === "sentences" && pairs.length ? <MarkdownPreview content={content} pairs={pairs} className="bilingual-preview" label="逐句对照预览" /> :
          <>{style === "sentences" && <p className="reader-hint">当前译文没有逐句对应信息，先显示左右对照；重新翻译可恢复逐句显示。</p>}
            <div className="translation-columns" role="region" aria-label="原文与译文左右对照">
              <section><h3>原文</h3><MarkdownPreview content={content} label="对照原文" /></section>
              <section><h3>译文</h3><MarkdownPreview content={translation} label="对照译文" /></section>
            </div></>
        ) : <MarkdownPreview content={result} label={view === "translate" ? "翻译预览" : `LLM ${analysisMode ? analysisLabels[analysisMode] : "结果"}`} />}
    </> : <div className="reader-empty">{view === "translate" ? "点击“翻译内容”生成译文，可核对后选择覆盖。" : view === "conflicts" ? "检查内部矛盾或所选文档间的冲突，列出原文依据、影响与处理建议。" : view === "quality" ? "检查触发条件、规则清晰度、流程完整性、依赖及可验证性，并列出问题依据。" : view === "improvements" ? "生成按优先级排序的改进意见、建议改写示例与验证方法。" : "点击“生成概览”了解用途、适用场景、关键规则与依赖。"}</div>}
    {confirm && <Dialog title={`用译文覆盖 · ${name}`} close={() => { if (busy !== "replace") setConfirm(false); }} wide>
      <p>确认后将替换当前文件正文，保留 YAML 元信息并自动备份原文。</p>
      {target.scope === "global" && <p>覆盖全局 Skill 源文件，软链接客户端立即生效；已有复制副本保持原样。</p>}
      <div className="translation-compare"><section><h3>当前原文</h3><MarkdownPreview content={content} /></section><section><h3>将写入的译文</h3><MarkdownPreview content={translation} /></section></div>
      {error && <p role="alert" className="llm-error">{error}</p>}
      <footer className="reader-confirm"><button className="secondary" disabled={!!busy} onClick={() => setConfirm(false)}>取消</button><button className="primary" disabled={!!busy} onClick={() => void replace()}>确认备份并覆盖</button></footer>
    </Dialog>}
  </section>;
}
