import { expect, test, type Page, type TestInfo } from "@playwright/test";

async function openApp(page: Page) {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.addInitScript(() => {
    try {
      localStorage.clear();
    } catch {
      // data: 预览框架没有可用的本地存储。
    }
  });
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "综测记录" })).toBeVisible();
  await expect(page.getByText("国家级大学生创新训练项目结题")).toBeVisible();
}

async function expectNoPageOverflow(page: Page) {
  const dimensions = await page.evaluate(() => ({
    body: document.body.scrollWidth,
    document: document.documentElement.scrollWidth,
    viewport: window.innerWidth,
    workspace: document.querySelector<HTMLElement>(".workspace")?.scrollWidth,
    workspaceClient:
      document.querySelector<HTMLElement>(".workspace")?.clientWidth,
  }));
  expect(dimensions.body).toBeLessThanOrEqual(dimensions.viewport);
  expect(dimensions.document).toBeLessThanOrEqual(dimensions.viewport);
  expect(dimensions.workspace).toBeLessThanOrEqual(
    dimensions.workspaceClient ?? dimensions.viewport,
  );
}

async function expectWithinViewport(page: Page, selector: string) {
  await expect
    .poll(async () =>
      page.locator(selector).evaluate((element) => {
        const box = element.getBoundingClientRect();
        return (
          box.left >= 0 &&
          box.top >= 0 &&
          box.right <= window.innerWidth + 1 &&
          box.bottom <= window.innerHeight + 1
        );
      }),
    )
    .toBe(true);
}

async function attachScreenshot(page: Page, testInfo: TestInfo, name: string) {
  await testInfo.attach(`${name}-${testInfo.project.name}`, {
    body: await page.screenshot({ animations: "disabled" }),
    contentType: "image/png",
  });
}

test.beforeEach(async ({ page }) => {
  await openApp(page);
});

test("记录页和编辑抽屉在目标视口内完整显示", async ({ page }, testInfo) => {
  await expectNoPageOverflow(page);
  await attachScreenshot(page, testInfo, "记录页");

  await page
    .getByRole("button", {
      name: "查看 国家级大学生创新训练项目结题",
    })
    .click();

  await expect(page.getByText("编辑综测记录")).toBeVisible();
  await expect(page.getByLabel("活动名称")).toHaveValue(
    "国家级大学生创新训练项目结题",
  );
  await expect(page.getByLabel("活动日期")).toHaveValue("2026-08-16");
  await expect(page.getByLabel("综测分数")).toHaveValue("12.50");

  const drawer = page.locator(".record-drawer");
  await expect(drawer).toBeVisible();
  await expect
    .poll(async () =>
      drawer.evaluate((element) => {
        const box = element.getBoundingClientRect();
        return box.left >= 0 && box.right <= window.innerWidth + 1;
      }),
    )
    .toBe(true);
  await attachScreenshot(page, testInfo, "编辑抽屉");

  await page
    .getByRole("button", {
      name: "预览 全国大学生创新项目结题证书.pdf",
    })
    .click();
  const previewDialog = page.getByRole("dialog", { name: "证明材料预览" });
  await expect(previewDialog).toBeVisible();
  await expect(
    previewDialog.getByTitle("预览 全国大学生创新项目结题证书.pdf"),
  ).toBeVisible();
  await attachScreenshot(page, testInfo, "材料预览");
});

test("统计图表完成绘制且画布非空", async ({ page }, testInfo) => {
  await page.getByRole("button", { name: "统计" }).click();
  await expect(page.getByRole("heading", { name: "统计分析" })).toBeVisible();
  await expect(page.locator(".chart-panel canvas")).toHaveCount(3);

  await expect
    .poll(async () =>
      page.locator(".chart-panel canvas").evaluateAll((canvases) =>
        canvases.every((canvas) => {
          const context = canvas.getContext("2d");
          if (!context || canvas.width === 0 || canvas.height === 0)
            return false;
          const pixels = context.getImageData(
            0,
            0,
            canvas.width,
            canvas.height,
          ).data;
          for (let index = 0; index < pixels.length; index += 16) {
            if (
              pixels[index + 3] > 0 &&
              (pixels[index] < 245 ||
                pixels[index + 1] < 245 ||
                pixels[index + 2] < 245)
            ) {
              return true;
            }
          }
          return false;
        }),
      ),
    )
    .toBe(true);

  await expectNoPageOverflow(page);
  await attachScreenshot(page, testInfo, "统计页");
});

test("设置标签页在最小窗口下保持可用", async ({ page }, testInfo) => {
  await page.getByRole("button", { name: "设置" }).click();
  await expect(page.getByRole("heading", { name: "设置" })).toBeVisible();

  await page.getByRole("tab", { name: "活动类别" }).click();
  await expect(page.getByRole("heading", { name: "活动类别" })).toBeVisible();

  await page.getByRole("tab", { name: "备份与存储" }).click();
  await expect(page.getByRole("button", { name: "创建备份" })).toBeVisible();
  await expect(
    page.getByRole("button", { name: "迁移数据位置" }),
  ).toBeVisible();

  await page.getByRole("tab", { name: "软件更新" }).click();
  await expect(page.getByRole("button", { name: "检查更新" })).toBeVisible();

  await expectNoPageOverflow(page);
  await attachScreenshot(page, testInfo, "设置页");
});

test("记录分页可往返且工作区不产生横向溢出", async ({ page }, testInfo) => {
  await page.evaluate(() => {
    const storageKey = "zongce-records.browser-mock.v1";
    const raw = localStorage.getItem(storageKey);
    if (!raw) throw new Error("浏览器演示数据未初始化");
    const state = JSON.parse(raw) as {
      records: Array<Record<string, unknown>>;
    };
    const template = state.records[0];
    const timestamp = new Date().toISOString();
    for (let index = 1; index <= 25; index += 1) {
      state.records.push({
        ...template,
        id: `pagination-record-${index}`,
        name: `分页测试记录 ${String(index).padStart(2, "0")}`,
        date: `2025-10-${String((index % 28) + 1).padStart(2, "0")}`,
        createdAt: timestamp,
        updatedAt: timestamp,
        materials: [],
      });
    }
    localStorage.setItem(storageKey, JSON.stringify(state));
  });

  await page.getByLabel("每页条数").selectOption("25");
  await expect(page.getByText("第 1 页，共 2 页（31 条）")).toBeVisible();
  await page.getByRole("button", { name: "下一页" }).click();
  await expect(page.getByText("第 2 页，共 2 页（31 条）")).toBeVisible();
  await expect(page.locator("tbody tr")).toHaveCount(6);
  await expect(page.getByRole("button", { name: "下一页" })).toBeDisabled();
  await page.getByRole("button", { name: "上一页" }).click();
  await expect(page.getByText("第 1 页，共 2 页（31 条）")).toBeVisible();
  await expect(page.locator("tbody tr")).toHaveCount(25);

  await expectNoPageOverflow(page);
  await attachScreenshot(page, testInfo, "记录分页");
});

test("统计筛选与记录页共享且月度图表可下钻", async ({ page }) => {
  await page.getByRole("button", { name: "统计" }).click();
  await page.getByLabel("统计材料状态").selectOption("missing");
  await expect(
    page.locator(".stats-kpis .kpi-item").filter({ hasText: "记录总数" }),
  ).toContainText("2");

  await page.getByRole("button", { name: "记录" }).click();
  await expect(page.getByLabel("筛选材料状态")).toHaveValue("missing");
  await expect(page.locator("tbody tr")).toHaveCount(2);

  await page.getByRole("button", { name: "统计" }).click();
  await page.getByRole("button", { name: "清除统计筛选" }).click();
  const trendCanvas = page.locator(".trend-panel canvas");
  await expect(trendCanvas).toBeVisible();
  const box = await trendCanvas.boundingBox();
  expect(box).not.toBeNull();
  await trendCanvas.click({
    position: {
      x: 44,
      y: 20 + ((box?.height ?? 246) - 56) * (14 / 15),
    },
  });
  await expect(page.getByRole("heading", { name: "综测记录" })).toBeVisible();
  await expect(page.getByLabel("开始日期")).toHaveValue("2025-09-01");
  await expect(page.getByLabel("结束日期")).toHaveValue("2025-09-30");
  await expect(page.getByText("学院迎新晚会节目组织")).toBeVisible();
  await expectNoPageOverflow(page);
});

test("Excel 疑似重复项选择在导入前明确可控", async ({ page }, testInfo) => {
  await page.getByRole("button", { name: "设置" }).click();
  await page.getByRole("button", { name: "选择文件" }).click();

  const dialog = page.getByRole("dialog", { name: "确认 Excel 导入" });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByText("1 疑似重复")).toBeVisible();
  const duplicateSwitch = dialog.getByRole("switch", {
    name: "将疑似重复行作为新记录导入",
  });
  await expect(duplicateSwitch).not.toBeChecked();
  await expect(dialog.getByRole("button", { name: "确认导入" })).toBeVisible();

  await duplicateSwitch.check();
  await expect(duplicateSwitch).toBeChecked();
  await expect(
    dialog.getByRole("button", { name: "确认并导入重复项" }),
  ).toBeVisible();
  await expectWithinViewport(page, ".import-dialog");
  await attachScreenshot(page, testInfo, "Excel 导入重复项");
});

test("备份恢复展示全部冲突策略并二次确认替换", async ({ page }, testInfo) => {
  await page.getByRole("button", { name: "设置" }).click();
  await page.getByRole("tab", { name: "备份与存储" }).click();
  await page.getByRole("button", { name: "选择备份" }).click();

  const dialog = page.getByRole("dialog", { name: "恢复完整备份" });
  await expect(dialog).toBeVisible();
  const restoreMode = dialog.getByLabel("恢复方式");
  await expect(restoreMode.locator("option")).toHaveCount(4);

  await restoreMode.selectOption("merge_import");
  await expect(dialog.getByText("冲突时采用备份中的字段和附件")).toBeVisible();
  await restoreMode.selectOption("merge_copy");
  await expect(dialog.getByText("作为独立副本导入")).toBeVisible();
  await restoreMode.selectOption("replace");
  await expect(dialog.getByText("由备份内容原子替换")).toBeVisible();
  await expectWithinViewport(page, ".backup-dialog");
  await attachScreenshot(page, testInfo, "备份恢复策略");

  await dialog.getByRole("button", { name: "开始恢复" }).click();
  const confirm = page.getByRole("dialog", { name: "确认替换当前数据" });
  await expect(confirm).toBeVisible();
  await expect(confirm.getByRole("button", { name: "确认替换" })).toBeVisible();
  await confirm.getByRole("button", { name: "取消" }).click();
});
