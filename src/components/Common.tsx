import type { ReactNode } from "react";
import {
  Button,
  Dialog,
  DialogActions,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
  Tooltip,
} from "@fluentui/react-components";
import { AlertCircle, CheckCircle2, Info, X } from "lucide-react";

const iconProps = { size: 18, strokeWidth: 1.8 } as const;

export type NoticeKind = "success" | "error" | "info";

export interface Notice {
  id: string;
  kind: NoticeKind;
  title: string;
  message?: string;
}

export function NoticeViewport({
  notices,
  onDismiss,
}: {
  notices: Notice[];
  onDismiss: (id: string) => void;
}) {
  const icon = (kind: NoticeKind) => {
    if (kind === "success") return <CheckCircle2 {...iconProps} />;
    if (kind === "error") return <AlertCircle {...iconProps} />;
    return <Info {...iconProps} />;
  };
  return (
    <div
      className="notice-viewport"
      aria-live="polite"
      aria-relevant="additions"
    >
      {notices.map((notice) => (
        <div
          className={`notice notice-${notice.kind}`}
          key={notice.id}
          role="status"
        >
          <div className="notice-icon" aria-hidden="true">
            {icon(notice.kind)}
          </div>
          <div className="notice-content">
            <strong>{notice.title}</strong>
            {notice.message ? <span>{notice.message}</span> : null}
          </div>
          <Tooltip content="关闭通知" relationship="label">
            <Button
              appearance="subtle"
              size="small"
              icon={<X {...iconProps} />}
              aria-label="关闭通知"
              onClick={() => onDismiss(notice.id)}
            />
          </Tooltip>
        </div>
      ))}
    </div>
  );
}

export function ConfirmDialog({
  open,
  title,
  children,
  confirmLabel = "确认",
  cancelLabel = "取消",
  danger = false,
  busy = false,
  onConfirm,
  onCancel,
}: {
  open: boolean;
  title: string;
  children: ReactNode;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
  busy?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <Dialog
      open={open}
      onOpenChange={(_, data) => !data.open && !busy && onCancel()}
    >
      <DialogSurface className="confirm-dialog">
        <DialogBody>
          <DialogTitle>{title}</DialogTitle>
          <DialogContent>{children}</DialogContent>
          <DialogActions>
            <Button appearance="secondary" disabled={busy} onClick={onCancel}>
              {cancelLabel}
            </Button>
            <Button
              appearance="primary"
              className={danger ? "danger-button" : undefined}
              disabled={busy}
              onClick={onConfirm}
            >
              {busy ? "处理中..." : confirmLabel}
            </Button>
          </DialogActions>
        </DialogBody>
      </DialogSurface>
    </Dialog>
  );
}

export function EmptyState({
  icon,
  title,
  description,
  action,
}: {
  icon: ReactNode;
  title: string;
  description: string;
  action?: ReactNode;
}) {
  return (
    <div className="empty-state">
      <div className="empty-icon" aria-hidden="true">
        {icon}
      </div>
      <h2>{title}</h2>
      <p>{description}</p>
      {action ? <div className="empty-action">{action}</div> : null}
    </div>
  );
}

export function LoadingRows({ count = 6 }: { count?: number }) {
  return (
    <div className="loading-rows" aria-label="正在加载">
      {Array.from({ length: count }, (_, index) => (
        <div className="loading-row" key={index}>
          <span className="skeleton skeleton-check" />
          <span className="skeleton skeleton-wide" />
          <span className="skeleton skeleton-medium" />
          <span className="skeleton skeleton-short" />
          <span className="skeleton skeleton-medium" />
        </div>
      ))}
    </div>
  );
}

export function PageHeading({
  title,
  description,
  actions,
}: {
  title: string;
  description: string;
  actions?: ReactNode;
}) {
  return (
    <header className="page-heading">
      <div>
        <h1>{title}</h1>
        <p>{description}</p>
      </div>
      {actions ? <div className="page-actions">{actions}</div> : null}
    </header>
  );
}

export function OperationPath({ path }: { path?: string }) {
  if (!path) return null;
  return (
    <span className="operation-path" title={path}>
      {path}
    </span>
  );
}
