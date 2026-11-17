import { defineConfig } from "@playwright/test";
import path from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";

const webDir = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(webDir, "..");
const apiToken = "playwright-api-token-0123456789-abcdef";
const supervisorToken = "playwright-supervisor-token-0123456789-abcdef";

export default defineConfig({
  testDir: "./e2e",
  timeout: 45_000,
  expect: { timeout: 10_000 },
  fullyParallel: false,
  workers: 1,
  reporter: "list",
  use: {
    baseURL: "http://127.0.0.1:5173",
    browserName: "chromium",
    headless: true,
  },
  webServer: [
    {
      command: "python3 scripts/mock_gpu_services.py",
      cwd: root,
      url: "http://127.0.0.1:11500/health",
      timeout: 30_000,
      reuseExistingServer: false,
      env: { E2E_SUPERVISOR_TOKEN: supervisorToken },
    },
    {
      command: "target/release/dread-universal",
      cwd: root,
      url: "http://127.0.0.1:8765/health",
      timeout: 45_000,
      reuseExistingServer: false,
      env: {
        DREAD_BIND: "127.0.0.1:8765",
        DREAD_API_TOKEN: apiToken,
        DREAD_OPERATOR_ID: "playwright-ci",
        DREAD_DB_PATH: path.join(tmpdir(), `dread-universal-e2e-${process.pid}.sqlite3`),
        DREAD_GPU_SUPERVISOR_URL: "http://127.0.0.1:11502",
        DREAD_GPU_SUPERVISOR_TOKEN: supervisorToken,
      },
    },
    {
      command: "npm run dev -- --host 127.0.0.1 --port 5173 --strictPort",
      cwd: webDir,
      url: "http://127.0.0.1:5173",
      timeout: 30_000,
      reuseExistingServer: false,
    },
  ],
});
