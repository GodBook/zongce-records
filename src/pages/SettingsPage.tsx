import { useEffect, useState } from "react";
import {
  Button,
  Dialog,
  DialogActions,
  DialogBody,
  DialogContent,
  DialogSurface,
  DialogTitle,
  Field,
  Input,
  Select,
  Switch,
  Tab,
  TabList,
  Tooltip,
} from "@fluentui/react-components";
import {
  Archive,
  Check,
  Database,
  Download,
  ExternalLink,
  FileDown,
  FileInput,
  FolderCog,
  HardDrive,
  Pencil,
  Plus,
  RefreshCw,
  RotateCcw,
  Save,
  ShieldCheck,
  Tags,
  Upload,
  X,
} from "lucide-react";
import type {
  AppInitialization,
  BackupInspection,
  BackupRestoreMode,
  Category,
  ImportPreview,
  StorageStatus,
  UpdateInfo,
} from "../types";
import { api } from "../lib/api";
import {
  DEFAULT_FILTER,
  errorMessage,
  formatBytes,
  formatDateTime,
} from "../lib/utils";
import { ConfirmDialog, EmptyState, PageHeading } from "../components/Common";

const iconProps = { size: 18, strokeWidth: 1.8 } as const;
type SettingsTab = "exchange" | "categories" | "backup" | "update";

function SettingSection({
  title,
  description,
  children,
}: {
  title: string;
  description: string;
  children: React.ReactNode;
}) {
  return (
    <section className="settings-section">
      <header>
        <h2>{title}</h2>
        <p>{description}</p>
      </header>
      <div className="settings-section-content">{children}</div>
    </section>
  );
}

function ActionRow({
  icon,
  title,
  description,
  action,
}: {
  icon: React.ReactNode;
  title: string;
  description: string;
  action: React.ReactNode;
}) {
  return (
    <div className="settings-action-row">
      <span className="settings-action-icon" aria-hidden="true">
        {icon}
      </span>
      <span className="settings-action-copy">
        <strong>{title}</strong>
        <small>{description}</small>
      </span>
      <div className="settings-action-control">{action}</div>
    </div>
  );
}

export function SettingsPage({
  categories,
  initialization,
  onDataChanged,
  notify,
}: {
  categories: Category[];
  initialization: AppInitialization;
  onDataChanged: () => void;
  notify: (
    kind: "success" | "error" | "info",
    title: string,
    message?: string,
  ) => void;
}) {
  const [activeTab, setActiveTab] = useState<SettingsTab>("exchange");
  const [busyAction, setBusyAction] = useState("");
  const [importPreview, setImportPreview] = useState<ImportPreview | null>(
    null,
  );
  const [includeDuplicates, setIncludeDuplicates] = useState(false);
  const [backup, setBackup] = useState<BackupInspection | null>(null);
  const [restoreMode, setRestoreMode] = useState<BackupRestoreMode>("merge");
  const [replaceConfirm, setReplaceConfirm] = useState(false);
  const [storage, setStorage] = useState<StorageStatus | null>(null);
  const [storageError, setStorageError] = useState("");
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [updateConfirm, setUpdateConfirm] = useState(false);
  const [updateProgress, setUpdateProgress] = useState<number | null>(null);
  const [newCategory, setNewCategory] = useState("");
  const [editingCategory, setEditingCategory] = useState<Category | null>(null);
  const [editingName, setEditingName] = useState("");

  async function loadStorage() {
    setStorageError("");
    try {
      setStorage(await api.getStorageStatus());
    } catch (reason) {
      setStorageError(errorMessage(reason));
    }
  }

  useEffect(() => {
    if (activeTab === "backup" && !storage && !storageError) void loadStorage();
  }, [activeTab, storage, storageError]);

  async function run(
    key: string,
    action: () => Promise<unknown | null>,
    successTitle: string,
    after?: () => void,
  ) {
    setBusyAction(key);
    try {
      const result = await action();
      if (result) {
        const details =
          typeof result === "object"
            ? (result as { message?: string; path?: string })
            : {};
        notify("success", successTitle, details.path ?? details.message);
        after?.();
      }
    } catch (reason) {
      notify("error", "操作失败", errorMessage(reason));
    } finally {
      setBusyAction("");
    }
  }

  async function previewImport() {
    setBusyAction("import-preview");
    try {
      const preview = await api.previewExcel();
      if (preview) {
        setIncludeDuplicates(false);
        setImportPreview(preview);
      }
    } catch (reason) {
      notify("error", "无法读取 Excel", errorMessage(reason));
    } finally {
      setBusyAction("");
    }
  }

  async function commitImport() {
    if (!importPreview) return;
    await run(
      "import-commit",
      () => api.commitExcel(importPreview.token, includeDuplicates),
      "Excel 导入完成",
      () => {
        setImportPreview(null);
        onDataChanged();
      },
    );
  }

  async function inspectBackup() {
    setBusyAction("backup-inspect");
    try {
      const result = await api.inspectBackup();
      if (result) {
        setBackup(result);
        setRestoreMode("merge");
      }
    } catch (reason) {
      notify("error", "无法读取备份", errorMessage(reason));
    } finally {
      setBusyAction("");
    }
  }

  async function restoreSelectedBackup() {
    if (!backup) return;
    if (restoreMode === "replace" && !replaceConfirm) {
      setReplaceConfirm(true);
      return;
    }
    await run(
      "backup-restore",
      () => api.restoreBackup(backup.token, restoreMode),
      "备份恢复完成",
      () => {
        setBackup(null);
        setReplaceConfirm(false);
        onDataChanged();
        void loadStorage();
      },
    );
  }

  async function saveNewCategory() {
    const name = newCategory.trim();
    if (!name) return;
    await run(
      "category-new",
      () => api.saveCategory({ name }),
      "类别已添加",
      () => {
        setNewCategory("");
        onDataChanged();
      },
    );
  }

  async function saveEditedCategory() {
    if (!editingCategory || !editingName.trim()) return;
    await run(
      `category-edit-${editingCategory.id}`,
      () => api.saveCategory({ id: editingCategory.id, name: editingName }),
      "类别已更新",
      () => {
        setEditingCategory(null);
        onDataChanged();
      },
    );
  }

  async function toggleCategory(category: Category, checked: boolean) {
    await run(
      `category-toggle-${category.id}`,
      () => api.setCategoryActive(category.id, checked),
      checked ? "类别已启用" : "类别已停用",
      onDataChanged,
    );
  }

  async function checkUpdate() {
    setBusyAction("update");
    try {
      const result = await api.checkForUpdate();
      setUpdate(result);
      notify(
        result.available ? "info" : "success",
        result.available ? "发现新版本" : "已经是最新版本",
        result.available ? `可更新至 ${result.version}` : result.currentVersion,
      );
    } catch (reason) {
      notify("error", "检查更新失败", errorMessage(reason));
    } finally {
      setBusyAction("");
    }
  }

  async function installUpdate() {
    if (!update?.available) return;
    setBusyAction("update-install");
    setUpdateProgress(0);
    try {
      await api.installUpdate((downloaded, total) => {
        if (!total || total <= 0) {
          setUpdateProgress(null);
          return;
        }
        setUpdateProgress(
          Math.min(100, Math.round((downloaded / total) * 100)),
        );
      });
    } catch (reason) {
      setUpdateConfirm(false);
      notify("error", "更新安装失败", errorMessage(reason));
    } finally {
      setBusyAction("");
      setUpdateProgress(null);
    }
  }

  async function openReleaseNotes() {
    if (!update?.version) return;
    try {
      await api.openReleaseNotes(update.version);
    } catch (reason) {
      notify("error", "无法打开发布说明", errorMessage(reason));
    }
  }

  return (
    <div className="page settings-page">
      <PageHeading
        title="设置"
        description="管理类别、数据交换、本地备份和软件更新"
      />

      <TabList
        className="settings-tabs"
        selectedValue={activeTab}
        onTabSelect={(_, data) =>
          setActiveTab(String(data.value) as SettingsTab)
        }
      >
        <Tab value="exchange" icon={<FileInput {...iconProps} />}>
          数据交换
        </Tab>
        <Tab value="categories" icon={<Tags {...iconProps} />}>
          活动类别
        </Tab>
        <Tab value="backup" icon={<Database {...iconProps} />}>
          备份与存储
        </Tab>
        <Tab value="update" icon={<Download {...iconProps} />}>
          软件更新
        </Tab>
      </TabList>

      <div className="settings-content">
        {activeTab === "exchange" ? (
          <>
            <SettingSection
              title="Excel 导入"
              description="使用官方模板批量整理记录，提交前会先展示检查结果。"
            >
              <ActionRow
                icon={<FileDown {...iconProps} />}
                title="下载导入模板"
                description="包含字段说明、示例数据和级别取值。"
                action={
                  <Button
                    appearance="secondary"
                    icon={<Download {...iconProps} />}
                    disabled={Boolean(busyAction)}
                    onClick={() =>
                      void run(
                        "template",
                        () => api.exportExcelTemplate(),
                        "模板已保存",
                      )
                    }
                  >
                    下载模板
                  </Button>
                }
              />
              <ActionRow
                icon={<FileInput {...iconProps} />}
                title="从 Excel 导入"
                description="识别常用表头别名，不会删除记录已有的附件。"
                action={
                  <Button
                    appearance="primary"
                    icon={<Upload {...iconProps} />}
                    disabled={Boolean(busyAction)}
                    onClick={() => void previewImport()}
                  >
                    选择文件
                  </Button>
                }
              />
            </SettingSection>

            <SettingSection
              title="Excel 导出"
              description="生成记录明细和统计汇总两个工作表。"
            >
              <ActionRow
                icon={<FileDown {...iconProps} />}
                title="导出全部记录"
                description="包含所有未删除记录、材料状态与分类汇总。"
                action={
                  <Button
                    appearance="secondary"
                    icon={<Download {...iconProps} />}
                    disabled={Boolean(busyAction)}
                    onClick={() =>
                      void run(
                        "export-all",
                        () => api.exportExcel(DEFAULT_FILTER),
                        "Excel 已导出",
                      )
                    }
                  >
                    导出全部
                  </Button>
                }
              />
            </SettingSection>
          </>
        ) : null}

        {activeTab === "categories" ? (
          <SettingSection
            title="活动类别"
            description="停用类别不会影响已有记录，但新增记录将不再显示该类别。"
          >
            <form
              className="category-create"
              onSubmit={(event) => {
                event.preventDefault();
                void saveNewCategory();
              }}
            >
              <Field label="新增类别">
                <Input
                  value={newCategory}
                  maxLength={30}
                  placeholder="输入类别名称"
                  onChange={(_, data) => setNewCategory(data.value)}
                />
              </Field>
              <Button
                type="submit"
                appearance="primary"
                icon={<Plus {...iconProps} />}
                disabled={!newCategory.trim() || Boolean(busyAction)}
              >
                添加类别
              </Button>
            </form>
            <div className="category-table-wrap">
              <table className="category-table">
                <thead>
                  <tr>
                    <th>类别名称</th>
                    <th>记录数</th>
                    <th>来源</th>
                    <th>状态</th>
                    <th>
                      <span className="visually-hidden">操作</span>
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {categories.map((category) => {
                    const editing = editingCategory?.id === category.id;
                    return (
                      <tr key={category.id}>
                        <td>
                          {editing ? (
                            <Input
                              size="small"
                              value={editingName}
                              maxLength={30}
                              aria-label={`编辑 ${category.name}`}
                              onChange={(_, data) => setEditingName(data.value)}
                              onKeyDown={(event) => {
                                if (event.key === "Enter")
                                  void saveEditedCategory();
                                if (event.key === "Escape")
                                  setEditingCategory(null);
                              }}
                            />
                          ) : (
                            <strong>{category.name}</strong>
                          )}
                        </td>
                        <td className="numeric-cell">{category.recordCount}</td>
                        <td>
                          <span className="source-label">
                            {category.isBuiltin ? "内置" : "自定义"}
                          </span>
                        </td>
                        <td>
                          <Switch
                            checked={category.isActive}
                            label={category.isActive ? "已启用" : "已停用"}
                            disabled={
                              busyAction === `category-toggle-${category.id}`
                            }
                            onChange={(_, data) =>
                              void toggleCategory(category, data.checked)
                            }
                          />
                        </td>
                        <td className="category-actions">
                          {editing ? (
                            <>
                              <Tooltip content="保存" relationship="label">
                                <Button
                                  appearance="subtle"
                                  size="small"
                                  icon={<Check {...iconProps} />}
                                  aria-label={`保存 ${category.name}`}
                                  disabled={
                                    !editingName.trim() || Boolean(busyAction)
                                  }
                                  onClick={() => void saveEditedCategory()}
                                />
                              </Tooltip>
                              <Tooltip content="取消" relationship="label">
                                <Button
                                  appearance="subtle"
                                  size="small"
                                  icon={<X {...iconProps} />}
                                  aria-label="取消编辑类别"
                                  onClick={() => setEditingCategory(null)}
                                />
                              </Tooltip>
                            </>
                          ) : (
                            <Tooltip content="重命名" relationship="label">
                              <Button
                                appearance="subtle"
                                size="small"
                                icon={<Pencil {...iconProps} />}
                                aria-label={`重命名 ${category.name}`}
                                onClick={() => {
                                  setEditingCategory(category);
                                  setEditingName(category.name);
                                }}
                              />
                            </Tooltip>
                          )}
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          </SettingSection>
        ) : null}

        {activeTab === "backup" ? (
          <>
            <SettingSection
              title="完整备份"
              description="备份包含数据库和所有仍被引用的证明材料。"
            >
              <ActionRow
                icon={<Archive {...iconProps} />}
                title="创建完整备份"
                description="建议在学期结束和批量导入前各保存一份。"
                action={
                  <Button
                    appearance="primary"
                    icon={<Save {...iconProps} />}
                    disabled={Boolean(busyAction)}
                    onClick={() =>
                      void run(
                        "backup-export",
                        () => api.exportBackup(),
                        "完整备份已创建",
                        () => void loadStorage(),
                      )
                    }
                  >
                    创建备份
                  </Button>
                }
              />
              <ActionRow
                icon={<RotateCcw {...iconProps} />}
                title="从备份恢复"
                description="先校验备份完整性，再选择合并或替换。"
                action={
                  <Button
                    appearance="secondary"
                    icon={<Upload {...iconProps} />}
                    disabled={Boolean(busyAction)}
                    onClick={() => void inspectBackup()}
                  >
                    选择备份
                  </Button>
                }
              />
            </SettingSection>

            <SettingSection
              title="数据位置"
              description="迁移过程会复制并校验数据，成功切换后才清理旧副本。"
            >
              {storageError ? (
                <EmptyState
                  icon={<RefreshCw size={26} strokeWidth={1.6} />}
                  title="无法读取存储状态"
                  description={storageError}
                  action={
                    <Button
                      appearance="primary"
                      onClick={() => void loadStorage()}
                    >
                      重试
                    </Button>
                  }
                />
              ) : !storage ? (
                <div className="storage-loading">
                  <span className="skeleton" />
                  <span className="skeleton" />
                </div>
              ) : (
                <div className="storage-panel">
                  <div className="storage-path">
                    <span className="settings-action-icon" aria-hidden="true">
                      <HardDrive {...iconProps} />
                    </span>
                    <span>
                      <small>当前数据位置</small>
                      <strong title={storage.root}>{storage.root}</strong>
                    </span>
                    <span
                      className={`storage-health ${storage.writable ? "healthy" : "error"}`}
                    >
                      {storage.writable ? (
                        <ShieldCheck size={16} strokeWidth={1.8} />
                      ) : (
                        <X size={16} strokeWidth={1.8} />
                      )}
                      {storage.writable ? "可正常写入" : "位置不可用"}
                    </span>
                  </div>
                  <dl className="storage-metrics">
                    <div>
                      <dt>数据库</dt>
                      <dd>{formatBytes(storage.databaseBytes)}</dd>
                    </div>
                    <div>
                      <dt>证明材料</dt>
                      <dd>{formatBytes(storage.materialBytes)}</dd>
                    </div>
                    <div>
                      <dt>内部恢复点</dt>
                      <dd>{storage.recoveryPointCount} 个</dd>
                    </div>
                    <div>
                      <dt>目标磁盘可用</dt>
                      <dd>{formatBytes(storage.availableBytes)}</dd>
                    </div>
                  </dl>
                  <Button
                    appearance="secondary"
                    icon={<FolderCog {...iconProps} />}
                    disabled={Boolean(busyAction)}
                    onClick={() =>
                      void run(
                        "storage-migrate",
                        () => api.migrateDataRoot(),
                        "数据位置已迁移",
                        () => void loadStorage(),
                      )
                    }
                  >
                    迁移数据位置
                  </Button>
                </div>
              )}
            </SettingSection>
          </>
        ) : null}

        {activeTab === "update" ? (
          <SettingSection
            title="软件更新"
            description="更新由 GitHub Releases 分发，并在安装前验证 Tauri 签名。"
          >
            <div className="update-panel">
              <div className="update-mark" aria-hidden="true">
                <Download size={26} strokeWidth={1.7} />
              </div>
              <div className="update-copy">
                <h3>综测记录</h3>
                <p>当前版本 {initialization.appVersion}</p>
                {update ? (
                  <div
                    className={`update-result ${update.available ? "available" : "current"}`}
                  >
                    <strong>
                      {update.available
                        ? `可更新至 ${update.version}`
                        : "当前已经是最新版本"}
                    </strong>
                    <span>{update.notes}</span>
                    {update.publishedAt ? (
                      <small>发布于 {formatDateTime(update.publishedAt)}</small>
                    ) : null}
                  </div>
                ) : (
                  <span className="update-hint">
                    每天首次启动时自动检查一次，也可以现在手动检查。
                  </span>
                )}
              </div>
              <Button
                appearance="primary"
                icon={<RefreshCw {...iconProps} />}
                disabled={busyAction === "update"}
                onClick={() => void checkUpdate()}
              >
                {busyAction === "update" ? "正在检查..." : "检查更新"}
              </Button>
            </div>
            {update?.available ? (
              <div className="update-actions">
                <Button
                  appearance="primary"
                  icon={<Download {...iconProps} />}
                  disabled={Boolean(busyAction)}
                  onClick={() => setUpdateConfirm(true)}
                >
                  {busyAction === "update-install"
                    ? updateProgress === null
                      ? "正在下载..."
                      : `正在下载 ${updateProgress}%`
                    : "下载并安装"}
                </Button>
                <Button
                  appearance="secondary"
                  icon={<ExternalLink {...iconProps} />}
                  disabled={Boolean(busyAction)}
                  onClick={() => void openReleaseNotes()}
                >
                  查看发布说明
                </Button>
              </div>
            ) : null}
          </SettingSection>
        ) : null}
      </div>

      <Dialog
        open={Boolean(importPreview)}
        onOpenChange={(_, data) =>
          !data.open && !busyAction && setImportPreview(null)
        }
      >
        <DialogSurface className="import-dialog">
          <DialogBody>
            <DialogTitle>确认 Excel 导入</DialogTitle>
            <DialogContent>
              {importPreview ? (
                <>
                  <p className="dialog-intro">
                    <strong>{importPreview.fileName}</strong>，共读取{" "}
                    {importPreview.total} 行。
                  </p>
                  <div className="import-summary" aria-label="导入检查汇总">
                    <span>
                      <strong>{importPreview.newCount}</strong> 新增
                    </span>
                    <span>
                      <strong>{importPreview.updateCount}</strong> 更新
                    </span>
                    <span>
                      <strong>{importPreview.duplicateCount}</strong> 疑似重复
                    </span>
                    <span>
                      <strong>{importPreview.errorCount}</strong> 错误
                    </span>
                  </div>
                  <div className="import-table-wrap">
                    <table className="import-table">
                      <thead>
                        <tr>
                          <th>行</th>
                          <th>状态</th>
                          <th>活动名称</th>
                          <th>说明</th>
                        </tr>
                      </thead>
                      <tbody>
                        {importPreview.rows.map((row) => (
                          <tr key={row.row}>
                            <td className="numeric-cell">{row.row}</td>
                            <td>
                              <span
                                className={`import-status status-${row.status}`}
                              >
                                {
                                  {
                                    new: "新增",
                                    update: "更新",
                                    skip: "跳过",
                                    duplicate: "疑似重复",
                                    error: "错误",
                                  }[row.status]
                                }
                              </span>
                            </td>
                            <td>{row.name}</td>
                            <td className="muted-cell">{row.message}</td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                  {importPreview.duplicateCount > 0 ? (
                    <div className="duplicate-import-choice">
                      <Switch
                        checked={includeDuplicates}
                        label="将疑似重复行作为新记录导入"
                        onChange={(_, data) =>
                          setIncludeDuplicates(data.checked)
                        }
                      />
                      <span>
                        关闭时会跳过这些行；开启后将为每行创建新的记录 ID。
                      </span>
                    </div>
                  ) : null}
                </>
              ) : null}
            </DialogContent>
            <DialogActions>
              <Button
                appearance="secondary"
                disabled={Boolean(busyAction)}
                onClick={() => setImportPreview(null)}
              >
                取消
              </Button>
              <Button
                appearance="primary"
                disabled={
                  Boolean(busyAction) || Boolean(importPreview?.errorCount)
                }
                onClick={() => void commitImport()}
              >
                {busyAction === "import-commit"
                  ? "正在导入..."
                  : includeDuplicates
                    ? "确认并导入重复项"
                    : "确认导入"}
              </Button>
            </DialogActions>
          </DialogBody>
        </DialogSurface>
      </Dialog>

      <Dialog
        open={Boolean(backup)}
        onOpenChange={(_, data) => !data.open && !busyAction && setBackup(null)}
      >
        <DialogSurface className="backup-dialog">
          <DialogBody>
            <DialogTitle>恢复完整备份</DialogTitle>
            <DialogContent>
              {backup ? (
                <>
                  <div
                    className={`backup-integrity ${backup.integrityValid ? "valid" : "invalid"}`}
                  >
                    {backup.integrityValid ? (
                      <ShieldCheck {...iconProps} />
                    ) : (
                      <X {...iconProps} />
                    )}
                    <span>
                      <strong>
                        {backup.integrityValid
                          ? "完整性校验通过"
                          : "备份文件已损坏"}
                      </strong>
                      <small>{backup.fileName}</small>
                    </span>
                  </div>
                  <dl className="backup-details">
                    <div>
                      <dt>创建时间</dt>
                      <dd>{formatDateTime(backup.createdAt)}</dd>
                    </div>
                    <div>
                      <dt>软件版本</dt>
                      <dd>{backup.appVersion}</dd>
                    </div>
                    <div>
                      <dt>记录数量</dt>
                      <dd>{backup.recordCount} 条</dd>
                    </div>
                    <div>
                      <dt>材料数量</dt>
                      <dd>{backup.materialCount} 份</dd>
                    </div>
                    <div>
                      <dt>备份大小</dt>
                      <dd>{formatBytes(backup.totalBytes)}</dd>
                    </div>
                  </dl>
                  <Field label="恢复方式">
                    <Select
                      value={restoreMode}
                      onChange={(_, data) =>
                        setRestoreMode(data.value as BackupRestoreMode)
                      }
                    >
                      <option value="merge">冲突时保留本地版本（推荐）</option>
                      <option value="merge_import">冲突时导入备份版本</option>
                      <option value="merge_copy">冲突记录作为副本导入</option>
                      <option value="replace">替换当前数据</option>
                    </Select>
                  </Field>
                  <p className="restore-explanation">
                    {
                      {
                        merge: "同 ID 同内容的记录将跳过，冲突时保留本地记录。",
                        merge_import:
                          "同 ID 冲突时采用备份中的字段和附件，未冲突记录正常合并。",
                        merge_copy:
                          "同 ID 冲突时为备份记录生成新 ID，作为独立副本导入。",
                        replace: "当前数据会先完整保留，再由备份内容原子替换。",
                      }[restoreMode]
                    }
                  </p>
                </>
              ) : null}
            </DialogContent>
            <DialogActions>
              <Button
                appearance="secondary"
                disabled={Boolean(busyAction)}
                onClick={() => setBackup(null)}
              >
                取消
              </Button>
              <Button
                appearance="primary"
                disabled={Boolean(busyAction) || !backup?.integrityValid}
                onClick={() => void restoreSelectedBackup()}
              >
                {busyAction === "backup-restore" ? "正在恢复..." : "开始恢复"}
              </Button>
            </DialogActions>
          </DialogBody>
        </DialogSurface>
      </Dialog>

      <ConfirmDialog
        open={replaceConfirm}
        title="确认替换当前数据"
        confirmLabel="确认替换"
        danger
        busy={busyAction === "backup-restore"}
        onCancel={() => setReplaceConfirm(false)}
        onConfirm={() => void restoreSelectedBackup()}
      >
        替换会将当前数据库和材料切换为备份内容。系统会保留可回滚的旧数据恢复点，请确认继续。
      </ConfirmDialog>

      <ConfirmDialog
        open={updateConfirm}
        title="安装软件更新"
        confirmLabel="下载并安装"
        busy={busyAction === "update-install"}
        onCancel={() => setUpdateConfirm(false)}
        onConfirm={() => void installUpdate()}
      >
        将下载并验证 v{update?.version}{" "}
        的签名，安装完成后软件会自动重启。本地记录和证明材料不会被删除。
      </ConfirmDialog>
    </div>
  );
}
