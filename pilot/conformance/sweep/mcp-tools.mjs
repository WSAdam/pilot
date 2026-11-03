// Calls every Playwright MCP tool once against pilot and records which work,
// for compat/mcp-tools.json. A measurement, not a test: failures here are
// gaps to close (each fix then gets a conformance test).
//
// Usage: npm run sweep  (PILOT_BIN selects the binary; default release build)
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';
import { freshProfileDir, noWindowEnv, startFixtureServer } from '../support.mjs';

const here = fileURLToPath(new URL('.', import.meta.url));
const pilot = process.env.PILOT_BIN ?? resolve(here, '../../../target/release/pilot');
const mcpCli = fileURLToPath(new URL('../node_modules/@playwright/mcp/cli.js', import.meta.url));
const mcpVersion = (await import('@playwright/mcp/package.json', { with: { type: 'json' } })).default.version;
const fixture = await startFixtureServer();
const sweepUrl = `${fixture.baseUrl}/sweep.html`;
const outputDir = await freshProfileDir('sweep-output');
const uploadFile = resolve(outputDir, 'sweep.txt');
await writeFile(uploadFile, 'sweep upload\n');

const client = new Client({ name: 'pilot-sweep', version: '0.1.0' });
await client.connect(new StdioClientTransport({
  command: process.execPath,
  args: [mcpCli, '--headless', '--isolated', '--caps', 'vision,pdf,testing', '--output-dir', outputDir, '--executable-path', pilot],
  env: noWindowEnv,
  stderr: 'ignore',
}));

const text = (result) => (result.content ?? []).map((part) => part.text ?? '').join('\n');
async function call(name, args = {}) {
  const result = await client.callTool({ name, arguments: args });
  if (result.isError) throw new Error(text(result).split('\n').slice(0, 3).join(' ').slice(0, 300));
  return text(result);
}
const refFor = (snapshot, label) => snapshot.match(new RegExp(`${label}[^\\n]*\\[ref=((?:f\\d+)?e\\d+)\\]`))?.[1];
// Generic elements show their text after the ref: `generic [ref=e17]: Hover target`.
const refForText = (snapshot, shown) => snapshot.match(new RegExp(`\\[ref=((?:f\\d+)?e\\d+)\\]: ${shown}$`, 'm'))?.[1];
async function fresh() {
  await call('browser_navigate', { url: sweepUrl });
  return call('browser_snapshot', {});
}
async function ref(label) {
  const snapshot = await call('browser_snapshot', {});
  const found = refFor(snapshot, label) ?? refForText(snapshot, label);
  if (!found) throw new Error(`no ref for ${label}`);
  return found;
}
const page = (fn) => call('browser_evaluate', { function: fn });
async function expectOut(expected) {
  const shown = await page('() => document.getElementById("out").textContent');
  if (!shown.includes(expected)) throw new Error(`#out is not "${expected}": ${shown.slice(0, 200)}`);
}
async function center(selector) {
  const shown = await page(`() => { const r = document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect(); return JSON.stringify([r.x + r.width / 2, r.y + r.height / 2]); }`);
  const [x, y] = JSON.parse(shown.match(/\[[^\]]+\]/)[0].replace(/\\"/g, '"'));
  return { x: Math.round(x), y: Math.round(y) };
}

const steps = {
  browser_navigate: async () => { await call('browser_navigate', { url: sweepUrl }); },
  browser_snapshot: async () => { if (!/heading "Sweep"/.test(await fresh())) throw new Error('no heading in snapshot'); },
  browser_click: async () => { await fresh(); await call('browser_click', { element: 'Say hi', target: await ref('button "Say hi"') }); await expectOut('hi'); },
  'browser_click (double)': async () => { await fresh(); await call('browser_click', { element: 'Say hi', target: await ref('button "Say hi"'), doubleClick: true }); await expectOut('double hi'); },
  browser_type: async () => { await fresh(); await call('browser_type', { element: 'Name', target: await ref('textbox "Name"'), text: 'Ada' }); const value = await page('() => document.getElementById("name").value'); if (!value.includes('Ada')) throw new Error(value); },
  browser_fill_form: async () => {
    const snapshot = await fresh();
    await call('browser_fill_form', { fields: [
      { name: 'Name', type: 'textbox', target: refFor(snapshot, 'textbox "Name"'), value: 'Grace' },
      { name: 'Agree', type: 'checkbox', target: refFor(snapshot, 'checkbox "Agree"'), value: 'true' },
      { name: 'Color', type: 'combobox', target: refFor(snapshot, 'combobox "Color"'), value: 'green' },
    ] });
    const values = await page('() => ["name", "agree", "color"].map((id) => { const field = document.getElementById(id); return id === "agree" ? field.checked : field.value; }).join()');
    if (!values.includes('Grace,true,green')) throw new Error(values);
  },
  browser_select_option: async () => { await fresh(); await call('browser_select_option', { element: 'Color', target: await ref('combobox "Color"'), values: ['green'] }); const value = await page('() => document.getElementById("color").value'); if (!value.includes('green')) throw new Error(value); },
  browser_hover: async () => { await fresh(); await call('browser_hover', { element: 'Hover target', target: await ref('Hover target') }); await expectOut('hovered'); },
  browser_drag: async () => { const snapshot = await fresh(); await call('browser_drag', { startElement: 'Drag me', startTarget: refForText(snapshot, 'Drag me'), endElement: 'Drop here', endTarget: refForText(snapshot, 'Drop here') }); await expectOut('dropped'); },
  browser_drop: async () => { await fresh(); await call('browser_drop', { element: 'Drop here', target: await ref('Drop here'), data: { 'text/plain': 'dropped-data' } }); await expectOut('dropped'); },
  browser_press_key: async () => { await fresh(); await call('browser_press_key', { key: 'ArrowDown' }); await expectOut('key ArrowDown'); },
  browser_handle_dialog: async () => { await fresh(); const clicked = await call('browser_click', { element: 'Confirm', target: await ref('button "Confirm"') }); if (!/dialog/i.test(clicked)) throw new Error('no modal dialog state after the click'); await call('browser_handle_dialog', { accept: true }); await expectOut('confirmed'); },
  browser_file_upload: async () => { await fresh(); const clicked = await call('browser_click', { element: 'File', target: await ref('button "File"') }); if (!/file chooser/i.test(clicked)) throw new Error('no file chooser state after the click'); await call('browser_file_upload', { paths: [uploadFile] }); await expectOut('file sweep.txt'); },
  browser_evaluate: async () => { await fresh(); if (!(await page('() => document.title')).includes('sweep')) throw new Error('title'); },
  browser_console_messages: async () => { await fresh(); await call('browser_click', { element: 'Log and fetch', target: await ref('button "Log and fetch"') }); const messages = await call('browser_console_messages', { level: 'info' }); if (!messages.includes('sweep log')) throw new Error(messages.slice(0, 200)); },
  browser_network_requests: async () => { await fresh(); await call('browser_click', { element: 'Log and fetch', target: await ref('button "Log and fetch"') }); await expectOut('fetched'); const requests = await call('browser_network_requests', { static: false }); if (!requests.includes('greeting.html')) throw new Error(requests.slice(0, 200)); },
  browser_network_request: async () => { await fresh(); await call('browser_network_request', { index: 1 }); },
  browser_navigate_back: async () => { await fresh(); await call('browser_navigate', { url: `${fixture.baseUrl}/greeting.html` }); await call('browser_navigate_back', {}); if (!(await page('() => location.pathname')).includes('sweep')) throw new Error('not back'); },
  browser_tabs: async () => {
    await fresh();
    await call('browser_tabs', { action: 'new', url: `${fixture.baseUrl}/greeting.html` });
    const list = await call('browser_tabs', { action: 'list' });
    if (!list.includes('greeting')) throw new Error(list.slice(0, 200));
    await call('browser_tabs', { action: 'select', index: 0 });
    await call('browser_tabs', { action: 'close', index: 1 });
  },
  browser_take_screenshot: async () => { await fresh(); await call('browser_take_screenshot', { scale: 'css' }); },
  'browser_take_screenshot (full page)': async () => { await fresh(); await call('browser_take_screenshot', { fullPage: true, scale: 'css' }); },
  'browser_take_screenshot (element)': async () => { await fresh(); await call('browser_take_screenshot', { element: 'Say hi', target: await ref('button "Say hi"'), scale: 'css' }); },
  browser_resize: async () => { await fresh(); await call('browser_resize', { width: 800, height: 600 }); const width = await page('() => innerWidth'); await call('browser_resize', { width: 1280, height: 720 }); if (!width.includes('800')) throw new Error(width); },
  browser_emulate_media: async () => { await fresh(); await call('browser_emulate_media', { colorScheme: 'dark' }); const dark = await page('() => matchMedia("(prefers-color-scheme: dark)").matches'); await call('browser_emulate_media', { colorScheme: 'light' }); if (!dark.includes('true')) throw new Error(dark); },
  browser_wait_for: async () => { await fresh(); await call('browser_wait_for', { text: 'Sweep' }); },
  browser_find: async () => { await fresh(); const found = await call('browser_find', { text: 'Hover target' }); if (!found.includes('Hover target')) throw new Error(found.slice(0, 200)); },
  browser_pdf_save: async () => { await fresh(); await call('browser_pdf_save', {}); },
  browser_mouse_click_xy: async () => { await fresh(); await call('browser_mouse_click_xy', await center('#hi')); await expectOut('hi'); },
  browser_mouse_move_xy: async () => { await fresh(); await call('browser_mouse_move_xy', await center('#hover')); await expectOut('hovered'); },
  'browser_mouse_down / browser_mouse_up': async () => { await fresh(); await call('browser_mouse_move_xy', await center('h1')); await call('browser_mouse_down', {}); await call('browser_mouse_up', {}); await expectOut('down-up'); },
  browser_mouse_drag_xy: async () => { await fresh(); const start = await center('#source'); const end = await center('#target'); await call('browser_mouse_drag_xy', { startX: start.x, startY: start.y, endX: end.x, endY: end.y }); await expectOut('dropped'); },
  browser_mouse_wheel: async () => { await fresh(); await call('browser_mouse_move_xy', await center('h1')); await call('browser_mouse_wheel', { deltaX: 0, deltaY: 400 }); const scrolled = await page('() => scrollY'); if (!/[1-9]/.test(scrolled)) throw new Error(`scrollY ${scrolled}`); },
  browser_run_code_unsafe: async () => { await fresh(); const result = await call('browser_run_code_unsafe', { code: 'async (page) => await page.title()' }); if (!result.includes('sweep')) throw new Error(result.slice(0, 200)); },
  browser_generate_locator: async () => { await fresh(); const locator = await call('browser_generate_locator', { element: 'Name', target: await ref('textbox "Name"') }); if (!/Name/.test(locator)) throw new Error(locator.slice(0, 200)); },
  browser_verify_element_visible: async () => { await fresh(); await call('browser_verify_element_visible', { role: 'button', accessibleName: 'Say hi' }); },
  browser_verify_text_visible: async () => { await fresh(); await call('browser_verify_text_visible', { text: 'Sweep' }); },
  browser_verify_list_visible: async () => { await fresh(); await call('browser_verify_list_visible', { element: 'Fruits', target: await ref('list "Fruits"'), items: ['Apple', 'Banana'] }); },
  browser_verify_value: async () => { await fresh(); await call('browser_type', { element: 'Name', target: await ref('textbox "Name"'), text: 'Lin' }); await call('browser_verify_value', { type: 'textbox', element: 'Name', target: await ref('textbox "Name"'), value: 'Lin' }); },
  browser_close: async () => { await fresh(); await call('browser_close', {}); },
};

const results = [];
for (const [name, step] of Object.entries(steps)) {
  const started = performance.now();
  try {
    await step();
    results.push({ tool: name, ok: true, ms: Math.round(performance.now() - started) });
    console.log(`✅ ${name}`);
  } catch (error) {
    results.push({ tool: name, ok: false, ms: Math.round(performance.now() - started), error: error.message });
    console.log(`❌ ${name}: ${error.message}`);
  }
}
await client.close();
await fixture.close();

const passed = results.filter((result) => result.ok).length;
const report = {
  date: new Date().toISOString().slice(0, 10),
  playwrightMcp: mcpVersion,
  pilot,
  passed,
  total: results.length,
  results,
};
const compat = resolve(here, '../../../compat');
await mkdir(compat, { recursive: true });
await writeFile(resolve(compat, 'mcp-tools.json'), `${JSON.stringify(report, null, 2)}\n`);
console.log(`🔍 ${passed}/${results.length} Playwright MCP ${mcpVersion} tool checks pass; wrote compat/mcp-tools.json`);
