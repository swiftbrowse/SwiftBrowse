import { BASE, it, role, suite } from './harness.mjs';

suite('tabs and frames', () => {
  it('follows a target=_blank link into a new tab', async (page) => {
    await page.navigate(BASE + 'tabs.html');
    await page.click('Open about in a new tab');
    await page.switchTab({ url_contains: 'about.html' });
    await page.expect({ type: 'title_equals', value: 'About' });
    await page.closeTab();
    await page.expect({ type: 'title_equals', value: 'Tabs' });
  });

  it('signs in through a popup', async (page) => {
    await page.navigate(BASE + 'tabs.html');
    await page.click('Sign in with popup');
    await page.switchTab({ last: true });
    await page.expect({ type: 'title_equals', value: 'Sign in' });
    await page.fill('Email', 'ada@example.com');
    await page.switchTab({ index: 0 });
    await page.expect({ type: 'title_equals', value: 'Tabs' });
  });

  it('opens a tab itself', async (page) => {
    await page.navigate(BASE + 'index.html');
    await page.newTab({ url: BASE + 'login.html' });
    await page.expect({ type: 'title_equals', value: 'Sign in' });
    await page.switchTab({ title_contains: 'Todos' });
    await page.expect({ type: 'visible', target: 'New todo' });
  });

  it('works inside a same-site iframe', async (page) => {
    await page.navigate(BASE + 'frames.html');
    await page.switchFrame({ target: { by: 'css', selector: 'iframe[name=same]' } });
    await page.fill('Name', 'Ada');
    await page.check('Agree');
    await page.click('Save', { expect: { type: 'text_visible', text: 'Saved Ada in same' } });
    await page.switchFrame({ main: true });
    await page.expect({ type: 'title_equals', value: 'Frames' });
  });

  it('works inside a cross-site iframe', async (page) => {
    await page.navigate(BASE + 'frames.html');
    await page.switchFrame({ name: 'cross' });
    await page.fill('Name', 'Grace');
    await page.click('Save', { expect: { type: 'text_visible', text: 'Saved Grace in cross' } });
    await page.evaluate('location.hostname');
  });

  it('works inside a nested iframe', async (page) => {
    await page.navigate(BASE + 'frames.html');
    await page.switchFrame({ url_contains: 'who=inner' });
    await page.fill('Name', 'Linus');
    await page.click('Save', { expect: { type: 'text_visible', text: 'Saved Linus in inner' } });
  });
});
