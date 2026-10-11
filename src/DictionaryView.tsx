import { useRef, useState } from "react";
import { Button, Dialog, TextField, SegmentedControl } from "@radix-ui/themes";
import { ContextMenu } from "radix-ui";
import { BookText, Search, Sparkles, Text } from "lucide-react";
import ReplacementRules from "./ReplacementRules";
import type { DictionaryEntry, ReplacementRule } from "./types";

type Props = {
  mutationError: string;
  entries: DictionaryEntry[];
  rules: ReplacementRule[];
  onMutate: (command: string, args?: Record<string, unknown>) => Promise<boolean>;
};

export default function DictionaryView({ entries, rules, onMutate, mutationError }: Props) {
  const [tab, setTab] = useState("words");
  const [query, setQuery] = useState("");
  const [newTerm, setNewTerm] = useState("");
  const [editing, setEditing] = useState<DictionaryEntry | null>(null);
  const [editedTerm, setEditedTerm] = useState("");
  const importInput = useRef<HTMLInputElement>(null);
  const search = query.trim().toLocaleLowerCase();
  const filtered = entries.filter((entry) => (tab === "pending" ? entry.source === "suggested" : entry.source !== "suggested") && entry.term.toLocaleLowerCase().includes(search));

  async function addTerm() {
    if (await onMutate("dictionary_add", { term: newTerm })) setNewTerm("");
  }

  async function importTerms(file: File | undefined) {
    if (!file) return;
    try {
      const content = new TextDecoder("utf-8", { fatal: true }).decode(await file.arrayBuffer());
      await onMutate("dictionary_import", { content });
    } catch (error) {
      console.error(error);
    } finally {
      if (importInput.current) importInput.current.value = "";
    }
  }

  async function saveTerm() {
    if (!editing) return;
    if (await onMutate("dictionary_update", { id: editing.id, term: editedTerm })) {
      setEditing(null);
    }
  }

  return (
    <div className="dictionary-page">
      <header className="page-heading">
        <h1>词典</h1>
        <TextField.Root className="search-field" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="搜索词典" aria-label="搜索词典">
          <TextField.Slot><Search size={16} aria-hidden="true" /></TextField.Slot>
        </TextField.Root>
      </header>
      <SegmentedControl.Root className="dictionary-tabs" value={tab} onValueChange={setTab} aria-label="词典分类">
        <SegmentedControl.Item value="words">专有词</SegmentedControl.Item>
        <SegmentedControl.Item value="rules">替换规则</SegmentedControl.Item>
        <SegmentedControl.Item value="pending">待确认 {entries.filter(e => e.source === "suggested").length + rules.filter(r => r.source === "suggested").length}</SegmentedControl.Item>
      </SegmentedControl.Root>
      {tab !== "words" && <ReplacementRules mutationError={mutationError} rules={rules} pending={tab === "pending"} query={query} onMutate={onMutate} />}
      {tab === "words" && <div className="dictionary-toolbar">
        <TextField.Root className="dictionary-input"
          value={newTerm}
          onChange={(event) => setNewTerm(event.target.value)}
          onKeyDown={(event) => { if (event.key === "Enter") void addTerm(); }}
          placeholder="添加专有名词"
          aria-label="添加专有名词"
        />
        <Button type="button" disabled={!newTerm.trim()} onClick={addTerm}>添加</Button>
        <Button type="button" variant="surface" color="gray" onClick={() => importInput.current?.click()}>导入</Button>
        <span className="count">{entries.filter(e => e.source !== "suggested").length} 个词条</span>
        <input
          ref={importInput}
          className="file-input"
          type="file"
          accept=".txt,.csv,text/plain,text/csv"
          onChange={(event) => importTerms(event.target.files?.[0])}
          aria-label="导入词典"
        />
      </div>}
      {tab !== "rules" && (filtered.length === 0 ? (
        tab === "words" ? <div className="dictionary-empty"><BookText size={22} aria-hidden="true" />词典为空</div> : null
      ) : (
        <div className="dictionary-list">
          {filtered.map((entry) => (
            <ContextMenu.Root key={entry.id}>
              <ContextMenu.Trigger asChild>
                <div className={`surface dictionary-row ${entry.source}`}>
                  {entry.source === "manual" ? <Text size={16} aria-hidden="true" /> : <Sparkles size={16} aria-hidden="true" />}
                  <span>{entry.term}</span>
                  <div className="dictionary-actions">
                    {entry.source === "suggested" ? (
                      <>
                        <button type="button" onClick={() => onMutate("dictionary_accept", { id: entry.id })}>接受</button>
                        <button type="button" onClick={() => onMutate("dictionary_delete", { id: entry.id })}>忽略</button>
                      </>
                    ) : (
                      <>
                        <button type="button" onClick={() => { setEditing(entry); setEditedTerm(entry.term); }}>编辑</button>
                        <button type="button" onClick={() => onMutate("dictionary_delete", { id: entry.id })}>删除</button>
                      </>
                    )}
                  </div>
                </div>
              </ContextMenu.Trigger>
              <ContextMenu.Portal>
                <ContextMenu.Content className="entry-context-menu" collisionPadding={8}>
                  {entry.source === "suggested" ? <>
                    <ContextMenu.Item className="entry-context-item" onSelect={() => onMutate("dictionary_accept", { id: entry.id })}>接受</ContextMenu.Item>
                    <ContextMenu.Item className="entry-context-item danger" onSelect={() => onMutate("dictionary_delete", { id: entry.id })}>忽略</ContextMenu.Item>
                  </> : <>
                    <ContextMenu.Item className="entry-context-item" onSelect={() => { setEditing(entry); setEditedTerm(entry.term); }}>编辑</ContextMenu.Item>
                    <ContextMenu.Item className="entry-context-item danger" onSelect={() => onMutate("dictionary_delete", { id: entry.id })}>删除</ContextMenu.Item>
                  </>}
                </ContextMenu.Content>
              </ContextMenu.Portal>
            </ContextMenu.Root>
          ))}
        </div>
      ))}
      <Dialog.Root open={editing !== null} onOpenChange={(open) => { if (!open) setEditing(null); }}>
        <Dialog.Content className="dictionary-dialog" aria-label="编辑词条">
          <Dialog.Title>编辑词条</Dialog.Title>
          <TextField.Root className="dictionary-edit-input" value={editedTerm} onChange={(event) => setEditedTerm(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") void saveTerm(); }} aria-label="编辑词条" />
          {mutationError && <div className="error" role="alert">{mutationError}</div>}
          <div className="dictionary-dialog-actions">
            <Button type="button" variant="surface" color="gray" onClick={() => setEditing(null)}>取消</Button>
            <Button type="button" disabled={!editedTerm.trim()} onClick={saveTerm}>保存</Button>
          </div>
        </Dialog.Content>
      </Dialog.Root>
    </div>
  );
}
