export type UsageRecord = {
  id: string;
  createdAt: string;
  mode: string | null;
  appName: string | null;
  language: string | null;
  refineStyle: string | null;
  recognitionModel: string | null;
  textModel: string | null;
  backend: string | null;
  asrProvider: string | null;
  asrInputTokens: number | null;
  asrOutputTokens: number | null;
  asrBilledSeconds: number | null;
  transcriptCharacters: number;
  generatedCharacters: number;
  insertedCharacters: number | null;
  duration: number;
  processingDuration: number | null;
  transcriptionDuration: number | null;
  modelDuration: number | null;
  insertionDuration: number | null;
  hasError: boolean;
  promptTokens: number | null;
  completionTokens: number | null;
};

export type StatisticsSnapshot = {
  records: UsageRecord[];
  dictionaryTerms: number;
  learnedTerms: number;
};
export type Metric = "count" | "characters" | "duration";
export type ActivityDay = {
  date: string;
  count: number;
  characters: number;
  duration: number;
};

export function dayKey(date: Date): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}

export function shiftDay(date: Date, amount: number): Date {
  const next = new Date(date);
  next.setHours(0, 0, 0, 0);
  next.setDate(next.getDate() + amount);
  return next;
}

export function activityDays(records: UsageRecord[]): Map<string, ActivityDay> {
  const days = new Map<string, ActivityDay>();
  for (const record of records) {
    const key = dayKey(new Date(record.createdAt));
    const day = days.get(key) ?? {
      date: key,
      count: 0,
      characters: 0,
      duration: 0,
    };
    day.count += 1;
    day.characters += record.generatedCharacters;
    day.duration += record.duration;
    days.set(key, day);
  }
  return days;
}

export function summarize(records: UsageRecord[], now = new Date()) {
  const days = activityDays(records);
  const active = [...days.keys()].sort();
  let longestStreak = 0;
  let streak = 0;
  let previous = "";
  for (const key of active) {
    const preceding = dayKey(shiftDay(new Date(`${key}T12:00:00`), -1));
    streak = previous === preceding ? streak + 1 : 1;
    longestStreak = Math.max(longestStreak, streak);
    previous = key;
  }
  let cursor = shiftDay(now, days.has(dayKey(now)) ? 0 : -1);
  let currentStreak = 0;
  while (days.has(dayKey(cursor))) {
    currentStreak += 1;
    cursor = shiftDay(cursor, -1);
  }
  const duration = records.reduce((sum, record) => sum + record.duration, 0);
  const dictation = records.filter(
    (record) =>
      record.mode === "dictate" &&
      record.transcriptCharacters > 0 &&
      record.duration > 0,
  );
  const dictationSeconds = dictation.reduce(
    (sum, record) => sum + record.duration,
    0,
  );
  return {
    count: records.length,
    characters: records.reduce(
      (sum, record) => sum + record.generatedCharacters,
      0,
    ),
    insertedCharacters: records.reduce(
      (sum, record) => sum + (record.insertedCharacters ?? 0),
      0,
    ),
    inserted: records.filter((record) => record.insertedCharacters !== null)
      .length,
    errors: records.filter((record) => record.hasError).length,
    duration,
    averageDuration: records.length ? duration / records.length : null,
    longestDuration: records.length
      ? records.reduce((max, record) => Math.max(max, record.duration), 0)
      : null,
    activeDays: days.size,
    currentStreak,
    longestStreak,
    peakDay: [...days.values()].reduce(
      (max, day) => Math.max(max, day.characters),
      0,
    ),
    speed: dictationSeconds
      ? (dictation.reduce(
          (sum, record) => sum + record.transcriptCharacters,
          0,
        ) /
          dictationSeconds) *
        60
      : null,
  };
}

export function recordsInRange(
  records: UsageRecord[],
  range: string,
  now = new Date(),
): UsageRecord[] {
  const start =
    range === "all" ? -Infinity : shiftDay(now, 1 - Number(range)).getTime();
  return records.filter((record) => {
    const time = new Date(record.createdAt).getTime();
    return time >= start && time <= now.getTime();
  });
}

export function trendSeries(
  records: UsageRecord[],
  range: string,
  grouping: string,
  now = new Date(),
): ActivityDay[] {
  const selected = recordsInRange(records, range, now);
  const days = activityDays(selected);
  const first = [...days.keys()].sort()[0];
  const start =
    range === "all"
      ? first
        ? new Date(`${first}T00:00:00`)
        : shiftDay(now, -29)
      : shiftDay(now, 1 - Number(range));
  const series = new Map<string, ActivityDay>();
  const total = { count: 0, characters: 0, duration: 0 };
  for (let date = start; date <= now; date = shiftDay(date, 1)) {
    const key = dayKey(date);
    const day = days.get(key) ?? {
      date: key,
      count: 0,
      characters: 0,
      duration: 0,
    };
    const bucket =
      grouping === "weekly"
        ? dayKey(shiftDay(date, -((date.getDay() + 6) % 7)))
        : key;
    const value = series.get(bucket) ?? {
      date: bucket,
      count: 0,
      characters: 0,
      duration: 0,
    };
    for (const metric of ["count", "characters", "duration"] as const) {
      total[metric] += day[metric];
      value[metric] =
        grouping === "cumulative" ? total[metric] : value[metric] + day[metric];
    }
    series.set(bucket, value);
  }
  return [...series.values()];
}

export function distribution(
  records: UsageRecord[],
  field:
    | "mode"
    | "appName"
    | "language"
    | "refineStyle"
    | "recognitionModel"
    | "textModel"
    | "backend"
    | "asrProvider",
) {
  const counts = new Map<string, number>();
  for (const record of records) {
    const name = record[field] ?? "未记录";
    counts.set(name, (counts.get(name) ?? 0) + 1);
  }
  return [...counts]
    .map(([name, count]) => ({ name, count }))
    .sort((a, b) => b.count - a.count || a.name.localeCompare(b.name));
}

export function averageTiming(
  records: UsageRecord[],
  field:
    | "processingDuration"
    | "transcriptionDuration"
    | "modelDuration"
    | "insertionDuration",
): number | null {
  const values = records
    .map((record) => record[field])
    .filter(
      (value): value is number =>
        value !== null && (field !== "modelDuration" || value > 0),
    );
  return values.length
    ? values.reduce((sum, value) => sum + value, 0) / values.length
    : null;
}
