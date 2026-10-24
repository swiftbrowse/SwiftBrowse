import { test, expect } from '@playwright/test';
import { MEMBER_STATE } from './auth-state.mjs';

test.describe('members', () => {
  test.use({ storageState: MEMBER_STATE });

  test('starts logged in', async ({ page }) => {
    await page.goto('members.html');
    await expect(page.getByText('Members area for Ada')).toBeVisible();
  });

  test('each test gets its own copy of the state', async ({ page }) => {
    await page.goto('members.html');
    await page.evaluate(() => localStorage.setItem('member_name', 'Changed'));
    await page.reload();
    await expect(page.getByText('Members area for Changed')).toBeVisible();
  });
});

test.describe('guests', () => {
  test('are not members', async ({ page }) => {
    await page.goto('members.html');
    await expect(page.getByText('Members only')).toBeVisible();
  });
});
