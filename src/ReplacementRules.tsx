import { useState } from "react";
import { Button, Dialog, Switch, TextField } from "@radix-ui/themes";
import { ArrowRight } from "lucide-react";
import type { ReplacementRule } from "./types";

type Props = {
  mutationError: string;
  rules: ReplacementRule[];
  pending: boolean;
  query: string;
  onMutate: (command: string, args?: Record<string, unknown>) => Promise<boolean>;
};

export default function ReplacementRules({ rules, pending, query, onMutate, mutationError }: Props) {
  const [editing, setEditing] = useState<Partial<ReplacementRule> | null>(null);
  const [saving, setSaving] = useState(false);
  const search = query.trim().toLocaleLowerCase();
  const filtered = rules.filter(r => (r.source === "suggested") === pending && `${r.from} ${r.to}`.toLocaleLowerCase().includes(search));
  async function save() {
    if (!editing || saving) return;
    setSaving(true);
    if (await onMutate("rule_save", { id: editing.id ?? null, from: editing.from, to: editing.to, enabled: editing.enabled ?? true })) setEditing(null);
    setSaving(false);
  }
  return <>
    {!pending && <div className="dictionary-toolbar"><Button onClick={() => setEditing({ from: "", to: "", enabled: true })}>添加规则</Button></div>}
    <div className="dictionary-list">
      {filtered.map(rule => <div className={`surface dictionary-row replacement-row ${!rule.enabled && !pending ? "rule-disabled" : ""}`} key={rule.id}>
        <div className="replacement-pair"><span>{rule.from}</span><ArrowRight size={16} aria-hidden="true" /><span>{rule.to}</span></div>
        <div className="dictionary-actions">
          {!pending && <Switch checked={rule.enabled} aria-label={`启用 ${rule.from} 替换规则`} onCheckedChange={enabled => { void onMutate("rule_save", { id: rule.id, from: rule.from, to: rule.to, enabled }); }} />}
          {pending && <Button variant="ghost" onClick={() => onMutate("rule_save", { id: rule.id, from: rule.from, to: rule.to, enabled: true })}>接受</Button>}
          <Button variant="ghost" color="gray" onClick={() => setEditing({ ...rule, enabled: pending || rule.enabled })}>编辑</Button>
          <Button variant="ghost" color="gray" onClick={() => onMutate("rule_delete", { id: rule.id })}>{pending ? "忽略" : "删除"}</Button>
        </div>
      </div>)}
    </div>
    <Dialog.Root open={editing !== null} onOpenChange={open => { if (!open && !saving) setEditing(null); }}>
      <Dialog.Content className="dictionary-dialog" aria-describedby={undefined}>
        <Dialog.Title>{editing?.id ? "编辑规则" : "添加规则"}</Dialog.Title>
        <div className="rule-fields">
          <label>原词<TextField.Root value={editing?.from ?? ""} onChange={e => setEditing(current => ({ ...current, from: e.target.value }))} maxLength={80} /></label>
          <label>替换为<TextField.Root value={editing?.to ?? ""} onChange={e => setEditing(current => ({ ...current, to: e.target.value }))} maxLength={80} /></label>
        </div>
        {mutationError && <div className="error" role="alert">{mutationError}</div>}
          <div className="dictionary-dialog-actions">
          <Button variant="surface" color="gray" disabled={saving} onClick={() => setEditing(null)}>取消</Button>
          <Button disabled={saving || !editing?.from?.trim() || !editing?.to?.trim() || editing.from.trim() === editing.to.trim()} onClick={save}>保存</Button>
        </div>
      </Dialog.Content>
    </Dialog.Root>
  </>;
}
