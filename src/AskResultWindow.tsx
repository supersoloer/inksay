import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Sparkles, X } from "lucide-react";

type AskResult = { sessionId: string; text: string };

export default function AskResultWindow() {
  const [result, setResult] = useState<AskResult | null>(null);
  const text = result?.text ?? "";
  const [copied, setCopied] = useState(false);
  const [writing, setWriting] = useState(false);
  const [error, setError] = useState(false);

  useEffect(() => {
    let active = true;
    let receivedUpdate = false;
    let remove: (() => void) | undefined;
    listen<AskResult>("ask-result-changed", (event) => {
      if (!active) return;
      receivedUpdate = true;
      setResult(event.payload);
      setCopied(false);
      setError(false);
    }).then((unlisten) => {
      if (!active) {
        unlisten();
        return;
      }
      remove = unlisten;
      invoke<AskResult | null>("ask_result_get")
        .then((value) => {
          if (active && !receivedUpdate && value) setResult(value);
        })
        .catch(console.error);
    }).catch(console.error);
    return () => { active = false; remove?.(); };
  }, []);

  useEffect(() => {
    if (!result) return;
    const frame = window.requestAnimationFrame(() => {
      invoke("ask_result_presented", { sessionId: result.sessionId }).catch(console.error);
    });
    return () => window.cancelAnimationFrame(frame);
  }, [result]);

  async function copy() {
    try {
      await invoke("ask_result_copy", { sessionId: result?.sessionId });
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch (reason) {
      console.error(reason);
    }
  }

  async function write() {
    setWriting(true);
    try {
      await invoke("ask_result_write", { sessionId: result?.sessionId });
      setError(false);
    } catch (reason) {
      console.error(reason);
      invoke<AskResult | null>("ask_result_get")
        .then((current) => setResult(current))
        .catch(console.error);
      setError(true);
    } finally {
      setWriting(false);
    }
  }

  return (
    <div className="ask-result-window">
      <header data-tauri-drag-region>
        <span><Sparkles size={16} aria-hidden="true" />询问结果</span>
        <button type="button" aria-label="关闭" onClick={() => invoke("ask_result_dismiss", { sessionId: result?.sessionId ?? null }).catch(console.error)}><X size={16} aria-hidden="true" /></button>
      </header>
      <div className="ask-result-text">{text}</div>
      {error && <span className="ask-result-error">写入失败</span>}
      <footer>
        <button type="button" disabled={!text} onClick={copy}>{copied ? "已复制" : "复制"}</button>
        <button type="button" disabled={!text || writing} aria-busy={writing} onClick={write}>写入</button>
      </footer>
    </div>
  );
}
