import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/browser",
  timeout: 30_000,
  workers: 2,
  use: {
    baseURL: "http://127.0.0.1:5179",
    viewport: { width: 1100, height: 820 },
    channel: process.env.PLAYWRIGHT_CHANNEL || undefined,
    screenshot: "only-on-failure",
    trace: "retain-on-failure",
  },
  webServer: {
    command:
      "node node_modules/vite/bin/vite.js --host 127.0.0.1 --port 5179 --strictPort",
    url: "http://127.0.0.1:5179",
    reuseExistingServer: !process.env.CI,
  },
});
