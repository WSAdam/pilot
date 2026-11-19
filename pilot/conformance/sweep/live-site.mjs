// Drives a live public site through Playwright MCP on pilot, as an agent
// would, for compat/live-site.json (the v1 suite's live-site check, chosen
// by the user on 2026-11-19 in place of their own apps). The site is
// the-internet.herokuapp.com, built for testing browser automation: each flow
// below is one of its examples, done only with MCP tools (snapshot refs,
// clicks, typing, dialogs, uploads, tabs) and checked against what the page
// then shows. A measurement, not a test: a failing flow is a lead, and a
// pilot bug it finds gets a conformance test.
//
// Usage: npm run live-site  (PILOT_BIN selects the binary; default release build)
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';
import { freshProfileDir, noWindowEnv } from '../support.mjs';

const here = fileURLToPath(new URL('.', import.meta.url));
const pilot = process.env.PILOT_BIN ?? resolve(here, '../../../target/release/pilot');
const mcpCli = fileURLToPath(new URL('../node_modules/@playwright/mcp/cli.js', import.meta.url));
const SITE = 'https://the-internet.herokuapp.com';
const outputDir = await freshProfileDir('live-site-output');
const uploadFile = resolve(outputDir, 'pilot-upload.txt');
await writeFile(uploadFile, 'uploaded by pilot\n');

const client = new Client({ name: 'pilot-live-site', version: '0.1.0' });
await client.connect(new StdioClientTransport({
  command: process.execPath,
  args: [mcpCli, '--headless', '--isolated', '--output-dir', outputDir, '--executable-path', pilot],
  env: noWindowEnv,
  stderr: 'ignore',
}));

const text = (result) => (result.content ?? []).map((part) => part.text ?? '').join('\n');
async function call(name, args = {}) {
  const result = await client.callTool({ name, arguments: args });
  if (result.isError) throw new Error(text(result).split('\n').slice(0, 3).join(' ').slice(0, 300));
  return text(result);
}
const escape = (value) => value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
/** The ref of the first snapshot line whose role and name match, e.g. `button "Login"`. */
function refIn(snapshot, role, name) {
  const named = name === undefined ? '' : ` "${name instanceof RegExp ? name.source : escape(name)}"`;
  const pattern = new RegExp(`- ${role}${named}[^\\n]*\\[ref=((?:f\\d+)?e\\d+)\\]`);
  return snapshot.match(pattern)?.[1];
}
/** Clears a dialog or file chooser a failed flow left open, so the next flow starts clean. */
async function clearModalState() {
  await client.callTool({ name: 'browser_handle_dialog', arguments: { accept: false } }).catch(() => {});
  await client.callTool({ name: 'browser_file_upload', arguments: {} }).catch(() => {});
}
async function open(path) {
  await clearModalState();
  await call('browser_navigate', { url: `${SITE}${path}` });
  return call('browser_snapshot', {});
}
async function target(role, name) {
  const snapshot = await call('browser_snapshot', {});
  const ref = refIn(snapshot, role, name);
  if (!ref) throw new Error(`no ${role} "${name}" in the snapshot`);
  return ref;
}
async function click(role, name) {
  return call('browser_click', { element: `${role} ${name}`, target: await target(role, name) });
}
const evaluate = (fn) => call('browser_evaluate', { function: fn });
/** Polls the page's visible text until it contains `wanted`. */
async function waitForText(wanted, timeoutMs = 15_000) {
  const deadline = Date.now() + timeoutMs;
  let shown = '';
  while (Date.now() < deadline) {
    shown = await evaluate('() => document.body.innerText');
    if (shown.includes(wanted)) return;
    await new Promise((done) => setTimeout(done, 500));
  }
  throw new Error(`"${wanted}" never appeared: ${shown.replace(/\s+/g, ' ').slice(0, 160)}`);
}
async function expectSnapshot(wanted) {
  const snapshot = await call('browser_snapshot', {});
  if (!snapshot.includes(wanted)) throw new Error(`snapshot lacks "${wanted}"`);
}

// The Login button's name starts with its Font Awesome icon's glyph.
const LOGIN_BUTTON = /[^"]*Login/;

const flows = {
  'log in and out (form authentication)': async () => {
    const snapshot = await open('/login');
    await call('browser_fill_form', { fields: [
      { name: 'Username', type: 'textbox', target: refIn(snapshot, 'textbox', 'Username'), value: 'tomsmith' },
      { name: 'Password', type: 'textbox', target: refIn(snapshot, 'textbox', 'Password'), value: 'SuperSecretPassword!' },
    ] });
    await click('button', LOGIN_BUTTON);
    await waitForText('You logged into a secure area!');
    await click('link', 'Logout');
    await waitForText('You logged out of the secure area!');
  },
  'a wrong password is refused': async () => {
    const snapshot = await open('/login');
    await call('browser_fill_form', { fields: [
      { name: 'Username', type: 'textbox', target: refIn(snapshot, 'textbox', 'Username'), value: 'tomsmith' },
      { name: 'Password', type: 'textbox', target: refIn(snapshot, 'textbox', 'Password'), value: 'wrong' },
    ] });
    await click('button', LOGIN_BUTTON);
    await waitForText('Your password is invalid!');
  },
  'check and uncheck checkboxes': async () => {
    const snapshot = await open('/checkboxes');
    const boxes = [...snapshot.matchAll(/- checkbox[^\n]*\[ref=((?:f\d+)?e\d+)\]/g)].map((match) => match[1]);
    if (boxes.length !== 2) throw new Error(`${boxes.length} checkboxes in the snapshot`);
    await call('browser_click', { element: 'checkbox 1', target: boxes[0] });
    await call('browser_click', { element: 'checkbox 2', target: boxes[1] });
    const states = await evaluate('() => [...document.querySelectorAll("#checkboxes input")].map((box) => box.checked).join()');
    if (!states.includes('true,false')) throw new Error(`checked states ${states}`);
  },
  'pick from a dropdown': async () => {
    await open('/dropdown');
    await call('browser_select_option', { element: 'Dropdown', target: await target('combobox'), values: ['Option 2'] });
    const value = await evaluate('() => document.getElementById("dropdown").value');
    if (!value.includes('2')) throw new Error(`dropdown value ${value}`);
  },
  'accept a JavaScript alert': async () => {
    await open('/javascript_alerts');
    const clicked = await click('button', 'Click for JS Alert');
    if (!/dialog/i.test(clicked)) throw new Error('no dialog after the click');
    await call('browser_handle_dialog', { accept: true });
    await waitForText('You successfully clicked an alert');
  },
  'dismiss a confirm': async () => {
    await open('/javascript_alerts');
    await click('button', 'Click for JS Confirm');
    await call('browser_handle_dialog', { accept: false });
    await waitForText('You clicked: Cancel');
  },
  'answer a prompt': async () => {
    await open('/javascript_alerts');
    await click('button', 'Click for JS Prompt');
    await call('browser_handle_dialog', { accept: true, promptText: 'pilot' });
    await waitForText('You entered: pilot');
  },
  'wait for content loaded after a delay': async () => {
    await open('/dynamic_loading/2');
    await click('button', 'Start');
    await waitForText('Hello World!');
  },
  'enable a disabled input and type in it': async () => {
    await open('/dynamic_controls');
    await click('button', 'Enable');
    await waitForText("It's enabled!");
    const snapshot = await call('browser_snapshot', {});
    const box = [...snapshot.matchAll(/- textbox[^\n]*\[ref=((?:f\d+)?e\d+)\]/g)].map((match) => match[1]).at(-1);
    await call('browser_type', { element: 'text input', target: box, text: 'typed by pilot' });
    const value = await evaluate('() => document.querySelector("#input-example input").value');
    if (!value.includes('typed by pilot')) throw new Error(`input value ${value}`);
  },
  'add and remove elements': async () => {
    await open('/add_remove_elements/');
    await click('button', 'Add Element');
    await click('button', 'Add Element');
    await click('button', 'Delete');
    const count = await evaluate('() => document.querySelectorAll("#elements button").length');
    if (!/\b1\b/.test(count)) throw new Error(`${count} Delete buttons, expected 1`);
  },
  'upload a file': async () => {
    const snapshot = await open('/upload');
    const chooser = refIn(snapshot, 'button', /Choose File[^"]*/) ?? refIn(snapshot, 'button');
    const clicked = await call('browser_click', { element: 'file input', target: chooser });
    if (!/file chooser/i.test(clicked)) throw new Error('no file chooser after the click');
    await call('browser_file_upload', { paths: [uploadFile] });
    await click('button', 'Upload');
    await waitForText('File Uploaded!');
    await waitForText('pilot-upload.txt');
  },
  'hover to reveal a caption': async () => {
    await open('/hovers');
    await call('browser_hover', { element: 'first avatar', target: await target('img', 'User Avatar') });
    await expectSnapshot('name: user1');
  },
  'press a key in a field': async () => {
    await open('/key_presses');
    await call('browser_click', { element: 'key field', target: await target('textbox') });
    await call('browser_press_key', { key: 'a' });
    await waitForText('You entered: A');
  },
  'swap two boxes by drag and drop': async () => {
    const snapshot = await open('/drag_and_drop');
    // Each box shows its letter: `generic [ref=e12]: A` (or its header's line).
    const shown = (letter) => snapshot.match(new RegExp(`\\[ref=((?:f\\d+)?e\\d+)\\]: ${letter}$`, 'm'))?.[1];
    const [from, to] = [shown('A'), shown('B')];
    if (!from || !to) throw new Error('no refs for the two boxes');
    await call('browser_drag', { startElement: 'box A', startTarget: from, endElement: 'box B', endTarget: to });
    const order = await evaluate('() => [...document.querySelectorAll("#columns header")].map((header) => header.textContent).join()');
    if (!order.includes('B,A')) throw new Error(`box order ${order}`);
  },
  'open a link in a new window and switch to it': async () => {
    await open('/windows');
    await click('link', 'Click Here');
    const tabs = await call('browser_tabs', { action: 'list' });
    if (!/New Window/.test(tabs)) throw new Error(`tabs: ${tabs.slice(0, 200)}`);
    await call('browser_tabs', { action: 'select', index: 1 });
    await expectSnapshot('New Window');
    await call('browser_tabs', { action: 'close', index: 1 });
  },
  'read text from nested frames': async () => {
    await open('/nested_frames');
    for (const word of ['LEFT', 'MIDDLE', 'RIGHT', 'BOTTOM']) await expectSnapshot(word);
  },
};

const results = [];
for (const [flow, run] of Object.entries(flows)) {
  const started = Date.now();
  try {
    await run();
    results.push({ flow, ok: true, ms: Date.now() - started });
    console.log(`✅ ${flow}`);
  } catch (error) {
    const message = String(error.message).split('\n')[0].slice(0, 240);
    results.push({ flow, ok: false, ms: Date.now() - started, error: message });
    console.log(`❌ ${flow} — ${message}`);
  }
}
await client.close();

const passed = results.filter((result) => result.ok).length;
const report = { date: new Date().toISOString().slice(0, 10), site: SITE, pilot, passed, total: results.length, results };
const compat = resolve(here, '../../../compat');
await mkdir(compat, { recursive: true });
await writeFile(resolve(compat, 'live-site.json'), `${JSON.stringify(report, null, 2)}\n`);
console.log(`🔍 ${passed}/${results.length} live-site flows pass; wrote compat/live-site.json`);
