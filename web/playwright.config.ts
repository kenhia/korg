import { defineConfig, devices } from "@playwright/test";

// E2E runs against a running korg-api that serves the built web bundle.
// Start one (see README) and point KORG_E2E_URL at it; defaults to the
// local preview on :8090.
const baseURL = process.env.KORG_E2E_URL ?? "http://127.0.0.1:8090";

export default defineConfig({
  testDir: "./tests/e2e",
  fullyParallel: true,
  workers: 4,
  retries: 2,
  reporter: "list",
  use: {
    baseURL,
    trace: "on-first-retry",
  },
  webServer: process.env.KORG_E2E_URL
    ? undefined
    : {
        // KORG_IMG_ROOT is set because its default is the container's
        // `/data/images`, which a dev box has neither. Without it the image
        // suite fails on a permission error that says nothing about images.
        command:
          "pnpm build && cd .. && KORG_TIMEZONE=Etc/UTC KORG_WEB_DIR=$PWD/web/build KORG_IMG_ROOT=$PWD/.e2e-images KORG_LISTEN_ADDR=127.0.0.1:8090 cargo run -p korg-api",
        url: "http://127.0.0.1:8090/api/health",
        reuseExistingServer: true,
        timeout: 120_000,
      },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
});
