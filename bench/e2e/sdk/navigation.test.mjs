import { BASE, it, role, suite } from './harness.mjs';

suite('navigation', () => {
  it('help menu opens on hover', async (page) => {
    await page.navigate(BASE + 'index.html');
    await page.expect({ type: 'hidden', target: role('menuitem', 'About') });
    await page.hover(role('button', 'Help'), { expect: { type: 'visible', target: role('menuitem', 'About') } });
    await page.click(role('menuitem', 'About'), { expect: { type: 'title_equals', value: 'About' } });
  });

  it('back and forward', async (page) => {
    await page.navigate(BASE + 'index.html');
    await page.click(role('link', 'Sign in'), { expect: { type: 'title_equals', value: 'Sign in' } });
    await page.goBack();
    await page.expect({ type: 'title_equals', value: 'Todos' });
    await page.goForward();
    await page.expect({ type: 'title_equals', value: 'Sign in' });
  });
});
