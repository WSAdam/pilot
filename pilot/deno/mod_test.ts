// launchPilot (mod.ts) starts pilot on a free port and connects Playwright to
// it in one call. PILOT_BIN selects the binary (default: the release build).
import {
  assert,
  assertEquals,
  assertMatch,
  assertNotEquals,
  assertRejects,
} from "@std/assert";
import { launchPilot } from "./mod.ts";

const pilotBin = Deno.env.get("PILOT_BIN") ??
  new URL("../../target/release/pilot", import.meta.url).pathname;

const PAGE = `<!doctype html><title>deno helper</title>
<label>Name <input id=name></label>
<button onclick="document.getElementById('out').textContent = 'Hello, ' + document.getElementById('name').value">Greet</button>
<p id=out></p>`;

/** True while something accepts connections on the port. */
async function listening(port: number): Promise<boolean> {
  try {
    const connection = await Deno.connect({ hostname: "127.0.0.1", port });
    connection.close();
    return true;
  } catch {
    return false;
  }
}

/** True while the process exists (kill -0). */
async function alive(pid: number): Promise<boolean> {
  const { success } = await new Deno.Command("kill", {
    args: ["-0", String(pid)],
    stderr: "null",
  }).output();
  return success;
}

/** Waits up to two seconds for the process to be gone (and reaped). */
async function gone(pid: number): Promise<boolean> {
  for (let attempt = 0; attempt < 20; attempt++) {
    if (!(await alive(pid))) return true;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  return false;
}

// 2026-11-18: TODO line 92; a Deno project had to spawn pilot, find its port
// and call connectOverCDP itself.
Deno.test("launchPilot starts pilot on a free port and returns a connected browser that drives a page", async () => {
  const server = Deno.serve(
    { port: 0, hostname: "127.0.0.1", onListen() {} },
    () => new Response(PAGE, { headers: { "content-type": "text/html" } }),
  );
  const logLines: string[] = [];
  const pilot = await launchPilot({
    executablePath: pilotBin,
    env: { PILOT_NO_WINDOW: "1" },
    onLog: (line) => logLines.push(line),
  });
  const port = Number(new URL(pilot.wsEndpoint).port);
  try {
    assertMatch(
      pilot.wsEndpoint,
      /^ws:\/\/127\.0\.0\.1:\d+\/devtools\/browser\/\S+$/,
    );
    assertNotEquals(port, 0);
    assertNotEquals(port, 9222, "pilot's fixed default port, not a free one");
    assert(pilot.browser.isConnected());
    assert(
      logLines.some((line) => line.startsWith("DevTools listening on ws://")),
      "onLog sees pilot's stderr",
    );

    const page = await pilot.browser.newPage();
    await page.goto(`http://127.0.0.1:${server.addr.port}/`);
    await page.getByRole("textbox", { name: "Name" }).fill("Deno");
    await page.getByRole("button", { name: "Greet" }).click();
    assertEquals(await page.locator("#out").textContent(), "Hello, Deno");
  } finally {
    await pilot.close();
    await server.shutdown();
  }
  assert(!pilot.browser.isConnected());
  assert(!(await alive(pilot.pid)), "pilot exited after close()");
  assert(!(await listening(port)), "pilot's port is closed after close()");
  await pilot.close(); // a second close is a no-op
});

Deno.test("launchPilot works with await using and the browser's default context", async () => {
  let pid = 0;
  {
    await using pilot = await launchPilot({
      executablePath: pilotBin,
      env: { PILOT_NO_WINDOW: "1" },
    });
    pid = pilot.pid;
    const [context] = pilot.browser.contexts();
    const page = context.pages()[0] ?? await context.newPage();
    await page.setContent("<p id=x>default context</p>");
    assertEquals(await page.locator("#x").textContent(), "default context");
  }
  assert(!(await alive(pid)), "pilot exited when the block ended");
});

Deno.test("launchPilot names the binary when it cannot be started", async () => {
  const missing = "/nonexistent/pilot-for-launchPilot-test";
  await assertRejects(
    () => launchPilot({ executablePath: missing, timeout: 5_000 }),
    Error,
    `could not start pilot at ${missing}`,
  );
});

Deno.test("launchPilot reports a binary that exits before announcing its endpoint, with its stderr", async () => {
  const dir = await Deno.makeTempDir({ prefix: "launch-pilot-test-" });
  try {
    const script = `${dir}/exits`;
    await Deno.writeTextFile(
      script,
      "#!/bin/sh\necho 'cannot open profile' >&2\nexit 7\n",
    );
    await Deno.chmod(script, 0o755);
    const error = await assertRejects(
      () => launchPilot({ executablePath: script, timeout: 5_000 }),
      Error,
      "pilot exited (code 7) before announcing its DevTools endpoint",
    );
    assertMatch(error.message, /cannot open profile/);
  } finally {
    await Deno.remove(dir, { recursive: true });
  }
});

Deno.test("launchPilot kills a binary that never announces its endpoint once the timeout passes", async () => {
  const dir = await Deno.makeTempDir({ prefix: "launch-pilot-test-" });
  try {
    const script = `${dir}/silent`;
    const pidFile = `${dir}/pid`;
    await Deno.writeTextFile(
      script,
      `#!/bin/sh\necho $$ > '${pidFile}'\nexec sleep 30\n`,
    );
    await Deno.chmod(script, 0o755);
    const started = performance.now();
    await assertRejects(
      () => launchPilot({ executablePath: script, timeout: 500 }),
      Error,
      "pilot did not announce its DevTools endpoint within 500 ms",
    );
    assert(performance.now() - started < 5_000, "the timeout ends the launch");
    const pid = Number((await Deno.readTextFile(pidFile)).trim());
    assert(!(await alive(pid)), "the silent process was killed");
  } finally {
    await Deno.remove(dir, { recursive: true });
  }
});

Deno.test("a Deno script that exits without close() does not leave pilot running", async () => {
  const dir = await Deno.makeTempDir({ prefix: "launch-pilot-test-" });
  try {
    const script = `${dir}/forgets-close.ts`;
    await Deno.writeTextFile(
      script,
      `import { launchPilot } from ${
        JSON.stringify(new URL("./mod.ts", import.meta.url).href)
      };
const pilot = await launchPilot({ executablePath: ${
        JSON.stringify(pilotBin)
      }, env: { PILOT_NO_WINDOW: "1" } });
console.log("pid=" + pilot.pid);
Deno.exit(0);
`,
    );
    const run = await new Deno.Command(Deno.execPath(), {
      args: ["run", "-A", "--no-config", script],
      stdout: "piped",
      stderr: "piped",
    }).output();
    const stderr = new TextDecoder().decode(run.stderr);
    assert(run.success, `the script ran: ${stderr}`);
    const stdout = new TextDecoder().decode(run.stdout);
    const pid = Number(stdout.match(/^pid=(\d+)$/m)?.[1]);
    assert(pid > 0, `the script printed pilot's pid: ${stderr}`);
    assert(await gone(pid), `pilot ${pid} was killed when the script exited`);
  } finally {
    await Deno.remove(dir, { recursive: true });
  }
});
