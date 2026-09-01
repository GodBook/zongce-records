import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Button,
  FluentProvider,
  Select,
  createLightTheme,
  type BrandVariants,
} from "@fluentui/react-components";
import {
  BarChart3,
  ClipboardList,
  Plus,
  RefreshCw,
  Settings,
  Trash2,
} from "lucide-react";
import appIcon from "../assets/综测记录图标.svg";
import "./App.css";
import {
  NoticeViewport,
  type Notice,
  type NoticeKind,
} from "./components/Common";
import { RecordDrawer } from "./components/RecordDrawer";
import { api } from "./lib/api";
import { DEFAULT_FILTER, createId, errorMessage } from "./lib/utils";
import { RecordsPage } from "./pages/RecordsPage";
import { SettingsPage } from "./pages/SettingsPage";
import { StatisticsPage } from "./pages/StatisticsPage";
import { TrashPage } from "./pages/TrashPage";
import type {
  AppInitialization,
  AssessmentRecord,
  BackupRestoreMode,
  Category,
  PageKey,
  RecordFilter,
  StatisticsResult,
  BackupInspection,
} from "./types";

const brand: BrandVariants = {
  10: "#061f17",
  20: "#0b3326",
  30: "#0d4935",
  40: "#105f45",
  50: "#176b4d",
  60: "#247a5a",
  70: "#348968",
  80: "#469977",
  90: "#5aa988",
  100: "#70b99a",
  110: "#88c8ac",
  120: "#a1d7bf",
  130: "#bbe5d2",
  140: "#d3efe1",
  150: "#e8f7ef",
  160: "#f5fbf8",
};

const theme = {
  ...createLightTheme(brand),
  fontFamilyBase: '"Microsoft YaHei UI", "Segoe UI", sans-serif',
  fontFamilyMonospace: "Bahnschrift, Consolas, monospace",
  borderRadiusMedium: "4px",
  borderRadiusLarge: "6px",
  borderRadiusXLarge: "6px",
};

const EMPTY_STATS: StatisticsResult = {
  summary: {
    recordCount: 0,
    totalScore: "0.00",
    materialCount: 0,
    missingMaterialCount: 0,
  },
  byLevel: [],
  byCategory: [],
  monthly: [],
};

const NAV_ITEMS: Array<{
  key: PageKey;
  label: string;
  icon: typeof ClipboardList;
}> = [
  { key: "records", label: "记录", icon: ClipboardList },
  { key: "statistics", label: "统计", icon: BarChart3 },
  { key: "trash", label: "回收站", icon: Trash2 },
  { key: "settings", label: "设置", icon: Settings },
];

const UPDATE_CHECK_STORAGE_KEY = "zongce-records.last-update-check.v1";
const UPDATE_CHECK_INTERVAL = 24 * 60 * 60 * 1000;

function RecoveryScreen({
  initialization,
  onRecovered,
}: {
  initialization: AppInitialization;
  onRecovered: () => void;
}) {
  const [backup, setBackup] = useState<BackupInspection | null>(null);
  const [mode, setMode] = useState<BackupRestoreMode>("replace");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  async function inspect() {
    setBusy(true);
    setError("");
    try {
      const result = await api.inspectBackup();
      if (result) setBackup(result);
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setBusy(false);
    }
  }

  async function restore() {
    if (!backup) return;
    if (
      mode === "replace" &&
      !window.confirm("替换将覆盖当前数据目录，是否继续？")
    )
      return;
    setBusy(true);
    setError("");
    try {
      await api.restoreBackup(backup.token, mode);
      onRecovered();
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setBusy(false);
    }
  }

  async function chooseNewLocation() {
    setBusy(true);
    setError("");
    try {
      const result = await api.migrateDataRoot();
      if (result) onRecovered();
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setBusy(false);
    }
  }

  return (
    <FluentProvider theme={theme} className="app-provider">
      <div className="startup-screen recovery-screen">
        <img src={appIcon} alt="" />
        <h1>需要恢复本地数据</h1>
        <p>
          {initialization.recoveryRequired
            ? "数据库完整性检查未通过。请选择最近的 .zcbak 备份完成恢复。"
            : "当前数据目录不可用，业务功能已暂停。"}
        </p>
        {backup ? (
          <div className="recovery-panel">
            <strong>{backup.fileName}</strong>
            <span>
              {backup.integrityValid
                ? `校验通过，包含 ${backup.recordCount} 条记录、${backup.materialCount} 份材料。`
                : "校验失败，请选择其他备份文件。"}
            </span>
            {backup.integrityValid ? (
              <>
                <Select
                  aria-label="恢复方式"
                  value={mode}
                  onChange={(_, data) =>
                    setMode(data.value as BackupRestoreMode)
                  }
                >
                  {initialization.databaseHealthy ? (
                    <>
                      <option value="merge">冲突时保留本地版本</option>
                      <option value="merge_import">冲突时导入备份版本</option>
                      <option value="merge_copy">冲突记录作为副本导入</option>
                    </>
                  ) : null}
                  <option value="replace">使用备份替换当前数据</option>
                </Select>
                <Button
                  appearance="primary"
                  disabled={busy}
                  onClick={() => void restore()}
                >
                  {busy ? "恢复中…" : "开始恢复"}
                </Button>
              </>
            ) : null}
          </div>
        ) : null}
        {error ? <p role="alert">{error}</p> : null}
        <Button
          appearance="primary"
          disabled={busy}
          onClick={() => void inspect()}
        >
          {busy ? "正在校验…" : "选择 .zcbak 备份"}
        </Button>
        {!initialization.databaseHealthy ? (
          <Button
            appearance="secondary"
            disabled={busy}
            onClick={() => void chooseNewLocation()}
          >
            选择新的数据位置
          </Button>
        ) : null}
      </div>
    </FluentProvider>
  );
}

function App() {
  const [page, setPage] = useState<PageKey>("records");
  const [initialization, setInitialization] =
    useState<AppInitialization | null>(null);
  const [initialError, setInitialError] = useState("");
  const [categories, setCategories] = useState<Category[]>([]);
  const [records, setRecords] = useState<AssessmentRecord[]>([]);
  const [academicYears, setAcademicYears] = useState<string[]>([]);
  const [trash, setTrash] = useState<AssessmentRecord[]>([]);
  const [statistics, setStatistics] = useState<StatisticsResult>(EMPTY_STATS);
  const [filter, setFilter] = useState<RecordFilter>(DEFAULT_FILTER);
  const [loading, setLoading] = useState(true);
  const [dataError, setDataError] = useState("");
  const [refreshKey, setRefreshKey] = useState(0);
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [editingRecord, setEditingRecord] = useState<AssessmentRecord | null>(
    null,
  );
  const [notices, setNotices] = useState<Notice[]>([]);
  const searchRef = useRef<HTMLInputElement>(null);

  const notify = useCallback(
    (kind: NoticeKind, title: string, message?: string) => {
      const id = createId();
      setNotices((current) => [
        ...current.slice(-3),
        { id, kind, title, message },
      ]);
      window.setTimeout(
        () => setNotices((current) => current.filter((item) => item.id !== id)),
        5200,
      );
    },
    [],
  );

  const initialize = useCallback(async () => {
    setInitialError("");
    try {
      setInitialization(await api.initializeApp());
    } catch (reason) {
      setInitialError(errorMessage(reason));
    }
  }, []);

  useEffect(() => {
    void initialize();
  }, [initialize]);

  useEffect(() => {
    if (!initialization || !api.isTauri()) return;
    const now = Date.now();
    let lastCheck = 0;
    try {
      lastCheck = Number(localStorage.getItem(UPDATE_CHECK_STORAGE_KEY) ?? 0);
    } catch {
      // 更新检查不依赖浏览器存储可用性。
    }
    if (Number.isFinite(lastCheck) && now - lastCheck < UPDATE_CHECK_INTERVAL) {
      return;
    }
    try {
      localStorage.setItem(UPDATE_CHECK_STORAGE_KEY, String(now));
    } catch {
      // 写入失败时仍允许本次静默检查。
    }
    void api
      .checkForUpdate()
      .then((update) => {
        if (update.available) {
          notify(
            "info",
            `发现新版本 ${update.version}`,
            "可在设置中查看并安装更新",
          );
        }
      })
      .catch(() => undefined);
  }, [initialization, notify]);

  useEffect(() => {
    if (
      !initialization ||
      initialization.recoveryRequired ||
      !initialization.databaseHealthy
    )
      return;
    let active = true;
    setLoading(true);
    setDataError("");
    Promise.all([
      api.listCategories(),
      api.listRecords(filter),
      api.getStatistics(filter),
      api.listRecords({ ...DEFAULT_FILTER, trashedOnly: true, pageSize: 1000 }),
      api.listAcademicYears(),
    ])
      .then(
        ([
          nextCategories,
          recordResult,
          nextStatistics,
          trashResult,
          nextYears,
        ]) => {
          if (!active) return;
          setCategories(nextCategories);
          setRecords(recordResult.items);
          setStatistics(nextStatistics);
          setTrash(trashResult.items);
          setAcademicYears(nextYears);
        },
      )
      .catch((reason) => {
        if (active) setDataError(errorMessage(reason));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [initialization, filter, refreshKey]);

  const refresh = useCallback(() => setRefreshKey((value) => value + 1), []);
  const updateFilter = useCallback((patch: Partial<RecordFilter>) => {
    setFilter((current) => ({ ...current, ...patch }));
  }, []);
  const openNew = useCallback(() => {
    setEditingRecord(null);
    setDrawerOpen(true);
  }, []);
  const openEdit = useCallback((record: AssessmentRecord) => {
    setEditingRecord(record);
    setDrawerOpen(true);
  }, []);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!(event.ctrlKey || event.metaKey)) return;
      if (event.key.toLowerCase() === "n") {
        event.preventDefault();
        setPage("records");
        openNew();
      }
      if (event.key.toLowerCase() === "f") {
        event.preventDefault();
        setPage("records");
        window.setTimeout(() => searchRef.current?.focus(), 0);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [openNew]);

  const content = useMemo(() => {
    if (
      !initialization ||
      initialization.recoveryRequired ||
      !initialization.databaseHealthy
    )
      return null;
    if (page === "records") {
      return (
        <RecordsPage
          records={records}
          total={statistics.summary.recordCount}
          categories={categories}
          summary={statistics.summary}
          filter={filter}
          loading={loading}
          error={dataError}
          searchRef={searchRef}
          onFilterChange={updateFilter}
          onNew={openNew}
          onEdit={openEdit}
          onReload={refresh}
          onDataChanged={refresh}
          notify={notify}
          academicYears={academicYears}
        />
      );
    }
    if (page === "statistics") {
      return (
        <StatisticsPage
          stats={statistics}
          categories={categories}
          academicYears={academicYears}
          filter={filter}
          loading={loading}
          error={dataError}
          onFilterChange={updateFilter}
          onDrilldown={(patch) => {
            updateFilter({ ...patch, page: 1 });
            setPage("records");
          }}
          onReload={refresh}
        />
      );
    }
    if (page === "trash") {
      return (
        <TrashPage
          records={trash}
          loading={loading}
          error={dataError}
          onReload={refresh}
          onDataChanged={refresh}
          notify={notify}
        />
      );
    }
    return (
      <SettingsPage
        initialization={initialization}
        categories={categories}
        onDataChanged={refresh}
        notify={notify}
      />
    );
  }, [
    academicYears,
    categories,
    dataError,
    filter,
    initialization,
    loading,
    notify,
    openEdit,
    openNew,
    page,
    records,
    refresh,
    statistics,
    trash,
    updateFilter,
  ]);

  if (!initialization) {
    return (
      <FluentProvider theme={theme} className="app-provider">
        <div className="startup-screen">
          <img src={appIcon} alt="" />
          <h1>综测记录</h1>
          {initialError ? (
            <>
              <p>{initialError}</p>
              <Button
                appearance="primary"
                icon={<RefreshCw size={18} />}
                onClick={() => void initialize()}
              >
                重新加载
              </Button>
            </>
          ) : (
            <div className="startup-loader" aria-label="正在初始化本地数据" />
          )}
        </div>
      </FluentProvider>
    );
  }

  if (initialization.recoveryRequired || !initialization.databaseHealthy) {
    return (
      <RecoveryScreen
        initialization={initialization}
        onRecovered={() => void initialize()}
      />
    );
  }

  return (
    <FluentProvider theme={theme} className="app-provider">
      <div className="app-shell">
        <aside className="sidebar">
          <div className="app-brand">
            <img src={appIcon} alt="" />
            <div>
              <strong>综测记录</strong>
              <span>v{initialization.appVersion}</span>
            </div>
          </div>
          <nav aria-label="主导航">
            {NAV_ITEMS.map((item) => {
              const Icon = item.icon;
              return (
                <button
                  key={item.key}
                  className={page === item.key ? "active" : undefined}
                  aria-current={page === item.key ? "page" : undefined}
                  onClick={() => setPage(item.key)}
                >
                  <Icon size={19} strokeWidth={1.8} aria-hidden="true" />
                  <span>{item.label}</span>
                  {item.key === "trash" && trash.length ? (
                    <small>{trash.length}</small>
                  ) : null}
                </button>
              );
            })}
          </nav>
          <div className="sidebar-footer">
            <span
              className={
                initialization.databaseHealthy
                  ? "status-dot healthy"
                  : "status-dot"
              }
            />
            <div>
              <strong>
                {initialization.databaseHealthy
                  ? "本地数据正常"
                  : "需要恢复数据"}
              </strong>
              <span title={initialization.storageRoot}>
                {initialization.storageRoot}
              </span>
            </div>
          </div>
        </aside>
        <main className="workspace">{content}</main>
        {page === "records" ? (
          <Button
            className="compact-new-button"
            appearance="primary"
            icon={<Plus size={18} />}
            aria-label="新增记录"
            onClick={openNew}
          />
        ) : null}
      </div>
      <RecordDrawer
        open={drawerOpen}
        record={editingRecord}
        categories={categories}
        onClose={() => setDrawerOpen(false)}
        onSaved={(record) => {
          setDrawerOpen(false);
          setEditingRecord(record);
          notify("success", "记录已保存", record.name);
          refresh();
        }}
        notify={notify}
      />
      <NoticeViewport
        notices={notices}
        onDismiss={(id) =>
          setNotices((current) => current.filter((item) => item.id !== id))
        }
      />
    </FluentProvider>
  );
}

export default App;
