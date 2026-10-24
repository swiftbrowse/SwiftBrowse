// Compile-only check of the SDK's types: `npx -p typescript tsc -p test/tsconfig.json`.
import { launch, SwiftBrowseError, type ActionResult, type Condition } from '../src/index.js';

async function main(): Promise<void> {
  const browser = await launch({ allowHosts: ['example.com'] });
  const page = await browser.newSession({ viewport: [1024, 768] });
  await page.navigate('https://example.com', { wait_until: 'network_idle' });
  await page.click('More information', { expect: { type: 'url_contains', value: 'iana' } });
  await page.fill({ by: 'label', text: 'Email' }, 'ada@example.com');
  await page.selectOption({ by: 'role', role: 'combobox', name: 'Plan' }, 'Pro');
  await page.route({ url: '**/api/*', method: 'POST', status: 201, json: { ok: true }, times: 1 });
  await page.switchTab({ url_contains: 'help', last: true });
  await page.switchFrame({ target: { by: 'css', selector: 'iframe' } });
  const done: Condition = { type: 'text_visible', text: 'Done' };
  const r: ActionResult = await page.expect([done, { type: 'no_console_errors' }]);
  console.log(r.ok, r.timing.duration_ms, await page.text());
  try {
    await page.click({ by: 'test_id', id: 'save' });
  } catch (e) {
    if (e instanceof SwiftBrowseError) console.log(e.kind, e.result.error?.message);
  }
  // @ts-expect-error: unknown condition type
  await page.expect({ type: 'title_is', value: 'x' });
  // @ts-expect-error: fill needs a value
  await page.fill('Email');
  // @ts-expect-error: unknown locator strategy
  await page.click({ by: 'magic', q: 'x' });
  await browser.close();
}
void main;
