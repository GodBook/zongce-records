import type { AssessmentLevel, RecordFilter } from "../types";

export const LEVEL_META: Record<
  AssessmentLevel,
  { label: string; shortLabel: string; color: string }
> = {
  college: { label: "院级", shortLabel: "院", color: "#34705a" },
  school: { label: "校级", shortLabel: "校", color: "#2f6da1" },
  provincial: { label: "省级", shortLabel: "省", color: "#a36b18" },
  national: { label: "国家级", shortLabel: "国", color: "#a13d42" },
};

export const DEFAULT_FILTER: RecordFilter = {
  query: "",
  academicYear: "all",
  dateFrom: "",
  dateTo: "",
  categoryId: "all",
  level: "all",
  materialStatus: "all",
  sort: "dateDesc",
  page: 1,
  pageSize: 100,
};

export function createId(): string {
  return (
    globalThis.crypto?.randomUUID?.() ??
    `local-${Date.now()}-${Math.random().toString(16).slice(2)}`
  );
}

export function todayLocal(): string {
  const now = new Date();
  const offset = now.getTimezoneOffset() * 60_000;
  return new Date(now.getTime() - offset).toISOString().slice(0, 10);
}

export function academicYearFor(date: string): string {
  const [year, month] = date.split("-").map(Number);
  const start = month >= 9 ? year : year - 1;
  return `${start}-${start + 1}`;
}

export function currentAcademicYear(): string {
  return academicYearFor(todayLocal());
}

export function academicYearOptions(records: { date: string }[]): string[] {
  const years = new Set(records.map((record) => academicYearFor(record.date)));
  years.add(currentAcademicYear());
  return [...years].sort((a, b) => b.localeCompare(a));
}

export function scoreToCents(value: string): number {
  const normalized = value.trim();
  if (!/^\d+(?:\.\d{1,2})?$/.test(normalized)) {
    throw new Error("分数格式无效");
  }
  const [integer, decimal = ""] = normalized.split(".");
  const cents = Number(integer) * 100 + Number(decimal.padEnd(2, "0"));
  if (!Number.isSafeInteger(cents)) {
    throw new Error("分数数值过大");
  }
  return cents;
}

export function centsToScore(cents: number): string {
  const sign = cents < 0 ? "-" : "";
  const absolute = Math.abs(Math.round(cents));
  const integer = Math.floor(absolute / 100);
  const decimal = String(absolute % 100).padStart(2, "0");
  return `${sign}${integer}.${decimal}`;
}

export function formatScore(value: string): string {
  try {
    return new Intl.NumberFormat("zh-CN", {
      minimumFractionDigits: 2,
      maximumFractionDigits: 2,
    }).format(scoreToCents(value) / 100);
  } catch {
    return value;
  }
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
}

export function formatDateTime(value: string): string {
  if (!value) return "";
  return new Intl.DateTimeFormat("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value));
}

export function fileNameFromPath(path: string): string {
  return path.split(/[\\/]/).pop() || "未命名文件";
}

export function mimeFromName(name: string): string {
  const extension = name.split(".").pop()?.toLowerCase();
  const known: Record<string, string> = {
    pdf: "application/pdf",
    png: "image/png",
    jpg: "image/jpeg",
    jpeg: "image/jpeg",
    webp: "image/webp",
    doc: "application/msword",
    docx: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    xls: "application/vnd.ms-excel",
    xlsx: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    zip: "application/zip",
  };
  return known[extension ?? ""] ?? "application/octet-stream";
}

export function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  if (error && typeof error === "object" && "message" in error) {
    return String((error as { message: unknown }).message);
  }
  return "操作失败，请稍后重试";
}
