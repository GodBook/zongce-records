import { useEffect, useMemo, useState, type RefObject } from "react";
import {
  Button,
  Checkbox,
  Input,
  Select,
  Tooltip,
} from "@fluentui/react-components";
import {
  Archive,
  ChevronRight,
  FileArchive,
  FileCheck2,
  FileWarning,
  ListChecks,
  Plus,
  RefreshCw,
  Search,
  SlidersHorizontal,
  Trash2,
  X,
} from "lucide-react";
import type {
  AssessmentLevel,
  AssessmentRecord,
  Category,
  MetricSummary,
  RecordFilter,
} from "../types";
import { api } from "../lib/api";
import {
  LEVEL_META,
  academicYearOptions,
  currentAcademicYear,
  errorMessage,
  formatScore,
} from "../lib/utils";
import {
  ConfirmDialog,
  EmptyState,
  LoadingRows,
  PageHeading,
} from "../components/Common";

const iconProps = { size: 18, strokeWidth: 1.8 } as const;

function KpiStrip({ summary }: { summary: MetricSummary }) {
  const items = [
    {
      label: "记录总数",
      value: String(summary.recordCount),
      suffix: "条",
      icon: <ListChecks {...iconProps} />,
    },
    {
      label: "累计综测分",
      value: formatScore(summary.totalScore),
      suffix: "分",
      icon: <Archive {...iconProps} />,
    },
    {
      label: "证明材料",
      value: String(summary.materialCount),
      suffix: "份",
      icon: <FileCheck2 {...iconProps} />,
    },
    {
      label: "待补材料",
      value: String(summary.missingMaterialCount),
      suffix: "条",
      icon: <FileWarning {...iconProps} />,
    },
  ];
  return (
    <section className="kpi-strip" aria-label="综测记录概览">
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

export function RecordsPage({
  records,
  total,
  categories,
  summary,
  filter,
  loading,
  error,
  searchRef,
  onFilterChange,
  onNew,
  onEdit,
  onReload,
  onDataChanged,
  notify,
  academicYears,
}: {
  records: AssessmentRecord[];
  total: number;
  categories: Category[];
  summary: MetricSummary;
  filter: RecordFilter;
  loading: boolean;
  error: string;
  searchRef: RefObject<HTMLInputElement | null>;
  onFilterChange: (patch: Partial<RecordFilter>) => void;
  onNew: () => void;
  onEdit: (record: AssessmentRecord) => void;
  onReload: () => void;
  onDataChanged: () => void;
  notify: (
    kind: "success" | "error" | "info",
    title: string,
    message?: string,
  ) => void;
  academicYears?: string[];
}) {
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [deleteConfirmOpen, setDeleteConfirmOpen] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [exporting, setExporting] = useState(false);
  // 学年筛选不能依赖当前页；保留数据中出现的学年并补齐当前前后数年。
  const years = useMemo(() => {
    const discovered = academicYears ?? academicYearOptions(records);
    const current = Number(currentAcademicYear().slice(0, 4));
    const range = Array.from({ length: 8 }, (_, index) => {
      const start = current + 2 - index;
      return `${start}-${start + 1}`;
    });
    return [...new Set([...range, ...discovered])].sort((a, b) =>
      b.localeCompare(a),
    );
  }, [academicYears, records]);
  const allVisibleSelected =
    records.length > 0 && records.every((record) => selected.has(record.id));
  const partiallySelected =
    records.some((record) => selected.has(record.id)) && !allVisibleSelected;
  const hasFilters = Boolean(
    filter.query ||
    filter.academicYear !== "all" ||
    filter.categoryId !== "all" ||
    filter.level !== "all" ||
    filter.materialStatus !== "all" ||
    filter.dateFrom ||
    filter.dateTo,
  );

  useEffect(() => {
    const visibleIds = new Set(records.map((record) => record.id));
    setSelected(
      (current) => new Set([...current].filter((id) => visibleIds.has(id))),
    );
  }, [records]);

  function toggleAll(checked: boolean) {
    setSelected(
      checked ? new Set(records.map((record) => record.id)) : new Set(),
    );
  }

  function toggleOne(id: string, checked: boolean) {
    setSelected((current) => {
      const next = new Set(current);
      if (checked) next.add(id);
      else next.delete(id);
      return next;
    });
  }

  async function removeSelected() {
    const ids = [...selected];
    if (ids.length === 0) return;
    setDeleting(true);
    try {
      const result = await api.moveRecordsToTrash(ids);
      setSelected(new Set());
      setDeleteConfirmOpen(false);
      notify("success", "已移入回收站", result.message);
      onDataChanged();
    } catch (reason) {
      notify("error", "删除失败", errorMessage(reason));
    } finally {
      setDeleting(false);
    }
  }

  async function exportPackage() {
    const ids = [...selected];
    if (ids.length === 0) return;
    setExporting(true);
    try {
      const result = await api.exportMaterialPackage(ids);
      if (!result) return;
      notify("success", "材料包已导出", result.path ?? result.message);
    } catch (reason) {
      notify("error", "导出失败", errorMessage(reason));
    } finally {
      setExporting(false);
    }
  }

  async function exportCurrent() {
    setExporting(true);
    try {
      const result = await api.exportExcel(filter);
      if (!result) return;
      notify("success", "Excel 已导出", result.path ?? result.message);
    } catch (reason) {
      notify("error", "导出失败", errorMessage(reason));
    } finally {
      setExporting(false);
    }
  }

  return (
    <div className="page records-page">
      <PageHeading
        title="综测记录"
        description={`${filter.academicYear === "all" ? currentAcademicYear() : filter.academicYear} 学年，共 ${total} 条记录`}
        actions={
          <>
            <Button
              appearance="secondary"
              icon={<FileArchive {...iconProps} />}
              disabled={exporting}
              onClick={() => void exportCurrent()}
            >
              导出当前结果
            </Button>
            <Button
              appearance="primary"
              icon={<Plus {...iconProps} />}
              onClick={onNew}
            >
              新增记录
            </Button>
          </>
        }
      />

      <KpiStrip summary={summary} />

      <section className="filter-bar" aria-label="记录筛选">
        <Input
          ref={searchRef}
          type="search"
          className="search-input"
          contentBefore={<Search size={17} strokeWidth={1.8} />}
          placeholder="搜索活动、备注或材料名称"
          aria-label="搜索记录"
          value={filter.query}
          onChange={(_, data) => onFilterChange({ query: data.value, page: 1 })}
        />
        <Select
          aria-label="筛选学年"
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
        <Input
          type="date"
          aria-label="开始日期"
          value={filter.dateFrom}
          onChange={(_, data) =>
            onFilterChange({ dateFrom: data.value, page: 1 })
          }
        />
        <Input
          type="date"
          aria-label="结束日期"
          value={filter.dateTo}
          onChange={(_, data) =>
            onFilterChange({ dateTo: data.value, page: 1 })
          }
        />
        <Select
          aria-label="记录排序"
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
        <Select
          aria-label="筛选活动类别"
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
        <Select
          aria-label="筛选综测级别"
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
        <Select
          aria-label="筛选材料状态"
          value={filter.materialStatus}
          onChange={(_, data) =>
            onFilterChange({
              materialStatus: data.value as RecordFilter["materialStatus"],
              page: 1,
            })
          }
        >
          <option value="all">全部材料状态</option>
          <option value="attached">材料齐全</option>
          <option value="missing">待补材料</option>
        </Select>
        {hasFilters ? (
          <Tooltip content="清除筛选" relationship="label">
            <Button
              appearance="subtle"
              icon={<X {...iconProps} />}
              aria-label="清除全部筛选"
              onClick={() =>
                onFilterChange({
                  query: "",
                  academicYear: "all",
                  categoryId: "all",
                  level: "all",
                  materialStatus: "all",
                  dateFrom: "",
                  dateTo: "",
                  sort: "dateDesc",
                  page: 1,
                })
              }
            />
          </Tooltip>
        ) : (
          <Tooltip content="筛选条件" relationship="label">
            <Button
              appearance="subtle"
              icon={<SlidersHorizontal {...iconProps} />}
              aria-label="筛选条件"
              disabled
            />
          </Tooltip>
        )}
      </section>

      {selected.size > 0 ? (
        <div className="selection-bar" role="status">
          <span>
            已选择 <strong>{selected.size}</strong> 条记录
          </span>
          <div>
            <Button
              size="small"
              appearance="secondary"
              icon={<FileArchive {...iconProps} />}
              disabled={exporting}
              onClick={() => void exportPackage()}
            >
              导出材料包
            </Button>
            <Button
              size="small"
              appearance="secondary"
              className="danger-secondary-button"
              icon={<Trash2 {...iconProps} />}
              onClick={() => setDeleteConfirmOpen(true)}
            >
              移入回收站
            </Button>
            <Tooltip content="取消选择" relationship="label">
              <Button
                size="small"
                appearance="subtle"
                icon={<X {...iconProps} />}
                aria-label="取消全部选择"
                onClick={() => setSelected(new Set())}
              />
            </Tooltip>
          </div>
        </div>
      ) : null}

      <section className="table-shell" aria-label="综测记录列表">
        {loading ? (
          <LoadingRows />
        ) : error ? (
          <EmptyState
            icon={<RefreshCw size={28} strokeWidth={1.6} />}
            title="记录加载失败"
            description={error}
            action={
              <Button appearance="primary" onClick={onReload}>
                重新加载
              </Button>
            }
          />
        ) : records.length === 0 ? (
          <EmptyState
            icon={
              hasFilters ? (
                <Search size={28} strokeWidth={1.6} />
              ) : (
                <ListChecks size={28} strokeWidth={1.6} />
              )
            }
            title={hasFilters ? "没有符合条件的记录" : "还没有综测记录"}
            description={
              hasFilters
                ? "调整或清除筛选条件后再试。"
                : "添加第一条活动记录，开始整理本学年的综测材料。"
            }
            action={
              hasFilters ? (
                <Button
                  appearance="secondary"
                  onClick={() =>
                    onFilterChange({
                      query: "",
                      academicYear: "all",
                      categoryId: "all",
                      level: "all",
                      materialStatus: "all",
                      dateFrom: "",
                      dateTo: "",
                      sort: "dateDesc",
                      page: 1,
                    })
                  }
                >
                  清除筛选
                </Button>
              ) : (
                <Button
                  appearance="primary"
                  icon={<Plus {...iconProps} />}
                  onClick={onNew}
                >
                  新增记录
                </Button>
              )
            }
          />
        ) : (
          <table className="data-table">
            <thead>
              <tr>
                <th className="checkbox-column">
                  <Checkbox
                    aria-label="选择当前全部记录"
                    checked={partiallySelected ? "mixed" : allVisibleSelected}
                    onChange={(_, data) => toggleAll(data.checked === true)}
                  />
                </th>
                <th>活动名称</th>
                <th>活动类别</th>
                <th>级别</th>
                <th>活动日期</th>
                <th className="numeric-column">分数</th>
                <th>证明材料</th>
                <th className="action-column">
                  <span className="visually-hidden">操作</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {records.map((record) => (
                <tr
                  key={record.id}
                  className={
                    selected.has(record.id) ? "selected-row" : undefined
                  }
                  onDoubleClick={() => onEdit(record)}
                >
                  <td className="checkbox-column">
                    <Checkbox
                      aria-label={`选择 ${record.name}`}
                      checked={selected.has(record.id)}
                      onChange={(_, data) =>
                        toggleOne(record.id, data.checked === true)
                      }
                    />
                  </td>
                  <td>
                    <button
                      className="record-name-button"
                      onClick={() => onEdit(record)}
                      title={record.name}
                    >
                      <strong>{record.name}</strong>
                      {record.notes ? <small>{record.notes}</small> : null}
                    </button>
                  </td>
                  <td>
                    <span className="category-text">{record.categoryName}</span>
                  </td>
                  <td>
                    <span className={`level-badge level-${record.level}`}>
                      {LEVEL_META[record.level].label}
                    </span>
                  </td>
                  <td className="date-cell">{record.date}</td>
                  <td className="numeric-column score-cell">
                    {formatScore(record.score)}
                  </td>
                  <td>
                    {record.materials.length ? (
                      <span className="material-status complete">
                        <FileCheck2 size={15} strokeWidth={1.8} />
                        {record.materials.length} 份
                      </span>
                    ) : (
                      <span className="material-status missing">
                        <FileWarning size={15} strokeWidth={1.8} />
                        待补材料
                      </span>
                    )}
                  </td>
                  <td className="action-column">
                    <Tooltip content="查看并编辑" relationship="label">
                      <Button
                        appearance="subtle"
                        size="small"
                        icon={<ChevronRight {...iconProps} />}
                        aria-label={`查看 ${record.name}`}
                        onClick={() => onEdit(record)}
                      />
                    </Tooltip>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>

      {total > 0 ? (
        <div className="pagination-bar" role="navigation" aria-label="记录分页">
          <span>
            第{" "}
            {Math.min(
              filter.page,
              Math.max(1, Math.ceil(total / filter.pageSize)),
            )}{" "}
            页，共 {Math.max(1, Math.ceil(total / filter.pageSize))} 页（{total}{" "}
            条）
          </span>
          <div className="pagination-controls">
            <Select
              aria-label="每页条数"
              value={String(filter.pageSize)}
              onChange={(_, data) =>
                onFilterChange({ pageSize: Number(data.value), page: 1 })
              }
            >
              <option value="25">25 条/页</option>
              <option value="50">50 条/页</option>
              <option value="100">100 条/页</option>
            </Select>
            <Button
              size="small"
              appearance="secondary"
              disabled={filter.page <= 1}
              onClick={() => onFilterChange({ page: filter.page - 1 })}
            >
              上一页
            </Button>
            <Button
              size="small"
              appearance="secondary"
              disabled={filter.page >= Math.ceil(total / filter.pageSize)}
              onClick={() => onFilterChange({ page: filter.page + 1 })}
            >
              下一页
            </Button>
          </div>
        </div>
      ) : null}

      <ConfirmDialog
        open={deleteConfirmOpen}
        title="将记录移入回收站"
        confirmLabel="移入回收站"
        danger
        busy={deleting}
        onCancel={() => setDeleteConfirmOpen(false)}
        onConfirm={() => void removeSelected()}
      >
        已选择 {selected.size} 条记录。移入回收站后保留 30 天，期间可以恢复。
      </ConfirmDialog>
    </div>
  );
}
