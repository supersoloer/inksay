import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { TriangleAlert } from "lucide-react";

type FloatingState = { stage: string; mode: string };

const modeName: Record<string, string> = { dictate: "听写", translate: "翻译", ask: "询问" };
const stageName: Record<string, string> = {
  transcribing: "识别中",
  refining: "整理中",
  injecting: "写入中",
  failed: "出错了",
};
const stageDetail: Record<string, string> = {
  recording: "松开结束 · Esc 取消",
  transcribing: "语音已录制",
  refining: "正在优化表达",
  injecting: "即将完成",
};

export default function FloatingBarWindow() {
  const [state, setState] = useState<FloatingState>({ stage: "idle", mode: "dictate" });
  const [level, setLevel] = useState(0);
  const [downloadProgress, setDownloadProgress] = useState<number | null>(null);

  useEffect(() => {
    invoke<FloatingState>("floating_get").then(setState).catch(console.error);
    const stageListener = listen<FloatingState>("floating-stage", (event) => {
      setState(event.payload);
      if (event.payload.stage === "recording") setDownloadProgress(null);
    });
    const levelListener = listen<number>("voice-level", (event) => setLevel(event.payload));
    const downloadListener = listen<[number, number | null]>("model-download-progress", (event) => {
      const [downloaded, total] = event.payload;
      setDownloadProgress(total ? downloaded / total : null);
    });
    return () => {
      stageListener.then((remove) => remove());
      levelListener.then((remove) => remove());
      downloadListener.then((remove) => remove());
    };
  }, []);

  const recording = state.stage === "recording";
  const processing = state.stage === "transcribing" || state.stage === "refining" || state.stage === "injecting";
  const activeStep = state.stage === "transcribing" ? 0 : state.stage === "refining" ? 1 : 2;
  return (
    <div className="floating-bar" role="status" aria-live="polite">
      <div className={`floating-icon ${state.stage === "failed" ? "error" : ""}`} aria-hidden="true">
        {recording ? (
          <div className="floating-wave">
            {Array.from({ length: 7 }, (_, index) => (
              <i key={index} style={{ height: `${2.5 + 14.5 * Math.min(1, Math.max(0, level)) * (1 - Math.abs(index - 3) / 3 * 0.55)}px` }} />
            ))}
          </div>
        ) : processing ? (
          <div className="floating-processing">{Array.from({ length: 5 }, (_, index) => <i key={index} />)}</div>
        ) : state.stage === "failed" ? <TriangleAlert size={15} /> : null}
      </div>
      <div className="floating-copy">
        <strong>{recording ? `${modeName[state.mode]} · 正在听…` : stageName[state.stage]}</strong>
        <small>{stageDetail[state.stage]}</small>
      </div>
      {downloadProgress !== null && state.stage === "transcribing" && (
        <svg className="floating-download" viewBox="0 0 20 20" role="progressbar" aria-label="模型下载进度" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(downloadProgress * 100)}>
          <circle cx="10" cy="10" r="7" fill="none" stroke="#d8e0ed" strokeWidth="2" />
          <circle cx="10" cy="10" r="7" fill="none" stroke="#2674e4" strokeWidth="2" strokeDasharray={`${downloadProgress * 44} 44`} transform="rotate(-90 10 10)" />
        </svg>
      )}
      {(processing || state.stage === "failed") && (downloadProgress === null || state.stage !== "transcribing") && (
        <div className="floating-steps" aria-hidden="true">
          {[0, 1, 2].map((step) => <i key={step} className={step <= activeStep ? `active ${step === activeStep ? "current" : ""}` : ""} />)}
        </div>
      )}
    </div>
  );
}
