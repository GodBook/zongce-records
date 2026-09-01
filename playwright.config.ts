import { defineConfig, devices } from "@playwright/test";

const port = 1421;
const baseURL = process.env.PLAYWRIGHT_BASE_URL ?? `http://127.0.0.1:${port}`;

export default defineConfig({
  testDir: "./tests/e2e",
  fullyParallel: true,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: [["list"], ["html", { open: "never" }]],
  use: {
    baseURL,
    trace: "on-first-retry",
    screenshot: "only-on-failure",
    colorScheme: "light",
  },
  projects: [
    {
      name: "desktop-1360",
      use: {
        ...devices["Desktop Chrome"],
        viewport: { width: 1360, height: 860 },
      },
    },
    {
      name: "desktop-minimum",
      use: {
        ...devices["Desktop Chrome"],
        viewport: { width: 1024, height: 720 },
      },
    },
  ],
  webServer: {
    command: `pnpm dev --host 127.0.0.1 --port ${port}`,
    url: baseURL,
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
  },
});
