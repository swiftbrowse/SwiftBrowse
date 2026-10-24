import { test, expect } from '@playwright/test';

const json = (route, body, status = 200) => route.fulfill({ status, json: body });

test.describe('network mocking', () => {
  test('renders mocked data', async ({ page }) => {
    const seen = page.waitForRequest((r) => r.url().includes('/api/todos') && r.method() === 'GET');
    await page.route('**/api/todos', (route) => json(route, [{ title: 'Mocked one' }, { title: 'Mocked two' }]));
    await page.goto('api.html');
    await expect(page.getByRole('status')).toHaveText('2 todos');
    await expect(page.getByText('Mocked two')).toBeVisible();
    await seen;
  });

  test('shows a server error', async ({ page }) => {
    await page.route('**/api/todos', (route) => route.fulfill({ status: 500, body: 'boom' }));
    await page.goto('api.html');
    await expect(page.getByText('Could not load todos (HTTP 500)')).toBeVisible();
  });

  test('shows a network error', async ({ page }) => {
    await page.route('**/api/todos', (route) => route.abort('connectionrefused'));
    await page.goto('api.html');
    await expect(page.getByText('Network error')).toBeVisible();
  });

  test('shows the loading state', async ({ page }) => {
    await page.route('**/api/todos', (route) => json(route, [{ title: 'Slow' }]));
    await page.goto('api.html');
    await page.unrouteAll();
    await page.route('**/api/todos', async (route) => {
      await new Promise((r) => setTimeout(r, 600));
      await json(route, [{ title: 'Slow' }]);
    });
    await page.getByRole('button', { name: 'Retry' }).click();
    await expect(page.getByText('Loading…')).toBeVisible();
    await expect(page.getByText('1 todos')).toBeVisible();
  });

  test('recovers after one failure', async ({ page }) => {
    await page.route('**/api/todos', (route) => json(route, [{ title: 'Back again' }]));
    await page.route('**/api/todos', (route) => route.fulfill({ status: 503 }), { times: 1 });
    await page.goto('api.html');
    await expect(page.getByText('HTTP 503')).toBeVisible();
    await page.getByRole('button', { name: 'Retry' }).click();
    await expect(page.getByText('Back again')).toBeVisible();
  });

  test('posts a new item', async ({ page }) => {
    await page.route('**/api/todos', (route) =>
      route.request().method() === 'POST' ? json(route, { id: 42 }, 201) : json(route, []));
    await page.goto('api.html');
    await page.getByLabel('New remote todo').fill('Write docs');
    const posted = page.waitForRequest((r) => r.url().includes('/api/todos') && r.method() === 'POST');
    await page.getByRole('button', { name: 'Save' }).click();
    await expect(page.getByText('Saved #42')).toBeVisible();
    await posted;
    await page.unrouteAll();
  });
});
