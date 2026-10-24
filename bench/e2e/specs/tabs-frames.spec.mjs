import { test, expect } from '@playwright/test';

test.describe('tabs and frames', () => {
  test('follows a target=_blank link into a new tab', async ({ page }) => {
    await page.goto('tabs.html');
    const [tab] = await Promise.all([page.waitForEvent('popup'), page.getByText('Open about in a new tab').click()]);
    await expect(tab).toHaveTitle('About');
    await tab.close();
    await expect(page).toHaveTitle('Tabs');
  });

  test('signs in through a popup', async ({ page }) => {
    await page.goto('tabs.html');
    const [popup] = await Promise.all([page.waitForEvent('popup'), page.getByRole('button', { name: 'Sign in with popup' }).click()]);
    await expect(popup).toHaveTitle('Sign in');
    await popup.getByLabel('Email').fill('ada@example.com');
    await page.bringToFront();
    await expect(page).toHaveTitle('Tabs');
  });

  test('opens a tab itself', async ({ page, context }) => {
    await page.goto('index.html');
    const tab = await context.newPage();
    await tab.goto('login.html');
    await expect(tab).toHaveTitle('Sign in');
    await page.bringToFront();
    await expect(page.getByLabel('New todo')).toBeVisible();
  });

  test('works inside a same-site iframe', async ({ page }) => {
    await page.goto('frames.html');
    const frame = page.frameLocator('iframe[name=same]');
    await frame.getByLabel('Name').fill('Ada');
    await frame.getByLabel('Agree').check();
    await frame.getByRole('button', { name: 'Save' }).click();
    await expect(frame.getByText('Saved Ada in same')).toBeVisible();
    await expect(page).toHaveTitle('Frames');
  });

  test('works inside a cross-site iframe', async ({ page }) => {
    await page.goto('frames.html');
    const frame = page.frameLocator('iframe[name=cross]');
    await frame.getByLabel('Name').fill('Grace');
    await frame.getByRole('button', { name: 'Save' }).click();
    await expect(frame.getByText('Saved Grace in cross')).toBeVisible();
    await page.frame({ name: 'cross' }).evaluate(() => location.hostname);
  });

  test('works inside a nested iframe', async ({ page }) => {
    await page.goto('frames.html');
    const frame = page.frameLocator('iframe[name=outer]').frameLocator('iframe[name=inner]');
    await frame.getByLabel('Name').fill('Linus');
    await frame.getByRole('button', { name: 'Save' }).click();
    await expect(frame.getByText('Saved Linus in inner')).toBeVisible();
  });
});
