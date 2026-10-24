import { test, expect } from '@playwright/test';

const email = 'ada@example.com';
const password = 'secret';

test.describe('sign in', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('login.html');
  });

  test('signs in and reaches the dashboard', async ({ page }) => {
    const errors = [];
    page.on('pageerror', (e) => errors.push(e));
    await page.getByLabel('Email').fill(email);
    await page.getByLabel('Password').fill(password);
    await page.getByLabel('Plan').selectOption({ label: 'Pro' });
    await page.getByLabel('Remember me').check();
    await page.getByRole('button', { name: 'Sign in' }).click();
    await expect(page).toHaveURL(/dashboard\.html/);
    await expect(page).toHaveTitle('Dashboard');
    await expect(page.getByTestId('hello')).toHaveText(`Welcome, ${email}`);
    await expect(page.getByText('pro')).toBeVisible();
    expect(errors).toEqual([]);
  });

  test('rejects a wrong password', async ({ page }) => {
    await page.getByLabel('Email').fill(email);
    await page.getByLabel('Password').fill('nope');
    await page.getByRole('button', { name: 'Sign in' }).click();
    await expect(page.getByRole('alert')).toHaveText('Wrong password');
    await expect(page).toHaveURL(/login\.html/);
  });

  test('validates the email', async ({ page }) => {
    await page.getByLabel('Email').fill('not-an-email');
    await page.getByRole('button', { name: 'Sign in' }).click();
    await expect(page.getByText('Enter a valid email')).toBeVisible();
  });

  test('signs out', async ({ page }) => {
    await page.getByLabel('Email').fill(email);
    await page.getByLabel('Password').fill(password);
    await page.getByRole('button', { name: 'Sign in' }).click();
    await expect(page).toHaveTitle('Dashboard');
    await page.getByRole('button', { name: 'Sign out' }).click();
    await expect(page).toHaveTitle('Sign in');
  });

  test('redirects to sign-in without a session', async ({ page }) => {
    await page.goto('dashboard.html');
    await expect(page).toHaveURL(/login\.html/);
  });
});
