export type VoiceMode = "dictate" | "translate" | "ask";
export type VoiceStage = "idle" | "recording" | "transcribing" | "refining" | "injecting" | "failed";

export type HistoryEntry = {
  id: string;
  sessionId?: string;
  backend?: string;
  asrProvider?: string | null;
  recognitionModel?: string | null;
  createdAt: string;
  transcript: string;
  refinedText: string;
  formattedText?: string;
  ruleCorrectedText?: string;
  appliedRules?: { id: string; from: string; to: string; count: number }[];
  userCorrectedText?: string;
  insertedText: string | null;
  duration: number;
  processingDuration?: number | null;
  transcriptionDuration?: number | null;
  encodingFinalizationDuration?: number | null;
  modelDuration?: number | null;
  insertionDuration?: number | null;
  mode: string | null;
  appName: string | null;
  language: string | null;
  error: string | null;
  hasAudio?: boolean;
};

export type DictionaryEntry = { id: string; term: string; source: string };
export type ReplacementRule = { id: string; from: string; to: string; enabled: boolean; source: string };
export type Snapshot = {
  replacementRules: ReplacementRule[];
  history: HistoryEntry[];
  dictionary: DictionaryEntry[];
};

export type SettingsSnapshot = {
  values: Record<string, unknown>;
  refineAPIKeySet: boolean;
  cloudASRAPIKeySet: boolean;
  platform: string;
};

export type MacPermissions = {
  microphone: "authorized" | "notDetermined" | "denied";
  accessibility: boolean;
  inputMonitoring: boolean;
};
