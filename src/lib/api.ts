import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { relaunch } from "@tauri-apps/plugin-process";
import {
  check,
  type DownloadEvent,
  type Update,
} from "@tauri-apps/plugin-updater";
import type {
  AppInitialization,
  AssessmentRecord,
  BackupInspection,
  BackupRestoreMode,
  Category,
  ChartDatum,
  ImportPreview,
  Material,
  MaterialPreview,
  MonthlyDatum,
  OperationResult,
  PendingMaterial,
  RecordDraft,
  RecordFilter,
  RecordListResult,
  StatisticsResult,
  StorageStatus,
  UpdateInfo,
} from "../types";
import {
  LEVEL_META,
  academicYearFor,
  centsToScore,
  createId,
  fileNameFromPath,
  mimeFromName,
  scoreToCents,
} from "./utils";

const MOCK_STORAGE_KEY = "zongce-records.browser-mock.v1";
const MAX_MATERIAL_BYTES = 200 * 1024 * 1024;
let pendingUpdate: Update | null = null;

interface MockState {
  categories: Category[];
  records: AssessmentRecord[];
  storageRoot: string;
  committedImports: string[];
}

export class ApiError extends Error {
  code: string;
  details?: unknown;

  constructor(code: string, message: string, details?: unknown) {
    super(message);
    this.name = "ApiError";
    this.code = code;
    this.details = details;
  }
}

function inTauri(): boolean {
  if (typeof window === "undefined") return false;
  const candidate = window as unknown as Record<string, unknown>;
  return "__TAURI_INTERNALS__" in candidate || "__TAURI__" in candidate;
}

function toApiError(error: unknown): ApiError {
  if (error instanceof ApiError) return error;
  if (typeof error === "string") return new ApiError("IPC_ERROR", error);
  if (error && typeof error === "object") {
    const value = error as {
      code?: unknown;
      message?: unknown;
      details?: unknown;
    };
    return new ApiError(
      typeof value.code === "string" ? value.code : "IPC_ERROR",
      typeof value.message === "string" ? value.message : "后端操作失败",
      value.details,
    );
  }
  return new ApiError("UNKNOWN", "发生未知错误");
}

async function call<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw toApiError(error);
  }
}

const initialCategories = [
  "学科竞赛",
  "科研创新",
  "社会实践",
  "志愿服务",
  "文体活动",
  "学生工作",
  "荣誉表彰",
  "其他",
];

function createMaterial(
  name: string,
  size: number,
  mimeType: string,
): Material {
  return {
    id: createId(),
    name,
    size,
    mimeType,
    createdAt: new Date().toISOString(),
  };
}

function createInitialMockState(): MockState {
  const now = new Date().toISOString();
  const categories = initialCategories.map((name, index) => ({
    id: `category-${index + 1}`,
    name,
    isActive: true,
    isBuiltin: true,
    recordCount: 0,
    createdAt: now,
    updatedAt: now,
  }));
  const categoryId = (name: string) =>
    categories.find((category) => category.name === name)?.id ??
    categories[7].id;
  const evidence1 = createMaterial(
    "全国大学生创新项目结题证书.pdf",
    1_842_304,
    "application/pdf",
  );
  const evidence2 = createMaterial(
    "蓝桥杯省赛获奖证书.png",
    892_114,
    "image/png",
  );
  const evidence3 = createMaterial(
    "志愿服务时长证明.pdf",
    442_910,
    "application/pdf",
  );
  const evidence4 = createMaterial(
    "优秀学生干部证书.jpg",
    1_238_210,
    "image/jpeg",
  );
  const rows: Array<
    Omit<
      AssessmentRecord,
      "id" | "revision" | "createdAt" | "updatedAt" | "deletedAt" | "purgeAt"
    >
  > = [
    {
      name: "国家级大学生创新训练项目结题",
      categoryId: categoryId("科研创新"),
      categoryName: "科研创新",
      level: "national",
      date: "2026-08-16",
      score: "12.50",
      notes: "项目按期完成结题，负责人。",
      materials: [evidence1],
    },
    {
      name: "蓝桥杯程序设计省赛二等奖",
      categoryId: categoryId("学科竞赛"),
      categoryName: "学科竞赛",
      level: "provincial",
      date: "2026-06-05",
      score: "8.00",
      notes: "软件赛道 C/C++ 组。",
      materials: [evidence2],
    },
    {
      name: "校青年志愿服务先进个人",
      categoryId: categoryId("志愿服务"),
      categoryName: "志愿服务",
      level: "school",
      date: "2026-05-18",
      score: "3.50",
      notes: "学年志愿服务累计 86 小时。",
      materials: [evidence3],
    },
    {
      name: "学院学业帮扶计划",
      categoryId: categoryId("社会实践"),
      categoryName: "社会实践",
      level: "college",
      date: "2026-03-22",
      score: "1.50",
      notes: "负责高等数学答疑。",
      materials: [],
    },
    {
      name: "校优秀学生干部",
      categoryId: categoryId("学生工作"),
      categoryName: "学生工作",
      level: "school",
      date: "2025-12-12",
      score: "5.00",
      notes: "担任班级学习委员。",
      materials: [evidence4],
    },
    {
      name: "学院迎新晚会节目组织",
      categoryId: categoryId("文体活动"),
      categoryName: "文体活动",
      level: "college",
      date: "2025-09-21",
      score: "1.00",
      notes: "负责节目统筹与现场协调。",
      materials: [],
    },
  ];
  const records = rows.map((row, index) => ({
    ...row,
    id: `demo-record-${index + 1}`,
    revision: 1,
    createdAt: now,
    updatedAt: now,
    deletedAt: null,
    purgeAt: null,
  }));
  return {
    categories,
    records,
    storageRoot: "浏览器演示数据 / localStorage",
    committedImports: [],
  };
}

function readMockState(): MockState {
  if (typeof localStorage === "undefined") return createInitialMockState();
  const stored = localStorage.getItem(MOCK_STORAGE_KEY);
  if (stored) {
    try {
      return JSON.parse(stored) as MockState;
    } catch {
      localStorage.removeItem(MOCK_STORAGE_KEY);
    }
  }
  const initial = createInitialMockState();
  writeMockState(initial);
  return initial;
}

function writeMockState(state: MockState): void {
  if (typeof localStorage !== "undefined") {
    localStorage.setItem(MOCK_STORAGE_KEY, JSON.stringify(state));
  }
}

function delay<T>(value: T, duration = 90): Promise<T> {
  return new Promise((resolve) =>
    window.setTimeout(() => resolve(value), duration),
  );
}

function categoryCounts(state: MockState): Map<string, number> {
  const counts = new Map<string, number>();
  state.records
    .filter((record) => !record.deletedAt)
    .forEach((record) =>
      counts.set(record.categoryId, (counts.get(record.categoryId) ?? 0) + 1),
    );
  return counts;
}

function listMockRecords(filter: RecordFilter): RecordListResult {
  const state = readMockState();
  const query = filter.query.trim().toLocaleLowerCase("zh-CN");
  const items = state.records.filter((record) => {
    if (filter.trashedOnly ? !record.deletedAt : Boolean(record.deletedAt))
      return false;
    if (
      query &&
      ![record.name, record.notes, ...record.materials.map((item) => item.name)]
        .join(" ")
        .toLocaleLowerCase("zh-CN")
        .includes(query)
    ) {
      return false;
    }
    if (
      filter.academicYear !== "all" &&
      academicYearFor(record.date) !== filter.academicYear
    ) {
      return false;
    }
    if (filter.dateFrom && record.date < filter.dateFrom) return false;
    if (filter.dateTo && record.date > filter.dateTo) return false;
    if (filter.categoryId !== "all" && record.categoryId !== filter.categoryId)
      return false;
    if (filter.level !== "all" && record.level !== filter.level) return false;
    if (filter.materialStatus === "attached" && record.materials.length === 0)
      return false;
    if (filter.materialStatus === "missing" && record.materials.length > 0)
      return false;
    return true;
  });
  items.sort((a, b) => {
    if (filter.sort === "dateAsc") return a.date.localeCompare(b.date);
    if (filter.sort === "scoreDesc")
      return scoreToCents(b.score) - scoreToCents(a.score);
    if (filter.sort === "updatedDesc")
      return b.updatedAt.localeCompare(a.updatedAt);
    return b.date.localeCompare(a.date);
  });
  const start = Math.max(0, (filter.page - 1) * filter.pageSize);
  return {
    items: items.slice(start, start + filter.pageSize),
    total: items.length,
  };
}

function sumScores(records: AssessmentRecord[]): string {
  return centsToScore(
    records.reduce((sum, record) => sum + scoreToCents(record.score), 0),
  );
}

function groupStatistics(
  records: AssessmentRecord[],
  keySelector: (record: AssessmentRecord) => string,
  labelSelector: (key: string) => string,
): ChartDatum[] {
  const groups = new Map<string, AssessmentRecord[]>();
  records.forEach((record) => {
    const key = keySelector(record);
    groups.set(key, [...(groups.get(key) ?? []), record]);
  });
  return [...groups.entries()].map(([key, rows]) => ({
    key,
    label: labelSelector(key),
    count: rows.length,
    score: sumScores(rows),
  }));
}

function mockStatistics(filter: RecordFilter): StatisticsResult {
  const all = listMockRecords({
    ...filter,
    page: 1,
    pageSize: Number.MAX_SAFE_INTEGER,
  }).items;
  const categories = readMockState().categories;
  const byLevel = (
    Object.keys(LEVEL_META) as Array<keyof typeof LEVEL_META>
  ).map((level) => {
    const rows = all.filter((record) => record.level === level);
    return {
      key: level,
      label: LEVEL_META[level].label,
      count: rows.length,
      score: sumScores(rows),
    };
  });
  const byCategory = groupStatistics(
    all,
    (record) => record.categoryId,
    (key) =>
      categories.find((category) => category.id === key)?.name ?? "已停用类别",
  ).sort((a, b) => scoreToCents(b.score) - scoreToCents(a.score));
  const monthGroups = new Map<string, AssessmentRecord[]>();
  all.forEach((record) => {
    const month = record.date.slice(0, 7);
    monthGroups.set(month, [...(monthGroups.get(month) ?? []), record]);
  });
  const monthly: MonthlyDatum[] = [...monthGroups.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([month, rows]) => ({
      month,
      count: rows.length,
      score: sumScores(rows),
    }));
  return {
    summary: {
      recordCount: all.length,
      totalScore: sumScores(all),
      materialCount: all.reduce(
        (sum, record) => sum + record.materials.length,
        0,
      ),
      missingMaterialCount: all.filter(
        (record) => record.materials.length === 0,
      ).length,
    },
    byLevel,
    byCategory,
    monthly,
  };
}

function normalizeRecordList(
  value: RecordListResult | AssessmentRecord[],
): RecordListResult {
  return Array.isArray(value) ? { items: value, total: value.length } : value;
}

async function selectSingleFile(
  title: string,
  extensions: string[],
): Promise<string | null> {
  if (!inTauri()) return `C:\\演示文件\\${title}.${extensions[0]}`;
  const selected = await open({
    title,
    multiple: false,
    directory: false,
    filters: [{ name: title, extensions }],
  });
  return typeof selected === "string" ? selected : null;
}

async function selectSaveFile(
  title: string,
  defaultPath: string,
  extension: string,
): Promise<string | null> {
  if (!inTauri()) return `C:\\演示文件\\${defaultPath}`;
  const selected = await save({
    title,
    defaultPath,
    filters: [{ name: title, extensions: [extension] }],
  });
  return typeof selected === "string" ? selected : null;
}

export const api = {
  isTauri: inTauri,

  async initializeApp(): Promise<AppInitialization> {
    if (inTauri()) return call<AppInitialization>("initialize_app");
    const state = readMockState();
    return delay({
      appVersion: "0.1.0",
      storageRoot: state.storageRoot,
      databaseHealthy: true,
      recoveryRequired: false,
    });
  },

  async listRecords(filter: RecordFilter): Promise<RecordListResult> {
    if (inTauri()) {
      const result = await call<RecordListResult | AssessmentRecord[]>(
        "list_records",
        { filter },
      );
      return normalizeRecordList(result);
    }
    return delay(listMockRecords(filter));
  },

  async listAcademicYears(): Promise<string[]> {
    if (inTauri()) return call<string[]>("list_academic_years");
    const years = new Set(
      readMockState()
        .records.filter((record) => !record.deletedAt)
        .map((record) => academicYearFor(record.date)),
    );
    return delay([...years].sort((a, b) => b.localeCompare(a)));
  },

  async getRecord(id: string): Promise<AssessmentRecord> {
    if (inTauri()) return call<AssessmentRecord>("get_record", { id });
    const record = readMockState().records.find((item) => item.id === id);
    if (!record) throw new ApiError("NOT_FOUND", "记录不存在或已被删除");
    return delay(record);
  },

  async saveRecord(draft: RecordDraft): Promise<AssessmentRecord> {
    if (inTauri()) return call<AssessmentRecord>("save_record", { draft });
    const state = readMockState();
    const category = state.categories.find(
      (item) => item.id === draft.categoryId,
    );
    if (!category)
      throw new ApiError("CATEGORY_NOT_FOUND", "所选活动类别不存在");
    const existingIndex = state.records.findIndex(
      (record) => record.id === draft.id,
    );
    const existing =
      existingIndex >= 0 ? state.records[existingIndex] : undefined;
    if (existing && existing.revision !== draft.revision) {
      throw new ApiError(
        "REVISION_CONFLICT",
        "记录已在其他窗口中更新，请重新打开后再保存",
      );
    }
    const retained =
      existing?.materials.filter((material) =>
        draft.attachmentIds.includes(material.id),
      ) ?? [];
    const created = draft.newAttachments.map((material) =>
      createMaterial(material.name, material.size, material.mimeType),
    );
    const now = new Date().toISOString();
    const record: AssessmentRecord = {
      id: draft.id,
      revision: existing ? existing.revision + 1 : 1,
      name: draft.name.trim(),
      categoryId: draft.categoryId,
      categoryName: category.name,
      level: draft.level,
      date: draft.date,
      score: centsToScore(scoreToCents(draft.score)),
      notes: draft.notes.trim(),
      materials: [...retained, ...created],
      createdAt: existing?.createdAt ?? now,
      updatedAt: now,
      deletedAt: null,
      purgeAt: null,
    };
    if (existingIndex >= 0) state.records[existingIndex] = record;
    else state.records.push(record);
    writeMockState(state);
    return delay(record);
  },

  async moveRecordsToTrash(ids: string[]): Promise<OperationResult> {
    if (inTauri())
      return call<OperationResult>("move_records_to_trash", { ids });
    const state = readMockState();
    const deletedAt = new Date();
    const purgeAt = new Date(deletedAt.getTime() + 30 * 24 * 60 * 60 * 1000);
    let affected = 0;
    state.records = state.records.map((record) => {
      if (!ids.includes(record.id) || record.deletedAt) return record;
      affected += 1;
      return {
        ...record,
        revision: record.revision + 1,
        deletedAt: deletedAt.toISOString(),
        purgeAt: purgeAt.toISOString(),
        updatedAt: deletedAt.toISOString(),
      };
    });
    writeMockState(state);
    return delay({
      success: true,
      message: `已将 ${affected} 条记录移入回收站`,
      affected,
    });
  },

  async restoreRecords(ids: string[]): Promise<OperationResult> {
    if (inTauri()) return call<OperationResult>("restore_records", { ids });
    const state = readMockState();
    let affected = 0;
    state.records = state.records.map((record) => {
      if (!ids.includes(record.id) || !record.deletedAt) return record;
      affected += 1;
      return {
        ...record,
        revision: record.revision + 1,
        deletedAt: null,
        purgeAt: null,
        updatedAt: new Date().toISOString(),
      };
    });
    writeMockState(state);
    return delay({
      success: true,
      message: `已恢复 ${affected} 条记录`,
      affected,
    });
  },

  async permanentlyDeleteRecords(ids: string[]): Promise<OperationResult> {
    if (inTauri())
      return call<OperationResult>("permanently_delete_records", { ids });
    const state = readMockState();
    const before = state.records.length;
    state.records = state.records.filter((record) => !ids.includes(record.id));
    const affected = before - state.records.length;
    writeMockState(state);
    return delay({
      success: true,
      message: `已永久删除 ${affected} 条记录`,
      affected,
    });
  },

  async listCategories(): Promise<Category[]> {
    if (inTauri()) return call<Category[]>("list_categories");
    const state = readMockState();
    const counts = categoryCounts(state);
    return delay(
      state.categories.map((category) => ({
        ...category,
        recordCount: counts.get(category.id) ?? 0,
      })),
    );
  },

  async saveCategory(input: { id?: string; name: string }): Promise<Category> {
    if (inTauri()) return call<Category>("save_category", { category: input });
    const state = readMockState();
    const name = input.name.trim();
    if (!name) throw new ApiError("VALIDATION", "类别名称不能为空");
    if (
      state.categories.some(
        (item) => item.name === name && item.id !== input.id,
      )
    ) {
      throw new ApiError("DUPLICATE", "已存在同名类别");
    }
    const now = new Date().toISOString();
    const index = state.categories.findIndex((item) => item.id === input.id);
    const category: Category =
      index >= 0
        ? { ...state.categories[index], name, updatedAt: now }
        : {
            id: input.id ?? createId(),
            name,
            isActive: true,
            isBuiltin: false,
            recordCount: 0,
            createdAt: now,
            updatedAt: now,
          };
    if (index >= 0) {
      const oldName = state.categories[index].name;
      state.categories[index] = category;
      state.records = state.records.map((record) =>
        record.categoryId === category.id
          ? { ...record, categoryName: category.name }
          : record,
      );
      if (oldName !== name) category.updatedAt = now;
    } else {
      state.categories.push(category);
    }
    writeMockState(state);
    return delay(category);
  },

  async setCategoryActive(id: string, isActive: boolean): Promise<Category> {
    if (inTauri())
      return call<Category>("set_category_active", { id, isActive });
    const state = readMockState();
    const index = state.categories.findIndex((item) => item.id === id);
    if (index < 0) throw new ApiError("NOT_FOUND", "类别不存在");
    state.categories[index] = {
      ...state.categories[index],
      isActive,
      updatedAt: new Date().toISOString(),
    };
    writeMockState(state);
    return delay(state.categories[index]);
  },

  async getStatistics(filter: RecordFilter): Promise<StatisticsResult> {
    if (inTauri()) return call<StatisticsResult>("get_statistics", { filter });
    return delay(mockStatistics(filter));
  },

  async exportExcel(filter: RecordFilter): Promise<OperationResult | null> {
    const path = await selectSaveFile(
      "导出综测记录",
      `综测记录_${new Date().toISOString().slice(0, 10)}.xlsx`,
      "xlsx",
    );
    if (!path) return null;
    if (inTauri())
      return call<OperationResult>("export_excel", {
        filter,
        templateOnly: false,
        path,
      });
    return delay(
      { success: true, message: "Excel 已导出", path: "下载 / 综测记录.xlsx" },
      280,
    );
  },

  async exportExcelTemplate(): Promise<OperationResult | null> {
    const path = await selectSaveFile(
      "保存综测记录导入模板",
      "综测记录导入模板.xlsx",
      "xlsx",
    );
    if (!path) return null;
    if (inTauri())
      return call<OperationResult>("export_excel", {
        templateOnly: true,
        path,
      });
    return delay({
      success: true,
      message: "导入模板已保存",
      path: "下载 / 综测记录导入模板.xlsx",
    });
  },

  async previewExcel(): Promise<ImportPreview | null> {
    const path = await selectSingleFile("选择综测记录 Excel", ["xlsx", "xls"]);
    if (!path) return null;
    if (inTauri()) return call<ImportPreview>("preview_excel", { path });
    return delay({
      token: createId(),
      fileName: fileNameFromPath(path),
      total: 5,
      newCount: 3,
      updateCount: 1,
      skipCount: 0,
      duplicateCount: 1,
      errorCount: 0,
      rows: [
        { row: 2, status: "new", name: "学院专业技能竞赛", message: "将新增" },
        {
          row: 3,
          status: "new",
          name: "暑期社会实践优秀个人",
          message: "将新增",
        },
        {
          row: 4,
          status: "update",
          name: "校青年志愿服务先进个人",
          message: "匹配记录 ID",
        },
        {
          row: 5,
          status: "duplicate",
          name: "蓝桥杯程序设计省赛二等奖",
          message: "可能与现有记录重复",
        },
        { row: 6, status: "new", name: "学院辩论赛二等奖", message: "将新增" },
      ],
    });
  },

  async commitExcel(
    token: string,
    includeDuplicates = false,
  ): Promise<OperationResult> {
    if (inTauri())
      return call<OperationResult>("commit_excel", {
        token,
        includeDuplicates,
      });
    const state = readMockState();
    if (state.committedImports.includes(token)) {
      return delay({
        success: true,
        message: "该批次已经导入，无需重复操作",
        affected: 0,
      });
    }
    const category =
      state.categories.find((item) => item.name === "学科竞赛") ??
      state.categories[0];
    const now = new Date().toISOString();
    state.records.push({
      id: createId(),
      revision: 1,
      name: "学院专业技能竞赛",
      categoryId: category.id,
      categoryName: category.name,
      level: "college",
      date: "2026-08-28",
      score: "2.00",
      notes: "由 Excel 导入。",
      materials: [],
      createdAt: now,
      updatedAt: now,
      deletedAt: null,
      purgeAt: null,
    });
    state.committedImports.push(token);
    writeMockState(state);
    return delay(
      { success: true, message: "导入完成，新增 3 条，更新 1 条", affected: 4 },
      320,
    );
  },

  async exportMaterialPackage(
    recordIds: string[],
  ): Promise<OperationResult | null> {
    const path = await selectSaveFile(
      "导出综测提交材料包",
      `综测提交材料包_${new Date().toISOString().slice(0, 10)}.zip`,
      "zip",
    );
    if (!path) return null;
    if (inTauri())
      return call<OperationResult>("export_material_package", {
        recordIds,
        path,
      });
    return delay(
      {
        success: true,
        message: `已导出 ${recordIds.length} 条记录的材料包`,
        path: "下载 / 综测提交材料包.zip",
        affected: recordIds.length,
      },
      320,
    );
  },

  async exportBackup(): Promise<OperationResult | null> {
    const path = await selectSaveFile(
      "创建综测记录完整备份",
      `综测记录完整备份_${new Date().toISOString().slice(0, 10)}.zcbak`,
      "zcbak",
    );
    if (!path) return null;
    if (inTauri()) return call<OperationResult>("export_backup", { path });
    return delay(
      {
        success: true,
        message: "完整备份已创建",
        path: "下载 / 综测记录完整备份.zcbak",
      },
      320,
    );
  },

  async inspectBackup(): Promise<BackupInspection | null> {
    const path = await selectSingleFile("选择综测备份", ["zcbak"]);
    if (!path) return null;
    if (inTauri()) return call<BackupInspection>("inspect_backup", { path });
    const state = readMockState();
    return delay({
      token: createId(),
      fileName: fileNameFromPath(path),
      createdAt: new Date(Date.now() - 86_400_000).toISOString(),
      appVersion: "0.1.0",
      recordCount: state.records.length,
      materialCount: state.records.reduce(
        (sum, item) => sum + item.materials.length,
        0,
      ),
      totalBytes: 8_246_210,
      integrityValid: true,
    });
  },

  async restoreBackup(
    token: string,
    mode: BackupRestoreMode,
  ): Promise<OperationResult> {
    if (inTauri())
      return call<OperationResult>("restore_backup", { token, mode });
    return delay(
      {
        success: true,
        message:
          mode === "replace"
            ? "备份已恢复，旧数据可从恢复点回滚"
            : mode === "merge_import"
              ? "备份已合并，冲突记录采用备份版本"
              : mode === "merge_copy"
                ? "备份已合并，冲突记录已创建副本"
                : "备份已合并，现有记录已保留",
      },
      420,
    );
  },

  async getStorageStatus(): Promise<StorageStatus> {
    if (inTauri()) return call<StorageStatus>("get_storage_status");
    const state = readMockState();
    const materialBytes = state.records.reduce(
      (sum, record) =>
        sum +
        record.materials.reduce((size, material) => size + material.size, 0),
      0,
    );
    return delay({
      root: state.storageRoot,
      databaseBytes: 1_482_752,
      materialBytes,
      recoveryPointCount: 4,
      writable: true,
      availableBytes: 86 * 1024 ** 3,
    });
  },

  async migrateDataRoot(): Promise<OperationResult | null> {
    let destination = "D:\\综测记录数据";
    if (inTauri()) {
      const selected = await open({
        title: "选择新的数据位置",
        directory: true,
        multiple: false,
      });
      if (typeof selected !== "string") return null;
      destination = selected;
      return call<OperationResult>("migrate_data_root", { destination });
    }
    const state = readMockState();
    state.storageRoot = destination;
    writeMockState(state);
    return delay(
      { success: true, message: "数据位置已迁移并完成校验", path: destination },
      420,
    );
  },

  async chooseMaterials(files?: FileList | File[]): Promise<PendingMaterial[]> {
    if (!inTauri()) {
      const browserFiles = Array.from(files ?? []);
      return browserFiles.map((file) => {
        if (file.size > MAX_MATERIAL_BYTES) {
          throw new ApiError("MATERIAL_TOO_LARGE", `${file.name} 超过 200 MB`);
        }
        return {
          clientId: createId(),
          name: file.name,
          size: file.size,
          mimeType: file.type || mimeFromName(file.name),
        };
      });
    }
    const selected = await open({
      title: "选择证明材料",
      multiple: true,
      directory: false,
      filters: [
        {
          name: "证明材料",
          extensions: [
            "pdf",
            "png",
            "jpg",
            "jpeg",
            "webp",
            "doc",
            "docx",
            "xls",
            "xlsx",
            "zip",
          ],
        },
      ],
    });
    const paths = typeof selected === "string" ? [selected] : (selected ?? []);
    return paths.map((path) => ({
      clientId: createId(),
      name: fileNameFromPath(path),
      size: 0,
      mimeType: mimeFromName(path),
      path,
    }));
  },

  async openMaterial(materialId: string): Promise<OperationResult> {
    if (inTauri())
      return call<OperationResult>("open_material", { materialId });
    return delay({
      success: true,
      message: "浏览器演示模式不读取本地材料文件",
    });
  },

  async getMaterialPreview(
    materialId: string,
  ): Promise<MaterialPreview | null> {
    if (!inTauri()) {
      const material = readMockState()
        .records.flatMap((record) => record.materials)
        .find((item) => item.id === materialId);
      if (!material) throw new ApiError("NOT_FOUND", "证明材料不存在");
      if (
        material.mimeType !== "application/pdf" &&
        !material.mimeType.startsWith("image/")
      ) {
        throw new ApiError(
          "PREVIEW_UNSUPPORTED",
          "该文件格式不支持内置预览，请使用系统程序打开",
        );
      }
      const previewMarkup = material.mimeType.startsWith("image/")
        ? `<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="800" viewBox="0 0 1200 800"><rect width="1200" height="800" fill="#f4f7f5"/><rect x="250" y="160" width="700" height="480" rx="6" fill="#ffffff" stroke="#b9c8c1" stroke-width="3"/><circle cx="600" cy="330" r="72" fill="#dcece4"/><path d="M566 330l24 24 50-58" fill="none" stroke="#267454" stroke-width="18" stroke-linecap="round" stroke-linejoin="round"/><text x="600" y="460" text-anchor="middle" font-family="Microsoft YaHei UI, sans-serif" font-size="34" fill="#293630">浏览器演示预览</text><text x="600" y="515" text-anchor="middle" font-family="Microsoft YaHei UI, sans-serif" font-size="22" fill="#728079">桌面版将显示已归档的原始图片</text></svg>`
        : `<!doctype html><html lang="zh-CN"><meta charset="utf-8"><style>html,body{height:100%;margin:0}body{display:grid;place-items:center;color:#293630;background:#f4f7f5;font:16px "Microsoft YaHei UI",sans-serif}.sheet{width:min(620px,70vw);padding:72px;background:#fff;border:1px solid #cfd9d4;text-align:center}.sheet strong{display:block;margin-bottom:14px;font-size:24px}.sheet span{color:#728079}</style><body><div class="sheet"><strong>PDF 浏览器演示预览</strong><span>桌面版将使用 WebView2 内置 PDF 阅读器显示原文件</span></div></body></html>`;
      return delay({
        name: material.name,
        mimeType: material.mimeType,
        path: "",
        url: `data:${material.mimeType.startsWith("image/") ? "image/svg+xml" : "text/html"};charset=utf-8,${encodeURIComponent(previewMarkup)}`,
      });
    }
    const preview = await call<Omit<MaterialPreview, "url">>(
      "get_material_preview",
      {
        materialId,
      },
    );
    return { ...preview, url: convertFileSrc(preview.path) };
  },

  async checkForUpdate(): Promise<UpdateInfo> {
    if (inTauri()) {
      if (pendingUpdate) {
        await pendingUpdate.close();
        pendingUpdate = null;
      }
      const update = await check({ timeout: 8_000 });
      pendingUpdate = update;
      if (!update) {
        const initialization = await call<AppInitialization>("initialize_app");
        return {
          available: false,
          currentVersion: initialization.appVersion,
          version: initialization.appVersion,
          publishedAt: "",
          notes: "当前已经是最新版本。",
        };
      }
      return {
        available: true,
        currentVersion: update.currentVersion,
        version: update.version,
        publishedAt: update.date ?? "",
        notes: update.body ?? "该版本没有发布说明。",
      };
    }
    return delay(
      {
        available: false,
        currentVersion: "0.1.0",
        version: "0.1.0",
        publishedAt: "2026-08-31T00:00:00.000Z",
        notes: "当前已经是最新版本。",
      },
      480,
    );
  },

  async installUpdate(
    onProgress?: (downloaded: number, total?: number) => void,
  ): Promise<void> {
    if (!inTauri()) {
      await delay(undefined, 600);
      return;
    }
    if (!pendingUpdate) {
      pendingUpdate = await check({ timeout: 8_000 });
    }
    if (!pendingUpdate) {
      throw new ApiError("UPDATE_NOT_AVAILABLE", "当前没有可安装的更新");
    }
    let downloaded = 0;
    let total: number | undefined;
    const handleEvent = (event: DownloadEvent) => {
      if (event.event === "Started") {
        downloaded = 0;
        total = event.data.contentLength;
      } else if (event.event === "Progress") {
        downloaded += event.data.chunkLength;
      } else {
        if (total !== undefined) downloaded = total;
      }
      onProgress?.(downloaded, total);
    };
    await pendingUpdate.downloadAndInstall(handleEvent, { timeout: 120_000 });
    pendingUpdate = null;
    await relaunch();
  },

  async openReleaseNotes(version: string): Promise<void> {
    const normalized = version.trim().replace(/^v/i, "");
    if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(normalized)) {
      throw new ApiError("INVALID_VERSION", "发布版本号无效");
    }
    const url = `https://github.com/GodBook/zongce-records/releases/tag/v${encodeURIComponent(normalized)}`;
    if (inTauri()) await openUrl(url);
    else window.open(url, "_blank", "noopener,noreferrer");
  },
};

export function resetBrowserMock(): void {
  if (typeof localStorage !== "undefined")
    localStorage.removeItem(MOCK_STORAGE_KEY);
}
