import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { CirclePlay, CircleStop, Search, TriangleAlert } from "lucide-react";
import { Button, Dialog, SegmentedControl, TextArea, TextField } from "@radix-ui/themes";
import { ContextMenu } from "radix-ui";
import OpenCC from "opencc-js/t2cn";
import type { HistoryEntry } from "./types";

type Props = {
  mutationError: string;
  entries: HistoryEntry[];
  onMutate: (command: string, args?: Record<string, unknown>) => Promise<boolean>;
  onDelete: (id: string) => void;
  onClear: () => void;
};

const toSimplified = OpenCC.Converter({ from: "tw", to: "cn" });

function displayText(text: string, language: string | null) {
  return language?.toLowerCase().startsWith("zh") ? toSimplified(text) : text;
}

function modeLabel(mode: string | null) {
  switch (mode) {
    case "translate": return "翻译";
    case "ask": return "询问";
    default: return "听写";
  }
}

function dateLabel(value: string, options: Intl.DateTimeFormatOptions) {
  const date = new Date(value);
  return Number.isNaN(date.getTime())
    ? value
    : new Intl.DateTimeFormat("zh-CN", options).format(date);
}

function latencyDescription(entry: HistoryEntry) {
  const parts: string[] = [];
  if (entry.transcriptionDuration != null) parts.push(`网络与识别 ${entry.transcriptionDuration.toFixed(2)}s`);
  if (entry.encodingFinalizationDuration) parts.push(`编码收尾 ${entry.encodingFinalizationDuration.toFixed(2)}s`);
  if (entry.modelDuration) parts.push(`模型 ${entry.modelDuration.toFixed(2)}s`);
  if (entry.insertionDuration) parts.push(`写入 ${entry.insertionDuration.toFixed(2)}s`);
  return parts.join(" · ");
}

export default function HistoryView({ entries, onDelete, onClear, onMutate, mutationError }: Props) {
  const [editing, setEditing] = useState<HistoryEntry | null>(null);
  const [corrected, setCorrected] = useState("");
  const [saving, setSaving] = useState(false);
  const [query, setQuery] = useState("");
  const [mode, setMode] = useState("all");
  const [expanded, setExpanded] = useState<string | null>(null);
  const [copiedID, setCopiedID] = useState<string | null>(null);
  const [playingID, setPlayingID] = useState<string | null>(null);
  const player = useRef<HTMLAudioElement | null>(null);
  const playRequest = useRef(0);

  useEffect(() => () => {
    playRequest.current += 1;
    player.current?.pause();
  }, []);

  const filtered = useMemo(() => {
    const raw = query.trim().toLocaleLowerCase();
    const normalized = toSimplified(raw);
    return entries.filter((entry) => {
      if (mode !== "all" && (entry.mode || "dictate") !== mode) return false;
      if (!raw) return true;
      const transcript = displayText(entry.transcript, entry.language).toLocaleLowerCase();
      const refined = displayText(entry.refinedText, entry.language).toLocaleLowerCase();
      return (entry.userCorrectedText ?? "").toLocaleLowerCase().includes(raw)
        || transcript.includes(normalized)
        || refined.includes(normalized)
        || entry.transcript.toLocaleLowerCase().includes(raw)
        || entry.refinedText.toLocaleLowerCase().includes(raw)
        || (entry.appName ?? "").toLocaleLowerCase().includes(raw);
    });
  }, [entries, query, mode]);

  const groups = useMemo(() => {
    const grouped = new Map<string, HistoryEntry[]>();
    for (const entry of filtered) {
      const date = new Date(entry.createdAt);
      const key = Number.isNaN(date.getTime()) ? entry.createdAt : date.toDateString();
      grouped.set(key, [...(grouped.get(key) ?? []), entry]);
    }
    return Array.from(grouped, ([key, group]) => ({ key, entries: group }))
      .sort((a, b) => new Date(b.entries[0].createdAt).getTime() - new Date(a.entries[0].createdAt).getTime());
  }, [filtered]);

  function edit(entry: HistoryEntry) {
    setEditing(entry);
    setCorrected(entry.userCorrectedText ?? entry.insertedText ?? entry.formattedText ?? entry.refinedText);
  }

  async function saveCorrection() {
    if (!editing || saving) return;
    setSaving(true);
    if (await onMutate("history_correct", { id: editing.id, text: corrected })) setEditing(null);
    setSaving(false);
  }

  async function copy(entry: HistoryEntry) {
    const text = displayText(entry.userCorrectedText ?? entry.insertedText ?? entry.formattedText ?? entry.refinedText, entry.language);
    if (!text) return;
    try {
      await writeText(text);
      setCopiedID(entry.id);
      window.setTimeout(() => setCopiedID((current) => current === entry.id ? null : current), 1500);
    } catch (error) {
      console.error(error);
    }
  }

  async function togglePlay(entry: HistoryEntry) {
    const request = ++playRequest.current;
    if (playingID === entry.id) {
      player.current?.pause();
      player.current = null;
      setPlayingID(null);
      return;
    }
    player.current?.pause();
    player.current = null;
    setPlayingID(null);
    try {
      const source = await invoke<string>("history_audio", { id: entry.id });
      if (request !== playRequest.current) return;
      const audio = new Audio(source);
      audio.onended = () => {
        if (player.current === audio) {
          player.current = null;
          setPlayingID(null);
        }
      };
      await audio.play();
      if (request !== playRequest.current) {
        audio.pause();
        return;
      }
      if (audio.ended) return;
      player.current = audio;
      setPlayingID(entry.id);
    } catch (error) {
      if (request === playRequest.current) {
        player.current = null;
        setPlayingID(null);
        console.error(error);
      }
    }
  }

  return (
    <div className="history-page">
      <header className="page-heading">
        <h1>历史记录</h1>
        <TextField.Root className="search-field" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="搜索转写内容" aria-label="搜索转写内容">
          <TextField.Slot><Search size={16} aria-hidden="true" /></TextField.Slot>
        </TextField.Root>
      </header>
      <div className="filters">
        <SegmentedControl.Root className="segmented-filter" value={mode} onValueChange={(value) => { if (value) setMode(value); }} aria-label="识别模式">
          {([ ["all", "全部"], ["dictate", "听写"], ["translate", "翻译"], ["ask", "询问"] ] as const).map(([value, label]) => (
            <SegmentedControl.Item key={value} value={value}>{label}</SegmentedControl.Item>
          ))}
        </SegmentedControl.Root>
        <span className="count">{filtered.length} 条</span>
        <button className="text-action" type="button" disabled={!entries.length} onClick={onClear}>清空</button>
      </div>
      {filtered.length === 0 ? (
        <div className="history-empty">{query ? "没有匹配的记录" : "还没有任何记录"}</div>
      ) : (
        <div className="history-groups">
          {groups.map((group) => (
            <section className="history-group" key={group.key}>
              <h2>{dateLabel(group.entries[0].createdAt, { year: "numeric", month: "long", day: "numeric" })}</h2>
              {group.entries.map((entry) => {
                const result = displayText(entry.userCorrectedText ?? entry.insertedText ?? entry.formattedText ?? entry.refinedText, entry.language);
                const transcript = displayText(entry.transcript, entry.language);
                return (
                  <ContextMenu.Root key={entry.id}>
                    <ContextMenu.Trigger asChild>
                      <article className="surface history-card">
                        <div className="card-meta">
                          {entry.backend === "Cloud" && <span>云端 · {entry.asrProvider === "openai" ? "OpenAI" : "兼容服务"}</span>}
                          <span>{dateLabel(entry.createdAt, { month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit" })}</span>
                          {entry.appName && <span className="app-name">{entry.appName}</span>}
                          <span>{entry.duration.toFixed(1)}s</span>
                          {entry.processingDuration != null && <span title={latencyDescription(entry)}>处理 {entry.processingDuration.toFixed(1)}s</span>}
                          <span>{modeLabel(entry.mode)}</span>
                          <div className="history-actions">
                            {entry.error && <span title={entry.error}><TriangleAlert size={14} aria-label={entry.error} /></span>}
                            {entry.hasAudio && (
                              <button type="button" className="icon-action" onClick={() => togglePlay(entry)} aria-label={playingID === entry.id ? "停止播放" : "回放这段录音"}>
                                {playingID === entry.id ? <CircleStop size={16} /> : <CirclePlay size={16} />}
                              </button>
                            )}
                            <button type="button" disabled={!result} onClick={() => copy(entry)}>{copiedID === entry.id ? "已复制" : "复制"}</button>
                            <Button variant="ghost" color="gray" disabled={!result} onClick={() => edit(entry)}>纠正</Button>
                            <button type="button" onClick={() => onDelete(entry.id)}>删除</button>
                          </div>
                        </div>
                        <div className={`entry-text ${expanded === entry.id ? "expanded" : ""}`}>{result || entry.error || "没有内容"}</div>
                        {transcript && (
                          <>
                            <button type="button" className="disclosure" aria-expanded={expanded === entry.id} onClick={() => setExpanded(expanded === entry.id ? null : entry.id)}>
                              {expanded === entry.id ? "⌄" : "›"} 处理对比
                            </button>
                            {expanded === entry.id && <div className="history-comparison">
                              <section><strong>识别原文</strong><div>{entry.transcript}</div></section>
                              {entry.ruleCorrectedText != null && <section><strong>词典纠正</strong><div>{entry.ruleCorrectedText}</div>
                                {!!entry.appliedRules?.length && <ul className="applied-rules">{entry.appliedRules.map(rule => <li key={rule.id}>{rule.from} → {rule.to} × {rule.count}</li>)}</ul>}
                              </section>}
                              <section><strong>{entry.modelDuration === 0 ? "输出结果" : "模型整理"}</strong><div>{entry.refinedText}</div></section>
                              {entry.formattedText != null && entry.formattedText !== entry.refinedText && <section><strong>文本格式</strong><div>{entry.formattedText}</div></section>}
                              {entry.insertedText != null && entry.insertedText !== (entry.formattedText ?? entry.refinedText) && <section><strong>实际写入</strong><div>{entry.insertedText}</div></section>}
                              {entry.userCorrectedText != null && <section><strong>手动纠正</strong><div>{entry.userCorrectedText}</div></section>}
                            </div>}
                          </>
                        )}
                      </article>
                    </ContextMenu.Trigger>
                    <ContextMenu.Portal>
                      <ContextMenu.Content className="entry-context-menu" collisionPadding={8}>
                        <ContextMenu.Item className="entry-context-item" disabled={!result} onSelect={() => copy(entry)}>复制</ContextMenu.Item>
                        <ContextMenu.Item className="entry-context-item" disabled={!result} onSelect={() => edit(entry)}>纠正</ContextMenu.Item>
                        <ContextMenu.Item className="entry-context-item danger" onSelect={() => onDelete(entry.id)}>删除</ContextMenu.Item>
                      </ContextMenu.Content>
                    </ContextMenu.Portal>
                  </ContextMenu.Root>
                );
              })}
            </section>
          ))}
        </div>
      )}
      <Dialog.Root open={editing !== null} onOpenChange={open => { if (!open && !saving) setEditing(null); }}>
        <Dialog.Content aria-describedby={undefined}>
          <Dialog.Title>纠正文本</Dialog.Title>
          <TextArea className="history-correction-input" value={corrected} onChange={e => setCorrected(e.target.value)} aria-label="纠正文本" maxLength={20000} />
          {mutationError && <div className="error" role="alert">{mutationError}</div>}
          <div className="dictionary-dialog-actions">
            <Button variant="surface" color="gray" disabled={saving} onClick={() => setEditing(null)}>取消</Button>
            <Button disabled={saving || !corrected.trim() || corrected.trim() === (editing?.userCorrectedText ?? editing?.insertedText ?? editing?.formattedText ?? editing?.refinedText)} onClick={saveCorrection}>保存</Button>
          </div>
        </Dialog.Content>
      </Dialog.Root>
    </div>
  );
}
