import { BASE, it, role, suite } from './harness.mjs';

suite('sign in', () => {
  const email = 'ada@example.com';
  const open = (page) => page.navigate(BASE + 'login.html');
  const signIn = async (page, password) => {
    await page.fill('Email', email);
    await page.fill('Password', password);
  };

  it('signs in and reaches the dashboard', async (page) => {
    await signIn(page, 'secret');
    await page.selectOption('Plan', 'Pro');
    await page.check('Remember me');
    await page.click(role('button', 'Sign in'), { expect: { type: 'url_contains', value: 'dashboard.html' } });
    await page.expect([
      { type: 'title_equals', value: 'Dashboard' },
      { type: 'text_equals', target: { by: 'test_id', id: 'hello' }, text: `Welcome, ${email}` },
      { type: 'text_visible', text: 'pro' },
      { type: 'no_console_errors' },
    ]);
  }, open);

  it('rejects a wrong password', async (page) => {
    await signIn(page, 'nope');
    await page.click(role('button', 'Sign in'), { expect: { type: 'text_equals', target: { by: 'role', role: 'alert' }, text: 'Wrong password' } });
    await page.expect({ type: 'url_contains', value: 'login.html' });
  }, open);

  it('validates the email', async (page) => {
    await page.fill('Email', 'not-an-email');
    await page.click(role('button', 'Sign in'), { expect: { type: 'text_visible', text: 'Enter a valid email' } });
  }, open);

  it('signs out', async (page) => {
    await signIn(page, 'secret');
    await page.click(role('button', 'Sign in'), { expect: { type: 'title_equals', value: 'Dashboard' } });
    await page.click('Sign out', { expect: { type: 'title_equals', value: 'Sign in' } });
  }, open);

  it('redirects to sign-in without a session', async (page) => {
    await page.navigate(BASE + 'dashboard.html');
    await page.expect({ type: 'url_contains', value: 'login.html' });
  });
});
