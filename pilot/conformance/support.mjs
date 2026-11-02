// Shared setup for the conformance tests: which pilot binary to launch, a
// fixture HTTP server, a fresh profile directory per launch, and pilot
// started on a debugging port for MCP's --cdp-endpoint mode.
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { createServer as createNetServer } from 'node:net';
import { mkdtemp, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = fileURLToPath(new URL('.', import.meta.url));

/** The pilot executable under test: PILOT_BIN, else the debug build. */
export const pilotBin = process.env.PILOT_BIN ?? resolve(here, '../../target/debug/pilot');

export async function startFixtureServer() {
  const server = createServer(async (request, response) => {
    try {
      const path = new URL(request.url, 'http://fixture').pathname;
      const body = await readFile(join(here, 'fixtures', path));
      response.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
      response.end(body);
    } catch {
      response.writeHead(404);
      response.end();
    }
  });
  await new Promise((done) => server.listen(0, '127.0.0.1', done));
  const baseUrl = `http://127.0.0.1:${server.address().port}`;
  console.log(`🚀 fixture server at ${baseUrl}, pilot at ${pilotBin}`);
  return { baseUrl, close: () => new Promise((done) => server.close(done)) };
}

export function freshProfileDir(label) {
  return mkdtemp(join(tmpdir(), `pilot-conformance-${label}-`));
}

async function freePort() {
  const server = createNetServer();
  await new Promise((done) => server.listen(0, '127.0.0.1', done));
  const { port } = server.address();
  await new Promise((done) => server.close(done));
  return port;
}

/** Starts `pilot --remote-debugging-port=<free port>` and waits for /json/version/. */
export async function startPilotOnPort(extraArgs = []) {
  const port = await freePort();
  const child = spawn(pilotBin, [`--remote-debugging-port=${port}`, ...extraArgs], { stdio: 'ignore' });
  const endpoint = `http://127.0.0.1:${port}`;
  const exited = new Promise((done) => child.once('exit', done));
  const stop = async () => {
    if (child.exitCode === null) child.kill('SIGTERM');
    await exited;
  };
  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) {
    if (child.exitCode !== null) throw new Error(`❌ pilot exited early with code ${child.exitCode}`);
    try {
      if ((await fetch(`${endpoint}/json/version/`)).ok) {
        console.log(`✅ pilot serving CDP at ${endpoint}`);
        return { endpoint, stop };
      }
    } catch {
      // Not listening yet.
    }
    await new Promise((done) => setTimeout(done, 20));
  }
  await stop();
  throw new Error(`❌ pilot did not serve ${endpoint}/json/version/ within 15 s`);
}
