import { useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { Plus, Pencil, Copy, Trash2, Play, Bookmark } from "lucide-react";
import { Dialog } from "./GlobalDialog";
import { resolvePreset, type Preset, type PresetResource, type PresetGroup, type PresetChange } from "./presets";
import "./presets.css";

type Choice = "inherit" | "on" | "off";
function StateChoice({ label, value, change }: { label: string; value: Choice; change: (value: Choice) => void }) {
  return <select aria-label={label} value={value} onChange={event => change(event.target.value as Choice)}>
    <option value="inherit">不单独设置</option><option value="on">开启</option><option value="off">关闭</option>
  </select>;
}
export function PresetPanel({ scope, groups, resources, clients, disabled, applyDisabled = false, apply }: {
  scope: string; groups: PresetGroup[]; resources: PresetResource[]; clients?: string[];
  disabled: boolean; applyDisabled?: boolean; apply: (changes: PresetChange[]) => Promise<void>;
}) {
  const [presets, setPresets] = useState<Preset[]>([]);
  const selectionKey = `skill-manager-preset:${scope}`;
  const [selected, setSelected] = useState(() => {
    try { return localStorage.getItem(selectionKey) ?? ""; } catch { return ""; }
  });
  useEffect(() => { try { localStorage.setItem(selectionKey, selected); } catch { /* Selection is optional UI state. */ } }, [selectionKey, selected]);
  const [draft, setDraft] = useState<Preset | null>(null);
  const [search, setSearch] = useState("");
  const [preview, setPreview] = useState<Preset | null>(null);
  const [busy, setBusy] = useState(false), [error, setError] = useState(""), [notice, setNotice] = useState("");
  const running = useRef(false);
  const active = presets.find(p => p.id === selected);
  const targets = (p: Preset) => clients ? p.clients : ["project"];
  const resolution = preview ? resolvePreset(preview, groups, resources, targets(preview)) : null;
  const match = active ? resolvePreset(active, groups, resources, targets(active)) : null;
  useEffect(() => {
    let live = true;
    if (isTauri()) invoke<Preset[]>("presets", { scope }).then(value => { if (live) setPresets(value); }).catch(e => { if (live) setError(String(e)); });
    return () => { live = false; };
  }, [scope]);
  async function run(action: () => Promise<void>) {
    if (running.current) return;
    running.current = true; setBusy(true); setError(""); setNotice("");
    try { await action(); } catch(e) { setError(String(e)); }
    finally { running.current = false; setBusy(false); }
  }
  function edit(value: Preset) { setError(""); setSearch(""); setDraft(structuredClone(value)); }
  function create() { edit({ id: crypto.randomUUID(), name: "", defaultState: "keep", groups: {}, resources: {}, clients: clients ? [clients.includes("Codex") ? "Codex" : clients[0]] : [] }); }
  function setRule(kind: "groups" | "resources", id: string, choice: Choice) {
    setDraft(previous => {
      if (!previous) return previous;
      const rules = { ...previous[kind] };
      if (choice === "inherit") delete rules[id]; else rules[id] = choice === "on";
      return { ...previous, [kind]: rules };
    });
  }
  const locked = disabled || busy || !isTauri();
  return <div className="preset-panel">
    <div className="preset-bar"><Bookmark size={16} /><strong>预设方案</strong>
      <select aria-label="选择预设方案" value={selected} disabled={busy} onChange={event => { setSelected(event.target.value); setNotice(""); setError(""); }}>
        <option value="">选择方案…</option>{presets.map(p => <option key={p.id} value={p.id}>{p.name}</option>)}
      </select>
      <button className="primary" disabled={locked || applyDisabled || !active} onClick={() => { setError(""); setPreview(active!); }}><Play size={14} />应用方案</button>
      <button className="secondary" disabled={locked} onClick={create}><Plus size={14} />新建方案</button>
      {!clients && <button className="secondary" disabled={locked} onClick={() => edit({ id: crypto.randomUUID(), name: "", defaultState: "keep", groups: {},
        resources: Object.fromEntries(resources.filter(r => r.states.project !== null).map(r => [r.id, r.states.project as boolean])), clients: [] })}>保存当前状态</button>}
      {active && <>
        <button className="icon-button" aria-label="编辑方案" disabled={locked} onClick={() => edit(active)}><Pencil size={15} /></button>
        <button className="icon-button" aria-label="复制方案" disabled={locked} onClick={() => edit({ ...active, id: crypto.randomUUID(), name: `${active.name} 副本` })}><Copy size={15} /></button>
        <button className="icon-button" aria-label="删除方案（保留资源状态）" disabled={locked} onClick={() => { void run(async () => {
          await invoke("delete_preset", { scope, id: active.id }); setPresets(previous => previous.filter(p => p.id !== active.id)); setSelected(""); setNotice("方案已删除，资源状态不变");
        }); }}><Trash2 size={15} /></button>
        <small>{match?.errors.length ? "方案需要检查" : match?.changes.length ? `${match.changes.length} 项待应用` : "当前状态符合方案"}</small>
      </>}
    </div>
    {!draft && !preview && (error || notice) && <p className={error ? "preset-error" : "muted"} role="status">{error || notice}</p>}
    {draft && <Dialog title={presets.some(p => p.id === draft.id) ? "编辑预设方案" : "新建预设方案"} wide close={() => { if (!busy) setDraft(null); }}>
      <form onSubmit={event => { event.preventDefault(); void run(async () => {
        if (!draft.name.trim()) throw new Error("请输入方案名称");
        if (clients && !draft.clients.length) throw new Error("请选择目标客户端");
        await invoke("save_preset", { scope, preset: draft });
        setPresets(await invoke<Preset[]>("presets", { scope })); setSelected(draft.id); setDraft(null); setNotice("方案已保存，点击应用方案执行");
      }); }}>
        <fieldset className="preset-fields" disabled={locked}>
          <label className="form-label">方案名称<input autoFocus maxLength={60} value={draft.name} onChange={e => setDraft({ ...draft, name: e.target.value })} placeholder="例如：活动开发、轻量维护" /></label>
          {clients && <div className="preset-clients" role="group" aria-label="方案目标客户端">目标客户端：{clients.map(client => <label key={client}>
            <input type="checkbox" checked={draft.clients.includes(client)} onChange={event => setDraft({ ...draft, clients: event.target.checked ? [...draft.clients, client] : draft.clients.filter(c => c !== client) })} />{client}</label>)}</div>}
          <label className="preset-default">未配置的资源<select aria-label="未配置资源的状态" value={draft.defaultState} onChange={event => setDraft({ ...draft, defaultState: event.target.value as Preset["defaultState"] })}>
            <option value="keep">保持当前状态</option><option value="on">统一开启</option><option value="off">统一关闭</option>
          </select></label>
          <p className="muted">单项设置优先于分组；分组按应用时的最新成员生效。保存方案不会立即改变资源状态。</p>
          <div className="preset-editor-columns">
            <section><h3>分组设置</h3><div className="preset-options">
              {groups.map(g => <label className="preset-rule" key={g.id}><span>{g.name}<small>{g.members.length} 项</small></span>
                <StateChoice label={`${g.name} 分组状态`} value={draft.groups[g.id] === undefined ? "inherit" : draft.groups[g.id] ? "on" : "off"} change={value => setRule("groups", g.id, value)} /></label>)}
              {Object.keys(draft.groups).filter(id => !groups.some(g => g.id === id)).map(id => <div className="preset-rule" key={id}><span>已删除分组：{id}</span><button type="button" onClick={() => setRule("groups", id, "inherit")}>移除设置</button></div>)}
            </div></section>
            <section><h3>单个资源覆盖</h3><input aria-label="搜索方案资源" value={search} onChange={event => setSearch(event.target.value)} placeholder="搜索 Skill / Rule…" />
              <div className="preset-options">{resources.filter(r => (r.name + r.id).toLowerCase().includes(search.toLowerCase())).map(r => <label className="preset-rule" key={r.id}><span title={r.id}>{r.name}<small>{groups.filter(g => g.members.includes(r.id)).map(g => g.name).join("、") || "未分组"}</small></span>
                <StateChoice label={`${r.name} 单项状态`} value={draft.resources[r.id] === undefined ? "inherit" : draft.resources[r.id] ? "on" : "off"} change={value => setRule("resources", r.id, value)} /></label>)}
                {Object.keys(draft.resources).filter(id => !resources.some(r => r.id === id)).map(id => <div className="preset-rule" key={id}><span>已删除资源：{id}</span><button type="button" onClick={() => setRule("resources", id, "inherit")}>移除设置</button></div>)}
              </div></section>
          </div>
          {error && <p className="preset-error" role="alert">{error}</p>}
          <footer><button type="button" className="secondary" onClick={() => setDraft(null)}>取消</button><button className="primary" type="submit" disabled={!draft.name.trim()}>保存方案</button></footer>
        </fieldset>
      </form>
    </Dialog>}
    {preview && resolution && <Dialog title={`应用方案：${preview.name}`} close={() => { if (!busy) setPreview(null); }}>
      <p>将开启 {resolution.changes.filter(c => c.enable).length} 项，关闭 {resolution.changes.filter(c => !c.enable).length} 项。关闭时保留源文件和恢复备份。</p>
      {resolution.errors.length > 0 && <div className="preset-errors" role="alert">{resolution.errors.map((e,i) => <p key={i}>{e}</p>)}</div>}
      <div className="change-list">{resolution.changes.map(c => <div key={`${c.id}:${c.client}`}><span className={`badge ${c.enable ? "enabled" : "disabled"}`}>{c.enable ? "开启" : "关闭"}</span><span title={c.id}>{c.name}{clients ? ` · ${c.client}` : ""}</span></div>)}</div>
      {!resolution.changes.length && !resolution.errors.length && <p>当前资源状态已符合方案，无需修改。</p>}
      {error && <p className="preset-error" role="alert">{error}</p>}
      <footer><button className="secondary" disabled={busy} onClick={() => setPreview(null)}>取消</button><button className="primary" disabled={locked || applyDisabled || !!resolution.errors.length || !resolution.changes.length} onClick={() => { void run(async () => {
        await apply(resolution.changes); setPreview(null); setNotice(`已应用「${preview.name}」`);
      }); }}>{busy ? "正在应用…" : "确认应用"}</button></footer>
    </Dialog>}
  </div>;
}
