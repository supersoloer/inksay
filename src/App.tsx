import { lazy, Suspense, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { setDockVisibility, setTheme } from "@tauri-apps/api/app";
import { listen } from "@tauri-apps/api/event";
import { BookText, ChartColumnIncreasing, History, House, Settings, type LucideIcon } from "lucide-react";
import HomeView from "./HomeView";
import HistoryView from "./HistoryView";
import DictionaryView from "./DictionaryView";
import type { MacPermissions, SettingsSnapshot, Snapshot, VoiceMode } from "./types";

const StatisticsView = lazy(() => import("./StatisticsView"));

type Section = "home" | "statistics" | "history" | "dictionary";
const sections: { id: Section; label: string; icon: LucideIcon }[] = [
  { id: "home", label: "首页", icon: House },
  { id: "dictionary", label: "词典", icon: BookText },
  { id: "statistics", label: "统计", icon: ChartColumnIncreasing },
  { id: "history", label: "历史记录", icon: History },
];
function App() {
  const isMac = navigator.platform.startsWith("Mac");
  const [section, setSection] = useState<Section>("home");
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [settings, setSettings] = useState<SettingsSnapshot | null>(null);
  const [permissions, setPermissions] = useState<MacPermissions | null>(null);
  const [selectedMode, setSelectedMode] = useState<VoiceMode>("dictate");
  const [selectedUseCaseID, setSelectedUseCaseID] = useState<string | null>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    invoke<Snapshot>("load_snapshot")
      .then(setSnapshot)
      .catch((reason) => setError(String(reason)));
  }, []);

  useEffect(() => {
    if (section !== "home") return;
    invoke<SettingsSnapshot>("settings_get")
      .then(async (next) => {
        setSettings(next);
        const appearance = String(next.values.appearance ?? "system");
        await setTheme(appearance === "system" ? null : appearance as "light" | "dark");
        if (next.platform === "macos") {
          await setDockVisibility(next.values.showInDock !== false);
          setPermissions(await invoke<MacPermissions>("permissions_status"));
        }
      })
      .catch(console.error);
    invoke<VoiceMode>("selected_mode").then(setSelectedMode).catch(console.error);
  }, [section]);

  useEffect(() => {
    if (section !== "home") return;
    const refreshStatus = () => {
      if (isMac) invoke<MacPermissions>("permissions_status").then(setPermissions).catch(console.error);
    };
    window.addEventListener("focus", refreshStatus);
    return () => window.removeEventListener("focus", refreshStatus);
  }, [section, isMac]);

  useEffect(() => {
    const unlisteners = [
      listen("voice-complete", () => {
        invoke<Snapshot>("load_snapshot").then(setSnapshot).catch(console.error);
      }),
      listen("history-changed", () => {
        invoke<Snapshot>("load_snapshot").then(setSnapshot).catch(console.error);
      }),
      listen("dictionary-changed", () => {
        invoke<Snapshot>("load_snapshot").then(setSnapshot).catch(console.error);
      }),
      listen<string>("voice-error", (event) => {
        console.error(event.payload);
      }),
      listen<SettingsSnapshot>("settings-changed", (event) => setSettings(event.payload)),
      listen<MacPermissions>("permissions-changed", (event) => setPermissions(event.payload)),
      listen<VoiceMode>("mode-selected", (event) => {
        setSelectedMode(event.payload);
        setSelectedUseCaseID(null);
      }),
    ];
    return () => {
      unlisteners.forEach((listener) => listener.then((remove) => remove()));
    };
  }, []);

  async function chooseMode(nextMode: VoiceMode, style?: string, useCaseID?: string) {
    try {
      const selected = await invoke<VoiceMode>("select_mode", { mode: nextMode, notify: false });
      setSelectedMode(selected);
      setSelectedUseCaseID(null);
      if (style) {
        setSettings(await invoke<SettingsSnapshot>("settings_set", { key: "refineStyle", value: style }));
      }
      setSelectedUseCaseID(useCaseID ?? null);
    } catch (reason) {
      console.error(reason);
    }
  }

  async function mutate(command: string, args: Record<string, unknown> = {}) {
    try {
      const next = await invoke<Snapshot | null>(command, args);
      if (next) setSnapshot(next);
      setError("");
      return next !== null;
    } catch (reason) {
      setError(String(reason));
      return false;
    }
  }

  return (
    <div className={`shell ${isMac ? "macos" : ""}`}>
      {isMac && <div className="window-drag-region" data-tauri-drag-region />}
      <aside className="sidebar">
        <div className="brand">
          <svg className="brand-mark" viewBox="4.5 2 60 60" fill="currentColor" aria-hidden="true">
            <rect x="8" y="24" width="5" height="16" rx="2.5" />
            <rect x="16" y="16" width="5" height="32" rx="2.5" />
            <rect x="24" y="8" width="5" height="48" rx="2.5" />
            <rect x="32" y="20" width="5" height="24" rx="2.5" />
            <rect x="40" y="12" width="5" height="40" rx="2.5" />
            <rect x="48" y="18" width="5" height="28" rx="2.5" />
            <rect x="56" y="24" width="5" height="16" rx="2.5" />
          </svg>InkSay
        </div>
        <nav aria-label="主导航">
          {sections.map((item) => (
            <button
              className={`nav-item ${section === item.id ? "active" : ""}`}
              key={item.id}
              onClick={() => {
                setSection(item.id);
              }}
              type="button"
            >
              <item.icon className="nav-icon" size={18} strokeWidth={1.9} aria-hidden="true" />
              {item.label}
            </button>
          ))}
        </nav>
        <button type="button" className="nav-item sidebar-settings" onClick={() => invoke("open_settings").catch(console.error)}>
          <Settings className="nav-icon" size={18} strokeWidth={1.9} aria-hidden="true" />设置
        </button>
      </aside>
      <main className="content">
        {error && (
          <div className="error" role="alert">
            {error}
          </div>
        )}
        {section === "home" && (
          <HomeView
            history={snapshot?.history ?? []}
            settings={settings}
            permissions={permissions}
            selectedMode={selectedMode}
            selectedUseCaseID={selectedUseCaseID}
            onSelectMode={chooseMode}
            onOpenRefinement={() => invoke("open_settings", { section: "refine" }).catch(console.error)}
            onOpenRecognition={() => invoke("open_settings", { section: "transcription" }).catch(console.error)}
            onOpenPermissions={() => invoke("open_settings", { section: "permission" }).catch(console.error)}
          />
        )}
        {section === "statistics" && <Suspense fallback={null}><StatisticsView /></Suspense>}
        {section === "history" && (
          <HistoryView
            mutationError={error}
            onMutate={mutate}
            entries={snapshot?.history ?? []}
            onDelete={(id) => { void mutate("history_delete", { id }); }}
            onClear={() => { void mutate("history_clear"); }}
          />
        )}
        {section === "dictionary" && (
          <DictionaryView mutationError={error} rules={snapshot?.replacementRules ?? []} entries={snapshot?.dictionary ?? []} onMutate={mutate} />
        )}
      </main>
    </div>
  );
}

export default App;
