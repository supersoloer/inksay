import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import HistoryView from "./HistoryView";
import DictionaryView from "./DictionaryView";
import type { Snapshot } from "./types";

export default function PageWindow({ page }: { page: "history" | "dictionary" }) {
  const isMac = navigator.platform.startsWith("Mac");
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    const refresh = () => invoke<Snapshot>("load_snapshot").then(setSnapshot).catch((reason) => setError(String(reason)));
    void refresh();
    const listener = listen(page === "history" ? "history-changed" : "dictionary-changed", refresh);
    return () => { listener.then((remove) => remove()); };
  }, [page]);

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
      <main className="content">
        {error && <div className="error" role="alert">{error}</div>}
        {page === "history" ? (
          <HistoryView
            mutationError={error}
            onMutate={mutate}
            entries={snapshot?.history ?? []}
            onDelete={(id) => { void mutate("history_delete", { id }); }}
            onClear={() => { void mutate("history_clear"); }}
          />
        ) : (
          <DictionaryView mutationError={error} rules={snapshot?.replacementRules ?? []} entries={snapshot?.dictionary ?? []} onMutate={mutate} />
        )}
      </main>
    </div>
  );
}
