import { Button } from "@radix-ui/themes";
import {
  AudioLines,
  CircleAlert,
  CaseUpper,
  Clock3,
  FileText,
  Gauge,
  History,
  Languages,
  Mail,
  MessageCircle,
  MessagesSquare,
  ShieldAlert,
  ShieldCheck,
  Sparkles,
  TextCursorInput,
  WandSparkles,
  type LucideIcon,
} from "lucide-react";
import type {
  HistoryEntry,
  MacPermissions,
  SettingsSnapshot,
  VoiceMode,
} from "./types";

type Props = {
  history: HistoryEntry[];
  settings: SettingsSnapshot | null;
  permissions: MacPermissions | null;
  selectedMode: VoiceMode;
  selectedUseCaseID: string | null;
  onSelectMode: (mode: VoiceMode, style?: string, useCaseID?: string) => void;
  onOpenPermissions: () => void;
  onOpenRecognition: () => void;
  onOpenRefinement: () => void;
};

type UseCase = {
  id: string;
  title: string;
  detail: string;
  icon: LucideIcon;
  color: string;
  mode: VoiceMode;
  style?: string;
};

const useCases: UseCase[] = [
  { id: "mail", title: "邮件", detail: "整理为清晰的书面表达", icon: Mail, color: "blue", mode: "dictate", style: "formal" },
  { id: "update", title: "团队消息", detail: "快速同步工作进展", icon: MessagesSquare, color: "mint", mode: "dictate", style: "clean" },
  { id: "prompt", title: "AI 提问", detail: "说出问题并生成回答", icon: Sparkles, color: "purple", mode: "ask" },
  { id: "document", title: "文档", detail: "整理口述内容为连贯文稿", icon: FileText, color: "indigo", mode: "dictate", style: "polish" },
  { id: "message", title: "快速消息", detail: "保留自然简洁的语气", icon: MessageCircle, color: "green", mode: "dictate", style: "concise" },
  { id: "edit", title: "编辑文字", detail: "改写当前选中的内容", icon: TextCursorInput, color: "orange", mode: "ask" },
];

function durationLabel(seconds: number) {
  const rounded = Math.round(seconds);
  if (rounded >= 3600) return `${Math.floor(rounded / 3600)}小时${Math.floor((rounded % 3600) / 60)}分钟`;
  if (rounded >= 60) return `${Math.floor(rounded / 60)}分钟${rounded % 60}秒`;
  return `${rounded}秒`;
}

export default function HomeView({
  history,
  settings,
  permissions,
  selectedMode,
  selectedUseCaseID,
  onSelectMode,
  onOpenPermissions,
  onOpenRecognition,
  onOpenRefinement,
}: Props) {
  const trigger = String(settings?.values.hotkeyTrigger ?? "rightOption");
  const triggerLabel = settings?.platform === "windows"
    ? trigger === "leftControl" ? "左 Ctrl" : "右 Ctrl"
    : trigger === "function" ? "Fn 键" : trigger === "leftOption" ? "左 Option" : "右 Option";
  const targetLanguage = String(settings?.values.translationTargetLanguage ?? "en-US");
  const modelName = String(settings?.values.refineModel ?? "").trim();
  const refinementConfigured = settings?.refineAPIKeySet && modelName.length > 0 && String(settings.values.refineBaseURL ?? "").trim().length > 0;
  const whisperModel = String(settings?.values.localWhisperModel ?? "large-v3-v20240930_626MB");
  const whisperName = ({ tiny: "Tiny", base: "Base", small: "Small", "large-v3-v20240930_626MB": "Large v3 Turbo" } as Record<string, string>)[whisperModel];
  const duration = history.reduce((sum, entry) => sum + entry.duration, 0);
  const characters = history.reduce(
    (sum, entry) => sum + Array.from(entry.insertedText ?? entry.refinedText).length,
    0,
  );
  const speed = duration > 0 ? Math.trunc((characters / duration) * 60) : 0;
  const granted = permissions
    ? Number(permissions.microphone === "authorized") + Number(permissions.accessibility) + Number(permissions.inputMonitoring)
    : 0;
  const cloudIncomplete = settings?.values.transcriptionBackend === "cloudASR" && (!settings.cloudASRAPIKeySet || !String(settings.values.cloudASRModel ?? "").trim() || !String(settings.values.cloudASRBaseURL ?? "").trim());
  const modes: { id: VoiceMode; title: string; detail: string; icon: LucideIcon; key?: string }[] = [
    { id: "dictate", title: "听写", detail: "整理语音并写入当前光标", icon: AudioLines },
    { id: "translate", title: "翻译", detail: `翻译为 ${targetLanguage}`, icon: Languages, key: "Shift" },
    { id: "ask", title: "询问", detail: "编辑选中文字或回答问题", icon: Sparkles, key: "Space" },
  ];

  return (
    <div className="voice-home">
      <header className="page-heading">
        <h1>语音输入</h1>
      </header>
      <div className="voice-layout">
        <div className="voice-primary">
          <div className="surface voice-modes" role="group" aria-label="语音输入">
            {modes.map((mode) => {
              const Icon = mode.icon;
              return (
                <button
                  type="button"
                  key={mode.id}
                  className={`voice-mode ${selectedMode === mode.id ? "selected" : ""}`}
                  aria-pressed={selectedMode === mode.id}
                  onClick={() => onSelectMode(mode.id)}
                >
                  <Icon size={20} strokeWidth={1.9} aria-hidden="true" />
                  <span className="voice-mode-copy">
                    <strong>{mode.title}</strong>
                    <small>{mode.detail}</small>
                  </span>
                  <span className="shortcut-group">
                    <kbd>{triggerLabel}</kbd>
                    {mode.key && <kbd>{mode.key}</kbd>}
                  </span>
                </button>
              );
            })}
          </div>
          <h2 className="use-case-heading">常用场景</h2>
          <div className="use-case-grid">
            {useCases.map((item) => {
              const Icon = item.icon;
              return (
                <button
                  type="button"
                  key={item.id}
                  className={`surface use-case ${selectedUseCaseID === item.id ? "selected" : ""}`}
                  aria-pressed={selectedUseCaseID === item.id}
                  onClick={() => onSelectMode(item.mode, item.style, item.id)}
                >
                  <Icon className={`use-case-icon ${item.color}`} size={26} strokeWidth={1.8} aria-hidden="true" />
                  <strong>{item.title}</strong>
                  <small>{item.detail}</small>
                </button>
              );
            })}
          </div>
        </div>
        <aside className="voice-aside">
          <div className="surface voice-status">
            <div className="voice-status-head">
              <strong>快捷键</strong>
              <kbd>{triggerLabel}</kbd>
            </div>
            <div className="voice-stat">
              <Clock3 aria-hidden="true" />
              <span><strong>{durationLabel(duration)}</strong><small>语音时长</small></span>
            </div>
            <div className="voice-stat">
              <CaseUpper aria-hidden="true" />
              <span><strong>{new Intl.NumberFormat("zh-CN", { notation: "compact" }).format(characters)}</strong><small>输出字符</small></span>
            </div>
            <div className="voice-stat">
              <Gauge aria-hidden="true" />
              <span><strong>{speed}/分</strong><small>平均速度</small></span>
            </div>
            <div className="voice-stat">
              <History aria-hidden="true" />
              <span><strong>{history.length}</strong><small>历史记录</small></span>
            </div>
            {permissions && (
              <div className="voice-permissions">
                {granted === 3 ? <ShieldCheck aria-hidden="true" /> : <ShieldAlert aria-hidden="true" />}
                <span>权限 {granted} / 3</span>
                {granted < 3 && <button type="button" onClick={onOpenPermissions}>去授权</button>}
              </div>
            )}
          </div>
          <div className="surface voice-provider">
            <div className="provider-section">
              <span><AudioLines size={16} aria-hidden="true" />{settings?.values.transcriptionBackend === "cloudASR" ? "云端识别" : "语音识别"}</span>
              {cloudIncomplete
                ? <Button className="voice-configure" variant="outline" color="red" size="1" onClick={onOpenRecognition}><CircleAlert size={14} aria-hidden="true" />未配置识别</Button>
                : <strong>{settings?.values.transcriptionBackend === "cloudASR" ? String(settings.values.cloudASRModel) : `Whisper ${whisperName}`}</strong>}
            </div>
            <div className="provider-section">
              <span><WandSparkles size={16} aria-hidden="true" />文字处理</span>
              {refinementConfigured
                ? <strong>{modelName}</strong>
                : <Button className="voice-configure" variant="outline" color="red" size="1" onClick={onOpenRefinement}><CircleAlert size={14} aria-hidden="true" />未配置文字处理</Button>}
            </div>
          </div>
        </aside>
      </div>
    </div>
  );
}
