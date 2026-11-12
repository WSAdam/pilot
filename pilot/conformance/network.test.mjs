// pilot's Network events, as Playwright sees them.
import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { chromium } from 'playwright-core';

import { noWindowEnv, pilotBin } from './support.mjs';

const LAUNCH_TIMEOUT_MS = 30_000;

let browser;
let site;
let base;

before(async () => {
  site = createServer(async (request, response) => {
    for await (const _chunk of request) {
      // Drain the form body.
    }
    const path = new URL(request.url, 'http://site').pathname;
    if (path === '/form') {
      response.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
      return response.end('<form method="post" action="/orders"><input name="item" value="Blue kettle"><button>Order</button></form>');
    }
    if (path === '/orders' && request.method === 'POST') {
      response.writeHead(302, { location: '/orders/100' });
      return response.end();
    }
    if (path === '/orders/100') {
      response.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
      return response.end(`<h1>Order 100</h1><p id="method">${request.method}</p>`);
    }
    response.writeHead(404);
    response.end();
  });
  await new Promise((done) => site.listen(0, '127.0.0.1', done));
  base = `http://127.0.0.1:${site.address().port}`;
  browser = await chromium.launch({ executablePath: pilotBin, headless: true, env: noWindowEnv, timeout: LAUNCH_TIMEOUT_MS });
});

after(async () => {
  await browser?.close();
  await new Promise((done) => site?.close(done) ?? done());
});

// 2026-11-12, found by the record→replay spike: the wire request after a form
// POST's 302 is a GET, but pilot announced the hop as a POST still carrying
// the form body, so Playwright, HAR files and recorders saw "POST /orders/100".
test('the redirect after a form POST is reported as a GET without the form body', async () => {
  const page = await browser.newPage();
  try {
    const requests = [];
    page.on('request', (request) => requests.push(request));
    await page.goto(`${base}/form`);
    await Promise.all([page.waitForURL(`${base}/orders/100`), page.click('button')]);
    assert.equal(await page.textContent('#method'), 'GET', 'the server saw a GET');
    const hop = requests.find((request) => request.url() === `${base}/orders/100`);
    assert.ok(hop, 'Playwright saw the redirect hop');
    assert.equal(hop.redirectedFrom()?.method(), 'POST');
    assert.equal(hop.method(), 'GET');
    assert.equal(hop.postData(), null);
  } finally {
    await page.close();
  }
});
