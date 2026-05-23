import { defineConfig, devices } from '@playwright/test';

const restPort = process.env.PLAYWRIGHT_REST_PORT ?? '3101';
const jsonrpcPort = process.env.PLAYWRIGHT_JSONRPC_PORT ?? '3102';

export default defineConfig({
  testDir: './tests',
  fullyParallel: false,
  retries: process.env.CI ? 2 : 0,
  reporter: process.env.CI ? [['github'], ['html', { open: 'never' }]] : [['list'], ['html', { open: 'never' }]],
  use: {
    baseURL: 'http://127.0.0.1',
    launchOptions: {
      slowMo: Number(process.env.PLAYWRIGHT_SLOW_MO ?? 0)
    },
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    video: 'retain-on-failure'
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] }
    }
  ],
  webServer: [
    {
      command: `PLAYWRIGHT_REST_ADDR=127.0.0.1:${restPort} cargo run --locked -p playwright-rest-fixture`,
      url: `http://127.0.0.1:${restPort}/api/v1/docs/openapi.json`,
      reuseExistingServer: !process.env.CI,
      timeout: 240_000
    },
    {
      command: `PLAYWRIGHT_JSONRPC_ADDR=127.0.0.1:${jsonrpcPort} cargo run --locked -p playwright-jsonrpc-fixture`,
      url: `http://127.0.0.1:${jsonrpcPort}/rpc/explorer/openrpc.json`,
      reuseExistingServer: !process.env.CI,
      timeout: 240_000
    }
  ]
});
