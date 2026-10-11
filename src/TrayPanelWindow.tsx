import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { LogicalSize } from "@tauri-apps/api/dpi";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { SegmentedControl } from "@radix-ui/themes";
import { AppWindow, BookText, History, Power, RotateCw, Settings, ShieldAlert, type LucideIcon } from "lucide-react";
import type { VoiceMode, VoiceStage } from "./types";

type TraySnapshot = {
  stage: VoiceStage;
  lastResult: string | null;
  selectedMode: VoiceMode;
  hotkeyActive: boolean;
  trigger: string;
  missingPermissions: string[];
};

const stageLabel: Record<VoiceStage, string> = {
  idle: "就绪",
  recording: "正在听…",
  transcribing: "识别中…",
  refining: "整理中…",
  injecting: "写入中…",
  failed: "出错",
};

export default function TrayPanelWindow() {
  const [snapshot, setSnapshot] = useState<TraySnapshot | null>(null);
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState("");
  const panel = useRef<HTMLDivElement>(null);

  async function refresh() {
    try {
      setSnapshot(await invoke<TraySnapshot>("tray_snapshot"));
      setError("");
    } catch (reason) {
      setError(String(reason));
    }
  }

  useEffect(() => {
    void refresh();
    const events = ["voice-stage", "voice-complete", "voice-error", "mode-selected", "settings-changed", "permissions-changed"];
    const listeners = events.map((name) => listen(name, () => { void refresh(); }));
    listeners.push(listen<boolean>("hotkey-status-changed", (event) => {
      setSnapshot((current) => current && { ...current, hotkeyActive: event.payload });
    }));
    return () => { listeners.forEach((listener) => listener.then((remove) => remove())); };
  }, []);

  useEffect(() => {
    const frame = window.requestAnimationFrame(() => {
      const height = Math.min(560, Math.max(240, Math.ceil(panel.current?.scrollHeight ?? 0)));
      getCurrentWindow().setSize(new LogicalSize(300, height)).catch(console.error);
    });
    return () => window.cancelAnimationFrame(frame);
  }, [snapshot, error]);

  async function selectMode(mode: VoiceMode) {
    try {
      await invoke("select_mode", { mode });
      await refresh();
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function action(command: string, args: Record<string, string> = {}) {
    try {
      await invoke(command, args);
      await refresh();
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function restartHotkey() {
    try {
      await invoke("hotkey_stop");
      await invoke("hotkey_start");
      await refresh();
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function copyResult() {
    if (!snapshot?.lastResult) return;
    try {
      await writeText(snapshot.lastResult);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch (reason) {
      setError(String(reason));
    }
  }

  const trigger = snapshot?.trigger === "function" ? "Fn 键"
    : snapshot?.trigger === "leftOption" ? "左 Option" : "右 Option";
  const actions: { label: string; icon: LucideIcon; command: string; args: Record<string, string> }[] = [
    { label: "打开主窗口", icon: AppWindow, command: "open_workspace", args: { page: "home" } },
    { label: "历史记录", icon: History, command: "open_workspace", args: { page: "history" } },
    { label: "词典", icon: BookText, command: "open_workspace", args: { page: "dictionary" } },
    { label: "设置…", icon: Settings, command: "open_settings", args: {} },
  ];

  return (
    <div className="tray-panel" ref={panel}>
      <div className="tray-card tray-status">
        <div className="tray-status-line">
          <span className={`status-dot ${snapshot?.hotkeyActive ? snapshot.stage : "inactive"}`} aria-hidden="true" />
          <strong>{snapshot?.hotkeyActive ? stageLabel[snapshot.stage] : "热键未生效"}</strong>
          <kbd>{trigger}</kbd>
        </div>
        {snapshot && snapshot.missingPermissions.length > 0 && (
          <div className="tray-permission">
            <ShieldAlert size={15} aria-hidden="true" />
            <span><strong>缺少权限</strong><small>{snapshot.missingPermissions.join("、")}</small></span>
            <button type="button" onClick={() => action("open_settings", { section: "permission" })}>去授权</button>
          </div>
        )}
      </div>
      <SegmentedControl.Root className="tray-modes" value={snapshot?.selectedMode} onValueChange={(mode) => { if (mode) void selectMode(mode as VoiceMode); }} aria-label="语音模式">
        {([ ["dictate", "听写"], ["translate", "翻译"], ["ask", "询问"] ] as const).map(([mode, label]) => (
          <SegmentedControl.Item key={mode} value={mode}>{label}</SegmentedControl.Item>
        ))}
      </SegmentedControl.Root>
      {snapshot?.lastResult && (
        <div className="tray-card tray-recent">
          <div><span>最近一次</span><button type="button" disabled={copied} onClick={copyResult}>{copied ? "已复制" : "复制"}</button></div>
          <p>{snapshot.lastResult}</p>
        </div>
      )}
      <nav className="tray-actions" aria-label="应用操作">
        {actions.map(({ label, icon: Icon, command, args }) => (
          <button type="button" key={label} onClick={() => action(command, args)}><Icon size={15} aria-hidden="true" />{label}</button>
        ))}
        <button type="button" onClick={restartHotkey}><RotateCw size={15} aria-hidden="true" />{snapshot?.hotkeyActive ? "重启热键监听" : "启动热键监听"}</button>
        <hr />
        <button type="button" onClick={() => action("app_exit")}><Power size={15} aria-hidden="true" />退出 InkSay</button>
      </nav>
      {error && <div className="tray-error" role="alert">{error}</div>}
    </div>
  );
}
