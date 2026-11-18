/**
 * Drive pilot from a Deno project in one call.
 *
 * {@link launchPilot} starts pilot with `--remote-debugging-port=0`, so the
 * operating system picks a free port, reads the `DevTools listening on ws://…`
 * line pilot prints on stderr, and connects Playwright to that endpoint over
 * CDP. The endpoint stays open for other CDP clients (Playwright MCP's
 * `--cdp-endpoint`, a recorder) while the script drives the browser.
 *
 * @example
 * ```ts
 * import { launchPilot } from "./mod.ts";
 *
 * await using pilot = await launchPilot();
 * const page = await pilot.browser.newPage();
 * await page.goto("https://example.com");
 * console.log(await page.title());
 * ```
 *
 * @module
 */
// Pinned inline, not through deno.json, so a project that imports this file by
// path gets the Playwright version pilot is tested with without an import map
// entry of its own. Deno shares one copy with the project's own import of the
// same version.
// deno-lint-ignore no-import-prefix
import { chromium } from "npm:playwright-core@1.65.0-alpha-2026-10-30";
// deno-lint-ignore no-import-prefix
import type { Browser } from "npm:playwright-core@1.65.0-alpha-2026-10-30";

/** Options for {@link launchPilot}. Every field is optional. */
export interface LaunchPilotOptions {
  /**
   * The pilot binary. Defaults to the `PILOT_BIN` environment variable, then
   * to `pilot` on the `PATH`.
   */
  executablePath?: string;
  /**
   * Run without pilot's watch-the-agent window (`--headless`). Defaults to
   * `true`. With `false`, pilot opens its window unless `PILOT_NO_WINDOW` is
   * set in its environment.
   */
  headless?: boolean;
  /**
   * The CDP port. Defaults to `0`, which lets the operating system pick a free
   * one; read the result from {@link PilotBrowser.wsEndpoint}.
   */
  port?: number;
  /**
   * A profile directory to keep cookies and storage in between runs
   * (`--user-data-dir`). Without one, pilot keeps its profile in memory.
   */
  userDataDir?: string;
  /** More Chromium switches for pilot, such as `--proxy-server=…`. */
  args?: string[];
  /** Extra environment variables for pilot, added to this process's own. */
  env?: Record<string, string>;
  /**
   * Milliseconds to wait for pilot to announce its endpoint and for
   * Playwright to connect. Defaults to 30000.
   */
  timeout?: number;
  /** Receives each line pilot writes to stderr (its log). */
  onLog?: (line: string) => void;
}

/** A running pilot and the Playwright browser connected to it. */
export interface PilotBrowser extends AsyncDisposable {
  /** The connected Playwright browser. */
  browser: Browser;
  /** pilot's browser-level CDP endpoint, `ws://127.0.0.1:<port>/devtools/browser/…`. */
  wsEndpoint: string;
  /** pilot's process id. */
  pid: number;
  /**
   * Closes the browser and waits for pilot to exit: it asks pilot to close
   * over CDP, as Playwright does, and kills it if it has not exited after five
   * seconds. Safe to call more than once.
   */
  close(): Promise<void>;
}

const DEFAULT_TIMEOUT_MS = 30_000;
const CLOSE_GRACE_MS = 5_000;
const LOG_LINES_KEPT = 20;
const ENDPOINT_LINE = /^DevTools listening on (ws:\/\/\S+)$/;

/**
 * Starts pilot on a free port and returns a Playwright browser connected to
 * it over CDP.
 *
 * pilot is killed if it does not announce its endpoint within `timeout`, if
 * Playwright cannot connect, or if this Deno process exits before
 * {@link PilotBrowser.close} is called. Needs `--allow-run`, `--allow-env`,
 * `--allow-net`, `--allow-read` and `--allow-write` (Playwright's own needs),
 * or `-A`.
 *
 * @param options How to start pilot; see {@link LaunchPilotOptions}.
 * @returns The connected browser, its endpoint, and a `close` that stops pilot.
 * @throws If pilot cannot be started, exits early, or misses the timeout; the
 *   error message carries pilot's last stderr lines.
 */
export async function launchPilot(
  options: LaunchPilotOptions = {},
): Promise<PilotBrowser> {
  const executablePath = options.executablePath ?? environmentPilot() ??
    "pilot";
  const timeout = options.timeout ?? DEFAULT_TIMEOUT_MS;
  const args = [
    `--remote-debugging-port=${options.port ?? 0}`,
    ...(options.headless === false ? [] : ["--headless"]),
    ...(options.userDataDir ? [`--user-data-dir=${options.userDataDir}`] : []),
    ...(options.args ?? []),
  ];
  console.info(`🚀 launchPilot: starting ${executablePath} ${args.join(" ")}`);

  let child: Deno.ChildProcess;
  try {
    child = new Deno.Command(executablePath, {
      args,
      env: options.env,
      stdin: "null",
      stdout: "null",
      stderr: "piped",
    }).spawn();
  } catch (error) {
    console.error(`❌ launchPilot: could not start ${executablePath}`, error);
    throw new Error(
      `could not start pilot at ${executablePath}: ${message(error)}`,
      { cause: error },
    );
  }

  // A script that exits without close() must not leave pilot running.
  const killOnUnload = () => kill(child);
  globalThis.addEventListener("unload", killOnUnload);

  // pilot logs to stderr for its whole life; reading it to the end keeps the
  // pipe from filling up and blocking pilot.
  const recentLines: string[] = [];
  const announced = Promise.withResolvers<string>();
  const drained = (async () => {
    for await (const line of lines(child.stderr)) {
      recentLines.push(line);
      if (recentLines.length > LOG_LINES_KEPT) recentLines.shift();
      options.onLog?.(line);
      const match = line.match(ENDPOINT_LINE);
      if (match) announced.resolve(match[1]);
    }
  })();

  const fail = async (reason: string, cause?: unknown): Promise<never> => {
    globalThis.removeEventListener("unload", killOnUnload);
    kill(child);
    await child.status;
    await drained.catch(() => {});
    const log = recentLines.length
      ? `\npilot's last stderr lines:\n${recentLines.join("\n")}`
      : "";
    console.error(`❌ launchPilot: ${reason}`);
    throw new Error(`${reason}${log}`, { cause });
  };

  const deadline = timer(timeout);
  try {
    const outcome = await Promise.race([
      announced.promise.then((wsEndpoint) => ({
        kind: "announced" as const,
        wsEndpoint,
      })),
      child.status.then((status) => ({ kind: "exited" as const, status })),
      deadline.promise.then(() => ({ kind: "timeout" as const })),
    ]);
    if (outcome.kind === "exited") {
      await drained.catch(() => {});
      return await fail(
        `pilot exited (code ${outcome.status.code}${
          outcome.status.signal ? `, signal ${outcome.status.signal}` : ""
        }) before announcing its DevTools endpoint`,
      );
    }
    if (outcome.kind === "timeout") {
      return await fail(
        `pilot did not announce its DevTools endpoint within ${timeout} ms`,
      );
    }

    const { wsEndpoint } = outcome;
    let browser: Browser;
    try {
      browser = await chromium.connectOverCDP(wsEndpoint, { timeout });
    } catch (error) {
      return await fail(
        `Playwright could not connect to pilot at ${wsEndpoint}: ${
          message(error)
        }`,
        error,
      );
    }
    console.info(
      `✅ launchPilot: pilot ${child.pid} connected at ${wsEndpoint}`,
    );

    let closing: Promise<void> | undefined;
    const close = () =>
      closing ??= (async () => {
        globalThis.removeEventListener("unload", killOnUnload);
        try {
          // Browser.close lets pilot shut down cleanly (and write its profile).
          const session = await browser.newBrowserCDPSession();
          await session.send("Browser.close");
        } catch {
          // pilot may exit before it answers, or be gone already.
        }
        const grace = timer(CLOSE_GRACE_MS);
        const exited = await Promise.race([
          child.status.then(() => true),
          grace.promise.then(() => false),
        ]);
        grace.cancel();
        if (!exited) {
          console.warn(
            `⚠️ launchPilot: pilot ${child.pid} ignored Browser.close; killing it`,
          );
          kill(child);
        }
        await child.status;
        await browser.close().catch(() => {});
        await drained.catch(() => {});
        console.info(`✅ launchPilot: pilot ${child.pid} closed`);
      })();

    return {
      browser,
      wsEndpoint,
      pid: child.pid,
      close,
      [Symbol.asyncDispose]: close,
    };
  } finally {
    deadline.cancel();
  }
}

function environmentPilot(): string | undefined {
  try {
    return Deno.env.get("PILOT_BIN") || undefined;
  } catch {
    // No --allow-env: fall back to the PATH.
    return undefined;
  }
}

function kill(child: Deno.ChildProcess): void {
  try {
    child.kill("SIGKILL");
  } catch {
    // Already exited.
  }
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** A cancellable timeout, so a finished launch leaves no timer behind. */
function timer(ms: number): { promise: Promise<void>; cancel: () => void } {
  let id: ReturnType<typeof setTimeout> | undefined;
  const promise = new Promise<void>((resolve) => {
    id = setTimeout(resolve, ms);
  });
  return { promise, cancel: () => clearTimeout(id) };
}

/** Splits a byte stream into lines, without their line endings. */
async function* lines(
  stream: ReadableStream<Uint8Array>,
): AsyncGenerator<string> {
  let pending = "";
  for await (const chunk of stream.pipeThrough(new TextDecoderStream())) {
    pending += chunk;
    let newline = pending.indexOf("\n");
    while (newline !== -1) {
      yield pending.slice(0, newline).replace(/\r$/, "");
      pending = pending.slice(newline + 1);
      newline = pending.indexOf("\n");
    }
  }
  if (pending) yield pending;
}
