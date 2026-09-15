import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./llm.css";
import { saveTranslationStyle, useTranslationStyle } from "./translationPreferences";
import type { TranslationStyle } from "./translation";

type Config = { baseUrl: string; model: string; hasKey: boolean; thinking: boolean };
export function LlmSettingsForm() {
  const style = useTranslationStyle();
  const [baseUrl, setBaseUrl] = useState("");
  const [model, setModel] = useState("");
  const [thinking, setThinking] = useState(false);
  const [key, setKey] = useState("");
  const [hasKey, setHasKey] = useState(false);
  const [clearKey, setClearKey] = useState(false);
  const [models, setModels] = useState<string[]>([]);
  const [busy, setBusy] = useState("加载配置…");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [loaded, setLoaded] = useState(false);
  useEffect(() => {
    let live = true;
    invoke<Config>("llm_config").then(c => {
      if (!live) return;
      setBaseUrl(c.baseUrl); setModel(c.model); setHasKey(c.hasKey); setThinking(c.thinking ?? false); setLoaded(true);
    }).catch(e => { if (live) setError(String(e)); }).finally(() => { if (live) setBusy(""); });
    return () => { live = false; };
  }, []);
  const input = () => ({ baseUrl, model, thinking, apiKey: clearKey ? "" : key || null });
  async function fetchModels() {
    setBusy("正在获取模型…"); setError(""); setNotice("");
    try {
      const list = await invoke<string[]>("llm_models", { input: input() });
      setModels(list); setNotice(`已获取 ${list.length} 个模型，请选择或手动填写。`);
    } catch (e) { setError(String(e)); } finally { setBusy(""); }
  }
  async function save() {
    setBusy("正在保存…"); setError("");
    try {
      const saved = await invoke<Config>("llm_save_config", { input: input() });
      setBaseUrl(saved.baseUrl); setModel(saved.model); setHasKey(saved.hasKey);
      setKey(""); setClearKey(false); setNotice("LLM 配置已保存"); setBusy("");
    }
    catch (e) { setError(String(e)); setBusy(""); }
  }
  return <>
    <fieldset id="translation-settings" tabIndex={-1} className="translation-style-settings">
      <legend>翻译显示风格</legend>
      {([
        ["sentences", "逐句对照", "每句原文下显示对应译文，保留标题、列表与代码结构"],
        ["columns", "左右对照", "左侧原文、右侧译文，共同滚动查看"],
        ["translation", "仅译文", "只显示翻译后的完整文档"],
      ] as const).map(([value, label, description]) => <label key={value}>
        <input type="radio" name="translation-style" value={value} checked={style === value}
          onChange={() => saveTranslationStyle(value as TranslationStyle)} />
        <span><strong>{label}</strong><small>{description}</small></span>
      </label>)}
      <small>显示风格自动保存并立即生效，无需重新翻译。</small>
    </fieldset>
    <form id="llm-settings" tabIndex={-1} aria-labelledby="llm-settings-title" className="llm-settings" onSubmit={e => { e.preventDefault(); if (!busy && loaded) void save(); }}>
      <h3 id="llm-settings-title">LLM 接口</h3>
      <p>使用 OpenAI 兼容接口翻译和解读文档。仅在点击生成时，向此地址发送当前文档。</p>
      <label>API 请求地址<input required type="url" autoComplete="off" spellCheck={false} placeholder="https://api.example.com/v1"
        value={baseUrl} disabled={!!busy} onChange={e => { setBaseUrl(e.target.value); setModels([]); }} /></label>
      <small>填写 API 基础地址（含版本路径），也支持完整的 /chat/completions 地址。</small>
      <label>API Key<input type="password" autoComplete="new-password" spellCheck={false} value={key} disabled={!!busy || clearKey}
        placeholder={hasKey ? "已保存密钥，留空保持不变" : "填写 API Key，本地免密服务可留空"} onChange={e => setKey(e.target.value)} /></label>
      {hasKey && <label className="llm-inline"><input type="checkbox" checked={clearKey} disabled={!!busy} onChange={e => setClearKey(e.target.checked)} />清除已保存密钥</label>}
      <small>密钥保存在本机应用配置文件中，仅当前用户可读写；界面不回显已保存密钥。</small>
      <label>模型名称<input required list="llm-models" autoComplete="off" spellCheck={false} placeholder="手动填写或获取后选择"
        value={model} disabled={!!busy} onChange={e => setModel(e.target.value)} /></label>
      {model.toLowerCase().startsWith("deepseek-v4") && <label className="llm-inline"><input type="checkbox" checked={thinking} disabled={!!busy} onChange={e => setThinking(e.target.checked)} />启用 DeepSeek 深度思考（更慢，默认关闭）</label>}
      <datalist id="llm-models">{models.map(m => <option key={m} value={m} />)}</datalist>
      <button type="button" className="secondary" disabled={!!busy || !loaded || !baseUrl.trim()} onClick={() => void fetchModels()}>自动获取模型</button>
      {!!models.length && <select aria-label="选择获取到的模型" value={models.includes(model) ? model : ""} onChange={e => setModel(e.target.value)}>
        <option value="" disabled>选择一个模型</option>{models.map(m => <option key={m}>{m}</option>)}
      </select>}
      <small>模型列表来自服务的 /models 接口；不提供此接口时可直接手动填写。</small>
      {busy && <p role="status">{busy}</p>}{notice && <p role="status">{notice}</p>}{error && <p className="llm-error" role="alert">{error}</p>}
      <footer><button className="primary" disabled={!!busy || !loaded || !model.trim() || !baseUrl.trim()}>保存 LLM 配置</button></footer>
    </form>
  </>;
}
