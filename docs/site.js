// Copy buttons, the mobile menu, search over page and section titles, and the
// "on this page" highlight. No dependencies.
(() => {
  for (const button of document.querySelectorAll('.copy')) {
    button.addEventListener('click', async () => {
      const text = button.parentElement.querySelector('code').innerText;
      try {
        await navigator.clipboard.writeText(text);
        button.textContent = 'Copied';
        button.classList.add('done');
        setTimeout(() => { button.textContent = 'Copy'; button.classList.remove('done'); }, 1500);
      } catch {}
    });
  }

  const menu = document.querySelector('.menu');
  menu.addEventListener('click', () => {
    const open = document.body.classList.toggle('nav-open');
    menu.setAttribute('aria-expanded', String(open));
  });

  const input = document.querySelector('.search');
  const box = document.querySelector('.results');
  let index = null;
  let active = -1;
  const load = () => index || (index = fetch('search.json').then((r) => r.json()).catch(() => []));
  const esc = (s) => s.replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);
  async function show() {
    const q = input.value.trim().toLowerCase();
    if (!q) { box.hidden = true; return; }
    const words = q.split(/\s+/);
    const entries = await load();
    const hits = entries
      .map((e) => {
        const hay = `${e.t} ${e.p} ${e.d || ''}`.toLowerCase();
        if (!words.every((w) => hay.includes(w))) return null;
        const t = e.t.toLowerCase();
        return { e, score: (t.startsWith(q) ? 0 : t.includes(q) ? 1 : 2) + (e.u.includes('#') ? 0.5 : 0) };
      })
      .filter(Boolean)
      .sort((a, b) => a.score - b.score)
      .slice(0, 12);
    active = -1;
    box.innerHTML = hits.length
      ? hits.map(({ e }) => `<a href="${e.u}">${esc(e.t)}<span>${esc(e.p)}</span></a>`).join('')
      : '<div class="none">No matches</div>';
    box.hidden = false;
  }
  input.addEventListener('focus', load);
  input.addEventListener('input', show);
  input.addEventListener('keydown', (ev) => {
    const links = [...box.querySelectorAll('a')];
    if (ev.key === 'ArrowDown' || ev.key === 'ArrowUp') {
      ev.preventDefault();
      if (!links.length) return;
      active = (active + (ev.key === 'ArrowDown' ? 1 : -1) + links.length) % links.length;
      links.forEach((l, i) => l.classList.toggle('active', i === active));
    } else if (ev.key === 'Enter' && links.length) {
      location.href = links[Math.max(active, 0)].href;
    } else if (ev.key === 'Escape') {
      box.hidden = true;
      input.blur();
    }
  });
  document.addEventListener('click', (ev) => { if (!ev.target.closest('.results, .search')) box.hidden = true; });
  document.addEventListener('keydown', (ev) => {
    if (ev.key === '/' && document.activeElement !== input) { ev.preventDefault(); input.focus(); }
  });

  const toc = [...document.querySelectorAll('.toc a')];
  if (toc.length) {
    const heads = toc.map((a) => document.getElementById(a.hash.slice(1))).filter(Boolean);
    const mark = () => {
      let current = heads[0];
      for (const h of heads) if (h.getBoundingClientRect().top < 120) current = h;
      toc.forEach((a) => a.classList.toggle('on', a.hash === `#${current.id}`));
    };
    addEventListener('scroll', mark, { passive: true });
    mark();
  }
})();
