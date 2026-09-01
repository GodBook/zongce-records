export type AssessmentLevel = "college" | "school" | "provincial" | "national";

export type PageKey = "records" | "statistics" | "trash" | "settings";

export interface Category {
  id: string;
  name: string;
  isActive: boolean;
  isBuiltin: boolean;
  recordCount: number;
  createdAt: string;
  updatedAt: string;
}

export interface Material {
  id: string;
  name: string;
  size: number;
  mimeType: string;
  sha256?: string;
  createdAt: string;
}

export interface MaterialPreview {
  name: string;
  mimeType: string;
  path: string;
  url: string;
}

export interface PendingMaterial {
  clientId: string;
  name: string;
  size: number;
  mimeType: string;
  path?: string;
}

export interface AssessmentRecord {
  id: string;
  revision: number;
  name: string;
  categoryId: string;
  categoryName: string;
  level: AssessmentLevel;
  date: string;
  score: string;
  notes: string;
  materials: Material[];
  createdAt: string;
  updatedAt: string;
  deletedAt: string | null;
  purgeAt: string | null;
}

export interface RecordDraft {
  id: string;
  revision: number;
  name: string;
  categoryId: string;
  level: AssessmentLevel;
  date: string;
  score: string;
  notes: string;
  attachmentIds: string[];
  newAttachments: PendingMaterial[];
}

export type MaterialStatus = "all" | "attached" | "missing";
export type RecordSort = "dateDesc" | "dateAsc" | "scoreDesc" | "updatedDesc";

export interface RecordFilter {
  query: string;
  academicYear: string;
  dateFrom: string;
  dateTo: string;
  categoryId: string;
  level: AssessmentLevel | "all";
  materialStatus: MaterialStatus;
  sort: RecordSort;
  page: number;
  pageSize: number;
  trashedOnly?: boolean;
}

export interface RecordListResult {
  items: AssessmentRecord[];
  total: number;
}

export interface MetricSummary {
  recordCount: number;
  totalScore: string;
  materialCount: number;
  missingMaterialCount: number;
}

export interface ChartDatum {
  key: string;
  label: string;
  count: number;
  score: string;
}

export interface MonthlyDatum {
  month: string;
  count: number;
  score: string;
}

export interface StatisticsResult {
  summary: MetricSummary;
  byLevel: ChartDatum[];
  byCategory: ChartDatum[];
  monthly: MonthlyDatum[];
}

export interface AppInitialization {
  appVersion: string;
  storageRoot: string;
  databaseHealthy: boolean;
  recoveryRequired: boolean;
}

export interface OperationResult {
  success: boolean;
  message: string;
  path?: string;
  affected?: number;
}

export interface ImportRowPreview {
  row: number;
  status: "new" | "update" | "skip" | "duplicate" | "error";
  name: string;
  message: string;
}

export interface ImportPreview {
  token: string;
  fileName: string;
  total: number;
  newCount: number;
  updateCount: number;
  skipCount: number;
  duplicateCount: number;
  errorCount: number;
  rows: ImportRowPreview[];
}

export interface BackupInspection {
  token: string;
  fileName: string;
  createdAt: string;
  appVersion: string;
  recordCount: number;
  materialCount: number;
  totalBytes: number;
  integrityValid: boolean;
}

export type BackupRestoreMode =
  "merge" | "merge_import" | "merge_copy" | "replace";

export interface StorageStatus {
  root: string;
  databaseBytes: number;
  materialBytes: number;
  recoveryPointCount: number;
  writable: boolean;
  availableBytes: number;
}

export interface UpdateInfo {
  available: boolean;
  currentVersion: string;
  version: string;
  publishedAt: string;
  notes: string;
}

export interface ApiProblem {
  code: string;
  message: string;
  details?: unknown;
}
