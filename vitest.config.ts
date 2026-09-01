import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import path from "node:path";

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      tabster: path.resolve("node_modules/tabster/dist/esm/index.js"),
    },
  },
  ssr: {
    noExternal: ["tabster", "keyborg", /^@fluentui\//],
  },
  test: {
    environment: "jsdom",
    globals: true,
    include: ["src/**/*.test.{ts,tsx}"],
    setupFiles: ["./src/test/setup.ts"],
    css: true,
    coverage: {
      reporter: ["text", "html"],
    },
    server: {
      deps: {
        inline: true,
      },
    },
  },
});
