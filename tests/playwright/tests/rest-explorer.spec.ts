import { expect, test, type Page } from '@playwright/test';

const REST_PORT = process.env.PLAYWRIGHT_REST_PORT ?? '3101';
const REST_URL = `http://127.0.0.1:${REST_PORT}/api/v1/docs`;

function escapeRegex(value: string) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

async function selectOperation(page: Page, method: string, path: string) {
  await page.getByRole('button', { name: new RegExp(`^${method}\\s+${escapeRegex(path)}(?:\\s|$)`) }).click();
}

async function send(page: Page) {
  await page.locator('#send-request').click();
}

test.describe('REST API explorer', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(REST_URL);
    await expect(page.locator('#service-name')).toContainText('ExplorerRestFixture Explorer');
    await expect(page.locator('#operation-list .op').first()).toBeVisible();
  });

  test('loads in dark mode and renders OpenAPI operations', async ({ page }) => {
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    await expect(page.locator('#service-subtitle')).toContainText('REST OpenAPI');
    await expect(page.locator('#operation-list')).toContainText('/health');
    await expect(page.locator('.op').filter({ hasText: '/health' })).toContainText('Check fixture `health`.');
    await expect(page.locator('#operation-list')).toContainText('/widgets');
    await expect(page.locator('#operation-list')).toContainText('/search/widgets');
    await expect(page.locator('#operation-list')).toContainText('/v2/widgets/{id}/rename');
    await expect(page.locator('#operation-list')).toContainText('/v1/widgets/{id}/rename');

    await selectOperation(page, 'GET', '/health');
    await expect(page.locator('#operation-description p code')).toContainText('health');
    await expect(page.locator('#operation-description strong')).toContainText('REST docs');
    await expect(page.locator('#operation-description li')).toContainText([
      'Shows operation details',
      'Preserves line breaks'
    ]);
    await expect(page.locator('#operation-description pre code')).toContainText('{"status":"ok"}');
    await expect(page.locator('#operation-description a').filter({ hasText: 'REST docs' })).toHaveAttribute(
      'href',
      'https://github.com/JedimEmO/rust-agent-stack/blob/main/documentation/ras-rest-macro.md'
    );
    const descriptionText = await page.locator('#operation-description').evaluate((el) => el.textContent ?? '');
    expect(descriptionText).toContain('Alpha line\nBeta line');

    await expect(page.locator('#request-form')).toContainText('Response schema');
    await expect(page.locator('#request-form')).toContainText('Health status returned by the fixture service.');
    await expect(page.locator('#request-form .schema-desc strong')).toContainText('Schema docs');
    await expect(page.locator('#request-form')).toContainText('Current health state.');
    const statusDocText = await page
      .locator('.schema-field')
      .filter({ hasText: 'Current health state.' })
      .locator('.schema-field-desc')
      .first()
      .evaluate((el) => el.textContent ?? '');
    expect(statusDocText).toContain('Current health state.\nThis field description keeps its line break.');
  });

  test('searches operations and switches request forms without stale UI', async ({ page }) => {
    await page.locator('#operation-search').fill('search');
    await expect(page.locator('.op').filter({ hasText: '/search/widgets' })).toBeVisible();
    await expect(page.locator('.op').filter({ hasText: '/health' })).toHaveCount(0);

    await page.locator('#operation-search').fill('');
    await selectOperation(page, 'GET', '/search/widgets');
    await page.locator('[data-query-param="q"]').fill('alpha');
    await page.locator('[data-query-param="limit"]').fill('2');
    await expect(page.locator('#request-url')).toContainText('/api/v1/search/widgets');
    await expect(page.locator('#request-url')).toContainText('q=alpha');
    await expect(page.locator('#request-url')).toContainText('limit=2');

    await selectOperation(page, 'GET', '/widgets/{id}');
    await expect(page.locator('[data-query-param="q"]')).toHaveCount(0);
    await page.locator('[data-path-param="id"]').fill('widget-123');
    await expect(page.locator('#request-url')).toContainText('/api/v1/widgets/widget-123');

    await selectOperation(page, 'GET', '/health');
    await expect(page.locator('#body-editor')).toHaveCount(0);
    await expect(page.locator('[data-path-param="id"]')).toHaveCount(0);
  });

  test('sends public and authenticated requests, then records history', async ({ page }) => {
    await selectOperation(page, 'GET', '/health');
    await send(page);
    await expect(page.locator('#response-status')).toContainText('200');
    await expect(page.locator('#response-output')).toContainText('"status": "ok"');
    await expect(page.locator('#history-list')).toContainText('GET /health');

    await selectOperation(page, 'POST', '/v1/widgets/{id}/rename');
    await page.locator('[data-path-param="id"]').fill('legacy-widget');
    await page.locator('#body-editor').fill(JSON.stringify({ name: 'Legacy REST Widget' }, null, 2));
    await send(page);
    await expect(page.locator('#response-status')).toContainText('200');
    await expect(page.locator('#response-output')).toContainText('Legacy REST Widget');
    await expect(page.locator('#history-list')).toContainText('POST /v1/widgets/{id}/rename');

    await selectOperation(page, 'POST', '/widgets');
    await page.locator('#body-editor').fill(JSON.stringify({ name: 'Created From UI', owner: 'playwright' }, null, 2));
    await send(page);
    await expect(page.locator('#response-status')).toContainText('401');

    await page.locator('#bearer-token').fill('admin-token');
    await page.locator('#save-token').click();
    await expect(page.locator('#auth-state')).toContainText('Token set');
    await send(page);
    await expect(page.locator('#response-status')).toContainText('201');
    await expect(page.locator('#response-output')).toContainText('created-widget');
    await expect(page.locator('#response-output')).toContainText('Created From UI');

    await page.locator('[data-response-tab="headers"]').click();
    await expect(page.locator('#response-output')).toContainText('content-type');
    await page.locator('[data-response-tab="request"]').click();
    await expect(page.locator('#response-output')).toContainText('Created From UI');
  });

  test('shows permission denied responses for insufficient token permissions', async ({ page }) => {
    await selectOperation(page, 'POST', '/widgets');
    await page.locator('#body-editor').fill(JSON.stringify({ name: 'Denied Widget', owner: 'playwright' }, null, 2));
    await page.locator('#bearer-token').fill('user-token');
    await page.locator('#save-token').click();
    await expect(page.locator('#auth-state')).toContainText('Token set');

    await send(page);
    await expect(page.locator('#response-status')).toContainText('403');
    await expect(page.locator('#history-list')).toContainText('403');
  });

  test('saves requests, restores history, and keeps tokens out of localStorage', async ({ page }) => {
    await selectOperation(page, 'POST', '/widgets');
    await page.locator('#bearer-token').fill('admin-token');
    await page.locator('#save-token').click();
    await page.locator('#body-editor').fill(JSON.stringify({ name: 'Saved Body', owner: 'saved-owner' }, null, 2));

    page.once('dialog', async (dialog) => {
      await dialog.accept('rest saved request');
    });
    await page.locator('#save-request').click();
    await expect(page.locator('#saved-list')).toContainText('rest saved request');

    await page.locator('#body-editor').fill(JSON.stringify({ name: 'Changed Body', owner: 'changed' }, null, 2));
    await page.locator('#saved-list').getByRole('button', { name: 'Load' }).click();
    await expect(page.locator('#body-editor')).toHaveValue(/Saved Body/);

    await send(page);
    await expect(page.locator('#history-list')).toContainText('POST /widgets');
    await page.locator('#body-editor').fill(JSON.stringify({ name: 'After History', owner: 'changed' }, null, 2));
    await page.locator('#history-list').getByRole('button', { name: 'Load request' }).first().click();
    await expect(page.locator('#body-editor')).toHaveValue(/Saved Body/);

    await page.reload();
    await expect(page.locator('#service-name')).toContainText('ExplorerRestFixture Explorer');
    await selectOperation(page, 'POST', '/widgets');
    await expect(page.locator('#saved-list')).toContainText('rest saved request');
    await expect(page.locator('#history-list')).toContainText('POST /widgets');

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
