import { useEffect, useState } from "react";
import { Button, Checkbox, Tooltip } from "@fluentui/react-components";
import { ArchiveRestore, Clock3, RefreshCw, Trash2 } from "lucide-react";
import type { AssessmentRecord } from "../types";
import { api } from "../lib/api";
import {
  LEVEL_META,
  errorMessage,
  formatDateTime,
  formatScore,
} from "../lib/utils";
import {
  ConfirmDialog,
  EmptyState,
  LoadingRows,
  PageHeading,
} from "../components/Common";

const iconProps = { size: 18, strokeWidth: 1.8 } as const;

function daysUntil(value: string | null): number {
  if (!value) return 0;
  return Math.max(
    0,
    Math.ceil((new Date(value).getTime() - Date.now()) / 86_400_000),
  );
}

export function TrashPage({
  records,
  loading,
  error,
  onReload,
  onDataChanged,
  notify,
}: {
  records: AssessmentRecord[];
  loading: boolean;
  error: string;
  onReload: () => void;
  onDataChanged: () => void;
  notify: (
    kind: "success" | "error" | "info",
    title: string,
    message?: string,
  ) => void;
}) {
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [busy, setBusy] = useState(false);
  const allSelected =
    records.length > 0 && records.every((record) => selected.has(record.id));
  const mixed =
    records.some((record) => selected.has(record.id)) && !allSelected;

  useEffect(() => {
    const visible = new Set(records.map((record) => record.id));
    setSelected(
      (current) => new Set([...current].filter((id) => visible.has(id))),
    );
  }, [records]);

  async function restore(ids: string[]) {
    if (ids.length === 0) return;
    setBusy(true);
    try {
      const result = await api.restoreRecords(ids);
      setSelected(new Set());
      notify("success", "记录已恢复", result.message);
      onDataChanged();
    } catch (reason) {
      notify("error", "恢复失败", errorMessage(reason));
    } finally {
      setBusy(false);
    }
  }

  async function removePermanently() {
    const ids = [...selected];
    if (ids.length === 0) return;
    setBusy(true);
    try {
      const result = await api.permanentlyDeleteRecords(ids);
      setSelected(new Set());
      setConfirmDelete(false);
      notify("success", "记录已永久删除", result.message);
      onDataChanged();
    } catch (reason) {
      notify("error", "永久删除失败", errorMessage(reason));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="page trash-page">
      <PageHeading
        title="回收站"
        description="删除的记录保留 30 天，到期后自动清理"
        actions={
          selected.size ? (
            <>
              <Button
                appearance="secondary"
                icon={<ArchiveRestore {...iconProps} />}
                disabled={busy}
                onClick={() => void restore([...selected])}
              >
                恢复所选
              </Button>
              <Button
                appearance="secondary"
                className="danger-secondary-button"
                icon={<Trash2 {...iconProps} />}
                disabled={busy}
                onClick={() => setConfirmDelete(true)}
              >
                永久删除
              </Button>
            </>
          ) : undefined
        }
      />

      <div className="trash-notice">
        <Clock3 {...iconProps} aria-hidden="true" />
        <span>恢复记录会同时恢复其材料引用。永久删除后无法撤销。</span>
      </div>

      <section
        className="table-shell trash-table-shell"
        aria-label="已删除的综测记录"
      >
        {loading ? (
          <LoadingRows count={5} />
        ) : error ? (
          <EmptyState
            icon={<RefreshCw size={28} strokeWidth={1.6} />}
            title="回收站加载失败"
            description={error}
            action={
              <Button appearance="primary" onClick={onReload}>
                重新加载
              </Button>
            }
          />
        ) : records.length === 0 ? (
          <EmptyState
            icon={<Trash2 size={28} strokeWidth={1.6} />}
            title="回收站为空"
            description="从记录页删除的内容会暂存在这里。"
          />
        ) : (
          <table className="data-table trash-table">
            <thead>
              <tr>
                <th className="checkbox-column">
                  <Checkbox
                    aria-label="选择回收站中的全部记录"
                    checked={mixed ? "mixed" : allSelected}
                    onChange={(_, data) =>
                      setSelected(
                        data.checked === true
                          ? new Set(records.map((record) => record.id))
                          : new Set(),
                      )
                    }
                  />
                </th>
                <th>活动名称</th>
                <th>级别</th>
                <th>活动日期</th>
                <th className="numeric-column">分数</th>
                <th>删除时间</th>
                <th>剩余时间</th>
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
                >
                  <td className="checkbox-column">
                    <Checkbox
                      aria-label={`选择 ${record.name}`}
                      checked={selected.has(record.id)}
                      onChange={(_, data) => {
                        setSelected((current) => {
                          const next = new Set(current);
                          if (data.checked === true) next.add(record.id);
                          else next.delete(record.id);
                          return next;
                        });
                      }}
                    />
                  </td>
                  <td>
                    <strong className="trash-record-name">{record.name}</strong>
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
                  <td className="muted-cell">
                    {record.deletedAt ? formatDateTime(record.deletedAt) : ""}
                  </td>
                  <td>
                    <span
                      className={`expiry-label ${daysUntil(record.purgeAt) <= 3 ? "urgent" : ""}`}
                    >
                      {daysUntil(record.purgeAt)} 天
                    </span>
                  </td>
                  <td className="action-column">
                    <Tooltip content="恢复记录" relationship="label">
                      <Button
                        appearance="subtle"
                        size="small"
                        icon={<ArchiveRestore {...iconProps} />}
                        aria-label={`恢复 ${record.name}`}
                        disabled={busy}
                        onClick={() => void restore([record.id])}
                      />
                    </Tooltip>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>

      <ConfirmDialog
        open={confirmDelete}
        title="永久删除记录"
        confirmLabel="永久删除"
        danger
        busy={busy}
        onCancel={() => setConfirmDelete(false)}
        onConfirm={() => void removePermanently()}
      >
        将永久删除 {selected.size}{" "}
        条记录。相关材料在不再被任何记录或恢复点引用后也会被清理，此操作无法撤销。
      </ConfirmDialog>
    </div>
  );
}
