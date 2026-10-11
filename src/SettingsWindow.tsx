import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { AudioLines, Info, Settings2, Shield, WandSparkles, X } from "lucide-react";
import SettingsView, { type SettingsSection } from "./SettingsView";

const sections: { id: SettingsSection; label: string; icon: typeof Settings2 }[] = [
  { id: "general", label: "通用", icon: Settings2 },
  { id: "transcription", label: "识别", icon: AudioLines },
  { id: "refine", label: "润色", icon: WandSparkles },
  { id: "permission", label: "权限", icon: Shield },
  { id: "about", label: "关于", icon: Info },
];

export default function SettingsWindow() {
  const isMac = navigator.platform.startsWith("Mac");
  const initial = new URLSearchParams(window.location.search).get("section");
  const [section, setSection] = useState<SettingsSection>(
    sections.some((item) => item.id === initial) ? initial as SettingsSection : "general",
  );

  useEffect(() => {
    const listener = listen<SettingsSection>("settings-section", (event) => setSection(event.payload));
    return () => { listener.then((remove) => remove()); };
  }, []);

  return (
    <div className={`settings-shell ${isMac ? "macos" : ""}`}>
      {isMac && <div className="window-drag-region" data-tauri-drag-region />}
      <aside className="settings-sidebar">
        <div className="brand">InkSay</div>
        <span className="settings-sidebar-label">设置</span>
        <nav aria-label="设置">
          {sections.filter((item) => isMac || item.id !== "permission").map((item) => (
            <button type="button" key={item.id} className={`nav-item ${section === item.id ? "active" : ""}`} onClick={() => setSection(item.id)}>
              <item.icon className="nav-icon" size={18} strokeWidth={1.9} aria-hidden="true" />
              {item.label}
            </button>
          ))}
        </nav>
      </aside>
      <main className="settings-content">
        <button type="button" className="settings-close" aria-label="关闭" onClick={() => getCurrentWindow().close()}><X size={18} aria-hidden="true" /></button>
        <SettingsView section={section} />
      </main>
    </div>
  );
}
