// Playwright launches pilot by executable path, exactly as it launches Chromium.
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { after, before, test } from 'node:test';
import { chromium, devices } from 'playwright-core';
import { freshProfileDir, noWindowEnv, pilotBin, startFixtureServer } from './support.mjs';

const LAUNCH_TIMEOUT_MS = 30_000;
let fixture;

before(async () => {
  fixture = await startFixtureServer();
});
after(async () => {
  await fixture.close();
});

async function driveGreeting(page) {
  await page.goto(`${fixture.baseUrl}/greeting.html`);
  assert.equal(await page.evaluate(() => 1 + 1), 2);
  await page.getByRole('textbox', { name: 'Name' }).fill('pilot');
  await page.getByRole('button', { name: 'Greet' }).click();
  assert.equal(await page.title(), 'greeted');
  const snapshot = await page.ariaSnapshot({ mode: 'ai' });
  assert.match(snapshot, /paragraph \[ref=e\d+\]: Hello, pilot/);
}

// 2026-11-02: layout froze after the first geometry read, so a button added
// later had a 0x0 box and Playwright's click timed out (upstream #1080).
test('a button added after the page loaded can be clicked', { timeout: 60_000 }, async () => {
  const browser = await chromium.launch({ executablePath: pilotBin, headless: true, env: noWindowEnv, timeout: LAUNCH_TIMEOUT_MS });
  try {
    const page = await (await browser.newContext()).newPage();
    await page.goto(`${fixture.baseUrl}/late-button.html`);
    await page.getByRole('button', { name: 'Add late button' }).click();
    await page.getByRole('button', { name: 'Late button', exact: true }).click({ timeout: 5_000 });
    assert.equal(await page.locator('#out').textContent(), 'late clicked');
  } finally {
    await browser.close();
  }
});

// 2026-11-02: geometry kept the pre-scroll position after Playwright scrolled
// the button into view, so it stayed "outside of the viewport" and the click
// timed out.
test('a button below the fold is scrolled into view and clicked', { timeout: 60_000 }, async () => {
  const browser = await chromium.launch({ executablePath: pilotBin, headless: true, env: noWindowEnv, timeout: LAUNCH_TIMEOUT_MS });
  try {
    const page = await (await browser.newContext()).newPage();
    await page.goto(`${fixture.baseUrl}/below-fold.html`);
    await page.getByRole('button', { name: 'Below the fold' }).click({ timeout: 5_000 });
    assert.equal(await page.locator('#out').textContent(), 'clicked below');
  } finally {
    await browser.close();
  }
});

// 2026-11-02: hit testing put an inline <label> above the form control inside
// it, so Playwright's click, check and fill failed with "<label> intercepts
// pointer events" (upstream #385).
test('form controls inside a label can be checked, chosen and filled', { timeout: 60_000 }, async () => {
  const browser = await chromium.launch({ executablePath: pilotBin, headless: true, env: noWindowEnv, timeout: LAUNCH_TIMEOUT_MS });
  try {
    const page = await (await browser.newContext()).newPage();
    const out = page.locator('#out');
    await page.goto(`${fixture.baseUrl}/labelled-controls.html`);
    await page.getByRole('checkbox', { name: 'I accept the terms' }).check({ timeout: 5_000 });
    assert.equal(await out.textContent(), 'terms true');
    await page.getByRole('radio', { name: 'Pro plan' }).check({ timeout: 5_000 });
    assert.equal(await out.textContent(), 'plan pro');
    await page.getByRole('textbox', { name: 'Email' }).click({ timeout: 5_000 });
    await page.keyboard.type('a@b.co');
    assert.equal(await out.textContent(), 'email a@b.co');

    await page.goto(`${fixture.baseUrl}/late-controls.html`);
    await page.getByRole('button', { name: 'Add controls' }).click();
    await page.getByRole('checkbox', { name: 'Agree' }).check({ timeout: 5_000 });
    assert.equal(await out.textContent(), 'agree true');
  } finally {
    await browser.close();
  }
});

// 2026-11-03: a mouse press on non-focusable content focused the body and
// scrolled it into view, jumping a long page to the bottom; a raw mouse drag
// (MCP's browser_mouse_drag_xy) then missed its drop target.
test('a mouse press keeps the scroll position and a raw mouse drag drops on its target', { timeout: 60_000 }, async () => {
  const browser = await chromium.launch({ executablePath: pilotBin, headless: true, env: noWindowEnv, timeout: LAUNCH_TIMEOUT_MS });
  try {
    const page = await (await browser.newContext()).newPage();
    await page.goto(`${fixture.baseUrl}/sweep.html`);
    const heading = await page.getByRole('heading', { name: 'Sweep' }).boundingBox();
    await page.mouse.move(heading.x + 5, heading.y + 5);
    await page.mouse.down();
    await page.mouse.up();
    assert.equal(await page.evaluate(() => scrollY), 0, 'a press on text does not scroll');

    const source = await page.locator('#source').boundingBox();
    const target = await page.locator('#target').boundingBox();
    await page.mouse.move(source.x + source.width / 2, source.y + source.height / 2);
    await page.mouse.down();
    await page.mouse.move(target.x + target.width / 2, target.y + target.height / 2);
    await page.mouse.up();
    assert.equal(await page.locator('#out').textContent(), 'dropped dragged');
  } finally {
    await browser.close();
  }
});

// 2026-11-03: Emulation.setDeviceMetricsOverride rejected mobile=true, so a
// context with a phone's device descriptor (MCP's --device) could not open.
test('a phone device descriptor opens a context at the phone\'s size', { timeout: 60_000 }, async () => {
  const browser = await chromium.launch({ executablePath: pilotBin, headless: true, env: noWindowEnv, timeout: LAUNCH_TIMEOUT_MS });
  try {
    const context = await browser.newContext({ ...devices['iPhone 13'] });
    const page = await context.newPage();
    await driveGreeting(page);
    assert.deepEqual(await page.evaluate(() => [innerWidth, innerHeight, devicePixelRatio]), [390, 664, 3]);
  } finally {
    await browser.close();
  }
});

// 2026-11-03: pilot fetched no images, so an image without dimensions laid
// out at 0x0 and a link floated over it (weather.gov's header) landed
// off-screen, where Playwright could not click it.
test('images load, take their size, and links placed over them can be clicked', { timeout: 60_000 }, async () => {
  const browser = await chromium.launch({ executablePath: pilotBin, headless: true, env: noWindowEnv, timeout: LAUNCH_TIMEOUT_MS });
  try {
    const page = await (await browser.newContext()).newPage();
    await page.goto(`${fixture.baseUrl}/image-layout.html`);
    assert.deepEqual(
      await page.locator('#logo').evaluate((img) => [img.naturalWidth, img.getBoundingClientRect().width]),
      [200, 200],
    );
    await page.locator('#overlay').click({ timeout: 5_000 });
    assert.equal(await page.locator('#out').textContent(), 'overlay clicked');
  } finally {
    await browser.close();
  }
});

// 2026-11-03: Enter in a text input fired no change event, so TodoMVC's
// vanilla app (which adds on change) never added a todo.
test('Enter in a text input commits it with a change event', { timeout: 60_000 }, async () => {
  const browser = await chromium.launch({ executablePath: pilotBin, headless: true, env: noWindowEnv, timeout: LAUNCH_TIMEOUT_MS });
  try {
    const page = await (await browser.newContext()).newPage();
    await page.goto(`${fixture.baseUrl}/enter-change.html`);
    const input = page.getByPlaceholder('What needs to be done?');
    for (const text of ['Buy milk', 'Walk the dog']) {
      await input.fill(text);
      await input.press('Enter');
    }
    assert.deepEqual(await page.locator('#list li').allTextContents(), ['Buy milk', 'Walk the dog']);
  } finally {
    await browser.close();
  }
});

// 2026-11-02: the same frozen layout made selectOption (#1289) and hover
// (#2069) time out on controls inserted after the first layout.
test('controls added after the page loaded can be selected and hovered', { timeout: 60_000 }, async () => {
  const browser = await chromium.launch({ executablePath: pilotBin, headless: true, env: noWindowEnv, timeout: LAUNCH_TIMEOUT_MS });
  try {
    const page = await (await browser.newContext()).newPage();
    await page.goto(`${fixture.baseUrl}/late-controls.html`);
    await page.getByRole('button', { name: 'Add controls' }).click();
    const out = page.locator('#out');
    await page.getByRole('combobox', { name: 'Size' }).selectOption('Large', { timeout: 5_000 });
    assert.equal(await out.textContent(), 'size Large');
    await page.getByText('Hover me').hover({ timeout: 5_000 });
    assert.equal(await out.textContent(), 'hovered');
  } finally {
    await browser.close();
  }
});

for (const headless of [true, false]) {
  test(`chromium.launch drives a page and closes (headless=${headless})`, { timeout: 60_000 }, async () => {
    const browser = await chromium.launch({ executablePath: pilotBin, headless, env: noWindowEnv, timeout: LAUNCH_TIMEOUT_MS });
    try {
      const context = await browser.newContext();
      await driveGreeting(await context.newPage());
    } finally {
      await browser.close();
    }
    assert.equal(browser.isConnected(), false);
  });

  // 2026-11-02: MCP's default mode. It hung because Page.setLifecycleEventsEnabled
  // replayed `load` before the earlier Page.getFrameTree reply on the start page.
  test(`launchPersistentContext starts on about:blank and drives it (headless=${headless})`, { timeout: 60_000 }, async () => {
    const profile = await freshProfileDir('persistent');
    const context = await chromium.launchPersistentContext(profile, {
      executablePath: pilotBin,
      headless,
      env: noWindowEnv,
      timeout: LAUNCH_TIMEOUT_MS,
    });
    try {
      const [startPage] = context.pages();
      assert.ok(startPage, 'the persistent context opens with its start page');
      assert.equal(startPage.url(), 'about:blank');
      await driveGreeting(startPage);
    } finally {
      await context.close();
    }
  });
}

// pilot's window is a second CDP client watching the same page (screencast,
// tab tracking); Playwright must drive the page exactly as without it. This
// opens a real window on a desktop.
test('chromium.launch drives a page with pilot\'s window open (headless=false)', { timeout: 60_000 }, async () => {
  const browser = await chromium.launch({ executablePath: pilotBin, headless: false, timeout: LAUNCH_TIMEOUT_MS });
  try {
    const page = await (await browser.newContext()).newPage();
    await driveGreeting(page);
    await page.screenshot();
  } finally {
    await browser.close();
  }
  assert.equal(browser.isConnected(), false);
});

// What each operating system's Chrome says about itself.
const HOST_CHROME = {
  darwin: { system: 'Macintosh; Intel Mac OS X 10_15_7', navigatorPlatform: 'MacIntel', platform: 'macOS' },
  linux: { system: process.arch === 'arm64' ? 'X11; Linux aarch64' : 'X11; Linux x86_64', navigatorPlatform: process.arch === 'arm64' ? 'Linux aarch64' : 'Linux x86_64', platform: 'Linux' },
  win32: { system: 'Windows NT 10.0; Win64; x64', navigatorPlatform: 'Win32', platform: 'Windows' },
}[process.platform];

// 2026-11-04: pilot claimed Chrome on Windows everywhere; on a Mac the TCP/IP
// fingerprint says macOS, a mismatch bot defenses check (compat/bot-walls.md).
test('pilot is Chrome on the host OS in its requests and its navigator', { timeout: 60_000 }, async () => {
  const requests = [];
  const server = createServer((request, response) => {
    requests.push(request.headers);
    response.writeHead(200, { 'content-type': 'text/html' });
    response.end('<title>identity</title>');
  });
  await new Promise((done) => server.listen(0, '127.0.0.1', done));
  const browser = await chromium.launch({ executablePath: pilotBin, headless: true, env: noWindowEnv, timeout: LAUNCH_TIMEOUT_MS });
  try {
    const page = await (await browser.newContext()).newPage();
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    const seen = await page.evaluate(async () => ({
      userAgent: navigator.userAgent,
      platform: navigator.platform,
      uaPlatform: navigator.userAgentData?.platform,
      high: await navigator.userAgentData?.getHighEntropyValues(['platformVersion', 'architecture', 'fullVersionList']),
    }));
    const headers = requests[0];
    assert.ok(headers, 'the server saw the navigation');
    assert.equal(headers['user-agent'], seen.userAgent, 'the page and the request name the same browser');
    assert.match(seen.userAgent, new RegExp(`^Mozilla/5\\.0 \\(${HOST_CHROME.system.replace(/[.()]/g, '\\$&')}\\) AppleWebKit/537\\.36 \\(KHTML, like Gecko\\) Chrome/\\d+\\.0\\.0\\.0 Safari/537\\.36$`));
    assert.equal(seen.platform, HOST_CHROME.navigatorPlatform);
    assert.equal(seen.uaPlatform, HOST_CHROME.platform);
    assert.equal(headers['sec-ch-ua-platform'], `"${HOST_CHROME.platform}"`);
    assert.match(seen.high.platformVersion, /^\d+\.\d+\.\d+$/);
    const chrome = seen.high.fullVersionList.find(({ brand }) => brand === 'Google Chrome');
    assert.match(chrome?.version ?? '', /^\d+\.0\.\d+\.\d+$/, 'the full version is in the client hints');
    assert.notEqual(chrome.version.split('.').slice(2).join('.'), '0.0', 'and it is a real build, not the reduced one');
  } finally {
    await browser.close();
    await new Promise((done) => server.close(done));
  }
});

// 2026-11-04: uncaught errors from inline scripts and event handlers never
// reached Playwright, and timer errors arrived wrapped in Moli's dispatch text
// (Playwright MCP console.spec "page error"): Moli dispatched the window
// error event but never reported the exception to the developer console.
test('uncaught errors reach Playwright as page errors with their own message', { timeout: 60_000 }, async () => {
  const server = createServer((request, response) => {
    response.writeHead(200, { 'content-type': 'text/html' });
    response.end('<!DOCTYPE html><script>throw new Error("inline at load");</script><button onclick="throw new Error(\'click handler\')">b</button>');
  });
  await new Promise((done) => server.listen(0, '127.0.0.1', done));
  const browser = await chromium.launch({ executablePath: pilotBin, headless: true, env: noWindowEnv, timeout: LAUNCH_TIMEOUT_MS });
  try {
    const page = await (await browser.newContext()).newPage();
    const errors = [];
    page.on('pageerror', (error) => errors.push(error));
    // The raw CDP events behind those page errors, printed only on failure
    // (gate run 3: inline and click errors arrived without the error object).
    const raw = [];
    const cdp = await page.context().newCDPSession(page);
    cdp.on('Runtime.executionContextCreated', ({ context }) => raw.push({ contextCreated: { id: context.id, name: context.name, origin: context.origin, auxData: context.auxData } }));
    cdp.on('Runtime.executionContextDestroyed', ({ executionContextId }) => raw.push({ contextDestroyed: executionContextId }));
    cdp.on('Runtime.exceptionThrown', ({ exceptionDetails }) => raw.push({ exceptionThrown: { text: exceptionDetails.text, executionContextId: exceptionDetails.executionContextId, url: exceptionDetails.url, hasException: Boolean(exceptionDetails.exception), description: exceptionDetails.exception?.description?.split('\n')[0] } }));
    await cdp.send('Runtime.enable');
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    await page.click('button');
    await page.evaluate(() => setTimeout(() => { throw new TypeError('from a timer'); }, 0));
    await page.waitForTimeout(500);
    const pageErrors = errors.map((error) => `${error.name}: ${error.message}`);
    assert.deepEqual(pageErrors, [
      'Error: inline at load',
      'Error: click handler',
      'TypeError: from a timer',
    ], `page errors ${JSON.stringify(pageErrors)}\nraw CDP ${JSON.stringify(raw, null, 1)}`);
    assert.match(errors[0].stack, /inline at load\n\s+at /, 'the stack is the error\'s own');
  } finally {
    await browser.close();
    await new Promise((done) => server.close(done));
  }
});

// 2026-11-04: console messages carried timestamp 0, so Playwright MCP's
// console log read [-1793731413422ms] and dropped lines.
test('console messages carry the time they were logged', { timeout: 60_000 }, async () => {
  const browser = await chromium.launch({ executablePath: pilotBin, headless: true, env: noWindowEnv, timeout: LAUNCH_TIMEOUT_MS });
  try {
    const page = await (await browser.newContext()).newPage();
    const messages = [];
    page.on('console', (message) => messages.push(message));
    const before = Date.now();
    await page.evaluate(() => { console.warn('warned'); console.error('failed'); });
    await page.waitForTimeout(200);
    const after = Date.now();
    assert.deepEqual(messages.map((message) => `${message.type()} ${message.text()}`), ['warning warned', 'error failed']);
    for (const message of messages) {
      const timestamp = message.timestamp();
      assert.ok(timestamp >= before - 1_000 && timestamp <= after + 1_000, `${message.text()} logged at ${timestamp}, between ${before} and ${after}`);
    }
  } finally {
    await browser.close();
  }
});
