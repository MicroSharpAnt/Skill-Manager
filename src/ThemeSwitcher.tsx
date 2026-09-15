import { useEffect, useState } from "react";
import { Check } from "lucide-react";

const themes = [
  {
    id: "signal",
    name: "信号蓝",
    description: "清晰 · 专注",
    nav: "#182d49",
    canvas: "#eef2f8",
    panel: "#ffffff",
    accent: "#315ed3",
    status: "#207453",
  },
  {
    id: "pine",
    name: "松针绿",
    description: "柔和 · 沉静",
    nav: "#193b33",
    canvas: "#edf3ef",
    panel: "#fbfefc",
    accent: "#23745b",
    status: "#206349",
  },
  {
    id: "amber",
    name: "琥珀灰",
    description: "温润 · 克制",
    nav: "#393930",
    canvas: "#f2f1ec",
    panel: "#fffefa",
    accent: "#805e24",
    status: "#38674c",
  },
  {
    id: "night",
    name: "夜航",
    description: "深色 · 低眩光",
    nav: "#101722",
    canvas: "#111a28",
    panel: "#1b283a",
    accent: "#a3bfff",
    status: "#a1dfbc",
  },
] as const;
type ThemeId = (typeof themes)[number]["id"];
const key = "skill-manager.theme";
function readTheme(): ThemeId {
  try {
    const saved = localStorage.getItem(key);
    if (themes.some((t) => t.id === saved)) return saved as ThemeId;
  } catch {
    /* Storage can be unavailable in preview environments. */
  }
  return "signal";
}
// Apply before React renders so relaunching a dark workspace does not flash white.
document.documentElement.dataset.theme = readTheme();

export function ThemeSettings() {
  const [theme, setTheme] = useState(readTheme);
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    try { localStorage.setItem(key, theme); } catch { /* Storage is optional. */ }
  }, [theme]);
  return <section className="theme-settings" aria-labelledby="theme-settings-title">
    <h3 id="theme-settings-title">外观主题</h3>
    <p className="settings-description">选择即生效，自动保存。</p>
          <div className="theme-options">
            {themes.map((t) => (
              <button
                key={t.id}
                className="theme-option"
                aria-pressed={theme === t.id}
                aria-label={`切换主题：${t.name}`}
                onClick={() => setTheme(t.id)}
              >
                <span
                  className="theme-preview"
                  aria-hidden="true"
                  style={{ background: t.canvas }}
                >
                  <span
                    className="theme-preview-nav"
                    style={{ background: t.nav }}
                  >
                    <i style={{ background: t.accent }} />
                    <i />
                    <i />
                  </span>
                  <span className="theme-preview-canvas">
                    <i style={{ background: t.accent }} />
                    <span style={{ background: t.panel }}>
                      {[0, 1, 2].map((i) => (
                        <b key={i}>
                          <em style={{ background: t.accent }} />
                          <i style={{ background: t.status }} />
                        </b>
                      ))}
                    </span>
                  </span>
                </span>
                <span className="theme-option-name">
                  {t.name}
                  {theme === t.id && <Check size={14} />}
                </span>
                <small>{t.description}</small>
              </button>
            ))}
          </div>
  </section>;
}
