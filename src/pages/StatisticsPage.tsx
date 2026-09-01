import { useMemo } from "react";
import { Button, Input, Select, Tooltip } from "@fluentui/react-components";
import ReactECharts from "echarts-for-react";
import {
  BarChart3,
  FileCheck2,
  FileWarning,
  ListChecks,
  RefreshCw,
  TrendingUp,
} from "lucide-react";
import type {
  AssessmentLevel,
  Category,
  RecordFilter,
  StatisticsResult,
} from "../types";
import { LEVEL_META, currentAcademicYear, formatScore } from "../lib/utils";
import { EmptyState, PageHeading } from "../components/Common";

const iconProps = { size: 18, strokeWidth: 1.8 } as const;

interface ChartClickParams {
  name?: string;
  data?: { key?: string };
}

function StatsKpis({ stats }: { stats: StatisticsResult }) {
  const items = [
    {
      label: "记录总数",
      value: String(stats.summary.recordCount),
      suffix: "条",
      icon: <ListChecks {...iconProps} />,
    },
    {
      label: "累计综测分",
      value: formatScore(stats.summary.totalScore),
      suffix: "分",
      icon: <TrendingUp {...iconProps} />,
    },
    {
      label: "证明材料",
      value: String(stats.summary.materialCount),
      suffix: "份",
      icon: <FileCheck2 {...iconProps} />,
    },
    {
      label: "待补材料",
      value: String(stats.summary.missingMaterialCount),
      suffix: "条",
      icon: <FileWarning {...iconProps} />,
    },
  ];
  return (
    <section className="kpi-strip stats-kpis" aria-label="统计概览">
      {items.map((item) => (
        <div className="kpi-item" key={item.label}>
          <span className="kpi-icon" aria-hidden="true">
            {item.icon}
          </span>
          <span className="kpi-label">{item.label}</span>
          <strong>{item.value}</strong>
          <small>{item.suffix}</small>
        </div>
      ))}
    </section>
  );
}

export function StatisticsPage({
  stats,
  categories,
  academicYears,
  filter,
  loading,
  error,
  onFilterChange,
  onDrilldown,
  onReload,
}: {
  stats: StatisticsResult;
  categories: Category[];
  academicYears: string[];
  filter: RecordFilter;
  loading: boolean;
  error: string;
  onFilterChange: (patch: Partial<RecordFilter>) => void;
  onDrilldown: (patch: Partial<RecordFilter>) => void;
  onReload: () => void;
}) {
  const years = useMemo(() => {
    const current = Number(currentAcademicYear().slice(0, 4));
    const nearby = Array.from({ length: 8 }, (_, index) => {
      const start = current + 2 - index;
      return `${start}-${start + 1}`;
    });
    return [...new Set([...nearby, ...academicYears])].sort((a, b) =>
      b.localeCompare(a),
    );
  }, [academicYears]);

  const levelOption = useMemo(
    () => ({
      animationDuration: 220,
      aria: { enabled: true, decal: { show: false } },
      grid: { left: 44, right: 18, top: 18, bottom: 34 },
      tooltip: {
        trigger: "axis",
        axisPointer: { type: "shadow" },
        formatter: (
          params: Array<{
            name: string;
            value: number;
            data: { score: string };
          }>,
        ) => {
          const item = params[0];
          return `${item.name}<br/>记录 ${item.value} 条<br/>分数 ${formatScore(item.data.score)} 分`;
        },
      },
      xAxis: {
        type: "category",
        axisLine: { lineStyle: { color: "#c8d1cd" } },
        axisTick: { show: false },
        axisLabel: { color: "#596661", fontFamily: "Microsoft YaHei UI" },
        data: stats.byLevel.map((item) => item.label),
      },
      yAxis: {
        type: "value",
        minInterval: 1,
        axisLabel: { color: "#6d7974", fontFamily: "Bahnschrift" },
        splitLine: { lineStyle: { color: "#e7ecea" } },
      },
      series: [
        {
          type: "bar",
          barWidth: 30,
          data: stats.byLevel.map((item) => ({
            name: item.label,
            key: item.key,
            value: item.count,
            score: item.score,
            itemStyle: {
              color: LEVEL_META[item.key as AssessmentLevel].color,
              borderRadius: [3, 3, 0, 0],
            },
          })),
        },
      ],
    }),
    [stats.byLevel],
  );

  const categoryOption = useMemo(
    () => ({
      animationDuration: 220,
      aria: { enabled: true, decal: { show: false } },
      grid: { left: 92, right: 24, top: 12, bottom: 28 },
      tooltip: {
        trigger: "axis",
        axisPointer: { type: "shadow" },
        formatter: (
          params: Array<{
            name: string;
            value: number;
            data: { count: number };
          }>,
        ) => {
          const item = params[0];
          return `${item.name}<br/>分数 ${formatScore(String(item.value))} 分<br/>记录 ${item.data.count} 条`;
        },
      },
      xAxis: {
        type: "value",
        axisLabel: { color: "#6d7974", fontFamily: "Bahnschrift" },
        splitLine: { lineStyle: { color: "#e7ecea" } },
      },
      yAxis: {
        type: "category",
        inverse: true,
        axisLine: { show: false },
        axisTick: { show: false },
        axisLabel: {
          color: "#3f4a46",
          width: 76,
          overflow: "truncate",
          fontFamily: "Microsoft YaHei UI",
        },
        data: stats.byCategory.map((item) => item.label),
      },
      series: [
        {
          type: "bar",
          barWidth: 16,
          itemStyle: { color: "#27745a", borderRadius: [0, 3, 3, 0] },
          data: stats.byCategory.map((item) => ({
            name: item.label,
            key: item.key,
            value: Number(item.score),
            count: item.count,
          })),
        },
      ],
    }),
    [stats.byCategory],
  );

  const trendOption = useMemo(
    () => ({
      animationDuration: 220,
      aria: { enabled: true, decal: { show: false } },
      grid: { left: 44, right: 26, top: 20, bottom: 36 },
      tooltip: {
        trigger: "axis",
        formatter: (
          params: Array<{ name: string; value: number; seriesName: string }>,
        ) =>
          `${params[0]?.name ?? ""}<br/>${params.map((item) => `${item.seriesName} ${item.value}`).join("<br/>")}`,
      },
      legend: {
        right: 8,
        top: 0,
        textStyle: { color: "#596661", fontFamily: "Microsoft YaHei UI" },
        itemWidth: 14,
        itemHeight: 8,
      },
      xAxis: {
        type: "category",
        boundaryGap: false,
        axisLine: { lineStyle: { color: "#c8d1cd" } },
        axisTick: { show: false },
        axisLabel: { color: "#6d7974", fontFamily: "Bahnschrift" },
        data: stats.monthly.map((item) => item.month.slice(5)),
      },
      yAxis: [
        {
          type: "value",
          axisLabel: { color: "#6d7974", fontFamily: "Bahnschrift" },
          splitLine: { lineStyle: { color: "#e7ecea" } },
        },
        { type: "value", show: false },
      ],
      series: [
        {
          name: "综测分",
          type: "line",
          showSymbol: true,
          symbolSize: 6,
          lineStyle: { width: 2, color: "#176b4d" },
          itemStyle: { color: "#176b4d" },
          data: stats.monthly.map((item) => Number(item.score)),
        },
        {
          name: "记录数",
          type: "line",
          yAxisIndex: 1,
          showSymbol: true,
          symbolSize: 5,
          lineStyle: { width: 1.5, color: "#4f6f83", type: "dashed" },
          itemStyle: { color: "#4f6f83" },
          data: stats.monthly.map((item) => item.count),
        },
      ],
    }),
    [stats.monthly],
  );

  const hasFilters = Boolean(
    filter.query ||
    filter.academicYear !== "all" ||
    filter.dateFrom ||
    filter.dateTo ||
    filter.categoryId !== "all" ||
    filter.level !== "all" ||
    filter.materialStatus !== "all",
  );

  return (
    <div className="page statistics-page">
      <PageHeading
        title="统计分析"
        description="统计结果与记录页使用同一组筛选条件"
      />

      <section className="stats-filter-bar" aria-label="统计筛选">
        <Input
          type="search"
          aria-label="统计搜索记录"
          placeholder="搜索活动、备注或材料"
          value={filter.query}
          onChange={(_, data) => onFilterChange({ query: data.value, page: 1 })}
        />
        <Input
          type="date"
          aria-label="统计开始日期"
          value={filter.dateFrom}
          onChange={(_, data) =>
            onFilterChange({ dateFrom: data.value, page: 1 })
          }
        />
        <Input
          type="date"
          aria-label="统计结束日期"
          value={filter.dateTo}
          onChange={(_, data) =>
            onFilterChange({ dateTo: data.value, page: 1 })
          }
        />
        <label>
          <span>学年</span>
          <Select
            value={filter.academicYear}
            onChange={(_, data) =>
              onFilterChange({ academicYear: data.value, page: 1 })
            }
          >
            <option value="all">全部学年</option>
            {years.map((year) => (
              <option value={year} key={year}>
                {year} 学年
              </option>
            ))}
          </Select>
        </label>
        <label>
          <span>材料</span>
          <Select
            aria-label="统计材料状态"
            value={filter.materialStatus}
            onChange={(_, data) =>
              onFilterChange({
                materialStatus: data.value as RecordFilter["materialStatus"],
                page: 1,
              })
            }
          >
            <option value="all">全部</option>
            <option value="attached">材料齐全</option>
            <option value="missing">待补材料</option>
          </Select>
        </label>
        <label>
          <span>排序</span>
          <Select
            aria-label="统计排序"
            value={filter.sort}
            onChange={(_, data) =>
              onFilterChange({
                sort: data.value as RecordFilter["sort"],
                page: 1,
              })
            }
          >
            <option value="dateDesc">日期从新到旧</option>
            <option value="dateAsc">日期从旧到新</option>
            <option value="scoreDesc">分数从高到低</option>
            <option value="updatedDesc">最近修改</option>
          </Select>
        </label>
        {hasFilters ? (
          <Tooltip content="清除筛选" relationship="label">
            <Button
              appearance="subtle"
              aria-label="清除统计筛选"
              onClick={() =>
                onFilterChange({
                  query: "",
                  academicYear: "all",
                  dateFrom: "",
                  dateTo: "",
                  categoryId: "all",
                  level: "all",
                  materialStatus: "all",
                  sort: "dateDesc",
                  page: 1,
                })
              }
            >
              清除
            </Button>
          </Tooltip>
        ) : null}
        <label>
          <span>活动类别</span>
          <Select
            value={filter.categoryId}
            onChange={(_, data) =>
              onFilterChange({ categoryId: data.value, page: 1 })
            }
          >
            <option value="all">全部类别</option>
            {categories.map((category) => (
              <option value={category.id} key={category.id}>
                {category.name}
              </option>
            ))}
          </Select>
        </label>
        <label>
          <span>综测级别</span>
          <Select
            value={filter.level}
            onChange={(_, data) =>
              onFilterChange({
                level: data.value as RecordFilter["level"],
                page: 1,
              })
            }
          >
            <option value="all">全部级别</option>
            {(
              Object.entries(LEVEL_META) as Array<
                [AssessmentLevel, (typeof LEVEL_META)[AssessmentLevel]]
              >
            ).map(([value, meta]) => (
              <option value={value} key={value}>
                {meta.label}
              </option>
            ))}
          </Select>
        </label>
      </section>

      {loading ? (
        <div className="statistics-loading" aria-label="正在加载统计">
          <div className="skeleton stats-kpi-skeleton" />
          <div className="stats-loading-grid">
            <div className="skeleton chart-skeleton" />
            <div className="skeleton chart-skeleton" />
          </div>
          <div className="skeleton chart-skeleton wide" />
        </div>
      ) : error ? (
        <div className="stats-error-wrap">
          <EmptyState
            icon={<RefreshCw size={28} strokeWidth={1.6} />}
            title="统计数据加载失败"
            description={error}
            action={
              <Button appearance="primary" onClick={onReload}>
                重新加载
              </Button>
            }
          />
        </div>
      ) : (
        <>
          <StatsKpis stats={stats} />
          {stats.summary.recordCount === 0 ? (
            <div className="stats-empty-wrap">
              <EmptyState
                icon={<BarChart3 size={28} strokeWidth={1.6} />}
                title="当前范围没有可统计的记录"
                description="调整学年、类别或级别筛选条件后再查看。"
              />
            </div>
          ) : (
            <div className="charts-layout">
              <section className="chart-panel">
                <header>
                  <h2>各级别记录</h2>
                  <span>点击柱形查看明细</span>
                </header>
                <ReactECharts
                  option={levelOption}
                  style={{ height: 254 }}
                  notMerge
                  onEvents={{
                    click: (params: ChartClickParams) => {
                      const level = stats.byLevel.find(
                        (item) => item.label === params.name,
                      )?.key;
                      if (level)
                        onDrilldown({ level: level as AssessmentLevel });
                    },
                  }}
                />
              </section>
              <section className="chart-panel">
                <header>
                  <h2>各类别分数</h2>
                  <span>按累计分数排序</span>
                </header>
                <ReactECharts
                  option={categoryOption}
                  style={{ height: 254 }}
                  notMerge
                  onEvents={{
                    click: (params: ChartClickParams) => {
                      const id = stats.byCategory.find(
                        (item) => item.label === params.name,
                      )?.key;
                      if (id) onDrilldown({ categoryId: id });
                    },
                  }}
                />
              </section>
              <section className="chart-panel trend-panel">
                <header>
                  <h2>月度趋势</h2>
                  <span>分数与记录数变化</span>
                </header>
                <ReactECharts
                  option={trendOption}
                  style={{ height: 246 }}
                  notMerge
                  onEvents={{
                    click: (params: ChartClickParams) => {
                      const month = stats.monthly.find(
                        (item) =>
                          item.month.slice(5) === params.name ||
                          item.month === params.name,
                      )?.month;
                      if (!month) return;
                      const [year, monthNumber] = month.split("-").map(Number);
                      const lastDay = new Date(year, monthNumber, 0).getDate();
                      onDrilldown({
                        dateFrom: `${month}-01`,
                        dateTo: `${month}-${String(lastDay).padStart(2, "0")}`,
                      });
                    },
                  }}
                />
              </section>
            </div>
          )}
        </>
      )}
    </div>
  );
}
