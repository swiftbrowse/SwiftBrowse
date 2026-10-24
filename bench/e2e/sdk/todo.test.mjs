import { BASE, it, role, suite } from './harness.mjs';

suite('todos', () => {
  const open = (page) => page.navigate(BASE + 'index.html');
  const add = async (page, text) => {
    await page.fill('New todo', text);
    await page.pressKey('Enter', { expect: { type: 'visible', target: role('checkbox', text) } });
  };

  it('adds an item', async (page) => {
    await page.fill('New todo', 'Buy milk');
    await page.click(role('button', 'Add'), { expect: { type: 'visible', target: role('checkbox', 'Buy milk') } });
    await page.expect([
      { type: 'text_equals', target: { by: 'test_id', id: 'count' }, text: '1 item left' },
      { type: 'value_equals', target: 'New todo', value: '' },
    ]);
  }, open);

  it('rejects an empty item', async (page) => {
    await page.click(role('button', 'Add'));
    await page.expect({ type: 'text_visible', text: 'Write something first' });
    await page.expect({ type: 'count', target: { by: 'css', selector: '#list li' }, count: 0 });
  }, open);

  it('completes and filters items', async (page) => {
    await add(page, 'Write tests');
    await add(page, 'Ship it');
    await page.check(role('checkbox', 'Write tests'));
    await page.expect({ type: 'text_equals', target: { by: 'test_id', id: 'count' }, text: '1 item left' });
    await page.click(role('button', 'Active'));
    await page.expect([{ type: 'count', target: { by: 'role', role: 'checkbox' }, count: 1 }, { type: 'visible', target: role('checkbox', 'Ship it') }]);
    await page.click(role('button', 'Completed', { exact: true }));
    await page.expect({ type: 'checked', target: role('checkbox', 'Write tests'), checked: true });
  }, open);

  it('deletes and clears items', async (page) => {
    await add(page, 'One');
    await add(page, 'Two');
    await page.click('Delete One', { expect: { type: 'count', target: { by: 'role', role: 'checkbox' }, count: 1 } });
    await page.check(role('checkbox', 'Two'));
    await page.click('Clear completed', { expect: { type: 'text_equals', target: { by: 'test_id', id: 'count' }, text: '0 items left' } });
  }, open);

  it('keeps items after a reload', async (page) => {
    await add(page, 'Persist me');
    await page.reload();
    await page.expect({ type: 'visible', target: role('checkbox', 'Persist me') });
    await page.evaluate("JSON.parse(localStorage.getItem('todos')).length");
  }, open);
});
