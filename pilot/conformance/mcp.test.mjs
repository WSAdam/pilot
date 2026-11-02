// Playwright MCP launches pilot with --executable-path and an agent drives it
// through MCP tools, in each of MCP's launch modes.
import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';
import { freshProfileDir, pilotBin, startFixtureServer, startPilotOnPort } from './support.mjs';

// The package does not export its CLI entry point, so run it by path.
const mcpCli = fileURLToPath(new URL('./node_modules/@playwright/mcp/cli.js', import.meta.url));
let fixture;

before(async () => {
  fixture = await startFixtureServer();
});
after(async () => {
  await fixture.close();
});

const toolText = (result) => (result.content ?? []).map((part) => part.text ?? '').join('\n');
const refFor = (snapshot, label) => snapshot.match(new RegExp(`${label}[^\\n]*\\[ref=((?:f\\d+)?e\\d+)\\]`))?.[1];

async function callTool(client, name, args) {
  const result = await client.callTool({ name, arguments: args });
  assert.ok(!result.isError, `${name} failed: ${toolText(result).slice(0, 800)}`);
  return toolText(result);
}

// Each mode yields MCP's arguments and, where pilot runs outside MCP, a stop.
const modes = {
  // MCP's default: a persistent profile, headed.
  default: async () => ({
    args: ['--executable-path', pilotBin, '--user-data-dir', await freshProfileDir('mcp-default')],
  }),
  headless: async () => ({
    args: ['--executable-path', pilotBin, '--headless', '--user-data-dir', await freshProfileDir('mcp-headless')],
  }),
  isolated: async () => ({ args: ['--executable-path', pilotBin, '--isolated'] }),
  // MCP connects to an already-running pilot instead of launching it.
  'cdp-endpoint': async () => {
    const pilot = await startPilotOnPort(['--headless']);
    return { args: ['--cdp-endpoint', pilot.endpoint], stop: pilot.stop };
  },
};

async function connectMcp(args) {
  const outputDir = await freshProfileDir('mcp-output');
  const client = new Client({ name: 'pilot-conformance', version: '0.1.0' });
  await client.connect(new StdioClientTransport({
    command: process.execPath,
    args: [mcpCli, '--output-dir', outputDir, ...args],
    stderr: 'inherit',
  }));
  return client;
}

// 2026-11-02: the walking skeleton's bug. Layout froze after the first
// geometry read, so a button added later had no snapshot ref (Playwright saw
// a 0x0, invisible box) and the agent could not click it.
test('Playwright MCP gives a button added after load a ref and clicks it', { timeout: 90_000 }, async () => {
  const client = await connectMcp(['--executable-path', pilotBin, '--isolated', '--headless']);
  try {
    await callTool(client, 'browser_navigate', { url: `${fixture.baseUrl}/late-button.html` });
    let snapshot = await callTool(client, 'browser_snapshot', {});
    await callTool(client, 'browser_click', { element: 'Add late button', target: refFor(snapshot, 'button "Add late button"') });
    snapshot = await callTool(client, 'browser_snapshot', {});
    const lateRef = refFor(snapshot, 'button "Late button"');
    assert.ok(lateRef, `the appended button has a ref:\n${snapshot}`);
    await callTool(client, 'browser_click', { element: 'Late button', target: lateRef });
    assert.match(await callTool(client, 'browser_snapshot', {}), /late clicked/);
  } finally {
    await client.close();
  }
});

// pilot holds the profile's SingletonLock like Chromium, so MCP refuses a
// second browser on it instead of starting one that fights over the profile.
test('a second Playwright MCP on the same profile reports it in use', { timeout: 90_000 }, async () => {
  const profile = await freshProfileDir('mcp-shared');
  const first = await connectMcp(['--executable-path', pilotBin, '--headless', '--user-data-dir', profile]);
  const second = await connectMcp(['--executable-path', pilotBin, '--headless', '--user-data-dir', profile]);
  try {
    await callTool(first, 'browser_navigate', { url: `${fixture.baseUrl}/greeting.html` });
    const refused = await second.callTool({
      name: 'browser_navigate',
      arguments: { url: `${fixture.baseUrl}/greeting.html` },
    });
    assert.ok(refused.isError, 'the second browser must not start on a profile in use');
    assert.match(toolText(refused), /already in use/);
  } finally {
    await callTool(first, 'browser_close', {}).catch(() => {});
    await first.close();
    await second.close();
  }
});

for (const [mode, setUp] of Object.entries(modes)) {
  test(`Playwright MCP drives pilot in ${mode} mode`, { timeout: 90_000 }, async () => {
    const { args, stop } = await setUp();
    // MCP writes snapshots and screenshots to files; keep them out of the repo.
    const outputDir = await freshProfileDir(`mcp-output-${mode}`);
    const client = new Client({ name: 'pilot-conformance', version: '0.1.0' });
    try {
      await client.connect(new StdioClientTransport({
        command: process.execPath,
        args: [mcpCli, '--output-dir', outputDir, ...args],
        stderr: 'inherit',
      }));
      await callTool(client, 'browser_navigate', { url: `${fixture.baseUrl}/greeting.html` });
      let snapshot = await callTool(client, 'browser_snapshot', {});
      const nameRef = refFor(snapshot, 'textbox "Name"');
      const greetRef = refFor(snapshot, 'button "Greet"');
      assert.ok(nameRef && greetRef, `snapshot lists the form with refs:\n${snapshot}`);
      await callTool(client, 'browser_type', { element: 'Name textbox', target: nameRef, text: 'pilot' });
      await callTool(client, 'browser_click', { element: 'Greet button', target: greetRef });
      snapshot = await callTool(client, 'browser_snapshot', {});
      assert.match(snapshot, /Hello, pilot/);
      await callTool(client, 'browser_take_screenshot', {});
      await callTool(client, 'browser_close', {});
    } finally {
      await client.close();
      await stop?.();
    }
  });
}
