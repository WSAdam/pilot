// Gaps found by running Playwright's own MCP suite against pilot (triage of
// 2026-11-10, compat/playwright-mcp-suite.json). Each test pins the Chrome
// behaviour a suite test relies on, and was proven red against the release
// binary of that date. Each is a `todo` until the build named in it lands:
// the test runs and reports, but does not fail the gate. The build that fixes
// a gap removes its `todo`.
import assert from 'node:assert/strict';
import { mkdtemp, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, before, test } from 'node:test';
import { chromium } from 'playwright-core';
import { noWindowEnv, pilotBin, startFixtureServer } from './support.mjs';

const LAUNCH_TIMEOUT_MS = 30_000;
let fixture;
let browser;

before(async () => {
  fixture = await startFixtureServer();
  browser = await chromium.launch({ executablePath: pilotBin, headless: true, env: noWindowEnv, timeout: LAUNCH_TIMEOUT_MS });
});
after(async () => {
  await browser?.close();
  await fixture?.close();
});

async function withPage(run) {
  const context = await browser.newContext();
  try {
    return await run(await context.newPage(), context);
  } finally {
    await context.close();
  }
}

// 2026-11-10: Storage.setCookies used the page's URL when a cookie named only a
// domain, so a cookie for example.com set while on 127.0.0.1 failed the domain
// match and was dropped (MCP cookies.spec: cookie_list by domain, cookie_delete).
test('a cookie set by domain alone is kept for a site the page never visited', { timeout: 60_000 }, () =>
  withPage(async (page, context) => {
    await page.goto(`${fixture.baseUrl}/greeting.html`);
    await context.addCookies([
      { name: 'apex', value: 'v', domain: 'example.com', path: '/' },
      { name: 'sub', value: 'v', domain: 'sub.example.com', path: '/' },
    ]);
    const cookies = (await context.cookies()).map((cookie) => `${cookie.name}@${cookie.domain.replace(/^\./, '')}`);
    assert.deepEqual(cookies.sort(), ['apex@example.com', 'sub@sub.example.com']);
  }));

// 2026-11-10: pilot's Network.requestWillBeSent carried the body as postData;
// current Playwright reads only postDataEntries, so request.postData() was null
// (MCP network.spec: browser_network_request details and parts).
test('a request body reaches Playwright as request.postData()', { timeout: 60_000 }, () =>
  withPage(async (page) => {
    await page.goto(`${fixture.baseUrl}/greeting.html`);
    const requestSeen = page.waitForRequest('**/api');
    await page.evaluate(() => { fetch('/api', { method: 'POST', body: '{"key":"value"}' }).catch(() => {}); });
    const request = await requestSeen;
    assert.equal(request.postData(), '{"key":"value"}');
  }));

// 2026-11-10: Page.captureScreenshot refused format "webp" (MCP
// browser_take_screenshot type webp, CLI screenshot --filename x.webp).
test('Page.captureScreenshot returns a WebP image for format "webp"', { timeout: 60_000 }, () =>
  withPage(async (page, context) => {
    await page.goto(`${fixture.baseUrl}/greeting.html`);
    const cdp = await context.newCDPSession(page);
    const { data } = await cdp.send('Page.captureScreenshot', { format: 'webp' });
    const bytes = Buffer.from(data, 'base64');
    assert.equal(bytes.subarray(0, 4).toString('latin1'), 'RIFF');
    assert.equal(bytes.subarray(8, 12).toString('latin1'), 'WEBP');
  }));

// 2026-11-10: pilot dropped --proxy-server as an unknown switch, so a launch
// with a proxy went direct and hosts only the proxy can reach failed with
// net::ERR_FAILED (MCP launch.spec --proxy-server).
test('a browser launched with a proxy sends its requests through it', { timeout: 60_000 }, async () => {
  const proxy = createServer((request, response) => {
    response.writeHead(200, { 'content-type': 'text/html' });
    response.end(`<title>proxied</title><p>Served by the proxy for ${request.url}</p>`);
  });
  await new Promise((done) => proxy.listen(0, '127.0.0.1', done));
  const proxied = await chromium.launch({
    executablePath: pilotBin, headless: true, env: noWindowEnv, timeout: LAUNCH_TIMEOUT_MS,
    proxy: { server: `http://127.0.0.1:${proxy.address().port}` },
  });
  try {
    const page = await (await proxied.newContext()).newPage();
    await page.goto('http://non-existent.test/target.html', { timeout: 10_000 });
    assert.match(await page.textContent('p'), /Served by the proxy for http:\/\/non-existent\.test\/target\.html/);
  } finally {
    await proxied.close();
    await new Promise((done) => proxy.close(done));
  }
});

// 2026-11-10: given --remote-debugging-pipe and --remote-debugging-port, pilot
// served only the pipe and never printed "DevTools listening on ws://…", which
// Playwright waits for on stderr when the launch names a port, so the launch
// hung. Chrome serves both, and MCP's crashed-browser test connects over the
// port to close the browser (MCP http.spec relaunches a crashed shared browser).
test('a pipe launch with --remote-debugging-port also serves CDP on that port', { timeout: 60_000 }, async () => {
  const probe = createServer();
  await new Promise((done) => probe.listen(0, '127.0.0.1', done));
  const { port } = probe.address();
  await new Promise((done) => probe.close(done));
  const both = await chromium.launch({ executablePath: pilotBin, headless: true, env: noWindowEnv, timeout: 10_000, args: [`--remote-debugging-port=${port}`] });
  try {
    const version = await fetch(`http://127.0.0.1:${port}/json/version`).then((response) => response.json());
    assert.match(version.webSocketDebuggerUrl, /^ws:\/\/127\.0\.0\.1:\d+\/devtools\/browser\//);
  } finally {
    await both.close();
  }
});

// 2026-11-10: pilot threw InvalidStateError for a redundant popover state change
// (the old spec). Chrome returns quietly, and Playwright's highlight overlay calls
// hidePopover() on a fresh element, so recorder setup died on its first line.
test('hidePopover on a closed popover and showPopover on an open one return quietly', { timeout: 60_000 }, () =>
  withPage(async (page) => {
    await page.goto(`${fixture.baseUrl}/greeting.html`);
    const outcome = await page.evaluate(() => {
      const popover = document.body.appendChild(document.createElement('div'));
      popover.popover = 'manual';
      const probe = (call) => { try { call(); return 'ok'; } catch (error) { return error.name; } };
      const hiddenHide = probe(() => popover.hidePopover());
      popover.showPopover();
      const shownShow = probe(() => popover.showPopover());
      return [hiddenHide, shownShow, popover.matches(':popover-open')].join('|');
    });
    assert.equal(outcome, 'ok|ok|true');
  }));

// 2026-11-10: Playwright's recorder (MCP browser_start_recording) recorded
// nothing: its overlay install threw on hidePopover(), so no listener was set up
// (MCP recorder.spec, CLI recording-start-stop).
test("Playwright's recorder records a click and a fill as code", { timeout: 60_000 }, () =>
  withPage(async (page, context) => {
    await page.setContent('<title>Title</title><button>Submit</button><input type="text">');
    const actions = [];
    await context._startRecording({ language: 'javascript' }, {
      actionAdded: (_page, _action, code) => actions.push(code),
      actionUpdated: (_page, _action, code) => { actions[Math.max(actions.length - 1, 0)] = code; },
      signalAdded: () => {},
    });
    await page.getByRole('button', { name: 'Submit' }).click();
    await page.getByRole('textbox').fill('Hello world');
    await new Promise((done) => setTimeout(done, 500));
    const recorded = actions.join('\n');
    assert.match(recorded, /getByRole\('button', \{ name: 'Submit' \}\)\.click\(\)/);
    assert.match(recorded, /getByRole\('textbox'\)\.fill\('Hello world'\)/);
  }));

// 2026-11-10: an iframe nested inside a data: URL iframe never became a frame,
// and page.ariaSnapshot failed with "Maximum call stack size exceeded" (MCP
// iframes.spec stitched aria frames, CLI click in iframe).
test('a nested iframe loads as its own frame and the snapshot stitches it in', { todo: 'Build 3', timeout: 60_000 }, () =>
  withPage(async (page) => {
    await page.goto(`data:text/html,<h1>Hello</h1><iframe src="data:text/html,<button>World</button><main><iframe src='data:text/html,<p>Nested</p>'></iframe></main>"></iframe>`);
    await page.waitForFunction(() => true);
    assert.equal(page.frames().length, 3);
    const snapshot = await page.ariaSnapshot({ mode: 'ai', timeout: 10_000 });
    assert.match(snapshot, /button "World" \[ref=f1e\d+\]/);
    assert.match(snapshot, /paragraph \[ref=f\d+e\d+\]: Nested/);
  }));

// 2026-11-10: the options of a <select multiple> listbox had 0x0 boxes, so the
// snapshot gave them no refs (MCP core.spec browser_select_option (multiple)).
test('each option of a select multiple listbox gets a box and a snapshot ref', { timeout: 60_000 }, () =>
  withPage(async (page) => {
    await page.setContent('<select multiple><option value="foo">Foo</option><option value="bar">Bar</option><option value="baz">Baz</option></select>');
    const boxes = await page.evaluate(() => [...document.querySelectorAll('option')].map((option) => option.getBoundingClientRect().height > 0));
    assert.deepEqual(boxes, [true, true, true]);
    const snapshot = await page.ariaSnapshot({ mode: 'ai' });
    assert.match(snapshot, /option "Foo" \[ref=e\d+\]/);
    assert.match(snapshot, /option "Baz" \[ref=e\d+\]/);
  }));

// 2026-11-10: a ResizeObserver on document.body did not fire when Playwright
// changed the viewport, though innerWidth changed (MCP core.spec browser_resize).
test('a ResizeObserver on the body fires when the viewport is resized', { timeout: 60_000 }, () =>
  withPage(async (page) => {
    await page.setContent(`<div id="size">Waiting for resize...</div><script>
      new ResizeObserver(() => { document.getElementById('size').textContent = 'Window size: ' + innerWidth + 'x' + innerHeight; }).observe(document.body);
    </script>`);
    await page.setViewportSize({ width: 390, height: 780 });
    await page.waitForFunction(() => document.getElementById('size').textContent === 'Window size: 390x780', null, { timeout: 5_000 });
  }));

// 2026-11-10: beforeunload never ran for navigations Playwright starts (goto,
// goBack, goForward), so a page that cancels it opened no dialog (MCP
// dialogs.spec dialog during navigate back, CLI go-back/go-forward with dialog).
test('going back from a page that cancels beforeunload opens a beforeunload dialog', { todo: 'Build 3', timeout: 60_000 }, () =>
  withPage(async (page) => {
    await page.goto(`${fixture.baseUrl}/greeting.html`);
    await page.goto(`${fixture.baseUrl}/leave-guard.html`);
    await page.click('#touch');
    const dialog = page.waitForEvent('dialog', { timeout: 5_000 });
    const back = page.goBack().catch(() => {});
    const opened = await dialog;
    assert.equal(opened.type(), 'beforeunload');
    await opened.accept();
    await back;
    assert.equal(await page.title(), 'pilot conformance');
  }));

// 2026-11-10: pilot refused every file:// navigation ("requires an explicitly
// granted browser capability"); Chrome loads local files, and Playwright MCP
// does its own gating (MCP core.spec allowUnrestrictedFileAccess).
test('a file:// URL loads the local file', { todo: 'Build 3', timeout: 60_000 }, () =>
  withPage(async (page) => {
    const dir = await mkdtemp(join(tmpdir(), 'pilot-file-url-'));
    const file = join(dir, 'test.txt');
    await writeFile(file, 'Test file content');
    await page.goto(`file://${file}`);
    assert.equal((await page.textContent('body')).trim(), 'Test file content');
  }));

// 2026-11-10: a tool the page registered through document.modelContext was not
// visible from Playwright's utility world, where page.webmcp (and so MCP) calls
// getTools(); pilot keeps one tool list per JS world, Chrome one per document
// (MCP webmcp.spec and webmcp-dynamic.spec, CLI webmcp-list/webmcp-call).
test('page.webmcp lists and calls a tool the page registered', { todo: 'Build 4', timeout: 60_000 }, async () => {
  const webmcpBrowser = await chromium.launch({ executablePath: pilotBin, headless: true, args: ['--enable-features=WebMCP'], env: noWindowEnv, timeout: LAUNCH_TIMEOUT_MS });
  try {
    const page = await (await webmcpBrowser.newContext()).newPage();
    await page.goto(`${fixture.baseUrl}/greeting.html`);
    await page.evaluate(() => {
      const modelContext = document.modelContext || navigator.modelContext;
      modelContext.registerTool({
        name: 'add',
        description: 'Adds two numbers',
        inputSchema: { type: 'object', properties: { a: { type: 'number' }, b: { type: 'number' } }, required: ['a', 'b'] },
        async execute(input) { return { content: [{ type: 'text', text: String(input.a + input.b) }] }; },
      });
    });
    const tools = await page.webmcp.tools();
    assert.deepEqual(tools.map((tool) => tool.name), ['add']);
    const result = await page.webmcp.callTool('add', { a: 2, b: 40 });
    assert.match(JSON.stringify(result), /"42"/);
  } finally {
    await webmcpBrowser.close();
  }
});
