// Playwright Test side of the e2e benchmark. Same app, same tests, same Chromium
// build (headless shell), tests fully parallel like `swiftbrowse test`.
import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: './specs',
  // The saved-login setup runs first; the other tests depend on it.
  projects: [
    { name: 'setup', testMatch: /.*\.setup\.mjs/ },
    { name: 'tests', testMatch: /.*\.spec\.mjs/, dependencies: ['setup'] },
  ],
  fullyParallel: true,
  retries: 0,
  reporter: 'dot',
  outputDir: process.env.PW_OUTPUT_DIR || 'test-results',
  use: {
    baseURL: process.env.BASE_URL,
    headless: true,
    // The benchmark passes the same binary to both tools.
    launchOptions: process.env.CHROMIUM ? { executablePath: process.env.CHROMIUM } : {},
  },
});
