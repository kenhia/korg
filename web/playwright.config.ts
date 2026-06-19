import { defineConfig, devices } from "@playwright/test";

// E2E runs against a running korg-api that serves the built web bundle.
// Start one (see README) and point KORG_E2E_URL at it; defaults to the
// local preview on :8090.
const baseURL = process.env.KORG_E2E_URL ?? "http://127.0.0.1:8090";

export default defineConfig({
  testDir: "./tests/e2e",
  fullyParallel: true,
  retries: 2,
  reporter: "list",
  use: {
    baseURL,
    trace: "on-first-retry",
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
});
