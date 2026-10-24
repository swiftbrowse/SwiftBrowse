import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { test, expect } from '@playwright/test';

const fixtures = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../../examples/e2e/tests/fixtures');

test.describe('upload', () => {
  test('uploads files through a hidden input', async ({ page }) => {
    await page.goto('upload.html');
    await page.getByLabel('Documents').setInputFiles([path.join(fixtures, 'notes.txt'), path.join(fixtures, 'contacts.csv')]);
    await expect(page.getByText('notes.txt (26 bytes): hello from a test fixture')).toBeVisible();
    await expect(page.getByText('contacts.csv (31 bytes): name,email')).toBeVisible();
  });
});
