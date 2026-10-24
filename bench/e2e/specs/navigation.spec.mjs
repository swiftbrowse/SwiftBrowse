import { test, expect } from '@playwright/test';

test.describe('navigation', () => {
  test('help menu opens on hover', async ({ page }) => {
    await page.goto('index.html');
    await expect(page.getByRole('menuitem', { name: 'About' })).toBeHidden();
    await page.getByRole('button', { name: 'Help' }).hover();
    await expect(page.getByRole('menuitem', { name: 'About' })).toBeVisible();
    await page.getByRole('menuitem', { name: 'About' }).click();
    await expect(page).toHaveTitle('About');
  });

  test('back and forward', async ({ page }) => {
    await page.goto('index.html');
    await page.getByRole('link', { name: 'Sign in' }).click();
    await expect(page).toHaveTitle('Sign in');
    await page.goBack();
    await expect(page).toHaveTitle('Todos');
    await page.goForward();
    await expect(page).toHaveTitle('Sign in');
  });
});
