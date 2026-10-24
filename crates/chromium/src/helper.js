// Installed into an isolated JS world: shares the DOM with the page but not
// its globals, so page scripts cannot read or tamper with it.
(() => {
  if (globalThis.__sb) return;

  // Script-driven DOM changes: elements added or removed after DOMContentLoaded
  // (parsing doesn't count). Installed at document start, so it sees an app's
  // whole render.
  let lastScriptChange = 0;
  let parsed = document.readyState !== 'loading';
  const tracker = new MutationObserver((ms) => {
    if (!parsed) return;
    for (const m of ms) {
      for (const n of m.addedNodes) if (n.nodeType === 1) { lastScriptChange = performance.now(); return; }
      for (const n of m.removedNodes) if (n.nodeType === 1) { lastScriptChange = performance.now(); return; }
    }
  });
  tracker.observe(document, { subtree: true, childList: true });
  if (!parsed) {
    document.addEventListener('DOMContentLoaded', () => {
      // Records are delivered asynchronously: drop the parser's still-pending
      // ones so they aren't mistaken for script changes after this point.
      tracker.takeRecords();
      parsed = true;
    }, { once: true });
  }
  const reg = new Map();
  const ids = new WeakMap();
  let next = 1;

  const fail = (kind, msg) => { throw new Error('SB:' + kind + ':' + msg); };
  const norm = (s) => (s || '').replace(/\s+/g, ' ').trim();
  const squash = (s) => (s || '').replace(/\s+/g, '').toLowerCase();
  const matches = (h, n, exact) => {
    h = norm(h).toLowerCase(); n = norm(n).toLowerCase();
    return exact ? h === n : h.includes(n);
  };

  function refOf(el) {
    let r = ids.get(el);
    if (!r) { r = 'e' + (next++); ids.set(el, r); reg.set(r, new WeakRef(el)); }
    return r;
  }
  function get(r) {
    const w = reg.get(r);
    const el = w && w.deref();
    if (!el || !el.isConnected) fail('stale_element', r + ' is no longer attached to the document');
    return el;
  }

  // Elements of the document and of every open shadow root, in tree order.
  function allElements() {
    const out = [];
    const walk = (root) => {
      const it = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT);
      let n = it.nextNode();
      while (n) {
        out.push(n);
        if (n.shadowRoot) walk(n.shadowRoot);
        n = it.nextNode();
      }
    };
    walk(document.documentElement || document);
    return out;
  }

  const INPUT_ROLES = {
    button: 'button', submit: 'button', reset: 'button', image: 'button', checkbox: 'checkbox',
    radio: 'radio', range: 'slider', number: 'spinbutton', search: 'searchbox',
  };
  const NAME_FROM_CONTENT = new Set([
    'button', 'link', 'heading', 'tab', 'menuitem', 'menuitemcheckbox', 'menuitemradio', 'option',
    'checkbox', 'radio', 'switch', 'cell', 'columnheader', 'rowheader', 'treeitem', 'tooltip', 'row',
  ]);
  const INTERACTIVE = new Set([
    'button', 'link', 'textbox', 'searchbox', 'combobox', 'listbox', 'checkbox', 'radio', 'slider',
    'spinbutton', 'switch', 'tab', 'menuitem', 'menuitemcheckbox', 'menuitemradio', 'option', 'treeitem',
  ]);
  const LABELABLE = new Set(['textbox', 'searchbox', 'combobox', 'listbox', 'checkbox', 'radio', 'slider', 'spinbutton', 'switch']);
  const GENERIC = new Set(['presentation', 'none', 'generic']);
  const SKIP_TEXT = new Set(['SCRIPT', 'STYLE', 'NOSCRIPT', 'TEMPLATE', 'HEAD', 'TITLE', 'META', 'LINK']);

  function role(el) {
    const explicit = el.getAttribute('role');
    if (explicit && explicit.trim()) return explicit.trim().split(/\s+/)[0];
    const t = el.localName;
    switch (t) {
      case 'a': case 'area': return el.hasAttribute('href') ? 'link' : null;
      case 'button': case 'summary': return 'button';
      case 'input': {
        const ty = (el.getAttribute('type') || 'text').toLowerCase();
        if (ty === 'hidden') return null;
        if (INPUT_ROLES[ty]) return INPUT_ROLES[ty];
        return el.hasAttribute('list') ? 'combobox' : 'textbox';
      }
      case 'textarea': return 'textbox';
      case 'select': return (el.multiple || el.size > 1) ? 'listbox' : 'combobox';
      case 'option': return 'option';
      case 'h1': case 'h2': case 'h3': case 'h4': case 'h5': case 'h6': return 'heading';
      case 'img': return el.getAttribute('alt') === '' ? 'presentation' : 'img';
      case 'nav': return 'navigation';
      case 'main': return 'main';
      case 'aside': return 'complementary';
      case 'form': return 'form';
      case 'dialog': return 'dialog';
      case 'ul': case 'ol': return 'list';
      case 'li': return 'listitem';
      case 'table': return 'table';
      case 'tr': return 'row';
      case 'td': return 'cell';
      case 'th': return 'columnheader';
      case 'progress': return 'progressbar';
      case 'article': return 'article';
      case 'section': return (el.hasAttribute('aria-label') || el.hasAttribute('aria-labelledby')) ? 'region' : null;
      default: return el.isContentEditable && el.getAttribute('contenteditable') !== null ? 'textbox' : null;
    }
  }

  // innerText is empty for content the browser hasn't rendered (content-visibility:
  // auto off-screen, virtualized lists) and ignores image/SVG labels, so fall
  // back to computing the text from the DOM like the accessible-name algorithm.
  function textOf(n) {
    const t = norm(n.innerText !== undefined ? n.innerText : n.textContent);
    return t || norm(contentText(n, 0)).slice(0, 300);
  }

  function contentText(node, depth) {
    if (depth > 25) return '';
    const out = [];
    for (const c of node.childNodes) {
      if (c.nodeType === 3) { out.push(c.data); continue; }
      if (c.nodeType !== 1 || c.getAttribute('aria-hidden') === 'true') continue;
      const st = getComputedStyle(c);
      if (st.display === 'none' || st.visibility === 'hidden' || SKIP_TEXT.has(c.tagName)) continue;
      const label = ariaLabel(c);
      if (label) { out.push(label); continue; }
      if (c.localName === 'img') { out.push(c.getAttribute('alt') || ''); continue; }
      if (c.localName === 'svg') { const t = c.querySelector('title'); out.push(t ? t.textContent : ''); continue; }
      out.push(contentText(c, depth + 1));
    }
    return out.join(' ');
  }

  // Hidden from assistive technology: such elements are not in the accessibility
  // tree, so they are left out of snapshots and role/label lookups.
  const exposed = (el) => !el.closest('[aria-hidden="true"], [inert]');

  function ariaLabel(el) {
    const lb = el.getAttribute('aria-labelledby');
    if (lb) {
      const t = lb.split(/\s+/).map((id) => document.getElementById(id)).filter(Boolean).map(textOf).join(' ');
      if (norm(t)) return norm(t);
    }
    return norm(el.getAttribute('aria-label') || '');
  }

  function labelText(el) {
    const a = ariaLabel(el);
    if (a) return a;
    if (el.labels && el.labels.length) return norm(Array.from(el.labels).map(textOf).join(' '));
    return '';
  }

  function name(el) {
    const a = ariaLabel(el);
    if (a) return a;
    const t = el.localName;
    if (t === 'input' || t === 'textarea' || t === 'select') {
      const ty = (el.type || '').toLowerCase();
      if (ty === 'button' || ty === 'submit' || ty === 'reset') {
        return norm(el.value || (ty === 'submit' ? 'Submit' : ty === 'reset' ? 'Reset' : ''));
      }
      if (ty === 'image') return norm(el.alt || '');
      return labelText(el) || norm(el.getAttribute('title') || el.getAttribute('placeholder') || '');
    }
    if (t === 'img') return norm(el.getAttribute('alt') || el.getAttribute('title') || '');
    if (t === 'fieldset') { const l = el.querySelector('legend'); if (l) return textOf(l); }
    if (t === 'table') { const c = el.querySelector('caption'); if (c) return textOf(c); }
    const r = role(el);
    if (NAME_FROM_CONTENT.has(r)) return textOf(el).slice(0, 200);
    return norm(el.getAttribute('title') || '');
  }

  function visible(el) {
    if (el.checkVisibility) {
      if (!el.checkVisibility({ checkVisibilityCSS: true })) return false;
    } else {
      const s = getComputedStyle(el);
      if (s.display === 'none' || s.visibility === 'hidden') return false;
    }
    const r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0;
  }

  const enabled = (el) => !(el.matches && el.matches(':disabled')) && el.getAttribute('aria-disabled') !== 'true';

  const NON_TEXT_INPUTS = new Set(['button', 'submit', 'reset', 'image', 'checkbox', 'radio', 'file', 'range', 'color', 'hidden']);
  function editable(el) {
    if (el.localName === 'textarea') return !el.readOnly;
    if (el.localName === 'input') return !el.readOnly && !NON_TEXT_INPUTS.has((el.type || '').toLowerCase());
    return el.isContentEditable;
  }

  const testId = (el) => el.getAttribute('data-testid') ?? el.getAttribute('data-test-id') ?? el.getAttribute('data-test');

  function info(el, withText = true) {
    const t = el.localName;
    const ty = t === 'input' ? (el.type || 'text').toLowerCase() : null;
    const out = {
      ref: refOf(el), tag: t, role: role(el), name: name(el),
      visible: visible(el), enabled: enabled(el), editable: editable(el),
    };
    if (withText && t !== 'input' && t !== 'textarea' && t !== 'select') {
      const tx = textOf(el).slice(0, 160);
      if (tx && tx !== out.name) out.text = tx;
    }
    if (ty) out.input_type = ty;
    if ((t === 'input' || t === 'textarea' || t === 'select') && ty !== 'password') out.value = el.value;
    const tid = testId(el);
    if (tid !== null) out.test_id = tid;
    if (t === 'a' && el.href) out.href = el.href;
    if (ty === 'checkbox' || ty === 'radio') out.checked = el.checked;
    else if (el.hasAttribute('aria-checked')) out.checked = el.getAttribute('aria-checked') === 'true';
    return out;
  }

  const MAX_MATCHES = 500;

  function query(loc) {
    let found;
    switch (loc.by) {
      case 'role': {
        const want = loc.role;
        found = allElements().filter((el) => {
          const r = role(el);
          if (!r || (want === '*' ? GENERIC.has(r) : r !== want) || !exposed(el)) return false;
          return loc.name == null || matches(name(el), loc.name, !!loc.exact);
        });
        break;
      }
      case 'label':
        found = allElements().filter((el) => LABELABLE.has(role(el)) && exposed(el) && matches(labelText(el), loc.text, !!loc.exact));
        break;
      case 'test_id':
        found = allElements().filter((el) => testId(el) === loc.id);
        break;
      case 'text': {
        const needle = squash(loc.text);
        const hits = new Set(allElements().filter((el) => {
          if (SKIP_TEXT.has(el.tagName) || !squash(el.textContent).includes(needle)) return false;
          return matches(textOf(el), loc.text, !!loc.exact);
        }));
        found = Array.from(hits).filter((el) => !Array.from(el.children).some((c) => hits.has(c)));
        break;
      }
      case 'css':
        try { found = Array.from(document.querySelectorAll(loc.selector)); } catch (e) { fail('invalid_argument', 'bad CSS selector: ' + e.message); }
        break;
      case 'xpath': {
        let snap;
        try { snap = document.evaluate(loc.expr, document, null, XPathResult.ORDERED_NODE_SNAPSHOT_TYPE, null); } catch (e) { fail('invalid_argument', 'bad XPath: ' + e.message); }
        found = [];
        for (let i = 0; i < snap.snapshotLength; i++) { const n = snap.snapshotItem(i); if (n.nodeType === 1) found.push(n); }
        break;
      }
      default: fail('invalid_argument', 'unsupported locator ' + loc.by);
    }
    return found.slice(0, MAX_MATCHES).map((el) => info(el));
  }

  function deepHit(x, y) {
    let hit = document.elementFromPoint(x, y);
    while (hit && hit.shadowRoot) {
      const inner = hit.shadowRoot.elementFromPoint(x, y);
      if (!inner || inner === hit) break;
      hit = inner;
    }
    return hit;
  }

  function receives(el, hit) {
    if (!hit) return false;
    if (hit === el || el.contains(hit)) return true;
    // Clicking a control's label activates the control.
    const lbl = hit.closest && hit.closest('label');
    return !!(lbl && lbl.control === el);
  }

  function checkActionable(el) {
    // Until the parser finishes, the page's own scripts may not have attached
    // their handlers yet: input now could be silently lost.
    if (document.readyState === 'loading') fail('not_actionable', 'the page is still loading');
    if (!visible(el)) fail('not_actionable', refOf(el) + ' is not visible');
    if (!enabled(el)) fail('not_actionable', refOf(el) + ' is disabled');
  }

  function describeBrief(el) {
    if (!el) return 'nothing';
    const n = name(el);
    return '<' + el.localName + (el.id ? '#' + el.id : '') + '>' + (n ? ' "' + n.slice(0, 40) + '"' : '');
  }

  // Click effect detection: what observably changed after a click. Style-only
  // attribute churn (animations) is ignored; focus moving to a clicked button
  // is not an effect.
  let armed = null;
  const stateOf = (el) => JSON.stringify([el.isConnected, el.checked, el.value, el.open,
    ...['aria-expanded', 'aria-pressed', 'aria-selected', 'aria-checked'].map((a) => el.getAttribute(a))]);
  function arm(el) {
    if (armed) armed.obs.disconnect();
    const rec = { dom: 0 };
    const obs = new MutationObserver((ms) => {
      for (const m of ms) {
        if (m.type === 'childList') rec.dom += [...m.addedNodes, ...m.removedNodes].filter((n) => n.nodeType === 1).length;
        else if (m.type === 'attributes' && m.attributeName !== 'style') rec.dom++;
      }
    });
    obs.observe(document, { subtree: true, childList: true, attributes: true });
    armed = { el, state: stateOf(el), url: location.href, rec, obs };
  }
  async function clickEffect(maxMs) {
    const a = armed;
    if (!a) return null;
    const kinds = () => {
      const k = [];
      if (location.href !== a.url) k.push('url_changed');
      if (stateOf(a.el) !== a.state) k.push('state_changed');
      if (a.rec.dom) k.push('dom_changed');
      return k;
    };
    const t0 = performance.now();
    let k = kinds();
    while (!k.length && performance.now() - t0 < maxMs) {
      await new Promise((res) => setTimeout(res, 15));
      k = kinds();
    }
    a.obs.disconnect();
    armed = null;
    return k;
  }

  async function prepareClick(r) {
    const pt = await pointAt(r);
    const el = get(r);
    arm(el);
    guardClick(el);
    return pt;
  }

  // Hit-target check on every event of the click: the page can move the target
  // between measuring and pressing, or between press and release (a late layout
  // shift). From the first event that would land elsewhere, the rest of the
  // click is blocked before page handlers see it, and the click is reported as
  // missed so it can be retried at the new position.
  const GUARDED = ['pointerdown', 'mousedown', 'pointerup', 'mouseup', 'click'];
  let guard = null;
  function guardClick(el) {
    clickDone();
    const g = { missed: false, seen: 0, hit: null };
    g.listener = (e) => {
      if (!g.missed) {
        const t = e.composedPath()[0];
        if (t instanceof Element && receives(el, t)) { g.seen++; return; }
        g.missed = true;
        g.hit = t instanceof Element ? describeBrief(t) : 'nothing';
      }
      e.preventDefault();
      e.stopImmediatePropagation();
    };
    for (const t of GUARDED) addEventListener(t, g.listener, true);
    guard = g;
  }
  function clickDone() {
    const g = guard;
    guard = null;
    if (!g) return null;
    for (const t of GUARDED) removeEventListener(t, g.listener, true);
    return { missed: g.missed, seen: g.seen, hit: g.hit };
  }

  // Actionable centre point of the element, after scrolling it into view.
  async function pointAt(r) {
    const el = get(r);
    checkActionable(el);
    el.scrollIntoView({ block: 'center', inline: 'center', behavior: 'instant' });
    // Stability: only pay for a frame when an animation could be moving the
    // target. A just-started animation is "pending" and reports its start
    // position on every frame until it begins, so wait for it to start first.
    const anims = document.getAnimations().filter((x) => x.playState === 'running' || x.pending);
    if (anims.length) {
      const started = Promise.all(anims.filter((x) => x.pending).map((x) => x.ready.catch(() => {})));
      await Promise.race([started, new Promise((res) => setTimeout(res, 100))]);
    }
    const a = el.getBoundingClientRect();
    if (anims.length) await new Promise((res) => requestAnimationFrame(res));
    const b = el.getBoundingClientRect();
    if (Math.abs(a.x - b.x) > 1 || Math.abs(a.y - b.y) > 1 || Math.abs(a.width - b.width) > 1) {
      fail('not_actionable', r + ' is still moving');
    }
    const x = b.x + b.width / 2;
    const y = b.y + b.height / 2;
    const hit = deepHit(x, y);
    if (!receives(el, hit)) fail('not_actionable', r + ' is covered by ' + describeBrief(hit));
    return { x, y };
  }

  function deepActive() {
    let a = document.activeElement;
    while (a && a.shadowRoot && a.shadowRoot.activeElement) a = a.shadowRoot.activeElement;
    return a;
  }

  function focus(r, requireEditable, selectAll) {
    const el = get(r);
    checkActionable(el);
    if (requireEditable && !editable(el)) fail('not_actionable', r + ' is not editable');
    el.scrollIntoView({ block: 'center', inline: 'center', behavior: 'instant' });
    el.focus();
    const act = deepActive();
    if (act !== el && !el.contains(act)) fail('not_actionable', r + ' did not take focus');
    if (selectAll) {
      if (typeof el.select === 'function') el.select();
      else if (el.isContentEditable) {
        const range = document.createRange();
        range.selectNodeContents(el);
        const sel = getSelection();
        sel.removeAllRanges();
        sel.addRange(range);
      }
    }
    return true;
  }

  const NATIVE_FILL = new Set(['date', 'time', 'datetime-local', 'month', 'week', 'color', 'range']);
  // Returns true when the value was set directly; false when the caller should type it.
  function prepareFill(r, value) {
    const el = get(r);
    const ty = el.localName === 'input' ? (el.type || '').toLowerCase() : '';
    if (NATIVE_FILL.has(ty)) {
      checkActionable(el);
      if (el.readOnly) fail('not_actionable', r + ' is read-only');
      el.focus();
      const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
      setter.call(el, value);
      el.dispatchEvent(new Event('input', { bubbles: true, composed: true }));
      el.dispatchEvent(new Event('change', { bubbles: true }));
      return true;
    }
    focus(r, true, true);
    return false;
  }

  function readValue(r) {
    const el = get(r);
    if ('value' in el && (el.localName === 'input' || el.localName === 'textarea' || el.localName === 'select')) return el.value;
    if (el.isContentEditable) return el.innerText;
    return null;
  }

  function selectOption(r, opt) {
    const el = get(r);
    if (el.localName !== 'select') fail('invalid_argument', r + ' is not a <select>');
    checkActionable(el);
    const options = Array.from(el.options);
    const o = options.find((o) => o.value === opt) || options.find((o) => matches(o.label || o.text, opt, true));
    if (!o) fail('not_found', 'no option with value or label "' + opt + '"');
    if (o.disabled) fail('not_actionable', 'option "' + opt + '" is disabled');
    if (el.multiple) o.selected = true; else el.value = o.value;
    el.dispatchEvent(new Event('input', { bubbles: true, composed: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
    return Array.from(el.selectedOptions).map((x) => x.value);
  }

  // Client-rendered apps can still be an empty shell when `load` fires. Only then,
  // wait for content to appear and the DOM to go quiet for 50 ms, up to maxMs.
  // Only rendered content counts: apps often inject hidden sprites, pixels or
  // links before they render anything a user could see.
  function looksEmpty() {
    const b = document.body;
    if (!b) return true;
    if (norm(b.innerText)) return false;
    for (const el of b.querySelectorAll('input, button, select, textarea, a[href], img, svg, canvas, video, iframe')) {
      if (visible(el)) return false;
    }
    return true;
  }

  // A page is settled when it shows something and its scripts have stopped
  // adding or removing elements for QUIET_MS. Static pages are settled at load
  // and pay nothing; client-rendered apps (including splash screens that are
  // replaced) are waited for, up to maxEmptyMs while empty and BUSY_CAP_MS
  // while visible content keeps changing (tickers, feeds).
  const QUIET_MS = 150;
  const BUSY_CAP_MS = 1500;
  const busy = () => lastScriptChange > 0 && performance.now() - lastScriptChange < QUIET_MS;

  // An empty page is only waited for while its scripts are changing the DOM:
  // after EMPTY_GIVE_UP_MS without changes it is treated as genuinely empty,
  // and later snapshots of the same document don't wait for content again.
  const EMPTY_GIVE_UP_MS = 1000;
  let emptyAccepted = false;
  const wantsContent = () => !emptyAccepted && looksEmpty();

  async function settle(maxEmptyMs) {
    if (!maxEmptyMs || (!wantsContent() && !busy())) return {};
    const started = performance.now();
    for (;;) {
      await new Promise((res) => setTimeout(res, 16));
      const empty = wantsContent();
      const now = performance.now();
      const elapsed = now - started;
      if (!empty && !busy()) return { rendered_after_load_ms: Math.round(elapsed) };
      const quietFor = now - Math.max(lastScriptChange, started);
      if (empty && (quietFor >= EMPTY_GIVE_UP_MS || elapsed >= maxEmptyMs)) {
        emptyAccepted = true;
        return { rendered_after_load_ms: Math.round(elapsed), still_empty: true };
      }
      if (!empty && elapsed >= BUSY_CAP_MS) return { rendered_after_load_ms: Math.round(elapsed), still_changing: true };
    }
  }

  // Snapshots never describe a page mid-render: an empty page, or one whose
  // scripts just changed it, is given time to settle first. Settled pages pay nothing.
  async function snapshot(mode, max, maxEmptyMs) {
    const settled = await settle(maxEmptyMs);
    return { ...snapshotNow(mode, max), ...settled };
  }

  function snapshotNow(mode, max) {
    const all = allElements();
    const headings = [];
    const picked = [];
    const messages = [];
    for (const el of all) {
      if (!exposed(el)) continue;
      const r = role(el);
      if (r === 'heading' && headings.length < 40 && visible(el)) {
        const m = /^h([1-6])$/.exec(el.localName);
        headings.push({ level: m ? +m[1] : (+el.getAttribute('aria-level') || 2), text: name(el) });
      }
      if (r === 'dialog' || r === 'alertdialog' || r === 'alert' || r === 'status' || el.hasAttribute('aria-live')) {
        if (visible(el)) {
          const text = (name(el) || textOf(el)).slice(0, 300);
          if (text) messages.push({ kind: r === 'alertdialog' ? 'dialog' : (r || 'live'), text, ref: refOf(el) });
        }
      }
      if (el.getAttribute('aria-invalid') === 'true' || safeMatches(el, ':user-invalid')) {
        messages.push({ kind: 'validation', text: el.validationMessage || 'invalid value', ref: refOf(el) });
      }
      if (mode === 'compact') {
        if (!visible(el)) continue;
        const tab = el.getAttribute('tabindex');
        if (INTERACTIVE.has(r) || el.isContentEditable && el.getAttribute('contenteditable') !== null || (tab !== null && +tab >= 0 && !GENERIC.has(r))) {
          if (r === 'option' && el.localName === 'option') continue;
          picked.push(el);
        }
      } else {
        if (SKIP_TEXT.has(el.tagName)) continue;
        const ownText = Array.from(el.childNodes).some((n) => n.nodeType === 3 && n.textContent.trim());
        if ((r && !GENERIC.has(r)) || ownText) picked.push(el);
      }
    }
    return {
      url: location.href,
      title: document.title,
      headings,
      elements: picked.slice(0, max).map((el) => info(el)),
      messages,
      total_elements: picked.length,
      truncated: picked.length > max,
    };
  }

  function safeMatches(el, sel) { try { return el.matches(sel); } catch (_) { return false; } }

  globalThis.__sb = {
    query,
    describe: (r) => info(get(r)),
    readValue,
    prepareClick,
    clickDone,
    pointAt,
    readText: (r) => textOf(get(r)),
    element: (r) => get(r),
    fileNames: (r) => Array.from(get(r).files || [], (f) => f.name),
    // Identifies the current document: timeOrigin changes with every new document.
    docState: () => ({ origin: performance.timeOrigin, url: location.href, ready: document.readyState }),
    focus: (r) => focus(r, false, false),
    prepareFill,
    selectOption,
    snapshot,
    pageState: () => ({ url: location.href, title: document.title }),
    // Resolves with the page state once the window load event has fired.
    loaded: (maxRenderMs) => new Promise((res) => {
      const done = (extra) => res({ url: location.href, title: document.title, ...extra });
      const afterLoad = () => settle(maxRenderMs).then(done);
      if (document.readyState === 'complete') afterLoad();
      else addEventListener('load', afterLoad, { once: true });
    }),
    settle: (maxRenderMs) => settle(maxRenderMs),
    clickEffect,
  };
})();
