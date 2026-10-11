import AboutSettings from "./AboutSettings";
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { setDockVisibility, setTheme } from "@tauri-apps/api/app";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { Select, Switch, TextArea, TextField, Button } from "@radix-ui/themes";
import {
  AudioLines,
  Cpu,
  Globe2,
  Keyboard,
  Languages,
  Palette,
  ScanText,
  Server,
  Shield,
  SlidersHorizontal,
  Stethoscope,
  WandSparkles,
  Zap,
} from "lucide-react";
import type { MacPermissions, SettingsSnapshot } from "./types";

export type SettingsSection =
  "general" | "transcription" | "refine" | "permission" | "about";
type AudioInputDevice = { id: string; name: string };

function SettingSelect({
  label,
  value,
  options,
  disabled,
  onValueChange,
}: {
  label: string;
  value: string;
  options: { value: string; label: string }[];
  disabled?: boolean;
  onValueChange: (value: string) => void;
}) {
  return (
    <Select.Root
      value={value || "__inksay_empty__"}
      onValueChange={(next) =>
        onValueChange(next === "__inksay_empty__" ? "" : next)
      }
      disabled={disabled}
    >
      <Select.Trigger className="settings-select" aria-label={label} />
      <Select.Content position="popper" className="settings-select-content">
        {options.map((option) => (
          <Select.Item
            key={option.value}
            value={option.value || "__inksay_empty__"}
          >
            {option.label}
          </Select.Item>
        ))}
      </Select.Content>
    </Select.Root>
  );
}

export default function SettingsView({
  section,
}: {
  section: SettingsSection;
}) {
  const [settings, setSettings] = useState<SettingsSnapshot | null>(null);
  const [busy, setBusy] = useState(false);
  const [permissions, setPermissions] = useState<MacPermissions | null>(null);
  const [permissionBusy, setPermissionBusy] = useState(false);
  const recognitionSource = String(settings?.values.transcriptionBackend ?? "cloudASR");
  const [cloudKeyDraft, setCloudKeyDraft] = useState("");
  const [cloudKeyDirty, setCloudKeyDirty] = useState(false);
  const [cloudTesting, setCloudTesting] = useState(false);
  const [cloudTestResult, setCloudTestResult] = useState("");
  const [settingError, setSettingError] = useState("");

  const [apiKeyDraft, setApiKeyDraft] = useState("");
  const [apiKeyDirty, setApiKeyDirty] = useState(false);
  const [inputDevices, setInputDevices] = useState<AudioInputDevice[]>([]);
  const [launchAtLogin, setLaunchAtLogin] = useState<boolean | null>(null);
  const [modelBytes, setModelBytes] = useState<number | null>(null);
  const [recordingBytes, setRecordingBytes] = useState<number | null>(null);
  const [diagnosticsError, setDiagnosticsError] = useState(false);

  const refreshPermissions = useCallback(() => {
    invoke<MacPermissions>("permissions_status")
      .then(setPermissions)
      .catch(console.error);
  }, []);

  useEffect(() => {
    invoke<SettingsSnapshot>("settings_get")
      .then(setSettings)
      .catch(console.error);
    isEnabled().then(setLaunchAtLogin).catch(console.error);
  }, []);

  useEffect(() => {
    const listener = listen<SettingsSnapshot>("settings-changed", (event) =>
      setSettings(event.payload),
    );
    return () => {
      listener.then((remove) => remove());
    };
  }, []);

  useEffect(() => {
    const listener = listen<MacPermissions>("permissions-changed", (event) =>
      setPermissions(event.payload),
    );
    return () => {
      listener.then((remove) => remove());
    };
  }, []);

  useEffect(() => {
    if (settings?.platform !== "macos") return;
    refreshPermissions();
    window.addEventListener("focus", refreshPermissions);
    return () => window.removeEventListener("focus", refreshPermissions);
  }, [settings?.platform, refreshPermissions]);

  useEffect(() => {
    if (section !== "general") return;
    invoke<AudioInputDevice[]>("audio_input_devices")
      .then(setInputDevices)
      .catch(console.error);
    invoke<number>("recording_bytes")
      .then(setRecordingBytes)
      .catch(console.error);
  }, [section]);

  useEffect(() => {
    if (section !== "transcription") return;
    invoke<number>("model_size").then(setModelBytes).catch(console.error);
    const listener = listen("model-ready", () => {
      invoke<number>("model_size").then(setModelBytes).catch(console.error);
    });
    return () => {
      listener.then((remove) => remove());
    };
  }, [section]);

  async function requestPermission(
    kind: "microphone" | "accessibility" | "inputMonitoring",
  ) {
    setPermissionBusy(true);
    try {
      if (
        kind === "microphone" &&
        permissions?.microphone === "notDetermined"
      ) {
        await invoke("microphone_request");
      } else {
        await invoke("permissions_open", { kind });
      }
      refreshPermissions();
    } catch (reason) {
      console.error(reason);
    } finally {
      setPermissionBusy(false);
    }
  }

  async function updateHotkey(
    key: "hotkeyTrigger" | "hotkeyMode",
    value: string,
  ) {
    setBusy(true);
    let stopped = false;
    try {
      if (await invoke<boolean>("hotkey_status")) {
        await invoke("hotkey_stop");
        stopped = true;
      }
      const next = await invoke<SettingsSnapshot>("settings_set", {
        key,
        value,
      });
      setSettings(next);
    } catch (reason) {
      console.error(reason);
    } finally {
      if (stopped) {
        try {
          await invoke("hotkey_start");
        } catch (reason) {
          console.error(reason);
        }
      }
      setBusy(false);
    }
  }

  async function updateSetting(key: string, value: string | boolean | number) {
    const previousAppearance = String(settings?.values.appearance ?? "system");
    const previousDockVisibility = settings?.values.showInDock !== false;
    const previousLaunchAtLogin = launchAtLogin;
    let systemChanged = false;
    try {
      if (key === "appearance") {
        await setTheme(value === "system" ? null : (value as "light" | "dark"));
        systemChanged = true;
      }
      if (key === "showInDock" && settings?.platform === "macos") {
        await setDockVisibility(Boolean(value));
        systemChanged = true;
      }
      if (key === "launchAtLogin") {
        if (value) await enable();
        else await disable();
        systemChanged = true;
      }
      const next = await invoke<SettingsSnapshot>("settings_set", {
        key,
        value,
      });
      setSettings(next);
      setSettingError("");
      if (key.startsWith("cloudASR")) setCloudTestResult("");
      if (key === "launchAtLogin") setLaunchAtLogin(Boolean(value));
      if (key === "recordingRetention") {
        invoke<number>("recording_bytes")
          .then(setRecordingBytes)
          .catch(console.error);
      }
      return true;
    } catch (reason) {
      console.error(reason);
      setSettingError(String(reason));
      if (key.startsWith("cloudASR") || key === "transcriptionBackend") {
        invoke<SettingsSnapshot>("settings_get").then(setSettings).catch(console.error);
      }
      if (systemChanged) {
        try {
          if (key === "appearance") {
            await setTheme(
              previousAppearance === "system"
                ? null
                : (previousAppearance as "light" | "dark"),
            );
          } else if (key === "showInDock") {
            await setDockVisibility(previousDockVisibility);
          } else if (
            key === "launchAtLogin" &&
            previousLaunchAtLogin !== null
          ) {
            if (previousLaunchAtLogin) await enable();
            else await disable();
          }
        } catch (rollbackError) {
          console.error(rollbackError);
        }
      }
      return false;
    }
  }

  function editLocal(key: string, value: string) {
    if (!settings) return;
    setSettings({ ...settings, values: { ...settings.values, [key]: value } });
  }

  async function commit(key: string) {
    if (settings) await updateSetting(key, String(settings.values[key] ?? ""));
  }

  async function saveAPIKey() {
    if (!apiKeyDirty) return;
    if (await updateSetting("refineAPIKey", apiKeyDraft)) {
      setApiKeyDraft("");
      setApiKeyDirty(false);
    }
  }

  async function saveCloudKey() {
    if (!cloudKeyDirty) return;
    if (await updateSetting("cloudASRAPIKey", cloudKeyDraft)) {
      setCloudKeyDraft("");
      setCloudKeyDirty(false);
    }
  }

  async function testCloudRecognition() {
    setCloudTesting(true);
    setCloudTestResult("");
    try {
      await invoke("cloud_asr_test");
      setCloudTestResult("识别测试通过");
    } catch (reason) {
      setCloudTestResult(String(reason));
    } finally {
      setCloudTesting(false);
    }
  }

  async function exportDiagnostics() {
    setDiagnosticsError(false);
    try {
      await invoke("diagnostics_export");
    } catch (reason) {
      console.error(reason);
      setDiagnosticsError(true);
    }
  }

  const canUseHotkey =
    settings?.platform === "macos" || settings?.platform === "windows";
  const isWindows = settings?.platform === "windows";
  const triggerName = isWindows
    ? settings?.values.hotkeyTrigger === "leftControl"
      ? "左 Ctrl"
      : "右 Ctrl"
    : settings?.values.hotkeyTrigger === "function"
      ? "Fn 键"
      : settings?.values.hotkeyTrigger === "leftOption"
        ? "左 Option"
        : "右 Option";
  const title = {
    general: "通用",
    transcription: "识别",
    refine: "润色",
    permission: "权限",
    about: "关于",
  }[section];
  return (
    <>
      <header className="page-heading">
        <h1>{title}</h1>
      </header>
      {section === "about" && <AboutSettings />}
      {section === "general" && (
        <>
          <section className="surface settings-card" aria-label="热键">
            <h2>
              <Keyboard aria-hidden="true" />
              热键
            </h2>
            <div className="settings-row">
              <span>触发键</span>
              <SettingSelect
                label="触发键"
                value={String(
                  settings?.values.hotkeyTrigger ??
                    (isWindows ? "rightControl" : "rightOption"),
                )}
                onValueChange={(value) => updateHotkey("hotkeyTrigger", value)}
                disabled={!canUseHotkey || busy}
                options={
                  isWindows
                    ? [
                        { value: "rightControl", label: "右 Ctrl" },
                        { value: "leftControl", label: "左 Ctrl" },
                      ]
                    : [
                        { value: "rightOption", label: "右 Option" },
                        { value: "function", label: "Fn 键" },
                        { value: "leftOption", label: "左 Option" },
                      ]
                }
              />
            </div>
            <div className="settings-row">
              <span>触发方式</span>
              <SettingSelect
                label="触发方式"
                value={String(settings?.values.hotkeyMode ?? "hold")}
                onValueChange={(value) => updateHotkey("hotkeyMode", value)}
                disabled={!canUseHotkey || busy}
                options={[
                  { value: "hold", label: "按住说话" },
                  { value: "toggle", label: "按一下开始 / 再按结束" },
                ]}
              />
            </div>
            {(
              [
                ["听写", ""],
                ["翻译", "Shift"],
                ["询问", "Space"],
              ] as const
            ).map(([label, modifier]) => (
              <div className="settings-row" key={label}>
                <span>{label}</span>
                <span className="shortcut-group">
                  <kbd>{triggerName}</kbd>
                  {modifier && <kbd>{modifier}</kbd>}
                </span>
              </div>
            ))}
          </section>
          <section className="surface settings-card" aria-label="外观">
            <h2>
              <Palette aria-hidden="true" />
              外观
            </h2>
            <div className="settings-row">
              <span>主题</span>
              <SettingSelect
                label="主题"
                value={String(settings?.values.appearance ?? "system")}
                onValueChange={(value) => updateSetting("appearance", value)}
                disabled={!settings}
                options={[
                  { value: "system", label: "跟随系统" },
                  { value: "light", label: "浅色" },
                  { value: "dark", label: "深色" },
                ]}
              />
            </div>
            {settings?.platform === "macos" && (
              <label className="settings-row">
                <span>在 Dock 中显示</span>
                <Switch
                  aria-label="在 Dock 中显示"
                  checked={settings?.values.showInDock !== false}
                  onCheckedChange={(checked) =>
                    updateSetting("showInDock", checked)
                  }
                  disabled={!settings}
                />
              </label>
            )}
          </section>
          <section className="surface settings-card" aria-label="启动">
            <h2>
              <Zap aria-hidden="true" />
              启动
            </h2>
            <label className="settings-row">
              <span>登录时启动</span>
              <Switch
                aria-label="登录时启动"
                checked={launchAtLogin === true}
                onCheckedChange={(checked) =>
                  updateSetting("launchAtLogin", checked)
                }
                disabled={launchAtLogin === null}
              />
            </label>
          </section>
          <section className="surface settings-card" aria-label="语言">
            <h2>
              <Globe2 aria-hidden="true" />
              语言
            </h2>
            <label className="settings-row">
              <span>语言代码</span>
              <TextField.Root
                className="settings-field"
                value={String(settings?.values.language ?? "zh-CN")}
                onChange={(event) => editLocal("language", event.target.value)}
                onBlur={() => commit("language")}
                disabled={!settings}
              />
            </label>
            <label className="settings-row">
              <span>自动检测语言</span>
              <Switch
                aria-label="自动检测语言"
                checked={settings?.values.autoDetectLanguage === true}
                onCheckedChange={(checked) =>
                  updateSetting("autoDetectLanguage", checked)
                }
                disabled={!settings}
              />
            </label>
          </section>
          <section className="surface settings-card" aria-label="上下文">
            <h2>
              <ScanText aria-hidden="true" />
              上下文
            </h2>
            <label className="settings-row">
              <span>读取当前窗口内容</span>
              <Switch
                aria-label="读取当前窗口内容"
                checked={settings?.values.includeContext !== false}
                onCheckedChange={(checked) =>
                  updateSetting("includeContext", checked)
                }
                disabled={!settings || !canUseHotkey}
              />
            </label>
          </section>
          <section className="surface settings-card" aria-label="行为">
            <h2>
              <SlidersHorizontal aria-hidden="true" />
              行为
            </h2>
            <div className="settings-row">
              <span>写入方式</span>
              <SettingSelect
                label="写入方式"
                value={String(
                  settings?.values.injectionStrategy ?? "automatic",
                )}
                onValueChange={(value) =>
                  updateSetting("injectionStrategy", value)
                }
                disabled={!settings || !canUseHotkey}
                options={[
                  { value: "automatic", label: settings?.platform === "windows" ? "自动" : "自动（优先原生插入）" },
                  { value: "pasteboardOnly", label: "总是模拟粘贴" },
                ]}
              />
            </div>
            <label className="settings-row">
              <span>静音自动结束</span>
              <span className="settings-number">
                <TextField.Root
                  className="settings-field settings-number-field"
                  type="number"
                  min="0"
                  step="0.5"
                  value={String(settings?.values.silenceAutoStopSeconds ?? 0)}
                  onChange={(event) =>
                    editLocal("silenceAutoStopSeconds", event.target.value)
                  }
                  onBlur={(event) =>
                    updateSetting(
                      "silenceAutoStopSeconds",
                      Number(event.target.value),
                    )
                  }
                  disabled={!settings}
                />
                <span>秒</span>
              </span>
            </label>
          </section>
          <section className="surface settings-card" aria-label="音频">
            <h2>
              <AudioLines aria-hidden="true" />
              音频
            </h2>
            <div className="settings-row">
              <span>麦克风</span>
              <SettingSelect
                label="麦克风"
                value={String(settings?.values.preferredMicrophoneUID ?? "")}
                onValueChange={(value) =>
                  updateSetting("preferredMicrophoneUID", value)
                }
                disabled={!settings}
                options={[
                  { value: "", label: "系统默认" },
                  ...inputDevices.map((device) => ({
                    value: device.id,
                    label: device.name,
                  })),
                ]}
              />
            </div>
            {canUseHotkey && (
              <label className="settings-row">
                <span>操作提示音</span>
                <Switch
                  aria-label="操作提示音"
                  checked={settings?.values.interactionSounds !== false}
                  onCheckedChange={(checked) =>
                    updateSetting("interactionSounds", checked)
                  }
                  disabled={!settings}
                />
              </label>
            )}
            {canUseHotkey && (
              <label className="settings-row">
                <span>录音期间静音系统输出</span>
                <Switch
                  aria-label="录音期间静音系统输出"
                  checked={settings?.values.muteSystemOutput !== false}
                  onCheckedChange={(checked) =>
                    updateSetting("muteSystemOutput", checked)
                  }
                  disabled={!settings}
                />
              </label>
            )}
            <div className="settings-row">
              <span>音频保留</span>
              <SettingSelect
                label="音频保留"
                value={String(settings?.values.recordingRetention ?? "day30")}
                onValueChange={(value) =>
                  updateSetting("recordingRetention", value)
                }
                disabled={!settings}
                options={[
                  { value: "off", label: "不保留" },
                  { value: "day7", label: "保留 7 天" },
                  { value: "day30", label: "保留 30 天" },
                  { value: "forever", label: "永久保留" },
                ]}
              />
            </div>
            <div className="settings-row">
              <span>已占用</span>
              <span>
                {recordingBytes === null
                  ? ""
                  : new Intl.NumberFormat("zh-CN", {
                      style: "unit",
                      unit: "megabyte",
                      maximumFractionDigits: 1,
                    }).format(recordingBytes / 1_000_000)}
              </span>
            </div>
          </section>
          <section className="surface settings-card" aria-label="诊断">
            <h2>
              <Stethoscope aria-hidden="true" />
              诊断
            </h2>
            <div className="settings-row">
              <span>诊断报告</span>
              <Button
                className="settings-export"
                type="button"
                variant="surface"
                color="gray"
                onClick={exportDiagnostics}
              >
                导出…
              </Button>
            </div>
            <div className="settings-row">
              <span>运行日志</span>
              <Button
                className="settings-link"
                type="button"
                variant="ghost"
                onClick={() =>
                  invoke("diagnostics_reveal_log").catch(console.error)
                }
              >
                {isWindows ? "在资源管理器中显示" : "在 Finder 中显示"}
              </Button>
            </div>
            {diagnosticsError && (
              <div className="settings-error" role="alert">
                导出失败
              </div>
            )}
          </section>
        </>
      )}
      {section === "transcription" && (
        <>
          <section className="surface settings-card" aria-label="识别方式">
            <div className="settings-row">
              <span>识别方式</span>
              <SettingSelect
                label="识别方式"
                value={recognitionSource}
                onValueChange={(value) => {
                  void updateSetting("transcriptionBackend", value);
                }}
                disabled={!settings || cloudTesting}
                options={[
                  { value: "cloudASR", label: "云端识别" },
                  { value: "localWhisper", label: "本地 Whisper" },
                ]}
              />
            </div>
          </section>
          {settingError && (
            <div className="settings-error" role="alert">
              {settingError}
            </div>
          )}
          {recognitionSource === "cloudASR" ? (
            <section className="surface settings-card" aria-label="云端识别">
              <h2>
                <Server aria-hidden="true" />
                云端识别
              </h2>
              <div className="settings-row">
                <span>服务</span>
                <SettingSelect
                  label="识别服务"
                  value={String(settings?.values.cloudASRProvider ?? "openai")}
                  onValueChange={(value) =>
                    updateSetting("cloudASRProvider", value)
                  }
                  disabled={cloudTesting}
                  options={[
                    { value: "openai", label: "OpenAI" },
                    { value: "bailian", label: "阿里云百炼" },
                    { value: "compatible", label: "自定义" },
                  ]}
                />
              </div>
              {settings?.values.cloudASRProvider === "bailian" && (
                <div className="settings-row">
                  <span>接入方式</span>
                  <SettingSelect
                    label="接入方式"
                    value={settings.values.cloudASRProtocol === "audioMessages" ? "audioMessages" : "dashscope"}
                    onValueChange={(value) => updateSetting("cloudASRProtocol", value)}
                    disabled={cloudTesting}
                    options={[
                      { value: "audioMessages", label: "OpenAI 兼容" },
                      { value: "dashscope", label: "DashScope" },
                    ]}
                  />
                </div>
              )}
              {settings?.values.cloudASRProvider === "compatible" && (
              <div className="settings-row">
                <span>接口协议</span>
                <SettingSelect
                  label="接口协议"
                  value={String(settings?.values.cloudASRProtocol ?? "transcription")}
                  onValueChange={(value) => updateSetting("cloudASRProtocol", value)}
                  disabled={cloudTesting}
                  options={[
                    { value: "transcription", label: "OpenAI 音频转写" },
                    { value: "audioMessages", label: "OpenAI 音频消息" },
                    { value: "dashscope", label: "DashScope 同步识别" },
                  ]}
                />
              </div>
              )}
              {settings && settings.values.cloudASRProvider !== "openai" && (
                <div className="settings-row">
                  <span>使用完整请求地址</span>
                  <Switch
                    aria-label="使用完整请求地址"
                    checked={Boolean(settings.values.cloudASRFullURL)}
                    onCheckedChange={(value) => updateSetting("cloudASRFullURL", value)}
                    disabled={cloudTesting}
                  />
                </div>
              )}
              {settings && settings.values.cloudASRProvider !== "openai" && (
                <label className="settings-row">
                  <span>{settings.values.cloudASRFullURL ? "请求地址" : "Base URL"}</span>
                  <TextField.Root
                    className="settings-field"
                    value={String(settings.values.cloudASRBaseURL)}
                    onChange={(event) =>
                      editLocal("cloudASRBaseURL", event.target.value)
                    }
                    onBlur={() => commit("cloudASRBaseURL")}
                    disabled={cloudTesting}
                  />
                </label>
              )}
              <label className="settings-row">
                <span>模型</span>
                <TextField.Root
                  className="settings-field"
                  value={String(settings?.values.cloudASRModel ?? "whisper-1")}
                  onChange={(event) =>
                    editLocal("cloudASRModel", event.target.value)
                  }
                  onBlur={() => commit("cloudASRModel")}
                  disabled={cloudTesting}
                />
              </label>
              <div className="settings-row">
                <span>API Key</span>
                <div className="settings-cloud-key">
                  <TextField.Root
                    className="settings-field"
                    aria-label="云端识别 API Key"
                    type="password"
                    autoComplete="off"
                    value={cloudKeyDraft}
                    placeholder={settings?.cloudASRAPIKeySet ? "••••••••" : ""}
                    onChange={(event) => {
                      setCloudKeyDraft(event.target.value);
                      setCloudKeyDirty(true);
                    }}
                    onKeyDown={(event) => {
                      if (
                        (event.key === "Backspace" || event.key === "Delete") &&
                        !cloudKeyDraft
                      )
                        setCloudKeyDirty(true);
                    }}
                    onBlur={saveCloudKey}
                    disabled={cloudTesting}
                  />
                  <Button
                    variant="ghost"
                    color="gray"
                    disabled={cloudTesting}
                    onClick={async () => {
                      if (await updateSetting("cloudASRAPIKey", "")) {
                        setCloudKeyDraft("");
                        setCloudKeyDirty(false);
                      }
                    }}
                  >
                    清除
                  </Button>
                </div>
              </div>
              <div className="settings-row">
                <span>录音将发送至所选服务</span>
                <Button
                  variant="surface"
                  loading={cloudTesting}
                  disabled={
                    !settings?.cloudASRAPIKeySet ||
                    !String(settings.values.cloudASRModel).trim() ||
                    cloudKeyDirty
                  }
                  onClick={testCloudRecognition}
                >
                  测试识别
                </Button>
              </div>
              {cloudTestResult && (
                <div className="settings-row" role="status">
                  {cloudTestResult}
                </div>
              )}
            </section>
          ) : (
            <section
              className="surface settings-card"
              aria-label="本机 Whisper"
            >
              <h2>
                <Cpu aria-hidden="true" />
                本机 Whisper
              </h2>
              <div className="settings-row">
                <span>模型</span>
                <SettingSelect
                  label="模型"
                  value={String(
                    settings?.values.localWhisperModel ??
                      "large-v3-v20240930_626MB",
                  )}
                  onValueChange={(value) =>
                    updateSetting("localWhisperModel", value)
                  }
                  disabled={!settings}
                  options={[
                    { value: "tiny", label: "Tiny" },
                    { value: "base", label: "Base" },
                    { value: "small", label: "Small" },
                    {
                      value: "large-v3-v20240930_626MB",
                      label: "Large v3 Turbo",
                    },
                  ]}
                />
              </div>
              <div className="settings-row">
                <span>已占用</span>
                <span>
                  {modelBytes === null
                    ? ""
                    : modelBytes === 0
                      ? "尚未下载"
                      : new Intl.NumberFormat("zh-CN", {
                          style: "unit",
                          unit: "megabyte",
                          maximumFractionDigits: 1,
                        }).format(modelBytes / 1_000_000)}
                </span>
              </div>
            </section>
          )}
        </>
      )}
      {section === "refine" && (
        <>
          <section className="surface settings-card" aria-label="整理风格">
            <h2>
              <WandSparkles aria-hidden="true" />
              整理风格
            </h2>
            <div className="settings-row">
              <span>风格</span>
              <SettingSelect
                label="风格"
                value={String(settings?.values.refineStyle ?? "clean")}
                onValueChange={(value) => updateSetting("refineStyle", value)}
                disabled={!settings}
                options={[
                  { value: "off", label: "不处理" },
                  { value: "clean", label: "去口语" },
                  { value: "polish", label: "通顺润色" },
                  { value: "formal", label: "转为书面语" },
                  { value: "concise", label: "精简" },
                  { value: "custom", label: "自定义" },
                ]}
              />
            </div>
            {settings?.values.refineStyle === "custom" && (
              <label className="settings-row settings-multiline">
                <span>自定义指令</span>
                <TextArea
                  className="settings-textarea"
                  value={String(settings.values.customRefineInstruction ?? "")}
                  onChange={(event) =>
                    editLocal("customRefineInstruction", event.target.value)
                  }
                  onBlur={() => commit("customRefineInstruction")}
                />
              </label>
            )}
          </section>
          <section className="surface settings-card" aria-label="文本格式">
            <h2><ScanText aria-hidden="true" />文本格式</h2>
            {([
              ["chineseEnglishSpacing", "中英文间距", "space", "自动添加"],
              ["chineseNumberSpacing", "中文与数字间距", "space", "自动添加"],
              ["chinesePunctuation", "中文标点", "standard", "规范中文标点"],
            ] as const).map(([key, label, value, option]) => (
              <div className="settings-row" key={key}>
                <span>{label}</span>
                <SettingSelect label={label} value={String(settings?.values[key] ?? "preserve")} disabled={!settings}
                  onValueChange={value => updateSetting(key, value)}
                  options={[{ value: "preserve", label: "保持原样" }, { value, label: option }]} />
              </div>
            ))}
          </section>
          <section className="surface settings-card" aria-label="翻译">
            <h2>
              <Languages aria-hidden="true" />
              翻译
            </h2>
            <label className="settings-row">
              <span>目标语言</span>
              <TextField.Root
                className="settings-field"
                value={String(
                  settings?.values.translationTargetLanguage ?? "en-US",
                )}
                onChange={(event) =>
                  editLocal("translationTargetLanguage", event.target.value)
                }
                onBlur={() => commit("translationTargetLanguage")}
                disabled={!settings}
              />
            </label>
          </section>
          <section className="surface settings-card" aria-label="模型接口">
            <h2>
              <Server aria-hidden="true" />
              模型接口
            </h2>
            <label className="settings-row">
              <span>Base URL</span>
              <TextField.Root
                className="settings-field"
                value={String(settings?.values.refineBaseURL ?? "")}
                onChange={(event) =>
                  editLocal("refineBaseURL", event.target.value)
                }
                onBlur={() => commit("refineBaseURL")}
                disabled={!settings}
              />
            </label>
            <label className="settings-row">
              <span>模型</span>
              <TextField.Root
                className="settings-field"
                value={String(settings?.values.refineModel ?? "")}
                onChange={(event) =>
                  editLocal("refineModel", event.target.value)
                }
                onBlur={() => commit("refineModel")}
                disabled={!settings}
              />
            </label>
            <label className="settings-row">
              <span>API Key</span>
              <span className="settings-secret">
                <TextField.Root
                  className="settings-field"
                  type="password"
                  value={apiKeyDraft}
                  placeholder={settings?.refineAPIKeySet ? "••••••••" : ""}
                  onChange={(event) => {
                    setApiKeyDraft(event.target.value);
                    setApiKeyDirty(true);
                  }}
                  onKeyDown={(event) => {
                    if (
                      (event.key === "Backspace" || event.key === "Delete") &&
                      !event.currentTarget.value &&
                      settings?.refineAPIKeySet
                    )
                      setApiKeyDirty(true);
                  }}
                  onBlur={saveAPIKey}
                  disabled={!settings}
                />
              </span>
            </label>
          </section>
        </>
      )}
      {section === "permission" && settings?.platform === "macos" && (
        <section className="surface settings-card" aria-label="权限">
          <h2>
            <Shield aria-hidden="true" />
            权限
          </h2>
          {(
            [
              ["microphone", "麦克风"],
              ["accessibility", "辅助功能"],
              ["inputMonitoring", "输入监控"],
            ] as const
          ).map(([kind, label]) => {
            const granted =
              kind === "microphone"
                ? permissions?.microphone === "authorized"
                : permissions?.[kind] === true;
            return (
              <div className="settings-row" key={kind}>
                <span className="permission-name">
                  <span
                    className={`permission-mark ${granted ? "granted" : ""}`}
                    role="status"
                    aria-label={granted ? "已授权" : "缺失"}
                  >
                    {granted ? "✓" : "○"}
                  </span>
                  {label}
                </span>
                {!granted && (
                  <Button
                    type="button"
                    className="permission-action"
                    disabled={permissionBusy || !permissions}
                    aria-busy={permissionBusy}
                    onClick={() => requestPermission(kind)}
                  >
                    {kind === "microphone" &&
                    permissions?.microphone === "notDetermined"
                      ? "授权"
                      : "去开启"}
                  </Button>
                )}
              </div>
            );
          })}
        </section>
      )}
    </>
  );
}
