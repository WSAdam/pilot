// Playwright launches pilot by executable path, exactly as it launches Chromium.
import assert from 'node:assert/strict';
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
