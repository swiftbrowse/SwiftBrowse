// Builds the documentation site into docs/dist: the Markdown pages in
// docs/pages, plus two references generated from the binary itself (every
// operation with its arguments, from `swiftbrowse sdk --schema`, and every
// command with its options, from `--help`), so they can't drift from the code.
//
// Usage: node build.mjs [path-to-swiftbrowse-binary]   (default: ../target/release/swiftbrowse)

import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { Marked } from 'marked';

const here = path.dirname(fileURLToPath(import.meta.url));
const BIN = path.resolve(process.argv[2] || path.join(here, '../target/release/swiftbrowse'));
const OUT = path.join(here, 'dist');
const REPO = 'https://github.com/swiftbrowse/SwiftBrowse';

// Sidebar order. `generated` pages are written below instead of read from pages/.
const NAV = [
  ['Start', [
    ['index', 'Overview'],
    ['getting-started', 'Getting started'],
  ]],
  ['Writing tests', [
    ['writing-tests', 'Test files'],
    ['locators', 'Targets and locators'],
    ['assertions', 'Assertions'],
    ['network', 'Network mocking'],
    ['auth', 'Saved logins'],
    ['tabs-frames-uploads', 'Tabs, frames and uploads'],
  ]],
  ['Running tests', [
    ['test-runner', 'The test runner'],
    ['debugging', 'Traces, video and reports'],
    ['ci', 'Continuous integration'],
  ]],
  ['Tests in code', [
    ['typescript', 'TypeScript and JavaScript'],
    ['python', 'Python'],
  ]],
  ['AI agents', [
    ['mcp', 'MCP server'],
  ]],
  ['Reference', [
    ['operations', 'Operations', 'generated'],
    ['cli', 'Command line', 'generated'],
    ['results', 'Results and errors'],
    ['configuration', 'Configuration and security'],
  ]],
  ['More', [
    ['benchmarks', 'Benchmarks'],
    ['limitations', 'Limitations and roadmap'],
  ]],
];

const version = execFileSync(BIN, ['--version']).toString().trim().split(' ').pop();
const { ops } = JSON.parse(execFileSync(BIN, ['sdk', '--schema']).toString());

// ---------------------------------------------------------------------------
// Markdown with heading anchors

const slugify = (s) => s.toLowerCase().replace(/<[^>]+>/g, '').replace(/[`*_]/g, '').replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');
const escapeHtml = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

function render(md) {
  const headings = [];
  const used = new Set();
  const marked = new Marked({
    gfm: true,
    renderer: {
      heading({ tokens, depth, text }) {
        const html = this.parser.parseInline(tokens);
        let id = slugify(text);
        for (let n = 2; used.has(id); n++) id = `${slugify(text)}-${n}`;
        used.add(id);
        if (depth === 2 || depth === 3) headings.push({ depth, id, text: html.replace(/<[^>]+>/g, '') });
        return `<h${depth} id="${id}"><a class="anchor" href="#${id}" aria-hidden="true">#</a>${html}</h${depth}>\n`;
      },
      code({ text, lang }) {
        return `<div class="code"><button class="copy" type="button" aria-label="Copy">Copy</button><pre><code${lang ? ` class="lang-${escapeHtml(lang)}"` : ''}>${escapeHtml(text)}</code></pre></div>\n`;
      },
    },
  });
  // Rewrite links to other pages: `writing-tests.md#x` → `writing-tests.html#x`.
  marked.use({ walkTokens(t) { if (t.type === 'link' && /^[a-z0-9-]+\.md(#|$)/.test(t.href)) t.href = t.href.replace(/\.md/, '.html'); } });
  // Wrap tables so wide ones scroll instead of widening the page.
  const html = marked.parse(md).replace(/<table>/g, '<div class="table"><table>').replace(/<\/table>/g, '</table></div>');
  return { html, headings };
}

// ---------------------------------------------------------------------------
// Generated pages

const camel = (s) => s.replace(/_([a-z])/g, (_, c) => c.toUpperCase());

function typeOf(schema, defs) {
  if (!schema) return 'any';
  if (schema.$ref) {
    const name = schema.$ref.split('/').pop();
    if (name === 'Target') return 'target';
    if (name === 'Condition') return 'condition';
    const def = defs?.[name];
    if (def?.enum) return def.enum.map((v) => `"${v}"`).join(' | ');
    if (def?.oneOf?.every((o) => 'const' in o)) return def.oneOf.map((o) => `"${o.const}"`).join(' | ');
    return name;
  }
  if (schema.anyOf) {
    const types = schema.anyOf.map((s) => typeOf(s, defs)).filter((t) => t !== 'null');
    return (types.length > 1 ? types.filter((t) => t !== 'any') : types).join(' | ');
  }
  if (schema.oneOf?.every((o) => 'const' in o)) return schema.oneOf.map((o) => `"${o.const}"`).join(' | ');
  if (schema.enum) return schema.enum.map((v) => `"${v}"`).join(' | ');
  const types = [].concat(schema.type || []).filter((t) => t !== 'null');
  if (types[0] === 'array') return `${typeOf(schema.items, defs)}[]`;
  return types.join(' | ') || 'any';
}

// Descriptions for arguments the schemas leave undocumented because they mean the same everywhere.
const COMMON = {
  timeout_ms: "How long to wait, in ms. Default: the session's timeout (10 s; in `swiftbrowse test`, `--expect-timeout-ms`).",
  conditions: 'Every condition must hold: see [Assertions](assertions.md).',
};

const ONE_LINE = (s) => (s || '').replace(/\s*\n\s*/g, ' ').trim();

function operationsPage() {
  const groups = [
    ['Navigation', ['navigate', 'go_back', 'go_forward', 'reload']],
    ['Reading the page', ['get_page_snapshot', 'get_accessibility_tree', 'find_element', 'get_screenshot', 'get_dom_snapshot']],
    ['Actions', ['click', 'hover', 'check', 'fill', 'type_text', 'press_key', 'select_option', 'upload_file', 'evaluate']],
    ['Waiting and assertions', ['wait_for', 'assert_page_state']],
    ['Network', ['route', 'unroute', 'list_routes', 'get_requests']],
    ['Tabs and frames', ['new_tab', 'list_tabs', 'switch_tab', 'close_tab', 'list_frames', 'switch_frame']],
    ['Saved state', ['save_storage_state', 'load_storage_state']],
    ['Video', ['start_video', 'stop_video']],
    ['Diagnostics and reports', ['get_console_errors', 'get_network_failures', 'get_test_report', 'get_capabilities']],
  ];
  const listed = new Set(groups.flatMap(([, names]) => names));
  const rest = ops.map((o) => o.name).filter((n) => !listed.has(n));
  if (rest.length) groups.push(['Other', rest]);

  let md = `# Operations\n\nEvery operation SwiftBrowse can perform. This page is generated from the engine's own schemas (\`swiftbrowse sdk --schema\`, version ${version}), so it matches the binary exactly.\n\nThe same operation has the same name and arguments everywhere:\n\n- **Test files and \`run\` scripts:** a step \`{ "op": "fill", "target": "Email", "value": "ada@example.com" }\`. Test files also have [\`expect\`](assertions.md).\n- **MCP:** a tool named after the operation, with the arguments as its input.\n- **TypeScript SDK:** \`page.fill('Email', 'ada@example.com', { timeout_ms: 2000 })\`, a camelCase method, with required arguments in order and the rest in an options object.\n- **Python SDK:** \`page.fill("Email", "ada@example.com", timeout_ms=2000)\`, with required arguments positionally and the rest by keyword.\n\nA \`target\` is a string or a locator object: see [Targets and locators](locators.md). A \`condition\` is described in [Assertions](assertions.md). Every operation returns an [ActionResult](results.md).\n\n`;
  md += groups.map(([title, names]) => `- **${title}:** ${names.map((n) => `[\`${n}\`](#${slugify(n)})`).join(', ')}`).join('\n') + '\n';

  for (const [title, names] of groups) {
    md += `\n## ${title}\n`;
    for (const name of names) {
      const op = ops.find((o) => o.name === name);
      if (!op) continue;
      const s = op.schema;
      const required = s.required || [];
      const props = Object.entries(s.properties || {});
      const order = [...required, ...props.map(([k]) => k).filter((k) => !required.includes(k))];
      md += `\n### ${name}\n\n${ONE_LINE(op.description)}\n\n`;
      if (props.length) {
        md += '| Argument | Type | Description |\n|---|---|---|\n';
        for (const k of order) {
          const p = s.properties[k];
          const type = typeOf(p, s.$defs);
          let desc = type === 'target'
            ? 'A string or a locator: see [Targets and locators](locators.md).'
            : type === 'condition'
              ? 'See [Assertions](assertions.md).'
              : ONE_LINE(p.description || (p.$ref && s.$defs?.[p.$ref.split('/').pop()]?.description) || COMMON[k] || '');
          if (p.default !== undefined && p.default !== null) desc += `${desc ? ' ' : ''}Default: \`${JSON.stringify(p.default)}\`.`;
          md += `| \`${k}\`${required.includes(k) ? ' (required)' : ''} | ${type.replace(/\|/g, '\\|')} | ${desc.replace(/\|/g, '\\|')} |\n`;
        }
      } else {
        md += 'No arguments.\n';
      }
      const pos = required;
      const tsArgs = [...pos, ...(props.length > required.length ? ['{ …options }'] : [])].join(', ');
      const pyArgs = [...pos, ...(props.length > required.length ? ['**options'] : [])].join(', ');
      md += `\nTypeScript: \`page.${camel(name)}(${tsArgs})\` · Python: \`page.${name}(${pyArgs})\`\n`;
    }
  }
  return md;
}

function cliPage() {
  const help = (args) => execFileSync(BIN, [...args, '--help']).toString().trimEnd();
  let md = `# Command line\n\nEvery \`swiftbrowse\` command and option, generated from \`--help\` of version ${version}.\n\n`;
  md += '```text\n' + help([]) + '\n```\n';
  for (const cmd of ['test', 'run', 'mcp', 'sdk', 'doctor', 'snapshot', 'screenshot', 'capabilities', 'daemon']) {
    md += `\n## swiftbrowse ${cmd}\n\n\`\`\`text\n${help([cmd])}\n\`\`\`\n`;
  }
  md += '\n## Exit codes\n\n`swiftbrowse test`: `0` every test passed (flaky tests count as passed), `1` a test failed, `2` the configuration or a test file is invalid (nothing ran). `swiftbrowse run`: `0` when every step succeeded, otherwise `1`.\n';
  return md;
}

// ---------------------------------------------------------------------------
// Layout

const CSS = fs.readFileSync(path.join(here, 'theme.css'), 'utf8');
const JS = fs.readFileSync(path.join(here, 'site.js'), 'utf8');

function page({ slug, title, html, headings }) {
  const nav = NAV.map(([group, items]) => `<div class="group"><div class="group-title">${group}</div>${items
    .map(([s, label]) => `<a href="${s}.html"${s === slug ? ' aria-current="page"' : ''}>${label}</a>`).join('')}</div>`).join('');
  const toc = headings.filter((h) => h.depth === 2);
  const flat = NAV.flatMap(([, items]) => items);
  const i = flat.findIndex(([s]) => s === slug);
  const prev = flat[i - 1], next = flat[i + 1];
  const pager = `<nav class="pager">${prev ? `<a class="prev" href="${prev[0]}.html"><span>Previous</span>${prev[1]}</a>` : '<span></span>'}${next ? `<a class="next" href="${next[0]}.html"><span>Next</span>${next[1]}</a>` : ''}</nav>`;
  const pageTitle = slug === 'index' ? 'SwiftBrowse Docs' : `${title} · SwiftBrowse Docs`;
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${escapeHtml(pageTitle)}</title>
<meta name="description" content="SwiftBrowse: the AI browser for browser automation and end-to-end testing. ${escapeHtml(title)}.">
<link rel="icon" href="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 32 32'%3E%3Crect width='32' height='32' rx='7' fill='%232563eb'/%3E%3Cpath d='M9 22 L16 9 L23 22' stroke='white' stroke-width='3.2' fill='none' stroke-linecap='round' stroke-linejoin='round'/%3E%3C/svg%3E">
<style>${CSS}</style>
</head>
<body>
<header class="top">
  <button class="menu" type="button" aria-label="Menu" aria-expanded="false">☰</button>
  <a class="brand" href="index.html"><span class="logo" aria-hidden="true"></span>SwiftBrowse <span class="ver">v${version}</span></a>
  <input class="search" type="search" placeholder="Search the docs" aria-label="Search the docs">
  <div class="results" hidden></div>
  <a class="gh" href="${REPO}">GitHub</a>
</header>
<div class="shell">
  <aside class="side"><nav>${nav}</nav></aside>
  <main>
    <article>${html}</article>
    ${pager}
    <footer>SwiftBrowse ${version} · Apache-2.0 · <a href="${REPO}/issues">Report an issue</a> · <a href="${REPO}/edit/main/docs/pages/${slug}.md">Edit this page</a></footer>
  </main>
  <aside class="toc">${toc.length > 1 ? `<div class="toc-title">On this page</div>${toc.map((h) => `<a href="#${h.id}">${escapeHtml(h.text)}</a>`).join('')}` : ''}</aside>
</div>
<script>${JS}</script>
</body>
</html>
`;
}

// ---------------------------------------------------------------------------

fs.rmSync(OUT, { recursive: true, force: true });
fs.mkdirSync(OUT, { recursive: true });
const search = [];
for (const [, items] of NAV) {
  for (const [slug, label, kind] of items) {
    const md = kind === 'generated'
      ? { operations: operationsPage, cli: cliPage }[slug]()
      : fs.readFileSync(path.join(here, 'pages', `${slug}.md`), 'utf8').replaceAll('{{version}}', version);
    const { html, headings } = render(md);
    const title = (md.match(/^# (.+)$/m) || [, label])[1].replace(/`/g, '');
    fs.writeFileSync(path.join(OUT, `${slug}.html`), page({ slug, title, html, headings }));
    search.push({ u: `${slug}.html`, t: title, p: label });
    for (const h of headings) {
      const op = slug === 'operations' && ops.find((o) => o.name === h.text);
      search.push({ u: `${slug}.html#${h.id}`, t: h.text, p: label, ...(op ? { d: ONE_LINE(op.description) } : {}) });
    }
  }
}
fs.writeFileSync(path.join(OUT, 'search.json'), JSON.stringify(search));
console.log(`built ${search.filter((s) => !s.u.includes('#')).length} pages for SwiftBrowse ${version} into ${path.relative(process.cwd(), OUT) || '.'}`);
