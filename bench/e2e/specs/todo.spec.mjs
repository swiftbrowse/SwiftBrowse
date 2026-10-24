import { test, expect } from '@playwright/test';

test.describe('todos', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('index.html');
  });

  test('adds an item', async ({ page }) => {
    await page.getByLabel('New todo').fill('Buy milk');
    await page.getByRole('button', { name: 'Add' }).click();
    await expect(page.getByRole('checkbox', { name: 'Buy milk' })).toBeVisible();
    await expect(page.getByTestId('count')).toHaveText('1 item left');
    await expect(page.getByLabel('New todo')).toHaveValue('');
  });

  test('rejects an empty item', async ({ page }) => {
    await page.getByRole('button', { name: 'Add' }).click();
    await expect(page.getByText('Write something first')).toBeVisible();
    await expect(page.locator('#list li')).toHaveCount(0);
  });

  test('completes and filters items', async ({ page }) => {
    await page.getByLabel('New todo').fill('Write tests');
    await page.keyboard.press('Enter');
    await page.getByLabel('New todo').fill('Ship it');
    await page.keyboard.press('Enter');
    await expect(page.getByRole('checkbox')).toHaveCount(2);
    await page.getByRole('checkbox', { name: 'Write tests' }).check();
    await expect(page.getByTestId('count')).toHaveText('1 item left');
    await page.getByRole('button', { name: 'Active' }).click();
    await expect(page.getByRole('checkbox')).toHaveCount(1);
    await expect(page.getByRole('checkbox', { name: 'Ship it' })).toBeVisible();
    await page.getByRole('button', { name: 'Completed', exact: true }).click();
    await expect(page.getByRole('checkbox', { name: 'Write tests' })).toBeChecked();
  });

  test('deletes and clears items', async ({ page }) => {
    await page.getByLabel('New todo').fill('One');
    await page.keyboard.press('Enter');
    await page.getByLabel('New todo').fill('Two');
    await page.keyboard.press('Enter');
    await expect(page.getByRole('checkbox')).toHaveCount(2);
    await page.getByRole('button', { name: 'Delete One' }).click();
    await expect(page.getByRole('checkbox')).toHaveCount(1);
    await page.getByRole('checkbox', { name: 'Two' }).check();
    await page.getByRole('button', { name: 'Clear completed' }).click();
    await expect(page.getByTestId('count')).toHaveText('0 items left');
  });

  test('keeps items after a reload', async ({ page }) => {
    await page.getByLabel('New todo').fill('Persist me');
    await page.keyboard.press('Enter');
    await expect(page.getByRole('checkbox', { name: 'Persist me' })).toBeVisible();
    await page.reload();
    await expect(page.getByRole('checkbox', { name: 'Persist me' })).toBeVisible();
    expect(await page.evaluate(() => JSON.parse(localStorage.getItem('todos')).length)).toBe(1);
  });
});
