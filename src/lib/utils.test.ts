import { describe, expect, it } from "vitest";
import { academicYearFor, centsToScore, scoreToCents } from "./utils";

describe("精确分数", () => {
  it("按百分位解析并格式化，不引入浮点误差", () => {
    expect(scoreToCents("0.10") + scoreToCents("0.20")).toBe(30);
    expect(scoreToCents("123.4")).toBe(12_340);
    expect(centsToScore(12_340)).toBe("123.40");
  });

  it("拒绝负数和超过两位小数", () => {
    expect(() => scoreToCents("-1")).toThrow("分数格式无效");
    expect(() => scoreToCents("1.001")).toThrow("分数格式无效");
    expect(() => scoreToCents("999999999999999999999")).toThrow("分数数值过大");
  });
});

describe("学年边界", () => {
  it("8 月属于上一学年，9 月进入新学年", () => {
    expect(academicYearFor("2026-08-31")).toBe("2025-2026");
    expect(academicYearFor("2026-09-01")).toBe("2026-2027");
  });
});
