import { expect, test, type Page } from '@playwright/test';

const RPC_PORT = process.env.PLAYWRIGHT_JSONRPC_PORT ?? '3102';
const RPC_URL = `http://127.0.0.1:${RPC_PORT}/rpc/explorer`;

async function selectMethod(page: Page, name: string) {
  await page.locator('.op').filter({ hasText: name }).click();
}

async function send(page: Page) {
  await page.locator('#send-request').click();
}

test.describe('JSON-RPC API explorer', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(RPC_URL);
    await expect(page.locator('#service-name')).toContainText('ExplorerRpcFixture Explorer');
    await expect(page.locator('#operation-list .op').first()).toBeVisible();
  });

  test('loads in dark mode and renders OpenRPC methods', async ({ page }) => {
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    await expect(page.locator('#service-subtitle')).toContainText('JSON-RPC OpenRPC');
    await expect(page.locator('#operation-list')).toContainText('ping');
    await expect(page.locator('.op').filter({ hasText: 'ping' })).toContainText('Echo a `PingRequest` message.');
    await expect(page.locator('#operation-list')).toContainText('rename_widget.v2');
    await expect(page.locator('#operation-list')).toContainText('rename_widget.v1');
    await expect(page.locator('#operation-list')).toContainText('create_widget');
    await expect(page.locator('#operation-list')).toContainText('current_profile');

    await selectMethod(page, 'ping');
    await expect(page.locator('#operation-description p code')).toContainText('PingRequest');
    await expect(page.locator('#operation-description strong')).toContainText('Use this in tests.');
    await expect(page.locator('#operation-description li')).toContainText(['Confirms list rendering', 'Preserves list items']);
    await expect(page.locator('#operation-description pre code')).toContainText('{"message":"hello"}');
    await expect(page.locator('#operation-description a').filter({ hasText: 'Rust API Stack' })).toHaveAttribute(
      'href',
      'https://github.com/JedimEmO/rust-agent-stack/blob/main/crates/rpc/ras-jsonrpc-macro/README.md'
    );
    const descriptionText = await page.locator('#operation-description').evaluate((el) => el.textContent ?? '');
    expect(descriptionText).toContain('Line one\nLine two');

    await expect(page.locator('#request-form')).toContainText('Params schema');
    await expect(page.locator('#request-form')).toContainText('Request payload for the ping method.');
    await expect(page.locator('#request-form .schema-desc strong')).toContainText('Schema docs');
    await expect(page.locator('#request-form')).toContainText('Message echoed by the fixture service.');
    const messageDocText = await page
      .locator('.schema-field')
      .filter({ hasText: 'Message echoed by the fixture service.' })
      .locator('.schema-field-desc')
      .first()
      .evaluate((el) => el.textContent ?? '');
    expect(messageDocText).toContain('Message echoed by the fixture service.\nThis line must stay on a new line.');
    await expect(page.locator('#request-form')).toContainText('Result schema');
    await expect(page.locator('#request-form')).toContainText('Response returned by the ping method.');
    await expect(page.locator('#request-form')).toContainText('Message returned from the fixture service.');
  });

  test('searches methods and switches params editor without stale UI', async ({ page }) => {
    await page.locator('#operation-search').fill('create');
    await expect(page.locator('.op').filter({ hasText: 'create_widget' })).toBeVisible();
    await expect(page.locator('.op').filter({ hasText: 'ping' })).toHaveCount(0);

    await page.locator('#operation-search').fill('');
    await selectMethod(page, 'ping');
    await expect(page.locator('#params-editor')).toBeVisible();
    await page.locator('#params-editor').fill(JSON.stringify({ message: 'hello' }, null, 2));
    await expect(page.locator('#request-url')).toHaveText(`http://127.0.0.1:${RPC_PORT}/rpc`);

    await selectMethod(page, 'no_params');
    await expect(page.locator('#params-editor')).toHaveCount(0);
    await expect(page.locator('#request-form')).toContainText('This method has no params.');

    await selectMethod(page, 'create_widget');
    await expect(page.locator('#params-editor')).toBeVisible();
    await expect(page.locator('#params-editor')).not.toHaveValue(/hello/);
  });

  test('sends public and authenticated JSON-RPC requests', async ({ page }) => {
    await selectMethod(page, 'ping');
    await page.locator('#params-editor').fill(JSON.stringify({ message: 'browser' }, null, 2));
    const originalRequestId = await page.locator('#rpc-request-id').inputValue();
    await page.getByRole('button', { name: 'Regenerate' }).click();
    await expect(page.locator('#rpc-request-id')).not.toHaveValue(originalRequestId);

    await send(page);
    await expect(page.locator('#response-status')).toContainText('200');
    await expect(page.locator('#response-output')).toContainText('pong: browser');
    await expect(page.locator('#history-list')).toContainText('RPC ping');

    await selectMethod(page, 'rename_widget.v1');
    await page.locator('#params-editor').fill(JSON.stringify({ name: 'Legacy Widget' }, null, 2));
    await send(page);
    await expect(page.locator('#response-status')).toContainText('200');
    await expect(page.locator('#response-output')).toContainText('Legacy Widget');
    await expect(page.locator('#response-output')).not.toContainText('notified');
    await expect(page.locator('#history-list')).toContainText('RPC rename_widget.v1');

    await selectMethod(page, 'create_widget');
    await page.locator('#params-editor').fill(JSON.stringify({ name: 'RPC Widget', owner: 'playwright' }, null, 2));
    await send(page);
    await expect(page.locator('#response-status')).toContainText('RPC error');
    await expect(page.locator('#response-output')).toContainText('Authentication');

    await page.locator('#bearer-token').fill('admin-token');
    await page.locator('#save-token').click();
    await expect(page.locator('#auth-state')).toContainText('Token set');
    await send(page);
    await expect(page.locator('#response-status')).toContainText('200');
    await expect(page.locator('#response-output')).toContainText('rpc-created-widget');
    await expect(page.locator('#response-output')).toContainText('RPC Widget');

    await page.locator('[data-response-tab="request"]').click();
    await expect(page.locator('#response-output')).toContainText('create_widget');
    await expect(page.locator('#response-output')).toContainText('RPC Widget');
  });

  test('saves JSON-RPC requests, restores history, and keeps tokens out of localStorage', async ({ page }) => {
    await selectMethod(page, 'create_widget');
    await page.locator('#bearer-token').fill('admin-token');
    await page.locator('#save-token').click();
    await page.locator('#params-editor').fill(JSON.stringify({ name: 'Saved RPC', owner: 'saved-owner' }, null, 2));

    page.once('dialog', async (dialog) => {
      await dialog.accept('rpc saved request');
    });
    await page.locator('#save-request').click();
    await expect(page.locator('#saved-list')).toContainText('rpc saved request');

    await page.locator('#params-editor').fill(JSON.stringify({ name: 'Changed RPC', owner: 'changed' }, null, 2));
    await page.locator('#saved-list').getByRole('button', { name: 'Load' }).click();
    await expect(page.locator('#params-editor')).toHaveValue(/Saved RPC/);

    await send(page);
    await expect(page.locator('#history-list')).toContainText('RPC create_widget');
    await page.locator('#params-editor').fill(JSON.stringify({ name: 'After History', owner: 'changed' }, null, 2));
    await page.locator('#history-list').getByRole('button', { name: 'Load request' }).first().click();
    await expect(page.locator('#params-editor')).toHaveValue(/Saved RPC/);

    await page.reload();
    await expect(page.locator('#service-name')).toContainText('ExplorerRpcFixture Explorer');
    await selectMethod(page, 'create_widget');
    await expect(page.locator('#saved-list')).toContainText('rpc saved request');
    await expect(page.locator('#history-list')).toContainText('RPC create_widget');

    const localStorageValues = await page.evaluate(() => Object.values(localStorage).join('\n'));
    expect(localStorageValues).not.toContain('admin-token');
    expect(localStorageValues).not.toContain('user-token');
    const sessionStorageValues = await page.evaluate(() => Object.values(sessionStorage).join('\n'));
    expect(sessionStorageValues).toContain('admin-token');
  });

  test('persists theme preference in localStorage', async ({ page }) => {
    await page.locator('#theme-toggle').click();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
    await page.reload();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
    const theme = await page.evaluate(() => localStorage.getItem('ras-explorer-theme'));
    expect(theme).toBe('light');
  });
});
