import path from 'node:path';
import { BASE, FIXTURES, it, suite } from './harness.mjs';

suite('network and upload', () => {
  const api = (page) => page.navigate(BASE + 'api.html');

  it('uploads files through a hidden input', async (page) => {
    await page.navigate(BASE + 'upload.html');
    await page.uploadFile('Documents', [path.join(FIXTURES, 'notes.txt'), path.join(FIXTURES, 'contacts.csv')]);
    await page.expect([
      { type: 'text_visible', text: 'notes.txt (26 bytes): hello from a test fixture' },
      { type: 'text_visible', text: 'contacts.csv (31 bytes): name,email' },
    ]);
  });

  it('renders mocked data', async (page) => {
    await page.route({ url: '**/api/todos', json: [{ title: 'Mocked one' }, { title: 'Mocked two' }] });
    await api(page);
    await page.expect([
      { type: 'text_equals', target: { by: 'role', role: 'status' }, text: '2 todos' },
      { type: 'text_visible', text: 'Mocked two' },
      { type: 'request_seen', url_contains: '/api/todos', method: 'GET' },
    ]);
  });

  it('shows a server error', async (page) => {
    await page.route({ url: '**/api/todos', status: 500, body: 'boom' });
    await api(page);
    await page.expect({ type: 'text_visible', text: 'Could not load todos (HTTP 500)' });
  });

  it('shows a network error', async (page) => {
    await page.route({ url: '**/api/todos', action: 'abort', error: 'ConnectionRefused' });
    await api(page);
    await page.expect({ type: 'text_visible', text: 'Network error' });
  });

  it('shows the loading state', async (page) => {
    await page.route({ url: '**/api/todos', json: [{ title: 'Slow' }] });
    await api(page);
    await page.route({ url: '**/api/todos', json: [{ title: 'Slow' }], delay_ms: 600 });
    await page.click('Retry', { expect: { type: 'text_visible', text: 'Loading…' } });
    await page.expect({ type: 'text_visible', text: '1 todos' });
  });

  it('recovers after one failure', async (page) => {
    await page.route({ url: '**/api/todos', json: [{ title: 'Back again' }] });
    await page.route({ url: '**/api/todos', status: 503, times: 1 });
    await api(page);
    await page.expect({ type: 'text_visible', text: 'HTTP 503' });
    await page.click('Retry', { expect: { type: 'text_visible', text: 'Back again' } });
  });

  it('posts a new item', async (page) => {
    await page.route({ url: '**/api/todos', method: 'GET', json: [] });
    await page.route({ url: '**/api/todos', method: 'POST', status: 201, json: { id: 42 } });
    await api(page);
    await page.fill('New remote todo', 'Write docs');
    await page.click('Save', { expect: { type: 'text_visible', text: 'Saved #42' } });
    await page.expect({ type: 'request_seen', url_contains: '/api/todos', method: 'POST' });
    await page.unroute();
  });
});
