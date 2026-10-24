import { test as setup, expect } from '@playwright/test';
import { MEMBER_STATE } from './auth-state.mjs';

setup('joins and saves the logged-in state', async ({ page }) => {
  await page.goto('join.html');
  await page.getByLabel('Your name').fill('Ada');
  await page.getByRole('button', { name: 'Join' }).click();
  await expect(page.getByText('Members area for Ada')).toBeVisible();
  await page.context().storageState({ path: MEMBER_STATE });
});
