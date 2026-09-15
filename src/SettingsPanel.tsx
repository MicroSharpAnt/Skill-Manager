import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { ThemeSettings } from "./ThemeSwitcher";
import { LlmSettingsForm } from "./LlmSettings";
import "./settings.css";

type Page = "project" | "global" | "recovery" | "settings";
type SettingsSection = "general" | "translation" | "llm";
const SettingsContext = createContext<{
  page: Page;
  setPage: (page: Page) => void;
  openSettings: (section?: SettingsSection) => void;
  section: SettingsSection;
  revision: number;
}>({ page: "project", setPage: () => {}, openSettings: () => {}, section: "general", revision: 0 });
export const useSettings = () => useContext(SettingsContext);

export function SettingsPage() {
  const { section, revision } = useSettings();
  const page = useRef<HTMLElement>(null);
  useEffect(() => {
    const target = section === "general"
      ? page.current
      : page.current?.querySelector<HTMLElement>(`#${section}-settings`);
    if (section === "general") window.scrollTo({ top: 0 });
    else target?.scrollIntoView({ block: "start" });
    target?.focus({ preventScroll: true });
  }, [section, revision]);
  return <section className="settings-page" aria-labelledby="settings-title" ref={page} tabIndex={-1}>
    <div className="page-heading">
      <div className="eyebrow">应用偏好与接口配置</div>
      <h1 id="settings-title">设置</h1>
      <p>管理外观主题、翻译显示风格与 LLM 接口。</p>
    </div>
    <div className="settings-content">
      <ThemeSettings />
      <LlmSettingsForm />
    </div>
  </section>;
}

export function SettingsProvider({ children }: { children: ReactNode }) {
  const [page, setPage] = useState<Page>("project");
  const [request, setRequest] = useState({ section: "general" as SettingsSection, revision: 0 });
  return <SettingsContext.Provider value={{ page, setPage, ...request, openSettings: (section = "general") => {
    setRequest(old => ({ section, revision: old.revision + 1 }));
    setPage("settings");
  } }}>
    {children}
  </SettingsContext.Provider>;
}
