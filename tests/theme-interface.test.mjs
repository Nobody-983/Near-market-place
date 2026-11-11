import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';

const root = new URL('../frontend/Nft/', import.meta.url);

test('theme is applied before React, persisted, and exposed through an accessible switch', async () => {
  const [html, app, context, toggle, sidebar, auth] = await Promise.all([
    readFile(new URL('index.html', root), 'utf8'),
    readFile(new URL('src/App.jsx', root), 'utf8'),
    readFile(new URL('src/context/themeContext.jsx', root), 'utf8'),
    readFile(new URL('src/components/themeToggle.jsx', root), 'utf8'),
    readFile(new URL('src/components/sidebar.jsx', root), 'utf8'),
    readFile(new URL('src/pages/auth.jsx', root), 'utf8'),
  ]);
  assert.match(html, /near-nft-theme/);
  assert.ok(html.indexOf('near-nft-theme') < html.indexOf('src="/src/main.jsx"'));
  assert.match(html, /prefers-color-scheme: light/);
  assert.match(app, /<ThemeProvider>/);
  assert.match(sidebar, /<ThemeToggle inline/);
  assert.match(auth, /<ThemeToggle \/>/);
  assert.match(context, /localStorage\.setItem\(STORAGE_KEY, value\)/);
  assert.match(toggle, /aria-label=/);
});

test('global interaction and motion styles cover pointer, focus, loading transitions and reduced motion', async () => {
  const css = await readFile(new URL('src/index.css', root), 'utf8');
  assert.match(css, /button:not\(:disabled\)[\s\S]*cursor: pointer/);
  assert.match(css, /:focus-visible/);
  assert.match(css, /button:not\(:disabled\):active/);
  assert.match(css, /@keyframes modal-panel/);
  assert.match(css, /@media \(prefers-reduced-motion: reduce\)/);
  assert.match(css, /--near-accent: #00c781/);
  assert.match(css, /:root\[data-theme="light"\]/);
});
