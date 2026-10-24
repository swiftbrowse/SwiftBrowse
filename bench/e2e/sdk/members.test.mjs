import { before } from 'node:test';
import { BASE, it, role, sessionOf, suite } from './harness.mjs';

suite('members', () => {
  // The saved-login setup: join once, save the state, start every member test from it.
  before(async () => {
    const page = await sessionOf();
    await page.navigate(BASE + 'join.html');
    await page.fill('Your name', 'Ada');
    await page.click(role('button', 'Join'), { expect: { type: 'text_visible', text: 'Members area for Ada' } });
    await page.saveStorageState('member');
    await page.close();
  });
  const member = (page) => page.loadStorageState('member');

  it('starts logged in', async (page) => {
    await page.navigate(BASE + 'members.html');
    await page.expect({ type: 'text_visible', text: 'Members area for Ada' });
  }, member);

  it('each test gets its own copy of the state', async (page) => {
    await page.navigate(BASE + 'members.html');
    await page.evaluate("localStorage.setItem('member_name', 'Changed')");
    await page.reload();
    await page.expect({ type: 'text_visible', text: 'Members area for Changed' });
  }, member);

  it('guests are not members', async (page) => {
    await page.navigate(BASE + 'members.html');
    await page.expect({ type: 'text_visible', text: 'Members only' });
  });
});
