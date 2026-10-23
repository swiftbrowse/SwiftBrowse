// Installed into an isolated JS world: shares the DOM with the page but not
// its globals, so page scripts cannot read or tamper with it.
(() => {
  if (globalThis.__sb) return;
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

  function textOf(n) { return norm(n.innerText !== undefined ? n.innerText : n.textContent); }

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
          if (!r || (want === '*' ? GENERIC.has(r) : r !== want)) return false;
          return loc.name == null || matches(name(el), loc.name, !!loc.exact);
        });
        break;
      }
      case 'label':
        found = allElements().filter((el) => LABELABLE.has(role(el)) && matches(labelText(el), loc.text, !!loc.exact));
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
    if (!visible(el)) fail('not_actionable', refOf(el) + ' is not visible');
    if (!enabled(el)) fail('not_actionable', refOf(el) + ' is disabled');
  }

  function describeBrief(el) {
    if (!el) return 'nothing';
    const n = name(el);
    return '<' + el.localName + (el.id ? '#' + el.id : '') + '>' + (n ? ' "' + n.slice(0, 40) + '"' : '');
  }

  async function prepareClick(r) {
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

  function snapshot(mode, max) {
    const all = allElements();
    const headings = [];
    const picked = [];
    const messages = [];
    for (const el of all) {
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
    focus: (r) => focus(r, false, false),
    prepareFill,
    selectOption,
    snapshot,
    pageState: () => ({ url: location.href, title: document.title }),
    // Resolves with the page state once the window load event has fired.
    loaded: () => new Promise((res) => {
      const done = () => res({ url: location.href, title: document.title });
      if (document.readyState === 'complete') done();
      else addEventListener('load', done, { once: true });
    }),
  };
})();
