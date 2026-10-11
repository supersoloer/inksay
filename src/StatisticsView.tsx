import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  Select,
  SegmentedControl,
  Progress,
  Tooltip as RadixTooltip,
} from "@radix-ui/themes";
import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  CartesianGrid,
  Cell,
  Pie,
  PieChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import {
  activityDays,
  averageTiming,
  dayKey,
  distribution,
  recordsInRange,
  shiftDay,
  summarize,
  trendSeries,
  type Metric,
  type StatisticsSnapshot,
  type UsageRecord,
} from "./statistics";
import "./StatisticsView.css";

const number = new Intl.NumberFormat("zh-CN", { maximumFractionDigits: 1 });
const compact = new Intl.NumberFormat("zh-CN", {
  notation: "compact",
  maximumFractionDigits: 1,
});
const modeNames: Record<string, string> = {
  dictate: "听写",
  translate: "翻译",
  ask: "询问",
};
const styleNames: Record<string, string> = {
  off: "不处理",
  clean: "去口语",
  polish: "通顺润色",
  formal: "转为书面语",
  concise: "精简",
  custom: "自定义",
};
const languageNames: Record<string, string> = {
  zh: "中文",
  "zh-CN": "简体中文",
  "zh-TW": "繁体中文",
  en: "英语",
  "en-US": "英语",
  ja: "日语",
  ko: "韩语",
  fr: "法语",
  de: "德语",
  es: "西班牙语",
};
const modelNames: Record<string, string> = {
  tiny: "Tiny",
  base: "Base",
  small: "Small",
  "large-v3-v20240930_626MB": "Large v3 Turbo",
};
const colors = ["#4e83d9", "#66a995", "#a17bc3", "#d6a05d", "#8795ab"];
const metricNames = { count: "次数", characters: "字符", duration: "时长" };

function duration(seconds: number | null) {
  if (seconds === null) return "—";
  if (seconds >= 3600) return `${number.format(seconds / 3600)} 小时`;
  if (seconds >= 60) return `${number.format(seconds / 60)} 分钟`;
  return `${number.format(seconds)} 秒`;
}
function metricLabel(value: number, metric: Metric) {
  return metric === "duration"
    ? duration(value)
    : `${number.format(value)} ${metric === "count" ? "次" : "字符"}`;
}

function ActivityHeatmap({
  records,
  now,
}: {
  records: UsageRecord[];
  now: Date;
}) {
  const [year, setYear] = useState(now.getFullYear());
  const [focusedDay, setFocusedDay] = useState(dayKey(now));
  const [metric, setMetric] = useState<Metric>("count");
  const days = useMemo(() => activityDays(records), [records]);
  const years = [
    ...new Set([
      now.getFullYear(),
      ...records.map((record) => new Date(record.createdAt).getFullYear()),
    ]),
  ].sort((a, b) => b - a);
  const start = new Date(year, 0, 1);
  const gridStart = shiftDay(start, -((start.getDay() + 6) % 7));
  const end = new Date(year, 11, 31);
  const weeks = Math.ceil(
    (Math.round((Date.UTC(year, 11, 31) - Date.UTC(year, 0, 1)) / 86400000) +
      1 +
      ((start.getDay() + 6) % 7)) /
      7,
  );
  const max = Math.max(
    1,
    ...[...days.values()]
      .filter((day) => day.date.startsWith(String(year)))
      .map((day) => day[metric]),
  );
  const months = Array.from({ length: 12 }, (_, month) => ({
    month,
    week: Math.floor(
      (Math.round(
        (Date.UTC(year, month, 1) - Date.UTC(year, 0, 1)) / 86400000,
      ) +
        ((start.getDay() + 6) % 7)) /
        7,
    ),
  }));
  return (
    <section className="stats-activity" aria-label="年度活跃">
      <div className="stats-section-heading">
        <h2>年度活跃</h2>
        <div className="stats-controls">
          <SegmentedControl.Root
            size="1"
            value={metric}
            onValueChange={(value) => setMetric(value as Metric)}
            aria-label="活跃指标"
          >
            {Object.entries(metricNames).map(([value, label]) => (
              <SegmentedControl.Item key={value} value={value}>
                {label}
              </SegmentedControl.Item>
            ))}
          </SegmentedControl.Root>
          <Select.Root
            value={String(year)}
            onValueChange={(value) => {
              setYear(Number(value));
              setFocusedDay(`${value}-01-01`);
            }}
          >
            <Select.Trigger aria-label="统计年份" variant="surface" />
            <Select.Content>
              {years.map((value) => (
                <Select.Item key={value} value={String(value)}>
                  {value} 年
                </Select.Item>
              ))}
            </Select.Content>
          </Select.Root>
        </div>
      </div>
      <div className="stats-heatmap-scroll">
        <div
          className="stats-heatmap"
          style={{ gridTemplateColumns: `repeat(${weeks}, minmax(8px, 1fr))` }}
          onKeyDown={(event) => {
            const steps: Record<string, number> = {
              ArrowLeft: -7,
              ArrowRight: 7,
              ArrowUp: -1,
              ArrowDown: 1,
            };
            const step = steps[event.key];
            if (step === undefined) return;
            const cells = [
              ...event.currentTarget.querySelectorAll<HTMLButtonElement>(
                "button",
              ),
            ];
            const index = cells.indexOf(event.target as HTMLButtonElement);
            if (index < 0) return;
            event.preventDefault();
            cells[
              Math.max(0, Math.min(cells.length - 1, index + step))
            ]?.focus();
          }}
        >
          {Array.from({ length: weeks * 7 }, (_, index) => {
            const date = shiftDay(gridStart, index);
            const key = dayKey(date);
            const outside = date < start || date > end || date > now;
            const value = days.get(key)?.[metric] ?? 0;
            const level =
              value === 0 ? 0 : Math.max(1, Math.ceil((value / max) * 4));
            return outside ? (
              <span key={key} className="stats-heat-cell outside" />
            ) : (
              <RadixTooltip
                key={key}
                content={`${key} · ${metricLabel(value, metric)}`}
              >
                <button
                  type="button"
                  tabIndex={focusedDay === key ? 0 : -1}
                  onFocus={() => setFocusedDay(key)}
                  className={`stats-heat-cell level-${level}`}
                  aria-label={`${key}，${metricLabel(value, metric)}`}
                />
              </RadixTooltip>
            );
          })}
        </div>
        <div
          className="stats-months"
          style={{ gridTemplateColumns: `repeat(${weeks}, minmax(8px, 1fr))` }}
        >
          {months.map(({ month, week }) => (
            <span key={month} style={{ gridColumn: `${week + 1} / span 3` }}>
              {month + 1}月
            </span>
          ))}
        </div>
      </div>
      <div className="stats-heat-legend">
        <span>少</span>
        {[0, 1, 2, 3, 4].map((level) => (
          <i key={level} className={`stats-heat-cell level-${level}`} />
        ))}
        <span>多</span>
      </div>
    </section>
  );
}

export default function StatisticsView() {
  const [snapshot, setSnapshot] = useState<StatisticsSnapshot | null>(null);
  const [error, setError] = useState("");
  const [range, setRange] = useState("30");
  const [grouping, setGrouping] = useState("daily");
  const [metric, setMetric] = useState<Metric>("characters");
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    let active = true;
    const refresh = () => {
      setNow(new Date());
      invoke<StatisticsSnapshot>("statistics_get")
        .then((value) => {
          if (active) {
            setSnapshot(value);
            setError("");
          }
        })
        .catch((reason) => {
          if (active) setError(String(reason));
        });
    };
    refresh();
    const listeners = [
      "voice-complete",
      "voice-error",
      "history-changed",
      "dictionary-changed",
    ].map((event) => listen(event, refresh));
    window.addEventListener("focus", refresh);
    const timer = window.setInterval(() => setNow(new Date()), 60_000);
    return () => {
      active = false;
      window.clearInterval(timer);
      window.removeEventListener("focus", refresh);
      listeners.forEach((listener) => listener.then((remove) => remove()));
    };
  }, []);
  const records = snapshot?.records ?? [];
  const total = useMemo(() => summarize(records, now), [records, now]);
  const selected = useMemo(
    () => recordsInRange(records, range, now),
    [records, range, now],
  );
  const summary = useMemo(() => summarize(selected, now), [selected, now]);
  const trend = useMemo(
    () => trendSeries(records, range, grouping, now),
    [records, range, grouping, now],
  );
  const modes = distribution(selected, "mode").map((row) => ({
    ...row,
    color:
      colors[
        ({ dictate: 0, translate: 1, ask: 2 } as Record<string, number>)[
          row.name
        ] ?? 4
      ],
    name: modeNames[row.name] ?? row.name,
  }));
  const apps = distribution(selected, "appName").slice(0, 6);
  const hours = Array.from({ length: 24 }, (_, hour) => ({
    hour: `${String(hour).padStart(2, "0")}:00`,
    count: 0,
  }));
  selected.forEach((record) => {
    hours[new Date(record.createdAt).getHours()].count += 1;
  });
  const timings = (
    [
      ["语音识别", "transcriptionDuration"],
      ["文本处理", "modelDuration"],
      ["跨应用写入", "insertionDuration"],
    ] as const
  ).map(([name, field]) => ({ name, seconds: averageTiming(selected, field) }));
  return (
    <div className="statistics-view" aria-busy={!snapshot && !error}>
      <header className="page-heading">
        <h1>使用统计</h1>
      </header>
      {error && (
        <div className="error" role="alert">
          {error}
        </div>
      )}
      {!snapshot ? (
        <div
          className="stats-loading"
          role="progressbar"
          aria-label="加载统计"
        />
      ) : (
        <>
          <div className="stats-overview">
            {[
              [number.format(total.characters), "累计生成字符"],
              [duration(total.duration), "累计录音时长"],
              [number.format(total.count), "累计处理记录"],
              [`${total.currentStreak} 天`, "当前连续使用"],
              [`${total.longestStreak} 天`, "最长连续使用"],
            ].map(([value, label]) => (
              <div key={label}>
                <strong>{value}</strong>
                <span>{label}</span>
              </div>
            ))}
          </div>
          <ActivityHeatmap records={records} now={now} />
          <div className="stats-section-heading stats-period-heading">
            <h2>使用分析</h2>
            <Select.Root value={range} onValueChange={setRange}>
              <Select.Trigger aria-label="分析时间范围" variant="surface" />
              <Select.Content>
                <Select.Item value="7">近 7 天</Select.Item>
                <Select.Item value="30">近 30 天</Select.Item>
                <Select.Item value="90">近 90 天</Select.Item>
                <Select.Item value="all">全部记录</Select.Item>
              </Select.Content>
            </Select.Root>
          </div>
          <section className="stats-trend" aria-label="使用趋势">
            <div className="stats-section-heading">
              <Select.Root
                value={metric}
                onValueChange={(value) => setMetric(value as Metric)}
              >
                <Select.Trigger aria-label="趋势指标" variant="surface" />
                <Select.Content>
                  <Select.Item value="characters">生成字符</Select.Item>
                  <Select.Item value="count">处理次数</Select.Item>
                  <Select.Item value="duration">录音时长</Select.Item>
                </Select.Content>
              </Select.Root>
              <SegmentedControl.Root
                size="1"
                value={grouping}
                onValueChange={setGrouping}
                aria-label="趋势汇总方式"
              >
                <SegmentedControl.Item value="daily">
                  每日
                </SegmentedControl.Item>
                <SegmentedControl.Item value="weekly">
                  每周
                </SegmentedControl.Item>
                <SegmentedControl.Item value="cumulative">
                  累计
                </SegmentedControl.Item>
              </SegmentedControl.Root>
            </div>
            <div className="stats-chart stats-trend-chart">
              <ResponsiveContainer width="100%" height="100%">
                <AreaChart
                  data={trend}
                  margin={{ top: 12, right: 12, left: 0, bottom: 0 }}
                >
                  <CartesianGrid
                    vertical={false}
                    stroke="var(--stats-grid)"
                    strokeDasharray="3 4"
                  />
                  <XAxis
                    dataKey="date"
                    tickFormatter={(value: string) =>
                      value.slice(5).replace("-", "/")
                    }
                    minTickGap={42}
                    tickLine={false}
                    axisLine={false}
                    tick={{ fill: "var(--stats-muted)", fontSize: 11 }}
                  />
                  <YAxis
                    width={48}
                    tickFormatter={(value: number) =>
                      metric === "duration"
                        ? value >= 60
                          ? `${number.format(value / 60)}分`
                          : `${number.format(value)}秒`
                        : compact.format(value)
                    }
                    allowDecimals={metric === "duration"}
                    tickLine={false}
                    axisLine={false}
                    tick={{ fill: "var(--stats-muted)", fontSize: 11 }}
                  />
                  <Tooltip
                    formatter={(value) => [
                      metricLabel(Number(value), metric),
                      metricNames[metric],
                    ]}
                    contentStyle={tooltipStyle}
                  />
                  <Area
                    type="monotone"
                    dataKey={metric}
                    stroke={colors[0]}
                    fill={colors[0]}
                    fillOpacity={0.13}
                    strokeWidth={2}
                    isAnimationActive={false}
                  />
                </AreaChart>
              </ResponsiveContainer>
            </div>
          </section>
          <div className="stats-two-columns">
            <section className="stats-section">
              <h2>使用洞察</h2>
              <dl className="stats-facts">
                {[
                  ["活跃天数", `${summary.activeDays} 天`],
                  ["已写入记录", `${summary.inserted} / ${summary.count}`],
                  ["写入字符", number.format(summary.insertedCharacters)],
                  ["单日最多生成字符", number.format(summary.peakDay)],
                  [
                    "听写速度",
                    summary.speed === null
                      ? "—"
                      : `${number.format(summary.speed)} 字符/分`,
                  ],
                  ["平均录音", duration(summary.averageDuration)],
                  ["最长录音", duration(summary.longestDuration)],
                  [
                    "平均处理耗时",
                    duration(averageTiming(selected, "processingDuration")),
                  ],
                  ["含异常记录", number.format(summary.errors)],
                ].map(([label, value]) => (
                  <div key={label}>
                    <dt>{label}</dt>
                    <dd>{value}</dd>
                  </div>
                ))}
              </dl>
            </section>
            <section className="stats-section">
              <h2>模式分布</h2>
              <div className="stats-mode-chart">
                <div className="stats-donut">
                  <ResponsiveContainer width="100%" height="100%">
                    <PieChart>
                      <Pie
                        data={modes}
                        dataKey="count"
                        nameKey="name"
                        innerRadius={52}
                        outerRadius={72}
                        paddingAngle={modes.length > 1 ? 3 : 0}
                        stroke="none"
                        isAnimationActive={false}
                      >
                        {modes.map((row) => (
                          <Cell key={row.name} fill={row.color} />
                        ))}
                      </Pie>
                      <Tooltip
                        contentStyle={tooltipStyle}
                        formatter={(value) =>
                          `${number.format(Number(value))} 次`
                        }
                      />
                    </PieChart>
                  </ResponsiveContainer>
                  <div className="stats-donut-label">
                    <strong>{number.format(summary.count)}</strong>
                    <span>处理记录</span>
                  </div>
                </div>
                <ul className="stats-legend">
                  {modes.map((row) => (
                    <li key={row.name}>
                      <i style={{ background: row.color }} />
                      <span>{row.name}</span>
                      <strong>
                        {number.format((row.count / summary.count) * 100)}%
                      </strong>
                    </li>
                  ))}
                </ul>
              </div>
            </section>
            <section className="stats-section">
              <h2>常用应用</h2>
              <div className="stats-chart stats-app-chart">
                <ResponsiveContainer width="100%" height="100%">
                  <BarChart
                    data={apps}
                    layout="vertical"
                    margin={{ left: 0, right: 32, top: 8, bottom: 0 }}
                  >
                    <XAxis type="number" hide />
                    <YAxis
                      dataKey="name"
                      type="category"
                      width={112}
                      axisLine={false}
                      tickLine={false}
                      tick={{ fill: "var(--stats-text)", fontSize: 11 }}
                      tickFormatter={(value: string) =>
                        value.length > 16 ? `${value.slice(0, 15)}…` : value
                      }
                    />
                    <Tooltip
                      contentStyle={tooltipStyle}
                      cursor={{ fill: "var(--stats-grid)" }}
                      formatter={(value) => [
                        `${number.format(Number(value))} 次`,
                        "使用次数",
                      ]}
                    />
                    <Bar
                      dataKey="count"
                      fill={colors[0]}
                      radius={[0, 4, 4, 0]}
                      maxBarSize={18}
                      label={{
                        position: "right",
                        fill: "var(--stats-muted)",
                        fontSize: 10,
                      }}
                      isAnimationActive={false}
                    />
                  </BarChart>
                </ResponsiveContainer>
              </div>
            </section>
            <section className="stats-section">
              <h2>使用时段</h2>
              <div className="stats-chart stats-app-chart">
                <ResponsiveContainer width="100%" height="100%">
                  <BarChart
                    data={hours}
                    margin={{ left: -20, right: 0, top: 8, bottom: 0 }}
                  >
                    <CartesianGrid
                      vertical={false}
                      stroke="var(--stats-grid)"
                    />
                    <XAxis
                      dataKey="hour"
                      ticks={["00:00", "06:00", "12:00", "18:00", "23:00"]}
                      tick={{ fill: "var(--stats-muted)", fontSize: 10 }}
                      tickLine={false}
                      axisLine={false}
                    />
                    <YAxis
                      allowDecimals={false}
                      tick={{ fill: "var(--stats-muted)", fontSize: 10 }}
                      tickLine={false}
                      axisLine={false}
                    />
                    <Tooltip
                      contentStyle={tooltipStyle}
                      cursor={{ fill: "var(--stats-grid)" }}
                      formatter={(value) => [
                        `${number.format(Number(value))} 次`,
                        "使用次数",
                      ]}
                    />
                    <Bar
                      dataKey="count"
                      fill={colors[1]}
                      radius={[3, 3, 0, 0]}
                      isAnimationActive={false}
                    />
                  </BarChart>
                </ResponsiveContainer>
              </div>
            </section>
            <section className="stats-section">
              <h2>处理耗时</h2>
              <dl className="stats-timings">
                {timings.map((row) => (
                  <div key={row.name}>
                    <dt>{row.name}</dt>
                    <dd>{duration(row.seconds)}</dd>
                    <Progress
                      size="1"
                      value={
                        ((row.seconds ?? 0) /
                          Math.max(
                            1,
                            ...timings.map((item) => item.seconds ?? 0),
                          )) *
                        100
                      }
                      aria-label={`${row.name}平均耗时`}
                    />
                  </div>
                ))}
              </dl>
            </section>
            <section className="stats-section">
              <h2>当前词典</h2>
              <dl className="stats-facts">
                <div>
                  <dt>词条总数</dt>
                  <dd>{number.format(snapshot.dictionaryTerms)}</dd>
                </div>
                <div>
                  <dt>已学习词条</dt>
                  <dd>{number.format(snapshot.learnedTerms)}</dd>
                </div>
              </dl>
            </section>
            {(
              [
                ["language", "识别语言"],
                ["refineStyle", "整理风格"],
                ["recognitionModel", "识别模型"],
                ["textModel", "文本模型"],
              ] as const
            ).map(([field, title]) => (
              <section className="stats-section" key={field}>
                <h2>{title}</h2>
                <dl className="stats-facts">
                  {distribution(
                    field === "refineStyle"
                      ? selected.filter((record) => record.mode === "dictate")
                      : selected,
                    field,
                  )
                    .slice(0, 5)
                    .map((row) => (
                      <div key={row.name}>
                        <dt>
                          {field === "refineStyle"
                            ? (styleNames[row.name] ?? row.name)
                            : field === "language"
                              ? (languageNames[row.name] ?? row.name)
                              : field === "recognitionModel"
                                ? (modelNames[row.name] ?? row.name)
                                : row.name}
                        </dt>
                        <dd>{number.format(row.count)} 次</dd>
                      </div>
                    ))}
                </dl>
              </section>
            ))}
            <section className="stats-section">
              <h2>识别方式</h2>
              <dl className="stats-facts">
                {distribution(selected, "backend").map((row) => (
                  <div key={row.name}>
                    <dt>
                      {row.name === "Whisper"
                        ? "本地 Whisper"
                        : row.name === "Cloud"
                          ? "云端识别"
                          : row.name}
                    </dt>
                    <dd>{number.format(row.count)} 次</dd>
                  </div>
                ))}
              </dl>
            </section>
            <section className="stats-section">
              <h2>云端识别用量</h2>
              <dl className="stats-facts">
                {(
                  [
                    ["asrInputTokens", "已记录输入 Token"],
                    ["asrOutputTokens", "已记录输出 Token"],
                    ["asrBilledSeconds", "已记录计费时长"],
                  ] as const
                ).map(([field, label]) => {
                  const measured = selected.filter(
                    (record) => record[field] != null,
                  );
                  const value = measured.reduce(
                    (sum, record) => sum + record[field]!,
                    0,
                  );
                  return (
                    <div key={field}>
                      <dt>{label}</dt>
                      <dd>
                        {measured.length
                          ? field === "asrBilledSeconds"
                            ? duration(value)
                            : number.format(value)
                          : "—"}
                      </dd>
                    </div>
                  );
                })}
              </dl>
            </section>
            <section className="stats-section">
              <h2>文本模型用量</h2>
              <dl className="stats-facts">
                {(
                  [
                    ["promptTokens", "已记录输入 Token"],
                    ["completionTokens", "已记录输出 Token"],
                  ] as const
                ).map(([field, label]) => {
                  const measured = selected.filter(
                    (record) => record[field] !== null,
                  );
                  return (
                    <div key={field}>
                      <dt>{label}</dt>
                      <dd>
                        {measured.length
                          ? number.format(
                              measured.reduce(
                                (sum, record) => sum + record[field]!,
                                0,
                              ),
                            )
                          : "—"}
                      </dd>
                    </div>
                  );
                })}
                <div>
                  <dt>有用量记录的调用</dt>
                  <dd>
                    {
                      selected.filter(
                        (record) =>
                          record.promptTokens !== null ||
                          record.completionTokens !== null,
                      ).length
                    }
                  </dd>
                </div>
              </dl>
            </section>
          </div>
        </>
      )}
    </div>
  );
}

const tooltipStyle = {
  background: "var(--stats-surface)",
  border: "1px solid var(--stats-grid)",
  borderRadius: 8,
  color: "var(--stats-text)",
  fontSize: 12,
};
