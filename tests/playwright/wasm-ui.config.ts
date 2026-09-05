import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: './tests',
  testMatch: 'wasm-ui.spec.ts',
  retries: 0,
  use: {
    ...devices['Desktop Chrome'],
    baseURL: 'http://127.0.0.1:3103',
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure'
  },
  webServer: {
    command: '../../examples/wasm-ui-demo/node_modules/.bin/vite --host 127.0.0.1 --port 3103 ../../examples/wasm-ui-demo/dist',
    url: 'http://127.0.0.1:3103',
    reuseExistingServer: false
  }
});
