import test from "node:test";
process.env.TZ = "Asia/Singapore";
import assert from "node:assert/strict";
import {
  summarize,
  recordsInRange,
  trendSeries,
  averageTiming,
  activityDays,
} from "../src/statistics.ts";

function record(date, extra = {}) {
  return {
    id: date,
    createdAt: date,
    mode: "dictate",
    appName: "Editor",
    language: "zh",
    refineStyle: "clean",
    recognitionModel: null,
    textModel: null,
    transcriptCharacters: 20,
    generatedCharacters: 25,
    insertedCharacters: null,
    duration: 10,
    processingDuration: null,
    transcriptionDuration: null,
    modelDuration: null,
    insertionDuration: null,
    hasError: false,
    promptTokens: null,
    completionTokens: null,
    ...extra,
  };
}

test("calendar streak includes yesterday, breaks gaps and handles year boundaries", () => {
  const records = ["2025-12-30", "2025-12-31", "2026-01-01", "2026-01-04"].map(
    (date) => record(`${date}T12:00:00`),
  );
  const result = summarize(records, new Date("2026-01-05T13:00:00"));
  assert.equal(result.longestStreak, 3);
  assert.equal(result.currentStreak, 1);
  assert.equal(
    summarize(records, new Date("2026-01-06T12:00:00")).currentStreak,
    0,
  );
});
test("empty data stays empty and missing timing never becomes zero", () => {
  assert.equal(summarize([]).count, 0);
  assert.equal(summarize([]).speed, null);
  assert.equal(
    averageTiming([record("2026-01-01T12:00:00")], "processingDuration"),
    null,
  );
  assert.equal(
    averageTiming(
      [record("2026-01-01T12:00:00", { modelDuration: 0 })],
      "modelDuration",
    ),
    null,
  );
});
test("dictation speed excludes answer expansion; averages use measured samples only", () => {
  const records = [
    record("2026-01-01T12:00:00", { processingDuration: 2 }),
    record("2026-01-02T12:00:00", { mode: "ask", generatedCharacters: 1000 }),
  ];
  assert.equal(summarize(records).speed, 120);
  assert.equal(averageTiming(records, "processingDuration"), 2);
});
test("ranges include today and six earlier local days, cumulative fills empty days", () => {
  const now = new Date("2026-01-08T12:00:00");
  const records = [
    record("2026-01-01T23:59:59"),
    record("2026-01-02T00:00:00"),
    record("2026-01-08T10:00:00"),
  ];
  assert.equal(recordsInRange(records, "7", now).length, 2);
  const series = trendSeries(records, "7", "cumulative", now);
  assert.equal(series.length, 7);
  assert.equal(series[0].count, 1);
  assert.equal(series[6].count, 2);
  assert.equal(series[6].characters, 50);
});
test("UTC timestamps use local calendar days and weekly grouping sums once", () => {
  const row = record("2026-01-01T20:00:00Z");
  assert.equal([...activityDays([row]).keys()][0], "2026-01-02");
  const result = trendSeries(
    [row, record("2026-01-05T12:00:00+08:00")],
    "7",
    "weekly",
    new Date("2026-01-08T12:00:00+08:00"),
  );
  assert.equal(
    result.reduce((sum, item) => sum + item.count, 0),
    2,
  );
  assert.equal(result.length, 2);
});
