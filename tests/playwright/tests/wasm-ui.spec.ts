import { test, expect } from '@playwright/test';

test('login, task actions, and failures preserve reactive UI state', async ({ page }) => {
  const tasks: Array<Record<string, unknown>> = [];
  const requests: Array<{ method: string; params: any }> = [];
  const browserErrors: string[] = [];
  let failCreate = false;
  page.on('pageerror', error => browserErrors.push(error.message));
  await page.route('**/rpc', async route => {
    const request = route.request().postDataJSON();
    requests.push(request);
    let result: unknown;
    switch (request.method) {
      case 'sign_in':
        result = request.params.WithCredentials.password === 'password'
          ? { Success: { jwt: 'browser-test-token' } }
          : { Failure: { msg: 'Invalid credentials' } };
        break;
      case 'list_tasks': result = { tasks, total: tasks.length }; break;
      case 'get_dashboard_stats': result = {
        total_tasks: tasks.length,
        completed_tasks: tasks.filter(task => task.completed).length,
        pending_tasks: tasks.filter(task => !task.completed).length,
        high_priority_tasks: tasks.filter(task => task.priority === 'High').length
      }; break;
      case 'create_task':
        expect(route.request().headers().authorization).toBe('Bearer browser-test-token');
        if (failCreate) {
          await route.fulfill({ json: { jsonrpc: '2.0', id: request.id, error: { code: -32603, message: 'Creation failed' } } });
          return;
        }
        result = { ...request.params, id: `task-${tasks.length + 1}`, completed: false,
          created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z' };
        tasks.push(result as Record<string, unknown>);
        break;
      case 'update_task': {
        const task = tasks.find(task => task.id === request.params.id)!;
        for (const [key, value] of Object.entries(request.params)) {
          if (value !== null) task[key] = value;
        }
        result = task;
        break;
      }
      case 'delete_task': tasks.splice(tasks.findIndex(task => task.id === request.params), 1); result = true; break;
      case 'sign_out': result = null; break;
      default: throw new Error(`Unexpected RPC method: ${request.method}`);
    }
    await route.fulfill({ json: { jsonrpc: '2.0', id: request.id, result } });
  });

  await page.goto('/');
  await page.getByPlaceholder('Enter your username').fill('user');
  await page.getByPlaceholder('Enter your password').fill('wrong');
  await page.getByRole('button', { name: 'Sign In', exact: true }).click();
  await expect(page.getByText('Invalid credentials', { exact: true })).toBeVisible();
  await page.getByPlaceholder('Enter your password').fill('password');
  await page.getByRole('button', { name: 'Sign In', exact: true }).click();
  await expect(page.getByText('No tasks yet. Create your first task!')).toBeVisible();

  await page.getByPlaceholder('What needs to be done?').fill('Review module boundaries');
  await page.getByPlaceholder('Add more details...').fill('Verify browser interactions');
  await page.getByRole('button', { name: 'High', exact: true }).click();
  await page.getByRole('button', { name: 'Create Task', exact: true }).click();
  await expect(page.getByText('Review module boundaries', { exact: true })).toBeVisible();
  await expect(page.getByPlaceholder('What needs to be done?')).toHaveValue('');
  await page.getByRole('checkbox').check();
  await expect.poll(() => tasks[0]?.completed).toBe(true);
  await expect(page.getByRole('checkbox')).toBeChecked();

  failCreate = true;
  await page.getByPlaceholder('What needs to be done?').fill('Keep this draft');
  await page.getByRole('button', { name: 'Create Task', exact: true }).click();
  await expect.poll(() => requests.filter(request => request.method === 'create_task').length).toBe(2);
  await expect(page.getByPlaceholder('What needs to be done?')).toHaveValue('Keep this draft');
  await expect(page.getByRole('checkbox')).toHaveCount(1);
  await page.getByRole('button', { name: 'Delete', exact: true }).click();
  await expect(page.getByText('No tasks yet. Create your first task!')).toBeVisible();
  expect(requests.some(request => request.method === 'list_tasks')).toBe(true);
  expect(browserErrors).toEqual([]);
});
